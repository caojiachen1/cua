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
}

struct Captured {
    shot: capture::Shot,
    path: std::path::PathBuf,
    origin: [i32; 2],
    dpi: f32,
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
) -> Vec<Element> {
    let mut uia_elems = Vec::new();
    for n in nodes {
        if let Some(e) = fusion::uia_element(n, frame_id, origin) {
            uia_elems.push(e);
        }
    }
    let visual = fusion::visual_elements(frame_id, build);
    fusion::combine(uia_elems, visual)
}

fn capture_window(hwnd: isize, out: Option<&str>, id: &str) -> Result<Captured> {
    let h = windows::Win32::Foundation::HWND(hwnd as *mut _);
    let shot = capture::shot_window(h, false)?;
    let rect = crate::win::window_rect(h)?;
    let path = out.map(std::path::PathBuf::from).unwrap_or_else(|| {
        util::ensure_data_dir()
            .join("cache")
            .join(format!("shot-{hwnd}-{id}.png"))
    });
    capture::save_png(&shot, &path)?;
    Ok(Captured {
        shot,
        path,
        origin: [rect[0], rect[1]],
        dpi: crate::win::dpi_scale(h),
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
    let mut tree = uia::walk_window_opts(hwnd, opts)?;
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
    let (cap, capture_facts) = if req.no_shot {
        (None, CaptureFacts { ok: false, rendered: false, fresh_known: false })
    } else {
        match capture_window(req.hwnd, req.out.as_deref(), &state_id) {
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

    let mut quality0 = quality::evaluate(
        &tree.nodes,
        tree.truncated,
        false,
        capture_facts,
        &[],
        origin,
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

    // ---- OCR + detector ----
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

    if decision.run_vision {
        let shot = cap.as_ref().ok_or_else(|| anyhow!("vision requested without a frame"))?;
        let want_ocr = true;
        let t_load = Instant::now();
        // OCR and the detector are independent; load both before running so
        // GPU shader compilation and tract optimization overlap.
        let (ocr_loaded, det_loaded) = std::thread::scope(|s| {
            let ocr = if want_ocr {
                Some(s.spawn(|| AnyOcr::load(req.ocr_profile)))
            } else {
                None
            };
            let det = s.spawn(Detector::load);
            let ocr_result = match ocr {
                Some(h) => h.join().unwrap_or_else(|_| Err(anyhow!("OCR loader panicked"))),
                None => Err(anyhow!(
                    "ocr profile '{}' is not installed in this build",
                    req.ocr_profile.as_str()
                )),
            };
            (
                ocr_result,
                det.join().unwrap_or_else(|_| Err(anyhow!("detector loader panicked"))),
            )
        });
        match (ocr_loaded, det_loaded) {
            (Ok(engine), Ok(det)) => {
                ocr_backend = engine.backend().to_string();
                det_backend = det.backend().to_string();
                let t = Instant::now();
                let (ocr_res, det_res) = std::thread::scope(|s| {
                    let h1 = s.spawn(|| {
                        let mut engine = engine;
                        engine.recognize(shot.shot.width, shot.shot.height, &shot.shot.rgba)
                    });
                    let h2 = s.spawn(|| det.detect(shot.shot.width, shot.shot.height, &shot.shot.rgba));
                    (
                        h1.join().unwrap_or_else(|_| Err(anyhow!("OCR worker panicked"))),
                        h2.join().unwrap_or_else(|_| Err(anyhow!("detector worker panicked"))),
                    )
                });
                match ocr_res {
                    Ok(lines) => {
                        ocr_lines = lines;
                        stages.ocr = Stage::complete(t_load.elapsed().as_millis() as u64);
                    }
                    Err(e) => stages.ocr = Stage::failed(format!("{e:#}")),
                }
                match det_res {
                    Ok(items) => {
                        det_items = items;
                        stages.detector = Stage::complete(t_load.elapsed().as_millis() as u64);
                    }
                    Err(e) => stages.detector = Stage::failed(format!("{e:#}")),
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
                        stages.ocr = Stage::complete(t_load.elapsed().max(t.elapsed()).as_millis() as u64);
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
                        stages.detector = Stage::complete(t_load.elapsed().max(t.elapsed()).as_millis() as u64);
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
    let ocr_items: Vec<OcrItem> = ocr_lines
        .iter()
        .map(|l| OcrItem {
            rect: clip_rect(l.rect, cap.as_ref().map(|c| c.shot.width).unwrap_or(0), cap.as_ref().map(|c| c.shot.height).unwrap_or(0)),
            text: l.text.clone(),
            confidence: Some(l.confidence),
        })
        .collect();
    let mut build = fusion::fuse_boxes(&det_items, &ocr_items, 0.7);
    stages.fusion = if decision.run_vision {
        Stage::complete(0)
    } else {
        Stage::skipped("uia_only")
    };

    quality0 = quality::evaluate(
        &tree.nodes,
        tree.truncated,
        false,
        capture_facts,
        &build.coverage,
        origin,
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

    let captions_requested = req.detail.captions() && !build.icons.is_empty();
    let mut captioner: Option<Captioner> = None;
    if captions_requested {
        let unlabeled = build.icons.iter().filter(|i| i.text.is_none()).count();
        if unlabeled == 0 {
            stages.caption = Stage::skipped("no_unlabeled_icons");
        } else if Instant::now() >= deadline {
            stages.caption = Stage::skipped("deadline_before_caption");
        } else {
            let unlabeled = build.icons.iter().filter(|i| i.text.is_none()).count();
            let batch_pref = std::env::var("CUA_FLORENCE_BATCH")
                .map(|v| v != "0")
                .unwrap_or(true);
            match Captioner::load_for(batch_pref && unlabeled >= 8) {
                Ok(c) => {
                    caption_backend = c.backend_name().to_string();
                    captioner = Some(c);
                    stages.caption = Stage::complete(0);
                }
                Err(e) => stages.caption = Stage::failed(format!("{e:#}")),
            }
        }
    } else if req.detail.captions() {
        stages.caption = Stage::skipped("no_icons");
    } else {
        stages.caption = Stage::skipped("fast_detail");
    }

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
    if let Some(cap_engine) = captioner.as_mut() {
        let shot = cap.as_ref().ok_or_else(|| anyhow!("caption without frame"))?;
        let batch = cap_engine.batch_size().max(1);
        let pending: Vec<usize> = build
            .icons
            .iter()
            .enumerate()
            .filter(|(_, i)| i.text.is_none() && i.caption.is_none())
            .map(|(idx, _)| idx)
            .collect();
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
            Stage::complete(done as u64)
        } else {
            Stage { status: StageStatus::Pending, reason: Some(format!("{unfinished}_icons_unfinished")), elapsed_ms: Some(done as u64), cache_hit: false }
        };
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
    let elements = element_list(&tree.nodes, build, frame_id, origin);
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
