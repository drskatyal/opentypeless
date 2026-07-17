//! The Conductor — the drawer-aware session orchestrator.
//!
//! One spoken command flows: snapshot the screen into the [`Blackboard`], route
//! the transcript onto the drawer with a single [`selection::select`] call, then
//! carry out each resulting mission in order — an `OpenFlow` replays a saved
//! recipe on the [`FlowRunner`]; a `Novel` goal is planned and executed from
//! primitives. A mission that pauses for the user (confirm / pick) suspends the
//! whole queue; [`Conductor::decide`] answers it and the remaining missions
//! continue. The blackboard carries context across dictations, so a follow-up
//! command is planned against what the last one left behind.
//!
//! This sits above the older single-path [`super::session::ActSession`]: same
//! safety discipline (capability gate, kill switch, injection fences), plus the
//! drawer and the cross-dictation loop.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use super::action::ActionPlan;
use super::backend::AccessibilityBackend;
use super::blackboard::Blackboard;
use super::events::{ActEvent, AskOption};
use super::executor::{ExecError, ExecResult, Executor, StepOutcome, UserDecision};
use super::flow::FlowFile;
use super::flow_registry::FlowRegistry;
use super::flow_runner::{FlowOutcome, FlowRunError, FlowRunner, Resume, ResumeDecision};
use super::grounding_packet::{GroundingPacket, DEFAULT_MAX_ELEMENTS, DEFAULT_MAX_NAME_CHARS};
use super::llm::LlmClient;
use super::planner::{PlanRequest, Planner};
use super::selection::{self, Mission, SelectionError};
use super::session::ActMode;

/// Slot name -> value, as filled by the selection layer.
type SlotMap = HashMap<String, String>;

/// Where the Conductor is in its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConductorState {
    Idle,
    Armed,
    Working,
    AwaitingConfirm,
    AwaitingChoice,
}

impl ConductorState {
    pub fn name(&self) -> &'static str {
        match self {
            ConductorState::Idle => "idle",
            ConductorState::Armed => "armed",
            ConductorState::Working => "working",
            ConductorState::AwaitingConfirm => "awaiting_confirm",
            ConductorState::AwaitingChoice => "awaiting_choice",
        }
    }
}

/// A session-level failure.
#[derive(Debug, PartialEq, Eq)]
pub enum ConductorError {
    NotArmed,
    Busy,
}

impl std::fmt::Display for ConductorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConductorError::NotArmed => write!(f, "Conductor is not armed"),
            ConductorError::Busy => write!(f, "Conductor is busy"),
        }
    }
}
impl std::error::Error for ConductorError {}

/// The suspended work when a mission pauses for the user.
struct Pending {
    kind: PendingKind,
    /// Missions not yet started, resumed after the paused one finishes.
    queue: VecDeque<Mission>,
}

enum PendingKind {
    /// Paused inside a leaf recipe.
    Flow {
        file: Box<FlowFile>,
        slots: SlotMap,
        resume: Resume,
        /// Options offered (to map a numbered pick back to a row).
        options: Vec<AskOption>,
    },
    /// Paused inside a novel plan.
    Novel { remaining: ActionPlan },
}

/// The drawer-aware orchestrator.
pub struct Conductor {
    registry: FlowRegistry,
    llm: Arc<dyn LlmClient>,
    runner: FlowRunner,
    planner: Planner,
    executor: Executor,
    backend: Arc<dyn AccessibilityBackend>,
    board: Blackboard,
    mode: ActMode,
    state: ConductorState,
    pending: Option<Pending>,
}

/// Where the mission loop lands after one mission.
enum Step {
    /// Mission finished (completed or failed-but-recoverable) — go to the next.
    Next,
    /// Mission paused for the user; the loop stops and stores the continuation.
    Paused(PendingKind),
    /// Abort the whole batch (kill switch).
    Stop,
}

impl Conductor {
    #[allow(clippy::too_many_arguments)] // an orchestrator wires several collaborators
    pub fn new(
        registry: FlowRegistry,
        llm: Arc<dyn LlmClient>,
        runner: FlowRunner,
        planner: Planner,
        executor: Executor,
        backend: Arc<dyn AccessibilityBackend>,
        mode: ActMode,
    ) -> Self {
        Self {
            registry,
            llm,
            runner,
            planner,
            executor,
            backend,
            board: Blackboard::new(),
            mode,
            state: ConductorState::Idle,
            pending: None,
        }
    }

    pub fn state(&self) -> &ConductorState {
        &self.state
    }

    pub fn is_armed(&self) -> bool {
        !matches!(self.state, ConductorState::Idle)
    }

    /// Arm so incoming transcripts are acted on.
    pub fn arm(&mut self) {
        if matches!(self.state, ConductorState::Idle) {
            self.state = ConductorState::Armed;
        }
    }

    /// Return to Idle; drops any suspended work.
    pub fn disarm(&mut self) {
        self.state = ConductorState::Idle;
        self.pending = None;
    }

    /// Trip the kill switch, drop the queue, and reset to the armed baseline.
    pub fn abort(&mut self) {
        self.runner.kill_switch().trip();
        self.executor.kill_switch().trip();
        self.pending = None;
        self.state = self.baseline();
    }

    fn baseline(&self) -> ConductorState {
        match self.mode {
            ActMode::Batch => ConductorState::Idle,
            ActMode::Vad => ConductorState::Armed,
        }
    }

    /// Handle one final transcript end to end. Returns the events to emit.
    pub async fn on_transcript(
        &mut self,
        transcript: String,
    ) -> Result<Vec<ActEvent>, ConductorError> {
        match self.state {
            ConductorState::Armed => {}
            ConductorState::Idle => return Err(ConductorError::NotArmed),
            _ => return Err(ConductorError::Busy),
        }

        // Fresh kill switches for a new command.
        self.runner.kill_switch().reset();
        self.executor.kill_switch().reset();

        let mut events = Vec::new();
        self.state = ConductorState::Working;
        events.push(self.state_event());

        // Observe the screen into the blackboard (context for routing + planning).
        if let Ok(snapshot) = self.backend.snapshot().await {
            self.board.observe(&snapshot);
        }

        // Route onto the drawer.
        let selection = match selection::select(
            self.llm.as_ref(),
            &self.registry,
            &transcript,
            self.board.focus_app.as_deref(),
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                self.state = self.baseline();
                events.push(ActEvent::Error {
                    message: describe_selection_error(&e),
                });
                events.push(self.state_event());
                return Ok(events);
            }
        };

        let queue: VecDeque<Mission> = selection.missions.into();
        self.drive_queue(queue, &mut events).await;
        Ok(events)
    }

    /// Answer a paused mission (confirm / pick) and continue the queue.
    pub async fn decide(
        &mut self,
        decision: UserDecision,
    ) -> Result<Vec<ActEvent>, ConductorError> {
        let Some(pending) = self.pending.take() else {
            return Err(ConductorError::NotArmed);
        };
        let mut events = Vec::new();
        self.state = ConductorState::Working;
        events.push(self.state_event());

        let Pending { kind, queue } = pending;
        match self.resume_paused(kind, decision, &mut events).await {
            Step::Next => self.drive_queue(queue, &mut events).await,
            Step::Paused(kind) => self.suspend(kind, queue, &mut events),
            Step::Stop => {
                self.state = self.baseline();
                events.push(self.state_event());
            }
        }
        Ok(events)
    }

    /// Run missions until the queue drains or one pauses / aborts.
    async fn drive_queue(&mut self, mut queue: VecDeque<Mission>, events: &mut Vec<ActEvent>) {
        while let Some(mission) = queue.pop_front() {
            match self.run_mission(mission, events).await {
                Step::Next => continue,
                Step::Paused(kind) => {
                    self.suspend(kind, queue, events);
                    return;
                }
                Step::Stop => {
                    self.state = self.baseline();
                    events.push(self.state_event());
                    return;
                }
            }
        }
        // Everything ran. Refresh the frame for the next dictation.
        if let Ok(snapshot) = self.backend.snapshot().await {
            self.board.observe(&snapshot);
        }
        self.state = self.baseline();
        events.push(self.state_event());
    }

    /// Store a pause: remember the continuation and reflect it in state + events.
    fn suspend(&mut self, kind: PendingKind, queue: VecDeque<Mission>, events: &mut Vec<ActEvent>) {
        self.state = match &kind {
            PendingKind::Flow { .. } | PendingKind::Novel { .. } => match events.last() {
                Some(ActEvent::AskUser { .. }) => ConductorState::AwaitingChoice,
                _ => ConductorState::AwaitingConfirm,
            },
        };
        self.pending = Some(Pending { kind, queue });
        events.push(self.state_event());
    }

    /// Carry out one mission.
    async fn run_mission(&mut self, mission: Mission, events: &mut Vec<ActEvent>) -> Step {
        match mission {
            Mission::OpenFlow { id, slots } => self.run_flow(&id, slots, events).await,
            Mission::Novel { goal } => self.run_novel(goal, events).await,
        }
    }

    /// Open a saved recipe and replay it (a branch hands its context to the planner).
    async fn run_flow(&mut self, id: &str, slots: SlotMap, events: &mut Vec<ActEvent>) -> Step {
        let Some(file) = self.registry.open(id).cloned() else {
            events.push(ActEvent::Error {
                message: format!("that saved task is unavailable ({id})"),
            });
            return Step::Next;
        };
        let local = RefCell::new(Vec::new());
        let outcome = {
            let emit = |e: ActEvent| local.borrow_mut().push(e);
            self.runner.run(&file, &slots, &emit).await
        };
        events.extend(local.into_inner());
        self.absorb_flow(file, slots, outcome, events).await
    }

    /// Map a flow outcome onto state, events, and (on a branch) a planner handoff.
    async fn absorb_flow(
        &mut self,
        file: FlowFile,
        slots: SlotMap,
        outcome: Result<FlowOutcome, FlowRunError>,
        events: &mut Vec<ActEvent>,
    ) -> Step {
        match outcome {
            Ok(FlowOutcome::Done { verified }) => {
                self.board.record(format!("ran {}", file.id));
                events.push(ActEvent::Result {
                    ok: true,
                    summary: if verified {
                        format!("Done: {}", file.name)
                    } else {
                        format!("Ran {} (couldn't verify)", file.name)
                    },
                });
                Step::Next
            }
            Ok(FlowOutcome::Failed { step, error }) => {
                self.board.record(format!("{} failed", file.id));
                events.push(ActEvent::Result {
                    ok: false,
                    summary: format!("Couldn't finish {}: {error} (at {step})", file.name),
                });
                Step::Next
            }
            Ok(FlowOutcome::NeedsConfirm { reason, resume }) => {
                events.push(ActEvent::Confirm {
                    summary: format!("Continue {}?", file.name),
                    reason,
                });
                Step::Paused(PendingKind::Flow {
                    file: Box::new(file),
                    slots,
                    resume,
                    options: Vec::new(),
                })
            }
            Ok(FlowOutcome::NeedsChoice {
                prompt,
                options,
                resume,
            }) => {
                events.push(ActEvent::AskUser {
                    prompt,
                    options: options.clone(),
                });
                Step::Paused(PendingKind::Flow {
                    file: Box::new(file),
                    slots,
                    resume,
                    options,
                })
            }
            Ok(FlowOutcome::Branch { context, slots }) => {
                // A branch recipe reasons via the planner over its context.
                self.board.record(format!("branch {}", file.id));
                self.run_novel_with(context, Some(slot_context(&slots)), events)
                    .await
            }
            Ok(FlowOutcome::Aborted) => {
                events.push(ActEvent::Result {
                    ok: false,
                    summary: "Stopped".into(),
                });
                Step::Stop
            }
            Err(e) => {
                events.push(ActEvent::Error {
                    message: format!("Couldn't read the screen: {e}"),
                });
                Step::Next
            }
        }
    }

    /// Plan a novel goal from primitives and execute it.
    async fn run_novel(&mut self, goal: String, events: &mut Vec<ActEvent>) -> Step {
        self.run_novel_with(goal, None, events).await
    }

    async fn run_novel_with(
        &mut self,
        goal: String,
        extra_context: Option<String>,
        events: &mut Vec<ActEvent>,
    ) -> Step {
        let packet = match self.backend.snapshot().await {
            Ok(snap) => {
                GroundingPacket::from_snapshot(&snap, DEFAULT_MAX_ELEMENTS, DEFAULT_MAX_NAME_CHARS)
            }
            Err(e) => {
                events.push(ActEvent::Error {
                    message: format!("Couldn't read the screen: {e}"),
                });
                return Step::Next;
            }
        };
        let mut prior = self.board.context_summary();
        if let Some(extra) = extra_context {
            prior.push('\n');
            prior.push_str(&extra);
        }
        let prior_context = (!prior.trim().is_empty()).then_some(prior);

        let hint = goal.clone();
        let plan = match self
            .planner
            .plan(PlanRequest {
                transcript: goal,
                packet,
                prior_context,
            })
            .await
        {
            Ok(res) => res.plan,
            Err(e) => {
                events.push(ActEvent::Error {
                    message: e.to_string(),
                });
                return Step::Next;
            }
        };

        let result = self
            .executor
            .execute_plan_with_context(plan.clone(), &hint)
            .await;
        self.absorb_novel(plan, result, &hint, events)
    }

    /// Map an executor result onto state + events, storing any remainder on pause.
    fn absorb_novel(
        &mut self,
        plan: ActionPlan,
        result: Result<ExecResult, ExecError>,
        hint: &str,
        events: &mut Vec<ActEvent>,
    ) -> Step {
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                events.push(ActEvent::Error {
                    message: e.to_string(),
                });
                return Step::Next;
            }
        };

        let done = result
            .outcomes
            .iter()
            .take_while(|o| matches!(o, StepOutcome::Done { .. }))
            .count();
        let remaining: Vec<_> = plan.actions.iter().skip(done).cloned().collect();

        match result.outcomes.last() {
            Some(StepOutcome::NeedsConfirm { action, reason }) => {
                events.push(ActEvent::Confirm {
                    summary: format!("{} {}", action.kind(), action.target().unwrap_or("")),
                    reason: reason.clone(),
                });
                Step::Paused(PendingKind::Novel {
                    remaining: ActionPlan::new(remaining),
                })
            }
            Some(StepOutcome::NeedsAskUser { prompt, options }) => {
                events.push(ActEvent::AskUser {
                    prompt: prompt.clone(),
                    options: options.clone(),
                });
                Step::Paused(PendingKind::Novel {
                    remaining: ActionPlan::new(remaining),
                })
            }
            Some(StepOutcome::Aborted) => {
                events.push(ActEvent::Result {
                    ok: false,
                    summary: "Stopped".into(),
                });
                Step::Stop
            }
            _ => {
                self.board.record(short_goal(hint));
                let (ok, summary) = summarize_novel(&result);
                events.push(ActEvent::Result { ok, summary });
                Step::Next
            }
        }
    }

    /// Resume the paused mission with the user's decision.
    async fn resume_paused(
        &mut self,
        kind: PendingKind,
        decision: UserDecision,
        events: &mut Vec<ActEvent>,
    ) -> Step {
        match kind {
            PendingKind::Flow {
                file,
                slots,
                resume,
                options,
            } => {
                let rd = flow_decision(&decision, &options);
                let local = RefCell::new(Vec::new());
                let outcome = {
                    let emit = |e: ActEvent| local.borrow_mut().push(e);
                    self.runner.resume(&file, &slots, resume, rd, &emit).await
                };
                events.extend(local.into_inner());
                self.absorb_flow(*file, slots, outcome, events).await
            }
            PendingKind::Novel { remaining } => {
                let result = self
                    .executor
                    .resume_after_user(remaining.clone(), decision)
                    .await;
                self.absorb_novel(remaining, result, "", events)
            }
        }
    }

    fn state_event(&self) -> ActEvent {
        ActEvent::State {
            state: self.state.name().to_string(),
        }
    }
}

/// Translate a generic [`UserDecision`] into a flow [`ResumeDecision`], mapping a
/// numbered pick back to the option row it names.
fn flow_decision(decision: &UserDecision, options: &[AskOption]) -> ResumeDecision {
    match decision {
        UserDecision::ConfirmAllow => ResumeDecision::Approve,
        UserDecision::ConfirmDeny | UserDecision::Cancel => ResumeDecision::Decline,
        UserDecision::AskUserPick { index } => options
            .iter()
            .find(|o| o.index == *index)
            .cloned()
            .map(ResumeDecision::Choose)
            .unwrap_or(ResumeDecision::Decline),
    }
}

/// A short, PHI-free description of a novel goal for the history.
fn short_goal(goal: &str) -> String {
    let g = goal.trim();
    if g.is_empty() {
        "did a task".to_string()
    } else if g.len() <= 48 {
        g.to_string()
    } else {
        format!("{}…", &g[..47])
    }
}

/// A one-line context note from a branch's carried slots.
fn slot_context(slots: &SlotMap) -> String {
    if slots.is_empty() {
        return String::new();
    }
    let mut names: Vec<&str> = slots.keys().map(String::as_str).collect();
    names.sort_unstable();
    format!("carried: {}", names.join(", "))
}

fn summarize_novel(result: &ExecResult) -> (bool, String) {
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
        _ => (false, "Stopped".to_string()),
    }
}

fn describe_selection_error(e: &SelectionError) -> String {
    match e {
        SelectionError::Empty => "I couldn't tell what to do from that.".to_string(),
        other => format!("Couldn't route that command: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::audit::AuditLog;
    use crate::act::capability::CapabilityGate;
    use crate::act::element::{ActionPattern, Bounds, Role, Snapshot, UiElement};
    use crate::act::flow::{FlowFile, FlowKind, FlowStatus, FlowStep, OnFail, Selector};
    use crate::act::killswitch::KillSwitch;
    use crate::act::llm::test_support::FixtureLlmClient;
    use crate::act::mock_backend::MockBackend;

    fn snap(elements: Vec<UiElement>) -> Snapshot {
        Snapshot {
            app: "Spotify".into(),
            window_title: "Spotify".into(),
            focused: None,
            pointer: None,
            selection_text_len: 0,
            elements,
        }
    }

    fn el(path: &str, role: Role, name: &str) -> UiElement {
        UiElement {
            path: path.into(),
            role,
            name: name.into(),
            description: String::new(),
            value_len: 0,
            states: vec![],
            bounds: Some(Bounds {
                x: 0,
                y: 0,
                w: 5,
                h: 5,
            }),
            patterns: vec![ActionPattern::Invoke, ActionPattern::SetValue],
        }
    }

    fn open_gmail_flow() -> FlowFile {
        FlowFile {
            id: "open_gmail".into(),
            name: "Open Gmail".into(),
            description: "open gmail in the browser".into(),
            aliases: vec![],
            kind: FlowKind::Leaf,
            app_scope: vec![],
            preconditions: vec![],
            slots: vec![],
            steps: vec![FlowStep {
                id: "s1".into(),
                intent: "open gmail".into(),
                action: "uri".into(),
                target: None,
                value: Some("https://mail.google.com".into()),
                pick: None,
                bind: None,
                wait_before: None,
                postcondition: None,
                on_fail: OnFail::Abort,
            }],
            branch_context: None,
            verify: None,
            status: FlowStatus::Smoke,
            version: 1,
            health: Default::default(),
        }
    }

    fn conductor(
        registry: FlowRegistry,
        responses: Vec<Result<String, crate::error::AppError>>,
        backend: Arc<MockBackend>,
    ) -> Conductor {
        let llm = Arc::new(FixtureLlmClient::new(responses));
        let mut gate = CapabilityGate::new();
        gate.grant(crate::act::capability::Capability::NetNavigate);
        let runner = FlowRunner::new(
            backend.clone() as Arc<dyn AccessibilityBackend>,
            gate.clone(),
            KillSwitch::new(),
        );
        let planner = Planner::new(llm.clone(), "fast".into());
        let executor = Executor::new(
            backend.clone() as Arc<dyn AccessibilityBackend>,
            gate,
            None::<AuditLog>,
            KillSwitch::new(),
        );
        Conductor::new(
            registry,
            llm,
            runner,
            planner,
            executor,
            backend as Arc<dyn AccessibilityBackend>,
            ActMode::Batch,
        )
    }

    #[tokio::test]
    async fn transcript_before_arming_is_rejected() {
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        let mut c = conductor(FlowRegistry::new(), vec![], backend);
        assert_eq!(
            c.on_transcript("open gmail".into()).await,
            Err(ConductorError::NotArmed)
        );
    }

    #[tokio::test]
    async fn opens_a_saved_flow_selected_by_the_model() {
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        let registry = FlowRegistry::from_files([open_gmail_flow()]);
        let selection = r#"{"missions":[{"type":"open_flow","id":"open_gmail","slots":{}}]}"#;
        let mut c = conductor(registry, vec![Ok(selection.into())], backend.clone());
        c.arm();

        let events = c.on_transcript("open gmail".into()).await.unwrap();
        assert_eq!(backend.opened_uris(), vec!["https://mail.google.com"]);
        assert!(events
            .iter()
            .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));
        assert!(matches!(c.state(), ConductorState::Idle));
    }

    #[tokio::test]
    async fn runs_two_missions_in_order() {
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        let registry = FlowRegistry::from_files([open_gmail_flow()]);
        // Two OpenFlow missions for the same file (idempotent uri) proves ordering
        // and that the queue drains fully.
        let selection = r#"{"missions":[
            {"type":"open_flow","id":"open_gmail","slots":{}},
            {"type":"open_flow","id":"open_gmail","slots":{}}
        ]}"#;
        let mut c = conductor(registry, vec![Ok(selection.into())], backend.clone());
        c.arm();

        let events = c.on_transcript("open gmail twice".into()).await.unwrap();
        assert_eq!(backend.opened_uris().len(), 2);
        let results = events
            .iter()
            .filter(|e| matches!(e, ActEvent::Result { ok: true, .. }))
            .count();
        assert_eq!(results, 2);
    }

    #[tokio::test]
    async fn unknown_flow_id_is_surfaced_not_run() {
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        // The registry is empty; selection validation downgrades the invented id to
        // a Novel(transcript). "copy" then resolves via the planner fast-path.
        let selection = r#"{"missions":[{"type":"open_flow","id":"ghost","slots":{}}]}"#;
        let mut c = conductor(
            FlowRegistry::new(),
            vec![Ok(selection.into())],
            backend.clone(),
        );
        c.arm();
        let events = c.on_transcript("copy".into()).await.unwrap();
        // Downgraded to Novel "copy" -> fast-path ctrl+c on the mock.
        assert_eq!(backend.keys(), vec!["ctrl+c".to_string()]);
        assert!(events.iter().any(|e| matches!(e, ActEvent::Result { .. })));
    }

    #[tokio::test]
    async fn novel_mission_plans_and_executes_via_fast_path() {
        let backend = Arc::new(MockBackend::new(snap(vec![el("#/1", Role::Button, "X")])));
        let selection = r#"{"missions":[{"type":"novel","goal":"copy"}]}"#;
        let mut c = conductor(
            FlowRegistry::new(),
            vec![Ok(selection.into())],
            backend.clone(),
        );
        c.arm();
        let events = c.on_transcript("copy".into()).await.unwrap();
        assert_eq!(backend.keys(), vec!["ctrl+c".to_string()]);
        assert!(events
            .iter()
            .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));
    }

    #[tokio::test]
    async fn flow_confirm_pauses_then_resumes_and_continues_queue() {
        // A launch flow (ungranted AppLaunch -> Confirm) pauses; approving it
        // completes it, then the second mission runs.
        let mut launch = open_gmail_flow();
        launch.id = "launch_spotify".into();
        launch.name = "Launch Spotify".into();
        launch.steps = vec![FlowStep {
            id: "s1".into(),
            intent: "launch".into(),
            action: "launch".into(),
            target: None,
            value: Some("Spotify".into()),
            pick: None,
            bind: None,
            wait_before: None,
            postcondition: None,
            on_fail: OnFail::Abort,
        }];
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        let registry = FlowRegistry::from_files([launch, open_gmail_flow()]);
        let selection = r#"{"missions":[
            {"type":"open_flow","id":"launch_spotify","slots":{}},
            {"type":"open_flow","id":"open_gmail","slots":{}}
        ]}"#;
        let mut c = conductor(registry, vec![Ok(selection.into())], backend.clone());
        c.arm();

        let events = c
            .on_transcript("launch spotify then open gmail".into())
            .await
            .unwrap();
        assert!(events.iter().any(|e| matches!(e, ActEvent::Confirm { .. })));
        assert!(matches!(c.state(), ConductorState::AwaitingConfirm));
        assert!(backend.launched().is_empty(), "nothing before approval");

        let after = c.decide(UserDecision::ConfirmAllow).await.unwrap();
        assert_eq!(backend.launched(), vec!["Spotify".to_string()]);
        assert_eq!(backend.opened_uris(), vec!["https://mail.google.com"]);
        assert!(matches!(c.state(), ConductorState::Idle));
        assert!(after
            .iter()
            .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));
    }

    #[tokio::test]
    async fn selection_error_is_surfaced_and_returns_to_baseline() {
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        // Invalid JSON from the model -> selection error.
        let mut c = conductor(FlowRegistry::new(), vec![Ok("not json".into())], backend);
        c.arm();
        let events = c.on_transcript("do something".into()).await.unwrap();
        assert!(events.iter().any(|e| matches!(e, ActEvent::Error { .. })));
        assert!(matches!(c.state(), ConductorState::Idle));
    }
}
