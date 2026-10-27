// browser.rs — isolated CDP browser control (ZCode browser-use / Qoder Browser Connector analog).
// Own Edge/Chrome profile + own debug port: never touches the user's browser session.
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use crate::util::Receipt;
use crate::BrowserOp;

pub const PORT: u16 = 9334;

fn browser_meta_path() -> std::path::PathBuf {
    crate::util::ensure_data_dir().join("browser.json")
}

fn load_active() -> Option<String> {
    let b = std::fs::read(browser_meta_path()).ok()?;
    let v: Value = serde_json::from_slice(&b).ok()?;
    v["active_target"].as_str().map(|s| s.to_string())
}

fn save_active(id: &str) {
    let v = json!({"active_target": id});
    let _ = std::fs::write(browser_meta_path(), serde_json::to_vec(&v).unwrap_or_default());
}

fn clear_active() {
    let _ = std::fs::remove_file(browser_meta_path());
}

// ---------- minimal HTTP for localhost DevTools endpoints ----------

fn http_get(port: u16, path: &str) -> Result<Value> {
    http_request(port, "GET", path)
}

fn http_request(port: u16, method: &str, path: &str) -> Result<Value> {
    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).map_err(|e| anyhow!("connect {port}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| anyhow!("set_read_timeout: {e}"))?;
    let req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(req.as_bytes())
        .map_err(|e| anyhow!("write: {e}"))?;
    let mut buf: Vec<u8> = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        let mut chunk = [0u8; 16384];
        let n = match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        buf.extend_from_slice(&chunk[..n]);
        if std::time::Instant::now() > deadline {
            break;
        }
        if buf.windows(4).any(|w| w == b"0\r\n\r\n") {
            break; // terminal chunked body
        }
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..p]).to_ascii_lowercase();
            if !head.contains("transfer-encoding: chunked") {
                if let Some(cl) = head.lines().find_map(|l| {
                    l.strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                }) {
                    if buf.len() >= p + 4 + cl {
                        break;
                    }
                }
            }
        }
    }
    if buf.is_empty() {
        bail!("devtools returned no data for {path}");
    }
    let raw = String::from_utf8_lossy(&buf).to_string();
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow!("bad http response (got {} bytes, head {:?})", buf.len(), raw.chars().take(80).collect::<String>()))?;
    let lower = head.to_ascii_lowercase();
    let body = if lower.contains("transfer-encoding: chunked") {
        dechunk(body)
    } else {
        body.to_string()
    };
    serde_json::from_str(body.trim())
        .map_err(|e| anyhow!("bad json from devtools: {e} (head: {})", raw.chars().take(100).collect::<String>()))
}

fn dechunk(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    loop {
        let Some((line, after)) = rest.split_once("\r\n") else { break };
        let Ok(size) = usize::from_str_radix(line.trim(), 16) else { break };
        if size == 0 {
            break;
        }
        if after.len() < size {
            out.push_str(after);
            break;
        }
        out.push_str(&after[..size]);
        rest = &after[size..];
        if let Some(r) = rest.strip_prefix("\r\n") {
            rest = r;
        }
    }
    out
}

// ---------- CDP over websocket ----------

struct Cdp {
    ws: tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl Cdp {
    fn connect() -> Result<(Cdp, Value)> {
        // fail fast when the debug endpoint is down (e.g. browser already closed)
        http_get(PORT, "/json/version")
            .map_err(|e| anyhow!("browser is not running on port {PORT} ({e:#})"))?;
        // the page target can lag the CDP endpoint on cold start; retry until it exists
        let mut last_err = None;
        for _ in 0..60 {
            match Self::connect_once() {
                Ok(v) => return Ok(v),
                Err(e) => last_err = Some(e),
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Err(last_err.unwrap_or_else(|| anyhow!("no page target")))
    }

    fn connect_once() -> Result<(Cdp, Value)> {
        let pages = page_targets()?;
        let active = load_active();
        let page = active
            .as_deref()
            .and_then(|id| pages.iter().find(|t| t["id"].as_str() == Some(id)))
            .or_else(|| pages.first())
            .ok_or_else(|| anyhow!("no page target yet"))?
            .clone();
        let url = page["webSocketDebuggerUrl"]
            .as_str()
            .ok_or_else(|| anyhow!("no webSocketDebuggerUrl"))?
            .to_string();
        if let Some(id) = page["id"].as_str() {
            if active.as_deref() != Some(id) {
                save_active(id);
            }
        }
        let (ws, _) = tungstenite::connect(url).map_err(|e| anyhow!("ws connect: {e}"))?;
        Ok((Cdp::wrap(ws), page))
    }

    fn wrap(ws: tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>) -> Cdp {
        // bounded reads so deadlines actually fire even when no event arrives
        if let tungstenite::stream::MaybeTlsStream::Plain(s) = ws.get_ref() {
            let _ = s.set_read_timeout(Some(Duration::from_millis(250)));
        }
        Cdp { ws, next_id: 1 }
    }

    fn from_url(url: String) -> Result<Cdp> {
        let (ws, _) = tungstenite::connect(url).map_err(|e| anyhow!("ws connect: {e}"))?;
        Ok(Cdp::wrap(ws))
    }

    fn call(&mut self, method: &str, params: Value, wait_event: Option<&str>, deadline: Duration) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"id": id, "method": method, "params": params});
        tungstenite::Message::Text(msg.to_string().into());
        self.ws.send(tungstenite::Message::Text(msg.to_string().into()))?;
        let started = Instant::now();
        let mut result: Option<Value> = None;
        loop {
            if started.elapsed() > deadline {
                bail!("CDP timeout waiting for {method}{}", wait_event.map(|e| format!(" / {e}")).unwrap_or_default());
            }
            let m = match self.ws.read() {
                Ok(m) => m,
                Err(tungstenite::Error::Io(e))
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            let Value::Object(obj) = (match m {
                tungstenite::Message::Text(t) => serde_json::from_str::<Value>(&t)?,
                tungstenite::Message::Ping(_) | tungstenite::Message::Pong(_) | tungstenite::Message::Binary(_) => continue,
                tungstenite::Message::Close(c) => bail!("ws closed: {c:?}"),
                _ => continue,
            }) else {
                continue;
            };
            if obj.get("id").and_then(|v| v.as_u64()) == Some(id) {
                if let Some(err) = obj.get("error") {
                    bail!("CDP error from {method}: {err}");
                }
                result = Some(obj.get("result").cloned().unwrap_or(Value::Null));
                if wait_event.is_none() {
                    break;
                }
            }
            if let Some(ev) = wait_event {
                if obj.get("method").and_then(|v| v.as_str()) == Some(ev) {
                    return Ok(result.unwrap_or(Value::Null));
                }
            }
        }
        result.ok_or_else(|| anyhow!("no result"))
    }

    fn eval(&mut self, expr: &str) -> Result<Value> {
        let r = self.call("Runtime.evaluate", json!({"expression": expr, "returnByValue": true, "awaitPromise": true}), None, Duration::from_secs(10))?;
        if let Some(details) = r["exceptionDetails"].as_object() {
            bail!("page exception: {}", details.get("text").map(|s| s.to_string()).unwrap_or_default());
        }
        Ok(r["result"]["value"].clone())
    }
}

fn page_targets() -> Result<Vec<Value>> {
    let meta = http_get(PORT, "/json/list")?;
    let arr = meta.as_array().ok_or_else(|| anyhow!("bad target list"))?;
    Ok(arr.iter().filter(|t| t["type"] == "page").cloned().collect())
}

fn resolve_target_arg(arg: &str) -> Result<Value> {
    let pages = page_targets()?;
    if let Ok(idx) = arg.parse::<usize>() {
        return pages
            .get(idx)
            .cloned()
            .ok_or_else(|| anyhow!("no tab at index {idx} ({} pages)", pages.len()));
    }
    pages
        .iter()
        .find(|t| t["id"].as_str() == Some(arg))
        .or_else(|| {
            pages.iter().find(|t| {
                t["id"]
                    .as_str()
                    .map(|id| id.starts_with(arg))
                    .unwrap_or(false)
            })
        })
        .cloned()
        .ok_or_else(|| anyhow!("no tab matching '{arg}'"))
}

fn connect_browser_cdp() -> Result<Cdp> {
    let v = http_get(PORT, "/json/version")?;
    let url = v["webSocketDebuggerUrl"]
        .as_str()
        .ok_or_else(|| anyhow!("browser endpoint has no webSocketDebuggerUrl"))?
        .to_string();
    Cdp::from_url(url)
}

fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b':' | b'/' | b'?') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn tabs_json() -> Result<Value> {
    let pages = page_targets()?;
    let active = load_active();
    let tabs: Vec<Value> = pages
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let id = t["id"].as_str().unwrap_or("");
            json!({
                "index": i,
                "id": id,
                "title": t["title"],
                "url": t["url"],
                "active": active.as_deref() == Some(id),
            })
        })
        .collect();
    Ok(json!({
        "schema": "cua-browser-tabs-v1",
        "port": PORT,
        "count": tabs.len(),
        "tabs": tabs,
    }))
}

// ---------- process launch ----------

fn wait_page_target(deadline_ms: usize) -> bool {
    for _ in 0..(deadline_ms / 250) {
        if let Ok(v) = http_get(PORT, "/json/list") {
            if v.as_array().map(|a| a.iter().any(|t| t["type"] == "page")).unwrap_or(false) {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

fn ensure_running(exe_hint: Option<&str>) -> Result<()> {
    // already running?
    if http_get(PORT, "/json/version").is_ok() {
        if wait_page_target(8000) {
            return Ok(());
        }
        // stale/dying instance: try launching into it; if the port frees up late,
        // the wait loop below still rescues us.
    }
    let candidates: Vec<String> = if let Some(e) = exe_hint {
        vec![e.to_string()]
    } else {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
        let pfx86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".into());
        vec![
            format!("{local}\\Microsoft\\Edge\\Application\\msedge.exe"),
            format!("{pf}\\Microsoft\\Edge\\Application\\msedge.exe"),
            format!("{pfx86}\\Microsoft\\Edge\\Application\\msedge.exe"),
            format!("{pf}\\Google\\Chrome\\Application\\chrome.exe"),
            format!("{pfx86}\\Google\\Chrome\\Application\\chrome.exe"),
            format!("{local}\\Google\\Chrome\\Application\\chrome.exe"),
        ]
    };
    let exe = candidates
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
        .ok_or_else(|| anyhow!("no Edge/Chrome found; pass --exe"))?;
    let profile = crate::util::ensure_data_dir().join("browser-profile");
    let args = format!(
        "--remote-debugging-port={PORT} --user-data-dir=\"{}\" --no-first-run --no-default-browser-check --window-size=1280,800 about:blank",
        profile.to_string_lossy()
    );
    crate::spawn_process(&exe, Some(&args), None)?;
    // wait for CDP + a page target
    for _ in 0..120 {
        if http_get(PORT, "/json/version").is_ok() {
            if !wait_page_target(15_000) {
                bail!("CDP is up but no page target appeared");
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    bail!("browser started but CDP did not come up on port {PORT} within 30s")
}

// ---------- commands ----------

pub fn dispatch(op: BrowserOp) -> Result<i32> {
    match op {
        BrowserOp::Open { url, exe } => cmd_open(url, exe),
        BrowserOp::Tabs => cmd_tabs(),
        BrowserOp::Navigate { url } => cmd_navigate(&url),
        BrowserOp::Shot { out } => cmd_shot(&out),
        BrowserOp::Snapshot => cmd_snapshot(),
        BrowserOp::Click { x, y, selector, text } => cmd_click(x, y, selector, text),
        BrowserOp::Type { text, selector, clear } => cmd_type(&text, selector, clear),
        BrowserOp::Keys { keys } => cmd_keys(&keys),
        BrowserOp::Eval { expr } => cmd_eval(&expr),
        BrowserOp::Wait { selector, text, timeout_ms } => cmd_wait(selector, text, timeout_ms),
        BrowserOp::Reload => cmd_reload(),
        BrowserOp::Scroll { dx, dy } => cmd_scroll(dx, dy),
        BrowserOp::TabNew { url } => cmd_tab_new(url),
        BrowserOp::TabSwitch { target } => cmd_tab_switch(&target),
        BrowserOp::TabClose { target } => cmd_tab_close(&target),
        BrowserOp::Back => cmd_back(),
        BrowserOp::Close => cmd_close(),
    }
}

fn cmd_open(url: Option<String>, exe: Option<String>) -> Result<i32> {
    ensure_running(exe.as_deref())?;
    let (mut cdp, target) = Cdp::connect()?;
    if let Some(id) = target["id"].as_str() {
        save_active(id);
    }
    cdp.call("Page.enable", json!({}), None, Duration::from_secs(5))?;
    let mut final_url = target["url"].as_str().unwrap_or("").to_string();
    if let Some(u) = url {
        cdp.call(
            "Page.navigate",
            json!({"url": u}),
            Some("Page.loadEventFired"),
            Duration::from_secs(15),
        )?;
        // read back the actual URL (deterministic contract, like Qoder's tab claim)
        if let Ok(v) = cdp.eval("location.href") {
            if let Some(s) = v.as_str() {
                final_url = s.to_string();
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-browser-v1",
            "port": PORT,
            "title": target["title"],
            "url": final_url,
            "note": "isolated profile; user browser untouched"
        }))?
    );
    Ok(0)
}

fn cmd_tabs() -> Result<i32> {
    let v = tabs_json()?;
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(0)
}

fn cmd_navigate(url: &str) -> Result<i32> {
    let started = Instant::now();
    let (mut cdp, target) = Cdp::connect()?;
    if let Some(id) = target["id"].as_str() {
        save_active(id);
    }
    cdp.call("Page.enable", json!({}), None, Duration::from_secs(5))?;
    let r = cdp.call("Page.navigate", json!({"url": url}), Some("Page.loadEventFired"), Duration::from_secs(20));
    let detail = match &r {
        Ok(_) => format!("navigated to {url} (load event)"),
        Err(_) => format!("navigated to {url} (no load event within timeout)"),
    };
    let mut receipt = match r {
        Ok(_) => Receipt::new("browser-navigate").ok(json!({"url": url}), &detail),
        Err(e) => Receipt::new("browser-navigate").fail(json!({"url": url}), &format!("{e:#}")),
    };
    receipt.elapsed_ms = Some(started.elapsed().as_millis());
    receipt.print();
    Ok(receipt.exit_code())
}

fn cmd_shot(out: &str) -> Result<i32> {
    let (mut cdp, _) = Cdp::connect()?;
    let r = cdp.call("Page.captureScreenshot", json!({"format": "png"}), None, Duration::from_secs(10))?;
    let data = r["data"].as_str().ok_or_else(|| anyhow!("no screenshot data"))?;
    let bytes = b64_decode(data)?;
    let path = std::path::PathBuf::from(out);
    let n = bytes.len();
    std::fs::write(&path, &bytes)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"path": path.to_string_lossy(), "bytes": n}))?
    );
    Ok(0)
}

fn cmd_snapshot() -> Result<i32> {
    let (mut cdp, _) = Cdp::connect()?;
    let expr = "(() => ({title: document.title, url: location.href, text: (document.body?document.body.innerText:'').slice(0,5000), clickables: [...document.querySelectorAll('a,button,input,select,textarea,[role=button],[role=link]')].slice(0,100).map((e,i)=>({i, tag:e.tagName.toLowerCase(), text:((e.innerText||e.value||e.placeholder||'')+'').trim().slice(0,60), href:e.href||null, rect:(r=>[r.x|0,r.y|0,r.width|0,r.height|0])(e.getBoundingClientRect())})).filter(c=>c.text||c.href)}))()";
    let v = cdp.eval(expr)?;
    println!("{}", serde_json::to_string_pretty(&json!({
        "schema": "cua-browser-snapshot-v1",
        "snapshot": v,
        "note": "clickables are the analog of ZCode AI/ARIA domSnapshot; use cua browser click --x --y with rect centers"
    }))?);
    Ok(0)
}

fn cmd_click(x: Option<i32>, y: Option<i32>, selector: Option<String>, text: Option<String>) -> Result<i32> {
    let started = Instant::now();
    let (mut cdp, _) = Cdp::connect()?;
    let mut target_info = json!({"x": x, "y": y, "selector": selector, "text": text});
    let r = (|| -> Result<(i32, i32, String)> {
        let (cx, cy, detail) = match (x, y, &selector, &text) {
            (Some(x), Some(y), None, None) => (x, y, format!("({x},{y})")),
            (None, None, Some(sel), None) => {
                let expr = format!(
                    "(() => {{ const el=document.querySelector({}); if(!el) return null; el.scrollIntoView({{block:'center'}}); const r=el.getBoundingClientRect(); return {{x:r.x+r.width/2, y:r.y+r.height/2, tag:el.tagName.toLowerCase(), text:((el.innerText||el.value||'')+'').trim().slice(0,60)}}; }})()",
                    js_str(sel)
                );
                let v = cdp.eval(&expr)?;
                if v.is_null() {
                    bail!("selector not found: {sel}");
                }
                let cx = v["x"].as_f64().unwrap_or(0.0).round() as i32;
                let cy = v["y"].as_f64().unwrap_or(0.0).round() as i32;
                target_info = json!({"selector": sel, "x": cx, "y": cy, "tag": v["tag"], "text": v["text"]});
                (cx, cy, format!("'{}' <{}> at ({cx},{cy})", v["text"].as_str().unwrap_or(""), v["tag"].as_str().unwrap_or("")))
            }
            (None, None, None, Some(wanted)) => {
                let expr = format!(
                    "(() => {{ const t={}; const els=[...document.querySelectorAll('a,button,input,select,textarea,[role=button],[role=link]')]; const norm=e=>((e.innerText||e.value||e.placeholder||'')+'').trim(); const el=els.find(e=>norm(e)===t)||els.find(e=>norm(e).includes(t)); if(!el) return null; el.scrollIntoView({{block:'center'}}); const r=el.getBoundingClientRect(); return {{x:r.x+r.width/2, y:r.y+r.height/2, tag:el.tagName.toLowerCase(), text:norm(el).slice(0,60)}}; }})()",
                    js_str(wanted)
                );
                let v = cdp.eval(&expr)?;
                if v.is_null() {
                    bail!("no clickable with text: {wanted}");
                }
                let cx = v["x"].as_f64().unwrap_or(0.0).round() as i32;
                let cy = v["y"].as_f64().unwrap_or(0.0).round() as i32;
                target_info = json!({"text": wanted, "x": cx, "y": cy, "tag": v["tag"], "matched": v["text"]});
                (cx, cy, format!("'{}' <{}> at ({cx},{cy})", v["text"].as_str().unwrap_or(""), v["tag"].as_str().unwrap_or("")))
            }
            _ => bail!("specify exactly one of --x/--y, --selector, or --text"),
        };
        cdp.call("Input.dispatchMouseEvent", json!({"type": "mouseMoved", "x": cx, "y": cy}), None, Duration::from_secs(5))?;
        cdp.call("Input.dispatchMouseEvent", json!({"type": "mousePressed", "x": cx, "y": cy, "button": "left", "buttons": 1, "clickCount": 1}), None, Duration::from_secs(5))?;
        cdp.call("Input.dispatchMouseEvent", json!({"type": "mouseReleased", "x": cx, "y": cy, "button": "left", "buttons": 1, "clickCount": 1}), None, Duration::from_secs(5))?;
        Ok((cx, cy, detail))
    })();
    let mut receipt = match r {
        Ok((_cx, _cy, detail)) => Receipt::new("browser-click")
            .ok(target_info, &format!("clicked {detail} via CDP Input")),
        Err(e) => Receipt::new("browser-click").fail(target_info, &format!("{e:#}")),
    };
    receipt.elapsed_ms = Some(started.elapsed().as_millis());
    receipt.print();
    Ok(receipt.exit_code())
}

fn cmd_type(text: &str, selector: Option<String>, clear: bool) -> Result<i32> {
    let (mut cdp, _) = Cdp::connect()?;
    let target = match &selector {
        Some(sel) => json!({"selector": sel}),
        None => json!({"chars": text.chars().count()}),
    };
    let r = (|| -> Result<String> {
        if let Some(sel) = &selector {
            let expr = format!(
                "(() => {{ const el=document.querySelector({}); if(!el) return false; el.focus(); if ({clear}) {{ if ('value' in el) {{ el.value=''; el.dispatchEvent(new Event('input',{{bubbles:true}})); }} else if (el.isContentEditable) {{ el.textContent=''; }} }} else {{ if (el.select) {{ try {{ el.select(); }} catch(e) {{}} }} }} return true; }})()",
                js_str(sel),
                clear = clear
            );
            if cdp.eval(&expr)?.as_bool() != Some(true) {
                bail!("selector not found: {sel}");
            }
        }
        cdp.call("Input.insertText", json!({"text": text}), None, Duration::from_secs(5))?;
        Ok(format!(
            "inserted {} chars{} via CDP",
            text.chars().count(),
            selector
                .as_deref()
                .map(|s| format!(" into {s}"))
                .unwrap_or_default()
        ))
    })();
    let receipt = match r {
        Ok(detail) => Receipt::new("browser-type").ok(target, &detail),
        Err(e) => Receipt::new("browser-type").fail(target, &format!("{e:#}")),
    };
    receipt.print();
    Ok(receipt.exit_code())
}

fn cmd_keys(keys: &str) -> Result<i32> {
    let (mut cdp, _) = Cdp::connect()?;
    let combo = crate::input::parse_combo(keys)?;
    let vk = combo.key.0;
    let (key, code, wvk) = vk_to_cdp(vk);
    let mut modifiers = 0u32;
    for m in &combo.modifiers {
        match *m {
            0x11 => modifiers |= 2,  // ctrl
            0x10 => modifiers |= 8,  // shift
            0x12 => modifiers |= 1,  // alt
            _ => {}
        }
    }
    let r = (|| -> Result<()> {
        for m in &combo.modifiers {
            let (mk, mc, mv) = vk_to_cdp(*m);
            cdp.call("Input.dispatchKeyEvent", json!({"type": "rawKeyDown", "key": mk, "code": mc, "windowsVirtualKeyCode": mv, "modifiers": modifiers}), None, Duration::from_secs(5))?;
        }
        cdp.call("Input.dispatchKeyEvent", json!({"type": "keyDown", "key": key, "code": code, "windowsVirtualKeyCode": wvk, "modifiers": modifiers}), None, Duration::from_secs(5))?;
        cdp.call("Input.dispatchKeyEvent", json!({"type": "keyUp", "key": key, "code": code, "windowsVirtualKeyCode": wvk, "modifiers": modifiers}), None, Duration::from_secs(5))?;
        for m in combo.modifiers.iter().rev() {
            let (mk, mc, mv) = vk_to_cdp(*m);
            cdp.call("Input.dispatchKeyEvent", json!({"type": "keyUp", "key": mk, "code": mc, "windowsVirtualKeyCode": mv, "modifiers": modifiers}), None, Duration::from_secs(5))?;
        }
        Ok(())
    })();
    let receipt = match r {
        Ok(()) => Receipt::new("browser-keys").ok(json!({"keys": keys}), "dispatched via CDP"),
        Err(e) => Receipt::new("browser-keys").fail(json!({}), &format!("{e:#}")),
    };
    receipt.print();
    Ok(receipt.exit_code())
}

fn vk_to_cdp(vk: u16) -> (&'static str, &'static str, u32) {
    match vk {
        0x0D => ("\r", "Enter", 13),
        0x09 => ("\t", "Tab", 9),
        0x1B => ("Escape", "Escape", 27),
        0x08 => ("Backspace", "Backspace", 8),
        0x2E => ("Delete", "Delete", 46),
        0x25 => ("ArrowLeft", "ArrowLeft", 37),
        0x26 => ("ArrowUp", "ArrowUp", 38),
        0x27 => ("ArrowRight", "ArrowRight", 39),
        0x28 => ("ArrowDown", "ArrowDown", 40),
        0x24 => ("Home", "Home", 36),
        0x23 => ("End", "End", 35),
        0x21 => ("PageUp", "PageUp", 33),
        0x22 => ("PageDown", "PageDown", 34),
        0x20 => (" ", "Space", 32),
        0x10 => ("Shift", "ShiftLeft", 16),
        0x11 => ("Control", "ControlLeft", 17),
        0x12 => ("Alt", "AltLeft", 18),
        v if (0x70..=0x7B).contains(&v) => {
            let idx = v - 0x70;
            static NAMES: [&str; 12] = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"];
            (NAMES[idx as usize], NAMES[idx as usize], v as u32)
        }
        v if (0x30..=0x39).contains(&v) => {
            static NAMES: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
            let i = (v - 0x30) as usize;
            (NAMES[i], NAMES[i], v as u32)
        }
        v if (0x41..=0x5A).contains(&v) => {
            let c = (b'A' + (v - 0x41) as u8) as char;
            let s = c.to_string();
            // leak tiny static strings for lifetime simplicity
            let leaked: &'static str = Box::leak(s.into_boxed_str());
            (leaked, leaked, v as u32)
        }
        _ => ("Unidentified", "Unidentified", vk as u32),
    }
}

fn cmd_eval(expr: &str) -> Result<i32> {
    let (mut cdp, _) = Cdp::connect()?;
    let v = cdp.eval(expr)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-browser-eval-v1",
            "result": v,
        }))?
    );
    Ok(0)
}

fn cmd_wait(selector: Option<String>, text: Option<String>, timeout_ms: u64) -> Result<i32> {
    if selector.is_none() && text.is_none() {
        bail!("browser wait requires --selector or --text");
    }
    let started = Instant::now();
    let timeout = Duration::from_millis(timeout_ms.max(50));
    let (mut cdp, _) = Cdp::connect()?;
    cdp.call("Page.enable", json!({}), None, Duration::from_secs(5))?;
    loop {
        let mut parts: Vec<String> = Vec::new();
        if let Some(sel) = &selector {
            parts.push(format!("!!document.querySelector({})", js_str(sel)));
        }
        if let Some(t) = &text {
            parts.push(format!(
                "((document.body&&document.body.innerText)||'').includes({})",
                js_str(t)
            ));
        }
        let expr = format!("(() => ({}) )()", parts.join(" && "));
        let ok = cdp.eval(&expr).ok().and_then(|v| v.as_bool()).unwrap_or(false);
        if ok {
            let title = cdp.eval("document.title").unwrap_or(Value::Null);
            let url = cdp.eval("location.href").unwrap_or(Value::Null);
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "schema": "cua-browser-wait-v1",
                    "satisfied": true,
                    "elapsed_ms": started.elapsed().as_millis(),
                    "title": title,
                    "url": url,
                }))?
            );
            return Ok(0);
        }
        if started.elapsed() >= timeout {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "schema": "cua-browser-wait-v1",
                    "satisfied": false,
                    "elapsed_ms": started.elapsed().as_millis(),
                    "selector": selector,
                    "text": text,
                }))?
            );
            return Ok(1);
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn cmd_reload() -> Result<i32> {
    let started = Instant::now();
    let (mut cdp, _) = Cdp::connect()?;
    cdp.call("Page.enable", json!({}), None, Duration::from_secs(5))?;
    let r = cdp.call("Page.reload", json!({"ignoreCache": false}), Some("Page.loadEventFired"), Duration::from_secs(15));
    let receipt = match r {
        Ok(_) => Receipt::new("browser-reload").ok(json!({}), "page reloaded (load event)"),
        Err(e) => Receipt::new("browser-reload").fail(json!({}), &format!("{e:#}")),
    };
    let mut receipt = receipt;
    receipt.elapsed_ms = Some(started.elapsed().as_millis());
    receipt.print();
    Ok(receipt.exit_code())
}

fn cmd_scroll(dx: i32, dy: i32) -> Result<i32> {
    let (mut cdp, _) = Cdp::connect()?;
    let r = (|| -> Result<Value> {
        let expr = format!("(() => {{ window.scrollBy({dx},{dy}); return {{x: window.scrollX, y: window.scrollY}}; }})()");
        cdp.eval(&expr)
    })();
    let receipt = match r {
        Ok(pos) => Receipt::new("browser-scroll").ok(pos.clone(), &format!("scrolled by ({dx},{dy})")),
        Err(e) => Receipt::new("browser-scroll").fail(json!({}), &format!("{e:#}")),
    };
    receipt.print();
    Ok(receipt.exit_code())
}

fn cmd_tab_new(url: Option<String>) -> Result<i32> {
    let target_url = url.unwrap_or_else(|| "about:blank".into());
    let path = format!("/json/new?{}", url_encode(&target_url));
    let v = http_request(PORT, "PUT", &path)
        .or_else(|_| http_request(PORT, "GET", &path))
        .map_err(|e| anyhow!("open new tab: {e}"))?;
    let id = v["id"].as_str().ok_or_else(|| anyhow!("new tab response has no id: {v}"))?.to_string();
    save_active(&id);
    // the target may take a moment to appear in /json/list
    for _ in 0..40 {
        if page_targets().map(|p| p.iter().any(|t| t["id"].as_str() == Some(id.as_str()))).unwrap_or(false) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-browser-tab-v1",
            "action": "new",
            "id": id,
            "url": v["url"],
            "title": v["title"],
            "active": true,
        }))?
    );
    Ok(0)
}

fn cmd_tab_switch(arg: &str) -> Result<i32> {
    let t = resolve_target_arg(arg)?;
    let id = t["id"].as_str().ok_or_else(|| anyhow!("target has no id"))?.to_string();
    let mut cdp = connect_browser_cdp()?;
    cdp.call("Target.activateTarget", json!({"targetId": id}), None, Duration::from_secs(5))?;
    save_active(&id);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-browser-tab-v1",
            "action": "switch",
            "id": id,
            "title": t["title"],
            "url": t["url"],
            "active": true,
        }))?
    );
    Ok(0)
}

fn cmd_tab_close(arg: &str) -> Result<i32> {
    let t = resolve_target_arg(arg)?;
    let id = t["id"].as_str().ok_or_else(|| anyhow!("target has no id"))?.to_string();
    let mut cdp = connect_browser_cdp()?;
    let r = cdp.call("Target.closeTarget", json!({"targetId": id}), None, Duration::from_secs(5))?;
    if load_active().as_deref() == Some(id.as_str()) {
        clear_active();
        // hand activity to a surviving tab so index-based commands stay deterministic
        if let Ok(pages) = page_targets() {
            if let Some(next) = pages.first().and_then(|t| t["id"].as_str()) {
                save_active(next);
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-browser-tab-v1",
            "action": "close",
            "id": id,
            "success": r["success"],
        }))?
    );
    Ok(0)
}

fn cmd_back() -> Result<i32> {
    let (mut cdp, _) = Cdp::connect()?;
    let _ = cdp.eval("history.back()")?;
    println!("{}", serde_json::to_string_pretty(&json!({"ok": true}))?);
    Ok(0)
}

fn cmd_close() -> Result<i32> {
    let res = (|| -> Result<&str> {
        if http_get(PORT, "/json/version").is_err() {
            return Ok("browser was not running");
        }
        let mut cdp = connect_browser_cdp()?;
        // Browser.close commonly closes the socket before replying; sending it is success.
        let _ = cdp.call("Browser.close", json!({}), None, Duration::from_secs(5));
        Ok("browser closed")
    })();
    clear_active();
    let receipt = match res {
        Ok(detail) => Receipt::new("browser-close").ok(json!({}), detail),
        Err(e) => Receipt::new("browser-close").fail(json!({}), &format!("{e:#}")),
    };
    receipt.print();
    Ok(receipt.exit_code())
}

// ---------- base64 ----------

pub fn b64_decode(s: &str) -> Result<Vec<u8>> {
    let table: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for c in s.bytes() {
        if c == b'=' || c == b'\n' || c == b'\r' {
            continue;
        }
        let v = table
            .iter()
            .position(|t| *t == c)
            .ok_or_else(|| anyhow!("bad base64 char {c}"))? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}
