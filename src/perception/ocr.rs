// perception/ocr.rs — PP-OCRv4 text detection/recognition via tract (pure Rust).
// The model graphs are fixed-shape ONNX exports of the public PP-OCRv4 assets.
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use tract_onnx::prelude::*;

use super::coordinates::{Letterbox, Rect};

type Run = tract_onnx::prelude::RunnableModel<
    tract_onnx::prelude::TypedFact,
    Box<dyn tract_onnx::prelude::TypedOp>,
    tract_onnx::prelude::TypedModel,
>;

const DET_SIZE: u32 = 640;
const REC_H: u32 = 48;
const REC_W: u32 = 320;

#[derive(Clone, Debug)]
pub struct TextLine {
    pub rect: Rect,
    pub text: String,
    pub confidence: f32,
}

pub struct PpOcr {
    det: Run,
    rec: Run,
    cls: Option<Run>,
    dict: Vec<char>,
    det_size: u32,
}

fn load(path: &std::path::Path) -> Result<Run> {
    if !path.is_file() {
        bail!("OCR model missing: {}", path.display());
    }
    tract_onnx::onnx()
        .model_for_path(path)
        .with_context(|| format!("load {}", path.display()))?
        .into_optimized()?
        .into_runnable()
        .with_context(|| format!("optimize {}", path.display()))
}

fn resize_rgba(w: u32, h: u32, rgba: &[u8], nw: u32, nh: u32) -> image::RgbaImage {
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec()).unwrap_or_else(|| {
        image::RgbaImage::new(w, h)
    });
    image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle)
}

impl PpOcr {
    pub fn load(dir: &std::path::Path) -> Result<Self> {
        let det = load(&dir.join("det640.onnx"))?;
        let rec = load(&dir.join("rec320.onnx"))?;
        let cls = Some(load(&dir.join("cls192.onnx"))?);
        let keys = std::fs::read_to_string(dir.join("ppocr_keys_v1.txt"))?;
        let mut dict: Vec<char> = keys.lines().map(|l| l.chars().next().unwrap_or('?')).collect();
        dict.push(' ');
        Ok(Self { det, rec, cls, dict, det_size: DET_SIZE })
    }

    pub fn dict_len(&self) -> usize {
        self.dict.len()
    }

    /// Detect and recognize text lines in an RGBA frame.
    pub fn recognize(&self, width: u32, height: u32, rgba: &[u8]) -> Result<Vec<TextLine>> {
        let boxes = self.detect(width, height, rgba)?;
        let mut out = Vec::new();
        for (r, det_score) in boxes {
            match self.recognize_crop(width, height, rgba, &r) {
                Ok((text, conf)) if !text.is_empty() => out.push(TextLine {
                    rect: r,
                    text,
                    confidence: conf.min(det_score.max(conf)),
                }),
                _ => {}
            }
        }
        Ok(out)
    }

    fn detect(&self, width: u32, height: u32, rgba: &[u8]) -> Result<Vec<(Rect, f32)>> {
        let lb = Letterbox::fit(width, height, self.det_size, self.det_size);
        let rw = (width as f32 * lb.scale).round().max(1.0) as u32;
        let rh = (height as f32 * lb.scale).round().max(1.0) as u32;
        let img = resize_rgba(width, height, rgba, rw, rh);
        let mut data = vec![0f32; 3 * (self.det_size * self.det_size) as usize];
        let n = (self.det_size * self.det_size) as usize;
        let px = lb.pad_x as usize;
        let py = lb.pad_y as usize;
        for y in 0..rh as usize {
            for x in 0..rw as usize {
                let p = img.get_pixel(x as u32, y as u32).0;
                let idx = (py + y) * self.det_size as usize + px + x;
                for c in 0..3 {
                    data[c * n + idx] = (p[2 - c] as f32 / 255.0 - 0.5) / 0.5;
                }
            }
        }
        let input = Tensor::from_shape(
            &[1, 3, self.det_size as usize, self.det_size as usize],
            &data,
        )?;
        let out = self.det.run(tvec!(input.into()))?;
        let arr = out[0].to_array_view::<f32>()?;
        let shape = arr.shape().to_vec();
        let h = *shape.get(2).unwrap_or(&0);
        let w = *shape.get(3).unwrap_or(&0);
        let prob = arr.as_slice().ok_or_else(|| anyhow!("det output not contiguous"))?;

        let mut mask = vec![false; h * w];
        for y in 0..h {
            for x in 0..w {
                mask[y * w + x] = prob[y * w + x] > 0.3;
            }
        }
        // 2x2 dilation (RapidOCR default use_dilation=true)
        let src = mask.clone();
        for y in 0..h {
            for x in 0..w {
                if src[y * w + x] {
                    if y + 1 < h {
                        mask[(y + 1) * w + x] = true;
                    }
                    if x + 1 < w {
                        mask[y * w + x + 1] = true;
                    }
                    if y + 1 < h && x + 1 < w {
                        mask[(y + 1) * w + x + 1] = true;
                    }
                }
            }
        }

        let mut seen = vec![false; h * w];
        let mut comps: Vec<(Rect, f32)> = Vec::new();
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let idx = y * w + x;
                if !mask[idx] || seen[idx] {
                    continue;
                }
                seen[idx] = true;
                stack.push((x, y));
                let (mut x0, mut y0, mut x1, mut y1) = (x, y, x, y);
                let mut count = 0usize;
                let mut sum = 0f32;
                while let Some((cx, cy)) = stack.pop() {
                    count += 1;
                    sum += prob[cy * w + cx];
                    x0 = x0.min(cx);
                    y0 = y0.min(cy);
                    x1 = x1.max(cx);
                    y1 = y1.max(cy);
                    for dy in -1i32..=1 {
                        for dx in -1i32..=1 {
                            let nx = cx as i32 + dx;
                            let ny = cy as i32 + dy;
                            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                                continue;
                            }
                            let ni = ny as usize * w + nx as usize;
                            if mask[ni] && !seen[ni] {
                                seen[ni] = true;
                                stack.push((nx as usize, ny as usize));
                            }
                        }
                    }
                }
                let bw = (x1 - x0 + 1) as f32;
                let bh = (y1 - y0 + 1) as f32;
                if bw < 3.0 || bh < 3.0 {
                    continue;
                }
                let score = sum / count as f32;
                if score < 0.5 {
                    continue;
                }
                let perimeter = 2.0 * (bw + bh);
                let d = bw * bh * 1.6 / perimeter.max(1.0);
                let boxed = Rect::new(x0 as f32 - d, y0 as f32 - d, bw + 2.0 * d, bh + 2.0 * d);
                if boxed.w < 5.0 || boxed.h < 3.0 {
                    continue;
                }
                // Map from the letterboxed detector image back to frame pixels.
                let mut r = Rect::new(
                    (boxed.x - lb.pad_x) / lb.scale,
                    (boxed.y - lb.pad_y) / lb.scale,
                    boxed.w / lb.scale,
                    boxed.h / lb.scale,
                );
                r = r.clamp_to(&Rect::new(0.0, 0.0, width as f32, height as f32));
                if r.w < 3.0 || r.h < 3.0 {
                    continue;
                }
                comps.push((r, score));
            }
        }
        comps.sort_by(|a, b| {
            a.0.y.partial_cmp(&b.0.y).unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.x.partial_cmp(&b.0.x).unwrap_or(std::cmp::Ordering::Equal))
        });
        Ok(comps)
    }

    fn recognize_crop(
        &self,
        width: u32,
        height: u32,
        rgba: &[u8],
        rect: &Rect,
    ) -> Result<(String, f32)> {
        let r = rect.clamp_to(&Rect::new(0.0, 0.0, width as f32, height as f32));
        if r.w < 2.0 || r.h < 2.0 {
            return Ok((String::new(), 0.0));
        }
        let (crop, cw, ch) = crop_rgba(rgba, width, height, &r);
        let ratio = cw as f32 / ch.max(1) as f32;
        let rw = (REC_H as f32 * ratio).ceil().max(1.0).min(REC_W as f32) as u32;
        let resized = resize_rgba(cw, ch, &crop, rw, REC_H);
        let n = (REC_H * REC_W) as usize;
        let mut data = vec![0f32; 3 * n];
        for y in 0..REC_H as usize {
            for x in 0..rw as usize {
                let p = resized.get_pixel(x as u32, y as u32).0;
                let idx = y * REC_W as usize + x;
                for c in 0..3 {
                    data[c * n + idx] = (p[2 - c] as f32 / 255.0 - 0.5) / 0.5;
                }
            }
        }
        let input = Tensor::from_shape(&[1, 3, REC_H as usize, REC_W as usize], &data)?;
        let out = self.rec.run(tvec!(input.into()))?;
        let arr = out[0].to_array_view::<f32>()?;
        let shape = arr.shape().to_vec();
        let steps = *shape.get(1).unwrap_or(&0);
        let classes = *shape.get(2).unwrap_or(&0);
        let data = arr.as_slice().ok_or_else(|| anyhow!("rec output not contiguous"))?;
        if classes == 0 {
            return Ok((String::new(), 0.0));
        }
        let mut text = String::new();
        let mut last = usize::MAX;
        let mut probs: Vec<f32> = Vec::new();
        for t in 0..steps {
            let row = &data[t * classes..(t + 1) * classes];
            let mut best = 0usize;
            let mut best_v = f32::NEG_INFINITY;
            for (i, v) in row.iter().enumerate() {
                if *v > best_v {
                    best_v = *v;
                    best = i;
                }
            }
            if best != last && best != 0 {
                if best >= 1 && best - 1 < self.dict.len() {
                    text.push(self.dict[best - 1]);
                    probs.push(best_v);
                }
            }
            last = best;
        }
        let conf = if probs.is_empty() { 0.0 } else { probs.iter().sum::<f32>() / probs.len() as f32 };
        Ok((text, conf))
    }
}

fn crop_rgba(rgba: &[u8], width: u32, height: u32, r: &Rect) -> (Vec<u8>, u32, u32) {
    let x0 = r.x.floor().max(0.0) as u32;
    let y0 = r.y.floor().max(0.0) as u32;
    let x1 = (r.x + r.w).ceil().min(width as f32).max(0.0) as u32;
    let y1 = (r.y + r.h).ceil().min(height as f32).max(0.0) as u32;
    let mut out = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 4) as usize);
    for y in y0..y1 {
        let start = ((y * width + x0) * 4) as usize;
        let end = ((y * width + x1) * 4) as usize;
        out.extend_from_slice(&rgba[start..end]);
    }
    (out, x1 - x0, y1 - y0)
}

/// Rotate a crop 180 degrees when the direction classifier is confident.
pub fn rotate_180(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; rgba.len()];
    for y in 0..h {
        for x in 0..w {
            let si = ((y * w + x) * 4) as usize;
            let di = (((h - 1 - y) * w + (w - 1 - x)) * 4) as usize;
            out[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_respects_frame_bounds() {
        let rgba = vec![255u8; 10 * 10 * 4];
        let (c, w, h) = crop_rgba(&rgba, 10, 10, &Rect::new(-5.0, -5.0, 12.0, 12.0));
        assert_eq!((c.len(), w, h), (7 * 7 * 4, 7, 7));
    }
}
