// perception/template.rs — pure-Rust zero-mean normalized cross-correlation.
// CPU only; used for repeatable visual anchors and wait conditions. A match is
// evidence for a location, never permission to click.
use rayon::prelude::*;

use super::coordinates::Rect;

#[derive(Clone, Debug, PartialEq)]
pub struct Gray {
    pub w: u32,
    pub h: u32,
    pub pix: Vec<f32>,
}

impl Gray {
    pub fn from_rgba(w: u32, h: u32, rgba: &[u8]) -> Self {
        let mut pix = Vec::with_capacity((w * h) as usize);
        for p in rgba.chunks_exact(4) {
            pix.push(0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32);
        }
        Self { w, h, pix }
    }

    pub fn from_luma(w: u32, h: u32, pix: Vec<u8>) -> Self {
        Self { w, h, pix: pix.into_iter().map(|v| v as f32).collect() }
    }

    pub fn get(&self, x: u32, y: u32) -> f32 {
        self.pix[(y * self.w + x) as usize]
    }
}

#[derive(Clone, Debug)]
pub struct Template {
    pub gray: Gray,
}

impl Template {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let img = image::open(path)?.to_rgba8();
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            anyhow::bail!("template image is empty");
        }
        Ok(Self { gray: Gray::from_rgba(w, h, img.as_raw()) })
    }

    pub fn from_rgba(w: u32, h: u32, rgba: &[u8]) -> Self {
        Self { gray: Gray::from_rgba(w, h, rgba) }
    }
}

#[derive(Clone, Debug)]
pub struct Match {
    pub rect: Rect,
    pub score: f32,
    pub scale: f32,
}

#[derive(Clone, Debug, Default)]
pub struct MatchSet {
    pub matches: Vec<Match>,
    pub ambiguous: bool,
    pub best_score: f32,
    pub second_score: f32,
}

fn resize_gray(src: &Gray, nw: u32, nh: u32) -> Gray {
    let nw = nw.max(1);
    let nh = nh.max(1);
    if nw == src.w && nh == src.h {
        return src.clone();
    }
    let mut pix = vec![0.0f32; (nw * nh) as usize];
    let sx = src.w as f32 / nw as f32;
    let sy = src.h as f32 / nh as f32;
    for y in 0..nh {
        let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, src.h as f32 - 1.0);
        let y0 = fy.floor() as u32;
        let y1 = (y0 + 1).min(src.h - 1);
        let wy = fy - y0 as f32;
        for x in 0..nw {
            let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, src.w as f32 - 1.0);
            let x0 = fx.floor() as u32;
            let x1 = (x0 + 1).min(src.w - 1);
            let wx = fx - x0 as f32;
            let a = src.get(x0, y0) * (1.0 - wx) + src.get(x1, y0) * wx;
            let b = src.get(x0, y1) * (1.0 - wx) + src.get(x1, y1) * wx;
            pix[(y * nw + x) as usize] = a * (1.0 - wy) + b * wy;
        }
    }
    Gray { w: nw, h: nh, pix }
}

#[derive(Clone, Copy)]
struct Prepared {
    w: u32,
    h: u32,
    sum: f32,
    sum2: f32,
}

fn prepare(t: &Gray) -> Prepared {
    let sum: f32 = t.pix.iter().sum();
    let sum2: f32 = t.pix.iter().map(|v| v * v).sum();
    Prepared { w: t.w, h: t.h, sum, sum2 }
}

#[derive(Clone, Copy)]
struct Integrals {
    w: u32,
    h: u32,
    s: f32,
    s2: f32,
}

fn integral(src: &Gray) -> Vec<Integrals> {
    let w = src.w as usize;
    let h = src.h as usize;
    let mut out = vec![Integrals { w: src.w, h: src.h, s: 0.0, s2: 0.0 }; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row_s = 0.0;
        let mut row_s2 = 0.0;
        for x in 0..w {
            let v = src.pix[y * w + x];
            row_s += v;
            row_s2 += v * v;
            let up = out[y * (w + 1) + x + 1];
            out[(y + 1) * (w + 1) + x + 1] = Integrals { w: 0, h: 0, s: up.s + row_s, s2: up.s2 + row_s2 };
        }
    }
    out
}

fn box_sum(integ: &[Integrals], stride: usize, w: u32, h: u32, x: usize, y: usize, tw: usize, th: usize) -> (f32, f32) {
    let _ = (w, h);
    let a = integ[y * stride + x];
    let b = integ[y * stride + x + tw];
    let c = integ[(y + th) * stride + x];
    let d = integ[(y + th) * stride + x + tw];
    (d.s - b.s - c.s + a.s, d.s2 - b.s2 - c.s2 + a.s2)
}

/// Search one scale. Returns raw positions/scores.
fn search_scale(src: &Gray, tpl: &Gray, min_score: f32) -> Vec<Match> {
    if tpl.w > src.w || tpl.h > src.h {
        return Vec::new();
    }
    let prep = prepare(tpl);
    let tpl_mean = prep.sum / (prep.w * prep.h) as f32;
    let tpl_var = prep.sum2 - prep.sum * prep.sum / (prep.w * prep.h) as f32;
    if tpl_var <= 1e-4 {
        return Vec::new();
    }
    let integ = integral(src);
    let stride = src.w as usize + 1;
    let tw = tpl.w as usize;
    let th = tpl.h as usize;
    let nx = src.w as usize - tw + 1;
    let ny = src.h as usize - th + 1;
    let mut out: Vec<Match> = (0..ny)
        .into_par_iter()
        .flat_map_iter(|y| {
            let mut row: Vec<Match> = Vec::new();
            for x in 0..nx {
                let (s, s2) = box_sum(&integ, stride, src.w, src.h, x, y, tw, th);
                let n = (tw * th) as f32;
                let _mean = s / n;
                let var = s2 - s * s / n;
                if var <= 1e-4 {
                    continue;
                }
                let mut dot = 0.0f32;
                for ty in 0..th {
                    let so = (y + ty) * src.w as usize + x;
                    let to = ty * tw;
                    for tx in 0..tw {
                        dot += src.pix[so + tx] * tpl.pix[to + tx];
                    }
                }
                let cov = dot - s * tpl_mean;
                let score = cov / (var.sqrt() * tpl_var.sqrt()) as f32;
                if score >= min_score {
                    row.push(Match {
                        rect: Rect::new(x as f32, y as f32, tw as f32, th as f32),
                        score,
                        scale: 1.0,
                    });
                }
            }
            row.into_iter()
        })
        .collect();
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    out
}

fn nms(mut matches: Vec<Match>, iou: f32, max: usize) -> Vec<Match> {
    let mut kept = Vec::new();
    while let Some(m) = matches.first().cloned() {
        matches.remove(0);
        if kept.iter().all(|k: &Match| k.rect.iou(&m.rect) <= iou) {
            kept.push(m);
            if kept.len() >= max {
                break;
            }
        }
    }
    kept
}

/// Multi-scale search with a coarse pass to bound CPU cost. The returned rects
/// are in source-image coordinates and the score is normalized cross
/// correlation in [-1, 1]; `threshold` is caller-calibrated, not universal.
pub fn find(
    src: &Gray,
    tpl: &Template,
    threshold: f32,
    scales: &[f32],
    max_matches: usize,
) -> MatchSet {
    let long = src.w.max(src.h) as f32;
    let coarse = (long / 512.0).max(1.0);
    let csrc = if coarse > 1.0 {
        resize_gray(src, (src.w as f32 / coarse) as u32, (src.h as f32 / coarse) as u32)
    } else {
        src.clone()
    };
    let mut candidates: Vec<Match> = Vec::new();
    for &scale in scales {
        let tw = ((tpl.gray.w as f32 * scale) as u32).max(1);
        let th = ((tpl.gray.h as f32 * scale) as u32).max(1);
        let ct = resize_gray(&tpl.gray, (tw as f32 / coarse).max(1.0) as u32, (th as f32 / coarse).max(1.0) as u32);
        candidates.extend(search_scale(&csrc, &ct, threshold).into_iter().map(|mut m| {
            m.rect = Rect::new(m.rect.x * coarse, m.rect.y * coarse, m.rect.w * coarse, m.rect.h * coarse);
            m.scale = scale;
            m
        }));
    }
    if candidates.is_empty() {
        return MatchSet::default();
    }
    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    // Full-resolution refinement around the best coarse candidates.
    let mut refined: Vec<Match> = Vec::new();
    for c in candidates.iter().take(8) {
        let tw = ((tpl.gray.w as f32 * c.scale) as u32).max(1);
        let th = ((tpl.gray.h as f32 * c.scale) as u32).max(1);
        let t = resize_gray(&tpl.gray, tw, th);
        let pad = (long / 512.0).ceil() as i32 + 2;
        let rx = (c.rect.x as i32 - pad).max(0) as u32;
        let ry = (c.rect.y as i32 - pad).max(0) as u32;
        let rw = (tw as i32 + 2 * pad).min(src.w as i32 - rx as i32).max(1) as u32;
        let rh = (th as i32 + 2 * pad).min(src.h as i32 - ry as i32).max(1) as u32;
        let mut crop = Vec::with_capacity((rw * rh) as usize);
        for y in ry..ry + rh {
            for x in rx..rx + rw {
                crop.push(src.get(x, y));
            }
        }
        let sub = Gray { w: rw, h: rh, pix: crop };
        let mut best = search_scale(&sub, &t, threshold);
        if let Some(mut m) = best.into_iter().next() {
            m.rect.x += rx as f32;
            m.rect.y += ry as f32;
            m.scale = c.scale;
            refined.push(m);
        }
    }
    let mut matches = nms(refined, 0.3, max_matches);
    matches.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let best = matches.first().map(|m| m.score).unwrap_or(0.0);
    let second = matches.get(1).map(|m| m.score).unwrap_or(0.0);
    MatchSet {
        ambiguous: matches.len() > 1 && (best - second).abs() < 0.02,
        matches,
        best_score: best,
        second_score: second,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray_from(rows: &[&str]) -> Gray {
        let h = rows.len() as u32;
        let w = rows[0].len() as u32;
        let mut pix = Vec::new();
        for row in rows {
            for ch in row.chars() {
                pix.push(if ch == '#' { 255.0 } else { 0.0 });
            }
        }
        Gray { w, h, pix }
    }

    #[test]
    fn finds_an_exact_pattern() {
        let src = gray_from(&[".......", "..#....", "...#...", "......."]);
        let tpl = gray_from(&["#.", ".#"]);
        let m = find(&src, &Template { gray: tpl }, 0.9, &[1.0], 4);
        assert_eq!(m.matches.len(), 1);
        assert_eq!(m.matches[0].rect.x, 2.0);
        assert_eq!(m.matches[0].rect.y, 1.0);
    }

    #[test]
    fn rejects_flat_regions() {
        let src = gray_from(&["########", "########", "########"]);
        let tpl = gray_from(&["##", "##"]);
        let m = find(&src, &Template { gray: tpl }, 0.5, &[1.0], 4);
        assert!(m.matches.is_empty());
    }
}
