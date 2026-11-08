// win.rs — window enumeration, selection, activation, process info (Qoder list_apps/list_windows/activate_window)
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThreadId, OpenProcess, TerminateProcess, PROCESS_TERMINATE,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::System::Threading::AttachThreadInput;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetSystemMetrics,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, GetWindowRect, IsIconic,
    IsWindow, IsWindowVisible, IsZoomed, SetForegroundWindow, ShowWindow, BringWindowToTop,
    SW_RESTORE, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

#[derive(Serialize, Clone, Debug)]
pub struct WinInfo {
    pub hwnd: isize,
    pub title: String,
    pub class: String,
    pub pid: u32,
    pub process: String,
    pub visible: bool,
    pub minimized: bool,
    pub maximized: bool,
    pub foreground: bool,
    pub rect: [i32; 4],
    pub desktop_area: bool, // covers the whole virtual screen (desktop/shell windows)
}

pub fn dpi_aware() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

pub fn screen_metrics() -> (i32, i32, i32, i32, i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
            GetSystemMetrics(SM_CXSCREEN),
            GetSystemMetrics(SM_CYSCREEN),
        )
    }
}

thread_local! {
    static PROC_CACHE: std::cell::RefCell<Option<std::collections::HashMap<u32, String>>> =
        const { std::cell::RefCell::new(None) };
}

fn snapshot_process_names() -> std::collections::HashMap<u32, String> {
    let mut map = std::collections::HashMap::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return map;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(0);
                map.insert(
                    entry.th32ProcessID,
                    String::from_utf16_lossy(&entry.szExeFile[..len]),
                );
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(snap);
    }
    map
}

/// Reuse one process-table snapshot for a whole window enumeration.
pub fn with_process_cache<T>(f: impl FnOnce() -> T) -> T {
    let prev = PROC_CACHE.with(|c| c.borrow_mut().replace(snapshot_process_names()));
    let out = f();
    PROC_CACHE.with(|c| *c.borrow_mut() = prev);
    out
}

pub fn process_name(pid: u32) -> Option<String> {
    if let Some(name) = PROC_CACHE.with(|c| c.borrow().as_ref().and_then(|m| m.get(&pid).cloned())) {
        return Some(name);
    }
    snapshot_process_names().get(&pid).cloned()
}

pub fn window_text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..len as usize])
    }
}

pub fn window_class(hwnd: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetClassNameW(hwnd, &mut buf);
        if n <= 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&buf[..n as usize])
        }
    }
}

/// Per-monitor DPI scale for a window (1.0 at 96 DPI). The process is
/// per-monitor DPI aware, so window and capture coordinates stay in physical
/// pixels; this is metadata for consumers, not a coordinate conversion.
pub fn dpi_scale(hwnd: HWND) -> f32 {
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
}

/// DWM renders a DPI-unaware window at its own DPI (96) inside a
/// physical-pixel surface, so PrintWindow frames are smaller than window
/// pixels by this factor. 1.0 for every DPI-aware window, including windows on
/// a monitor whose DPI differs from the system DPI.
pub fn frame_scale(hwnd: HWND) -> f32 {
    use windows::Win32::UI::HiDpi::{
        AreDpiAwarenessContextsEqual, GetDpiForSystem, GetDpiForWindow,
        GetWindowDpiAwarenessContext, DPI_AWARENESS_CONTEXT_UNAWARE,
        DPI_AWARENESS_CONTEXT_UNAWARE_GDISCALED,
    };
    unsafe {
        let ctx = GetWindowDpiAwarenessContext(hwnd);
        let unaware = AreDpiAwarenessContextsEqual(ctx, DPI_AWARENESS_CONTEXT_UNAWARE).as_bool()
            || AreDpiAwarenessContextsEqual(ctx, DPI_AWARENESS_CONTEXT_UNAWARE_GDISCALED).as_bool();
        if !unaware {
            return 1.0;
        }
        let win_dpi = GetDpiForWindow(hwnd);
        let sys_dpi = GetDpiForSystem();
        if win_dpi == 0 || sys_dpi == 0 {
            return 1.0;
        }
        let s = sys_dpi as f32 / win_dpi as f32;
        if s.is_finite() && (1.0..=4.0).contains(&s) { s } else { 1.0 }
    }
}

pub fn window_rect(hwnd: HWND) -> Result<[i32; 4]> {
    unsafe {
        let mut r = RECT::default();
        let ok = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
        .is_ok();
        if !ok {
            GetWindowRect(hwnd, &mut r).map_err(|e| anyhow!("GetWindowRect: {e}"))?;
        }
        Ok([r.left, r.top, r.right - r.left, r.bottom - r.top])
    }
}

pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::OpenProcessToken;
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut ret = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elev as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        )
        .is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(token);
        ok && elev.TokenIsElevated != 0
    }
}

pub fn foreground_hwnd() -> HWND {
    unsafe { GetForegroundWindow() }
}

// ---- enumeration ----

thread_local! {
    static ENUM_OUT: std::cell::RefCell<Vec<WinInfo>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn list_windows(include_invisible: bool) -> Vec<WinInfo> {
    with_process_cache(|| {
        ENUM_OUT.with(|c| c.borrow_mut().clear());
        unsafe {
            let _ = EnumWindows(Some(enum_cb), LPARAM(0));
        }
        let mut out = ENUM_OUT.with(|c| std::mem::take(&mut *c.borrow_mut()));
        out.retain(|w| include_invisible || w.visible);
        out
    })
}

unsafe extern "system" fn enum_cb(hwnd: HWND, _l: LPARAM) -> BOOL {
    if let Some(info) = snapshot_window(hwnd) {
        ENUM_OUT.with(|c| c.borrow_mut().push(info));
    }
    BOOL(1)
}

pub fn snapshot_window(hwnd: HWND) -> Option<WinInfo> {
    unsafe {
        if !IsWindow(hwnd).as_bool() {
            return None;
        }
        let title = window_text(hwnd);
        let class = window_class(hwnd);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 && title.is_empty() && class.is_empty() {
            return None;
        }
        let visible = IsWindowVisible(hwnd).as_bool();
        let rect = window_rect(hwnd).unwrap_or([0, 0, 0, 0]);
        let (vx, vy, vw, vh, _, _) = screen_metrics();
        let desktop_area =
            rect[0] <= vx && rect[1] <= vy && rect[2] >= vw && rect[3] >= vh && vw > 100;
        Some(WinInfo {
            hwnd: hwnd.0 as isize,
            title,
            class,
            pid,
            process: process_name(pid).unwrap_or_default(),
            visible,
            minimized: IsIconic(hwnd).as_bool(),
            maximized: IsZoomed(hwnd).as_bool(),
            foreground: GetForegroundWindow() == hwnd,
            rect,
            desktop_area,
        })
    }
}

// ---- selection ----

pub fn resolve_window(sel: &str) -> Result<HWND> {
    let sel = sel.trim();
    if let Some(rest) = sel.strip_prefix("pid:") {
        let pid: u32 = rest.parse().map_err(|_| anyhow!("bad pid '{rest}'"))?;
        let cands: Vec<WinInfo> = list_windows(true).into_iter().filter(|w| w.pid == pid).collect();
        return pick(cands, sel);
    }
    if let Some(rest) = sel.strip_prefix("class:") {
        let cands: Vec<WinInfo> = list_windows(true)
            .into_iter()
            .filter(|w| w.class.eq_ignore_ascii_case(rest))
            .collect();
        return pick(cands, sel);
    }
    if let Some(rest) = sel.strip_prefix("title:") {
        let want = rest.to_lowercase();
        let cands: Vec<WinInfo> = list_windows(true)
            .into_iter()
            .filter(|w| !w.desktop_area && w.title.to_lowercase() == want)
            .collect();
        return pick(cands, sel);
    }
    // process:charmap.exe | process:notepad  (for apps whose title is localized or empty)
    for prefix in ["process:", "exe:", "app:"] {
        if let Some(rest) = sel.strip_prefix(prefix) {
            let want = rest.to_ascii_lowercase();
            let want_exe = if want.ends_with(".exe") { want.clone() } else { format!("{want}.exe") };
            let cands: Vec<WinInfo> = list_windows(true)
                .into_iter()
                .filter(|w| {
                    !w.desktop_area && !w.process.is_empty()
                        && (w.process.eq_ignore_ascii_case(&want)
                            || w.process.eq_ignore_ascii_case(&want_exe)
                            || w.process.to_ascii_lowercase() == want_exe)
                })
                .collect();
            return pick(cands, sel);
        }
    }
    if let Ok(num) = sel.parse::<isize>() {
        let h = HWND(num as *mut _);
        if unsafe { IsWindow(h).as_bool() } {
            return Ok(h);
        }
        bail!("window handle {num} does not exist");
    }
    if let Some(hex) = sel.strip_prefix("0x").or_else(|| sel.strip_prefix("0X")) {
        if let Ok(num) = isize::from_str_radix(hex, 16) {
            let h = HWND(num as *mut _);
            if unsafe { IsWindow(h).as_bool() } {
                return Ok(h);
            }
            bail!("window handle {num:#x} does not exist");
        }
    }
    let norm = sel.to_lowercase();
    let all = list_windows(true);
    let mut cands: Vec<WinInfo> = all
        .iter()
        .filter(|w| {
            w.visible && !w.desktop_area && !w.title.is_empty() && w.title.to_lowercase().contains(&norm)
        })
        .cloned()
        .collect();
    if cands.is_empty() {
        cands = all
            .into_iter()
            .filter(|w| !w.desktop_area && w.title.to_lowercase().contains(&norm))
            .collect();
    }
    pick(cands, sel)
}

fn pick(mut cands: Vec<WinInfo>, sel: &str) -> Result<HWND> {
    // A window can disappear between enumeration and use; never hand a dead
    // handle to capture/UIA just because it still appears in the snapshot.
    cands.retain(|w| unsafe { IsWindow(HWND(w.hwnd as *mut _)).as_bool() });
    if cands.is_empty() {
        bail!("no window matching '{sel}'");
    }
    // Ranking: not minimized > foreground > visible > biggest area > longest title.
    // Without the area term, helper windows ('CiceroUIWndFrame', IME frames, tooltips)
    // would outrank the app's real main window.
    cands.sort_by_key(|a| {
        (
            a.minimized,
            !a.foreground,
            !a.visible,
            std::cmp::Reverse(a.rect[2].max(0) as i64 * a.rect[3].max(0) as i64),
            std::cmp::Reverse(a.title.chars().count()),
        )
    });
    Ok(HWND(cands[0].hwnd as *mut _))
}

// ---- activation ----

thread_local! {
    static FOUND_PID_WND: std::cell::Cell<isize> = std::cell::Cell::new(0);
}

/// Root pid plus every process created under it. Launchers (notepad.exe's stub,
/// Electron helpers) own the real window from a child process.
fn pid_family(root: u32) -> Vec<u32> {
    let mut parents: Vec<(u32, u32)> = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return vec![root];
        };
        let mut e = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                parents.push((e.th32ProcessID, e.th32ParentProcessID));
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(snap);
    }
    let mut family = vec![root];
    let mut i = 0;
    while i < family.len() {
        let cur = family[i];
        for (pid, ppid) in &parents {
            if *ppid == cur && !family.contains(pid) {
                family.push(*pid);
            }
        }
        i += 1;
    }
    family
}

fn first_visible_window_of_pid(pid: u32) -> Option<HWND> {
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let pid = lparam.0 as u32;
        let mut p = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut p));
        if p == pid && IsWindowVisible(hwnd).as_bool() {
            FOUND_PID_WND.with(|c| c.set(hwnd.0 as isize));
            return BOOL(0);
        }
        BOOL(1)
    }
    FOUND_PID_WND.with(|c| c.set(0));
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(pid as isize));
    }
    let raw = FOUND_PID_WND.with(|c| c.get());
    if raw == 0 { None } else { Some(HWND(raw as *mut _)) }
}

/// Every existing top-level window handle. Used to spot windows that appear
/// after a launch even when a broker (UWP/ApplicationFrameHost) owns them.
pub fn top_level_hwnds() -> std::collections::HashSet<isize> {
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let set = &mut *(lparam.0 as *mut std::collections::HashSet<isize>);
        set.insert(hwnd.0 as isize);
        BOOL(1)
    }
    let mut set = std::collections::HashSet::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut set as *mut _ as isize));
    }
    set
}

fn first_new_visible_window(
    before: &std::collections::HashSet<isize>,
    own_pid: u32,
) -> Option<HWND> {
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let (before, own_pid, found) = &mut *(
            lparam.0 as *mut (
                &std::collections::HashSet<isize>,
                u32,
                Option<HWND>,
            )
        );
        if found.is_some() {
            return BOOL(0);
        }
        if before.contains(&(hwnd.0 as isize)) || !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let mut p = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut p));
        if p != *own_pid {
            *found = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut state = (before, own_pid, None);
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut state as *mut _ as isize));
    }
    state.2
}

/// Keep a just-started process out of the user's way: pin its first top-level
/// window to the bottom of the z-order without activating it, and give the
/// previous foreground window its focus back when the app raised itself
/// anyway (WinUI/Electron apps ignore SW_SHOWNOACTIVATE).
/// Returns (focus_restored, first_window).
pub fn keep_launched_background(
    pid: u32,
    prev: HWND,
    before: &std::collections::HashSet<isize>,
    timeout: std::time::Duration,
) -> (bool, Option<HWND>) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    };
    let started = std::time::Instant::now();
    let min_watch = std::time::Duration::from_millis(900);
    let settle = std::time::Duration::from_millis(400);
    let mut restored = false;
    let mut handled: Option<HWND> = None;
    let mut last_steal: Option<std::time::Instant> = None;
    let mut family = vec![pid];
    let mut tick: u32 = 0;
    while started.elapsed() < timeout {
        if tick % 10 == 0 {
            family = pid_family(pid);
        }
        tick += 1;
        let own_pid = std::process::id();
        let found = family
            .iter()
            .find_map(|p| first_visible_window_of_pid(*p))
            .or_else(|| first_new_visible_window(before, own_pid));
        if let Some(h) = found {
            if handled != Some(h) {
                handled = Some(h);
                unsafe {
                    let _ = SetWindowPos(
                        h,
                        HWND_BOTTOM,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                    );
                }
            }
            let fg = unsafe { GetForegroundWindow() };
            if !prev.is_invalid() && fg != prev {
                let mut fpid = 0u32;
                unsafe { GetWindowThreadProcessId(fg, Some(&mut fpid)); }
                let mut hpid = 0u32;
                unsafe { GetWindowThreadProcessId(h, Some(&mut hpid)); }
                if family.contains(&fpid) || fpid == hpid || fg == h {
                    last_steal = Some(std::time::Instant::now());
                    if matches!(activate_window(prev), Ok(true)) {
                        restored = true;
                    }
                }
            }
        }
        // early exit once the app has stopped grabbing focus for a while;
        // slow starters (Blender) keep the watcher alive past the first frame
        let quiet = last_steal.map(|t| t.elapsed() >= settle).unwrap_or(true);
        if started.elapsed() >= min_watch && quiet {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(12));
    }
    (restored, handled)
}

pub fn activate_window(hwnd: HWND) -> Result<bool> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_MENU};
    unsafe {
        if !IsWindow(hwnd).as_bool() {
            bail!("window no longer exists");
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        if GetForegroundWindow() == hwnd {
            return Ok(true);
        }
        let cur = GetCurrentThreadId();
        let mut tpid = 0u32;
        let target_thread = GetWindowThreadProcessId(hwnd, Some(&mut tpid));
        let fg = GetForegroundWindow();
        let mut fpid = 0u32;
        let fg_thread = if fg.is_invalid() {
            0
        } else {
            GetWindowThreadProcessId(fg, Some(&mut fpid))
        };
        let mut attached = false;
        if target_thread != 0 && target_thread != cur {
            attached = AttachThreadInput(cur, target_thread, true).as_bool();
            if fg_thread != 0 && fg_thread != target_thread {
                let _ = AttachThreadInput(cur, fg_thread, true);
            }
        }
        let mut ok = SetForegroundWindow(hwnd).as_bool();
        if !ok {
            // documented workaround: brief alt keystroke unlocks SetForegroundWindow
            keybd_event(VK_MENU.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
            ok = SetForegroundWindow(hwnd).as_bool();
            keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        }
        if !ok {
            // last resort: ask the shell to release the lock and push the window to the top;
            // some apps (QQ Music's main window) only accept the raise on the second attempt
            use windows::Win32::UI::WindowsAndMessaging::{LockSetForegroundWindow, LSFW_UNLOCK};
            let _ = LockSetForegroundWindow(LSFW_UNLOCK);
            let _ = BringWindowToTop(hwnd);
            for _ in 0..3 {
                if SetForegroundWindow(hwnd).as_bool() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(40));
            }
        }
        if attached {
            let _ = AttachThreadInput(cur, target_thread, false);
            if fg_thread != 0 && fg_thread != target_thread {
                let _ = AttachThreadInput(cur, fg_thread, false);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
        Ok(GetForegroundWindow() == hwnd)
    }
}

pub fn is_window_alive(hwnd: HWND) -> bool {
    unsafe { IsWindow(hwnd).as_bool() }
}

/// Which top-level window would receive a click at this screen point?
pub fn window_from_point(x: i32, y: i32) -> Option<HWND> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::WindowFromPoint;
    let h = unsafe { WindowFromPoint(POINT { x, y }) };
    if h.is_invalid() { None } else { Some(root_window(h)) }
}

/// True when the point belongs to the same top-level window as `hwnd`.
/// A covering popup/other app makes the physical click land elsewhere, so the
/// caller must refuse instead of reporting that it clicked its target.
pub fn point_hits_window(hwnd: HWND, x: i32, y: i32) -> bool {
    match window_from_point(x, y) {
        Some(hit) => hit.0 == root_window(hwnd).0,
        None => false,
    }
}

pub fn ensure_point_hits(hwnd: HWND, x: i32, y: i32) -> Result<()> {
    if point_hits_window(hwnd, x, y) {
        return Ok(());
    }
    let cover = window_from_point(x, y).and_then(snapshot_window);
    match cover {
        Some(w) => bail!(
            "refusing physical input at ({x},{y}): that point belongs to '{}' (hwnd {:#x}, pid {}), not the target — bring the target to the front or use --background / an a11y action",
            w.title,
            w.hwnd,
            w.pid
        ),
        None => bail!("refusing physical input at ({x},{y}): no window owns that point"),
    }
}

pub fn is_foreground(hwnd: HWND) -> bool {
    let fg = foreground_hwnd();
    !fg.is_invalid() && root_window(fg).0 == root_window(hwnd).0
}

/// Top-level root of a (possibly child) window — used to scope screenshot IDs
/// to the window a coordinate belongs to.
pub fn root_window(hwnd: HWND) -> HWND {
    use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GA_ROOT};
    unsafe {
        let r = GetAncestor(hwnd, GA_ROOT);
        if r.is_invalid() { hwnd } else { r }
    }
}

/// 100ns FILETIME ticks of process creation, used to prove that a stored PID
/// still refers to the process a sandbox launched (PIDs are reused).
pub fn process_creation_time(pid: u32) -> Option<u64> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok = GetProcessTimes(h, &mut created, &mut exited, &mut kernel, &mut user).is_ok();
        let _ = CloseHandle(h);
        if ok {
            Some(((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64)
        } else {
            None
        }
    }
}

pub fn kill_process_tree(pid: u32) -> u32 {
    unsafe {
        let mut killed = 0;
        if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snap, &mut entry).is_ok() {
                loop {
                    if entry.th32ParentProcessID == pid {
                        killed += kill_process_tree(entry.th32ProcessID);
                    }
                    if Process32NextW(snap, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = windows::Win32::Foundation::CloseHandle(snap);
        }
        if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            if TerminateProcess(h, 1).is_ok() {
                killed += 1;
            }
            let _ = windows::Win32::Foundation::CloseHandle(h);
        }
        killed
    }
}
