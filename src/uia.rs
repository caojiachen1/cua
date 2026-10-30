// uia.rs — UI Automation tree + patterns (Qoder get_window_state / Trae get_app_state / ZCode getAXState)
// Element-index actions follow the a11y-first rule: index invoke > ValuePattern > coordinate fallback.
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use uiautomation::controls::ControlType;
use uiautomation::patterns::{
    UIExpandCollapsePattern, UIGridPattern, UIInvokePattern, UILegacyIAccessiblePattern,
    UIItemContainerPattern, UIRangeValuePattern, UIScrollItemPattern, UISelectionItemPattern,
    UITableItemPattern, UITablePattern, UITextPattern, UITogglePattern, UITransformPattern,
    UIValuePattern, UIVirtualizedItemPattern,
};
use uiautomation::types::{ExpandCollapseState, Handle, Rect, ToggleState, UIProperty};
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
    /// the walk hit its depth or node budget before the tree was exhausted
    pub truncated: bool,
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
    let mut truncated = false;

    // BFS with stack of (element, depth)
    let mut queue: std::collections::VecDeque<(UIElement, usize)> = std::collections::VecDeque::new();
    queue.push_back((root, 0));
    while let Some((el, depth)) = queue.pop_front() {
        if nodes.len() >= max_nodes || depth > max_depth {
            // This element was already popped and will never be processed, so the
            // walk is incomplete even when no siblings remain queued behind it.
            truncated = true;
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
    Ok(Tree { nodes, sig, truncated })
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

pub fn nodes_json(nodes: &[Node]) -> Vec<serde_json::Value> {
    nodes
        .iter()
        .map(|n| {
            serde_json::json!({
                "i": n.i, "type": n.ctype, "name": n.name, "class": n.class,
                "rect": n.rect, "rid": n.rid, "disabled": n.disabled, "offscreen": n.offscreen,
                "value": n.value, "toggle": n.toggle, "selected": n.selected,
                "expanded": n.expanded, "range": n.range, "patterns": n.patterns,
            })
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
    let tree = walk_window(hwnd, 14, 2000, false, false)?;
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
