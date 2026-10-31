// main.rs — cua: Windows computer-use CLI.
// Feature fusion: Qoder (observe->act->refresh, screenshotId staleness guard, no-negotiables,
// confirmation levels), Trae (tool surface incl. batch/clipboard/grounding/verify steps),
// ZCode (a11y-first, strategy auto|a11y|event, action receipts, background observation),
// + sandbox hidden-desktop executor for true background control.
mod action;
mod capture;
mod input;
mod ocr;
mod rec;
mod sandbox;
mod ubb;
mod uia;
mod util;
mod win;

use anyhow::{anyhow, bail, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};

use action::{ActionOutcome, ActionRequest, InputMode};
use util::{Receipt, SafetyConfig};
use win::{resolve_window, WinInfo};

#[derive(Parser)]
#[command(name = "cua", version, about = "Windows computer-use CLI (Trae + Qoder + ZCode fusion)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
    /// Resolve every --index against this immutable observation id instead of
    /// the latest observation. A wrong id fails the action; it never silently
    /// re-observes.
    #[arg(long, global = true, value_name = "ID")]
    observation_id: Option<String>,
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
        #[arg(long, help = "walk the raw view instead of the control view: includes IsControlElement=false nodes the control view hides (decorative panes, MSAA-bridged legacy content)")]
        raw: bool,
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
    /// Which UIA element sits at a screen coordinate (ElementFromPoint refined to
    /// the innermost control on control+raw views; passive — no pointer/focus change)
    ElementAt {
        #[arg(long, allow_hyphen_values = true)]
        x: i32,
        #[arg(long, allow_hyphen_values = true)]
        y: i32,
    },
    /// Current pointer position and window under it
    Cursor,
    /// Local Windows OCR over a window, the screen, or an image file
    Ocr {
        #[arg(long, conflicts_with_all = ["screen", "file"])]
        window: Option<String>,
        #[arg(long, conflicts_with = "file")]
        screen: bool,
        #[arg(long, conflicts_with_all = ["window", "screen"])]
        file: Option<String>,
        #[arg(long, default_value_t = 200)]
        max_lines: usize,
    },
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
        #[arg(long, help = "deliver WM_MOUSEMOVE via window messages instead of moving the real pointer")]
        background: bool,
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
        #[arg(long, help = "with --background: deliver WM_CHAR to this element's field (multi-edit windows)")]
        index: Option<usize>,
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
        #[arg(long, help = "deliver WM_MOUSEWHEEL via window messages instead of real input (no focus change)")]
        background: bool,
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
        #[arg(long, help = "deliver button/move messages instead of real input (no cursor movement)")]
        background: bool,
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
    /// Add an item to a multi-select container
    #[command(name = "select-add")]
    SelectAdd {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long)]
        record: Option<String>,
        #[arg(long)]
        sandbox: bool,
    },
    /// Remove an item from a multi-select container
    #[command(name = "select-remove")]
    SelectRemove {
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
    /// Read UIA Table/TableItem headers and major order
    Table {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long, default_value_t = 20)]
        max_headers: usize,
        #[arg(long)]
        sandbox: bool,
    },
    /// Find an item through an ItemContainer provider
    #[command(name = "find-item")]
    FindItem {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long, default_value = "name", help = "name|automation-id|class-name|help-text|item-type|item-status")]
        property: String,
        #[arg(long)]
        value: String,
        #[arg(long)]
        sandbox: bool,
    },
    /// Read an element's TextPattern document range (bounded)
    Text {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long, default_value_t = 4000, help = "maximum characters returned")]
        max_chars: usize,
        #[arg(long)]
        sandbox: bool,
    },
    /// Scroll an item into view via UIA ScrollItemPattern
    #[command(name = "scroll-item")]
    ScrollItem {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long, help = "align the item to the top of the viewport")]
        align_top: bool,
        #[arg(long)]
        record: Option<String>,
        #[arg(long)]
        sandbox: bool,
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
    /// Wait until a window exists/disappears/is foreground, or a cached element
    /// reaches a value/toggle/selection/expand/enabled state.
    Wait {
        #[arg(long)]
        window: String,
        #[arg(long, help = "wait until the window no longer exists")]
        gone: bool,
        #[arg(long, help = "wait until the window is foreground")]
        foreground: bool,
        #[arg(long, help = "cached element index to poll (requires at least one condition)")]
        index: Option<usize>,
        #[arg(long, help = "with --index: wait until ValuePattern/RangeValue equals this")]
        value: Option<String>,
        #[arg(long, help = "with --index: wait until TogglePattern is on|off|indeterminate")]
        toggle: Option<String>,
        #[arg(long, help = "with --index: wait until SelectionItemPattern is selected (true|false)")]
        selected: Option<bool>,
        #[arg(long, help = "with --index: wait until ExpandCollapsePattern is expanded|collapsed")]
        expanded: Option<String>,
        #[arg(long, help = "with --index: wait until the element is enabled")]
        enabled: bool,
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
    /// Force-cancel a stuck recorder (identity-checked) and keep its frames
    Cancel,
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
    /// Is the sandbox alive: desktop, keeper, tracked pids
    Status,
    State {
        #[arg(long)]
        window: String,
        #[arg(long, default_value_t = 12)]
        depth: usize,
        #[arg(long)]
        no_shot: bool,
        #[arg(long)]
        out: Option<String>,
        #[arg(long, help = "diff against previous state of this sandbox window")]
        diff: bool,
        #[arg(long, help = "redact text values (Trae mask)")]
        redact: bool,
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
        #[arg(long, help = "deliver WM_CHAR to this element's field instead of the first edit-like child")]
        index: Option<usize>,
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
        #[arg(long, allow_hyphen_values = true)]
        x: Option<i32>,
        #[arg(long, allow_hyphen_values = true)]
        y: Option<i32>,
        #[arg(long)]
        window: Option<String>,
        #[arg(long, help = "state_id from the last sandbox observation (required for explicit coordinates)")]
        shot_id: Option<String>,
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
        #[arg(long, help = "message-level drag on this window instead of the one under the start point")]
        window: Option<String>,
        #[arg(long, help = "state_id from the last sandbox observation (required for coordinate drags)")]
        shot_id: Option<String>,
    },
    /// Message-level hover (WM_MOUSEMOVE) on a sandbox window; the pointer is global
    /// so the real cursor is never moved
    Hover {
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
    },
    /// Move/resize a sandbox window (SetWindowPos on the hidden desktop; no user impact)
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
    },
    Minimize {
        #[arg(long)]
        window: String,
    },
    Maximize {
        #[arg(long)]
        window: String,
    },
    Restore {
        #[arg(long)]
        window: String,
    },
    /// Wait until a sandbox window exists / disappears
    Wait {
        #[arg(long)]
        window: String,
        #[arg(long, help = "wait until the window no longer exists")]
        gone: bool,
        #[arg(long, default_value_t = 10000)]
        timeout_ms: u64,
        #[arg(long, default_value_t = 100)]
        interval_ms: u64,
    },
    /// Session clipboard (shared with the user desktop — same window station)
    Clipboard {
        #[command(subcommand)]
        op: ClipOp,
    },
    /// Dump a UIA Grid/Table inside the sandbox as rows x cols
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
    /// Force a virtualized UIA item in the sandbox to materialize
    Realize {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
    },
    /// Read Table/TableItem headers inside the sandbox
    Table {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long, default_value_t = 20)]
        max_headers: usize,
    },
    /// Find an item through ItemContainerPattern inside the sandbox
    #[command(name = "find-item")]
    FindItem {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long, default_value = "name")]
        property: String,
        #[arg(long)]
        value: String,
    },
    /// Local OCR over a sandbox window screenshot
    Ocr {
        #[arg(long)]
        window: String,
        #[arg(long, default_value_t = 200)]
        max_lines: usize,
    },
    /// Read a TextPattern range inside the sandbox
    Text {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long, default_value_t = 4000)]
        max_chars: usize,
    },
    /// Scroll an item into view inside the sandbox
    #[command(name = "scroll-item")]
    ScrollItem {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
        #[arg(long)]
        align_top: bool,
    },
    /// Report or apply UIA TransformPattern inside the sandbox (move/resize via the app)
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
    #[command(name = "select-add")]
    SelectAdd {
        #[arg(long)]
        window: String,
        #[arg(long)]
        index: usize,
    },
    #[command(name = "select-remove")]
    SelectRemove {
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
    // The clap value (one enum holding every subcommand) and the dispatch frame
    // are large enough to exhaust the default 1 MiB debug main-thread stack.
    // Run parsing and dispatch on a thread with an explicit, generous stack.
    let worker = std::thread::Builder::new()
        .name("cua-main".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let cli = Cli::parse();
            if let Some(id) = &cli.observation_id {
                util::set_observation_override(id);
            }
            dispatch(cli.cmd)
        })
        .expect("spawn cua-main");
    let code = worker.join().unwrap_or(1);
    std::process::exit(code);
}

fn dispatch(cmd: Cmd) -> i32 {
    let r: Result<i32> = (|| {
        match cmd {
            Cmd::Check => cmd_check(),
            Cmd::Apps { all } => cmd_apps(all),
            Cmd::State { window, depth, no_shot, out, diff, redact, patterns, raw } => {
                cmd_state(&window, depth, no_shot, out, diff, redact, patterns, raw)
            }
            Cmd::Shot { window, screen, out } => cmd_shot(window, screen, &out),
            Cmd::Grounding { window, query } => cmd_grounding(&window, &query),
            Cmd::ElementAt { x, y } => cmd_element_at(x, y),
            Cmd::Cursor => cmd_cursor(),
            Cmd::Ocr { window, screen, file, max_lines } => {
                cmd_ocr(window, screen, file, max_lines)
            }
            Cmd::Hover { window, index, x, y, shot_id, background, record } => {
                cmd_hover(window, index, x, y, shot_id, background, record)
            }
            Cmd::Click { window, index, x, y, shot_id, right, double, strategy, background, record, sandbox } => {
                cmd_click(window, index, x, y, shot_id, right, double, &strategy, background, record, sandbox)
            }
            Cmd::Type { text, window, index, background, record, sandbox } => {
                cmd_type(&text, window, index, background, record, sandbox)
            }
            Cmd::Key { keys, window, confirm, background, record, sandbox } => {
                cmd_key(&keys, window, confirm, background, record, sandbox)
            }
            Cmd::Scroll { dx, dy, x, y, window, shot_id, background, record, sandbox } => {
                cmd_scroll(dx, dy, x, y, window, shot_id, background, record, sandbox)
            }
            Cmd::Drag { x1, y1, x2, y2, shot_id, background, record, sandbox } => {
                cmd_drag(x1, y1, x2, y2, shot_id, background, record, sandbox)
            }
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
            Cmd::SelectAdd { window, index, record, sandbox } => {
                cmd_select_many(&window, index, record, sandbox, true)
            }
            Cmd::SelectRemove { window, index, record, sandbox } => {
                cmd_select_many(&window, index, record, sandbox, false)
            }
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
            Cmd::Table { window, index, max_headers, sandbox } => {
                cmd_table(&window, index, max_headers, sandbox)
            }
            Cmd::FindItem { window, index, property, value, sandbox } => {
                cmd_find_item(&window, index, &property, &value, sandbox)
            }
            Cmd::Text { window, index, max_chars, sandbox } => {
                cmd_text(&window, index, max_chars, sandbox)
            }
            Cmd::ScrollItem { window, index, align_top, record, sandbox } => {
                cmd_scroll_item(&window, index, align_top, record, sandbox)
            }
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
            Cmd::Wait {
                window,
                gone,
                foreground,
                index,
                value,
                toggle,
                selected,
                expanded,
                enabled,
                timeout_ms,
                interval_ms,
            } => cmd_wait(
                &window,
                gone,
                foreground,
                index.map(|i| WaitElement { index: i, value, toggle, selected, expanded, enabled }),
                timeout_ms,
                interval_ms,
            ),
            Cmd::Close { window, confirm, record } => cmd_window_op(&window, "close", record, confirm),
            Cmd::Batch { file, continue_on_error } => cmd_batch(&file, continue_on_error),
            Cmd::Replay { name, verify } => cmd_replay(&name, verify),
            Cmd::Sandbox { op } => sandbox::dispatch(op),
            Cmd::Browser { rest } => ubb::cmd_browser(&rest),
            Cmd::Rec { op } => match op {
                RecOp::Start { window, fps, out, max_seconds } => rec::cmd_start(window, fps, out, max_seconds),
                RecOp::Stop { out, keep_frames } => rec::cmd_stop(out, keep_frames),
                RecOp::Cancel => rec::cmd_cancel(),
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
    F: FnOnce(&Target) -> Result<(String, Option<String>)>,
{
    guarded_action_req(
        ActionRequest::new(cmd_name, argv, record, InputMode::Auto),
        target_of,
        pre,
        f,
    )
}

/// Typed variant: the input mode is an explicit part of the request instead of a
/// pair of booleans/strings interpreted differently by each command.
fn guarded_action_req<F>(
    req: ActionRequest,
    target_of: impl Fn() -> Result<Target>,
    pre: impl Fn(&SafetyConfig, &Target) -> Result<(), String>,
    f: F,
) -> Result<i32>
where
    F: FnOnce(&Target) -> Result<(String, Option<String>)>,
{
    let started = std::time::Instant::now();
    let cfg = util::load_config();
    let (target, outcome) = match target_of() {
        Err(e) => (Value::Null, ActionOutcome::failed(format!("{e}"))),
        Ok(t) => {
            let tj = target_json(&t);
            if let Err(reason) = pre(&cfg, &t) {
                (tj, ActionOutcome::refused(reason))
            } else {
                match f(&t) {
                    Ok((detail, sid)) => (tj, ActionOutcome::ok(detail, sid)),
                    Err(e) => (tj, ActionOutcome::failed(format!("{e:#}"))),
                }
            }
        }
    };
    let mut receipt = Receipt::new(&req.cmd);
    receipt.target = target;
    receipt.input_mode = Some(req.input_mode.as_str().to_string());
    outcome.apply(&mut receipt);
    receipt.elapsed_ms = Some(started.elapsed().as_millis());
    let sig_after = if receipt.status == "ok" {
        receipt
            .target
            .get("hwnd")
            .and_then(|h| h.as_i64())
            .and_then(|h| util::state_fingerprint(h as isize).ok())
    } else {
        None
    };
    let receipt = util::finish_action(&req.meta(), receipt, sig_after.as_deref());
    receipt.print();
    Ok(receipt.exit_code())
}

fn strip_record_args(args: &[String]) -> Vec<String> {
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
        if a.starts_with("--record=") {
            continue;
        }
        out.push(a.clone());
    }
    out
}

fn record_argv(record: &Option<String>) -> Vec<String> {
    if record.is_none() {
        return Vec::new();
    }
    // store argv without exe path and without the --record <name> pair so replay does not re-record
    strip_record_args(&std::env::args().skip(1).collect::<Vec<_>>())
}

/// Sandbox actions print their own receipt, so `guarded_action` never sees them.
/// Record the original argv here instead of silently dropping `--record`.
fn sandbox_recorded(code: i32, cmd: &str, record: &Option<String>) -> Result<i32> {
    if code == 0 && record.is_some() {
        let meta = util::ActionMeta::new(cmd, record_argv(record), record);
        let r = Receipt::new(cmd).ok(json!({"desktop": "cua-sandbox"}), "sandbox action");
        let r = util::finish_action(&meta, r, None);
        if r.evidence == "incomplete" {
            eprintln!("warning: {}", r.detail.clone().unwrap_or_default());
        }
    }
    Ok(code)
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
        "session": util::session(),
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

/// Multiset diff between the previous observation of a window and the current tree:
/// added/removed (type|name) counts plus value changes keyed on type+name+class+rect.
pub(crate) fn diff_trees(prev_elems: &[Value], nodes: &[uia::Node], redact: bool) -> Value {
    let mut cur_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for n in nodes.iter() {
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
    for n in prev_elems {
        if let Some(key) = node_key_json(n) {
            prev_values.insert(key, n["value"].as_str().unwrap_or("").to_string());
        }
    }
    let mut value_changes = 0usize;
    let mut examples_values: Vec<Value> = Vec::new();
    if !redact {
        for n in nodes {
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
        "added": added_n,
        "removed": removed_n,
        "value_changes": if redact { Value::Null } else { json!(value_changes) },
        "examples_added": added.iter().take(5).cloned().collect::<Vec<_>>(),
        "examples_removed": removed.iter().take(5).cloned().collect::<Vec<_>>(),
        "examples_value_changes": examples_values,
    })
}

fn cmd_state(
    sel: &str,
    depth: usize,
    no_shot: bool,
    out: Option<String>,
    diff: bool,
    redact: bool,
    patterns: bool,
    raw: bool,
) -> Result<i32> {
    let t = resolve_target(sel)?;
    // Walk unredacted: the element cache must keep real names for actions.
    // Redaction is applied to the output copy only.
    let opts = uia::WalkOptions {
        max_depth: depth,
        max_nodes: 2000,
        redact: false,
        with_patterns: patterns,
        view: if raw { uia::WalkView::Raw } else { uia::WalkView::Control },
    };
    let mut tree = uia::walk_window_opts(t.hwnd, opts)?;
    let mut warmup_retry = false;
    let root_class = tree.nodes.first().map(|n| n.class.clone()).unwrap_or_default();
    let chromium_shell = root_class.contains("Chrome_WidgetWin") && tree.nodes.len() < 40;
    if !raw && (tree.nodes.len() <= 3 || chromium_shell) {
        // Chromium/Electron only builds its UIA tree after the first client
        // query; the very first walk can come back as a small shell (even a
        // 14-node one). One warmup retry makes the first `cua state` on an
        // Electron app return the real tree.
        std::thread::sleep(std::time::Duration::from_millis(400));
        if let Ok(second) = uia::walk_window_opts(t.hwnd, opts) {
            if second.nodes.len() > tree.nodes.len() {
                tree = second;
                warmup_retry = true;
            }
        }
    }
    let out_nodes = if redact {
        uia::redacted_nodes(&tree.nodes)
    } else {
        tree.nodes.clone()
    };
    let state_id = util::new_state_id();
    let mut prev_elems: Vec<Value> = util::load_observation_nodes(t.info.hwnd)
        .and_then(|e| serde_json::from_value::<Vec<Value>>(e).ok())
        .unwrap_or_default();
    if redact {
        // The cached snapshot may come from an unredacted observation. Redact it
        // with the same policy before it feeds diff examples, otherwise
        // `--redact --diff` can print names the user asked to hide.
        for n in prev_elems.iter_mut() {
            if let Some(s) = n["name"].as_str() {
                n["name"] = json!(uia::redact_text(s));
            }
            if let Some(s) = n["value"].as_str() {
                n["value"] = json!(uia::redact_text(s));
            }
        }
    }

    // diff against previous state of the same window
    let diff_v = if diff {
        let prev = util::load_last_state();
        match prev {
            Some(p) if p["hwnd"].as_i64() == Some(t.info.hwnd as i64) => {
                let mut d = diff_trees(&prev_elems, &out_nodes, redact);
                d["changed"] = Value::Bool(p["sig"] != Value::String(tree.sig.clone()));
                d
            }
            _ => json!({"changed": null, "note": "no previous state for this window"}),
        }
    } else {
        Value::Null
    };

    // elements registry for index-based actions (real names); the cache keeps the
    // full relocation payload, the printed copy is lean
    let cache_value = json!(uia::nodes_json(&tree.nodes));
    util::save_elements(t.info.hwnd, &cache_value);
    let elems = uia::nodes_json_lean(&out_nodes);
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
                Some(json!({"path": path.to_string_lossy(), "width": s.width, "height": s.height, "rendered": s.rendered, "status": s.status}))
            }
            Err(e) => Some(json!({"error": format!("{e:#}")})),
        }
    };

    let obs_path = util::save_observation(
        &state_id,
        t.info.hwnd,
        &tree.sig,
        tree.truncated,
        &cache_value,
        shot_path.as_ref(),
    )?;
    util::save_last_observation(t.info.hwnd, &state_id);

    let v = json!({
        "state_id": state_id,
        "window": target_json(&t),
        "node_count": tree.nodes.len(),
        "truncated": tree.truncated,
        "errors": tree.errors,
        "last_error": tree.last_error,
        "view": if raw { "raw" } else { "control" },
        "warmup_retry": if warmup_retry { Value::Bool(true) } else { Value::Null },
        "sig": tree.sig,
        "nodes": elems,
        "screenshot": shot_path,
        "observation": obs_path.to_string_lossy(),
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
        "status": shot.status,
    });
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(0)
}

fn cmd_grounding(sel: &str, query: &str) -> Result<i32> {
    let t = resolve_target(sel)?;
    let g = uia::ground_tree(t.hwnd, query)?;
    // Publish the exact tree the indices came from as an immutable observation.
    // State/shot stay untouched, so coordinate guards keep their last visual id.
    let elems = uia::nodes_json(&g.tree.nodes);
    let cache_value = json!(elems.clone());
    util::save_elements(t.info.hwnd, &cache_value);
    let obs_id = util::new_state_id();
    let obs_path = util::save_observation(
        &obs_id,
        t.info.hwnd,
        &g.tree.sig,
        g.tree.truncated,
        &cache_value,
        None,
    )?;
    util::save_last_observation(t.info.hwnd, &obs_id);
    let items: Vec<Value> = g
        .hits
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
        "node_count": g.tree.nodes.len(),
        "truncated": g.tree.truncated,
        "elements_cached": true,
        "observation_id": obs_id,
        "observation": obs_path.to_string_lossy(),
        "note": "local UIA grounding; the returned indices belong to the immutable observation saved by this call (state/shot are unchanged)"
    });
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(0)
}

/// `cua element-at --x --y`: which UIA element is at this screen point?
/// Passive UIA only (ElementFromPoint refined to the innermost control across
/// control+raw views): never moves the pointer or touches focus, so it is safe
/// to run while a game owns the foreground.
fn cmd_element_at(x: i32, y: i32) -> Result<i32> {
    let hit = uia::element_at_point(x, y)?;
    // Map into the observation cache of the hit's own top-level window, so the
    // agent can go straight from "what is here?" to `click --index N`.
    let hwnd_isize = hit.root_hwnd;
    let rid_i64 = |rid: &[i32]| -> Vec<i64> { rid.iter().map(|v| *v as i64).collect() };
    let cache = (hwnd_isize != 0)
        .then(|| util::load_observation_nodes(hwnd_isize))
        .flatten();
    let find_index = |want: &[i32]| -> Option<usize> {
        cache.as_ref().and_then(|nodes| {
            nodes.as_array().and_then(|arr| {
                arr.iter().position(|n| {
                    n["rid"].as_array().map(|a| {
                        a.iter().filter_map(|v| v.as_i64()).collect::<Vec<i64>>() == rid_i64(want)
                    }).unwrap_or(false)
                })
            })
        })
    };
    // The refined hit is often an inner container of the observed element (e.g.
    // the icon label inside a TreeItem); fall back to the nearest cached ancestor.
    let exact = find_index(&hit.rid);
    let (index, via) = match exact {
        Some(i) => (Some(i), "exact-rid"),
        None => hit
            .ancestor_rids
            .iter()
            .find_map(|rid| find_index(rid).map(|i| (Some(i), "nearest-ancestor")))
            .unwrap_or((None, "unmapped")),
    };
    let v = json!({
        "schema": "cua-element-at-v1",
        "x": x,
        "y": y,
        "index": index,
        "via": via,
        "element": {
            "name": hit.name,
            "type": hit.ctype,
            "class": hit.class,
            "aid": hit.aid,
            "rect": hit.rect,
            "rid": hit.rid,
            "control": hit.control,
            "tab_item": hit.tab_item,
        },
        "center": [hit.rect[0] + hit.rect[2] / 2, hit.rect[1] + hit.rect[3] / 2],
        "observation_window": hwnd_isize,
        "ancestors": hit.ancestor_rids.len(),
        "note": "passive UIA probe (no pointer/focus change); index refers to the last state/grounding observation of the window that owns the point",
    });
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(0)
}

fn cmd_ocr(
    window: Option<String>,
    screen: bool,
    file: Option<String>,
    max_lines: usize,
) -> Result<i32> {
    let max_lines = max_lines.clamp(1, 1000);
    let (width, height, rgba, source, window_json) = if let Some(path) = file {
        let img = image::open(&path)?.to_rgba8();
        (
            img.width(),
            img.height(),
            img.into_raw(),
            json!({"file": path}),
            Value::Null,
        )
    } else if screen {
        let s = capture::shot_screen(true)?;
        (s.width, s.height, s.rgba, json!({"screen": true}), Value::Null)
    } else {
        let sel = window.ok_or_else(|| anyhow!("--window, --screen or --file required"))?;
        let t = resolve_target(&sel)?;
        let cfg = util::load_config();
        safety_gate(&cfg, &t).map_err(|r| anyhow!(r))?;
        let s = capture::shot_window(t.hwnd, false)?;
        if !s.rendered {
            bail!("capture produced no content (minimized/DRM window)");
        }
        (s.width, s.height, s.rgba, json!({"window": target_json(&t)}), target_json(&t))
    };
    let out = ocr::limit_lines(ocr::recognize(width, height, &rgba)?, max_lines);
    let mut out = out;
    out["schema"] = json!("cua-ocr-v1");
    out["source"] = source;
    out["window"] = window_json;
    out["image"] = json!({"width": width, "height": height});
    println!("{}", serde_json::to_string_pretty(&out)?);
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
    background: bool,
    record: Option<String>,
) -> Result<i32> {
    let argv = record_argv(&record);
    let mode = InputMode::from_background(background);
    guarded_action_req(
        ActionRequest::new("hover", argv, &record, mode),
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
            let hover_at = |cx: i32, cy: i32, note: &str| -> Result<(String, Option<String>)> {
                if mode.is_background() {
                    let root = win::root_window(t.hwnd);
                    input::post_hover(root, cx, cy)?;
                    return Ok((format!("{note}hovered ({cx},{cy}) via message-level input (background; pointer untouched)"), None));
                }
                win::ensure_point_hits(t.hwnd, cx, cy)?;
                input::move_to(cx, cy)?;
                Ok((format!("{note}hovered at ({cx},{cy})"), None))
            };
            if let Some(idx) = index {
                match target_for_index(t.info.hwnd, idx)? {
                    ElementTarget::Rid(rid) => {
                        let (_rect, (cx, cy), name) = uia::element_center(t.hwnd, &rid)?;
                        hover_at(cx, cy, &format!("over '{}': ", name))
                    }
                    ElementTarget::Rect { rect, ctype, name } => {
                        let (cx, cy, via) = locate_rect(t.hwnd, &rect, &ctype, &name)?;
                        hover_at(cx, cy, &format!("no runtime id; {via}, "))
                    }
                }
            } else {
                let (px, py) = (x.unwrap(), y.unwrap());
                hover_at(px, py, "")
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
    let mode = InputMode::parse(strategy, background)?;
    if sandbox {
        let code = sandbox::cmd_click(window, index, x, y, shot_id, right, double)?;
        return sandbox_recorded(code, "click", &record);
    }
    let argv = record_argv(&record);
    guarded_action_req(
        ActionRequest::new("click", argv, &record, mode),
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
                        let d = match mode {
                            InputMode::A11y | InputMode::BackgroundA11y => {
                                // strict: a missing pattern must not silently
                                // degrade to messages or the real pointer
                                uia::click_element_strict(t.hwnd, &rid, double, right)?
                            }
                            InputMode::Background => {
                                uia::click_element_background(t.hwnd, &rid, double, right)?
                            }
                            InputMode::Event => uia::click_element_event(t.hwnd, &rid, double, right)?,
                            InputMode::Auto => uia::click_element(t.hwnd, &rid, double, right)?,
                        };
                        Ok((d, Some(util::new_state_id())))
                    }
                    ElementTarget::Rect { rect, ctype, name } => {
                        if mode.is_a11y_only() {
                            bail!("element {idx} has no runtime id for a11y actions (strategy=a11y forbids the rectangle/message fallback) — re-observe or use strategy=auto");
                        }
                        let (cx, cy, via) = locate_rect(t.hwnd, &rect, &ctype, &name)?;
                        if mode.is_background() {
                            let root = win::root_window(t.hwnd);
                            input::post_click(root, cx, cy, right, double)?;
                            Ok((format!("no runtime id; {via}, clicked ({cx},{cy}) via message-level input"), Some(util::new_state_id())))
                        } else {
                            win::ensure_point_hits(t.hwnd, cx, cy)?;
                            input::click(cx, cy, right, double)?;
                            Ok((format!("no runtime id; {via}, clicked ({cx},{cy})"), Some(util::new_state_id())))
                        }
                    }
                }
            } else {
                if mode.is_a11y_only() {
                    bail!("strategy=a11y cannot click raw coordinates (there is no element pattern to invoke) — use --index or strategy=auto/event");
                }
                let (x, y) = (x.ok_or_else(|| anyhow!("--index or --x/--y required"))?, y.ok_or_else(|| anyhow!("--y required"))?);
                if mode.is_background() {
                    let root = win::root_window(t.hwnd);
                    input::post_click(root, x, y, right, double)?;
                    Ok((
                        format!("clicked ({x},{y}) via message-level input (background; focus untouched)"),
                        Some(util::new_state_id()),
                    ))
                } else {
                    win::ensure_point_hits(t.hwnd, x, y)?;
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
    if let Some(id) = util::observation_override() {
        // an explicit binding must fail loudly instead of falling back
        match util::load_observation_nodes(hwnd) {
            Some(cache) => return node_from_cache(&cache, idx),
            None => bail!(
                "observation '{id}' not found or not of this window (--observation-id binding is strict; drop the flag or re-observe)"
            ),
        }
    }
    let cache = util::load_observation_nodes(hwnd)
        .ok_or_else(|| anyhow!("no state for this window — run 'cua state --window ...' first"))?;
    node_from_cache(&cache, idx)
}

fn node_from_cache(cache: &serde_json::Value, idx: usize) -> Result<serde_json::Value> {
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
    index: Option<usize>,
    background: bool,
    record: Option<String>,
    sandbox: bool,
) -> Result<i32> {
    if sandbox {
        let code = sandbox::cmd_type(&text, window, index)?;
        return sandbox_recorded(code, "type", &record);
    }
    let argv = record_argv(&record);
    let text = text.to_string();
    if index.is_some() && !background {
        bail!("--index only applies to --background (foreground typing goes to the focused control)");
    }
    let mode = InputMode::from_background(background);
    guarded_action_req(
        ActionRequest::new("type", argv, &record, mode),
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
            if mode.is_background() {
                let root = win::root_window(t.hwnd);
                match index {
                    Some(idx) => {
                        // aim the WM_CHAR stream at this element's field
                        let (cx, cy) = match target_for_index(t.info.hwnd, idx)? {
                            ElementTarget::Rid(rid) => uia::element_center(t.hwnd, &rid)?.1,
                            ElementTarget::Rect { rect, ctype, name } => {
                                let (cx, cy, _via) = locate_rect(t.hwnd, &rect, &ctype, &name)?;
                                (cx, cy)
                            }
                        };
                        let n = input::post_type_at_point(root, cx, cy, &text)?;
                        return Ok((
                            format!("typed {n} units into element {idx} via message-level input (background; focus untouched)"),
                            None,
                        ));
                    }
                    None => {
                        let target = input::find_input_target(root);
                        let n = input::post_type(target, &text)?;
                        return Ok((
                            format!("typed {n} units via message-level input (background; focus untouched)"),
                            None,
                        ));
                    }
                }
            }
            if window.is_some() {
                require_foreground(t.hwnd)?;
            }
            if !win::is_foreground(t.hwnd) {
                bail!("focus moved away from the target before typing; nothing was typed — re-focus it or use --background");
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
        let code = sandbox::cmd_key(&keys, window, confirm)?;
        return sandbox_recorded(code, "key", &record);
    }
    let argv = record_argv(&record);
    let combo = input::parse_combo(keys)?;
    let keys = keys.to_string();
    let mode = InputMode::from_background(background);
    guarded_action_req(
        ActionRequest::new("key", argv, &record, mode),
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
            // Qoder Always-Confirm: alt+f4 matched on the parsed combo (order/spacing-proof)
            if input::is_alt_f4(&combo) && !confirm {
                return Err("refused: 'alt+f4' is Always-Confirm level — pass --confirm".into());
            }
            Ok(())
        },
        |t| {
            if mode.is_background() {
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
            if !win::is_foreground(t.hwnd) {
                bail!("focus moved away from the target before sending keys; nothing was typed — re-focus it or use --background");
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
    background: bool,
    record: Option<String>,
    sandbox: bool,
) -> Result<i32> {
    if sandbox {
        let code = sandbox::cmd_scroll(dx, dy, x, y, window, shot_id)?;
        return sandbox_recorded(code, "scroll", &record);
    }
    let argv = record_argv(&record);
    let mode = InputMode::from_background(background);
    guarded_action_req(
        ActionRequest::new("scroll", argv, &record, mode),
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
            if mode.is_background() {
                let root = win::root_window(t.hwnd);
                input::post_scroll(root, px, py, dx, dy)?;
                return Ok((
                    format!("scrolled dx={dx} dy={dy} at ({px},{py}) via message-level input (background; focus untouched)"),
                    None,
                ));
            }
            // best effort: wheel messages follow the pointer on Win10+, so raising is
            // only needed for "scroll inactive windows = off"; never fail the scroll on it
            if window.is_some() {
                let _ = win::activate_window(t.hwnd);
            }
            win::ensure_point_hits(t.hwnd, px, py)?;
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

fn cmd_drag(x1: i32, y1: i32, x2: i32, y2: i32, shot_id: Option<String>, background: bool, record: Option<String>, sandbox: bool) -> Result<i32> {
    if sandbox {
        let code = sandbox::cmd_drag(x1, y1, x2, y2, None, shot_id)?;
        return sandbox_recorded(code, "drag", &record);
    }
    let argv = record_argv(&record);
    let mode = InputMode::from_background(background);
    guarded_action_req(
        ActionRequest::new("drag", argv, &record, mode),
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
        |t| {
            if mode.is_background() {
                let root = win::root_window(t.hwnd);
                input::post_drag(root, x1, y1, x2, y2)?;
                return Ok((
                    format!("dragged ({x1},{y1})->({x2},{y2}) via message-level input (background; cursor untouched)"),
                    Some(util::new_state_id()),
                ));
            }
            input::drag(x1, y1, x2, y2)?;
            Ok((format!("dragged ({x1},{y1})->({x2},{y2})"), Some(util::new_state_id())))
        },
    )
}

fn cmd_set_value(sel: &str, index: usize, value: &str, record: Option<String>, sandbox: bool) -> Result<i32> {
    if sandbox {
        let code = sandbox::cmd_set_value(sel, index, &value)?;
        return sandbox_recorded(code, "set-value", &record);
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
        let code = sandbox::cmd_toggle(sel, index)?;
        return sandbox_recorded(code, "toggle", &record);
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
        let code = sandbox::cmd_select(sel, index)?;
        return sandbox_recorded(code, "select", &record);
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

fn cmd_select_many(
    sel: &str,
    index: usize,
    record: Option<String>,
    sandbox: bool,
    add: bool,
) -> Result<i32> {
    let cmd = if add { "select-add" } else { "select-remove" };
    if sandbox {
        let code = sandbox::cmd_select_many(sel, index, add)?;
        return sandbox_recorded(code, cmd, &record);
    }
    let argv = record_argv(&record);
    guarded_action(
        cmd,
        argv,
        &record,
        || resolve_target(sel),
        |cfg, t| safety_gate(cfg, t),
        |t| {
            let rid = rid_for_index(t.info.hwnd, index)?;
            let d = if add {
                uia::select_add_element(t.hwnd, &rid)?
            } else {
                uia::select_remove_element(t.hwnd, &rid)?
            };
            Ok((d, Some(util::new_state_id())))
        },
    )
}

fn cmd_expand(sel: &str, index: usize, record: Option<String>, sandbox: bool) -> Result<i32> {
    if sandbox {
        let code = sandbox::cmd_expand(sel, index)?;
        return sandbox_recorded(code, "expand", &record);
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
        let code = sandbox::cmd_collapse(sel, index)?;
        return sandbox_recorded(code, "collapse", &record);
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
        let code = sandbox::cmd_set_range(sel, index, value)?;
        return sandbox_recorded(code, "set-range", &record);
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

pub(crate) fn parse_pair(spec: &str) -> Result<(f64, f64)> {
    let mut it = spec.split([',', 'x', ' ']).filter(|p| !p.is_empty());
    let a: f64 = it.next().ok_or_else(|| anyhow!("expected \"a,b\""))?.parse()?;
    let b: f64 = it.next().ok_or_else(|| anyhow!("expected \"a,b\""))?.parse()?;
    Ok((a, b))
}

fn cmd_grid(sel: &str, index: usize, row: Option<i32>, col: Option<i32>, invoke: bool, max_cells: usize) -> Result<i32> {
    let t = resolve_target(sel)?;
    let cfg = util::load_config();
    if let Err(reason) = safety_gate(&cfg, &t) {
        let r = Receipt::new("grid").refused(target_json(&t), &reason);
        r.print();
        return Ok(r.exit_code());
    }
    let rid = rid_for_index(t.info.hwnd, index)?;
    let name = t
        .info
        .title
        .clone();
    let out = match (row, col) {
        (Some(r), Some(c)) => uia::grid_cell(t.hwnd, &rid, r, c, invoke, false)?,
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
        |cfg, t| safety_gate(cfg, t),
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
    let cfg = util::load_config();
    if let Err(reason) = safety_gate(&cfg, &t) {
        let r = Receipt::new("realize").refused(target_json(&t), &reason);
        r.print();
        return Ok(r.exit_code());
    }
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

fn cmd_table(sel: &str, index: usize, max_headers: usize, sandbox: bool) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_table(sel, index, max_headers);
    }
    let t = resolve_target(sel)?;
    let rid = rid_for_index(t.info.hwnd, index)?;
    let table = uia::table_element(t.hwnd, &rid, max_headers)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-table-v1",
            "window": target_json(&t),
            "index": index,
            "table": table,
        }))?
    );
    Ok(0)
}

fn cmd_find_item(
    sel: &str,
    index: usize,
    property: &str,
    value: &str,
    sandbox: bool,
) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_find_item(sel, index, property, value);
    }
    let t = resolve_target(sel)?;
    let rid = rid_for_index(t.info.hwnd, index)?;
    let item = uia::find_item_element(t.hwnd, &rid, property, value)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-item-v1",
            "window": target_json(&t),
            "index": index,
            "property": property,
            "value": value,
            "found": true,
            "element": item,
        }))?
    );
    Ok(0)
}

fn cmd_text(sel: &str, index: usize, max_chars: usize, sandbox: bool) -> Result<i32> {
    if sandbox {
        return sandbox::cmd_text(sel, index, max_chars);
    }
    let t = resolve_target(sel)?;
    let rid = rid_for_index(t.info.hwnd, index)?;
    let (text, truncated) = uia::text_element(t.hwnd, &rid, max_chars)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-text-v1",
            "window": target_json(&t),
            "index": index,
            "chars": text.chars().count(),
            "truncated": truncated,
            "text": text,
        }))?
    );
    Ok(0)
}

fn cmd_scroll_item(
    sel: &str,
    index: usize,
    align_top: bool,
    record: Option<String>,
    sandbox: bool,
) -> Result<i32> {
    if sandbox {
        let code = sandbox::cmd_scroll_item(sel, index, align_top)?;
        return sandbox_recorded(code, "scroll-item", &record);
    }
    let argv = record_argv(&record);
    guarded_action(
        "scroll-item",
        argv,
        &record,
        || resolve_target(sel),
        |cfg, t| safety_gate(cfg, t),
        |t| {
            let rid = rid_for_index(t.info.hwnd, index)?;
            let d = uia::scroll_item_element(t.hwnd, &rid, align_top)?;
            Ok((d, None))
        },
    )
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
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::System::Ole::CF_UNICODETEXT;
    unsafe {
        OpenClipboard(None).map_err(|e| anyhow!("OpenClipboard: {e}"))?;
        let result = (|| -> Result<String> {
            let h = GetClipboardData(CF_UNICODETEXT.0 as u32).map_err(|e| anyhow!("GetClipboardData: {e}"))?;
            let hg = windows::Win32::Foundation::HGLOBAL(h.0);
            // Bound the UTF-16 scan by the real allocation size before touching
            // the pointer: the byte count is authoritative, not a 1 MiB guess.
            let bytes = GlobalSize(hg) as usize;
            if bytes < 2 {
                bail!("clipboard object too small ({bytes} bytes)");
            }
            let ptr = GlobalLock(hg) as *const u16;
            if ptr.is_null() {
                bail!("GlobalLock failed");
            }
            let max_units = bytes / 2;
            let mut len = 0usize;
            while len < max_units && *ptr.add(len) != 0 {
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
                // ownership never left us: free instead of leaking
                let _ = windows::Win32::Foundation::GlobalFree(h);
                bail!("GlobalLock failed");
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(h);
            SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(h.0))
                .map_err(|e| {
                    // SetClipboardData failed: the system did not take ownership,
                    // so the allocation must be returned or it leaks
                    let _ = windows::Win32::Foundation::GlobalFree(h);
                    anyhow!("SetClipboardData: {e}")
                })?;
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
        let code = sandbox::cmd_launch(exe, args)?;
        return sandbox_recorded(code, "launch", &record);
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

struct WaitElement {
    index: usize,
    value: Option<String>,
    toggle: Option<String>,
    selected: Option<bool>,
    expanded: Option<String>,
    enabled: bool,
}

#[allow(clippy::too_many_arguments)]
fn wait_report(
    sel: &str,
    gone: bool,
    foreground: bool,
    element: &Option<WaitElement>,
    satisfied: bool,
    elapsed: u128,
    last: Value,
    error: Option<String>,
) -> Result<i32> {
    let found = if element.is_none() { last.clone() } else { Value::Null };
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "cua-wait-v1",
            "window": sel,
            "gone": gone,
            "foreground": foreground,
            "element": element.as_ref().map(|e| json!({"index": e.index})),
            "satisfied": satisfied,
            "elapsed_ms": elapsed,
            "found": found,
            "last": last,
            "error": error,
        }))?
    );
    Ok(if satisfied { 0 } else { 1 })
}

fn cmd_wait(
    sel: &str,
    gone: bool,
    foreground: bool,
    element: Option<WaitElement>,
    timeout_ms: u64,
    interval_ms: u64,
) -> Result<i32> {
    let started = std::time::Instant::now();
    let interval = std::time::Duration::from_millis(interval_ms.max(20));
    let timeout = std::time::Duration::from_millis(timeout_ms.max(50));

    if let Some(el) = &element {
        if gone || foreground {
            bail!("--index cannot be combined with --gone/--foreground");
        }
        let has_cond = el.value.is_some()
            || el.toggle.is_some()
            || el.selected.is_some()
            || el.expanded.is_some()
            || el.enabled;
        if !has_cond {
            bail!("--index requires at least one of --value/--toggle/--selected/--expanded/--enabled");
        }
        if let Some(t) = &el.toggle {
            let t = t.to_ascii_lowercase();
            if !["on", "off", "indeterminate"].contains(&t.as_str()) {
                bail!("--toggle expects on|off|indeterminate");
            }
        }
        if let Some(x) = &el.expanded {
            let x = x.to_ascii_lowercase();
            if !["expanded", "collapsed"].contains(&x.as_str()) {
                bail!("--expanded expects expanded|collapsed");
            }
        }
    }

    // Resolve the element once: the cache is an observation snapshot, so polling
    // a missing index would never start working on its own.
    let mut target: Option<(isize, Vec<i32>)> = None;
    if let Some(el) = &element {
        let t = resolve_target(sel)?;
        match target_for_index(t.info.hwnd, el.index)? {
            ElementTarget::Rid(rid) => target = Some((t.info.hwnd, rid)),
            ElementTarget::Rect { .. } => {
                let elapsed = started.elapsed().as_millis();
                return wait_report(
                    sel,
                    gone,
                    foreground,
                    &element,
                    false,
                    elapsed,
                    Value::Null,
                    Some(format!(
                        "element {} has no runtime id; wait needs a stable a11y element — re-observe or use a window condition",
                        el.index
                    )),
                );
            }
        }
    }

    let mut last;
    loop {
        let satisfied;
        if let Some(el) = &element {
            let (hwnd_i, rid) = target.as_ref().unwrap();
            let hwnd = windows::Win32::Foundation::HWND(*hwnd_i as *mut _);
            let mut ok = true;
            let mut readings = serde_json::Map::new();
            let read_err = |e: anyhow::Error| format!("{e:#}");

            if let Some(want) = &el.value {
                match uia::value_any_element(hwnd, rid) {
                    Ok(cur) => {
                        readings.insert("value".into(), json!(cur));
                        if &cur != want {
                            ok = false;
                        }
                    }
                    Err(e) => {
                        readings.insert("value_error".into(), json!(read_err(e)));
                        ok = false;
                    }
                }
            }
            if let Some(want) = &el.toggle {
                match uia::toggle_of_element(hwnd, rid) {
                    Ok(cur) => {
                        readings.insert("toggle".into(), json!(cur));
                        if !cur.eq_ignore_ascii_case(want) {
                            ok = false;
                        }
                    }
                    Err(e) => {
                        readings.insert("toggle_error".into(), json!(read_err(e)));
                        ok = false;
                    }
                }
            }
            if let Some(want) = el.selected {
                match uia::selected_of_element(hwnd, rid) {
                    Ok(cur) => {
                        readings.insert("selected".into(), json!(cur));
                        if cur != want {
                            ok = false;
                        }
                    }
                    Err(e) => {
                        readings.insert("selected_error".into(), json!(read_err(e)));
                        ok = false;
                    }
                }
            }
            if let Some(want) = &el.expanded {
                match uia::expanded_of_element(hwnd, rid) {
                    Ok(cur) => {
                        readings.insert("expanded".into(), json!(cur));
                        if !cur.eq_ignore_ascii_case(want) {
                            ok = false;
                        }
                    }
                    Err(e) => {
                        readings.insert("expanded_error".into(), json!(read_err(e)));
                        ok = false;
                    }
                }
            }
            if el.enabled {
                match uia::enabled_of_element(hwnd, rid) {
                    Ok(cur) => {
                        readings.insert("enabled".into(), json!(cur));
                        if !cur {
                            ok = false;
                        }
                    }
                    Err(e) => {
                        readings.insert("enabled_error".into(), json!(read_err(e)));
                        ok = false;
                    }
                }
            }
            satisfied = ok;
            last = Value::Object(readings);
        } else {
            let found = win::resolve_window(sel).ok().and_then(win::snapshot_window);
            satisfied = if gone {
                found.is_none()
            } else {
                match &found {
                    Some(w) => !foreground || w.foreground,
                    None => false,
                }
            };
            last = found
                .map(|w| serde_json::to_value(w).unwrap_or(Value::Null))
                .unwrap_or(Value::Null);
        }

        if satisfied {
            let elapsed = started.elapsed().as_millis();
            return wait_report(sel, gone, foreground, &element, true, elapsed, last, None);
        }
        if started.elapsed() >= timeout {
            let elapsed = started.elapsed().as_millis();
            return wait_report(sel, gone, foreground, &element, false, elapsed, last, None);
        }
        std::thread::sleep(interval);
    }
}

use windows::Win32::Foundation::{LPARAM, WPARAM};

struct BatchStep {
    argv: Vec<String>,
    capture: Option<String>,
}

fn valid_var_name(s: &str) -> bool {
    let mut it = s.chars();
    match it.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    s.len() <= 64 && it.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn parse_steps(v: &Value) -> Result<Vec<BatchStep>> {
    let arr = v
        .as_array()
        .or_else(|| v["steps"].as_array())
        .ok_or_else(|| anyhow!("batch file must be a JSON array of argv arrays or {{\"steps\": [...]}}"))?;
    arr.iter()
        .map(|item| {
            if let Some(a) = item.as_array() {
                let argv = a
                    .iter()
                    .map(|x| x.as_str().map(|s| s.to_string()).ok_or_else(|| anyhow!("argv entries must be strings")))
                    .collect::<Result<Vec<_>>>()?;
                return Ok(BatchStep { argv, capture: None });
            }
            if let Some(o) = item.as_object() {
                let a = o
                    .get("argv")
                    .or_else(|| o.get("args"))
                    .and_then(|x| x.as_array())
                    .ok_or_else(|| anyhow!("step object needs an argv array"))?;
                let argv = a
                    .iter()
                    .map(|x| x.as_str().map(|s| s.to_string()).ok_or_else(|| anyhow!("argv entries must be strings")))
                    .collect::<Result<Vec<_>>>()?;
                let capture = o
                    .get("as")
                    .or_else(|| o.get("capture"))
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                if let Some(name) = &capture {
                    if !valid_var_name(name) {
                        bail!("batch variable name '{name}' must match [A-Za-z_][A-Za-z0-9_]{{0,63}}");
                    }
                }
                return Ok(BatchStep { argv, capture });
            }
            bail!("step must be an argv array or {{\"argv\": [...], \"as\": \"name\"}}");
        })
        .collect()
}

fn batch_var<'a>(vars: &'a std::collections::HashMap<String, Value>, expr: &str) -> Result<&'a Value> {
    let (name, path) = match expr.split_once('.') {
        Some((n, p)) => (n, p),
        None => (expr, ""),
    };
    let mut cur = vars
        .get(name)
        .ok_or_else(|| anyhow!("batch variable '{name}' is not bound yet"))?;
    if path.is_empty() {
        return Ok(cur);
    }
    for seg in path.split('.') {
        if seg.is_empty() {
            bail!("bad batch path '{expr}'");
        }
        let (field, index) = if let Some(open) = seg.find('[') {
            if !seg.ends_with(']') {
                bail!("bad array index in batch path '{expr}'");
            }
            let idx: usize = seg[open + 1..seg.len() - 1]
                .parse()
                .map_err(|_| anyhow!("bad array index in batch path '{expr}'"))?;
            (&seg[..open], Some(idx))
        } else {
            (seg, None)
        };
        if !field.is_empty() {
            cur = cur
                .get(field)
                .ok_or_else(|| anyhow!("batch path '{expr}' has no field '{field}'"))?;
        }
        if let Some(i) = index {
            cur = cur
                .get(i)
                .ok_or_else(|| anyhow!("batch path '{expr}' index {i} is out of range"))?;
        }
    }
    Ok(cur)
}

fn scalar_text(v: &Value, expr: &str) -> Result<String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Null => bail!("batch path '{expr}' is null"),
        _ => bail!("batch path '{expr}' is not a scalar"),
    }
}

/// Expand `${var}` / `${var.path[0].field}` from captured step results. No
/// evaluation, loops, or shell: only field/array lookup and scalar stringification.
fn expand_arg(s: &str, vars: &std::collections::HashMap<String, Value>) -> Result<String> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| anyhow!("unterminated ${{...}} in '{s}'"))?;
        let expr = &after[..end];
        out.push_str(&scalar_text(batch_var(vars, expr)?, expr)?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn cmd_batch(file: &str, continue_on_error: bool) -> Result<i32> {
    let raw = std::fs::read_to_string(file)?.trim_start_matches('\u{feff}').to_string();
    let v: Value = serde_json::from_str(&raw)?;
    let steps = parse_steps(&v)?;
    let mut vars: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    let mut results = Vec::new();
    let mut codes = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        let argv: Vec<String> = match step.argv.iter().map(|a| expand_arg(a, &vars)).collect::<Result<Vec<_>>>() {
            Ok(a) => a,
            Err(e) => {
                codes.push(1);
                results.push(json!({"step": i, "argv": step.argv, "exit": 1, "error": format!("{e:#}")}));
                if !continue_on_error {
                    break;
                }
                continue;
            }
        };
        eprintln!("[batch {}/{}] {:?}", i + 1, steps.len(), argv);
        let (code, parsed) = run_argv(&argv)?;
        let mut entry = json!({"step": i, "argv": argv, "exit": code});
        if let Some(name) = &step.capture {
            if code == 0 {
                match parsed {
                    Some(value) => {
                        vars.insert(name.clone(), value);
                        entry["captured"] = json!(name);
                    }
                    None => {
                        entry["error"] = json!("step produced no JSON result to capture");
                        codes.push(1);
                        results.push(entry);
                        if !continue_on_error {
                            break;
                        }
                        continue;
                    }
                }
            } else {
                entry["error"] = json!("step failed; variable was not bound");
            }
        }
        codes.push(code);
        results.push(entry);
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
            "variables": vars.keys().collect::<Vec<_>>(),
            "ok": ok,
        }))?
    );
    Ok(if ok { 0 } else { 1 })
}

fn run_argv(argv: &[String]) -> Result<(i32, Option<Value>)> {
    let exe = std::env::current_exe()?;
    let out = std::process::Command::new(exe).args(argv).output()?;
    // do NOT forward child stdout (it is a separate JSON doc); echo to stderr for humans
    let parsed = serde_json::from_slice::<Value>(&out.stdout).ok();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !stdout.trim().is_empty() {
        eprintln!("[step] {}", stdout.trim_end());
    }
    if !stderr.trim().is_empty() {
        eprintln!("{}", stderr.trim_end());
    }
    Ok((out.status.code().unwrap_or(1), parsed))
}

fn cmd_replay(name: &str, verify: bool) -> Result<i32> {
    let path = util::ensure_data_dir().join("recordings").join(format!("{}.jsonl", util::sanitize(name)));
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| anyhow!("recording '{name}' not found at {}", path.display()))?;
    let mut results = Vec::new();
    let mut all_ok = true;
    let mut all_verified = true;
    let mut verified_steps = 0usize;
    let mut unverified_steps = 0usize;
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
                    if verified == Some(true) {
                        verified_steps += 1;
                    } else {
                        // Missing fingerprint or unresolved target is not evidence
                        // of success; count it as unverified rather than skipping.
                        unverified_steps += 1;
                        all_verified = false;
                    }
                }
                None => {
                    verified = None;
                    unverified_steps += 1;
                    all_verified = false;
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
            "verified_steps": verified_steps,
            "unverified_steps": unverified_steps,
            "coverage": if verify {
                format!("{verified_steps}/{}", verified_steps + unverified_steps)
            } else {
                String::new()
            },
        }))?
    );
    Ok(if all_ok && (!verify || all_verified) { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pair_accepts_the_documented_separators() {
        assert_eq!(parse_pair("10,20").unwrap(), (10.0, 20.0));
        assert_eq!(parse_pair("10x20").unwrap(), (10.0, 20.0));
        assert_eq!(parse_pair("-1.5 2.5").unwrap(), (-1.5, 2.5));
        assert!(parse_pair("10").is_err());
        assert!(parse_pair("a,b").is_err());
    }


    #[test]
    fn strip_record_args_removes_both_record_forms() {
        let args: Vec<String> = [
            "click", "--record", "trace", "--index", "3", "--record=other", "--window", "x",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            strip_record_args(&args),
            vec!["click", "--index", "3", "--window", "x"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
    }
}

#[cfg(test)]
mod batch_tests {
    use super::*;
    use std::collections::HashMap;

    fn vars() -> HashMap<String, Value> {
        let mut m = HashMap::new();
        m.insert(
            "st".to_string(),
            serde_json::json!({
                "nodes": [
                    {"i": 0, "name": "root"},
                    {"i": 1, "name": "edit"}
                ],
                "state_id": "s-1",
                "ok": true
            }),
        );
        m
    }

    #[test]
    fn placeholders_resolve_fields_and_arrays() {
        let m = vars();
        assert_eq!(expand_arg("${st.nodes[1].i}", &m).unwrap(), "1");
        assert_eq!(expand_arg("id=${st.state_id}", &m).unwrap(), "id=s-1");
        assert_eq!(expand_arg("${st.ok}", &m).unwrap(), "true");
        assert_eq!(expand_arg("no-placeholder", &m).unwrap(), "no-placeholder");
    }

    #[test]
    fn placeholders_reject_unknown_paths_and_objects() {
        let m = vars();
        assert!(expand_arg("${missing}", &m).is_err());
        assert!(expand_arg("${st.nodes[9].i}", &m).is_err());
        assert!(expand_arg("${st.nodes}", &m).is_err());
        assert!(expand_arg("${st.state_id", &m).is_err());
    }

    #[test]
    fn batch_variable_names_are_restricted() {
        assert!(valid_var_name("st"));
        assert!(valid_var_name("_step1"));
        assert!(!valid_var_name("1st"));
        assert!(!valid_var_name("a.b"));
        assert!(!valid_var_name("a b"));
    }

    #[test]
    fn step_objects_and_legacy_arrays_parse() {
        let v = serde_json::json!({
            "steps": [
                ["state", "--window", "App"],
                {"argv": ["click", "--index", "${st.nodes[1].i}"], "as": "st2"}
            ]
        });
        let steps = parse_steps(&v).unwrap();
        assert_eq!(steps.len(), 2);
        assert!(steps[0].capture.is_none());
        assert_eq!(steps[1].capture.as_deref(), Some("st2"));
        assert_eq!(steps[1].argv[2], "${st.nodes[1].i}");
    }
}
