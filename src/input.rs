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
    VK_DOWN, VK_END, VK_ESCAPE, VK_F4, VK_HOME, VK_INSERT, VK_LEFT, VK_MENU, VK_MULTIPLY, VK_NEXT,
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
        bail!("SendInput move failed: {}", windows::core::Error::from_win32());
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

fn send_inputs(inputs: &[INPUT], what: &str) -> Result<()> {
    let n = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if n != inputs.len() as u32 {
        bail!("SendInput {what} partial/failed ({n}/{})", inputs.len());
    }
    Ok(())
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
        send_inputs(&[mouse_input(down, 0, 0, 0)], "button down")?;
        std::thread::sleep(std::time::Duration::from_millis(25));
        send_inputs(&[mouse_input(up, 0, 0, 0)], "button up")?;
        if i + 1 < times {
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
    }
    Ok(())
}

/// Independent per-axis wheel notches; the old shared max-loop sent both axes
/// `max(|dx|,|dy|)` times, so `dx=1,dy=5` scrolled horizontally five notches.
fn wheel_events(dx: i32, dy: i32) -> Vec<(bool, u32)> {
    const WHEEL_DELTA: u32 = 120;
    let mut out = Vec::new();
    for _ in 0..dy.abs() {
        out.push((false, (dy.signum() as i32 * WHEEL_DELTA as i32) as u32));
    }
    for _ in 0..dx.abs() {
        out.push((true, (dx.signum() as i32 * WHEEL_DELTA as i32) as u32));
    }
    out
}

pub fn scroll(x: i32, y: i32, dx: i32, dy: i32) -> Result<()> {
    move_to(x, y)?;
    std::thread::sleep(std::time::Duration::from_millis(30));
    let mut inputs = Vec::new();
    for (horizontal, data) in wheel_events(dx, dy) {
        inputs.push(INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: data,
                    dwFlags: MOUSE_EVENT_FLAGS(if horizontal { MOUSEEVENTF_HWHEEL.0 } else { MOUSEEVENTF_WHEEL.0 }),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }
    if inputs.is_empty() {
        bail!("scroll needs a non-zero dx or dy");
    }
    send_inputs(&inputs, "scroll")
}

pub fn drag(x1: i32, y1: i32, x2: i32, y2: i32) -> Result<()> {
    move_to(x1, y1)?;
    std::thread::sleep(std::time::Duration::from_millis(80));
    send_inputs(&[mouse_input(MouseFlags::DOWN_L, 0, 0, 0)], "drag button down")?;
    let finish = |why: Option<anyhow::Error>| -> Result<()> {
        // A failed step must not leave the physical button held down.
        let up = send_inputs(&[mouse_input(MouseFlags::UP_L, 0, 0, 0)], "drag button up");
        match why {
            Some(e) => Err(e),
            None => up,
        }
    };
    std::thread::sleep(std::time::Duration::from_millis(80));
    let steps = 24;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let x = (x1 as f32 + (x2 - x1) as f32 * t) as i32;
        let y = (y1 as f32 + (y2 - y1) as f32 * t) as i32;
        if let Err(e) = move_to(x, y) {
            return finish(Some(e));
        }
        std::thread::sleep(std::time::Duration::from_millis(12));
    }
    std::thread::sleep(std::time::Duration::from_millis(60));
    finish(None)
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

/// Always-Confirm level (Qoder policy): alt+f4 closes windows regardless of
/// modifier/key order or spacing, so it is matched on the parsed combo.
pub fn is_alt_f4(combo: &KeyCombo) -> bool {
    combo.modifiers.contains(&(VK_MENU.0 as u16)) && combo.key.0 == VK_F4.0 as u16
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
    // No-negotiables matched on the parsed combo (a raw string guard would miss "alt + f4").
    // System surfaces that must never be automated even with --confirm:
    if vms.contains(&(VK_CONTROL.0 as u16))
        && vms.contains(&(VK_SHIFT.0 as u16))
        && vk == VK_ESCAPE.0 as u16
    {
        bail!("refused by safety policy: ctrl+shift+esc opens Task Manager, which is on the no-negotiable deny list");
    }
    if vms.contains(&(VK_CONTROL.0 as u16))
        && vms.contains(&(VK_MENU.0 as u16))
        && vk == VK_DELETE.0 as u16
    {
        bail!("refused by safety policy: ctrl+alt+del is a secure attention sequence and cannot be automated");
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
    if cl.contains("edit") || cl.contains("richedit") || cl.contains("renderwidget") || cl.contains("document") || cl.contains("scintilla") {
        let p = lparam.0 as *mut isize;
        if !p.is_null() && *p == 0 {
            *p = hwnd.0 as isize;
        }
    }
    windows::Win32::Foundation::BOOL(1)
}

/// The child window that should receive mouse messages for a screen point
/// (shared by post_click / post_scroll / post_drag / post_type_at_point).
fn child_at_point(top: HWND, x: i32, y: i32) -> (HWND, i32, i32) {
    unsafe {
        let mut pt = windows::Win32::Foundation::POINT { x, y };
        let _ = ScreenToClient(top, &mut pt);
        let child = ChildWindowFromPointEx(top, pt, CWP_SKIPINVISIBLE);
        if !child.is_invalid() && child != top {
            let mut cp = windows::Win32::Foundation::POINT { x, y };
            let _ = ScreenToClient(child, &mut cp);
            (child, cp.x, cp.y)
        } else {
            (top, pt.x, pt.y)
        }
    }
}

fn post(hwnd: HWND, msg: u32, w: usize, l: isize, what: &str) -> Result<()> {
    unsafe { PostMessageW(hwnd, msg, WPARAM(w), LPARAM(l)) }
        .map_err(|e| anyhow!("PostMessageW {what} ({msg:#x}) failed: {e}"))
}

pub fn post_type(target: HWND, text: &str) -> Result<usize> {
    let mut n = 0usize;
    for ch in text.chars() {
        if ch == '\n' {
            post(target, WM_CHAR, 13, 0, "WM_CHAR newline")
                .map_err(|e| anyhow!("{e} after {n} units"))?;
            n += 1;
            continue;
        }
        for unit in ch.encode_utf16(&mut [0u16; 2]) {
            post(target, WM_CHAR, *unit as usize, 0, "WM_CHAR")
                .map_err(|e| anyhow!("{e} after {n} units"))?;
            n += 1;
        }
    }
    if n == 0 {
        bail!("nothing to type");
    }
    Ok(n)
}

/// Message-level typing aimed at a specific screen point: the edit-like child under
/// the point gets the WM_CHAR stream, so a window with several input fields can be
/// addressed without raising it. Falls back to the first edit-like child.
pub fn post_type_at_point(top: HWND, x: i32, y: i32, text: &str) -> Result<usize> {
    let (child, _, _) = child_at_point(top, x, y);
    let target = if child == top { find_input_target(top) } else { child };
    post_type(target, text)
}

/// Message-level hover: a WM_MOUSEMOVE stream at the point, no cursor movement.
/// Only apps that react to hover messages without tracking the global pointer care.
pub fn post_hover(top: HWND, x: i32, y: i32) -> Result<()> {
    let (target, cx, cy) = child_at_point(top, x, y);
    let lp = ((cy as isize) << 16) | (cx as isize & 0xFFFF);
    post(target, WM_MOUSEMOVE, 0, lp, "WM_MOUSEMOVE")
}

pub fn post_key(target: HWND, combo: &KeyCombo) -> Result<()> {
    let (vk, scan) = (combo.key.0, combo.key.1.unwrap_or(0));
    let ext = if is_extended_vk(vk) { 1usize } else { 0 };
    let lp_down = (1usize) | ((scan as usize) << 16) | (ext << 24);
    let lp_up = lp_down | (1 << 30) | (1 << 31);
    for m in &combo.modifiers {
        let s = unsafe { MapVirtualKeyW(*m as u32, MAPVK_VK_TO_VSC) } as usize;
        post(target, WM_KEYDOWN, *m as usize, (1 | (s << 16)) as isize, "modifier keydown")?;
    }
    post(target, WM_KEYDOWN, vk as usize, lp_down as isize, "keydown")?;
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
            post(target, WM_CHAR, ch as usize, lp_down as isize, "WM_CHAR for key")?;
        }
    }
    post(target, WM_KEYUP, vk as usize, lp_up as isize, "keyup")?;
    for m in combo.modifiers.iter().rev() {
        let s = unsafe { MapVirtualKeyW(*m as u32, MAPVK_VK_TO_VSC) } as usize;
        post(
            target,
            WM_KEYUP,
            *m as usize,
            (1 | (s << 16) | (1 << 30) | (1 << 31)) as isize,
            "modifier keyup",
        )?;
    }
    Ok(())
}

pub fn post_click(top: HWND, x: i32, y: i32, right: bool, double: bool) -> Result<()> {
    let (target, client_x, client_y) = child_at_point(top, x, y);
    let mk: u32 = if right { 0x0002 } else { 0x0001 }; // MK_RBUTTON / MK_LBUTTON
    let (down, up, dbl) = if right {
        (WM_RBUTTONDOWN, WM_RBUTTONUP, WM_RBUTTONDOWN)
    } else {
        (WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK)
    };
    let lp = ((client_y as isize) << 16) | (client_x as isize & 0xFFFF);
    post(target, WM_MOUSEMOVE, 0, lp, "WM_MOUSEMOVE")?;
    post(target, down, mk as usize, lp, "button down")?;
    post(target, up, 0, lp, "button up")?;
    if double {
        // The documented double-click sequence is DOWN, UP, DBLCLK, UP. Sending
        // DBLCLK as both downs loses the initial press for controls that care.
        post(target, dbl, mk as usize, lp, "WM_LBUTTONDBLCLK")?;
        post(target, up, 0, lp, "button up (double)")?;
    }
    Ok(())
}

const MK_LBUTTON_DOWN: usize = 0x0001; // MK_LBUTTON while button held

/// Message-level wheel scroll (for sandbox desktops where SendInput is blocked).
/// dy>0 scrolls up (wheel away), matching WHEEL_DELTA=120 per notch.
pub fn post_scroll(top: HWND, x: i32, y: i32, dx: i32, dy: i32) -> Result<()> {
    use windows::Win32::UI::WindowsAndMessaging::{WM_MOUSEHWHEEL, WM_MOUSEWHEEL};
    if dx == 0 && dy == 0 {
        bail!("scroll needs a non-zero dx or dy");
    }
    let (target, _, _) = child_at_point(top, x, y);
    // Wheel messages carry screen coordinates in lParam (unlike the button
    // messages above), so keep the point the caller supplied.
    let lp = ((y as isize) << 16) | (x as isize & 0xFFFF);
    for (horizontal, data) in wheel_events(dx, dy) {
        let wp = ((data as usize) << 16) | 0; // delta<<16, no keys
        post(
            target,
            if horizontal { WM_MOUSEHWHEEL } else { WM_MOUSEWHEEL },
            wp,
            lp,
            "wheel",
        )?;
    }
    Ok(())
}

/// Message-level drag: button down, interpolated moves with button held, button up.
pub fn post_drag(top: HWND, x1: i32, y1: i32, x2: i32, y2: i32) -> Result<()> {
    let (target, cx1, cy1) = child_at_point(top, x1, y1);
    let to_client = |x: i32, y: i32| -> (i32, i32) {
        let mut p = windows::Win32::Foundation::POINT { x, y };
        unsafe {
            let _ = ScreenToClient(target, &mut p);
        }
        (p.x, p.y)
    };
    let lp = ((cy1 as isize) << 16) | (cx1 as isize & 0xFFFF);
    post(target, WM_MOUSEMOVE, 0, lp, "WM_MOUSEMOVE")?;
    post(target, WM_LBUTTONDOWN, MK_LBUTTON_DOWN, lp, "WM_LBUTTONDOWN")?;
    let mut interrupted: Option<anyhow::Error> = None;
    let steps = 20;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let (cx, cy) = to_client(
            (x1 as f32 + (x2 - x1) as f32 * t) as i32,
            (y1 as f32 + (y2 - y1) as f32 * t) as i32,
        );
        let lp = ((cy as isize) << 16) | (cx as isize & 0xFFFF);
        if let Err(e) = post(target, WM_MOUSEMOVE, MK_LBUTTON_DOWN, lp, "WM_MOUSEMOVE (drag)") {
            interrupted = Some(e);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Always release the logical button, even if a move failed.
    let (cx2, cy2) = to_client(x2, y2);
    let lp = ((cy2 as isize) << 16) | (cx2 as isize & 0xFFFF);
    let up = post(target, WM_LBUTTONUP, 0, lp, "WM_LBUTTONUP");
    match interrupted {
        Some(e) => Err(e),
        None => up,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shift_count(c: &KeyCombo) -> usize {
        c.modifiers.iter().filter(|m| **m == VK_SHIFT.0 as u16).count()
    }

    #[test]
    fn auto_shift_is_added_exactly_once() {
        let c = parse_combo("ctrl+A").unwrap();
        assert_eq!(shift_count(&c), 1, "modifiers={:?}", c.modifiers);
    }

    #[test]
    fn explicit_shift_is_not_duplicated() {
        let c = parse_combo("ctrl+shift+a").unwrap();
        assert_eq!(shift_count(&c), 1, "modifiers={:?}", c.modifiers);
    }

    #[test]
    fn alt_f4_is_recognised_regardless_of_spacing_or_order() {
        for spec in ["alt+f4", "alt + f4", "alt + F4", "ALT+F4"] {
            let c = parse_combo(spec).unwrap();
            assert!(is_alt_f4(&c), "{spec} -> {:?}", c.key);
        }
    }

    #[test]
    fn dangerous_combos_stay_refused() {
        for spec in ["win+r", "ctrl+shift+esc", "ctrl+alt+del"] {
            assert!(parse_combo(spec).is_err(), "{spec} should be refused");
        }
    }

    #[test]
    fn sticky_keys_never_become_text() {
        assert!(parse_combo("ctrl+A").unwrap().text.is_none());
        assert_eq!(parse_combo("A").unwrap().text.as_deref(), Some("A"));
        assert_eq!(parse_combo("enter").unwrap().text, None);
    }

    #[test]
    fn wheel_axes_are_independent() {
        let ev = wheel_events(1, 5);
        assert_eq!(ev.iter().filter(|(h, _)| !*h).count(), 5);
        assert_eq!(ev.iter().filter(|(h, _)| *h).count(), 1);
        let neg = wheel_events(-2, 0);
        assert_eq!(neg.len(), 2);
        assert!(neg.iter().all(|(h, d)| *h && *d == (-120i32) as u32));
    }

    #[test]
    fn extended_keys_are_flagged() {
        assert!(is_extended_vk(VK_HOME.0 as u16));
        assert!(is_extended_vk(VK_INSERT.0 as u16));
        assert!(!is_extended_vk(VK_SPACE.0 as u16));
    }
}
