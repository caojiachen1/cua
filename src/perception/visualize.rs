// perception/visualize.rs — turn a screenshot into a human-reviewable
// annotated PNG. Boxes are colored by provenance and labeled with the parsed
// text/caption; the same data is returned as JSON by the CLI.
use std::path::{Path, PathBuf};
use std::time::Instant;

use ab_glyph::{Font, FontVec, PxScale};
use anyhow::{anyhow, Result};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_hollow_rect_mut, draw_text_mut, text_size};
use imageproc::rect::Rect as DrawRect;

use super::fusion::{self, DetItem, OcrItem};
use super::ocr_backend::AnyOcr;
use super::types::{Detail, Element, OcrProfile, Source};
use super::vision::{captions_enabled, Captioner, Detector};

pub struct AnalyzeOutput {
    pub elements: Vec<Element>,
    pub ocr_backend: String,
    pub detector_backend: String,
    pub caption_backend: String,
    pub warnings: Vec<String>,
    pub elapsed_ms: u64,
}

fn crop_rgba(rgba: &[u8], width: u32, height: u32, r: &super::coordinates::Rect) -> (Vec<u8>, u32, u32) {
    let x0 = r.x.floor().clamp(0.0, width as f32) as u32;
    let y0 = r.y.floor().clamp(0.0, height as f32) as u32;
    let x1 = (r.x + r.w).ceil().clamp(0.0, width as f32) as u32;
    let y1 = (r.y + r.h).ceil().clamp(0.0, height as f32) as u32;
    let mut out = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 4) as usize);
    for y in y0..y1 {
        let s = ((y * width + x0) * 4) as usize;
        let e = ((y * width + x1) * 4) as usize;
        out.extend_from_slice(&rgba[s..e]);
    }
    (out, x1 - x0, y1 - y0)
}

pub fn analyze(
    width: u32,
    height: u32,
    rgba: &[u8],
    detail: Detail,
    profile: OcrProfile,
) -> Result<AnalyzeOutput> {
    let started = Instant::now();
    let mut warnings = Vec::new();
    let mut ocr_lines = Vec::new();
    let mut det_items: Vec<DetItem> = Vec::new();
    let mut ocr_backend = String::new();
    let mut detector_backend = String::new();
    let mut caption_backend = String::new();

    // Same shape as the live pipeline: load both, then run both concurrently.
    let (ocr_res, det_res) = std::thread::scope(|scope| {
        let ocr = scope.spawn(|| {
            let mut engine = AnyOcr::load(profile)?;
            let backend = engine.backend().to_string();
            let lines = engine.recognize(width, height, rgba)?;
            Ok::<_, anyhow::Error>((backend, lines))
        });
        let det = scope.spawn(|| {
            let detector = Detector::load()?;
            let backend = detector.backend().to_string();
            let items = detector.detect_tiled(
                width,
                height,
                rgba,
                super::vision::det_tile_budget(detail),
            )?;
            Ok::<_, anyhow::Error>((backend, items))
        });
        (
            ocr.join().unwrap_or_else(|_| Err(anyhow!("OCR worker panicked"))),
            det.join().unwrap_or_else(|_| Err(anyhow!("detector worker panicked"))),
        )
    });
    match ocr_res {
        Ok((backend, lines)) => {
            ocr_backend = backend;
            ocr_lines = lines;
        }
        Err(e) => warnings.push(format!("ocr: {e:#}")),
    }
    match det_res {
        Ok((backend, items)) => {
            detector_backend = backend;
            det_items = items;
        }
        Err(e) => warnings.push(format!("detector: {e:#}")),
    }

    let ocr_items: Vec<OcrItem> = ocr_lines
        .iter()
        .map(|l| OcrItem {
            rect: l.rect,
            text: l.text.clone(),
            confidence: Some(l.confidence),
        })
        .collect();
    let build = fusion::fuse_boxes(&det_items, &ocr_items, 0.7);
    let mut build = build;

    let captions_on = detail.captions() && captions_enabled();
    if captions_on {
        let unlabeled = build.icons.iter().filter(|i| i.text.is_none()).count();
        if unlabeled > 0 {
            let unlabeled = build.icons.iter().filter(|i| i.text.is_none()).count();
            let batch_pref = std::env::var("CUA_FLORENCE_BATCH")
                .map(|v| v != "0")
                .unwrap_or(true);
            match Captioner::load_for(batch_pref && unlabeled >= 8) {
                Ok(mut cap) => {
                    caption_backend = cap.backend_name().to_string();
                    let mut crops = Vec::new();
                    let mut valid = Vec::new();
                    for (idx, icon) in build.icons.iter_mut().enumerate() {
                        if icon.text.is_some() || icon.caption.is_some() {
                            continue;
                        }
                        let r = icon.rect.clamp_to(&super::coordinates::Rect::new(
                            0.0,
                            0.0,
                            width as f32,
                            height as f32,
                        ));
                        if r.w < 2.0 || r.h < 2.0 {
                            icon.caption_status = "failed_crop".into();
                            continue;
                        }
                        let (crop, cw, ch) = crop_rgba(rgba, width, height, &r);
                        crops.push((cw, ch, crop));
                        valid.push(idx);
                    }
                    if !crops.is_empty() {
                        match cap.caption_batch(&crops) {
                            Ok(texts) => {
                                for (slot, text) in texts.into_iter().enumerate() {
                                    let idx = valid[slot];
                                    if text.is_empty() {
                                        build.icons[idx].caption_status = "empty".into();
                                    } else {
                                        build.icons[idx].caption = Some(text);
                                        build.icons[idx].caption_status = "complete".into();
                                    }
                                }
                            }
                            Err(e) => {
                                for idx in valid {
                                    build.icons[idx].caption_status = format!("failed: {e}");
                                }
                            }
                        }
                    }
                }
                Err(e) => warnings.push(format!("caption: {e:#}")),
            }
        }
    } else {
        let status = if detail.captions() { "skipped_disabled" } else { "skipped_fast" };
        for icon in build.icons.iter_mut() {
            if icon.text.is_none() {
                icon.caption_status = status.into();
            }
        }
    }

    let elements = fusion::visual_elements("viz", &build);
    Ok(AnalyzeOutput {
        elements,
        ocr_backend,
        detector_backend,
        caption_backend: if caption_backend.is_empty() {
            if !detail.captions() {
                "skipped_fast".into()
            } else if !captions_enabled() {
                "disabled".into()
            } else {
                "not_loaded".into()
            }
        } else {
            caption_backend
        },
        warnings,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

fn color_for(src: Source) -> Rgba<u8> {
    match src {
        Source::Uia => Rgba([40, 120, 255, 255]),
        Source::Ocr => Rgba([0, 170, 90, 255]),
        Source::Detector => Rgba([255, 145, 0, 255]),
        Source::Caption | Source::Fused => Rgba([200, 60, 200, 255]),
    }
}

fn load_font() -> Option<FontVec> {
    for p in [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(p) {
            if let Ok(font) = FontVec::try_from_vec(bytes) {
                return Some(font);
            }
        }
    }
    None
}

fn label_text(e: &Element, idx: usize) -> String {
    let body = e
        .text
        .as_deref()
        .or(e.caption.as_deref())
        .unwrap_or_else(|| match e.kind.as_str() {
            "text" => "text",
            "icon" => "icon",
            _ => "element",
        });
    let mut s = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > 36 {
        s = s.chars().take(35).collect::<String>() + "…";
    }
    format!("#{idx} {s}")
}

/// Render an annotated PNG. The screenshot stays untouched on the left; each
/// parsed element gets a colored box plus a numbered badge, and a side panel
/// lists #index, provenance and text/caption without covering the UI.
pub fn render(
    width: u32,
    height: u32,
    rgba: &[u8],
    elements: &[Element],
    out: &Path,
) -> Result<(u32, u32)> {
    let shot = RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| anyhow!("frame buffer does not match {}x{}", width, height))?;
    let font = load_font();
    const HEADER: u32 = 46;
    const PANEL: u32 = 380;
    let rows = elements.len().min(80);
    let row_h = 20u32;
    let panel_h = HEADER + 54 + rows as u32 * row_h + 28;
    let canvas_w = width.saturating_add(PANEL);
    let canvas_h = (HEADER + height).max(panel_h);
    let mut img = RgbaImage::from_pixel(canvas_w, canvas_h, Rgba([238, 238, 238, 255]));
    image::imageops::replace(&mut img, &shot, 0, HEADER as i64);

    if let Some(font) = font.as_ref() {
        let counts = elements.iter().fold((0, 0, 0, 0), |mut acc, e| {
            match e.provenance {
                Source::Ocr => acc.0 += 1,
                Source::Detector => acc.1 += 1,
                Source::Fused | Source::Caption => acc.2 += 1,
                Source::Uia => acc.3 += 1,
            }
            acc
        });
        // Header
        draw_filled_rect_mut(&mut img, DrawRect::at(0, 0).of_size(canvas_w, HEADER), Rgba([18, 18, 18, 255]));
        draw_text_mut(&mut img, Rgba([255, 255, 255, 255]), 12, 5, PxScale::from(17.0), font, "cua perception · annotated parse");
        let summary = format!(
            "OCR {}   icons {}   captions {}   UIA {}   total {}",
            counts.0, counts.1, counts.2, counts.3, elements.len()
        );
        draw_text_mut(&mut img, Rgba([210, 210, 210, 255]), 12, 26, PxScale::from(13.0), font, &summary);

        // Side panel
        draw_filled_rect_mut(&mut img, DrawRect::at(width as i32, 0).of_size(PANEL, canvas_h), Rgba([24, 24, 28, 255]));
        draw_filled_rect_mut(&mut img, DrawRect::at(width as i32, 0).of_size(2, canvas_h), Rgba([70, 70, 80, 255]));
        let px = width as i32 + 14;
        draw_text_mut(&mut img, Rgba([255, 255, 255, 255]), px, 10, PxScale::from(15.0), font, "解析结果 / LEGEND");
        draw_text_mut(
            &mut img,
            Rgba([170, 170, 180, 255]),
            px,
            31,
            PxScale::from(11.0),
            font,
            "green=OCR   orange=icon",
        );
        draw_text_mut(
            &mut img,
            Rgba([170, 170, 180, 255]),
            px,
            45,
            PxScale::from(11.0),
            font,
            "purple=caption   blue=UIA",
        );
        let lines: Vec<String> = elements
            .iter()
            .enumerate()
            .take(80)
            .map(|(i, e)| {
                let tag = match e.provenance {
                    Source::Ocr => "OCR",
                    Source::Detector => "icon",
                    Source::Fused | Source::Caption => "cap",
                    Source::Uia => "UIA",
                };
                let body = e
                    .text
                    .as_deref()
                    .or(e.caption.as_deref())
                    .unwrap_or(&e.kind)
                    .split_whitespace().collect::<Vec<_>>().join(" ");
                let mut body = body;
                if body.chars().count() > 40 {
                    body = body.chars().take(39).collect::<String>() + "…";
                }
                format!("#{}  [{}] {}", i + 1, tag, body)
            })
            .collect();
        for (i, line) in lines.iter().enumerate() {
            let y = HEADER as i32 + 52 + i as i32 * row_h as i32;
            let e = &elements[i];
            let c = color_for(e.provenance);
            draw_filled_rect_mut(&mut img, DrawRect::at(px, y + 4).of_size(10, 10), c);
            draw_text_mut(&mut img, Rgba([230, 230, 230, 255]), px + 18, y, PxScale::from(12.5), font, line);
        }
        if elements.len() > 80 {
            draw_text_mut(
                &mut img,
                Rgba([180, 180, 190, 255]),
                px,
                HEADER as i32 + 52 + 80 * row_h as i32,
                PxScale::from(12.0),
                font,
                &format!("+{} more — see JSON", elements.len() - 80),
            );
        }
    }

    // Boxes and small number badges only; the panel carries the full labels.
    for (i, e) in elements.iter().enumerate() {
        let [x, y, w, h] = e.rect_px;
        if w < 1.0 || h < 1.0 {
            continue;
        }
        let x = x.max(0.0).min(width as f32 - 1.0) as i32;
        let y = y.max(0.0).min(height as f32 - 1.0) as i32 + HEADER as i32;
        let w = w.min((width as f32 - (x as f32)).max(1.0)) as u32;
        let h = h.min((height as f32 - (y - HEADER as i32) as f32).max(1.0)) as u32;
        let color = color_for(e.provenance);
        draw_hollow_rect_mut(&mut img, DrawRect::at(x, y).of_size(w.max(1), h.max(1)), color);
        draw_hollow_rect_mut(&mut img, DrawRect::at(x + 1, y + 1).of_size(w.saturating_sub(2).max(1), h.saturating_sub(2).max(1)), Rgba([255, 255, 255, 180]));
        if let Some(font) = font.as_ref() {
            let badge = format!("#{}", i + 1);
            let (tw, th) = text_size(PxScale::from(12.0), font, &badge);
            let bx = x.max(0);
            let by = (y - th as i32 - 2).max(HEADER as i32);
            draw_filled_rect_mut(&mut img, DrawRect::at(bx, by).of_size(tw + 4, th + 2), Rgba([18, 18, 18, 230]));
            draw_text_mut(&mut img, Rgba([255, 255, 255, 255]), bx + 2, by + 1, PxScale::from(12.0), font, &badge);
        }
    }

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    img.save(out).map_err(|e| anyhow!("write {}: {e}", out.display()))?;
    Ok((canvas_w, canvas_h))
}

pub fn default_output(input: Option<&str>) -> PathBuf {
    if let Some(p) = input {
        let p = Path::new(p);
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("frame");
        return p.with_file_name(format!("{stem}-perception.png"));
    }
    crate::util::ensure_data_dir().join(format!("perception-{}.png", crate::util::new_state_id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_is_bounded_and_keeps_an_index() {
        let e = Element {
            element_id: "x".into(),
            kind: "text".into(),
            rect_px: [1.0, 2.0, 3.0, 4.0],
            coordinate_space: "image_px".into(),
            text: Some("a".repeat(80)),
            caption: None,
            provenance: Source::Ocr,
            confidence: Default::default(),
            target: Default::default(),
            uia_index: None,
            uia_rid: None,
            uia_type: None,
            container: None,
            fingerprint: None,
            caption_status: None,
            relations: vec![],
            age_ms: None,
        };
        let s = label_text(&e, 7);
        assert!(s.starts_with("#7 "));
        assert!(s.chars().count() <= 40);
    }
}
