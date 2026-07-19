//! The Act session orchestrator: transcript -> plan -> execute, driving the
//! state machine and emitting [`ActEvent`]s for the UI.
//!
//! One final transcript flows: snapshot the focused window -> build a capped,
//! PHI-safe [`GroundingPacket`] -> [`Planner::plan`] (fast-path or LLM) ->
//! [`Executor::execute_plan`]. Confirm / ask_user pause the plan; the UI answers
//! via [`ActSession::on_user_decision`], which resumes the stored remainder.
//!
//! Batch (hold-to-talk) returns to Idle after each command; VAD (hands-free)
//! returns to Armed so the session keeps listening.

use std::sync::Arc;

use super::action::{Action, ActionPlan};
use super::backend::AccessibilityBackend;
use super::events::{ActEvent, AskOption};
use super::executor::{ExecResult, Executor, StepOutcome, UserDecision};
use super::grounding_packet::{GroundingPacket, DEFAULT_MAX_ELEMENTS, DEFAULT_MAX_NAME_CHARS};
use super::planner::{PlanRequest, Planner};

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

/// Orchestrates transcript -> plan -> execute for one voice-control session.
pub struct ActSession {
    planner: Planner,
    executor: Executor,
    backend: Arc<dyn AccessibilityBackend>,
    mode: ActMode,
    state: SessionState,
    /// The un-executed tail of a plan paused on Confirm / ask_user.
    pending: Option<ActionPlan>,
}

impl ActSession {
    pub fn new(
        planner: Planner,
        executor: Executor,
        backend: Arc<dyn AccessibilityBackend>,
        mode: ActMode,
    ) -> Self {
        Self {
            planner,
            executor,
            backend,
            mode,
            state: SessionState::Idle,
            pending: None,
        }
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn is_armed(&self) -> bool {
        !matches!(self.state, SessionState::Idle)
    }

    /// Arm the session so incoming transcripts are acted on.
    pub fn arm(&mut self) {
        if matches!(self.state, SessionState::Idle) {
            self.state = SessionState::Armed;
        }
    }

    /// Return to Idle (Act off); does not touch an in-flight abort.
    pub fn disarm(&mut self) {
        self.state = SessionState::Idle;
        self.pending = None;
    }

    /// Trip the kill switch and reset to the armed baseline for this mode.
    pub fn abort(&mut self) {
        self.executor.kill_switch().trip();
        self.pending = None;
        self.state = self.baseline_state();
    }

    fn baseline_state(&self) -> SessionState {
        match self.mode {
            ActMode::Batch => SessionState::Idle,
            ActMode::Vad => SessionState::Armed,
        }
    }

    /// Handle a final transcript: plan and execute it. Returns the events to emit.
    pub async fn on_final_transcript(
        &mut self,
        transcript: String,
    ) -> Result<Vec<ActEvent>, SessionError> {
        // Only act when armed and not mid-command; drop while busy (VAD storms).
        match self.state {
            SessionState::Armed => {}
            SessionState::Idle => return Err(SessionError::NotArmed),
            _ => return Err(SessionError::Busy),
        }

        // Fresh kill switch for a new command.
        self.executor.kill_switch().reset();

        let mut events = Vec::new();
        self.state = SessionState::Planning;
        events.push(self.state_event());

        // Snapshot the focused window and build a capped, PHI-safe packet.
        let packet = match self.backend.snapshot().await {
            Ok(snap) => {
                GroundingPacket::from_snapshot(&snap, DEFAULT_MAX_ELEMENTS, DEFAULT_MAX_NAME_CHARS)
            }
            Err(e) => {
                self.state = self.baseline_state();
                events.push(ActEvent::Error {
                    message: format!("Could not read the screen: {e}"),
                });
                events.push(self.state_event());
                return Ok(events);
            }
        };

        // Keep a copy of the spoken command as the destructive-classifier hint
        // (enables the "confirm-activator under destructive intent" branch).
        let transcript_hint = transcript.clone();
        let plan = match self
            .planner
            .plan(PlanRequest {
                transcript,
                packet,
                prior_context: None,
            })
            .await
        {
            Ok(res) => res.plan,
            Err(e) => {
                self.state = self.baseline_state();
                events.push(ActEvent::Error {
                    message: e.to_string(),
                });
                events.push(self.state_event());
                return Ok(events);
            }
        };

        self.state = SessionState::Executing;
        events.push(self.state_event());

        let result = self
            .executor
            .execute_plan_with_context(plan.clone(), &transcript_hint)
            .await;
        self.absorb_result(plan, result, &mut events);
        Ok(events)
    }

    /// Handle the user's answer to a Confirm / ask_user pause.
    pub async fn on_user_decision(
        &mut self,
        decision: UserDecision,
    ) -> Result<Vec<ActEvent>, SessionError> {
        let remaining = match (&self.state, self.pending.take()) {
            (SessionState::AwaitingConfirm { .. }, Some(p))
            | (SessionState::AwaitingAskUser { .. }, Some(p)) => p,
            _ => return Err(SessionError::NotArmed),
        };

        let mut events = Vec::new();
        self.state = SessionState::Executing;
        events.push(self.state_event());

        let result = self
            .executor
            .resume_after_user(remaining.clone(), decision)
            .await;
        self.absorb_result(remaining, result, &mut events);
        Ok(events)
    }

    /// Map an [`ExecResult`] onto session state + events, storing any remainder.
    fn absorb_result(
        &mut self,
        plan: ActionPlan,
        result: Result<ExecResult, super::executor::ExecError>,
        events: &mut Vec<ActEvent>,
    ) {
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                self.state = self.baseline_state();
                events.push(ActEvent::Error {
                    message: e.to_string(),
                });
                events.push(self.state_event());
                return;
            }
        };

        // The tail that still needs running starts at the first non-Done outcome.
        let done = result
            .outcomes
            .iter()
            .take_while(|o| matches!(o, StepOutcome::Done { .. }))
            .count();
        let remaining: Vec<Action> = plan.actions.iter().skip(done).cloned().collect();

        match result.outcomes.last() {
            Some(StepOutcome::NeedsConfirm { action, reason }) => {
                self.pending = Some(ActionPlan::new(remaining));
                self.state = SessionState::AwaitingConfirm {
                    action: action.clone(),
                    reason: reason.clone(),
                };
                events.push(ActEvent::Confirm {
                    summary: format!("{} {}", action.kind(), action.target().unwrap_or("")),
                    reason: reason.clone(),
                });
                events.push(self.state_event());
            }
            Some(StepOutcome::NeedsAskUser { prompt, options }) => {
                self.pending = Some(ActionPlan::new(remaining));
                self.state = SessionState::AwaitingAskUser {
                    prompt: prompt.clone(),
                    options: options.clone(),
                };
                events.push(ActEvent::AskUser {
                    prompt: prompt.clone(),
                    options: options.clone(),
                });
                events.push(self.state_event());
            }
            _ => {
                // Completed / denied / failed / aborted — the command is over.
                let (ok, summary) = summarize(&result);
                self.state = self.baseline_state();
                events.push(ActEvent::Result { ok, summary });
                events.push(self.state_event());
            }
        }
    }

    fn state_event(&self) -> ActEvent {
        ActEvent::State {
            state: self.state.name().to_string(),
        }
    }
}

/// A short, PHI-safe outcome summary.
fn summarize(result: &ExecResult) -> (bool, String) {
    if result.completed {
        let n = result.outcomes.len();
        return (
            true,
            format!("Done ({n} step{})", if n == 1 { "" } else { "s" }),
        );
    }
    match result.outcomes.last() {
        Some(StepOutcome::Denied { reason, .. }) => (false, format!("Blocked: {reason}")),
        Some(StepOutcome::Failed { error, .. }) => (false, format!("Couldn't do that: {error}")),
        Some(StepOutcome::Aborted) => (false, "Stopped".to_string()),
        _ => (false, "Stopped".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::audit::AuditLog;
    use crate::act::capability::CapabilityGate;
    use crate::act::element::{ActionPattern, Bounds, Role, Snapshot, UiElement};
    use crate::act::killswitch::KillSwitch;
    use crate::act::llm::test_support::FixtureLlmClient;
    use crate::act::mock_backend::MockBackend;

    fn snapshot_with_button() -> Snapshot {
        Snapshot {
            app: "Editor".into(),
            window_title: "Untitled".into(),
            focused: Some("#/1".into()),
            pointer: None,
            selection_text_len: 0,
            elements: vec![UiElement {
                path: "#/1".into(),
                role: Role::Button,
                name: "Save".into(),
                description: String::new(),
                value_len: 0,
                states: vec![],
                bounds: Some(Bounds {
                    x: 0,
                    y: 0,
                    w: 5,
                    h: 5,
                }),
                patterns: vec![ActionPattern::Invoke],
            }],
        }
    }

    fn session(mode: ActMode) -> (ActSession, Arc<MockBackend>) {
        let backend = Arc::new(MockBackend::new(snapshot_with_button()));
        let planner = Planner::new(Arc::new(FixtureLlmClient::new(vec![])), "fast".into());
        let executor = Executor::new(
            backend.clone() as Arc<dyn AccessibilityBackend>,
            CapabilityGate::new(),
            None::<AuditLog>,
            KillSwitch::new(),
        );
        let s = ActSession::new(
            planner,
            executor,
            backend.clone() as Arc<dyn AccessibilityBackend>,
            mode,
        );
        (s, backend)
    }

    #[tokio::test]
    async fn transcript_before_arming_is_rejected() {
        let (mut s, _b) = session(ActMode::Batch);
        assert!(matches!(
            s.on_final_transcript("copy".into()).await,
            Err(SessionError::NotArmed)
        ));
    }

    #[tokio::test]
    async fn fast_path_command_executes_end_to_end_over_mock() {
        // "copy" resolves via the fast-path (no LLM) to a ctrl+c key; the mock
        // backend records the key press. This is the core loop smoke test.
        let (mut s, backend) = session(ActMode::Batch);
        s.arm();
        let events = s.on_final_transcript("copy".into()).await.unwrap();

        assert_eq!(backend.keys(), vec!["ctrl+c".to_string()]);
        // Batch returns to Idle; the last event is a State idle after a Result.
        assert!(matches!(s.state(), SessionState::Idle));
        assert!(events
            .iter()
            .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));
    }

    #[tokio::test]
    async fn vad_mode_returns_to_armed_after_a_command() {
        let (mut s, _b) = session(ActMode::Vad);
        s.arm();
        s.on_final_transcript("copy".into()).await.unwrap();
        assert!(matches!(s.state(), SessionState::Armed));
    }

    #[tokio::test]
    async fn abort_trips_kill_and_resets_to_baseline() {
        let (mut s, _b) = session(ActMode::Vad);
        s.arm();
        s.abort();
        assert!(matches!(s.state(), SessionState::Armed)); // vad baseline
    }

    #[test]
    fn mode_maps_from_stt_mode() {
        assert_eq!(ActMode::from_stt_mode("realtime"), ActMode::Vad);
        assert_eq!(ActMode::from_stt_mode("batch"), ActMode::Batch);
    }

    #[test]
    fn state_names_are_stable() {
        assert_eq!(SessionState::Idle.name(), "idle");
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
