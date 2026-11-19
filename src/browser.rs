//! Browser control: drives the user's own logged-in Edge/Chrome through the
//! CUA Browser extension (`browser/extension`), the local bridge daemon in
//! `crates/browser-bridge`, and this CLI.
//!
//! Nothing here spawns a helper process. A call is one loopback WebSocket frame
//! to `cua browser serve`, which is this same binary running detached.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::util::{self, Receipt};
use cua_browser_bridge::{client, config as bcfg, Launch};

pub const USAGE: &str = r#"cua browser <tool> [json-params]        # run one browser tool
              [--browser <slot>]         # target one paired slot (cuaedge/cuachrome/default)
              [--out <file>]             # save binary output (screenshot/PDF) to a file
              [--timeout <ms>]           # override the call deadline

Core loop:
  cua browser status                        # daemon + extension health
  cua browser session.start
  cua browser tab.create '{"url":"example.com"}'
  cua browser snapshot                      # accessibility tree with @eN refs
  cua browser click '{"ref":"@e3"}'
  cua browser wait_for '{"condition":"text_present","text":"Welcome"}'
  cua browser screenshot --out shot.png
  cua browser session.stop                  # always end with this

Setup:  cua browser install   (loads the extension into Edge/Chrome, pairs it)
Admin:  cua browser serve | stop | mcp   (mcp = MCP stdio adapter for IDE clients)
Remote: cua browser tunnel.start '{"base_url":"https://relay","token":"<relay token>","allow":"read"}'
        cua browser tunnel.status | tunnel.stop   (opt-in remote relay, default off)
Also:   list (full tool catalog) · browsers · find · read_page · get_html · fill ·
        press_key · scroll · tab.list/select/close · console.list · network.list ·
        extract · read_site
Refs come only from the newest snapshot; navigation invalidates them.
Safety (enforced by the extension): password/OTP/payment fields are never auto-filled,
destructive labels and restricted URLs are refused, and the user can interrupt at any time."#;

pub fn port() -> u16 {
    if let Ok(p) = std::env::var("CUA_BROWSER_PORT") {
        if let Ok(n) = p.trim().parse::<u16>() {
            if n > 0 {
                return n;
            }
        }
    }
    if let Some(n) = util::load_config_json()
        .get("browser_port")
        .and_then(|v| v.as_u64())
    {
        if n > 0 && n < 65536 {
            return n as u16;
        }
    }
    bcfg::port_of(&bcfg::read())
}

pub fn token() -> Result<String> {
    if let Ok(t) = std::env::var("CUA_BROWSER_TOKEN") {
        if t.len() >= 32 {
            return Ok(t);
        }
    }
    bcfg::token_and_port()
        .map(|(t, _)| t)
        .map_err(|e| anyhow::anyhow!(e))
}

fn timeout() -> Duration {
    let ms = std::env::var("CUA_BROWSER_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .or_else(|| {
            util::load_config_json()
                .get("browser_timeout_ms")
                .and_then(|v| v.as_u64())
        })
        .unwrap_or(120_000);
    Duration::from_millis(ms.clamp(1_000, 1_800_000))
}

fn launch() -> Launch {
    Launch {
        exe: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("cua.exe")),
        subcommand: vec!["browser".into(), "serve".into()],
    }
}

/// Cheap liveness probe for code that must not start a daemon (the child
/// session pairs against the main session's daemon, never its own).
pub fn daemon_alive() -> bool {
    client::health(port(), 400).is_some()
}

/// Extension root (the directory holding manifest.json).
pub fn extension_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CUA_BROWSER_EXTENSION_DIR") {
        let pb = PathBuf::from(p);
        if pb.join("manifest.json").is_file() {
            return Some(pb);
        }
    }
    if let Some(p) = util::load_config_json()
        .get("browser_extension_dir")
        .and_then(|v| v.as_str())
    {
        let pb = PathBuf::from(p);
        if pb.join("manifest.json").is_file() {
            return Some(pb);
        }
    }
    // The checkout's own browser/extension comes FIRST. A copy under the data
    // dir used to win, which meant editing the sources changed nothing the
    // browser was actually loading — the classic "I reloaded and the UI did not
    // change" trap. Whoever installed cua keeps owning the extension; the data
    // dir copy stays only as a fallback for a bin-only deployment.
    let rels: [&[&str]; 2] = [&["browser", "extension"], &[]];
    for dir in search_roots() {
        for rel in rels {
            let mut p = dir.clone();
            for part in rel {
                p.push(part);
            }
            if p.join("manifest.json").is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Directories worth probing: next to the running cua.exe, then up to three
/// ancestors (target/debug -> target -> repo), then the working directory.
fn search_roots() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe.parent().map(|p| p.to_path_buf());
        let mut depth = 0;
        while let Some(dir) = cur {
            if !out.iter().any(|d| d == &dir) {
                out.push(dir.clone());
            }
            depth += 1;
            if depth >= 4 {
                break;
            }
            cur = dir.parent().map(|p| p.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if !out.iter().any(|d| d == &cwd) {
            out.push(cwd);
        }
    }
    out
}

/// Run one daemon-side method (`browser.pair`, `meta.browsers`, a tool, ...) and
/// return its result object.
pub fn call_json(method: &str, params: &serde_json::Value) -> Result<serde_json::Value> {
    let token = token()?;
    let port = port();
    let deadline = timeout();
    let ms = deadline.as_millis() as u64;
    let mut out = client::call(port, &token, method, params, None, ms);
    if let Err(client::CallError::NoDaemon) = out {
        if client::ensure(port, &launch()).is_ok() {
            out = client::call(port, &token, method, params, None, ms);
        }
    }
    let v = out.map_err(|e| explain_rejection(e, port, &token))?;
    Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null))
}

/// Turn a rejected handshake into the actual cause when the port is held by a
/// daemon started from another config (a leftover install, a test data dir).
fn explain_rejection(e: client::CallError, port: u16, token: &str) -> anyhow::Error {
    if let client::CallError::Rejected = e {
        if let Some(false) = client::token_matches(port, token) {
            return anyhow::anyhow!(
                "port {port} is held by a daemon started with a different token \
                 (this cua expects {}); stop that process (Task Manager, or run \
                 `cua browser stop` from the install that started it) and retry",
                cua_browser_bridge::token_id(token)
            );
        }
    }
    anyhow::anyhow!("{e}")
}

fn file_size_note(bytes: &[u8]) -> String {
    format!("{} KB", bytes.len() / 1024)
}

pub fn cmd_browser(rest: &[String]) -> Result<i32> {
    let mut slot: Option<String> = None;
    let mut out_file: Option<String> = None;
    let mut extra_timeout: Option<u64> = None;
    let mut serve_port: Option<u16> = None;
    let mut serve_token: Option<String> = None;
    let mut pos: Vec<String> = Vec::new();
    let mut i = 0;
    let raw_call = rest.join(" ");
    while i < rest.len() {
        match rest[i].as_str() {
            "--browser" => {
                i += 1;
                slot = rest.get(i).cloned();
            }
            "--out" => {
                i += 1;
                out_file = rest.get(i).cloned();
            }
            "--timeout" => {
                i += 1;
                extra_timeout = rest.get(i).and_then(|v| v.parse().ok());
            }
            "--token" => {
                i += 1;
                serve_token = rest.get(i).cloned();
            }
            "--port" => {
                i += 1;
                serve_port = rest.get(i).and_then(|v| v.parse().ok());
            }
            other => pos.push(other.to_string()),
        }
        i += 1;
    }

    let cmd = pos.first().cloned().unwrap_or_default();
    if cmd.is_empty() || cmd == "help" || cmd == "--help" || cmd == "-h" {
        println!("{USAGE}");
        return Ok(0);
    }

    if cmd == "serve" {
        // Detached daemon mode: `cua browser serve --port N --token T`.
        let token = match serve_token.or_else(|| std::env::var("CUA_BROWSER_TOKEN").ok()) {
            Some(t) if t.len() >= 32 => t,
            _ => token()?,
        };
        cua_browser_bridge::serve(serve_port.unwrap_or_else(port), token);
    }

    if cmd == "install" {
        return install();
    }

    if cmd == "mcp" {
        // MCP stdio adapter: the IDE spawns `cua.exe browser mcp`; this process
        // never returns until the client closes stdin.
        cua_browser_bridge::mcp::serve_stdio(launch());
    }

    // The extension is optional for the next few commands, mandatory after.
    let token = token()?;
    let port = port();
    let ms = extra_timeout.unwrap_or_else(|| timeout().as_millis() as u64);

    if cmd == "stop" {
        match client::stop(port, &token) {
            Ok(body) => {
                let r = Receipt::new("browser")
                    .ok(serde_json::json!({ "call": "stop", "port": port }), &body);
                r.print();
                return Ok(r.exit_code());
            }
            Err(()) => {
                let r = Receipt::new("browser")
                    .fail(serde_json::json!({ "call": "stop", "port": port }), "no daemon is running");
                r.print();
                return Ok(r.exit_code());
            }
        }
    }

    if cmd == "status" {
        let cfg_token = token.clone();
        let h = client::health(port, 800).or_else(|| {
            client::ensure(port, &launch()).ok()?;
            client::health(port, 800)
        });
        let mut out = serde_json::json!({ "port": port, "daemon": h.is_some() });
        if let (Some(a), Some(h)) = (out.as_object_mut(), h.as_ref().and_then(|v| v.as_object())) {
            for (k, v) in h {
                a.insert(k.clone(), v.clone());
            }
        }
        let connected = h
            .as_ref()
            .and_then(|h| h.get("extension"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        // A daemon left over from another install answers /health but rejects our
        // token; say that here instead of letting every call fail with a 1008.
        let same_token = client::token_matches(port, &token).unwrap_or(true);
        let r = Receipt::new("browser").ok(serde_json::json!({ "call": "status" }), "status");
        let mut v = serde_json::to_value(&r)?;
        v["backend"] = serde_json::json!("browser");
        v["port"] = out["port"].clone();
        v["daemon"] = out["daemon"].clone();
        v["extension"] = serde_json::json!(connected);
        v["token_mismatch"] = serde_json::json!(!same_token);
        if !same_token {
            eprintln!(
                "# port {port} is held by a daemon started with a different token ({});                  stop that process and retry",
                cua_browser_bridge::token_id(&token)
            );
        }
        if let Some(obj) = out.as_object() {
            for (k, val) in obj {
                if k != "port" && k != "daemon" {
                    v[k] = val.clone();
                }
            }
        }
        v["result"] = out.clone();
        if !connected {
            eprintln!("# daemon is up, extension is not — run `cua browser install`, then load the extension once:");
            eprintln!("#   token {cfg_token}");
        }
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(0);
    }

    // `reload` is sugar for the extension.reload tool: fire it, then poll
    // /health until the extension reconnects with a fresh instance_id. This is
    // the cross-session way to pick up a code change on disk — the a11y
    // edge://extensions route only works where a visible browser window and a
    // human-scale UIA tree exist.
    if cmd == "reload" {
        let slot_arg = slot.as_deref();
        let _ = client::call(port, &token, "tool.extension.reload", &serde_json::Value::Null, slot_arg, ms);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut back = false;
        while std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(400));
            if let Some(h) = client::health(port, 800) {
                let ok = h.get("extension").and_then(|v| v.as_bool()).unwrap_or(false);
                if ok {
                    back = true;
                    let r = Receipt::new("browser").ok(
                        serde_json::json!({ "call": "reload", "slot": slot_arg }),
                        "extension reloaded and reconnected",
                    );
                    println!("{}", serde_json::to_string_pretty(&r)?);
                    return Ok(0);
                }
            }
        }
        let r = Receipt::new("browser").fail(
            serde_json::json!({ "call": "reload", "slot": slot_arg }),
            if back { "reload sent" } else { "extension did not reconnect within 20s" },
        );
        println!("{}", serde_json::to_string_pretty(&r)?);
        return Ok(1);
    }

    // `list` is sugar for the catalog tool; everything else is the tool name.
    let method = match cmd.as_str() {
        "list" => "tool.list".to_string(),
        "browsers" => "meta.browsers".to_string(),
        other => other.to_string(),
    };
    let params = match pos.get(1) {
        Some(raw) => serde_json::from_str::<serde_json::Value>(raw)
            .with_context(|| format!("params must be valid JSON: {raw}"))?,
        None => serde_json::Value::Null,
    };

    let mut res = client::call(port, &token, &method, &params, slot.as_deref(), ms);
    if let Err(client::CallError::NoDaemon) = res {
        if client::ensure(port, &launch()).is_ok() {
            res = client::call(port, &token, &method, &params, slot.as_deref(), ms);
        }
    }

    match res {
        Ok(msg) => {
            let result = msg
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let mut detail = summarize(&result);
            let mut extra = serde_json::Map::new();
            if let (Some(file), Some(b64)) = (
                out_file.as_deref(),
                result.get("data_base64").and_then(|v| v.as_str()),
            ) {
                let (saved, bytes) = save_base64(file, b64)?;
                detail = format!("saved {saved} ({})", file_size_note(&bytes));
                extra.insert("saved_to".into(), serde_json::json!(saved));
            }
            let r = Receipt::new("browser").ok(
                serde_json::json!({ "call": raw_call, "tool": method, "params": params }),
                &detail,
            );
            let mut v = serde_json::to_value(&r)?;
            v["backend"] = serde_json::json!("browser");
            v["port"] = serde_json::json!(port);
            for (k, val) in extra {
                v[k] = val;
            }
            let mut result = result;
            if let Some(o) = result.as_object_mut() {
                o.remove("data_base64");
            }
            v["result"] = result;
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(r.exit_code())
        }
        Err(e) => {
            let timed_out = matches!(&e, client::CallError::Timeout(_));
            let e = explain_rejection(e, port, &token);
            let r = Receipt::new("browser").fail(
                serde_json::json!({ "call": raw_call, "tool": method, "params": params }),
                &format!("{e}"),
            );
            let mut v = serde_json::to_value(&r)?;
            v["backend"] = serde_json::json!("browser");
            v["port"] = serde_json::json!(port);
            v["timed_out"] = serde_json::json!(timed_out);
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(r.exit_code())
        }
    }
}

fn save_base64(file: &str, b64: &str) -> Result<(String, Vec<u8>)> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .context("base64 decode failed")?;
    let path = Path::new(file);
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).ok();
        }
    }
    std::fs::write(path, &bytes).with_context(|| format!("write {file}"))?;
    let saved = std::fs::canonicalize(path)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| file.to_string());
    Ok((saved, bytes))
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
    if v.get("data_base64").is_some() {
        return "binary output; pass --out FILE to save it".to_string();
    }
    v.to_string()
}

/// Extension sources inside the checkout — what `install` copies *from*. Kept
/// separate from `extension_dir()` (which prefers the installed copy) so a
/// rebuild always refreshes the browser's copy.
fn source_extension_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CUA_BROWSER_EXTENSION_DIR") {
        let pb = PathBuf::from(p);
        if pb.join("manifest.json").is_file() {
            return Some(pb);
        }
    }
    for dir in search_roots() {
        let p = dir.join("browser").join("extension");
        if p.join("manifest.json").is_file() {
            return Some(p);
        }
    }
    None
}

/// `cua browser install`: put the extension where the browser can load it and
/// register the native-messaging host so it pairs without copying a token.
fn install() -> Result<i32> {
    let src = source_extension_dir().ok_or_else(|| {
        anyhow::anyhow!(
            "extension sources not found — run this from the cua checkout (browser/extension)"
        )
    })?;
    // The sources ARE the installed extension — the data-dir copy is no longer
    // authoritative, so editing the checkout takes effect on the next reload.
    // The copy is still refreshed when it already exists, because a profile
    // that was paired before this change keeps pointing at that path and Edge
    // refuses a junction/unpacked extension whose manifest it cannot read.
    let dest = src.clone();
    let legacy = bcfg::default_extension_dir();
    if legacy.join("manifest.json").is_file() {
        if let Err(e) = copy_dir(&src, &legacy) {
            eprintln!("# warning: could not refresh the legacy copy at {}: {e}", legacy.display());
        }
    }
    let (token, _) = bcfg::token_and_port().map_err(|e| anyhow::anyhow!(e))?;
    let mut cfg = bcfg::read().as_object().cloned().unwrap_or_default();
    cfg.insert(
        "extension_dir".into(),
        serde_json::json!(dest.display().to_string()),
    );
    bcfg::write(&cfg);
    // An unpacked extension's id is a hash of its directory. Register every
    // directory a user might have loaded the extension from, so an older copy
    // they loaded before this change still pairs.
    let mut dirs: Vec<PathBuf> = vec![dest.clone()];
    if legacy != dest && legacy.join("manifest.json").is_file() {
        dirs.push(legacy);
    }
    let manifest = cua_browser_bridge::register_native_host(&dirs)
        .map_err(|e| anyhow::anyhow!("native host registration failed: {e}"))?;
    let origins: Vec<String> = dirs
        .iter()
        .map(|d| cua_browser_bridge::native_origin(d))
        .collect();
    let r = Receipt::new("browser").ok(
        serde_json::json!({ "call": "install" }),
        &format!("extension at {}", dest.display()),
    );
    let mut v = serde_json::to_value(&r)?;
    v["extension_dir"] = serde_json::json!(dest.display().to_string());
    v["source_dir"] = serde_json::json!(
        source_extension_dir().map(|p| p.display().to_string()).unwrap_or_default()
    );
    v["native_host_manifest"] = serde_json::json!(manifest);
    v["extension_origins"] = serde_json::json!(origins);
    v["token"] = serde_json::json!(token);
    v["next"] = serde_json::json!([
        "edge://extensions (or chrome://extensions) → Developer mode → Load unpacked",
        "either copy works (both origins are registered):",
        serde_json::json!(dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>()),
        "after editing the extension, press ↻ on its card; `cua browser status` confirms the pairing"
    ]);
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(r.exit_code())
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).with_context(|| format!("mkdir {}", dst.display()))?;
    for entry in std::fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .with_context(|| format!("copy {}", from.display()))?;
        }
    }
    Ok(())
}

/// Native-messaging host mode: the browser launches `cua.exe <origin>` when the
/// extension opens a `connectNative` port. Never returns.
pub fn native_host(origin: &str) -> Result<i32> {
    let ext = extension_dir().unwrap_or_else(bcfg::default_extension_dir);
    cua_browser_bridge::native_host(origin, &ext, &launch());
}
