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

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
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
use super::grounding_packet::{
    GroundingPacket, DEFAULT_MAX_ELEMENTS, DEFAULT_MAX_NAME_CHARS, SCREENSHOT_MAX_ELEMENTS,
};
use super::llm::LlmClient;
use super::plan_mode::PlanMode;
use super::planner::{Perception, PlanRequest, PlanSource, Planner, CONTINUATION_MARKER};
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

/// How long the adaptive post-navigation settle polls the screen before it stops
/// waiting and re-plans anyway. A page/app usually repaints well under this; we
/// proceed on the FIRST snapshot that differs from the pre-navigation screen, so
/// this is only the worst-case ceiling, not a flat wait.
const NAV_SETTLE_BUDGET_MS: u64 = 1800;

/// Poll cadence while waiting for the screen to change after a navigation.
const NAV_SETTLE_POLL_MS: u64 = 120;

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
    /// Screen-aware perception mode for novel planning (tree / hybrid / vision).
    /// Set from config after construction; defaults to `Tree`.
    plan_mode: PlanMode,
    /// Where a composed document is saved. `None` resolves to the OS Documents
    /// folder at save time; tests override it to a scratch directory.
    documents_dir: Option<PathBuf>,
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
            plan_mode: PlanMode::Tree,
            documents_dir: None,
        }
    }

    /// Override where composed documents are saved (defaults to the OS Documents
    /// folder). Used by tests to redirect saves to a scratch directory.
    pub fn set_documents_dir(&mut self, dir: PathBuf) {
        self.documents_dir = Some(dir);
    }

    /// Set the screen-aware perception mode used for novel planning. Called after
    /// construction from the persisted `act_plan_mode` config.
    pub fn set_plan_mode(&mut self, mode: PlanMode) {
        self.plan_mode = mode;
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

        // Fast-path grammar FIRST. Fixed verbs (copy/paste/cut/undo/redo/select-all/
        // save/new-tab/close-tab/next-field/submit/stop) resolve to a key combo with
        // NO screen snapshot and NO model round-trip. Running it here — ahead of the
        // UIA snapshot and the Gemini selection call below — is what makes "copy that"
        // land in well under 100ms instead of first paying a full tree walk plus a
        // selection round-trip only to arrive at the same key press. A miss (`None`)
        // falls through to the normal snapshot + routing path unchanged.
        if let Some(plan) = super::fastpath::resolve(&transcript) {
            let task_id = "t0".to_string();
            events.push(ActEvent::TaskSpawned {
                id: task_id.clone(),
                label: short_goal(&transcript),
                status: Some("running".into()),
            });
            self.current_task = Some(task_id);
            let started = std::time::Instant::now();
            let step = self.run_fastpath(plan, &transcript, &mut events).await;
            tracing::info!(
                target: "act_scoreboard",
                mode = "fastpath",
                total_ms = started.elapsed().as_millis() as u64,
                "act scoreboard"
            );
            match step {
                Step::Paused(kind) => self.suspend(kind, VecDeque::new(), &mut events),
                Step::Next | Step::Stop => {
                    self.current_task = None;
                    self.state = self.baseline();
                    events.push(self.state_event());
                }
            }
            return Ok(events);
        }

        // Per-stage timers: the snapshot walk and the selection round-trip are the
        // two costs a novel command pays before any work; surfaced on the scoreboard
        // line so a regression (e.g. a fast path that silently stopped being fast) is
        // measurable instead of a mystery.
        let route_started = std::time::Instant::now();

        // Observe the screen into the blackboard (context for routing + planning).
        if let Ok(snapshot) = self.backend.snapshot().await {
            self.board.observe(&snapshot);
        }
        let snapshot_ms = route_started.elapsed().as_millis() as u64;

        // Route onto the drawer.
        tracing::info!(transcript = %transcript, "Act on_transcript: routing");
        let selection_started = std::time::Instant::now();
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
        let selection_ms = selection_started.elapsed().as_millis() as u64;

        let summary: Vec<String> = selection
            .missions
            .iter()
            .map(|m| match m {
                Mission::OpenFlow { id, .. } => format!("open_flow:{id}"),
                Mission::Novel { .. } => "novel".to_string(),
                Mission::Compose { .. } => "compose".to_string(),
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

        // Spawn every agent's card up front, dimmed as `queued`, the moment the
        // mission list resolves — so the board shows all N orbs at once instead of
        // one appearing lazily as each mission starts. `run_mission` later flips the
        // matching card to `running`.
        for (id, mission) in &queue {
            events.push(ActEvent::TaskSpawned {
                id: id.clone(),
                label: self.mission_label(mission),
                status: Some("queued".into()),
            });
        }

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
        // Scoreboard: one comparable line per command so the three perception
        // modes can be graded on the same tasks (goal-level result + which mode
        // ran), now with the per-stage latency that used to be invisible — the
        // snapshot walk, the selection round-trip, and the whole-command total.
        // This is the instrument that catches a "fast path" silently costing
        // seconds (the fastpath-behind-selection bug) instead of it being a mystery.
        tracing::info!(
            target: "act_scoreboard",
            mode = self.plan_mode.as_str(),
            results,
            errors,
            ok = (errors == 0 && results > 0),
            snapshot_ms,
            selection_ms,
            total_ms = route_started.elapsed().as_millis() as u64,
            "act scoreboard"
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
        // The card was already spawned (as `queued`) when the command's mission list
        // resolved. Flip it to `running` before any work begins — a progress line is
        // the "started" signal the board uses to light the orb up.
        self.current_task = Some(task_id.clone());
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
            Mission::Compose {
                topic,
                kind,
                target_app,
            } => {
                tracing::info!(kind = %kind, topic = %topic, target_app = ?target_app, "Act composing content");
                self.run_compose(topic, kind, target_app, events).await
            }
            Mission::Answer { question } => {
                tracing::info!(question = %question, "Act answering (talk-back)");
                self.run_answer(question, events).await
            }
        }
    }

    /// Execute a fast-path plan (fixed-verb grammar) directly — no snapshot, no
    /// selection, no planner. Runs the single-shot plan once and classifies its
    /// outcome with the SAME logic the novel loop uses, tagged [`PlanSource::FastPath`]
    /// so a clean completion is treated as goal-done (not "one step of a longer
    /// goal"). The one place a fixed verb can pause is the destructive classifier
    /// forcing a confirm on a bare Enter aimed at a destructive focused control
    /// ("submit" while a Delete button is focused) — that routes through the normal
    /// Novel confirm/resume path via the returned [`Step::Paused`].
    async fn run_fastpath(
        &mut self,
        plan: ActionPlan,
        transcript: &str,
        events: &mut Vec<ActEvent>,
    ) -> Step {
        let result = self
            .executor
            .execute_plan_with_context(plan.clone(), transcript)
            .await;
        let mut history = Vec::new();
        match self.classify_novel(
            plan,
            PlanSource::FastPath,
            result,
            transcript,
            &mut history,
            events,
        ) {
            LoopStep::Return(step) => step,
            // A fixed verb has no re-plan loop; if the single action didn't cleanly
            // finish (a recoverable step failure / backend error), close the card as
            // failed here rather than escalating to the planner.
            LoopStep::Continue => {
                let summary = "Couldn't do that".to_string();
                self.finish_task(false, &summary, events);
                events.push(ActEvent::Result { ok: false, summary });
                Step::Next
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
            Mission::Compose { kind, topic, .. } => {
                let k = kind.trim();
                let k = if k.is_empty() { "note" } else { k };
                short_goal(&format!("write a {k}: {topic}"))
            }
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

    /// Generate substantive document content for an authoring request, SAVE it, and
    /// — when the user named an app — OPEN it in front of them.
    ///
    /// The selection layer routes "write / draft / compose a report / summary /
    /// letter about X" here (NOT to `take_a_note`, which types a short LITERAL
    /// snippet). We GENERATE the body with one LLM call
    /// ([`super::compose::compose_body`]) and write it as a `.docx` into the user's
    /// Documents folder ([`super::compose::save_docx`]) — a reliable pass that never
    /// loses the work.
    ///
    /// When the request named a `target_app` ("write an email **in Word**"), we then
    /// OPEN the saved document in its default handler so it lands in front of the user
    /// (a `.docx` opens in Word) instead of only sitting on disk — this is what "open
    /// Microsoft Word" means for a fresh compose. We open the file we just AUTHORED,
    /// never a screen-derived path, so it is a trusted, deterministic step (no
    /// capability confirm) with no observe/act/re-plan loop. The body is already saved,
    /// so if opening fails we still report success with the path. When no app is named,
    /// the document is saved silently, as before. (To TYPE into an already-open editor,
    /// the selection prompt routes "open Word and write …" to the Novel/planner path.)
    async fn run_compose(
        &mut self,
        topic: String,
        kind: String,
        target_app: Option<String>,
        events: &mut Vec<ActEvent>,
    ) -> Step {
        let body = super::compose::compose_body(self.llm.as_ref(), &topic, &kind).await;
        if body.trim().is_empty() {
            let message = format!("Couldn't generate the {}", short_kind(&kind));
            self.finish_task(false, &message, events);
            events.push(ActEvent::Error { message });
            return Step::Next;
        }

        let dir = self.compose_documents_dir();
        let path = match super::compose::save_docx(&dir, &topic, &kind, &body) {
            Ok(path) => path,
            Err(e) => {
                let message = format!("Couldn't save the {}: {e}", short_kind(&kind));
                self.finish_task(false, &message, events);
                events.push(ActEvent::Error { message });
                return Step::Next;
            }
        };
        self.board.record(format!("saved a {}", short_kind(&kind)));

        // The user named an app to write in → open the saved document so it shows up
        // in front of them (a blank spoken target collapses to None upstream, so a
        // present value is a real request). Opening is best-effort: the work is saved.
        let wants_open = target_app
            .as_deref()
            .map(str::trim)
            .is_some_and(|a| !a.is_empty());
        let opened = wants_open
            && self
                .backend
                .open_path(&path.to_string_lossy())
                .await
                .is_ok();

        let summary = if opened {
            format!("Wrote the {} and opened it", short_kind(&kind))
        } else {
            format!("Saved the {} to {}", short_kind(&kind), path.display())
        };
        self.finish_task(true, &summary, events);
        events.push(ActEvent::Result { ok: true, summary });
        Step::Next
    }

    /// The directory composed documents are saved to: the test/config override if
    /// set, else the OS Documents folder, else the current directory as a last
    /// resort (so a headless environment without a home still saves *somewhere*).
    fn compose_documents_dir(&self) -> PathBuf {
        self.documents_dir
            .clone()
            .or_else(dirs::document_dir)
            .unwrap_or_else(|| PathBuf::from("."))
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
        // Normalized URIs already opened during THIS goal. Feeds the anti-re-
        // navigation guard so the planner can't reopen a page (and spawn another
        // browser tab) it already navigated to earlier in the same loop.
        let mut visited_uris: HashSet<String> = HashSet::new();

        for iter in 0..MAX_NOVEL_ITERS {
            // (a) Fresh snapshot -> grounding packet + a cheap fingerprint. A
            // snapshot failure (e.g. a wedged UIA provider) degrades to empty
            // grounding rather than sinking the command: untargeted actions
            // (launch, type, key, uri) still run; targeted ones fail validation.
            let (packet, fingerprint) = match self.backend.snapshot().await {
                Ok(snap) => {
                    self.board.observe(&snap);
                    let fp = snapshot_fingerprint(&snap);
                    // A screenshot turn (hybrid / vision) leans on the image and
                    // often acts by coordinate, so serialize a leaner tree; tree
                    // mode keeps the full budget.
                    let max_elems = if self.plan_mode.needs_screenshot() {
                        SCREENSHOT_MAX_ELEMENTS
                    } else {
                        DEFAULT_MAX_ELEMENTS
                    };
                    let packet =
                        GroundingPacket::from_snapshot(&snap, max_elems, DEFAULT_MAX_NAME_CHARS);
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

            // (c) Perception: for hybrid / vision modes, capture a screenshot to
            // hand the planner. A missing platform impl (`Ok(None)`) or a capture
            // error degrades to tree grounding for this turn rather than failing —
            // the mode toggle never breaks Act.
            let perception = if self.plan_mode.needs_screenshot() {
                match self.backend.capture_screen().await {
                    Ok(Some(png)) => Perception {
                        mode: self.plan_mode,
                        screenshot_png: Some(png),
                    },
                    Ok(None) => {
                        tracing::debug!(
                            mode = self.plan_mode.as_str(),
                            "act novel: no screen capture available; using tree grounding"
                        );
                        Perception::tree()
                    }
                    Err(e) => {
                        tracing::warn!(mode = self.plan_mode.as_str(), error = %e, "act novel: screen capture failed; using tree grounding");
                        Perception::tree()
                    }
                }
            } else {
                Perception::tree()
            };

            // (d) Plan the next batch.
            let (plan, source) = match self
                .planner
                .plan_perceived(
                    PlanRequest {
                        transcript: goal.clone(),
                        packet,
                        prior_context,
                    },
                    perception,
                )
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

            // Anti-re-navigation guard: drop any `uri` to a page we already opened
            // this goal (which would spawn another tab and reset progress). If that
            // empties the batch, it becomes a short wait + re-observe instead.
            let plan = dedupe_navigation(plan, &mut visited_uris, &mut history);

            // Adaptive wait: a batch that navigates (uri / launch) usually carries a
            // trailing flat `wait` the model guessed to let the page/app appear. Drop
            // that blind sleep and instead poll the screen after execution (see
            // `settle_after_nav`), proceeding on the first change. `fingerprint` (this
            // iteration's pre-execution screen) is the baseline the poll compares
            // against.
            let navigated = plan_navigates(&plan);
            let plan = if navigated {
                strip_trailing_nav_waits(plan)
            } else {
                plan
            };

            // No-progress guard: identical first action AND unchanged screen two
            // iterations running means the loop is stuck — bail with a clear result
            // rather than spinning.
            let first_sig = plan
                .actions
                .first()
                .map(action_signature)
                .unwrap_or_default();
            let sig = (first_sig, fingerprint.clone());
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
                    // If this batch navigated, adaptively settle before re-planning:
                    // poll until the screen changes from `fingerprint` (or the budget
                    // elapses) instead of trusting the model's flat wait.
                    if navigated {
                        self.settle_after_nav(&fingerprint).await;
                    }
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

    /// Adaptive post-navigation settle: after a batch that opened a URL or launched
    /// an app, poll the snapshot fingerprint and return as soon as the screen
    /// differs from `baseline_fp` (the page/app has appeared), or after
    /// [`NAV_SETTLE_BUDGET_MS`]. This replaces the model's blind flat `wait` (which
    /// burns its full guess even when the page painted in a fraction of it) with an
    /// adaptive one: a screen that settles in 300ms proceeds in ~300ms. The freshly
    /// observed snapshot is folded into the blackboard so the next plan grounds on
    /// the settled screen.
    async fn settle_after_nav(&mut self, baseline_fp: &str) {
        let start = std::time::Instant::now();
        let budget = std::time::Duration::from_millis(NAV_SETTLE_BUDGET_MS);
        loop {
            if let Ok(snap) = self.backend.snapshot().await {
                if snapshot_fingerprint(&snap) != baseline_fp {
                    self.board.observe(&snap);
                    return;
                }
            }
            if start.elapsed() >= budget {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(NAV_SETTLE_POLL_MS)).await;
        }
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

/// Normalize a URI for "have we already opened this?" comparison: trimmed,
/// lowercased, fragment stripped, no trailing slash. Deliberately lenient — two
/// URIs that differ only in case, a trailing slash, or a `#fragment` are the same
/// destination for the purpose of not opening it twice.
fn normalize_uri(uri: &str) -> String {
    let base = uri.split('#').next().unwrap_or(uri);
    base.trim().trim_end_matches('/').to_lowercase()
}

/// Strip redundant navigations from a freshly planned batch.
///
/// The planner occasionally re-issues a `uri` to a page it already opened during
/// THIS goal — a transient blank/loading frame or a focus blip makes the current
/// screen look "wrong", so it tries to open the destination again. On Windows
/// every such re-open spawns a NEW browser tab and throws away the progress
/// already made, which is exactly the tab-spawning loop users hit ("it opens
/// YouTube, then opens YouTube again, and never plays").
///
/// So: drop any `uri` whose normalized destination is already in `visited`, and
/// record the ones we keep. If dropping empties the batch, fall back to a single
/// short `wait` — the page we already opened is almost certainly still settling,
/// and re-observing after a beat is strictly better than opening it a third time.
fn dedupe_navigation(
    plan: ActionPlan,
    visited: &mut HashSet<String>,
    history: &mut Vec<String>,
) -> ActionPlan {
    let mut kept: Vec<Action> = Vec::with_capacity(plan.actions.len());
    let mut dropped_nav = false;
    for action in plan.actions {
        match &action {
            Action::Uri { uri, .. } => {
                let key = normalize_uri(uri);
                // Empty (defensive) or first-seen this goal -> allow the open.
                if key.is_empty() || visited.insert(key) {
                    kept.push(action);
                } else {
                    dropped_nav = true;
                }
            }
            _ => kept.push(action),
        }
    }
    if kept.is_empty() && dropped_nav {
        push_progress(
            history,
            "already on that page — waiting for it to load".into(),
        );
        return ActionPlan::new(vec![Action::Wait { ms: 1200 }]);
    }
    ActionPlan::new(kept)
}

/// Whether a batch performs a navigation — opens a URL or launches an app. These
/// are the actions after which a page/app needs a beat to appear, so a navigating
/// batch triggers the adaptive settle (and the trailing-wait strip) in the loop.
fn plan_navigates(plan: &ActionPlan) -> bool {
    plan.actions
        .iter()
        .any(|a| matches!(a, Action::Uri { .. } | Action::Launch { .. }))
}

/// Drop the blind flat `wait` the model appends after a navigation, so the
/// Conductor's adaptive settle governs the pause instead. Only waits that TRAIL
/// the final navigation with nothing but more waits / a `stop` after them are
/// removed — the canonical "launch, wait 5000, stop" tail. A `wait` that gates a
/// later interactive step in the SAME batch (e.g. `uri, wait, type`) is kept,
/// since dropping it would type into a page that hasn't loaded.
fn strip_trailing_nav_waits(plan: ActionPlan) -> ActionPlan {
    let actions = plan.actions;
    let Some(nav_idx) = actions
        .iter()
        .rposition(|a| matches!(a, Action::Uri { .. } | Action::Launch { .. }))
    else {
        return ActionPlan::new(actions);
    };
    // Only strip when everything after the last navigation is inert (wait / stop);
    // otherwise a later action depends on the wait and it must stay.
    let inert_tail = actions
        .iter()
        .skip(nav_idx + 1)
        .all(|a| matches!(a, Action::Wait { .. } | Action::Stop));
    if !inert_tail {
        return ActionPlan::new(actions);
    }
    let kept: Vec<Action> = actions
        .into_iter()
        .enumerate()
        .filter(|(i, a)| !(*i > nav_idx && matches!(a, Action::Wait { .. })))
        .map(|(_, a)| a)
        .collect();
    ActionPlan::new(kept)
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
        Action::Click { x, y } => format!("clicked at ({x}, {y})"),
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

/// A short, safe label for a composed document kind ("report", "summary", …) for
/// user-facing result strings, defaulting to "document" when the kind is blank.
fn short_kind(kind: &str) -> String {
    let k = kind.trim();
    if k.is_empty() {
        "document".to_string()
    } else {
        short_goal(k)
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
    async fn spawns_all_cards_queued_up_front_then_flips_to_running() {
        // The board must show every agent the instant the mission list resolves:
        // one `queued` TaskSpawned per mission, all emitted before any of them runs
        // (before the first TaskProgress that flips a card to running).
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        let registry = FlowRegistry::from_files([open_gmail_flow()]);
        let selection = r#"{"missions":[
            {"type":"open_flow","id":"open_gmail","slots":{}},
            {"type":"open_flow","id":"open_gmail","slots":{}}
        ]}"#;
        let mut c = conductor(registry, vec![Ok(selection.into())], backend.clone());
        c.arm();

        let events = c.on_transcript("open gmail twice".into()).await.unwrap();

        // Two queued cards, keyed t0/t1, each carrying the queued status.
        let spawned: Vec<(&str, Option<&str>)> = events
            .iter()
            .filter_map(|e| match e {
                ActEvent::TaskSpawned { id, status, .. } => Some((id.as_str(), status.as_deref())),
                _ => None,
            })
            .collect();
        assert_eq!(
            spawned,
            vec![("t0", Some("queued")), ("t1", Some("queued"))]
        );

        // No mission re-spawns a card; the running flip is a progress event, and
        // every queued spawn precedes the first one.
        let last_spawn = events
            .iter()
            .rposition(|e| matches!(e, ActEvent::TaskSpawned { .. }))
            .unwrap();
        let first_progress = events
            .iter()
            .position(|e| matches!(e, ActEvent::TaskProgress { .. }))
            .unwrap();
        assert!(
            last_spawn < first_progress,
            "all cards must spawn before any mission runs"
        );
    }

    fn scratch_docs_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("otl-conductor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn compose_with_target_app_saves_and_opens_the_document() {
        // The real bug: "write a detailed report in Notepad on X" typed the LITERAL
        // instruction, and a later fix saved a .docx but NEVER opened the app the user
        // named. Compose must now GENERATE a body, SAVE it as a .docx, and — because
        // the user named an app — OPEN that saved document in its default handler so it
        // lands in front of them. It must NOT type the instruction and must NOT drive
        // the app with keystrokes/clipboard (opening a file is not editor automation).
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        let docs = scratch_docs_dir();

        let report =
            "MRI of the Right Foot: Morton Neuroma\n\nTechnique: ...\n\nFindings: A well-defined \
             mass is seen in the third intermetatarsal space.\n\nImpression: Findings consistent \
             with a Morton neuroma.";
        // Two LLM turns share the fixture: (1) selection routes to compose, (2)
        // compose_body returns the generated report.
        let selection = r#"{"missions":[
            {"type":"compose","topic":"MRI right foot, imaging features of Morton neuroma","kind":"report","target_app":"Notepad"}
        ]}"#;
        let compose = format!(r#"{{"body":{report:?}}}"#);
        let mut c = conductor(
            FlowRegistry::new(),
            vec![Ok(selection.into()), Ok(compose)],
            backend.clone(),
        );
        c.set_documents_dir(docs.clone());
        c.arm();

        let events = c
            .on_transcript(
                "write a detailed report in Notepad on MRI right foot with imaging features of Morton neuroma".into(),
            )
            .await
            .unwrap();

        // Exactly one .docx was saved to the Documents dir.
        let saved: Vec<_> = std::fs::read_dir(&docs)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("docx"))
            .collect();
        assert_eq!(saved.len(), 1, "one report should be saved, got {saved:?}");
        assert!(
            std::fs::read(&saved[0]).unwrap().starts_with(b"PK"),
            "a real .docx"
        );

        // The saved document was opened exactly once, by its own path — and NOTHING
        // was typed, pasted, keyed, or app-focused (opening ≠ editor automation).
        let launched = backend.launched();
        assert_eq!(
            launched.len(),
            1,
            "the saved doc should be opened once, got {launched:?}"
        );
        assert_eq!(launched[0], saved[0].to_string_lossy());
        assert!(backend.typed().is_empty());
        assert!(backend.clipboard_sets().is_empty());
        assert!(backend.keys().is_empty());
        assert!(backend.focused_apps().is_empty());

        // The result reports that the document was written AND opened.
        assert!(events.iter().any(|e| matches!(
            e,
            ActEvent::Result { ok: true, summary } if summary.contains("opened")
        )));
        assert!(matches!(c.state(), ConductorState::Armed));
        std::fs::remove_dir_all(&docs).ok();
    }

    #[tokio::test]
    async fn compose_without_target_app_saves_silently_and_opens_nothing() {
        // No app named → save the .docx and report the path; open nothing, automate
        // nothing. This preserves the quiet "just file it" path for a bare compose.
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        let docs = scratch_docs_dir();
        let note = "A Short Note\n\nJust a body line.";
        let selection = r#"{"missions":[
            {"type":"compose","topic":"a short note about nothing","kind":"note"}
        ]}"#;
        let compose = format!(r#"{{"body":{note:?}}}"#);
        let mut c = conductor(
            FlowRegistry::new(),
            vec![Ok(selection.into()), Ok(compose)],
            backend.clone(),
        );
        c.set_documents_dir(docs.clone());
        c.arm();

        let events = c
            .on_transcript("write a note about nothing".into())
            .await
            .unwrap();

        let saved: Vec<_> = std::fs::read_dir(&docs)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("docx"))
            .collect();
        assert_eq!(saved.len(), 1, "one note should be saved, got {saved:?}");

        // Nothing opened, nothing automated.
        assert!(
            backend.launched().is_empty(),
            "no app opened without a target"
        );
        assert!(backend.focused_apps().is_empty());
        assert!(backend.typed().is_empty());
        assert!(backend.clipboard_sets().is_empty());
        assert!(backend.keys().is_empty());

        // The result reports the saved path (not "opened").
        assert!(events.iter().any(|e| matches!(
            e,
            ActEvent::Result { ok: true, summary } if summary.contains(".docx")
        )));
        std::fs::remove_dir_all(&docs).ok();
    }

    #[tokio::test]
    async fn compose_generation_failure_surfaces_an_error_and_saves_nothing() {
        // If generation fails, we must never save a blank/apology document.
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        let docs = scratch_docs_dir();
        let selection = r#"{"missions":[
            {"type":"compose","topic":"anything","kind":"report"}
        ]}"#;
        let mut c = conductor(
            FlowRegistry::new(),
            vec![
                Ok(selection.into()),
                Err(crate::error::AppError::Network("down".into())),
            ],
            backend.clone(),
        );
        c.set_documents_dir(docs.clone());
        c.arm();

        let events = c
            .on_transcript("write a report on anything".into())
            .await
            .unwrap();
        let count = std::fs::read_dir(&docs).unwrap().count();
        assert_eq!(count, 0, "nothing saved on generation failure");
        assert!(backend.typed().is_empty());
        assert!(events.iter().any(|e| matches!(e, ActEvent::Error { .. })));
        std::fs::remove_dir_all(&docs).ok();
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
        // over the real built-in drawer. Turn 1 opens a settings deep-link (leaf
        // uri, via selection); turn 2 copies — which the fast-path grammar resolves
        // to ctrl+c with NO model round-trip, so it consumes no canned response;
        // turn 3 asks a question (talk-back answer, via selection). The blackboard
        // persists across turns and nothing throws end to end.
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
            // (no response for "copy that" — the fast path handles it before selection)
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
    async fn fastpath_command_runs_before_selection_without_a_model_call() {
        // "copy that" must resolve via the fast-path grammar at the TOP of
        // on_transcript — ahead of the snapshot + selection round-trip — so it
        // needs no LLM response at all. With zero canned responses, any selection
        // call would fail; the command still presses ctrl+c and reports success,
        // proving the fast path skips the model entirely.
        let backend = Arc::new(MockBackend::new(snap(vec![])));
        let mut c = conductor(FlowRegistry::new(), vec![], backend.clone());
        c.arm();

        let events = c.on_transcript("copy that".into()).await.unwrap();

        assert_eq!(backend.keys(), vec!["ctrl+c".to_string()]);
        assert!(events
            .iter()
            .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));
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
    fn dedupe_navigation_drops_repeat_uri_and_waits() {
        // The tab-spawning loop: the planner re-issues a `uri` to a page it already
        // opened this goal. First open is allowed; the re-open is dropped, and since
        // that empties the batch it becomes a short wait so the loop re-observes
        // instead of spawning another browser tab.
        let mut visited = HashSet::new();
        let mut history = Vec::new();
        let url = "https://www.youtube.com/results?search_query=hotel+california";

        let first = dedupe_navigation(
            ActionPlan::new(vec![Action::Uri {
                uri: url.into(),
                origin: Default::default(),
            }]),
            &mut visited,
            &mut history,
        );
        assert_eq!(first.actions.len(), 1);
        assert!(matches!(first.actions[0], Action::Uri { .. }));

        // Same page again (trailing slash + case differ but it's the same target).
        let again = dedupe_navigation(
            ActionPlan::new(vec![Action::Uri {
                uri: "https://www.YouTube.com/results?search_query=hotel+california/".into(),
                origin: Default::default(),
            }]),
            &mut visited,
            &mut history,
        );
        assert_eq!(again.actions.len(), 1);
        assert!(
            matches!(again.actions[0], Action::Wait { .. }),
            "a redundant navigation must collapse to a wait, not a re-open"
        );
    }

    #[test]
    fn dedupe_navigation_keeps_new_uri_and_trailing_actions() {
        // A batch that opens a *new* page then acts on it is untouched; only an
        // already-visited destination is stripped.
        let mut visited = HashSet::new();
        visited.insert(normalize_uri("https://www.youtube.com"));
        let mut history = Vec::new();

        let plan = dedupe_navigation(
            ActionPlan::new(vec![
                Action::Uri {
                    uri: "https://www.youtube.com".into(), // already visited -> dropped
                    origin: Default::default(),
                },
                Action::Uri {
                    uri: "https://www.youtube.com/results?search_query=x".into(), // new -> kept
                    origin: Default::default(),
                },
                Action::Wait { ms: 500 },
            ]),
            &mut visited,
            &mut history,
        );
        assert_eq!(plan.actions.len(), 2);
        assert!(matches!(plan.actions[0], Action::Uri { .. }));
        assert!(matches!(plan.actions[1], Action::Wait { .. }));
    }

    #[test]
    fn plan_navigates_detects_uri_and_launch() {
        assert!(plan_navigates(&ActionPlan::new(vec![Action::Uri {
            uri: "https://youtube.com".into(),
            origin: Default::default(),
        }])));
        assert!(plan_navigates(&ActionPlan::new(vec![Action::Launch {
            target: "spotify".into(),
            origin: Default::default(),
        }])));
        assert!(!plan_navigates(&ActionPlan::new(vec![
            Action::Key {
                combo: "ctrl+a".into()
            },
            Action::Wait { ms: 500 },
        ])));
    }

    #[test]
    fn strip_trailing_nav_waits_drops_the_blind_post_nav_wait() {
        // The canonical "launch, wait 5000, stop": the trailing wait is the model's
        // blind guess, replaced by the adaptive settle — it is removed; the launch
        // and stop stay.
        let plan = strip_trailing_nav_waits(ActionPlan::new(vec![
            Action::Launch {
                target: "spotify".into(),
                origin: Default::default(),
            },
            Action::Wait { ms: 5000 },
            Action::Stop,
        ]));
        let kinds: Vec<&str> = plan.actions.iter().map(|a| a.kind()).collect();
        assert_eq!(kinds, vec!["launch", "stop"]);
    }

    #[test]
    fn strip_trailing_nav_waits_keeps_a_wait_that_gates_a_later_step() {
        // `uri, wait, type`: the wait gates the type (which needs the page loaded),
        // so it must NOT be stripped — the tail is not inert.
        let plan = strip_trailing_nav_waits(ActionPlan::new(vec![
            Action::Uri {
                uri: "https://example.com".into(),
                origin: Default::default(),
            },
            Action::Wait { ms: 800 },
            Action::Type {
                text: "hello".into(),
                clear: false,
            },
        ]));
        let kinds: Vec<&str> = plan.actions.iter().map(|a| a.kind()).collect();
        assert_eq!(kinds, vec!["uri", "wait", "type"]);
    }

    #[test]
    fn strip_trailing_nav_waits_is_a_noop_without_navigation() {
        let plan = strip_trailing_nav_waits(ActionPlan::new(vec![
            Action::Key {
                combo: "ctrl+a".into(),
            },
            Action::Wait { ms: 500 },
        ]));
        let kinds: Vec<&str> = plan.actions.iter().map(|a| a.kind()).collect();
        assert_eq!(kinds, vec!["key", "wait"]);
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
