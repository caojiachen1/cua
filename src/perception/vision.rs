// perception/vision.rs — YOLOv9-E icon detection (Burn/WGPU) and Florence-2
// captioning (tract). Both models are the pinned official OmniParser v2
// assets; the Rust code implements their preprocessing and postprocessing.
#[allow(clippy::all, dead_code, unused_imports)]
mod generated_yolo {
    include!("generated_yolo.rs");
}

use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;
use burn::prelude::*;
use tract_onnx::prelude::*;

use super::coordinates::{Letterbox, Rect};
use super::fusion::DetItem;
use super::models;

type Run = tract_onnx::prelude::RunnableModel<
    tract_onnx::prelude::TypedFact,
    Box<dyn tract_onnx::prelude::TypedOp>,
    tract_onnx::prelude::TypedModel,
>;

pub const DET_SIZE: u32 = 640;
pub const DET_THRESHOLD: f32 = 0.05;
pub const DET_NMS_IOU: f32 = 0.1;
pub const DET_MAX: usize = 300;
/// Cross-pass dedupe: same icon seen by two overlapping tiles.
const TILE_MERGE_IOU: f32 = 0.55;

/// How many extra 640px passes each detail level may spend. `CUA_DET_TILES`
/// overrides it (0 disables tiling, N caps it).
pub fn det_tile_budget(detail: super::types::Detail) -> usize {
    if let Ok(v) = std::env::var("CUA_DET_TILES") {
        if let Ok(n) = v.trim().parse::<usize>() {
            return n;
        }
    }
    match detail {
        // fast stays single-pass; balanced covers a large window at ~0.5x with
        // the minimum tile cover; full runs native-resolution tiles.
        super::types::Detail::Fast => 0,
        super::types::Detail::Balanced => 4,
        super::types::Detail::Full => 24,
    }
}
pub const CAPTION_SIZE: u32 = 64;
pub const CAPTION_MAX_TOKENS: usize = 20;
pub const CAPTION_EOS: usize = 2;

pub struct Detector {
    model: generated_yolo::Model,
    device: Device,
    backend: &'static str,
}

impl Detector {
    pub fn load() -> Result<Self> {
        let bpk = models::yolo_bpk();
        if !bpk.is_file() {
            bail!(
                "YOLOv9-E Burn weights missing: {} (run 'cua perception models verify' or the installer)",
                bpk.display()
            );
        }
        // Prefer the discrete GPU; fall back to the pure-Rust CPU backend when
        // WGPU cannot initialize. The backend is reported, never hidden.
        let (device, backend) = match std::panic::catch_unwind(|| Device::wgpu(Default::default())) {
            Ok(d) => (d, "wgpu"),
            Err(_) => (Device::flex(), "flex-cpu"),
        };
        let model = generated_yolo::Model::from_file(&bpk, &device);
        Ok(Self { model, device, backend })
    }

    pub fn backend(&self) -> &'static str {
        self.backend
    }

    fn drop_uniform_large(items: Vec<DetItem>, width: u32, height: u32, rgba: &[u8]) -> Vec<DetItem> {
        let frame_area = (width * height).max(1) as f32;
        items
            .into_iter()
            .filter(|d| {
                if d.rect.area() < frame_area * 0.05 {
                    return true;
                }
                let r = d.rect.clamp_to(&Rect::new(0.0, 0.0, width as f32, height as f32));
                if r.w < 2.0 || r.h < 2.0 {
                    return false;
                }
                let step_x = (r.w / 12.0).max(1.0) as u32;
                let step_y = (r.h / 12.0).max(1.0) as u32;
                let mut min = 255u8;
                let mut max = 0u8;
                let mut y = r.y as u32;
                while y < (r.y + r.h) as u32 && y < height {
                    let mut x = r.x as u32;
                    while x < (r.x + r.w) as u32 && x < width {
                        let i = ((y * width + x) * 4) as usize;
                        let lum = ((rgba[i] as u32 * 299 + rgba[i + 1] as u32 * 587 + rgba[i + 2] as u32 * 114) / 1000) as u8;
                        min = min.min(lum);
                        max = max.max(lum);
                        x += step_x;
                    }
                    y += step_y;
                }
                max.saturating_sub(min) >= 4
            })
            .collect()
    }

    /// Full-frame pass. Large frames are letterboxed down to 640, which is why
    /// small toolbar icons can vanish; `detect_tiled` adds 1:1 passes.
    pub fn detect(&self, width: u32, height: u32, rgba: &[u8]) -> Result<Vec<DetItem>> {
        self.detect_tiled(width, height, rgba, 0)
    }

    /// `tile_budget` is the maximum number of extra 640x640 passes (0 disables).
    /// Each tile is cut at native resolution, so a 16px icon in a 2560px window
    /// is seen at 16px instead of 4px.
    pub fn detect_tiled(
        &self,
        width: u32,
        height: u32,
        rgba: &[u8],
        tile_budget: usize,
    ) -> Result<Vec<DetItem>> {
        if width == 0 || height == 0 || rgba.len() < (width * height * 4) as usize {
            bail!("invalid frame for detector");
        }
        let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())
            .ok_or_else(|| anyhow!("rgba buffer size mismatch"))?;
        let (raw, lb) = self.forward_letterboxed(&img)?;
        let items = decode_raw(&raw, &lb, width, height)?;
        let mut items = Self::drop_uniform_large(items, width, height, rgba);
        if tile_budget == 0 || width.max(height) <= DET_SIZE + DET_SIZE / 4 {
            return Ok(items);
        }
        // A CPU fallback would pay 15x the forward cost; keep that bounded.
        let budget = if self.backend == "flex-cpu" { tile_budget.min(4) } else { tile_budget };
        let mut extra = self.detect_tiles(width, height, rgba, budget)?;
        if extra.is_empty() {
            return Ok(items);
        }
        items.append(&mut extra);
        let merged = nms(items, TILE_MERGE_IOU, DET_MAX * 2);
        Ok(Self::drop_uniform_large(merged, width, height, rgba))
    }

    fn forward_letterboxed(&self, img: &image::RgbaImage) -> Result<(Vec<f32>, Letterbox)> {
        let (width, height) = img.dimensions();
        let lb = Letterbox::fit(width, height, DET_SIZE, DET_SIZE);
        let rw = (width as f32 * lb.scale).round().max(1.0) as u32;
        let rh = (height as f32 * lb.scale).round().max(1.0) as u32;
        let resized = image::imageops::resize(img, rw, rh, image::imageops::FilterType::Triangle);
        let mut data = vec![0f32; 3 * (DET_SIZE * DET_SIZE) as usize];
        let n = (DET_SIZE * DET_SIZE) as usize;
        let pad_x = lb.pad_x as usize;
        let pad_y = lb.pad_y as usize;
        // Official YOLOv9 preprocessing: RGB, 0..1, pad 114/255.
        let fill = 114.0f32 / 255.0;
        data.iter_mut().for_each(|v| *v = fill);
        for y in 0..rh as usize {
            for x in 0..rw as usize {
                let p = resized.get_pixel(x as u32, y as u32).0;
                let idx = (pad_y + y) * DET_SIZE as usize + pad_x + x;
                for c in 0..3 {
                    data[c * n + idx] = p[c] as f32 / 255.0;
                }
            }
        }
        let input = burn::tensor::Tensor::<4>::from_data(
            TensorData::new(data, [1, 3, DET_SIZE as usize, DET_SIZE as usize]),
            &self.device,
        );
        let out = self.model.forward(input).into_data();
        let raw: Vec<f32> = out
            .try_to_vec::<f32>()
            .map_err(|e| anyhow!("detector output: {e:?}"))?;
        Ok((raw, lb))
    }

    fn detect_tiles(
        &self,
        width: u32,
        height: u32,
        rgba: &[u8],
        budget: usize,
    ) -> Result<Vec<DetItem>> {
        if budget == 0 {
            return Ok(Vec::new());
        }
        // Smallest native tile that still fits the budget.
        let mut tile_src = DET_SIZE;
        let mut grid = tile_grid(width, height, tile_src);
        while grid.0 * grid.1 > budget as u32 && tile_src < DET_SIZE * 2 {
            tile_src += 128;
            grid = tile_grid(width, height, tile_src);
        }
        let stride = ((tile_src as f32 * 0.8).round() as i32).max(1);
        let mut items = Vec::new();
        for row in 0..grid.1 as i32 {
            for col in 0..grid.0 as i32 {
                let x = (col * stride).min(width as i32 - tile_src as i32).max(0) as u32;
                let y = (row * stride).min(height as i32 - tile_src as i32).max(0) as u32;
                let tw = tile_src.min(width - x);
                let th = tile_src.min(height - y);
                if tw < 32 || th < 32 {
                    continue;
                }
                // A flat background tile cannot contain an icon; skipping it
                // keeps the extra passes focused on real UI.
                if tile_is_uniform(rgba, width, x, y, tw, th) {
                    continue;
                }
                let crop = crop_rgba(rgba, width, x, y, tw, th);
                let img = image::RgbaImage::from_raw(tw, th, crop)
                    .ok_or_else(|| anyhow!("tile buffer size mismatch"))?;
                let (raw, lb) = self.forward_letterboxed(&img)?;
                let mut local = decode_raw(&raw, &lb, tw, th)?;
                for it in &mut local {
                    it.rect.x += x as f32;
                    it.rect.y += y as f32;
                }
                items.extend(local);
            }
        }
        Ok(items)
    }
}

/// Tiles overlap by 20% so a control on a seam is still whole in one pass.
fn tile_grid(width: u32, height: u32, tile: u32) -> (u32, u32) {
    let stride = ((tile as f32 * 0.8).round() as u32).max(1);
    let axis = |total: u32| -> u32 {
        if total <= tile {
            1
        } else {
            (total - tile + stride - 1) / stride + 1
        }
    };
    (axis(width), axis(height))
}

fn tile_is_uniform(rgba: &[u8], width: u32, x: u32, y: u32, cw: u32, ch: u32) -> bool {
    let step_x = (cw / 16).max(1);
    let step_y = (ch / 16).max(1);
    let mut min = 255u8;
    let mut max = 0u8;
    let mut yy = y;
    while yy < y + ch {
        let mut xx = x;
        while xx < x + cw {
            let i = ((yy * width + xx) * 4) as usize;
            if i + 2 < rgba.len() {
                let lum = ((rgba[i] as u32 * 299 + rgba[i + 1] as u32 * 587 + rgba[i + 2] as u32 * 114) / 1000) as u8;
                min = min.min(lum);
                max = max.max(lum);
            }
            xx += step_x;
        }
        yy += step_y;
    }
    max.saturating_sub(min) < 6
}

fn crop_rgba(rgba: &[u8], width: u32, x: u32, y: u32, cw: u32, ch: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((cw * ch * 4) as usize);
    for row in 0..ch {
        let start = (((y + row) * width + x) * 4) as usize;
        out.extend_from_slice(&rgba[start..start + (cw * 4) as usize]);
    }
    out
}

/// Decode the flat [1,42000] output. The concatenation order is the official
/// scripted model's: for each of strides 8/16/32, class logits then four
/// distances, each flattened in NCHW order.
pub fn decode_raw(raw: &[f32], lb: &Letterbox, width: u32, height: u32) -> Result<Vec<DetItem>> {
    let grids = [
        (8usize, (DET_SIZE / 8) as usize),
        (16usize, (DET_SIZE / 16) as usize),
        (32usize, (DET_SIZE / 32) as usize),
    ];
    let mut offset = 0usize;
    let mut candidates: Vec<DetItem> = Vec::new();
    for (stride, grid) in grids {
        let n = grid * grid;
        if offset + 5 * n > raw.len() {
            bail!("detector raw output is shorter than expected");
        }
        let logits = &raw[offset..offset + n];
        let dist = &raw[offset + n..offset + 5 * n];
        offset += 5 * n;
        for gy in 0..grid {
            for gx in 0..grid {
                let i = gy * grid + gx;
                let score = 1.0 / (1.0 + (-logits[i]).exp());
                if score < DET_THRESHOLD {
                    continue;
                }
                let cx = (gx as f32 + 0.5) * stride as f32;
                let cy = (gy as f32 + 0.5) * stride as f32;
                let lt = dist[i] * stride as f32;
                let tt = dist[n + i] * stride as f32;
                let rb = dist[2 * n + i] * stride as f32;
                let bb = dist[3 * n + i] * stride as f32;
                let boxed = Rect::new(cx - lt, cy - tt, lt + rb, tt + bb);
                let mut r = Rect::new(
                    (boxed.x - lb.pad_x) / lb.scale,
                    (boxed.y - lb.pad_y) / lb.scale,
                    boxed.w / lb.scale,
                    boxed.h / lb.scale,
                );
                r = r.clamp_to(&Rect::new(0.0, 0.0, width as f32, height as f32));
                if r.w < 2.0 || r.h < 2.0 {
                    continue;
                }
                candidates.push(DetItem { rect: r, score });
            }
        }
    }
    Ok(nms(candidates, DET_NMS_IOU, DET_MAX))
}

fn nms(mut items: Vec<DetItem>, iou: f32, max: usize) -> Vec<DetItem> {
    items.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let mut keep = Vec::new();
    for c in items {
        if keep.len() >= max {
            break;
        }
        if keep.iter().all(|k: &DetItem| k.rect.iou(&c.rect) <= iou) {
            keep.push(c);
        }
    }
    keep
}

pub const CAPTION_BATCH_SIZE: usize = 8;

enum FlorenceBackend {
    Ort {
        enc: ort::session::Session,
        dec: ort::session::Session,
        batch: usize,
        device: String,
    },
    Tract {
        enc: Run,
        dec: Run,
        batch: usize,
    },
}

impl FlorenceBackend {
    fn batch(&self) -> usize {
        match self {
            Self::Ort { batch, .. } | Self::Tract { batch, .. } => *batch,
        }
    }

    fn device(&self) -> &str {
        match self {
            Self::Ort { device, .. } => device,
            Self::Tract { .. } => "tract-cpu",
        }
    }

    fn encode(&mut self, pixels: &[f32], batch: usize) -> Result<Vec<f32>> {
        match self {
            Self::Ort { enc, .. } => {
                let input = ort::value::Tensor::from_array((
                    [batch, 3, CAPTION_SIZE as usize, CAPTION_SIZE as usize],
                    pixels.to_vec(),
                ))
                .map_err(|e| anyhow!("ORT encoder input: {e}"))?;
                let outputs = enc
                    .run(ort::inputs![input])
                    .map_err(|e| anyhow!("ORT encoder run: {e}"))?;
                let (_, data) = outputs[0]
                    .try_extract_tensor::<f32>()
                    .map_err(|e| anyhow!("ORT encoder output: {e}"))?;
                Ok(data.to_vec())
            }
            Self::Tract { enc, .. } => {
                let input = tract_onnx::prelude::Tensor::from_shape(
                    &[batch, 3, CAPTION_SIZE as usize, CAPTION_SIZE as usize],
                    pixels,
                )?;
                let out = enc.run(tvec!(input.into()))?.remove(0);
                let arr = out.to_array_view::<f32>()?;
                Ok(arr.as_slice().unwrap().to_vec())
            }
        }
    }

    /// Decode one step. Only the logits for `step` are copied out of the
    /// session output; the full [batch, seq, vocab] tensor stays in the
    /// runtime buffer instead of being copied and then thrown away.
    fn decode(&mut self, ids: &[i64], hidden: &[f32], batch: usize, step: usize, seq: usize) -> Result<Vec<f32>> {
        match self {
            Self::Ort { dec, .. } => {
                let id_tensor = ort::value::Tensor::from_array((
                    [batch, CAPTION_MAX_TOKENS + 1],
                    ids.to_vec(),
                ))
                .map_err(|e| anyhow!("ORT decoder ids: {e}"))?;
                let h_tensor = ort::value::Tensor::from_array((
                    [batch, 13, 768],
                    hidden.to_vec(),
                ))
                .map_err(|e| anyhow!("ORT decoder hidden: {e}"))?;
                let outputs = dec
                    .run(ort::inputs![id_tensor, h_tensor])
                    .map_err(|e| anyhow!("ORT decoder run: {e}"))?;
                let (_, data) = outputs[0]
                    .try_extract_tensor::<f32>()
                    .map_err(|e| anyhow!("ORT decoder output: {e}"))?;
                let vocab = data.len() / (batch * seq);
                if vocab == 0 {
                    bail!("Florence decoder returned an empty vocabulary");
                }
                let mut rows = Vec::with_capacity(batch * vocab);
                for row in 0..batch {
                    let off = (row * seq + step) * vocab;
                    rows.extend_from_slice(&data[off..off + vocab]);
                }
                Ok(rows)
            }
            Self::Tract { dec, .. } => {
                let id_tensor = tract_onnx::prelude::Tensor::from_shape(
                    &[batch, CAPTION_MAX_TOKENS + 1],
                    ids,
                )?;
                let h_tensor =
                    tract_onnx::prelude::Tensor::from_shape(&[batch, 13, 768], hidden)?;
                let out = dec.run(tvec!(id_tensor.into(), h_tensor.into()))?;
                let arr = out[0].to_array_view::<f32>()?;
                let data = arr.as_slice().unwrap();
                let vocab = data.len() / (batch * seq);
                if vocab == 0 {
                    bail!("Florence decoder returned an empty vocabulary");
                }
                let mut rows = Vec::with_capacity(batch * vocab);
                for row in 0..batch {
                    let off = (row * seq + step) * vocab;
                    rows.extend_from_slice(&data[off..off + vocab]);
                }
                Ok(rows)
            }
        }
    }
}

pub struct Captioner {
    backend: FlorenceBackend,
    tokenizer: tokenizers::Tokenizer,
}

fn prepare_caption_crop(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<f32>> {
    if w < 2 || h < 2 {
        anyhow::bail!("caption crop too small");
    }
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec())
        .ok_or_else(|| anyhow!("caption crop size mismatch"))?;
    let img = image::imageops::resize(
        &img,
        CAPTION_SIZE,
        CAPTION_SIZE,
        image::imageops::FilterType::Triangle,
    );
    let n = (CAPTION_SIZE * CAPTION_SIZE) as usize;
    let mut data = vec![0f32; 3 * n];
    let mean = [0.485f32, 0.456, 0.406];
    let std = [0.229f32, 0.224, 0.225];
    for y in 0..CAPTION_SIZE as usize {
        for x in 0..CAPTION_SIZE as usize {
            let p = img.get_pixel(x as u32, y as u32).0;
            let idx = y * CAPTION_SIZE as usize + x;
            for c in 0..3 {
                data[c * n + idx] = (p[c] as f32 / 255.0 - mean[c]) / std[c];
            }
        }
    }
    Ok(data)
}

fn best_token(row: &[f32], generated: &[u32]) -> usize {
    let mut best = 0usize;
    let mut best_v = f32::NEG_INFINITY;
    for (i, v) in row.iter().enumerate() {
        if *v > best_v {
            best_v = *v;
            best = i;
        }
    }
    if generated.len() >= 2 {
        let a = generated[generated.len() - 2];
        let b = generated[generated.len() - 1];
        let mut banned = vec![false; row.len()];
        for w in generated.windows(3) {
            if w[0] == a && w[1] == b && (w[2] as usize) < banned.len() {
                banned[w[2] as usize] = true;
            }
        }
        if banned.iter().any(|v| *v) {
            let mut best2 = 0usize;
            let mut best2_v = f32::NEG_INFINITY;
            for (i, v) in row.iter().enumerate() {
                if banned[i] {
                    continue;
                }
                if *v > best2_v {
                    best2_v = *v;
                    best2 = i;
                }
            }
            best = best2;
        }
    }
    best
}

fn clean_caption(text: String) -> String {
    let text = text.trim().to_string();
    let lower = text.to_ascii_lowercase();
    let path_like = text.starts_with("M0")
        || text.starts_with("m0")
        || (lower.contains("l0")
            && text
                .chars()
                .all(|c| c.is_ascii_digit() || " ,.mlhvcsqtaz-MLHVC SQT AZ".contains(c)));
    if path_like { String::new() } else { text }
}

fn ort_session_pair(
    enc_path: &Path,
    dec_path: &Path,
    gpu: bool,
) -> Result<(ort::session::Session, ort::session::Session)> {
    let make = |path: &Path| -> Result<ort::session::Session> {
        let mut b = ort::session::Session::builder()
            .map_err(|e| anyhow!("ORT session builder: {e}"))?;
        if gpu {
            b = b
                .with_execution_providers([ort::ep::DirectML::default()
                    .with_device_id(0)
                    .build()
                    .error_on_failure()])
                .map_err(|e| anyhow!("ORT DirectML registration: {e}"))?;
        }
        b.commit_from_file(path)
            .map_err(|e| anyhow!("ORT load {}: {e}", path.display()))
    };
    Ok((make(enc_path)?, make(dec_path)?))
}

impl Captioner {
    pub fn load() -> Result<Self> {
        Self::load_for(true)
    }

    /// GPU (DirectML) is preferred for both batch and single graphs. If the
    /// provider cannot create a session, ONNX Runtime CPU is used; if ORT is
    /// unusable, the tract implementation is the final CPU fallback.
    pub fn load_for(batch_requested: bool) -> Result<Self> {
        let batch_enc = models::omni_onnx("fl_encoder_b8.onnx");
        let batch_dec = models::omni_onnx("fl_decoder_b8.onnx");
        let single_enc = models::omni_onnx("fl_encoder.onnx");
        let single_dec = models::omni_onnx("fl_decoder.onnx");
        let batch_files = batch_enc.is_file() && batch_dec.is_file();
        let single_files = single_enc.is_file() && single_dec.is_file();
        let use_batch = batch_files && (batch_requested || !single_files);
        let (enc_path, dec_path, batch) = if use_batch {
            (batch_enc, batch_dec, CAPTION_BATCH_SIZE)
        } else {
            (single_enc, single_dec, 1)
        };
        if !enc_path.is_file() || !dec_path.is_file() {
            bail!(
                "Florence-2 ONNX graphs missing under {} (run 'cua perception models install')",
                models::omni_dir().display()
            );
        }
        let tok_path = models::florence_tokenizer();
        let tokenizer = tokenizers::Tokenizer::from_file(&tok_path)
            .map_err(|e| anyhow!("tokenizer {}: {e}", tok_path.display()))?;

        let backend = match ort_session_pair(&enc_path, &dec_path, true) {
            Ok((enc, dec)) => FlorenceBackend::Ort {
                enc,
                dec,
                batch,
                device: "ort-directml".into(),
            },
            Err(gpu_err) => {
                eprintln!("warning: Florence DirectML unavailable ({gpu_err:#}); trying ORT CPU");
                match ort_session_pair(&enc_path, &dec_path, false) {
                    Ok((enc, dec)) => FlorenceBackend::Ort {
                        enc,
                        dec,
                        batch,
                        device: "ort-cpu".into(),
                    },
                    Err(cpu_err) => {
                        eprintln!("warning: Florence ORT CPU unavailable ({cpu_err:#}); using tract CPU");
                        FlorenceBackend::Tract {
                            enc: load_run(&enc_path)?,
                            dec: load_run(&dec_path)?,
                            batch,
                        }
                    }
                }
            }
        };
        Ok(Self { backend, tokenizer })
    }

    pub fn batch_size(&self) -> usize {
        self.backend.batch()
    }

    pub fn backend_name(&self) -> &str {
        self.backend.device()
    }

    pub fn caption(&mut self, w: u32, h: u32, rgba: &[u8]) -> Result<String> {
        let out = self.caption_batch(&[(w, h, rgba.to_vec())])?;
        Ok(out.into_iter().next().unwrap_or_default())
    }

    /// Caption up to `batch_size` crops per forward pass. Rows are padded with
    /// zero pixels and ignored in the output; generation stops per row at EOS
    /// and for the whole batch when every real row is done.
    pub fn caption_batch(&mut self, crops: &[(u32, u32, Vec<u8>)]) -> Result<Vec<String>> {
        let mut out = vec![String::new(); crops.len()];
        if crops.is_empty() {
            return Ok(out);
        }
        let n_pix = (CAPTION_SIZE * CAPTION_SIZE) as usize;
        let batch = self.backend.batch();
        for start in (0..crops.len()).step_by(batch) {
            let end = (start + batch).min(crops.len());
            let real = end - start;
            let mut pixels = vec![0f32; batch * 3 * n_pix];
            for (i, (w, h, rgba)) in crops[start..end].iter().enumerate() {
                if let Ok(data) = prepare_caption_crop(*w, *h, rgba) {
                    pixels[i * 3 * n_pix..(i + 1) * 3 * n_pix].copy_from_slice(&data);
                }
            }
            let hidden = self.backend.encode(&pixels, batch)?;
            if hidden.len() != batch * 13 * 768 {
                bail!("unexpected Florence hidden size {}", hidden.len());
            }
            let seq = CAPTION_MAX_TOKENS + 1;
            let mut ids = vec![1i64; batch * seq];
            for row in 0..batch {
                ids[row * seq] = 2;
            }
            let mut generated = vec![Vec::<u32>::new(); batch];
            let mut done = vec![true; batch];
            for row in 0..real {
                done[row] = false;
            }
            for step in 0..CAPTION_MAX_TOKENS {
                if done.iter().all(|v| *v) {
                    break;
                }
                let logits = self.backend.decode(&ids, &hidden, batch, step, seq)?;
                let vocab = logits.len() / batch;
                if vocab == 0 {
                    bail!("Florence decoder returned an empty vocabulary");
                }
                for row in 0..real {
                    if done[row] {
                        continue;
                    }
                    let row_logits = &logits[row * vocab..(row + 1) * vocab];
                    let best = best_token(row_logits, &generated[row]);
                    if best == CAPTION_EOS {
                        done[row] = true;
                    } else {
                        generated[row].push(best as u32);
                        ids[row * seq + step + 1] = best as i64;
                    }
                }
            }
            for row in 0..real {
                let text = self
                    .tokenizer
                    .decode(&generated[row], true)
                    .unwrap_or_default();
                out[start + row] = clean_caption(text);
            }
        }
        Ok(out)
    }
}
fn load_run(path: &std::path::Path) -> Result<Run> {
    tract_onnx::onnx()
        .model_for_path(path)
        .with_context(|| format!("load {}", path.display()))?
        .into_optimized()?
        .into_runnable()
        .with_context(|| format!("optimize {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nms_keeps_the_best_overlapping_box() {
        let items = vec![
            DetItem { rect: Rect::new(0.0, 0.0, 10.0, 10.0), score: 0.9 },
            DetItem { rect: Rect::new(1.0, 1.0, 10.0, 10.0), score: 0.8 },
        ];
        let k = nms(items, 0.1, 10);
        assert_eq!(k.len(), 1);
        assert_eq!(k[0].score, 0.9);
    }
}
