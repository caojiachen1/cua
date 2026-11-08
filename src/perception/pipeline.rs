// perception/pipeline.rs — one bounded observation: UIA first, then the
// minimum visual evidence the router asks for. Every emitted file/snapshot is
// immutable and has its own id.
use std::time::Instant;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::capture;
use crate::uia;
use crate::util;

use super::coordinates::{FrameMap, Rect};
use super::fusion::{self, DetItem, OcrItem};
use super::models;
use super::ocr;
use super::ocr_backend::AnyOcr;
use super::quality::{self, CaptureFacts};
use super::router::{self, RouteInput, RouteState};
use super::types::*;
use super::vision::{Captioner, Detector};

pub struct Request {
    pub hwnd: isize,
    pub window: Value,
    pub depth: usize,
    pub mode: Mode,
    pub detail: Detail,
    pub ocr_profile: OcrProfile,
    pub no_shot: bool,
    pub out: Option<String>,
    pub stream: bool,
    pub deadline_ms: Option<u64>,
    /// Mask recognised text in the snapshot JSON and blur its pixels in the
    /// saved PNG. Geometry/provenance stay usable.
    pub redact: bool,
}

struct Captured {
    shot: capture::Shot,
    path: std::path::PathBuf,
    origin: [i32; 2],
    dpi: f32,
    scale: f32,
    content_rect: [i32; 4],
}

fn now_iso() -> String {
    util::now_iso()
}

fn clip_rect(r: Rect, w: u32, h: u32) -> Rect {
    r.clamp_to(&Rect::new(0.0, 0.0, w as f32, h as f32))
}

fn element_list(
    nodes: &[uia::Node],
    build: &fusion::VisualBuild,
    frame_id: &str,
    origin: [i32; 2],
    scale: f32,
) -> Vec<Element> {
    let mut uia_elems = Vec::new();
    for n in nodes {
        if let Some(e) = fusion::uia_element(n, frame_id, origin, scale) {
            uia_elems.push(e);
        }
    }
    let visual = fusion::visual_elements(frame_id, build);
    fusion::combine(uia_elems, visual)
}

fn capture_window(hwnd: isize, id: &str) -> Result<Captured> {
    let h = windows::Win32::Foundation::HWND(hwnd as *mut _);
    let shot = capture::shot_window(h, false)?;
    let rect = crate::win::window_rect(h)?;
    // The cache copy is the observation's evidence and is never overwritten by
    // a later observation; --out is only an additional export.
    let path = util::ensure_data_dir()
        .join("cache")
        .join(format!("shot-{hwnd}-{id}.png"));
    capture::save_png(&shot, &path)?;
    Ok(Captured {
        shot,
        path,
        origin: [rect[0], rect[1]],
        dpi: crate::win::dpi_scale(h),
        scale: crate::win::frame_scale(h),
        content_rect: rect,
    })
}

fn frame_info(cap: &Captured, frame_id: &str) -> FrameInfo {
    FrameInfo {
        frame_id: frame_id.to_string(),
        image_width: cap.shot.width,
        image_height: cap.shot.height,
        screen_origin: cap.origin,
        window_rect: cap.content_rect,
        dpi_scale: cap.dpi,
        pixel_scale: cap.scale,
        capture_status: cap.shot.status.to_string(),
        rendered: cap.shot.rendered,
        captured_at: now_iso(),
        content_freshness: Some("unknown".into()),
        client_origin: [0, 0],
    }
}

fn route_for_mode(
    mode: Mode,
    q: &RegionQuality,
    capture: CaptureFacts,
    denied: Option<&str>,
) -> router::Decision {
    if mode == Mode::Auto {
        return router::decide(RouteInput {
            mode,
            quality: q,
            capture,
            permission_denied: denied,
            target_valid: true,
        });
    }
    let mut d = router::decide(RouteInput {
        mode,
        quality: q,
        capture,
        permission_denied: denied,
        target_valid: true,
    });
    if d.state == RouteState::Blocked {
        return d;
    }
    match mode {
        Mode::Uia => d.state = RouteState::UiaPrimary,
        Mode::Vision => d.state = RouteState::VisionPrimary,
        Mode::Hybrid => d.state = RouteState::Hybrid,
        Mode::Auto => {}
    }
    d
}

pub fn run(req: Request) -> Result<i32> {
    let started = Instant::now();
    let deadline = started + std::time::Duration::from_millis(req.deadline_ms.unwrap_or(5000));
    let hwnd = windows::Win32::Foundation::HWND(req.hwnd as *mut _);
    let state_id = util::new_state_id();
    let frame_id = format!("frame-{}", state_id);
    let parent = util::last_observation_id(req.hwnd);
    let mut warnings: Vec<String> = Vec::new();

    // ---- UIA ----
    let t_uia = Instant::now();
    let opts = uia::WalkOptions {
        max_depth: req.depth,
        max_nodes: 2000,
        redact: false,
        with_patterns: false,
        view: uia::WalkView::Control,
    };
    let mut tree = match uia::walk_window_opts(hwnd, opts) {
        Ok(t) => t,
        Err(e) => {
            let text = format!("{e:#}");
            let lower = text.to_ascii_lowercase();
            let denied = lower.contains("access is denied")
                || lower.contains("access denied")
                || lower.contains("0x80070005")
                || lower.contains("permission");
            // A permission failure is never bypassed with pixels; a provider
            // failure only costs the UIA layer.
            if denied || req.mode == Mode::Uia {
                return Err(anyhow!("UIA unavailable: {text}"));
            }
            warnings.push(format!("uia_unavailable: {text}"));
            uia::Tree {
                nodes: Vec::new(),
                sig: "uia_unavailable".into(),
                truncated: true,
                errors: 1,
                last_error: Some(text),
            }
        }
    };
    let root_class = tree.nodes.first().map(|n| n.class.clone()).unwrap_or_default();
    if !req.no_shot && (tree.nodes.len() <= 3 || (root_class.contains("Chrome_WidgetWin") && tree.nodes.len() < 40)) {
        std::thread::sleep(std::time::Duration::from_millis(400));
        if let Ok(second) = uia::walk_window_opts(hwnd, opts) {
            if second.nodes.len() > tree.nodes.len() {
                tree = second;
            }
        }
    }
    let uia_elapsed = t_uia.elapsed().as_millis() as u64;

    // ---- capture ----
    let (mut cap, capture_facts) = if req.no_shot {
        (None, CaptureFacts { ok: false, rendered: false, fresh_known: false })
    } else {
        match capture_window(req.hwnd, &state_id) {
            Ok(c) => {
                let f = CaptureFacts {
                    ok: true,
                    rendered: c.shot.rendered,
                    fresh_known: false,
                };
                (Some(c), f)
            }
            Err(e) => {
                warnings.push(format!("capture: {e:#}"));
                (None, CaptureFacts { ok: false, rendered: false, fresh_known: false })
            }
        }
    };
    let origin = cap.as_ref().map(|c| c.origin).unwrap_or([0, 0]);

    let frame_scale = cap.as_ref().map(|c| c.scale).unwrap_or(1.0);
    let mut quality0 = quality::evaluate(
        &tree.nodes,
        tree.truncated,
        false,
        capture_facts,
        &[],
        origin,
        frame_scale,
    );
    let sampled_actionability = if matches!(req.mode, Mode::Auto | Mode::Hybrid) {
        let (known, ratio) = uia::sample_actionability(hwnd, &tree.nodes, 24);
        if known {
            quality0.actionable = ratio;
            quality0.actionable_known = true;
            quality0.notes.retain(|n| n != "patterns_not_sampled");
            Some(ratio)
        } else {
            None
        }
    } else {
        None
    };
    let mut decision = route_for_mode(req.mode, &quality0, capture_facts, None);
    if req.no_shot && decision.run_vision {
        decision.run_vision = false;
        decision.reasons.push("no_shot".into());
        if decision.state == RouteState::VisionPrimary {
            decision.state = RouteState::UiaPrimary;
        }
    }
    if req.mode == Mode::Vision && req.no_shot {
        return Err(anyhow!(
            "vision requires a new frame; --no-shot and --perception vision conflict (no implicit capture)"
        ));
    }

    // ---- OCR + detector (or a cached visual build for an unchanged frame) ----
    let mut stages = Stages {
        uia: Stage::complete(uia_elapsed),
        ocr: Stage::skipped("not_requested"),
        detector: Stage::skipped("not_requested"),
        fusion: Stage::skipped("not_requested"),
        caption: Stage::skipped("not_requested"),
    };
    let mut ocr_lines: Vec<ocr::TextLine> = Vec::new();
    let mut det_items: Vec<DetItem> = Vec::new();
    let mut ocr_backend = String::new();
    let mut det_backend = String::new();
    let mut caption_backend = String::new();
    let mut build = fusion::VisualBuild::default();
    let mut cache_hit = false;

    if decision.run_vision {
        let shot = cap.as_ref().ok_or_else(|| anyhow!("vision requested without a frame"))?;
        let hash = frame_hash(shot.shot.width, shot.shot.height, &shot.shot.rgba);
        let shot_thumb = frame_thumb(shot.shot.width, shot.shot.height, &shot.shot.rgba);
        if let Some(cached) = load_frame_cache(
            req.hwnd,
            &tree.sig,
            hash,
            &shot_thumb,
            shot.shot.width,
            shot.shot.height,
            frame_scale,
            req.ocr_profile.as_str(),
        ) {
            ocr_backend = cached.0.clone();
            det_backend = cached.1.clone();
            caption_backend = cached.2.clone();
            build = cached.3;
            cache_hit = true;
            stages.ocr = Stage::cached("frame_unchanged");
            stages.detector = Stage::cached("frame_unchanged");
            stages.fusion = Stage::cached("frame_unchanged");
        } else {
            let t_load = Instant::now();
            // OCR and the detector are independent; load both before running so
            // GPU shader compilation and tract optimization overlap.
            let (ocr_loaded, det_loaded) = std::thread::scope(|s| {
                let ocr = s.spawn(|| AnyOcr::load(req.ocr_profile));
                let det = s.spawn(Detector::load);
                (
                    ocr.join().unwrap_or_else(|_| Err(anyhow!("OCR loader panicked"))),
                    det.join().unwrap_or_else(|_| Err(anyhow!("detector loader panicked"))),
                )
            });
            let load_ms = t_load.elapsed().as_millis() as u64;
            match (ocr_loaded, det_loaded) {
                (Ok(engine), Ok(det)) => {
                    ocr_backend = engine.backend().to_string();
                    det_backend = det.backend().to_string();
                    let (ocr_res, det_res) = std::thread::scope(|s| {
                        let h1 = s.spawn(|| {
                            let mut engine = engine;
                            let t = Instant::now();
                            let r = engine.recognize(shot.shot.width, shot.shot.height, &shot.shot.rgba);
                            (r, t.elapsed().as_millis() as u64)
                        });
                        let h2 = s.spawn(|| {
                            let t = Instant::now();
                            let r = det.detect(shot.shot.width, shot.shot.height, &shot.shot.rgba);
                            (r, t.elapsed().as_millis() as u64)
                        });
                        (
                            h1.join().unwrap_or_else(|_| (Err(anyhow!("OCR worker panicked")), 0)),
                            h2.join().unwrap_or_else(|_| (Err(anyhow!("detector worker panicked")), 0)),
                        )
                    });
                    match ocr_res {
                        (Ok(lines), ms) => {
                            ocr_lines = lines;
                            stages.ocr = Stage::complete_loaded(ms, load_ms);
                        }
                        (Err(e), _) => stages.ocr = Stage::failed(format!("{e:#}")),
                    }
                    match det_res {
                        (Ok(items), ms) => {
                            det_items = items;
                            stages.detector = Stage::complete_loaded(ms, load_ms);
                        }
                        (Err(e), _) => stages.detector = Stage::failed(format!("{e:#}")),
                    }
                }
                (Ok(engine), Err(e)) => {
                    stages.detector = Stage::failed(format!("{e:#}"));
                    let backend = engine.backend().to_string();
                    let t = Instant::now();
                    match { let mut engine = engine; engine.recognize(shot.shot.width, shot.shot.height, &shot.shot.rgba) } {
                        Ok(lines) => {
                            ocr_lines = lines;
                            ocr_backend = backend;
                            stages.ocr = Stage::complete_loaded(t.elapsed().as_millis() as u64, load_ms);
                        }
                        Err(e) => stages.ocr = Stage::failed(format!("{e:#}")),
                    }
                }
                (Err(e), Ok(det)) => {
                    stages.ocr = Stage::failed(format!("{e:#}"));
                    det_backend = det.backend().to_string();
                    let t = Instant::now();
                    match det.detect(shot.shot.width, shot.shot.height, &shot.shot.rgba) {
                        Ok(items) => {
                            det_items = items;
                            stages.detector = Stage::complete_loaded(t.elapsed().as_millis() as u64, load_ms);
                        }
                        Err(e) => stages.detector = Stage::failed(format!("{e:#}")),
                    }
                }
                (Err(e1), Err(e2)) => {
                    stages.ocr = Stage::failed(format!("{e1:#}"));
                    stages.detector = Stage::failed(format!("{e2:#}"));
                }
            }
        }
    }
    if decision.run_vision && !cache_hit {
        let ocr_items: Vec<OcrItem> = ocr_lines
            .iter()
            .map(|l| OcrItem {
                rect: clip_rect(l.rect, cap.as_ref().map(|c| c.shot.width).unwrap_or(0), cap.as_ref().map(|c| c.shot.height).unwrap_or(0)),
                text: l.text.clone(),
                confidence: Some(l.confidence),
            })
            .collect();
        build = fusion::fuse_boxes(&det_items, &ocr_items, 0.7);
        stages.fusion = Stage::complete(0);
    } else if !decision.run_vision {
        stages.fusion = Stage::skipped("uia_only");
    }

    // Redaction happens at the source: the snapshot JSON and the saved PNG are
    // both produced from masked data, so no later path can leak the text.
    if req.redact {
        let rects = redaction_rects(&tree.nodes, &build, origin, frame_scale);
        if let Some(c) = cap.as_mut() {
            blur_rects(&mut c.shot, &rects);
            capture::save_png(&c.shot, &c.path)?;
        }
        redact_build(&mut build);
        for n in &mut tree.nodes {
            if !n.name.trim().is_empty() {
                n.name = "***".into();
            }
            if n.value.as_deref().map(|v| !v.trim().is_empty()).unwrap_or(false) {
                n.value = Some("***".into());
            }
        }
    }

    // Export happens after any redaction, so --out can never contain the
    // unblurred frame.
    if let (Some(out), Some(c)) = (req.out.as_deref(), cap.as_ref()) {
        let target = std::path::PathBuf::from(out);
        if target != c.path {
            if let Some(parent) = target.parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
            std::fs::copy(&c.path, &target)
                .map_err(|e| anyhow!("export screenshot {}: {e}", target.display()))?;
        }
    }


    quality0 = quality::evaluate(
        &tree.nodes,
        tree.truncated,
        false,
        capture_facts,
        &build.coverage,
        origin,
        frame_scale,
    );
    if let Some(ratio) = sampled_actionability {
        quality0.actionable = ratio;
        quality0.actionable_known = true;
        quality0.notes.retain(|n| n != "patterns_not_sampled");
    }
    decision = route_for_mode(req.mode, &quality0, capture_facts, None);
    let vision_actually_ran = stages.ocr.is_complete() || stages.detector.is_complete();
    decision.run_vision = vision_actually_ran;
    if req.no_shot && decision.run_vision {
        decision.run_vision = false;
        if decision.state == RouteState::VisionPrimary {
            decision.state = RouteState::UiaPrimary;
        }
    }

    // ---- immutable snapshots ----
    let cache_nodes = serde_json::to_value(uia::nodes_json(&tree.nodes))?;
    let lean_nodes = serde_json::to_value(uia::nodes_json_lean(&tree.nodes))?;
    let screenshot = cap.as_ref().map(|c| {
        json!({
            "path": c.path.to_string_lossy(),
            "width": c.shot.width,
            "height": c.shot.height,
            "rendered": c.shot.rendered,
            "status": c.shot.status,
            "origin": c.origin,
        })
    });

    let mut seq = 1u32;
    let mut revision = 1u32;
    let mut parent_cur = parent.clone();
    let mut current_id = state_id.clone();
    let mut emitted: Option<Value> = None;

    let mut captioner: Option<Captioner> = None;
    let unlabeled = build.icons.iter().filter(|i| i.text.is_none()).count();
    // Per-detail caption budget: fast never captions, balanced only labels the
    // first screenful, full works through the whole icon list.
    let caption_limit = match req.detail {
        Detail::Fast => 0usize,
        Detail::Balanced => 12,
        Detail::Full => usize::MAX,
    };
    if !req.detail.captions() {
        stages.caption = Stage::skipped("fast_detail");
    } else if build.icons.is_empty() {
        stages.caption = Stage::skipped("no_icons");
    } else if unlabeled == 0 {
        stages.caption = Stage::skipped("no_unlabeled_icons");
    } else if Instant::now() >= deadline {
        stages.caption = Stage::skipped("deadline_before_caption");
    } else if req.redact {
        stages.caption = Stage::skipped("redacted");
    } else if caption_limit == 0 {
        stages.caption = Stage::skipped("caption_budget_zero");
    } else {
        stages.caption = Stage::pending("caption_pending");
    }
    let caption_planned = matches!(stages.caption.status, StageStatus::Pending)
        && Instant::now() < deadline;

    // A stream consumer gets the usable fast snapshot before captions. The
    // partial file is immutable and never rewritten; the final revision points
    // at it explicitly.
    if req.stream {
        let partial = assemble(
            &req,
            &current_id,
            seq,
            revision,
            parent_cur.clone(),
            &tree,
            &lean_nodes,
            &screenshot,
            &build,
            &stages,
            &decision,
            &quality0,
            &frame_id,
            cap.as_ref(),
            origin,
            started,
            true,
            Some("caption_pending".into()),
            &warnings,
            &ocr_backend,
            &det_backend,
            &caption_backend,
        );
        save_snapshot(&current_id, req.hwnd, &cache_nodes, &partial)?;
        println!("{}", serde_json::to_string(&partial)?);
        emitted = Some(partial);
        seq += 1;
        revision += 1;
        parent_cur = Some(current_id.clone());
        current_id = util::new_state_id();
    }

    // ---- captions ----
    if caption_planned {
        let batch_pref = std::env::var("CUA_FLORENCE_BATCH")
            .map(|v| v != "0")
            .unwrap_or(true);
        let t_caption_load = Instant::now();
        match Captioner::load_for(batch_pref && unlabeled >= 8) {
            Ok(c) => {
                caption_backend = c.backend_name().to_string();
                captioner = Some(c);
                stages.caption.load_ms = Some(t_caption_load.elapsed().as_millis() as u64);
            }
            Err(e) => stages.caption = Stage::failed(format!("{e:#}")),
        }
    }
    if let Some(cap_engine) = captioner.as_mut() {
        let shot = cap.as_ref().ok_or_else(|| anyhow!("caption without frame"))?;
        let started_caption = Instant::now();
        let batch = cap_engine.batch_size().max(1);
        let mut pending: Vec<usize> = build
            .icons
            .iter()
            .enumerate()
            .filter(|(_, i)| i.text.is_none() && i.caption.is_none())
            .map(|(idx, _)| idx)
            .collect();
        if pending.len() > caption_limit {
            for idx in &pending[caption_limit..] {
                build.icons[*idx].caption_status = "skipped_budget".into();
            }
            pending.truncate(caption_limit);
        }
        let mut done = 0usize;
        let mut pos = 0usize;
        while pos < pending.len() {
            if Instant::now() >= deadline {
                for idx in &pending[pos..] {
                    build.icons[*idx].caption_status = "pending_deadline".into();
                }
                break;
            }
            let end = (pos + batch).min(pending.len());
            let mut crops: Vec<(u32, u32, Vec<u8>)> = Vec::new();
            let mut valid: Vec<usize> = Vec::new();
            for idx in &pending[pos..end] {
                let icon = &build.icons[*idx];
                let r = clip_rect(icon.rect, shot.shot.width, shot.shot.height);
                if r.w < 2.0 || r.h < 2.0 {
                    build.icons[*idx].caption_status = "failed_crop".into();
                    continue;
                }
                let (crop, cw, ch) = crop_rgba(
                    shot.shot.rgba.as_slice(),
                    shot.shot.width,
                    shot.shot.height,
                    &r,
                );
                crops.push((cw, ch, crop));
                valid.push(*idx);
            }
            if !crops.is_empty() {
                match cap_engine.caption_batch(&crops) {
                    Ok(texts) => {
                        for (slot, text) in texts.into_iter().enumerate() {
                            let icon = &mut build.icons[valid[slot]];
                            if text.is_empty() {
                                icon.caption_status = "empty".into();
                            } else {
                                icon.caption = Some(text);
                                icon.caption_status = "complete".into();
                                done += 1;
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
            pos = end;
        }
        // "empty" means Florence ran and produced no useful caption; that is a
        // completed attempt, not a pending stage. Only deadline/crop failures
        // leave the stage partial.
        let unfinished = build
            .icons
            .iter()
            .filter(|i| {
                i.text.is_none()
                    && (i.caption_status == "pending"
                        || i.caption_status == "pending_deadline"
                        || i.caption_status.starts_with("failed"))
            })
            .count();
        stages.caption = if unfinished == 0 {
            Stage::complete(started_caption.elapsed().as_millis() as u64)
        } else {
            Stage {
                status: StageStatus::Pending,
                reason: Some(format!("{unfinished}_icons_unfinished")),
                elapsed_ms: Some(started_caption.elapsed().as_millis() as u64),
                load_ms: None,
                cache_hit: false,
            }
        };
    }

    if decision.run_vision && !cache_hit && !req.redact {
        if let Some(c) = cap.as_ref() {
            let hash = frame_hash(c.shot.width, c.shot.height, &c.shot.rgba);
            let _ = save_frame_cache(
                req.hwnd,
                &tree.sig,
                hash,
                &frame_thumb(c.shot.width, c.shot.height, &c.shot.rgba),
                c.shot.width,
                c.shot.height,
                frame_scale,
                req.ocr_profile.as_str(),
                &ocr_backend,
                &det_backend,
                &caption_backend,
                &build,
            );
        }
    }

    let final_snapshot = assemble(
        &req,
        &current_id,
        seq,
        revision,
        parent_cur,
        &tree,
        &lean_nodes,
        &screenshot,
        &build,
        &stages,
        &decision,
        &quality0,
        &frame_id,
        cap.as_ref(),
        origin,
        started,
        false,
        None,
        &warnings,
        &ocr_backend,
        &det_backend,
        &caption_backend,
    );
    save_snapshot(&current_id, req.hwnd, &cache_nodes, &final_snapshot)?;
    if let Some(partial) = emitted {
        if req.stream {
            // final line follows the partial; both are separate JSON documents
            println!("{}", serde_json::to_string(&final_snapshot)?);
        } else {
            let _ = partial;
            println!("{}", serde_json::to_string_pretty(&final_snapshot)?);
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&final_snapshot)?);
    }
    Ok(0)
}

fn save_snapshot(
    id: &str,
    hwnd: isize,
    cache_nodes: &Value,
    snapshot: &Value,
) -> Result<()> {
    let mut cache = snapshot.clone();
    if let Some(o) = cache.as_object_mut() {
        o.insert("nodes".into(), cache_nodes.clone());
        o.remove("uia_nodes");
        o.insert("schema".into(), json!(SCHEMA_V2));
    }
    util::save_observation_value(id, hwnd, &cache)?;
    util::save_last_observation(hwnd, id);
    // Keep the v1 screenshotId guard usable for coordinate actions that follow
    // a v2 observation: the same immutable id is the fresh shot id.
    if let Some(sig) = snapshot["uia_sig"].as_str().or_else(|| snapshot["sig"].as_str()) {
        util::save_last_state(id, hwnd, sig);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn assemble(
    req: &Request,
    id: &str,
    seq: u32,
    revision: u32,
    parent: Option<String>,
    tree: &uia::Tree,
    lean_nodes: &Value,
    screenshot: &Option<Value>,
    build: &fusion::VisualBuild,
    stages: &Stages,
    decision: &router::Decision,
    quality: &RegionQuality,
    frame_id: &str,
    cap: Option<&Captured>,
    origin: [i32; 2],
    started: Instant,
    partial: bool,
    partial_reason: Option<String>,
    warnings: &[String],
    ocr_backend: &str,
    det_backend: &str,
    caption_backend: &str,
) -> Value {
    let mut stages = stages.clone();
    if partial {
        stages.caption = Stage::pending("caption_pending");
    }
    let elements = element_list(
        &tree.nodes,
        build,
        frame_id,
        origin,
        cap.map(|c| c.scale).unwrap_or(1.0),
    );
    let mut q = quality.clone();
    q.state = decision.state.as_str().to_string();
    let mut qs = vec![q];
    if tree.truncated {
        qs[0].notes.push("tree_truncated".into());
    }
    let stages_json = serde_json::to_value(&stages).unwrap_or(Value::Null);
    let missing = if partial {
        Some(partial_reason.unwrap_or_else(|| "caption_pending".into()))
    } else {
        let mut miss = Vec::new();
        for (name, s) in [
            ("ocr", &stages.ocr),
            ("detector", &stages.detector),
            ("caption", &stages.caption),
        ] {
            if s.status == StageStatus::Failed {
                miss.push(format!("{name}_failed"));
            }
            if s.status == StageStatus::Pending {
                miss.push(format!("{name}_pending"));
            }
        }
        if miss.is_empty() { None } else { Some(miss.join(",")) }
    };
    let status = if partial {
        "partial"
    } else if missing.is_some() {
        "partial"
    } else {
        "complete"
    };
    json!({
        "schema": SCHEMA_V2,
        "request_id": format!("req-{id}"),
        "seq": seq,
        "observation_id": id,
        "frame_id": frame_id,
        "revision": revision,
        "parent": parent,
        "created": now_iso(),
        "hwnd": req.hwnd,
        "sig": tree.sig,
        "truncated": tree.truncated,
        "window": req.window,
        "perception": {
            "mode": req.mode.as_str(),
            "detail": req.detail.as_str(),
            "ocr_profile": req.ocr_profile.as_str(),
            "backend": {
                "ocr": ocr_backend,
                "detector": det_backend,
                "caption": if caption_backend.is_empty() { "not_loaded" } else { caption_backend },
            },
            "model_revision": models::OMNI_REVISION,
            "route": {
                "requested": req.mode.as_str(),
                "state": decision.state.as_str(),
                "reason": decision.reason(),
                "reasons": decision.reasons,
                "vision_used": decision.run_vision,
                "dead_ends": decision.blocked,
            },
            "quality": qs,
            "warnings": warnings,
        },
        "stages": stages_json,
        "frame": cap.map(|c| frame_info(c, frame_id)),
        "image": cap.map(|c| json!({"width": c.shot.width, "height": c.shot.height})),
        "elements": elements,
        "uia_nodes": lean_nodes,
        "screenshot": screenshot,
        "status": status,
        "terminal": !partial,
        "reason": missing,
        "elapsed_ms": started.elapsed().as_millis() as u64,
    })
}

fn crop_rgba(rgba: &[u8], width: u32, height: u32, r: &Rect) -> (Vec<u8>, u32, u32) {
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


// ---------------- incremental reuse ----------------

fn cache_enabled() -> bool {
    std::env::var("CUA_PERCEPTION_CACHE")
        .map(|v| v != "0")
        .unwrap_or(true)
}

fn cache_path(hwnd: isize) -> std::path::PathBuf {
    util::ensure_data_dir()
        .join("cache")
        .join(format!("percept-{hwnd}.json"))
}

/// Cheap deterministic fingerprint of the captured frame. Sampling keeps this
/// O(few thousand reads) even for a 4K window.
fn frame_hash(w: u32, h: u32, rgba: &[u8]) -> u64 {
    let mut acc: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u8| {
        acc ^= v as u64;
        acc = acc.wrapping_mul(0x100_0000_01b3);
    };
    mix(w as u8);
    mix((w >> 8) as u8);
    mix(h as u8);
    mix((h >> 8) as u8);
    let n = (w as usize).saturating_mul(h as usize).max(1);
    let step = (n / 8192).max(1);
    let mut i = 0usize;
    while i < n {
        let p = i * 4;
        if p + 2 < rgba.len() {
            mix(rgba[p]);
            mix(rgba[p + 1]);
            mix(rgba[p + 2]);
        }
        i += step;
    }
    acc
}

/// 32x32 luma thumbnail. Comparing thumbnails tolerates a blinking caret or a
/// hover animation while still catching a real layout/content change.
fn frame_thumb(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    const N: u32 = 32;
    let mut out = vec![0u8; (N * N) as usize];
    if w == 0 || h == 0 {
        return out;
    }
    for y in 0..N {
        for x in 0..N {
            let mut sum = 0u32;
            let mut count = 0u32;
            for sy in 0..3u32 {
                for sx in 0..3u32 {
                    let px = ((x * 3 + sx) * w / (N * 3)).min(w - 1);
                    let py = ((y * 3 + sy) * h / (N * 3)).min(h - 1);
                    let p = ((py * w + px) * 4) as usize;
                    if p + 2 < rgba.len() {
                        sum += (rgba[p] as u32 * 299 + rgba[p + 1] as u32 * 587 + rgba[p + 2] as u32 * 114) / 1000;
                        count += 1;
                    }
                }
            }
            out[(y * N + x) as usize] = if count > 0 { (sum / count) as u8 } else { 0 };
        }
    }
    out
}

fn thumbs_close(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() || a.is_empty() {
        return false;
    }
    let changed = a
        .iter()
        .zip(b.iter())
        .filter(|(x, y)| ((**x as i16) - (**y as i16)).abs() > 24)
        .count();
    // up to 2% of cells may differ (caret, hover, antialiasing)
    changed * 50 <= a.len()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CachedIcon {
    rect: [f32; 4],
    score: f32,
    text: Option<String>,
    ocr_confidence: Option<f32>,
    caption: Option<String>,
    caption_status: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CachedText {
    rect: [f32; 4],
    text: String,
    confidence: Option<f32>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct FrameCacheFile {
    sig: String,
    hash: u64,
    width: u32,
    height: u32,
    scale: f32,
    #[serde(default)]
    thumb: Vec<u8>,
    #[serde(default)]
    ocr_profile: String,
    #[serde(default)]
    ocr_backend: String,
    #[serde(default)]
    det_backend: String,
    #[serde(default)]
    caption_backend: String,
    icons: Vec<CachedIcon>,
    texts: Vec<CachedText>,
}

fn load_frame_cache(
    hwnd: isize,
    sig: &str,
    hash: u64,
    thumb: &[u8],
    w: u32,
    h: u32,
    scale: f32,
    ocr_profile: &str,
) -> Option<(String, String, String, fusion::VisualBuild)> {
    if !cache_enabled() {
        return None;
    }
    let bytes = std::fs::read(cache_path(hwnd)).ok()?;
    let cached: FrameCacheFile = serde_json::from_slice(&bytes).ok()?;
    if cached.sig != sig
        || (cached.hash != hash && !thumbs_close(&cached.thumb, thumb))
        || cached.width != w
        || cached.height != h
        || (cached.scale - scale).abs() > 0.001
        || cached.ocr_profile != ocr_profile
    {
        return None;
    }
    let mut build = fusion::VisualBuild::default();
    for i in cached.icons {
        build.icons.push(fusion::FusedIcon {
            rect: Rect::new(i.rect[0], i.rect[1], i.rect[2], i.rect[3]),
            score: i.score,
            text: i.text,
            ocr_confidence: i.ocr_confidence,
            caption: i.caption,
            caption_status: i.caption_status,
        });
    }
    for t in cached.texts {
        build.texts.push(OcrItem {
            rect: Rect::new(t.rect[0], t.rect[1], t.rect[2], t.rect[3]),
            text: t.text,
            confidence: t.confidence,
        });
    }
    for t in &build.texts {
        build.coverage.push(quality::VisualCandidate {
            rect: t.rect,
            text: Some(t.text.clone()),
        });
    }
    for i in &build.icons {
        build.coverage.push(quality::VisualCandidate {
            rect: i.rect,
            text: i.text.clone(),
        });
    }
    Some((
        cached.ocr_backend,
        cached.det_backend,
        cached.caption_backend,
        build,
    ))
}

fn save_frame_cache(
    hwnd: isize,
    sig: &str,
    hash: u64,
    thumb: &[u8],
    w: u32,
    h: u32,
    scale: f32,
    ocr_profile: &str,
    ocr_backend: &str,
    det_backend: &str,
    caption_backend: &str,
    build: &fusion::VisualBuild,
) -> Result<()> {
    if !cache_enabled() {
        return Ok(());
    }
    let file = FrameCacheFile {
        sig: sig.to_string(),
        hash,
        width: w,
        height: h,
        scale,
        thumb: thumb.to_vec(),
        ocr_profile: ocr_profile.to_string(),
        ocr_backend: ocr_backend.to_string(),
        det_backend: det_backend.to_string(),
        caption_backend: caption_backend.to_string(),
        icons: build
            .icons
            .iter()
            .map(|i| CachedIcon {
                rect: i.rect.to_array(),
                score: i.score,
                text: i.text.clone(),
                ocr_confidence: i.ocr_confidence,
                caption: i.caption.clone(),
                caption_status: i.caption_status.clone(),
            })
            .collect(),
        texts: build
            .texts
            .iter()
            .map(|t| CachedText {
                rect: t.rect.to_array(),
                text: t.text.clone(),
                confidence: t.confidence,
            })
            .collect(),
    };
    let path = cache_path(hwnd);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&file)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

// ---------------- redaction ----------------

fn redaction_rects(
    nodes: &[uia::Node],
    build: &fusion::VisualBuild,
    origin: [i32; 2],
    scale: f32,
) -> Vec<Rect> {
    let mut out = Vec::new();
    for n in nodes {
        if n.offscreen || n.rect[2] <= 0 || n.rect[3] <= 0 {
            continue;
        }
        let has_text = !n.name.trim().is_empty()
            || n.value.as_deref().map(|v| !v.trim().is_empty()).unwrap_or(false);
        if !has_text {
            continue;
        }
        out.push(Rect::new(
            (n.rect[0] - origin[0]) as f32 / scale,
            (n.rect[1] - origin[1]) as f32 / scale,
            n.rect[2] as f32 / scale,
            n.rect[3] as f32 / scale,
        ));
    }
    out.extend(build.texts.iter().map(|t| t.rect));
    out.extend(build.icons.iter().map(|i| i.rect));
    out
}

/// Mosaic each region so the saved frame cannot be read back as text.
fn blur_rects(shot: &mut capture::Shot, rects: &[Rect]) {
    const BLOCK: usize = 6;
    let w = shot.width as usize;
    let h = shot.height as usize;
    if w == 0 || h == 0 {
        return;
    }
    for r in rects {
        let x0 = r.x.max(0.0) as usize;
        let y0 = r.y.max(0.0) as usize;
        let x1 = ((r.x + r.w).max(0.0) as usize).min(w);
        let y1 = ((r.y + r.h).max(0.0) as usize).min(h);
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let mut by = y0;
        while by < y1 {
            let ey = (by + BLOCK).min(y1);
            let mut bx = x0;
            while bx < x1 {
                let ex = (bx + BLOCK).min(x1);
                let mut sum = [0u32; 3];
                let mut n = 0u32;
                for yy in by..ey {
                    for xx in bx..ex {
                        let p = (yy * w + xx) * 4;
                        sum[0] += shot.rgba[p] as u32;
                        sum[1] += shot.rgba[p + 1] as u32;
                        sum[2] += shot.rgba[p + 2] as u32;
                        n += 1;
                    }
                }
                if n > 0 {
                    let avg = [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8];
                    for yy in by..ey {
                        for xx in bx..ex {
                            let p = (yy * w + xx) * 4;
                            shot.rgba[p] = avg[0];
                            shot.rgba[p + 1] = avg[1];
                            shot.rgba[p + 2] = avg[2];
                        }
                    }
                }
                bx = ex;
            }
            by = ey;
        }
    }
}

fn redact_build(build: &mut fusion::VisualBuild) {
    for i in &mut build.icons {
        if i.text.is_some() {
            i.text = Some("***".into());
        }
        if i.caption.is_some() {
            i.caption = Some("***".into());
        }
    }
    for t in &mut build.texts {
        t.text = "***".into();
    }
    for c in &mut build.coverage {
        if c.text.is_some() {
            c.text = Some("***".into());
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn shot(w: u32, h: u32, rgba: Vec<u8>) -> capture::Shot {
        capture::Shot { width: w, height: h, rgba, rendered: true, status: "captured" }
    }

    #[test]
    fn frame_hash_is_stable_and_pixel_sensitive() {
        let a = vec![10u8; 64 * 64 * 4];
        let mut b = a.clone();
        b[(32 * 64 + 32) * 4] = 200;
        let h0 = frame_hash(64, 64, &a);
        assert_eq!(h0, frame_hash(64, 64, &a));
        assert_ne!(h0, frame_hash(64, 64, &b));
    }

    #[test]
    fn redaction_masks_text_but_keeps_geometry() {
        let mut build = fusion::VisualBuild::default();
        build.texts.push(OcrItem {
            rect: Rect::new(1.0, 2.0, 3.0, 4.0),
            text: "secret".into(),
            confidence: Some(0.9),
        });
        build.icons.push(fusion::FusedIcon {
            rect: Rect::new(5.0, 6.0, 7.0, 8.0),
            score: 0.8,
            text: Some("user@example.com".into()),
            ocr_confidence: Some(0.9),
            caption: Some("avatar".into()),
            caption_status: "complete".into(),
        });
        redact_build(&mut build);
        assert_eq!(build.texts[0].text, "***");
        assert_eq!(build.texts[0].rect.x, 1.0);
        assert_eq!(build.icons[0].text.as_deref(), Some("***"));
        assert_eq!(build.icons[0].caption.as_deref(), Some("***"));
        assert_eq!(build.icons[0].rect.w, 7.0);
    }

    #[test]
    fn blur_flattens_pixels_inside_a_rect() {
        let (w, h) = (24u32, 24u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                let p = (y * w as usize + x) * 4;
                let v = if (x + y) % 2 == 0 { 0 } else { 255 };
                rgba[p] = v;
                rgba[p + 1] = v;
                rgba[p + 2] = v;
                rgba[p + 3] = 255;
            }
        }
        let mut s = shot(w, h, rgba);
        blur_rects(&mut s, &[Rect::new(0.0, 0.0, 24.0, 24.0)]);
        for y in 0..6usize {
            let mut values = Vec::new();
            for x in 0..6usize {
                values.push(s.rgba[(y * 24 + x) * 4]);
            }
            let max = *values.iter().max().unwrap();
            let min = *values.iter().min().unwrap();
            assert!(max - min <= 1, "block not flattened: {values:?}");
        }
    }
}
