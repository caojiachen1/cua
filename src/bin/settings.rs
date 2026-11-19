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
    full_mode: bool,
    require_shot_id_for_coords: bool,
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
            full_mode: bool_of(&v, "full_mode", false),
            require_shot_id_for_coords: bool_of(&v, "require_shot_id_for_coords", true),
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
    obj.insert("full_mode".into(), json!(s.full_mode));
    obj.insert("require_shot_id_for_coords".into(), json!(s.require_shot_id_for_coords));
    obj.insert("deny_processes".into(), json!(s.deny_processes));
    obj.insert("deny_key_modifiers".into(), json!(s.deny_key_modifiers));
    let pretty = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    std::fs::write(config_path(), pretty).map_err(|e| e.to_string())
}

struct SettingsView {
    cfg: Settings,
    status: Option<&'static str>,
    error: Option<String>,
}

#[derive(Clone, Copy)]
enum Toggle {
    Preview,
    ShowViewer,
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
        SettingsView { cfg: Settings::load(), status: None, error: None }
    }

    fn toggle(&mut self, t: Toggle, cx: &mut Context<Self>) {
        match t {
            Toggle::Preview => self.cfg.pointer_preview = !self.cfg.pointer_preview,
            Toggle::ShowViewer => self.cfg.session_start_show_viewer = !self.cfg.session_start_show_viewer,
            Toggle::FullMode => self.cfg.full_mode = !self.cfg.full_mode,
            Toggle::RequireShot => self.cfg.require_shot_id_for_coords = !self.cfg.require_shot_id_for_coords,
        }
        self.status = None;
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

        let mut root = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x202020))
            .text_color(rgb(0xffffff))
            .p(px(22.))
            .gap(px(14.))
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
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
                    ),
            );

        root = root.child(
            section("常规", full_rows, cx),
        );
        root = root.child(chips(
            "拒绝进程（这些进程上的输入会被拒绝）",
            KNOWN_DENY_PROCESSES,
            &deny_processes,
            Chip::DenyProcess as fn(usize) -> Chip,
            cx,
        ));
        root = root.child(chips(
            "拒绝修饰键（含这些修饰键的组合键会被拒绝）",
            KNOWN_DENY_KEY_MODIFIERS,
            &deny_mods,
            Chip::DenyModifier as fn(usize) -> Chip,
            cx,
        ));

        // save button + status
        let mut bar = div().flex().items_center().justify_between().mt(px(6.));
        bar = bar.child(
            div().flex().flex_col().gap(px(2.)).child(match (&self.status, &self.error) {
                (Some(s), _) => div()
                    .text_size(px(11.5))
                    .text_color(rgb(0x22c55e))
                    .child(s.to_string()),
                (_, Some(e)) => div()
                    .text_size(px(11.5))
                    .text_color(rgb(0xef4444))
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
        root = root.child(bar);

        root
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

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(680.), px(700.)), cx);
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
            full_mode: true,
            require_shot_id_for_coords: false,
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
        let reloaded = Settings::load();
        assert!(!reloaded.pointer_preview);
        assert!(reloaded.session_start_show_viewer);
        assert!(!reloaded.require_shot_id_for_coords);
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
