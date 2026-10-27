//! Browser control is delegated to UBB (Universal Browser Bridge), which drives the
//! user's own logged-in Edge/Chrome through an extension + local daemon. cua only
//! resolves the `ubb` CLI, forwards the call, and wraps the answer in a receipt.

use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

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
    let out = cmd.output()?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let ok = out.status.success();
    let parsed: Option<serde_json::Value> = serde_json::from_str(&stdout).ok();
    let detail = if ok {
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
