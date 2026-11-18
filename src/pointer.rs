//! Global pointer preview: a click-through topmost overlay that shows a fake
//! cursor wherever an agent action is about to land, what it is doing while it
//! runs, and what it did afterwards — for the global desktop commands, for
//! actions inside a Child Session, and (separately, in-page) for the browser.
//!
//! Architecture: `cua preview serve` is a detached resident process that owns
//! one layered window and listens on a loopback UDP port (persisted in
//! `preview/preview-<session>.json` together with its pid). Every mutating
//! command in `dispatch` emits two packets — `pre` ("即将 …") before the action
//! and `post` ("已 …" / "✗ …") after — fire-and-forget, the same way the
//! browser's CUA_CURSOR works. The overlay can never fail the action it
//! decorates: UDP delivery is best-effort and every send is swallowed.
//!
//! Session scoping: the port file name carries `CUA_SESSION`, so a Child
//! Session's agent spawns its OWN host inside the child desktop (where the
//! read-only RDP viewer shows it), while the parent's host stays on the user's
//! desktop. Sandbox commands are skipped on purpose — the hidden desktop is
//! invisible, so a pointer preview there would be a lie.

use serde_json::{json, Value};
use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
use std::time::Duration;

const GLIDE_MS: u128 = 240;

pub fn enabled() -> bool {
    if std::env::var("CUA_POINTER_PREVIEW")
        .map(|v| v == "0" || v.eq_ignore_ascii_case("false"))
        .unwrap_or(false)
    {
        return false;
    }
    crate::util::load_config_json()
        .get("pointer_preview")
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

fn state_file() -> PathBuf {
    let base = crate::util::data_dir().join("preview");
    let _ = std::fs::create_dir_all(&base);
    let sess = std::env::var("CUA_SESSION").unwrap_or_default();
    if sess.is_empty() {
        base.join("preview.json")
    } else {
        base.join(format!("preview-{sess}.json"))
    }
}

/// One preview packet. `x`/`y` are screen coords (physical, virtual-desktop
/// space); `None` keeps the cursor where it last was (key-only actions).
#[derive(Clone, Debug)]
pub struct Spec {
    pub phase: &'static str, // pre | post
    pub verb: &'static str,  // 点击 / 输入 / 按键 / 滚动 / 拖拽 / 悬停 / 切换 / 关闭 …
    pub detail: String,      // window title / selector / coords — one short line
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub click: bool,
    pub ok: Option<bool>,
    /// a11y actions have no coordinates — the emitter resolves this selector to
    /// the target window's rect centre, so the preview still lands on target
    pub window_sel: Option<String>,
}

impl Spec {
    pub fn to_packet(&self) -> Value {
        json!({
            "phase": self.phase,
            "verb": self.verb,
            "detail": self.detail,
            "x": self.x,
            "y": self.y,
            "click": self.click,
            "ok": self.ok,
            "window": self.window_sel,
        })
    }
}

fn read_port() -> Option<(u16, u32)> {
    let raw = std::fs::read_to_string(state_file()).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let port = v.get("port")?.as_u64()? as u16;
    let pid = v.get("pid")?.as_u64()? as u32;
    Some((port, pid))
}

fn pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        unsafe {
            match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                Ok(h) => {
                    let _ = CloseHandle(h);
                    true
                }
                Err(_) => false,
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        false
    }
}

fn spawn_host() -> bool {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return false,
    };
    cua_browser_bridge::spawn_detached_process(&exe.display().to_string(), "preview serve")
}

/// Fire-and-forget: make sure the host is alive, then send the packet. The
/// only blocking part is the one-time host spawn wait (≤2 s per session);
/// every send is sub-millisecond and best-effort.
pub fn emit(spec: Spec) {
    if !enabled() {
        return;
    }
    let resend_after_spawn = spec.phase == "pre";
    std::thread::spawn(move || {
        // a11y actions carry no coordinates: resolve the target window's rect
        // centre here (on the preview thread — never blocking the action)
        let spec = match (&spec.window_sel, spec.x, spec.y) {
            (Some(sel), None, None) => {
                let centered = match crate::win::resolve_window(sel) {
                    Ok(hwnd) => match crate::win::window_rect(hwnd) {
                        Ok([rx, ry, rw, rh]) => Some((rx + rw / 2, ry + rh / 2)),
                        _ => None,
                    },
                    Err(_) => None,
                };
                match centered {
                    Some((x, y)) => {
                        let mut s2 = spec.clone();
                        s2.x = Some(x);
                        s2.y = Some(y);
                        s2
                    }
                    None => spec.clone(),
                }
            }
            _ => spec.clone(),
        };
        let payload = spec.to_packet().to_string();
        let ensure = |attempt: u8| -> Option<u16> {
            if let Some((port, pid)) = read_port() {
                if pid_alive(pid) {
                    return Some(port);
                }
            }
            if attempt > 0 {
                return None;
            }
            if !spawn_host() {
                return None;
            }
            for _ in 0..20 {
                std::thread::sleep(Duration::from_millis(100));
                if let Some((port, pid)) = read_port() {
                    if pid_alive(pid) {
                        return Some(port);
                    }
                }
            }
            None
        };
        let Some(port) = ensure(0) else {
            return;
        };
        let Ok(sock) = UdpSocket::bind("127.0.0.1:0") else {
            return;
        };
        let dst = format!("127.0.0.1:{port}");
        let _ = sock.send_to(payload.as_bytes(), &dst);
        // the very first packet after a cold spawn races the host's bind —
        // resend once; UDP is stateless so a duplicate only re-anchors the dot
        if resend_after_spawn {
            std::thread::sleep(Duration::from_millis(250));
            let _ = sock.send_to(payload.as_bytes(), &dst);
        }
    });
}

/// Maps a dispatched command to its preview packet, or None for commands that
/// must not show one: read-only probes, the browser (its own in-page cursor
/// covers it), the sandbox (the hidden desktop is invisible), and the child
/// proxy (the child-side dispatch emits inside the session).
pub fn pre_spec(cmd: &crate::Cmd) -> Option<Spec> {
    use crate::Cmd;
    let d = |window: &Option<String>, x: Option<i32>, y: Option<i32>| {
        let mut t = window.clone().unwrap_or_default();
        if let (Some(x), Some(y)) = (x, y) {
            t.push_str(&format!(" ({x},{y})"));
        }
        t.chars().take(44).collect::<String>()
    };
    match cmd {
        Cmd::Click { window, x, y, sandbox, double, .. } if !*sandbox => Some(Spec {
            phase: "pre",
            verb: if *double { "双击" } else { "点击" },
            detail: d(window, *x, *y),
            x: *x,
            y: *y,
            click: true,
            ok: None,
            window_sel: window.clone(),
        }),
        Cmd::Hover { window, x, y, .. } => Some(Spec {
            phase: "pre", verb: "悬停", detail: d(window, *x, *y), x: *x, y: *y, click: false, ok: None, window_sel: window.clone(),
        }),
        Cmd::Scroll { dx: _, dy: _, x, y, window, .. } => Some(Spec {
            phase: "pre", verb: "滚动", detail: d(window, *x, *y), x: *x, y: *y, click: false, ok: None, window_sel: window.clone(),
        }),
        Cmd::Drag { x1, y1, .. } => Some(Spec {
            phase: "pre", verb: "拖拽", detail: format!("({x1},{y1}) →"), x: Some(*x1), y: Some(*y1), click: false, ok: None, window_sel: None,
        }),
        Cmd::Key { keys, window, .. } => Some(Spec {
            phase: "pre", verb: "按键", detail: keys.chars().take(24).collect(), x: None, y: None, click: false, ok: None, window_sel: window.clone(),
        }),
        Cmd::Type { text, window, .. } => Some(Spec {
            phase: "pre",
            verb: "输入",
            detail: format!("{} ({})", window.clone().unwrap_or_default(), text.chars().count()).chars().take(44).collect(),
            x: None,
            y: None,
            click: false,
            ok: None,
            window_sel: window.clone(),
        }),
        Cmd::SetValue { window, clear, value, .. } => Some(Spec {
            phase: "pre",
            verb: if *clear { "清空" } else { "输入" },
            detail: d(&Some(window.clone()), None, None),
            x: None,
            y: None,
            click: false,
            ok: None,
            window_sel: Some(window.clone()),
        }),
        Cmd::Toggle { window, .. } => Some(Spec { phase: "pre", verb: "切换", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()) }),
        Cmd::Select { window, .. } => Some(Spec { phase: "pre", verb: "选择", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()) }),
        Cmd::SelectAdd { window, .. } => Some(Spec { phase: "pre", verb: "多选", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()) }),
        Cmd::SelectRemove { window, .. } => Some(Spec { phase: "pre", verb: "取消选择", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()) }),
        Cmd::Expand { window, .. } => Some(Spec { phase: "pre", verb: "展开", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()) }),
        Cmd::Collapse { window, .. } => Some(Spec { phase: "pre", verb: "折叠", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()) }),
        Cmd::SetRange { window, .. } => Some(Spec { phase: "pre", verb: "设值", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()) }),
        Cmd::Realize { window, .. } => Some(Spec { phase: "pre", verb: "展开", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()) }),
        Cmd::Close { window, .. } => Some(Spec { phase: "pre", verb: "关闭", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()) }),
        _ => None,
    }
}

/// The post packet mirrors the pre one with the outcome attached.
pub fn post_from(pre: &Spec, ok: bool) -> Spec {
    Spec {
        phase: "post",
        verb: pre.verb,
        detail: pre.detail.clone(),
        x: pre.x,
        y: pre.y,
        click: pre.click,
        ok: Some(ok),
            window_sel: pre.window_sel.clone(),
    }
}

/// `cua preview stop` — tell the host to exit.
pub fn stop() -> i32 {
    let Some((port, _)) = read_port() else {
        eprintln!("preview host is not running");
        return 1;
    };
    match UdpSocket::bind("127.0.0.1:0") {
        Ok(sock) => {
            let _ = sock.send_to(b"{\"phase\":\"shutdown\"}", format!("127.0.0.1:{port}"));
            println!("preview host told to exit (port {port})");
            0
        }
        Err(_) => 1,
    }
}

/// `cua preview serve` — owns the overlay window + UDP listener; runs until
/// stopped or idle for 30 minutes.
pub fn serve() -> i32 {
    #[cfg(windows)]
    {
        return host::run();
    }
    #[cfg(not(windows))]
    {
        eprintln!("preview overlay is Windows-only");
        1
    }
}

// ------------------------------------------------------------------ host side

#[cfg(windows)]
mod host {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, HDC, HBITMAP, HGDIOBJ, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, BLENDFUNCTION, GdiFlush,
    };
    use windows::Win32::Graphics::GdiPlus::{
        GdipCreateBitmapFromScan0, GdipCreateFont, GdipCreateFontFamilyFromName,
        GdipCreateFromHDC, GdipCreatePen1, GdipCreateSolidFill, GdipDeleteBrush, GdipDeleteFont,
        GdipDeleteFontFamily, GdipDeleteGraphics, GdipDeletePen, GdipDrawEllipse,
        GdipDrawRectangle, GdipDrawString, GdipFillEllipse, GdipFillRectangle, GdipGetImageGraphicsContext,
        GdipDisposeImage, GdipFlush, FlushIntentionFlush, GdipSetSmoothingMode, GdipGraphicsClear, GdiplusShutdown, GdiplusStartup, GdiplusStartupInput, GpBrush,
        GpSolidFill, Status, FontStyleRegular, GdiplusStartupOutput, GpImage, GpBitmap,
        GpFont, GpFontFamily, GpGraphics, GpPen, RectF, SmoothingModeAntiAlias, UnitPixel,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::Foundation::HINSTANCE;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
        GetSystemMetrics, PeekMessageW, PostQuitMessage, RegisterClassExW, ShowWindow,
        TranslateMessage, WINDOW_EX_STYLE, WNDCLASSEXW, MSG,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_EX_TRANSPARENT, WS_POPUP, PM_REMOVE, SW_HIDE, SW_SHOWNOACTIVATE, WM_QUIT,
        SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
        UpdateLayeredWindow, ULW_ALPHA,
    };

    const WINDOW_W: i32 = 680;
    const WINDOW_H: i32 = 180;
    const FPS_MS: u64 = 33;
    const IDLE_EXIT_SECS: u64 = 1800;

    static RUNNING: AtomicBool = AtomicBool::new(false);

    extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == 0x0010 {
            // WM_CLOSE
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        } else {
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
    }


    struct HostState {
        phase: &'static str,
        verb: String,
        detail: String,
        target: (i32, i32),
        current: (f64, f64),
        glide_from: (f64, f64),
        glide_start: std::time::Instant,
        click: bool,
        ok: Option<bool>,
        ripple_start: Option<std::time::Instant>,
    }

    impl HostState {
        fn ripple_t(&self) -> f32 {
            match self.ripple_start {
                Some(t0) => ((t0.elapsed().as_millis() as f32) / 600.0).clamp(0.0, 1.0),
                None => 0.0,
            }
        }
    }

    fn apply(state: &mut HostState, v: &Value) {
        if std::env::var("CUA_PREVIEW_DEBUG")
            .map(|x| x != "0" && !x.eq_ignore_ascii_case("false"))
            .unwrap_or(false)
        {
            eprintln!(
                "[preview] {} {} @ {:?},{:?} ({})",
                v.get("phase").and_then(|p| p.as_str()).unwrap_or("?"),
                v.get("verb").and_then(|s| s.as_str()).unwrap_or("?"),
                v.get("x").and_then(|x| x.as_i64()),
                v.get("y").and_then(|y| y.as_i64()),
                v.get("detail").and_then(|s| s.as_str()).unwrap_or(""),
            );
        }
        let phase = v
            .get("phase")
            .and_then(|p| p.as_str())
            .unwrap_or("post")
            .to_string();
        state.verb = v
            .get("verb")
            .and_then(|s| s.as_str())
            .unwrap_or("动作")
            .to_string();
        state.detail = v
            .get("detail")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .chars()
            .take(44)
            .collect();
        state.click = v.get("click").and_then(|c| c.as_bool()).unwrap_or(false);
        state.ok = v.get("ok").and_then(|o| o.as_bool());
        if let (Some(x), Some(y)) = (
            v.get("x").and_then(|n| n.as_i64()).map(|n| n as i32),
            v.get("y").and_then(|n| n.as_i64()).map(|n| n as i32),
        ) {
            state.glide_from = state.current;
            state.target = (x, y);
            state.glide_start = std::time::Instant::now();
        }
        state.phase = match phase.as_str() {
            "pre" => "pre",
            "act" => "act",
            _ => "post",
        };
        if state.phase == "post" && state.click && state.ok != Some(false) {
            state.ripple_start = Some(std::time::Instant::now());
        }
    }

    struct Ui {
        hwnd: HWND,
        mem_dc: HDC,
        dib: HBITMAP,
        old_bmp: HGDIOBJ,
        bits: *mut u8,
        bmp: *mut GpBitmap,
        graphics: *mut GpGraphics,
        font_main: *mut GpFont,
        font_dim: *mut GpFont,
        family: *mut GpFontFamily,
        bubble_brush: *mut GpBrush,
        text_brush: *mut GpBrush,
        dim_brush: *mut GpBrush,
    }

    unsafe impl Send for Ui {}

    unsafe fn create_ui(hinstance: HINSTANCE, gdi_token: &mut usize) -> Option<Ui> {
        // GDI+ must be initialised before any Gdip* call — missing this is
        // why the first build failed with "window/GDI+ init failed"
        let si = GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: windows::Win32::Foundation::BOOL(0),
            SuppressExternalCodecs: windows::Win32::Foundation::BOOL(0),
        };
        let output: *mut GdiplusStartupOutput = std::ptr::null_mut();
        if GdiplusStartup(gdi_token, &si, output) != Status(0) {
            eprintln!("preview: GdiplusStartup failed");
            return None;
        }
        let class_name: Vec<u16> = "cua-pointer-preview\0".encode_utf16().collect();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: PCWSTR::from_raw(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let ex = WS_EX_LAYERED
            | WS_EX_TRANSPARENT
            | WS_EX_TOPMOST
            | WS_EX_NOACTIVATE
            | WS_EX_TOOLWINDOW;
        let hwnd = CreateWindowExW(
            ex,
            PCWSTR::from_raw(class_name.as_ptr()),
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            WINDOW_W,
            WINDOW_H,
            None,
            None,
            hinstance,
            None,
        )
        .ok()?;
        // 32bpp top-down ARGB DIB — GDI+ draws premultiplied pixels into it,
        // UpdateLayeredWindow composites it onto the screen
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = WINDOW_W;
        bmi.bmiHeader.biHeight = -WINDOW_H; // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let screen_dc = GetDC(windows::Win32::Foundation::HWND::default());
        let dib = CreateDIBSection(screen_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let mem_dc = CreateCompatibleDC(screen_dc);
        ReleaseDC(windows::Win32::Foundation::HWND::default(), screen_dc);
        let old_bmp = SelectObject(mem_dc, HGDIOBJ(dib.0));
        // GDI+ via the HDC writes RGB but leaves the alpha channel at 0 — ULW's
        // AC_SRC_ALPHA blend then treats every pixel as fully transparent and
        // the overlay is invisible on screen even though ULW succeeds. Building
        // the Graphics from a bitmap wrapping the same scan0 writes premultiplied
        // ARGB straight into the DIB, which is what ULW needs.
        const PIXEL_32BPP_ARGB: i32 = 0x0026200A; // PixelFormat32bppARGB
        let mut bmp: *mut GpBitmap = std::ptr::null_mut();
        if GdipCreateBitmapFromScan0(WINDOW_W, WINDOW_H, WINDOW_W * 4,
                                     PIXEL_32BPP_ARGB, Some(bits as *const u8), &mut bmp) != Status(0) {
            return None;
        }
        let mut graphics: *mut GpGraphics = std::ptr::null_mut();
        if GdipGetImageGraphicsContext(bmp as *mut GpImage, &mut graphics) != Status(0) {
            return None;
        }
        GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);
        let name_w: Vec<u16> = "Microsoft YaHei UI\0".encode_utf16().collect();
        let mut family: *mut GpFontFamily = std::ptr::null_mut();
        if GdipCreateFontFamilyFromName(PCWSTR::from_raw(name_w.as_ptr()), std::ptr::null_mut(), &mut family) != Status(0) {
            let fb: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
            if GdipCreateFontFamilyFromName(PCWSTR::from_raw(fb.as_ptr()), std::ptr::null_mut(), &mut family) != Status(0) {
                return None;
            }
        }
        let mut font_main: *mut GpFont = std::ptr::null_mut();
        let mut font_dim: *mut GpFont = std::ptr::null_mut();
        if GdipCreateFont(family, 17.0, FontStyleRegular.0, UnitPixel, &mut font_main) != Status(0) {
            return None;
        }
        if GdipCreateFont(family, 14.0, FontStyleRegular.0, UnitPixel, &mut font_dim) != Status(0) {
            return None;
        }
        let mut bubble_sf: *mut GpSolidFill = std::ptr::null_mut();
        let mut text_sf: *mut GpSolidFill = std::ptr::null_mut();
        let mut dim_sf: *mut GpSolidFill = std::ptr::null_mut();
        if GdipCreateSolidFill(0xE6101824_u32, &mut bubble_sf) != Status(0) {
            return None;
        }
        if GdipCreateSolidFill(0xFFFFFFFF_u32, &mut text_sf) != Status(0) {
            return None;
        }
        if GdipCreateSolidFill(0xB3FFFFFF_u32, &mut dim_sf) != Status(0) {
            return None;
        }
        let bubble_brush = bubble_sf as *mut GpBrush;
        let text_brush = text_sf as *mut GpBrush;
        let dim_brush = dim_sf as *mut GpBrush;
        Some(Ui {
            hwnd,
            mem_dc,
            dib,
            old_bmp,
            bits: bits as *mut u8,
            bmp,
            graphics,
            font_main,
            font_dim,
            family,
            bubble_brush,
            text_brush,
            dim_brush,
        })
    }

    fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
        ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
    }

    unsafe fn draw_frame(ui: &Ui, state: &mut HostState) {
        // clear to fully transparent
        let n = (WINDOW_W * WINDOW_H * 4) as usize;
        std::ptr::write_bytes(ui.bits, 0, n);
        let dot = (70i32, 110i32);
        let (dot_c, ring_c) = match (state.phase, state.ok) {
            ("pre", _) => (argb(255, 245, 158, 11), argb(220, 245, 158, 11)),
            ("act", _) => (argb(255, 239, 68, 68), argb(220, 239, 68, 68)),
            ("post", Some(false)) => (argb(255, 239, 68, 68), argb(220, 239, 68, 68)),
            _ => (argb(255, 34, 197, 94), argb(220, 34, 197, 94)),
        };
        // bubble
        let bw = 320.0f32;
        let bh = 66.0f32;
        let bx = (dot.0 + 20) as f32;
        let by = (dot.1 - 40) as f32;
        let _ = GdipFillRectangle(ui.graphics, ui.bubble_brush, bx, by, bw, bh);
        let mut pen: *mut GpPen = std::ptr::null_mut();
        if GdipCreatePen1(0x66000000, 1.0, UnitPixel, &mut pen) == Status(0) {
            let _ = GdipDrawRectangle(ui.graphics, pen, bx, by, bw, bh);
            GdipDeletePen(pen);
        }
        // text: line1 = phase + verb, line2 = detail
        let phase_word = match state.phase {
            "pre" => "即将",
            "act" => "正在",
            _ => {
                if state.ok == Some(false) {
                    "✗"
                } else {
                    "已"
                }
            }
        };
        let line1 = format!("{phase_word} {}", state.verb);
        let w1: Vec<u16> = line1.encode_utf16().collect();
        let w2: Vec<u16> = format!("{}\0", state.detail).encode_utf16().collect();
        let r1 = RectF { X: bx + 14.0, Y: by + 9.0, Width: bw - 22.0, Height: 24.0 };
        let _ = GdipDrawString(ui.graphics, PCWSTR::from_raw(w1.as_ptr()), (w1.len() - 1) as i32, ui.font_main, &r1, std::ptr::null(), ui.text_brush);
        let r2 = RectF { X: bx + 14.0, Y: by + 36.0, Width: bw - 22.0, Height: 22.0 };
        let _ = GdipDrawString(ui.graphics, PCWSTR::from_raw(w2.as_ptr()), (w2.len() - 1) as i32, ui.font_dim, &r2, std::ptr::null(), ui.dim_brush);
        // dot + border
        let mut fill: *mut GpSolidFill = std::ptr::null_mut();
        if GdipCreateSolidFill(dot_c, &mut fill) == Status(0) {
            let _ = GdipFillEllipse(ui.graphics, fill as *mut GpBrush, (dot.0 - 11) as f32, (dot.1 - 11) as f32, 22.0, 22.0);
            GdipDeleteBrush(fill as *mut GpBrush);
        }
        let mut bpen: *mut GpPen = std::ptr::null_mut();
        if GdipCreatePen1(argb(255, 255, 255, 255), 2.0, UnitPixel, &mut bpen) == Status(0) {
            let _ = GdipDrawEllipse(ui.graphics, bpen, (dot.0 - 11) as f32, (dot.1 - 11) as f32, 22.0, 22.0);
            GdipDeletePen(bpen);
        }
        // success ripple
        if state.phase == "post" && state.ok != Some(false) && state.click {
            let t = state.ripple_t();
            let r = 11.0 + 30.0 * t;
            let a = (220.0 * (1.0 - t)) as u8;
            let mut rpen: *mut GpPen = std::ptr::null_mut();
            if GdipCreatePen1(argb(a, 34, 197, 94), 2.5, UnitPixel, &mut rpen) == Status(0) {
                let _ = GdipDrawEllipse(ui.graphics, rpen, (dot.0 as f32) - r, (dot.1 as f32) - r, r * 2.0, r * 2.0);
                GdipDeletePen(rpen);
            }
        }
        let _ = ring_c;
        // GDI+ renders lazily — without the flush, ULW reads the freshly-cleared
        // (all-zero) pixels and the overlay stays invisible forever
        GdipFlush(ui.graphics, FlushIntentionFlush);
    }

    unsafe fn paint(hwnd: HWND, ui: &Ui, state: &mut HostState) {
        // glide: ease-out cubic over GLIDE_MS
        let t = ((state.glide_start.elapsed().as_millis() as f64) / (super::GLIDE_MS as f64)).clamp(0.0, 1.0);
        let e = 1.0 - (1.0 - t).powi(3);
        state.current.0 = state.glide_from.0 + (state.target.0 as f64 - state.glide_from.0) * e;
        state.current.1 = state.glide_from.1 + (state.target.1 as f64 - state.glide_from.1) * e;
        draw_frame(ui, state);
        GdiFlush();
        // the window FOLLOWS the dot: ULW repositions via pptDst
        let vs_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vs_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vs_w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vs_h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        let dst_x = (state.current.0 as i32 - 80).clamp(vs_x, vs_x + vs_w - WINDOW_W);
        let dst_y = (state.current.1 as i32 - 110).clamp(vs_y, vs_y + vs_h - WINDOW_H);
        let dst = POINT { x: dst_x, y: dst_y };
        let size = SIZE { cx: WINDOW_W, cy: WINDOW_H };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: 0,     // AC_SRC_OVER
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: 1, // AC_SRC_ALPHA
        };
        let screen_dc = GetDC(windows::Win32::Foundation::HWND::default());
        let ulw_r = UpdateLayeredWindow(
            hwnd,
            screen_dc,
            Some(&dst),
            Some(&size),
            ui.mem_dc,
            Some(&src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        ReleaseDC(windows::Win32::Foundation::HWND::default(), screen_dc);
    }

    pub fn run() -> i32 {
        if RUNNING.swap(true, Ordering::SeqCst) {
            eprintln!("preview host already running in this session");
            return 1;
        }
        let sock = (8767..=8790)
            .find_map(|p| UdpSocket::bind(("127.0.0.1", p)).ok())
            .or_else(|| UdpSocket::bind(("127.0.0.1", 0)).ok());
        let Some(sock) = sock else {
            eprintln!("preview: no UDP port available");
            return 1;
        };
        let _ = sock.set_read_timeout(Some(Duration::from_millis(400)));
        let port = sock.local_addr().map(|a| a.port()).unwrap_or(0);
        let pid = std::process::id();
        let sf = super::state_file();
        let _ = std::fs::write(&sf, json!({ "port": port, "pid": pid }).to_string());
        eprintln!("[cua-preview] listening on 127.0.0.1:{port} (pid {pid})");
        let (tx, rx) = mpsc::channel::<Value>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match sock.recv_from(&mut buf) {
                    Ok((n, _)) => {
                        let Ok(v) = serde_json::from_slice::<Value>(&buf[..n]) else {
                            continue;
                        };
                        if v.get("phase").and_then(|p| p.as_str()) == Some("shutdown") {
                            let _ = tx.send(v);
                            std::thread::sleep(Duration::from_millis(200));
                            std::process::exit(0);
                        }
                        if tx.send(v).is_err() {
                            return;
                        }
                    }
                    Err(_) => { /* read timeout — keeps the loop cadence */ }
                }
            }
        });
        let code = pump(&rx, &sf);
        let _ = std::fs::remove_file(&sf);
        code
    }

    fn pump(rx: &Receiver<Value>, sf: &std::path::Path) -> i32 {
        unsafe {
            let hmodule = GetModuleHandleW(PCWSTR::null()).unwrap_or_default();
            let hinstance = HINSTANCE(hmodule.0);
            let class_name: Vec<u16> = "cua-pointer-preview\0".encode_utf16().collect();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                lpszClassName: PCWSTR::from_raw(class_name.as_ptr()),
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let mut gdi_token = 0usize;
            let Some(ui) = create_ui(hinstance, &mut gdi_token) else {
                eprintln!("preview: window/GDI+ init failed");
                return 1;
            };
            // ULW composites the content but does NOT show a hidden window —
            // without this the overlay is never visible on screen at all
            let _ = ShowWindow(ui.hwnd, SW_SHOWNOACTIVATE);

            let mut state = HostState {
                phase: "post",
                verb: String::new(),
                detail: String::new(),
                target: (0, 0),
                current: (0.0, 0.0),
                glide_from: (0.0, 0.0),
                glide_start: std::time::Instant::now() - Duration::from_secs(3600),
                click: false,
                ok: Some(true),
                ripple_start: None,
            };
            let mut idle = std::time::Instant::now();
            let mut quit = false;
            loop {
                while let Ok(v) = rx.try_recv() {
                    idle = std::time::Instant::now();
                    if v.get("phase").and_then(|p| p.as_str()) == Some("shutdown") {
                        quit = true;
                        break;
                    }
                    apply(&mut state, &v);
                }
                if quit {
                    break;
                }
                paint(ui.hwnd, &ui, &mut state);
                // non-blocking message pump
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        quit = true;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                if quit {
                    break;
                }
                if idle.elapsed() > Duration::from_secs(IDLE_EXIT_SECS) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(FPS_MS));
            }
            let _ = DestroyWindow(ui.hwnd);
            GdipDeleteFont(ui.font_main);
            GdipDeleteFont(ui.font_dim);
            GdipDeleteFontFamily(ui.family);
            GdipDeleteBrush(ui.bubble_brush);
            GdipDeleteBrush(ui.text_brush);
            GdipDeleteBrush(ui.dim_brush);
            GdipDeleteGraphics(ui.graphics);
            GdipDisposeImage(ui.bmp as *mut GpImage);
            GdiplusShutdown(gdi_token);
            let _ = SelectObject(ui.mem_dc, ui.old_bmp);
            let _ = DeleteObject(ui.dib);
            let _ = DeleteDC(ui.mem_dc);
            0
        }
    }

}
