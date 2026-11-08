// perception/quality.rs — per-region evidence, never a single "UIA score".
// Reading, locating, acting, coverage and capture are kept separate so a high
// OCR/name count cannot compensate for missing capabilities or bad pixels.
use std::collections::HashMap;

use crate::uia::Node;

use super::coordinates::Rect;
use super::text::normalize;
use super::types::RegionQuality;

#[derive(Clone, Copy, Debug, Default)]
pub struct CaptureFacts {
    pub ok: bool,
    pub rendered: bool,
    pub fresh_known: bool,
}

#[derive(Clone, Debug)]
pub struct VisualCandidate {
    pub rect: Rect,
    pub text: Option<String>,
}

fn is_container(ctype: &str) -> bool {
    matches!(
        ctype,
        "Pane" | "Group" | "Custom" | "Window" | "Document" | "List" | "Table" | "DataGrid" | "Tree" | "Menu" | "MenuBar"
    )
}

fn is_interactive(ctype: &str) -> bool {
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
            | "Slider"
            | "Spinner"
            | "SplitButton"
            | "TabItem"
            | "TreeItem"
            | "DataItem"
            | "ScrollBar"
    )
}

fn node_content(n: &Node) -> bool {
    !n.offscreen
        && !n.rect.iter().all(|v| *v == 0)
        && (!n.name.trim().is_empty()
            || n.value.as_deref().map(|v| !v.trim().is_empty()).unwrap_or(false)
            || is_interactive(&n.ctype))
}

fn text_overlaps(a: &str, b: &str) -> bool {
    let a = normalize(a).to_lowercase();
    let b = normalize(b).to_lowercase();
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.contains(&b) || b.contains(&a)
}

fn candidate_matches_node(v: &VisualCandidate, n: &Node, origin: [i32; 2], scale: f32) -> bool {
    // UIA rects are physical screen pixels; frame rects are image pixels. On a
    // DPI-virtualized window those differ by `scale`, so both factors apply.
    let nr = Rect::new(
        (n.rect[0] - origin[0]) as f32 / scale,
        (n.rect[1] - origin[1]) as f32 / scale,
        n.rect[2] as f32 / scale,
        n.rect[3] as f32 / scale,
    );
    if nr.is_empty() {
        return false;
    }
    let geometric = v.rect.iou(&nr) >= 0.25
        || nr.contains_ratio(&v.rect) >= 0.55
        || v.rect.contains_ratio(&nr) >= 0.55;
    if let Some(text) = &v.text {
        let node_text = n
            .value
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&n.name);
        if text_overlaps(text, node_text) && geometric {
            return true;
        }
    }
    geometric && (is_interactive(&n.ctype) || !n.name.trim().is_empty())
}

/// Compute whole-window quality from a UIA walk and optional visual candidates.
/// `coverage` is None until a visual baseline exists, which is deliberately
/// different from 0 (unknown vs known-missing).
pub fn evaluate(
    nodes: &[Node],
    truncated: bool,
    patterns_sampled: bool,
    capture: CaptureFacts,
    visual: &[VisualCandidate],
    origin: [i32; 2],
    scale: f32,
) -> RegionQuality {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let mut child_count: HashMap<usize, usize> = HashMap::new();
    for n in nodes {
        if let Some(pa) = n.pa {
            *child_count.entry(pa).or_default() += 1;
        }
    }
    let leaves: Vec<&Node> = nodes
        .iter()
        .filter(|n| child_count.get(&n.i).copied().unwrap_or(0) == 0)
        .filter(|n| !n.offscreen)
        .collect();
    let content: Vec<&Node> = leaves.iter().copied().filter(|n| node_content(n)).collect();

    let named = content
        .iter()
        .filter(|n| {
            !n.name.trim().is_empty()
                || n.value.as_deref().map(|v| !v.trim().is_empty()).unwrap_or(false)
        })
        .count();
    let locatable = content
        .iter()
        .filter(|n| {
            !n.rid.is_empty()
                || !n.name.trim().is_empty()
                || !n.aid.trim().is_empty()
                || !n.class.trim().is_empty()
        })
        .count();
    let patterned = content.iter().filter(|n| !n.patterns.is_empty()).count();
    let total = content.len().max(1) as f32;
    let mut notes = Vec::new();
    if truncated {
        notes.push("uia_tree_truncated".to_string());
    }
    if nodes.is_empty() {
        notes.push("uia_empty".to_string());
    }
    if !patterns_sampled {
        notes.push("patterns_not_sampled".to_string());
    }
    if !capture.ok {
        notes.push("capture_unavailable".to_string());
    }
    if !capture.rendered && capture.ok {
        notes.push("capture_not_rendered".to_string());
    }
    if !capture.fresh_known {
        notes.push("content_freshness_unknown".to_string());
    }
    let coverage = if visual.is_empty() {
        None
    } else {
        let matched = visual
            .iter()
            .filter(|v| {
                content.iter().any(|n| candidate_matches_node(v, n, origin, scale))
            })
            .count();
        Some(matched as f32 / visual.len() as f32)
    };
    let actionable_known = patterns_sampled && !content.is_empty();
    let actionable = if actionable_known { patterned as f32 / total } else { 0.0 };
    RegionQuality {
        rect_px: nodes
            .iter()
            .filter(|n| !n.offscreen)
            .fold(None::<Rect>, |acc, n| {
                let r = Rect::new(
                    (n.rect[0] - origin[0]) as f32 / scale,
                    (n.rect[1] - origin[1]) as f32 / scale,
                    n.rect[2] as f32 / scale,
                    n.rect[3] as f32 / scale,
                );
                if r.is_empty() {
                    acc
                } else {
                    Some(acc.map(|a| a.union(&r)).unwrap_or(r))
                }
            })
            .map(|r| r.to_array())
            .unwrap_or([0.0, 0.0, 0.0, 0.0]),
        state: "unknown".to_string(),
        coverage,
        readable: named as f32 / total,
        locatable: locatable as f32 / total,
        actionable,
        actionable_known,
        leaf_count: content.len(),
        named_leaves: named,
        capture_ok: capture.ok && capture.rendered,
        truncated,
        notes,
    }
}

/// Convenience for the special "no provider at all" case: no leaves, no
/// coverage, capture may still be good. The router treats this as a visual
/// candidate, not as a zero score.
pub fn empty(capture: CaptureFacts) -> RegionQuality {
    evaluate(&[], false, false, capture, &[], [0, 0], 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(i: usize, name: &str, ctype: &str, rect: [i32; 4], patterns: &[&str]) -> Node {
        Node {
            i,
            ctype: ctype.into(),
            name: name.into(),
            value: None,
            class: String::new(),
            aid: String::new(),
            rect,
            disabled: false,
            offscreen: false,
            ctl: true,
            rid: vec![1, i as i32],
            pa: None,
            ri: i as u32,
            toggle: None,
            selected: None,
            expanded: None,
            range: None,
            patterns: patterns.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn unknown_coverage_is_not_zero() {
        let nodes = vec![node(1, "Save", "Button", [10, 10, 80, 30], &[])];
        let q = evaluate(&nodes, false, true, CaptureFacts { ok: true, rendered: true, fresh_known: true }, &[], [0, 0], 1.0);
        assert_eq!(q.coverage, None);
        assert!(q.readable > 0.0);
    }

    #[test]
    fn container_panes_do_not_count_as_content() {
        let nodes = vec![
            node(0, "", "Pane", [0, 0, 1000, 800], &[]),
            node(1, "OK", "Button", [10, 10, 80, 30], &["Invoke"]),
        ];
        let mut a = nodes[1].clone();
        a.pa = Some(0);
        let q = evaluate(&[nodes[0].clone(), a], false, true, CaptureFacts { ok: true, rendered: true, fresh_known: true }, &[], [0, 0], 1.0);
        assert_eq!(q.leaf_count, 1);
        assert!((q.actionable - 1.0).abs() < 1e-5);
    }

    #[test]
    fn coverage_respects_dpi_scale() {
        // Window is DPI-virtualized: frame pixels are screen pixels / 1.5.
        let nodes = vec![node(1, "Save", "Button", [150, 150, 120, 45], &["Invoke"])];
        let vis = vec![VisualCandidate {
            rect: Rect::new(100.0, 100.0, 80.0, 30.0),
            text: Some("Save".into()),
        }];
        let scaled = evaluate(
            &nodes,
            false,
            true,
            CaptureFacts { ok: true, rendered: true, fresh_known: true },
            &vis,
            [0, 0],
            1.5,
        );
        assert_eq!(scaled.coverage, Some(1.0));
        let unscaled = evaluate(
            &nodes,
            false,
            true,
            CaptureFacts { ok: true, rendered: true, fresh_known: true },
            &vis,
            [0, 0],
            1.0,
        );
        assert_eq!(unscaled.coverage, Some(0.0));
    }

    #[test]
    fn coverage_uses_visual_candidates() {
        let nodes = vec![node(1, "Save", "Button", [10, 10, 80, 30], &[])];
        let vis = vec![
            VisualCandidate { rect: Rect::new(10.0, 10.0, 80.0, 30.0), text: Some("Save".into()) },
            VisualCandidate { rect: Rect::new(300.0, 300.0, 40.0, 20.0), text: Some("Canvas".into()) },
        ];
        let q = evaluate(&nodes, false, false, CaptureFacts { ok: true, rendered: true, fresh_known: true }, &vis, [0, 0], 1.0);
        assert!((q.coverage.unwrap() - 0.5).abs() < 1e-5);
    }
}
