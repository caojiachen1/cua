//! Global pointer preview: a click-through topmost overlay that shows a fake
//! cursor wherever an agent action is about to land, what it is doing while it
//! runs, and what it did afterwards — for the global desktop commands, for
//! actions inside a Child Session, and (separately, in-page) for the browser.
//! The dot is pinned to the exact target (bubble flips sides near screen
//! edges), colour-coded by phase (amber 即将 / blue 正在 / green 已 / red ✗,
//! same palette as the browser cursor), and fades out ~8 s after the last
//! action so nothing stays parked on screen.
//!
//! Architecture: `cua preview serve` is a detached resident process that owns
//! one layered window and listens on a loopback UDP port (persisted in
//! `preview/preview-<session>.json` together with its pid). Every mutating
//! command in `dispatch` emits two packets — `pre` ("即将 …", async) before
//! the action and `post` ("已 …" / "✗ …", synchronous — the CLI is about to
//! exit, so a detached send would be killed first) — fire-and-forget, the
//! same way the browser's CUA_CURSOR works. The overlay can never fail the
//! action it decorates: UDP delivery is best-effort and every send is
//! swallowed.
//!
//! Session scoping: the port file name carries `CUA_SESSION`, so a Child
//! Session's agent spawns its OWN host inside the child desktop (where the
//! read-only RDP viewer shows it), while the parent's host stays on the user's
//! desktop. Sandbox commands are skipped on purpose — the hidden desktop is
//! invisible, so a pointer preview there would be a lie.

use serde_json::{json, Value};
use std::io::Write as _;
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
    /// the element index for an a11y action. When present the emitter resolves
    /// the element's own rect centre from the cached observation, so a UIA
    /// click previews where it actually lands (the pointer never moves there).
    pub index: Option<usize>,
    /// delivery type shown in the bubble: 键鼠 (real SendInput), UIA
    /// (accessibility patterns, the real pointer never moves), 消息 (posted
    /// window messages), 窗口 (window management), 系统 (ShellExecute/process
    /// launch), 剪贴板 (clipboard write)
    pub tag: &'static str,
    /// where the POST packet should anchor the dot, when the action ends
    /// somewhere other than it started (drag: the release point). Not
    /// serialized — consumed by `post_from` only.
    pub post_xy: Option<(i32, i32)>,
}

impl Spec {
    pub fn to_packet(&self) -> Value {
        json!({
            "phase": self.phase,
            "verb": self.verb,
            "tag": self.tag,
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
/// every send is sub-millisecond and best-effort. Returns a handle the caller
/// can settle on — fast actions (clipboard, set-value) finish so quickly that
/// a detached pre thread would otherwise be killed by process exit before its
/// send ran.
pub fn emit(spec: Spec) -> PreEmit {
    // async is safe for the pre packet as long as the caller settles before
    // returning: dispatch keeps running through the action
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        deliver(spec);
        let _ = tx.send(());
    });
    PreEmit { done: rx }
}

/// Handle to the in-flight pre packet. `settle` waits briefly for it so a
/// fast action does not tear the thread down before the UDP send ran, and so
/// the pre is always on the wire before the post.
pub struct PreEmit {
    done: std::sync::mpsc::Receiver<()>,
}

impl PreEmit {
    pub fn settle(&self, timeout: std::time::Duration) {
        let _ = self.done.recv_timeout(timeout);
    }
}

/// Synchronous variant for the post packet. The CLI is about to exit by the
/// time the post packet is emitted — a detached thread would be killed by
/// `process::exit` before its `send_to` ran, so post frames ("已 …"/"✗ …")
/// never reached the host and the overlay froze on the amber "即将" frame.
pub fn emit_sync(spec: Spec) {
    deliver(spec);
}

fn deliver(spec: Spec) {
    if !enabled() {
        return;
    }
    // a11y actions carry no coordinates: resolve the *element's* rect centre
    // from the cached observation so the dot lands where UIA will actually
    // click, not on the window centre. Falls back to the window centre when
    // there is no index, no cache, or the element is off-screen/zero-sized.
    let spec = match (&spec.window_sel, spec.x, spec.y) {
        (Some(sel), None, None) => {
            let resolved = match crate::win::resolve_window(sel) {
                Ok(hwnd) => element_centre(hwnd.0 as isize, spec.index).or_else(|| {
                    crate::win::window_rect(hwnd)
                        .ok()
                        .map(|[rx, ry, rw, rh]| anchored_centre(rx, ry, rw, rh))
                }),
                Err(_) => None,
            };
            match resolved {
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
        // another emitter (the pre/post pair race ~a few ms apart) may
        // already be spawning the host — poll briefly before spawning a
        // second one; two hosts mean takeover kills the one holding the
        // half-drawn frame and the post packet lands on the wrong port
        for _ in 0..8 {
            std::thread::sleep(Duration::from_millis(150));
            if let Some((port, pid)) = read_port() {
                if pid_alive(pid) {
                    return Some(port);
                }
            }
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
    if spec.phase == "pre" {
        std::thread::sleep(Duration::from_millis(250));
        let _ = sock.send_to(payload.as_bytes(), &dst);
    }
}

/// Clamp a rect centre into the visible virtual screen: minimized and
/// off-screen windows report coords like -32000, and a preview anchored there
/// would be invisible. Off-screen centres snap to the virtual-screen centre.
fn virtual_screen() -> (i32, i32, i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}

fn anchored_centre(rx: i32, ry: i32, rw: i32, rh: i32) -> (i32, i32) {
    let (vx, vy, vw, vh) = virtual_screen();
    let (cx, cy) = (rx + rw / 2, ry + rh / 2);
    if cx < vx || cx > vx + vw || cy < vy || cy > vy + vh {
        return (vx + vw / 2, vy + vh / 2);
    }
    (cx, cy)
}

/// Centre of the element with `index` in the cached observation for `hwnd`.
///
/// This is what makes a UIA preview honest: `cua click --index 12` never moves
/// the pointer, so a dot on the window centre would be a lie. The cache is the
/// same `elements-<hwnd>.json` the action path resolves indices against.
///
/// Returns None when there is no index, no cache, no matching element, or the
/// element reports a degenerate rect (some virtualized list items do) — the
/// caller then falls back to the window centre.
fn element_centre(hwnd: isize, index: Option<usize>) -> Option<(i32, i32)> {
    let index = index? as i64;
    let cache = crate::util::load_elements(hwnd)?;
    centre_from_cache(&cache, index)
}

/// Pure part of `element_centre`: find `index` in a cached element array and
/// return its rect centre. Split out so it can be tested without touching the
/// on-disk cache.
fn centre_from_cache(cache: &Value, index: i64) -> Option<(i32, i32)> {
    let arr = cache.as_array()?;
    let node = arr
        .iter()
        .find(|n| n.get("i").and_then(|v| v.as_i64()) == Some(index))?;
    let r = node.get("rect")?.as_array()?;
    if r.len() < 4 {
        return None;
    }
    let (x, y) = (r[0].as_i64()? as i32, r[1].as_i64()? as i32);
    let (w, h) = (r[2].as_i64()? as i32, r[3].as_i64()? as i32);
    // a zero/negative extent means UIA could not measure the element
    if w <= 0 || h <= 0 {
        return None;
    }
    Some(anchored_centre(x, y, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn element_centre_uses_the_element_rect_not_the_window() {
        // A menu bar at the window's top-left corner: the preview must follow
        // the element (256,256), never the window centre (1188,729). This is
        // the regression that made every UIA preview sit in the screen centre.
        let cache = json!([
            {"i": 0, "rect": [228, 228, 1921, 1002]},
            {"i": 10, "rect": [239, 239, 34, 34]},
        ]);
        assert_eq!(centre_from_cache(&cache, 10), Some((256, 256)));
        assert_eq!(centre_from_cache(&cache, 0), Some((1188, 729)));
    }

    #[test]
    fn element_centre_rejects_missing_and_degenerate_rects() {
        let cache = json!([
            {"i": 1, "rect": [10, 10, 0, 0]},
            {"i": 2, "rect": [10, 10, 40]},
            {"i": 3},
        ]);
        assert_eq!(centre_from_cache(&cache, 1), None, "zero-size element");
        assert_eq!(centre_from_cache(&cache, 2), None, "short rect array");
        assert_eq!(centre_from_cache(&cache, 3), None, "no rect at all");
        assert_eq!(centre_from_cache(&cache, 99), None, "index not in cache");
    }
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
    // real key input lands on the focused window, so its centre is the honest
    // anchor for window-less key/type actions — clamped into the visible
    // screen because a minimized foreground window reports coords like -32000.
    // With NO foreground window at all (e.g. an idle RDP child session) the
    // screen centre is the fallback: the action must still be previewed.
    let fg = || -> Option<(i32, i32)> {
        let hwnd = crate::win::foreground_hwnd();
        if !hwnd.is_invalid() {
            if let Ok([rx, ry, rw, rh]) = crate::win::window_rect(hwnd) {
                return Some(anchored_centre(rx, ry, rw, rh));
            }
        }
        let (vx, vy, vw, vh) = virtual_screen();
        Some((vx + vw / 2, vy + vh / 2))
    };
    match cmd {
        // every arm below that carries a `sandbox` flag must gate on it: a
        // sandbox action runs on the hidden desktop, so a preview dot on the
        // user's real desktop would be a lie. `tag` says how the action will
        // actually be delivered — 键鼠 moves the real pointer/keyboard, UIA
        // drives accessibility patterns without touching the pointer, 消息
        // posts window messages, 窗口/系统 manage windows and processes.
        Cmd::Click { window, index, x, y, sandbox, double, background, delivery, .. } if !*sandbox => {
            let tag = if *background || delivery.as_deref() == Some("background") {
                "消息"
            } else if index.is_some() {
                "UIA"
            } else {
                "键鼠"
            };
            Some(Spec {
                phase: "pre",
                verb: if *double { "双击" } else { "点击" },
                tag,
                detail: d(window, *x, *y),
                x: *x,
                y: *y,
                click: true,
                ok: None,
                window_sel: window.clone(),
                index: *index,
                post_xy: None,
            })
        }
        Cmd::Hover { window, x, y, background, .. } => Some(Spec {
            phase: "pre",
            verb: "悬停",
            tag: if *background { "消息" } else { "键鼠" },
            detail: d(window, *x, *y),
            x: *x,
            y: *y,
            click: false,
            ok: None,
            window_sel: window.clone(),
                index: None,
            post_xy: None,
        }),
        Cmd::Scroll { dx: _, dy: _, x, y, window, sandbox, background, .. } if !*sandbox => Some(Spec {
            phase: "pre",
            verb: "滚动",
            tag: if *background { "消息" } else { "键鼠" },
            detail: d(window, *x, *y),
            x: *x,
            y: *y,
            click: false,
            ok: None,
            window_sel: window.clone(),
                index: None,
            post_xy: None,
        }),
        Cmd::Drag { x1, y1, x2, y2, sandbox, background, .. } if !*sandbox => Some(Spec {
            // pre anchors at the press point; the post packet re-anchors at the
            // release point via post_xy (a drag that keeps showing its start
            // would misreport where the input ended)
            phase: "pre",
            verb: "拖拽",
            tag: if *background { "消息" } else { "键鼠" },
            detail: format!("({x1},{y1}) → ({x2},{y2})"),
            x: Some(*x1),
            y: Some(*y1),
            click: false,
            ok: None,
            window_sel: None,
                index: None,
            post_xy: Some((*x2, *y2)),
        }),
        Cmd::Key { keys, window, sandbox, background, .. } if !*sandbox => {
            let (x, y) = if window.is_none() { fg().map(|(a, b)| (Some(a), Some(b))).unwrap_or((None, None)) } else { (None, None) };
            Some(Spec {
                phase: "pre",
                verb: "按键",
                tag: if *background { "消息" } else { "键鼠" },
                detail: keys.chars().take(24).collect(),
                x,
                y,
                click: false,
                ok: None,
                window_sel: window.clone(),
                index: None,
                post_xy: None,
            })
        }
        Cmd::Type { text, window, sandbox, background, .. } if !*sandbox => {
            let (x, y) = if window.is_none() { fg().map(|(a, b)| (Some(a), Some(b))).unwrap_or((None, None)) } else { (None, None) };
            Some(Spec {
                phase: "pre",
                verb: "输入",
                tag: if *background { "消息" } else { "键鼠" },
                detail: format!("{} ({})", window.clone().unwrap_or_default(), text.chars().count()).chars().take(44).collect(),
                x,
                y,
                click: false,
                ok: None,
                window_sel: window.clone(),
                index: None,
                post_xy: None,
            })
        }
        Cmd::SetValue { window, clear, sandbox, .. } if !*sandbox => Some(Spec {
            phase: "pre",
            verb: if *clear { "清空" } else { "输入" },
            tag: "UIA",
            detail: d(&Some(window.clone()), None, None),
            x: None,
            y: None,
            click: false,
            ok: None,
            window_sel: Some(window.clone()),
                index: None,
            post_xy: None,
        }),
        Cmd::Toggle { window, index, sandbox, .. } if !*sandbox => Some(Spec { phase: "pre", verb: "切换", tag: "UIA", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: Some(*index), post_xy: None }),
        Cmd::Select { window, index, sandbox, .. } if !*sandbox => Some(Spec { phase: "pre", verb: "选择", tag: "UIA", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: Some(*index), post_xy: None }),
        Cmd::SelectAdd { window, index, sandbox, .. } if !*sandbox => Some(Spec { phase: "pre", verb: "多选", tag: "UIA", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: Some(*index), post_xy: None }),
        Cmd::SelectRemove { window, index, sandbox, .. } if !*sandbox => Some(Spec { phase: "pre", verb: "取消选择", tag: "UIA", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: Some(*index), post_xy: None }),
        Cmd::Expand { window, index, sandbox, .. } if !*sandbox => Some(Spec { phase: "pre", verb: "展开", tag: "UIA", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: Some(*index), post_xy: None }),
        Cmd::Collapse { window, index, sandbox, .. } if !*sandbox => Some(Spec { phase: "pre", verb: "折叠", tag: "UIA", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: Some(*index), post_xy: None }),
        Cmd::SetRange { window, index, sandbox, .. } if !*sandbox => Some(Spec { phase: "pre", verb: "设值", tag: "UIA", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: Some(*index), post_xy: None }),
        Cmd::ScrollItem { window, index, sandbox, .. } if !*sandbox => Some(Spec { phase: "pre", verb: "滚动到", tag: "UIA", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: Some(*index), post_xy: None }),
        Cmd::Realize { window, index, .. } => Some(Spec { phase: "pre", verb: "展开", tag: "UIA", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: Some(*index), post_xy: None }),
        Cmd::Grid { window, row, col, invoke, .. } if *invoke && row.is_some() && col.is_some() => Some(Spec { phase: "pre", verb: "选中", tag: "UIA", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: None, post_xy: None }),
        Cmd::Transform { window, move_to, size, zoom, .. } if move_to.is_some() || size.is_some() || zoom.is_some() => Some(Spec { phase: "pre", verb: "变换", tag: "UIA", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: None, post_xy: None }),
        // window management: real desktop changes, but never the pointer
        Cmd::Close { window, .. } => Some(Spec { phase: "pre", verb: "关闭", tag: "窗口", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: None, post_xy: None }),
        Cmd::Activate { window, .. } => Some(Spec { phase: "pre", verb: "激活", tag: "窗口", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: None, post_xy: None }),
        Cmd::Minimize { window, .. } => Some(Spec { phase: "pre", verb: "最小化", tag: "窗口", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: None, post_xy: None }),
        Cmd::Maximize { window, .. } => Some(Spec { phase: "pre", verb: "最大化", tag: "窗口", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: None, post_xy: None }),
        Cmd::Restore { window, .. } => Some(Spec { phase: "pre", verb: "还原", tag: "窗口", detail: d(&Some(window.clone()), None, None), x: None, y: None, click: false, ok: None, window_sel: Some(window.clone()), index: None, post_xy: None }),
        Cmd::Resize { window, width, height, x, y, .. } => Some(Spec {
            phase: "pre",
            verb: "调整大小",
            tag: "窗口",
            detail: format!("{} → {}x{} @({},{})", window,
                width.map_or(-1, |v| v), height.map_or(-1, |v| v),
                x.map_or(-1, |v| v), y.map_or(-1, |v| v))
                .chars().take(44).collect(),
            x: None,
            y: None,
            click: false,
            ok: None,
            window_sel: Some(window.clone()),
                index: None,
            post_xy: None,
        }),
        // process launches: no pointer involved
        Cmd::Open { name, .. } => Some(Spec { phase: "pre", verb: "打开", tag: "系统", detail: name.chars().take(24).collect(), x: None, y: None, click: false, ok: None, window_sel: None, index: None, post_xy: None }),
        Cmd::Launch { exe, sandbox, .. } if !*sandbox => Some(Spec { phase: "pre", verb: "启动", tag: "系统", detail: exe.chars().take(24).collect(), x: None, y: None, click: false, ok: None, window_sel: None, index: None, post_xy: None }),
        Cmd::Clipboard { op: crate::ClipOp::Set { .. } } => {
            let (x, y) = fg().map(|(a, b)| (Some(a), Some(b))).unwrap_or((None, None));
            Some(Spec { phase: "pre", verb: "剪贴", tag: "剪贴板", detail: "写入剪贴板".into(), x, y, click: false, ok: None, window_sel: None, index: None, post_xy: None })
        }
        _ => None,
    }
}

/// The post packet mirrors the pre one with the outcome attached. When the
/// action ends somewhere else than it started (drag), the dot re-anchors at
/// `post_xy`.
pub fn post_from(pre: &Spec, ok: bool) -> Spec {
    let (x, y) = match pre.post_xy {
        Some((px, py)) => (Some(px), Some(py)),
        None => (pre.x, pre.y),
    };
    Spec {
        phase: "post",
        verb: pre.verb,
        tag: pre.tag,
        detail: pre.detail.clone(),
        x,
        y,
        click: pre.click,
        ok: Some(ok),
        window_sel: pre.window_sel.clone(),
                index: None,
        post_xy: None,
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
    // after the last packet the overlay holds still for a moment so the user
    // can read the outcome, then fades out — a parked fake cursor is residue
    const FADE_HOLD_MS: u64 = 8000;
    const FADE_MS: f64 = 600.0;
    // dot + bubble geometry (physical px): the dot is deliberately small —
    // it must mark a point, not cover it
    const DOT_R: f32 = 8.0;
    const BUBBLE_W: f32 = 300.0;
    const BUBBLE_H: f32 = 56.0;
    // the dot's home position inside the window bitmap
    const DOT_HOME: (i32, i32) = (70, 110);

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
        tag: String,
        detail: String,
        target: (i32, i32),
        current: (f64, f64),
        glide_from: (f64, f64),
        glide_start: std::time::Instant,
        click: bool,
        ok: Option<bool>,
        ripple_start: Option<std::time::Instant>,
        /// false until the first packet: the very first appearance must JUMP to
        /// its target, not glide in from the virtual-screen origin
        anchored: bool,
    }

    impl HostState {
        fn ripple_t(&self) -> f32 {
            match self.ripple_start {
                Some(t0) => ((t0.elapsed().as_millis() as f32) / 600.0).clamp(0.0, 1.0),
                None => 0.0,
            }
        }
    }

    /// Truncate to a pixel budget: CJK/fullwidth glyphs count ~2× an ASCII
    /// char. Keeps one-line details inside the bubble instead of wrapping.
    fn fit_width(s: &str, max_px: f32) -> String {
        let mut w = 0.0;
        let mut end = s.len();
        for (i, c) in s.char_indices() {
            let cw = if (c as u32) > 0x2E7F { 12.5 } else { 6.5 };
            if w + cw > max_px {
                end = i;
                break;
            }
            w += cw;
        }
        s[..end].to_string()
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
        state.tag = v
            .get("tag")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        state.detail = fit_width(
            v.get("detail").and_then(|s| s.as_str()).unwrap_or(""),
            BUBBLE_W - 28.0,
        );
        state.click = v.get("click").and_then(|c| c.as_bool()).unwrap_or(false);
        state.ok = v.get("ok").and_then(|o| o.as_bool());
        if let (Some(x), Some(y)) = (
            v.get("x").and_then(|n| n.as_i64()).map(|n| n as i32),
            v.get("y").and_then(|n| n.as_i64()).map(|n| n as i32),
        ) {
            state.glide_from = if state.anchored {
                state.current
            } else {
                (x as f64, y as f64) // first appearance: jump, don't glide
            };
            state.target = (x, y);
            state.anchored = true;
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
        if GdipCreateFont(family, 15.0, FontStyleRegular.0, UnitPixel, &mut font_main) != Status(0) {
            return None;
        }
        if GdipCreateFont(family, 12.0, FontStyleRegular.0, UnitPixel, &mut font_dim) != Status(0) {
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

    unsafe fn draw_frame(ui: &Ui, state: &mut HostState, dot: (i32, i32)) {
        // clear to fully transparent
        let n = (WINDOW_W * WINDOW_H * 4) as usize;
        std::ptr::write_bytes(ui.bits, 0, n);
        // one colour per phase, same palette as the browser's in-page cursor:
        // amber 即将 → blue 正在 → green 已, red is reserved for ✗
        let dot_c = match (state.phase, state.ok) {
            ("pre", _) => argb(255, 245, 158, 11),
            ("act", _) => argb(255, 59, 130, 246),
            ("post", Some(false)) => argb(255, 239, 68, 68),
            _ => argb(255, 34, 197, 94),
        };
        // bubble: right of the dot when it fits, otherwise flipped to the left;
        // above the dot unless that would leave the window
        let bx = {
            let right = dot.0 as f32 + 16.0;
            let left = dot.0 as f32 - 16.0 - BUBBLE_W;
            if right + BUBBLE_W <= (WINDOW_W - 4) as f32 {
                right
            } else {
                left.clamp(4.0, (WINDOW_W - 4) as f32 - BUBBLE_W)
            }
        };
        let mut by = dot.1 as f32 - 8.0 - BUBBLE_H;
        if by < 4.0 {
            by = dot.1 as f32 + 16.0;
        }
        let by = by.clamp(4.0, (WINDOW_H - 4) as f32 - BUBBLE_H);
        let _ = GdipFillRectangle(ui.graphics, ui.bubble_brush, bx, by, BUBBLE_W, BUBBLE_H);
        let mut pen: *mut GpPen = std::ptr::null_mut();
        if GdipCreatePen1(0x66000000, 1.0, UnitPixel, &mut pen) == Status(0) {
            let _ = GdipDrawRectangle(ui.graphics, pen, bx, by, BUBBLE_W, BUBBLE_H);
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
        let line1 = if state.tag.is_empty() {
            format!("{phase_word} {}\0", state.verb)
        } else {
            format!("{phase_word} [{}] {}\0", state.tag, state.verb)
        };
        let w1: Vec<u16> = line1.encode_utf16().collect();
        let w2: Vec<u16> = format!("{}\0", state.detail).encode_utf16().collect();
        let r1 = RectF { X: bx + 14.0, Y: by + 6.0, Width: BUBBLE_W - 24.0, Height: 24.0 };
        let _ = GdipDrawString(ui.graphics, PCWSTR::from_raw(w1.as_ptr()), (w1.len() - 1) as i32, ui.font_main, &r1, std::ptr::null(), ui.text_brush);
        let r2 = RectF { X: bx + 14.0, Y: by + 31.0, Width: BUBBLE_W - 24.0, Height: 20.0 };
        let _ = GdipDrawString(ui.graphics, PCWSTR::from_raw(w2.as_ptr()), (w2.len() - 1) as i32, ui.font_dim, &r2, std::ptr::null(), ui.dim_brush);
        // dot + white border
        let mut fill: *mut GpSolidFill = std::ptr::null_mut();
        if GdipCreateSolidFill(dot_c, &mut fill) == Status(0) {
            let _ = GdipFillEllipse(ui.graphics, fill as *mut GpBrush, (dot.0 as f32) - DOT_R, (dot.1 as f32) - DOT_R, DOT_R * 2.0, DOT_R * 2.0);
            GdipDeleteBrush(fill as *mut GpBrush);
        }
        let mut bpen: *mut GpPen = std::ptr::null_mut();
        if GdipCreatePen1(argb(255, 255, 255, 255), 2.0, UnitPixel, &mut bpen) == Status(0) {
            let _ = GdipDrawEllipse(ui.graphics, bpen, (dot.0 as f32) - DOT_R, (dot.1 as f32) - DOT_R, DOT_R * 2.0, DOT_R * 2.0);
            GdipDeletePen(bpen);
        }
        // success ripple
        if state.phase == "post" && state.ok != Some(false) && state.click {
            let t = state.ripple_t();
            let r = DOT_R + 28.0 * t;
            let a = (220.0 * (1.0 - t)) as u8;
            let mut rpen: *mut GpPen = std::ptr::null_mut();
            if GdipCreatePen1(argb(a, 34, 197, 94), 2.5, UnitPixel, &mut rpen) == Status(0) {
                let _ = GdipDrawEllipse(ui.graphics, rpen, (dot.0 as f32) - r, (dot.1 as f32) - r, r * 2.0, r * 2.0);
                GdipDeletePen(rpen);
            }
        }
        // GDI+ renders lazily — without the flush, ULW reads the freshly-cleared
        // (all-zero) pixels and the overlay stays invisible forever
        GdipFlush(ui.graphics, FlushIntentionFlush);
    }

    unsafe fn paint(hwnd: HWND, ui: &Ui, state: &mut HostState, alpha: u8) {
        // glide: ease-out cubic over GLIDE_MS
        let t = ((state.glide_start.elapsed().as_millis() as f64) / (super::GLIDE_MS as f64)).clamp(0.0, 1.0);
        let e = 1.0 - (1.0 - t).powi(3);
        state.current.0 = state.glide_from.0 + (state.target.0 as f64 - state.glide_from.0) * e;
        state.current.1 = state.glide_from.1 + (state.target.1 as f64 - state.glide_from.1) * e;
        // The dot must sit EXACTLY on the point the input will land on. Placing
        // the window at target-DOT_HOME and clamping that into the virtual
        // screen would drag the dot hundreds of px away from edge targets, so
        // instead the window is clamped first and the dot is drawn wherever the
        // target then falls inside the bitmap — the dot is pinned, the window
        // follows.
        let vs_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vs_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vs_w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vs_h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        let dst_x = (state.current.0 as i32 - DOT_HOME.0).clamp(vs_x, vs_x + vs_w - WINDOW_W);
        let dst_y = (state.current.1 as i32 - DOT_HOME.1).clamp(vs_y, vs_y + vs_h - WINDOW_H);
        let dot = (
            state.current.0 as i32 - dst_x,
            state.current.1 as i32 - dst_y,
        );
        draw_frame(ui, state, dot);
        GdiFlush();
        let dst = POINT { x: dst_x, y: dst_y };
        let size = SIZE { cx: WINDOW_W, cy: WINDOW_H };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: 0,     // AC_SRC_OVER
            BlendFlags: 0,
            // whole-window alpha: the idle fade-out ramps this to 0
            SourceConstantAlpha: alpha,
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
        let pid = std::process::id();
        // take over from an orphaned host: a second `preview serve` would
        // otherwise leave the old one topmost on screen with a frozen frame it
        // can never repaint (nothing sends to its port any more)
        if let Some((old_port, old_pid)) = read_port() {
            if old_pid != pid && pid_alive(old_pid) {
                if let Ok(s) = UdpSocket::bind("127.0.0.1:0") {
                    let _ = s.send_to(b"{\"phase\":\"shutdown\"}", format!("127.0.0.1:{old_port}"));
                    for _ in 0..6 {
                        std::thread::sleep(Duration::from_millis(100));
                        if !pid_alive(old_pid) {
                            break;
                        }
                    }
                }
            }
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
                            // forward and let the pump quit: exiting here would
                            // race (and lose) the pump's window/GDI+ teardown
                            // and state-file cleanup
                            let _ = tx.send(v);
                            return;
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
                tag: String::new(),
                detail: String::new(),
                target: (0, 0),
                current: (0.0, 0.0),
                glide_from: (0.0, 0.0),
                glide_start: std::time::Instant::now() - Duration::from_secs(3600),
                click: false,
                ok: Some(true),
                ripple_start: None,
                anchored: false,
            };
            // nothing is painted until the first packet arrives: a layered
            // window that has never seen ULW is not composited at all, so a
            // freshly-started host leaves the screen clean instead of showing
            // a stray "已" frame at the virtual-screen origin
            let mut has_packet = false;
            let mut last_packet = std::time::Instant::now();
            let mut last_alpha = 255u8;
            let mut idle = std::time::Instant::now();
            let mut quit = false;
            loop {
                let mut got = false;
                while let Ok(v) = rx.try_recv() {
                    idle = std::time::Instant::now();
                    if v.get("phase").and_then(|p| p.as_str()) == Some("shutdown") {
                        quit = true;
                        break;
                    }
                    apply(&mut state, &v);
                    got = true;
                }
                if quit {
                    break;
                }
                if got {
                    last_packet = std::time::Instant::now();
                    has_packet = true;
                }
                // repaint only while something actually moves: a packet just
                // landed, the dot is gliding, a ripple is expanding, or the
                // idle fade is running. A settled overlay costs no GDI+ work.
                // Packets without coordinates (bare key/type) never paint
                // before the dot has a real anchor — a corner (0,0) frame
                // would be a lie about where the input goes.
                if has_packet && state.anchored {
                    let glide_active = state.glide_start.elapsed().as_millis() < GLIDE_MS;
                    let ripple_active = state
                        .ripple_start
                        .map_or(false, |t| t.elapsed().as_millis() < 600);
                    let since = last_packet.elapsed().as_millis();
                    let fade_t = if since > FADE_HOLD_MS as u128 + FADE_MS as u128 {
                        1.0
                    } else if since > FADE_HOLD_MS as u128 {
                        (since - FADE_HOLD_MS as u128) as f64 / FADE_MS
                    } else {
                        0.0
                    };
                    let alpha = (255.0 * (1.0 - fade_t)) as u8;
                    if got
                        || glide_active
                        || ripple_active
                        || (fade_t > 0.0 && alpha != last_alpha)
                    {
                        paint(ui.hwnd, &ui, &mut state, alpha);
                        last_alpha = alpha;
                    }
                }
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
