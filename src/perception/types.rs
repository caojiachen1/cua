// perception/types.rs — stable data model shared by the CLI, fusion and the
// perception runtime. Nothing here captures, infers or mutates state.
use serde::{Deserialize, Serialize};

pub const SCHEMA_V2: &str = "cua-observation-v2";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Auto,
    Uia,
    Vision,
    Hybrid,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Uia => "uia",
            Mode::Vision => "vision",
            Mode::Hybrid => "hybrid",
        }
    }
    pub fn wants_vision(self) -> bool {
        matches!(self, Mode::Auto | Mode::Vision | Mode::Hybrid)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Detail {
    Fast,
    Balanced,
    Full,
}

impl Detail {
    pub fn as_str(self) -> &'static str {
        match self {
            Detail::Fast => "fast",
            Detail::Balanced => "balanced",
            Detail::Full => "full",
        }
    }
    pub fn captions(self) -> bool {
        !matches!(self, Detail::Fast)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum OcrProfile {
    /// CommandFlow-rs' RapidOCR PP-OCRv4 engine (ONNX Runtime). Default.
    #[serde(rename = "paddleocr")]
    #[value(name = "paddleocr")]
    PaddleOcr,
    /// The official OmniParser service OCR (EasyOCR CRAFT + English recognizer).
    EasyocrEnReference,
    /// The earlier pure-Rust tract port of PP-OCRv4 (kept as an option).
    PpocrZhDesktop,
}

impl OcrProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            OcrProfile::PaddleOcr => "paddleocr",
            OcrProfile::EasyocrEnReference => "easyocr-en-reference",
            OcrProfile::PpocrZhDesktop => "ppocr-zh-desktop",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    Pending,
    Complete,
    Failed,
    Skipped,
}

impl StageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            StageStatus::Pending => "pending",
            StageStatus::Complete => "complete",
            StageStatus::Failed => "failed",
            StageStatus::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stage {
    pub status: StageStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cache_hit: bool,
}

impl Stage {
    pub fn pending(reason: impl Into<String>) -> Self {
        Self { status: StageStatus::Pending, reason: Some(reason.into()), elapsed_ms: None, cache_hit: false }
    }
    pub fn complete(elapsed_ms: u64) -> Self {
        Self { status: StageStatus::Complete, reason: None, elapsed_ms: Some(elapsed_ms), cache_hit: false }
    }
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self { status: StageStatus::Skipped, reason: Some(reason.into()), elapsed_ms: None, cache_hit: false }
    }
    pub fn failed(reason: impl Into<String>) -> Self {
        Self { status: StageStatus::Failed, reason: Some(reason.into()), elapsed_ms: None, cache_hit: false }
    }
    pub fn is_complete(&self) -> bool {
        self.status == StageStatus::Complete
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stages {
    pub uia: Stage,
    pub ocr: Stage,
    pub detector: Stage,
    pub fusion: Stage,
    pub caption: Stage,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    Uia,
    Ocr,
    Detector,
    Caption,
    Fused,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Uia => "uia",
            Source::Ocr => "ocr",
            Source::Detector => "detector",
            Source::Caption => "caption",
            Source::Fused => "fused",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Confidence {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uia: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detector: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actionability: Option<f32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TargetInfo {
    /// Action channels that are proven for this element. Empty means unknown,
    /// never an implicit permission to use physical input.
    pub channels: Vec<String>,
}

impl TargetInfo {
    pub fn unknown() -> Self {
        Self { channels: Vec::new() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Relation {
    pub kind: String,
    pub element_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Element {
    pub element_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// x, y, w, h in the frame's pixel coordinate space
    pub rect_px: [f32; 4],
    pub coordinate_space: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    pub provenance: Source,
    pub confidence: Confidence,
    pub target: TargetInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uia_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uia_rid: Option<Vec<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uia_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption_status: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<Relation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrameInfo {
    pub frame_id: String,
    pub image_width: u32,
    pub image_height: u32,
    /// screen coordinates of the image origin (physical pixels, DPI aware)
    pub screen_origin: [i32; 2],
    /// window rectangle at capture time (physical pixels)
    pub window_rect: [i32; 4],
    pub dpi_scale: f32,
    pub capture_status: String,
    pub rendered: bool,
    pub captured_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_freshness: Option<String>,
    /// image origin inside the window client area (crop offset)
    pub client_origin: [i32; 2],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegionQuality {
    pub rect_px: [f32; 4],
    pub state: String,
    pub coverage: Option<f32>,
    pub readable: f32,
    pub locatable: f32,
    pub actionable: f32,
    pub actionable_known: bool,
    pub leaf_count: usize,
    pub named_leaves: usize,
    pub capture_ok: bool,
    pub truncated: bool,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteInfo {
    pub requested: String,
    pub state: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    pub vision_used: bool,
    pub dead_ends: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PerceptionMeta {
    pub mode: String,
    pub detail: String,
    pub ocr_profile: String,
    pub backend: String,
    pub model_revision: String,
    pub route: RouteInfo,
    pub quality: Vec<RegionQuality>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotV2 {
    pub schema: String,
    pub request_id: String,
    pub seq: u32,
    pub observation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<String>,
    pub revision: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub created: String,
    pub hwnd: isize,
    pub window: serde_json::Value,
    pub perception: PerceptionMeta,
    pub stages: Stages,
    pub frame: Option<FrameInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<Element>,
    /// UIA nodes are kept in the same immutable snapshot for index actions.
    /// They are the exact tree the UIA indices refer to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uia_nodes: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<serde_json::Value>,
    pub status: String,
    pub terminal: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub elapsed_ms: u64,
}

impl SnapshotV2 {
    pub fn partial_reason(&self) -> Option<String> {
        let mut missing = Vec::new();
        for (name, s) in [
            ("uia", &self.stages.uia),
            ("ocr", &self.stages.ocr),
            ("detector", &self.stages.detector),
            ("fusion", &self.stages.fusion),
            ("caption", &self.stages.caption),
        ] {
            if s.status == StageStatus::Pending || s.status == StageStatus::Failed {
                missing.push(format!("{name}:{}", s.status.as_str()));
            }
        }
        if missing.is_empty() { None } else { Some(missing.join(",")) }
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
