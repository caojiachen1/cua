// perception/ocr_backend.rs — profile dispatch. PaddleOCR (ORT) is default;
// the tract port remains available for a no-native comparison.
use anyhow::{bail, Result};

use super::ocr::{PpOcr, TextLine};
use super::paddle_ocr::PaddleOcr;
use super::types::OcrProfile;

pub enum AnyOcr {
    Paddle(PaddleOcr),
    Rust(PpOcr),
}

impl AnyOcr {
    pub fn load(profile: OcrProfile, dir: &std::path::Path) -> Result<Self> {
        match profile {
            OcrProfile::PaddleOcr => Ok(Self::Paddle(PaddleOcr::load(dir)?)),
            OcrProfile::PpocrZhDesktop => Ok(Self::Rust(PpOcr::load(dir)?)),
            OcrProfile::EasyocrEnReference => bail!(
                "ocr profile 'easyocr-en-reference' is not installed in this build (available: paddleocr, ppocr-zh-desktop)"
            ),
        }
    }

    pub fn backend(&self) -> &'static str {
        match self {
            Self::Paddle(v) => v.backend(),
            Self::Rust(_) => "tract-cpu/ppocr-rust",
        }
    }

    pub fn recognize(&mut self, w: u32, h: u32, rgba: &[u8]) -> Result<Vec<TextLine>> {
        match self {
            Self::Paddle(v) => v.recognize(w, h, rgba),
            Self::Rust(v) => v.recognize(w, h, rgba),
        }
    }
}
