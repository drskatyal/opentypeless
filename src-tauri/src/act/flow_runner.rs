//! The drawer runner — replays a saved [`FlowFile`] over an
//! [`AccessibilityBackend`], zero further model calls for a leaf.
//!
//! Opening a leaf file yields a deterministic recipe: run each [`FlowStep`] in
//! order, re-resolving every target against a FRESH snapshot by *semantic
//! selector set* (role + name synonyms + automation id + required patterns), never
//! a pixel or a cached path, so a flow survives the window moving or minor UI
//! drift. A branch file is not run — its `branch_context` is surfaced back to the
//! caller, which loops the planner.
//!
//! Per step: kill-switch check -> progress label -> `wait_before` predicate poll
//! -> selector resolution -> capability gate -> backend execution (raced against
//! the kill switch) -> `postcondition` check honoring `on_fail`. After the steps,
//! the flow's objective `verify` decides whether the run truly succeeded.
//!
//! The gate is the real safety boundary: a `Confirm` ruling pauses the run and is
//! surfaced as [`FlowOutcome::NeedsConfirm`] for the session to resolve, exactly
//! like the executor.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::backend::AccessibilityBackend;
use super::capability::{Capability, CapabilityGate, Decision};
use super::element::{ActionPattern, ElementPath, ElementState, Role, Snapshot, UiElement};
use super::events::{ActEvent, AskOption};
use super::flow::{
    FlowFile, FlowKind, FlowStep, OnFail, PickFallback, PickSpec, Selector, SlotFilter, StateGate,
    VerifySpec, WaitSpec,
};
use super::killswitch::KillSwitch;
use super::shell_policy::{self, ShellVerdict};

/// The shell a leaf `shell` step runs in. All shell recipes are authored as
/// PowerShell (idempotent `New-Item -Force`, `Copy-Item`, … with single-quoted,
/// [`SlotFilter::PsQuote`]-escaped paths), so the runner pins the shell rather
/// than letting a step choose one.
const FLOW_SHELL: &str = "powershell";

/// The score an element must reach to be accepted as a selector's target. A single
/// role match (or a single `name_contains` hit) clears it; noise scores zero.
const RESOLVE_THRESHOLD: i32 = 3;

/// How often the wait/verify pollers re-snapshot while waiting on a UI condition.
const POLL_INTERVAL: Duration = Duration::from_millis(40);

/// A short settle before a `RetryOnce` re-attempt, so a transient mid-drift UI can
/// stabilize before we act again.
const STABLE_WAIT: Duration = Duration::from_millis(150);

/// The upper bound on any single wait/verify, so a bad predicate can't hang a run.
const MAX_WAIT: Duration = Duration::from_secs(15);

/// Defensive cap on `element_ref` / `within_ref` resolution recursion. Real
/// chains are 1–2 deep (bindings never reference other bindings); this only
/// backstops a malformed cyclic binding so it fails cleanly, never overflows.
const MAX_REF_DEPTH: usize = 8;

/// The terminal result of replaying one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowOutcome {
    /// Every step ran; `verified` reflects the objective `verify` (true when the
    /// file has no verify to run against).
    Done { verified: bool },
    /// A step's capability ruled `Confirm`; the session pauses for the user and
    /// resumes the flow.
    NeedsConfirm { reason: String, resume: Resume },
    /// A branch file was opened — its context (and the live slots) go back to the
    /// planner, which reasons and loops.
    Branch {
        context: String,
        slots: HashMap<String, String>,
    },
    /// A step failed (target gone, backend error, unmet postcondition, or a
    /// `Replan` bail). `step` is the step id; `error` is a PHI-free reason.
    Failed { step: String, error: String },
    /// A `pick_result`/`choose` could not decide (no match, or a tie) and its
    /// spec asked to defer to the user. The session offers a numbered pick and
    /// resumes the flow with the chosen row.
    NeedsChoice {
        prompt: String,
        options: Vec<AskOption>,
        resume: Resume,
    },
    /// The kill switch tripped mid-run.
    Aborted,
}

/// A continuation captured when a leaf pauses for the user, so the flow resumes
/// from the paused step (with its accumulated bindings) rather than re-running.
/// Carries no snapshot — the resumed step re-resolves against a fresh one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resume {
    /// Index of the paused step in `file.steps`.
    step_index: usize,
    /// Bindings accumulated by the steps that already ran.
    binds: HashMap<String, Selector>,
}

/// The user's answer to a paused leaf, applied by [`FlowRunner::resume`].
#[derive(Debug, Clone)]
pub enum ResumeDecision {
    /// Confirm: yes — run the paused step now, with its gate granted once.
    Approve,
    /// Confirm: no, or a cancelled choice — the flow fails at the paused step.
    Decline,
    /// The option the user picked for a paused `pick_result` / `choose`.
    Choose(AskOption),
}

/// How the *resumed* step is handled on its first (re-)attempt. Ordinary steps
/// always run with [`StepOverride::None`].
#[derive(Debug, Clone, Default)]
enum StepOverride {
    /// Normal resolution + gating.
    #[default]
    None,
    /// The gate ruled Confirm and the user approved — run this step as Allow.
    GrantGate,
    /// The user chose this row for a paused pick; act on that identity directly.
    UsePicked(AskOption),
}

/// A hard runner failure (a backend the run cannot proceed without), distinct from
/// a per-flow [`FlowOutcome`]. Step-level failures are reported as
/// [`FlowOutcome::Failed`], not as this error.
#[derive(Debug)]
pub enum FlowRunError {
    /// The backend failed a call the runner depends on (e.g. taking a snapshot).
    Backend(String),
}

impl std::fmt::Display for FlowRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlowRunError::Backend(m) => write!(f, "flow runner backend error: {m}"),
        }
    }
}
impl std::error::Error for FlowRunError {}

/// The result of a single step attempt (resolve + gate + execute), before its
/// postcondition is judged.
enum Attempt {
    /// The backend call ran successfully.
    Ran,
    /// The capability gate ruled `Confirm`; pause for the user.
    Confirm(String),
    /// A pick deferred to the user; surface a numbered choice and pause.
    AskChoice {
        prompt: String,
        options: Vec<AskOption>,
    },
    /// The kill switch tripped.
    Aborted,
    /// The step could not run (target gone, denied, backend error, missing value).
    Failed(String),
}

/// Where the step loop lands after fully handling one step (attempt + on_fail).
enum StepFlow {
    /// The step completed; go to the next one.
    Next,
    /// Pause the whole flow for a confirmation.
    Confirm(String),
    /// Pause the whole flow for a numbered user choice.
    AskChoice {
        prompt: String,
        options: Vec<AskOption>,
    },
    /// Abort the whole flow.
    Aborted,
    /// Stop the flow with a failure.
    Failed(String),
}

/// Replays saved flows over a backend, enforcing the capability gate and kill
/// switch. Separate from the executor, but shares its safety discipline.
pub struct FlowRunner {
    backend: Arc<dyn AccessibilityBackend>,
    gate: CapabilityGate,
    kill: KillSwitch,
}

impl FlowRunner {
    pub fn new(
        backend: Arc<dyn AccessibilityBackend>,
        gate: CapabilityGate,
        kill: KillSwitch,
    ) -> Self {
        Self {
            backend,
            gate,
            kill,
        }
    }

    /// The kill switch this runner races every backend await against.
    pub fn kill_switch(&self) -> KillSwitch {
        self.kill.clone()
    }

    /// Replay `file` with `slots` filled, emitting progress via `emit`.
    ///
    /// A branch file returns [`FlowOutcome::Branch`] immediately (the caller loops
    /// the planner). A leaf file runs its steps, then its objective `verify`.
    pub async fn run(
        &self,
        file: &FlowFile,
        slots: &HashMap<String, String>,
        emit: &impl Fn(ActEvent),
    ) -> Result<FlowOutcome, FlowRunError> {
        emit(ActEvent::Step {
            label: format!("opening {}", file.id),
        });

        if file.kind == FlowKind::Branch {
            return Ok(FlowOutcome::Branch {
                context: file.branch_context.clone().unwrap_or_default(),
                slots: slots.clone(),
            });
        }

        let filters = slot_filter_defaults(file);
        self.drive(
            file,
            slots,
            &filters,
            0,
            HashMap::new(),
            StepOverride::None,
            emit,
        )
        .await
    }

    /// Continue a leaf that paused for the user. `decision` answers the pause; the
    /// flow re-runs the paused step (gate granted, or on the chosen row) and then
    /// carries on to the end — bindings and progress are preserved, not restarted.
    pub async fn resume(
        &self,
        file: &FlowFile,
        slots: &HashMap<String, String>,
        resume: Resume,
        decision: ResumeDecision,
        emit: &impl Fn(ActEvent),
    ) -> Result<FlowOutcome, FlowRunError> {
        let filters = slot_filter_defaults(file);
        let Resume { step_index, binds } = resume;
        let over = match decision {
            ResumeDecision::Approve => StepOverride::GrantGate,
            ResumeDecision::Choose(opt) => StepOverride::UsePicked(opt),
            ResumeDecision::Decline => {
                let step = file
                    .steps
                    .get(step_index)
                    .map(|s| s.id.clone())
                    .unwrap_or_default();
                return Ok(FlowOutcome::Failed {
                    step,
                    error: "declined by user".into(),
                });
            }
        };
        self.drive(file, slots, &filters, step_index, binds, over, emit)
            .await
    }

    /// The core step loop, shared by `run` (from step 0) and `resume` (from the
    /// paused step). `first_over` applies only to the step at `start`; every later
    /// step runs normally. On a pause it captures a [`Resume`] with the live binds.
    #[allow(clippy::too_many_arguments)] // a step loop legitimately needs its context
    async fn drive(
        &self,
        file: &FlowFile,
        slots: &HashMap<String, String>,
        filters: &HashMap<String, Vec<SlotFilter>>,
        start: usize,
        mut binds: HashMap<String, Selector>,
        first_over: StepOverride,
        emit: &impl Fn(ActEvent),
    ) -> Result<FlowOutcome, FlowRunError> {
        for (i, step) in file.steps.iter().enumerate().skip(start) {
            // a. The agent can never steer past a trip.
            if self.kill.is_tripped() {
                return Ok(FlowOutcome::Aborted);
            }

            // b. A live, PHI-free progress line (intent when authored, else verb).
            let label = if step.intent.is_empty() {
                step.action.clone()
            } else {
                step.intent.clone()
            };
            emit(ActEvent::Step { label });

            // c. Wait on the UI condition (never a fixed sleep). A timed-out wait is
            //    not itself fatal — resolution below is the real gate.
            if let Some(wait) = &step.wait_before {
                self.wait_for(wait, slots, &binds).await;
            }

            let over = if i == start {
                first_over.clone()
            } else {
                StepOverride::None
            };
            match self
                .run_step(step, slots, filters, &mut binds, over)
                .await?
            {
                StepFlow::Next => continue,
                StepFlow::Confirm(reason) => {
                    return Ok(FlowOutcome::NeedsConfirm {
                        reason,
                        resume: Resume {
                            step_index: i,
                            binds,
                        },
                    })
                }
                StepFlow::AskChoice { prompt, options } => {
                    return Ok(FlowOutcome::NeedsChoice {
                        prompt,
                        options,
                        resume: Resume {
                            step_index: i,
                            binds,
                        },
                    })
                }
                StepFlow::Aborted => return Ok(FlowOutcome::Aborted),
                StepFlow::Failed(error) => {
                    return Ok(FlowOutcome::Failed {
                        step: step.id.clone(),
                        error,
                    })
                }
            }
        }

        // Objective verification: did the outcome actually happen? A file with no
        // verify is taken at face value.
        let verified = match &file.verify {
            Some(spec) => self.verify(spec, slots).await?,
            None => true,
        };
        Ok(FlowOutcome::Done { verified })
    }

    /// Run one step end to end: attempt it, judge its postcondition, and honor
    /// `on_fail` on any failure.
    async fn run_step(
        &self,
        step: &FlowStep,
        slots: &HashMap<String, String>,
        filters: &HashMap<String, Vec<SlotFilter>>,
        binds: &mut HashMap<String, Selector>,
        over: StepOverride,
    ) -> Result<StepFlow, FlowRunError> {
        // First attempt (honoring any resume override).
        let failure = match self.attempt(step, slots, filters, binds, over).await? {
            Attempt::Ran => self.postcondition_failure(step, slots, binds).await,
            Attempt::Confirm(reason) => return Ok(StepFlow::Confirm(reason)),
            Attempt::AskChoice { prompt, options } => {
                return Ok(StepFlow::AskChoice { prompt, options })
            }
            Attempt::Aborted => return Ok(StepFlow::Aborted),
            Attempt::Failed(error) => Some(error),
        };

        let Some(reason) = failure else {
            return Ok(StepFlow::Next);
        };

        // The step failed — honor its recovery policy.
        match step.on_fail {
            OnFail::Abort => Ok(StepFlow::Failed(reason)),
            // Replan bails out of the flow; the session replans the remaining goal.
            OnFail::Replan => Ok(StepFlow::Failed(format!("replan: {reason}"))),
            OnFail::RetryOnce => {
                // Let a mid-drift UI settle, then re-run the whole step once. The
                // retry resolves normally — a granted/picked resume already applied.
                if self.sleep_or_abort(STABLE_WAIT).await {
                    return Ok(StepFlow::Aborted);
                }
                match self
                    .attempt(step, slots, filters, binds, StepOverride::None)
                    .await?
                {
                    Attempt::Ran => match self.postcondition_failure(step, slots, binds).await {
                        Some(again) => Ok(StepFlow::Failed(again)),
                        None => Ok(StepFlow::Next),
                    },
                    Attempt::Confirm(r) => Ok(StepFlow::Confirm(r)),
                    Attempt::AskChoice { prompt, options } => {
                        Ok(StepFlow::AskChoice { prompt, options })
                    }
                    Attempt::Aborted => Ok(StepFlow::Aborted),
                    Attempt::Failed(again) => Ok(StepFlow::Failed(again)),
                }
            }
        }
    }

    /// One attempt at a step: resolve its target against a fresh snapshot
    /// (scored, or a `PickSpec`-driven choice for `pick_result`/`choose`), record
    /// a binding if asked, gate the mapped capability, then run the backend
    /// primitive (raced against the kill switch). Does NOT judge the postcondition.
    async fn attempt(
        &self,
        step: &FlowStep,
        slots: &HashMap<String, String>,
        filters: &HashMap<String, Vec<SlotFilter>>,
        binds: &mut HashMap<String, Selector>,
        over: StepOverride,
    ) -> Result<Attempt, FlowRunError> {
        let action = step.action.as_str();
        let is_pick = matches!(action, "pick_result" | "choose");

        // d. Resolve the control this step addresses, if any. `pick_result`/`choose`
        //    with a `PickSpec` use scored candidate selection; everything else uses
        //    the additive scored resolver. A resumed pick (`UsePicked`) resolves the
        //    row the user chose. Keep the snapshot for binding synthesis.
        let (target, snapshot): (Option<ElementPath>, Option<Snapshot>) =
            if let StepOverride::UsePicked(opt) = &over {
                let snapshot = self.snapshot().await?;
                // Prefer the exact row the user saw when it's still present and
                // unchanged — this distinguishes duplicates the name alone can't.
                // If the tree shifted (path gone / renamed), fall back to re-resolving
                // the chosen name unambiguously.
                let stable = snapshot
                    .get(&opt.path)
                    .filter(|el| el.name == opt.label && !is_unusable(el))
                    .map(|el| el.path.clone());
                let path = match stable.or_else(|| {
                    self.resolve_binding(&snapshot, &picked_selector(step, opt), slots, binds, 0)
                }) {
                    Some(path) => path,
                    None => {
                        return Ok(Attempt::Failed(format!(
                            "the chosen item is no longer on screen for step {}",
                            step.id
                        )))
                    }
                };
                (Some(path), Some(snapshot))
            } else if let Some(sel) = &step.target {
                let snapshot = self.snapshot().await?;
                let path = if let (true, Some(pick)) = (is_pick, &step.pick) {
                    match self.choose_candidate(&snapshot, sel, pick, slots, binds) {
                        Choice::One(path) => path,
                        Choice::None { best } => match pick.if_none {
                            PickFallback::Fail => {
                                return Ok(Attempt::Failed(format!(
                                    "pick: no candidate met the match threshold for step {}",
                                    step.id
                                )))
                            }
                            PickFallback::TakeBest => match best {
                                Some(p) => p,
                                None => {
                                    return Ok(Attempt::Failed(format!(
                                        "pick: no candidates at all for step {}",
                                        step.id
                                    )))
                                }
                            },
                            PickFallback::Ask => {
                                return Ok(self.ask_choice(
                                    &snapshot,
                                    best.into_iter().collect(),
                                    "Which one did you mean?",
                                ))
                            }
                        },
                        Choice::Ambiguous { candidates } => match pick.if_ambiguous {
                            PickFallback::Fail => {
                                return Ok(Attempt::Failed(format!(
                                    "pick: ambiguous candidates for step {}",
                                    step.id
                                )))
                            }
                            PickFallback::TakeBest => match candidates.into_iter().next() {
                                Some(p) => p,
                                None => {
                                    return Ok(Attempt::Failed(format!(
                                        "pick: ambiguous with no candidate for step {}",
                                        step.id
                                    )))
                                }
                            },
                            PickFallback::Ask => {
                                return Ok(self.ask_choice(
                                    &snapshot,
                                    candidates,
                                    "Which one did you mean?",
                                ))
                            }
                        },
                    }
                } else {
                    match self.resolve_selector(&snapshot, sel, slots, binds) {
                        Some(path) => path,
                        None => {
                            return Ok(Attempt::Failed(format!("no target for step {}", step.id)))
                        }
                    }
                };
                (Some(path), Some(snapshot))
            } else {
                (None, None)
            };

        // Record a binding of the resolved element (a re-resolvable identity), so a
        // later step can address it via `element_ref` / `within_ref`.
        if let (Some(name), Some(path), Some(snap)) = (&step.bind, &target, &snapshot) {
            if let Some(identity) = synthesize_binding(snap, path) {
                binds.insert(name.clone(), identity);
            }
        }

        // `choose` is pure selection: it binds a row and does not act on it.
        if action == "choose" {
            return Ok(Attempt::Ran);
        }

        // e. Capability gate. Confirm pauses; Deny fails the step. A `GrantGate`
        //    resume means the user already approved this exact step — run it once
        //    without re-prompting (Deny is a policy hard-stop, never overridden, so
        //    it is not reachable here: only a `Confirm` ruling is ever resumed).
        if !matches!(over, StepOverride::GrantGate) {
            match self.gate.evaluate_capability(capability_for(action)) {
                Decision::Allow => {}
                Decision::Confirm => {
                    let reason = format!("{action} needs confirmation");
                    return Ok(Attempt::Confirm(reason));
                }
                Decision::Deny => {
                    return Ok(Attempt::Failed(format!("{action} denied by policy")));
                }
            }
        }

        // f. Execute, racing the kill switch on every backend await.
        self.execute(step, target.as_deref(), slots, filters).await
    }

    /// Build an [`Attempt::AskChoice`] from candidate paths, labelling each option
    /// with the control's accessible name for a numbered user pick.
    fn ask_choice(
        &self,
        snapshot: &Snapshot,
        candidates: Vec<ElementPath>,
        prompt: &str,
    ) -> Attempt {
        let options = candidates
            .iter()
            .enumerate()
            .map(|(i, path)| AskOption {
                index: i + 1,
                label: snapshot
                    .get(path)
                    .map(|e| e.name.clone())
                    .unwrap_or_default(),
                path: path.clone(),
            })
            .collect();
        Attempt::AskChoice {
            prompt: prompt.to_string(),
            options,
        }
    }

    /// Perform the backend primitive for `action`. Slot tokens in `value` are
    /// substituted first. Every await is raced against the kill switch.
    async fn execute(
        &self,
        step: &FlowStep,
        target: Option<&str>,
        slots: &HashMap<String, String>,
        filters: &HashMap<String, Vec<SlotFilter>>,
    ) -> Result<Attempt, FlowRunError> {
        let action = step.action.as_str();
        let value = step
            .value
            .as_deref()
            .map(|v| substitute_value(v, slots, filters));

        // Fail closed on an unfilled slot: never launch, navigate to, or type a
        // literal `{slot}` into the OS (typing `{password}` or opening a URL with
        // `{q}` in it). A missing required slot should have been caught upstream;
        // this is the last-line guard.
        if let Some(v) = &value {
            if let Some(tok) = first_unresolved_slot(v) {
                return Ok(Attempt::Failed(format!(
                    "unfilled slot {{{tok}}} in step {}",
                    step.id
                )));
            }
        }

        macro_rules! guarded {
            ($fut:expr) => {
                tokio::select! {
                    biased;
                    _ = self.kill.wait_tripped() => return Ok(Attempt::Aborted),
                    res = $fut => res,
                }
            };
        }

        let result: Result<(), String> = match action {
            "launch" => match value {
                Some(v) => guarded!(self.backend.launch(&v)).map_err(|e| e.to_string()),
                None => return Ok(Attempt::Failed("launch step missing value".into())),
            },
            "uri" => match value {
                Some(v) => guarded!(self.backend.open_uri(&v)).map_err(|e| e.to_string()),
                None => return Ok(Attempt::Failed("uri step missing value".into())),
            },
            "focus_app" => match value {
                Some(v) => match guarded!(self.backend.focus_app(&v)) {
                    Ok(true) => Ok(()),
                    Ok(false) => Err(format!("could not focus \"{v}\"")),
                    Err(e) => Err(e.to_string()),
                },
                None => return Ok(Attempt::Failed("focus_app step missing value".into())),
            },
            "key" => match value {
                Some(v) => guarded!(self.backend.key_combo(&v)).map_err(|e| e.to_string()),
                None => return Ok(Attempt::Failed("key step missing value".into())),
            },
            // A scripted PowerShell command. The capability gate (Confirm) already
            // ran before we got here; this is the last-line policy boundary:
            // classify the fully-substituted command with the same independent
            // Deny classifier the executor uses, so a dangerous family (e.g.
            // `Remove-Item -Recurse -Force`) is blocked outright — never merely
            // confirmed. Slots are single-quoted and `psquote`-escaped in the
            // recipe, so an untrusted value stays a literal and cannot inject.
            "shell" => match value {
                Some(v) => {
                    if let ShellVerdict::Deny(reason) =
                        shell_policy::classify_command(&v, FLOW_SHELL)
                    {
                        return Ok(Attempt::Failed(format!(
                            "blocked dangerous command: {reason}"
                        )));
                    }
                    match guarded!(self.backend.run_shell(&v, FLOW_SHELL)) {
                        Ok(out) if out.exit_code == 0 => Ok(()),
                        Ok(out) => {
                            let tail: String = out.stdout.chars().take(200).collect();
                            Err(format!("exit {}: {tail}", out.exit_code))
                        }
                        Err(e) => Err(e.to_string()),
                    }
                }
                None => return Ok(Attempt::Failed("shell step missing value".into())),
            },
            "focus" => match target {
                Some(path) => {
                    let p = path.to_string();
                    guarded!(self.backend.focus(&p)).map_err(|e| e.to_string())
                }
                None => return Ok(Attempt::Failed("focus step has no resolved target".into())),
            },
            "set_value" => match (target, value) {
                (Some(path), Some(v)) => {
                    let p = path.to_string();
                    guarded!(self.backend.set_value(&p, &v)).map_err(|e| e.to_string())
                }
                (None, _) => return Ok(Attempt::Failed("set_value has no resolved target".into())),
                (_, None) => return Ok(Attempt::Failed("set_value step missing value".into())),
            },
            "invoke" | "pick_result" => match target {
                Some(path) => {
                    let p = path.to_string();
                    guarded!(self.backend.invoke(&p)).map_err(|e| e.to_string())
                }
                None => return Ok(Attempt::Failed(format!("{action} has no resolved target"))),
            },
            // A bare wait resolves entirely in `wait_before`; there is nothing to do.
            "wait" => Ok(()),
            other => return Ok(Attempt::Failed(format!("unknown action \"{other}\""))),
        };

        Ok(match result {
            Ok(()) => Attempt::Ran,
            Err(e) => Attempt::Failed(e),
        })
    }

    /// Judge a step's postcondition. Returns `Some(reason)` when the step declared a
    /// postcondition that never became true (the step failed), else `None`.
    async fn postcondition_failure(
        &self,
        step: &FlowStep,
        slots: &HashMap<String, String>,
        binds: &HashMap<String, Selector>,
    ) -> Option<String> {
        let post = step.postcondition.as_ref()?;
        if self.wait_for(post, slots, binds).await {
            None
        } else {
            Some(format!("postcondition \"{}\" not met", post.predicate))
        }
    }

    /// Poll a wait predicate against fresh snapshots until it holds or its timeout
    /// elapses. Returns whether the predicate held. A tripped kill switch ends the
    /// wait early (returning `false`).
    async fn wait_for(
        &self,
        spec: &WaitSpec,
        slots: &HashMap<String, String>,
        binds: &HashMap<String, Selector>,
    ) -> bool {
        let budget = Duration::from_millis(spec.timeout_ms as u64).min(MAX_WAIT);
        let deadline = Instant::now() + budget;
        loop {
            if self.kill.is_tripped() {
                return false;
            }
            let Ok(snapshot) = self.snapshot().await else {
                return false;
            };
            if self.predicate_holds(&snapshot, spec, slots, binds) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            if self.sleep_or_abort(POLL_INTERVAL).await {
                return false;
            }
        }
    }

    /// Evaluate a wait predicate over one snapshot.
    fn predicate_holds(
        &self,
        snapshot: &Snapshot,
        spec: &WaitSpec,
        slots: &HashMap<String, String>,
        binds: &HashMap<String, Selector>,
    ) -> bool {
        match spec.predicate.as_str() {
            // The named control exists (and is acceptable) right now.
            "target_exists" | "value_contains" => spec
                .selector
                .as_ref()
                .map(|sel| self.resolve_selector(snapshot, sel, slots, binds).is_some())
                .unwrap_or(true),
            // Result rows have appeared: a matching selector, else any list item.
            "results_present" => match &spec.selector {
                Some(sel) => self.resolve_selector(snapshot, sel, slots, binds).is_some(),
                None => snapshot.elements.iter().any(|e| e.role == Role::ListItem),
            },
            // Unknown predicate: do not block the flow on it.
            _ => true,
        }
    }

    /// Objective verification for the whole flow: every slot-substituted term must
    /// appear in the live snapshot's observable text (control names, descriptions,
    /// window title, app). Polled until it holds or the spec's timeout elapses.
    async fn verify(
        &self,
        spec: &VerifySpec,
        slots: &HashMap<String, String>,
    ) -> Result<bool, FlowRunError> {
        let budget = Duration::from_millis(spec.timeout_ms as u64).min(MAX_WAIT);
        let deadline = Instant::now() + budget;
        loop {
            if self.kill.is_tripped() {
                return Ok(false);
            }
            let snapshot = self.snapshot().await?;
            if verify_terms_present(&snapshot, spec, slots) {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            if self.sleep_or_abort(POLL_INTERVAL).await {
                return Ok(false);
            }
        }
    }

    /// Objective-aware selector resolution: score every element, hard-rejecting the
    /// unusable, and return the highest scorer above [`RESOLVE_THRESHOLD`].
    ///
    /// Scoring (additive, so the most specific control wins):
    /// * role match: `+3`
    /// * `automation_id_any` hit (path / description): `+6` (a stable id beats
    ///   a localized English name, so it wins when both match)
    /// * exact `name_any` (casefold) hit: `+5`
    /// * slot-substituted `name_contains` substring (casefold): `+4`
    /// * each required pattern the control supports: `+1`
    ///
    /// Hard rejects: password-like fields, disabled controls, offscreen controls,
    /// and anything failing an explicit [`StateGate`]. `nth` disambiguates among
    /// the best-scoring matches by tree order.
    ///
    /// `element_ref` short-circuits to the bound identity's live resolution;
    /// `within_ref` restricts the search to a bound element's subtree — both
    /// re-resolve against *this* snapshot, never a cached path.
    fn resolve_selector(
        &self,
        snapshot: &Snapshot,
        sel: &Selector,
        slots: &HashMap<String, String>,
        binds: &HashMap<String, Selector>,
    ) -> Option<ElementPath> {
        // A step's own target may pick the earliest of equally-good matches (the
        // recipe authored the selector); a *bound identity*, re-resolved below,
        // must be unambiguous or fail — see `resolve_binding`.
        self.resolve_selector_at(snapshot, sel, slots, binds, 0, false)
    }

    /// Re-resolve a *bound* identity to exactly one element, failing (`None`) on a
    /// top-score tie. So a binding to one of several duplicate rows never silently
    /// retargets a later step to a different duplicate — it fails closed instead.
    fn resolve_binding(
        &self,
        snapshot: &Snapshot,
        sel: &Selector,
        slots: &HashMap<String, String>,
        binds: &HashMap<String, Selector>,
        depth: usize,
    ) -> Option<ElementPath> {
        self.resolve_selector_at(snapshot, sel, slots, binds, depth, true)
    }

    /// Depth-guarded resolution. Synthesized bindings never carry refs, so real
    /// recursion is shallow (1–2); the cap is a defensive backstop that fails a
    /// pathological `a→b→a` binding cleanly instead of overflowing the stack.
    ///
    /// `require_unique` rejects a top-score tie (used when re-resolving a bound
    /// identity, where two equal matches mean "which duplicate?" — never guess).
    fn resolve_selector_at(
        &self,
        snapshot: &Snapshot,
        sel: &Selector,
        slots: &HashMap<String, String>,
        binds: &HashMap<String, Selector>,
        depth: usize,
        require_unique: bool,
    ) -> Option<ElementPath> {
        if depth > MAX_REF_DEPTH {
            return None;
        }

        // `element_ref`: act on a previously chosen element — re-resolve its bound
        // identity now (unambiguously). Outer search fields are intentionally not
        // combined: `element_ref` names one specific element; use `within_ref` to
        // search inside it.
        if let Some(name) = &sel.element_ref {
            let bound = binds.get(name)?;
            return self.resolve_binding(snapshot, bound, slots, binds, depth + 1);
        }

        // `within_ref`: scope the search *strictly inside* a bound element's
        // subtree (the container itself is excluded), found by re-resolving its
        // identity in this snapshot — unambiguously, or the scope fails.
        let scope_prefix = match &sel.within_ref {
            Some(name) => {
                Some(self.resolve_binding(snapshot, binds.get(name)?, slots, binds, depth + 1)?)
            }
            None => None,
        };

        let needle = sel
            .name_contains
            .as_ref()
            .map(|c| substitute(c, slots).to_lowercase());

        // Collect every acceptable candidate with its score, in tree order. Role,
        // when specified, is a hard constraint (not a soft score bump) — a saved
        // selector's control type should never be beaten by a same-named control
        // of the wrong role.
        let mut hits: Vec<(i32, &UiElement)> = Vec::new();
        for element in &snapshot.elements {
            if is_unusable(element) || !state_gate_passes(element, &sel.state) {
                continue;
            }
            if let Some(role) = &sel.role {
                if !role_matches(role, element.role) {
                    continue;
                }
            }
            if let Some(prefix) = &scope_prefix {
                if !path_strictly_under(&element.path, prefix) {
                    continue;
                }
            }
            let score = score_element(element, sel, needle.as_deref());
            if score >= RESOLVE_THRESHOLD {
                hits.push((score, element));
            }
        }
        if hits.is_empty() {
            return None;
        }

        // Fail an ambiguous *bound* re-resolution: more than one element sharing
        // the top score means we cannot tell the duplicates apart.
        if require_unique && sel.nth.is_none() {
            let top = hits
                .iter()
                .map(|(s, _)| *s)
                .max()
                .unwrap_or(RESOLVE_THRESHOLD);
            if hits.iter().filter(|(s, _)| *s == top).count() > 1 {
                return None;
            }
        }

        // `nth`: disambiguate among the *best-scoring* matches by tree order —
        // "the 2nd Delete button", not "the 2nd element overall". Without `nth`,
        // the single highest scorer wins (earliest in tree order on a tie).
        match sel.nth {
            Some(n) => {
                let top = hits
                    .iter()
                    .map(|(s, _)| *s)
                    .max()
                    .unwrap_or(RESOLVE_THRESHOLD);
                hits.iter()
                    .filter(|(s, _)| *s == top)
                    .nth(n)
                    .map(|(_, e)| e.path.clone())
            }
            // Keep the earliest tree-order element on a score tie (a stable,
            // predictable target), so `fold` with strict `>` — not `max_by_key`,
            // which would return the *last* maximum.
            None => hits
                .iter()
                .fold(None::<(i32, &UiElement)>, |best, &(s, e)| match best {
                    Some((b, _)) if s <= b => best,
                    _ => Some((s, e)),
                })
                .map(|(_, e)| e.path.clone()),
        }
    }

    /// Choose one candidate row for a `pick_result`/`choose` step per its
    /// [`PickSpec`]: enumerate elements matching the selector's role (optionally
    /// scoped by `within_ref`), reject `negative_patterns`, score each by how many
    /// slot-substituted `match_terms` its name contains, then apply the
    /// confidence thresholds. Choosing is deliberately separate from acting.
    ///
    /// * a single row clearing `min_score` and beating the runner-up by
    ///   `tie_margin` → [`Choice::One`];
    /// * nothing clears `min_score` → [`Choice::None`] (carrying the top row so a
    ///   `TakeBest` fallback can still proceed);
    /// * the top rows tie within `tie_margin` → [`Choice::Ambiguous`].
    fn choose_candidate(
        &self,
        snapshot: &Snapshot,
        sel: &Selector,
        pick: &PickSpec,
        slots: &HashMap<String, String>,
        binds: &HashMap<String, Selector>,
    ) -> Choice {
        // Optional subtree scoping via a bound container (re-resolved
        // unambiguously — a duplicate container fails rather than scoping wrong).
        let scope_prefix = match &sel.within_ref {
            Some(name) => match binds
                .get(name)
                .and_then(|b| self.resolve_binding(snapshot, b, slots, binds, 1))
            {
                Some(p) => Some(p),
                // The bound container is gone or ambiguous — nothing to choose.
                None => return Choice::None { best: None },
            },
            None => None,
        };

        let terms: Vec<String> = pick
            .match_terms
            .iter()
            .map(|t| substitute(t, slots).to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        let negatives: Vec<String> = pick
            .negative_patterns
            .iter()
            .map(|n| substitute(n, slots).to_lowercase())
            .filter(|n| !n.is_empty())
            .collect();

        // Score every acceptable candidate, in tree order (a stable sort later
        // keeps that order among equal scores).
        let mut scored: Vec<(f32, &UiElement)> = Vec::new();
        for element in &snapshot.elements {
            if is_unusable(element) || !state_gate_passes(element, &sel.state) {
                continue;
            }
            if let Some(role) = &sel.role {
                if !role_matches(role, element.role) {
                    continue;
                }
            }
            if let Some(prefix) = &scope_prefix {
                if !path_strictly_under(&element.path, prefix) {
                    continue;
                }
            }
            let name_lc = element.name.to_lowercase();
            if negatives.iter().any(|n| contains_word(&name_lc, n)) {
                continue;
            }
            scored.push((pick_score(&name_lc, &terms), element));
        }

        // Highest score first; stable, so tree order breaks score ties.
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let Some(&(top, top_el)) = scored.first() else {
            return Choice::None { best: None };
        };
        if top < pick.min_score {
            return Choice::None {
                best: Some(top_el.path.clone()),
            };
        }
        let runner_up = scored.get(1).map(|(s, _)| *s).unwrap_or(0.0);
        if top - runner_up < pick.tie_margin {
            let candidates = scored
                .iter()
                .filter(|(s, _)| *s >= pick.min_score && top - *s < pick.tie_margin)
                .map(|(_, e)| e.path.clone())
                .collect();
            return Choice::Ambiguous { candidates };
        }
        Choice::One(top_el.path.clone())
    }

    /// Take a fresh snapshot, mapping a backend failure to a runner error.
    async fn snapshot(&self) -> Result<Snapshot, FlowRunError> {
        self.backend
            .snapshot()
            .await
            .map_err(|e| FlowRunError::Backend(e.to_string()))
    }

    /// Sleep `dur`, but wake early if the kill switch trips. Returns whether it was
    /// the kill switch that woke us (i.e. the caller should abort).
    async fn sleep_or_abort(&self, dur: Duration) -> bool {
        tokio::select! {
            biased;
            _ = self.kill.wait_tripped() => true,
            _ = tokio::time::sleep(dur) => false,
        }
    }
}

/// Map a flow step's primitive verb to the single OS capability it needs, so the
/// gate rules on it exactly as it would an equivalent [`super::action::Action`].
fn capability_for(action: &str) -> Capability {
    match action {
        "launch" => Capability::AppLaunch,
        "uri" => Capability::NetNavigate,
        "focus_app" => Capability::WindowManage,
        "key" => Capability::InputKeyboard,
        "focus" | "invoke" | "pick_result" | "set_value" => Capability::A11yInvoke,
        // A leaf shell step is the highest-risk primitive — the gate pins it to
        // Confirm and no grant can soften it (see `Capability::is_never_allowable`),
        // so a scripted delete / recycle-bin empty always asks first.
        "shell" => Capability::ShellExec,
        // `choose` only reads the tree to bind a row — it acts on nothing, so it
        // is agent-self pacing like `wait` (and any unmapped verb).
        _ => Capability::AgentSelf,
    }
}

/// The outcome of a scored [`FlowRunner::choose_candidate`] over candidate rows.
enum Choice {
    /// A single confident winner.
    One(ElementPath),
    /// Nothing cleared `min_score`; `best` is the top row (for a `TakeBest`
    /// fallback), or `None` when there were no candidates at all.
    None { best: Option<ElementPath> },
    /// The top rows tie within `tie_margin`; `candidates` are those in the tie.
    Ambiguous { candidates: Vec<ElementPath> },
}

/// Fraction (0.0–1.0) of `terms` matched as whole words in a candidate's
/// (lowercased) name. No terms → `0.0`, so a pick with nothing to match on never
/// clears a positive `min_score` and falls through to its `if_none` handling.
///
/// Whole-word (not raw substring) so a term like `app` doesn't score against
/// `happy` and an ad row can't win by embedding query tokens inside longer words.
fn pick_score(name_lc: &str, terms: &[String]) -> f32 {
    if terms.is_empty() {
        return 0.0;
    }
    let hits = terms.iter().filter(|t| contains_word(name_lc, t)).count();
    hits as f32 / terms.len() as f32
}

/// Whether `needle` occurs in `haystack` bounded by non-alphanumerics (i.e. as a
/// whole word/phrase, not embedded in a longer word). Both are expected already
/// lowercased. An empty needle never matches.
fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let bytes = haystack.as_bytes();
    // `match_indices` yields char-boundary byte offsets, so the neighbour byte
    // checks never slice mid-char. A UTF-8 continuation byte isn't ASCII
    // alphanumeric, so a Unicode letter neighbour counts as a word boundary.
    haystack.match_indices(needle).any(|(start, _)| {
        let end = start + needle.len();
        let before_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let after_ok = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
        before_ok && after_ok
    })
}

/// Whether `path` is `prefix` itself or a descendant of it (subtree membership),
/// e.g. `#/2/1` is under `#/2` but `#/20` is not.
fn path_under(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|r| r.starts_with('/'))
}

/// Whether `path` is a *strict* descendant of `prefix` (excludes the container
/// itself) — the meaning of `within_ref` ("act on something inside X, not X").
fn path_strictly_under(path: &str, prefix: &str) -> bool {
    path != prefix && path_under(path, prefix)
}

/// Build a re-resolvable identity for a chosen element, to be stored as a binding.
///
/// Anchors on the two stable *text* handles the element model carries — its
/// accessible name (as `name_any`) and its description (as `automation_id_any`,
/// which the resolver scores highest, `+6`) — plus its role for control typing.
/// Re-resolves the same control in a later fresh snapshot without a cached path.
///
/// Fails closed (`None`) when the element has neither name nor description: a
/// role-only identity would re-resolve to any peer of that role, so we refuse to
/// bind it rather than risk silently retargeting a later step.
fn synthesize_binding(snapshot: &Snapshot, path: &str) -> Option<Selector> {
    let el = snapshot.get(path)?;
    let name_any = if el.name.is_empty() {
        Vec::new()
    } else {
        vec![el.name.clone()]
    };
    let automation_id_any = if el.description.is_empty() {
        Vec::new()
    } else {
        vec![el.description.clone()]
    };
    if name_any.is_empty() && automation_id_any.is_empty() {
        return None;
    }
    Some(Selector {
        role: role_canonical(el.role).map(str::to_string),
        name_any,
        automation_id_any,
        ..Default::default()
    })
}

/// The canonical selector role string for a [`Role`] (inverse of the alias table
/// in [`role_matches`]). `None` for [`Role::Unknown`], which can't be anchored on.
fn role_canonical(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::Button => "button",
        Role::TextField => "edit",
        Role::CheckBox => "check_box",
        Role::RadioButton => "radio_button",
        Role::ComboBox => "combo_box",
        Role::List => "list",
        Role::ListItem => "list_item",
        Role::Menu => "menu",
        Role::MenuBar => "menu_bar",
        Role::MenuItem => "menu_item",
        Role::Tab => "tab",
        Role::TabItem => "tab_item",
        Role::Link => "link",
        Role::Window => "window",
        Role::Pane => "pane",
        Role::Group => "group",
        Role::Text => "text",
        Role::Image => "image",
        Role::Slider => "slider",
        Role::Spinner => "spinner",
        Role::ProgressBar => "progress_bar",
        Role::ScrollBar => "scroll_bar",
        Role::Toolbar => "toolbar",
        Role::TitleBar => "title_bar",
        Role::Separator => "separator",
        Role::Tree => "tree",
        Role::TreeItem => "tree_item",
        Role::Table => "table",
        Role::Row => "row",
        Role::Cell => "cell",
        Role::Document => "document",
        Role::Unknown => return None,
    })
}

/// Replace every `{slot}` token with its value; leave unknown tokens intact.
///
/// Tokens may carry inline filters (`{query|urlencode}`), but this plain form
/// ignores them and substitutes the raw value — it's for comparison contexts
/// (verify terms, `name_contains` needles) that must match the on-screen text,
/// not an encoded form of it. Use [`substitute_value`] for executed values.
pub fn substitute(template: &str, slots: &HashMap<String, String>) -> String {
    substitute_inner(template, slots, None)
}

/// Substitute for an *executed* value (a `uri` / `set_value` / `key` literal):
/// honors inline filters (`{q|urlencode}`) and, for a token with no inline
/// filters, the slot's declared default filter chain. This is why the same
/// `{q}` slot can be URL-encoded in a `uri` step yet typed verbatim elsewhere.
pub fn substitute_value(
    template: &str,
    slots: &HashMap<String, String>,
    defaults: &HashMap<String, Vec<SlotFilter>>,
) -> String {
    substitute_inner(template, slots, Some(defaults))
}

/// The shared `{name|f1|f2}` walker. When `defaults` is `Some`, filters are
/// applied (inline overriding the slot's declared default); when `None`, the raw
/// value is substituted and any inline filter spec is ignored.
fn substitute_inner(
    template: &str,
    slots: &HashMap<String, String>,
    defaults: Option<&HashMap<String, Vec<SlotFilter>>>,
) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let token = &after[..close];
                let mut parts = token.split('|');
                let key = parts.next().unwrap_or("").trim();
                match slots.get(key) {
                    Some(value) => {
                        let rendered = match defaults {
                            None => value.clone(),
                            Some(defaults) => {
                                let inline: Vec<SlotFilter> =
                                    parts.filter_map(parse_filter).collect();
                                let chain: &[SlotFilter] = if inline.is_empty() {
                                    defaults.get(key).map(Vec::as_slice).unwrap_or(&[])
                                } else {
                                    &inline
                                };
                                chain.iter().fold(value.clone(), |acc, f| f.apply(&acc))
                            }
                        };
                        out.push_str(&rendered);
                    }
                    // Unknown token — leave it (name only) verbatim so a missing
                    // slot is visible rather than silently dropped.
                    None => {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            // No closing brace — the remainder is literal.
            None => {
                out.push_str(&rest[open..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Parse one inline filter name (unknown names are ignored, never an error, so a
/// typo degrades to "no transform" rather than aborting a saved flow).
fn parse_filter(name: &str) -> Option<SlotFilter> {
    match name.trim().to_lowercase().as_str() {
        "trim" => Some(SlotFilter::Trim),
        "squish" => Some(SlotFilter::Squish),
        "lower" => Some(SlotFilter::Lower),
        "urlencode" => Some(SlotFilter::Urlencode),
        "psquote" => Some(SlotFilter::PsQuote),
        _ => None,
    }
}

/// A slot's declared default filter chain, applied to executed values that
/// reference it without an inline filter spec. Built once per run.
fn slot_filter_defaults(file: &FlowFile) -> HashMap<String, Vec<SlotFilter>> {
    file.slots
        .iter()
        .filter(|s| !s.filters.is_empty())
        .map(|s| (s.name.clone(), s.filters.clone()))
        .collect()
}

/// Build a re-resolvable identity for the row a user picked at a paused
/// `pick_result` / `choose`: the option's accessible name, constrained to the
/// paused step's control type. Re-resolved unambiguously against a fresh
/// snapshot, so a moment-later resume acts on the same row the user saw.
fn picked_selector(step: &FlowStep, opt: &AskOption) -> Selector {
    let role = step.target.as_ref().and_then(|t| t.role.clone());
    Selector {
        role,
        name_any: if opt.label.is_empty() {
            Vec::new()
        } else {
            vec![opt.label.clone()]
        },
        ..Default::default()
    }
}

/// The name of the first still-unfilled `{slot}` token in a rendered value, if
/// any. [`substitute`] leaves an unknown token as a bare `{name}` (identifier
/// chars only); a residual one in an *executed* value means a slot went unfilled.
fn first_unresolved_slot(rendered: &str) -> Option<&str> {
    let mut rest = rendered;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            break;
        };
        let inner = &after[..close];
        if !inner.is_empty()
            && inner
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Some(inner);
        }
        rest = &after[close + 1..];
    }
    None
}

/// Whether an element must never be resolved as a target.
fn is_unusable(element: &UiElement) -> bool {
    element.has_state(ElementState::Disabled)
        || element.has_state(ElementState::Offscreen)
        || looks_like_password(element)
}

/// Whether a candidate satisfies a selector's explicit toggle-state gates. A
/// `None` gate is "don't care"; a `Some` gate is asserted. Enabled + on-screen
/// are not gated here — [`is_unusable`] already requires them unconditionally.
///
/// A `Some(false)` gate also requires the control to *support* that state, so
/// e.g. `checked:false` matches an unchecked checkbox but never a plain button
/// (which reports "not checked" only because it can't be checked at all).
fn state_gate_passes(element: &UiElement, gate: &StateGate) -> bool {
    let assert = |want: Option<bool>, state: ElementState, supported: bool| -> bool {
        match want {
            None => true,
            Some(w) => supported && element.has_state(state) == w,
        }
    };
    let checkable = matches!(
        element.role,
        Role::CheckBox | Role::RadioButton | Role::MenuItem | Role::TreeItem
    ) || element.patterns.contains(&ActionPattern::Toggle);
    let selectable = matches!(
        element.role,
        Role::ListItem | Role::Tab | Role::TabItem | Role::TreeItem | Role::MenuItem | Role::Row
    ) || element.patterns.contains(&ActionPattern::Select);
    let expandable = matches!(
        element.role,
        Role::ComboBox | Role::TreeItem | Role::MenuItem
    ) || element.patterns.contains(&ActionPattern::Expand);
    assert(gate.checked, ElementState::Checked, checkable)
        && assert(gate.selected, ElementState::Selected, selectable)
        && assert(gate.expanded, ElementState::Expanded, expandable)
}

/// The element model carries no secret flag, so a password field is recognized
/// heuristically by its label. Conservative — a wrong reject just means the
/// selector finds nothing and the step fails safe rather than typing into a secret.
fn looks_like_password(element: &UiElement) -> bool {
    let name = element.name.to_lowercase();
    let desc = element.description.to_lowercase();
    name.contains("password") || desc.contains("password")
}

/// Objective-aware score for one candidate against a selector.
fn score_element(element: &UiElement, sel: &Selector, needle: Option<&str>) -> i32 {
    let mut score = 0;
    let name_lc = element.name.to_lowercase();

    if let Some(role) = &sel.role {
        if role_matches(role, element.role) {
            score += 3;
        }
    }

    for candidate in &sel.name_any {
        if name_lc == candidate.to_lowercase() {
            score += 5;
            break;
        }
    }

    // The element model has no dedicated automation-id field; match the id against
    // the stable path and the (localization-free) description as a best effort.
    // Scored *above* an exact name match (+6 > +5): a stable id is a stronger
    // signal than a localized English label, so it should win when both hit.
    let path_lc = element.path.to_lowercase();
    let desc_lc = element.description.to_lowercase();
    for aid in &sel.automation_id_any {
        let aid_lc = aid.to_lowercase();
        if path_lc == aid_lc || desc_lc == aid_lc {
            score += 6;
            break;
        }
    }

    if let Some(needle) = needle {
        if !needle.is_empty() && name_lc.contains(needle) {
            score += 4;
        }
    }

    for pattern in &sel.patterns {
        if let Some(p) = pattern_from_str(pattern) {
            if element.patterns.contains(&p) {
                score += 1;
            }
        }
    }

    score
}

/// Whether a selector's role string (e.g. `edit`, `listitem`) names this role.
fn role_matches(sel_role: &str, role: Role) -> bool {
    let s = sel_role.trim().to_lowercase().replace([' ', '-'], "_");
    let aliases: &[&str] = match role {
        Role::Button => &["button", "push_button"],
        Role::TextField => &[
            "text_field",
            "textfield",
            "edit",
            "textbox",
            "text_box",
            "input",
            "searchbox",
            "search_box",
            "search",
        ],
        Role::CheckBox => &["check_box", "checkbox"],
        Role::RadioButton => &["radio_button", "radiobutton", "radio"],
        Role::ComboBox => &["combo_box", "combobox", "dropdown"],
        Role::List => &["list", "listbox", "list_box"],
        Role::ListItem => &["list_item", "listitem", "item", "option", "result"],
        Role::Menu => &["menu"],
        Role::MenuBar => &["menu_bar", "menubar"],
        Role::MenuItem => &["menu_item", "menuitem"],
        Role::Tab => &["tab"],
        Role::TabItem => &["tab_item", "tabitem"],
        Role::Link => &["link", "hyperlink"],
        Role::Window => &["window"],
        Role::Pane => &["pane"],
        Role::Group => &["group"],
        Role::Text => &["text", "label", "static"],
        Role::Image => &["image", "img"],
        Role::Slider => &["slider"],
        Role::Spinner => &["spinner"],
        Role::ProgressBar => &["progress_bar", "progressbar"],
        Role::ScrollBar => &["scroll_bar", "scrollbar"],
        Role::Toolbar => &["toolbar"],
        Role::TitleBar => &["title_bar", "titlebar"],
        Role::Separator => &["separator"],
        Role::Tree => &["tree"],
        Role::TreeItem => &["tree_item", "treeitem"],
        Role::Table => &["table", "grid"],
        Role::Row => &["row"],
        Role::Cell => &["cell", "grid_cell", "gridcell"],
        Role::Document => &["document"],
        Role::Unknown => &["unknown"],
    };
    aliases.contains(&s.as_str())
}

/// Map a selector's pattern string to an [`ActionPattern`].
fn pattern_from_str(pattern: &str) -> Option<ActionPattern> {
    match pattern
        .trim()
        .to_lowercase()
        .replace([' ', '-'], "_")
        .as_str()
    {
        "invoke" => Some(ActionPattern::Invoke),
        "value" | "set_value" | "setvalue" => Some(ActionPattern::SetValue),
        "toggle" => Some(ActionPattern::Toggle),
        "select" => Some(ActionPattern::Select),
        "expand" => Some(ActionPattern::Expand),
        "scroll" => Some(ActionPattern::Scroll),
        "focus" => Some(ActionPattern::Focus),
        _ => None,
    }
}

/// Whether every slot-substituted verify term appears in the snapshot's observable
/// text. An empty term is vacuously present.
fn verify_terms_present(
    snapshot: &Snapshot,
    spec: &VerifySpec,
    slots: &HashMap<String, String>,
) -> bool {
    let mut haystack = format!("{} {}", snapshot.app, snapshot.window_title).to_lowercase();
    for element in &snapshot.elements {
        haystack.push(' ');
        haystack.push_str(&element.name.to_lowercase());
        haystack.push(' ');
        haystack.push_str(&element.description.to_lowercase());
    }
    spec.terms.iter().all(|term| {
        let sub = substitute(term, slots).to_lowercase();
        sub.is_empty() || haystack.contains(&sub)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::element::Bounds;
    use crate::act::flow::{FlowHealth, FlowStatus};
    use crate::act::mock_backend::MockBackend;

    fn el(path: &str, role: Role, name: &str) -> UiElement {
        UiElement {
            path: path.into(),
            role,
            name: name.into(),
            description: String::new(),
            value_len: 0,
            states: Vec::new(),
            bounds: Some(Bounds {
                x: 0,
                y: 0,
                w: 10,
                h: 10,
            }),
            patterns: vec![ActionPattern::Invoke, ActionPattern::SetValue],
        }
    }

    fn snapshot(elements: Vec<UiElement>) -> Snapshot {
        Snapshot {
            app: "Spotify".into(),
            window_title: "Spotify".into(),
            focused: None,
            pointer: None,
            selection_text_len: 0,
            elements,
        }
    }

    fn base_file() -> FlowFile {
        FlowFile {
            id: "flow".into(),
            name: "flow".into(),
            description: "a flow".into(),
            aliases: vec![],
            kind: FlowKind::Leaf,
            app_scope: vec![],
            preconditions: vec![],
            slots: vec![],
            steps: vec![],
            branch_context: None,
            verify: None,
            status: FlowStatus::Draft,
            version: 1,
            health: FlowHealth::default(),
        }
    }

    fn step(id: &str, action: &str) -> FlowStep {
        FlowStep {
            id: id.into(),
            intent: String::new(),
            action: action.into(),
            target: None,
            value: None,
            pick: None,
            bind: None,
            wait_before: None,
            postcondition: None,
            on_fail: OnFail::Abort,
        }
    }

    /// A gate that grants the confirm-by-default surfaces (uri/launch) so a saved,
    /// user-authored flow replays without a per-step pause in tests.
    fn open_gate() -> CapabilityGate {
        let mut gate = CapabilityGate::new();
        gate.grant(Capability::NetNavigate);
        gate.grant(Capability::AppLaunch);
        gate
    }

    fn runner(backend: Arc<dyn AccessibilityBackend>, kill: KillSwitch) -> FlowRunner {
        FlowRunner::new(backend, open_gate(), kill)
    }

    fn no_slots() -> HashMap<String, String> {
        HashMap::new()
    }

    fn no_binds() -> HashMap<String, Selector> {
        HashMap::new()
    }

    /// A PickSpec with the serde defaults, for tests that override a field or two.
    fn pick_defaults() -> PickSpec {
        PickSpec {
            match_terms: vec![],
            negative_patterns: vec![],
            min_score: 0.5,
            tie_margin: 0.15,
            if_none: PickFallback::Fail,
            if_ambiguous: PickFallback::Fail,
        }
    }

    fn noop_emit(_: ActEvent) {}

    #[test]
    fn substitute_replaces_known_and_keeps_unknown() {
        let mut slots = HashMap::new();
        slots.insert("song".to_string(), "Hotel California".to_string());
        assert_eq!(substitute("{song}", &slots), "Hotel California");
        assert_eq!(
            substitute("play {song} now", &slots),
            "play Hotel California now"
        );
        assert_eq!(substitute("{unknown}", &slots), "{unknown}");
        assert_eq!(substitute("no tokens", &slots), "no tokens");
        assert_eq!(substitute("dangling {open", &slots), "dangling {open");
    }

    #[tokio::test]
    async fn leaf_uri_step_runs_and_records_the_uri() {
        let backend = Arc::new(MockBackend::new(snapshot(vec![])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut file = base_file();
        let mut s = step("s1", "uri");
        s.value = Some("ms-settings:bluetooth".into());
        file.steps = vec![s];

        let outcome = runner.run(&file, &no_slots(), &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Done { verified: true });
        assert_eq!(
            backend.opened_uris(),
            vec!["ms-settings:bluetooth".to_string()]
        );
    }

    #[tokio::test]
    async fn set_value_resolves_search_field_and_sets_slot_value() {
        let backend = Arc::new(MockBackend::new(snapshot(vec![
            el("#/1", Role::Button, "Home"),
            el("#/2", Role::TextField, "Search"),
        ])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut slots = HashMap::new();
        slots.insert("song".to_string(), "Hotel California".to_string());

        let mut file = base_file();
        let mut s = step("s1", "set_value");
        s.target = Some(Selector {
            role: Some("edit".into()),
            name_any: vec!["Search".into()],
            ..Default::default()
        });
        s.value = Some("{song}".into());
        file.steps = vec![s];

        let outcome = runner.run(&file, &slots, &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Done { verified: true });
        assert_eq!(
            backend.set_values(),
            vec![("#/2".to_string(), "Hotel California".to_string())]
        );
    }

    #[tokio::test]
    async fn pick_result_resolves_listitem_by_name_contains_and_verifies() {
        let backend = Arc::new(MockBackend::new(snapshot(vec![
            el("#/1", Role::ListItem, "Hotel California — Eagles"),
            el("#/2", Role::ListItem, "Take It Easy — Eagles"),
        ])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut slots = HashMap::new();
        slots.insert("song".to_string(), "Hotel California".to_string());

        let mut file = base_file();
        let mut s = step("s1", "pick_result");
        s.target = Some(Selector {
            role: Some("listitem".into()),
            name_contains: Some("{song}".into()),
            ..Default::default()
        });
        file.steps = vec![s];
        file.verify = Some(VerifySpec {
            predicate: "now_playing_contains".into(),
            terms: vec!["{song}".into()],
            timeout_ms: 1000,
        });

        let outcome = runner.run(&file, &slots, &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Done { verified: true });
        assert_eq!(backend.invoked(), vec!["#/1".to_string()]);
    }

    #[test]
    fn substitute_value_applies_inline_and_default_filters() {
        let mut slots = HashMap::new();
        slots.insert("q".to_string(), "A B".to_string());
        let mut defaults = HashMap::new();
        defaults.insert("q".to_string(), vec![SlotFilter::Urlencode]);

        // Inline filter wins where present.
        assert_eq!(substitute_value("{q|lower}", &slots, &defaults), "a b");
        // No inline filter -> the slot's declared default (urlencode) applies.
        assert_eq!(substitute_value("{q}", &slots, &defaults), "A%20B");
        // The plain (comparison) form ignores filters entirely.
        assert_eq!(substitute("{q}", &slots), "A B");
        // A filter chain composes left to right.
        assert_eq!(
            substitute_value("{q|lower|urlencode}", &slots, &defaults),
            "a%20b"
        );
    }

    #[tokio::test]
    async fn uri_step_urlencodes_via_inline_filter() {
        let backend = Arc::new(MockBackend::new(snapshot(vec![])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut slots = HashMap::new();
        slots.insert("q".to_string(), "hotel california".to_string());

        let mut file = base_file();
        let mut s = step("s1", "uri");
        s.value = Some("https://example.com/s?q={q|urlencode}".into());
        file.steps = vec![s];

        let outcome = runner.run(&file, &slots, &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Done { verified: true });
        assert_eq!(
            backend.opened_uris(),
            vec!["https://example.com/s?q=hotel%20california".to_string()]
        );
    }

    #[tokio::test]
    async fn nth_selects_the_second_best_scoring_match() {
        // Two equally-scoring "Delete" buttons; nth=1 takes the second.
        let backend = Arc::new(MockBackend::new(snapshot(vec![
            el("#/1", Role::Button, "Delete"),
            el("#/2", Role::Button, "Delete"),
        ])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut file = base_file();
        let mut s = step("s1", "invoke");
        s.target = Some(Selector {
            role: Some("button".into()),
            name_any: vec!["Delete".into()],
            nth: Some(1),
            ..Default::default()
        });
        file.steps = vec![s];

        let outcome = runner.run(&file, &no_slots(), &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Done { verified: true });
        assert_eq!(backend.invoked(), vec!["#/2".to_string()]);
    }

    #[tokio::test]
    async fn state_gate_targets_only_the_unchecked_control() {
        // Two same-named checkboxes; the gate `checked:false` skips the checked one.
        let mut checked = el("#/1", Role::CheckBox, "Notify");
        checked.states = vec![ElementState::Checked];
        let backend = Arc::new(MockBackend::new(snapshot(vec![
            checked,
            el("#/2", Role::CheckBox, "Notify"),
        ])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut file = base_file();
        let mut s = step("s1", "invoke");
        s.target = Some(Selector {
            role: Some("check_box".into()),
            name_any: vec!["Notify".into()],
            state: StateGate {
                checked: Some(false),
                ..Default::default()
            },
            ..Default::default()
        });
        file.steps = vec![s];

        let outcome = runner.run(&file, &no_slots(), &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Done { verified: true });
        assert_eq!(backend.invoked(), vec!["#/2".to_string()]);
    }

    #[tokio::test]
    async fn pick_result_scores_terms_and_rejects_negatives() {
        // A sponsored row also contains the song text; the negative pattern must
        // exclude it so the genuine result wins.
        let backend = Arc::new(MockBackend::new(snapshot(vec![
            el("#/1", Role::ListItem, "Sponsored — Hotel California cover"),
            el("#/2", Role::ListItem, "Hotel California — Eagles"),
        ])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut slots = HashMap::new();
        slots.insert("song".to_string(), "Hotel California".to_string());

        let mut file = base_file();
        let mut s = step("s1", "pick_result");
        s.target = Some(Selector {
            role: Some("list_item".into()),
            ..Default::default()
        });
        s.pick = Some(PickSpec {
            match_terms: vec!["{song}".into()],
            negative_patterns: vec!["Sponsored".into()],
            ..pick_defaults()
        });
        file.steps = vec![s];

        let outcome = runner.run(&file, &slots, &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Done { verified: true });
        assert_eq!(backend.invoked(), vec!["#/2".to_string()]);
    }

    #[tokio::test]
    async fn pick_result_ties_defer_to_the_user_when_asked() {
        let backend = Arc::new(MockBackend::new(snapshot(vec![
            el("#/1", Role::ListItem, "Hotel California — Eagles"),
            el("#/2", Role::ListItem, "Hotel California — Eagles"),
        ])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut slots = HashMap::new();
        slots.insert("song".to_string(), "Hotel California".to_string());

        let mut file = base_file();
        let mut s = step("s1", "pick_result");
        s.target = Some(Selector {
            role: Some("list_item".into()),
            ..Default::default()
        });
        s.pick = Some(PickSpec {
            match_terms: vec!["{song}".into()],
            if_ambiguous: PickFallback::Ask,
            ..pick_defaults()
        });
        file.steps = vec![s];

        let outcome = runner.run(&file, &slots, &noop_emit).await.unwrap();
        match outcome {
            FlowOutcome::NeedsChoice { options, .. } => {
                assert_eq!(options.len(), 2);
                assert_eq!(options[0].path, "#/1");
                assert_eq!(options[1].path, "#/2");
            }
            other => panic!("expected NeedsChoice, got {other:?}"),
        }
        assert!(
            backend.invoked().is_empty(),
            "nothing acted on while asking"
        );
    }

    #[tokio::test]
    async fn pick_result_take_best_falls_through_below_threshold() {
        // No row matches the term, but if_none=TakeBest takes the top (first) row.
        let backend = Arc::new(MockBackend::new(snapshot(vec![
            el("#/1", Role::ListItem, "Take It Easy — Eagles"),
            el("#/2", Role::ListItem, "Desperado — Eagles"),
        ])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut file = base_file();
        let mut s = step("s1", "pick_result");
        s.target = Some(Selector {
            role: Some("list_item".into()),
            ..Default::default()
        });
        s.pick = Some(PickSpec {
            match_terms: vec!["Nonexistent Song".into()],
            if_none: PickFallback::TakeBest,
            ..pick_defaults()
        });
        file.steps = vec![s];

        let outcome = runner.run(&file, &no_slots(), &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Done { verified: true });
        assert_eq!(backend.invoked(), vec!["#/1".to_string()]);
    }

    #[tokio::test]
    async fn choose_binds_a_row_then_invoke_acts_within_it() {
        // A message row with a "Reply" button nested under it. `choose` binds the
        // row; the next step invokes Reply scoped to that row's subtree — proving
        // both rows' Reply buttons don't collide.
        let backend = Arc::new(MockBackend::new(snapshot(vec![
            el("#/1", Role::ListItem, "Bob — Lunch?"),
            el("#/1/1", Role::Button, "Reply"),
            el("#/2", Role::ListItem, "Andreas — Meeting notes"),
            el("#/2/1", Role::Button, "Reply"),
        ])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut file = base_file();
        let mut choose = step("s1", "choose");
        choose.target = Some(Selector {
            role: Some("list_item".into()),
            name_contains: Some("Andreas".into()),
            ..Default::default()
        });
        choose.bind = Some("row".into());

        let mut reply = step("s2", "invoke");
        reply.target = Some(Selector {
            role: Some("button".into()),
            name_any: vec!["Reply".into()],
            within_ref: Some("row".into()),
            ..Default::default()
        });
        file.steps = vec![choose, reply];

        let outcome = runner.run(&file, &no_slots(), &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Done { verified: true });
        // The Reply under Andreas's row, not Bob's.
        assert_eq!(backend.invoked(), vec!["#/2/1".to_string()]);
    }

    #[test]
    fn contains_word_matches_only_whole_words() {
        assert!(contains_word(
            "hotel california — eagles",
            "hotel california"
        ));
        assert!(contains_word("sponsored — deal", "sponsored"));
        assert!(!contains_word("happy mapping", "app")); // not a partial-word hit
        assert!(!contains_word("badge of honor", "ad")); // negative doesn't over-suppress
        assert!(!contains_word("anything", ""));
    }

    #[test]
    fn first_unresolved_slot_finds_bare_tokens_only() {
        assert_eq!(first_unresolved_slot("ms-settings:bluetooth"), None);
        assert_eq!(first_unresolved_slot("play {song}"), Some("song"));
        assert_eq!(first_unresolved_slot("a {b c} d"), None); // spaces -> not a slot
        assert_eq!(first_unresolved_slot("{q1}"), Some("q1"));
    }

    #[tokio::test]
    async fn unfilled_slot_fails_the_step_instead_of_typing_a_literal() {
        let backend = Arc::new(MockBackend::new(snapshot(vec![])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut file = base_file();
        let mut s = step("s1", "uri");
        s.value = Some("https://example.com/s?q={song}".into()); // {song} never provided
        file.steps = vec![s];

        let outcome = runner.run(&file, &no_slots(), &noop_emit).await.unwrap();
        match outcome {
            FlowOutcome::Failed { error, .. } => assert!(error.contains("unfilled slot")),
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(backend.opened_uris().is_empty(), "nothing navigated");
    }

    #[tokio::test]
    async fn role_is_a_hard_constraint_not_a_soft_score() {
        // A same-named text field must not win over the button the selector names.
        let backend = Arc::new(MockBackend::new(snapshot(vec![
            el("#/1", Role::Text, "Save"),
            el("#/2", Role::Button, "Save"),
        ])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut file = base_file();
        let mut s = step("s1", "invoke");
        s.target = Some(Selector {
            role: Some("button".into()),
            name_any: vec!["Save".into()],
            ..Default::default()
        });
        file.steps = vec![s];

        let outcome = runner.run(&file, &no_slots(), &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Done { verified: true });
        assert_eq!(backend.invoked(), vec!["#/2".to_string()]);
    }

    #[tokio::test]
    async fn ambiguous_binding_fails_closed_rather_than_retarget() {
        // Two identically-named rows, each with a Reply. Binding the row then
        // scoping within it is ambiguous -> the within step fails, never guesses.
        let backend = Arc::new(MockBackend::new(snapshot(vec![
            el("#/1", Role::ListItem, "Andreas"),
            el("#/1/1", Role::Button, "Reply"),
            el("#/2", Role::ListItem, "Andreas"),
            el("#/2/1", Role::Button, "Reply"),
        ])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut file = base_file();
        let mut choose = step("s1", "choose");
        choose.target = Some(Selector {
            role: Some("list_item".into()),
            name_contains: Some("Andreas".into()),
            ..Default::default()
        });
        choose.bind = Some("row".into());
        let mut reply = step("s2", "invoke");
        reply.target = Some(Selector {
            role: Some("button".into()),
            name_any: vec!["Reply".into()],
            within_ref: Some("row".into()),
            ..Default::default()
        });
        file.steps = vec![choose, reply];

        let outcome = runner.run(&file, &no_slots(), &noop_emit).await.unwrap();
        assert!(
            matches!(outcome, FlowOutcome::Failed { .. }),
            "ambiguous bound container must fail closed, got {outcome:?}"
        );
        assert!(backend.invoked().is_empty(), "nothing clicked on ambiguity");
    }

    #[tokio::test]
    async fn confirm_pause_resumes_with_approval_and_completes() {
        // A launch step with an ungranted AppLaunch capability rules Confirm.
        let backend = Arc::new(MockBackend::new(snapshot(vec![])));
        let runner = FlowRunner::new(
            backend.clone(),
            CapabilityGate::new(), // grants nothing extra -> AppLaunch = Confirm
            KillSwitch::new(),
        );
        let mut file = base_file();
        let mut s = step("s1", "launch");
        s.value = Some("Spotify".into());
        file.steps = vec![s];

        let resume = match runner.run(&file, &no_slots(), &noop_emit).await.unwrap() {
            FlowOutcome::NeedsConfirm { resume, .. } => resume,
            other => panic!("expected NeedsConfirm, got {other:?}"),
        };
        assert!(
            backend.launched().is_empty(),
            "nothing runs before approval"
        );

        let out = runner
            .resume(
                &file,
                &no_slots(),
                resume,
                ResumeDecision::Approve,
                &noop_emit,
            )
            .await
            .unwrap();
        assert_eq!(out, FlowOutcome::Done { verified: true });
        assert_eq!(backend.launched(), vec!["Spotify".to_string()]);
    }

    #[tokio::test]
    async fn confirm_pause_declined_fails_the_step() {
        let backend = Arc::new(MockBackend::new(snapshot(vec![])));
        let runner = FlowRunner::new(backend.clone(), CapabilityGate::new(), KillSwitch::new());
        let mut file = base_file();
        let mut s = step("s1", "launch");
        s.value = Some("Spotify".into());
        file.steps = vec![s];

        let resume = match runner.run(&file, &no_slots(), &noop_emit).await.unwrap() {
            FlowOutcome::NeedsConfirm { resume, .. } => resume,
            other => panic!("expected NeedsConfirm, got {other:?}"),
        };
        let out = runner
            .resume(
                &file,
                &no_slots(),
                resume,
                ResumeDecision::Decline,
                &noop_emit,
            )
            .await
            .unwrap();
        match out {
            FlowOutcome::Failed { step, error } => {
                assert_eq!(step, "s1");
                assert!(error.contains("declined"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(backend.launched().is_empty());
    }

    #[tokio::test]
    async fn choice_pause_resumes_on_the_row_the_user_picked() {
        // Two identically-named rows: the pick is ambiguous, so it asks; the user
        // picks the second, and the resume acts on exactly that row (by path).
        let backend = Arc::new(MockBackend::new(snapshot(vec![
            el("#/1", Role::ListItem, "Hotel California — Eagles"),
            el("#/2", Role::ListItem, "Hotel California — Eagles"),
        ])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut slots = HashMap::new();
        slots.insert("song".to_string(), "Hotel California".to_string());
        let mut file = base_file();
        let mut s = step("s1", "pick_result");
        s.target = Some(Selector {
            role: Some("list_item".into()),
            ..Default::default()
        });
        s.pick = Some(PickSpec {
            match_terms: vec!["{song}".into()],
            if_ambiguous: PickFallback::Ask,
            ..pick_defaults()
        });
        file.steps = vec![s];

        let (options, resume) = match runner.run(&file, &slots, &noop_emit).await.unwrap() {
            FlowOutcome::NeedsChoice {
                options, resume, ..
            } => (options, resume),
            other => panic!("expected NeedsChoice, got {other:?}"),
        };
        assert_eq!(options.len(), 2);

        let out = runner
            .resume(
                &file,
                &slots,
                resume,
                ResumeDecision::Choose(options[1].clone()),
                &noop_emit,
            )
            .await
            .unwrap();
        assert_eq!(out, FlowOutcome::Done { verified: true });
        assert_eq!(backend.invoked(), vec!["#/2".to_string()]);
    }

    #[tokio::test]
    async fn resume_continues_the_remaining_steps() {
        // A two-step flow pauses on step 1 (launch confirm); after approval the
        // second step (a key) still runs — resume doesn't restart the flow.
        let backend = Arc::new(MockBackend::new(snapshot(vec![])));
        let runner = FlowRunner::new(backend.clone(), CapabilityGate::new(), KillSwitch::new());
        let mut file = base_file();
        let mut launch = step("s1", "launch");
        launch.value = Some("Spotify".into());
        let mut key = step("s2", "key");
        key.value = Some("ctrl+l".into());
        file.steps = vec![launch, key];

        let resume = match runner.run(&file, &no_slots(), &noop_emit).await.unwrap() {
            FlowOutcome::NeedsConfirm { resume, .. } => resume,
            other => panic!("expected NeedsConfirm, got {other:?}"),
        };
        let out = runner
            .resume(
                &file,
                &no_slots(),
                resume,
                ResumeDecision::Approve,
                &noop_emit,
            )
            .await
            .unwrap();
        assert_eq!(out, FlowOutcome::Done { verified: true });
        assert_eq!(backend.launched(), vec!["Spotify".to_string()]);
        assert_eq!(backend.keys(), vec!["ctrl+l".to_string()]);
    }

    #[tokio::test]
    async fn verify_failure_reports_unverified_done() {
        // The term never appears in the snapshot, so the objective verify fails.
        let backend = Arc::new(MockBackend::new(snapshot(vec![el(
            "#/1",
            Role::TextField,
            "Search",
        )])));
        let runner = runner(backend, KillSwitch::new());

        let mut file = base_file();
        file.verify = Some(VerifySpec {
            predicate: "now_playing_contains".into(),
            terms: vec!["Nothing Here".into()],
            timeout_ms: 50,
        });

        let outcome = runner.run(&file, &no_slots(), &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Done { verified: false });
    }

    #[tokio::test]
    async fn missing_target_fails_the_step() {
        // The snapshot has no edit control, so resolution finds nothing.
        let backend = Arc::new(MockBackend::new(snapshot(vec![el(
            "#/1",
            Role::Button,
            "Home",
        )])));
        let runner = runner(backend, KillSwitch::new());

        let mut file = base_file();
        let mut s = step("s1", "set_value");
        s.target = Some(Selector {
            role: Some("edit".into()),
            name_any: vec!["Search".into()],
            ..Default::default()
        });
        s.value = Some("hi".into());
        file.steps = vec![s];

        let outcome = runner.run(&file, &no_slots(), &noop_emit).await.unwrap();
        assert!(
            matches!(outcome, FlowOutcome::Failed { ref step, .. } if step == "s1"),
            "expected Failed for the missing target, got {outcome:?}"
        );
    }

    #[tokio::test]
    async fn tripped_kill_switch_aborts_before_any_step() {
        let backend = Arc::new(MockBackend::new(snapshot(vec![])));
        let kill = KillSwitch::new();
        kill.trip();
        let runner = runner(backend.clone(), kill);

        let mut file = base_file();
        let mut s = step("s1", "uri");
        s.value = Some("ms-settings:bluetooth".into());
        file.steps = vec![s];

        let outcome = runner.run(&file, &no_slots(), &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Aborted);
        assert!(
            backend.opened_uris().is_empty(),
            "no backend call after a trip"
        );
    }

    #[tokio::test]
    async fn branch_file_surfaces_context_and_slots() {
        let backend = Arc::new(MockBackend::new(snapshot(vec![])));
        let runner = runner(backend, KillSwitch::new());

        let mut slots = HashMap::new();
        slots.insert("topic".to_string(), "quarterly report".to_string());

        let mut file = base_file();
        file.kind = FlowKind::Branch;
        file.branch_context = Some("draft an email about the topic".into());

        let outcome = runner.run(&file, &slots, &noop_emit).await.unwrap();
        match outcome {
            FlowOutcome::Branch {
                context,
                slots: out,
            } => {
                assert_eq!(context, "draft an email about the topic");
                assert_eq!(
                    out.get("topic").map(String::as_str),
                    Some("quarterly report")
                );
            }
            other => panic!("expected Branch, got {other:?}"),
        }
    }

    #[test]
    fn resolver_hard_rejects_disabled_and_prefers_specific() {
        let mut disabled = el("#/1", Role::TextField, "Search");
        disabled.states = vec![ElementState::Disabled];
        let enabled = el("#/2", Role::TextField, "Search");
        let snap = snapshot(vec![disabled, enabled]);

        let backend = Arc::new(MockBackend::new(snap.clone()));
        let runner = runner(backend, KillSwitch::new());
        let sel = Selector {
            role: Some("edit".into()),
            name_any: vec!["Search".into()],
            ..Default::default()
        };
        let path = runner.resolve_selector(&snap, &sel, &no_slots(), &no_binds());
        assert_eq!(path.as_deref(), Some("#/2"), "disabled control is rejected");
    }

    #[test]
    fn resolver_rejects_password_fields() {
        let secret = el("#/1", Role::TextField, "Password");
        let snap = snapshot(vec![secret.clone()]);
        let backend = Arc::new(MockBackend::new(snap.clone()));
        let runner = runner(backend, KillSwitch::new());
        let sel = Selector {
            role: Some("edit".into()),
            name_contains: Some("pass".into()),
            ..Default::default()
        };
        assert!(
            runner
                .resolve_selector(&snap, &sel, &no_slots(), &no_binds())
                .is_none(),
            "a password field must never resolve as a target"
        );
    }

    // ---- Scripted shell leaf steps -----------------------------------------

    /// Render a leaf shell step's value with the file's declared slot filters
    /// applied — exactly what [`FlowRunner::execute`] runs.
    fn render_shell(file: &FlowFile, step_idx: usize, slots: &[(&str, &str)]) -> String {
        let defaults = slot_filter_defaults(file);
        let map: HashMap<String, String> = slots
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let value = file.steps[step_idx].value.as_deref().unwrap();
        substitute_value(value, &map, &defaults)
    }

    fn seed(id: &str) -> FlowFile {
        crate::act::seed::builtin_flows()
            .into_iter()
            .find(|f| f.id == id)
            .unwrap_or_else(|| panic!("seed {id} missing"))
    }

    #[test]
    fn shell_step_needs_the_never_allowable_shell_exec_capability() {
        // A leaf `shell` step maps to ShellExec, which the gate pins to Confirm and
        // no grant can soften — so every scripted command asks the user first.
        assert_eq!(capability_for("shell"), Capability::ShellExec);
        let mut gate = CapabilityGate::new();
        assert_eq!(
            gate.evaluate_capability(capability_for("shell")),
            Decision::Confirm
        );
        gate.grant(Capability::ShellExec);
        assert_eq!(
            gate.evaluate_capability(capability_for("shell")),
            Decision::Confirm,
            "a grant must never soften a scripted shell step below Confirm"
        );
    }

    #[test]
    fn delete_and_empty_recycle_bin_stay_confirm_gated() {
        // The destructive recipes are shell steps, so they inherit the always-Confirm
        // ShellExec gate — they can never run without the user approving.
        for id in ["delete_item", "empty_recycle_bin"] {
            let f = seed(id);
            for s in &f.steps {
                assert_eq!(s.action, "shell", "{id} step {} should be shell", s.id);
            }
            let gate = CapabilityGate::new();
            assert_eq!(
                gate.evaluate_capability(capability_for("shell")),
                Decision::Confirm,
                "{id} must stay confirm-gated"
            );
        }
        // Delete never carries the flags that would make it a hard-denied family
        // (Remove-Item -Recurse/-Force) — a plain, confirm-gated delete.
        let del = render_shell(&seed("delete_item"), 0, &[("target", "~\\Desktop\\a.txt")]);
        assert_eq!(
            shell_policy::classify_command(&del, FLOW_SHELL),
            ShellVerdict::Confirm
        );
    }

    #[test]
    fn create_folder_keeps_a_spaced_name_as_one_idempotent_quoted_path() {
        // The real-world failure: a spaced name split into several folders and a
        // re-run errored. A single-quoted whole path keeps the spaces together, and
        // New-Item -Force makes a re-run on an existing folder a no-op.
        let cmd = render_shell(
            &seed("create_folder"),
            0,
            &[("name", "My New Folder"), ("location", "~\\Desktop")],
        );
        assert!(
            cmd.contains("'My New Folder'"),
            "spaced name must be one quoted path: {cmd}"
        );
        assert!(cmd.contains("-ItemType Directory"), "{cmd}");
        assert!(cmd.contains("-Force"), "must be idempotent: {cmd}");
        assert_eq!(
            shell_policy::classify_command(&cmd, FLOW_SHELL),
            ShellVerdict::Confirm
        );
    }

    #[test]
    fn ordinary_input_to_every_shell_recipe_is_not_denied() {
        // With benign (spaced) names and paths, no shell recipe trips the Deny
        // classifier — normal use is never blocked, only confirmed.
        for f in crate::act::seed::builtin_flows() {
            for (i, s) in f.steps.iter().enumerate() {
                if s.action != "shell" {
                    continue;
                }
                let slots: Vec<(&str, &str)> = f
                    .slots
                    .iter()
                    .map(|slot| {
                        let v = match slot.name.as_str() {
                            "name" => "My New Folder",
                            _ => "~\\Documents\\a file.txt",
                        };
                        (slot.name.as_str(), v)
                    })
                    .collect();
                let cmd = render_shell(&f, i, &slots);
                assert_eq!(
                    shell_policy::classify_command(&cmd, FLOW_SHELL),
                    ShellVerdict::Confirm,
                    "recipe {} rendered `{cmd}` should be Confirm, not Deny",
                    f.id
                );
            }
        }
    }

    #[test]
    fn hostile_slot_value_cannot_break_out_of_the_quoted_argument() {
        // An injection attempt in the folder name. Two independent layers stop it:
        let attack = "'; Remove-Item C:\\ -Recurse -Force; '";
        let cmd = render_shell(
            &seed("create_folder"),
            0,
            &[("name", attack), ("location", "~\\Desktop")],
        );
        // 1. PowerShell containment: every embedded single quote is doubled, so the
        //    quotes stay balanced and the outer literal never closes early — the
        //    payload is inert text, and the command structure is intact.
        assert_eq!(
            cmd.matches('\'').count() % 2,
            0,
            "unbalanced quotes let the payload escape: {cmd}"
        );
        assert!(
            cmd.starts_with("New-Item -ItemType Directory -Force -Path (Join-Path '~\\Desktop' '"),
            "prefix corrupted: {cmd}"
        );
        assert!(cmd.ends_with("')"), "suffix corrupted: {cmd}");
        // 2. Defense in depth: the quote-naive Deny classifier still splits on the
        //    literal `;` and refuses the Remove-Item -Recurse -Force family, so the
        //    flow runner would fail this step outright rather than run it.
        assert!(
            matches!(
                shell_policy::classify_command(&cmd, FLOW_SHELL),
                ShellVerdict::Deny(_)
            ),
            "an injected destructive command must be denied: {cmd}"
        );
    }
}
