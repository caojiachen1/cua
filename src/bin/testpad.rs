// testpad.rs — classic Win32 fixture app for cua e2e tests.
// Plain user32 windows so it runs on hidden desktops and exposes UIA via the MSAA proxy.
// Controls: multiline EDIT, buttons, checkbox (Toggle), listbox (SelectionItem),
// trackbar (RangeValue), and a custom hit area that records right/double/message clicks.
#![windows_subsystem = "windows"]
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
    PostQuitMessage, RegisterClassW, SendMessageW, ShowWindow, TranslateMessage, CS_DBLCLKS,
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, HMENU, MSG, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CHAR, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_GETTEXT, WM_GETTEXTLENGTH, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETTEXT, WNDCLASSW,
    WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE, WS_VSCROLL,
};

const IDC_EDIT: isize = 1001;
const IDC_BTN: isize = 1002;
const IDC_CHECK: isize = 1003;
const IDC_LIST: isize = 1004;
const IDC_TRACK: isize = 1005;
const IDC_HIT: isize = 1006;

const BS_AUTOCHECKBOX: u32 = 0x0003;
const LBS_NOTIFY: u32 = 0x0001;
const TBS_AUTOTICKS: u32 = 0x0001;
const LB_ADDSTRING: u32 = 0x0180;
const TBM_SETRANGE: u32 = 0x0406;
const TBM_SETPOS: u32 = 0x0405;
const TBM_SETTICFREQ: u32 = 0x0414;

unsafe extern "system" fn hit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN => {
            append_line("MDOWN");
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            append_line("CLICK");
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            append_line("DBLCLK");
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            append_line("RDOWN");
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            append_line("RCLICK");
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            let hinstance = GetModuleHandleW(PCWSTR::null()).unwrap_or_default();
            // 0x4 == ES_MULTILINE
            let edit_style = WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_VSCROLL.0 | 0x4);
            let _edit = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                w!(""),
                edit_style,
                8, 8, 600, 230,
                hwnd,
                HMENU(IDC_EDIT as *mut _),
                windows::Win32::Foundation::HINSTANCE(hinstance.0),
                None,
            );
            let _btn = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("BUTTON"),
                w!("AppendMark"),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
                8, 246, 140, 30,
                hwnd,
                HMENU(IDC_BTN as *mut _),
                windows::Win32::Foundation::HINSTANCE(hinstance.0),
                None,
            );
            let _check = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("BUTTON"),
                w!("AutoMark"),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | BS_AUTOCHECKBOX),
                156, 246, 110, 30,
                hwnd,
                HMENU(IDC_CHECK as *mut _),
                windows::Win32::Foundation::HINSTANCE(hinstance.0),
                None,
            );
            let _hit = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("CuaHitArea"),
                w!("HitArea"),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
                274, 246, 160, 30,
                hwnd,
                HMENU(IDC_HIT as *mut _),
                windows::Win32::Foundation::HINSTANCE(hinstance.0),
                None,
            );
            let list = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("LISTBOX"),
                w!(""),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_VSCROLL.0 | LBS_NOTIFY),
                8, 284, 200, 110,
                hwnd,
                HMENU(IDC_LIST as *mut _),
                windows::Win32::Foundation::HINSTANCE(hinstance.0),
                None,
            );
            if let Ok(list) = list {
                for item in ["alpha", "beta", "gamma"] {
                    let mut w: Vec<u16> = item.encode_utf16().chain(std::iter::once(0)).collect();
                    SendMessageW(list, LB_ADDSTRING, WPARAM(0), LPARAM(w.as_mut_ptr() as isize));
                }
            }
            let track = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("msctls_trackbar32"),
                w!("Volume"),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | TBS_AUTOTICKS),
                220, 284, 220, 45,
                hwnd,
                HMENU(IDC_TRACK as *mut _),
                windows::Win32::Foundation::HINSTANCE(hinstance.0),
                None,
            );
            if let Ok(track) = track {
                SendMessageW(track, TBM_SETRANGE, WPARAM(1), LPARAM(((0i32) | (100i32 << 16)) as isize));
                SendMessageW(track, TBM_SETPOS, WPARAM(1), LPARAM(25));
                SendMessageW(track, TBM_SETTICFREQ, WPARAM(10), LPARAM(0));
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            if (wp.0 as isize & 0xFFFF) == IDC_BTN {
                append_mark();
            }
            LRESULT(0)
        }
        WM_CHAR => {
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let _ = EndPaint(hwnd, &ps);
            let _ = hdc;
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

static mut EDIT_HWND: isize = 0;

unsafe fn edit_hwnd() -> HWND {
    HWND(EDIT_HWND as *mut _)
}

unsafe fn edit_text() -> String {
    let h = edit_hwnd();
    let len = SendMessageW(h, WM_GETTEXTLENGTH, WPARAM(0), LPARAM(0)).0 as usize;
    let mut buf = vec![0u16; len + 1];
    SendMessageW(h, WM_GETTEXT, WPARAM(buf.len()), LPARAM(buf.as_mut_ptr() as isize));
    String::from_utf16_lossy(&buf[..len])
}

unsafe fn set_edit_text(text: &str) {
    let mut w: Vec<u16> = text.encode_utf16().collect();
    w.push(0);
    SendMessageW(edit_hwnd(), WM_SETTEXT, WPARAM(0), LPARAM(w.as_mut_ptr() as isize));
}

unsafe fn append_line(line: &str) {
    let s = edit_text();
    set_edit_text(&format!("{}\n{}", s.trim_end_matches('\0'), line));
}

unsafe fn append_mark() {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    append_line(&format!("MARK:{secs}"));
}

fn title_from_args() -> String {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--title" {
            if let Some(t) = args.next() {
                return t;
            }
        }
    }
    "CuaTestPad Fixture".to_string()
}

fn main() -> anyhow::Result<()> {
    unsafe {
        let hinstance = GetModuleHandleW(PCWSTR::null()).unwrap_or_default();
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: windows::Win32::Foundation::HINSTANCE(hinstance.0),
            lpszClassName: w!("CuaTestPad"),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let hit = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(hit_proc),
            hInstance: windows::Win32::Foundation::HINSTANCE(hinstance.0),
            lpszClassName: w!("CuaHitArea"),
            ..Default::default()
        };
        RegisterClassW(&hit);
        let title = title_from_args();
        let mut title_w: Vec<u16> = title.encode_utf16().collect();
        title_w.push(0);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("CuaTestPad"),
            PCWSTR(title_w.as_ptr()),
            WS_OVERLAPPEDWINDOW | WINDOW_STYLE(WS_VISIBLE.0),
            CW_USEDEFAULT, CW_USEDEFAULT, 660, 520,
            None,
            None,
            windows::Win32::Foundation::HINSTANCE(hinstance.0),
            None,
        )?;
        // remember the edit control for the button handlers
        let edit = windows::Win32::UI::WindowsAndMessaging::GetDlgItem(hwnd, IDC_EDIT as i32);
        EDIT_HWND = edit.unwrap_or_default().0 as isize;
        let _ = ShowWindow(hwnd, SW_SHOW);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
