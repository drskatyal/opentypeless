//! The Act session orchestrator: transcript -> plan -> execute, with the two
//! control modalities (hold-to-talk batch, hands-free VAD) and the confirm /
//! ask_user pause-and-resume state machine.
//!
//! TODO(act-phase1): the orchestration is wired in a later step; the state types
//! below are the frozen contract shared with the wiring layer.

use super::action::Action;
use super::events::AskOption;

/// The control modality, derived from the STT mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActMode {
    /// Hold-to-talk: transcribe the whole clip, then plan + execute.
    Batch,
    /// Hands-free: each VAD segment is planned + executed; session stays armed.
    Vad,
}

impl ActMode {
    pub fn from_stt_mode(stt_mode: &str) -> Self {
        if stt_mode == "realtime" {
            ActMode::Vad
        } else {
            ActMode::Batch
        }
    }
}

/// Where the session is in its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Armed,
    Planning,
    Executing,
    AwaitingConfirm {
        action: Action,
        reason: String,
    },
    AwaitingAskUser {
        prompt: String,
        options: Vec<AskOption>,
    },
    CoolingDown,
}

impl SessionState {
    /// A short, wire-friendly name for the `act://event` State payload.
    pub fn name(&self) -> &'static str {
        match self {
            SessionState::Idle => "idle",
            SessionState::Armed => "armed",
            SessionState::Planning => "planning",
            SessionState::Executing => "executing",
            SessionState::AwaitingConfirm { .. } => "awaiting_confirm",
            SessionState::AwaitingAskUser { .. } => "awaiting_ask_user",
            SessionState::CoolingDown => "cooling_down",
        }
    }
}

/// A session-level failure.
#[derive(Debug)]
pub enum SessionError {
    NotArmed,
    Busy,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::NotArmed => write!(f, "Act session is not armed"),
            SessionError::Busy => write!(f, "Act session is busy"),
        }
    }
}
impl std::error::Error for SessionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_maps_from_stt_mode() {
        assert_eq!(ActMode::from_stt_mode("realtime"), ActMode::Vad);
        assert_eq!(ActMode::from_stt_mode("batch"), ActMode::Batch);
        assert_eq!(ActMode::from_stt_mode("anything else"), ActMode::Batch);
    }

    #[test]
    fn state_names_are_stable() {
        assert_eq!(SessionState::Idle.name(), "idle");
        assert_eq!(SessionState::Armed.name(), "armed");
        assert_eq!(
            SessionState::AwaitingAskUser {
                prompt: "?".into(),
                options: vec![]
            }
            .name(),
            "awaiting_ask_user"
        );
    }
}
