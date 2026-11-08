// perception/serve.rs — optional resident inference service.
//
// The CLI is one-shot, so every observation would otherwise reload OCR/YOLO/
// Florence. `cua perception serve` keeps them in memory and answers over a
// loopback socket; frames travel as a raw RGBA file path, never over the wire.
// Any failure falls back to the in-process path, so the service is an
// optimisation, never a dependency.
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use super::ocr::TextLine;
use super::ocr_backend::AnyOcr;
use super::types::OcrProfile;
use super::vision::{det_tile_budget, Captioner, Detector};
use super::fusion::DetItem;
use super::coordinates::Rect;

const PROTOCOL: u64 = 1;

fn state_path() -> PathBuf {
    crate::util::ensure_data_dir()
        .join("cache")
        .join("perception-serve.json")
}

#[derive(Clone, Debug)]
pub struct ServeState {
    pub port: u16,
    pub token: String,
}

fn read_state() -> Option<ServeState> {
    let bytes = std::fs::read(state_path()).ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    Some(ServeState {
        port: v["port"].as_u64()? as u16,
        token: v["token"].as_str()?.to_string(),
    })
}

/// On by default: the service is created on first use. `CUA_PERCEPTION_SERVE=0`
/// forces the purely in-process path.
pub fn enabled() -> bool {
    std::env::var("CUA_PERCEPTION_SERVE")
        .map(|v| v != "0")
        .unwrap_or(true)
}

fn call(state: &ServeState, payload: &Value, timeout: Duration) -> Result<Value> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], state.port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500))
        .map_err(|e| anyhow!("connect perception serve: {e}"))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut req = payload.clone();
    if let Some(o) = req.as_object_mut() {
        o.insert("token".into(), json!(state.token));
        o.insert("protocol".into(), json!(PROTOCOL));
    }
    let line = serde_json::to_string(&req)?;
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut reader = BufReader::new(stream);
    let mut resp = String::new();
    reader.read_line(&mut resp)?;
    let v: Value = serde_json::from_str(resp.trim())
        .map_err(|e| anyhow!("perception serve reply: {e}"))?;
    if v["ok"].as_bool().unwrap_or(false) {
        Ok(v)
    } else {
        bail!(
            "perception serve: {}",
            v["error"].as_str().unwrap_or("unknown error")
        )
    }
}

/// Connect to the running service, or spawn one and wait for it.
pub fn ensure() -> Result<ServeState> {
    if let Some(st) = read_state() {
        if call(&st, &json!({"op":"ping"}), Duration::from_millis(800)).is_ok() {
            return Ok(st);
        }
    }
    let exe = std::env::current_exe().context("current_exe")?;
    spawn_detached(&exe).context("spawn perception serve")?;
    let deadline = Instant::now() + Duration::from_secs(25);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
        if let Some(st) = read_state() {
            if call(&st, &json!({"op":"ping"}), Duration::from_millis(800)).is_ok() {
                return Ok(st);
            }
        }
    }
    bail!("perception serve did not become ready within 25s")
}

pub struct AnalyzeReply {
    pub ocr: Vec<TextLine>,
    pub det: Vec<DetItem>,
    pub ocr_backend: String,
    pub det_backend: String,
    pub ocr_ms: u64,
    pub det_ms: u64,
}

pub fn analyze(
    state: &ServeState,
    frame_path: &std::path::Path,
    width: u32,
    height: u32,
    profile: OcrProfile,
    tile_budget: usize,
    timeout: Duration,
) -> Result<AnalyzeReply> {
    let v = call(
        state,
        &json!({
            "op": "analyze",
            "frame": frame_path.to_string_lossy(),
            "width": width,
            "height": height,
            "profile": profile.as_str(),
            "tiles": tile_budget,
        }),
        timeout,
    )?;
    let ocr = v["ocr"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|o| {
                    let r = o["rect"].as_array()?;
                    Some(TextLine {
                        rect: Rect::new(
                            r.first()?.as_f64()? as f32,
                            r.get(1)?.as_f64()? as f32,
                            r.get(2)?.as_f64()? as f32,
                            r.get(3)?.as_f64()? as f32,
                        ),
                        text: o["text"].as_str()?.to_string(),
                        confidence: o["confidence"].as_f64().unwrap_or(0.0) as f32,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let det = v["det"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|o| {
                    let r = o["rect"].as_array()?;
                    Some(DetItem {
                        rect: Rect::new(
                            r.first()?.as_f64()? as f32,
                            r.get(1)?.as_f64()? as f32,
                            r.get(2)?.as_f64()? as f32,
                            r.get(3)?.as_f64()? as f32,
                        ),
                        score: o["score"].as_f64().unwrap_or(0.0) as f32,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(AnalyzeReply {
        ocr,
        det,
        ocr_backend: v["ocr_backend"].as_str().unwrap_or("serve").to_string(),
        det_backend: v["det_backend"].as_str().unwrap_or("serve").to_string(),
        ocr_ms: v["ocr_ms"].as_u64().unwrap_or(0),
        det_ms: v["det_ms"].as_u64().unwrap_or(0),
    })
}

pub struct CaptionReply {
    pub texts: Vec<String>,
    pub backend: String,
}

pub fn caption(
    state: &ServeState,
    crops: &[(u32, u32, Vec<u8>)],
    timeout: Duration,
) -> Result<CaptionReply> {
    let items: Vec<Value> = crops
        .iter()
        .map(|(w, h, rgba)| json!({"w": w, "h": h, "hex": hex::encode(rgba)}))
        .collect();
    let v = call(state, &json!({"op":"caption","crops":items}), timeout)?;
    let texts = v["texts"]
        .as_array()
        .map(|a| a.iter().map(|t| t.as_str().unwrap_or("").to_string()).collect())
        .unwrap_or_default();
    Ok(CaptionReply {
        texts,
        backend: v["backend"].as_str().unwrap_or("serve").to_string(),
    })
}

/// Spawn the service with `bInheritHandles = FALSE`: with inherited handles the
/// child keeps the caller's stdout pipe open (Rust's Command inherits the whole
/// handle table when stdio is redirected), so a parent that captures our output
/// would wait forever for EOF.
#[cfg(windows)]
fn spawn_detached(exe: &std::path::Path) -> Result<()> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        CreateProcessW, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
        STARTUPINFOW,
    };
    let mut cmd: Vec<u16> = format!("\"{}\" perception serve --quiet", exe.display())
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            windows::core::PWSTR(cmd.as_mut_ptr()),
            None,
            None,
            false,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            None,
            PCWSTR::null(),
            &si,
            &mut pi,
        )
        .map_err(|e| anyhow!("CreateProcessW serve: {e}"))?;
        let _ = CloseHandle(pi.hThread);
        let _ = CloseHandle(pi.hProcess);
    }
    Ok(())
}

#[cfg(not(windows))]
fn spawn_detached(exe: &std::path::Path) -> Result<()> {
    std::process::Command::new(exe)
        .args(["perception", "serve", "--quiet"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("spawn perception serve")?;
    Ok(())
}

/// Ask a running service to exit. Missing service is not an error.
pub fn stop() -> Result<bool> {
    let Some(st) = read_state() else {
        return Ok(false);
    };
    let _ = call(&st, &json!({"op":"shutdown"}), Duration::from_millis(800));
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if read_state().is_none() {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(false)
}

// ---------------- server ----------------

#[derive(Default)]
struct Engines {
    ocr: Option<(String, AnyOcr)>,
    det: Option<Detector>,
    cap: Option<Captioner>,
}

fn serve_error(msg: impl std::fmt::Display) -> Value {
    json!({"ok": false, "error": msg.to_string()})
}

fn handle(stream: TcpStream, token: &str, engines: &mut Engines) -> Result<bool> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let req: Value = match serde_json::from_str(line.trim()) {
        Ok(v) => v,
        Err(e) => {
            let mut s = stream;
            let _ = writeln!(s, "{}", serve_error(format!("bad request: {e}")));
            return Ok(false);
        }
    };
    let reply = if req["token"].as_str() != Some(token) {
        serve_error("unauthorized")
    } else {
        dispatch(&req, engines)
    };
    if !reply["ok"].as_bool().unwrap_or(false) && reply["error"].is_null() {
        return Ok(false);
    }
    let shutdown = req["op"].as_str() == Some("shutdown");
    let mut s = stream;
    let _ = writeln!(s, "{}", reply);
    Ok(shutdown)
}

fn dispatch(req: &Value, engines: &mut Engines) -> Value {
    match req["op"].as_str().unwrap_or("") {
        "ping" => json!({
            "ok": true,
            "protocol": PROTOCOL,
            "pid": std::process::id(),
            "ocr_loaded": engines.ocr.is_some(),
            "detector_loaded": engines.det.is_some(),
            "caption_loaded": engines.cap.is_some(),
        }),
        "analyze" => match analyze_frame(req, engines) {
            Ok(v) => v,
            Err(e) => serve_error(format!("{e:#}")),
        },
        "caption" => match caption_crops(req, engines) {
            Ok(v) => v,
            Err(e) => serve_error(format!("{e:#}")),
        },
        other => serve_error(format!("unknown op '{other}'")),
    }
}

fn analyze_frame(req: &Value, engines: &mut Engines) -> Result<Value> {
    let path = req["frame"]
        .as_str()
        .ok_or_else(|| anyhow!("missing frame path"))?;
    let width = req["width"].as_u64().unwrap_or(0) as u32;
    let height = req["height"].as_u64().unwrap_or(0) as u32;
    let profile = req["profile"].as_str().unwrap_or("paddleocr");
    let tiles = req["tiles"].as_u64().unwrap_or(0) as usize;
    let want = OcrProfile::PaddleOcr;
    let want = match profile {
        "easyocr-en-reference" => OcrProfile::EasyocrEnReference,
        "ppocr-zh-desktop" => OcrProfile::PpocrZhDesktop,
        _ => want,
    };
    let rgba = std::fs::read(path).with_context(|| format!("read frame {path}"))?;
    if rgba.len() < (width as usize * height as usize * 4) {
        bail!("frame {path} is shorter than {width}x{height} RGBA");
    }
    if engines.ocr.as_ref().map(|(p, _)| p.as_str()) != Some(want.as_str()) {
        engines.ocr = Some((want.as_str().to_string(), AnyOcr::load(want)?));
    }
    if engines.det.is_none() {
        engines.det = Some(Detector::load()?);
    }
    let Engines { ocr, det, .. } = engines;
    let ocr_engine = ocr.as_mut().map(|(_, e)| e).unwrap();
    let detector = det.as_mut().unwrap();
    let (ocr_res, det_res) = std::thread::scope(|s| {
        let o = s.spawn(|| {
            let t = Instant::now();
            let r = ocr_engine.recognize(width, height, &rgba);
            (r, t.elapsed().as_millis() as u64)
        });
        let d = s.spawn(|| {
            let t = Instant::now();
            let r = detector.detect_tiled(width, height, &rgba, tiles);
            (r, t.elapsed().as_millis() as u64)
        });
        (
            o.join().unwrap_or_else(|_| (Err(anyhow!("ocr panicked")), 0)),
            d.join().unwrap_or_else(|_| (Err(anyhow!("detector panicked")), 0)),
        )
    });
    let (ocr_lines, ocr_ms) = match ocr_res {
        (Ok(lines), ms) => (lines, ms),
        (Err(e), _) => return Err(e),
    };
    let (det_items, det_ms) = match det_res {
        (Ok(items), ms) => (items, ms),
        (Err(e), _) => return Err(e),
    };
    let ocr_backend = ocr.as_ref().map(|(_, e)| e.backend()).unwrap_or("serve");
    let det_backend = det.as_ref().map(|d| d.backend()).unwrap_or("serve");
    Ok(json!({
        "ok": true,
        "ocr_backend": ocr_backend,
        "det_backend": det_backend,
        "ocr_ms": ocr_ms,
        "det_ms": det_ms,
        "ocr": ocr_lines.iter().map(|l| json!({
            "rect": [l.rect.x, l.rect.y, l.rect.w, l.rect.h],
            "text": l.text,
            "confidence": l.confidence,
        })).collect::<Vec<_>>(),
        "det": det_items.iter().map(|d| json!({
            "rect": [d.rect.x, d.rect.y, d.rect.w, d.rect.h],
            "score": d.score,
        })).collect::<Vec<_>>(),
    }))
}

#[derive(Deserialize)]
struct CropIn {
    w: u32,
    h: u32,
    hex: String,
}

fn caption_crops(req: &Value, engines: &mut Engines) -> Result<Value> {
    let crops: Vec<CropIn> = serde_json::from_value(req["crops"].clone())
        .context("crops must be [{w,h,hex}]")?;
    let parsed: Vec<(u32, u32, Vec<u8>)> = crops
        .into_iter()
        .map(|c| {
            let bytes = hex::decode(&c.hex).context("crop hex")?;
            Ok((c.w, c.h, bytes))
        })
        .collect::<Result<_>>()?;
    if engines.cap.is_none() {
        engines.cap = Some(Captioner::load_for(true)?);
    }
    let cap = engines.cap.as_mut().unwrap();
    let t = Instant::now();
    let texts = cap.caption_batch(&parsed)?;
    Ok(json!({
        "ok": true,
        "backend": cap.backend_name(),
        "batch": cap.batch_size(),
        "elapsed_ms": t.elapsed().as_millis() as u64,
        "texts": texts,
    }))
}

/// Long-running entry point. Idle services exit after `idle`.
pub fn serve(idle: Duration, quiet: bool) -> Result<i32> {
    // The service outlives the CLI that spawned it. Close the inherited std
    // handles so a redirected stdout (a pipe in tests/parents) gets EOF and the
    // caller never waits on a handle this process still holds.
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Console::{
            GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        };
        for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            if let Ok(handle) = GetStdHandle(which) {
                if !handle.is_invalid() {
                    let _ = windows::Win32::Foundation::CloseHandle(handle);
                }
            }
        }
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).context("bind 127.0.0.1")?;
    let port = listener.local_addr()?.port();
    let token = crate::util::new_state_id();
    let state = json!({
        "schema": "cua-perception-serve-v1",
        "protocol": PROTOCOL,
        "port": port,
        "token": token,
        "pid": std::process::id(),
        "started": crate::util::now_iso(),
    });
    std::fs::write(state_path(), serde_json::to_vec(&state)?)?;
    if !quiet {
        eprintln!("perception serve listening on 127.0.0.1:{port} (pid {})", std::process::id());
    }
    listener.set_nonblocking(true)?;
    let mut engines = Engines::default();
    let mut last = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                last = Instant::now();
                // A nonblocking listener can hand back nonblocking sockets; the
                // request handler must block for the line it expects.
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
                match handle(stream, &token, &mut engines) {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(e) => {
                        if !quiet {
                            eprintln!("serve request failed: {e:#}");
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if last.elapsed() > idle {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e).context("accept"),
        }
    }
    let _ = std::fs::remove_file(state_path());
    Ok(0)
}

/// Frame dump the service reads. Deleted by the caller after the reply.
pub fn write_frame(dir: &std::path::Path, id: &str, rgba: &[u8]) -> Result<PathBuf> {
    let path = dir.join(format!("frame-{id}.rgba"));
    std::fs::write(&path, rgba).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

pub fn tile_budget(detail: super::types::Detail) -> usize {
    det_tile_budget(detail)
}
