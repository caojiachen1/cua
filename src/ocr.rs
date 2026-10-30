// ocr.rs — local Windows.Media.Ocr over a captured screenshot. No cloud, no
// model download beyond the OCR language pack Windows already uses.
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

/// Recognize RGBA pixels. Word rectangles are in image pixels.
pub fn recognize(width: u32, height: u32, rgba: &[u8]) -> Result<Value> {
    if width == 0 || height == 0 || rgba.len() < (width as usize * height as usize * 4) {
        bail!("invalid image buffer for OCR");
    }
    let max_dim = OcrEngine::MaxImageDimension().unwrap_or(10000);
    if width > max_dim || height > max_dim {
        bail!(
            "image {width}x{height} exceeds the OCR engine limit ({max_dim}px); capture a smaller region"
        );
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }

    // SoftwareBitmap wants BGRA8; alpha is opaque in captures.
    let mut bgra = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) {
        bgra.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }
    let writer = DataWriter::new().map_err(|e| anyhow!("DataWriter: {e}"))?;
    writer
        .WriteBytes(&bgra)
        .map_err(|e| anyhow!("DataWriter.WriteBytes: {e}"))?;
    let buffer = writer
        .DetachBuffer()
        .map_err(|e| anyhow!("DataWriter.DetachBuffer: {e}"))?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        width as i32,
        height as i32,
    )
    .map_err(|e| anyhow!("SoftwareBitmap.CreateCopyFromBuffer: {e}"))?;

    let engine = match OcrEngine::TryCreateFromUserProfileLanguages() {
        Ok(e) => e,
        Err(first) => {
            let langs = OcrEngine::AvailableRecognizerLanguages()
                .map_err(|e| anyhow!("OCR engine unavailable ({first}); languages: {e}"))?;
            if langs.Size().unwrap_or(0) == 0 {
                bail!(
                    "no OCR language is installed (Settings > Time & Language > Language); \
                     the user-profile OCR engine also failed: {first}"
                );
            }
            let lang = langs.GetAt(0).map_err(|e| anyhow!("OCR language: {e}"))?;
            OcrEngine::TryCreateFromLanguage(&lang)
                .map_err(|e| anyhow!("TryCreateFromLanguage: {e}"))?
        }
    };

    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(|e| anyhow!("RecognizeAsync: {e}"))?
        .get()
        .map_err(|e| anyhow!("OCR recognize: {e}"))?;
    let lines = result.Lines().map_err(|e| anyhow!("OCR lines: {e}"))?;
    let count = lines.Size().unwrap_or(0);
    let mut out_lines = Vec::new();
    let mut all = Vec::new();
    for i in 0..count {
        let line = lines.GetAt(i).map_err(|e| anyhow!("OCR line {i}: {e}"))?;
        let text = line.Text().map(|s| s.to_string_lossy()).unwrap_or_default();
        let words = line.Words().map_err(|e| anyhow!("OCR words: {e}"))?;
        let wcount = words.Size().unwrap_or(0);
        let mut out_words = Vec::new();
        for j in 0..wcount {
            let w = words.GetAt(j).map_err(|e| anyhow!("OCR word {j}: {e}"))?;
            let r = w.BoundingRect().unwrap_or_default();
            out_words.push(json!({
                "text": w.Text().map(|s| s.to_string_lossy()).unwrap_or_default(),
                "rect": [r.X as i32, r.Y as i32, r.Width as i32, r.Height as i32],
            }));
        }
        all.push(text.clone());
        out_lines.push(json!({"text": text, "words": out_words}));
    }
    Ok(json!({
        "text": all.join("\n"),
        "lines": out_lines,
        "line_count": count,
    }))
}

/// Apply the CLI line budget consistently to desktop and sandbox OCR output.
pub fn limit_lines(mut out: Value, max_lines: usize) -> Value {
    let max = max_lines.clamp(1, 1000);
    let mut lines = out["lines"].as_array().cloned().unwrap_or_default();
    let truncated = lines.len() > max;
    if truncated {
        lines.truncate(max);
    }
    let text = lines
        .iter()
        .map(|l| l["text"].as_str().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    out["text"] = json!(text);
    out["lines"] = json!(lines);
    out["truncated"] = json!(truncated);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_buffers() {
        assert!(recognize(0, 0, &[]).is_err());
        assert!(recognize(2, 2, &[0; 4]).is_err());
    }
}
