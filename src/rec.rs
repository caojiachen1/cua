//! Screen recording: a detached child grabs frames while an agent works, and `rec stop`
//! stitches them into an mp4 with ffmpeg when one is available, or into a self-contained
//! HTML player when it is not. Frames are JPEG so a long session stays writable.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::os::windows::ffi::OsStrExt as _;
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

pub fn cmd_start(window: Option<String>, fps: u32, out: Option<String>, max_seconds: u64, quality: Option<u8>) -> Result<i32> {
    if let Some(st) = load() {
        if worker_alive(&st) {
            bail!("a recording is already running: '{}' ({})", st.name, st.dir.display());
        }
        // A crashed/exited worker must not block every future recording.
        let _ = std::fs::remove_file(state_path());
    }
    // CLI flags win; otherwise the settings app's rec_fps / rec_quality defaults
    // apply, then the built-in 2 fps / 80 quality.
    let cfg = crate::util::load_config_json();
    let cfg_fps = cfg.get("rec_fps").and_then(|v| v.as_u64()).map(|f| f as u32);
    let cfg_quality = cfg.get("rec_quality").and_then(|v| v.as_u64()).map(|q| q as u8);
    let fps = cfg_fps.unwrap_or(fps).clamp(1, 30);
    let quality = quality.or(cfg_quality).unwrap_or(80).clamp(1, 100);
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
        "--quality".into(), quality.to_string(),
    ];
    if let Some(w) = &window {
        args.push("--window".into());
        args.push(format!("\"{w}\""));
    }
    let pid = spawn_detached(&exe, &args)?;
    let created = crate::win::process_creation_time(pid);
    let st = RecState { name, dir, fps, quality, window, pid, created, started: util::now_iso() };
    let _ = std::fs::write(state_path(), serde_json::to_vec(&st)?);
    let r = util::Receipt::new("rec.start").ok(
        serde_json::json!({ "name": st.name, "fps": st.fps, "target": st.window.clone().unwrap_or_else(|| "screen".into()) }),
        &format!("recording to {} at {} fps", frames_dir(&st).display(), st.fps),
    );
    r.print();
    Ok(0)
}

/// Internal: the detached capture loop. Runs until STOP appears or max-seconds elapses.
///
/// Screen captures take the fast path (reused GDI objects, cursor baked in,
/// GDI+ encode on a worker thread) and record a wall-clock offset per tick so
/// `rec stop` can build a variable-frame-rate timeline: a capture slower than
/// the requested fps must still play back at the pace the user actually saw,
/// which is the whole point of a recording. Ticks whose frame is
/// byte-identical to the previous one are written as "repeats" (no new JPEG);
/// the timeline extends that frame's duration, so idle stretches cost no disk
/// yet stay time-accurate.
pub fn cmd_capture(dir: &str, fps: u32, window: Option<String>, max_seconds: u64, quality: u8) -> Result<i32> {
    let dir = PathBuf::from(dir);
    let frames = dir.join("frames");
    std::fs::create_dir_all(&frames)?;
    let stop = dir.join("STOP");
    let started = std::time::Instant::now();
    let tick = (1000 / fps.max(1)) as u64;

    // only the full-screen path gets the reusable-GDI pipeline; window targets
    // keep the per-frame PrintWindow capture (each target may come and go)
    let mut grabber = if window.is_none() { Some(capture::ScreenGrabber::new(true)?) } else { None };
    let (w, h) = grabber.as_ref().map(|g| g.size()).unwrap_or((0, 0));

    // two encoder workers round-robined by frame index: one GDI+ encode takes
    // ~40-60 ms at 2.5K, so a single worker caps the sustained rate at ~15 fps
    // no matter how fast the capture thread grabs
    let mut encoders = Vec::new();
    let mut enc_txs = Vec::new();
    for k in 0..2usize {
        let (tx, rx) = std::sync::mpsc::sync_channel::<(std::sync::Arc<Vec<u8>>, u64)>(8);
        enc_txs.push(tx);
        let frames = frames.clone();
        let jpeg = std::sync::Arc::new(capture::GdiJpeg::new()?);
        encoders.push(std::thread::spawn(move || {
            let mut ok = 0u64;
            for (buf, idx) in rx {
                let p = frames.join(format!("frame_{:06}.jpg", idx));
                if jpeg.save(&buf, w, h, quality, &p).is_ok() {
                    ok += 1;
                }
            }
            ok
        }));
    }

    let mut prev: Option<std::sync::Arc<Vec<u8>>> = None;
    let mut i = 0u64; // encoded frames
    let mut n = 0u64; // ticks
    let mut tsv = std::io::BufWriter::new(std::fs::File::create(dir.join("timestamps.tsv"))?);
    let mut flushed = 0u64;
    loop {
        if stop.exists() || started.elapsed().as_secs() >= max_seconds {
            break;
        }
        n += 1;
        let ms = started.elapsed().as_millis() as u64;
        if let Some(g) = grabber.as_mut() {
            match g.grab(true) {
                Ok(scan) => {
                    if prev.as_ref().map_or(false, |p| p.as_slice() == scan) {
                        // identical screen: extend the previous frame instead
                        // of writing a duplicate JPEG
                        let _ = writeln!(tsv, "\t{ms}");
                    } else {
                        let arc = std::sync::Arc::new(scan.to_vec());
                        // round-robin the two encode workers; if a worker died
                        // the frame cannot be written — record a repeat so the
                        // timeline stays honest
                        let sent = enc_txs[(i % 2) as usize].send((arc.clone(), i)).is_ok();
                        if sent {
                            let _ = writeln!(tsv, "{}\t{ms}", i);
                            i += 1;
                        } else {
                            let _ = writeln!(tsv, "\t{ms}");
                        }
                        prev = Some(arc);
                    }
                }
                Err(_) => {
                    let _ = writeln!(tsv, "\t{ms}");
                }
            }
        } else {
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
                    if capture::save_jpeg(&s, &p, quality).is_ok() {
                        let _ = writeln!(tsv, "{}\t{ms}", i);
                        i += 1;
                    } else {
                        let _ = writeln!(tsv, "\t{ms}");
                    }
                } else {
                    let _ = writeln!(tsv, "\t{ms}");
                }
            } else {
                let _ = writeln!(tsv, "\t{ms}");
            }
        }
        if n - flushed >= 16 {
            // a killed worker still leaves usable timeline data
            let _ = tsv.flush();
            flushed = n;
        }
        // Pace on ticks attempted, not frames written, so a failed capture cannot spin the CPU.
        let ahead = n * tick;
        let elapsed = started.elapsed().as_millis() as u64;
        if ahead > elapsed {
            std::thread::sleep(std::time::Duration::from_millis(ahead - elapsed));
        }
    }
    let _ = tsv.flush();
    drop(enc_txs);
    let encoded: u64 = encoders.into_iter().map(|h| h.join().unwrap_or(0)).sum();
    let _ = std::fs::write(
        dir.join("captured.json"),
        format!(
            "{{\"frames\":{encoded},\"ticks\":{n},\"duration_ms\":{},\"width\":{w},\"height\":{h}}}",
            started.elapsed().as_millis()
        ),
    );
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
    let timeline = read_timeline(&st.dir, st.fps);
    let mp4 = match &out {
        Some(o) => PathBuf::from(o),
        None => st.dir.join(format!("{}.mp4", st.name)),
    };
    // built-in H.264 first: full resolution, real wall-clock timeline, no
    // ffmpeg needed. ffmpeg and the HTML player stay as fallbacks.
    let (encoded, detail) = match unsafe { encode_mf(&mp4, &st.dir, &timeline) } {
        Ok(()) => (
            "mp4".to_string(),
            format!(
                "{} frames -> {} ({} KB, wall-clock {}.{:03}s, Media Foundation H.264)",
                frames.len(), mp4.display(),
                std::fs::metadata(&mp4).map(|m| m.len() / 1024).unwrap_or(0),
                timeline.total_ms / 1000, timeline.total_ms % 1000
            ),
        ),
        Err(mf_err) => match find_ffmpeg() {
            Some(ff) => match encode(&ff, &st.dir, &mp4, &timeline) {
                Ok(size) => (
                    "mp4".to_string(),
                    format!(
                        "{} frames -> {} ({} KB, wall-clock {}.{:03}s)",
                        frames.len(), mp4.display(), size / 1024,
                        timeline.total_ms / 1000, timeline.total_ms % 1000
                    ),
                ),
                Err(e) => (
                    "frames".to_string(),
                    format!("{} frames; Media Foundation: {mf_err}; ffmpeg failed: {e}", frames.len()),
                ),
            },
            None => (
                "html".to_string(),
                format!(
                    "{} frames; Media Foundation unavailable ({mf_err}), no ffmpeg on PATH -> HTML player",
                    frames.len()
                ),
            ),
        },
    };
    if encoded != "mp4" {
        write_player(&st.dir, &frames, &timeline, st.fps)?;
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

/// Playback timeline built from the capture loop's wall-clock ticks: one cue
/// per encoded frame with the duration it must stay on screen. Captures
/// slower than the requested fps and idle stretches (identical-frame ticks)
/// are both faithfully represented — the artifact plays back at the pace the
/// user actually saw, not at a nominal fps.
struct Timeline {
    /// (frame file name, on-screen seconds)
    cues: Vec<(String, f64)>,
    /// total wall-clock duration of the capture session
    total_ms: u64,
}

fn read_timeline(dir: &Path, fps: u32) -> Timeline {
    let meta = std::fs::read(dir.join("captured.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    let dur_ms = meta
        .as_ref()
        .and_then(|v| v.get("duration_ms").and_then(|x| x.as_u64()))
        .unwrap_or(0);
    let entries: Vec<(Option<u64>, u64)> = std::fs::read(dir.join("timestamps.tsv"))
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let (a, b) = l.split_once('\t')?;
                    let idx = if a.is_empty() { None } else { a.parse().ok() };
                    Some((idx, b.trim().parse::<u64>().ok()?))
                })
                .collect()
        })
        .unwrap_or_default();
    if entries.is_empty() {
        return Timeline { cues: Vec::new(), total_ms: dur_ms };
    }
    // one cue per encoded frame, in tick order (frame idx appears exactly once)
    let cues: Vec<(u64, u64)> = entries.iter().filter_map(|(idx, ms)| idx.map(|k| (k, *ms))).collect();
    let total = if dur_ms > 0 { dur_ms } else { entries.last().map(|e| e.1).unwrap_or(0) };
    let mut out = Vec::new();
    for k in 0..cues.len() {
        let start = cues[k].1;
        let end = cues.get(k + 1).map(|c| c.1).unwrap_or(total.max(start + 1));
        out.push((format!("frame_{:06}.jpg", cues[k].0), (end.saturating_sub(start)) as f64 / 1000.0));
    }
    Timeline { cues: out, total_ms: total }
}

/// H.264 MP4 via Windows' built-in Media Foundation encoder — no ffmpeg. Each
/// cue becomes one RGB32 sample stamped with its wall-clock offset, so the
/// variable frame rate survives into the container. MF's RGB32 is BGRA; the
/// decoded JPEG rows are expanded RGB→BGRX while copying.
unsafe fn encode_mf(out: &Path, dir: &Path, timeline: &Timeline) -> Result<()> {
    use windows::Win32::Media::MediaFoundation as mf;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    if timeline.cues.is_empty() {
        bail!("empty timeline");
    }
    // frame size: recorded by the capture worker, else probed from a frame
    let (w, h) = std::fs::read(dir.join("captured.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| {
            let w = v.get("width")?.as_u64()? as u32;
            let h = v.get("height")?.as_u64()? as u32;
            Some((w, h))
        })
        .unwrap_or_else(|| {
            let p = dir.join("frames").join(&timeline.cues[0].0);
            image::image_dimensions(&p).unwrap_or((0, 0))
        });
    if w == 0 || h == 0 {
        bail!("unknown frame dimensions");
    }
    // bitrate scales with resolution: ~4.5 Mbps per megapixel at 30 fps
    let bitrate: u32 = (((w as u64 * h as u64) / 100_000) * 450)
        .clamp(4_000_000, 30_000_000) as u32;

    let com = CoInitializeEx(None, COINIT_MULTITHREADED);
    if com.is_err() && com != windows::Win32::Foundation::RPC_E_CHANGED_MODE {
        com.ok()?;
    }
    mf::MFStartup(mf::MF_VERSION, mf::MFSTARTUP_NOSOCKET)?;
    let result = encode_mf_inner(out, dir, timeline, w, h, bitrate);
    let _ = mf::MFShutdown();
    result
}

fn encode_mf_inner(
    out: &Path,
    dir: &Path,
    timeline: &Timeline,
    w: u32,
    h: u32,
    bitrate: u32,
) -> Result<()> {
    use windows::Win32::Media::MediaFoundation as mf;
    use windows::core::HSTRING;

    unsafe {
        let mut wide: Vec<u16> = out.as_os_str().encode_wide().collect();
        wide.push(0);
        let hs = HSTRING::from_wide(&wide)?;
        let writer = mf::MFCreateSinkWriterFromURL(
            &hs,
            None,
            None,
        )?;
        let out_type = mf::MFCreateMediaType()?;
        out_type.SetGUID(&mf::MF_MT_MAJOR_TYPE, &mf::MFMediaType_Video)?;
        out_type.SetGUID(&mf::MF_MT_SUBTYPE, &mf::MFVideoFormat_H264)?;
        out_type.SetUINT32(&mf::MF_MT_AVG_BITRATE, bitrate)?;
        out_type.SetUINT64(&mf::MF_MT_FRAME_SIZE, ((w as u64) << 32) | h as u64)?;
        out_type.SetUINT64(&mf::MF_MT_FRAME_RATE, (30u64 << 32) | 1)?;
        out_type.SetUINT32(&mf::MF_MT_INTERLACE_MODE, mf::MFVideoInterlace_Progressive.0 as u32)?;
        out_type.SetUINT32(&mf::MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
        let stream_id = writer.AddStream(&out_type)?;
        let in_type = mf::MFCreateMediaType()?;
        in_type.SetGUID(&mf::MF_MT_MAJOR_TYPE, &mf::MFMediaType_Video)?;
        in_type.SetGUID(&mf::MF_MT_SUBTYPE, &mf::MFVideoFormat_RGB32)?;
        in_type.SetUINT64(&mf::MF_MT_FRAME_SIZE, ((w as u64) << 32) | h as u64)?;
        in_type.SetUINT64(&mf::MF_MT_FRAME_RATE, (30u64 << 32) | 1)?;
        in_type.SetUINT32(&mf::MF_MT_INTERLACE_MODE, mf::MFVideoInterlace_Progressive.0 as u32)?;
        in_type.SetUINT32(&mf::MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
        writer.SetInputMediaType(stream_id, &in_type, None)?;
        writer.BeginWriting()?;

        let frame_bytes = (w as usize * h as usize * 4).max(4);
        let mut start_ms = 0u64;
        for (name, dur) in &timeline.cues {
            let p = dir.join("frames").join(name);
            let bytes = std::fs::read(&p)?;
            let img = image::load_from_memory(&bytes)?.to_rgb8();
            if img.width() != w || img.height() != h {
                bail!("frame {} is {}x{}, expected {w}x{h}", name, img.width(), img.height());
            }
            let sample = mf::MFCreateSample()?;
            let buf = mf::MFCreateMemoryBuffer(frame_bytes as u32)?;
            let mut pb: *mut u8 = std::ptr::null_mut();
            let mut max_len = 0u32;
            buf.Lock(&mut pb, Some(&mut max_len), None)?;
            // MF RGB32 is bottom-up DIB order — write the rows flipped, or the
            // mp4 plays upside down
            for row in 0..h as usize {
                let src = &img.as_raw()[row * w as usize * 3..][..w as usize * 3];
                let dst = pb.add((h as usize - 1 - row) * w as usize * 4);
                for x in 0..w as usize {
                    *dst.add(x * 4) = src[x * 3 + 2];     // B
                    *dst.add(x * 4 + 1) = src[x * 3 + 1]; // G
                    *dst.add(x * 4 + 2) = src[x * 3];     // R
                    *dst.add(x * 4 + 3) = 255;
                }
            }
            buf.Unlock()?;
            buf.SetCurrentLength(frame_bytes as u32)?;
            sample.AddBuffer(&buf)?;
            sample.SetSampleTime((start_ms * 10_000) as i64)?;
            sample.SetSampleDuration((*dur * 10_000.0).round() as i64)?;
            writer.WriteSample(stream_id, &sample)?;
            start_ms += (*dur * 1000.0).round() as u64;
        }
        // the H.264 MFT does not extend the FINAL sample's duration — without
        // padding, the mp4 ends at the last changed frame and the closing hold
        // (e.g. the fade-out tail) is cut off
        let tail = timeline.total_ms.saturating_sub(start_ms);
        if tail > 0 && !timeline.cues.is_empty() {
            let name = &timeline.cues.last().unwrap().0;
            let bytes = std::fs::read(dir.join("frames").join(name))?;
            let img = image::load_from_memory(&bytes)?.to_rgb8();
            let mut pad = 0u64;
            while pad < tail {
                let step = pad.min(250);
                let sample = mf::MFCreateSample()?;
                let buf = mf::MFCreateMemoryBuffer(frame_bytes as u32)?;
                let mut pb: *mut u8 = std::ptr::null_mut();
                let mut max_len = 0u32;
                buf.Lock(&mut pb, Some(&mut max_len), None)?;
                for row in 0..h as usize {
                    let src = &img.as_raw()[row * w as usize * 3..][..w as usize * 3];
                    let dst = pb.add((h as usize - 1 - row) * w as usize * 4);
                    for x in 0..w as usize {
                        *dst.add(x * 4) = src[x * 3 + 2];
                        *dst.add(x * 4 + 1) = src[x * 3 + 1];
                        *dst.add(x * 4 + 2) = src[x * 3];
                        *dst.add(x * 4 + 3) = 255;
                    }
                }
                buf.Unlock()?;
                buf.SetCurrentLength(frame_bytes as u32)?;
                sample.AddBuffer(&buf)?;
                sample.SetSampleTime(((start_ms + step) * 10_000) as i64)?;
                sample.SetSampleDuration(250 * 10_000)?;
                writer.WriteSample(stream_id, &sample)?;
                pad += 250;
            }
        }
        writer.Finalize()?;
        Ok(())
    }
}

fn encode(ff: &Path, dir: &Path, out: &Path, timeline: &Timeline) -> Result<u64> {    if let Some(p) = out.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut cmd = std::process::Command::new(ff);
    cmd.args(["-y", "-hide_banner", "-loglevel", "error", "-nostdin"]);
    if timeline.cues.is_empty() {
        // no timestamps (legacy/killed worker): uniform spacing from fps
        cmd.args(["-framerate", "10", "-i", "frame_%06d.jpg"]).current_dir(dir);
    } else {
        // variable frame rate straight from the wall-clock timeline; resampled
        // to 30 fps CFR at encode so every player shows the true pace
        let mut list = String::from("ffconcat version 1.0\n");
        for (name, d) in &timeline.cues {
            list.push_str(&format!("file '{}'\nduration {:.3}\n", dir.join("frames").join(name).display(), d));
        }
        // concat-demuxer quirk: the final duration is honoured only when the
        // last file is listed a second time
        if let Some((name, d)) = timeline.cues.last() {
            list.push_str(&format!("file '{}'\nduration {:.3}\n", dir.join("frames").join(name).display(), d));
        }
        std::fs::write(dir.join("list.ffconcat"), list)?;
        cmd.args(["-f", "concat", "-safe", "0", "-i"])
            .arg(dir.join("list.ffconcat"))
            .args(["-vf", "fps=30"]);
    }
    cmd.args(["-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p", "-movflags", "+faststart"])
        .arg(out)
        .stdin(std::process::Stdio::null());
    let status = cmd.status()?;
    if !status.success() {
        bail!("ffmpeg exited with {status}");
    }
    Ok(std::fs::metadata(out).map(|m| m.len()).unwrap_or(0))
}

/// Dependency-free fallback: a single HTML file that replays the JPEG frames
/// on the REAL wall-clock timeline (per-frame durations from the capture
/// ticks), so idle stretches and slow captures look exactly like what the
/// user saw. Frames are embedded as data URLs (bounded) so the artifact stands
/// alone and stays reviewable; a multi-hour recording that would blow the
/// budget falls back to file references.
fn write_player(dir: &Path, frames: &[PathBuf], timeline: &Timeline, fps: u32) -> Result<()> {
    const EMBED_BUDGET: u64 = 48 * 1024 * 1024; // base64 inflates ~4/3; keep the html sane
    let total: u64 = frames.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
    let embed = total <= EMBED_BUDGET;
    let mut body = String::new();
    for p in frames.iter() {
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
    }
    // per-frame on-screen ms; a timeline mismatch (or its absence) falls back
    // to uniform 1000/fps spacing
    let dur: Vec<u64> = if timeline.cues.len() == frames.len() {
        timeline.cues.iter().map(|(_, d)| ((*d * 1000.0).round() as u64).max(1)).collect()
    } else {
        vec![1000 / fps.max(1) as u64; frames.len()]
    };
    let total_ms: u64 = dur.iter().sum();
    let wall = format!("{}.{:03}", total_ms / 1000, total_ms % 1000);
    let count = frames.len();
    let html = format!(
        r#"<!doctype html><meta charset="utf-8"><title>cua recording</title>
<style>body{{margin:0;background:#111;color:#ddd;font:14px system-ui;display:flex;flex-direction:column;height:100vh}}
#stage{{flex:1;display:grid;place-items:center;overflow:hidden}}img{{max-width:100%;max-height:100%;display:none}}
img.on{{display:block}}#bar{{padding:8px 12px;display:flex;gap:10px;align-items:center;background:#1b1b1b}}</style>
<div id="stage">{body}</div>
<div id="bar"><button id="pp">⏸</button><input id="rng" type="range" min="0" max="{last}" value="0">
<span id="lbl"></span><span>{count} frames · wall-clock {wall}s · ←/→ 单帧步进</span></div>
<script>
const f=[...document.images];const dur={dur_json};const cum=[0];
for(let k=0;k<dur.length;k++)cum.push(cum[k]+dur[k]);
let i=0,t=null;
const fmt=s=>`${{Math.floor(s/60)}}:${{(s%60).toFixed(1).padStart(4,'0')}}`;
function show(n){{i=Math.max(0,Math.min(n,f.length-1));f.forEach((e,k)=>e.classList.toggle('on',k===i));rng.value=i;lbl.textContent=`${{i+1}}/${{f.length}}  ${{fmt(cum[i]/1000)}}`;}}
function tick(){{t=setTimeout(()=>{{if(i+1<f.length){{show(i+1);tick();}}else{{stop();}}}},dur[i]);pp.textContent='⏸';}}
function play(){{stop();tick();}}
function stop(){{if(t)clearTimeout(t);t=null;pp.textContent='▶';}}
pp.onclick=()=>t?stop():play();
rng.oninput=e=>{{stop();show(+e.target.value)}};
document.onkeydown=e=>{{if(e.key==='ArrowRight'){{stop();show(i+1)}}if(e.key==='ArrowLeft'){{stop();show(i-1)}}}};
show(0);play();
</script>"#,
        body = body,
        last = count.saturating_sub(1),
        count = count,
        wall = wall,
        dur_json = serde_json::to_string(&dur)?,
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
