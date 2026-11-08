// perception/mod.rs — Rust screen perception. OCR defaults to paddle-ocr-rs
// (ONNX Runtime), detection is Burn/WGPU and captions are tract; no Python,
// OpenCV or LibTorch is loaded by the CLI.
pub mod coordinates;
pub mod download;
pub mod easyocr;
pub mod fusion;
pub mod models;
pub mod ocr;
pub mod ocr_backend;
pub mod paddle_ocr;
pub mod pipeline;
pub mod quality;
pub mod router;
pub mod serve;
pub mod template;
pub mod text;
pub mod types;
pub mod vision;
pub mod visualize;
