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

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use super::action::{Action, ActionPlan};
use super::backend::AccessibilityBackend;
use super::blackboard::Blackboard;
use super::element::Snapshot;
use super::events::{ActEvent, AskOption};
use super::executor::{ExecError, ExecResult, Executor, StepOutcome, UserDecision};
use super::flow::{FlowFile, SlotFilter};
use super::flow_registry::FlowRegistry;
use super::flow_runner::{
    substitute_value, FlowOutcome, FlowRunError, FlowRunner, Resume, ResumeDecision,
};
use super::grounding_packet::{GroundingPacket, DEFAULT_MAX_ELEMENTS, DEFAULT_MAX_NAME_CHARS};
use super::llm::LlmClient;
use super::planner::{PlanRequest, PlanSource, Planner, CONTINUATION_MARKER};
use super::selection::{self, Mission, SelectionError};

/// Slot name -> value, as filled by the selection layer.
type SlotMap = HashMap<String, String>;

/// Cap on control names listed in a talk-back screen summary.
const SCREEN_SUMMARY_CAP: usize = 24;

/// Upper bound on observe->plan->act iterations for one novel goal. Each iteration
/// re-observes the screen and re-plans the next batch, so the loop adapts when the
/// UI isn't what a one-shot plan assumed — but it must terminate.
const MAX_NOVEL_ITERS: usize = 8;

/// Cap on the trusted per-goal progress history carried between iterations.
const NOVEL_HISTORY_CAP: usize = 16;

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
    /// Stable board id of the paused mission, so its `TaskResult` matches its card.
    task_id: String,
    /// Missions not yet started, resumed after the paused one finishes.
    queue: VecDeque<QueuedMission>,
}

/// A mission tagged with the stable board id its card is keyed by (e.g. "t0").
type QueuedMission = (String, Mission);

enum PendingKind {
    /// Paused inside a leaf recipe.
    Flow {
        file: Box<FlowFile>,
        slots: SlotMap,
        resume: Resume,
        /// Options offered (to map a numbered pick back to a row).
        options: Vec<AskOption>,
    },
    /// Paused inside a novel plan. Carries the pre-approved `remaining` batch to
    /// run first on resume, PLUS the `goal` and trusted `history` so that after the
    /// user answers, execution re-enters the closed loop (re-observe + re-plan the
    /// rest of the goal) rather than only replaying a stale `remaining`.
    Novel {
        remaining: ActionPlan,
        goal: String,
        history: Vec<String>,
    },
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
    state: ConductorState,
    pending: Option<Pending>,
    /// The board id of the mission currently running (set in `run_mission`, and
    /// restored on resume), so a terminal outcome emits the matching `TaskResult`.
    current_task: Option<String>,
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

/// Where the novel closed loop lands after classifying one iteration's execution
/// result: either terminate (handing a [`Step`] back to the mission driver) or
/// re-observe and re-plan the next batch.
enum LoopStep {
    Return(Step),
    Continue,
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
    ) -> Self {
        Self {
            registry,
            llm,
            runner,
            planner,
            executor,
            backend,
            board: Blackboard::new(),
            state: ConductorState::Idle,
            pending: None,
            current_task: None,
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

    /// The state to return to after a command / abort. The Conductor stays
    /// *armed* (ready for the next command) until explicitly disarmed on disable —
    /// so a persistent voice assistant, and the dedicated Act hotkey, always find
    /// it ready. (The STT hold-to-talk vs hands-free distinction lives in the
    /// recording layer, not here.)
    fn baseline(&self) -> ConductorState {
        ConductorState::Armed
    }

    /// Handle one final transcript end to end. Returns the events to emit.
    ///
    /// Barge-in: a new command while the Conductor is *paused* for the user
    /// (awaiting a confirm / pick) abandons that pause and runs the new command —
    /// the user changed their mind. A command while actively `Working` is
    /// rejected as `Busy` (the caller trips the kill switch via [`Self::abort`]
    /// to interrupt in-flight execution).
    pub async fn on_transcript(
        &mut self,
        transcript: String,
    ) -> Result<Vec<ActEvent>, ConductorError> {
        // Barge-in: a new command abandons any paused mission. Capture its card id
        // so we can close the card out below — a card must never hang on "running".
        let superseded = match self.state {
            ConductorState::Armed => None,
            ConductorState::AwaitingConfirm | ConductorState::AwaitingChoice => {
                self.pending.take().map(|p| p.task_id)
            }
            ConductorState::Idle => return Err(ConductorError::NotArmed),
            ConductorState::Working => return Err(ConductorError::Busy),
        };

        // Fresh kill switches for a new command.
        self.runner.kill_switch().reset();
        self.executor.kill_switch().reset();

        let mut events = Vec::new();
        // Emit a terminal result for the abandoned card (don't rely on the UI's
        // own reset), then clear the tracked task so a later branch can't
        // misattribute a result to it.
        if let Some(id) = superseded.filter(|s| !s.is_empty()) {
            events.push(ActEvent::TaskResult {
                id,
                ok: false,
                summary: "Superseded by a new command".into(),
            });
        }
        self.current_task = None;
        self.state = ConductorState::Working;
        events.push(self.state_event());

        // Observe the screen into the blackboard (context for routing + planning).
        if let Ok(snapshot) = self.backend.snapshot().await {
            self.board.observe(&snapshot);
        }

        // Route onto the drawer.
        tracing::info!(transcript = %transcript, "Act on_transcript: routing");
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
                let message = describe_selection_error(&e);
                tracing::warn!(error = %e, "Act selection failed");
                self.state = self.baseline();
                events.push(ActEvent::Error { message });
                events.push(self.state_event());
                return Ok(events);
            }
        };

        let summary: Vec<String> = selection
            .missions
            .iter()
            .map(|m| match m {
                Mission::OpenFlow { id, .. } => format!("open_flow:{id}"),
                Mission::Novel { .. } => "novel".to_string(),
                Mission::Answer { .. } => "answer".to_string(),
            })
            .collect();
        tracing::info!(
            count = selection.missions.len(),
            missions = ?summary,
            "Act selection resolved"
        );

        // Tag each mission with a stable board id ("t0", "t1", …) so the Agents
        // board can key one card per mission across its whole lifecycle.
        let queue: VecDeque<QueuedMission> = selection
            .missions
            .into_iter()
            .enumerate()
            .map(|(i, m)| (format!("t{i}"), m))
            .collect();
        self.drive_queue(queue, &mut events).await;

        let errors = events
            .iter()
            .filter(|e| matches!(e, ActEvent::Error { .. }))
            .count();
        let results = events
            .iter()
            .filter(|e| matches!(e, ActEvent::Result { .. }))
            .count();
        tracing::info!(
            results,
            errors,
            total_events = events.len(),
            "Act command finished"
        );
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

        let Pending {
            kind,
            task_id,
            queue,
        } = pending;
        self.current_task = Some(task_id);
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

    /// Undo the last edit — send the focused app's own Undo (Ctrl+Z). This is the
    /// honest, universally-reversible meaning of "undo": it reverses the last
    /// typing/edit in whatever surface has focus. Actions with no app-level undo
    /// (an opened URL, a launched app) are simply not reversed by it.
    pub async fn undo(&mut self) -> Result<Vec<ActEvent>, ConductorError> {
        if !self.is_armed() {
            return Err(ConductorError::NotArmed);
        }
        if matches!(self.state, ConductorState::Working) {
            return Err(ConductorError::Busy);
        }
        // Barge past a pause: undo is itself the user's new intent.
        self.pending = None;
        self.executor.kill_switch().reset();

        let mut events = Vec::new();
        let ok = self.backend.key_combo("ctrl+z").await.is_ok();
        self.board.record("undo (ctrl+z)");
        events.push(ActEvent::Result {
            ok,
            summary: if ok {
                "Undid the last edit".into()
            } else {
                "Couldn't send undo".into()
            },
        });
        self.state = self.baseline();
        events.push(self.state_event());
        Ok(events)
    }

    /// Run missions until the queue drains or one pauses / aborts.
    async fn drive_queue(
        &mut self,
        mut queue: VecDeque<QueuedMission>,
        events: &mut Vec<ActEvent>,
    ) {
        while let Some((id, mission)) = queue.pop_front() {
            match self.run_mission(id, mission, events).await {
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
    fn suspend(
        &mut self,
        kind: PendingKind,
        queue: VecDeque<QueuedMission>,
        events: &mut Vec<ActEvent>,
    ) {
        self.state = match &kind {
            PendingKind::Flow { .. } | PendingKind::Novel { .. } => match events.last() {
                Some(ActEvent::AskUser { .. }) => ConductorState::AwaitingChoice,
                _ => ConductorState::AwaitingConfirm,
            },
        };
        // The paused card stays "running"; remember its id for the eventual result.
        let task_id = self.current_task.clone().unwrap_or_default();
        self.pending = Some(Pending {
            kind,
            task_id,
            queue,
        });
        events.push(self.state_event());
    }

    /// Carry out one mission. `task_id` keys its card on the Agents board.
    async fn run_mission(
        &mut self,
        task_id: String,
        mission: Mission,
        events: &mut Vec<ActEvent>,
    ) -> Step {
        // Announce the card and mark it running before any work begins.
        self.current_task = Some(task_id.clone());
        events.push(ActEvent::TaskSpawned {
            id: task_id.clone(),
            label: self.mission_label(&mission),
        });
        events.push(ActEvent::TaskProgress {
            id: task_id,
            text: "Working…".into(),
        });

        match mission {
            Mission::OpenFlow { id, slots, .. } => {
                tracing::info!(flow = %id, slots = ?slots, "Act running flow");
                self.run_flow(&id, slots, events).await
            }
            Mission::Novel { goal, .. } => {
                tracing::info!(goal = %goal, "Act running novel goal (planner)");
                self.run_novel(goal, events).await
            }
            Mission::Answer { question } => {
                tracing::info!(question = %question, "Act answering (talk-back)");
                self.run_answer(question, events).await
            }
        }
    }

    /// A short human title for a mission's card: the saved flow's name (falling
    /// back to its id), the novel goal, or the question asked.
    fn mission_label(&self, mission: &Mission) -> String {
        match mission {
            Mission::OpenFlow { id, .. } => self
                .registry
                .open(id)
                .map(|f| f.name.clone())
                .unwrap_or_else(|| id.clone()),
            Mission::Novel { goal, .. } => short_goal(goal),
            Mission::Answer { question } => short_goal(question),
        }
    }

    /// Emit the terminal `TaskResult` for the running card, if one is tracked.
    fn finish_task(&self, ok: bool, summary: &str, events: &mut Vec<ActEvent>) {
        if let Some(id) = &self.current_task {
            events.push(ActEvent::TaskResult {
                id: id.clone(),
                ok,
                summary: summary.to_string(),
            });
        }
    }

    /// Answer a question from the current state (talk-back), without acting.
    async fn run_answer(&mut self, question: String, events: &mut Vec<ActEvent>) -> Step {
        let screen = self.screen_summary().await;
        let context = self.board.context_summary();
        let text = super::answer::answer(self.llm.as_ref(), &question, &context, &screen).await;
        self.board
            .record(format!("answered: {}", short_goal(&question)));
        self.finish_task(true, &text, events);
        events.push(ActEvent::Say { text });
        Step::Next
    }

    /// A compact, fenced list of on-screen control names for a talk-back answer.
    /// PHI-free (names/labels only, capped), and empty when the screen can't be read.
    async fn screen_summary(&self) -> String {
        let Ok(snapshot) = self.backend.snapshot().await else {
            return String::new();
        };
        let mut names: Vec<&str> = snapshot
            .interactive()
            .map(|e| e.name.as_str())
            .filter(|n| !n.is_empty())
            .take(SCREEN_SUMMARY_CAP)
            .collect();
        names.dedup();
        if names.is_empty() {
            format!(
                "SCREEN: {} — {} (no named controls)",
                snapshot.app, snapshot.window_title
            )
        } else {
            format!(
                "SCREEN: {} — {}; controls: {}",
                snapshot.app,
                snapshot.window_title,
                names.join(", ")
            )
        }
    }

    /// Open a saved recipe and replay it (a branch hands its context to the planner).
    async fn run_flow(&mut self, id: &str, mut slots: SlotMap, events: &mut Vec<ActEvent>) -> Step {
        let Some(file) = self.registry.open(id).cloned() else {
            let message = format!("that saved task is unavailable ({id})");
            self.finish_task(false, &message, events);
            events.push(ActEvent::Error { message });
            return Step::Next;
        };
        // Fill any declared slot that the model left unset with its default, so an
        // optional slot referenced in a value never renders as a literal `{token}`.
        for slot in &file.slots {
            if !slots.contains_key(&slot.name) {
                if let Some(default) = &slot.default {
                    slots.insert(slot.name.clone(), default.clone());
                }
            }
        }
        let local = std::sync::Mutex::new(Vec::new());
        let outcome = {
            let emit = |e: ActEvent| local.lock().unwrap().push(e);
            self.runner.run(&file, &slots, &emit).await
        };
        events.extend(local.into_inner().unwrap());
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
                let summary = if verified {
                    format!("Done: {}", file.name)
                } else {
                    format!("Ran {} (couldn't verify)", file.name)
                };
                self.finish_task(true, &summary, events);
                events.push(ActEvent::Result { ok: true, summary });
                Step::Next
            }
            Ok(FlowOutcome::Failed { step, error }) => {
                self.board.record(format!("{} failed", file.id));
                let summary = format!("Couldn't finish {}: {error} (at {step})", file.name);
                self.finish_task(false, &summary, events);
                events.push(ActEvent::Result { ok: false, summary });
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
                // A branch recipe reasons via the planner over its context. The
                // raw `branch_context` still carries `{slot}` placeholders — bake
                // the carried slot *values* in (same filter chain the leaf steps
                // use) so the planner sees "take a note pineapple 1234 test", not
                // the literal "{text}". Any leftover token (a missing required
                // slot) is stripped so a `{name}` never leaks to the planner.
                self.board.record(format!("branch {}", file.id));
                let filters = branch_slot_filters(&file);
                let goal = strip_unresolved_tokens(&substitute_value(&context, &slots, &filters));
                self.run_novel_with(goal, slot_context(&slots), events)
                    .await
            }
            Ok(FlowOutcome::Aborted) => {
                self.finish_task(false, "Stopped", events);
                events.push(ActEvent::Result {
                    ok: false,
                    summary: "Stopped".into(),
                });
                Step::Stop
            }
            Err(e) => {
                let message = format!("Couldn't read the screen: {e}");
                self.finish_task(false, &message, events);
                events.push(ActEvent::Error { message });
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
        // Start-of-goal: clear the executor's focus-guard latch so it begins inert
        // and never inherits a previous goal's expected app. The guard is then
        // armed by this goal's own launch/focus and PERSISTS across the loop's
        // iterations (each iteration runs via `execute_plan_continuing`), so a
        // launch in one iteration still guards a `Type` in a later one. This is the
        // one place the guard resets per novel goal — not per execute call, and not
        // on pause/resume re-entry into `novel_loop`.
        self.executor.reset_focus_guard();
        let mut history: Vec<String> = Vec::new();
        // A branch's carried slot context ("carried slots: text=hello") is trusted;
        // seed it as the first progress note so it rides along every iteration.
        if let Some(extra) = extra_context {
            push_progress(&mut history, extra);
        }
        self.novel_loop(goal, history, events).await
    }

    /// The closed loop (ReAct-style): observe -> plan the next batch -> act ->
    /// re-observe, until the goal is done, a step pauses for the user, the kill
    /// switch trips, or the iteration budget / no-progress guard stops it. Each
    /// iteration re-grounds against a FRESH snapshot, so a goal adapts when the UI
    /// isn't what a one-shot plan assumed.
    async fn novel_loop(
        &mut self,
        goal: String,
        mut history: Vec<String>,
        events: &mut Vec<ActEvent>,
    ) -> Step {
        // (first planned-action signature, snapshot fingerprint) of the previous
        // iteration — the no-progress guard aborts if both repeat unchanged.
        let mut prev_sig: Option<(String, String)> = None;

        for iter in 0..MAX_NOVEL_ITERS {
            // (a) Fresh snapshot -> grounding packet + a cheap fingerprint. A
            // snapshot failure (e.g. a wedged UIA provider) degrades to empty
            // grounding rather than sinking the command: untargeted actions
            // (launch, type, key, uri) still run; targeted ones fail validation.
            let (packet, fingerprint) = match self.backend.snapshot().await {
                Ok(snap) => {
                    self.board.observe(&snap);
                    let fp = snapshot_fingerprint(&snap);
                    let packet = GroundingPacket::from_snapshot(
                        &snap,
                        DEFAULT_MAX_ELEMENTS,
                        DEFAULT_MAX_NAME_CHARS,
                    );
                    (packet, fp)
                }
                Err(e) => {
                    tracing::warn!(iter, error = %e, "act novel: snapshot failed; planning with empty grounding");
                    (GroundingPacket::empty(), String::new())
                }
            };

            // (b) Trusted context = session state + the progress block (goal +
            // history). The progress block's marker puts the planner in next-step
            // (continuation) mode. History holds only OUR outcome descriptions and
            // the goal — never raw untrusted screen text.
            let prior_context = novel_prior_context(&self.board, &goal, &history);

            // (c) Plan the next batch.
            let (plan, source) = match self
                .planner
                .plan(PlanRequest {
                    transcript: goal.clone(),
                    packet,
                    prior_context,
                })
                .await
            {
                Ok(res) => (res.plan, res.source),
                Err(e) => {
                    let message = e.to_string();
                    tracing::warn!(iter, error = %message, "act novel: planning failed");
                    self.finish_task(false, &message, events);
                    events.push(ActEvent::Error { message });
                    return Step::Next;
                }
            };

            // No-progress guard: identical first action AND unchanged screen two
            // iterations running means the loop is stuck — bail with a clear result
            // rather than spinning.
            let first_sig = plan
                .actions
                .first()
                .map(action_signature)
                .unwrap_or_default();
            let sig = (first_sig, fingerprint);
            if prev_sig.as_ref() == Some(&sig) {
                tracing::warn!(
                    iter,
                    "act novel: no progress (same first action + unchanged screen); aborting"
                );
                let summary = format!("Couldn't make progress: {}", history_tail(&history));
                self.board.record(short_goal(&goal));
                self.finish_task(false, &summary, events);
                events.push(ActEvent::Result { ok: false, summary });
                return Step::Next;
            }
            prev_sig = Some(sig);

            let kinds: Vec<&str> = plan.actions.iter().map(|a| a.kind()).collect();
            tracing::info!(
                iter,
                goal = %short_goal(&goal),
                source = ?source,
                actions = ?kinds,
                "act novel: executing iteration"
            );

            // (d) Execute the batch. Use the "continuing" variant so the focus
            // guard armed by an earlier iteration's launch/focus survives into
            // this one — the guard is reset once per goal in `run_novel_with`,
            // never per iteration.
            let result = self
                .executor
                .execute_plan_continuing(plan.clone(), &goal)
                .await;

            // (e) Classify: pause, stop, terminal success/failure, or continue.
            match self.classify_novel(plan, source, result, &goal, &mut history, events) {
                LoopStep::Return(step) => {
                    tracing::info!(iter, "act novel: iteration terminal");
                    return step;
                }
                LoopStep::Continue => {
                    tracing::info!(iter, "act novel: adapting — re-observing and re-planning");
                    continue;
                }
            }
        }

        tracing::warn!(
            iters = MAX_NOVEL_ITERS,
            "act novel: exhausted iteration budget"
        );
        let summary = format!(
            "Couldn't finish after {MAX_NOVEL_ITERS} tries: {}",
            history_tail(&history)
        );
        self.board.record(short_goal(&goal));
        self.finish_task(false, &summary, events);
        events.push(ActEvent::Result { ok: false, summary });
        Step::Next
    }

    /// Classify one iteration's executor result and update the trusted `history`.
    /// Terminal cases (done / blocked / aborted / paused) emit their events and
    /// return [`LoopStep::Return`]; a per-step failure or backend error records the
    /// error and returns [`LoopStep::Continue`] so the loop re-observes and adapts.
    /// Shared by both the loop and the pause/resume re-entry path.
    fn classify_novel(
        &mut self,
        plan: ActionPlan,
        source: PlanSource,
        result: Result<ExecResult, ExecError>,
        goal: &str,
        history: &mut Vec<String>,
        events: &mut Vec<ActEvent>,
    ) -> LoopStep {
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                let msg = short_err(&e.to_string());
                tracing::warn!(error = %msg, "act novel: executor error; re-observing to adapt");
                push_progress(history, format!("execution error: {msg}"));
                return LoopStep::Continue;
            }
        };

        // Trusted, PHI-free notes for the steps that completed (fed to the next
        // plan so the model knows what's already done and won't repeat it). Also
        // record any app/URI a completed step opened onto the blackboard so a later
        // iteration (or a later mission) reuses it instead of relaunching a second
        // copy — this is the cross-task "what's already open" memory.
        for o in &result.outcomes {
            if let StepOutcome::Done { action, .. } = o {
                if let Some(target) = opened_target(action) {
                    self.board.note_opened(target);
                }
                if let Some(note) = done_summary(action) {
                    push_progress(history, note);
                }
            }
        }

        let done = result
            .outcomes
            .iter()
            .take_while(|o| matches!(o, StepOutcome::Done { .. }))
            .count();
        let remaining: Vec<Action> = plan.actions.iter().skip(done).cloned().collect();

        match result.outcomes.last() {
            Some(StepOutcome::NeedsConfirm { action, reason }) => {
                events.push(ActEvent::Confirm {
                    summary: format!("{} {}", action.kind(), action.target().unwrap_or("")),
                    reason: reason.clone(),
                });
                LoopStep::Return(Step::Paused(PendingKind::Novel {
                    remaining: ActionPlan::new(remaining),
                    goal: goal.to_string(),
                    history: history.clone(),
                }))
            }
            Some(StepOutcome::NeedsAskUser { prompt, options }) => {
                events.push(ActEvent::AskUser {
                    prompt: prompt.clone(),
                    options: options.clone(),
                });
                LoopStep::Return(Step::Paused(PendingKind::Novel {
                    remaining: ActionPlan::new(remaining),
                    goal: goal.to_string(),
                    history: history.clone(),
                }))
            }
            Some(StepOutcome::Aborted) => {
                self.finish_task(false, "Stopped", events);
                events.push(ActEvent::Result {
                    ok: false,
                    summary: "Stopped".into(),
                });
                LoopStep::Return(Step::Stop)
            }
            // A safety refusal (capability / elevated / classifier) is terminal —
            // re-planning would only re-hit the same boundary.
            Some(StepOutcome::Denied { reason, .. }) => {
                push_progress(history, format!("blocked: {}", short_err(reason)));
                let summary = format!("Blocked: {reason}");
                self.board.record(short_goal(goal));
                self.finish_task(false, &summary, events);
                events.push(ActEvent::Result { ok: false, summary });
                LoopStep::Return(Step::Next)
            }
            // A recoverable step failure (stale target gone, verify failed, backend
            // error): record it and let the loop re-observe + re-plan.
            Some(StepOutcome::Failed { error, .. }) => {
                push_progress(history, format!("step failed: {}", short_err(error)));
                tracing::info!(error = %short_err(error), "act novel: step failed; will re-observe");
                LoopStep::Continue
            }
            // No blocking outcome: the batch completed. Trust the model's
            // completion contract to decide DONE-ness — do NOT infer it from the
            // fact that a batch merely interacted. The goal is done only when the
            // model emitted an explicit `stop` (or a bare stop-only batch), or when
            // the deterministic fast path produced a single-shot plan. An
            // interacting batch that did NOT stop is treated as one step of a
            // multi-step goal (write intro→table→conclusion, fill a multi-field
            // form): CONTINUE — re-observe and re-plan the rest — rather than
            // declaring success after the first ≤6-action batch. The no-progress
            // guard (same first action + unchanged screen) and MAX_NOVEL_ITERS bound
            // the loop, and re-observing before re-acting prevents blind repetition.
            // A genuinely-finished single batch (e.g. "click Retry" → one invoke,
            // model stops) still terminates via its trailing `stop`. A setup-only
            // batch (launch/uri/wait/focus, no stop) likewise continues so the model
            // can act on the app it just brought up.
            _ => {
                let explicit_stop = plan.actions.iter().any(|a| matches!(a, Action::Stop));
                let goal_done =
                    result.completed && (explicit_stop || source == PlanSource::FastPath);
                if goal_done {
                    self.board.record(short_goal(goal));
                    let (ok, summary) = summarize_novel(&result);
                    self.finish_task(ok, &summary, events);
                    events.push(ActEvent::Result { ok, summary });
                    LoopStep::Return(Step::Next)
                } else {
                    LoopStep::Continue
                }
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
                let local = std::sync::Mutex::new(Vec::new());
                let outcome = {
                    let emit = |e: ActEvent| local.lock().unwrap().push(e);
                    self.runner.resume(&file, &slots, resume, rd, &emit).await
                };
                events.extend(local.into_inner().unwrap());
                self.absorb_flow(*file, slots, outcome, events).await
            }
            PendingKind::Novel {
                remaining,
                goal,
                mut history,
            } => {
                // Run the pre-approved batch, then feed its result through the same
                // classifier. If it didn't itself pause/stop/finish, RE-ENTER the
                // loop so the rest of the goal is re-observed and re-planned — not
                // just replayed from the stale `remaining`.
                let result = self
                    .executor
                    .resume_after_user(remaining.clone(), decision)
                    .await;
                match self.classify_novel(
                    remaining,
                    PlanSource::Llm,
                    result,
                    &goal,
                    &mut history,
                    events,
                ) {
                    LoopStep::Return(step) => step,
                    LoopStep::Continue => self.novel_loop(goal, history, events).await,
                }
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

/// A cheap, PHI-free fingerprint of a snapshot (app, window, element count) used
/// by the no-progress guard to tell whether the screen changed between iterations.
fn snapshot_fingerprint(snap: &Snapshot) -> String {
    format!("{}|{}|{}", snap.app, snap.window_title, snap.elements.len())
}

/// A stable, PHI-free signature of an action for the no-progress guard: its kind
/// plus any target path (never the typed text).
fn action_signature(action: &Action) -> String {
    format!("{}:{}", action.kind(), action.target().unwrap_or(""))
}

/// Append a trusted progress note, keeping only the most recent [`NOVEL_HISTORY_CAP`].
fn push_progress(history: &mut Vec<String>, note: String) {
    history.push(note);
    let overflow = history.len().saturating_sub(NOVEL_HISTORY_CAP);
    if overflow > 0 {
        history.drain(0..overflow);
    }
}

/// A trusted, PHI-free one-line note for a completed action, fed back to the
/// planner as progress. Never includes typed text, document content, or on-screen
/// values — only the action kind and (for launch/focus) the app name. Returns
/// `None` for steps that carry no useful progress signal.
fn done_summary(action: &Action) -> Option<String> {
    Some(match action {
        Action::Launch { target, .. } => format!("launched {}", short_goal(target)),
        Action::Uri { .. } => "opened a link".into(),
        Action::Key { combo } => format!("pressed {combo}"),
        Action::Type { .. } => "typed text".into(),
        Action::Invoke { .. } => "activated a control".into(),
        Action::Focus { .. } => "moved focus".into(),
        Action::FocusApp { name } => format!("focused {}", short_goal(name)),
        Action::SelectMenu { .. } => "selected a menu item".into(),
        Action::Scroll { .. } => "scrolled".into(),
        Action::Clipboard { .. } => "used the clipboard".into(),
        Action::Shell { .. } => "ran a command".into(),
        Action::Wait { .. } | Action::Stop | Action::AskUser { .. } => return None,
    })
}

/// The app / URI a completed action opened or focused, for the blackboard's
/// "already open this session" memory. Only the three "bring something up"
/// primitives qualify; every other action returns `None`.
fn opened_target(action: &Action) -> Option<String> {
    match action {
        Action::Launch { target, .. } => Some(target.clone()),
        Action::FocusApp { name } => Some(name.clone()),
        Action::Uri { uri, .. } => Some(uri.clone()),
        _ => None,
    }
}

/// Collapse an error string to a single, bounded line for the history / summary.
fn short_err(e: &str) -> String {
    let one_line = e.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= 80 {
        one_line
    } else {
        one_line.chars().take(79).collect::<String>() + "…"
    }
}

/// The most recent progress note, for a terminal "here's how far we got" summary.
fn history_tail(history: &[String]) -> String {
    history
        .last()
        .cloned()
        .unwrap_or_else(|| "no steps ran".to_string())
}

/// Build the trusted `prior_context` for a loop iteration: the session context
/// block followed by a [`CONTINUATION_MARKER`]-fenced progress block (the goal and
/// our own step notes). Presence of the marker switches the planner to next-step
/// mode. Always `Some` (the goal is always available), so novel planning is always
/// held to the per-iteration action budget.
fn novel_prior_context(board: &Blackboard, goal: &str, history: &[String]) -> Option<String> {
    let mut out = String::new();
    let ctx = board.context_summary();
    if !ctx.trim().is_empty() {
        out.push_str(&ctx);
        out.push('\n');
    }
    out.push_str(CONTINUATION_MARKER);
    out.push_str(
        " (trusted — what has already happened this goal; SCREEN_CONTEXT is the CURRENT screen)\n",
    );
    out.push_str(&format!("goal: {goal}\n"));
    out.push_str("steps so far:\n");
    if history.is_empty() {
        out.push_str("  - (nothing yet)\n");
    } else {
        for h in history {
            out.push_str(&format!("  - {h}\n"));
        }
    }
    out.push_str("<<<END_PROGRESS");
    Some(out)
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

/// A trusted `key=value` context note from a branch's carried slots, handed to
/// the planner alongside the substituted goal so it can reason over the actual
/// values (not just their names). `None` when there are no slots.
fn slot_context(slots: &SlotMap) -> Option<String> {
    if slots.is_empty() {
        return None;
    }
    let mut pairs: Vec<(&String, &String)> = slots.iter().collect();
    pairs.sort_unstable_by(|a, b| a.0.cmp(b.0));
    let rendered: Vec<String> = pairs.iter().map(|(k, v)| format!("{k}={v}")).collect();
    Some(format!("carried slots: {}", rendered.join(", ")))
}

/// The slot-filter default map for a branch file, mirroring the leaf runner's
/// `slot_filter_defaults` so a value baked into `branch_context` is transformed
/// exactly as it would be inside a leaf step.
fn branch_slot_filters(file: &FlowFile) -> HashMap<String, Vec<SlotFilter>> {
    file.slots
        .iter()
        .filter(|s| !s.filters.is_empty())
        .map(|s| (s.name.clone(), s.filters.clone()))
        .collect()
}

/// Drop any leftover `{token}` placeholder — a slot that was neither provided
/// nor defaulted — so a missing required slot degrades to a gap rather than a
/// literal `{name}` echoed to the planner (and ultimately typed into an app).
fn strip_unresolved_tokens(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => rest = &after[close + 1..],
            // No closing brace — the remainder is literal text, keep it.
            None => {
                out.push_str(&rest[open..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
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
        assert!(matches!(c.state(), ConductorState::Armed));
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
    async fn novel_loop_completes_in_one_iteration_when_model_stops() {
        // A single-batch goal the model finishes in one iteration by emitting the
        // interaction AND a trailing `stop` (its completion contract). The closed
        // loop terminates on the stop — exactly one planner call (a second would
        // exhaust the fixture and panic). Proves the "click Retry → one invoke,
        // model stops" case still terminates cleanly under the trust-the-stop rule.
        let backend = Arc::new(MockBackend::new(snap(vec![el(
            "#/1",
            Role::Button,
            "Play",
        )])));
        let selection = r#"{"missions":[{"type":"novel","goal":"press the play button"}]}"#;
        let plan = r##"{"actions":[{"op":"invoke","target":"#/1"},{"op":"stop"}]}"##;
        let mut c = conductor(
            FlowRegistry::new(),
            vec![Ok(selection.into()), Ok(plan.into())],
            backend.clone(),
        );
        c.arm();
        let events = c
            .on_transcript("press the play button".into())
            .await
            .unwrap();
        assert_eq!(backend.invoked(), vec!["#/1".to_string()]);
        assert!(events
            .iter()
            .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));
        assert!(matches!(c.state(), ConductorState::Armed));
    }

    #[tokio::test]
    async fn novel_loop_continues_after_interacting_batch_without_stop() {
        // The premature-success fix (#3): a batch that INTERACTED (a type) but did
        // NOT emit `stop` must NOT be declared done — a multi-step goal (write
        // intro→conclusion) needs more iterations. Iter 1 types the intro (no stop)
        // -> the loop continues, re-observes, and re-plans; iter 2 emits stop ->
        // success. Three LLM turns (selection + two planner calls) proves the loop
        // ran twice rather than terminating after the first interacting batch.
        let backend = Arc::new(MockBackend::new(snap(vec![el(
            "#/1",
            Role::TextField,
            "Body",
        )])));
        let selection = r#"{"missions":[{"type":"novel","goal":"write the intro then finish"}]}"#;
        let iter1 = r#"{"actions":[{"op":"type","text":"intro"}]}"#;
        let iter2 = r#"{"actions":[{"op":"stop"}]}"#;
        let mut c = conductor(
            FlowRegistry::new(),
            vec![Ok(selection.into()), Ok(iter1.into()), Ok(iter2.into())],
            backend.clone(),
        );
        c.arm();
        let events = c
            .on_transcript("write the intro then finish".into())
            .await
            .unwrap();
        // The interaction happened once (iter 1), and the loop only stopped on the
        // model's explicit stop (iter 2) — not prematurely after iter 1.
        assert_eq!(backend.typed(), vec!["intro".to_string()]);
        assert!(events
            .iter()
            .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));
        assert!(matches!(c.state(), ConductorState::Armed));
    }

    #[tokio::test]
    async fn novel_loop_reobserves_after_setup_only_batch_until_stop() {
        // Iter 1 is a setup-only uri (no stop) — the loop must NOT declare the goal
        // done; it re-observes and re-plans. Iter 2 the model emits stop -> success.
        // Three LLM turns total (selection + two planner calls) proves the loop ran
        // twice and terminated cleanly.
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        let selection = r#"{"missions":[{"type":"novel","goal":"open the page then finish"}]}"#;
        let iter1 =
            r#"{"actions":[{"op":"uri","uri":"https://example.com","origin":"world_knowledge"}]}"#;
        let iter2 = r#"{"actions":[{"op":"stop"}]}"#;
        let mut c = conductor(
            FlowRegistry::new(),
            vec![Ok(selection.into()), Ok(iter1.into()), Ok(iter2.into())],
            backend.clone(),
        );
        c.arm();
        let events = c
            .on_transcript("open the page then finish".into())
            .await
            .unwrap();
        assert_eq!(
            backend.opened_uris(),
            vec!["https://example.com".to_string()]
        );
        assert!(events
            .iter()
            .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));
        assert!(matches!(c.state(), ConductorState::Armed));
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
        assert!(matches!(c.state(), ConductorState::Armed));
        assert!(after
            .iter()
            .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));
    }

    #[tokio::test]
    async fn answer_mission_replies_without_acting() {
        let backend = Arc::new(MockBackend::new(snap(vec![el(
            "#/1",
            Role::Button,
            "Play",
        )])));
        // First LLM turn routes to an Answer; the second is the talk-back reply.
        let selection = r#"{"missions":[{"type":"answer","question":"what can I click?"}]}"#;
        let reply = r#"{"answer":"You can click Play."}"#;
        let mut c = conductor(
            FlowRegistry::new(),
            vec![Ok(selection.into()), Ok(reply.into())],
            backend.clone(),
        );
        c.arm();
        let events = c.on_transcript("what can I click?".into()).await.unwrap();
        assert!(events
            .iter()
            .any(|e| matches!(e, ActEvent::Say { text } if text.contains("Play"))));
        // Talk-back never acts.
        assert!(backend.keys().is_empty());
        assert!(backend.invoked().is_empty());
        assert!(matches!(c.state(), ConductorState::Armed));
    }

    #[tokio::test]
    async fn barge_in_abandons_a_pause_and_runs_the_new_command() {
        let mut launch = open_gmail_flow();
        launch.id = "launch_spotify".into();
        launch.steps[0].action = "launch".into();
        launch.steps[0].value = Some("Spotify".into());
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        let registry = FlowRegistry::from_files([launch, open_gmail_flow()]);
        let first = r#"{"missions":[{"type":"open_flow","id":"launch_spotify","slots":{}}]}"#;
        let second = r#"{"missions":[{"type":"open_flow","id":"open_gmail","slots":{}}]}"#;
        let mut c = conductor(
            registry,
            vec![Ok(first.into()), Ok(second.into())],
            backend.clone(),
        );
        c.arm();

        // First command pauses on the launch confirm.
        c.on_transcript("launch spotify".into()).await.unwrap();
        assert!(matches!(c.state(), ConductorState::AwaitingConfirm));

        // The user barges in with a different command instead of answering.
        let events = c
            .on_transcript("actually, open gmail".into())
            .await
            .unwrap();
        assert!(
            backend.launched().is_empty(),
            "the abandoned launch never ran"
        );
        assert_eq!(backend.opened_uris(), vec!["https://mail.google.com"]);
        assert!(matches!(c.state(), ConductorState::Armed));
        assert!(events
            .iter()
            .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));
    }

    #[tokio::test]
    async fn undo_sends_the_focused_app_undo() {
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        let mut c = conductor(FlowRegistry::new(), vec![], backend.clone());
        c.arm();
        let events = c.undo().await.unwrap();
        assert_eq!(backend.keys(), vec!["ctrl+z".to_string()]);
        assert!(events
            .iter()
            .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));
    }

    #[tokio::test]
    async fn undo_before_arming_is_rejected() {
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        let mut c = conductor(FlowRegistry::new(), vec![], backend);
        assert_eq!(c.undo().await, Err(ConductorError::NotArmed));
    }

    #[tokio::test]
    async fn multi_turn_conversation_over_the_real_seed_drawer() {
        // A smoke test of the assembled system: three dictations in a row, routed
        // over the real built-in drawer, each driven by a canned selection call.
        // Turn 1 opens a settings deep-link (leaf uri); turn 2 copies (leaf key);
        // turn 3 asks a question (talk-back answer). The blackboard persists across
        // turns and nothing throws end to end.
        let backend = Arc::new(MockBackend::new(snap(vec![el(
            "#/1",
            Role::Button,
            "Send",
        )])));
        let registry = FlowRegistry::from_files(crate::act::seed::builtin_flows());
        let responses = vec![
            Ok(
                r#"{"missions":[{"type":"open_flow","id":"settings_bluetooth","slots":{}}]}"#
                    .into(),
            ),
            Ok(r#"{"missions":[{"type":"open_flow","id":"copy","slots":{}}]}"#.into()),
            Ok(r#"{"missions":[{"type":"answer","question":"what can I click?"}]}"#.into()),
            Ok(r#"{"answer":"You can click Send."}"#.into()),
        ];
        let mut c = conductor(registry, responses, backend.clone());
        c.arm();

        let e1 = c
            .on_transcript("open bluetooth settings".into())
            .await
            .unwrap();
        assert_eq!(backend.opened_uris(), vec!["ms-settings:bluetooth"]);
        assert!(e1
            .iter()
            .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));
        assert!(matches!(c.state(), ConductorState::Armed));

        let e2 = c.on_transcript("copy that".into()).await.unwrap();
        assert_eq!(backend.keys(), vec!["ctrl+c".to_string()]);
        assert!(e2
            .iter()
            .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));

        let e3 = c.on_transcript("what can I click?".into()).await.unwrap();
        assert!(e3
            .iter()
            .any(|e| matches!(e, ActEvent::Say { text } if text.contains("Send"))));
        assert!(matches!(c.state(), ConductorState::Armed));
    }

    #[tokio::test]
    async fn seed_flow_with_optional_default_slot_runs_clean() {
        // A real seed (compose_gmail) selected with no `to`: the slot default ""
        // fills it, so the URL renders without an unresolved-slot failure.
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        let registry = FlowRegistry::from_files(crate::act::seed::builtin_flows());
        let selection = r#"{"missions":[{"type":"open_flow","id":"compose_gmail","slots":{}}]}"#;
        let mut c = conductor(registry, vec![Ok(selection.into())], backend.clone());
        c.arm();
        let events = c.on_transcript("compose a gmail".into()).await.unwrap();
        assert_eq!(
            backend.opened_uris(),
            vec!["https://mail.google.com/mail/?view=cm&fs=1&to="]
        );
        assert!(events
            .iter()
            .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));
    }

    #[test]
    fn branch_context_substitutes_slot_values_not_names() {
        // The core of B1: a branch's `branch_context` template gets the carried slot
        // *value* baked in (via the same substitute path leaf steps use), so the
        // planner sees the actual text — never the literal `{text}` placeholder.
        let mut slots = SlotMap::new();
        slots.insert("text".into(), "pineapple 1234 test".into());
        let filters = branch_slot_filters(&FlowFile {
            slots: vec![],
            ..open_gmail_flow()
        });
        let resolved =
            strip_unresolved_tokens(&substitute_value("take a note {text}", &slots, &filters));
        assert_eq!(resolved, "take a note pineapple 1234 test");
    }

    #[test]
    fn missing_slot_does_not_leak_a_literal_token() {
        // A required slot the model never filled must not surface as `{name}`.
        let slots = SlotMap::new();
        let resolved = strip_unresolved_tokens(&substitute_value(
            "type {text} now",
            &slots,
            &HashMap::new(),
        ));
        assert_eq!(resolved, "type  now");
        assert!(!resolved.contains('{'));
    }

    #[test]
    fn slot_context_is_trusted_key_value() {
        let mut slots = SlotMap::new();
        slots.insert("song".into(), "yesterday".into());
        slots.insert("text".into(), "hello".into());
        assert_eq!(
            slot_context(&slots).as_deref(),
            Some("carried slots: song=yesterday, text=hello")
        );
        assert_eq!(slot_context(&SlotMap::new()), None);
    }

    #[tokio::test]
    async fn selection_error_is_surfaced_and_returns_to_baseline() {
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        // Invalid JSON from the model -> selection error.
        let mut c = conductor(FlowRegistry::new(), vec![Ok("not json".into())], backend);
        c.arm();
        let events = c.on_transcript("do something".into()).await.unwrap();
        assert!(events.iter().any(|e| matches!(e, ActEvent::Error { .. })));
        assert!(matches!(c.state(), ConductorState::Armed));
    }
}
