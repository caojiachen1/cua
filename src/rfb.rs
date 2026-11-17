// rfb.rs — minimal RFB (VNC) protocol server in pure Rust.
// Reference behavior: TightVNC server (RFB 3.8 handshake, None auth, Raw
// encoding, client-driven framebuffer updates, pointer/key injection via
// SendInput into the *current* session).
//
// Runs as `cua rfb-server --port 5901` inside whichever session should be
// watched: GDI capture is session-local, so the routing flag plays no role.

use anyhow::{bail, Context, Result};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, VkKeyScanW, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL,
    MOUSE_EVENT_FLAGS, VIRTUAL_KEY, VK_LWIN, VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};
use windows::Win32::Foundation::HWND;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn cmd_rfb_server(port: u16) -> Result<i32> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("bind 127.0.0.1:{port}"))?;
    log_line(&format!("listening 127.0.0.1:{port}"));
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                if let Err(e) = serve_client(s) {
                    eprintln!("rfb client ended: {e:#}");
                }
            }
            Err(_) => continue,
        }
    }
    Ok(0)
}

/// 一个客户端一条连接：握手 → ServerInit → 消息循环。
fn serve_client(mut stream: TcpStream) -> Result<()> {
    stream.set_nodelay(true).ok();

    // ---- 版本交换 ----
    stream.write_all(b"RFB 003.008\n")?;
    let mut ver = [0u8; 12];
    stream.read_exact(&mut ver).context("client version")?;
    let ver_str = String::from_utf8_lossy(&ver).to_string();
    let minor: u32 = ver_str
        .strip_prefix("RFB 003.")
        .and_then(|m| m.trim().parse().ok())
        .unwrap_or(8);

    // ---- 安全握手：仅 None(type=1)。3.3 服务器直接发 u32；3.7+ 发列表 ----
    if minor < 7 {
        stream.write_all(&1u32.to_be_bytes())?;
    } else {
        stream.write_all(&[1u8, 1u8])?; // 1 种类型：None
        let mut chosen = [0u8; 1];
        stream.read_exact(&mut chosen).context("security choice")?;
        if minor >= 8 {
            stream.write_all(&0u32.to_be_bytes())?; // SecurityResult: OK
        }
    }

    // ---- ClientInit ----
    let mut shared = [0u8; 1];
    stream.read_exact(&mut shared).context("ClientInit")?;

    // ---- ServerInit：声明 RGBA（redShift=0/greenShift=8/blueShift=16）----
    let shot = crate::capture::shot_screen(true).context("initial capture")?;
    let (w, h) = (shot.width as u16, shot.height as u16);
    let mut init = Vec::with_capacity(8 + 16 + 4 + 8);
    init.extend_from_slice(&w.to_be_bytes());
    init.extend_from_slice(&h.to_be_bytes());
    init.push(32); // bits-per-pixel
    init.push(24); // depth
    init.push(0); // big-endian
    init.push(1); // true-colour
    init.extend_from_slice(&255u16.to_be_bytes()); // red max
    init.extend_from_slice(&255u16.to_be_bytes()); // green max
    init.extend_from_slice(&255u16.to_be_bytes()); // blue max
    init.push(0); // red shift  (RGBA 字节序)
    init.push(8); // green shift
    init.push(16); // blue shift
    init.extend_from_slice(&[0, 0, 0]); // padding
    let name = b"cua-rfb";
    init.extend_from_slice(&(name.len() as u32).to_be_bytes());
    init.extend_from_slice(name);
    stream.write_all(&init).context("ServerInit")?;
    drop(shot);

    // ---- 消息循环 ----
    let mut prev_frame: Option<Vec<u8>> = None;
    let mut prev_mask: u8 = 0;
    let mut buf = [0u8; 1];
    loop {
        stream.read_exact(&mut buf).context("client message")?;
        match buf[0] {
            0 => {
                // SetPixelFormat：19 字节，忽略（维持 ServerInit 声明的格式）
                let mut skip = [0u8; 19];
                stream.read_exact(&mut skip)?;
            }
            2 => {
                // SetEncodings：1 pad + 2 count + 4*n，忽略
                let mut hdr = [0u8; 3];
                stream.read_exact(&mut hdr)?;
                let count = u16::from_be_bytes([hdr[1], hdr[2]]) as u32;
                let mut skip = vec![0u8; 4 * count as usize];
                stream.read_exact(&mut skip)?;
            }
            3 => {
                // FramebufferUpdateRequest：incr(1)+x(2)+y(2)+w(2)+h(2)
                let mut r = [0u8; 9];
                stream.read_exact(&mut r)?;
                send_update(&mut stream, &mut prev_frame)?;
            }
            4 => {
                // KeyEvent：down(1)+pad(3)+keysym(4)
                let mut k = [0u8; 8];
                stream.read_exact(&mut k)?;
                let down = k[0] & 1 == 1;
                let keysym = u32::from_be_bytes([k[4], k[5], k[6], k[7]]);
                inject_key(keysym, down);
            }
            5 => {
                // PointerEvent：mask(1)+x(2)+y(2)
                let mut m = [0u8; 5];
                stream.read_exact(&mut m)?;
                let mask = m[0];
                let x = u16::from_be_bytes([m[1], m[2]]) as i32;
                let y = u16::from_be_bytes([m[3], m[4]]) as i32;
                inject_pointer(mask, x, y, &mut prev_mask);
            }
            6 => {
                // ClientCutText：pad(3)+len(4)+data
                let mut skip = [0u8; 3];
                stream.read_exact(&mut skip)?;
                let mut lb = [0u8; 4];
                stream.read_exact(&mut lb)?;
                let len = u32::from_be_bytes(lb) as usize;
                if len > 1 << 20 {
                    bail!("cut text too large");
                }
                let mut skip = vec![0u8; len];
                stream.read_exact(&mut skip)?;
            }
            _ => {
                // 其余类型（1 FixColourMapEntries 等）按未知长度跳过不安全，
                // 直接断开让客户端重连
                bail!("unsupported client message {}", buf[0]);
            }
        }
    }
}

/// 抓屏 → 变化检测 → FramebufferUpdate（Raw 全帧或空更新）。
fn send_update(stream: &mut TcpStream, prev_frame: &mut Option<Vec<u8>>) -> Result<()> {
    let shot = match crate::capture::shot_screen(true) {
        Ok(s) => s,
        Err(_) => {
            // 抓屏失败：发空更新，保持连接
            stream.write_all(&[0u8, 0u8, 0u8, 0u8])?;
            return Ok(());
        }
    };
    // 抽样统计非零字节占比（黑帧检测）
    let mut nonzero = 0usize;
    { let d = &shot.rgba; let mut i = 0; while i < d.len() { if d[i] != 0 { nonzero += 1; } i += 64; } }
    let total_sampled = shot.rgba.len() / 64 + 1;
    let pct = if total_sampled > 0 { nonzero * 100 / total_sampled } else { 0 };

    let changed = match prev_frame {
        Some(p) => p.len() != shot.rgba.len() || p.as_slice() != shot.rgba.as_slice(),
        None => true,
    };
    if !changed {
        stream.write_all(&[0u8, 0u8, 0u8, 0u8])?; // rectCount=0
        return Ok(());
    }
    let w = shot.width as u16;
    let h = shot.height as u16;
    let mut out = Vec::with_capacity(4 + 12 + 4 + shot.rgba.len());
    out.push(0u8); // FramebufferUpdate
    out.push(0u8); // pad
    out.extend_from_slice(&1u16.to_be_bytes()); // 1 rect
    out.extend_from_slice(&0u16.to_be_bytes()); // x
    out.extend_from_slice(&0u16.to_be_bytes()); // y
    out.extend_from_slice(&w.to_be_bytes());
    out.extend_from_slice(&h.to_be_bytes());
    out.extend_from_slice(&0i32.to_be_bytes()); // Raw encoding
    out.extend_from_slice(&shot.rgba);
    stream.write_all(&out)?;
    *prev_frame = Some(shot.rgba);
    log_line(&format!("frame {}B nonzero={}%", out.len(), pct));
    Ok(())
}

fn virtual_screen() -> (i32, i32, i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}

// ---------------- 输入注入（会话本地 SendInput） ----------------

fn send_mouse(flags: u32, nx: i32, ny: i32, wheel: i32) {
    let mut inp = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: windows::Win32::UI::Input::KeyboardAndMouse::MOUSEINPUT {
                dx: nx,
                dy: ny,
                mouseData: wheel as u32,
                dwFlags: MOUSE_EVENT_FLAGS(flags),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        SendInput(&[inp], std::mem::size_of::<INPUT>() as i32);
    }
}

fn send_key(vk: u16, down: bool) {
    let mut inp = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: if down {
                    KEYBD_EVENT_FLAGS(0)
                } else {
                    KEYBD_EVENT_FLAGS(KEYEVENTF_KEYUP.0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        SendInput(&[inp], std::mem::size_of::<INPUT>() as i32);
    }
}

fn inject_pointer(mask: u8, x: i32, y: i32, prev_mask: &mut u8) {
    let (vx, vy, vw, vh) = virtual_screen();
    let sx = vx + x;
    let sy = vy + y;
    let nx = if vw > 1 { (sx - vx) * 65535 / (vw - 1) } else { 0 };
    let ny = if vh > 1 { (sy - vy) * 65535 / (vh - 1) } else { 0 };

    // 移动
    send_mouse(MOUSEEVENTF_MOVE.0 | MOUSEEVENTF_ABSOLUTE.0 | MOUSEEVENTF_VIRTUALDESK.0, nx, ny, 0);

    // 按键沿：prev -> cur
    let press = |new: bool, old: bool, d: u32, u: u32| {
        if new && !old {
            send_mouse(d, 0, 0, 0);
        } else if !new && old {
            send_mouse(u, 0, 0, 0);
        }
    };
    let left = mask & 1 != 0;
    let mid = mask & 2 != 0;
    let right = mask & 4 != 0;
    let wheel_up = mask & 8 != 0;
    let wheel_down = mask & 16 != 0;
    let old_left = *prev_mask & 1 != 0;
    let old_mid = *prev_mask & 2 != 0;
    let old_right = *prev_mask & 4 != 0;
    press(left, old_left, MOUSEEVENTF_LEFTDOWN.0, MOUSEEVENTF_LEFTUP.0);
    press(mid, old_mid, MOUSEEVENTF_MIDDLEDOWN.0, MOUSEEVENTF_MIDDLEUP.0);
    press(right, old_right, MOUSEEVENTF_RIGHTDOWN.0, MOUSEEVENTF_RIGHTUP.0);
    if wheel_up && *prev_mask & 8 == 0 {
        send_mouse(MOUSEEVENTF_WHEEL.0, 0, 0, 120);
    }
    if wheel_down && *prev_mask & 16 == 0 {
        send_mouse(MOUSEEVENTF_WHEEL.0, 0, 0, -120);
    }
    *prev_mask = mask;
}

/// X keysym → VK，注入 KeyEvent。
fn inject_key(keysym: u32, down: bool) {
    // 修饰键
    let vk_opt: Option<u16> = match keysym {
        0xFFE1 | 0xFFE2 => Some(0x10),          // Shift
        0xFFE3 | 0xFFE5 => Some(0x11),          // Ctrl
        0xFFE7 | 0xFFE9 | 0xFF7F => Some(0x12), // Alt / AltGr
        0xFFEB => Some(VK_LWIN.0 as u16),
        0xFFEC => Some(VK_RWIN.0 as u16),
        0xFF08 => Some(0x08),                   // BackSpace
        0xFF09 => Some(0x09),                   // Tab
        0xFF0D => Some(0x0D),                   // Return
        0xFF1B => Some(0x1B),                   // Escape
        0xFFFF => Some(0x2E),                   // Delete
        0xFF50 => Some(0x24),                   // Home
        0xFF51 => Some(0x25),                   // Left
        0xFF52 => Some(0x26),                   // Up
        0xFF53 => Some(0x27),                   // Right
        0xFF54 => Some(0x28),                   // Down
        0xFF55 => Some(0x21),                   // Prior
        0xFF56 => Some(0x22),                   // Next
        0xFF57 => Some(0x23),                   // End
        0xFF63 => Some(0x2D),                   // Insert
        0xFFBE..=0xFFC9 => Some((keysym - 0xFFBE) as u16 + 0x70), // F1-F12
        0x0020..=0x00FF => {
            // latin-1 可打印：VkKeyScanW 处理标点/大小写
            let ch = keysym as u16;
            let scanned = unsafe { VkKeyScanW(ch as u16) as i16 };
            if scanned == -1 {
                None
            } else {
                let vk = (scanned & 0xFF) as u16;
                let shift = (scanned >> 8) & 1 == 1;
                if down {
                    if shift {
                        send_key(0x10, true);
                    }
                    send_key(vk, true);
                    if shift {
                        send_key(0x10, false);
                    }
                } else {
                    // down 事件只负责按下；释放由独立 KeyEvent 处理
                    // VNC 客户端对按下与释放各发一次 KeyEvent(down=false)，
                    // 这里为兼容 shift 配对，字符键的 up 忽略 shift。
                    send_key(vk, false);
                }
                return;
            }
        }
        _ => None,
    };
    if let Some(vk) = vk_opt {
        send_key(vk, down);
    }
}

/// `cua rfb-server [--port N]` 前缀分发。
pub fn maybe_rfb(raw: &[String]) -> Option<i32> {
    let mut rest: Vec<String> = raw.iter().skip(1).cloned().collect();
    if rest.first().map(|s| s == "rfb-server").unwrap_or(false) {
        rest.remove(0);
    } else {
        return None;
    }
    let mut port: u16 = 5901;
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "--port" {
            if let Some(v) = rest.get(i + 1) {
                port = v.parse().unwrap_or(5901);
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    match cmd_rfb_server(port) {
        Ok(code) => Some(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            Some(1)
        }
    }
}

fn log_line(m: &str) {
    let dir = std::env::var("CUA_DATA_DIR").unwrap_or_else(|_| ".".into());
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::path::Path::new(&dir).join("rfb-server.log"))
        .and_then(|mut f| {
            std::io::Write::write_all(
                &mut f,
                format!("{} {}
", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0), m).as_bytes(),
            )
        });
}
