//! In-process caller for the daemon.
//!
//! `cua browser` used to shell out to a helper binary for every call: resolve an
//! executable, scrub inherited env, spawn 1.5 MB of static binary, hand-shake a
//! WebSocket, print JSON to stdout and parse it back. All of that existed only
//! because the client and the daemon lived in different processes. Linked in,
//! the call is one socket and one frame — and the daemon's structured error
//! comes back as a value instead of a line of stderr.

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::protocol::Message;

#[derive(Debug)]
pub enum CallError {
    /// Nothing is listening: the daemon has to be started.
    NoDaemon,
    /// Connected, but the call outlived its deadline.
    Timeout(u64),
    /// The daemon refused the token (close code 1008).
    Rejected,
    /// Transport dropped mid-call.
    Closed(String),
    /// The daemon answered with an `error` object.
    Failed(Value),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::NoDaemon => write!(f, "no daemon listening"),
            CallError::Timeout(ms) => write!(f, "timed out after {ms} ms"),
            CallError::Rejected => write!(
                f,
                "the daemon rejected the token (delete {} or re-pair)",
                crate::config::path().display()
            ),
            CallError::Closed(e) => write!(f, "connection closed ({e})"),
            CallError::Failed(v) => match (
                v.get("code").and_then(|c| c.as_str()),
                v.get("message").and_then(|m| m.as_str()),
            ) {
                (Some(c), Some(m)) => write!(f, "{c}: {m}"),
                _ => write!(f, "{v}"),
            },
        }
    }
}

impl std::error::Error for CallError {}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime")
}

async fn health_async(port: u16, ms: u64) -> Option<Value> {
    crate::health(port, ms).await
}

/// One `/health` probe, blocking.
pub fn health(port: u16, ms: u64) -> Option<Value> {
    rt().block_on(health_async(port, ms))
}

/// Whether the daemon on `port` was started with `token`. `None` means nothing
/// answered (or the daemon is too old to publish a token id).
pub fn token_matches(port: u16, token: &str) -> Option<bool> {
    let h = health(port, 600)?;
    let id = h.get("token_id").and_then(|v| v.as_str())?;
    Some(id == crate::token_id(token))
}

/// One JSON-RPC round trip on a fresh connection, blocking.
///
/// `browser` is the multi-slot routing hint: `None` means "the only connected
/// slot" (or a `BROWSER_REQUIRED` refusal when several are connected).
pub fn call(
    port: u16,
    token: &str,
    method: &str,
    params: &Value,
    browser: Option<&str>,
    timeout_ms: u64,
) -> Result<Value, CallError> {
    rt().block_on(rpc(port, token, method, params, browser, timeout_ms))
}

async fn rpc(
    port: u16,
    token: &str,
    method: &str,
    params: &Value,
    browser: Option<&str>,
    timeout_ms: u64,
) -> Result<Value, CallError> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::header::{HeaderName, HeaderValue};

    let url = format!("ws://127.0.0.1:{port}/");
    let mut req = url
        .into_client_request()
        .map_err(|_| CallError::NoDaemon)?;
    req.headers_mut().insert(
        HeaderName::from_static("sec-websocket-protocol"),
        HeaderValue::from_str(&format!("cua-agent.{token}"))
            .map_err(|_| CallError::Rejected)?,
    );
    let (ws, _resp) = tokio_tungstenite::connect_async(req)
        .await
        .map_err(|e| match e {
            tokio_tungstenite::tungstenite::Error::Io(io)
                if io.kind() == std::io::ErrorKind::ConnectionRefused =>
            {
                CallError::NoDaemon
            }
            other => CallError::Closed(other.to_string()),
        })?;
    let (mut write, mut read) = ws.split();
    let envelope = match browser {
        Some(b) => json!({"id": 1, "method": method, "params": params, "browser": b}),
        None => json!({"id": 1, "method": method, "params": params}),
    };
    write
        .send(Message::Text(envelope.to_string().into()))
        .await
        .map_err(|e| CallError::Closed(e.to_string()))?;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let remain = deadline.saturating_duration_since(Instant::now());
        if remain.is_zero() {
            return Err(CallError::Timeout(timeout_ms));
        }
        match tokio::time::timeout(remain, read.next()).await {
            Err(_) => return Err(CallError::Timeout(timeout_ms)),
            Ok(None) => return Err(CallError::Closed("eof".into())),
            Ok(Some(Ok(Message::Text(t)))) => {
                if let Ok(m) = serde_json::from_str::<Value>(t.as_str()) {
                    if m.get("id").and_then(|v| v.as_i64()) == Some(1) {
                        return match m.get("error") {
                            Some(e) => Err(CallError::Failed(e.clone())),
                            None => Ok(m),
                        };
                    }
                }
            }
            Ok(Some(Ok(Message::Close(Some(cf))))) => {
                let code = u16::from(cf.code);
                return Err(if code == 1008 {
                    CallError::Rejected
                } else {
                    CallError::Closed(format!("close {code}"))
                });
            }
            Ok(Some(Ok(Message::Close(None)))) => return Err(CallError::Closed("close".into())),
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => return Err(CallError::Closed(e.to_string())),
        }
    }
}

/// Ask the daemon to exit; `Err` means it was not running.
pub fn stop(port: u16, token: &str) -> Result<String, ()> {
    rt().block_on(crate::http_shutdown(port, token))
}

/// Start the daemon and wait for `/health`, blocking.
pub fn ensure(port: u16, launch: &crate::Launch) -> Result<(), CallError> {
    rt().block_on(async {
        if health_async(port, 400).await.is_some() {
            return Ok(());
        }
        if crate::config::read().get("no_autostart").and_then(|v| v.as_bool()) == Some(true) {
            return Err(CallError::NoDaemon);
        }
        let (token, _) = crate::config::token_and_port().map_err(|e| CallError::Closed(e))?;
        launch.spawn(port, &token);
        if crate::wait_daemon(port).await {
            Ok(())
        } else {
            Err(CallError::NoDaemon)
        }
    })
}
