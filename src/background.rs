// background.rs — Windows background delivery rungs for the active desktop.
//
// This is deliberately *not* the hidden-desktop sandbox path: synthetic pen
// input is coordinate-routed by the system input queue, so it can only reach a
// top-level window on the current input desktop. The sandbox keeps using UIA and
// window messages (see input.rs / uia.rs).
use anyhow::{bail, Result};
use windows::Win32::Foundation::{HANDLE, HWND, POINT};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Controls::{
    CreateSyntheticPointerDevice, DestroySyntheticPointerDevice, POINTER_FEEDBACK_DEFAULT,
    POINTER_TYPE_INFO, POINTER_TYPE_INFO_0,
};
use windows::Win32::UI::Input::Pointer::{
    InjectSyntheticPointerInput, POINTER_FLAG_DOWN, POINTER_FLAG_INCONTACT, POINTER_FLAG_INRANGE,
    POINTER_FLAG_UP, POINTER_INFO, POINTER_PEN_INFO,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, IsWindow,
    SetForegroundWindow, SetWindowLongPtrW, WindowFromPoint, GA_ROOT, GWL_EXSTYLE, PT_PEN,
    WS_EX_NOACTIVATE,
};

// Windows headers define this as 1; the windows-rs bindings keep penFlags as a
// raw u32 on some versions.
const PEN_FLAG_BARREL: u32 = 0x0000_0001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    MouseClick,
}

pub fn is_chromium_class(class: &str) -> bool {
    class.starts_with("Chrome_WidgetWin_") || class.starts_with("CefBrowser")
}

pub fn is_wpf_class(class: &str) -> bool {
    class.starts_with("HwndWrapper")
}

pub fn is_tk_class(class: &str) -> bool {
    class == "TkTopLevel" || class.starts_with("TkTopLevel.")
}

pub fn is_gtk_class(class: &str) -> bool {
    class.starts_with("gdkWindow") || class.starts_with("gdkSurface")
}

/// True when the target class is empirically known to silently drop a posted
/// left-button click. At that point the background path must use a system-queue
/// actuator (pen injection) or refuse, never report PostMessage success.
pub fn would_be_silently_dropped(hwnd: HWND, _kind: EventKind) -> bool {
    let class = crate::win::window_class(hwnd);
    is_chromium_class(&class)
        || is_wpf_class(&class)
        || is_tk_class(&class)
        || is_gtk_class(&class)
}

pub fn target_class(hwnd: HWND) -> String {
    crate::win::window_class(hwnd)
}

/// True when `target`'s top-level root is the window actually visible at the
/// screen point. Synthetic pen input is hit-tested by the system, so an
/// occluded target would click the occluder instead.
pub fn target_visible_at_point(target: HWND, x: i32, y: i32) -> bool {
    let target_root = crate::win::root_window(target);
    let top = unsafe { WindowFromPoint(POINT { x, y }) };
    if top.is_invalid() {
        return false;
    }
    unsafe { GetAncestor(top, GA_ROOT) == target_root }
}

struct NoActivateGuard {
    root: isize,
    applied: bool,
}

impl NoActivateGuard {
    fn arm(hwnd: HWND) -> Self {
        let root = crate::win::root_window(hwnd);
        let prev = unsafe { GetWindowLongPtrW(root, GWL_EXSTYLE) };
        let want = WS_EX_NOACTIVATE.0 as isize;
        let applied = (prev & want) == 0 && unsafe {
            SetWindowLongPtrW(root, GWL_EXSTYLE, prev | want);
            (GetWindowLongPtrW(root, GWL_EXSTYLE) & want) != 0
        };
        Self { root: root.0 as isize, applied }
    }
}

impl Drop for NoActivateGuard {
    fn drop(&mut self) {
        if !self.applied {
            return;
        }
        let root = HWND(self.root as *mut _);
        unsafe {
            let current = GetWindowLongPtrW(root, GWL_EXSTYLE);
            let want = WS_EX_NOACTIVATE.0 as isize;
            SetWindowLongPtrW(root, GWL_EXSTYLE, current & !want);
        }
    }
}

fn restore_foreground(previous: HWND) {
    if previous.is_invalid() {
        return;
    }
    for _ in 0..2 {
        if unsafe { GetForegroundWindow() } == previous {
            return;
        }
        unsafe {
            let current = GetForegroundWindow();
            let current_thread = GetWindowThreadProcessId(current, None);
            let my_thread = GetCurrentThreadId();
            let attached = current_thread != 0 && current_thread != my_thread;
            if attached {
                let _ = AttachThreadInput(my_thread, current_thread, true);
            }
            let _ = SetForegroundWindow(previous);
            if attached {
                let _ = AttachThreadInput(my_thread, current_thread, false);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(12));
    }
}

fn pen_taps(x: i32, y: i32, barrel: bool, count: usize) -> Result<()> {
    unsafe {
        let device = CreateSyntheticPointerDevice(PT_PEN, 1, POINTER_FEEDBACK_DEFAULT)
            .map_err(|e| anyhow::anyhow!("CreateSyntheticPointerDevice(PT_PEN): {e}"))?;
        let pen_flags = if barrel { PEN_FLAG_BARREL } else { 0 };
        let make = |flags| POINTER_TYPE_INFO {
            r#type: PT_PEN,
            Anonymous: POINTER_TYPE_INFO_0 {
                penInfo: POINTER_PEN_INFO {
                    pointerInfo: POINTER_INFO {
                        pointerType: PT_PEN,
                        pointerId: 0,
                        pointerFlags: flags,
                        sourceDevice: HANDLE::default(),
                        hwndTarget: HWND::default(),
                        ptPixelLocation: POINT { x, y },
                        ..Default::default()
                    },
                    penFlags: pen_flags,
                    penMask: 0,
                    pressure: 512,
                    rotation: 0,
                    tiltX: 0,
                    tiltY: 0,
                },
            },
        };
        let down = make(POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT);
        let up = make(POINTER_FLAG_UP);
        let mut result: Result<()> = Ok(());
        for i in 0..count.max(1) {
            let r1 = InjectSyntheticPointerInput(device, &[down]);
            std::thread::sleep(std::time::Duration::from_millis(25));
            let r2 = InjectSyntheticPointerInput(device, &[up]);
            if let Err(e) = r1.and(r2) {
                result = Err(anyhow::anyhow!("InjectSyntheticPointerInput(PT_PEN): {e}"));
                break;
            }
            if i + 1 < count.max(1) {
                std::thread::sleep(std::time::Duration::from_millis(70));
            }
        }
        let _ = DestroySyntheticPointerDevice(device);
        result
    }
}

/// Coordinate-routed background click on the active desktop. The target must be
/// visible at the point; the target is held non-activatable and the user's
/// previous foreground is restored. Left/right are supported; middle is not.
pub fn inject_click(target: HWND, x: i32, y: i32, right: bool, double: bool) -> Result<String> {
    if target.is_invalid() {
        bail!("invalid target window");
    }
    unsafe {
        if !IsWindow(target).as_bool() {
            bail!("invalid or stale target window");
        }
    }
    if !target_visible_at_point(target, x, y) {
        bail!(
            "background coordinate injection cannot reach the target at ({x},{y}) — it is occluded by another window"
        );
    }
    let previous = unsafe { GetForegroundWindow() };
    let result = {
        let _noact = NoActivateGuard::arm(target);
        pen_taps(x, y, right, if double { 2 } else { 1 })
    };
    if result.is_ok() && previous != target {
        restore_foreground(previous);
    }
    result?;
    Ok(format!(
        "injected {} {} at ({x},{y}) via synthetic pen (background; no focus or cursor change)",
        if double { "double" } else { "single" },
        if right { "right click" } else { "left click" }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_families_are_recognized() {
        assert!(is_chromium_class("Chrome_WidgetWin_1"));
        assert!(is_chromium_class("Chrome_WidgetWin_0"));
        assert!(is_chromium_class("CefBrowserWindow"));
        assert!(!is_chromium_class("Notepad"));
        assert!(is_wpf_class("HwndWrapper[App.exe;;guid]"));
        assert!(is_tk_class("TkTopLevel"));
        assert!(is_tk_class("TkTopLevel.foo"));
        assert!(is_gtk_class("gdkWindowToplevel"));
        assert!(is_gtk_class("gdkSurfaceToplevel"));
    }
}
