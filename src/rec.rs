//! Screen recording: a detached child grabs frames while an agent works, and `rec stop`
//! stitches them into an mp4 with ffmpeg when one is available, or into a self-contained
//! HTML player when it is not. Frames are JPEG so a long session stays writable.

use anyhow::{bail, Result};
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
    pub started: String,
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

pub fn cmd_start(window: Option<String>, fps: u32, out: Option<String>, max_seconds: u64) -> Result<i32> {
    if let Some(st) = load() {
        bail!("a recording is already running: '{}' ({})", st.name, st.dir.display());
    }
    let fps = fps.clamp(1, 15);
    let name = out.clone().unwrap_or_else(|| format!("rec-{}", util::now_iso().replace([':', '-'], "")));
    let dir = util::ensure_data_dir().join("recordings").join(util::sanitize(&name));
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
    let st = RecState { name, dir, fps, quality: 62, window, pid, started: util::now_iso() };
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
    let _ = std::fs::write(st.dir.join("STOP"), "");
    let mut waited = 0;
    loop {
        if st.dir.join("captured.json").exists() || waited > 40 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
        waited += 1;
    }
    let frames = list_frames(&frames_dir(&st));
    let secs = st.dir.join("captured.json");
    let _ = std::fs::remove_file(&secs);
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
        write_player(&st.dir, frames.len(), st.fps)?;
    }
    if encoded == "mp4" && !keep_frames {
        let _ = std::fs::remove_dir_all(frames_dir(&st));
    }
    let r = util::Receipt::new("rec.stop").ok(
        serde_json::json!({
            "name": st.name, "frames": frames.len(), "fps": st.fps,
            "encoded": encoded, "video": if encoded == "mp4" { mp4.display().to_string() } else { String::new() },
            "dir": st.dir.display().to_string(),
        }),
        &detail,
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
                }))?
            );
        }
        None => println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "recording": false }))?),
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

/// Dependency-free fallback: a single HTML file that replays the JPEG frames.
fn write_player(dir: &Path, count: usize, fps: u32) -> Result<()> {
    let mut imgs = String::new();
    for i in 0..count {
        imgs.push_str(&format!("<img src=\"frames/frame_{:06}.jpg\" alt=\"\">\n", i));
    }
    let html = format!(
        r#"<!doctype html><meta charset="utf-8"><title>cua recording</title>
<style>body{{margin:0;background:#111;color:#ddd;font:14px system-ui;display:flex;flex-direction:column;height:100vh}}
#stage{{flex:1;display:grid;place-items:center;overflow:hidden}}img{{max-width:100%;max-height:100%;display:none}}
img.on{{display:block}}#bar{{padding:8px 12px;display:flex;gap:10px;align-items:center;background:#1b1b1b}}</style>
<div id="stage">{imgs}</div>
<div id="bar"><button id="pp">⏸</button><input id="rng" type="range" min="0" max="{last}" value="0">
<span id="lbl"></span><span>{count} frames @ {fps} fps</span></div>
<script>
const f=[...document.images];let i=0,t=null;const speed={fps};
function show(n){{i=(n+f.length)%f.length;f.forEach((e,k)=>e.classList.toggle('on',k===i));rng.value=i;lbl.textContent=`${{i+1}} / ${{f.length}}`;}}
function play(){{stop();t=setInterval(()=>show(i+1),1000/speed);pp.textContent='⏸';}}
function stop(){{if(t)clearInterval(t);t=null;pp.textContent='▶';}}
pp.onclick=()=>t?stop():play();rng.oninput=e=>{{stop();show(+e.target.value)}};
document.onkeydown=e=>{{if(e.key==='ArrowRight'){{stop();show(i+1)}}if(e.key==='ArrowLeft'){{stop();show(i-1)}}}};
show(0);play();
</script>"#,
        imgs = imgs,
        last = count.saturating_sub(1),
        count = count,
        fps = fps,
    );
    std::fs::write(dir.join("player.html"), html)?;
    Ok(())
}
