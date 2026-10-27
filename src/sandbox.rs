// sandbox.rs — hidden-desktop executor (true background control).
// Complements Qoder/Trae/ZCode (all foreground-required): apps live on an invisible
// Win32 desktop; SendInput/UIA/PrintWindow run on a thread attached to that desktop.
use anyhow::{anyhow, bail, Result};
use serde_json::json;
use windows::Win32::Foundation::{GENERIC_ALL, HWND};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, CreateDesktopW, OpenDesktopW, SetThreadDesktop, DESKTOP_CONTROL_FLAGS, HDESK,
};

use crate::util::Receipt;
use crate::SandboxOp;

pub const DESKTOP_NAME: &str = "cua-sandbox";
const READY_EVENT: &str = "Local\\cua-sandbox-ready";
/// Named job held by the keeper: closing it (keeper exit) kills every process the
/// sandbox launched, including children that re-launch themselves (msinfo32 does).
const JOB_NAME: &str = "Local\\cua-sandbox-job";
const JOB_OBJECT_ASSIGN_PROCESS: u32 = 0x0001;

unsafe fn create_job_holding() -> Result<windows::Win32::Foundation::HANDLE> {
    use windows::Win32::System::JobObjects::{
        CreateJobObjectW, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JobObjectExtendedLimitInformation,
    };
    let mut name: Vec<u16> = JOB_NAME.encode_utf16().chain(std::iter::once(0)).collect();
    let h = CreateJobObjectW(None, windows::core::PCWSTR(name.as_mut_ptr()))
        .map_err(|e| anyhow!("CreateJobObjectW: {e}"))?;
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    SetInformationJobObject(
        h,
        JobObjectExtendedLimitInformation,
        &info as *const _ as *const _,
        std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
    )
    .map_err(|e| anyhow!("SetInformationJobObject: {e}"))?;
    Ok(h)
}

/// Attach a freshly created (suspended) process to the sandbox job.
fn assign_to_job(hproc: windows::Win32::Foundation::HANDLE) -> Result<()> {
    use windows::Win32::System::JobObjects::{AssignProcessToJobObject, OpenJobObjectW};
    for _ in 0..40 {
        let mut name: Vec<u16> = JOB_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        let hjob = unsafe {
            OpenJobObjectW(
                JOB_OBJECT_ASSIGN_PROCESS,
                false,
                windows::core::PCWSTR(name.as_mut_ptr()),
            )
        };
        if let Ok(hjob) = hjob {
            let r = unsafe { AssignProcessToJobObject(hjob, hproc) };
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(hjob);
            }
            r.map_err(|e| anyhow!("AssignProcessToJobObject: {e}"))?;
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    bail!("sandbox job '{JOB_NAME}' is not available (keeper missing)")
}

/// Launch `exe` on the hidden desktop, suspended, joined to the job, then resumed.
fn spawn_on_sandbox(exe: &str, args: Option<&str>) -> Result<u32> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::System::Threading::{
        CreateProcessW, ResumeThread, TerminateProcess, CREATE_NEW_CONSOLE, CREATE_SUSPENDED,
        CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW,
    };
    let mut cmd: Vec<u16> = if exe.contains('\\') || exe.contains('/') {
        format!("\"{exe}\" {}", args.unwrap_or("")).trim_end().encode_utf16().collect()
    } else {
        format!("{exe} {}", args.unwrap_or("")).trim_end().encode_utf16().collect()
    };
    cmd.push(0);
    let mut desktop_w: Vec<u16> = DESKTOP_NAME.encode_utf16().chain(std::iter::once(0)).collect();
    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        lpDesktop: windows::core::PWSTR(desktop_w.as_mut_ptr()),
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            PWSTR(cmd.as_mut_ptr()),
            None,
            None,
            false,
            CREATE_NEW_CONSOLE | CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
            None,
            None,
            &si,
            &mut pi,
        )
        .map_err(|e| anyhow!("CreateProcessW('{exe}'): {e}"))?;
        if let Err(e) = assign_to_job(pi.hProcess) {
            let _ = TerminateProcess(pi.hProcess, 1);
            let _ = windows::Win32::Foundation::CloseHandle(pi.hThread);
            let _ = windows::Win32::Foundation::CloseHandle(pi.hProcess);
            return Err(e);
        }
        ResumeThread(pi.hThread);
        let pid = pi.dwProcessId;
        let _ = windows::Win32::Foundation::CloseHandle(pi.hThread);
        let _ = windows::Win32::Foundation::CloseHandle(pi.hProcess);
        Ok(pid)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct SandboxState {
    desktop: String,
    pids: Vec<u32>,
    started: String,
}

fn state_path() -> std::path::PathBuf {
    crate::util::ensure_data_dir().join("sandbox.json")
}

fn save_state(s: &SandboxState) -> Result<()> {
    std::fs::write(state_path(), serde_json::to_vec(s)?)?;
    Ok(())
}

fn load_state() -> Option<SandboxState> {
    std::fs::read(state_path()).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

fn desktop_name_wide() -> Vec<u16> {
    DESKTOP_NAME.encode_utf16().chain(std::iter::once(0)).collect()
}

fn try_open_desktop() -> Result<HDESK> {
    unsafe {
        let mut name = desktop_name_wide();
        OpenDesktopW(
            windows::core::PCWSTR(name.as_mut_ptr()),
            DESKTOP_CONTROL_FLAGS(0),
            false,
            GENERIC_ALL.0,
        )
        .map_err(|e| anyhow!("desktop '{DESKTOP_NAME}' not available: {e}"))
    }
}

fn try_create_desktop() -> Result<HDESK> {
    unsafe {
        let mut name = desktop_name_wide();
        CreateDesktopW(
            windows::core::PCWSTR(name.as_mut_ptr()),
            None,
            None,
            DESKTOP_CONTROL_FLAGS(0),
            GENERIC_ALL.0,
            None,
        )
        .map_err(|e| anyhow!("CreateDesktopW: {e}"))
    }
}

/// Run `f` on a fresh thread attached to the sandbox desktop.
fn with_sandbox<T, F>(f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    std::thread::spawn(move || {
        let h = try_open_desktop()?;
        unsafe {
            SetThreadDesktop(h).map_err(|e| anyhow!("SetThreadDesktop: {e}"))?;
        }
        let out = f();
        unsafe {
            let _ = CloseDesktop(h);
        }
        out
    })
    .join()
    .map_err(|e| anyhow!("sandbox worker panicked: {e:?}"))?
}

// ---- desktop window enumeration ----

thread_local! {
    static SB_OUT: std::cell::RefCell<Vec<crate::win::WinInfo>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn enum_desktop_windows(h: HDESK, visible_only: bool) -> Result<Vec<crate::win::WinInfo>> {
    crate::win::with_process_cache(|| {
        SB_OUT.with(|c| c.borrow_mut().clear());
        unsafe {
            windows::Win32::System::StationsAndDesktops::EnumDesktopWindows(
                h,
                Some(enum_cb),
                windows::Win32::Foundation::LPARAM(0),
            )
            .map_err(|e| anyhow!("EnumDesktopWindows: {e}"))?;
        }
        let mut out = SB_OUT.with(|c| std::mem::take(&mut *c.borrow_mut()));
        out.retain(|w| !visible_only || w.visible);
        Ok(out)
    })
}

unsafe extern "system" fn enum_cb(
    hwnd: HWND,
    _l: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::BOOL {
    if let Some(info) = crate::win::snapshot_window(hwnd) {
        SB_OUT.with(|c| c.borrow_mut().push(info));
    }
    windows::Win32::Foundation::BOOL(1)
}

fn enum_desktop_windows_all() -> Result<Vec<crate::win::WinInfo>> {
    let h = try_open_desktop()?;
    let out = enum_desktop_windows(h, false);
    unsafe {
        let _ = CloseDesktop(h);
    }
    out
}

// ---- keepalive keeper (resident desktop-handle holder) ----

fn cmd_keepalive() -> Result<i32> {
    use windows::Win32::System::Threading::{CreateEventW, SetEvent};
    unsafe {
        let mut held: Option<HDESK> = None;
        for _ in 0..100 {
            if let Ok(d) = try_open_desktop() {
                held = Some(d);
                break;
            }
            if let Ok(d) = try_create_desktop() {
                held = Some(d);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let h = held.ok_or_else(|| anyhow!("keepalive: could not acquire desktop"))?;
        SetThreadDesktop(h).map_err(|e| anyhow!("SetThreadDesktop: {e}"))?;
        // hold the sandbox job open for the keeper's whole lifetime
        let _job = create_job_holding()?;

        let mut evt_name: Vec<u16> = format!("{READY_EVENT}\0").encode_utf16().collect();
        let evt = CreateEventW(None, true, false, windows::core::PCWSTR(evt_name.as_mut_ptr()))
            .map_err(|e| anyhow!("CreateEventW: {e}"))?;
        let _ = SetEvent(evt);

        // hold until sandbox.json disappears
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if !state_path().exists() {
                break;
            }
        }
        let _ = CloseDesktop(h);
    }
    Ok(0)
}

// ---- subcommand dispatch ----

pub fn dispatch(op: SandboxOp) -> Result<i32> {
    match op {
        SandboxOp::Start { exe, args } => cmd_start(&exe, args),
        SandboxOp::Apps => cmd_apps(),
        SandboxOp::State { window, depth, no_shot, out, patterns } => {
            cmd_state(&window, depth, no_shot, out, patterns)
        }
        SandboxOp::Shot { window, out } => cmd_shot(&window, &out),
        SandboxOp::Click { window, index, x, y, shot_id, right, double } => {
            cmd_click(window, index, x, y, shot_id, right, double)
        }
        SandboxOp::Type { text, window } => cmd_type(&text, window),
        SandboxOp::Key { keys, window, confirm } => cmd_key(&keys, window, confirm),
        SandboxOp::Scroll { dx, dy, window } => cmd_scroll(dx, dy, window),
        SandboxOp::Drag { x1, y1, x2, y2 } => cmd_drag(x1, y1, x2, y2),
        SandboxOp::GetValue { window, index } => cmd_get_value(&window, index),
        SandboxOp::SetValue { window, index, value, clear } => {
            let v = if clear {
                String::new()
            } else {
                value.ok_or_else(|| anyhow!("--value or --clear required"))?
            };
            cmd_set_value(&window, index, &v)
        }
        SandboxOp::Launch { exe, args } => cmd_launch_add(&exe, args),
        SandboxOp::Close { window, confirm } => cmd_close(&window, confirm),
        SandboxOp::Toggle { window, index } => cmd_toggle(&window, index),
        SandboxOp::Select { window, index } => cmd_select(&window, index),
        SandboxOp::Expand { window, index } => cmd_expand(&window, index),
        SandboxOp::Collapse { window, index } => cmd_collapse(&window, index),
        SandboxOp::SetRange { window, index, value } => cmd_set_range(&window, index, value),
        SandboxOp::GetRange { window, index } => cmd_get_range(&window, index),
        SandboxOp::Stop => cmd_stop(),
        SandboxOp::Keepalive => cmd_keepalive(),
    }
}

static READY_HANDLE: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

fn spawn_silent(exe: &str, args: Option<&str>) -> Result<u32> {
    use windows::Win32::System::Threading::{
        CreateProcessW, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
        STARTUPINFOW,
    };
    use windows::core::{PCWSTR, PWSTR};
    let mut cmd: Vec<u16> = format!("\"{exe}\" {}", args.unwrap_or(""))
        .trim_end()
        .encode_utf16()
        .collect();
    cmd.push(0);
    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            PWSTR(cmd.as_mut_ptr()),
            None,
            None,
            false,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            None,
            None,
            &si,
            &mut pi,
        )
        .map_err(|e| anyhow!("spawn '{exe}': {e}"))?;
        let _ = windows::Win32::Foundation::CloseHandle(pi.hThread);
        let _ = windows::Win32::Foundation::CloseHandle(pi.hProcess);
    }
    Ok(pi.dwProcessId)
}

pub fn cmd_start(exe: &str, args: Option<String>) -> Result<i32> {
    let cfg = crate::util::load_config();
    if let Some(d) = crate::util::denied_token(&cfg, exe) {
        let r = Receipt::new("sandbox-start").refused(
            json!({"exe": exe}),
            &format!("refused by safety policy: '{exe}' is on the no-negotiable deny list (matched '{d}')"),
        );
        r.print();
        return Ok(r.exit_code());
    }
    // clean previous sandbox
    if let Some(prev) = load_state() {
        for pid in &prev.pids {
            crate::win::kill_process_tree(*pid);
        }
        let _ = std::fs::remove_file(state_path());
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    // ensure desktop exists
    if try_open_desktop().is_err() {
        try_create_desktop()?;
    }

    // reset readiness event
    unsafe {
        use windows::Win32::System::Threading::{CreateEventW, ResetEvent};
        let mut evt_name: Vec<u16> = format!("{READY_EVENT}\0").encode_utf16().collect();
        let ready = CreateEventW(None, true, false, windows::core::PCWSTR(evt_name.as_mut_ptr()))
            .map_err(|e| anyhow!("CreateEventW(ready): {e}"))?;
        let _ = ResetEvent(ready);
        READY_HANDLE.store(ready.0 as isize, std::sync::atomic::Ordering::SeqCst);
    }

    // resident keeper holds the desktop object alive across CLI invocations
    let keeper_exe = std::env::current_exe().map_err(|e| anyhow!("current_exe: {e}"))?;
    let keeper_pid = spawn_silent(&keeper_exe.to_string_lossy(), Some("sandbox keepalive"))?;

    // wait for keeper to attach (max 5s)
    unsafe {
        use windows::Win32::Foundation::WAIT_TIMEOUT;
        use windows::Win32::System::Threading::WaitForSingleObject;
        let ready =
            windows::Win32::Foundation::HANDLE(READY_HANDLE.load(std::sync::atomic::Ordering::SeqCst) as *mut _);
        let mut ok = false;
        for _ in 0..50 {
            if WaitForSingleObject(ready, 100) != WAIT_TIMEOUT {
                ok = true;
                break;
            }
        }
        if !ok {
            bail!("sandbox keeper did not signal ready within 5s");
        }
    }

    let pid = spawn_on_sandbox(exe, args.as_deref())?;
    let st = SandboxState {
        desktop: DESKTOP_NAME.into(),
        pids: vec![pid, keeper_pid],
        started: crate::util::now_iso(),
    };
    save_state(&st)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-sandbox-v1",
            "desktop": DESKTOP_NAME,
            "pid": pid,
            "keeper_pid": keeper_pid,
            "exe": exe,
            "note": "app is running on an invisible desktop; the user's desktop is unaffected. Use 'cua sandbox apps/state/shot/click/type' to control it, 'cua sandbox stop' to end."
        }))?
    );
    Ok(0)
}

/// sandbox launch == start (command parity with Qoder launch_app)
pub fn cmd_launch(exe: &str, args: Option<String>) -> Result<i32> {
    cmd_start(exe, args)
}

pub fn cmd_apps() -> Result<i32> {
    let wins = with_sandbox(enum_desktop_windows_all)?;
    let v = json!({"desktop": DESKTOP_NAME, "count": wins.len(), "windows": wins});
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(0)
}

fn rank(w: &crate::win::WinInfo) -> (bool, bool, i64, usize) {
    // max_by_key => best first: visible, titled, largest area, longest title.
    (
        w.visible,
        !w.title.is_empty(),
        w.rect[2].max(0) as i64 * w.rect[3].max(0) as i64,
        w.title.chars().count(),
    )
}

fn resolve_in_sandbox(sel: &str) -> Result<crate::win::WinInfo> {
    let wins = enum_desktop_windows_all()?;
    let norm = sel.to_lowercase();
    if let Some(rest) = sel.strip_prefix("pid:") {
        let pid: u32 = rest.parse().map_err(|_| anyhow!("bad pid"))?;
        return wins
            .into_iter()
            .filter(|w| w.pid == pid)
            .max_by_key(rank)
            .ok_or_else(|| anyhow!("no window with pid {pid} on sandbox desktop"));
    }
    if let Some(rest) = sel.strip_prefix("title:") {
        let want = rest.to_lowercase();
        return wins
            .into_iter()
            .filter(|w| !w.desktop_area && w.title.to_lowercase() == want)
            .max_by_key(rank)
            .ok_or_else(|| anyhow!("no sandbox window with exact title '{rest}'"));
    }
    for prefix in ["process:", "exe:", "app:"] {
        if let Some(rest) = sel.strip_prefix(prefix) {
            let want = rest.to_ascii_lowercase();
            let want_exe = if want.ends_with(".exe") { want.clone() } else { format!("{want}.exe") };
            return wins
                .into_iter()
                .filter(|w| {
                    !w.desktop_area
                        && (w.process.eq_ignore_ascii_case(&want)
                            || w.process.eq_ignore_ascii_case(&want_exe))
                })
                .max_by_key(rank)
                .ok_or_else(|| anyhow!("no sandbox window from process '{rest}'"));
        }
    }
    wins.into_iter()
        .filter(|w| {
            !w.desktop_area
                && !w.title.is_empty()
                && (w.title.to_lowercase().contains(&norm) || w.class.to_lowercase().contains(&norm))
        })
        .max_by_key(rank)
        .ok_or_else(|| anyhow!("no sandbox window matching '{sel}' (try 'cua sandbox apps')"))
}

pub fn cmd_state(sel: &str, depth: usize, no_shot: bool, out: Option<String>, patterns: bool) -> Result<i32> {
    let info = resolve_in_sandbox(sel)?;
    let hwnd_isize = info.hwnd;
    let payload = with_sandbox(move || -> Result<serde_json::Value> {
        let hwnd = HWND(hwnd_isize as *mut _);
        let tree = crate::uia::walk_window(hwnd, depth, 2000, false, patterns)?;
        let state_id = crate::util::new_state_id();
        let elems: Vec<serde_json::Value> = tree
            .nodes
            .iter()
            .map(|n| {
                json!({"i": n.i, "type": n.ctype, "name": n.name, "class": n.class,
                       "rect": n.rect, "rid": n.rid, "disabled": n.disabled, "offscreen": n.offscreen,
                       "value": n.value, "toggle": n.toggle, "selected": n.selected,
                       "expanded": n.expanded, "range": n.range, "patterns": n.patterns})
            })
            .collect();
        crate::util::save_elements(hwnd_isize, &json!(elems));
        crate::util::save_last_state(&state_id, hwnd_isize, &tree.sig);
        let shot = if no_shot {
            None
        } else {
            let path = out.map(std::path::PathBuf::from).unwrap_or_else(|| {
                crate::util::ensure_data_dir()
                    .join("cache")
                    .join(format!("sandbox-shot-{}-{state_id}.png", hwnd_isize))
            });
            match crate::capture::shot_window(hwnd, false) {
                Ok(s) => {
                    crate::capture::save_png(&s, &path)?;
                    Some(json!({"path": path.to_string_lossy(), "width": s.width, "height": s.height, "rendered": s.rendered}))
                }
                Err(e) => Some(json!({"error": format!("{e:#}")})),
            }
        };
        Ok(json!({
            "state_id": state_id,
            "window": {"hwnd": hwnd_isize, "title": info.title, "class": info.class, "pid": info.pid, "process": info.process},
            "node_count": tree.nodes.len(),
            "sig": tree.sig,
            "nodes": elems,
            "screenshot": shot,
        }))
    })?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(0)
}

pub fn cmd_shot(sel: &str, out: &str) -> Result<i32> {
    let info = resolve_in_sandbox(sel)?;
    let hwnd_isize = info.hwnd;
    let out = out.to_string();
    let payload = with_sandbox(move || -> Result<serde_json::Value> {
        let hwnd = HWND(hwnd_isize as *mut _);
        let s = crate::capture::shot_window(hwnd, false)?;
        let path = std::path::PathBuf::from(&out);
        crate::capture::save_png(&s, &path)?;
        let state_id = crate::util::new_state_id();
        crate::util::save_last_state(&state_id, hwnd_isize, "");
        Ok(json!({"state_id": state_id, "path": path.to_string_lossy(), "width": s.width, "height": s.height, "rendered": s.rendered}))
    })?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(0)
}

fn sandbox_action<F>(cmd: &str, f: F) -> Result<i32>
where
    F: FnOnce() -> Result<(String, String)> + Send + 'static, // -> (status, detail)
{
    let started = std::time::Instant::now();
    let payload = with_sandbox(f);
    let mut r = match payload {
        Ok((status, detail)) => {
            let mut r = Receipt::new(cmd);
            r.target = json!({"desktop": DESKTOP_NAME});
            match status.as_str() {
                "ok" => {
                    r.dispatched = true;
                    r.status = "ok".into();
                }
                "refused" => r.status = "refused".into(),
                _ => r.status = "failed".into(),
            }
            r.detail = Some(detail);
            r
        }
        Err(e) => Receipt::new(cmd).fail(json!({}), &format!("{e:#}")),
    };
    r.elapsed_ms = Some(started.elapsed().as_millis());
    r.print();
    Ok(r.exit_code())
}

pub fn cmd_click(
    window: Option<String>,
    index: Option<usize>,
    x: Option<i32>,
    y: Option<i32>,
    shot_id: Option<String>,
    right: bool,
    double: bool,
) -> Result<i32> {
    let sel = window.clone();
    sandbox_action("sandbox-click", move || {
        let cfg = crate::util::load_config();
        // guard-first: a coordinate action without a fresh shot is refused even
        // if the target window has meanwhile disappeared.
        if index.is_none() && cfg.require_shot_id_for_coords {
            if let Err(reason) = crate::check_shot_fresh(shot_id.as_deref()) {
                return Ok((
                    "refused".into(),
                    format!("{reason} — run 'cua sandbox state/shot' first"),
                ));
            }
        }
        match (index, x, y) {
            (Some(idx), _, _) => {
                let sel = sel.ok_or_else(|| anyhow!("--window required for index clicks"))?;
                let info = resolve_in_sandbox(&sel)?;
                let hwnd = HWND(info.hwnd as *mut _);
                let rid = crate::rid_for_index(info.hwnd, idx)?;
                let detail = crate::uia::click_element(hwnd, &rid, double, right)?;
                Ok(("ok".into(), detail))
            }
            (_, Some(px), Some(py)) => {
                // msg-level click: find top-level window containing the point on the sandbox desktop
                let mut target: Option<(isize, String)> = None;
                for w in enum_desktop_windows_all()? {
                    let r = w.rect;
                    if px >= r[0] && py >= r[1] && px < r[0] + r[2] && py < r[1] + r[3] && r[2] > 0 {
                        target = Some((w.hwnd, w.title));
                        break;
                    }
                }
                let (hwnd, title) = target.ok_or_else(|| anyhow!("no sandbox window at ({px},{py})"))?;
                if cfg.require_shot_id_for_coords {
                    if let Err(reason) = crate::check_shot_id(shot_id.as_deref(), HWND(hwnd as *mut _)) {
                        return Ok((
                            "refused".into(),
                            format!("{reason} — run 'cua sandbox state/shot' first"),
                        ));
                    }
                }
                crate::input::post_click(HWND(hwnd as *mut _), px, py, right, double)?;
                Ok(("ok".into(), format!("clicked ({px},{py}) in '{title}' via message-level input")))
            }
            _ => Ok(("failed".into(), "--index or --x/--y required".into())),
        }
    })
}

pub fn cmd_type(text: &str, window: Option<String>) -> Result<i32> {
    let text = text.to_string();
    sandbox_action("sandbox-type", move || {
        let (top, title) = match &window {
            Some(sel) => {
                let info = resolve_in_sandbox(sel)?;
                (HWND(info.hwnd as *mut _), info.title)
            }
            None => {
                let wins = enum_desktop_windows_all()?;
                let w = wins.iter().find(|w| w.foreground).or_else(|| wins.iter().find(|w| w.visible)).ok_or_else(|| anyhow!("no sandbox window"))?;
                (HWND(w.hwnd as *mut _), w.title.clone())
            }
        };
        // msg-level input: SendInput is blocked on non-input desktops
        let target = crate::input::find_input_target(top);
        let n = crate::input::post_type(target, &text).map_err(|e| anyhow!("post_type: {e}"))?;
        Ok(("ok".into(), format!("typed {n} units into '{}' via message-level input (sandbox desktop)", title)))
    })
}

pub fn cmd_key(keys: &str, window: Option<String>, confirm: bool) -> Result<i32> {
    let combo = crate::input::parse_combo(keys)?;
    let keys = keys.to_string();
    sandbox_action("sandbox-key", move || {
        if keys.eq_ignore_ascii_case("alt+f4") && !confirm {
            return Ok(("refused".into(), "alt+f4 is Always-Confirm level — pass --confirm".into()));
        }
        let (top, title) = match &window {
            Some(sel) => {
                let info = resolve_in_sandbox(sel)?;
                (HWND(info.hwnd as *mut _), info.title)
            }
            None => {
                let wins = enum_desktop_windows_all()?;
                let w = wins.iter().find(|w| w.foreground).or_else(|| wins.iter().find(|w| w.visible)).ok_or_else(|| anyhow!("no sandbox window"))?;
                (HWND(w.hwnd as *mut _), w.title.clone())
            }
        };
        let target = crate::input::find_input_target(top);
        if let Some(text) = &combo.text {
            let n = crate::input::post_type(target, text)?;
            return Ok((
                "ok".into(),
                format!("typed {n} printable units for key '{keys}' into '{title}' via message-level input"),
            ));
        }
        crate::input::post_key(target, &combo)?;
        Ok(("ok".into(), format!("pressed {keys} via message-level input into '{}' (note: shortcuts reading real key state need foreground mode)", title)))
    })
}

pub fn cmd_scroll(dx: i32, dy: i32, window: Option<String>) -> Result<i32> {
    sandbox_action("sandbox-scroll", move || {
        let (px, py) = match &window {
            Some(sel) => {
                let info = resolve_in_sandbox(sel)?;
                let hwnd = HWND(info.hwnd as *mut _);
                crate::win::activate_window(hwnd)?;
                let r = info.rect;
                (r[0] + r[2] / 2, r[1] + r[3] / 2)
            }
            None => crate::input::cursor_pos(),
        };
        // msg-level: SendInput is blocked on non-input desktops
        let wins = enum_desktop_windows_all()?;
        let w = wins
            .iter()
            .find(|w| {
                let r = w.rect;
                px >= r[0] && py >= r[1] && px < r[0] + r[2].max(1) && py < r[1] + r[3].max(1) && r[2] > 0
            })
            .or_else(|| wins.iter().find(|w| w.visible))
            .ok_or_else(|| anyhow!("no sandbox window to scroll"))?;
        crate::input::post_scroll(HWND(w.hwnd as *mut _), px, py, dx, dy)?;
        Ok(("ok".into(), format!("scrolled dx={dx} dy={dy} via message-level input")))
    })
}

pub fn cmd_drag(x1: i32, y1: i32, x2: i32, y2: i32) -> Result<i32> {
    sandbox_action("sandbox-drag", move || {
        // msg-level drag on the window under the start point
        let wins = enum_desktop_windows_all()?;
        let w = wins
            .iter()
            .find(|w| {
                let r = w.rect;
                x1 >= r[0] && y1 >= r[1] && x1 < r[0] + r[2].max(1) && y1 < r[1] + r[3].max(1) && r[2] > 0
            })
            .or_else(|| wins.iter().find(|w| w.visible))
            .ok_or_else(|| anyhow!("no sandbox window under drag start"))?;
        let hwnd = w.hwnd;
        crate::input::post_drag(HWND(hwnd as *mut _), x1, y1, x2, y2)?;
        Ok(("ok".into(), format!("dragged ({x1},{y1})->({x2},{y2}) via message-level input")))
    })
}

pub fn cmd_get_value(sel: &str, index: usize) -> Result<i32> {
    let hwnd_isize = resolve_in_sandbox(sel)?.hwnd;
    let payload = with_sandbox(move || -> Result<serde_json::Value> {
        let rid = crate::rid_for_index(hwnd_isize, index)?;
        let v = crate::uia::get_value_element(HWND(hwnd_isize as *mut _), &rid)?;
        Ok(json!({"schema": "cua-value-v1", "value": v}))
    })?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(0)
}

pub fn cmd_set_value(sel: &str, index: usize, value: &str) -> Result<i32> {
    let hwnd_isize = resolve_in_sandbox(sel)?.hwnd;
    let value = value.to_string();
    sandbox_action("sandbox-set-value", move || {
        let rid = crate::rid_for_index(hwnd_isize, index)?;
        let d = crate::uia::set_value_element(HWND(hwnd_isize as *mut _), &rid, &value)?;
        Ok(("ok".into(), d))
    })
}

/// Add another app to the running sandbox desktop without disturbing the existing one.
pub fn cmd_launch_add(exe: &str, args: Option<String>) -> Result<i32> {
    let started = std::time::Instant::now();
    let cfg = crate::util::load_config();
    if let Some(d) = crate::util::denied_token(&cfg, exe) {
        let mut r = Receipt::new("sandbox-launch").refused(
            json!({"exe": exe}),
            &format!("refused by safety policy: '{exe}' is on the no-negotiable deny list (matched '{d}')"),
        );
        r.elapsed_ms = Some(started.elapsed().as_millis());
        r.print();
        return Ok(r.exit_code());
    }
    let mut st = load_state().ok_or_else(|| anyhow!("sandbox is not running — use 'cua sandbox start' first"))?;
    if try_open_desktop().is_err() {
        bail!("sandbox desktop is gone — restart with 'cua sandbox start'");
    }
    let pid = spawn_on_sandbox(exe, args.as_deref())?;
    st.pids.push(pid);
    save_state(&st)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-sandbox-v1",
            "desktop": DESKTOP_NAME,
            "pid": pid,
            "exe": exe,
            "apps": st.pids.len(),
            "note": "app added to the existing invisible desktop"
        }))?
    );
    Ok(0)
}

pub fn cmd_close(sel: &str, confirm: bool) -> Result<i32> {
    let info = resolve_in_sandbox(sel)?;
    let hwnd_isize = info.hwnd;
    let title = info.title.clone();
    sandbox_action("sandbox-close", move || {
        if !confirm {

            return Ok((
                "refused".into(),
                "'close' is Always-Confirm level — pass --confirm".into(),
            ));
        }
        let hwnd = HWND(hwnd_isize as *mut _);
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
            let _ = PostMessageW(hwnd, WM_CLOSE, windows::Win32::Foundation::WPARAM(0), windows::Win32::Foundation::LPARAM(0));
        }
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let alive = enum_desktop_windows_all()
                .map(|ws| ws.iter().any(|w| w.hwnd == hwnd_isize))
                .unwrap_or(false);
            if !alive {
                return Ok(("ok".into(), format!("closed '{title}' gracefully")));
            }
        }
        let killed = crate::win::kill_process_tree(info.pid);
        Ok((
            "ok".into(),
            format!("'{title}' did not close in 3s; force-terminated {killed} process(es)"),
        ))
    })
}

fn sandbox_a11y_action<F>(cmd: &str, sel: &str, index: usize, f: F) -> Result<i32>
where
    F: FnOnce(HWND, &[i32]) -> Result<String> + Send + 'static,
{
    let hwnd_isize = resolve_in_sandbox(sel)?.hwnd;
    sandbox_action(cmd, move || {
        let rid = crate::rid_for_index(hwnd_isize, index)?;
        let d = f(HWND(hwnd_isize as *mut _), &rid)?;
        Ok(("ok".into(), d))
    })
}

pub fn cmd_toggle(sel: &str, index: usize) -> Result<i32> {
    sandbox_a11y_action("sandbox-toggle", sel, index, |hwnd, rid| {
        let (d, state) = crate::uia::toggle_element(hwnd, rid)?;
        Ok(format!("{d} (now {state})"))
    })
}

pub fn cmd_select(sel: &str, index: usize) -> Result<i32> {
    sandbox_a11y_action("sandbox-select", sel, index, |hwnd, rid| {
        let (d, selected) = crate::uia::select_element(hwnd, rid)?;
        Ok(format!("{d} (selected={selected})"))
    })
}

pub fn cmd_expand(sel: &str, index: usize) -> Result<i32> {
    sandbox_a11y_action("sandbox-expand", sel, index, |hwnd, rid| {
        let (d, state) = crate::uia::expand_element(hwnd, rid)?;
        Ok(format!("{d} (now {state})"))
    })
}

pub fn cmd_collapse(sel: &str, index: usize) -> Result<i32> {
    sandbox_a11y_action("sandbox-collapse", sel, index, |hwnd, rid| {
        let (d, state) = crate::uia::collapse_element(hwnd, rid)?;
        Ok(format!("{d} (now {state})"))
    })
}

pub fn cmd_set_range(sel: &str, index: usize, value: f64) -> Result<i32> {
    sandbox_a11y_action("sandbox-set-range", sel, index, move |hwnd, rid| {
        crate::uia::set_range_element(hwnd, rid, value)
    })
}

pub fn cmd_get_range(sel: &str, index: usize) -> Result<i32> {
    let hwnd_isize = resolve_in_sandbox(sel)?.hwnd;
    let payload = with_sandbox(move || -> Result<serde_json::Value> {
        let rid = crate::rid_for_index(hwnd_isize, index)?;
        let v = crate::uia::get_range_element(HWND(hwnd_isize as *mut _), &rid)?;
        Ok(json!({"schema": "cua-range-v1", "value": v}))
    })?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(0)
}

pub fn status() -> serde_json::Value {
    match load_state() {
        Some(st) => json!({"running": true, "desktop": st.desktop, "pids": st.pids, "started": st.started}),
        None => json!({"running": false}),
    }
}

pub fn cmd_stop() -> Result<i32> {
    let st = load_state();
    let mut pids: Vec<u32> = st.as_ref().map(|s| s.pids.clone()).unwrap_or_default();
    // Sweep: anything owning a window on the hidden desktop is sandbox content
    // (children re-parented away from the tracked pid would otherwise leak).
    if let Ok(wins) = with_sandbox(enum_desktop_windows_all) {
        for w in wins {
            if w.pid != 0 && !pids.contains(&w.pid) {
                pids.push(w.pid);
            }
        }
    }
    // drop the state file first: the keeper exits and closes the job, which
    // terminates every remaining process in it.
    if st.is_some() {
        let _ = std::fs::remove_file(state_path());
    }
    let me = std::process::id();
    let mut killed = 0u32;
    for p in pids.iter().filter(|p| **p != me) {
        killed += crate::win::kill_process_tree(*p);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-sandbox-stop-v1",
            "killed_processes": killed,
            "note": "sandbox job closed (KILL_ON_JOB_CLOSE); hidden desktop released when its last handle closes"
        }))?
    );
    Ok(0)
}
