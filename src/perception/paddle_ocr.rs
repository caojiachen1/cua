// perception/paddle_ocr.rs — the CommandFlow-rs PaddleOCR backend.
// This deliberately uses the same paddle-ocr-rs engine (ONNX Runtime) that
// CommandFlow-rs uses. It is the default OCR profile; the earlier tract port
// remains selectable as ppocr-zh-desktop.
use anyhow::{anyhow, bail, Context, Result};
use paddle_ocr_rs::{
    EngineConfig, ModelConfig, OcrCallOptions, OcrInput, OcrResult, RapidOcrEngine,
};

use super::coordinates::Rect;
use super::ocr::TextLine;

pub struct PaddleOcr {
    engine: RapidOcrEngine,
    backend: &'static str,
}

fn require(path: &std::path::Path) -> Result<()> {
    if !path.is_file() {
        bail!("PaddleOCR asset missing: {}", path.display());
    }
    Ok(())
}

impl PaddleOcr {
    pub fn load(dir: &std::path::Path) -> Result<Self> {
        let det = dir.join("ch_PP-OCRv4_det_infer.onnx");
        let cls = dir.join("ch_ppocr_mobile_v2.0_cls_infer.onnx");
        let rec = dir.join("ch_PP-OCRv4_rec_infer.onnx");
        let keys = dir.join("ppocr_keys_v1.txt");
        for p in [&det, &cls, &rec, &keys] {
            require(p)?;
        }
        let mut cfg = EngineConfig::default();
        cfg.det.model_path = Some(det);
        cfg.det.allow_download = false;
        cfg.det.model_store_dir = Some(dir.to_path_buf());
        cfg.cls.model_path = Some(cls);
        cfg.cls.allow_download = false;
        cfg.cls.model_store_dir = Some(dir.to_path_buf());
        cfg.rec.model = ModelConfig {
            model_path: Some(rec),
            rec_keys_path: Some(keys),
            allow_download: false,
            ..cfg.rec.model
        };
        cfg.rec.model_store_dir = Some(dir.to_path_buf());
        let engine = RapidOcrEngine::new(cfg).map_err(|e| anyhow!("PaddleOCR init: {e}"))?;
        Ok(Self { engine, backend: "paddle-ocr/ort" })
    }

    pub fn backend(&self) -> &'static str {
        self.backend
    }

    pub fn recognize(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<Vec<TextLine>> {
        if width == 0 || height == 0 || rgba.len() < (width * height * 4) as usize {
            bail!("invalid image buffer for PaddleOCR");
        }
        let input = OcrInput::RgbaU8 {
            width: width as usize,
            height: height as usize,
            data: rgba.to_vec(),
        };
        let opts = OcrCallOptions {
            use_det: Some(true),
            use_cls: Some(true),
            use_rec: Some(true),
            ..Default::default()
        };
        let out = self
            .engine
            .run(input, opts)
            .map_err(|e| anyhow!("PaddleOCR run: {e}"))
            .with_context(|| format!("{}x{}", width, height))?;
        let (boxes, txts, scores) = match out {
            OcrResult::Full(f) => (f.boxes, f.txts, f.scores),
            OcrResult::Det(d) => {
                let texts = vec![String::new(); d.boxes.len()];
                let scores = vec![0.0f32; d.boxes.len()];
                (d.boxes, texts, scores)
            }
            _ => return Ok(Vec::new()),
        };
        let mut lines = Vec::with_capacity(txts.len());
        for (i, text) in txts.into_iter().enumerate() {
            let text = text.trim().to_string();
            if text.is_empty() {
                continue;
            }
            let quad = boxes.get(i).copied().unwrap_or([[0.0; 2]; 4]);
            let mut x0 = f32::INFINITY;
            let mut y0 = f32::INFINITY;
            let mut x1 = f32::NEG_INFINITY;
            let mut y1 = f32::NEG_INFINITY;
            for p in quad {
                x0 = x0.min(p[0]);
                y0 = y0.min(p[1]);
                x1 = x1.max(p[0]);
                y1 = y1.max(p[1]);
            }
            if !x0.is_finite() || x1 <= x0 || y1 <= y0 {
                continue;
            }
            let r = Rect::new(x0, y0, x1 - x0, y1 - y0)
                .clamp_to(&Rect::new(0.0, 0.0, width as f32, height as f32));
            if r.w < 2.0 || r.h < 2.0 {
                continue;
            }
            lines.push(TextLine {
                rect: r,
                text,
                confidence: scores.get(i).copied().unwrap_or(0.0),
            });
        }
        Ok(lines)
    }
}
