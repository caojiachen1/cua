// main.rs — cua: Windows computer-use CLI.
// Feature fusion: Qoder (observe->act->refresh, screenshotId staleness guard, no-negotiables,
// confirmation levels), Trae (tool surface incl. batch/clipboard/grounding/verify steps),
// ZCode (a11y-first, strategy auto|a11y|event, action receipts, background observation),
// + sandbox hidden-desktop executor for true background control.
mod capture;
mod input;
mod rec;
mod sandbox;
mod ubb;
mod uia;
mod util;
mod win;

use anyhow::{anyhow, bail, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};

use util::{Receipt, SafetyConfig};
use win::{resolve_window, WinInfo};

#[derive(Parser)]
#[command(name = "cua", version, about = "Windows computer-use CLI (Trae + Qoder + ZCode fusion)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Health & permission probe (Trae check_permissions)
    Check,
    /// List top-level windows/apps (Qoder list_apps / Trae list_apps / ZCode listApps)
    Apps {
        #[arg(long)]
        all: bool,
    },
    /// Window state: UIA tree + screenshot + state_id (Qoder get_window_state)
    State {
        #[arg(long)]
        window: String,
        #[arg(long, default_value_t = 12)]
        depth: usize,
        #[arg(long)]
        no_shot: bool,
        #[arg(long)]
        out: Option<String>,
        #[arg(long, help = "diff against previous state of this window (ZCode getAXState diff)")]
        diff: bool,
        #[arg(long, help = "redact text values (Trae mask)")]
        redact: bool,
        #[arg(long, help = "also list supported UIA patterns per element (slower)")]
        patterns: bool,
    },
    /// Screenshot a window (works for background/occluded) or the screen
    Shot {
        #[arg(long)]
        window: Option<String>,
        #[arg(long, conflicts_with = "window")]
        screen: bool,
        #[arg(long)]
        out: String,
    },
    /// Resolve a text query to element indices + coordinates (Trae grounding, local UIA)
    Grounding {
        #[arg(long)]
        window: String,
        #[arg(long)]
        query: String,
    },
    /// Current pointer position and window under it
    Cursor,
    /// Move the pointer to an element or coordinate (hover; no click)
    Hover {
        #[arg(long)]
        window: Option<String>,
        #[arg(long)]
        index: Option<usize>,
        #[arg(long, allow_hyphen_values = true)]
        x: Option<i32>,
        #[arg(long, allow_hyphen_values = true)]
        y: Option<i32>,
        #[arg(long, help = "state_id from the last state/shot (Qoder screenshotId guard)")]
        shot_id: Option<String>,
        #[arg(long)]
        record: Option<String>,
    },
    /// Click element by a11y index or screen coordinates (ZCode strategy a11y|event)
    Click {
        #[arg(long)]
        window: Option<String>,
        #[arg(long)]
        index: Option<usize>,
        #[arg(long, allow_hyphen_values = true)]
        x: Option<i32>,
        #[arg(long, allow_hyphen_values = true)]
        y: Option<i32>,
        #[arg(long, help = "state_id from the last state/shot (Qoder screenshotId guard)")]
        shot_id: Option<String>,
        #[arg(long)]
        right: bool,
        #[arg(long)]
        double: bool,
        #[arg(long, default_value = "auto")]
        strategy: String,
        #[arg(long, help = "deliver via window messages instead of real input (no focus change)")]
        background: bool,
        #[arg(long)]
        record: Option<String>,
        #[arg(long)]
        sandbox: bool,
    },
    /// Type text into the focused (or given) window via SendInput unicode
    Type {
        #[arg(long)]
        text: String,
        #[arg(long)]
        window: Option<String>,
        #[arg(long, help = "deliver via window messages instead of real input (no focus change)")]
        background: bool,
        #[arg(long)]
        record: Option<String>,
        #[arg(long)]
        sandbox: bool,
    },
    /// Press a key combo, e.g. "ctrl+s" (Win key refused by policy)
    Key {
        #[arg(long)]
        keys: String,
        #[arg(long)]
        window: Option<String>,
        #[arg(long)]
        confirm: bool,
        #[arg(long, help = "deliver via window messages instead of real input (no focus change)")]
        background: bool,
        #[arg(long)]
        record: Option<String>,
        #[arg(long)]
        sandbox: bool,
    },
    /// Scroll (wheel notches)
    Scroll {
        #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
        dx: i32,
        #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
        dy: i32,
        #[arg(long, allow_hyphen_values = true)]
        x: Option<i32>,
        #[arg(long, allow_hyphen_values = true)]
        y: Option<i32>,
        #[arg(long)]
        window: Option<String>,
        #[arg(long)]
        shot_id: Option<String>,
        #[arg(long)]
        record: Option<String>,
        #[arg(long)]
        sandbox: bool,
    },
    /// Drag from (x1,y1) to (x2,y2) with real input
    Drag {
        #[arg(long, allow_hyphen_values = true)]
        x1: i32,
        #[arg(long, allow_hyphen_values = true)]
        y1: i32,
        #[arg(long, allow_hyphen_values = true)]
        x2: i32,
        #[arg(long, allow_hyphen_values = true)]
        y2: i32,
        #[arg(long)]
        shot_id: Option<String>,
        #[arg(long)]
        record: Option<String>,
        #[arg(long)]
        sandbox: bool,
    },
    /// Set element value via UIA ValuePattern (Qoder/Trae set_value)
    #[command(name = "set-value")]
    SetValue {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long, conflicts_with = "clear", required_unless_present = "clear")]
        value: Option<String>,
        #[arg(long, help = "set the field to an empty string")]
        clear: bool,
        #[arg(long)]
        record: Option<String>,
        #[arg(long)]
        sandbox: bool,
    },
    /// Read element value via UIA ValuePattern (Trae get_value)
    #[command(name = "get-value")]
    GetValue {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long)]
        sandbox: bool,
    },
    /// Toggle a checkbox/radio via UIA TogglePattern
    Toggle {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long)]
        record: Option<String>,
        #[arg(long)]
        sandbox: bool,
    },
    /// Select a list/tab/tree item via UIA SelectionItemPattern
    Select {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long)]
        record: Option<String>,
        #[arg(long)]
        sandbox: bool,
    },
    /// Expand a combo box/menu/tree item via UIA ExpandCollapsePattern
    Expand {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long)]
        record: Option<String>,
        #[arg(long)]
        sandbox: bool,
    },
    /// Collapse a combo box/menu/tree item via UIA ExpandCollapsePattern
    Collapse {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long)]
        record: Option<String>,
        #[arg(long)]
        sandbox: bool,
    },
    /// Set a slider/spinner value via UIA RangeValuePattern
    #[command(name = "set-range")]
    SetRange {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long, allow_hyphen_values = true)]
        value: f64,
        #[arg(long)]
        record: Option<String>,
        #[arg(long)]
        sandbox: bool,
    },
    /// Read a slider/spinner value via UIA RangeValuePattern
    #[command(name = "get-range")]
    GetRange {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long)]
        sandbox: bool,
    },
    /// Read a UIA Grid/Table as a 2D matrix, or resolve/act on one (row, col) cell
    Grid {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long, allow_hyphen_values = true)]
        row: Option<i32>,
        #[arg(long, allow_hyphen_values = true)]
        col: Option<i32>,
        #[arg(long, help = "with --row/--col: drive the cell through the a11y chain")]
        invoke: bool,
        #[arg(long, default_value_t = 200, help = "cell budget for the full dump")]
        max_cells: usize,
    },
    /// Report or apply UIA TransformPattern (move/resize without SetWindowPos or the mouse)
    Transform {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long, allow_hyphen_values = true)]
        move_to: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        size: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        zoom: Option<f64>,
        #[arg(long)]
        record: Option<String>,
    },
    /// Force a virtualized UIA item to materialize so its children appear in the tree
    Realize {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
    },
    /// Clipboard read/write (Trae clipboard)
    Clipboard {
        #[command(subcommand)]
        op: ClipOp,
    },
    /// Launch an executable (Qoder launch_app / Trae launch_app)
    Launch {
        #[arg(long)]
        exe: String,
        #[arg(long, allow_hyphen_values = true)]
        args: Option<String>,
        #[arg(long, help = "working directory for the new process")]
        cwd: Option<String>,
        #[arg(long, help = "inherit this shell's environment verbatim (default: strip ELECTRON_/NODE_ vars that break app startup)")]
        no_clean_env: bool,
        #[arg(long)]
        record: Option<String>,
        #[arg(long)]
        sandbox: bool,
    },
    /// Open a registered app/URL by name via ShellExecute (Trae open_app)
    Open {
        #[arg(long)]
        name: String,
        #[arg(long)]
        record: Option<String>,
    },
    /// Activate (focus) a window (Qoder activate_window)
    Activate {
        #[arg(long)]
        window: String,
        #[arg(long)]
        record: Option<String>,
    },
    Minimize {
        #[arg(long)]
        window: String,
        #[arg(long)]
        record: Option<String>,
    },
    Maximize {
        #[arg(long)]
        window: String,
        #[arg(long)]
        record: Option<String>,
    },
    /// Restore a minimized/maximized window to normal size
    Restore {
        #[arg(long)]
        window: String,
        #[arg(long)]
        record: Option<String>,
    },
    /// Move/resize a window (SetWindowPos; no focus change)
    Resize {
        #[arg(long)]
        window: String,
        #[arg(long, allow_hyphen_values = true)]
        width: Option<i32>,
        #[arg(long, allow_hyphen_values = true)]
        height: Option<i32>,
        #[arg(long, allow_hyphen_values = true)]
        x: Option<i32>,
        #[arg(long, allow_hyphen_values = true)]
        y: Option<i32>,
        #[arg(long)]
        record: Option<String>,
    },
    /// Wait until a window exists / disappears / becomes foreground
    Wait {
        #[arg(long)]
        window: String,
        #[arg(long, help = "wait until the window no longer exists")]
        gone: bool,
        #[arg(long, help = "wait until the window is foreground")]
        foreground: bool,
        #[arg(long, default_value_t = 10000)]
        timeout_ms: u64,
        #[arg(long, default_value_t = 100)]
        interval_ms: u64,
    },
    /// Close a window (Always-Confirm level: needs --confirm, Qoder policy)
    Close {
        #[arg(long)]
        window: String,
        #[arg(long)]
        confirm: bool,
        #[arg(long)]
        record: Option<String>,
    },
    /// Run a sequence of actions from a JSON file (Trae batch)
    Batch {
        #[arg(long)]
        file: String,
        #[arg(long)]
        continue_on_error: bool,
    },
    /// Replay a recording; --verify compares per-step state fingerprints (Trae verify_recorded_step)
    Replay {
        name: String,
        #[arg(long)]
        verify: bool,
    },
    /// Hidden-desktop executor: full real input on an invisible desktop (true background)
    Sandbox {
        #[command(subcommand)]
        op: SandboxOp,
    },
    /// Browser control bridged to UBB (the user's own Edge/Chrome); `browser help` lists tools
    Browser {
        /// ubb tool name + JSON params, e.g. `tab.create {"url":"example.com"}`
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        rest: Vec<String>,
    },
    /// Screen recording (JPEG frames -> mp4, or an HTML player when ffmpeg is missing)
    Rec {
        #[command(subcommand)]
        op: RecOp,
    },
}

#[derive(Subcommand)]
enum RecOp {
    /// Start capturing in a detached background process
    Start {
        #[arg(long)]
        window: Option<String>,
        #[arg(long, default_value_t = 2)]
        fps: u32,
        #[arg(long)]
        out: Option<String>,
        #[arg(long, default_value_t = 900, help = "hard stop so a forgotten recording cannot fill the disk")]
        max_seconds: u64,
    },
    /// Stop, encode, and report the artifact
    Stop {
        #[arg(long)]
        out: Option<String>,
        #[arg(long)]
        keep_frames: bool,
    },
    Status,
    /// Internal: the detached capture loop
    #[command(hide = true)]
    Capture {
        #[arg(long)]
        dir: String,
        #[arg(long)]
        fps: u32,
        #[arg(long)]
        window: Option<String>,
        #[arg(long, default_value_t = 900)]
        max_seconds: u64,
    },
}

#[derive(Subcommand)]
enum ClipOp {
    Get,
    Set {
        #[arg(long)]
        text: String,
        #[arg(long)]
        record: Option<String>,
    },
}

#[derive(Subcommand)]
enum SandboxOp {
    Start {
        #[arg(long)]
        exe: String,
        #[arg(long, allow_hyphen_values = true)]
        args: Option<String>,
    },
    Apps,
    State {
        #[arg(long)]
        window: String,
        #[arg(long, default_value_t = 12)]
        depth: usize,
        #[arg(long)]
        no_shot: bool,
        #[arg(long)]
        out: Option<String>,
        #[arg(long, help = "also list supported UIA patterns per element (slower)")]
        patterns: bool,
    },
    Shot {
        #[arg(long)]
        window: String,
        #[arg(long)]
        out: String,
    },
    Click {
        #[arg(long)]
        window: Option<String>,
        #[arg(long)]
        index: Option<usize>,
        #[arg(long, allow_hyphen_values = true)]
        x: Option<i32>,
        #[arg(long, allow_hyphen_values = true)]
        y: Option<i32>,
        #[arg(long)]
        shot_id: Option<String>,
        #[arg(long)]
        right: bool,
        #[arg(long)]
        double: bool,
    },
    Type {
        #[arg(long)]
        text: String,
        #[arg(long)]
        window: Option<String>,
    },
    Key {
        #[arg(long)]
        keys: String,
        #[arg(long)]
        window: Option<String>,
        #[arg(long)]
        confirm: bool,
    },
    Scroll {
        #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
        dx: i32,
        #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
        dy: i32,
        #[arg(long)]
        window: Option<String>,
    },
    Drag {
        #[arg(long, allow_hyphen_values = true)]
        x1: i32,
        #[arg(long, allow_hyphen_values = true)]
        y1: i32,
        #[arg(long, allow_hyphen_values = true)]
        x2: i32,
        #[arg(long, allow_hyphen_values = true)]
        y2: i32,
    },
    #[command(name = "get-value")]
    GetValue {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
    },
    #[command(name = "set-value")]
    SetValue {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long, conflicts_with = "clear", required_unless_present = "clear")]
        value: Option<String>,
        #[arg(long, help = "set the field to an empty string")]
        clear: bool,
    },
    /// Launch an additional app on the running sandbox desktop (additive)
    Launch {
        #[arg(long)]
        exe: String,
        #[arg(long, allow_hyphen_values = true)]
        args: Option<String>,
    },
    /// Close one sandbox window gracefully (Always-Confirm level)
    Close {
        #[arg(long)]
        window: String,
        #[arg(long)]
        confirm: bool,
    },
    Toggle {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
    },
    Select {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
    },
    Expand {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
    },
    Collapse {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
    },
    #[command(name = "set-range")]
    SetRange {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long, allow_hyphen_values = true)]
        value: f64,
    },
    #[command(name = "get-range")]
    GetRange {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
    },
    Stop,
    /// (internal) resident desktop-handle keeper
    Keepalive,
}


fn main() {
    win::dpi_aware();
    // UTF-8 console output so JSON with CJK titles survives pipes (PowerShell decodes UTF-8)
    unsafe {
        let _ = windows::Win32::System::Console::SetConsoleOutputCP(65001);
        let _ = windows::Win32::System::Console::SetConsoleCP(65001);
    }
    let cli = Cli::parse();
    let code = dispatch(cli.cmd);
    std::process::exit(code);
}

fn dispatch(cmd: Cmd) -> i32 {
    let r: Result<i32> = (|| {
        match cmd {
            Cmd::Check => cmd_check(),
            Cmd::Apps { all } => cmd_apps(all),
            Cmd::State { window, depth, no_shot, out, diff, redact, patterns } => {
                cmd_state(&window, depth, no_shot, out, diff, redact, patterns)
            }
            Cmd::Shot { window, screen, out } => cmd_shot(window, screen, &out),
            Cmd::Grounding { window, query } => cmd_grounding(&window, &query),
            Cmd::Cursor => cmd_cursor(),
            Cmd::Hover { window, index, x, y, shot_id, record } => {
                cmd_hover(window, index, x, y, shot_id, record)
            }
            Cmd::Click { window, index, x, y, shot_id, right, double, strategy, background, record, sandbox } => {
                cmd_click(window, index, x, y, shot_id, right, double, &strategy, background, record, sandbox)
            }
            Cmd::Type { text, window, background, record, sandbox } => {
                cmd_type(&text, window, background, record, sandbox)
            }
            Cmd::Key { keys, window, confirm, background, record, sandbox } => {
                cmd_key(&keys, window, confirm, background, record, sandbox)
            }
            Cmd::Scroll { dx, dy, x, y, window, shot_id, record, sandbox } => {
                cmd_scroll(dx, dy, x, y, window, shot_id, record, sandbox)
            }
            Cmd::Drag { x1, y1, x2, y2, shot_id, record, sandbox } => cmd_drag(x1, y1, x2, y2, shot_id, record, sandbox),
            Cmd::SetValue { window, index, value, clear, record, sandbox } => {
                let v = if clear {
                    String::new()
                } else {
                    value.ok_or_else(|| anyhow!("--value or --clear required"))?
                };
                cmd_set_value(&window, index, &v, record, sandbox)
            }
            Cmd::GetValue { window, index, sandbox } => cmd_get_value(&window, index, sandbox),
            Cmd::Toggle { window, index, record, sandbox } => cmd_toggle(&window, index, record, sandbox),
            Cmd::Select { window, index, record, sandbox } => cmd_select(&window, index, record, sandbox),
            Cmd::Expand { window, index, record, sandbox } => cmd_expand(&window, index, record, sandbox),
            Cmd::Collapse { window, index, record, sandbox } => cmd_collapse(&window, index, record, sandbox),
            Cmd::SetRange { window, index, value, record, sandbox } => {
                cmd_set_range(&window, index, value, record, sandbox)
            }
            Cmd::GetRange { window, index, sandbox } => cmd_get_range(&window, index, sandbox),
            Cmd::Grid { window, index, row, col, invoke, max_cells } => {
                cmd_grid(&window, index, row, col, invoke, max_cells)
            }
            Cmd::Transform { window, index, move_to, size, zoom, record } => {
                cmd_transform(&window, index, move_to, size, zoom, record)
            }
            Cmd::Realize { window, index } => cmd_realize(&window, index),
            Cmd::Clipboard { op } => cmd_clipboard(op),
            Cmd::Launch { exe, args, cwd, no_clean_env, record, sandbox } => {
                cmd_launch(&exe, args, cwd, !no_clean_env, record, sandbox)
            }
            Cmd::Open { name, record } => cmd_open(&name, record),
            Cmd::Activate { window, record } => cmd_window_op(&window, "activate", record, false),
            Cmd::Minimize { window, record } => cmd_window_op(&window, "minimize", record, false),
            Cmd::Maximize { window, record } => cmd_window_op(&window, "maximize", record, false),
            Cmd::Restore { window, record } => cmd_window_op(&window, "restore", record, false),
            Cmd::Resize { window, width, height, x, y, record } => {
                cmd_resize(&window, width, height, x, y, record)
            }
            Cmd::Wait { window, gone, foreground, timeout_ms, interval_ms } => {
                cmd_wait(&window, gone, foreground, timeout_ms, interval_ms)
            }
            Cmd::Close { window, confirm, record } => cmd_window_op(&window, "close", record, confirm),
            Cmd::Batch { file, continue_on_error } => cmd_batch(&file, continue_on_error),
            Cmd::Replay { name, verify } => cmd_replay(&name, verify),
            Cmd::Sandbox { op } => sandbox::dispatch(op),
            Cmd::Browser { rest } => ubb::cmd_browser(&rest),
            Cmd::Rec { op } => match op {
                RecOp::Start { window, fps, out, max_seconds } => rec::cmd_start(window, fps, out, max_seconds),
                RecOp::Stop { out, keep_frames } => rec::cmd_stop(out, keep_frames),
                RecOp::Status => rec::cmd_status(),
                RecOp::Capture { dir, fps, window, max_seconds } => rec::cmd_capture(&dir, fps, window, max_seconds),
            },
        }
    })();
    match r {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    }
}

// ---------------- helpers ----------------

pub(crate) struct Target {
    pub(crate) info: WinInfo,
    pub(crate) hwnd: windows::Win32::Foundation::HWND,
}

pub(crate) fn resolve_target(sel: &str) -> Result<Target> {
    let hwnd = resolve_window(sel)?;
    let info = win::snapshot_window(hwnd).ok_or_else(|| anyhow!("cannot snapshot window"))?;
    Ok(Target { info, hwnd })
}

fn safety_gate(cfg: &SafetyConfig, t: &Target) -> Result<(), String> {
    util::check_target_allowed(cfg, t.info.pid, t.info.hwnd)
}

fn require_shot_id(cfg: &SafetyConfig, shot_id: &Option<String>, target: &Target) -> Result<(), String> {
    if !cfg.require_shot_id_for_coords {
        return Ok(());
    }
    check_shot_id(shot_id.as_deref(), target.hwnd)
}

/// Freshness half of the screenshotId guard (no target window needed).
pub fn check_shot_fresh(shot_id: Option<&str>) -> Result<(), String> {
    let last = util::load_last_state()
        .ok_or_else(|| "coordinate action refused: no fresh observation — run 'cua state'/'cua shot' first (Qoder screenshotId policy)".to_string())?;
    let expected = last["state_id"].as_str().unwrap_or("");
    let got = shot_id.unwrap_or("");
    if got != expected || expected.is_empty() {
        return Err(format!(
            "coordinate action refused: stale or missing shot_id (got '{}', expected fresh id from last observation)",
            got
        ));
    }
    Ok(())
}

/// Qoder screenshotId guard, scoped to the observation's window: a fresh screen
/// capture (hwnd 0) covers any point; a window capture only covers that window.
pub fn check_shot_id(shot_id: Option<&str>, target_hwnd: windows::Win32::Foundation::HWND) -> Result<(), String> {
    check_shot_fresh(shot_id)?;
    let last = util::load_last_state().unwrap_or(Value::Null);
    let observed = last["hwnd"].as_i64().unwrap_or(0);
    if observed != 0 {
        let a = win::root_window(windows::Win32::Foundation::HWND(observed as *mut _));
        let b = win::root_window(target_hwnd);
        if a.0 != b.0 {
            return Err(format!(
                "coordinate action refused: the last observation was for hwnd {:#x}, but the target is hwnd {:#x} — re-observe the target window (shot_id is scoped to its window; screen captures cover any point)",
                observed, b.0 as isize
            ));
        }
    }
    Ok(())
}

fn target_json(t: &Target) -> Value {
    json!({
        "hwnd": t.info.hwnd,
        "title": t.info.title,
        "class": t.info.class,
        "pid": t.info.pid,
        "process": t.info.process,
        "rect": t.info.rect,
    })
}

/// Run an action with receipt + optional recording. Returns exit code.
fn guarded_action<F>(
    cmd_name: &str,
    argv: Vec<String>,
    record: &Option<String>,
    target_of: impl Fn() -> Result<Target>,
    pre: impl Fn(&SafetyConfig, &Target) -> Result<(), String>,
    f: F,
) -> Result<i32>
where
    F: FnOnce(&Target) -> Result<(String, Option<String>)>, // -> (detail, state_id_after)
{
    let started = std::time::Instant::now();
    let cfg = util::load_config();
    let receipt = match target_of() {
        Err(e) => Receipt::new(cmd_name).fail(Value::Null, &format!("{e}")),
        Ok(t) => {
            if let Err(reason) = pre(&cfg, &t) {
                Receipt::new(cmd_name).refused(target_json(&t), &reason)
            } else {
                match f(&t) {
                    Ok((detail, sid)) => {
                        let mut r = Receipt::new(cmd_name).ok(target_json(&t), &detail);
                        r.state_id = sid;
                        r
                    }
                    Err(e) => Receipt::new(cmd_name).fail(target_json(&t), &format!("{e:#}")),
                }
            }
        }
    };
    let mut receipt = receipt;
    receipt.elapsed_ms = Some(started.elapsed().as_millis());
    if let Some(name) = record {
        let sig_after = if receipt.status == "ok" {
            receipt
                .target
                .get("hwnd")
                .and_then(|h| h.as_i64())
                .and_then(|h| util::state_fingerprint(h as isize).ok())
        } else {
            None
        };
        let _ = util::record_append(name, cmd_name, &json!(argv), &receipt, sig_after.as_deref());
        if receipt.status == "ok" {
            receipt.detail = Some(format!(
                "{}; recorded to '{}'",
                receipt.detail.clone().unwrap_or_default(),
                name
            ));
        }
    }
    receipt.print();
    Ok(receipt.exit_code())
}

fn record_argv(record: &Option<String>) -> Vec<String> {
    if record.is_none() {
        return Vec::new();
    }
    // store argv without exe path and without the --record <name> pair so replay does not re-record
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if a == "--record" {
            skip = true;
            continue;
        }
        out.push(a);
    }
    out
}

// ---------------- observation commands ----------------

fn cmd_check() -> Result<i32> {
    let (vx, vy, vw, vh, pw, ph) = win::screen_metrics();
    let elevated = win::is_elevated();
    let uia_ok = uia::automation().is_ok();
    let recordings = std::fs::read_dir(util::ensure_data_dir().join("recordings"))
        .map(|d| d.count())
        .unwrap_or(0);
    let cfg = util::load_config();
    let (cx, cy) = input::cursor_pos();
    let ubb_cli = ubb::resolve().map(|c| c.label());
    let out = json!({
        "tool": "cua",
        "version": env!("CARGO_PKG_VERSION"),
        "platform": "windows",
        "elevated": elevated,
        "uia": if uia_ok { "ok" } else { "unavailable" },
        "screen": {"virtual": [vx, vy, vw, vh], "primary": [pw, ph]},
        "cursor": [cx, cy],
        "sandbox": sandbox::status(),
        "browser": {"backend": "ubb", "cli": ubb_cli},
        "safety": {
            "deny_processes": cfg.deny_processes,
            "require_shot_id_for_coords": cfg.require_shot_id_for_coords,
            "levels": cfg.levels,
        },
        "receipt_schema": util::RECEIPT_SCHEMA,
        "data_dir": util::data_dir().to_string_lossy(),
        "config_path": util::data_dir().join("config.json").to_string_lossy(),
        "recordings": recordings,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(0)
}

fn cmd_apps(all: bool) -> Result<i32> {
    let wins = win::list_windows(all);
    let fg = win::foreground_hwnd();
    let fg_info = win::snapshot_window(fg);
    let v = json!({
        "count": wins.len(),
        "foreground_hwnd": fg.0 as isize,
        "foreground_title": fg_info.map(|w| w.title),
        "windows": wins,
    });
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(0)
}

fn node_key(ctype: &str, name: &str, class: &str, rect: [i32; 4]) -> String {
    format!(
        "{}\u{1}{}\u{1}{}\u{1}{},{},{},{}",
        ctype, name, class, rect[0], rect[1], rect[2], rect[3]
    )
}

fn node_key_json(n: &Value) -> Option<String> {
    let rect = n["rect"].as_array()?;
    let r: Vec<i32> = rect.iter().filter_map(|v| v.as_i64().map(|x| x as i32)).collect();
    if r.len() != 4 {
        return None;
    }
    Some(node_key(
        n["type"].as_str().or(n["ctype"].as_str()).unwrap_or(""),
        n["name"].as_str().unwrap_or(""),
        n["class"].as_str().unwrap_or(""),
        [r[0], r[1], r[2], r[3]],
    ))
}

fn cmd_state(
    sel: &str,
    depth: usize,
    no_shot: bool,
    out: Option<String>,
    diff: bool,
    redact: bool,
    patterns: bool,
) -> Result<i32> {
    let t = resolve_target(sel)?;
    let tree = uia::walk_window(t.hwnd, depth, 2000, redact, patterns)?;
    let state_id = util::new_state_id();
    let prev_elems: Vec<Value> = util::load_elements(t.info.hwnd)
        .and_then(|e| serde_json::from_value::<Vec<Value>>(e).ok())
        .unwrap_or_default();

    // diff against previous state of the same window
    let diff_v = if diff {
        let prev = util::load_last_state();
        match prev {
            Some(p) if p["hwnd"].as_i64() == Some(t.info.hwnd as i64) => {
                // multisets, not sets: two identical "Untitled" tabs must count as +1
                let mut cur_counts: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();
                for n in tree.nodes.iter() {
                    *cur_counts
                        .entry(format!("{}|{}", n.ctype, n.name))
                        .or_insert(0) += 1;
                }
                let mut prev_counts: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();
                for n in prev_elems.iter() {
                    *prev_counts
                        .entry(format!(
                            "{}|{}",
                            n["type"].as_str().or(n["ctype"].as_str()).unwrap_or(""),
                            n["name"].as_str().unwrap_or("")
                        ))
                        .or_insert(0)
                        += 1;
                }
                let mut added: Vec<String> = Vec::new();
                let mut removed: Vec<String> = Vec::new();
                let mut added_n = 0usize;
                let mut removed_n = 0usize;
                for (k, c) in &cur_counts {
                    let pc = prev_counts.get(k).copied().unwrap_or(0);
                    if *c > pc {
                        added_n += c - pc;
                        added.push(format!("{k} x{c}"));
                    }
                }
                for (k, c) in &prev_counts {
                    let cc = cur_counts.get(k).copied().unwrap_or(0);
                    if *c > cc {
                        removed_n += c - cc;
                        removed.push(format!("{k} x{c}"));
                    }
                }
                let mut prev_values: std::collections::HashMap<String, String> = std::collections::HashMap::new();
                for n in &prev_elems {
                    if let Some(key) = node_key_json(n) {
                        prev_values.insert(key, n["value"].as_str().unwrap_or("").to_string());
                    }
                }
                let mut value_changes = 0usize;
                let mut examples_values: Vec<Value> = Vec::new();
                if !redact {
                    for n in &tree.nodes {
                        let key = node_key(&n.ctype, &n.name, &n.class, n.rect);
                        if let Some(pv) = prev_values.get(&key) {
                            let cv = n.value.clone().unwrap_or_default();
                            if pv != &cv {
                                value_changes += 1;
                                if examples_values.len() < 5 {
                                    examples_values.push(json!({
                                        "type": n.ctype, "name": n.name, "from": pv, "to": cv
                                    }));
                                }
                            }
                        }
                    }
                }
                json!({
                    "changed": p["sig"] != Value::String(tree.sig.clone()),
                    "added": added_n,
                    "removed": removed_n,
                    "value_changes": if redact { Value::Null } else { json!(value_changes) },
                    "examples_added": added.iter().take(5).cloned().collect::<Vec<_>>(),
                    "examples_removed": removed.iter().take(5).cloned().collect::<Vec<_>>(),
                    "examples_value_changes": examples_values,
                })
            }
            _ => json!({"changed": null, "note": "no previous state for this window"}),
        }
    } else {
        Value::Null
    };

    // elements registry for index-based actions
    let elems: Vec<Value> = tree
        .nodes
        .iter()
        .map(|n| {
            json!({
                "i": n.i, "type": n.ctype, "name": n.name, "class": n.class,
                "rect": n.rect, "rid": n.rid, "disabled": n.disabled, "offscreen": n.offscreen,
                "value": n.value, "toggle": n.toggle, "selected": n.selected,
                "expanded": n.expanded, "range": n.range, "patterns": n.patterns,
            })
        })
        .collect();
    util::save_elements(t.info.hwnd, &json!(elems));
    util::save_last_state(&state_id, t.info.hwnd, &tree.sig);

    // screenshot
    let shot_path = if no_shot {
        None
    } else {
        let path = out.map(std::path::PathBuf::from).unwrap_or_else(|| {
            util::ensure_data_dir()
                .join("cache")
                .join(format!("shot-{}-{state_id}.png", t.info.hwnd))
        });
        let shot = capture::shot_window(t.hwnd, false);
        match shot {
            Ok(s) => {
                capture::save_png(&s, &path)?;
                Some(json!({"path": path.to_string_lossy(), "width": s.width, "height": s.height, "rendered": s.rendered}))
            }
            Err(e) => Some(json!({"error": format!("{e:#}")})),
        }
    };

    let v = json!({
        "state_id": state_id,
        "window": target_json(&t),
        "node_count": tree.nodes.len(),
        "sig": tree.sig,
        "nodes": elems,
        "screenshot": shot_path,
        "diff": diff_v,
    });
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(0)
}

fn cmd_shot(window: Option<String>, screen: bool, out: &str) -> Result<i32> {
    let (shot, hwnd) = if screen {
        (capture::shot_screen(true)?, 0isize)
    } else {
        let sel = window.ok_or_else(|| anyhow!("--window or --screen required"))?;
        let t = resolve_target(&sel)?;
        (capture::shot_window(t.hwnd, false)?, t.info.hwnd)
    };
    let path = std::path::PathBuf::from(out);
    capture::save_png(&shot, &path)?;
    let state_id = util::new_state_id();
    util::save_last_state(&state_id, hwnd, "");
    let v = json!({
        "state_id": state_id,
        "path": path.to_string_lossy(),
        "width": shot.width,
        "height": shot.height,
        "rendered": shot.rendered,
    });
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(0)
}

fn cmd_grounding(sel: &str, query: &str) -> Result<i32> {
    let t = resolve_target(sel)?;
    let hits = uia::ground(t.hwnd, query)?;
    let items: Vec<Value> = hits
        .iter()
        .map(|n| {
            let center = [
                n.rect[0] + n.rect[2] / 2,
                n.rect[1] + n.rect[3] / 2,
            ];
            json!({"i": n.i, "type": n.ctype, "name": n.name, "rect": n.rect, "center": center, "rid": n.rid})
        })
        .collect();
    let v = json!({
        "query": query,
        "found": !items.is_empty(),
        "matches": items,
        "note": "local UIA grounding (Trae com.bytedance/grounding analog; on-device, no cloud model)"
    });
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(0)
}

fn cmd_cursor() -> Result<i32> {
    let (x, y) = input::cursor_pos();
    let win = window_from_point(x, y).ok().and_then(|h| {
        let root = win::root_window(h);
        win::snapshot_window(root).map(|info| {
            json!({
                "hwnd": info.hwnd, "title": info.title, "class": info.class,
                "pid": info.pid, "process": info.process, "rect": info.rect,
            })
        })
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-pointer-v1",
            "x": x,
            "y": y,
            "window": win,
        }))?
    );
    Ok(0)
}

fn cmd_hover(
    window: Option<String>,
    index: Option<usize>,
    x: Option<i32>,
    y: Option<i32>,
    shot_id: Option<String>,
    record: Option<String>,
) -> Result<i32> {
    let argv = record_argv(&record);
    guarded_action(
        "hover",
        argv,
        &record,
        || {
            if let Some(sel) = &window {
                return resolve_target(sel);
            }
            if let (Some(px), Some(py)) = (x, y) {
                let h = window_from_point(px, py)?;
                let info =
                    win::snapshot_window(h).ok_or_else(|| anyhow!("cannot snapshot window under point"))?;
                return Ok(Target { info, hwnd: h });
            }
            bail!("--window (element mode) or --x/--y (coordinate mode) required")
        },
        |cfg, t| {
            safety_gate(cfg, t)?;
            if index.is_none() {
                require_shot_id(cfg, &shot_id, t)?;
            }
            Ok(())
        },
        |t| {
            if let Some(idx) = index {
                match target_for_index(t.info.hwnd, idx)? {
                    ElementTarget::Rid(rid) => {
                        let (_rect, (cx, cy), name) = uia::element_center(t.hwnd, &rid)?;
                        input::move_to(cx, cy)?;
                        Ok((format!("hovered at ({cx},{cy}) over '{}'", name), None))
                    }
                    ElementTarget::Rect { rect, ctype, name } => {
                        let (cx, cy, via) = locate_rect(t.hwnd, &rect, &ctype, &name)?;
                        input::move_to(cx, cy)?;
                        Ok((format!("no runtime id; {via}, hovered at ({cx},{cy})"), None))
                    }
                }
            } else {
                let (px, py) = (x.unwrap(), y.unwrap());
                input::move_to(px, py)?;
                Ok((format!("hovered at ({px},{py})"), None))
            }
        },
    )
}

// ---------------- action commands ----------------

fn cmd_click(
    window: Option<String>,
    index: Option<usize>,
    x: Option<i32>,
    y: Option<i32>,
    shot_id: Option<String>,
    right: bool,
    double: bool,
    strategy: &str,
    background: bool,
    record: Option<String>,
    sandbox: bool,
) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_click(window, index, x, y, shot_id, right, double);
    }
    let argv = record_argv(&record);
    let strategy = strategy.to_string();
    guarded_action(
        "click",
        argv,
        &record,
        || {
            if let Some(sel) = &window {
                return resolve_target(sel);
            }
            if let (Some(px), Some(py)) = (x, y) {
                let h = window_from_point(px, py)?;
                let info =
                    win::snapshot_window(h).ok_or_else(|| anyhow!("cannot snapshot window under point"))?;
                return Ok(Target { info, hwnd: h });
            }
            bail!("--window (element mode) or --x/--y (coordinate mode) required")
        },
        |cfg, t| {
            safety_gate(cfg, t)?;
            if index.is_some() {
                return Ok(());
            }
            require_shot_id(cfg, &shot_id, t)
        },
        |t| {
            if let Some(idx) = index {
                match target_for_index(t.info.hwnd, idx)? {
                    ElementTarget::Rid(rid) => {
                        let d = if strategy == "a11y" {
                            uia::click_element_strict(t.hwnd, &rid, double, right)?
                        } else {
                            uia::click_element(t.hwnd, &rid, double, right)?
                        };
                        Ok((d, Some(util::new_state_id())))
                    }
                    ElementTarget::Rect { rect, ctype, name } => {
                        let (cx, cy, via) = locate_rect(t.hwnd, &rect, &ctype, &name)?;
                        if background {
                            let root = win::root_window(t.hwnd);
                            input::post_click(root, cx, cy, right, double)?;
                            Ok((format!("no runtime id; {via}, clicked ({cx},{cy}) via message-level input"), Some(util::new_state_id())))
                        } else {
                            input::click(cx, cy, right, double)?;
                            Ok((format!("no runtime id; {via}, clicked ({cx},{cy})"), Some(util::new_state_id())))
                        }
                    }
                }
            } else {
                let (x, y) = (x.ok_or_else(|| anyhow!("--index or --x/--y required"))?, y.ok_or_else(|| anyhow!("--y required"))?);
                if background {
                    let root = win::root_window(t.hwnd);
                    input::post_click(root, x, y, right, double)?;
                    Ok((
                        format!("clicked ({x},{y}) via message-level input (background; focus untouched)"),
                        Some(util::new_state_id()),
                    ))
                } else {
                    input::click(x, y, right, double)?;
                    Ok((format!("clicked ({x},{y}) via event strategy"), Some(util::new_state_id())))
                }
            }
        },
    )
}

/// What a cached element index can be re-resolved to. Some providers (QQ Music's
/// TXGuiFoundation) hand out an empty runtime id, leaving the rectangle as the only handle.
pub enum ElementTarget {
    Rid(Vec<i32>),
    Rect { rect: [i32; 4], ctype: String, name: String },
}

fn cached_node(hwnd: isize, idx: usize) -> Result<serde_json::Value> {
    let cache = util::load_elements(hwnd).ok_or_else(|| anyhow!("no state for this window — run 'cua state --window ...' first"))?;
    let arr = cache.as_array().ok_or_else(|| anyhow!("corrupt element cache"))?;
    arr.iter()
        .find(|n| n["i"].as_u64() == Some(idx as u64))
        .cloned()
        .ok_or_else(|| anyhow!("index {idx} not in last state (count {}) — re-observe", arr.len()))
}

pub fn target_for_index(hwnd: isize, idx: usize) -> Result<ElementTarget> {
    let node = cached_node(hwnd, idx)?;
    let rid: Vec<i32> = node["rid"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_i64().map(|x| x as i32)).collect())
        .unwrap_or_default();
    if !rid.is_empty() {
        return Ok(ElementTarget::Rid(rid));
    }
    let rect: Vec<i64> = node["rect"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();
    if rect.len() == 4 && rect[2] > 0 && rect[3] > 0 {
        return Ok(ElementTarget::Rect {
            rect: [rect[0] as i32, rect[1] as i32, rect[2] as i32, rect[3] as i32],
            ctype: node["type"].as_str().unwrap_or_default().to_string(),
            name: node["name"].as_str().unwrap_or_default().to_string(),
        });
    }
    bail!("index {idx} has neither a runtime id nor a usable rectangle — re-observe");
}

/// Without a runtime id an index cannot be re-resolved by identity, and a churning tree
/// (QQ Music renumbers its nodes constantly) would silently send the click elsewhere.
/// Re-walk and relocate the element by name+type; use the cached rectangle only when the
/// element carries no name to search for.
pub fn locate_rect(hwnd: windows::Win32::Foundation::HWND, rect: &[i32; 4], ctype: &str, name: &str) -> Result<(i32, i32, String)> {
    let cached = (rect[0] + rect[2] / 2, rect[1] + rect[3] / 2);
    if name.trim().is_empty() {
        return Ok((cached.0, cached.1, "cached rect (unnamed element)".into()));
    }
    let hits = uia::ground(hwnd, name).unwrap_or_default();
    let mut best: Option<&uia::Node> = None;
    for n in hits.iter() {
        let exact_name = n.name == name;
        let same_type = n.ctype.eq_ignore_ascii_case(ctype);
        if !exact_name && !same_type {
            continue;
        }
        match best {
            Some(b) => {
                let better = (exact_name && b.name != name) || (same_type && b.ctype != ctype);
                if better {
                    best = Some(n);
                }
            }
            None => best = Some(n),
        }
    }
    match best {
        Some(n) => Ok((
            n.rect[0] + n.rect[2] / 2,
            n.rect[1] + n.rect[3] / 2,
            format!("re-located '{}' by name+type", n.name),
        )),
        None => bail!(
            "index has no runtime id and '{}' is no longer in the tree — re-observe before acting",
            name
        ),
    }
}

pub fn rid_for_index(hwnd: isize, idx: usize) -> Result<Vec<i32>> {
    match target_for_index(hwnd, idx)? {
        ElementTarget::Rid(rid) => Ok(rid),
        ElementTarget::Rect { .. } => bail!(
            "index {idx} has no runtime id: this app's UIA provider does not expose one, so pattern \
             actions cannot target it — use 'click --index' (falls back to its rectangle) or --x/--y"
        ),
    }
}

fn cmd_type(
    text: &str,
    window: Option<String>,
    background: bool,
    record: Option<String>,
    sandbox: bool,
) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_type(&text, window);
    }
    let argv = record_argv(&record);
    let text = text.to_string();
    guarded_action(
        "type",
        argv,
        &record,
        || match &window {
            Some(sel) => resolve_target(sel),
            None => {
                let h = win::foreground_hwnd();
                let info = win::snapshot_window(h).ok_or_else(|| anyhow!("no foreground window"))?;
                Ok(Target { info, hwnd: h })
            }
        },
        |cfg, t| safety_gate(cfg, t),
        |t| {
            if background {
                let root = win::root_window(t.hwnd);
                let target = input::find_input_target(root);
                let n = input::post_type(target, &text)?;
                return Ok((
                    format!("typed {n} units via message-level input (background; focus untouched)"),
                    None,
                ));
            }
            if window.is_some() {
                require_foreground(t.hwnd)?;
            }
            input::type_text(&text)?;
            Ok((format!("typed {} chars", text.chars().count()), None))
        },
    )
}

fn cmd_key(
    keys: &str,
    window: Option<String>,
    confirm: bool,
    background: bool,
    record: Option<String>,
    sandbox: bool,
) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_key(&keys, window, confirm);
    }
    let argv = record_argv(&record);
    let combo = input::parse_combo(keys)?;
    let keys = keys.to_string();
    guarded_action(
        "key",
        argv,
        &record,
        || match &window {
            Some(sel) => resolve_target(sel),
            None => {
                let h = win::foreground_hwnd();
                let info = win::snapshot_window(h).ok_or_else(|| anyhow!("no foreground window"))?;
                Ok(Target { info, hwnd: h })
            }
        },
        |cfg, t| {
            safety_gate(cfg, t)?;
            // Qoder Always-Confirm: alt+f4 (and win key handled at parse)
            if keys.eq_ignore_ascii_case("alt+f4") && !confirm {
                return Err("refused: 'alt+f4' is Always-Confirm level — pass --confirm".into());
            }
            Ok(())
        },
        |t| {
            if background {
                let root = win::root_window(t.hwnd);
                let target = input::find_input_target(root);
                if let Some(text) = &combo.text {
                    let n = input::post_type(target, text)?;
                    return Ok((
                        format!("typed {n} printable units for key '{keys}' via message-level input (background)"),
                        None,
                    ));
                }
                input::post_key(target, &combo)?;
                return Ok((
                    format!("pressed {keys} via message-level input (background; shortcuts reading raw key state need foreground)"),
                    None,
                ));
            }
            if window.is_some() {
                require_foreground(t.hwnd)?;
            }
            if let Some(text) = &combo.text {
                input::type_text(text)?;
                return Ok((
                    format!("typed printable key '{keys}' as text (IME-safe; use ctrl/alt combos for shortcuts)"),
                    None,
                ));
            }
            input::press_combo(&combo)?;
            Ok((format!("pressed {keys}"), None))
        },
    )
}

fn cmd_scroll(
    dx: i32,
    dy: i32,
    x: Option<i32>,
    y: Option<i32>,
    window: Option<String>,
    shot_id: Option<String>,
    record: Option<String>,
    sandbox: bool,
) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_scroll(dx, dy, window);
    }
    let argv = record_argv(&record);
    guarded_action(
        "scroll",
        argv,
        &record,
        || {
            let t = match &window {
                Some(sel) => Some(resolve_target(sel)?),
                None => None,
            };
            // need a Target even when window omitted: use point/foreground
            match t {
                Some(t) => Ok(t),
                None => {
                    let (px, py) = match (x, y) {
                        (Some(a), Some(b)) => (a, b),
                        _ => input::cursor_pos(),
                    };
                    // find window under point for receipt/safety
                    let h = window_from_point(px, py)?;
                    let info = win::snapshot_window(h).ok_or_else(|| anyhow!("cannot snapshot window under point"))?;
                    Ok(Target { info, hwnd: h })
                }
            }
        },
        |cfg, t| {
            safety_gate(cfg, t)?;
            // explicit coordinates are a coordinate action: same screenshotId guard
            if x.is_some() && y.is_some() {
                require_shot_id(cfg, &shot_id, t)?;
            }
            Ok(())
        },
        |t| {
            let (px, py) = match (x, y) {
                (Some(a), Some(b)) => (a, b),
                _ => {
                    let r = t.info.rect;
                    (r[0] + r[2] / 2, r[1] + r[3] / 2)
                }
            };
            if window.is_some() {
                win::activate_window(t.hwnd)?;
            }
            input::scroll(px, py, dx, dy)?;
            Ok((format!("scrolled dx={dx} dy={dy} at ({px},{py})"), None))
        },
    )
}

/// Keyboard input goes to whatever the OS has focused, so a window we could not raise means
/// the keystrokes would land in the user's application. Refuse instead of misfiring.
fn require_foreground(hwnd: windows::Win32::Foundation::HWND) -> Result<()> {
    if win::activate_window(hwnd)? {
        return Ok(());
    }
    bail!(
        "cannot bring this window to the foreground (Windows focus-steal protection), so nothing \
         was typed — use --background, an a11y action (set-value/toggle/click --index), or ask \
         the user to click it first"
    )
}

fn window_from_point(x: i32, y: i32) -> Result<windows::Win32::Foundation::HWND> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::WindowFromPoint;
    let h = unsafe { WindowFromPoint(POINT { x, y }) };
    if h.is_invalid() {
        bail!("no window at point");
    }
    Ok(h)
}

fn cmd_drag(x1: i32, y1: i32, x2: i32, y2: i32, shot_id: Option<String>, record: Option<String>, sandbox: bool) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_drag(x1, y1, x2, y2);
    }
    let argv = record_argv(&record);
    guarded_action(
        "drag",
        argv,
        &record,
        || {
            // target = window under start point
            let h = window_from_point(x1, y1)?;
            let info = win::snapshot_window(h).ok_or_else(|| anyhow!("cannot snapshot window"))?;
            Ok(Target { info, hwnd: h })
        },
        |cfg, t| {
            safety_gate(cfg, t)?;
            require_shot_id(cfg, &shot_id, t)
        },
        |_t| {
            input::drag(x1, y1, x2, y2)?;
            Ok((format!("dragged ({x1},{y1})->({x2},{y2})"), Some(util::new_state_id())))
        },
    )
}

fn cmd_set_value(sel: &str, index: usize, value: &str, record: Option<String>, sandbox: bool) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_set_value(sel, index, &value);
    }
    let argv = record_argv(&record);
    let value = value.to_string();
    guarded_action(
        "set-value",
        argv,
        &record,
        || resolve_target(sel),
        |cfg, t| safety_gate(cfg, t),
        |t| {
            let rid = rid_for_index(t.info.hwnd, index)?;
            let d = uia::set_value_element(t.hwnd, &rid, &value)?;
            Ok((d, Some(util::new_state_id())))
        },
    )
}

fn cmd_get_value(sel: &str, index: usize, sandbox: bool) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_get_value(sel, index);
    }
    let t = resolve_target(sel)?;
    let rid = rid_for_index(t.info.hwnd, index)?;
    let v = uia::get_value_element(t.hwnd, &rid)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-value-v1",
            "window": target_json(&t),
            "index": index,
            "value": v,
        }))?
    );
    Ok(0)
}

fn cmd_toggle(sel: &str, index: usize, record: Option<String>, sandbox: bool) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_toggle(sel, index);
    }
    let argv = record_argv(&record);
    guarded_action(
        "toggle",
        argv,
        &record,
        || resolve_target(sel),
        |cfg, t| safety_gate(cfg, t),
        |t| {
            let rid = rid_for_index(t.info.hwnd, index)?;
            let (d, state) = uia::toggle_element(t.hwnd, &rid)?;
            Ok((format!("{d} (now {state})"), Some(util::new_state_id())))
        },
    )
}

fn cmd_select(sel: &str, index: usize, record: Option<String>, sandbox: bool) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_select(sel, index);
    }
    let argv = record_argv(&record);
    guarded_action(
        "select",
        argv,
        &record,
        || resolve_target(sel),
        |cfg, t| safety_gate(cfg, t),
        |t| {
            let rid = rid_for_index(t.info.hwnd, index)?;
            let (d, selected) = uia::select_element(t.hwnd, &rid)?;
            Ok((format!("{d} (selected={selected})"), Some(util::new_state_id())))
        },
    )
}

fn cmd_expand(sel: &str, index: usize, record: Option<String>, sandbox: bool) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_expand(sel, index);
    }
    let argv = record_argv(&record);
    guarded_action(
        "expand",
        argv,
        &record,
        || resolve_target(sel),
        |cfg, t| safety_gate(cfg, t),
        |t| {
            let rid = rid_for_index(t.info.hwnd, index)?;
            let (d, state) = uia::expand_element(t.hwnd, &rid)?;
            Ok((format!("{d} (now {state})"), Some(util::new_state_id())))
        },
    )
}

fn cmd_collapse(sel: &str, index: usize, record: Option<String>, sandbox: bool) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_collapse(sel, index);
    }
    let argv = record_argv(&record);
    guarded_action(
        "collapse",
        argv,
        &record,
        || resolve_target(sel),
        |cfg, t| safety_gate(cfg, t),
        |t| {
            let rid = rid_for_index(t.info.hwnd, index)?;
            let (d, state) = uia::collapse_element(t.hwnd, &rid)?;
            Ok((format!("{d} (now {state})"), Some(util::new_state_id())))
        },
    )
}

fn cmd_set_range(sel: &str, index: usize, value: f64, record: Option<String>, sandbox: bool) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_set_range(sel, index, value);
    }
    let argv = record_argv(&record);
    guarded_action(
        "set-range",
        argv,
        &record,
        || resolve_target(sel),
        |cfg, t| safety_gate(cfg, t),
        |t| {
            let rid = rid_for_index(t.info.hwnd, index)?;
            let d = uia::set_range_element(t.hwnd, &rid, value)?;
            Ok((d, Some(util::new_state_id())))
        },
    )
}

fn parse_pair(spec: &str) -> Result<(f64, f64)> {
    let mut it = spec.split([',', 'x', ' ']).filter(|p| !p.is_empty());
    let a: f64 = it.next().ok_or_else(|| anyhow!("expected \"a,b\""))?.parse()?;
    let b: f64 = it.next().ok_or_else(|| anyhow!("expected \"a,b\""))?.parse()?;
    Ok((a, b))
}

fn cmd_grid(sel: &str, index: usize, row: Option<i32>, col: Option<i32>, invoke: bool, max_cells: usize) -> Result<i32> {
    let t = resolve_target(sel)?;
    let rid = rid_for_index(t.info.hwnd, index)?;
    let name = t
        .info
        .title
        .clone();
    let out = match (row, col) {
        (Some(r), Some(c)) => uia::grid_cell(t.hwnd, &rid, r, c, invoke)?,
        _ => uia::grid_dump(t.hwnd, &rid, max_cells)?,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-grid-v1",
            "window": target_json(&t),
            "index": index,
            "element": name,
            "grid": out,
        }))?
    );
    Ok(0)
}

fn cmd_transform(
    sel: &str,
    index: usize,
    move_to: Option<String>,
    size: Option<String>,
    zoom: Option<f64>,
    record: Option<String>,
) -> Result<i32> {
    let argv = record_argv(&record);
    let mov = move_to.as_deref().map(parse_pair).transpose()?;
    let sz = size.as_deref().map(parse_pair).transpose()?;
    guarded_action(
        "transform",
        argv,
        &record,
        || resolve_target(sel),
        |_cfg, _t| Ok(()),
        |t| {
            let rid = rid_for_index(t.info.hwnd, index)?;
            if mov.is_none() && sz.is_none() && zoom.is_none() {
                let caps = uia::transform_report(t.hwnd, &rid)?;
                return Ok((format!("capabilities: {}", caps), None));
            }
            let d = uia::transform_do(t.hwnd, &rid, mov, sz, zoom)?;
            Ok((d, None))
        },
    )
}

fn cmd_realize(sel: &str, index: usize) -> Result<i32> {
    let t = resolve_target(sel)?;
    let rid = rid_for_index(t.info.hwnd, index)?;
    let d = uia::realize(t.hwnd, &rid)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-realize-v1",
            "window": target_json(&t),
            "index": index,
            "detail": d,
        }))?
    );
    Ok(0)
}

fn cmd_get_range(sel: &str, index: usize, sandbox: bool) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_get_range(sel, index);
    }
    let t = resolve_target(sel)?;
    let rid = rid_for_index(t.info.hwnd, index)?;
    let v = uia::get_range_element(t.hwnd, &rid)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-range-v1",
            "window": target_json(&t),
            "index": index,
            "range": v,
        }))?
    );
    Ok(0)
}

fn cmd_clipboard(op: ClipOp) -> Result<i32> {
    match op {
        ClipOp::Get => {
            let text = clipboard_get()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"schema": "cua-clipboard-v1", "text": text}))?
            );
            Ok(0)
        }
        ClipOp::Set { text, record } => {
            let argv = record_argv(&record);
            let len = text.chars().count();
            guarded_action(
                "clipboard",
                argv,
                &record,
                || {
                    let h = win::foreground_hwnd();
                    let info = win::snapshot_window(h).unwrap_or(WinInfo {
                        hwnd: 0,
                        title: "(no window)".into(),
                        class: String::new(),
                        pid: 0,
                        process: String::new(),
                        visible: false,
                        minimized: false,
                        maximized: false,
                        foreground: false,
                        rect: [0, 0, 0, 0],
                        desktop_area: false,
                    });
                    Ok(Target { info, hwnd: h })
                },
                |_, _| Ok(()),
                |_t| {
                    clipboard_set(&text)?;
                    Ok((format!("clipboard set ({len} chars)"), None))
                },
            )
        }
    }
}

pub fn clipboard_get() -> Result<String> {
    use windows::Win32::System::DataExchange::{GetClipboardData, OpenClipboard, CloseClipboard};
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::CF_UNICODETEXT;
    unsafe {
        OpenClipboard(None).map_err(|e| anyhow!("OpenClipboard: {e}"))?;
        let result = (|| -> Result<String> {
            let h = GetClipboardData(CF_UNICODETEXT.0 as u32).map_err(|e| anyhow!("GetClipboardData: {e}"))?;
            let hg = windows::Win32::Foundation::HGLOBAL(h.0);
            let ptr = GlobalLock(hg) as *const u16;
            if ptr.is_null() {
                bail!("GlobalLock failed");
            }
            let mut len = 0usize;
            while *ptr.add(len) != 0 && len < 1 << 20 {
                len += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            let _ = GlobalUnlock(hg);
            Ok(s)
        })();
        let _ = CloseClipboard();
        result
    }
}

pub fn clipboard_set(text: &str) -> Result<()> {
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::CF_UNICODETEXT;
    use windows::Win32::Foundation::HANDLE;
    unsafe {
        OpenClipboard(None).map_err(|e| anyhow!("OpenClipboard: {e}"))?;
        let result = (|| -> Result<()> {
            EmptyClipboard().map_err(|e| anyhow!("EmptyClipboard: {e}"))?;
            let mut wide: Vec<u16> = text.encode_utf16().collect();
            wide.push(0);
            let bytes = wide.len() * 2;
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(|e| anyhow!("GlobalAlloc: {e}"))?;
            let ptr = GlobalLock(h) as *mut u16;
            if ptr.is_null() {
                bail!("GlobalLock failed");
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(h);
            SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(h.0))
                .map_err(|e| anyhow!("SetClipboardData: {e}"))?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}

fn cmd_launch(
    exe: &str,
    args: Option<String>,
    cwd: Option<String>,
    clean_env: bool,
    record: Option<String>,
    sandbox: bool,
) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_launch(exe, args);
    }
    let argv = record_argv(&record);
    let exe = exe.to_string();
    let args = args.clone();
    let cwd2 = cwd.clone();
    guarded_action(
        "launch",
        argv,
        &record,
        || Ok(Target {
            info: WinInfo {
                hwnd: 0, title: exe.clone(), class: String::new(), pid: 0,
                process: exe.clone(), visible: false, minimized: false, maximized: false,
                foreground: false, rect: [0, 0, 0, 0], desktop_area: false,
            },
            hwnd: windows::Win32::Foundation::HWND::default(),
        }),
        |cfg, t| {
            // safety: launching a denied binary/URI (cmd.exe, ms-settings:...) is refused
            if let Some(d) = util::denied_token(cfg, &t.info.process) {
                return Err(format!(
                    "refused by safety policy: launching '{}' is on the no-negotiable deny list (matched '{d}')",
                    t.info.process
                ));
            }
            Ok(())
        },
        |_t| {
            let stripped = if clean_env { util::stripped_env_keys() } else { Vec::new() };
            let pid = spawn_process_opts(&exe, args.as_deref(), None, cwd2.as_deref(), clean_env)?;
            let mut detail = format!("launched pid {pid}");
            if let Some(c) = &cwd {
                detail.push_str(&format!(" (cwd {c})"));
            }
            if !stripped.is_empty() {
                detail.push_str(&format!("; env sanitized, stripped {}", stripped.join(",")));
            }
            Ok((detail, None))
        },
    )
}

pub fn spawn_process(exe: &str, args: Option<&str>, desktop: Option<&str>) -> Result<u32> {
    spawn_process_opts(exe, args, desktop, None, true)
}

/// Launch a process. `clean_env` strips automation-shell variables that break other
/// apps' startup (see util::sanitized_env_block); `cwd` sets their working directory.
pub fn spawn_process_opts(
    exe: &str,
    args: Option<&str>,
    desktop: Option<&str>,
    cwd: Option<&str>,
    clean_env: bool,
) -> Result<u32> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Threading::{
        CreateProcessW, CREATE_NEW_CONSOLE, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
        STARTUPINFOW,
    };
    let mut cmd: Vec<u16> = if exe.contains('\\') || exe.contains('/') {
        format!("\"{exe}\" {}", args.unwrap_or("")).trim_end().encode_utf16().collect()
    } else {
        format!("{exe} {}", args.unwrap_or("")).trim_end().encode_utf16().collect()
    };
    cmd.push(0);
    let mut si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut desktop_w: Option<Vec<u16>> = None;
    if let Some(d) = desktop {
        let mut w: Vec<u16> = d.encode_utf16().collect();
        w.push(0);
        si.lpDesktop = windows::core::PWSTR(w.as_mut_ptr());
        desktop_w = Some(w);
    }
    let env_block = if clean_env { Some(util::sanitized_env_block()) } else { None };
    let cwd_w: Option<Vec<u16>> = cwd.map(|c| c.encode_utf16().chain(std::iter::once(0)).collect());
    let cwd_arg = match &cwd_w {
        Some(v) => PCWSTR(v.as_ptr()),
        None => PCWSTR::null(),
    };
    let mut pi = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            windows::core::PWSTR(cmd.as_mut_ptr()),
            None,
            None,
            false,
            CREATE_NEW_CONSOLE | CREATE_UNICODE_ENVIRONMENT,
            env_block.as_ref().map(|b| b.as_ptr() as *const _),
            cwd_arg,
            &si,
            &mut pi,
        )
        .map_err(|e| anyhow!("CreateProcessW('{exe}'): {e}"))?;
    }
    let _ = (desktop_w, cwd_w, env_block);
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(pi.hThread);
        let _ = windows::Win32::Foundation::CloseHandle(pi.hProcess);
    }
    Ok(pi.dwProcessId)
}

fn cmd_open(name: &str, record: Option<String>) -> Result<i32> {
    let argv = record_argv(&record);
    let name = name.to_string();
    guarded_action(
        "open",
        argv,
        &record,
        || Ok(Target {
            info: WinInfo {
                hwnd: 0, title: name.clone(), class: String::new(), pid: 0,
                process: String::new(), visible: false, minimized: false, maximized: false,
                foreground: false, rect: [0, 0, 0, 0], desktop_area: false,
            },
            hwnd: windows::Win32::Foundation::HWND::default(),
        }),
        |cfg, t| {
            // safety: ShellExecute of a denied binary/alias/scheme is refused.
            // http/https are not deny-listed, so ordinary URLs still open.
            if let Some(d) = util::denied_token(cfg, &t.info.title) {
                return Err(format!(
                    "refused by safety policy: opening '{}' is on the no-negotiable deny list (matched '{d}')",
                    t.info.title
                ));
            }
            Ok(())
        },
        |t| {
            // An .exe target goes through CreateProcess so we can hand it a clean
            // environment and its own directory as cwd, like a shell launch does.
            if name.to_ascii_lowercase().ends_with(".exe") && std::path::Path::new(&name).exists() {
                let stripped = util::stripped_env_keys();
                let cwd = std::path::Path::new(&name)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string());
                let pid = spawn_process_opts(&name, None, None, cwd.as_deref(), true)?;
                let mut detail = format!("opened '{name}' (pid {pid}) with a sanitized environment");
                if !stripped.is_empty() {
                    detail.push_str(&format!("; stripped {}", stripped.join(",")));
                }
                if let Some(c) = &cwd {
                    detail.push_str(&format!("; cwd {c}"));
                }
                return Ok((detail, None));
            }
            let _ = t;
            use windows::Win32::Foundation::HINSTANCE;
            use windows::core::PCWSTR;
            use windows::Win32::UI::Shell::ShellExecuteW;
            use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
            let mut verb_w: Vec<u16> = "open\0".encode_utf16().collect();
            let mut file_w: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
            let r: HINSTANCE = unsafe {
                ShellExecuteW(
                    None,
                    PCWSTR(verb_w.as_mut_ptr()),
                    PCWSTR(file_w.as_mut_ptr()),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    SW_SHOWNORMAL,
                )
            };
            if (r.0 as isize) <= 32 {
                bail!("ShellExecuteW failed (code {}) for '{name}'", r.0 as isize);
            }
            Ok((format!("opened '{name}' via ShellExecute"), None))
        },
    )
}

fn cmd_window_op(sel: &str, op: &str, record: Option<String>, confirm: bool) -> Result<i32> {
    let argv = record_argv(&record);
    let op = op.to_string();
    let confirm_flag = confirm;
    guarded_action(
        &op,
        argv,
        &record,
        || resolve_target(sel),
        |cfg, t| {
            safety_gate(cfg, t)?;
            if op == "close" && !confirm_flag {
                return Err("refused: 'close' is Always-Confirm level (Qoder policy) — pass --confirm".into());
            }
            Ok(())
        },
        |t| {
            use windows::Win32::UI::WindowsAndMessaging::{
                PostMessageW, ShowWindow, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, WM_CLOSE,
            };
            unsafe {
                match op.as_str() {
                    "activate" => {
                        let ok = win::activate_window(t.hwnd)?;
                        if !ok {
                            bail!("SetForegroundWindow was blocked (focus-steal protection); retry once");
                        }
                        Ok(("window activated".into(), None))
                    }
                    "minimize" => {
                        let _ = ShowWindow(t.hwnd, SW_MINIMIZE);
                        Ok(("window minimized".into(), None))
                    }
                    "maximize" => {
                        let _ = ShowWindow(t.hwnd, SW_MAXIMIZE);
                        Ok(("window maximized".into(), None))
                    }
                    "restore" => {
                        let _ = ShowWindow(t.hwnd, SW_RESTORE);
                        Ok(("window restored".into(), None))
                    }
                    "close" => {
                        let _ = PostMessageW(t.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                        // wait for the window to die (graceful), force-kill as fallback
                        for _ in 0..30 {
                            if !win::is_window_alive(t.hwnd) {
                                return Ok(("window closed gracefully".into(), None));
                            }
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                        let mut pid = 0u32;
                        windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
                            t.hwnd,
                            Some(&mut pid),
                        );
                        let killed = win::kill_process_tree(pid);
                        Ok((format!("window did not close in 3s; force-terminated {killed} process(es)"), None))
                    }
                    _ => bail!("unknown op"),
                }
            }
        },
    )
}

fn cmd_resize(
    sel: &str,
    width: Option<i32>,
    height: Option<i32>,
    x: Option<i32>,
    y: Option<i32>,
    record: Option<String>,
) -> Result<i32> {
    let argv = record_argv(&record);
    guarded_action(
        "resize",
        argv,
        &record,
        || resolve_target(sel),
        |cfg, t| safety_gate(cfg, t),
        |t| {
            use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};
            let cur = t.info.rect;
            let nx = x.unwrap_or(cur[0]);
            let ny = y.unwrap_or(cur[1]);
            let nw = width.unwrap_or(cur[2]);
            let nh = height.unwrap_or(cur[3]);
            if nw < 50 || nh < 50 {
                bail!("refusing resize below 50x50 (got {nw}x{nh})");
            }
            unsafe {
                SetWindowPos(t.hwnd, None, nx, ny, nw, nh, SWP_NOZORDER | SWP_NOACTIVATE)
                    .map_err(|e| anyhow!("SetWindowPos: {e}"))?;
            }
            Ok((
                format!(
                    "window resized from {}x{} at ({},{}) to {nw}x{nh} at ({nx},{ny})",
                    cur[2], cur[3], cur[0], cur[1]
                ),
                None,
            ))
        },
    )
}

fn cmd_wait(sel: &str, gone: bool, foreground: bool, timeout_ms: u64, interval_ms: u64) -> Result<i32> {
    let started = std::time::Instant::now();
    let interval = std::time::Duration::from_millis(interval_ms.max(20));
    let timeout = std::time::Duration::from_millis(timeout_ms.max(50));
    let mut last: Option<WinInfo>;
    loop {
        let found = win::resolve_window(sel).ok().and_then(win::snapshot_window);
        last = found.clone();
        let satisfied = if gone {
            found.is_none()
        } else {
            match &found {
                Some(w) => !foreground || w.foreground,
                None => false,
            }
        };
        if satisfied {
            let elapsed = started.elapsed().as_millis();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "schema": "cua-wait-v1",
                    "window": sel,
                    "gone": gone,
                    "foreground": foreground,
                    "satisfied": true,
                    "elapsed_ms": elapsed,
                    "found": found,
                }))?
            );
            return Ok(0);
        }
        if started.elapsed() >= timeout {
            let elapsed = started.elapsed().as_millis();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "schema": "cua-wait-v1",
                    "window": sel,
                    "gone": gone,
                    "foreground": foreground,
                    "satisfied": false,
                    "elapsed_ms": elapsed,
                    "found": last,
                }))?
            );
            return Ok(1);
        }
        std::thread::sleep(interval);
    }
}

use windows::Win32::Foundation::{LPARAM, WPARAM};

fn cmd_batch(file: &str, continue_on_error: bool) -> Result<i32> {
    let raw = std::fs::read_to_string(file)?.trim_start_matches('\u{feff}').to_string();
    let v: Value = serde_json::from_str(&raw)?;
    let steps: Vec<Vec<String>> = if let Some(arr) = v.as_array() {
        arr.iter().map(|s| to_argv(s)).collect::<Result<_>>()?
    } else if let Some(arr) = v["steps"].as_array() {
        arr.iter().map(|s| to_argv(s)).collect::<Result<_>>()?
    } else {
        bail!("batch file must be a JSON array of argv arrays or {{\"steps\": [...]}}");
    };
    let mut results = Vec::new();
    let mut codes = Vec::new();
    for (i, argv) in steps.iter().enumerate() {
        eprintln!("[batch {}/{}] {:?}", i + 1, steps.len(), argv);
        let code = run_argv(argv)?;
        codes.push(code);
        results.push(json!({"step": i, "argv": argv, "exit": code}));
        if code != 0 && !continue_on_error {
            break;
        }
    }
    let ok = codes.iter().all(|c| *c == 0);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-batch-result-v1",
            "steps": results,
            "ok": ok,
        }))?
    );
    Ok(if ok { 0 } else { 1 })
}

fn to_argv(v: &Value) -> Result<Vec<String>> {
    if let Some(a) = v.as_array() {
        return a
            .iter()
            .map(|x| x.as_str().map(|s| s.to_string()).ok_or_else(|| anyhow!("argv entries must be strings")))
            .collect();
    }
    bail!("step must be an argv array");
}

fn run_argv(argv: &[String]) -> Result<i32> {
    let exe = std::env::current_exe()?;
    let out = std::process::Command::new(exe).args(argv).output()?;
    // do NOT forward child stdout (it is a separate JSON doc); echo to stderr for humans
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !stdout.trim().is_empty() {
        eprintln!("[step] {}", stdout.trim_end());
    }
    if !stderr.trim().is_empty() {
        eprintln!("{}", stderr.trim_end());
    }
    Ok(out.status.code().unwrap_or(1))
}

fn cmd_replay(name: &str, verify: bool) -> Result<i32> {
    let path = util::ensure_data_dir().join("recordings").join(format!("{}.jsonl", util::sanitize(name)));
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| anyhow!("recording '{name}' not found at {}", path.display()))?;
    let mut results = Vec::new();
    let mut all_ok = true;
    let mut all_verified = true;
    for (i, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let rec: Value = serde_json::from_str(line)?;
        let cmd = rec["cmd"].as_str().unwrap_or("").to_string();
        let sig_before = rec["state_sig_after"].as_str().map(|s| s.to_string());
        let mut argv: Vec<String> = rec["args"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        if argv.first().map(|a| a.as_str()) != Some(cmd.as_str()) {
            argv.insert(0, cmd.clone());
        }
        eprintln!("[replay {}/{}] {:?}", i + 1, cmd, argv);
        let out = std::process::Command::new(std::env::current_exe()?)
            .args(&argv)
            .output()?;
        let code = out.status.code().unwrap_or(1);
        let step_ok = code == 0;
        let mut verified: Option<bool> = None;
        if verify {
            match &sig_before {
                Some(expected) => {
                    // recompute fingerprint of the target window after replayed action
                    let hwnd = rec["args"]
                        .as_array()
                        .and_then(|a| {
                            a.iter().position(|x| x == "--window").and_then(|p| a.get(p + 1)).and_then(|v| v.as_str().map(|s| s.to_string()))
                        });
                    if let Some(sel) = hwnd {
                        if let Ok(t) = resolve_target(&sel) {
                            if let Ok(cur) = util::state_fingerprint(t.info.hwnd) {
                                verified = Some(cur == *expected);
                            }
                        }
                    }
                    if verified != Some(true) {
                        all_verified = false;
                    }
                }
                None => {
                    verified = None;
                }
            }
        }
        if !step_ok {
            all_ok = false;
        }
        results.push(json!({
            "step": i, "cmd": cmd, "exit": code, "ok": step_ok,
            "stdout_tail": String::from_utf8_lossy(&out.stdout).lines().last().unwrap_or(""),
            "verified": verified,
        }));
        if !step_ok {
            break; // stop on first failure, like batch
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-replay-result-v1",
            "recording": name,
            "steps": results,
            "ok": all_ok,
            "verified": if verify { Value::Bool(all_verified) } else { Value::Null },
        }))?
    );
    Ok(if all_ok && (!verify || all_verified) { 0 } else { 1 })
}
