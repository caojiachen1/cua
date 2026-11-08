// perception/easyocr.rs — Rust port of the official OmniParser EasyOCR
// reference path: CRAFT detector + english_g2 CTC recognizer. The networks are
// official EasyOCR weights exported to ONNX and run through ONNX Runtime.
use anyhow::{anyhow, bail, Context, Result};
use ort::session::Session;

use super::coordinates::Rect;
use super::ocr::TextLine;

pub struct EasyOcr {
    craft: Session,
    rec: Session,
    chars: Vec<String>,
    backend: &'static str,
}

fn load_session(path: &std::path::Path) -> Result<Session> {
    Session::builder()
        .map_err(|e| anyhow!("EasyOCR session builder: {e}"))?
        .commit_from_file(path)
        .map_err(|e| anyhow!("EasyOCR load {}: {e}", path.display()))
}

impl EasyOcr {
    pub fn load(dir: &std::path::Path) -> Result<Self> {
        let craft_path = dir.join("craft_mlt_25k.onnx");
        let rec_path = dir.join("english_g2.onnx");
        let chars_path = dir.join("characters.json");
        for p in [&craft_path, &rec_path, &chars_path] {
            if !p.is_file() {
                bail!("EasyOCR asset missing: {}", p.display());
            }
        }
        let chars: Vec<String> = serde_json::from_slice(
            &std::fs::read(&chars_path).with_context(|| format!("read {}", chars_path.display()))?,
        )
        .context("EasyOCR characters.json")?;
        if chars.len() < 2 {
            bail!("EasyOCR character table is empty");
        }
        Ok(Self {
            craft: load_session(&craft_path)?,
            rec: load_session(&rec_path)?,
            chars,
            backend: "easyocr-craft-en/ort-cpu",
        })
    }

    pub fn backend(&self) -> &'static str {
        self.backend
    }

    pub fn recognize(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<Vec<TextLine>> {
        if width < 8 || height < 8 || rgba.len() < (width * height * 4) as usize {
            bail!("invalid image for EasyOCR");
        }
        // EasyOCR resize_aspect_ratio(canvas_size=2560, mag_ratio=1): keep the
        // original size below the canvas, otherwise scale the long side to it.
        let max_side = 2560.0f32;
        let scale = (max_side / width.max(height) as f32).min(1.0);
        let sw = (width as f32 * scale).round().max(1.0) as u32;
        let sh = (height as f32 * scale).round().max(1.0) as u32;
        let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())
            .ok_or_else(|| anyhow!("EasyOCR RGBA size mismatch"))?;
        let resized = image::imageops::resize(&img, sw, sh, image::imageops::FilterType::Triangle);
        let pw = (sw + 31) / 32 * 32;
        let ph = (sh + 31) / 32 * 32;
        let n = (pw * ph) as usize;
        let mut data = vec![0f32; 3 * n];
        let mean = [0.485f32, 0.456, 0.406];
        let std = [0.229f32, 0.224, 0.225];
        for y in 0..sh as usize {
            for x in 0..sw as usize {
                let p = resized.get_pixel(x as u32, y as u32).0;
                let idx = y * pw as usize + x;
                for c in 0..3 {
                    data[c * n + idx] = (p[c] as f32 - mean[c] * 255.0) / (std[c] * 255.0);
                }
            }
        }
        let input = ort::value::Tensor::from_array((
            [1usize, 3, ph as usize, pw as usize],
            data,
        ))
        .map_err(|e| anyhow!("EasyOCR CRAFT input: {e}"))?;
        let outputs = self
            .craft
            .run(ort::inputs![input])
            .map_err(|e| anyhow!("EasyOCR CRAFT run: {e}"))?;
        let (shape, scores) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| anyhow!("EasyOCR CRAFT output: {e}"))?;
        let dims = shape.iter().map(|v| *v as usize).collect::<Vec<_>>();
        if dims.len() != 4 || dims[1] != 2 {
            bail!("unexpected EasyOCR CRAFT shape {dims:?}");
        }
        let (hh, ww) = (dims[2], dims[3]);
        let textmap = scores[0..hh * ww].to_vec();
        let linkmap = scores[hh * ww..2 * hh * ww].to_vec();
        drop(outputs);
        let boxes = group_boxes(craft_boxes(&textmap, &linkmap, hh, ww, sw, sh, scale));
        let mut out = Vec::new();
        for r in boxes {
            if let Ok((text, conf)) = self.recognize_crop(width, height, rgba, &r) {
                if !text.is_empty() {
                    out.push(TextLine { rect: r, text, confidence: conf });
                }
            }
        }
        Ok(out)
    }

    fn recognize_crop(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
        rect: &Rect,
    ) -> Result<(String, f32)> {
        let r = rect.clamp_to(&Rect::new(0.0, 0.0, width as f32, height as f32));
        if r.w < 2.0 || r.h < 2.0 {
            return Ok((String::new(), 0.0));
        }
        let x0 = r.x.floor() as u32;
        let y0 = r.y.floor() as u32;
        let x1 = (r.x + r.w).ceil().min(width as f32) as u32;
        let y1 = (r.y + r.h).ceil().min(height as f32) as u32;
        let cw = (x1 - x0).max(1);
        let ch = (y1 - y0).max(1);
        let mut crop = image::GrayImage::new(cw, ch);
        for y in 0..ch {
            for x in 0..cw {
                let i = (((y0 + y) * width + x0 + x) * 4) as usize;
                let lum = ((rgba[i] as u32 * 299 + rgba[i + 1] as u32 * 587 + rgba[i + 2] as u32 * 114) / 1000) as u8;
                crop.put_pixel(x, y, image::Luma([lum]));
            }
        }
        let rw = ((64.0 * cw as f32 / ch as f32).ceil() as u32).clamp(1, 400);
        let resized = image::imageops::resize(&crop, rw, 64, image::imageops::FilterType::CatmullRom);
        let mut data = vec![0f32; 64 * 400];
        let last = resized.get_pixel(rw - 1, 32).0[0] as f32;
        // EasyOCR NormalizePAD fills the remaining width with the last column.
        let last_norm = (last / 255.0 - 0.5) / 0.5;
        for val in data.iter_mut() {
            *val = last_norm;
        }
        for y in 0..64 {
            for x in 0..rw {
                data[(y * 400 + x) as usize] =
                    (resized.get_pixel(x, y).0[0] as f32 / 255.0 - 0.5) / 0.5;
            }
        }
        let input = ort::value::Tensor::from_array(([1usize, 1, 64, 400], data))
            .map_err(|e| anyhow!("EasyOCR rec input: {e}"))?;
        let outputs = self
            .rec
            .run(ort::inputs![input])
            .map_err(|e| anyhow!("EasyOCR rec run: {e}"))?;
        let (shape, logits) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| anyhow!("EasyOCR rec output: {e}"))?;
        let dims = shape.iter().map(|v| *v as usize).collect::<Vec<_>>();
        if dims.len() != 3 {
            bail!("unexpected EasyOCR rec shape {dims:?}");
        }
        let (steps, classes) = (dims[1], dims[2]);
        let mut text = String::new();
        let mut last = usize::MAX;
        let mut probs = Vec::new();
        for t in 0..steps {
            let row = &logits[t * classes..(t + 1) * classes];
            let mut best = 0usize;
            let mut best_v = f32::NEG_INFINITY;
            for (i, v) in row.iter().enumerate() {
                if *v > best_v {
                    best_v = *v;
                    best = i;
                }
            }
            if best != last && best != 0 {
                if let Some(ch) = self.chars.get(best) {
                    text.push_str(ch);
                    // Log-sum-exp stable softmax for a meaningful confidence.
                    let max = best_v;
                    let sum: f32 = row.iter().map(|v| (v - max).exp()).sum();
                    probs.push(1.0 / sum.max(1e-9));
                }
            }
            last = best;
        }
        let text = text.trim().to_string();
        let conf = if probs.is_empty() { 0.0 } else { probs.iter().sum::<f32>() / probs.len() as f32 };
        Ok((text, conf))
    }
}

/// CRAFT score-map postprocess. This follows the official low/link/text
/// threshold + connected-component + expanded box logic, using axis-aligned
/// boxes for UI text instead of OpenCV minAreaRect.
fn craft_boxes(
    textmap: &[f32],
    linkmap: &[f32],
    hh: usize,
    ww: usize,
    sw: u32,
    sh: u32,
    scale: f32,
) -> Vec<Rect> {
    let low_text = 0.4f32;
    let link_threshold = 0.4f32;
    let text_threshold = 0.7f32;
    let mut mask = vec![false; hh * ww];
    for i in 0..hh * ww {
        if textmap[i] > low_text || linkmap[i] > link_threshold {
            mask[i] = true;
        }
    }
    let mut seen = vec![false; hh * ww];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for y in 0..hh {
        for x in 0..ww {
            let idx = y * ww + x;
            if !mask[idx] || seen[idx] {
                continue;
            }
            seen[idx] = true;
            stack.push((x, y));
            let (mut x0, mut y0, mut x1, mut y1) = (x, y, x, y);
            let mut area = 0usize;
            let mut max_text = 0f32;
            while let Some((cx, cy)) = stack.pop() {
                area += 1;
                max_text = max_text.max(textmap[cy * ww + cx]);
                x0 = x0.min(cx);
                y0 = y0.min(cy);
                x1 = x1.max(cx);
                y1 = y1.max(cy);
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let nx = cx as i32 + dx;
                    let ny = cy as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= ww as i32 || ny >= hh as i32 {
                        continue;
                    }
                    let ni = ny as usize * ww + nx as usize;
                    if mask[ni] && !seen[ni] {
                        seen[ni] = true;
                        stack.push((nx as usize, ny as usize));
                    }
                }
            }
            if area < 10 || max_text < text_threshold {
                continue;
            }
            let bw = (x1 - x0 + 1) as f32;
            let bh = (y1 - y0 + 1) as f32;
            let niter = ((area as f32 * bw.min(bh) / (bw * bh).max(1.0)).sqrt() * 2.0).round() as f32;
            let mut r = Rect::new(
                (x0 as f32 - niter) * 2.0 / scale,
                (y0 as f32 - niter) * 2.0 / scale,
                (bw + 2.0 * niter) * 2.0 / scale,
                (bh + 2.0 * niter) * 2.0 / scale,
            );
            r = r.clamp_to(&Rect::new(0.0, 0.0, sw as f32 / scale, sh as f32 / scale));
            if r.w >= 3.0 && r.h >= 3.0 {
                out.push(r);
            }
        }
    }
    out.sort_by(|a, b| {
        a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });
    out
}


/// Merge CRAFT word components into horizontal lines. EasyOCR's
/// `group_text_box` does this after the raw detector; the thresholds here are
/// a UI-oriented approximation of its y-center/height/width logic.
fn group_boxes(mut boxes: Vec<Rect>) -> Vec<Rect> {
    boxes.sort_by(|a, b| {
        a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });
    loop {
        let mut merged = false;
        'outer: for i in 0..boxes.len() {
            for j in (i + 1)..boxes.len() {
                let a = boxes[i];
                let b = boxes[j];
                let ay = a.y + a.h / 2.0;
                let by = b.y + b.h / 2.0;
                let min_h = a.h.min(b.h);
                let max_h = a.h.max(b.h);
                let y_ok = (ay - by).abs() <= min_h * 0.5;
                let h_ok = min_h / max_h.max(1.0) >= 0.5;
                let gap = (a.x.max(b.x) - a.x2().min(b.x2())).max(0.0);
                let x_ok = gap <= max_h * 0.8;
                if y_ok && h_ok && x_ok {
                    boxes[i] = a.union(&b);
                    boxes.remove(j);
                    merged = true;
                    break 'outer;
                }
            }
        }
        if !merged {
            break;
        }
    }
    boxes.sort_by(|a, b| {
        a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });
    boxes
}
