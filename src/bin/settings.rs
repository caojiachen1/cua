// cua-settings — the GPUI settings app for the cua CLI.
//
// Every optional cua setting lives in one flat JSON file
// (%LOCALAPPDATA%\cua\config.json) that the CLI reads lazily on each run:
// pointer_preview, session_start_show_viewer, the SafetyConfig fields
// (full_mode, require_shot_id_for_coords, deny_processes, deny_key_modifiers,
// levels) and a few browser keys. This window edits that file directly so the
// user can choose their own defaults instead of being asked mid-flight by the
// agent. Unknown keys are preserved on save (the merge mirrors
// util::set_full_mode), and a malformed config is never overwritten.

// GUI subsystem: launching the settings window must not flash a console.
#![windows_subsystem = "windows"]

use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, FontWeight, SharedString,
    Window, WindowBounds, WindowOptions,
};
use serde_json::{json, Value};

const KNOWN_DENY_PROCESSES: &[&str] = &[
    "cmd",
    "powershell",
    "pwsh",
    "windowsterminal",
    "conhost",
    "wt",
    "taskmgr",
    "regedit",
    "mmc",
    "control",
    "1password",
    "keepassxc",
    "keepass",
    "bitwarden",
    "dashlane",
    "lastpass",
    "ms-settings",
    "systemsettings",
];

const KNOWN_DENY_KEY_MODIFIERS: &[&str] = &["win", "meta", "logo"];

/// The deny lists util::SafetyConfig::default ships with — an absent
/// config.json must preview these, or one save would wipe the deny list.
const DEFAULT_DENY_PROCESSES: &[&str] = KNOWN_DENY_PROCESSES;
const DEFAULT_DENY_KEY_MODIFIERS: &[&str] = KNOWN_DENY_KEY_MODIFIERS;

fn data_dir() -> std::path::PathBuf {
    if let Ok(d) = std::env::var("CUA_DATA_DIR") {
        if !d.trim().is_empty() {
            return std::path::PathBuf::from(d);
        }
    }
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    std::path::Path::new(&base).join("cua")
}

fn config_path() -> std::path::PathBuf {
    data_dir().join("config.json")
}

fn load_config() -> Value {
    std::fs::read(config_path())
        .ok()
        .and_then(|b| {
            let b = b.strip_prefix(b"\xEF\xBB\xBF".as_slice()).unwrap_or(&b).to_vec();
            serde_json::from_slice(&b).ok()
        })
        .unwrap_or_else(|| json!({}))
}

fn bool_of(v: &Value, key: &str, default: bool) -> bool {
    v.get(key).and_then(|x| x.as_bool()).unwrap_or(default)
}

fn strings_of(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

#[derive(Clone)]
struct Settings {
    pointer_preview: bool,
    session_start_show_viewer: bool,
    /// Run child-session automation without a visible viewer window by default
    /// (the "后台电脑控制" switch — maps to the same config key the CLI reads).
    session_headless: bool,
    full_mode: bool,
    require_shot_id_for_coords: bool,
    /// Recorder JPEG quality 1-100 (cua rec --quality default).
    rec_quality: u8,
    /// Recorder frames per second.
    rec_fps: u32,
    deny_processes: Vec<String>,
    deny_key_modifiers: Vec<String>,
}

impl Settings {
    fn load() -> Self {
        let v = load_config();
        let exists = config_path().exists();
        // an absent config.json: the CLI would fall back to the shipped
        // deny lists (util::SafetyConfig::default) — mirror them here so the
        // first save does not wipe them
        let deny_processes = {
            let list = strings_of(&v, "deny_processes");
            if list.is_empty() && !exists {
                DEFAULT_DENY_PROCESSES.iter().map(|s| s.to_string()).collect()
            } else {
                list
            }
        };
        let deny_key_modifiers = {
            let list = strings_of(&v, "deny_key_modifiers");
            if list.is_empty() && !exists {
                DEFAULT_DENY_KEY_MODIFIERS.iter().map(|s| s.to_string()).collect()
            } else {
                list
            }
        };
        Settings {
            pointer_preview: bool_of(&v, "pointer_preview", true),
            session_start_show_viewer: bool_of(&v, "session_start_show_viewer", false),
            session_headless: bool_of(&v, "session_headless", false),
            full_mode: bool_of(&v, "full_mode", false),
            require_shot_id_for_coords: bool_of(&v, "require_shot_id_for_coords", true),
            rec_quality: v
                .get("rec_quality")
                .and_then(|x| x.as_u64())
                .map(|q| q.clamp(1, 100) as u8)
                .unwrap_or(80),
            rec_fps: v
                .get("rec_fps")
                .and_then(|x| x.as_u64())
                .map(|f| f.clamp(1, 30) as u32)
                .unwrap_or(2),
            deny_processes,
            deny_key_modifiers,
        }
    }
}

/// Merge the edited keys into the existing config object (unknown keys —
/// browser_port, levels, whatever the CLI adds next — survive untouched).
/// A malformed or non-object config is never overwritten.
fn save_settings(s: &Settings) -> Result<(), String> {
    // A malformed (non-object) config must never be overwritten: read the RAW
    // file — load_config() would silently hand back an empty object for it.
    let existing = std::fs::read(config_path())
        .ok()
        .and_then(|b| {
            let b = b.strip_prefix(b"\xEF\xBB\xBF".as_slice()).unwrap_or(&b).to_vec();
            serde_json::from_slice::<Value>(&b).ok()
        })
        .filter(|v| v.is_object());
    if config_path().exists() && existing.is_none() {
        return Err("config.json 不是合法的 JSON 对象，拒绝覆盖".into());
    }
    let mut v = existing.unwrap_or_else(|| json!({}));
    let obj = v.as_object_mut().unwrap();
    obj.insert("pointer_preview".into(), json!(s.pointer_preview));
    obj.insert("session_start_show_viewer".into(), json!(s.session_start_show_viewer));
    obj.insert("session_headless".into(), json!(s.session_headless));
    obj.insert("full_mode".into(), json!(s.full_mode));
    obj.insert("require_shot_id_for_coords".into(), json!(s.require_shot_id_for_coords));
    obj.insert("rec_quality".into(), json!(s.rec_quality));
    obj.insert("rec_fps".into(), json!(s.rec_fps));
    obj.insert("deny_processes".into(), json!(s.deny_processes));
    obj.insert("deny_key_modifiers".into(), json!(s.deny_key_modifiers));
    let pretty = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    std::fs::write(config_path(), pretty).map_err(|e| e.to_string())
}

struct SettingsView {
    cfg: Settings,
    status: Option<&'static str>,
    error: Option<String>,
    /// Read-only environment probes, refreshed on open and on demand. They
    /// never write anything — they only answer "is this feature usable here?".
    health: Vec<Probe>,
}

/// One line in the 状态检测 section: what was checked and how it came out.
#[derive(Clone)]
struct Probe {
    label: &'static str,
    state: ProbeState,
    detail: String,
}

#[derive(Clone, Copy, PartialEq)]
enum ProbeState {
    Ok,
    Warn,
    Fail,
    Unknown,
}

impl ProbeState {
    fn glyph(self) -> &'static str {
        match self {
            ProbeState::Ok => "✓",
            ProbeState::Warn => "!",
            ProbeState::Fail => "✕",
            ProbeState::Unknown => "?",
        }
    }
    fn color(self) -> u32 {
        match self {
            ProbeState::Ok => 0xffffff,
            ProbeState::Warn => 0xc9c9c9,
            ProbeState::Fail => 0x8a8a8a,
            ProbeState::Unknown => 0x5a5a5a,
        }
    }
}

/// Where cua.exe / cua-session-host.exe live — the same lookup the CLI does
/// (CUA_SESSION_HOST override, else next to the running executable).
fn exe_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

fn session_host_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("CUA_SESSION_HOST") {
        let p = std::path::PathBuf::from(p);
        if p.is_file() {
            return p;
        }
    }
    exe_dir().join("cua-session-host.exe")
}

/// Child Sessions (WTS) — the hard prerequisite for `cua session start`.
/// Reads the same WTSIsChildSessionsEnabled the CLI gates on; a missing or
/// failing call is reported as unknown, never as a false "off".
fn probe_child_sessions() -> (ProbeState, String) {
    #[link(name = "wtsapi32")]
    extern "system" {
        fn WTSIsChildSessionsEnabled(enabled: *mut i32) -> i32;
    }
    let mut enabled = 0i32;
    let ok = unsafe { WTSIsChildSessionsEnabled(&mut enabled) };
    if ok == 0 {
        return (ProbeState::Unknown, "无法查询 WTS 子会话状态".into());
    }
    if enabled != 0 {
        (ProbeState::Ok, "已启用 · 可直接 cua session start".into())
    } else {
        (
            ProbeState::Fail,
            "未启用 · 运行 cua session setup（需管理员）后注销重登".into(),
        )
    }
}

fn probe_session_host() -> (ProbeState, String) {
    let p = session_host_path();
    if p.is_file() {
        (ProbeState::Ok, p.display().to_string())
    } else {
        (
            ProbeState::Fail,
            format!("缺少 {} · 重跑 install.ps1", p.display()),
        )
    }
}

/// Is a child session already running? Reads the state file the CLI writes and
/// checks the recorded host pid is still alive.
fn probe_session_running() -> (ProbeState, String) {
    let path = data_dir().join("session.json");
    let Ok(bytes) = std::fs::read(&path) else {
        return (ProbeState::Warn, "无运行中的子会话".into());
    };
    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let pid = v.get("host_pid").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let sid = v.get("child_session_id").and_then(|x| x.as_u64()).unwrap_or(0);
    if pid == 0 {
        return (ProbeState::Warn, "无运行中的子会话".into());
    }
    if pid_alive(pid) {
        (ProbeState::Ok, format!("运行中 · 宿主 pid {pid} · 会话 {sid}"))
    } else {
        (ProbeState::Warn, "状态文件残留，宿主已退出".into())
    }
}

/// A pid is alive iff OpenProcess + GetExitCodeProcess reports STILL_ACTIVE.
/// This bin cannot reach the CLI's crate::win helpers, so the probe is local.
fn pid_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code).is_ok();
        let _ = CloseHandle(h);
        ok && code == STILL_ACTIVE.0 as u32
    }
}

/// Browser bridge daemon + extension. The daemon publishes /health on the
/// configured port; we do a raw TCP GET with a short deadline so the settings
/// window never hangs when nothing is listening.
fn probe_browser_bridge() -> (ProbeState, String) {
    let cfg = load_config();
    let port = cfg
        .get("browser_port")
        .and_then(|x| x.as_u64())
        .map(|p| p as u16)
        .unwrap_or(8765);
    let body = http_get_health(port);
    match body {
        Some(v) => {
            let ext = v.get("extension").and_then(|x| x.as_bool()).unwrap_or(false);
            let build = v
                .get("extension_build")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            if ext {
                (
                    ProbeState::Ok,
                    if build.is_empty() {
                        format!("端口 {port} · 扩展已连接")
                    } else {
                        format!("端口 {port} · 扩展已连接（{build}）")
                    },
                )
            } else {
                (
                    ProbeState::Warn,
                    format!("端口 {port} · 守护进程在跑，扩展未连接"),
                )
            }
        }
        None => (
            ProbeState::Warn,
            format!("端口 {port} 无响应 · cua browser status 可拉起"),
        ),
    }
}

fn http_get_health(port: u16) -> Option<Value> {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(300)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_millis(600)));
    let _ = s.set_write_timeout(Some(Duration::from_millis(600)));
    let req = format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > 256 * 1024 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let body = text.split("\r\n\r\n").nth(1)?;
    serde_json::from_str(body.trim()).ok()
}

/// Pointer preview host: the overlay process is `cua preview serve`. A live
/// preview-<session>.json with a live pid means the bubble will show up.
fn probe_preview_host() -> (ProbeState, String) {
    let dir = data_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return (ProbeState::Warn, "未启动（首次动作时自动拉起）".into());
    };
    let mut alive = 0usize;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("preview-") || !name.ends_with(".json") {
            continue;
        }
        let Ok(bytes) = std::fs::read(e.path()) else { continue };
        let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        let pid = v.get("pid").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        if pid != 0 && pid_alive(pid) {
            alive += 1;
        }
    }
    if alive > 0 {
        (ProbeState::Ok, format!("{alive} 个预览宿主在线"))
    } else {
        (ProbeState::Warn, "未启动（首次动作时自动拉起）".into())
    }
}

fn probe_rec_encoder() -> (ProbeState, String) {
    // Media Foundation H.264 is compiled in; only the ffmpeg fallback needs a
    // binary. Report the compiled path as usable, and whether ffmpeg is around.
    let cfg = load_config();
    let ff = cfg
        .get("ffmpeg_path")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty());
    match ff {
        Some(p) if std::path::Path::new(&p).is_file() => {
            (ProbeState::Ok, format!("内置 H.264 + ffmpeg 回退（{p}）"))
        }
        Some(p) => (ProbeState::Warn, format!("内置 H.264 可用；ffmpeg_path 无效（{p}）")),
        None => (ProbeState::Ok, "内置 H.264（Media Foundation）".into()),
    }
}

/// Config file sanity: parseable, and not silently falling back to defaults.
fn probe_config() -> (ProbeState, String) {
    let p = config_path();
    let Ok(bytes) = std::fs::read(&p) else {
        return (ProbeState::Warn, "不存在 · 首次保存时创建".into());
    };
    let b = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    match serde_json::from_slice::<Value>(b) {
        Ok(v) if v.is_object() => (ProbeState::Ok, p.display().to_string()),
        Ok(_) => (
            ProbeState::Fail,
            "不是 JSON 对象 · CLI 会忽略它，且保存被拒".into(),
        ),
        Err(e) => (ProbeState::Fail, format!("JSON 解析失败：{e}")),
    }
}

fn run_probes() -> Vec<Probe> {
    let mut out = Vec::new();
    let (s, d) = probe_config();
    out.push(Probe { label: "配置文件", state: s, detail: d });
    let (s, d) = probe_child_sessions();
    out.push(Probe { label: "子会话（WTS）", state: s, detail: d });
    let (s, d) = probe_session_host();
    out.push(Probe { label: "子会话宿主", state: s, detail: d });
    let (s, d) = probe_session_running();
    out.push(Probe { label: "子会话运行中", state: s, detail: d });
    let (s, d) = probe_browser_bridge();
    out.push(Probe { label: "浏览器桥接", state: s, detail: d });
    let (s, d) = probe_preview_host();
    out.push(Probe { label: "指针预览宿主", state: s, detail: d });
    let (s, d) = probe_rec_encoder();
    out.push(Probe { label: "录制编码器", state: s, detail: d });
    out
}

#[derive(Clone, Copy)]
enum Toggle {
    Preview,
    ShowViewer,
    Headless,
    FullMode,
    RequireShot,
}

#[derive(Clone, Copy)]
enum Chip {
    DenyProcess(usize),
    DenyModifier(usize),
}

impl SettingsView {
    fn new() -> Self {
        SettingsView {
            cfg: Settings::load(),
            status: None,
            error: None,
            health: run_probes(),
        }
    }

    fn toggle(&mut self, t: Toggle, cx: &mut Context<Self>) {
        match t {
            Toggle::Preview => self.cfg.pointer_preview = !self.cfg.pointer_preview,
            Toggle::ShowViewer => self.cfg.session_start_show_viewer = !self.cfg.session_start_show_viewer,
            Toggle::Headless => self.cfg.session_headless = !self.cfg.session_headless,
            Toggle::FullMode => self.cfg.full_mode = !self.cfg.full_mode,
            Toggle::RequireShot => self.cfg.require_shot_id_for_coords = !self.cfg.require_shot_id_for_coords,
        }
        self.status = None;
        cx.notify();
    }

    /// Re-run every environment probe. Cheap (a 300ms TCP connect worst case)
    /// but done on the UI thread so the result is consistent with one click.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.health = run_probes();
        cx.notify();
    }

    fn toggle_chip(&mut self, c: Chip, cx: &mut Context<Self>) {
        let (list, item) = match c {
            Chip::DenyProcess(i) => (&mut self.cfg.deny_processes, KNOWN_DENY_PROCESSES[i]),
            Chip::DenyModifier(i) => (&mut self.cfg.deny_key_modifiers, KNOWN_DENY_KEY_MODIFIERS[i]),
        };
        let item = item.to_string();
        match list.iter().position(|x| *x == item) {
            Some(pos) => {
                list.remove(pos);
            }
            None => list.push(item),
        }
        self.status = None;
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        match save_settings(&self.cfg) {
            Ok(()) => {
                self.error = None;
                self.status = Some("已保存 — 下一次 cua 命令即生效");
            }
            Err(e) => {
                self.error = Some(e);
                self.status = None;
            }
        }
        cx.notify();
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // snapshot the chip lists so the render borrows stay simple
        let deny_processes = self.cfg.deny_processes.clone();
        let deny_mods = self.cfg.deny_key_modifiers.clone();

        let mut row = |label: &'static str, hint: &'static str, on: bool, t: Toggle, cx: &mut Context<Self>| {
            div()
                .id(SharedString::from(format!("row-{}", label)))
                .flex()
                .items_center()
                .justify_between()
                .w_full()
                .py(px(10.))
                .on_click(cx.listener(move |this, _ev, _window, cx| this.toggle(t, cx)))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x2b2b2b)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(rgb(0xffffff))
                                .font_weight(FontWeight::MEDIUM)
                                .child(label.to_string()),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(0x8a8a8a))
                                .child(hint.to_string()),
                        ),
                )
                .child(toggle_switch(on))
        };

        let mut chips = |title: &'static str,
                         items: &'static [&'static str],
                         active: &[String],
                         chip: fn(usize) -> Chip,
                         cx: &mut Context<Self>| {
            let mut wrap = div().flex().flex_wrap().gap(px(6.)).w_full();
            for (i, item) in items.iter().enumerate() {
                let on = active.iter().any(|x| x == item);
                wrap = wrap.child(
                    div()
                        .id(SharedString::from(format!("chip-{}-{}", title, item)))
                        .flex()
                        .items_center()
                        .px(px(9.))
                        .py(px(5.))
                        .rounded(px(999.))
                        .text_size(px(11.5))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _ev, _window, cx| this.toggle_chip(chip(i), cx)))
                        .when(on, |s| {
                            s.bg(rgb(0xffffff))
                                .text_color(rgb(0x000000))
                                .font_weight(FontWeight::MEDIUM)
                        })
                        .when(!on, |s| {
                            s.bg(rgb(0x2b2b2b))
                                .text_color(rgb(0x8a8a8a))
                                .border_1()
                                .border_color(rgb(0x3d3d3d))
                        })
                        .child((*item).to_string()),
                );
            }
            section(
                title,
                div().flex().flex_col().gap(px(6.)).w_full().child(wrap),
                cx,
            )
        };

        let mut full_rows = div().flex().flex_col().gap(px(2.)).w_full();
        full_rows = full_rows.child(row(
            "指针预览",
            "动作落点的全局气泡提示（桌面与子会话）",
            self.cfg.pointer_preview,
            Toggle::Preview,
            cx,
        ));
        full_rows = full_rows.child(row(
            "后台电脑控制",
            "子会话默认无感运行，不弹观察窗；cua session view 随时调出",
            self.cfg.session_headless,
            Toggle::Headless,
            cx,
        ));
        full_rows = full_rows.child(row(
            "启动时显示观察窗",
            "默认无感后台：观察窗隐藏，cua session view 随时调出",
            self.cfg.session_start_show_viewer,
            Toggle::ShowViewer,
            cx,
        ));
        full_rows = full_rows.child(row(
            "坐标操作需新鲜截图",
            "拒绝没有刚观察过的 shot_id 的坐标点击/拖拽",
            self.cfg.require_shot_id_for_coords,
            Toggle::RequireShot,
            cx,
        ));
        full_rows = full_rows.child(row(
            "完全模式（风险）",
            "关闭拒绝进程强制；坐标、Always-Confirm 与 Win 键防护仍然生效",
            self.cfg.full_mode,
            Toggle::FullMode,
            cx,
        ));

        // 固定头部（标题 + 配置路径）
        let header = div()
            .flex()
            .items_baseline()
            .justify_between()
            .w_full()
            .child(
                div()
                    .text_size(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("CUA 设置"),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(0x737373))
                    .child(config_path().display().to_string()),
            );

        // 可滚动主体：设置项 + 状态检测
        let mut body = div()
            .id("body")
            .flex()
            .flex_col()
            .w_full()
            .flex_1()
            .gap(px(14.))
            .overflow_y_scroll();

        body = body.child(section("常规", full_rows, cx));
        body = body.child(chips(
            "拒绝进程（这些进程上的输入会被拒绝）",
            KNOWN_DENY_PROCESSES,
            &deny_processes,
            Chip::DenyProcess as fn(usize) -> Chip,
            cx,
        ));
        body = body.child(chips(
            "拒绝修饰键（含这些修饰键的组合键会被拒绝）",
            KNOWN_DENY_KEY_MODIFIERS,
            &deny_mods,
            Chip::DenyModifier as fn(usize) -> Chip,
            cx,
        ));

        // 状态检测：只读探测每项功能在这台机器上到底能不能用
        let mut probe_list = div().flex().flex_col().gap(px(5.)).w_full();
        for p in &self.health {
            let c = p.state.color();
            probe_list = probe_list.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .w_full()
                    .child(
                        div()
                            .flex_none()
                            .w(px(14.))
                            .text_size(px(11.5))
                            .text_color(rgb(c))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(p.state.glyph()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(0xffffff))
                                    .child(p.label),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(rgb(0x8a8a8a))
                                    .child(p.detail.clone()),
                            ),
                    ),
            );
        }
        let probe_body = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .w_full()
            .child(probe_list)
            .child(
                div()
                    .id("refresh-probes")
                    .flex()
                    .items_center()
                    .justify_center()
                    .w_full()
                    .py(px(6.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgb(0x3d3d3d))
                    .text_size(px(11.5))
                    .text_color(rgb(0xc9c9c9))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x2b2b2b)))
                    .on_click(cx.listener(|this, _ev, _window, cx| this.refresh(cx)))
                    .child("重新检测"),
            );
        body = body.child(section("状态检测（只读）", probe_body, cx));

        // 固定底栏：保存状态 + 按钮
        let mut bar = div().flex().items_center().justify_between().w_full();
        bar = bar.child(
            div().flex().flex_col().gap(px(2.)).child(match (&self.status, &self.error) {
                (Some(s), _) => div()
                    .text_size(px(11.5))
                    .text_color(rgb(0xffffff))
                    .child(s.to_string()),
                (_, Some(e)) => div()
                    .text_size(px(11.5))
                    .text_color(rgb(0x8a8a8a))
                    .child(format!("保存失败：{e}")),
                _ => div().text_size(px(11.5)).text_color(rgb(0x525252)).child("修改后点击保存"),
            }),
        );
        bar = bar.child(
            div()
                .id("save")
                .flex()
                .items_center()
                .px(px(18.))
                .py(px(8.))
                .rounded(px(10.))
                .bg(rgb(0xffffff))
                .text_color(rgb(0x000000))
                .text_size(px(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0xd4d4d4)))
                .on_click(cx.listener(|this, _ev, _window, cx| this.save(cx)))
                .child("保存"),
        );
        // 三段式根布局：头部固定、主体滚动、底栏固定
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x202020))
            .text_color(rgb(0xffffff))
            .p(px(22.))
            .gap(px(14.))
            .overflow_hidden()
            .child(header)
            .child(body)
            .child(bar)
    }
}

fn section(title: &'static str, body: impl IntoElement, cx: &mut Context<SettingsView>) -> impl IntoElement {
    let _ = cx;
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .w_full()
        .border_1()
        .border_color(rgb(0x2e2e2e))
        .rounded(px(12.))
        .p(px(16.))
        .bg(rgb(0x282828))
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(0x737373))
                .font_weight(FontWeight::MEDIUM)
                .child(title.to_string()),
        )
        .child(body)
}

/// A compact pill-style toggle: 34x18 track, 14px knob, white = on.
fn toggle_switch(on: bool) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_end()
        .w(px(34.))
        .h(px(18.))
        .rounded(px(999.))
        .p(px(2.))
        .when(on, |s| s.bg(rgb(0xffffff)).justify_end())
        .when(!on, |s| s.bg(rgb(0x3d3d3d)).justify_start())
        .child(
            div()
                .w(px(14.))
                .h(px(14.))
                .rounded(px(999.))
                .when(on, |s| s.bg(rgb(0x000000)))
                .when(!on, |s| s.bg(rgb(0x8a8a8a))),
        )
}

/// Effective scale of the primary monitor (1.0 at 96 DPI, 1.5 at 150%). The
/// process is DPI-unaware before gpui sets awareness, so GetDpiForSystem can
/// report 96 on a scaled display — GetDpiForMonitor with the monitor's actual
/// DPI is what we want.
fn screen_scale() -> f32 {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows::Win32::UI::HiDpi::{
        GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    };
    unsafe {
        let pt = POINT { x: 0, y: 0 };
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
        let mut dpi_x = 96u32;
        let mut dpi_y = 96u32;
        if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() && dpi_x > 0 {
            return dpi_x as f32 / 96.0;
        }
        // fall back to the monitor's own rectangle vs. the virtual screen
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(mon, &mut info).as_bool() {
            let w = (info.rcMonitor.right - info.rcMonitor.left) as f32;
            if w > 0.0 {
                return (w / 1280.0).max(1.0);
            }
        }
        1.0
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        // Display::bounds() is already logical (divided by the monitor scale).
        // But gpui's Windows backend multiplies the requested window size by
        // scale_factor again, so to land at `target` logical px we must request
        // target / scale. The scale is read from the same display the window
        // opens on.
        let display = cx.primary_display();
        let (sw, sh) = display
            .as_ref()
            .map(|d| {
                let b = d.bounds();
                (f32::from(b.size.width), f32::from(b.size.height))
            })
            .unwrap_or((1280.0, 800.0));
        let scale = screen_scale().max(1.0);
        let w = (700f32.min(sw - 40.0).max(480.0)) / scale;
        let h = (860f32.min(sh - 130.0).max(400.0)) / scale;
        let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(SharedString::from("CUA 设置")),
                ..Default::default()
            }),
            ..Default::default()
        };
        cx.open_window(options, |_, cx| cx.new(|_| SettingsView::new())).unwrap();
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolated_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cua-settings-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // ONE sequential test: CUA_DATA_DIR is process-global and the tests share it.
    #[test]
    fn settings_save_load_and_guards() {
        let dir = isolated_dir();
        std::env::set_var("CUA_DATA_DIR", &dir);

        // 1. absent config previews the shipped deny lists
        let s = Settings::load();
        assert_eq!(
            s.deny_processes,
            DEFAULT_DENY_PROCESSES.iter().map(|x| x.to_string()).collect::<Vec<_>>()
        );

        // 2. save round-trips and preserves unknown keys
        std::fs::write(
            config_path(),
            r#"{"browser_port": 8765, "levels": {"close": "confirm"}, "deny_processes": ["cmd"]}"#,
        )
        .unwrap();
        let s = Settings {
            pointer_preview: false,
            session_start_show_viewer: true,
            session_headless: true,
            full_mode: true,
            require_shot_id_for_coords: false,
            rec_quality: 65,
            rec_fps: 5,
            deny_processes: vec!["cmd".into(), "extra-custom".into()],
            deny_key_modifiers: vec!["win".into()],
        };
        save_settings(&s).unwrap();
        let raw = load_config();
        assert_eq!(raw.get("browser_port").and_then(|x| x.as_u64()), Some(8765));
        assert_eq!(
            raw.get("levels").and_then(|x| x.get("close")).and_then(|x| x.as_str()),
            Some("confirm")
        );
        assert_eq!(raw.get("pointer_preview").and_then(|x| x.as_bool()), Some(false));
        assert_eq!(raw.get("session_headless").and_then(|x| x.as_bool()), Some(true));
        assert_eq!(raw.get("rec_quality").and_then(|x| x.as_u64()), Some(65));
        assert_eq!(raw.get("rec_fps").and_then(|x| x.as_u64()), Some(5));
        let reloaded = Settings::load();
        assert!(!reloaded.pointer_preview);
        assert!(reloaded.session_start_show_viewer);
        assert!(reloaded.session_headless);
        assert!(!reloaded.require_shot_id_for_coords);
        assert_eq!(reloaded.rec_quality, 65);
        assert_eq!(reloaded.rec_fps, 5);
        assert_eq!(reloaded.deny_processes, vec!["cmd".to_string(), "extra-custom".to_string()]);

        // 3. a malformed config is never overwritten
        std::fs::write(config_path(), b"not json at all {").unwrap();
        let err = save_settings(&s).unwrap_err();
        assert!(err.contains("拒绝覆盖"));
        assert_eq!(std::fs::read(config_path()).unwrap(), b"not json at all {");

        std::env::remove_var("CUA_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
