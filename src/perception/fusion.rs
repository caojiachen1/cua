// perception/fusion.rs — the two-layer model from the plan:
//   1. strict OmniParser box fusion (remove_overlap_new semantics),
//   2. cua's provenance/identity layer (UIA + visual, immutable snapshot ids).
use super::coordinates::Rect;
use super::quality::VisualCandidate;
use super::text::normalize;
use super::types::{Confidence, Element, Relation, Source, TargetInfo};
use crate::uia::Node;

#[derive(Clone, Debug)]
pub struct OcrItem {
    pub rect: Rect,
    pub text: String,
    pub confidence: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct DetItem {
    pub rect: Rect,
    pub score: f32,
}

#[derive(Clone, Debug, Default)]
pub struct VisualBuild {
    /// icons with OCR text merged in (content first, then unlabeled icons)
    pub icons: Vec<FusedIcon>,
    /// standalone OCR text boxes that are not inside an icon
    pub texts: Vec<OcrItem>,
    pub coverage: Vec<VisualCandidate>,
}

#[derive(Clone, Debug)]
pub struct FusedIcon {
    pub rect: Rect,
    pub score: f32,
    pub text: Option<String>,
    pub ocr_confidence: Option<f32>,
    pub caption: Option<String>,
    pub caption_status: String,
}

fn icon_elems(dets: &[DetItem]) -> Vec<FusedIcon> {
    dets.iter()
        .map(|d| FusedIcon {
            rect: d.rect,
            score: d.score,
            text: None,
            ocr_confidence: None,
            caption: None,
            caption_status: "pending".into(),
        })
        .collect()
}

/// OmniParser's remove_overlap_new:
///  - overlap = max(IoU, inter/A, inter/B); if > threshold drop the larger box
///  - OCR inside an icon (intersection/ocr_area > 0.8) is merged into its
///    content and removed from the standalone text list
///  - an icon inside an OCR box is dropped entirely
///  - content-bearing icons sort before unlabeled icons
pub fn fuse_boxes(dets: &[DetItem], ocr: &[OcrItem], overlap_threshold: f32) -> VisualBuild {
    let icons = icon_elems(dets);
    let mut drop_icon = vec![false; icons.len()];
    // Pass 1: overlapping duplicates keep the smaller, tighter box.
    for i in 0..icons.len() {
        for j in 0..icons.len() {
            if i == j {
                continue;
            }
            if icons[i].rect.overlap(&icons[j].rect) > overlap_threshold
                && icons[i].rect.area() > icons[j].rect.area()
            {
                drop_icon[i] = true;
                break;
            }
        }
    }
    // Pass 2: an icon sitting inside a larger OCR line is the line's content.
    for (i, icon) in icons.iter().enumerate() {
        if !drop_icon[i] && ocr.iter().any(|t| t.rect.contains_ratio(&icon.rect) > 0.80) {
            drop_icon[i] = true;
        }
    }
    // Pass 3: attach labels without consuming the source list, so an icon that
    // is dropped later can never take its OCR text down with it.
    let mut taken = vec![false; ocr.len()];
    let mut kept: Vec<FusedIcon> = Vec::new();
    for (i, icon) in icons.iter().enumerate() {
        if drop_icon[i] {
            continue;
        }
        let mut fi = icon.clone();
        let mut labels: Vec<String> = Vec::new();
        let mut confidence = None;
        for (k, t) in ocr.iter().enumerate() {
            if taken[k] || icon.rect.contains_ratio(&t.rect) <= 0.80 {
                continue;
            }
            taken[k] = true;
            labels.push(t.text.clone());
            if confidence.is_none() {
                confidence = t.confidence;
            }
        }
        if !labels.is_empty() {
            fi.text = Some(normalize(&labels.join(" ")));
            fi.ocr_confidence = confidence;
            fi.caption_status = "not_needed".into();
        }
        kept.push(fi);
    }
    let texts: Vec<OcrItem> = ocr
        .iter()
        .enumerate()
        .filter(|(k, _)| !taken[*k])
        .map(|(_, t)| t.clone())
        .collect();
    // Stable sort: content first, geometry as the deterministic tie-break.
    kept.sort_by(|a, b| {
        let ac = a.text.is_none() as u8;
        let bc = b.text.is_none() as u8;
        ac.cmp(&bc)
            .then_with(|| a.rect.y.partial_cmp(&b.rect.y).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.rect.x.partial_cmp(&b.rect.x).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut coverage: Vec<VisualCandidate> = Vec::new();
    for t in &texts {
        coverage.push(VisualCandidate { rect: t.rect, text: Some(t.text.clone()) });
    }
    for i in &kept {
        coverage.push(VisualCandidate { rect: i.rect, text: i.text.clone() });
    }
    VisualBuild { icons: kept, texts, coverage }
}

/// Convert a UIA node into an immutable element. Only content-bearing leaves
/// and interactive controls are published; huge containers are not evidence.
pub fn uia_element(n: &Node, frame_id: &str, origin: [i32; 2], scale: f32) -> Option<Element> {
    let name = n.name.trim();
    let has_value = n.value.as_deref().map(|v| !v.trim().is_empty()).unwrap_or(false);
    if name.is_empty() && !has_value && n.patterns.is_empty() {
        return None;
    }
    if n.offscreen || n.rect.iter().all(|v| *v == 0) {
        return None;
    }
    let mut channels = Vec::new();
    for p in &n.patterns {
        match p.as_str() {
            "Invoke" => channels.push("uia.invoke".to_string()),
            "Value" => channels.push("uia.value".to_string()),
            "Toggle" => channels.push("uia.toggle".to_string()),
            "SelectionItem" => channels.push("uia.select".to_string()),
            "ExpandCollapse" => channels.push("uia.expand_collapse".to_string()),
            "RangeValue" => channels.push("uia.range_value".to_string()),
            "ScrollItem" => channels.push("uia.scroll_item".to_string()),
            _ => {}
        }
    }
    Some(Element {
        element_id: format!("uia:{frame_id}:{}", n.i),
        kind: "uia".into(),
        rect_px: [
            (n.rect[0] - origin[0]) as f32 / scale,
            (n.rect[1] - origin[1]) as f32 / scale,
            n.rect[2] as f32 / scale,
            n.rect[3] as f32 / scale,
        ],
        coordinate_space: "image_px".into(),
        text: if has_value { n.value.clone() } else if !name.is_empty() { Some(name.to_string()) } else { None },
        caption: None,
        provenance: Source::Uia,
        confidence: Confidence {
            uia: Some(1.0),
            ocr: None,
            detector: None,
            caption: None,
            actionability: if channels.is_empty() { None } else { Some(1.0) },
        },
        target: TargetInfo { channels },
        uia_index: Some(n.i),
        uia_rid: if n.rid.is_empty() { None } else { Some(n.rid.clone()) },
        uia_type: Some(n.ctype.clone()),
        caption_status: None,
        relations: Vec::new(),
        age_ms: None,
    })
}

/// Build visual elements from a fused visual result and optional captions.
/// Captions are aligned by icon index; a missing/slow caption leaves the icon
/// unlabeled rather than inventing semantics.
pub fn visual_elements(frame_id: &str, build: &VisualBuild) -> Vec<Element> {
    let mut out = Vec::new();
    for (i, t) in build.texts.iter().enumerate() {
        out.push(Element {
            element_id: format!("vision:{frame_id}:t{i}"),
            kind: "text".into(),
            rect_px: t.rect.to_array(),
            coordinate_space: "image_px".into(),
            text: Some(t.text.clone()),
            caption: None,
            provenance: Source::Ocr,
            confidence: Confidence {
                uia: None,
                ocr: t.confidence,
                detector: None,
                caption: None,
                actionability: None,
            },
            target: TargetInfo::unknown(),
            uia_index: None,
            uia_rid: None,
            uia_type: None,
            caption_status: None,
            relations: Vec::new(),
            age_ms: None,
        });
    }
    for (i, icon) in build.icons.iter().enumerate() {
        let mut relations = Vec::new();
        // Attach OCR texts geometrically contained in the icon.
        for (j, t) in build.texts.iter().enumerate() {
            if icon.rect.contains_ratio(&t.rect) > 0.6 {
                relations.push(Relation {
                    kind: "contains_text".into(),
                    element_id: format!("vision:{frame_id}:t{j}"),
                });
            }
        }
        out.push(Element {
            element_id: format!("vision:{frame_id}:i{i}"),
            kind: "icon".into(),
            rect_px: icon.rect.to_array(),
            coordinate_space: "image_px".into(),
            text: icon.text.clone(),
            caption: icon.caption.clone(),
            provenance: if icon.caption.is_some() { Source::Fused } else { Source::Detector },
            confidence: Confidence {
                uia: None,
                ocr: icon.ocr_confidence,
                detector: Some(icon.score),
                caption: None,
                actionability: None,
            },
            target: TargetInfo::unknown(),
            uia_index: None,
            uia_rid: None,
            uia_type: None,
            caption_status: Some(icon.caption_status.clone()),
            relations,
            age_ms: None,
        });
    }
    // Reading order: top-to-bottom bands, then left-to-right.
    out.sort_by(|a, b| {
        let ay = a.rect_px[1];
        let by = b.rect_px[1];
        let band = 8.0;
        (ay / band).round().partial_cmp(&(by / band).round()).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.rect_px[0].partial_cmp(&b.rect_px[0]).unwrap_or(std::cmp::Ordering::Equal))
    });
    out
}

/// Merge UIA and visual elements into one list, keeping both sources and
/// marking visual elements that are already covered by a named UIA leaf.
pub fn combine(uia: Vec<Element>, visual: Vec<Element>) -> Vec<Element> {
    let mut out = uia;
    for v in visual {
        let vr = Rect::new(v.rect_px[0], v.rect_px[1], v.rect_px[2], v.rect_px[3]);
        if let Some(u) = out.iter_mut().find(|u| {
            u.provenance == Source::Uia
                && u.text.is_some()
                && Rect::new(u.rect_px[0], u.rect_px[1], u.rect_px[2], u.rect_px[3]).iou(&vr) > 0.4
        }) {
            // The UIA leaf wins the geometry, but a Florence caption is still
            // evidence: keep it instead of discarding the visual element whole.
            if u.caption.is_none() {
                u.caption = v.caption.clone();
            }
            continue;
        }
        out.push(v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ocr(r: [f32; 4], t: &str) -> OcrItem {
        OcrItem { rect: Rect::new(r[0], r[1], r[2], r[3]), text: t.into(), confidence: Some(0.9) }
    }

    #[test]
    fn ocr_inside_icon_becomes_content() {
        let det = vec![DetItem { rect: Rect::new(0.0, 0.0, 100.0, 40.0), score: 0.9 }];
        let texts = vec![ocr([10.0, 10.0, 40.0, 12.0], "Save")];
        let b = fuse_boxes(&det, &texts, 0.7);
        assert_eq!(b.icons.len(), 1);
        assert_eq!(b.icons[0].text.as_deref(), Some("Save"));
        assert!(b.texts.is_empty());
    }

    #[test]
    fn icon_inside_ocr_is_dropped() {
        let det = vec![DetItem { rect: Rect::new(10.0, 10.0, 20.0, 20.0), score: 0.9 }];
        let texts = vec![ocr([0.0, 0.0, 200.0, 100.0], "A long line")];
        let b = fuse_boxes(&det, &texts, 0.7);
        assert!(b.icons.is_empty());
        assert_eq!(b.texts.len(), 1);
    }

    #[test]
    fn dropped_icon_keeps_its_text() {
        // The icon sits inside a big OCR line (so it is dropped) and contains a
        // smaller label of its own: the label must survive as standalone text.
        let det = vec![DetItem { rect: Rect::new(10.0, 10.0, 20.0, 20.0), score: 0.9 }];
        let texts = vec![
            ocr([0.0, 0.0, 200.0, 100.0], "Header"),
            ocr([12.0, 12.0, 12.0, 8.0], "Tag"),
        ];
        let b = fuse_boxes(&det, &texts, 0.7);
        assert!(b.icons.is_empty());
        let names: Vec<&str> = b.texts.iter().map(|t| t.text.as_str()).collect();
        assert!(names.contains(&"Header"), "lost the containing line: {names:?}");
        assert!(names.contains(&"Tag"), "lost the icon label: {names:?}");
    }

    #[test]
    fn larger_duplicate_icon_is_removed() {
        let det = vec![
            DetItem { rect: Rect::new(0.0, 0.0, 100.0, 100.0), score: 0.9 },
            DetItem { rect: Rect::new(5.0, 5.0, 80.0, 80.0), score: 0.8 },
        ];
        let b = fuse_boxes(&det, &[], 0.7);
        assert_eq!(b.icons.len(), 1);
        assert_eq!(b.icons[0].rect.w, 80.0);
    }
}
