//! Opt-in long-polling relay: exposes this daemon's tool surface to a REMOTE
//! base-url. Nothing ever starts it on its own — only an explicit `tunnel.start`
//! does — the state lives in daemon memory only (nothing lands on disk), and
//! the default `allow:"read"` forwards just the read-only perception whitelist.
//! `allow:"all"` forwards the full tool surface a local agent could reach; the
//! extension's own confirm/policy/shield gates still apply on every command,
//! and every forwarded command goes through the same agent routing a local
//! call would. The relay token is never echoed in full. Protocol per poll:
//!
//!   GET  {base}/tunnels/{id}/poll?limit=1&timeout_ms=15000   Bearer <relay token>
//!        204 = idle · 200 = {id, tool, params} · 401/403 & 404/410 stop the loop
//!   POST {base}/tunnels/{id}/response  {id, result|error}
//!
//! Failures back off 1s → 30s.

use crate::AppState;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const POLL_TIMEOUT_MS: u64 = 15_000;
const BACKOFF_MAX_MS: u64 = 30_000;
const CMD_TIMEOUT: Duration = Duration::from_secs(15);
const HTTP_TIMEOUT: Duration = Duration::from_secs(POLL_TIMEOUT_MS / 1000 + 10);

/// Read-only perception whitelist for `allow:"read"` — the plan's frozen seven
/// plus the two unambiguously read-only helpers. Anything else is refused
/// HERE, before it ever reaches a slot.
const READ_TOOLS: &[&str] = &[
    "snapshot",
    "read_page",
    "tab.list",
    "frames.list",
    "console.list",
    "network.list",
    "screenshot",
    "frames.probe",
    "extract",
];

struct Inner {
    id: String,
    base: String,
    token: String,
    allow: String,
    started_at: String,
    last_poll: Option<String>,
    stop_reason: Option<String>,
    polls: u64,
    commands: u64,
    errors: u64,
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static INNER: Mutex<Option<Inner>> = Mutex::new(None);

fn with_inner<T>(f: impl FnOnce(&mut Inner) -> T) -> T {
    let mut guard = INNER.lock().unwrap();
    if guard.is_none() {
        *guard = Some(Inner {
            id: String::new(),
            base: String::new(),
            token: String::new(),
            allow: "read".to_string(),
            started_at: String::new(),
            last_poll: None,
            stop_reason: None,
            polls: 0,
            commands: 0,
            errors: 0,
        });
    }
    f(guard.as_mut().unwrap())
}

fn status_result() -> Value {
    let running = RUNNING.load(Ordering::SeqCst);
    with_inner(|i| {
        json!({
            "running": running,
            "id": if i.id.is_empty() { Value::Null } else { json!(i.id) },
            "base": if i.base.is_empty() { Value::Null } else { json!(i.base) },
            "allow": i.allow,
            "started_at": if i.started_at.is_empty() { Value::Null } else { json!(i.started_at) },
            "last_poll": i.last_poll,
            "stop_reason": i.stop_reason,
            "stats": { "polls": i.polls, "commands": i.commands, "errors": i.errors },
            // the token authenticates to the REMOTE relay — never echoed in full
            "token": if i.token.is_empty() { Value::Null } else { json!(format!("{}\u{2026}", &i.token[..i.token.len().min(4)])) },
        })
    })
}

/// Daemon method `tunnel.start`: validate, remember, spawn the poll loop.
pub fn start(state: &Arc<AppState>, params: &Value) -> Result<Value, Value> {
    let base = {
        let raw = params
            .get("base_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim_end_matches('/')
            .to_string();
        if !(raw.starts_with("https://") || raw.starts_with("http://")) {
            return Err(json!({"code": "BAD_PARAMS", "message": "tunnel.start needs base_url as an absolute http(s) URL, e.g. \"https://relay.example.com\""}));
        }
        raw
    };
    let token = params
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if token.is_empty() {
        return Err(json!({"code": "BAD_PARAMS", "message": "tunnel.start needs the relay token (Authorization: Bearer <token> on every poll/response)"}));
    }
    let allow = if params.get("allow").and_then(|v| v.as_str()) == Some("all") {
        "all"
    } else {
        "read"
    };
    let id = match params.get("id").and_then(|v| v.as_str()) {
        Some(x)
            if !x.is_empty()
                && x.len() <= 64
                && x.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) =>
        {
            x.to_string()
        }
        _ => crate::config::random_token()
            .map(|t| format!("cua-{}", &t[..8]))
            .unwrap_or_else(|| "cua-tunnel".to_string()),
    };
    if RUNNING.swap(true, Ordering::SeqCst) {
        let existing = with_inner(|i| format!("id {:?}, base {}", i.id, i.base));
        return Err(json!({"code": "TUNNEL_ALREADY_RUNNING", "message": format!("a tunnel is already running ({existing}) — tunnel.stop first")}));
    }
    with_inner(|i| {
        i.id = id.clone();
        i.base = base.clone();
        i.token = token.clone();
        i.allow = allow.to_string();
        i.started_at = crate::iso_now();
        i.last_poll = None;
        i.stop_reason = None;
        i.polls = 0;
        i.commands = 0;
        i.errors = 0;
    });
    let state = state.clone();
    let loop_id = id.clone();
    std::thread::Builder::new()
        .name(format!("tunnel-{loop_id}"))
        .spawn(move || poll_loop(state))
        .map_err(|e| {
            RUNNING.store(false, Ordering::SeqCst);
            json!({"code": "INTERNAL", "message": format!("spawn tunnel loop: {e}")})
        })?;
    Ok(status_result())
}

/// Daemon method `tunnel.stop`.
pub fn stop() -> Value {
    if !RUNNING.swap(false, Ordering::SeqCst) {
        return with_inner(|i| {
            json!({
                "running": false,
                "stop_reason": i.stop_reason.clone().unwrap_or_else(|| "not running".to_string()),
                "stats": { "polls": i.polls, "commands": i.commands, "errors": i.errors },
            })
        });
    }
    with_inner(|i| i.stop_reason = Some("stopped by request".to_string()));
    status_result()
}

/// Daemon method `tunnel.status`.
pub fn status() -> Value {
    status_result()
}

/// Allow-gate for one relay command: `read` = whitelist only, `all` = the full
/// surface (the extension's own gates still apply per command).
fn gate(tool: &str) -> Result<(), Value> {
    let allow = with_inner(|i| i.allow.clone());
    if allow == "all" || READ_TOOLS.contains(&tool) {
        return Ok(());
    }
    Err(json!({
        "code": "TUNNEL_TOOL_DENIED",
        "message": format!(
            "allow:\"{allow}\" forwards only read-only perception tools ({}); \"{tool}\" is refused here — run it through a local agent",
            READ_TOOLS.join(", ")
        ),
    }))
}

fn bump_error() {
    with_inner(|i| i.errors += 1);
}

fn stop_with_reason(reason: String) {
    RUNNING.store(false, Ordering::SeqCst);
    with_inner(|i| i.stop_reason = Some(reason));
}

/// The long-poll loop, on its own thread (blocking HTTP).
fn poll_loop(state: Arc<AppState>) {
    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    let (base, token, id) = with_inner(|i| (i.base.clone(), i.token.clone(), i.id.clone()));
    let poll_url = format!("{base}/tunnels/{id}/poll?limit=1&timeout_ms={POLL_TIMEOUT_MS}");
    let resp_url = format!("{base}/tunnels/{id}/response");
    let mut backoff_ms: u64 = 1_000;
    while RUNNING.load(Ordering::SeqCst) {
        let res = match agent
            .get(&poll_url)
            .set("Authorization", &format!("Bearer {token}"))
            .call()
        {
            Ok(r) => Some(r),
            Err(ureq::Error::Status(_code, r)) => Some(r),
            Err(_) => {
                // transport error (timeout, DNS, refused)
                if !RUNNING.load(Ordering::SeqCst) {
                    break;
                }
                bump_error();
                with_inner(|i| i.last_poll = Some(crate::iso_now()));
                std::thread::sleep(Duration::from_millis(backoff_ms));
                backoff_ms = (backoff_ms * 2).min(BACKOFF_MAX_MS);
                continue;
            }
        };
        if !RUNNING.load(Ordering::SeqCst) {
            break;
        }
        let Some(res) = res else { continue };
        with_inner(|i| {
            i.last_poll = Some(crate::iso_now());
            i.polls += 1;
        });
        match res.status() {
            401 | 403 => {
                stop_with_reason(format!("relay rejected the token (HTTP {})", res.status()));
                break;
            }
            404 | 410 => {
                stop_with_reason(format!("tunnel gone at the relay (HTTP {})", res.status()));
                break;
            }
            204 => {
                backoff_ms = 1_000; // healthy idle
                continue;
            }
            s if (200..300).contains(&s) => {
                backoff_ms = 1_000;
            }
            s => {
                bump_error();
                let _ = s;
                std::thread::sleep(Duration::from_millis(backoff_ms));
                backoff_ms = (backoff_ms * 2).min(BACKOFF_MAX_MS);
                continue;
            }
        }
        let body = match res.into_string() {
            Ok(b) => b,
            Err(e) => {
                bump_error();
                eprintln!("[cua-bridge-rs] tunnel poll body unreadable: {e}");
                continue;
            }
        };
        let cmd: Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                bump_error();
                eprintln!("[cua-bridge-rs] tunnel poll body was not JSON: {e}");
                continue;
            }
        };
        let tool = cmd
            .get("tool")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .strip_prefix("tool.")
            .unwrap_or("")
            .to_string();
        let params = cmd.get("params").cloned().unwrap_or(json!({}));
        let out = match gate(&tool) {
            Ok(()) => {
                if with_inner(|i| i.allow.clone()) == "all" {
                    eprintln!("[cua-bridge-rs] tunnel WARNING allow=all: executing remote command \"{tool}\"");
                } else {
                    eprintln!("[cua-bridge-rs] tunnel read command \"{tool}\"");
                }
                let out = crate::call_extension(&state, None, &tool, params, CMD_TIMEOUT);
                if out.is_err() {
                    bump_error();
                }
                match out {
                    Ok(result) => json!({ "result": result }),
                    Err(error) => json!({ "error": error }),
                }
            }
            Err(error) => json!({ "error": error }),
        };
        with_inner(|i| i.commands += 1);
        // a failed response POST is an error counter, never a loop stopper —
        // the command already ran, so losing the reply must not spin the relay
        let mut payload = json!({ "id": cmd.get("id").cloned().unwrap_or(Value::Null) });
        if let (Some(dst), Some(src)) = (payload.as_object_mut(), out.as_object()) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
        match agent
            .post(&resp_url)
            .set("Authorization", &format!("Bearer {token}"))
            .set("Content-Type", "application/json")
            .send_json(payload)
        {
            Ok(_) => {}
            Err(_) => {
                bump_error();
                eprintln!("[cua-bridge-rs] tunnel response POST failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn gate_enforces_the_read_whitelist() {
        assert!(gate("snapshot").is_ok());
        assert!(gate("read_page").is_ok());
        assert!(gate("extract").is_ok());
        assert!(gate("click").is_err());
        assert!(gate("secret.set").is_err());
        assert!(gate("cdp.call").is_err());
        assert!(gate("").is_err());
        assert!(gate("tool.snapshot").is_err(), "prefix must be stripped before the gate");
    }

    #[test]
    fn start_validates_params_without_touching_the_loop() {
        assert_eq!(
            start(&Arc::new(AppState::new()), &json!({}))
                .unwrap_err()
                .get("code")
                .and_then(|v| v.as_str()),
            Some("BAD_PARAMS")
        );
        assert_eq!(
            start(
                &Arc::new(AppState::new()),
                &json!({"base_url": "https://relay.example.com"})
            )
            .unwrap_err()
            .get("code")
            .and_then(|v| v.as_str()),
            Some("BAD_PARAMS")
        );
    }
}
