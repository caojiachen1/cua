// input.rs — SendInput mouse/keyboard (Qoder SendInput + Trae type_text/press_key + ZCode event strategy)
use anyhow::{anyhow, bail, Result};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, VkKeyScanW, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSE_EVENT_FLAGS, MOUSEINPUT,
    VIRTUAL_KEY, VK_ADD, VK_APPS, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_DECIMAL, VK_DELETE, VK_DIVIDE,
    VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_INSERT, VK_LEFT, VK_MENU, VK_MULTIPLY, VK_NEXT,
    VK_NUMLOCK, VK_PAUSE, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SNAPSHOT, VK_SPACE,
    VK_SUBTRACT, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::win::screen_metrics;

pub struct MouseFlags;
impl MouseFlags {
    pub const DOWN_L: u32 = MOUSEEVENTF_LEFTDOWN.0;
    pub const UP_L: u32 = MOUSEEVENTF_LEFTUP.0;
    pub const DOWN_R: u32 = MOUSEEVENTF_RIGHTDOWN.0;
    pub const UP_R: u32 = MOUSEEVENTF_RIGHTUP.0;
}

fn mouse_input(flags: u32, dx: i32, dy: i32, data: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: MOUSE_EVENT_FLAGS(flags | MOUSEEVENTF_MOVE.0),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn kb_input(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: KEYBD_EVENT_FLAGS(flags),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Physical-px -> normalized absolute (virtual desktop span).
fn to_abs(x: i32, y: i32) -> (i32, i32) {
    let (vx, vy, vw, vh, _, _) = screen_metrics();
    let nx = ((x - vx) as i64 * 65535 / (vw.max(1) - 1) as i64) as i32;
    let ny = ((y - vy) as i64 * 65535 / (vh.max(1) - 1) as i64) as i32;
    (nx.clamp(0, 65535), ny.clamp(0, 65535))
}

pub fn move_to(x: i32, y: i32) -> Result<()> {
    let (nx, ny) = to_abs(x, y);
    let inp = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: nx,
                dy: ny,
                mouseData: 0,
                dwFlags: MOUSE_EVENT_FLAGS(MOUSEEVENTF_MOVE.0 | MOUSEEVENTF_ABSOLUTE.0 | MOUSEEVENTF_VIRTUALDESK.0),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let n = unsafe { SendInput(&[inp], std::mem::size_of::<INPUT>() as i32) };
    if n != 1 {
        bail!("SendInput move failed");
    }
    Ok(())
}

pub fn cursor_pos() -> (i32, i32) {
    unsafe {
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        (p.x, p.y)
    }
}

pub fn click(x: i32, y: i32, right: bool, double: bool) -> Result<()> {
    move_to(x, y)?;
    std::thread::sleep(std::time::Duration::from_millis(30));
    let (down, up) = if right {
        (MouseFlags::DOWN_R, MouseFlags::UP_R)
    } else {
        (MouseFlags::DOWN_L, MouseFlags::UP_L)
    };
    let times = if double { 2 } else { 1 };
    for i in 0..times {
        let _ = unsafe { SendInput(&[mouse_input(down, 0, 0, 0)], std::mem::size_of::<INPUT>() as i32) };
        std::thread::sleep(std::time::Duration::from_millis(25));
        let _ = unsafe { SendInput(&[mouse_input(up, 0, 0, 0)], std::mem::size_of::<INPUT>() as i32) };
        if i + 1 < times {
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
    }
    Ok(())
}

pub fn scroll(x: i32, y: i32, dx: i32, dy: i32) -> Result<()> {
    move_to(x, y)?;
    std::thread::sleep(std::time::Duration::from_millis(30));
    let mut inputs = Vec::new();
    let steps = dy.abs().max(dx.abs()).max(1);
    let per = 120; // WHEEL_DELTA
    for _ in 0..steps {
        if dy != 0 {
            inputs.push(INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: (dy.signum() * per) as u32,
                        dwFlags: MOUSE_EVENT_FLAGS(MOUSEEVENTF_WHEEL.0),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }
        if dx != 0 {
            inputs.push(INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: (dx.signum() * per) as u32,
                        dwFlags: MOUSE_EVENT_FLAGS(MOUSEEVENTF_HWHEEL.0),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }
    }
    let n = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if n != inputs.len() as u32 {
        bail!("SendInput scroll partial ({n}/{})", inputs.len());
    }
    Ok(())
}

pub fn drag(x1: i32, y1: i32, x2: i32, y2: i32) -> Result<()> {
    move_to(x1, y1)?;
    std::thread::sleep(std::time::Duration::from_millis(80));
    let _ = unsafe { SendInput(&[mouse_input(MouseFlags::DOWN_L, 0, 0, 0)], std::mem::size_of::<INPUT>() as i32) };
    std::thread::sleep(std::time::Duration::from_millis(80));
    let steps = 24;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let x = (x1 as f32 + (x2 - x1) as f32 * t) as i32;
        let y = (y1 as f32 + (y2 - y1) as f32 * t) as i32;
        move_to(x, y)?;
        std::thread::sleep(std::time::Duration::from_millis(12));
    }
    std::thread::sleep(std::time::Duration::from_millis(60));
    let _ = unsafe { SendInput(&[mouse_input(MouseFlags::UP_L, 0, 0, 0)], std::mem::size_of::<INPUT>() as i32) };
    Ok(())
}

// ---------------- keyboard ----------------

/// (vk, needs_shift, resulting char for WM_CHAR)
fn char_key(ch: char) -> Option<(u16, bool, Option<u16>)> {
    if !ch.is_ascii_graphic() {
        return None;
    }
    let r = unsafe { VkKeyScanW(ch as u16) };
    if r == -1 {
        return None;
    }
    let vk = (r as u16) & 0xFF;
    let shift = (r as u16) & 0x100 != 0;
    Some((vk, shift, Some(ch as u16)))
}

fn named_vk(name: &str) -> Option<(u16, bool, Option<u16>)> {
    let n = name.to_ascii_lowercase();
    let named = match n.as_str() {
        "enter" | "return" => (VK_RETURN.0 as u16, Some(13u16)),
        "tab" => (VK_TAB.0 as u16, Some(9u16)),
        "esc" | "escape" => (VK_ESCAPE.0 as u16, None),
        "space" => (VK_SPACE.0 as u16, Some(32u16)),
        "backspace" | "bs" => (VK_BACK.0 as u16, Some(8u16)),
        "delete" | "del" => (VK_DELETE.0 as u16, None),
        "insert" | "ins" => (VK_INSERT.0 as u16, None),
        "up" => (VK_UP.0 as u16, None),
        "down" => (VK_DOWN.0 as u16, None),
        "left" => (VK_LEFT.0 as u16, None),
        "right" => (VK_RIGHT.0 as u16, None),
        "home" => (VK_HOME.0 as u16, None),
        "end" => (VK_END.0 as u16, None),
        "pageup" | "pgup" => (VK_PRIOR.0 as u16, None),
        "pagedown" | "pgdn" => (VK_NEXT.0 as u16, None),
        "shift" => (VK_SHIFT.0 as u16, None),
        "ctrl" | "control" => (VK_CONTROL.0 as u16, None),
        "alt" => (VK_MENU.0 as u16, None),
        "capslock" => (VK_CAPITAL.0 as u16, None),
        "numlock" => (VK_NUMLOCK.0 as u16, None),
        "printscreen" | "prtsc" => (VK_SNAPSHOT.0 as u16, None),
        "pause" => (VK_PAUSE.0 as u16, None),
        "apps" | "menu" => (VK_APPS.0 as u16, None),
        "multiply" => (VK_MULTIPLY.0 as u16, Some(b'*' as u16)),
        "add" => (VK_ADD.0 as u16, Some(b'+' as u16)),
        "subtract" => (VK_SUBTRACT.0 as u16, Some(b'-' as u16)),
        "decimal" => (VK_DECIMAL.0 as u16, Some(b'.' as u16)),
        "divide" => (VK_DIVIDE.0 as u16, Some(b'/' as u16)),
        s if s.len() == 4 && s.starts_with("num") && s.as_bytes()[3].is_ascii_digit() => {
            (0x60u16 + (s.as_bytes()[3] - b'0') as u16, Some(s.as_bytes()[3] as u16))
        }
        s if s.starts_with('f') && s[1..].parse::<u8>().map(|v| (1..=24).contains(&v)).unwrap_or(false) => {
            let f: u8 = s[1..].parse().unwrap();
            (0x70u16 + (f - 1) as u16, None)
        }
        _ => {
            let alias = match n.as_str() {
                "plus" => Some('+'),
                "minus" | "hyphen" | "dash" => Some('-'),
                "comma" => Some(','),
                "period" | "dot" | "fullstop" => Some('.'),
                "slash" => Some('/'),
                "backslash" => Some('\\'),
                "semicolon" => Some(';'),
                "quote" | "apostrophe" => Some('\''),
                "backtick" | "grave" => Some('`'),
                "leftbracket" | "lbracket" => Some('['),
                "rightbracket" | "rbracket" => Some(']'),
                "equal" | "equals" => Some('='),
                "tilde" => Some('~'),
                "exclam" | "bang" => Some('!'),
                "question" => Some('?'),
                "colon" => Some(':'),
                "at" => Some('@'),
                "hash" | "pound" => Some('#'),
                "dollar" => Some('$'),
                "percent" => Some('%'),
                "caret" => Some('^'),
                "ampersand" => Some('&'),
                "asterisk" | "star" => Some('*'),
                "lparen" | "leftparen" => Some('('),
                "rparen" | "rightparen" => Some(')'),
                "underscore" => Some('_'),
                "pipe" => Some('|'),
                "braceleft" | "lbrace" => Some('{'),
                "braceright" | "rbrace" => Some('}'),
                _ => None,
            };
            if let Some(ch) = alias {
                return char_key(ch);
            }
            // preserve original case for single-character keys ("A" vs "a")
            let mut it = name.chars();
            let (first, rest) = (it.next()?, it.next());
            if rest.is_none() {
                return char_key(first);
            }
            return None;
        }
    };
    Some((named.0, false, named.1))
}

/// Extended keys need KEYEVENTF_EXTENDEDKEY for correct scancode semantics.
fn is_extended_vk(vk: u16) -> bool {
    matches!(
        vk,
        0x21..=0x28 | 0x2D | 0x2E | 0x5B..=0x5D | 0x6F | 0x90 | 0x2C | 0x13
    )
}

#[derive(Debug, Clone)]
pub struct KeyCombo {
    pub modifiers: Vec<u16>,
    pub key: (u16, Option<u16>), // (vk, scan)
    pub ch: Option<u16>,         // WM_CHAR payload for message-level input
    pub text: Option<String>,    // intended printable text (IME-safe injection)
}

pub fn parse_combo(spec: &str) -> Result<KeyCombo> {
    let parts: Vec<&str> = spec.split('+').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        bail!("empty key spec");
    }
    let (last, mods) = parts.split_last().unwrap();
    let mut vms: Vec<u16> = Vec::new();
    for m in mods {
        let low = m.to_ascii_lowercase();
        if ["win", "meta", "logo"].contains(&low.as_str()) {
            bail!("the Windows-logo key is refused by the no-negotiable safety policy (Qoder rule): unpredictable focus stealing");
        }
        match named_vk(m) {
            Some((vk, _, _)) => vms.push(vk),
            None => bail!("unknown modifier '{m}'"),
        }
    }
    let user_shift = vms.contains(&(VK_SHIFT.0 as u16));
    let (vk, needs_shift, ch) = named_vk(last).ok_or_else(|| anyhow!("unknown key '{last}'"))?;
    if needs_shift && !user_shift {
        vms.push(VK_SHIFT.0 as u16);
    }
    // Bare printable keys become text injection (unicode): with an IME active,
    // raw letter key events only start composition and never reach the edit.
    // Bare printable keys -> text injection. Explicit modifiers stay real
    // key gestures (shortcuts), even if an IME would consume them.
    let text = if mods.is_empty() {
        ch.and_then(|u| char::from_u32(u as u32)).and_then(|c| {
            if c.is_control() {
                return None;
            }
            Some(c.to_string())
        })
    } else {
        None
    };
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16 };
    Ok(KeyCombo {
        modifiers: vms,
        key: (vk, Some(scan)),
        ch,
        text,
    })
}

pub fn press_combo(combo: &KeyCombo) -> Result<()> {
    let mut down: Vec<INPUT> = Vec::new();
    let mut up: Vec<INPUT> = Vec::new();
    for m in &combo.modifiers {
        down.push(kb_input(*m, 0, 0));
    }
    let ext = if is_extended_vk(combo.key.0) { KEYEVENTF_EXTENDEDKEY.0 } else { 0 };
    down.push(kb_input(combo.key.0, combo.key.1.unwrap_or(0), ext));
    up.insert(0, kb_input(combo.key.0, combo.key.1.unwrap_or(0), KEYEVENTF_KEYUP.0 | ext));
    for m in combo.modifiers.iter().rev() {
        up.push(kb_input(*m, 0, KEYEVENTF_KEYUP.0));
    }
    let mut all = down;
    all.extend(up);
    let n = unsafe { SendInput(&all, std::mem::size_of::<INPUT>() as i32) };
    if n != all.len() as u32 {
        bail!("SendInput key partial ({n}/{})", all.len());
    }
    Ok(())
}

pub fn type_text(text: &str) -> Result<()> {
    // KEYEVENTF_UNICODE per UTF-16 unit; newlines become Enter presses (Safer than CRLF injection)
    let mut events = 0usize;
    for ch in text.chars() {
        if ch == '\n' {
            let combo = parse_combo("enter")?;
            press_combo(&combo)?;
            events += 1;
            continue;
        }
        if ch == '\r' {
            continue;
        }
        let mut buf = [0u16; 2];
        for unit in ch.encode_utf16(&mut buf) {
            let down = kb_input(0, *unit, KEYEVENTF_UNICODE.0);
            let up = kb_input(0, *unit, KEYEVENTF_UNICODE.0 | KEYEVENTF_KEYUP.0);
            let n = unsafe { SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32) };
            if n != 2 {
                bail!("SendInput unicode failed");
            }
            events += 1;
        }
    }
    if events == 0 {
        bail!("nothing to type");
    }
    Ok(())
}

// ---------------- message-level input (PostMessage) ----------------
// For non-input desktops (sandbox): SendInput is blocked there, but window messages
// are deliverable. Covers plain typing into edit-like controls and mouse clicks.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    ChildWindowFromPointEx, CWP_SKIPINVISIBLE, PostMessageW, WM_CHAR, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP,
};

/// Find the best child control for message-level typing (EDIT-like), else the window itself.
pub fn find_input_target(top: HWND) -> HWND {
    unsafe {
        let mut best: isize = 0;
        let ctx = &mut best as *mut isize;
        let _ = windows::Win32::UI::WindowsAndMessaging::EnumChildWindows(
            top,
            Some(find_edit_cb),
            windows::Win32::Foundation::LPARAM(ctx as isize),
        );
        if best != 0 {
            HWND(best as *mut _)
        } else {
            top
        }
    }
}

unsafe extern "system" fn find_edit_cb(hwnd: HWND, lparam: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::BOOL {
    let class = crate::win::window_class(hwnd);
    let cl = class.to_ascii_lowercase();
    if cl.contains("edit") || cl.contains("richedit") || cl.contains("renderwidget") || cl.contains("document") {
        let p = lparam.0 as *mut isize;
        if !p.is_null() && *p == 0 {
            *p = hwnd.0 as isize;
        }
    }
    windows::Win32::Foundation::BOOL(1)
}

pub fn post_type(target: HWND, text: &str) -> Result<usize> {
    let mut n = 0usize;
    unsafe {
        for ch in text.chars() {
            if ch == '\n' {
                let _ = PostMessageW(target, WM_CHAR, WPARAM(13usize), LPARAM(0));
                n += 1;
                continue;
            }
            for unit in ch.encode_utf16(&mut [0u16; 2]) {
                let _ = PostMessageW(target, WM_CHAR, WPARAM(*unit as usize), LPARAM(0));
                n += 1;
            }
        }
    }
    if n == 0 {
        bail!("nothing to type");
    }
    Ok(n)
}

pub fn post_key(target: HWND, combo: &KeyCombo) -> Result<()> {
    unsafe {
        let (vk, scan) = (combo.key.0, combo.key.1.unwrap_or(0));
        let ext = if is_extended_vk(vk) { 1usize } else { 0 };
        let lp_down = (1usize) | ((scan as usize) << 16) | (ext << 24);
        let lp_up = lp_down | (1 << 30) | (1 << 31);
        for m in &combo.modifiers {
            let s = windows::Win32::UI::Input::KeyboardAndMouse::MapVirtualKeyW(*m as u32, MAPVK_VK_TO_VSC) as usize;
            let _ = PostMessageW(target, WM_KEYDOWN, WPARAM(*m as usize), LPARAM((1 | (s << 16)) as isize));
        }
        let _ = PostMessageW(target, WM_KEYDOWN, WPARAM(vk as usize), LPARAM(lp_down as isize));
        // printable input: also send WM_CHAR so edit-like controls receive it.
        // Skip when ctrl/alt are held: those are shortcuts, not text.
        let has_command_mod = combo
            .modifiers
            .iter()
            .any(|m| *m == VK_CONTROL.0 as u16 || *m == VK_MENU.0 as u16);
        if !has_command_mod {
            if let Some(mut ch) = combo.ch {
                if combo.modifiers.contains(&(VK_SHIFT.0 as u16)) {
                    if let Some(c) = char::from_u32(ch as u32) {
                        let up = c.to_ascii_uppercase();
                        if up.is_ascii_alphabetic() {
                            ch = up as u16;
                        }
                    }
                }
                let _ = PostMessageW(target, WM_CHAR, WPARAM(ch as usize), LPARAM(lp_down as isize));
            }
        }
        let _ = PostMessageW(target, WM_KEYUP, WPARAM(vk as usize), LPARAM(lp_up as isize));
        for m in combo.modifiers.iter().rev() {
            let s = windows::Win32::UI::Input::KeyboardAndMouse::MapVirtualKeyW(*m as u32, MAPVK_VK_TO_VSC) as usize;
            let _ = PostMessageW(target, WM_KEYUP, WPARAM(*m as usize), LPARAM((1 | (s << 16) | (1 << 30) | (1 << 31)) as isize));
        }
    }
    Ok(())
}

pub fn post_click(top: HWND, x: i32, y: i32, right: bool, double: bool) -> Result<()> {
    unsafe {
        // ChildWindowFromPointEx takes PARENT-CLIENT coords; then rebase to the child.
        let mut pt = windows::Win32::Foundation::POINT { x, y };
        let _ = ScreenToClient(top, &mut pt);
        let child = ChildWindowFromPointEx(top, pt, CWP_SKIPINVISIBLE);
        let (target, client) = if !child.is_invalid() && child != top {
            let mut cp = windows::Win32::Foundation::POINT { x, y };
            let _ = ScreenToClient(child, &mut cp);
            (child, cp)
        } else {
            (top, pt)
        };
        let mk: u32 = if right { 0x0002 } else { 0x0001 }; // MK_RBUTTON / MK_LBUTTON
        let (down, up, dbl) = if right {
            (WM_RBUTTONDOWN, WM_RBUTTONUP, WM_RBUTTONDOWN)
        } else {
            (WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK)
        };
        let lp = LPARAM(((client.y as isize) << 16) | (client.x as isize & 0xFFFF));
        let _ = PostMessageW(target, WM_MOUSEMOVE, WPARAM(0), lp);
        let msg_down = if double { dbl } else { down };
        let _ = PostMessageW(target, msg_down, WPARAM(mk as usize), lp);
        let _ = PostMessageW(target, up, WPARAM(0), lp);
        if double {
            let _ = PostMessageW(target, msg_down, WPARAM(mk as usize), lp);
            let _ = PostMessageW(target, up, WPARAM(0), lp);
        }
    }
    Ok(())
}

const MK_LBUTTON_DOWN: usize = 0x0001; // MK_LBUTTON while button held

/// Message-level wheel scroll (for sandbox desktops where SendInput is blocked).
/// dy>0 scrolls up (wheel away), matching WHEEL_DELTA=120 per notch.
pub fn post_scroll(top: HWND, x: i32, y: i32, dx: i32, dy: i32) -> Result<()> {
    use windows::Win32::UI::WindowsAndMessaging::{WM_MOUSEWHEEL, WM_MOUSEHWHEEL};
    unsafe {
        let mut pt = windows::Win32::Foundation::POINT { x, y };
        let _ = ScreenToClient(top, &mut pt);
        let child = ChildWindowFromPointEx(top, pt, CWP_SKIPINVISIBLE);
        let (target, client) = if !child.is_invalid() && child != top {
            let mut cp = windows::Win32::Foundation::POINT { x, y };
            let _ = ScreenToClient(child, &mut cp);
            (child, cp)
        } else {
            (top, pt)
        };
        let steps = dy.abs().max(dx.abs()).max(1);
        for _ in 0..steps {
            if dy != 0 {
                let wp = (((dy.signum() * 120) as usize) << 16) | 0; // delta<<16, no keys
                let lp = LPARAM(((client.y as isize) << 16) | (client.x as isize & 0xFFFF));
                let _ = PostMessageW(target, WM_MOUSEWHEEL, WPARAM(wp), lp);
            }
            if dx != 0 {
                let wp = (((dx.signum() * 120) as usize) << 16) | 0;
                let lp = LPARAM(((client.y as isize) << 16) | (client.x as isize & 0xFFFF));
                let _ = PostMessageW(target, WM_MOUSEHWHEEL, WPARAM(wp), lp);
            }
        }
    }
    Ok(())
}

/// Message-level drag: button down, interpolated moves with button held, button up.
pub fn post_drag(top: HWND, x1: i32, y1: i32, x2: i32, y2: i32) -> Result<()> {
    unsafe {
        let mut pt = windows::Win32::Foundation::POINT { x: x1, y: y1 };
        let _ = ScreenToClient(top, &mut pt);
        let child = ChildWindowFromPointEx(top, pt, CWP_SKIPINVISIBLE);
        let target = if !child.is_invalid() && child != top { child } else { top };
        let to_client = |x: i32, y: i32| -> (i32, i32) {
            let mut p = windows::Win32::Foundation::POINT { x, y };
            let _ = ScreenToClient(target, &mut p);
            (p.x, p.y)
        };
        let (cx1, cy1) = to_client(x1, y1);
        let lp = LPARAM(((cy1 as isize) << 16) | (cx1 as isize & 0xFFFF));
        let _ = PostMessageW(target, WM_MOUSEMOVE, WPARAM(0), lp);
        let _ = PostMessageW(target, WM_LBUTTONDOWN, WPARAM(MK_LBUTTON_DOWN), lp);
        let steps = 20;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let (cx, cy) = to_client(
                (x1 as f32 + (x2 - x1) as f32 * t) as i32,
                (y1 as f32 + (y2 - y1) as f32 * t) as i32,
            );
            let lp = LPARAM(((cy as isize) << 16) | (cx as isize & 0xFFFF));
            let _ = PostMessageW(target, WM_MOUSEMOVE, WPARAM(MK_LBUTTON_DOWN), lp);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let (cx2, cy2) = to_client(x2, y2);
        let lp = LPARAM(((cy2 as isize) << 16) | (cx2 as isize & 0xFFFF));
        let _ = PostMessageW(target, WM_LBUTTONUP, WPARAM(0), lp);
    }
    Ok(())
}
