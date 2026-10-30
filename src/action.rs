// action.rs — typed request/outcome contract for computer-use actions.
// The CLI still owns argument parsing; this module makes the execution mode and
// result semantics explicit instead of passing strategy/background strings around.
use anyhow::{bail, Result};

use crate::util::{ActionMeta, Receipt};

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
}

impl ActionRequest {
    pub fn new(cmd: &str, argv: Vec<String>, record: &Option<String>, input_mode: InputMode) -> Self {
        ActionRequest {
            cmd: cmd.to_string(),
            argv,
            record: record.clone(),
            input_mode,
        }
    }

    pub fn meta(&self) -> ActionMeta {
        ActionMeta::new(&self.cmd, self.argv.clone(), &self.record)
    }
}

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
