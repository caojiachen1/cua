//! The local browser bridge that sits between `cua browser` and the CUA Browser
//! extension. This crate is linked into `cua.exe` — there is no separate binary.
//!
//! The daemon is a WebSocket router on loopback: the extension dials in with
//! `cua-auth.<token>`, agents dial in with `cua-agent.<token>`, and one JSON-RPC
//! frame goes out and one comes back.
//!
//! ```text
//! cua browser <tool> '<json>'  ──ws──▶  daemon  ──ws──▶  extension (CDP)
//!        (client.rs)                   (this crate)        browser/extension/
//! ```
//!
//!   agent   → {"id":1,"method":"tool.x","params":{...}}
//!   agent   ← {"id":1,"result":{...}} | {"id":1,"error":{"code","message"}}
//!   cancel:    {"method":"cancel","params":{"id":1}}   status: {"id":0,"method":"status"}
//!
//! `cua.exe browser serve` runs it; `client.rs` is the in-process caller, so
//! there is no helper binary to resolve or spawn per call.

pub mod client;
pub mod config;
mod vault;

pub mod auditlog;
pub mod mcp;
pub mod tunnel;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Message, Role};
use tokio_tungstenite::tungstenite::Error as WsError;

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// ISO-8601 UTC timestamp for logs and records — no chrono dependency
/// (civil-from-days, Howard Hinnant's algorithm).
pub fn iso_from_secs(secs: u64) -> String {
    let rem = secs % 86_400;
    let days = (secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

pub fn iso_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    iso_from_secs(secs)
}

/// Short, non-reversible fingerprint of a daemon token. `/health` publishes it
/// so a client pointed at a daemon started from a *different* config can say so
/// instead of reporting a bare "token rejected".
pub fn token_id(token: &str) -> String {
    use sha2::Digest as _;
    let d = Sha256::digest(token.as_bytes());
    d.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

/// A browser slot name: the extension's hello carries it; one profile of one
/// browser binary registers under exactly one name.
fn valid_browser_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 32 && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

const PAIR_NONCE_TTL: Duration = Duration::from_secs(120);

// ------------------------------------------------------------------ shared state

type Tx = mpsc::UnboundedSender<Message>;

#[derive(Clone)]
enum Sink {
    Stdio,
    Ws(u64),
    /// daemon-internal one-shot caller (audit persistence, tunnel): the reply
    /// lands on this channel instead of a socket. The u64 is the identity used
    /// by cancel-matching; a Sender has no address to compare.
    Local(u64, std::sync::mpsc::Sender<Value>),
}

impl PartialEq for Sink {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Sink::Stdio, Sink::Stdio) => true,
            (Sink::Ws(a), Sink::Ws(b)) => a == b,
            (Sink::Local(a, _), Sink::Local(b, _)) => a == b,
            _ => false,
        }
    }
}

struct ExtState {
    out: Tx,
    info: Mutex<Value>,
    last_frame: Arc<Mutex<Instant>>,
    /// per-connection liveness counter: probe_ext waits for THIS slot's tick
    tick: Arc<AtomicU64>,
    /// connection generation: a late teardown of an old socket must not clear
    /// the extension slot that a newer socket now owns
    gen: u64,
    name: String,
}

struct Route {
    sink: Sink,
    agent_id: Value,
    /// which browser slot this in-flight call was routed to — a cancel and a
    /// disconnect both need the slot, not a global singleton
    browser: String,
}

/// Where nameless agent calls go. Default: the user's real logged-in browser,
/// auto-opened with its NORMAL profile when missing. `auto_workspace:true`
/// opts back into the workspace-first behaviour; `auto_open_user_browser:false`
/// with `auto_workspace:false` restores the legacy single-slot rules.
#[derive(Clone)]
struct AgentCfg {
    slot: String,
    auto_workspace: bool,
    wait_ms: u64,
    user_slot: String,
    auto_open_user: bool,
    user_wait_ms: u64,
    user_exe: String,
    fallback_exe: String,
}

struct AppState {
    exts: Mutex<HashMap<String, ExtState>>,
    ws_agents: Mutex<Vec<(u64, Tx)>>,
    routes: Mutex<HashMap<String, Route>>,
    seq: AtomicU64,
    agent_seq: AtomicU64,
    ext_seq: AtomicU64,
    /// browser.launch pairing nonces: nonce → (slot name, minted_at)
    nonces: Mutex<HashMap<String, (String, Instant)>>,
    /// serializes agent-workspace auto-launches: concurrent nameless calls
    /// share one spawn+wait instead of racing N browsers into existence
    launch_lock: tokio::sync::Mutex<()>,
    /// test seam — when Some, replaces browser.json for every
    /// config-reading path (agent routing + browser.launch); prod leaves None
    config_override: Mutex<Option<Value>>,
}

impl AppState {
    fn new() -> Self {
        Self {
            exts: Mutex::new(HashMap::new()),
            ws_agents: Mutex::new(Vec::new()),
            routes: Mutex::new(HashMap::new()),
            seq: AtomicU64::new(0),
            agent_seq: AtomicU64::new(0),
            ext_seq: AtomicU64::new(0),
            nonces: Mutex::new(HashMap::new()),
            launch_lock: tokio::sync::Mutex::new(()),
            config_override: Mutex::new(None),
        }
    }

    /// ~/.cua/config.json, or the test override when one is installed.
    fn read_config(&self) -> Value {
        if let Some(v) = self.config_override.lock().unwrap().clone() {
            return v;
        }
        read_config()
    }

    /// Route an agent request to an extension slot. `want == None` accepts the
    /// single connected slot (back-compat: one browser, zero config); with
    /// several slots a name is required and the refusal lists what exists.
    fn select_ext(&self, want: Option<&str>) -> Result<Tx, Value> {
        let exts = self.exts.lock().unwrap();
        match want {
            Some(n) => match exts.get(n) {
                Some(e) => Ok(e.out.clone()),
                None => Err(json!({
                    "code": "BROWSER_NOT_FOUND",
                    "message": format!("no browser slot named {n:?} is connected; connected: {}", exts.keys().cloned().collect::<Vec<_>>().join(", ")),
                })),
            },
            None => {
                if exts.len() == 1 {
                    Ok(exts.values().next().unwrap().out.clone())
                } else if exts.is_empty() {
                    Err(json!({"code": "EXTENSION_NOT_CONNECTED", "message": "load the CUA extension and check its popup"}))
                } else {
                    Err(json!({
                        "code": "BROWSER_REQUIRED",
                        "message": format!("{} browser slots are connected and no \"browser\" was given — pass the slot name (cua --browser <name> / params.browser): {}", exts.len(), exts.keys().cloned().collect::<Vec<_>>().join(", ")),
                    }))
                }
            }
        }
    }

    fn browser_names(&self) -> Vec<String> {
        let exts = self.exts.lock().unwrap();
        let mut v: Vec<String> = exts.keys().cloned().collect();
        v.sort();
        v
    }

    /// Routing target for nameless agent calls. Defaults now prefer the real
    /// logged-in browser (`user_browser`, auto-opened with its normal profile).
    /// `auto_workspace:true` opts back into workspace-first; disabling both
    /// restores legacy single-slot routing.
    fn agent_cfg(&self) -> AgentCfg {
        let cfg = self.read_config();
        AgentCfg {
            slot: cfg
                .get("agent_browser")
                .and_then(|v| v.as_str())
                .filter(|n| valid_browser_name(n))
                .unwrap_or("agent")
                .to_string(),
            auto_workspace: cfg.get("auto_workspace").and_then(|v| v.as_bool()).unwrap_or(false),
            wait_ms: cfg.get("workspace_wait_ms").and_then(|v| v.as_u64()).unwrap_or(20_000).clamp(500, 120_000),
            user_slot: cfg
                .get("user_browser")
                .and_then(|v| v.as_str())
                .filter(|n| valid_browser_name(n))
                .unwrap_or("default")
                .to_string(),
            auto_open_user: cfg.get("auto_open_user_browser").and_then(|v| v.as_bool()).unwrap_or(true),
            user_wait_ms: cfg.get("user_browser_wait_ms").and_then(|v| v.as_u64()).unwrap_or(20_000).clamp(500, 120_000),
            user_exe: cfg.get("user_browser_exe").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            fallback_exe: cfg.get("browser_exe").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        }
    }

    /// The user's real browser slot: configured `user_browser` when connected,
    /// else the only connected non-workspace slot. Several real slots without
    /// the configured name is ambiguous and refused.
    fn real_slot(&self, cfg: &AgentCfg) -> Result<Option<String>, Value> {
        let exts = self.exts.lock().unwrap();
        let is_workspace = |e: &ExtState| {
            e.info
                .lock()
                .unwrap()
                .get("workspace")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        };
        if let Some(e) = exts.get(&cfg.user_slot) {
            if !is_workspace(e) {
                return Ok(Some(cfg.user_slot.clone()));
            }
        }
        let reals: Vec<String> = exts
            .iter()
            .filter(|(_, e)| !is_workspace(e))
            .map(|(n, _)| n.clone())
            .collect();
        match reals.len() {
            0 => Ok(None),
            1 => Ok(Some(reals.into_iter().next().unwrap())),
            _ => Err(json!({
                "code": "BROWSER_REQUIRED",
                "message": format!(
                    "{} are connected real browser slots and none matches user_browser {:?} — pass the slot name (cua --browser <name> / params.browser)",
                    reals.join(", "), cfg.user_slot
                ),
            })),
        }
    }

    fn list_browsers(&self) -> Vec<Value> {
        let exts = self.exts.lock().unwrap();
        let mut rows: Vec<Value> = exts
            .iter()
            .map(|(name, e)| {
                let info = e.info.lock().unwrap();
                json!({
                    "name": name,
                    "build": info.get("build").cloned().unwrap_or(Value::Null),
                    "browser": info.get("browser").cloned().unwrap_or(Value::Null),
                    "version": info.get("version").cloned().unwrap_or(Value::Null),
                    "instance_id": info.get("instance_id").cloned().unwrap_or(Value::Null),
                    "workspace": info.get("workspace").and_then(|v| v.as_bool()).unwrap_or(false),
                    "since": e.last_frame.lock().unwrap().elapsed().as_secs(),
                })
            })
            .collect();
        rows.sort_by_key(|r| r.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string());
        rows
    }

    fn send_agent(&self, sink: &Sink, obj: &Value) {
        let text = obj.to_string();
        match sink {
            Sink::Stdio => {
                emit(&text);
            }
            Sink::Ws(id) => {
                let agents = self.ws_agents.lock().unwrap();
                if let Some((_, tx)) = agents.iter().find(|(i, _)| i == id) {
                    let _ = tx.send(Message::Text(text.clone().into()));
                }
            }
            Sink::Local(_, tx) => {
                let _ = tx.send(obj.clone());
            }
        }
    }

    fn broadcast(&self, obj: &Value) {
        let text = obj.to_string();
        emit(&text);
        let agents = self.ws_agents.lock().unwrap();
        for (_, tx) in agents.iter() {
            let _ = tx.send(Message::Text(text.clone().into()));
        }
    }
}

// ------------------------------------------------------------------ helpers

// console write that matches node's behavior on Windows: when stdout is a real
// console, go through WriteConsoleW so CJK renders under non-UTF8 codepages
fn emit(s: &str) {
    #[cfg(windows)]
    {
        use std::io::IsTerminal;
        if std::io::stdout().is_terminal() {
            #[link(name = "kernel32")]
            extern "system" {
                fn GetStdHandle(nstdhandle: u32) -> *mut core::ffi::c_void;
                fn WriteConsoleW(
                    consoleoutput: *mut core::ffi::c_void,
                    buffer: *const u16,
                    numberofcharstowrite: u32,
                    numberofcharswritten: *mut u32,
                    reserved: *mut core::ffi::c_void,
                ) -> i32;
            }
            const STD_OUTPUT_HANDLE: u32 = u32::MAX - 10; // (DWORD)-11
            unsafe {
                let h = GetStdHandle(STD_OUTPUT_HANDLE);
                let mut line = String::with_capacity(s.len() + 1);
                line.push_str(s);
                line.push(10u8 as char);
                let wide: Vec<u16> = line.encode_utf16().collect();
                let mut written = 0u32;
                if WriteConsoleW(h, wide.as_ptr(), wide.len() as u32, &mut written, std::ptr::null_mut()) != 0 {
                    daemon_log_line(s);
                    return;
                }
            }
        }
    }
    // the daemon runs detached with NO std handles (CreateProcessW is called
    // with bInheritHandles=false, which is what keeps an agent harness's pipe
    // from hanging on the first cold start) — writes may fail and MUST NOT
    // panic, so no println! here
    {
        use std::io::Write as _;
        let _ = std::io::stdout().lock().write_all(format!("{s}\n").as_bytes());
    }
    daemon_log_line(s);
}

/// The daemon's own log file (`browser-daemon.log`), mirrored from every
/// `emit`/`log` so crash traces survive even without std handles.
static DAEMON_LOG: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);

fn daemon_log_line(s: &str) {
    if let Ok(mut g) = DAEMON_LOG.lock() {
        if let Some(f) = g.as_mut() {
            use std::io::Write as _;
            let _ = writeln!(f, "{s}");
        }
    }
}

fn log(msg: &str) {
    eprintln!("[cua-bridge-rs] {msg}");
    daemon_log_line(&format!("[cua-bridge-rs] {msg}"));
}

fn major(v: &Value) -> u64 {
    v.as_str()
        .and_then(|s| s.split('.').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn close(code: u16, reason: &str) -> Message {
    Message::Close(Some(CloseFrame {
        code: CloseCode::from(code),
        reason: reason.to_owned().into(),
    }))
}

/// Drop ONE named slot. `only_gen` keeps a late teardown of an old socket from
/// clearing the slot a newer socket now owns. Every in-flight route that was
/// routed to this browser learns about it; routes on other slots are untouched.
fn drop_ext_named(state: &AppState, name: &str, reason: &str, only_gen: Option<u64>) {
    let victim = {
        let mut exts = state.exts.lock().unwrap();
        match exts.get(name) {
            Some(cur) => {
                if let Some(g) = only_gen {
                    if cur.gen != g {
                        return; // a late close of the OLD socket; the new one is live
                    }
                }
                exts.remove(name)
            }
            None => return,
        }
    };
    if let Some(e) = victim {
        let _ = e.out.send(close(1000, reason));
        log(&format!("extension disconnected: {name}"));
        let mut routes = state.routes.lock().unwrap();
        let orphaned: Vec<String> = routes
            .iter()
            .filter(|(_, r)| r.browser == name)
            .map(|(k, _)| k.clone())
            .collect();
        for k in orphaned {
            if let Some(r) = routes.remove(&k) {
                state.send_agent(
                    &r.sink,
                    &json!({"id": r.agent_id, "error": {"code": "EXTENSION_DISCONNECTED", "message": "extension went away"}}),
                );
            }
        }
    }
}

/// Send `{"method":"ping"}` to ONE named slot and return true if any frame
/// arrives from it within `ms` (mirrors wslib probe(): app-level ping).
async fn probe_ext(state: &Arc<AppState>, name: &str, ms: u64) -> bool {
    let (out, tick) = {
        let exts = state.exts.lock().unwrap();
        match exts.get(name) {
            Some(e) => (e.out.clone(), e.tick.clone()),
            None => return false,
        }
    };
    let before = tick.load(Ordering::SeqCst);
    let _ = out.send(Message::Text(json!({"method": "ping"}).to_string().into()));
    let deadline = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < deadline {
        if tick.load(Ordering::SeqCst) != before {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    false
}

/// Extract the routing hint from an agent request: the envelope `browser`
/// field, or `params.browser` (which is then stripped from what reaches the
/// tool so unknown-param handling stays the extension's business).
fn extract_browser(msg: &Value) -> (Option<String>, Value) {
    let env = msg.get("browser").and_then(|v| v.as_str()).map(String::from);
    let mut params = match msg.get("params") {
        Some(v) if !v.is_null() => v.clone(),
        _ => json!({}),
    };
    let inner = params.get("browser").and_then(|v| v.as_str()).map(String::from);
    if inner.is_some() {
        if let Some(o) = params.as_object_mut() {
            o.remove("browser");
        }
    }
    (env.or(inner), params)
}

/// Daemon-side methods answered WITHOUT touching any extension slot. These
/// never race a tool call: they are answered synchronously on the agent socket.
fn daemon_method(state: &Arc<AppState>, sink: &Sink, msg: &Value, method: &str) -> bool {
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    match method {
        "status" => {
            let names = state.browser_names();
            let cfg = state.agent_cfg();
            state.send_agent(
                sink,
                &json!({
                    "id": id,
                    "result": {
                        "extension_connected": !names.is_empty(),
                        "browsers": state.list_browsers(),
                        "inflight": state.routes.lock().unwrap().len(),
                        "agent_slot": cfg.slot,
                        "auto_workspace": cfg.auto_workspace,
                        "user_browser": cfg.user_slot,
                        "auto_open_user_browser": cfg.auto_open_user,
                    }
                }),
            );
            true
        }
        "meta.browsers" | "browsers.list" => {
            state.send_agent(sink, &json!({"id": id, "result": {"browsers": state.list_browsers()}}));
            true
        }
        "pair.register" => {
            let name = msg.pointer("/params/name").and_then(|v| v.as_str()).unwrap_or("");
            if !valid_browser_name(name) {
                state.send_agent(
                    sink,
                    &json!({"id": id, "error": {"code": "BAD_PARAMS", "message": "pair.register needs a slot name matching [a-z0-9_-]{1,32}"}}),
                );
                return true;
            }
            let Some(nonce) = crypto_token() else {
                state.send_agent(
                    sink,
                    &json!({"id": id, "error": {"code": "INTERNAL", "message": "OS CSPRNG unavailable"}}),
                );
                return true;
            };
            state.nonces.lock().unwrap().insert(nonce.clone(), (name.to_string(), Instant::now()));
            state.send_agent(sink, &json!({"id": id, "result": {"nonce": nonce, "ttl_s": PAIR_NONCE_TTL.as_secs()}}));
            true
        }
        "browser.pair" => {
            // Launch-less pairing: mint the same one-shot nonce browser.launch
            // uses, but let the caller spawn the browser (cua does this for its
            // CuaEdge/CuaChrome profiles in another Windows session).
            let name = msg.pointer("/params/name").and_then(|v| v.as_str()).unwrap_or("");
            if !valid_browser_name(name) {
                state.send_agent(
                    sink,
                    &json!({"id": id, "error": {"code": "BAD_PARAMS", "message": "browser.pair needs name matching [a-z0-9_-]{1,32} (the slot the instance registers under)"}}),
                );
                return true;
            }
            let empty = json!({});
            let params = msg.get("params").filter(|p| p.is_object()).unwrap_or(&empty);
            let ext_dir = match resolve_extension_dir(state, params) {
                Ok(d) => d,
                Err(e) => {
                    state.send_agent(sink, &json!({"id": id, "error": e}));
                    return true;
                }
            };
            let Some(nonce) = crypto_token() else {
                state.send_agent(
                    sink,
                    &json!({"id": id, "error": {"code": "INTERNAL", "message": "OS CSPRNG unavailable"}}),
                );
                return true;
            };
            state.nonces.lock().unwrap().insert(nonce.clone(), (name.to_string(), Instant::now()));
            state.send_agent(
                sink,
                &json!({"id": id, "result": {
                    "slot": name,
                    "pair_url": format!("http://127.0.0.1:{}/__cua_pair?nonce={nonce}", state_port(state)),
                    "extension_dir": ext_dir,
                    "extension_id": unpacked_ext_id(&ext_dir),
                    "ttl_s": PAIR_NONCE_TTL.as_secs(),
                    "note": "launch the browser yourself with --user-data-dir=<profile> --load-extension=<extension_dir> and pair_url as the first tab; it connects under slot <name> after that page runs",
                }}),
            );
            true
        }

        "diag.browsers" => {
            // read-only multi-browser scan (mirrors the Node daemon's daemonMethod;
            // what this minimal sibling skips is declared in result.limitations)
            state.send_agent(sink, &json!({"id": id, "result": diag_browsers_result(state)}));
            true
        }
        "tunnel.start" => match tunnel::start(state, msg.get("params").unwrap_or(&Value::Null)) {
            Ok(result) => {
                state.send_agent(sink, &json!({"id": id, "result": result}));
                true
            }
            Err(error) => {
                state.send_agent(sink, &json!({"id": id, "error": error}));
                true
            }
        },
        "tunnel.stop" => {
            state.send_agent(sink, &json!({"id": id, "result": tunnel::stop()}));
            true
        }
        "tunnel.status" => {
            state.send_agent(sink, &json!({"id": id, "result": tunnel::status()}));
            true
        }
        _ => false,
    }
}

// ------------------------------------------------------------- diag.browsers
// Read-only multi-browser diagnostics (ABSORB-CODEX-PLAN §9). daemon/diag.mjs
// (Node) is the REFERENCE implementation; this minimal sibling covers the same
// Windows matrix (install exe, one tasklist process pass, Preferences scan,
// NM manifest via HKCU registry / %APPDATA% / ~/.cua) and declares what it
// skips in result.limitations. tunnel stays Node-only (see the plan).

/// One Chromium-family browser for the diag scan (Windows matrix).
/// exe candidates are (root-key, subpath) pairs; roots: PF / PF86 / LOCALAPPDATA.
struct FamilyDiag {
    family: &'static str,
    display: &'static str,
    exes: &'static [(&'static str, &'static [&'static str])],
    process: &'static str,
    manage_url: &'static str,
    /// profile root: (env root name, fixed subpath segments)
    user_data: (&'static str, &'static [&'static str]),
    /// env-root-relative native-messaging manifest path
    nm_manifest: (&'static str, &'static [&'static str]),
    reg_key: &'static str,
}

const DIAG_FAMILIES: &[FamilyDiag] = &[
    FamilyDiag {
        family: "edge",
        display: "Microsoft Edge",
        process: "msedge.exe",
        manage_url: "edge://extensions",
        exes: &[
            ("PF86", &["Microsoft", "Edge", "Application", "msedge.exe"]),
            ("PF", &["Microsoft", "Edge", "Application", "msedge.exe"]),
        ],
        user_data: ("LOCALAPPDATA", &["Microsoft", "Edge", "User Data"]),
        nm_manifest: ("APPDATA", &["Microsoft", "Edge", "NativeMessagingHosts", "com.cua.bridge.json"]),
        reg_key: r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.cua.bridge",
    },
    FamilyDiag {
        family: "chrome",
        display: "Google Chrome",
        process: "chrome.exe",
        manage_url: "chrome://extensions",
        exes: &[
            ("PF", &["Google", "Chrome", "Application", "chrome.exe"]),
            ("PF86", &["Google", "Chrome", "Application", "chrome.exe"]),
            ("LOCALAPPDATA", &["Google", "Chrome", "Application", "chrome.exe"]), // per-user install
        ],
        user_data: ("LOCALAPPDATA", &["Google", "Chrome", "User Data"]),
        nm_manifest: ("APPDATA", &["Google", "Chrome", "NativeMessagingHosts", "com.cua.bridge.json"]),
        reg_key: r"HKCU\Software\Google\Chrome\NativeMessagingHosts\com.cua.bridge",
    },
    FamilyDiag {
        family: "brave",
        display: "Brave",
        process: "brave.exe",
        manage_url: "brave://extensions",
        exes: &[
            ("PF", &["BraveSoftware", "Brave-Browser", "Application", "brave.exe"]),
            ("PF86", &["BraveSoftware", "Brave-Browser", "Application", "brave.exe"]),
            ("LOCALAPPDATA", &["BraveSoftware", "Brave-Browser", "Application", "brave.exe"]),
        ],
        user_data: ("LOCALAPPDATA", &["BraveSoftware", "Brave-Browser", "User Data"]),
        nm_manifest: ("APPDATA", &["BraveSoftware", "Brave-Browser", "NativeMessagingHosts", "com.cua.bridge.json"]),
        reg_key: r"HKCU\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.cua.bridge",
    },
    FamilyDiag {
        family: "opera",
        display: "Opera",
        process: "opera.exe",
        manage_url: "opera://extensions",
        exes: &[
            ("LOCALAPPDATA", &["Programs", "Opera", "opera.exe"]), // per-user install is the common layout
            ("LOCALAPPDATA", &["Programs", "Opera GX", "opera.exe"]),
            ("PF", &["Opera", "opera.exe"]),
        ],
        user_data: ("APPDATA", &["Opera Software"]),
        nm_manifest: ("APPDATA", &["Opera Software", "NativeMessagingHosts", "com.cua.bridge.json"]),
        reg_key: r"HKCU\Software\Opera Software\NativeMessagingHosts\com.cua.bridge",
    },
    FamilyDiag {
        family: "vivaldi",
        display: "Vivaldi",
        process: "vivaldi.exe",
        manage_url: "vivaldi://extensions",
        exes: &[
            ("LOCALAPPDATA", &["Vivaldi", "Application", "vivaldi.exe"]),
            ("PF", &["Vivaldi", "Application", "vivaldi.exe"]),
        ],
        user_data: ("LOCALAPPDATA", &["Vivaldi", "User Data"]),
        nm_manifest: ("APPDATA", &["Vivaldi", "NativeMessagingHosts", "com.cua.bridge.json"]),
        reg_key: r"HKCU\Software\Vivaldi\NativeMessagingHosts\com.cua.bridge",
    },
];

const DIAG_MAX_PREFS_BYTES: u64 = 32 * 1024 * 1024;

/// env root by name, with the same fallbacks the Node scanner uses.
fn diag_env_root(name: &str) -> std::path::PathBuf {
    match name {
        "LOCALAPPDATA" => std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home_dir().unwrap_or_default().join("AppData").join("Local")),
        "APPDATA" => std::env::var_os("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home_dir().unwrap_or_default().join("AppData").join("Roaming")),
        "PF" => std::env::var_os("ProgramFiles")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Program Files")),
        "PF86" => std::env::var_os("ProgramFiles(x86)")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Program Files (x86)")),
        _ => std::path::PathBuf::new(),
    }
}

/// Backslash-normalized, lower-cased path for Preferences `path` comparison.
fn diag_norm_path(p: &str) -> String {
    let mut s = p.replace('/', "\\");
    while s.ends_with('\\') {
        s.pop();
    }
    s.to_lowercase()
}

/// The unpacked CUA extension dir: this binary lives in <repo>/bin, the manifest
/// in <repo>. None when the checkout layout cannot be proven (then no profile
/// can match, and the reply says so through extension.installed=false).
fn diag_cua_root() -> Option<std::path::PathBuf> {
    let repo = std::env::current_exe().ok()?.parent()?.parent()?.to_path_buf();
    if repo.join("manifest.json").is_file() {
        Some(repo)
    } else {
        None
    }
}

/// One `tasklist` pass, filtered locally; None = detection itself failed, so
/// per-family `running` becomes null instead of a false "not running".
fn diag_running_processes() -> Option<HashMap<String, ()>> {
    let mut cmd = std::process::Command::new("tasklist");
    cmd.args(["/FO", "CSV", "/NH"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let mut names = HashMap::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(rest) = line.strip_prefix('"') {
            if let Some(end) = rest.find('"') {
                names.insert(rest[..end].to_lowercase(), ());
            }
        }
    }
    Some(names)
}

/// `reg query <key> /ve` → the default value (the NM manifest path); None when
/// the key is missing or reg is unavailable.
fn diag_reg_default_value(key: &str) -> Option<String> {
    let mut cmd = std::process::Command::new("reg");
    cmd.args(["query", key, "/ve"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(i) = line.find("REG_SZ") {
            let v = line[i + 6..].trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Scan one profile's Preferences for the CUA unpacked entry. cua: "enabled" /
/// "disabled" / "unknown" (Chromium state 1/0/other) / null on read failure;
/// parse: ok / unreadable / unparsable / too_large.
fn diag_profile_cua(prefs: &std::path::Path, cua_norm: Option<&str>) -> Value {
    let meta = match std::fs::metadata(prefs) {
        Ok(m) if m.is_file() => m,
        _ => return json!({"cua": "unknown", "parse": "unreadable"}),
    };
    if meta.len() > DIAG_MAX_PREFS_BYTES {
        return json!({"cua": "unknown", "parse": "too_large"});
    }
    let text = match std::fs::read_to_string(prefs) {
        Ok(t) => t,
        Err(_) => return json!({"cua": "unknown", "parse": "unreadable"}),
    };
    let v: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return json!({"cua": "unknown", "parse": "unparsable"}),
    };
    let settings = match v.pointer("/extensions/settings").and_then(|s| s.as_object()) {
        Some(s) => s,
        None => return json!({"cua": "absent", "parse": "ok"}),
    };
    for (id, e) in settings {
        let p = e.get("path").and_then(|p| p.as_str()).unwrap_or("");
        if let Some(cua_norm) = cua_norm {
            if diag_norm_path(p) == cua_norm {
                let state = match e.get("state").and_then(|s| s.as_i64()) {
                    Some(1) => "enabled",
                    Some(0) => "disabled",
                    _ => "unknown",
                };
                return json!({"cua": state, "ext_id": id, "parse": "ok"});
            }
        }
    }
    json!({"cua": "absent", "parse": "ok"})
}

/// NM manifest for one family, candidates in priority order: the HKCU registry
/// value's path (what Chromium actually reads on Windows), the %APPDATA%
/// convention, then ~/.cua/com.cua.bridge.json (`cua install` layout).
/// valid = name com.cua.bridge AND the manifest's `path` exe exists.
fn diag_nm_manifest(f: &FamilyDiag) -> Value {
    let reg_value = diag_reg_default_value(f.reg_key);
    let mut candidates: Vec<(String, std::path::PathBuf)> = Vec::new();
    if let Some(v) = &reg_value {
        candidates.push(("registry".to_string(), std::path::PathBuf::from(v)));
    }
    let app_root = diag_env_root(f.nm_manifest.0);
    let app_segs = f.nm_manifest.1.iter().collect::<std::path::PathBuf>();
    candidates.push(("appdata".to_string(), app_root.join(app_segs)));
    if let Some(home) = home_dir() {
        candidates.push(("home".to_string(), home.join(".cua").join("com.cua.bridge.json")));
    }
    for (via, path) in candidates {
        if !path.is_file() {
            continue;
        }
        let parsed: Option<Value> = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok());
        let Some(manifest) = parsed else {
            return json!({
                "present": true, "valid": false, "path": path.display().to_string(), "via": via,
                "registry_value": reg_value, "registry_key": f.reg_key, "note": "manifest unparsable",
            });
        };
        let exe = manifest.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let exe_ok = !exe.is_empty() && std::path::Path::new(&exe).is_file();
        let name_ok = manifest.get("name").and_then(|v| v.as_str()) == Some("com.cua.bridge");
        return json!({
            "present": true, "valid": name_ok && exe_ok, "path": path.display().to_string(),
            "manifest_path_field": if exe.is_empty() { Value::Null } else { json!(exe) },
            "exe_ok": exe_ok, "via": via, "registry_value": reg_value, "registry_key": f.reg_key,
        });
    }
    json!({
        "present": false, "valid": false, "path": Value::Null, "via": Value::Null,
        "registry_value": reg_value, "registry_key": f.reg_key,
    })
}

/// Assemble one family's diag record: exe candidates through the root table,
/// profile scan, NM manifest evaluation.
fn diag_family(f: &FamilyDiag, running: &Option<HashMap<String, ()>>, cua_norm: Option<&str>) -> Value {
    let exe = f
        .exes
        .iter()
        .map(|(root, segs)| diag_env_root(root).join(segs.iter().collect::<std::path::PathBuf>()))
        .find(|p| p.is_file());

    let root = diag_env_root(f.user_data.0).join(f.user_data.1.iter().collect::<std::path::PathBuf>());
    let mut profiles: Vec<Value> = Vec::new();
    let mut scanned = false;
    if let Ok(rd) = std::fs::read_dir(&root) {
        scanned = true;
        let mut dirs: Vec<std::path::PathBuf> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .collect();
        dirs.sort();
        // DIFFERENCE vs Node: Rust lists only profiles that carry a CUA entry or
        // a read failure — Node also lists CUA-absent profiles (capped at 20).
        for dir in dirs {
            let prefs = dir.join("Preferences");
            if !prefs.is_file() {
                continue; // no Preferences → not a profile dir
            }
            let r = diag_profile_cua(&prefs, cua_norm);
            if r.get("cua") == Some(&json!("absent")) {
                continue;
            }
            let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let mut entry = json!({"name": name});
            if let (Some(o), Some(m)) = (entry.as_object_mut(), r.as_object()) {
                for (k, v) in m {
                    o.insert(k.clone(), v.clone());
                }
            }
            profiles.push(entry);
        }
    }
    let running = match running {
        None => Value::Null,
        Some(set) => json!(set.contains_key(f.process)),
    };
    let installed = profiles.iter().any(|p| p.get("cua").map(|u| u.is_string()).unwrap_or(false));
    let enabled = profiles.iter().any(|p| p.get("cua") == Some(&json!("enabled")));
    json!({
        "family": f.family, "display": f.display, "manage_url": f.manage_url,
        "installed": exe.is_some(),
        "exe_path": exe.map(|p| json!(p.display().to_string())).unwrap_or(Value::Null),
        "running": running,
        "extension": {
            "installed": installed,
            "enabled": enabled,
            "profiles": profiles,
            "profile_root": root.display().to_string(),
            "scanned": scanned,
        },
        "nm_manifest": diag_nm_manifest(f),
    })
}

/// `diag.browsers` result: the read-only scan plus this daemon's live state.
fn diag_browsers_result(state: &Arc<AppState>) -> Value {
    let running = diag_running_processes();
    let cua_norm = diag_cua_root().map(|p| diag_norm_path(&p.display().to_string()));
    let browsers: Vec<Value> = DIAG_FAMILIES.iter().map(|f| diag_family(f, &running, cua_norm.as_deref())).collect();
    let connected = !state.browser_names().is_empty();
    json!({
        "daemon": {
            "port": state_port(state),
            "port_ok": true,
            "pairing_ok": connected,
            "extension_connected": connected,
        },
        "browsers": browsers,
        "slots": state.list_browsers(),
        "limitations": [
            "daemon/diag.mjs (Node) is the reference implementation; this minimal scan lists only profiles carrying a CUA entry or a read failure (Node also lists CUA-absent profiles, capped at 20 per family)",
            "when tasklist fails, running is null rather than false; on non-Windows hosts the Windows matrix finds nothing",
        ],
    })
}

/// Spawn an isolated-profile browser instance that self-pairs as slot `name`.
/// Shared by the explicit `browser.launch` method and the nameless-call
/// workspace auto-launch. Returns the result object ({"error": …} on failure).
fn launch_browser_workspace(state: &Arc<AppState>, name: &str, params: &Value) -> Value {
    let cfg = state.read_config();    // 1) browser executable: param > config > auto-detect Edge then Chrome
    let param_exe = params.get("exe").and_then(|v| v.as_str()).unwrap_or("");
    let cfg_exe = cfg.get("browser_exe").and_then(|v| v.as_str()).unwrap_or("");
    let chosen = param_exe.to_string();
    let chosen = if !chosen.is_empty() {
        chosen
    } else if !cfg_exe.is_empty() {
        cfg_exe.to_string()
    } else {
        let mut found: Option<String> = None;
        for shortcut in [param_exe, "edge", "chrome"] {
            let base = match shortcut {
                "edge" => vec![
                    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe".to_string(),
                    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe".to_string(),
                ],
                "chrome" => vec![
                    r"C:\Program Files\Google\Chrome\Application\chrome.exe".to_string(),
                    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe".to_string(),
                ],
                "" => continue,
                other => vec![other.to_string()],
            };
            if let Some(p) = base.iter().find(|p| std::path::Path::new(p.as_str()).is_file()) {
                found = Some(p.clone());
                break;
            }
        }
        match found {
            Some(p) => p,
            None => return json!({"error": {"code": "BROWSER_EXE_NOT_FOUND", "message": "could not locate msedge.exe/chrome.exe — pass exe (\"edge\", \"chrome\" or an absolute path), or set browser_exe in ~/.cua/config.json"}}),
        }
    };
    if !std::path::Path::new(&chosen).is_file() {
        return json!({"error": {"code": "BROWSER_EXE_NOT_FOUND", "message": format!("exe not found: {chosen}")}});
    }
    // 2) extension dir: param > config > this binary's own repo checkout
    let param_ext = params.get("extension_dir").and_then(|v| v.as_str()).unwrap_or("");
    let cfg_ext = cfg.get("extension_dir").and_then(|v| v.as_str()).unwrap_or("");
    let ext_dir = if !param_ext.is_empty() {
        param_ext.to_string()
    } else if !cfg_ext.is_empty() {
        cfg_ext.to_string()
    } else {
        // the daemon binary lives in <repo>/bin, the extension manifest in <repo>
        let guess = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().and_then(|p| p.parent()).map(|p| p.to_path_buf()));
        match guess {
            Some(g) if g.join("manifest.json").is_file() => g.display().to_string(),
            _ => return json!({"error": {"code": "BAD_PARAMS", "message": "extension_dir unknown — pass it, or set extension_dir in ~/.cua/config.json"}}),
        }
    };
    if !std::path::Path::new(&ext_dir).join("manifest.json").is_file() {
        return json!({"error": {"code": "BAD_PARAMS", "message": format!("no manifest.json under {ext_dir}")}});
    }
    // 3) profile dir (kept: relaunching the same name reuses the profile, so
    //    logins made inside a launched instance survive)
    let profile_dir = home_dir()
        .unwrap_or_default()
        .join(".cua")
        .join("profiles")
        .join(name);
    if let Err(e) = std::fs::create_dir_all(&profile_dir) {
        return json!({"error": {"code": "PROFILE_DIR_FAILED", "message": format!("create_dir_all {}: {e}", profile_dir.display())}});
    }
    // 4) one-shot pairing nonce → the pair page embeds the credentials
    let Some(nonce) = crypto_token() else {
        return json!({"error": {"code": "INTERNAL", "message": "OS CSPRNG unavailable"}});
    };
    state.nonces.lock().unwrap().insert(nonce.clone(), (name.to_string(), Instant::now()));
    // unpacked extension id = sha256(abs path)[:32 hex] mapped to a-p — used by
    // the pair page's chrome.runtime.sendMessage target
    let ext_id = unpacked_ext_id(&ext_dir);
    let port = state_port(state);
    let url = format!("http://127.0.0.1:{port}/__cua_pair?nonce={nonce}");
    let mut cmd = std::process::Command::new(&chosen);
    cmd.args([
        format!("--user-data-dir={}", profile_dir.display()),
        format!("--load-extension={ext_dir}"),
        // background control: the workspace must not steal the user's focus.
        // Honoured by Chromium on Windows (first window starts minimized
        // without activation); where a build ignores it, the extension's own
        // one-shot minimize-on-start (cua_workspace flag) is the fallback.
        "--start-minimized".to_string(),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        url,
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    match cmd.spawn() {
        Ok(_) => json!({
            "launched": true,
            "name": name,
            "exe": chosen,
            "profile_dir": profile_dir.display().to_string(),
            "extension_id": ext_id,
        }),
        Err(e) => json!({"error": {"code": "SPAWN_FAILED", "message": format!("spawn {chosen}: {e}")}}),
    }
}

/// `browser.launch`: spawn a browser instance with an isolated profile and the
/// unpacked CUA extension, pointed at the daemon's one-shot pairing page. The
/// fresh profile's extension stores {token, port, slot name} through that page
/// and connects under the requested name.
fn browser_launch(state: &Arc<AppState>, msg: &Value) -> Value {
    let name = msg.pointer("/params/name").and_then(|v| v.as_str()).unwrap_or("");
    if !valid_browser_name(name) {
        return json!({"error": {"code": "BAD_PARAMS", "message": "browser.launch needs name matching [a-z0-9_-]{1,32} (the slot the new instance registers under)"}});
    }
    let empty = json!({});
    let params = msg.get("params").filter(|p| p.is_object()).unwrap_or(&empty);
    let mut out = launch_browser_workspace(state, name, params);
    if out.get("error").is_none() {
        if let Some(o) = out.as_object_mut() {
            o.insert("note".to_string(), json!("the fresh profile pairs itself through the daemon's one-shot page (started minimized — it does not steal focus); wait for the name to appear (cua browsers), then target it with browser:\"<name>\" / cua --browser <name>"));
        }
    }
    out
}

/// Port this daemon listens on — threaded through AppState via a lazily-set
/// cell so browser.launch can build the pair URL without new plumbing.
static DAEMON_PORT: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
fn state_port(_state: &Arc<AppState>) -> u16 {
    *DAEMON_PORT.get().unwrap_or(&8765)
}

/// Chrome-family unpacked extension id: sha256(absolute path), first 32 hex
/// chars, each hex digit mapped to a-p. Chromium hashes the RAW bytes of
/// base::FilePath::value — wchar_t (UTF-16LE) on Windows, char (UTF-8)
/// elsewhere. The old UTF-8 hash produced an id the real extension never had
/// (measured on Edge 156: sha256(utf16le) matches, sha256(utf8) does not), so
/// the pair page messaged a ghost and browser.launch could never pair.
fn unpacked_ext_id(path: &str) -> String {
    use sha2::Digest as _;
    let mut h = Sha256::new();
    if cfg!(windows) {
        for u in path.encode_utf16() {
            h.update(u.to_le_bytes());
        }
    } else {
        h.update(path.as_bytes());
    }
    let d = h.finalize();
    d.iter()
        .take(16)
        .flat_map(|b| [b >> 4, b & 0x0f])
        .map(|n| char::from(b'a' + n))
        .collect()
}

/// Extension root for pairing: param > config > the installed copy under the
/// data dir > the checkout (`browser/extension` next to the repo root).
fn resolve_extension_dir(state: &Arc<AppState>, params: &Value) -> Result<String, Value> {
    let cfg = state.read_config();
    let mut cands: Vec<String> = Vec::new();
    for v in [
        params.get("extension_dir").and_then(|v| v.as_str()).unwrap_or(""),
        cfg.get("extension_dir").and_then(|v| v.as_str()).unwrap_or(""),
    ] {
        let v = v.trim();
        if !v.is_empty() {
            cands.push(v.to_string());
        }
    }
    cands.push(
        crate::config::default_extension_dir()
            .display()
            .to_string(),
    );
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for up in [dir.to_path_buf(), dir.join("..").join("..")] {
                cands.push(up.join("browser").join("extension").display().to_string());
            }
        }
    }
    for c in cands {
        if std::path::Path::new(&c).join("manifest.json").is_file() {
            return Ok(c);
        }
    }
    Err(json!({"code": "BAD_PARAMS", "message": format!(
        "extension_dir unknown — run `cua browser install`, or set extension_dir in {}",
        crate::config::path().display())}))
}

/// One-shot tool call through the normal agent routing, for daemon-internal
/// features (audit persistence, tunnel). The call rides the same rules a local
/// agent gets — slot routing, vault substitution, INTERNAL_ONLY refusals — and
/// the reply lands on a local channel instead of any socket. A hung extension
/// cannot wedge the caller: `timeout` caps the wait, and a late reply finds the
/// receiver gone (the route map entry is consumed either way).
pub fn call_extension(
    state: &Arc<AppState>,
    slot: Option<&str>,
    tool: &str,
    params: Value,
    timeout: std::time::Duration,
) -> Result<Value, Value> {
    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    let token = state.seq.fetch_add(1, Ordering::SeqCst) + 1;
    let sink = Sink::Local(token, tx);
    let mut msg = json!({
        "id": format!("x{}", state.seq.fetch_add(1, Ordering::SeqCst) + 1),
        "method": tool,
        "params": params,
    });
    if let Some(name) = slot {
        msg["browser"] = json!(name);
    }
    handle_agent_message(state, &sink, &msg);
    match rx.recv_timeout(timeout) {
        Ok(v) => match v.get("error") {
            Some(e) => Err(e.clone()),
            None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
        },
        Err(_) => Err(json!({
            "code": "ROUTE_TIMEOUT",
            "message": format!("\"{tool}\" produced no reply within {} ms; the outcome is unknown", timeout.as_millis()),
        })),
    }
}

fn handle_agent_message(state: &Arc<AppState>, sink: &Sink, msg: &Value) {
    let method = msg.get("method").and_then(|m| m.as_str()).map(String::from);
    if method.as_deref() == Some("cancel") {
        let want = msg.pointer("/params/id").cloned();
        if let Some(want) = want {
            let target = {
                let routes = state.routes.lock().unwrap();
                routes.iter().find_map(|(int_id, r)| {
                    // identity, not just caller-chosen id: another agent using the
                    // same id must not cancel this agent's work
                    (r.agent_id == want && r.sink == *sink).then(|| (int_id.clone(), r.browser.clone()))
                })
            };
            if let Some((int_id, browser)) = target {
                if let Ok(out) = state.select_ext(Some(&browser)) {
                    let _ = out.send(Message::Text(
                        json!({"method": "cancel", "params": {"id": int_id}}).to_string().into(),
                    ));
                }
            }
        }
        return;
    }
    let Some(method) = method else {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        state.send_agent(
            sink,
            &json!({"id": id, "error": {"code": "BAD_REQUEST", "message": "need id and method"}}),
        );
        return;
    };
    // daemon-side methods never touch an extension slot
    if daemon_method(state, sink, msg, &method) {
        return;
    }
    if method == "browser.launch" {
        let out = browser_launch(state, msg);
        state.send_agent(sink, &json!({"id": msg.get("id").cloned().unwrap_or(Value::Null), "result": out}));
        return;
    }
    let id = msg.get("id").cloned();
    let Some(id) = id.filter(|v| !v.is_null()) else {
        state.send_agent(
            sink,
            &json!({"id": Value::Null, "error": {"code": "BAD_REQUEST", "message": "need id and method"}}),
        );
        return;
    };
    let (want, params) = extract_browser(msg);
    let cfg = state.agent_cfg();
    // browser.close must never start a browser just to be routed to it
    let auto_open_exempt = method == "browser.close";
    let want = match want {
        Some(n) => {
            let connected = state.exts.lock().unwrap().contains_key(&n);
            if !connected && n == cfg.user_slot && cfg.auto_open_user && !auto_open_exempt {
                let st = state.clone();
                let sink2 = sink.clone();
                let id2 = id.clone();
                let method2 = method.clone();
                let cfg2 = cfg.clone();
                tokio::spawn(async move {
                    match ensure_user_browser_slot(&st, &cfg2).await {
                        Ok(name) => forward_agent_request(&st, &sink2, id2, &method2, params, Some(name)),
                        Err(e) => st.send_agent(&sink2, &json!({"id": id2, "error": e})),
                    }
                });
                return;
            }
            Some(n)
        }
        None => {
            // explicit opt-in: workspace-first behaviour
            if cfg.auto_workspace {
                if state.exts.lock().unwrap().contains_key(&cfg.slot) {
                    Some(cfg.slot.clone())
                } else {
                    let st = state.clone();
                    let sink2 = sink.clone();
                    let id2 = id.clone();
                    let method2 = method.clone();
                    let cfg2 = cfg.clone();
                    tokio::spawn(async move {
                        match ensure_workspace_slot(&st, &cfg2).await {
                            Ok(name) => forward_agent_request(&st, &sink2, id2, &method2, params, Some(name)),
                            Err(e) => st.send_agent(&sink2, &json!({"id": id2, "error": e})),
                        }
                    });
                    return;
                }
            } else if cfg.auto_open_user {
                // default: the real logged-in browser, opened on demand
                match state.real_slot(&cfg) {
                    Ok(Some(n)) => Some(n),
                    Ok(None) if auto_open_exempt => None,
                    Ok(None) => {
                        let st = state.clone();
                        let sink2 = sink.clone();
                        let id2 = id.clone();
                        let method2 = method.clone();
                        let cfg2 = cfg.clone();
                        tokio::spawn(async move {
                            match ensure_user_browser_slot(&st, &cfg2).await {
                                Ok(name) => forward_agent_request(&st, &sink2, id2, &method2, params, Some(name)),
                                Err(e) => st.send_agent(&sink2, &json!({"id": id2, "error": e})),
                            }
                        });
                        return;
                    }
                    Err(e) => {
                        state.send_agent(sink, &json!({"id": id, "error": e}));
                        return;
                    }
                }
            } else {
                None // legacy select_ext rules
            }
        }
    };
    forward_agent_request(state, sink, id, &method, params, want);
}

/// Tail of agent request handling: resolve the slot, register the route,
/// forward. A refusal goes back on the agent socket verbatim.
fn forward_agent_request(
    state: &Arc<AppState>,
    sink: &Sink,
    id: Value,
    method: &str,
    params: Value,
    want: Option<String>,
) {
    // Forward-time credential pass: substitute {{secret:NAME}} where the schema
    // allows it, stamp the binding gate on secret.fill, restore placeholders on
    // replay.save, strip forged markers everywhere else. A refusal here never
    // reaches the extension, so no plaintext crosses the socket.
    let params = match vault::prepare_forward(method, &params) {
        Ok(fwd) => {
            let arr = |key: &str| -> Vec<String> {
                fwd.get(key)
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default()
            };
            let scope = arr("out_of_scope");
            if !scope.is_empty() {
                let what = if method == "fill_form" { "values[*]" } else { "value" };
                let msg = format!(
                    "{{{{secret:...}}}} is substituted only inside {what} — found in {}; nothing was forwarded",
                    scope.iter().take(3).map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
                );
                state.send_agent(sink, &json!({"id": id, "error": {"code": "SECRET_SCOPE", "message": msg}}));
                return;
            }
            let invalid = arr("invalid");
            if !invalid.is_empty() {
                let msg = format!(
                    "malformed secret name \"{}\" (allowed: letters, digits, _ . - up to 64 chars)",
                    invalid.first().map(|s| s.as_str()).unwrap_or(""),
                );
                state.send_agent(sink, &json!({"id": id, "error": {"code": "BAD_SECRET_NAME", "message": msg}}));
                return;
            }
            let missing = arr("missing");
            if !missing.is_empty() {
                let msg = format!(
                    "no secret named \"{}\" in the local vault — list names with the secret.list tool, store one with secret.set. Never put the plaintext value in a tool argument.",
                    missing.first().map(|s| s.as_str()).unwrap_or(""),
                );
                state.send_agent(sink, &json!({"id": id, "error": {"code": "SECRET_UNKNOWN", "message": msg}}));
                return;
            }
            let denied = arr("denied");
            if !denied.is_empty() {
                let msg = format!(
                    "secret \"{}\" has no origin binding and secret_origin_mode is strict — nothing was forwarded. Bind it in %USERPROFILE%\\.cua\\secrets.json (or via secret.set with origins), or relax the mode there.",
                    denied.first().map(|s| s.as_str()).unwrap_or(""),
                );
                state.send_agent(sink, &json!({"id": id, "error": {"code": "SECRET_ORIGIN_DENIED", "message": msg}}));
                return;
            }
            fwd.get("params").cloned().unwrap_or(params)
        }
        Err((code, message)) => {
            state.send_agent(sink, &json!({"id": id, "error": {"code": code, "message": message}}));
            return;
        }
    };
    match state.select_ext(want.as_deref()) {
        Ok(out) => {
            let int_id = format!("x{}", state.seq.fetch_add(1, Ordering::SeqCst) + 1);
            state.routes.lock().unwrap().insert(
                int_id.clone(),
                Route {
                    sink: sink.clone(),
                    agent_id: id.clone(),
                    browser: want.unwrap_or_else(|| state.browser_names().first().cloned().unwrap_or_default()),
                },
            );
            let _ = out.send(Message::Text(
                json!({"id": int_id, "method": method, "params": params}).to_string().into(),
            ));
        }
        Err(e) => state.send_agent(
            sink,
            &json!({"id": id, "error": {"code": e.get("code").cloned().unwrap_or(json!("EXTENSION_NOT_CONNECTED")), "message": e.get("message").cloned().unwrap_or(json!(""))}}),
        ),
    }
}

/// Resolve the browser binary for the user's normal profile. `user_browser_exe`
/// wins, then `browser_exe`; "edge"/"chrome" shortcuts and auto-detect follow.
fn resolve_user_browser_exe(cfg: &AgentCfg) -> Result<String, Value> {
    let edge = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ];
    let chrome = [
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ];
    let raw = if !cfg.user_exe.is_empty() { cfg.user_exe.as_str() } else { cfg.fallback_exe.as_str() };
    if !raw.is_empty() {
        if raw == "edge" || raw == "chrome" {
            let list = if raw == "edge" { &edge[..] } else { &chrome[..] };
            return list
                .iter()
                .find(|p| std::path::Path::new(p).is_file())
                .map(|p| p.to_string())
                .ok_or_else(|| json!({"code": "BROWSER_EXE_NOT_FOUND", "message": format!("could not locate {raw} — set user_browser_exe in ~/.cua/config.json")}));
        }
        return if std::path::Path::new(raw).is_file() {
            Ok(raw.to_string())
        } else {
            Err(json!({"code": "BROWSER_EXE_NOT_FOUND", "message": format!("user_browser_exe does not exist: {raw}")}))
        };
    }
    edge.iter()
        .chain(chrome.iter())
        .find(|p| std::path::Path::new(p).is_file())
        .map(|p| p.to_string())
        .ok_or_else(|| json!({"code": "BROWSER_EXE_NOT_FOUND", "message": "could not locate msedge.exe/chrome.exe — set user_browser_exe (or browser_exe) in ~/.cua/config.json"}))
}

/// Start the browser's NORMAL profile (no --user-data-dir, no --load-extension):
/// the extension already lives there with the user's cookies and logins.
fn launch_user_browser(cfg: &AgentCfg) -> Value {
    let exe = match resolve_user_browser_exe(cfg) {
        Ok(e) => e,
        Err(e) => return json!({"error": e}),
    };
    let mut cmd = std::process::Command::new(&exe);
    #[cfg(windows)]
    cmd.arg("--start-minimized");
    cmd.arg("about:blank");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    match cmd.spawn() {
        Ok(child) => json!({"launched": true, "exe": exe, "pid": child.id(), "profile": "default"}),
        Err(e) => json!({"error": {"code": "SPAWN_FAILED", "message": format!("spawn {exe}: {e}")}}),
    }
}

/// Wait for the user's real browser slot; launch the normal profile once when
/// missing. Never falls back to an isolated workspace.
async fn ensure_user_browser_slot(state: &Arc<AppState>, cfg: &AgentCfg) -> Result<String, Value> {
    let _guard = state.launch_lock.lock().await;
    if let Some(name) = state.real_slot(cfg)? {
        return Ok(name);
    }
    let launch = launch_user_browser(cfg);
    if let Some(e) = launch.get("error") {
        return Err(e.clone());
    }
    let exe = launch.get("exe").and_then(|v| v.as_str()).unwrap_or("").to_string();
    log(&format!(
        "user browser {:?} spawning ({}); waiting up to {}ms for its extension",
        cfg.user_slot, exe, cfg.user_wait_ms
    ));
    let deadline = Instant::now() + Duration::from_millis(cfg.user_wait_ms);
    while Instant::now() < deadline {
        if let Some(name) = state.real_slot(cfg)? {
            return Ok(name);
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    Err(json!({
        "code": "USER_BROWSER_LAUNCH_TIMEOUT",
        "message": format!(
            "the user browser was started with its normal profile ({}) but no real CUA slot paired within {}ms — check that the extension is installed and enabled in that profile, or set auto_open_user_browser:false / use browser.launch for an isolated instance",
            exe, cfg.user_wait_ms
        ),
    }))
}

/// Spawn the agent workspace (isolated profile, one-shot pair page) and wait
/// for it to self-pair under its slot name. Serialized by launch_lock so
/// concurrent nameless calls share one spawn; a caller that waited behind the
/// lock re-checks first (the workspace may have paired in the meantime).
async fn ensure_workspace_slot(state: &Arc<AppState>, cfg: &AgentCfg) -> Result<String, Value> {
    let _guard = state.launch_lock.lock().await;
    if state.exts.lock().unwrap().contains_key(&cfg.slot) {
        return Ok(cfg.slot.clone());
    }
    let launch = launch_browser_workspace(state, &cfg.slot, &json!({}));
    if let Some(e) = launch.get("error") {
        return Err(e.clone());
    }
    let exe = launch.get("exe").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let profile = launch.get("profile_dir").and_then(|v| v.as_str()).unwrap_or("").to_string();
    log(&format!(
        "agent workspace {:?} spawning ({}); waiting up to {}ms for it to self-pair",
        cfg.slot, exe, cfg.wait_ms
    ));
    let deadline = Instant::now() + Duration::from_millis(cfg.wait_ms);
    while Instant::now() < deadline {
        if state.exts.lock().unwrap().contains_key(&cfg.slot) {
            return Ok(cfg.slot.clone());
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    Err(json!({
        "code": "AGENT_LAUNCH_TIMEOUT",
        "message": format!(
            "the agent workspace {:?} was spawned (profile {}) but did not pair within {}ms — check that {} actually started and loaded the extension (--load-extension is dropped when another instance of the SAME profile is already running). In auto_workspace mode a nameless call stays on this workspace target; name another slot explicitly with cua --browser <name> / params.browser if you meant to.",
            cfg.slot, profile, cfg.wait_ms, exe
        ),
    }))
}

/// browser.launch is answered synchronously on the agent socket (std spawn is
/// non-blocking); no async plumbing needed.

/// Per-connection extension context handed to every inbound extension frame.
struct ExtConn {
    state: Arc<AppState>,
    tx: Tx,
    tick: Arc<AtomicU64>,
    last_frame: Arc<Mutex<Instant>>,
    /// set the moment hello registers this socket under a slot name
    name: Option<String>,
    gen: u64,
}

impl ExtConn {
    fn touch(&self) {
        self.tick.fetch_add(1, Ordering::SeqCst);
        *self.last_frame.lock().unwrap() = Instant::now();
    }
}

/// Register (or refuse) this connection under its hello-declared slot name.
/// A live incumbent under the same name wins the probe and refuses the
/// newcomer; a dead one is replaced, exactly like the old single-slot rule.
fn hello_register(conn: &mut ExtConn, msg: &Value) -> Result<(), String> {
    if conn.name.is_some() {
        return Ok(()); // re-hello on the same socket: info refresh only
    }
    let raw = msg.pointer("/params/browser_name").and_then(|v| v.as_str()).unwrap_or("default");
    let name = if valid_browser_name(raw) { raw.to_string() } else { "default".to_string() };
    let incumbent_dead = {
        let exts = conn.state.exts.lock().unwrap();
        match exts.get(&name) {
            Some(_) => None, // decided by the async probe below
            None => Some(()),
        }
    };
    if incumbent_dead.is_none() {
        // bounded probe of the incumbent — block_in_place keeps the connection
        // task honest without making hello_register async (registration only
        // ever happens once per socket, so the brief block is harmless here).
        // None means the slot vanished while we probed: register fresh.
        let probed: Option<bool> = tokio::task::block_in_place(|| {
            let out_tick = {
                let exts = conn.state.exts.lock().unwrap();
                exts.get(&name).map(|e| (e.out.clone(), e.tick.clone()))
            };
            let Some((out, tick)) = out_tick else { return None };
            let before = tick.load(Ordering::SeqCst);
            let _ = out.send(Message::Text(json!({"method": "ping"}).to_string().into()));
            let deadline = Instant::now() + Duration::from_millis(1500);
            let mut alive = false;
            while Instant::now() < deadline {
                if tick.load(Ordering::SeqCst) != before {
                    alive = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Some(alive)
        });
        match probed {
            Some(true) => {
                return Err(format!("duplicate extension connection for browser {name:?} (incumbent is alive)"));
            }
            Some(false) => {
                log("incumbent extension connection is dead, replacing");
                drop_ext_named(&conn.state, &name, "replaced", None);
            }
            None => {}
        }
    }
    let gen = conn.state.ext_seq.fetch_add(1, Ordering::SeqCst) + 1;
    let info = msg.get("params").cloned().unwrap_or(Value::Null);
    conn.state.exts.lock().unwrap().insert(
        name.clone(),
        ExtState {
            out: conn.tx.clone(),
            info: Mutex::new(info.clone()),
            last_frame: conn.last_frame.clone(),
            tick: conn.tick.clone(),
            gen,
            name: name.clone(),
        },
    );
    conn.name = Some(name.clone());
    conn.gen = gen;
    log(&format!("extension connected: browser {name:?}"));
    conn.state.broadcast(&json!({"method": "event.extension.connected", "params": {"browser": name, "info": info}}));
    Ok(())
}

fn handle_extension_message(conn: &mut ExtConn, msg: &Value) -> bool {
    let method = msg.get("method").and_then(|m| m.as_str()).map(String::from);
    match method.as_deref() {
        Some("ping") => {
            let _ = conn.tx.send(Message::Text(json!({"method": "pong"}).to_string().into()));
            false
        }
        Some("hello") => {
            let min = msg.pointer("/params/min_compatible").cloned().unwrap_or(json!("1.0"));
            if major(&min) > 1 {
                let _ = conn.tx.send(Message::Text(
                    json!({"id": msg.get("id"), "error": {"code": "version_skew", "message": "daemon supports 1.x, extension wants a newer protocol"}})
                        .to_string()
                        .into(),
                ));
                return false;
            }
            match hello_register(conn, msg) {
                Ok(()) => {
                    let _ = conn.tx.send(Message::Text(
                        json!({"id": msg.get("id"), "result": {"protocol_version": "1.0", "daemon": "cua-bridge-rs 1.1.0"}})
                            .to_string()
                            .into(),
                    ));
                    // refresh the stored info on every hello (a renamed profile
                    // reconnecting keeps its slot current)
                    if let Some(n) = &conn.name {
                        if let Some(e) = conn.state.exts.lock().unwrap().get_mut(n) {
                            *e.info.lock().unwrap() = msg.get("params").cloned().unwrap_or(Value::Null);
                        }
                    }
                    false
                }
                Err(reason) => {
                    let _ = conn.tx.send(Message::Text(
                        json!({"id": msg.get("id"), "error": {"code": "duplicate_browser_connection", "message": reason}})
                            .to_string()
                            .into(),
                    ));
                    true // close this newcomer socket
                }
            }
        }
        _ => {
            // Extension→daemon reverse RPC: the secret vault and the read-only
            // browser diagnostics scan. The reply goes back on this extension
            // socket ONLY — a vault value must never reach broadcast()/routes
            // and be copied to every connected agent.
            let reverse_id = msg.get("id").and_then(|v| v.as_str()).filter(|s| s.starts_with('s'));
            if msg.get("method").is_some() && reverse_id.is_some() {
                let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                let payload = match method {
                    "diag.browsers" => Ok(diag_browsers_result(&conn.state)),
                    m => vault::handle_reverse(m, &params),
                };
                let reply = match payload {
                    Ok(result) => json!({ "id": msg.get("id"), "result": result }),
                    Err((code, message)) => json!({ "id": msg.get("id"), "error": {"code": code, "message": message} }),
                };
                let _ = conn.tx.send(Message::Text(reply.to_string().into()));
                return false;
            }
            let has_id = msg.get("id").map(|v| !v.is_null()).unwrap_or(false);
            let is_reply = has_id && (msg.get("result").is_some() || msg.get("error").is_some());
            if is_reply {
                let key = msg.get("id").and_then(|v| v.as_str()).map(String::from);
                if let Some(key) = key {
                    let route = conn.state.routes.lock().unwrap().remove(&key);
                    if let Some(r) = route {
                        let mut out = json!({"id": r.agent_id});
                        if let (Some(a), Some(m)) = (out.as_object_mut(), msg.as_object()) {
                            for (k, v) in m {
                                if k != "id" {
                                    a.insert(k.clone(), v.clone());
                                }
                            }
                        }
                        conn.state.send_agent(&r.sink, &out);
                    }
                }
            } else if msg.get("method").is_some() {
                conn.state.broadcast(msg); // events → all agents
            }
            false
        }
    }
}

// ------------------------------------------------------------------ ws server

fn ws_accept(key: &str) -> String {
    let mut h = Sha1::new();
    h.update(key.as_bytes());
    h.update(GUID.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(h.finalize())
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

async fn handle_connection(state: Arc<AppState>, token: String, mut stream: TcpStream, addr: SocketAddr) {
    let loopback = match addr.ip() {
        std::net::IpAddr::V4(v4) => v4.is_loopback(),
        std::net::IpAddr::V6(v6) => v6.is_loopback(),
    };
    if !loopback {
        log("rejected non-loopback connection");
        return;
    }

    // read the HTTP request head (GET requests carry no body, so stopping at
    // the first \r\n\r\n is safe for both /health and the ws upgrade)
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(i) = find_head_end(&buf) {
            buf.truncate(i);
            break;
        }
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
        if buf.len() > 64 * 1024 {
            return;
        }
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let _method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    let mut sec_ws_key: Option<String> = None;
    let mut offered: Vec<String> = Vec::new();
    let mut is_upgrade = false;
    for line in lines {
        let Some((k, v)) = line.split_once(':') else { continue };
        match k.trim().to_ascii_lowercase().as_str() {
            "sec-websocket-key" => sec_ws_key = Some(v.trim().to_string()),
            "sec-websocket-protocol" => {
                offered = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
            }
            "upgrade" => is_upgrade = v.trim().eq_ignore_ascii_case("websocket"),
            _ => {}
        }
    }

    if !is_upgrade {
        let (status, body, content_type) = if path.starts_with("/shutdown") {
            if !path.contains(&format!("token={token}")) {
                ("403 Forbidden", "{}".to_string(), "application/json")
            } else {
                log("shutdown requested");
                let b = json!({"ok": true, "shutting_down": true}).to_string();
                let _ = write_http(&mut stream, "200 OK", &b, "application/json").await;
                // give the socket a beat to flush before exit, like the node daemon
                tokio::time::sleep(Duration::from_millis(150)).await;
                std::process::exit(0);
            }
        } else if path.starts_with("/health") {
            let names = state.browser_names();
            let agents = state.ws_agents.lock().unwrap().len() + 1; // + stdio agent
            // extension_build is the version proof (README: `extension: true`
            // alone never proved WHICH code is answering)
            let build = {
                let exts = state.exts.lock().unwrap();
                exts.values()
                    .filter_map(|e| e.info.lock().unwrap().get("build").and_then(|b| b.as_str()).map(String::from))
                    .next()
            };
            let route = state.agent_cfg();
            (
                "200 OK",
                json!({
                    "ok": true,
                    "extension": !names.is_empty(),
                    "browsers": names,
                    "extension_build": build,
                    "agents": agents,
                    "token_id": token_id(&token),
                    "user_browser": route.user_slot,
                    "auto_open_user_browser": route.auto_open_user,
                    "agent_slot": route.slot,
                    "auto_workspace": route.auto_workspace
                })
                .to_string(),
                "application/json",
            )
        } else if let Some(rest) = path.strip_prefix("/__cua_pair?") {
            // one-shot pairing page for browser.launch instances: the nonce is
            // minted by pair-registered launchers, single-use, 120s TTL, and
            // consumed the moment the page is served
            let nonce = rest
                .split('&')
                .find_map(|kv| kv.strip_prefix("nonce="))
                .unwrap_or("")
                .to_string();
            // The extension's content script cannot read the page's JS, so it
            // asks for the same credentials as JSON instead of rendering them.
            let want_json = rest.split('&').any(|kv| kv == "format=json");
            let claim: Option<String> = {
                let mut nonces = state.nonces.lock().unwrap();
                let valid = nonces
                    .get(&nonce)
                    .map(|(_, minted)| minted.elapsed() < PAIR_NONCE_TTL)
                    .unwrap_or(false);
                if valid {
                    nonces.remove(&nonce).map(|(name, _)| name)
                } else {
                    None
                }
            };
            match claim {
                Some(name) => {
                    let cfg = read_config();
                    let tok = cfg.get("token").and_then(|v| v.as_str()).unwrap_or(&token);
                    let ext_dir = cfg.get("extension_dir").and_then(|v| v.as_str()).map(String::from);
                    let ext_dir = ext_dir.unwrap_or_else(|| {
                        std::env::current_exe()
                            .ok()
                            .and_then(|e| e.parent().and_then(|p| p.parent()).map(|p| p.display().to_string()))
                            .unwrap_or_default()
                    });
                    let page = pair_page(tok, state_port(&state), &name, &nonce, &unpacked_ext_id(&ext_dir));
                    if want_json {
                        (
                            "200 OK",
                            json!({
                                "ok": true,
                                "slot": name,
                                "token": tok,
                                "port": state_port(&state),
                                "nonce": nonce,
                            })
                            .to_string(),
                            "application/json",
                        )
                    } else {
                        ("200 OK", page, "text/html; charset=utf-8")
                    }
                }
                None => (
                    "404 Not Found",
                    json!({"ok": false, "error": "unknown or expired pairing nonce (single-use, 120s TTL)"}).to_string(),
                    "application/json",
                ),
            }
        } else {
            ("404 Not Found", String::new(), "application/json")
        };
        let _ = write_http(&mut stream, status, &body, content_type).await;
        return;
    }

    // ---- websocket upgrade (echo the first offered protocol, like wslib)
    let Some(key) = sec_ws_key else { return };
    let mut resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n",
        ws_accept(&key)
    );
    if let Some(c) = offered.first() {
        resp.push_str(&format!("Sec-WebSocket-Protocol: {c}\r\n"));
    }
    resp.push_str("\r\n");
    if stream.write_all(resp.as_bytes()).await.is_err() {
        return;
    }
    let ws = tokio_tungstenite::WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
    let (mut write, mut read) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            if write.send(m).await.is_err() {
                break;
            }
        }
        let _ = write.close().await;
    });

    // ---- authenticate
    let role: &'static str = match &offered {
        p if p.first().map(String::as_str) == Some(&format!("cua-auth.{token}")) => "extension",
        p if p.first().map(String::as_str) == Some(&format!("cua-agent.{token}")) => "agent",
        p if !p.is_empty() => {
            let _ = tx.send(close(1008, "authentication failed"));
            return;
        }
        _ => {
            // no subprotocol: first message must be auth within 5s
            let first = tokio::time::timeout(Duration::from_secs(5), read.next()).await;
            let Ok(Some(Ok(Message::Text(t)))) = first else {
                let _ = tx.send(close(1008, "auth timeout"));
                return;
            };
            let msg: Value = serde_json::from_str(t.as_str()).unwrap_or(Value::Null);
            let ok = msg.get("method").and_then(|m| m.as_str()) == Some("auth")
                && msg.pointer("/params/token").and_then(|x| x.as_str()) == Some(token.as_str());
            if !ok {
                let _ = tx.send(close(1008, "authentication failed"));
                return;
            }
            if msg.pointer("/params/role").and_then(|r| r.as_str()) == Some("agent") {
                "agent"
            } else {
                "extension"
            }
        }
    };

    // ---- per-connection extension context (registration happens at hello)
    let tick = Arc::new(AtomicU64::new(0));
    let last_frame = Arc::new(Mutex::new(Instant::now()));
    let my_agent_id: u64;
    if role == "extension" {
        my_agent_id = 0; // the slot key is the hello-declared browser name
    } else {
        my_agent_id = state.agent_seq.fetch_add(1, Ordering::SeqCst) + 1;
        state.ws_agents.lock().unwrap().push((my_agent_id, tx.clone()));
        log("agent connected (ws)");
    }

    let sink = if role == "agent" { Sink::Ws(my_agent_id) } else { Sink::Stdio };

    if role == "extension" {
        // the FIRST frame must be hello: it decides which slot this socket owns
        let first = tokio::time::timeout(Duration::from_secs(5), read.next()).await;
        let Ok(Some(Ok(Message::Text(t)))) = first else {
            let _ = tx.send(close(1008, "expected hello within 5s"));
            return;
        };
        let msg: Value = serde_json::from_str(t.as_str()).unwrap_or(Value::Null);
        let mut conn = ExtConn {
            state: state.clone(),
            tx: tx.clone(),
            tick: tick.clone(),
            last_frame: last_frame.clone(),
            name: None,
            gen: 0,
        };
        conn.touch();
        if handle_extension_message(&mut conn, &msg) {
            return; // refused (duplicate live browser under that name)
        }
        // the rest of the socket's life
        while let Some(frame) = read.next().await {
            let msg = match frame {
                Ok(Message::Text(t)) => serde_json::from_str::<Value>(t.as_str()).unwrap_or(Value::Null),
                Ok(Message::Ping(p)) => {
                    let _ = tx.send(Message::Pong(p));
                    continue;
                }
                Ok(Message::Pong(_)) => {
                    conn.touch();
                    continue;
                }
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(_) => continue,
            };
            conn.touch();
            if handle_extension_message(&mut conn, &msg) {
                break; // refused mid-life (only possible at hello)
            }
        }
        if let Some(name) = &conn.name {
            drop_ext_named(&state, name, "gone", Some(conn.gen));
        }
        return;
    }

    while let Some(frame) = read.next().await {
        let msg = match frame {
            Ok(Message::Text(t)) => serde_json::from_str::<Value>(t.as_str()).unwrap_or(Value::Null),
            Ok(Message::Ping(p)) => {
                let _ = tx.send(Message::Pong(p));
                continue;
            }
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => continue,
        };
        handle_agent_message(&state, &sink, &msg);
    }

    // ---- teardown
    state.ws_agents.lock().unwrap().retain(|(i, _)| *i != my_agent_id);
    state
        .routes
        .lock()
        .unwrap()
        .retain(|_, r| !matches!(&r.sink, Sink::Ws(i) if *i == my_agent_id));
    log("agent disconnected (ws)");
}

async fn write_http(stream: &mut TcpStream, status: &str, body: &str, content_type: &str) -> std::io::Result<()> {
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(resp.as_bytes()).await
}

/// The pairing page a browser.launch instance opens first. It pushes the slot
/// name + daemon credentials into the extension (nonce single-use — this HTML
/// exists for exactly one fetch), retries while the service worker warms up,
/// and tells the human what happened.
fn pair_page(token: &str, port: u16, name: &str, nonce: &str, ext_id: &str) -> String {
    // the object literal itself (valid JSON = valid JS) — chrome.runtime
    // .sendMessage needs the message OBJECT; a JSON string would arrive with
    // no .type and be refused
    let payload = json!({
        "type": "CUA_PAIR_LAUNCH",
        "nonce": nonce,
        "token": token,
        "port": port,
        "name": name,
    })
    .to_string();
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>CUA pairing</title>
<style>body{{font:14px/1.6 system-ui;background:#1b1a20;color:#eee;padding:2rem}}code{{background:#333;border-radius:4px;padding:1px 6px}}</style></head>
<body><h2>CUA pairing…</h2><pre id="log" style="white-space:pre-wrap"></pre>
<script>
const EXT_ID = "{ext_id}";
const PAYLOAD = {payload};
const log = document.getElementById('log');
function out(s){{ log.textContent += s + "\n"; }}
let done = false;
function attempt() {{
  if (done) return;
  try {{
    chrome.runtime.sendMessage(EXT_ID, PAYLOAD, (r) => {{
      const err = chrome.runtime.lastError;
      if (err) {{ out("retrying… (" + err.message + ")"); return; }}
      done = true;
      if (r && r.ok) out("已配对为浏览器槽位 " + r.paired_as + " — 这个窗口可以关掉，也可以留给 agent 使用。");
      else out("扩展拒绝配对: " + JSON.stringify(r));
    }});
  }} catch (e) {{ out("retrying… (" + e + ")"); }}
}}
attempt(); setTimeout(attempt, 1500); setTimeout(attempt, 4000); setTimeout(attempt, 8000);
setTimeout(() => {{ if (!done) out("30s 内没有配上：确认这个浏览器实例加载了 CUA 扩展（--load-extension 生效），并且扩展是支持多浏览器的新构建。"); }}, 30000);
</script></body></html>"#
    )
}

pub async fn run_daemon(port: u16, token: String) {
    let state = Arc::new(AppState::new());
    // the daemon writes its own log (the detached process has no std handles)
    {
        let path = config::data_dir().join("browser-daemon.log");
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        {
            // the guard is scoped: daemon_log_line takes the same lock, and a
            // non-reentrant std Mutex held across that call deadlocks the
            // daemon before it ever binds (found by line instrumentation)
            let mut g = DAEMON_LOG.lock().unwrap();
            *g = std::fs::OpenOptions::new().create(true).append(true).open(&path).ok();
        }
        daemon_log_line(&format!("--- daemon start {} (port {port}) ---", crate::iso_now()));
    }
    // the durable audit trail: one thread drains every connected slot's ring
    auditlog::spawn(state.clone());
    let _ = DAEMON_PORT.set(port);
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap_or_else(|e| {
            log(&format!("server error {e}"));
            std::process::exit(1);
        });
    log(&format!(
        "listening on ws://127.0.0.1:{port}/ (token {}…)",
        &token[..token.len().min(4)]
    ));
    emit(&json!({"method": "event.daemon.ready", "params": {"port": port}}).to_string());

    // half-open watchdog: probe every silent slot and drop the dead ones
    {
        let state = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                let idle: Vec<String> = {
                    let exts = state.exts.lock().unwrap();
                    exts.iter()
                        .filter(|(_, e)| e.last_frame.lock().unwrap().elapsed() >= Duration::from_secs(45))
                        .map(|(k, _)| k.clone())
                        .collect()
                };
                for name in idle {
                    if probe_ext(&state, &name, 1200).await {
                        if let Some(e) = state.exts.lock().unwrap().get(&name) {
                            *e.last_frame.lock().unwrap() = Instant::now();
                        }
                    } else {
                        log("extension idle watchdog: dropping dead connection");
                        drop_ext_named(&state, &name, "dead", None);
                    }
                }
            }
        });
    }

    // stdio agent: newline-delimited JSON on stdin, replies/events on stdout
    {
        let state = state.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                let Ok(line) = line else { break };
                let line = line.trim();
                if line.is_empty() || line == "{" {
                    continue; // tolerate pretty-printed first brace, like the node daemon
                }
                let Ok(msg) = serde_json::from_str::<Value>(line) else {
                    println!(
                        "{}",
                        json!({"id": null, "error": {"code": "BAD_JSON", "message": "stdin lines must be one JSON object per line"}})
                    );
                    continue;
                };
                handle_agent_message(&state, &Sink::Stdio, &msg);
            }
            log("stdin closed; daemon stays alive for WS agents");
        });
    }

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let state = state.clone();
                let token = token.clone();
                tokio::spawn(handle_connection(state, token, stream, addr));
            }
            Err(e) => log(&format!("accept error {e}")),
        }
    }
}

/// Blocking entry point for `cua browser serve`: builds a runtime, runs the
/// daemon, never returns.
pub fn serve(port: u16, token: String) -> ! {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|e| {
            eprintln!("browser: tokio runtime: {e}");
            std::process::exit(1);
        });
    rt.block_on(run_daemon(port, token));
    std::process::exit(0);
}

// ------------------------------------------------------------- native messaging

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
}

fn config_path() -> std::path::PathBuf {
    crate::config::path()
}

const NATIVE_HOST_NAME: &str = "com.cua.browser";

/// Native-messaging origin for an unpacked extension: the browser derives the
/// extension id from the directory path, so the manifest has to list exactly
/// that origin and the host has to accept exactly that id.
pub fn native_origin(ext_dir: &std::path::Path) -> String {
    format!(
        "chrome-extension://{}/",
        unpacked_ext_id(&ext_dir.display().to_string())
    )
}

/// Native messaging host: launched by the browser for our own extension.
/// Protocol: 4-byte LE length-prefixed JSON frames on stdio.
/// `{"op":"pair"}` → ensures the config token exists and the daemon is
/// running, then answers {"ok",token,port} so the extension can auto-pair.
pub async fn run_native_host(origin: &str, ext_dir: &std::path::Path, launch: &Launch) {
    if !origin_allowed(origin, ext_dir) {
        std::process::exit(2); // not our extension — refuse
    }
    use std::io::{Read, Write};
    let stdin = std::io::stdin();
    let mut raw = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    loop {
        let mut lenb = [0u8; 4];
        if raw.read_exact(&mut lenb).is_err() {
            return; // browser closed the port
        }
        let len = u32::from_le_bytes(lenb) as usize;
        if len == 0 || len > 1024 * 1024 {
            return;
        }
        let mut buf = vec![0u8; len];
        if raw.read_exact(&mut buf).is_err() {
            return;
        }
        let msg: Value = serde_json::from_slice(&buf).unwrap_or(Value::Null);
        if msg.get("op").and_then(|v| v.as_str()) != Some("pair") {
            let reply = json!({"ok": false, "error": "unknown op"});
            let _ = out.write_all(&(reply.to_string().len() as u32).to_le_bytes());
            let _ = out.write_all(reply.to_string().as_bytes());
            let _ = out.flush();
            continue;
        }
        // 1) ensure the config token exists (generate if fresh machine)
        let mut cfg = read_config().as_object().cloned().unwrap_or_default();
        let token = match cfg.get("token").and_then(|v| v.as_str()) {
            Some(t) if t.len() >= 32 => t.to_string(),
            _ => {
                let Some(t) = crypto_token() else {
                    let reply = json!({"ok": false, "error": "OS CSPRNG unavailable — refusing to generate a predictable token"});
                    let _ = out.write_all(&(reply.to_string().len() as u32).to_le_bytes());
                    let _ = out.write_all(reply.to_string().as_bytes());
                    let _ = out.flush();
                    continue;
                };
                cfg.insert("token".into(), json!(t));
                write_config(&cfg);
                t
            }
        };
        // 2) ensure the daemon is running with that token
        let port = cfg
            .get("port")
            .and_then(|v| v.as_u64())
            .map(|v| v as u16)
            .unwrap_or(8765);
        let mut started = false;
        if health(port, 400).await.is_none() {
            launch.spawn(port, &token);
            started = wait_daemon(port).await;
        }
        let reply = json!({"ok": true, "token": token, "port": port, "daemon_started": started});
        let _ = out.write_all(&(reply.to_string().len() as u32).to_le_bytes());
        let _ = out.write_all(reply.to_string().as_bytes());
        let _ = out.flush();
    }
}

/// Blocking entry point for the native-messaging host mode.
pub fn native_host(origin: &str, ext_dir: &std::path::Path, launch: &Launch) -> ! {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|e| {
            eprintln!("browser: tokio runtime: {e}");
            std::process::exit(1);
        });
    rt.block_on(run_native_host(origin, ext_dir, launch));
    std::process::exit(0);
}

/// 64 hex chars from the OS CSPRNG. None means the platform RNG failed — the
/// caller must refuse pairing rather than fall back to a predictable token.
fn crypto_token() -> Option<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).ok()?;
    let mut t = String::with_capacity(64);
    for b in bytes {
        t.push_str(&format!("{b:02x}"));
    }
    Some(t)
}

/// Whether a native-messaging origin is one of ours. `ext_dir` covers the
/// single-copy case; when `install` registered several copies (installed +
/// checkout), the manifest's own list is the authority — an unpacked
/// extension's id is a hash of *its* directory, so every loadable copy needs
/// its origin listed or the browser refuses the connect before we run.
fn origin_allowed(origin: &str, ext_dir: &std::path::Path) -> bool {
    if origin == native_origin(ext_dir) {
        return true;
    }
    let manifest = config_path()
        .parent()
        .map(|d| d.join(format!("{NATIVE_HOST_NAME}.json")));
    if let Some(p) = manifest {
        if let Ok(bytes) = std::fs::read(&p) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(list) = v.get("allowed_origins").and_then(|x| x.as_array()) {
                    return list.iter().any(|o| o.as_str() == Some(origin));
                }
            }
        }
    }
    false
}

/// Part of `cua browser install`: register the native messaging host so the
/// extension auto-pairs without copying the token. Every copy the user might
/// load gets its origin listed (`ext_dirs`); Edge and Chrome keep separate
/// registry hives.
pub fn register_native_host(ext_dirs: &[std::path::PathBuf]) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let manifest_dir = config_path()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    std::fs::create_dir_all(&manifest_dir).map_err(|e| format!("mkdir: {e}"))?;
    let manifest_path = manifest_dir.join(format!("{NATIVE_HOST_NAME}.json"));
    let mut origins: Vec<String> = Vec::new();
    for d in ext_dirs {
        let o = native_origin(d);
        if !origins.contains(&o) {
            origins.push(o);
        }
    }
    let manifest = json!({
        "name": NATIVE_HOST_NAME,
        "description": "cua browser native bridge (token auto-pairing)",
        "path": exe.display().to_string(),
        "type": "stdio",
        "allowed_origins": origins,
    });
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap_or_default(),
    )
    .map_err(|e| format!("write manifest: {e}"))?;
    let manifest_str = manifest_path.display().to_string();
    // Chrome's key has NO Microsoft component; sharing a format string with
    // Edge silently registered Chrome under the wrong hive.
    for key in [
        format!(r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts\{NATIVE_HOST_NAME}"),
        format!(r"HKCU\Software\Google\Chrome\NativeMessagingHosts\{NATIVE_HOST_NAME}"),
    ] {
        let st = std::process::Command::new("reg")
            .args(["add", &key, "/ve", "/d", &manifest_str, "/f"])
            .output()
            .map_err(|e| format!("reg add: {e}"))?;
        if !st.status.success() {
            return Err(format!(
                "reg add {key} failed: {}",
                String::from_utf8_lossy(&st.stderr)
            ));
        }
    }
    Ok(manifest_str)
}

pub fn read_config() -> Value {
    crate::config::read()
}

pub fn write_config(map: &serde_json::Map<String, Value>) {
    crate::config::write(map)
}

pub async fn health(port: u16, ms: u64) -> Option<Value> {    let fut = async {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.ok()?;
        let req = format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
        s.write_all(req.as_bytes()).await.ok()?;
        let mut buf = Vec::new();
        // a process-exit teardown can RST after the payload; keep what we got
        let _ = s.read_to_end(&mut buf).await;
        let text = String::from_utf8_lossy(&buf);
        let body = text.split("\r\n\r\n").nth(1)?;
        // the node daemon answers with chunked encoding; slice out the JSON
        let (a, b) = (body.find('{')?, body.rfind('}')? + 1);
        serde_json::from_str(&body[a..b]).ok()
    };
    tokio::time::timeout(Duration::from_millis(ms), fut).await.ok()?
}



/// How to relaunch ourselves as the daemon. `cua` supplies
/// `Launch { exe: cua.exe, subcommand: ["browser", "serve"] }`, so the bridge
/// never has to know the binary name or where it lives.

/// Spawn a fully detached child with NO handle inheritance — the generic form
/// of `Launch::spawn` (why FALSE matters is documented there). Used by the
/// pointer-preview host as well: the first preview packet must not wedge the
/// agent's pipe the way the daemon once did.
pub fn spawn_detached_process(exe: &str, args: &str) -> bool {
    #[cfg(windows)]
    {
        spawn_detached_windows(exe, args)
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new(exe)
            .args(args.split_whitespace())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null())
            .spawn()
            .is_ok()
    }
}

#[cfg(windows)]
fn spawn_detached_windows(exe: &str, args: &str) -> bool {
    use std::os::windows::ffi::OsStrExt as _;
    #[repr(C)]
    struct StartupInfoW {
        cb: u32,
        reserved: *mut core::ffi::c_void,
        desktop: *mut core::ffi::c_void,
        title: *mut core::ffi::c_void,
        x: u32,
        y: u32,
        x_size: u32,
        y_size: u32,
        x_count_chars: u32,
        y_count_chars: u32,
        fill_attribute: u32,
        flags: u32,
        show_window: u16,
        cb_reserved2: u16,
        reserved2: *mut u8,
        std_input: *mut core::ffi::c_void,
        std_output: *mut core::ffi::c_void,
        std_error: *mut core::ffi::c_void,
    }
    #[repr(C)]
    struct ProcessInfo {
        process: *mut core::ffi::c_void,
        thread: *mut core::ffi::c_void,
        process_id: u32,
        thread_id: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateProcessW(
            application_name: *const u16,
            command_line: *mut u16,
            process_attributes: *const core::ffi::c_void,
            thread_attributes: *const core::ffi::c_void,
            inherit_handles: i32,
            creation_flags: u32,
            environment: *const core::ffi::c_void,
            current_directory: *const u16,
            startup_info: *mut StartupInfoW,
            process_information: *mut ProcessInfo,
        ) -> i32;
        fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
    }
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    let mut cmd: Vec<u16> = format!("\"{}\" {}", exe, args)
        .encode_utf16()
        .collect();
    cmd.push(0);
    let mut si = StartupInfoW {
        cb: std::mem::size_of::<StartupInfoW>() as u32,
        reserved: std::ptr::null_mut(),
        desktop: std::ptr::null_mut(),
        title: std::ptr::null_mut(),
        x: 0,
        y: 0,
        x_size: 0,
        y_size: 0,
        x_count_chars: 0,
        y_count_chars: 0,
        fill_attribute: 0,
        flags: 0,
        show_window: 0,
        cb_reserved2: 0,
        reserved2: std::ptr::null_mut(),
        std_input: std::ptr::null_mut(),
        std_output: std::ptr::null_mut(),
        std_error: std::ptr::null_mut(),
    };
    let mut pi = ProcessInfo {
        process: std::ptr::null_mut(),
        thread: std::ptr::null_mut(),
        process_id: 0,
        thread_id: 0,
    };
    unsafe {
        if CreateProcessW(
            std::ptr::null(),
            cmd.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0, // bInheritHandles = FALSE
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
            std::ptr::null(),
            std::ptr::null(),
            &mut si,
            &mut pi,
        ) != 0
        {
            let _ = CloseHandle(pi.process);
            let _ = CloseHandle(pi.thread);
            true
        } else {
            false
        }
    }
}

/// How to relaunch ourselves as the daemon. `cua` supplies
/// `Launch { exe: cua.exe, subcommand: ["browser", "serve"] }`, so the bridge
/// never has to know the binary name or where it lives.
pub struct Launch {
    pub exe: std::path::PathBuf,
    pub subcommand: Vec<String>,
}

impl Launch {
    /// Start the daemon detached; the handle is dropped on purpose so it
    /// outlives the CLI process. CreateProcessW with `bInheritHandles=false` —
    /// `std::process::Command` always passes TRUE, so the child would duplicate
    /// the caller's stdout pipe and any harness reading that pipe (head, python,
    /// an IDE) would wait for EOF forever on the first cold start. The daemon
    /// writes its own log instead of inheriting std handles.
    pub fn spawn(&self, port: u16, token: &str) {
        let args = format!("browser serve --port {port} --token {token}");
        if !crate::spawn_detached_process(&self.exe.display().to_string(), &args) {
            eprintln!("warning: failed to spawn the detached daemon");
        }
    }
}


pub async fn wait_daemon(port: u16) -> bool {
    // a 100 MB exe can take several seconds to bind; 5 s gave up too early
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if health(port, 400).await.is_some() {
            return true;
        }
    }
    false
}

pub async fn http_shutdown(port: u16, token: &str) -> Result<String, ()> {
    let fut = async {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.ok()?;
        let req = format!("POST /shutdown?token={token} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        s.write_all(req.as_bytes()).await.ok()?;
        let mut buf = Vec::new();
        // a process-exit teardown can RST after the payload; keep what we got
        let _ = s.read_to_end(&mut buf).await;
        let text = String::from_utf8_lossy(&buf).to_string();
        if text.starts_with("HTTP/1.1 403") {
            return None; // shutdown refused — caller must not report "not running"
        }
        if !text.starts_with("HTTP/1.1 200") {
            return None;
        }
        let body = text.split("\r\n\r\n").nth(1)?;
        let (a, b) = (body.find('{')?, body.rfind('}')? + 1);
        Some(body[a..b].to_string())
    };
    match tokio::time::timeout(Duration::from_millis(2000), fut).await {
        Ok(Some(body)) => Ok(body),
        _ => Err(()),
    }
}


// ================================================================ tests =====
//
// Multi-browser regressions (BUILD 2026.11.01-multi-browser): slots keyed by
// hello-declared browser name, per-name duplicate probes, per-slot routing,
// and the browser.launch/pair plumbing. All offline — no TcpStream needed; the
// ExtConn used by hello_register is built over plain unbounded channels.

#[cfg(test)]
mod tests {
    use super::*;

    /// Register a slot exactly as hello_register does (fetch_add + insert).
    fn register_ext(state: &AppState, name: &str, tx: Tx) -> u64 {
        let gen = state.ext_seq.fetch_add(1, Ordering::SeqCst) + 1;
        state.exts.lock().unwrap().insert(
            name.to_string(),
            ExtState {
                out: tx,
                info: Mutex::new(Value::Null),
                last_frame: Arc::new(Mutex::new(Instant::now())),
                tick: Arc::new(AtomicU64::new(0)),
                gen,
                name: name.to_string(),
            },
        );
        gen
    }

    fn mark_workspace(state: &AppState, name: &str) {
        let exts = state.exts.lock().unwrap();
        if let Some(e) = exts.get(name) {
            *e.info.lock().unwrap() = json!({"workspace": true});
        }
    }

    /// Spawn a stand-in for the live extension's read loop: every frame the
    /// daemon sends to that socket bumps the slot's tick — what makes
    /// probe_ext() return true.
    fn answer_pings(state: &Arc<AppState>, name: &str, mut rx: mpsc::UnboundedReceiver<Message>) {
        let tick = {
            let exts = state.exts.lock().unwrap();
            exts.get(name).map(|e| e.tick.clone())
        };
        let tick = tick.expect("slot must exist");
        tokio::spawn(async move {
            while rx.recv().await.is_some() {
                tick.fetch_add(1, Ordering::SeqCst);
            }
        });
    }

    fn make_conn(state: &Arc<AppState>, tx: Tx) -> ExtConn {
        ExtConn {
            state: state.clone(),
            tx,
            tick: Arc::new(AtomicU64::new(0)),
            last_frame: Arc::new(Mutex::new(Instant::now())),
            name: None,
            gen: 0,
        }
    }

    fn hello_msg(name: &str) -> Value {
        json!({"id": "hello", "method": "hello", "params": {
            "min_compatible": "1.0", "build": "test", "browser_name": name, "instance_id": "i1"
        }})
    }

    // ------------------------------------------------ lifecycle semantics

    #[tokio::test]
    async fn late_teardown_of_old_gen_keeps_the_new_slots() {
        let state = AppState::new();
        let (tx_old, _rx_old) = mpsc::unbounded_channel::<Message>();
        let gen1 = register_ext(&state, "main", tx_old);
        drop_ext_named(&state, "main", "dead", None);
        let (tx_live, mut rx_live) = mpsc::unbounded_channel::<Message>();
        let gen2 = register_ext(&state, "main", tx_live);
        assert!(gen2 > gen1);
        let (tx_agent, mut rx_agent) = mpsc::unbounded_channel::<Message>();
        state.ws_agents.lock().unwrap().push((3, tx_agent));
        state.routes.lock().unwrap().insert(
            "x1".into(),
            Route { sink: Sink::Ws(3), agent_id: json!(9), browser: "main".into() },
        );

        // the OLD socket's teardown finally arrives, late
        drop_ext_named(&state, "main", "gone", Some(gen1));

        let exts = state.exts.lock().unwrap();
        assert!(exts.contains_key("main"), "late teardown of dead gen-{gen1} cleared the live gen-{gen2} slot");
        assert_eq!(exts.get("main").unwrap().gen, gen2);
        drop(exts);
        assert!(
            state.select_ext(Some("main")).is_ok(),
            "daemon now reports the browser offline while gen-{gen2} is live"
        );
        assert!(rx_live.try_recv().is_err(), "old teardown leaked a Close onto the live socket");
        assert!(state.routes.lock().unwrap().contains_key("x1"), "old teardown failed the live era's in-flight route");
        assert!(rx_agent.try_recv().is_err(), "phantom EXTENSION_DISCONNECTED sent");
    }

    #[tokio::test]
    async fn current_gen_teardown_really_clears_the_slot_and_its_routes_only() {
        let state = AppState::new();
        let (tx_ext, mut rx_ext) = mpsc::unbounded_channel::<Message>();
        let (tx_agent_a, mut rx_a) = mpsc::unbounded_channel::<Message>();
        let (tx_agent_b, mut rx_b) = mpsc::unbounded_channel::<Message>();
        state.ws_agents.lock().unwrap().push((3, tx_agent_a));
        state.ws_agents.lock().unwrap().push((4, tx_agent_b));
        state.routes.lock().unwrap().insert(
            "x1".into(),
            Route { sink: Sink::Ws(3), agent_id: json!(9), browser: "main".into() },
        );
        state.routes.lock().unwrap().insert(
            "x2".into(),
            Route { sink: Sink::Ws(4), agent_id: json!(10), browser: "other".into() },
        );
        let (tx_other, _rx_other) = mpsc::unbounded_channel::<Message>();
        register_ext(&state, "other", tx_other);
        let gen = register_ext(&state, "main", tx_ext);

        drop_ext_named(&state, "main", "gone", Some(gen));

        assert!(
            !state.exts.lock().unwrap().contains_key("main"),
            "real disconnect was swallowed — browser would stay fake-online"
        );
        assert!(state.exts.lock().unwrap().contains_key("other"), "unrelated slot was dropped");
        match rx_ext.try_recv() {
            Ok(Message::Close(Some(f))) => {
                assert_eq!(u16::from(f.code), 1000);
                assert_eq!(f.reason.as_ref(), "gone");
            }
            other => panic!("expected Close(1000, gone) on the leaving socket, got {other:?}"),
        }
        // only the orphaned browser's routes drain
        assert!(!state.routes.lock().unwrap().contains_key("x1"), "dropped browser's route must drain");
        assert!(state.routes.lock().unwrap().contains_key("x2"), "other browser's route must survive");
        match rx_a.try_recv() {
            Ok(Message::Text(t)) => assert!(t.as_str().contains("EXTENSION_DISCONNECTED"), "{t}"),
            other => panic!("orphaned agent must learn the browser went away, got {other:?}"),
        }
        assert!(rx_b.try_recv().is_err(), "unrelated agent was told the extension died");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hello_refused_while_a_live_incumbent_holds_the_name() {
        let state = Arc::new(AppState::new());
        let (tx_inc, rx_inc) = mpsc::unbounded_channel::<Message>();
        register_ext(&state, "main", tx_inc);
        answer_pings(&state, "main", rx_inc); // incumbent is alive

        let (tx_new, mut rx_new) = mpsc::unbounded_channel::<Message>();
        let mut conn = make_conn(&state, tx_new);
        let refused = handle_extension_message(&mut conn, &hello_msg("main"));
        assert!(refused, "hello under a live incumbent's name must be refused");
        assert!(conn.name.is_none(), "refused hello must not claim the slot");
        assert_eq!(state.exts.lock().unwrap().len(), 1, "the incumbent slot must survive a refused hello");
        match rx_new.try_recv() {
            Ok(Message::Text(t)) => {
                let v: Value = serde_json::from_str(t.as_str()).unwrap();
                assert_eq!(v["error"]["code"], "duplicate_browser_connection");
            }
            other => panic!("refused socket must get a structured error, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hello_replaces_a_dead_incumbent_and_claims_the_name() {
        let state = Arc::new(AppState::new());
        let (tx_dead, _rx_dead) = mpsc::unbounded_channel::<Message>();
        let gen_old = register_ext(&state, "main", tx_dead); // nobody answers probes

        let (tx_new, mut rx_new) = mpsc::unbounded_channel::<Message>();
        let mut conn = make_conn(&state, tx_new);
        let refused = handle_extension_message(&mut conn, &hello_msg("main"));
        assert!(!refused, "a dead incumbent must be replaced, not refuse the newcomer");
        assert_eq!(conn.name.as_deref(), Some("main"));
        let exts = state.exts.lock().unwrap();
        assert_eq!(exts.get("main").unwrap().gen, conn.gen);
        assert!(conn.gen > gen_old);
        drop(exts);
        match rx_new.try_recv() {
            Ok(Message::Text(t)) => {
                let v: Value = serde_json::from_str(t.as_str()).unwrap();
                assert_eq!(v["id"], "hello");
                assert!(v.get("result").is_some());
            }
            other => panic!("accepted hello must be answered with a result, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn distinct_names_coexist_and_anonymous_hello_lands_on_default() {
        let state = Arc::new(AppState::new());
        let (tx_a, mut rx_a) = mpsc::unbounded_channel::<Message>();
        let (tx_b, _rx_b) = mpsc::unbounded_channel::<Message>();
        let mut conn_a = make_conn(&state, tx_a);
        let mut conn_b = make_conn(&state, tx_b);
        assert!(!handle_extension_message(&mut conn_a, &hello_msg("work")));
        // no browser_name at all (old build) → "default"
        let mut old_hello = hello_msg("");
        old_hello["params"].as_object_mut().unwrap().remove("browser_name");
        assert!(!handle_extension_message(&mut conn_b, &old_hello));
        assert_eq!(conn_a.name.as_deref(), Some("work"));
        assert_eq!(conn_b.name.as_deref(), Some("default"));
        assert_eq!(state.browser_names(), vec!["default".to_string(), "work".to_string()]);
        // each slot gets its own traffic
        match rx_a.try_recv() {
            Ok(Message::Text(t)) => assert!(t.contains("\"id\":\"hello\"")),
            other => panic!("work's hello reply missing: {other:?}"),
        }
    }

    // ------------------------------------------------ agent routing

    #[tokio::test]
    async fn routing_single_slot_back_compat_multi_slots_require_a_name() {
        let state = Arc::new(AppState::new());
        // legacy mode for THIS suite: both automations off
        *state.config_override.lock().unwrap() = Some(json!({"auto_workspace": false, "auto_open_user_browser": false}));
        // zero slots → EXTENSION_NOT_CONNECTED
        let (tx_agent, mut rx_agent) = mpsc::unbounded_channel::<Message>();
        state.ws_agents.lock().unwrap().push((1, tx_agent));
        handle_agent_message(&state, &Sink::Ws(1), &json!({"id": 1, "method": "tool.snapshot", "params": {}}));
        match rx_agent.try_recv() {
            Ok(Message::Text(t)) => assert!(t.contains("EXTENSION_NOT_CONNECTED"), "{t}"),
            other => panic!("empty state must refuse with EXTENSION_NOT_CONNECTED, got {other:?}"),
        }

        // exactly one slot → anonymous requests route there
        let (tx_main, mut rx_main) = mpsc::unbounded_channel::<Message>();
        register_ext(&state, "main", tx_main);
        handle_agent_message(&state, &Sink::Ws(1), &json!({"id": 2, "method": "tool.snapshot", "params": {}}));
        match rx_main.try_recv() {
            Ok(Message::Text(t)) => {
                let v: Value = serde_json::from_str(t.as_str()).unwrap();
                assert_eq!(v["method"], "tool.snapshot");
            }
            other => panic!("single slot must receive the request, got {other:?}"),
        }

        // a second slot appears → anonymous requests are refused, named ones route
        let (tx_work, mut rx_work) = mpsc::unbounded_channel::<Message>();
        register_ext(&state, "work", tx_work);
        handle_agent_message(&state, &Sink::Ws(1), &json!({"id": 3, "method": "tool.snapshot", "params": {}}));
        match rx_agent.try_recv() {
            Ok(Message::Text(t)) => assert!(t.contains("BROWSER_REQUIRED"), "{t}"),
            other => panic!("two slots + no browser must refuse, got {other:?}"),
        }
        handle_agent_message(
            &state,
            &Sink::Ws(1),
            &json!({"id": 4, "method": "tool.snapshot", "params": {}, "browser": "work"}),
        );
        match rx_work.try_recv() {
            Ok(Message::Text(t)) => assert!(t.contains("tool.snapshot"), "{t}"),
            other => panic!("named slot must receive its request, got {other:?}"),
        }
        // unknown name → honest refusal listing what exists
        handle_agent_message(
            &state,
            &Sink::Ws(1),
            &json!({"id": 5, "method": "tool.snapshot", "params": {}, "browser": "nope"}),
        );
        match rx_agent.try_recv() {
            Ok(Message::Text(t)) => {
                assert!(t.contains("BROWSER_NOT_FOUND"), "{t}");
                assert!(t.contains("work") && t.contains("main"), "refusal must name the connected slots: {t}");
            }
            other => panic!("unknown browser must refuse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn nameless_calls_target_the_agent_workspace_not_the_users_browser() {
        let state = Arc::new(AppState::new());
        *state.config_override.lock().unwrap() = Some(json!({"agent_browser": "agent", "auto_workspace": true}));
        let (tx_ws, _rx_ws) = mpsc::unbounded_channel::<Message>();
        let (tx_agent, mut rx_agent) = mpsc::unbounded_channel::<Message>();
        let (tx_main, mut rx_main) = mpsc::unbounded_channel::<Message>();
        let (tx_work, mut rx_work) = mpsc::unbounded_channel::<Message>();
        state.ws_agents.lock().unwrap().push((1, tx_ws));
        register_ext(&state, "main", tx_main); // the user's own browser
        register_ext(&state, "work", tx_work);
        register_ext(&state, "agent", tx_agent); // the workspace

        // a nameless call lands in the workspace even with three slots live
        handle_agent_message(&state, &Sink::Ws(1), &json!({"id": 1, "method": "tool.snapshot", "params": {}}));
        match rx_agent.try_recv() {
            Ok(Message::Text(t)) => {
                let v: Value = serde_json::from_str(t.as_str()).unwrap();
                assert_eq!(v["method"], "tool.snapshot");
            }
            other => panic!("the workspace slot must answer nameless calls, got {other:?}"),
        }
        assert!(rx_main.try_recv().is_err(), "the user's browser must never receive a nameless call");
        assert!(rx_work.try_recv().is_err());

        // an explicit hint still wins over the workspace default
        handle_agent_message(&state, &Sink::Ws(1), &json!({"id": 2, "method": "tool.snapshot", "params": {}, "browser": "work"}));
        assert!(rx_work.try_recv().is_ok());
        assert!(rx_agent.try_recv().is_err(), "an explicit hint must not touch the workspace");
    }

    #[tokio::test]
    async fn nameless_calls_prefer_the_real_browser_over_an_isolated_workspace() {
        let state = Arc::new(AppState::new());
        *state.config_override.lock().unwrap() = Some(json!({
            "auto_workspace": false, "auto_open_user_browser": true, "user_browser": "default",
        }));
        let (tx_ws, _rx_ws) = mpsc::unbounded_channel::<Message>();
        let (tx_default, mut rx_default) = mpsc::unbounded_channel::<Message>();
        let (tx_agent, mut rx_agent) = mpsc::unbounded_channel::<Message>();
        state.ws_agents.lock().unwrap().push((1, tx_ws));
        register_ext(&state, "default", tx_default);
        register_ext(&state, "agentx", tx_agent);
        mark_workspace(&state, "agentx");

        handle_agent_message(&state, &Sink::Ws(1), &json!({"id": 1, "method": "tool.snapshot", "params": {}}));
        match rx_default.try_recv() {
            Ok(Message::Text(t)) => {
                let v: Value = serde_json::from_str(t.as_str()).unwrap();
                assert_eq!(v["method"], "tool.snapshot");
            }
            other => panic!("the real browser must answer nameless calls, got {other:?}"),
        }
        assert!(rx_agent.try_recv().is_err(), "the isolated workspace must not receive the default call");
    }

    #[tokio::test]
    async fn missing_real_browser_launch_failure_never_falls_back_to_a_workspace() {
        let state = Arc::new(AppState::new());
        *state.config_override.lock().unwrap() = Some(json!({
            "auto_workspace": false, "auto_open_user_browser": true, "user_browser": "default",
            "user_browser_exe": "Z:/definitely/not/here.exe", "user_browser_wait_ms": 500,
        }));
        let (tx_agent, mut rx_agent) = mpsc::unbounded_channel::<Message>();
        let (tx_sink, mut rx_sink) = mpsc::unbounded_channel::<Message>();
        state.ws_agents.lock().unwrap().push((1, tx_sink));
        register_ext(&state, "agentx", tx_agent);
        mark_workspace(&state, "agentx");

        handle_agent_message(&state, &Sink::Ws(1), &json!({"id": 1, "method": "tool.snapshot", "params": {}}));
        let mut got: Option<Value> = None;
        for _ in 0..150 {
            if let Ok(Message::Text(t)) = rx_sink.try_recv() {
                got = serde_json::from_str(t.as_str()).ok();
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let v = got.expect("the launch task must answer this request");
        assert_eq!(v["error"]["code"], "BROWSER_EXE_NOT_FOUND", "{v}");
        assert!(rx_agent.try_recv().is_err(), "the isolated workspace must not inherit the request");
    }

    #[tokio::test]
    async fn browser_close_does_not_auto_open_a_browser() {
        let state = Arc::new(AppState::new());
        *state.config_override.lock().unwrap() = Some(json!({
            "auto_open_user_browser": true, "user_browser": "default",
            "user_browser_exe": "Z:/definitely/not/here.exe",
        }));
        let (tx_agent, mut rx_agent) = mpsc::unbounded_channel::<Message>();
        state.ws_agents.lock().unwrap().push((1, tx_agent));
        handle_agent_message(
            &state,
            &Sink::Ws(1),
            &json!({"id": 1, "method": "browser.close", "params": {"confirm": true}, "browser": "default"}),
        );
        match rx_agent.try_recv() {
            Ok(Message::Text(t)) => {
                assert!(t.contains("BROWSER_NOT_FOUND"), "browser.close must be routed, not launch a browser: {t}");
                assert!(t.contains("not connected") || t.contains("no browser slot"), "{t}");
            }
            other => panic!("browser.close must answer without launching, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn missing_workspace_launches_and_never_falls_back_to_user_slots() {
        let state = Arc::new(AppState::new());
        // a nonexistent exe keeps this offline: the launch machinery fails
        // BEFORE any process starts, and the user's connected slot must not
        // quietly inherit the request
        *state.config_override.lock().unwrap() = Some(json!({
            "agent_browser": "agent", "auto_workspace": true,
            "browser_exe": "Z:/definitely/not/here.exe", "workspace_wait_ms": 500,
        }));
        let (tx_agent, mut rx_agent) = mpsc::unbounded_channel::<Message>();
        let (tx_main, mut rx_main) = mpsc::unbounded_channel::<Message>();
        state.ws_agents.lock().unwrap().push((1, tx_agent));
        register_ext(&state, "main", tx_main); // the user's browser, connected and tempting

        handle_agent_message(&state, &Sink::Ws(1), &json!({"id": 1, "method": "tool.snapshot", "params": {}}));
        // the reply comes from the launch task — poll briefly for it
        let mut got: Option<Value> = None;
        for _ in 0..150 {
            if let Ok(Message::Text(t)) = rx_agent.try_recv() {
                got = serde_json::from_str(t.as_str()).ok();
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let v = got.expect("the launch task must answer this request");
        assert_eq!(v["error"]["code"], "BROWSER_EXE_NOT_FOUND", "{v}");
        assert!(
            rx_main.try_recv().is_err(),
            "a failed workspace launch must never fall back to the user's browser"
        );
    }

    #[tokio::test]
    async fn status_names_the_workspace_routing_target() {
        let state = Arc::new(AppState::new());
        *state.config_override.lock().unwrap() = Some(json!({"agent_browser": "ops", "auto_workspace": true}));
        let (tx_agent, mut rx_agent) = mpsc::unbounded_channel::<Message>();
        state.ws_agents.lock().unwrap().push((1, tx_agent));
        assert!(daemon_method(&state, &Sink::Ws(1), &json!({"id": 1, "method": "status"}), "status"));
        match rx_agent.try_recv() {
            Ok(Message::Text(t)) => {
                let v: Value = serde_json::from_str(t.as_str()).unwrap();
                assert_eq!(v["result"]["agent_slot"], "ops");
                assert_eq!(v["result"]["auto_workspace"], true);
                assert_eq!(v["result"]["user_browser"], "default");
                assert_eq!(v["result"]["auto_open_user_browser"], true);
            }
            other => panic!("status must answer inline, got {other:?}"),
        }
    }

    /// diag.browsers answers inline with the whole family matrix (the scan is
    /// read-only: tasklist + reg query + Preferences reads on this machine).
    #[tokio::test]
    async fn diag_browsers_answers_with_the_family_matrix() {
        let state = Arc::new(AppState::new());
        let (tx_agent, mut rx_agent) = mpsc::unbounded_channel::<Message>();
        state.ws_agents.lock().unwrap().push((1, tx_agent));
        assert!(daemon_method(&state, &Sink::Ws(1), &json!({"id": 9, "method": "diag.browsers"}), "diag.browsers"));
        for _ in 0..250 {
            if let Ok(Message::Text(t)) = rx_agent.try_recv() {
                let v: Value = serde_json::from_str(t.as_str()).unwrap();
                assert_eq!(v["id"], 9, "{v}");
                let fams: Vec<&str> = v["result"]["browsers"]
                    .as_array()
                    .expect("browsers must be an array")
                    .iter()
                    .map(|b| b["family"].as_str().expect("every browser names its family"))
                    .collect();
                assert_eq!(fams, vec!["edge", "chrome", "brave", "opera", "vivaldi"], "{v}");
                assert_eq!(v["result"]["daemon"]["port_ok"], json!(true), "{v}");
                assert!(v["result"]["daemon"]["extension_connected"] == json!(false), "{v}");
                assert!(v["result"]["slots"].is_array(), "{v}");
                for b in v["result"]["browsers"].as_array().unwrap() {
                    assert!(b["extension"].is_object() && b["nm_manifest"].is_object(), "{b}");
                    assert!(b["extension"]["profiles"].is_array(), "{b}");
                }
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("diag.browsers never answered");
    }

    #[tokio::test]
    async fn params_browser_is_stripped_and_envelope_browser_wins() {
        let state = Arc::new(AppState::new());
        let (tx_a, mut rx_a) = mpsc::unbounded_channel::<Message>();
        let (tx_b, mut rx_b) = mpsc::unbounded_channel::<Message>();
        register_ext(&state, "a", tx_a);
        register_ext(&state, "b", tx_b);
        // params.browser routes AND is stripped from the forwarded params
        handle_agent_message(
            &state,
            &Sink::Stdio,
            &json!({"id": 1, "method": "tool.tab.create", "params": {"browser": "b", "url": "example.com"}}),
        );
        match rx_b.try_recv() {
            Ok(Message::Text(t)) => {
                let v: Value = serde_json::from_str(t.as_str()).unwrap();
                assert!(v["params"].get("browser").is_none(), "params.browser must not reach the tool: {v}");
                assert_eq!(v["params"]["url"], "example.com");
            }
            other => panic!("params.browser routing failed: {other:?}"),
        }
        assert!(rx_a.try_recv().is_err(), "a must not receive b's call");
        // envelope browser beats params.browser
        handle_agent_message(
            &state,
            &Sink::Stdio,
            &json!({"id": 2, "method": "tool.snapshot", "params": {"browser": "a"}, "browser": "b"}),
        );
        assert!(rx_b.try_recv().is_ok());
        assert!(rx_a.try_recv().is_err());
    }

    #[tokio::test]
    async fn cancel_reaches_only_the_browser_the_route_used() {
        let state = Arc::new(AppState::new());
        let (tx_a, mut rx_a) = mpsc::unbounded_channel::<Message>();
        let (tx_b, mut rx_b) = mpsc::unbounded_channel::<Message>();
        register_ext(&state, "a", tx_a);
        register_ext(&state, "b", tx_b);
        state.routes.lock().unwrap().insert(
            "x7".into(),
            Route { sink: Sink::Ws(1), agent_id: json!(9), browser: "b".into() },
        );
        handle_agent_message(&state, &Sink::Ws(1), &json!({"method": "cancel", "params": {"id": 9}}));
        match rx_b.try_recv() {
            Ok(Message::Text(t)) => {
                let v: Value = serde_json::from_str(t.as_str()).unwrap();
                assert_eq!(v["params"]["id"], "x7");
            }
            other => panic!("cancel must reach the slot the route used, got {other:?}"),
        }
        assert!(rx_a.try_recv().is_err(), "cancel leaked to the wrong browser");
    }

    // ------------------------------------------------ daemon-side methods

    #[tokio::test]
    async fn status_and_meta_browsers_answer_without_touching_slots() {
        let state = Arc::new(AppState::new());
        let (tx_agent, mut rx_agent) = mpsc::unbounded_channel::<Message>();
        state.ws_agents.lock().unwrap().push((1, tx_agent));
        let (tx_ext, _rx_ext) = mpsc::unbounded_channel::<Message>();
        register_ext(&state, "main", tx_ext);
        handle_agent_message(&state, &Sink::Ws(1), &json!({"id": 1, "method": "meta.browsers"}));
        match rx_agent.try_recv() {
            Ok(Message::Text(t)) => {
                let v: Value = serde_json::from_str(t.as_str()).unwrap();
                assert_eq!(v["result"]["browsers"][0]["name"], "main");
            }
            other => panic!("meta.browsers must answer inline, got {other:?}"),
        }
        handle_agent_message(&state, &Sink::Ws(1), &json!({"id": 2, "method": "status"}));
        match rx_agent.try_recv() {
            Ok(Message::Text(t)) => {
                let v: Value = serde_json::from_str(t.as_str()).unwrap();
                assert_eq!(v["result"]["extension_connected"], true);
                assert_eq!(v["result"]["browsers"][0]["name"], "main");
            }
            other => panic!("status must answer inline, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn pair_register_mints_and_validates() {
        let state = Arc::new(AppState::new());
        let (tx_agent, mut rx_agent) = mpsc::unbounded_channel::<Message>();
        state.ws_agents.lock().unwrap().push((1, tx_agent));
        handle_agent_message(&state, &Sink::Ws(1), &json!({"id": 1, "method": "pair.register", "params": {"name": "BAD NAME"}}));
        match rx_agent.try_recv() {
            Ok(Message::Text(t)) => assert!(t.contains("BAD_PARAMS"), "{t}"),
            other => panic!("bad name must refuse, got {other:?}"),
        }
        handle_agent_message(&state, &Sink::Ws(1), &json!({"id": 2, "method": "pair.register", "params": {"name": "scratch"}}));
        let nonce = match rx_agent.try_recv() {
            Ok(Message::Text(t)) => {
                let v: Value = serde_json::from_str(t.as_str()).unwrap();
                v["result"]["nonce"].as_str().expect("nonce").to_string()
            }
            other => panic!("pair.register must mint a nonce, got {other:?}"),
        };
        assert_eq!(nonce.len(), 64);
        assert!(
            state.nonces.lock().unwrap().contains_key(&nonce),
            "the minted nonce must be registered for the pair page"
        );
    }

    #[test]
    fn unpacked_ext_id_shape_and_determinism() {
        let a = unpacked_ext_id(r"D:\Codebase\cua");
        let b = unpacked_ext_id(r"D:\Codebase\cua");
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| ('a'..='p').contains(&c)), "chrome id alphabet is a-p, got {a}");
        assert_eq!(a, b, "the id must be a pure function of the path");
        assert_ne!(a, unpacked_ext_id(r"D:\Codebase\cua2"), "different paths must not collide");
        // Chromium derives an unpacked extension's id from sha256 over the RAW
        // UTF-16LE bytes of the path, each nibble mapped to a-p. Verified against
        // a live Edge 156 registration (the UTF-8 hash never matched, which is
        // why the pair page could not reach the extension). Recomputed here so
        // the contract stays pinned to the algorithm, not to one directory.
        if cfg!(windows) {
            use sha2::Digest as _;
            let mut h = Sha256::new();
            for u in r"D:\Codebase\cua".encode_utf16() {
                h.update(u.to_le_bytes());
            }
            let want: String = h
                .finalize()
                .iter()
                .take(16)
                .flat_map(|x| [x >> 4, x & 0x0f])
                .map(|n| char::from(b'a' + n))
                .collect();
            assert_eq!(a, want, "id must be the sha256(utf16le(path))→a-p value Edge assigns");
        }
    }

    #[test]
    fn valid_browser_name_gate() {
        assert!(valid_browser_name("default"));
        assert!(valid_browser_name("work_2"));
        assert!(!valid_browser_name(""));
        assert!(!valid_browser_name("Main"));
        assert!(!valid_browser_name("has space"));
        assert!(!valid_browser_name(&"x".repeat(33)));
    }

    #[test]
    fn csprng_token_shape_and_uniqueness() {
        let a = crypto_token().expect("OS CSPRNG must work on the test host");
        let b = crypto_token().expect("OS CSPRNG must work on the test host");
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "token must be 64 hex chars");
        assert_ne!(a, b, "two CSPRNG draws collided");
    }
}
