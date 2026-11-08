// perception/router.rs — eligibility first, then per-task evidence.
// A permission denial is never "fixed" by switching to vision. A capture
// failure does not make a good UIA tree bad; it only disables visual evidence.
use super::quality::CaptureFacts;
use super::types::{Mode, RegionQuality};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteState {
    Probing,
    UiaPrimary,
    Hybrid,
    VisionPrimary,
    Blocked,
}

impl RouteState {
    pub fn as_str(self) -> &'static str {
        match self {
            RouteState::Probing => "probing",
            RouteState::UiaPrimary => "uia_primary",
            RouteState::Hybrid => "hybrid",
            RouteState::VisionPrimary => "vision_primary",
            RouteState::Blocked => "blocked",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Decision {
    pub state: RouteState,
    pub run_uia: bool,
    pub run_vision: bool,
    pub reasons: Vec<String>,
    pub blocked: bool,
}

impl Decision {
    fn new(state: RouteState, run_uia: bool, run_vision: bool, reasons: Vec<String>) -> Self {
        Self {
            state,
            run_uia,
            run_vision,
            reasons,
            blocked: state == RouteState::Blocked,
        }
    }
    pub fn reason(&self) -> String {
        self.reasons.join(";")
    }
}

pub struct RouteInput<'a> {
    pub mode: Mode,
    pub quality: &'a RegionQuality,
    pub capture: CaptureFacts,
    pub permission_denied: Option<&'a str>,
    pub target_valid: bool,
}

pub fn decide(input: RouteInput<'_>) -> Decision {
    let q = input.quality;
    if let Some(reason) = input.permission_denied {
        return Decision::new(
            RouteState::Blocked,
            false,
            false,
            vec![format!("permission_denied:{reason}")],
        );
    }
    if !input.target_valid {
        return Decision::new(RouteState::Blocked, false, false, vec!["target_invalid".into()]);
    }
    let capture_ok = input.capture.ok && input.capture.rendered;
    let has_uia = q.leaf_count > 0;
    let mut reasons = Vec::new();
    if q.truncated {
        reasons.push("uia_truncated".into());
    }
    if !capture_ok {
        reasons.push("capture_unusable".into());
    }
    if q.coverage.is_none() {
        reasons.push("no_visual_baseline".into());
    }

    match input.mode {
        Mode::Uia => {
            let mut r = reasons;
            r.push("explicit_uia".into());
            Decision::new(RouteState::UiaPrimary, true, false, r)
        }
        Mode::Vision => {
            if !capture_ok {
                return Decision::new(RouteState::Blocked, has_uia, false, {
                    reasons.push("vision_requires_capture".into());
                    reasons
                });
            }
            let mut r = reasons;
            r.push("explicit_vision".into());
            Decision::new(RouteState::VisionPrimary, has_uia, true, r)
        }
        Mode::Hybrid => {
            if !capture_ok {
                return Decision::new(RouteState::UiaPrimary, has_uia, false, {
                    reasons.push("hybrid_without_capture_uia_only".into());
                    reasons
                });
            }
            let mut r = reasons;
            r.push("explicit_hybrid".into());
            Decision::new(RouteState::Hybrid, has_uia, true, r)
        }
        Mode::Auto => {
            // Without pixels there is no justification for failing a usable
            // tree; report the capture gap and keep the UIA path.
            if !capture_ok {
                if has_uia {
                    reasons.push("auto_capture_bad_keep_uia".into());
                    return Decision::new(RouteState::UiaPrimary, true, false, reasons);
                }
                reasons.push("auto_unobservable".into());
                return Decision::new(RouteState::Blocked, false, false, reasons);
            }
            if !has_uia && q.coverage.is_none() {
                reasons.push("no_uia_baseline".into());
                return Decision::new(RouteState::Probing, false, true, reasons);
            }
            if q.coverage.is_none() {
                // We have a tree but no visual baseline. A bounded visual
                // sampling pass is required to decide, not assumed healthy.
                reasons.push("baseline_sampling".into());
                return Decision::new(RouteState::Probing, true, true, reasons);
            }
            if q.truncated {
                reasons.push("unfinished_walk_visual_sampling".into());
                return Decision::new(RouteState::Hybrid, true, true, reasons);
            }
            let coverage = q.coverage.unwrap_or(0.0);
            if coverage >= 0.85 && q.actionable_known && q.actionable >= 0.5 {
                reasons.push(format!("coverage_high:{coverage:.2}"));
                return Decision::new(RouteState::UiaPrimary, true, false, reasons);
            }
            if coverage < 0.40 {
                reasons.push(format!("coverage_low:{coverage:.2}"));
                return Decision::new(RouteState::VisionPrimary, has_uia, true, reasons);
            }
            reasons.push(format!("coverage_partial:{coverage:.2}"));
            Decision::new(RouteState::Hybrid, has_uia, true, reasons)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(coverage: Option<f32>, actionable: f32, known: bool, leaves: usize, truncated: bool) -> RegionQuality {
        RegionQuality {
            rect_px: [0.0, 0.0, 800.0, 600.0],
            state: String::new(),
            coverage,
            readable: 0.8,
            locatable: 0.8,
            actionable,
            actionable_known: known,
            leaf_count: leaves,
            named_leaves: leaves,
            capture_ok: true,
            truncated,
            notes: vec![],
        }
    }

    #[test]
    fn permission_denial_is_terminal() {
        let quality = q(Some(0.1), 0.0, false, 2, false);
        let d = decide(RouteInput {
            mode: Mode::Auto,
            quality: &quality,
            capture: CaptureFacts { ok: true, rendered: true, fresh_known: true },
            permission_denied: Some("protected process"),
            target_valid: true,
        });
        assert_eq!(d.state, RouteState::Blocked);
        assert!(!d.run_vision);
    }

    #[test]
    fn missing_baseline_triggers_sampling_not_vision() {
        let quality = q(None, 0.0, false, 50, false);
        let d = decide(RouteInput {
            mode: Mode::Auto,
            quality: &quality,
            capture: CaptureFacts { ok: true, rendered: true, fresh_known: true },
            permission_denied: None,
            target_valid: true,
        });
        assert_eq!(d.state, RouteState::Probing);
        assert!(d.run_vision && d.run_uia);
    }

    #[test]
    fn poor_capture_keeps_a_usable_tree() {
        let quality = q(Some(0.1), 0.0, false, 30, false);
        let d = decide(RouteInput {
            mode: Mode::Auto,
            quality: &quality,
            capture: CaptureFacts { ok: false, rendered: false, fresh_known: false },
            permission_denied: None,
            target_valid: true,
        });
        assert_eq!(d.state, RouteState::UiaPrimary);
        assert!(!d.run_vision);
    }

    #[test]
    fn low_coverage_routes_to_vision() {
        let quality = q(Some(0.2), 0.0, false, 12, false);
        let d = decide(RouteInput {
            mode: Mode::Auto,
            quality: &quality,
            capture: CaptureFacts { ok: true, rendered: true, fresh_known: true },
            permission_denied: None,
            target_valid: true,
        });
        assert_eq!(d.state, RouteState::VisionPrimary);
    }
}
