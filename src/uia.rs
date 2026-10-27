// uia.rs — UI Automation tree + patterns (Qoder get_window_state / Trae get_app_state / ZCode getAXState)
// Element-index actions follow the a11y-first rule: index invoke > ValuePattern > coordinate fallback.
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use uiautomation::controls::ControlType;
use uiautomation::patterns::{
    UIExpandCollapsePattern, UIInvokePattern, UILegacyIAccessiblePattern, UIRangeValuePattern,
    UISelectionItemPattern, UITogglePattern, UIValuePattern,
};
use uiautomation::types::{ExpandCollapseState, Handle, Rect, ToggleState};
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
    pub rect: [i32; 4],
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub disabled: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub offscreen: bool,
    pub rid: Vec<i32>,
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
}

fn rect_of(r: &Rect) -> [i32; 4] {
    [r.get_left(), r.get_top(), r.get_width(), r.get_height()]
}

fn type_name(t: ControlType) -> String {
    format!("{t:?}")
}

fn com_init() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    unsafe {
        // ignore S_FALSE / RPC_E_CHANGED_MODE
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

pub fn automation() -> Result<UIAutomation> {
    com_init();
    let a = UIAutomation::new()?;
    Ok(a)
}

pub fn walk_window(
    hwnd: HWND,
    max_depth: usize,
    max_nodes: usize,
    redact: bool,
    with_patterns: bool,
) -> Result<Tree> {
    let automation = automation()?;
    let root = automation
        .element_from_handle(Handle::from(hwnd.0 as isize))
        .map_err(|e| anyhow!("UIA element_from_handle failed: {e} (window may be a non-UIA surface)"))?;
    let walker = automation.get_control_view_walker().map_err(|e| anyhow!("walker: {e}"))?;

    let mut nodes: Vec<Node> = Vec::new();
    let mut sig_parts: Vec<String> = Vec::new();

    // BFS with stack of (element, depth)
    let mut queue: std::collections::VecDeque<(UIElement, usize)> = std::collections::VecDeque::new();
    queue.push_back((root, 0));
    while let Some((el, depth)) = queue.pop_front() {
        if nodes.len() >= max_nodes || depth > max_depth {
            break;
        }
        let ctype = el.get_control_type().map(type_name).unwrap_or_default();
        let mut name = el.get_name().unwrap_or_default();
        let class = el.get_classname().unwrap_or_default();
        let rect = el.get_bounding_rectangle().map(|r| rect_of(&r)).unwrap_or([0, 0, 0, 0]);
        let rid = el.get_runtime_id().unwrap_or_default();
        let disabled = el.is_enabled().map(|b| !b).unwrap_or(false);
        let offscreen = el.is_offscreen().unwrap_or(false);

        // value for editable-ish controls (Trae get_value semantics)
        let mut value = None;
        if matches!(ctype.as_str(), "Edit" | "Document" | "ComboBox" | "Spinner" | "ListItem") {
            if let Ok(vp) = el.get_pattern::<UIValuePattern>() {
                if let Ok(v) = vp.get_value() {
                    if !v.is_empty() {
                        value = Some(if redact { redact_text(&v) } else { v });
                    }
                }
            }
        }
        if redact {
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
        let patterns = if with_patterns && probes_patterns(&ctype) {
            pattern_names(&el)
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
        nodes.push(Node {
            i: nodes.len(),
            ctype,
            name,
            value,
            class,
            rect,
            disabled,
            offscreen,
            rid,
            toggle,
            selected,
            expanded,
            range,
            patterns,
        });

        if let Ok(child) = walker.get_first_child(&el) {
            queue.push_back((child.clone(), depth + 1));
            // enqueue siblings of child
            let mut cur = child;
            loop {
                match walker.get_next_sibling(&cur) {
                    Ok(next) => {
                        queue.push_back((next.clone(), depth + 1));
                        cur = next;
                    }
                    Err(_) => break,
                }
            }
        }
    }

    let joined = sig_parts.join("\u{1}");
    let sig = format!("{:x}", crate::util::fnv1a(joined.as_bytes()));
    Ok(Tree { nodes, sig })
}

fn redact_text(s: &str) -> String {
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

fn pattern_names(el: &UIElement) -> Vec<String> {
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
    let automation = automation()?;
    let root = automation
        .element_from_handle(Handle::from(hwnd.0 as isize))
        .map_err(|e| anyhow!("UIA lost the window: {e}"))?;
    if element_matches(&root, rid) {
        return Ok(root);
    }
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
    bail!("element rid {rid:?} not found — UI tree changed, re-run 'cua state' (Qoder screenshotId-style staleness guard)")
}

fn element_matches(el: &UIElement, rid: &[i32]) -> bool {
    el.get_runtime_id().map(|r| r == rid).unwrap_or(false)
}

/// a11y-first click: Invoke > Toggle > SelectionItem > Legacy default action > coordinates.
/// Right/double clicks always use real coordinate events (patterns have no such semantics).
pub fn click_element(hwnd: HWND, rid: &[i32], double: bool, right: bool) -> Result<String> {
    let el = find_by_rid(hwnd, rid)?;
    if double || right {
        return click_element_rect(&el, double, right);
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
        p.do_default_action().map_err(|e| anyhow!("default action failed: {e}"))?;
        return Ok("invoked element via LegacyIAccessible DoDefaultAction".into());
    }
    click_element_rect(&el, double, right)
}

fn click_element_rect(el: &UIElement, double: bool, right: bool) -> Result<String> {
    let r = el.get_bounding_rectangle().map_err(|e| anyhow!("rect: {e}"))?;
    let rect = rect_of(&r);
    if rect[2] <= 0 || rect[3] <= 0 {
        bail!("element has no actionable pattern and no usable rect");
    }
    let cx = rect[0] + rect[2] / 2;
    let cy = rect[1] + rect[3] / 2;
    crate::input::click(cx, cy, right, double)?;
    Ok(format!(
        "clicked element at ({cx},{cy}) via event strategy{}{} (no pattern for this gesture)",
        if right { " right" } else { "" },
        if double { " double" } else { "" }
    ))
}

/// a11y-only click: refuses coordinate fallback (ZCode strategy=a11y).
pub fn click_element_strict(hwnd: HWND, rid: &[i32], double: bool, right: bool) -> Result<String> {
    if double || right {
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
pub fn ground(hwnd: HWND, query: &str) -> Result<Vec<Node>> {
    let tree = walk_window(hwnd, 14, 2000, false, false)?;
    let q = query.to_lowercase();
    let hits: Vec<Node> = tree
        .nodes
        .into_iter()
        .filter(|n| {
            (!n.name.is_empty() && n.name.to_lowercase().contains(&q))
                || (!n.class.is_empty() && n.class.to_lowercase().contains(&q))
        })
        .collect();
    Ok(hits)
}

pub fn tree_signature(hwnd: HWND) -> Result<String> {
    let t = walk_window(hwnd, 6, 400, false, false)?;
    Ok(t.sig)
}
