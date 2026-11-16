// action.rs — typed request/outcome contract for computer-use actions.
// The CLI still owns argument parsing; this module makes the execution mode and
// result semantics explicit instead of passing strategy/background strings around.
use anyhow::{bail, Result};
use std::fmt;

use crate::util::{ActionMeta, Receipt};

/// Requested delivery contract. `auto` keeps the historical strategy/--background
/// behavior; `background` forbids focus/cursor changes; `foreground` is the
/// explicit real-input escalation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delivery {
    Auto,
    Background,
    Foreground,
}

impl Delivery {
    pub fn parse(spec: &str) -> Result<Self> {
        match spec.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "background" => Ok(Self::Background),
            "foreground" => Ok(Self::Foreground),
            other => bail!("unknown delivery '{other}' (expected auto|background|foreground)"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Background => "background",
            Self::Foreground => "foreground",
        }
    }

    pub fn is_background(self) -> bool {
        matches!(self, Self::Background)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputMode {
    /// a11y first, then the caller's event fallback
    Auto,
    /// accessibility patterns only (a gesture with no pattern is refused)
    A11y,
    /// real event semantics (physical pointer/keyboard)
    Event,
    /// message-level input; never touches the real pointer
    Background,
    /// accessibility patterns only, but without real input (background a11y)
    BackgroundA11y,
}

impl InputMode {
    pub fn parse(strategy: &str, background: bool) -> Result<Self> {
        match (strategy.to_ascii_lowercase().as_str(), background) {
            ("auto", false) => Ok(InputMode::Auto),
            ("auto", true) => Ok(InputMode::Background),
            ("a11y", false) => Ok(InputMode::A11y),
            ("a11y", true) => Ok(InputMode::BackgroundA11y),
            ("event", false) => Ok(InputMode::Event),
            ("event", true) => Ok(InputMode::Background),
            (other, _) => bail!("unknown strategy '{other}' (expected auto|a11y|event)"),
        }
    }

    pub fn from_background(background: bool) -> Self {
        if background {
            InputMode::Background
        } else {
            InputMode::Auto
        }
    }

    pub fn is_background(self) -> bool {
        matches!(self, InputMode::Background | InputMode::BackgroundA11y)
    }

    pub fn is_a11y_only(self) -> bool {
        matches!(self, InputMode::A11y | InputMode::BackgroundA11y)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            InputMode::Auto => "auto",
            InputMode::A11y => "a11y",
            InputMode::Event => "event",
            InputMode::Background => "background",
            InputMode::BackgroundA11y => "background-a11y",
        }
    }
}

pub struct ActionRequest {
    pub cmd: String,
    pub argv: Vec<String>,
    pub record: Option<String>,
    pub input_mode: InputMode,
    pub delivery: Delivery,
    /// Consume the last observation after a successful coordinate action, so a
    /// shot_id cannot authorize two dispatched actions.
    pub consume_shot: bool,
}

impl ActionRequest {
    pub fn new(cmd: &str, argv: Vec<String>, record: &Option<String>, input_mode: InputMode) -> Self {
        ActionRequest {
            cmd: cmd.to_string(),
            argv,
            record: record.clone(),
            input_mode,
            delivery: Delivery::Auto,
            consume_shot: false,
        }
    }

    pub fn with_delivery(mut self, delivery: Delivery) -> Self {
        self.delivery = delivery;
        self
    }

    pub fn with_consume_shot(mut self, consume: bool) -> Self {
        self.consume_shot = consume;
        self
    }

    pub fn meta(&self) -> ActionMeta {
        ActionMeta::new(&self.cmd, self.argv.clone(), &self.record)
    }
}

/// A dispatched action's result plus the delivery evidence the receipt needs.
/// Plain `(detail, state_id)` tuples convert into this, so existing commands keep
/// working while click can report its exact route and effect.
#[derive(Clone, Debug, Default)]
pub struct ActionReport {
    pub detail: String,
    pub state_id: Option<String>,
    pub actual_delivery: Option<String>,
    pub route: Option<String>,
    /// verified | unverifiable
    pub effect: Option<String>,
    pub code: Option<String>,
    pub suggestion: Option<String>,
}

impl ActionReport {
    pub fn new(detail: impl Into<String>, state_id: Option<String>) -> Self {
        Self {
            detail: detail.into(),
            state_id,
            ..Default::default()
        }
    }

    pub fn with_delivery(mut self, delivery: &str) -> Self {
        self.actual_delivery = Some(delivery.to_string());
        self
    }

    pub fn with_route(mut self, route: &str) -> Self {
        self.route = Some(route.to_string());
        self
    }

    pub fn with_effect(mut self, effect: &str) -> Self {
        self.effect = Some(effect.to_string());
        self
    }
}

impl From<(String, Option<String>)> for ActionReport {
    fn from((detail, state_id): (String, Option<String>)) -> Self {
        ActionReport::new(detail, state_id)
    }
}

/// A deliberate refusal with a stable machine-readable reason. `guarded_action_req`
/// turns this into `status:"refused"` (not `failed`) and copies the code and the
/// escalation suggestion into the receipt.
#[derive(Debug, Clone)]
pub struct ActionRefusal {
    pub detail: String,
    pub code: String,
    pub suggestion: Option<String>,
    pub route: Option<String>,
}

impl ActionRefusal {
    pub fn new(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            code: code.into(),
            suggestion: None,
            route: None,
        }
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    pub fn with_route(mut self, route: impl Into<String>) -> Self {
        self.route = Some(route.into());
        self
    }
}

impl fmt::Display for ActionRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for ActionRefusal {}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutcomeStatus {
    Ok,
    Refused,
    Failed,
}

pub struct ActionOutcome {
    pub status: OutcomeStatus,
    pub dispatched: bool,
    pub detail: String,
    pub state_id: Option<String>,
}

impl ActionOutcome {
    pub fn ok(detail: impl Into<String>, state_id: Option<String>) -> Self {
        ActionOutcome {
            status: OutcomeStatus::Ok,
            dispatched: true,
            detail: detail.into(),
            state_id,
        }
    }

    pub fn refused(detail: impl Into<String>) -> Self {
        ActionOutcome {
            status: OutcomeStatus::Refused,
            dispatched: false,
            detail: detail.into(),
            state_id: None,
        }
    }

    pub fn failed(detail: impl Into<String>) -> Self {
        ActionOutcome {
            status: OutcomeStatus::Failed,
            dispatched: false,
            detail: detail.into(),
            state_id: None,
        }
    }

    pub fn apply(&self, receipt: &mut Receipt) {
        receipt.status = match self.status {
            OutcomeStatus::Ok => "ok",
            OutcomeStatus::Refused => "refused",
            OutcomeStatus::Failed => "failed",
        }
        .into();
        receipt.dispatched = self.dispatched;
        receipt.detail = Some(self.detail.clone());
        receipt.state_id = self.state_id.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_mode_maps_strategy_and_background() {
        assert_eq!(InputMode::parse("auto", false).unwrap(), InputMode::Auto);
        assert_eq!(InputMode::parse("auto", true).unwrap(), InputMode::Background);
        assert_eq!(InputMode::parse("a11y", false).unwrap(), InputMode::A11y);
        assert_eq!(InputMode::parse("a11y", true).unwrap(), InputMode::BackgroundA11y);
        assert_eq!(InputMode::parse("event", false).unwrap(), InputMode::Event);
        assert_eq!(InputMode::parse("event", true).unwrap(), InputMode::Background);
        assert!(InputMode::parse("magic", false).is_err());
    }

    #[test]
    fn delivery_parses_and_names() {
        assert_eq!(Delivery::parse("auto").unwrap(), Delivery::Auto);
        assert_eq!(Delivery::parse("BACKGROUND").unwrap(), Delivery::Background);
        assert_eq!(Delivery::parse("foreground").unwrap(), Delivery::Foreground);
        assert!(Delivery::parse("magic").is_err());
        assert_eq!(Delivery::Auto.as_str(), "auto");
        assert!(Delivery::Background.is_background());
        assert!(!Delivery::Auto.is_background());
    }

    #[test]
    fn report_converts_from_legacy_tuple_and_carries_route() {
        let r: ActionReport = ("done".to_string(), Some("s-1".into())).into();
        assert_eq!(r.detail, "done");
        assert_eq!(r.state_id.as_deref(), Some("s-1"));
        let r = r.with_delivery("background").with_route("uia_pixel").with_effect("unverifiable");
        assert_eq!(r.actual_delivery.as_deref(), Some("background"));
        assert_eq!(r.route.as_deref(), Some("uia_pixel"));
        assert_eq!(r.effect.as_deref(), Some("unverifiable"));
    }

    #[test]
    fn refusal_is_a_typed_error_and_downgrades_to_refused() {
        let e = ActionRefusal::new("background_unavailable", "drops posted input")
            .with_route("pen")
            .with_suggestion("retry with --delivery foreground");
        assert_eq!(e.code, "background_unavailable");
        assert_eq!(e.route.as_deref(), Some("pen"));
        let any = anyhow::Error::new(e);
        assert!(any.downcast_ref::<ActionRefusal>().is_some());
    }

    #[test]
    fn mode_predicates_are_consistent() {
        assert!(InputMode::Background.is_background());
        assert!(InputMode::BackgroundA11y.is_background());
        assert!(!InputMode::Auto.is_background());
        assert!(InputMode::A11y.is_a11y_only());
        assert!(InputMode::BackgroundA11y.is_a11y_only());
        assert_eq!(InputMode::Auto.as_str(), "auto");
        assert_eq!(InputMode::BackgroundA11y.as_str(), "background-a11y");
        assert_eq!(InputMode::Background.as_str(), "background");
    }
}
