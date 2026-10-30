//! Browser control is delegated to UBB (Universal Browser Bridge), which drives the
//! user's own logged-in Edge/Chrome through an extension + local daemon. cua only
//! resolves the `ubb` CLI, forwards the call, and wraps the answer in a receipt.

use anyhow::Result;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::util::{self, Receipt};

pub const USAGE: &str = r#"cua browser <ubb-tool> [json-params]      # forwarded to the ubb CLI

Core loop:
  cua browser session.start
  cua browser tab.create '{"url":"example.com"}'
  cua browser snapshot                      # accessibility tree with @eN refs
  cua browser click '{"ref":"@e3"}'
  cua browser wait_for '{"condition":"text_present","text":"Welcome"}'
  cua browser screenshot --out shot.png
  cua browser session.stop                  # always end with this

Also: status · list (full tool catalog) · find · read_page · get_html · fill · press_key ·
scroll · tab.list/select/close · console.list · network.list · extract · read_site
Refs come only from the newest snapshot; navigation invalidates them.
Safety (enforced by the extension): password/OTP/payment fields are never auto-filled,
destructive labels and restricted URLs are refused, and the user can interrupt at any time."#;

pub enum Cli {
    Exe(PathBuf),
    Node(PathBuf),
}

impl Cli {
    pub fn label(&self) -> String {
        match self {
            Cli::Exe(p) => p.display().to_string(),
            Cli::Node(p) => format!("node {}", p.display()),
        }
    }
}

fn on_path(name: &str) -> Option<PathBuf> {
    let out = Command::new("where").args([name]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| PathBuf::from(l.trim()))
        .find(|p| p.exists())
}

/// Locate the ubb CLI: $CUA_UBB, then `ubb_path` in cua's config, then PATH.
pub fn resolve() -> Option<Cli> {
    if let Ok(p) = std::env::var("CUA_UBB") {
        if let Some(c) = candidate(&p) {
            return Some(c);
        }
    }
    if let Some(p) = util::load_config_json().get("ubb_path").and_then(|v| v.as_str()) {
        if let Some(c) = candidate(p) {
            return Some(c);
        }
    }
    on_path("ubb").or_else(|| on_path("ubb.exe")).map(Cli::Exe)
}

fn candidate(p: &str) -> Option<Cli> {
    let pb = PathBuf::from(p);
    if !pb.exists() {
        return None;
    }
    Some(if p.ends_with(".mjs") || p.ends_with(".js") { Cli::Node(pb) } else { Cli::Exe(pb) })
}

const OUTPUT_CAP: usize = 8 * 1024 * 1024;

fn ubb_timeout() -> Duration {
    let ms = std::env::var("CUA_UBB_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .or_else(|| {
            util::load_config_json()
                .get("ubb_timeout_ms")
                .and_then(|v| v.as_u64())
        })
        .unwrap_or(120_000);
    Duration::from_millis(ms.clamp(1_000, 1_800_000))
}

fn read_capped<R: Read>(mut r: R, cap: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match r.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if out.len() < cap {
                    let take = (cap - out.len()).min(n);
                    out.extend_from_slice(&chunk[..take]);
                }
                // Keep draining past the cap so the child cannot block on a full pipe.
            }
        }
    }
    out
}

fn drain<R: Read + Send + 'static>(r: R, tx: std::sync::mpsc::Sender<Vec<u8>>) {
    std::thread::spawn(move || {
        let _ = tx.send(read_capped(r, OUTPUT_CAP));
    });
}

/// Run a child with bounded output and a hard deadline. On timeout only the
/// process tree started here is killed; a UBB daemon (if separate) is untouched.
fn run_bounded(mut cmd: Command, timeout: Duration) -> Result<(ExitStatus, Vec<u8>, Vec<u8>, bool)> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let (txo, rxo) = std::sync::mpsc::channel();
    let (txe, rxe) = std::sync::mpsc::channel();
    if let Some(o) = child.stdout.take() {
        drain(o, txo);
    }
    if let Some(e) = child.stderr.take() {
        drain(e, txe);
    }
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait()? {
            Some(s) => break s,
            None => {
                if started.elapsed() >= timeout {
                    timed_out = true;
                    let _ = crate::win::kill_process_tree(child.id());
                    let _ = child.kill();
                    break child.wait()?;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    };
    // A grandchild outside the killed tree could keep a pipe open; never wait
    // forever for the readers.
    let stdout = rxo.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    let stderr = rxe.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    Ok((status, stdout, stderr, timed_out))
}

pub fn cmd_browser(rest: &[String]) -> Result<i32> {
    let call = rest.join(" ");
    if rest.is_empty() || rest[0] == "help" || rest[0] == "--help" {
        println!("{USAGE}");
        return Ok(0);
    }
    let Some(cli) = resolve() else {
        let r = Receipt::new("browser").fail(serde_json::json!({ "call": call }),
            "ubb CLI not found: install UBB, then point $CUA_UBB or `ubb_path` in the cua config at it");
        r.print();
        return Ok(r.exit_code());
    };
    let mut cmd = match &cli {
        Cli::Exe(p) => {
            let mut c = Command::new(p);
            c.args(rest);
            c
        }
        Cli::Node(script) => {
            let mut c = Command::new("node");
            c.arg(script);
            c.args(rest);
            c
        }
    };
    // A node-based CLI must not inherit the vars that break Electron apps either.
    cmd.env_remove("ELECTRON_RUN_AS_NODE");
    cmd.env_remove("NODE_OPTIONS");
    cmd.env_remove("NODE_PATH");
    let timeout = ubb_timeout();
    let (status, stdout_bytes, stderr_bytes, timed_out) = run_bounded(cmd, timeout)?;
    let stdout = String::from_utf8_lossy(&stdout_bytes).trim().to_string();
    let stderr = String::from_utf8_lossy(&stderr_bytes).trim().to_string();
    let ok = status.success() && !timed_out;
    let parsed: Option<serde_json::Value> = if timed_out {
        None
    } else {
        serde_json::from_str(&stdout).ok()
    };
    let detail = if timed_out {
        format!(
            "ubb call timed out after {} ms; the process tree started by cua was killed",
            timeout.as_millis()
        )
    } else if ok {
        parsed.as_ref().map(summarize).unwrap_or_else(|| stdout.clone())
    } else if !stderr.is_empty() {
        stderr.clone()
    } else {
        stdout.clone()
    };
    let base = if ok {
        Receipt::new("browser").ok(serde_json::json!({ "call": call }), &detail)
    } else {
        Receipt::new("browser").fail(serde_json::json!({ "call": call }), &detail)
    };
    let mut v = serde_json::to_value(&base)?;
    v["backend"] = serde_json::json!("ubb");
    v["cli"] = serde_json::json!(cli.label());
    v["timed_out"] = serde_json::json!(timed_out);
    v["timeout_ms"] = serde_json::json!(timeout.as_millis() as u64);
    if let Some(p) = parsed {
        v["result"] = p;
    } else if !stdout.is_empty() {
        v["output"] = serde_json::json!(stdout);
    }
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(base.exit_code())
}

fn summarize(v: &serde_json::Value) -> String {
    for k in ["note", "url", "title", "message", "status"] {
        if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
            return format!("{k}={s}");
        }
    }
    if let Some(n) = v.get("count").and_then(|x| x.as_i64()) {
        return format!("count={n}");
    }
    if let Some(t) = v.get("tab_id").and_then(|x| x.as_i64()) {
        return format!("tab_id={t}");
    }
    v.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_capped_keeps_the_limit_and_drains() {
        let data = vec![b'x'; 1000];
        assert_eq!(read_capped(std::io::Cursor::new(&data), 10).len(), 10);
        assert_eq!(read_capped(std::io::Cursor::new(&data), 5000).len(), 1000);
        assert!(read_capped(std::io::Cursor::new(Vec::<u8>::new()), 10).is_empty());
    }
}
