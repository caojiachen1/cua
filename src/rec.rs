//! Screen recording: a detached child grabs frames while an agent works, and `rec stop`
//! stitches them into an mp4 with ffmpeg when one is available, or into a self-contained
//! HTML player when it is not. Frames are JPEG so a long session stays writable.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::capture;
use crate::util;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RecState {
    pub name: String,
    pub dir: PathBuf,
    pub fps: u32,
    pub quality: u8,
    pub window: Option<String>,
    pub pid: u32,
    #[serde(default)]
    pub created: Option<u64>,
    pub started: String,
}

fn worker_alive(st: &RecState) -> bool {
    match st.created {
        Some(c) => crate::win::process_creation_time(st.pid) == Some(c),
        None => crate::win::process_name(st.pid)
            .map(|n| n.eq_ignore_ascii_case("cua.exe"))
            .unwrap_or(false),
    }
}

fn state_path() -> PathBuf {
    util::ensure_data_dir().join("recording.json")
}

fn load() -> Option<RecState> {
    let b = std::fs::read(state_path()).ok()?;
    let b = b.strip_prefix(b"\xEF\xBB\xBF".as_slice()).unwrap_or(&b).to_vec();
    serde_json::from_slice(&b).ok()
}

fn frames_dir(st: &RecState) -> PathBuf {
    st.dir.join("frames")
}

fn find_ffmpeg() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CUA_FFMPEG") {
        let pb = PathBuf::from(&p);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Some(p) = util::load_config_json().get("ffmpeg_path").and_then(|v| v.as_str()) {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    let out = std::process::Command::new("where").args(["ffmpeg"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| PathBuf::from(l.trim()))
        .find(|p| p.exists())
}

/// Spawn the capture loop truly detached. `std::process::Command` passes
/// `bInheritHandles=TRUE`, so the child duplicates the caller's *inheritable* stdout pipe
/// even when its own std handles point at NUL — which makes `powershell &cua rec start`
/// block until the recorder exits. CreateProcess with handle inheritance off fixes it.
fn spawn_detached(exe: &Path, args: &[String]) -> Result<u32> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            CreateProcessW, DETACHED_PROCESS, PROCESS_INFORMATION, STARTUPINFOW,
        };
        let mut cmd: Vec<u16> = format!("\"{}\" {}", exe.display(), args.join(" "))
            .encode_utf16()
            .collect();
        cmd.push(0);
        let mut si = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
        let mut pi = PROCESS_INFORMATION::default();
        unsafe {
            CreateProcessW(
                PCWSTR::null(),
                windows::core::PWSTR(cmd.as_mut_ptr()),
                None,
                None,
                false, // no handle inheritance
                DETACHED_PROCESS,
                None,
                PCWSTR::null(),
                &mut si,
                &mut pi,
            )
            .map_err(|e| anyhow::anyhow!("CreateProcessW(rec capture): {e}"))?;
            let pid = pi.dwProcessId;
            let _ = CloseHandle(pi.hThread);
            let _ = CloseHandle(pi.hProcess);
            return Ok(pid);
        }
    }
    #[cfg(not(windows))]
    {
        let child = std::process::Command::new(exe)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        Ok(child.id())
    }
}

/// Every start gets its own directory. Reusing one by display name let a
/// shorter re-recording inherit the previous run's tail frames.
fn session_dir_name(name: &str) -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let stamp: String = util::now_iso()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    format!("{}-{}-{}-{}", util::sanitize(name), stamp, std::process::id(), seq)
}

pub fn cmd_start(window: Option<String>, fps: u32, out: Option<String>, max_seconds: u64) -> Result<i32> {
    if let Some(st) = load() {
        if worker_alive(&st) {
            bail!("a recording is already running: '{}' ({})", st.name, st.dir.display());
        }
        // A crashed/exited worker must not block every future recording.
        let _ = std::fs::remove_file(state_path());
    }
    let fps = fps.clamp(1, 15);
    let name = out.clone().unwrap_or_else(|| format!("rec-{}", util::now_iso().replace([':', '-'], "")));
    let dir = util::ensure_data_dir()
        .join("recordings")
        .join(session_dir_name(&name));
    std::fs::create_dir_all(dir.join("frames"))?;
    let exe = std::env::current_exe()?;
    let mut args: Vec<String> = vec![
        "rec".into(), "capture".into(),
        "--dir".into(), format!("\"{}\"", dir.display()),
        "--fps".into(), fps.to_string(),
        "--max-seconds".into(), max_seconds.to_string(),
    ];
    if let Some(w) = &window {
        args.push("--window".into());
        args.push(format!("\"{w}\""));
    }
    let pid = spawn_detached(&exe, &args)?;
    let created = crate::win::process_creation_time(pid);
    let st = RecState { name, dir, fps, quality: 62, window, pid, created, started: util::now_iso() };
    let _ = std::fs::write(state_path(), serde_json::to_vec(&st)?);
    let r = util::Receipt::new("rec.start").ok(
        serde_json::json!({ "name": st.name, "fps": st.fps, "target": st.window.clone().unwrap_or_else(|| "screen".into()) }),
        &format!("recording to {} at {} fps", frames_dir(&st).display(), st.fps),
    );
    r.print();
    Ok(0)
}

/// Internal: the detached capture loop. Runs until STOP appears or max-seconds elapses.
pub fn cmd_capture(dir: &str, fps: u32, window: Option<String>, max_seconds: u64) -> Result<i32> {
    let dir = PathBuf::from(dir);
    let frames = dir.join("frames");
    std::fs::create_dir_all(&frames)?;
    let stop = dir.join("STOP");
    let started = std::time::Instant::now();
    let mut i = 0u64;
    let mut n = 0u64;
    let tick = (1000 / fps.max(1)) as u64;
    loop {
        if stop.exists() || started.elapsed().as_secs() >= max_seconds {
            break;
        }
        let shot = match &window {
            Some(sel) => match crate::resolve_target(sel) {
                Ok(t) => capture::shot_window(t.hwnd, false),
                Err(e) => Err(e.into()),
            },
            None => capture::shot_screen(true),
        };
        if let Ok(s) = shot {
            if s.rendered {
                let p = frames.join(format!("frame_{:06}.jpg", i));
                if capture::save_jpeg(&s, &p, 62).is_ok() {
                    i += 1;
                }
            }
        }
        // Pace on ticks attempted, not frames written, so a failed capture cannot spin the CPU.
        n += 1;
        let ahead = n * tick;
        let elapsed = started.elapsed().as_millis() as u64;
        if ahead > elapsed {
            std::thread::sleep(std::time::Duration::from_millis(ahead - elapsed));
        }
    }
    let _ = std::fs::write(dir.join("captured.json"), format!("{{\"frames\":{i}}}"));
    let _ = std::fs::remove_file(&stop);
    Ok(0)
}

pub fn cmd_stop(out: Option<String>, keep_frames: bool) -> Result<i32> {
    let Some(st) = load() else {
        let r = util::Receipt::new("rec.stop").fail(serde_json::json!({}), "no recording is running");
        r.print();
        return Ok(1);
    };
    let alive = worker_alive(&st);
    let done = st.dir.join("captured.json");
    if alive {
        if let Err(e) = std::fs::write(st.dir.join("STOP"), "") {
            let r = util::Receipt::new("rec.stop").fail(
                serde_json::json!({ "dir": st.dir.display().to_string() }),
                &format!("could not signal the capture worker to stop ({e}); recording state kept"),
            );
            r.print();
            return Ok(1);
        }
    }
    let mut confirmed = done.exists() || !alive;
    for _ in 0..40 {
        if confirmed {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
        confirmed = done.exists();
    }
    if !confirmed {
        // The worker may still be capturing (e.g. blocked in PrintWindow). Keep
        // the state and frames so a later stop can finish instead of silently
        // reporting a clean stop and losing control of the recorder.
        let r = util::Receipt::new("rec.stop").fail(
            serde_json::json!({
                "dir": st.dir.display().to_string(),
                "pid": st.pid,
                "name": st.name,
            }),
            "capture worker did not confirm stop within 10s; recording state and frames were kept",
        );
        r.print();
        return Ok(1);
    }
    let frames = list_frames(&frames_dir(&st));
    let _ = std::fs::remove_file(&done);
    let _ = std::fs::remove_file(state_path());
    if frames.is_empty() {
        let r = util::Receipt::new("rec.stop").fail(
            serde_json::json!({ "dir": st.dir.display().to_string() }),
            "no frames were captured (the target window may refuse capture)");
        r.print();
        return Ok(1);
    }
    let mp4 = match &out {
        Some(o) => PathBuf::from(o),
        None => st.dir.join(format!("{}.mp4", st.name)),
    };
    let (encoded, detail) = match find_ffmpeg() {
        Some(ff) => match encode(&ff, &frames_dir(&st), &mp4, st.fps) {
            Ok(size) => (
                "mp4".to_string(),
                format!("{} frames -> {} ({} KB, {} fps)", frames.len(), mp4.display(), size / 1024, st.fps),
            ),
            Err(e) => (
                "frames".to_string(),
                format!("{} frames; ffmpeg failed: {}", frames.len(), e),
            ),
        },
        None => (
            "html".to_string(),
            format!("{} frames; no ffmpeg on PATH (set $CUA_UBB's sibling CUA_FFMPEG or config ffmpeg_path) -> HTML player", frames.len()),
        ),
    };
    if encoded != "mp4" {
        write_player(&st.dir, &frames, st.fps)?;
    }
    if encoded == "mp4" && !keep_frames {
        let _ = std::fs::remove_dir_all(frames_dir(&st));
    }
    let r = util::Receipt::new("rec.stop").ok(
        serde_json::json!({
            "name": st.name, "frames": frames.len(), "fps": st.fps,
            "encoded": encoded, "video": if encoded == "mp4" { mp4.display().to_string() } else { String::new() },
            "dir": st.dir.display().to_string(),
            "worker_alive_before_stop": alive,
        }),
        &detail,
    );
    r.print();
    Ok(0)
}

fn kill_worker(st: &RecState) -> bool {
    match st.created {
        Some(c) if crate::win::process_creation_time(st.pid) == Some(c) => {
            crate::win::kill_process_tree(st.pid) > 0
        }
        None => crate::win::process_name(st.pid)
            .map(|n| n.eq_ignore_ascii_case("cua.exe"))
            .unwrap_or(false)
            && crate::win::kill_process_tree(st.pid) > 0,
        _ => false,
    }
}

/// Explicit escape hatch for `rec stop` timing out: kill the recorder only when
/// its creation time still matches, then clear state and keep the frames.
pub fn cmd_cancel() -> Result<i32> {
    let Some(st) = load() else {
        let r = util::Receipt::new("rec.cancel").fail(serde_json::json!({}), "no recording is running");
        r.print();
        return Ok(1);
    };
    let was_alive = worker_alive(&st);
    let killed = if was_alive { kill_worker(&st) } else { false };
    let frames = list_frames(&frames_dir(&st)).len();
    let _ = std::fs::remove_file(state_path());
    let r = util::Receipt::new("rec.cancel").ok(
        serde_json::json!({
            "name": st.name, "dir": st.dir.display().to_string(), "frames": frames,
            "worker_was_alive": was_alive, "worker_pid": st.pid, "killed": killed,
        }),
        &format!(
            "recording cancelled (worker alive={was_alive}, killed={killed}); {frames} frames kept in {}",
            st.dir.display()
        ),
    );
    r.print();
    Ok(0)
}

pub fn cmd_status() -> Result<i32> {
    match load() {
        Some(st) => {
            let n = list_frames(&frames_dir(&st)).len();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "recording": true, "name": st.name, "frames": n, "fps": st.fps,
                    "target": st.window.clone().unwrap_or_else(|| "screen".into()),
                    "started": st.started, "dir": st.dir.display().to_string(),
                    "worker_alive": worker_alive(&st), "worker_pid": st.pid,
                }))?
            );
        }
        None => println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "recording": false, "worker_alive": false }))?),
    }
    Ok(0)
}

fn list_frames(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "jpg").unwrap_or(false)).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn encode(ff: &Path, frames: &Path, out: &Path, fps: u32) -> Result<u64> {
    if let Some(p) = out.parent() {
        std::fs::create_dir_all(p)?;
    }
    let status = std::process::Command::new(ff)
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(["-nostdin", "-framerate", &fps.to_string(), "-i", "frame_%06d.jpg"])
        .args(["-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p", "-movflags", "+faststart"])
        .arg(out)
        .current_dir(frames)
        .stdin(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        bail!("ffmpeg exited with {status}");
    }
    Ok(std::fs::metadata(out).map(|m| m.len()).unwrap_or(0))
}

/// Dependency-free fallback: a single HTML file that replays the JPEG frames. Frames are
/// embedded as data URLs (bounded) so the artifact stands alone and stays reviewable; a
/// multi-hour recording that would blow the budget falls back to file references.
fn write_player(dir: &Path, frames: &[PathBuf], fps: u32) -> Result<()> {
    const EMBED_BUDGET: u64 = 48 * 1024 * 1024; // base64 inflates ~4/3; keep the html sane
    let total: u64 = frames.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
    let embed = total <= EMBED_BUDGET;
    let mut body = String::new();
    for (i, p) in frames.iter().enumerate() {
        let src = if embed {
            let bytes = std::fs::read(p)
                .with_context(|| format!("reading frame {}", p.display()))?;
            if bytes.is_empty() {
                bail!("frame {} is empty; refusing to write a player with a blank frame", p.display());
            }
            format!("data:image/jpeg;base64,{}", b64_encode(&bytes))
        } else {
            format!("frames/{}", p.file_name().unwrap_or_default().to_string_lossy())
        };
        body.push_str(&format!("<img src=\"{src}\" alt=\"\">\n"));
        let _ = i;
    }
    let count = frames.len();
    let html = format!(
        r#"<!doctype html><meta charset="utf-8"><title>cua recording</title>
<style>body{{margin:0;background:#111;color:#ddd;font:14px system-ui;display:flex;flex-direction:column;height:100vh}}
#stage{{flex:1;display:grid;place-items:center;overflow:hidden}}img{{max-width:100%;max-height:100%;display:none}}
img.on{{display:block}}#bar{{padding:8px 12px;display:flex;gap:10px;align-items:center;background:#1b1b1b}}</style>
<div id="stage">{body}</div>
<div id="bar"><button id="pp">⏸</button><input id="rng" type="range" min="0" max="{last}" value="0">
<span id="lbl"></span><span>{count} frames @ {fps} fps{mode}</span></div>
<script>
const f=[...document.images];let i=0,t=null;const speed={fps};
function show(n){{i=(n+f.length)%f.length;f.forEach((e,k)=>e.classList.toggle('on',k===i));rng.value=i;lbl.textContent=`${{i+1}} / ${{f.length}}`;}}
function play(){{stop();t=setInterval(()=>show(i+1),1000/speed);pp.textContent='⏸';}}
function stop(){{if(t)clearInterval(t);t=null;pp.textContent='▶';}}
pp.onclick=()=>t?stop():play();rng.oninput=e=>{{stop();show(+e.target.value)}};
document.onkeydown=e=>{{if(e.key==='ArrowRight'){{stop();show(i+1)}}if(e.key==='ArrowLeft'){{stop();show(i-1)}}}};
show(0);play();
</script>"#,
        body = body,
        last = count.saturating_sub(1),
        count = count,
        fps = fps,
        mode = if embed { " (embedded)" } else { "" },
    );
    std::fs::write(dir.join("player.html"), html)?;
    Ok(())
}

/// Small base64 (RFC 4648) encoder so the player needs no external crates.
fn b64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_session_gets_a_distinct_directory() {
        let a = session_dir_name("demo");
        let b = session_dir_name("demo");
        assert_ne!(a, b, "same display name must not share a frame directory");
        assert!(a.starts_with("demo-"));
        assert!(!a.contains(['/', ':']));
        assert!(!a.contains('\\'));
    }

    #[test]
    fn base64_round_trips_known_bytes() {
        assert_eq!(b64_encode(b""), "");
        assert_eq!(b64_encode(b"f"), "Zg==");
        assert_eq!(b64_encode(b"fo"), "Zm8=");
        assert_eq!(b64_encode(b"foo"), "Zm9v");
        assert_eq!(b64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(b64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
    }
}
