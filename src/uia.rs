// uia.rs — UI Automation tree + patterns (Qoder get_window_state / Trae get_app_state / ZCode getAXState)
// Element-index actions follow the a11y-first rule: index invoke > ValuePattern > coordinate fallback.
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::sync::OnceLock;
use uiautomation::controls::ControlType;
use uiautomation::patterns::{
    UIExpandCollapsePattern, UIGridPattern, UIInvokePattern, UILegacyIAccessiblePattern,
    UIItemContainerPattern, UIRangeValuePattern, UIScrollItemPattern, UISelectionItemPattern,
    UITableItemPattern, UITablePattern, UITextPattern, UITogglePattern, UITransformPattern,
    UIValuePattern, UIVirtualizedItemPattern,
};
use uiautomation::types::{ExpandCollapseState, Handle, Rect, ToggleState, TreeScope, UIProperty};
use uiautomation::variants::Variant;
use uiautomation::{UIAutomation, UIElement};

use windows::Win32::Foundation::HWND;

#[derive(Serialize, Clone, Debug)]
pub struct Node {
    pub i: usize,
    #[serde(rename = "type")]
    pub ctype: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub class: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub aid: String,
    pub rect: [i32; 4],
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub disabled: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub offscreen: bool,
    /// false only for raw-view entries the control view filters out
    /// (IsControlElement=false decorative/bridge nodes)
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub ctl: bool,
    pub rid: Vec<i32>,
    /// parent node index + 1-based position among its control-view siblings —
    /// the relocation payload kept in the element cache (stripped from output)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pa: Option<usize>,
    #[serde(skip_serializing_if = "is_zero_u32", default)]
    pub ri: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toggle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expanded: Option<String>,
    /// [value, minimum, maximum]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub patterns: Vec<String>,
}

pub struct Tree {
    pub nodes: Vec<Node>,
    pub sig: String,
    /// the walk hit its depth or node budget before the tree was exhausted
    pub truncated: bool,
    /// walker steps that failed even after one retry (dropped subtrees);
    /// any nonzero count also forces `truncated`
    pub errors: usize,
    /// the last walker error, for diagnosing why a tree came back partial
    pub last_error: Option<String>,
}

fn rect_of(r: &Rect) -> [i32; 4] {
    [r.get_left(), r.get_top(), r.get_width(), r.get_height()]
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

fn type_name(t: ControlType) -> String {
    format!("{t:?}")
}

/// One COM init + one IUIAutomation for the whole process.
///
/// `UIAutomation::new()` calls CoInitializeEx internally and nothing ever calls
/// CoUninitialize, so per-call construction (the old design) leaked apartment
/// references on every action — unbounded under wait/batch polling. Initializing
/// once for the process lifetime and cloning (AddRef) the cached instance is the
/// standard MTA client pattern; the single intentionally-leaked apartment
/// reference dies with the process.
static AUTOMATION: OnceLock<std::sync::Mutex<Option<SendAutomation>>> = OnceLock::new();

/// IUIAutomation obtained on an MTA thread is callable from any MTA thread of
/// this process (COM marshals through the apartment); this CLI runs its UIA work
/// on one thread anyway. The wrapper exists only so the process-wide instance
/// can live in a static.
struct SendAutomation(UIAutomation);
unsafe impl Send for SendAutomation {}
unsafe impl Sync for SendAutomation {}

pub fn automation() -> Result<UIAutomation> {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    static COM_ONCE: OnceLock<()> = OnceLock::new();
    COM_ONCE.get_or_init(|| unsafe {
        // S_FALSE / RPC_E_CHANGED_MODE are fine: the thread already has a
        // compatible apartment, and UIAutomation::new() tolerates both.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    });
    let cell = AUTOMATION.get_or_init(|| std::sync::Mutex::new(None));
    let mut guard = cell.lock().map_err(|_| anyhow!("uia automation mutex poisoned"))?;
    if guard.is_none() {
        let a = UIAutomation::new().map_err(|e| anyhow!("UIAutomation init: {e}"))?;
        *guard = Some(SendAutomation(a));
    }
    Ok(guard.as_ref().expect("set above").0.clone())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkView {
    /// the default tree agents act on
    Control,
    /// every element the provider exposes, including IsControlElement=false
    /// nodes (decorative panes, MSAA-bridged legacy content) the control view hides
    Raw,
}

#[derive(Clone, Copy, Debug)]
pub struct WalkOptions {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub redact: bool,
    pub with_patterns: bool,
    pub view: WalkView,
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self { max_depth: 12, max_nodes: 2000, redact: false, with_patterns: false, view: WalkView::Control }
    }
}

pub fn walk_window(
    hwnd: HWND,
    max_depth: usize,
    max_nodes: usize,
    redact: bool,
    with_patterns: bool,
) -> Result<Tree> {
    walk_window_opts(hwnd, WalkOptions { max_depth, max_nodes, redact, with_patterns, view: WalkView::Control })
}

pub fn walk_window_opts(hwnd: HWND, opts: WalkOptions) -> Result<Tree> {
    let automation = automation()?;
    let root = window_root(&automation, hwnd)?;
    let walker = match opts.view {
        WalkView::Control => automation.get_control_view_walker(),
        WalkView::Raw => automation.get_raw_view_walker(),
    }
    .map_err(|e| anyhow!("walker: {e}"))?;

    // Batch the core property reads into the navigation call itself (§ perf):
    // one build-cache navigation per element replaces ~8 separate cross-process
    // property calls. Cached reads that fail fall back to the live getter, and a
    // provider that refuses caching entirely falls back to the live path.
    // CUA_UIA_NO_CACHE=1 opts out (diagnosing providers with bad cache data).
    let cache_request = if std::env::var("CUA_UIA_NO_CACHE").map(|v| v == "1").unwrap_or(false) {
        None
    } else {
        automation.create_cache_request().ok().and_then(|cr| {
            for p in [
                UIProperty::Name,
                UIProperty::ClassName,
                UIProperty::AutomationId,
                UIProperty::ControlType,
                UIProperty::BoundingRectangle,
                UIProperty::IsEnabled,
                UIProperty::IsOffscreen,
                UIProperty::IsControlElement,
                UIProperty::RuntimeId,
            ] {
                if cr.add_property(p).is_err() {
                    return None;
                }
            }
            Some(cr)
        })
    };

    let mut nodes: Vec<Node> = Vec::new();
    let mut sig_parts: Vec<String> = Vec::new();
    let mut truncated = false;
    let mut errors = 0usize;
    let mut last_error: Option<String> = None;

    // BFS with entries (element, depth, parent node index, 1-based sibling index)
    let mut queue: std::collections::VecDeque<(UIElement, usize, Option<usize>, u32, bool)> =
        std::collections::VecDeque::new();
    // the root came from a non-cached handle lookup
    queue.push_back((root, 0, None, 0, false));
    while let Some((el, depth, pa, ri, cached_el)) = queue.pop_front() {
        if nodes.len() >= opts.max_nodes || depth > opts.max_depth {
            // This element was already popped and will never be processed, so the
            // walk is incomplete even when no siblings remain queued behind it.
            truncated = true;
            break;
        }
        let props = read_props(&el, cached_el && cache_request.is_some());
        let ctype = props.ctype;
        let mut name = props.name;
        let class = props.class;
        let aid = props.aid;
        let rect = props.rect;
        let rid = props.rid;
        let disabled = props.disabled;
        let offscreen = props.offscreen;
        let is_ctl = match opts.view {
            WalkView::Control => true,
            WalkView::Raw => props.is_ctl,
        };

        // value for editable-ish controls (Trae get_value semantics)
        let mut value = None;
        if matches!(ctype.as_str(), "Edit" | "Document" | "ComboBox" | "Spinner" | "ListItem") {
            if let Ok(vp) = el.get_pattern::<UIValuePattern>() {
                if let Ok(v) = vp.get_value() {
                    if !v.is_empty() {
                        value = Some(if opts.redact { redact_text(&v) } else { v });
                    }
                }
            }
        }
        if opts.redact {
            name = redact_text(&name);
        }

        // actionable state (cheap single-pattern probes; makes state/--diff content-aware)
        let toggle = if matches!(ctype.as_str(), "CheckBox" | "RadioButton" | "Button" | "MenuItem") {
            toggle_state(&el)
        } else {
            None
        };
        let selected = if matches!(
            ctype.as_str(),
            "ListItem" | "TabItem" | "TreeItem" | "DataItem" | "RadioButton"
        ) {
            selection_item_selected(&el)
        } else {
            None
        };
        let expanded = if matches!(ctype.as_str(), "ComboBox" | "MenuItem" | "TreeItem" | "Group") {
            expand_state(&el)
        } else {
            None
        };
        let range = if matches!(
            ctype.as_str(),
            "Slider" | "Spinner" | "ProgressBar" | "ScrollBar"
        ) {
            range_value(&el)
        } else {
            None
        };
        let patterns = if opts.with_patterns && probes_patterns(&ctype) {
            pattern_names_for(&el, &ctype)
        } else {
            Vec::new()
        };
        sig_parts.push(format!(
            "{}|{}|{}|{:?}|{}|{}|{}|{}|{:?}",
            ctype,
            name,
            class,
            rect,
            value.as_deref().unwrap_or(""),
            toggle.as_deref().unwrap_or(""),
            selected.map(|b| b as u8).unwrap_or(9),
            expanded.as_deref().unwrap_or(""),
            range
        ));
        let idx = nodes.len();
        nodes.push(Node {
            i: idx,
            ctype,
            name,
            value,
            class,
            aid,
            rect,
            disabled,
            offscreen,
            ctl: is_ctl,
            rid,
            pa,
            ri,
            toggle,
            selected,
            expanded,
            range,
            patterns,
        });

        if let Some(first) =
            first_child_with_retry(&walker, &el, cache_request.as_ref(), &mut errors, &mut last_error)
        {
            let mut sib = 0u32;
            let mut cur = first;
            loop {
                sib += 1;
                // children fetched through the build-cache navigation carry their
                // properties already
                queue.push_back((cur.clone(), depth + 1, Some(idx), sib, cache_request.is_some()));
                match next_sibling_with_retry(&walker, &cur, cache_request.as_ref(), &mut errors, &mut last_error) {
                    Some(next) => cur = next,
                    None => break,
                }
            }
        }
    }
    if errors > 0 {
        truncated = true;
    }

    let joined = sig_parts.join("\u{1}");
    let sig = format!("{:x}", crate::util::fnv1a(joined.as_bytes()));
    Ok(Tree { nodes, sig, truncated, errors, last_error })
}

/// Walker step with one retry for real transient failures (element mid-update,
/// RPC busy). Two special cases keep walks correct and fast:
/// - HRESULT 0 ("S_OK but null element"): Chromium providers answer "no children"
///   this way — a clean end of the branch, not an error.
/// - The last real double failure's error text is kept for the state report.
/// With a cache request the navigation call also fetches every core property.
fn first_child_with_retry(
    walker: &uiautomation::UITreeWalker,
    el: &UIElement,
    cache: Option<&uiautomation::core::UICacheRequest>,
    errors: &mut usize,
    last_error: &mut Option<String>,
) -> Option<UIElement> {
    let attempt = |el: &UIElement| match cache {
        Some(cr) => walker.get_first_child_build_cache(el, cr),
        None => walker.get_first_child(el),
    };
    match attempt(el) {
        Ok(c) => Some(c),
        Err(first) => {
            if first.code() == 0 {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
            match attempt(el) {
                Ok(c) => Some(c),
                Err(e) => {
                    *errors += 1;
                    *last_error = Some(format!(
                        "first_child of {} #{}: {first} / {e}",
                        el.get_control_type().map(type_name).unwrap_or_default(),
                        el.get_runtime_id().map(|r| r.first().copied().unwrap_or(0)).unwrap_or(0)
                    ));
                    None
                }
            }
        }
    }
}

fn next_sibling_with_retry(
    walker: &uiautomation::UITreeWalker,
    el: &UIElement,
    cache: Option<&uiautomation::core::UICacheRequest>,
    errors: &mut usize,
    last_error: &mut Option<String>,
) -> Option<UIElement> {
    let attempt = |el: &UIElement| match cache {
        Some(cr) => walker.get_next_sibling_build_cache(el, cr),
        None => walker.get_next_sibling(el),
    };
    match attempt(el) {
        Ok(c) => Some(c),
        Err(first) => {
            if first.code() == 0 {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
            match attempt(el) {
                Ok(c) => Some(c),
                Err(e) => {
                    *errors += 1;
                    *last_error = Some(format!(
                        "next_sibling of #{}: {first} / {e}",
                        el.get_runtime_id().map(|r| r.first().copied().unwrap_or(0)).unwrap_or(0)
                    ));
                    None
                }
            }
        }
    }
}

/// Core properties of one element, read from the client-side cache when the
/// element was fetched through a build-cache navigation; every cached read
/// falls back to the live getter so behavior matches the uncached path.
struct NodeProps {
    ctype: String,
    name: String,
    class: String,
    aid: String,
    rect: [i32; 4],
    rid: Vec<i32>,
    disabled: bool,
    offscreen: bool,
    is_ctl: bool,
}

fn read_props(el: &UIElement, cached: bool) -> NodeProps {
    if !cached {
        return NodeProps {
            ctype: el.get_control_type().map(type_name).unwrap_or_default(),
            name: el.get_name().unwrap_or_default(),
            class: el.get_classname().unwrap_or_default(),
            aid: el.get_automation_id().unwrap_or_default(),
            rect: el.get_bounding_rectangle().map(|r| rect_of(&r)).unwrap_or([0, 0, 0, 0]),
            rid: el.get_runtime_id().unwrap_or_default(),
            disabled: el.is_enabled().map(|b| !b).unwrap_or(false),
            offscreen: el.is_offscreen().unwrap_or(false),
            is_ctl: el.is_control_element().unwrap_or(true),
        };
    }
    NodeProps {
        ctype: el
            .get_cached_control_type()
            .map(type_name)
            .unwrap_or_else(|_| el.get_control_type().map(type_name).unwrap_or_default()),
        name: el
            .get_cached_name()
            .unwrap_or_else(|_| el.get_name().unwrap_or_default()),
        class: el
            .get_cached_classname()
            .unwrap_or_else(|_| el.get_classname().unwrap_or_default()),
        aid: el
            .get_cached_automation_id()
            .unwrap_or_else(|_| el.get_automation_id().unwrap_or_default()),
        rect: el
            .get_cached_bounding_rectangle()
            .map(|r| rect_of(&r))
            .unwrap_or_else(|_| el.get_bounding_rectangle().map(|r| rect_of(&r)).unwrap_or([0, 0, 0, 0])),
        rid: cached_rid(el),
        disabled: el.is_cached_enabled().map(|b| !b).unwrap_or_else(|_| {
            el.is_enabled().map(|b| !b).unwrap_or(false)
        }),
        offscreen: el
            .is_cached_offscreen()
            .unwrap_or_else(|_| el.is_offscreen().unwrap_or(false)),
        is_ctl: el
            .is_cached_control_element()
            .unwrap_or_else(|_| el.is_control_element().unwrap_or(true)),
    }
}

/// RuntimeId via the cache; falls back to the live call on any conversion miss.
fn cached_rid(el: &UIElement) -> Vec<i32> {
    el.get_cached_property_value(UIProperty::RuntimeId)
        .ok()
        .and_then(|v| v.get_value().ok())
        .and_then(|val| match val {
            uiautomation::variants::Value::ArrayI4(v) => Some(v),
            _ => None,
        })
        .unwrap_or_else(|| el.get_runtime_id().unwrap_or_default())
}

/// Resolve the UIA root element for a window handle. Some hosts refuse
/// ElementFromHandle for their top-level hwnd; fall back to scanning the
/// desktop root's direct children by native window handle before giving up.
fn window_root(automation: &UIAutomation, hwnd: HWND) -> Result<UIElement> {
    if let Ok(el) = automation.element_from_handle(Handle::from(hwnd.0 as isize)) {
        return Ok(el);
    }
    let desktop = automation
        .get_root_element()
        .map_err(|e| anyhow!("UIA element_from_handle failed: {e} and desktop root unavailable"))?;
    let target = hwnd.0 as isize;
    let matches_hwnd = |el: &UIElement| -> bool {
        el.get_native_window_handle().map(|h| -> isize { h.into() }).unwrap_or(0) == target
    };
    if matches_hwnd(&desktop) {
        return Ok(desktop);
    }
    if let Ok(walker) = automation.get_control_view_walker() {
        if let Some(mut cur) = walker.get_first_child(&desktop).ok() {
            loop {
                if matches_hwnd(&cur) {
                    return Ok(cur);
                }
                match walker.get_next_sibling(&cur) {
                    Ok(next) => cur = next,
                    Err(_) => break,
                }
            }
        }
    }
    bail!("UIA element_from_handle failed and no desktop-root child owns hwnd {:#x} (window may be a non-UIA surface)", target)
}

/// Output-only redaction. The action cache must keep the real names so a
/// no-runtime-id element can still be re-located by name after `--redact`.
pub fn redacted_nodes(nodes: &[Node]) -> Vec<Node> {
    nodes
        .iter()
        .cloned()
        .map(|mut n| {
            n.name = redact_text(&n.name);
            if let Some(v) = &n.value {
                n.value = Some(redact_text(v));
            }
            n
        })
        .collect()
}

/// Full node JSON — the element-cache payload. Carries the relocation fields
/// (`aid`, `pa`, `ri`, `ctl`) that the printed output strips.
pub fn nodes_json(nodes: &[Node]) -> Vec<serde_json::Value> {
    nodes
        .iter()
        .map(|n| {
            serde_json::json!({
                "i": n.i, "type": n.ctype, "name": n.name, "class": n.class,
                "aid": n.aid, "rect": n.rect, "rid": n.rid,
                "disabled": n.disabled, "offscreen": n.offscreen, "ctl": n.ctl,
                "pa": n.pa, "ri": n.ri,
                "value": n.value, "toggle": n.toggle, "selected": n.selected,
                "expanded": n.expanded, "range": n.range, "patterns": n.patterns,
            })
        })
        .collect()
}

/// Lean node JSON for printed observations: keeps `aid` when present (agents
/// use it as a stable handle), drops the internal relocation payload.
pub fn nodes_json_lean(nodes: &[Node]) -> Vec<serde_json::Value> {
    nodes
        .iter()
        .map(|n| {
            let mut v = serde_json::json!({
                "i": n.i, "type": n.ctype, "name": n.name, "class": n.class,
                "rect": n.rect, "rid": n.rid, "disabled": n.disabled, "offscreen": n.offscreen,
                "value": n.value, "toggle": n.toggle, "selected": n.selected,
                "expanded": n.expanded, "range": n.range, "patterns": n.patterns,
            });
            if !n.aid.is_empty() {
                v["aid"] = serde_json::json!(n.aid);
            }
            if !n.ctl {
                v["ctl"] = serde_json::json!(false);
            }
            v
        })
        .collect()
}

pub(crate) fn redact_text(s: &str) -> String {
    if s.chars().count() <= 1 {
        "*".repeat(s.len())
    } else {
        let mut c = s.chars();
        match c.next() {
            Some(first) => format!("{first}…"),
            None => String::new(),
        }
    }
}

fn toggle_state(el: &UIElement) -> Option<String> {
    let p = el.get_pattern::<UITogglePattern>().ok()?;
    let s = p.get_toggle_state().ok()?;
    Some(
        match s {
            ToggleState::On => "on",
            ToggleState::Off => "off",
            ToggleState::Indeterminate => "indeterminate",
        }
        .to_string(),
    )
}

fn selection_item_selected(el: &UIElement) -> Option<bool> {
    el.get_pattern::<UISelectionItemPattern>().ok()?.is_selected().ok()
}

fn expand_state(el: &UIElement) -> Option<String> {
    let s = el.get_pattern::<UIExpandCollapsePattern>().ok()?.get_state().ok()?;
    Some(
        match s {
            ExpandCollapseState::Expanded => "expanded",
            ExpandCollapseState::Collapsed => "collapsed",
            ExpandCollapseState::PartiallyExpanded => "partially-expanded",
            ExpandCollapseState::LeafNode => "leaf",
        }
        .to_string(),
    )
}

fn range_value(el: &UIElement) -> Option<[f64; 3]> {
    let p = el.get_pattern::<UIRangeValuePattern>().ok()?;
    let v = p.get_value().ok()?;
    let min = p.get_minimum().unwrap_or(0.0);
    let max = p.get_maximum().unwrap_or(0.0);
    Some([v, min, max])
}

/// Probing patterns is a cross-process COM call per pattern; skip bulk text nodes.
fn probes_patterns(ctype: &str) -> bool {
    matches!(
        ctype,
        "Button"
            | "CheckBox"
            | "RadioButton"
            | "ComboBox"
            | "Edit"
            | "Hyperlink"
            | "ListItem"
            | "MenuItem"
            | "Pane"
            | "Slider"
            | "Spinner"
            | "SplitButton"
            | "TabItem"
            | "TreeItem"
            | "DataItem"
            | "Document"
            | "List"
            | "Menu"
            | "MenuBar"
            | "Tree"
            | "Window"
            | "Custom"
            | "ScrollBar"
            | "Group"
            | "AppBar"
    )
}

fn pattern_names_for(el: &UIElement, ctype: &str) -> Vec<String> {
    let mut v = Vec::new();
    macro_rules! probe {
        ($t:ty, $name:literal) => {
            if el.get_pattern::<$t>().is_ok() {
                v.push($name.to_string());
            }
        };
    }
    probe!(UIInvokePattern, "Invoke");
    probe!(UITogglePattern, "Toggle");
    probe!(UISelectionItemPattern, "SelectionItem");
    probe!(UIValuePattern, "Value");
    probe!(UIRangeValuePattern, "RangeValue");
    probe!(UIExpandCollapsePattern, "ExpandCollapse");
    probe!(UILegacyIAccessiblePattern, "LegacyIAccessible");
    probe!(
        uiautomation::patterns::UIScrollItemPattern,
        "ScrollItem"
    );
    probe!(uiautomation::patterns::UIScrollPattern, "Scroll");
    probe!(uiautomation::patterns::UITextPattern, "Text");
    probe!(uiautomation::patterns::UIWindowPattern, "Window");
    // Grid/Transform/VirtualizedItem are only worth a probe on the types that can carry them.
    if ctype.is_empty() || matches!(ctype, "List" | "Table" | "DataGrid" | "Tree" | "Grid" | "Custom" | "Group") {
        probe!(UIGridPattern, "Grid");
        probe!(UITablePattern, "Table");
        probe!(UIItemContainerPattern, "ItemContainer");
    }
    if matches!(ctype, "DataItem" | "ListItem" | "TreeItem" | "Custom") {
        probe!(UITableItemPattern, "TableItem");
    }
    if ctype.is_empty() || matches!(ctype, "Window" | "Pane") {
        probe!(UITransformPattern, "Transform");
    }
    if matches!(ctype, "DataItem" | "ListItem" | "TreeItem" | "Custom") {
        probe!(UIVirtualizedItemPattern, "VirtualizedItem");
    }
    v
}

/// Rect + center for an element (used by hover and coordinate fallbacks).
pub fn element_center(hwnd: HWND, rid: &[i32]) -> Result<([i32; 4], (i32, i32), String)> {
    let el = find_by_rid(hwnd, rid)?;
    let r = el.get_bounding_rectangle().map_err(|e| anyhow!("rect: {e}"))?;
    let rect = rect_of(&r);
    if rect[2] <= 0 || rect[3] <= 0 {
        bail!("element has no usable rect");
    }
    let name = el.get_name().unwrap_or_default();
    Ok((rect, (rect[0] + rect[2] / 2, rect[1] + rect[3] / 2), name))
}

pub fn find_by_rid(hwnd: HWND, rid: &[i32]) -> Result<UIElement> {
    if let Ok(el) = locate_rid(hwnd, rid) {
        return Ok(el);
    }
    // A previous relocation already mapped this stale rid to a live one.
    if let Some(new_rid) = crate::util::load_rid_override(hwnd.0 as isize, rid) {
        if let Ok(el) = locate_rid(hwnd, &new_rid) {
            return Ok(el);
        }
    }
    // Runtime id dead (dialog reopened, list refreshed, app restarted): fall back
    // to attribute relocation from the cached observation before giving up.
    match relocate_cached(hwnd, rid) {
        Ok(el) => Ok(el),
        Err(reloc) => bail!(
            "element rid {rid:?} not found and attribute relocation failed ({reloc}) — UI tree changed, re-run 'cua state'"
        ),
    }
}

fn locate_rid(hwnd: HWND, rid: &[i32]) -> Result<UIElement> {
    let automation = automation()?;
    let root = window_root(&automation, hwnd)?;
    if element_matches(&root, rid) {
        return Ok(root);
    }
    // Bulk scan first: a single cross-process enumeration performed inside the
    // UIA core is far more robust on busy windows than per-node walker steps.
    if let Ok(cond) = automation.create_true_condition() {
        if let Ok(all) = root.find_all(TreeScope::Subtree, &cond) {
            for el in all.into_iter().take(20000) {
                if element_matches(&el, rid) {
                    return Ok(el);
                }
            }
        }
    }
    // Walker fallback for providers that refuse FindAll.
    let walker = automation.get_control_view_walker().map_err(|e| anyhow!("walker: {e}"))?;
    let mut stack = vec![root];
    let mut visited = 0usize;
    while let Some(el) = stack.pop() {
        visited += 1;
        if visited > 4000 {
            break;
        }
        if element_matches(&el, rid) {
            return Ok(el);
        }
        if let Ok(child) = walker.get_first_child(&el) {
            stack.push(child.clone());
            let mut cur = child;
            loop {
                match walker.get_next_sibling(&cur) {
                    Ok(next) => {
                        stack.push(next.clone());
                        cur = next;
                    }
                    Err(_) => break,
                }
            }
        }
    }
    bail!("rid not in this window")
}

fn element_matches(el: &UIElement, rid: &[i32]) -> bool {
    el.get_runtime_id().map(|r| r == rid).unwrap_or(false)
}

// ---------------- attribute relocation ----------------

/// The relocatable identity of a cached element: strong keys (automation id,
/// name) plus context (class, type, parent attributes, sibling position) and
/// the cached center for the point-hit fast path.
#[derive(Debug, Clone, Default)]
pub(crate) struct Locator {
    name: String,
    class: String,
    aid: String,
    ctype: String,
    parent_name: String,
    parent_class: String,
    parent_aid: String,
    rel: u32,
    cx: i32,
    cy: i32,
}

fn rid_of_node(node: &serde_json::Value) -> Vec<i32> {
    node["rid"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_i64().map(|x| x as i32)).collect())
        .unwrap_or_default()
}

fn locator_from_node(node: &serde_json::Value, all: &[serde_json::Value]) -> Option<Locator> {
    let rect = node["rect"].as_array()?;
    if rect.len() < 4 {
        return None;
    }
    let (l, t, w, h) = (
        rect[0].as_i64()? as i32,
        rect[1].as_i64()? as i32,
        rect[2].as_i64()? as i32,
        rect[3].as_i64()? as i32,
    );
    if w <= 0 || h <= 0 {
        return None;
    }
    let (parent_name, parent_class, parent_aid) = match node["pa"].as_u64().and_then(|i| all.get(i as usize)) {
        Some(p) => (
            p["name"].as_str().unwrap_or_default().to_string(),
            p["class"].as_str().unwrap_or_default().to_string(),
            p["aid"].as_str().unwrap_or_default().to_string(),
        ),
        None => (String::new(), String::new(), String::new()),
    };
    let s = |k: &str| node[k].as_str().unwrap_or_default().to_string();
    Some(Locator {
        name: s("name"),
        class: s("class"),
        aid: s("aid"),
        ctype: s("type"),
        parent_name,
        parent_class,
        parent_aid,
        rel: node["ri"].as_u64().unwrap_or(0) as u32,
        cx: l + w / 2,
        cy: t + h / 2,
    })
}

fn attr_text(el: &UIElement) -> (String, String, String, String) {
    (
        el.get_name().unwrap_or_default(),
        el.get_classname().unwrap_or_default(),
        el.get_automation_id().unwrap_or_default(),
        el.get_control_type().map(type_name).unwrap_or_default(),
    )
}

fn name_matches(actual: &str, want: &str) -> bool {
    let (n, w) = (actual.to_lowercase(), want.to_lowercase());
    n == w || (!n.is_empty() && n.contains(&w))
}

/// Re-locate a stale rid by the attributes cached at observation time and, on
/// success, patch the cache so later actions resolve directly. Throttled so a
/// wait loop cannot turn into a relocate storm on a huge tree.
fn relocate_cached(hwnd: HWND, old_rid: &[i32]) -> Result<UIElement> {
    static LAST_ATTEMPT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let last = LAST_ATTEMPT.load(std::sync::atomic::Ordering::Relaxed);
    if now.saturating_sub(last) < 500 {
        bail!("relocation throttled (last attempt {last} ms ago)");
    }
    LAST_ATTEMPT.store(now, std::sync::atomic::Ordering::Relaxed);

    let cache = crate::util::load_observation_nodes(hwnd.0 as isize)
        .ok_or_else(|| anyhow!("no cached observation for this window"))?;
    let arr = cache
        .as_array()
        .ok_or_else(|| anyhow!("corrupt element cache"))?
        .clone();
    let pos = arr
        .iter()
        .position(|n| rid_of_node(n) == old_rid)
        .ok_or_else(|| anyhow!("rid not in the cached observation"))?;
    let loc = locator_from_node(&arr[pos], &arr)
        .ok_or_else(|| anyhow!("cached element has no usable locator payload (no rect)"))?;
    if loc.aid.is_empty() && loc.name.is_empty() {
        bail!("cached element has neither automation id nor name to relocate by");
    }

    let automation = automation()?;
    let root = window_root(&automation, hwnd)?;

    // Fast path: the element is where it was; one point hit verified against
    // every cached strong attribute.
    if let Some(el) = point_hit_matches(&automation, &loc) {
        record_relocation(hwnd, old_rid, &cache, pos, &el);
        return Ok(el);
    }

    // Full scan scored against the locator. A candidate must match at least one
    // strong key (automation id exact, or name exact/contains) to be ranked.
    let cond = automation
        .create_true_condition()
        .map_err(|e| anyhow!("true condition: {e}"))?;
    let candidates = root
        .find_all(TreeScope::Subtree, &cond)
        .map_err(|e| anyhow!("find_all: {e}"))?;
    let mut scored: Vec<(i32, UIElement)> = Vec::new();
    for el in candidates.into_iter().take(20000) {
        let (name, class, aid, ctype) = attr_text(&el);
        let strong = (!loc.aid.is_empty() && aid.eq_ignore_ascii_case(&loc.aid))
            || (!loc.name.is_empty() && name_matches(&name, &loc.name));
        if !strong {
            continue;
        }
        if let Ok(r) = el.get_bounding_rectangle() {
            if r.get_width() <= 0 || r.get_height() <= 0 {
                continue; // dead providers advertise zero rects
            }
        }
        let score = locator_score(&loc, &name, &class, &aid, &ctype);
        if score > 0 {
            scored.push((score, el));
        }
    }
    if scored.is_empty() {
        bail!("no candidate matches the cached automation id/name");
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let margin = if scored.len() > 1 {
        scored[0].0.saturating_sub(scored[1].0)
    } else {
        i32::MAX // a single strong-key candidate is unique by construction
    };
    if margin < RELOC_MIN_MARGIN {
        // Tie-break with parent attributes and sibling position, then re-check.
        if let Ok(walker) = automation.get_control_view_walker() {
            rerank_with_context(&walker, &mut scored, &loc);
            scored.sort_by(|a, b| b.0.cmp(&a.0));
        }
        if scored.len() > 1 && scored[0].0.saturating_sub(scored[1].0) < RELOC_MIN_MARGIN {
            bail!(
                "ambiguous: {} candidates within {} points (top scores {}, {})",
                scored.len().min(9),
                RELOC_MIN_MARGIN,
                scored[0].0,
                scored[1].0
            );
        }
    }
    record_relocation(hwnd, old_rid, &cache, pos, &scored[0].1);
    Ok(scored[0].1.clone())
}

const RELOC_MIN_MARGIN: i32 = 40;

fn locator_score(loc: &Locator, name: &str, class: &str, aid: &str, ctype: &str) -> i32 {
    let mut score = 0;
    if !loc.name.is_empty() {
        let n = name.to_lowercase();
        let w = loc.name.to_lowercase();
        if n == w {
            score += 220;
        } else if !n.is_empty() && n.contains(&w) {
            score += 120;
        } else {
            score -= 22;
        }
    }
    if !loc.aid.is_empty() {
        if aid.eq_ignore_ascii_case(&loc.aid) {
            score += 140;
        } else {
            score -= 10;
        }
    }
    if !loc.class.is_empty() {
        if class.eq_ignore_ascii_case(&loc.class) {
            score += 55;
        } else {
            score -= 8;
        }
    }
    if !loc.ctype.is_empty() && ctype.eq_ignore_ascii_case(&loc.ctype) {
        score += 26;
    }
    score
}

/// Parent attributes + relative sibling position, applied only to the top
/// candidates during a tie-break (each check costs extra cross-process calls).
fn rerank_with_context(walker: &uiautomation::UITreeWalker, scored: &mut [(i32, UIElement)], loc: &Locator) {
    for (score, el) in scored.iter_mut().take(4) {
        let Ok(parent) = walker.get_parent(el) else { continue };
        if !loc.parent_aid.is_empty()
            && parent.get_automation_id().unwrap_or_default().eq_ignore_ascii_case(&loc.parent_aid)
        {
            *score += 42;
        }
        if !loc.parent_class.is_empty()
            && parent.get_classname().unwrap_or_default().eq_ignore_ascii_case(&loc.parent_class)
        {
            *score += 24;
        }
        if !loc.parent_name.is_empty()
            && name_matches(&parent.get_name().unwrap_or_default(), &loc.parent_name)
        {
            *score += 20;
        }
        if loc.rel > 0 {
            if let Some(rel) = sibling_position(walker, &parent, el) {
                if rel == loc.rel {
                    *score += 20;
                } else {
                    *score -= 6;
                }
            }
        }
    }
}

fn sibling_position(walker: &uiautomation::UITreeWalker, parent: &UIElement, target: &UIElement) -> Option<u32> {
    let target_rid = target.get_runtime_id().unwrap_or_default();
    let mut cur = walker.get_first_child(parent).ok()?;
    let mut i = 0u32;
    loop {
        i += 1;
        if i > 500 {
            return None;
        }
        if cur.get_runtime_id().map(|r| r == target_rid).unwrap_or(false) {
            return Some(i);
        }
        cur = walker.get_next_sibling(&cur).ok()?;
    }
}

/// Cached-center point hit verified against every cached strong attribute —
/// an O(1) recovery when the tree rebuilt in place.
fn point_hit_matches(automation: &UIAutomation, loc: &Locator) -> Option<UIElement> {
    if (loc.cx == 0 && loc.cy == 0) || (loc.aid.is_empty() && loc.name.is_empty()) {
        return None;
    }
    let el = automation
        .element_from_point(uiautomation::types::Point::new(loc.cx, loc.cy))
        .ok()?;
    let (name, class, aid, ctype) = attr_text(&el);
    if !loc.aid.is_empty() && !aid.eq_ignore_ascii_case(&loc.aid) {
        return None;
    }
    if !loc.class.is_empty() && !class.eq_ignore_ascii_case(&loc.class) {
        return None;
    }
    if !loc.ctype.is_empty() && !ctype.eq_ignore_ascii_case(&loc.ctype) {
        return None;
    }
    if !loc.name.is_empty() && !name_matches(&name, &loc.name) {
        return None;
    }
    Some(el)
}

/// Record a successful relocation: the authoritative old-rid -> new-rid override
/// (observations are immutable) plus a best-effort patch of the legacy elements
/// cache (new rid, fresh rect/automation id; name is left as observed so the
/// agent's mental model of the index does not shift).
fn record_relocation(hwnd: HWND, old_rid: &[i32], cache: &serde_json::Value, pos: usize, el: &UIElement) {
    if let Some(new_rid) = el.get_runtime_id().ok() {
        if !new_rid.is_empty() && new_rid != old_rid {
            crate::util::save_rid_override(hwnd.0 as isize, old_rid, &new_rid);
        }
    }
    let mut cache = cache.clone();
    let Some(arr) = cache.as_array_mut() else { return };
    if pos >= arr.len() {
        return;
    }
    let entry = &mut arr[pos];
    entry["rid"] = serde_json::json!(el.get_runtime_id().unwrap_or_default());
    if let Ok(r) = el.get_bounding_rectangle() {
        entry["rect"] = serde_json::json!(rect_of(&r));
    }
    let aid = el.get_automation_id().unwrap_or_default();
    if !aid.is_empty() {
        entry["aid"] = serde_json::json!(aid);
    }
    crate::util::save_elements(hwnd.0 as isize, &cache);
}

// ---------------- point -> element reverse lookup ----------------

/// What sits at a screen point, refined past the coarse container that
/// ElementFromPoint usually returns. Pure UIA: never moves the pointer,
/// never touches focus.
#[derive(Serialize, Clone, Debug)]
pub struct ElementHit {
    pub name: String,
    #[serde(rename = "type")]
    pub ctype: String,
    pub class: String,
    pub aid: String,
    pub rect: [i32; 4],
    pub rid: Vec<i32>,
    /// false when the hit is a raw-view-only node (IsControlElement=false)
    pub control: bool,
    /// true when the seed itself was a TabItem (tabs are returned as-is)
    pub tab_item: bool,
    /// native window handle of the hit's top-level ancestor (0 when none) —
    /// the right cache key for index mapping, unlike WindowFromPoint which can
    /// return an unrelated layered/popup window
    pub root_hwnd: isize,
    /// runtime ids of the hit and its ancestors, nearest first (capped) — lets
    /// callers map an inner hit back to the closest observed element
    pub ancestor_rids: Vec<Vec<i32>>,
}

const POINT_REFINE_MAX_DEPTH: usize = 14;
const POINT_REFINE_MAX_HOPS: usize = 4;

pub fn element_at_point(x: i32, y: i32) -> Result<ElementHit> {
    let automation = automation()?;
    let seed = automation
        .element_from_point(uiautomation::types::Point::new(x, y))
        .map_err(|e| anyhow!("UIA element_from_point failed: {e}"))?;
    let seed_type = seed.get_control_type().map(type_name).unwrap_or_default();
    let refined = if seed_type == "TabItem" {
        seed.clone()
    } else {
        // The smallest hit often lives in the raw view, so ancestors and
        // subtrees are searched on both walkers.
        let cw = automation.get_control_view_walker().ok();
        let rw = automation.get_raw_view_walker().ok();
        let mut roots: Vec<UIElement> = vec![seed.clone()];
        for walker in [&cw, &rw].into_iter().flatten() {
            let mut cur = seed.clone();
            for _ in 0..POINT_REFINE_MAX_HOPS {
                let Ok(parent) = walker.get_parent(&cur) else { break };
                roots.push(parent.clone());
                cur = parent;
            }
        }
        let mut best: Option<PointCand> = None;
        for root in roots {
            for walker in [&cw, &rw].into_iter().flatten() {
                if let Some(c) = best_point_hit_in_subtree(walker, &root, x, y, 0) {
                    best = pick_better_point_hit(best, Some(c));
                }
            }
        }
        best.map(|c| c.el).unwrap_or(seed)
    };
    Ok(hit_of(&automation, &refined, seed_type == "TabItem"))
}

fn hit_of(automation: &UIAutomation, el: &UIElement, tab_item: bool) -> ElementHit {
    let rect = el
        .get_bounding_rectangle()
        .map(|r| rect_of(&r))
        .unwrap_or([0, 0, 0, 0]);
    // Ancestor chain in the control view (runtime ids are view-independent).
    // The top-most handle-bearing ancestor is the element's real root window.
    let mut ancestor_rids: Vec<Vec<i32>> = vec![el.get_runtime_id().unwrap_or_default()];
    let mut root_hwnd: isize = 0;
    if let Ok(walker) = automation.get_control_view_walker() {
        let desktop = automation.get_root_element().ok();
        let mut cur = el.clone();
        for hop in 0..64 {
            let Ok(parent) = walker.get_parent(&cur) else { break };
            if parent.get_runtime_id().map(|r| r == cur.get_runtime_id().unwrap_or_default()).unwrap_or(true) {
                break; // guard against provider parent loops
            }
            // stop before the desktop itself: its NativeWindowHandle (class
            // #32769) would otherwise shadow the real top-level window's
            if desktop
                .as_ref()
                .and_then(|d| automation.compare_elements(&parent, d).ok())
                .unwrap_or(false)
            {
                break;
            }
            if let Ok(h) = parent.get_native_window_handle() {
                root_hwnd = h.into();
            }
            if hop < 8 {
                ancestor_rids.push(parent.get_runtime_id().unwrap_or_default());
            }
            cur = parent;
        }
    }
    ElementHit {
        name: el.get_name().unwrap_or_default(),
        ctype: el.get_control_type().map(type_name).unwrap_or_default(),
        class: el.get_classname().unwrap_or_default(),
        aid: el.get_automation_id().unwrap_or_default(),
        rect,
        rid: ancestor_rids.first().cloned().unwrap_or_default(),
        control: el.is_control_element().unwrap_or(true),
        tab_item,
        root_hwnd,
        ancestor_rids,
    }
}

struct PointCand {
    el: UIElement,
    key: PointHitKey,
}

/// Comparison payload of a point-hit candidate, free of COM handles so the
/// preference logic is unit-testable.
#[derive(Clone, Debug, PartialEq)]
struct PointHitKey {
    depth: usize,
    area: i64,
    ctype: String,
    class: String,
}

fn tabstrip_like(class: &str) -> bool {
    let c = class.trim().to_ascii_lowercase();
    c.contains("tabstrip") || c.contains("tab_drag")
}

/// Is `b` a better point hit than `a`? Preference order: TabItem explicitly
/// promoted (a tab bar hit must resolve to the tab, not its strip), tabstrip
/// containers demoted, then deeper, then smaller — i.e. the innermost real
/// control under the point.
fn point_hit_key_better(b: &PointHitKey, a: &PointHitKey) -> bool {
    let (a_tab, b_tab) = (a.ctype == "TabItem", b.ctype == "TabItem");
    if a_tab != b_tab {
        return b_tab;
    }
    let (a_strip, b_strip) = (tabstrip_like(&a.class), tabstrip_like(&b.class));
    if a_strip != b_strip {
        return !b_strip;
    }
    if a.depth != b.depth {
        return b.depth > a.depth;
    }
    if a.area != b.area {
        return b.area < a.area;
    }
    false
}

fn pick_better_point_hit(cur: Option<PointCand>, next: Option<PointCand>) -> Option<PointCand> {
    match (cur, next) {
        (None, n) => n,
        (c, None) => c,
        (Some(a), Some(b)) => {
            if point_hit_key_better(&b.key, &a.key) {
                Some(b)
            } else {
                Some(a)
            }
        }
    }
}

fn best_point_hit_in_subtree(
    walker: &uiautomation::UITreeWalker,
    root: &UIElement,
    x: i32,
    y: i32,
    depth: usize,
) -> Option<PointCand> {
    if depth > POINT_REFINE_MAX_DEPTH {
        return None;
    }
    let rect = root.get_bounding_rectangle().ok()?;
    let (l, t, w, h) = (rect.get_left(), rect.get_top(), rect.get_width(), rect.get_height());
    if w <= 0 || h <= 0 || x < l || x >= l + w || y < t || y >= t + h {
        return None;
    }
    let mut best = Some(PointCand {
        el: root.clone(),
        key: PointHitKey {
            depth,
            area: w as i64 * h as i64,
            ctype: root.get_control_type().map(type_name).unwrap_or_default(),
            class: root.get_classname().unwrap_or_default(),
        },
    });
    if let Ok(first) = walker.get_first_child(root) {
        let mut cur = first;
        loop {
            if let Some(c) = best_point_hit_in_subtree(walker, &cur, x, y, depth + 1) {
                best = pick_better_point_hit(best, Some(c));
            }
            match walker.get_next_sibling(&cur) {
                Ok(next) => cur = next,
                Err(_) => break,
            }
        }
    }
    best
}

/// a11y-first click: Invoke > Toggle > SelectionItem > Legacy default action > coordinates.
/// Right/double clicks always use real coordinate events (patterns have no such semantics).
pub fn click_element(hwnd: HWND, rid: &[i32], double: bool, right: bool) -> Result<String> {
    let el = find_by_rid(hwnd, rid)?;
    if double || right {
        return click_element_rect(&el, hwnd, double, right);
    }
    if let Ok(p) = el.get_pattern::<UIInvokePattern>() {
        p.invoke().map_err(|e| anyhow!("invoke failed: {e}"))?;
        return Ok("invoked element via UIA InvokePattern".into());
    }
    if let Ok(p) = el.get_pattern::<UITogglePattern>() {
        p.toggle().map_err(|e| anyhow!("toggle failed: {e}"))?;
        let state = toggle_state(&el).unwrap_or_else(|| "?".into());
        return Ok(format!("toggled element via UIA TogglePattern (now {state})"));
    }
    if let Ok(p) = el.get_pattern::<UISelectionItemPattern>() {
        p.select().map_err(|e| anyhow!("select failed: {e}"))?;
        return Ok("selected element via UIA SelectionItemPattern".into());
    }
    if let Ok(p) = el.get_pattern::<UILegacyIAccessiblePattern>() {
        if p.do_default_action().is_ok() {
            return Ok("invoked element via LegacyIAccessible DoDefaultAction".into());
        }
        // legacy providers are frequently stubs (Explorer's grid cells advertise the
        // interface and then refuse it), so a failure here must not block the rect fallback
    }
    click_element_rect(&el, hwnd, double, right)
}

fn click_element_rect(el: &UIElement, top: HWND, double: bool, right: bool) -> Result<String> {
    let r = el.get_bounding_rectangle().map_err(|e| anyhow!("rect: {e}"))?;
    let rect = rect_of(&r);
    if rect[2] <= 0 || rect[3] <= 0 {
        bail!("element has no actionable pattern and no usable rect");
    }
    let cx = rect[0] + rect[2] / 2;
    let cy = rect[1] + rect[3] / 2;
    crate::win::ensure_point_hits(top, cx, cy)?;
    crate::input::click(cx, cy, right, double)?;
    Ok(format!(
        "clicked element at ({cx},{cy}) via event strategy{}{} (no pattern for this gesture)",
        if right { " right" } else { "" },
        if double { " double" } else { "" }
    ))
}

/// Background click (`--background --index`): a11y patterns never move focus, and when
/// the element has none the gesture degrades to *message-level* input at the element's
/// rectangle instead of the real pointer a foreground click would steal.
pub fn click_element_background(hwnd: HWND, rid: &[i32], double: bool, right: bool) -> Result<String> {
    let el = find_by_rid(hwnd, rid)?;
    if !double && !right {
        if let Ok(p) = el.get_pattern::<UIInvokePattern>() {
            p.invoke().map_err(|e| anyhow!("invoke failed: {e}"))?;
            return Ok("invoked element via UIA InvokePattern (background; no focus change)".into());
        }
        if let Ok(p) = el.get_pattern::<UITogglePattern>() {
            p.toggle().map_err(|e| anyhow!("toggle failed: {e}"))?;
            let state = toggle_state(&el).unwrap_or_else(|| "?".into());
            return Ok(format!("toggled element via UIA TogglePattern (now {state}; background, no focus change)"));
        }
        if let Ok(p) = el.get_pattern::<UISelectionItemPattern>() {
            p.select().map_err(|e| anyhow!("select failed: {e}"))?;
            return Ok("selected element via UIA SelectionItemPattern (background; no focus change)".into());
        }
        if let Ok(p) = el.get_pattern::<UILegacyIAccessiblePattern>() {
            if p.do_default_action().is_ok() {
                return Ok("invoked element via LegacyIAccessible DoDefaultAction (background; no focus change)".into());
            }
        }
    }
    let r = el.get_bounding_rectangle().map_err(|e| anyhow!("rect: {e}"))?;
    let rect = rect_of(&r);
    if rect[2] <= 0 || rect[3] <= 0 {
        bail!("element has no actionable pattern and no usable rect");
    }
    let cx = rect[0] + rect[2] / 2;
    let cy = rect[1] + rect[3] / 2;
    let root = crate::win::root_window(hwnd);
    crate::input::post_click(root, cx, cy, right, double)?;
    Ok(format!(
        "clicked ({cx},{cy}) via message-level input (background; no a11y pattern for this gesture)"
    ))
}

/// strategy=event: skip the pattern chain and drive the element's rectangle with
/// real input. Used when the caller explicitly wants event semantics.
pub fn click_element_event(hwnd: HWND, rid: &[i32], double: bool, right: bool) -> Result<String> {
    let el = find_by_rid(hwnd, rid)?;
    click_element_rect(&el, hwnd, double, right)
}

/// a11y-only click: refuses coordinate fallback (ZCode strategy=a11y).
pub fn click_element_strict(hwnd: HWND, rid: &[i32], double: bool, right: bool) -> Result<String> {    if double || right {
        bail!("strategy=a11y cannot express right/double clicks (use strategy=auto or coordinates)");
    }
    let el = find_by_rid(hwnd, rid)?;
    if let Ok(p) = el.get_pattern::<UIInvokePattern>() {
        p.invoke()?;
        return Ok("invoked element via UIA InvokePattern".into());
    }
    if let Ok(p) = el.get_pattern::<UITogglePattern>() {
        p.toggle()?;
        let state = toggle_state(&el).unwrap_or_else(|| "?".into());
        return Ok(format!("toggled element via UIA TogglePattern (now {state})"));
    }
    if let Ok(p) = el.get_pattern::<UISelectionItemPattern>() {
        p.select()?;
        return Ok("selected element via UIA SelectionItemPattern".into());
    }
    if let Ok(p) = el.get_pattern::<UILegacyIAccessiblePattern>() {
        p.do_default_action()?;
        return Ok("invoked element via LegacyIAccessible DoDefaultAction".into());
    }
    bail!("element has no Invoke/Toggle/SelectionItem/Legacy pattern (strategy=a11y forbids coordinate fallback); re-observe or use strategy=auto")
}

pub fn set_value_element(hwnd: HWND, rid: &[i32], value: &str) -> Result<String> {
    let el = find_by_rid(hwnd, rid)?;
    if let Ok(p) = el.get_pattern::<UIValuePattern>() {
        p.set_value(value).map_err(|e| anyhow!("set_value failed: {e}"))?;
        return Ok(format!(
            "set value ({len} chars) via UIA ValuePattern",
            len = value.chars().count()
        ));
    }
    if let Ok(p) = el.get_pattern::<UIRangeValuePattern>() {
        let v: f64 = value
            .trim()
            .parse()
            .map_err(|_| anyhow!("element is a RangeValue control; value must be numeric"))?;
        p.set_value(v).map_err(|e| anyhow!("range set_value failed: {e}"))?;
        return Ok(format!("set range value {v} via UIA RangeValuePattern"));
    }
    if let Ok(p) = el.get_pattern::<UILegacyIAccessiblePattern>() {
        p.set_value(value).map_err(|e| anyhow!("legacy set_value failed: {e}"))?;
        return Ok(format!(
            "set value ({len} chars) via LegacyIAccessible",
            len = value.chars().count()
        ));
    }
    bail!("element does not support UIA ValuePattern/RangeValue/Legacy value; use 'type' instead")
}

pub fn get_value_element(hwnd: HWND, rid: &[i32]) -> Result<String> {
    let el = find_by_rid(hwnd, rid)?;
    let p = el
        .get_pattern::<UIValuePattern>()
        .map_err(|_| anyhow!("element does not support UIA ValuePattern"))?;
    Ok(p.get_value()?)
}

/// Value read for wait conditions: ValuePattern, then RangeValue, then Legacy.
pub fn value_any_element(hwnd: HWND, rid: &[i32]) -> Result<String> {
    let el = find_by_rid(hwnd, rid)?;
    if let Ok(p) = el.get_pattern::<UIValuePattern>() {
        return Ok(p.get_value()?);
    }
    if let Ok(p) = el.get_pattern::<UIRangeValuePattern>() {
        return Ok(p.get_value()?.to_string());
    }
    if let Ok(p) = el.get_pattern::<UILegacyIAccessiblePattern>() {
        return Ok(p.get_value()?);
    }
    bail!("element has no readable value (ValuePattern/RangeValue/Legacy)")
}

pub fn toggle_of_element(hwnd: HWND, rid: &[i32]) -> Result<String> {
    let el = find_by_rid(hwnd, rid)?;
    toggle_state(&el).ok_or_else(|| anyhow!("element does not expose TogglePattern"))
}

pub fn selected_of_element(hwnd: HWND, rid: &[i32]) -> Result<bool> {
    let el = find_by_rid(hwnd, rid)?;
    selection_item_selected(&el).ok_or_else(|| anyhow!("element does not expose SelectionItemPattern"))
}

pub fn select_add_element(hwnd: HWND, rid: &[i32]) -> Result<String> {
    let el = find_by_rid(hwnd, rid)?;
    let p = el
        .get_pattern::<UISelectionItemPattern>()
        .map_err(|_| anyhow!("element does not expose SelectionItemPattern"))?;
    p.add_to_selection()
        .map_err(|e| anyhow!("add_to_selection: {e}"))?;
    Ok("added element to the selection (SelectionItemPattern.AddToSelection)".into())
}

pub fn select_remove_element(hwnd: HWND, rid: &[i32]) -> Result<String> {
    let el = find_by_rid(hwnd, rid)?;
    let p = el
        .get_pattern::<UISelectionItemPattern>()
        .map_err(|_| anyhow!("element does not expose SelectionItemPattern"))?;
    p.remove_from_selection()
        .map_err(|e| anyhow!("remove_from_selection: {e}"))?;
    Ok("removed element from the selection (SelectionItemPattern.RemoveFromSelection)".into())
}

pub fn expanded_of_element(hwnd: HWND, rid: &[i32]) -> Result<String> {
    let el = find_by_rid(hwnd, rid)?;
    expand_state(&el).ok_or_else(|| anyhow!("element does not expose ExpandCollapsePattern"))
}

pub fn enabled_of_element(hwnd: HWND, rid: &[i32]) -> Result<bool> {
    let el = find_by_rid(hwnd, rid)?;
    Ok(el.is_enabled()?)
}

/// Read a bounded TextPattern document range. Returns (text, truncated).
pub fn text_element(hwnd: HWND, rid: &[i32], max_chars: usize) -> Result<(String, bool)> {
    let el = find_by_rid(hwnd, rid)?;
    let p = el
        .get_pattern::<UITextPattern>()
        .map_err(|_| anyhow!("element does not expose UIA TextPattern"))?;
    let range = p.get_document_range().map_err(|e| anyhow!("document range: {e}"))?;
    let max = max_chars.clamp(1, 1_000_000);
    let raw = range
        .get_text((max as i32).saturating_add(1))
        .map_err(|e| anyhow!("get_text: {e}"))?;
    let truncated = raw.chars().count() > max;
    Ok((raw.chars().take(max).collect(), truncated))
}

fn element_brief(el: &UIElement) -> serde_json::Value {
    serde_json::json!({
        "name": el.get_name().unwrap_or_default(),
        "type": el.get_control_type().map(type_name).unwrap_or_default(),
        "rect": el.get_bounding_rectangle().map(|r| rect_of(&r)).unwrap_or([0, 0, 0, 0]),
        "rid": el.get_runtime_id().unwrap_or_default(),
    })
}

fn header_briefs(items: Vec<UIElement>, max: usize) -> Vec<serde_json::Value> {
    items.iter().take(max).map(element_brief).collect()
}

/// Table headers and row/column-major order; also reports TableItem headers when
/// the element is a cell rather than the table itself.
pub fn table_element(hwnd: HWND, rid: &[i32], max_headers: usize) -> Result<serde_json::Value> {
    let el = find_by_rid(hwnd, rid)?;
    let max = max_headers.clamp(1, 100);
    let mut out = serde_json::Map::new();
    let mut found = false;
    if let Ok(p) = el.get_pattern::<UITablePattern>() {
        found = true;
        out.insert(
            "row_headers".into(),
            serde_json::json!(header_briefs(
                p.get_row_headers().map_err(|e| anyhow!("row headers: {e}"))?,
                max
            )),
        );
        out.insert(
            "column_headers".into(),
            serde_json::json!(header_briefs(
                p.get_column_headers().map_err(|e| anyhow!("column headers: {e}"))?,
                max
            )),
        );
        out.insert(
            "row_or_column_major".into(),
            serde_json::json!(format!(
                "{:?}",
                p.get_row_or_column_major().unwrap_or(uiautomation::types::RowOrColumnMajor::RowMajor)
            )),
        );
    }
    if let Ok(p) = el.get_pattern::<UITableItemPattern>() {
        found = true;
        out.insert(
            "row_header_items".into(),
            serde_json::json!(header_briefs(
                p.get_row_header_items().map_err(|e| anyhow!("row header items: {e}"))?,
                max
            )),
        );
        out.insert(
            "column_header_items".into(),
            serde_json::json!(header_briefs(
                p.get_column_header_items().map_err(|e| anyhow!("column header items: {e}"))?,
                max
            )),
        );
    }
    if !found {
        bail!("element does not expose UIA TablePattern or TableItemPattern");
    }
    Ok(serde_json::Value::Object(out))
}

fn property_from_name(name: &str) -> Result<UIProperty> {
    match name.to_ascii_lowercase().as_str() {
        "name" => Ok(UIProperty::Name),
        "automation-id" | "automationid" => Ok(UIProperty::AutomationId),
        "class" | "class-name" | "classname" => Ok(UIProperty::ClassName),
        "help-text" | "helptext" => Ok(UIProperty::HelpText),
        "item-type" | "itemtype" => Ok(UIProperty::ItemType),
        "item-status" | "itemstatus" => Ok(UIProperty::ItemStatus),
        other => bail!(
            "unsupported item property '{other}' (name|automation-id|class-name|help-text|item-type|item-status)"
        ),
    }
}

/// Ask an ItemContainer provider for an item by a string property.
pub fn find_item_element(
    hwnd: HWND,
    rid: &[i32],
    property: &str,
    value: &str,
) -> Result<serde_json::Value> {
    let el = find_by_rid(hwnd, rid)?;
    let p = el
        .get_pattern::<UIItemContainerPattern>()
        .map_err(|_| anyhow!("element does not expose UIA ItemContainerPattern"))?;
    let prop = property_from_name(property)?;
    let item = p
        .find_item_by_property(el.clone(), prop, Variant::from(value))
        .map_err(|e| anyhow!("FindItemByProperty: {e}"))?;
    Ok(element_brief(&item))
}

/// Scroll a scrollable/virtualized item into view through its own provider.
pub fn scroll_item_element(hwnd: HWND, rid: &[i32], align_top: bool) -> Result<String> {
    let el = find_by_rid(hwnd, rid)?;
    let p = el
        .get_pattern::<UIScrollItemPattern>()
        .map_err(|_| anyhow!("element does not expose UIA ScrollItemPattern"))?;
    p.scroll_into_view()
        .map_err(|e| anyhow!("scroll_into_view: {e}"))?;
    Ok(format!(
        "scrolled item into view via UIA ScrollItemPattern (align_to_top={align_top})"
    ))
}

/// Dump a Grid/Table provider as a 2D matrix of cell names (bounded by `max_cells`).
pub fn grid_dump(hwnd: HWND, rid: &[i32], max_cells: usize) -> Result<serde_json::Value> {
    let el = find_by_rid(hwnd, rid)?;
    let g = el
        .get_pattern::<UIGridPattern>()
        .map_err(|_| anyhow!("element does not support UIA GridPattern"))?;
    let rows = g.get_row_count().map_err(|e| anyhow!("row count: {e}"))?;
    let cols = g.get_column_count().map_err(|e| anyhow!("column count: {e}"))?;
    let shown = (max_cells / cols.max(1) as usize).max(1).min(rows.max(0) as usize);
    let mut cells: Vec<Vec<serde_json::Value>> = Vec::new();
    for r in 0..shown as i32 {
        let mut row = Vec::new();
        for c in 0..cols {
            row.push(cell_brief(g.get_item(r, c).ok()));
        }
        cells.push(row);
    }
    Ok(serde_json::json!({
        "rows": rows, "cols": cols, "rows_shown": shown, "cells": cells,
        "truncated": shown < rows.max(0) as usize,
    }))
}

fn cell_brief(el: Option<UIElement>) -> serde_json::Value {
    match el {
        Some(e) => serde_json::json!({
            "name": e.get_name().unwrap_or_default(),
            "type": e.get_control_type().map(type_name).unwrap_or_default(),
            "rect": e.get_bounding_rectangle().map(|r| rect_of(&r)).unwrap_or([0, 0, 0, 0]),
            "value": e.get_pattern::<UIValuePattern>().ok().and_then(|v| v.get_value().ok()),
        }),
        None => serde_json::Value::Null,
    }
}

/// Resolve one (row, col) cell; with `act`, drive it through the normal a11y chain by way of
/// the cell's own runtime id, falling back to its rectangle when the provider gives none.
pub fn grid_cell(
    hwnd: HWND,
    rid: &[i32],
    row: i32,
    col: i32,
    act: bool,
    background: bool,
) -> Result<serde_json::Value> {
    let el = find_by_rid(hwnd, rid)?;
    let g = el
        .get_pattern::<UIGridPattern>()
        .map_err(|_| anyhow!("element does not support UIA GridPattern"))?;
    let rows = g.get_row_count().unwrap_or(0);
    let cols = g.get_column_count().unwrap_or(0);
    if row < 0 || col < 0 || row >= rows || col >= cols {
        bail!("cell ({row},{col}) outside grid {rows}x{cols}");
    }
    let cell = g.get_item(row, col).map_err(|e| anyhow!("get_item({row},{col}): {e}"))?;
    let mut out = cell_brief(Some(cell.clone()));
    if act {
        let cell_rid = cell.get_runtime_id().unwrap_or_default();
        let detail = if !cell_rid.is_empty() {
            if background {
                click_element_background(hwnd, &cell_rid, false, false)?
            } else {
                click_element(hwnd, &cell_rid, false, false)?
            }
        } else {
            let r = rect_of(&cell.get_bounding_rectangle().map_err(|e| anyhow!("rect: {e}"))?);
            if r[2] <= 0 || r[3] <= 0 {
                bail!("cell ({row},{col}) has neither a runtime id nor a usable rectangle");
            }
            let (cx, cy) = (r[0] + r[2] / 2, r[1] + r[3] / 2);
            if background {
                let root = crate::win::root_window(hwnd);
                crate::input::post_click(root, cx, cy, false, false)?;
                format!("cell has no runtime id; clicked ({cx},{cy}) via message-level input")
            } else {
                crate::input::click(cx, cy, false, false)?;
                format!("cell has no runtime id; clicked ({cx},{cy}) at its rectangle")
            }
        };
        out["acted"] = serde_json::json!(detail);
    }
    Ok(out)
}

/// TransformPattern capabilities — tells us whether the app itself allows moving/resizing.
pub fn transform_report(hwnd: HWND, rid: &[i32]) -> Result<serde_json::Value> {
    let el = find_by_rid(hwnd, rid)?;
    let p = el
        .get_pattern::<UITransformPattern>()
        .map_err(|_| anyhow!("element does not support UIA TransformPattern"))?;
    Ok(serde_json::json!({
        "can_move": p.can_move().unwrap_or(false),
        "can_resize": p.can_resize().unwrap_or(false),
        "can_zoom": p.can_zoom().unwrap_or(false),
        "rect": el.get_bounding_rectangle().map(|r| rect_of(&r)).unwrap_or([0, 0, 0, 0]),
    }))
}

/// Move / resize / zoom through the app's own TransformPattern instead of SetWindowPos.
pub fn transform_do(
    hwnd: HWND,
    rid: &[i32],
    mov: Option<(f64, f64)>,
    size: Option<(f64, f64)>,
    zoom: Option<f64>,
) -> Result<String> {
    let el = find_by_rid(hwnd, rid)?;
    let p = el
        .get_pattern::<UITransformPattern>()
        .map_err(|_| anyhow!("element does not support UIA TransformPattern"))?;
    let mut done: Vec<String> = Vec::new();
    if let Some((x, y)) = mov {
        if !p.can_move().unwrap_or(false) {
            bail!("TransformPattern refuses to move this element (CanMove=false)");
        }
        p.move_to(x, y).map_err(|e| anyhow!("transform move: {e}"))?;
        done.push(format!("moved to ({x},{y})"));
    }
    if let Some((w, h)) = size {
        if !p.can_resize().unwrap_or(false) {
            bail!("TransformPattern refuses to resize this element (CanResize=false)");
        }
        p.resize(w, h).map_err(|e| anyhow!("transform resize: {e}"))?;
        done.push(format!("resized to {w}x{h}"));
    }
    if let Some(z) = zoom {
        p.zoom(z).map_err(|e| anyhow!("transform zoom: {e}"))?;
        done.push(format!("zoomed to {z}"));
    }
    if done.is_empty() {
        bail!("nothing to do — pass --move-to / --size / --zoom");
    }
    Ok(format!("{} via UIA TransformPattern", done.join(", ")))
}

/// VirtualizedItemPattern: force a provider to materialise an item that was never realized
/// (virtualised lists otherwise hide their children until this is called).
pub fn realize(hwnd: HWND, rid: &[i32]) -> Result<String> {
    let el = find_by_rid(hwnd, rid)?;
    let p = el
        .get_pattern::<UIVirtualizedItemPattern>()
        .map_err(|_| anyhow!("element does not support UIA VirtualizedItemPattern"))?;
    p.realize().map_err(|e| anyhow!("realize: {e}"))?;
    let name = el.get_name().unwrap_or_default();
    Ok(format!("realized '{}' via UIA VirtualizedItemPattern — re-run state to see its children", name))
}

pub fn toggle_element(hwnd: HWND, rid: &[i32]) -> Result<(String, String)> {
    let el = find_by_rid(hwnd, rid)?;
    let p = el
        .get_pattern::<UITogglePattern>()
        .map_err(|_| anyhow!("element does not support UIA TogglePattern"))?;
    p.toggle().map_err(|e| anyhow!("toggle failed: {e}"))?;
    Ok(("toggled via UIA TogglePattern".into(), toggle_state(&el).unwrap_or_else(|| "?".into())))
}

pub fn select_element(hwnd: HWND, rid: &[i32]) -> Result<(String, bool)> {
    let el = find_by_rid(hwnd, rid)?;
    let p = el
        .get_pattern::<UISelectionItemPattern>()
        .map_err(|_| anyhow!("element does not support UIA SelectionItemPattern"))?;
    p.select().map_err(|e| anyhow!("select failed: {e}"))?;
    let selected = p.is_selected().unwrap_or(true);
    Ok(("selected via UIA SelectionItemPattern".into(), selected))
}

pub fn expand_element(hwnd: HWND, rid: &[i32]) -> Result<(String, String)> {
    let el = find_by_rid(hwnd, rid)?;
    let p = el
        .get_pattern::<UIExpandCollapsePattern>()
        .map_err(|_| anyhow!("element does not support UIA ExpandCollapsePattern"))?;
    p.expand().map_err(|e| anyhow!("expand failed: {e}"))?;
    Ok(("expanded via UIA ExpandCollapsePattern".into(), expand_state(&el).unwrap_or_else(|| "?".into())))
}

pub fn collapse_element(hwnd: HWND, rid: &[i32]) -> Result<(String, String)> {
    let el = find_by_rid(hwnd, rid)?;
    let p = el
        .get_pattern::<UIExpandCollapsePattern>()
        .map_err(|_| anyhow!("element does not support UIA ExpandCollapsePattern"))?;
    p.collapse().map_err(|e| anyhow!("collapse failed: {e}"))?;
    Ok(("collapsed via UIA ExpandCollapsePattern".into(), expand_state(&el).unwrap_or_else(|| "?".into())))
}

pub fn set_range_element(hwnd: HWND, rid: &[i32], value: f64) -> Result<String> {
    let el = find_by_rid(hwnd, rid)?;
    let p = el
        .get_pattern::<UIRangeValuePattern>()
        .map_err(|_| anyhow!("element does not support UIA RangeValuePattern"))?;
    let (min, max) = (
        p.get_minimum().unwrap_or(f64::NAN),
        p.get_maximum().unwrap_or(f64::NAN),
    );
    if value < min || value > max {
        bail!("value {value} outside range [{min}, {max}]");
    }
    p.set_value(value).map_err(|e| anyhow!("range set_value failed: {e}"))?;
    Ok(format!("set range value {value} (min {min}, max {max}) via UIA RangeValuePattern"))
}

pub fn get_range_element(hwnd: HWND, rid: &[i32]) -> Result<serde_json::Value> {
    let el = find_by_rid(hwnd, rid)?;
    let p = el
        .get_pattern::<UIRangeValuePattern>()
        .map_err(|_| anyhow!("element does not support UIA RangeValuePattern"))?;
    Ok(serde_json::json!({
        "value": p.get_value()?,
        "minimum": p.get_minimum()?,
        "maximum": p.get_maximum()?,
        "readonly": p.is_readonly()?,
    }))
}

/// Local grounding (Trae com.bytedance/grounding analog, on-device instead of cloud):
/// resolve a text query to element indices + screen coordinates.
pub struct Grounding {
    pub tree: Tree,
    pub hits: Vec<Node>,
}

/// Grounding walks the same tree that becomes the element cache, so the indices
/// it returns can be used by the next action instead of pointing into a
/// different (older) traversal.
pub fn ground_tree(hwnd: HWND, query: &str) -> Result<Grounding> {
    // deep budget: Electron apps bury content under 15+ container levels
    let tree = walk_window(hwnd, 24, 2000, false, false)?;
    let q = query.to_lowercase();
    let hits: Vec<Node> = tree
        .nodes
        .iter()
        .filter(|n| {
            (!n.name.is_empty() && n.name.to_lowercase().contains(&q))
                || (!n.class.is_empty() && n.class.to_lowercase().contains(&q))
        })
        .cloned()
        .collect();
    Ok(Grounding { tree, hits })
}

/// Hits only (used to re-locate an element by name during a click fallback).
pub fn ground(hwnd: HWND, query: &str) -> Result<Vec<Node>> {
    Ok(ground_tree(hwnd, query)?.hits)
}

pub fn tree_signature(hwnd: HWND) -> Result<String> {
    let t = walk_window(hwnd, 6, 400, false, false)?;
    Ok(t.sig)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(name: &str, aid: &str, class: &str, ctype: &str) -> Locator {
        Locator {
            name: name.into(),
            aid: aid.into(),
            class: class.into(),
            ctype: ctype.into(),
            ..Default::default()
        }
    }

    #[test]
    fn scoring_prefers_exact_strong_keys() {
        let l = loc("Settings", "btn-ok", "Button", "Button");
        let exact = locator_score(&l, "Settings", "Button", "btn-ok", "Button");
        let partial = locator_score(&l, "Settings dialog", "Button", "", "Button");
        let wrong = locator_score(&l, "Other", "Pane", "xx", "Pane");
        assert!(exact > partial && partial > wrong);
        assert_eq!(exact, 220 + 140 + 55 + 26);
    }

    #[test]
    fn scoring_rejects_unrelated_candidates() {
        let l = loc("Settings", "", "", "");
        // name mismatch sinks the candidate even with matching type
        assert!(locator_score(&l, "Other", "", "", "Button") < 0);
    }

    #[test]
    fn name_match_is_case_insensitive_contains() {
        assert!(name_matches("Save All Files", "save all"));
        assert!(name_matches("Settings", "SETTINGS"));
        assert!(!name_matches("", "settings"));
        assert!(!name_matches("Documentation", "settings"));
    }

    #[test]
    fn point_hit_prefers_tab_over_its_strip() {
        let strip = PointHitKey { depth: 3, area: 900, ctype: "Pane".into(), class: "TabStrip".into() };
        let tab = PointHitKey { depth: 4, area: 100, ctype: "TabItem".into(), class: "TabItem".into() };
        assert!(point_hit_key_better(&tab, &strip));
        assert!(!point_hit_key_better(&strip, &tab));
    }

    #[test]
    fn point_hit_prefers_deeper_and_smaller() {
        let mk = |ctype: &str, depth: usize, area: i64| PointHitKey {
            depth,
            area,
            ctype: ctype.into(),
            class: String::new(),
        };
        // deeper wins over shallower
        assert!(point_hit_key_better(&mk("Pane", 5, 100), &mk("Pane", 2, 100)));
        // same depth: smaller area wins
        assert!(point_hit_key_better(&mk("Button", 5, 50), &mk("Pane", 5, 400)));
        // full tie: neither improves
        assert!(!point_hit_key_better(&mk("Pane", 5, 50), &mk("Pane", 5, 50)));
        // a tabstrip container at equal depth is demoted behind a plain control
        let strip = PointHitKey { depth: 5, area: 50, ctype: "Pane".into(), class: "Chrome_TabStrip".into() };
        assert!(point_hit_key_better(&mk("Button", 5, 50), &strip));
    }

    #[test]
    fn tabstrip_classes_are_demoted() {
        assert!(tabstrip_like("Chrome_TabStrip"));
        assert!(tabstrip_like("Tab_Drag_Context"));
        assert!(!tabstrip_like("Button"));
    }
}
