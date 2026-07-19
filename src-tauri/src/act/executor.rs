//! The executor: runs a validated [`ActionPlan`] over an [`AccessibilityBackend`].
//!
//! Per action: capability gate -> kill-switch check -> resolve the target against
//! a FRESH snapshot (re-ground on a stale path) -> execute (prefer a11y invoke)
//! -> verify -> audit. Confirm and ask_user pause the plan and are resumed via
//! [`Executor::resume_after_user`].

use std::sync::Arc;

use super::action::{Action, ActionPlan, ClipboardOp, Origin};
use super::audit::{AuditEntry, AuditLog};
use super::backend::AccessibilityBackend;
use super::capability::{CapabilityGate, Decision};
use super::destructive::{self, Destructive};
use super::element::{ElementPath, ElementState, Snapshot};
use super::events::AskOption;
use super::focus_guard;
use super::grounding::{self, Grounded};
use super::killswitch::KillSwitch;
use super::shell_policy::{self, ShellVerdict};

/// A destructive target the user has confirmed, pinned to the exact control they
/// approved: `(resolved path, resolved name)`. A pre-approval is honored only when
/// the re-resolved target still matches this fingerprint.
type ConfirmedTarget = (ElementPath, String);

/// The result of one action within a plan.
#[derive(Debug)]
pub enum StepOutcome {
    Done {
        action: Action,
        verified: bool,
    },
    NeedsConfirm {
        action: Action,
        reason: String,
    },
    NeedsAskUser {
        prompt: String,
        options: Vec<AskOption>,
    },
    Denied {
        action: Action,
        reason: String,
    },
    Failed {
        action: Action,
        error: String,
    },
    Aborted,
}

/// The result of executing (part of) a plan.
#[derive(Debug)]
pub struct ExecResult {
    pub outcomes: Vec<StepOutcome>,
    /// True only if every step ran to completion (no confirm/ask/deny/fail/abort).
    pub completed: bool,
}

/// The user's answer to a Confirm or ask_user pause.
#[derive(Debug, Clone)]
pub enum UserDecision {
    ConfirmAllow,
    ConfirmDeny,
    AskUserPick { index: usize },
    Cancel,
}

/// A hard executor failure (not a per-step outcome).
#[derive(Debug)]
pub enum ExecError {
    Backend(String),
    Cancelled,
    Internal(String),
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecError::Backend(m) => write!(f, "executor backend error: {m}"),
            ExecError::Cancelled => write!(f, "executor cancelled"),
            ExecError::Internal(m) => write!(f, "executor internal error: {m}"),
        }
    }
}
impl std::error::Error for ExecError {}

/// Where a single step handoff can land within the run loop.
enum Flow {
    /// The action ran; record the outcome and continue to the next action.
    Continue(StepOutcome),
    /// The plan must halt here (deny / fail); record the outcome and return with
    /// `completed=false`.
    Halt(StepOutcome),
    /// The plan paused for the user (confirm / ask_user); return immediately with
    /// this outcome and `completed=false`.
    Pause(StepOutcome),
    /// The kill switch tripped mid-execution; abort the whole plan.
    Aborted,
    /// A clean `Stop` action: end the plan without recording an outcome.
    Stop,
}

/// Runs plans over a backend, enforcing the capability gate and kill switch and
/// writing the audit log.
pub struct Executor {
    backend: Arc<dyn AccessibilityBackend>,
    gate: CapabilityGate,
    audit: Option<AuditLog>,
    kill: KillSwitch,
    /// The transcript that produced the current plan, threaded in via
    /// [`Executor::execute_plan_with_context`]. Used only as a hint for the
    /// destructive classifier's confirm-activator branch; never sent anywhere.
    transcript: String,
    /// The destructive target the user most recently confirmed (set when a step
    /// pauses on the destructive classifier, consumed on a matching resume). Binds
    /// the one-shot pre-approval to that exact control so a swapped-out control
    /// re-confirms instead of executing.
    confirmed_target: Option<ConfirmedTarget>,
    /// The app this command deliberately launched or switched to, if any. Set
    /// when a `Launch` / `FocusApp` action succeeds; consulted by the focus guard
    /// before any keystroke so `Type` / `Key` can never land in the window that
    /// was foreground *before* the switch (the "typed into the terminal" bug).
    /// Reset per command; never derived from the live foreground.
    expected_app: Option<String>,
    /// Fingerprint of the snapshot seen by the previous a11y step, used only to
    /// log whether the screen changed between steps (a "step-by-step trace"
    /// signal). Reset per command; never affects control flow.
    last_snapshot_fp: Option<String>,
}

impl Executor {
    pub fn new(
        backend: Arc<dyn AccessibilityBackend>,
        gate: CapabilityGate,
        audit: Option<AuditLog>,
        kill: KillSwitch,
    ) -> Self {
        Self {
            backend,
            gate,
            audit,
            kill,
            transcript: String::new(),
            confirmed_target: None,
            expected_app: None,
            last_snapshot_fp: None,
        }
    }

    pub fn kill_switch(&self) -> KillSwitch {
        self.kill.clone()
    }

    /// Execute a plan step by step. Returns early (completed=false) on the first
    /// Confirm / ask_user / Deny / Failed / Abort.
    ///
    /// This convenience form runs with no transcript hint, so the destructive
    /// classifier's confirm-activator branch (which needs the spoken intent) is
    /// inert; the destructive-target branch still applies. Callers that have the
    /// transcript should prefer [`Executor::execute_plan_with_context`].
    pub async fn execute_plan(&mut self, plan: ActionPlan) -> Result<ExecResult, ExecError> {
        self.execute_plan_with_context(plan, "").await
    }

    /// Execute a plan with the spoken `transcript` available to the destructive
    /// classifier. The public [`Executor::execute_plan`] signature is preserved
    /// (it delegates here with an empty transcript) so existing callers keep
    /// compiling; the session should call this variant with the real transcript so
    /// a bare "Yes"/"OK" confirm-activator can be caught when the intent that led
    /// there was destructive.
    pub async fn execute_plan_with_context(
        &mut self,
        plan: ActionPlan,
        transcript: &str,
    ) -> Result<ExecResult, ExecError> {
        // Single-plan entry point: a fresh command has launched/switched nothing
        // yet, so the focus guard starts inert and only arms once this command's
        // own launch/focus runs. (The closed loop uses `execute_plan_continuing`
        // instead, so a launch in one iteration keeps arming a Type in the next.)
        self.expected_app = None;
        self.execute_plan_continuing(plan, transcript).await
    }

    /// Execute one batch WITHOUT clearing the focus-guard latch (`expected_app`).
    ///
    /// The novel closed loop launches/focuses in one iteration and types on a
    /// LATER one, calling the executor once per iteration. If each call reset the
    /// guard, a launch's `expected_app` would be cleared before the follow-up
    /// `Type` ever consulted it, leaving the guard inert for exactly the
    /// launch-then-type pattern the loop uses. So the loop calls this variant and
    /// arms/clears the guard itself once per goal via [`Executor::reset_focus_guard`].
    ///
    /// The one-shot destructive pre-approval and the trace fingerprint are still
    /// cleared per batch — a freshly planned batch must never inherit a stale
    /// approval, and the fingerprint is logging-only.
    pub async fn execute_plan_continuing(
        &mut self,
        plan: ActionPlan,
        transcript: &str,
    ) -> Result<ExecResult, ExecError> {
        self.transcript = transcript.to_string();
        // A fresh batch clears any pre-approval left over from a prior one.
        self.confirmed_target = None;
        // A freshly planned batch's screen fingerprint is judged on its own.
        self.last_snapshot_fp = None;
        self.run(plan.actions, false).await
    }

    /// Clear the focus-guard latch so it starts inert for a NEW goal. The novel
    /// closed loop calls this once at goal start (not per iteration), so a
    /// launch/focus in one iteration arms the guard for a `Type` in a later one,
    /// while a brand-new goal never inherits the previous goal's expected app.
    pub fn reset_focus_guard(&mut self) {
        self.expected_app = None;
    }

    /// Resume the remaining plan after a Confirm / ask_user decision.
    ///
    /// - `ConfirmAllow` re-runs `remaining` with its first action pre-approved
    ///   (its gate `Confirm` is treated as `Allow` this once).
    /// - `ConfirmDeny` / `Cancel` refuse without touching the backend.
    /// - `AskUserPick` resolves the chosen path and runs it (invoke/focus), then
    ///   continues the rest of the plan.
    pub async fn resume_after_user(
        &mut self,
        remaining: ActionPlan,
        decision: UserDecision,
    ) -> Result<ExecResult, ExecError> {
        match decision {
            UserDecision::ConfirmAllow => self.run(remaining.actions, true).await,
            UserDecision::ConfirmDeny => {
                let action = remaining.actions.into_iter().next().unwrap_or(Action::Stop);
                Ok(ExecResult {
                    outcomes: vec![StepOutcome::Denied {
                        action,
                        reason: "user declined confirmation".into(),
                    }],
                    completed: false,
                })
            }
            UserDecision::Cancel => Ok(ExecResult {
                outcomes: vec![StepOutcome::Aborted],
                completed: false,
            }),
            UserDecision::AskUserPick { index } => {
                self.resume_with_pick(remaining.actions, index).await
            }
        }
    }

    /// The core run loop shared by `execute_plan` and the resume paths.
    ///
    /// When `first_preapproved` is set the first action's gate `Confirm` is
    /// softened to `Allow` exactly once (the user already consented).
    async fn run(
        &mut self,
        actions: Vec<Action>,
        first_preapproved: bool,
    ) -> Result<ExecResult, ExecError> {
        let mut outcomes: Vec<StepOutcome> = Vec::new();

        for (idx, action) in actions.into_iter().enumerate() {
            let preapproved = first_preapproved && idx == 0;

            // 1. Kill switch — abort before doing anything for this action.
            if self.kill.is_tripped() {
                outcomes.push(StepOutcome::Aborted);
                return Ok(ExecResult {
                    outcomes,
                    completed: false,
                });
            }

            // Capture the action's identity before it is moved into `step`, so the
            // per-step trace line can name it even after it re-grounds inside.
            let action_kind = action.kind();
            let action_target = action.target().map(|t| t.to_string());
            let flow = self.step(action, preapproved).await?;
            let (outcome_label, detail) = describe_flow(&flow);
            tracing::info!(
                step = idx,
                action = action_kind,
                target = action_target.as_deref().unwrap_or(""),
                outcome = outcome_label,
                detail = %detail,
                "act executor step"
            );
            match flow {
                Flow::Continue(outcome) => outcomes.push(outcome),
                Flow::Halt(outcome) | Flow::Pause(outcome) => {
                    outcomes.push(outcome);
                    return Ok(ExecResult {
                        outcomes,
                        completed: false,
                    });
                }
                Flow::Aborted => {
                    outcomes.push(StepOutcome::Aborted);
                    return Ok(ExecResult {
                        outcomes,
                        completed: false,
                    });
                }
                Flow::Stop => {
                    // A clean Stop ends the plan; everything up to here ran.
                    return Ok(ExecResult {
                        outcomes,
                        completed: true,
                    });
                }
            }
        }

        // Fell off the end with no early return: every step is a Done.
        let completed = outcomes
            .iter()
            .all(|o| matches!(o, StepOutcome::Done { .. }));
        Ok(ExecResult {
            outcomes,
            completed,
        })
    }

    /// Run one action end-to-end: gate -> snapshot -> re-ground -> elevation ->
    /// execute -> verify -> audit.
    async fn step(&mut self, action: Action, preapproved: bool) -> Result<Flow, ExecError> {
        // Script primitives (launch / uri / shell / wait / focus_app / clipboard)
        // are not accessibility-tree targets, so they take a separate path:
        // injection guards -> capability gate -> execute -> verify. No element
        // resolution, no destructive-control classifier (that's for a11y invokes).
        if is_script_primitive(&action) {
            return self.step_script(action, preapproved).await;
        }

        // 2. Capability gate. A one-time pre-approval softens Confirm to Allow.
        let decision = match (self.gate.evaluate(&action), preapproved) {
            (Decision::Confirm, true) => Decision::Allow,
            (d, _) => d,
        };
        match decision {
            Decision::Deny => {
                let reason = format!("capability denied: {}", action.kind());
                self.audit_step(&action, decision, "blocked: capability_denied");
                return Ok(Flow::Halt(StepOutcome::Denied { action, reason }));
            }
            Decision::Confirm => {
                let reason = format!("{} requires confirmation", action.kind());
                self.audit_step(&action, decision, "paused: needs_confirm");
                return Ok(Flow::Pause(StepOutcome::NeedsConfirm { action, reason }));
            }
            Decision::Allow => {}
        }

        // 3. Fresh snapshot.
        let snapshot = self
            .backend
            .snapshot()
            .await
            .map_err(|e| ExecError::Backend(e.to_string()))?;

        // Log whether the screen changed since the previous a11y step — a plain
        // "did anything move" signal that makes the trace readable.
        let snapshot_changed = self.note_snapshot_change(&snapshot);

        // 4. Resolve the target against the live snapshot; re-ground if stale.
        let action = match self.resolve_target(&action, &snapshot) {
            TargetResolution::Ready(a) => {
                let resolved_name = a
                    .target()
                    .and_then(|p| snapshot.get(p))
                    .map(|e| e.name.as_str())
                    .unwrap_or("");
                tracing::info!(
                    action = a.kind(),
                    target = a.target().unwrap_or(""),
                    resolved_name,
                    snapshot_changed,
                    "act executor step: target resolved"
                );
                a
            }
            TargetResolution::Ambiguous(options) => {
                let prompt = format!("Which one? ({})", action.kind());
                tracing::info!(
                    action = action.kind(),
                    target = action.target().unwrap_or(""),
                    candidates = options.len(),
                    "act executor step: target ambiguous (asking user)"
                );
                self.audit_step(&action, decision, "paused: ask_user");
                return Ok(Flow::Pause(StepOutcome::NeedsAskUser { prompt, options }));
            }
            TargetResolution::Gone => {
                tracing::info!(
                    action = action.kind(),
                    target = action.target().unwrap_or(""),
                    "act executor step: target gone (could not re-ground)"
                );
                self.audit_step(&action, decision, "failed: target_absent");
                return Ok(Flow::Halt(StepOutcome::Failed {
                    action,
                    error: "target no longer present".into(),
                }));
            }
        };

        // 5. Refuse to drive a higher-integrity (elevated) foreground app.
        if self
            .backend
            .focused_app_is_elevated()
            .await
            .map_err(|e| ExecError::Backend(e.to_string()))?
        {
            self.audit_step(&action, decision, "blocked: elevated");
            return Ok(Flow::Halt(StepOutcome::Denied {
                action,
                reason: "target app is elevated".into(),
            }));
        }

        // 5b. Runtime destructive classifier (defense in depth beyond the gate and
        // the planner). Runs after the target name is known so it sees the real
        // control about to be activated — including the FOCUSED control for a bare
        // Enter/Space press with no explicit target (fast-path "submit").
        let (name, desc) = self.resolved_identity(&action, &snapshot);
        if let Destructive::Confirm(reason) =
            destructive::classify(&action, &name, &desc, &self.transcript)
        {
            // A one-shot pre-approval is honored ONLY when the re-resolved target
            // still matches the exact (path, name) the user confirmed. Anything
            // else — a swapped control, a re-grounded path, a renamed button —
            // re-confirms rather than driving the wrong control.
            let fingerprint = self.target_fingerprint(&action, &snapshot);
            let honor =
                preapproved && fingerprint.is_some() && self.confirmed_target == fingerprint;
            if honor {
                self.confirmed_target = None; // consume the one-shot approval
            } else {
                self.confirmed_target = fingerprint;
                self.audit_step(&action, decision, "paused: needs_confirm_destructive");
                return Ok(Flow::Pause(StepOutcome::NeedsConfirm { action, reason }));
            }
        }

        // 5c. Focus guard. `Type` and `Key` go to whatever window is foreground,
        // so if this command launched/switched to an app, make sure that app is
        // actually in front before sending input — focus it or ABORT rather than
        // typing into the window that was foreground before the switch. Inert when
        // the command never moved apps (expected_app is None), so a plain
        // "copy that" against the current window is unaffected.
        if matches!(action, Action::Type { .. } | Action::Key { .. }) {
            if let Some(expected) = self.expected_app.clone() {
                if let Err(reason) =
                    focus_guard::ensure_target_focused(&self.backend, &self.kill, &expected).await
                {
                    self.audit_step(&action, decision, "blocked: focus_guard");
                    return Ok(Flow::Halt(StepOutcome::Failed {
                        action,
                        error: reason,
                    }));
                }
            }
        }

        // 5d. Reach below-the-fold targets. A control marked `offscreen` is
        // scrolled out of the viewport, so a coordinate click at its bounds hits
        // nothing (the "clicks empty space, no progress, aborts" failure on a
        // YouTube result). An a11y invoke-by-path still works, but bringing the
        // element into view first makes the invoke/focus land reliably and keeps
        // any bounds-based fallback on real pixels. Best-effort and non-fatal: on
        // any failure we act in place anyway, and the kill switch pre-empts it.
        if matches!(action, Action::Invoke { .. } | Action::Focus { .. }) {
            if let Some(path) = action.target() {
                let offscreen = snapshot
                    .get(path)
                    .is_some_and(|e| e.has_state(ElementState::Offscreen));
                if offscreen {
                    let path = path.to_string();
                    let kill = self.kill.clone();
                    tokio::select! {
                        biased;
                        _ = kill.wait_tripped() => return Ok(Flow::Aborted),
                        r = self.backend.scroll_into_view(&path) => {
                            if let Err(e) = r {
                                tracing::debug!(
                                    target = %path,
                                    error = %e,
                                    "act executor: scroll_into_view failed; acting in place"
                                );
                            }
                        }
                    }
                }
            }
        }

        // 6. Execute, racing the kill switch.
        match self.execute_action(&action).await? {
            Execution::Ok => {
                // 7. Verify the post-condition. On failure, HALT the plan rather
                // than run later steps — a destructive side effect must not be
                // amplified by continuing on top of an unexpected state.
                if self.verify(&action).await? {
                    self.audit_step(&action, decision, "ok");
                    Ok(Flow::Continue(StepOutcome::Done {
                        action,
                        verified: true,
                    }))
                } else {
                    self.audit_step(&action, decision, "failed: verify");
                    Ok(Flow::Halt(StepOutcome::Failed {
                        action,
                        error: "post-action verification failed".into(),
                    }))
                }
            }
            Execution::Failed(error) => {
                self.audit_step(&action, decision, "failed: backend");
                Ok(Flow::Halt(StepOutcome::Failed { action, error }))
            }
            Execution::Aborted => Ok(Flow::Aborted),
            Execution::Stop => {
                self.audit_step(&action, decision, "ok: stop");
                Ok(Flow::Stop)
            }
            Execution::AskUser { prompt, options } => {
                self.audit_step(&action, decision, "paused: ask_user");
                Ok(Flow::Pause(StepOutcome::NeedsAskUser { prompt, options }))
            }
        }
    }

    /// Run one script primitive: injection guards -> capability gate -> execute
    /// -> minimal verify. Guards deny BEFORE the confirm gate, so a dangerous or
    /// screen-originated command is refused outright and never offered as a
    /// "run this?" prompt.
    async fn step_script(&mut self, action: Action, preapproved: bool) -> Result<Flow, ExecError> {
        // Injection / dangerous-command guards (hard Deny, ahead of the gate).
        if let Some(reason) = self.script_guard(&action).await? {
            self.audit_step(&action, Decision::Deny, "blocked: script_guard");
            return Ok(Flow::Halt(StepOutcome::Denied { action, reason }));
        }

        // Capability gate. Launch/Uri/Shell/Clipboard default to Confirm; a
        // one-time pre-approval softens Confirm to Allow. Wait/FocusApp are Allow.
        let decision = match (self.gate.evaluate(&action), preapproved) {
            (Decision::Confirm, true) => Decision::Allow,
            (d, _) => d,
        };
        match decision {
            Decision::Deny => {
                let reason = format!("capability denied: {}", action.kind());
                self.audit_step(&action, decision, "blocked: capability_denied");
                return Ok(Flow::Halt(StepOutcome::Denied { action, reason }));
            }
            Decision::Confirm => {
                let reason = script_confirm_reason(&action);
                self.audit_step(&action, decision, "paused: needs_confirm");
                return Ok(Flow::Pause(StepOutcome::NeedsConfirm { action, reason }));
            }
            Decision::Allow => {}
        }

        match self.execute_script(&action).await? {
            Execution::Ok => {
                // Arm the focus guard: this command deliberately moved to an app, so
                // any later keystroke must land there, not in the prior foreground.
                match &action {
                    Action::Launch { target, .. } => {
                        self.expected_app = Some(target.clone());
                    }
                    Action::FocusApp { name } => {
                        self.expected_app = Some(name.clone());
                    }
                    _ => {}
                }
                // Launch/Uri only get best-effort verification (a window may take
                // time to appear), so they are Done-but-unverified; Shell/FocusApp
                // already encoded a concrete success signal (exit 0 / foreground).
                let verified = !matches!(action, Action::Launch { .. } | Action::Uri { .. });
                self.audit_step(&action, decision, "ok");
                Ok(Flow::Continue(StepOutcome::Done { action, verified }))
            }
            Execution::Failed(error) => {
                self.audit_step(&action, decision, "failed: backend");
                Ok(Flow::Halt(StepOutcome::Failed { action, error }))
            }
            Execution::Aborted => Ok(Flow::Aborted),
            Execution::Stop => Ok(Flow::Stop),
            Execution::AskUser { prompt, options } => {
                self.audit_step(&action, decision, "paused: ask_user");
                Ok(Flow::Pause(StepOutcome::NeedsAskUser { prompt, options }))
            }
        }
    }

    /// Injection / dangerous-command guards for a script primitive. Returns
    /// `Some(reason)` to deny the step outright. Layered with the planner's
    /// plan-time validation (defense in depth): the executor is the last line.
    async fn script_guard(&self, action: &Action) -> Result<Option<String>, ExecError> {
        match action {
            Action::Shell {
                command,
                shell,
                origin,
            } => {
                // A command the model derived from on-screen (untrusted) content is
                // never allowed to run.
                if *origin == Origin::Screen {
                    return Ok(Some("screen-originated shell command refused".into()));
                }
                // Independent Deny classifier — the real boundary, not the prompt.
                if let ShellVerdict::Deny(reason) = shell_policy::classify_command(command, shell) {
                    return Ok(Some(format!("blocked dangerous command: {reason}")));
                }
                // Laundering guard: a command that echoes a long span of on-screen
                // text is very likely injected through the model. Fail closed.
                let snapshot = self
                    .backend
                    .snapshot()
                    .await
                    .map_err(|e| ExecError::Backend(e.to_string()))?;
                if command_echoes_screen(command, &snapshot) {
                    return Ok(Some("shell command echoes on-screen text".into()));
                }
                Ok(None)
            }
            Action::Launch { target, origin } => {
                if *origin == Origin::Screen && shell_policy::is_risky_launch_target(target) {
                    return Ok(Some("screen-originated risky launch refused".into()));
                }
                Ok(None)
            }
            Action::Uri { uri, origin } => {
                if shell_policy::is_dangerous_uri_scheme(uri) {
                    return Ok(Some(format!("blocked dangerous URI scheme: {uri}")));
                }
                if *origin == Origin::Screen {
                    return Ok(Some("screen-originated URI refused".into()));
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// Perform one script primitive against the backend, racing the kill switch.
    async fn execute_script(&self, action: &Action) -> Result<Execution, ExecError> {
        let kill = self.kill.clone();
        match action {
            Action::Wait { ms } => {
                let dur = std::time::Duration::from_millis(clamp_wait_ms(*ms) as u64);
                tokio::select! {
                    biased;
                    _ = kill.wait_tripped() => Ok(Execution::Aborted),
                    _ = tokio::time::sleep(dur) => Ok(Execution::Ok),
                }
            }
            Action::Click { x, y } => {
                // `vision`-mode coordinate click. The focus guard (armed by an
                // earlier launch/focus in this run) still applies to the app that
                // owns the coordinate, and the kill switch pre-empts it.
                let res = tokio::select! {
                    biased;
                    _ = kill.wait_tripped() => return Ok(Execution::Aborted),
                    r = self.backend.click_point(*x, *y) => r,
                };
                Ok(res.map_or_else(|e| Execution::Failed(e.to_string()), |()| Execution::Ok))
            }
            Action::Launch { target, .. } => {
                // A web URL handed to `launch` can't be started as an application
                // (terminator fails with "Failed to launch application 'https://…'").
                // The model meant "open this link" — route it to the URI opener.
                if target_is_url(target) {
                    let res = tokio::select! {
                        biased;
                        _ = kill.wait_tripped() => return Ok(Execution::Aborted),
                        r = self.backend.open_uri(target) => r,
                    };
                    return Ok(
                        res.map_or_else(|e| Execution::Failed(e.to_string()), |()| Execution::Ok)
                    );
                }
                let res = tokio::select! {
                    biased;
                    _ = kill.wait_tripped() => return Ok(Execution::Aborted),
                    r = self.backend.launch(target) => r,
                };
                Ok(res.map_or_else(|e| Execution::Failed(e.to_string()), |()| Execution::Ok))
            }
            Action::Uri { uri, .. } => {
                let res = tokio::select! {
                    biased;
                    _ = kill.wait_tripped() => return Ok(Execution::Aborted),
                    r = self.backend.open_uri(uri) => r,
                };
                Ok(res.map_or_else(|e| Execution::Failed(e.to_string()), |()| Execution::Ok))
            }
            Action::FocusApp { name } => {
                let res = tokio::select! {
                    biased;
                    _ = kill.wait_tripped() => return Ok(Execution::Aborted),
                    r = self.backend.focus_app(name) => r,
                };
                match res {
                    Ok(true) => Ok(Execution::Ok),
                    Ok(false) => Ok(Execution::Failed(format!("could not focus \"{name}\""))),
                    Err(e) => Ok(Execution::Failed(e.to_string())),
                }
            }
            Action::Shell { command, shell, .. } => {
                let res = tokio::select! {
                    biased;
                    _ = kill.wait_tripped() => return Ok(Execution::Aborted),
                    r = self.backend.run_shell(command, shell) => r,
                };
                match res {
                    Ok(out) if out.exit_code == 0 => Ok(Execution::Ok),
                    Ok(out) => {
                        let tail: String = out.stdout.chars().take(200).collect();
                        Ok(Execution::Failed(format!("exit {}: {tail}", out.exit_code)))
                    }
                    Err(e) => Ok(Execution::Failed(e.to_string())),
                }
            }
            Action::Clipboard { op, text } => match op {
                ClipboardOp::Set => {
                    let res = tokio::select! {
                        biased;
                        _ = kill.wait_tripped() => return Ok(Execution::Aborted),
                        r = self.backend.clipboard_set(text) => r,
                    };
                    Ok(res.map_or_else(|e| Execution::Failed(e.to_string()), |()| Execution::Ok))
                }
                ClipboardOp::Get => {
                    let res = tokio::select! {
                        biased;
                        _ = kill.wait_tripped() => return Ok(Execution::Aborted),
                        r = self.backend.clipboard_get() => r,
                    };
                    Ok(res.map_or_else(|e| Execution::Failed(e.to_string()), |_| Execution::Ok))
                }
            },
            _ => Ok(Execution::Failed("not a script primitive".into())),
        }
    }

    /// Resolve an action's target path against the current snapshot, re-grounding
    /// a stale path by phrase. Actions without a target are returned as-is.
    fn resolve_target(&self, action: &Action, snapshot: &Snapshot) -> TargetResolution {
        let Some(path) = action.target() else {
            return TargetResolution::Ready(action.clone());
        };

        if snapshot.get(path).is_some() {
            return TargetResolution::Ready(action.clone());
        }

        // Stale path — re-ground. The executor does not retain the original spoken
        // phrase, so we ground by the target string itself: a resolver hit repairs
        // the action, an ambiguity asks the user, and nothing fails the step.
        match grounding::resolve(snapshot, path) {
            Grounded::One(resolved) => TargetResolution::Ready(retarget(action, resolved)),
            Grounded::Ambiguous(paths) => {
                let options = paths
                    .into_iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let label = snapshot
                            .get(&p)
                            .map(|e| e.name.clone())
                            .filter(|n| !n.is_empty())
                            .unwrap_or_else(|| p.clone());
                        AskOption {
                            index: i + 1,
                            label,
                            path: p,
                        }
                    })
                    .collect();
                TargetResolution::Ambiguous(options)
            }
            Grounded::None => TargetResolution::Gone,
        }
    }

    /// The accessibility (name, description) of the control an action will
    /// activate, as seen in the live snapshot. Targeted actions use their target
    /// element; a targetless [`Action::Key`] (bare Enter/Space) uses the FOCUSED
    /// element, so a fast-path "submit" is classified against the focused control.
    fn resolved_identity(&self, action: &Action, snapshot: &Snapshot) -> (String, String) {
        if let Some(path) = action.target() {
            if let Some(e) = snapshot.get(path) {
                return (e.name.clone(), e.description.clone());
            }
        }
        if matches!(action, Action::Key { .. }) {
            if let Some(e) = snapshot.focused_element() {
                return (e.name.clone(), e.description.clone());
            }
        }
        (String::new(), String::new())
    }

    /// The `(path, name)` fingerprint identifying the exact control an action will
    /// activate, used to bind a destructive pre-approval. `None` when there is no
    /// concrete control to pin (no target and nothing focused).
    fn target_fingerprint(&self, action: &Action, snapshot: &Snapshot) -> Option<ConfirmedTarget> {
        if let Some(path) = action.target() {
            let name = snapshot
                .get(path)
                .map(|e| e.name.clone())
                .unwrap_or_default();
            return Some((path.to_string(), name));
        }
        if matches!(action, Action::Key { .. }) {
            if let Some(path) = &snapshot.focused {
                let name = snapshot
                    .get(path)
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
                return Some((path.clone(), name));
            }
        }
        None
    }

    /// Best-effort post-condition verification for a just-run action. Returns
    /// `false` when the observed state contradicts the action's intent, which the
    /// caller turns into a plan-halting [`StepOutcome::Failed`]. Conservative: only
    /// checks a cheap, unambiguous post-condition (focus landed) and passes
    /// everything else, so it never spuriously fails a benign step.
    async fn verify(&self, action: &Action) -> Result<bool, ExecError> {
        match action {
            Action::Focus { target } => {
                let snapshot = self
                    .backend
                    .snapshot()
                    .await
                    .map_err(|e| ExecError::Backend(e.to_string()))?;
                Ok(snapshot.focused.as_deref() == Some(target.as_str()))
            }
            _ => Ok(true),
        }
    }

    /// Perform the backend operation for one action, racing the kill switch. A
    /// trip mid-await yields [`Execution::Aborted`].
    async fn execute_action(&self, action: &Action) -> Result<Execution, ExecError> {
        let kill = self.kill.clone();

        macro_rules! guarded {
            ($fut:expr) => {
                tokio::select! {
                    biased;
                    _ = kill.wait_tripped() => return Ok(Execution::Aborted),
                    res = $fut => res.map_err(|e: crate::error::AppError| e.to_string()),
                }
            };
        }

        let result: Result<(), String> = match action {
            Action::Focus { target } => guarded!(self.backend.focus(target)),
            Action::Invoke { target } => guarded!(self.backend.invoke(target)),
            Action::Key { combo } => guarded!(self.backend.key_combo(combo)),
            Action::Type { text, clear } => {
                let mut typed: Result<(), String> = Ok(());
                if *clear {
                    typed = guarded!(self.backend.key_combo("ctrl+a"));
                }
                if typed.is_ok() {
                    if should_paste(text) {
                        // Large or multi-line text is PASTED (clipboard + Ctrl+V)
                        // rather than typed key-by-key. Char-typing a long report
                        // blows past the UIA type watchdog (10s) — each 500-byte
                        // chunk is a single broker op that never returns, so the
                        // loop retries the same type forever. Paste is one fast op,
                        // and it inserts newlines as plain line breaks instead of
                        // the page breaks that keystroke `\n` produces in some
                        // editors (e.g. a Word dictation add-in intercepting Enter).
                        typed = guarded!(self.backend.clipboard_set(text));
                        if typed.is_ok() {
                            typed = guarded!(self.backend.key_combo("ctrl+v"));
                        }
                    } else {
                        // Short single-line text is still typed: it mimics real
                        // keystrokes for search boxes and does not clobber the
                        // clipboard. Chunked <=500 bytes on char boundaries (never
                        // splitting a CRLF); each chunk races the kill switch.
                        for chunk in chunk_type_text(text) {
                            typed = guarded!(self.backend.type_text(chunk));
                            if typed.is_err() {
                                break;
                            }
                        }
                    }
                }
                typed
            }
            Action::Scroll { amount, .. } => {
                // Drive the real backend scroll primitive. `amount` is a signed
                // wheel-notch count (positive = down, negative = up); a zero/absent
                // amount defaults to a modest downward nudge (the common "scroll
                // down to see more results" case). The target, if any, was already
                // brought into view by the offscreen-reach step below; the wheel
                // itself acts on the foreground scrollable region.
                guarded!(self.backend.scroll(0, scroll_dy(*amount)))
            }
            Action::SelectMenu { path } => {
                // Best-effort: walk the menu by invoking each item path in turn.
                if path.is_empty() {
                    return Ok(Execution::Failed("select_menu: empty menu path".into()));
                }
                let mut walked: Result<(), String> = Ok(());
                for item in path {
                    walked = guarded!(self.backend.invoke(item));
                    if walked.is_err() {
                        break;
                    }
                }
                walked
            }
            Action::AskUser { question, choices } => {
                let options = choices
                    .iter()
                    .enumerate()
                    .map(|(i, c)| AskOption {
                        index: i + 1,
                        label: c.clone(),
                        path: String::new(),
                    })
                    .collect();
                return Ok(Execution::AskUser {
                    prompt: question.clone(),
                    options,
                });
            }
            // Script primitives are routed to `step_script` before reaching here;
            // this arm only exists for match exhaustiveness and fails closed.
            Action::Launch { .. }
            | Action::Uri { .. }
            | Action::Shell { .. }
            | Action::Wait { .. }
            | Action::Click { .. }
            | Action::FocusApp { .. }
            | Action::Clipboard { .. } => {
                return Ok(Execution::Failed(
                    "script primitive reached the a11y path".into(),
                ));
            }
            Action::Stop => return Ok(Execution::Stop),
        };

        Ok(match result {
            Ok(()) => Execution::Ok,
            Err(error) => Execution::Failed(error),
        })
    }

    /// Resume by resolving the user's disambiguation pick to a concrete path and
    /// running that action (invoke/focus), then the rest of the plan.
    async fn resume_with_pick(
        &mut self,
        actions: Vec<Action>,
        index: usize,
    ) -> Result<ExecResult, ExecError> {
        if self.kill.is_tripped() {
            return Ok(ExecResult {
                outcomes: vec![StepOutcome::Aborted],
                completed: false,
            });
        }

        let mut rest = actions;
        if rest.is_empty() {
            return Ok(ExecResult {
                outcomes: Vec::new(),
                completed: true,
            });
        }
        let head = rest.remove(0);

        let snapshot = self
            .backend
            .snapshot()
            .await
            .map_err(|e| ExecError::Backend(e.to_string()))?;

        let picked = match head.target() {
            Some(path) => match grounding::resolve(&snapshot, path) {
                Grounded::Ambiguous(paths) => paths.into_iter().nth(index),
                Grounded::One(p) => Some(p),
                Grounded::None => None,
            },
            None => None,
        };

        let mut outcomes: Vec<StepOutcome> = Vec::new();

        let Some(path) = picked else {
            outcomes.push(StepOutcome::Failed {
                action: head,
                error: "disambiguation pick no longer present".into(),
            });
            return Ok(ExecResult {
                outcomes,
                completed: false,
            });
        };

        // The chosen element becomes the head action retargeted to that path.
        let resolved = retarget(&head, path);
        let action_kind = resolved.kind();
        let action_target = resolved.target().map(|t| t.to_string());
        let flow = self.step(resolved, false).await?;
        let (outcome_label, detail) = describe_flow(&flow);
        tracing::info!(
            step = "pick",
            action = action_kind,
            target = action_target.as_deref().unwrap_or(""),
            outcome = outcome_label,
            detail = %detail,
            "act executor step (resumed pick)"
        );
        match flow {
            Flow::Continue(outcome) => outcomes.push(outcome),
            Flow::Halt(outcome) | Flow::Pause(outcome) => {
                outcomes.push(outcome);
                return Ok(ExecResult {
                    outcomes,
                    completed: false,
                });
            }
            Flow::Aborted => {
                outcomes.push(StepOutcome::Aborted);
                return Ok(ExecResult {
                    outcomes,
                    completed: false,
                });
            }
            Flow::Stop => {
                return Ok(ExecResult {
                    outcomes,
                    completed: true,
                });
            }
        }

        // Continue with whatever remains after the pick.
        let tail = self.run(rest, false).await?;
        let completed = tail.completed;
        outcomes.extend(tail.outcomes);
        Ok(ExecResult {
            outcomes,
            completed,
        })
    }

    /// Record this step's snapshot fingerprint and report whether it differs from
    /// the previous a11y step's. Logging-only: it never gates execution.
    fn note_snapshot_change(&mut self, snapshot: &Snapshot) -> bool {
        let fp = snapshot_fingerprint(snapshot);
        let changed = self.last_snapshot_fp.as_deref() != Some(fp.as_str());
        self.last_snapshot_fp = Some(fp);
        changed
    }

    /// Append one audit row for a step, if an audit log is attached. Best-effort:
    /// a write error must not abort the plan. Never carries element values — the
    /// snapshot schema is value-free and is stored only as a hash.
    fn audit_step(&mut self, action: &Action, decision: Decision, result: &str) {
        if let Some(log) = self.audit.as_mut() {
            let entry = AuditEntry::new(
                action.kind(),
                &Snapshot::default(),
                vec![action.clone()],
                vec![decision],
                result,
            );
            let _ = log.append(&entry);
        }
    }
}

/// Outcome of resolving an action's target against the live snapshot.
enum TargetResolution {
    /// The action is ready to run (target present, re-grounded, or targetless).
    Ready(Action),
    /// The target re-grounded to several candidates; ask the user to pick.
    Ambiguous(Vec<AskOption>),
    /// The target is gone and could not be re-grounded.
    Gone,
}

/// Outcome of performing an action's backend operation.
enum Execution {
    Ok,
    Failed(String),
    Aborted,
    Stop,
    AskUser {
        prompt: String,
        options: Vec<AskOption>,
    },
}

/// Whether a `launch` target is really a web URL, so it should be opened via the
/// URI handler rather than started as an application. Only the explicit http(s)
/// schemes qualify — a bare app name ("chrome", "notepad") must still launch.
fn target_is_url(target: &str) -> bool {
    let lower = target.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Upper bound on a model-emitted `wait`. The planner habitually appends a flat
/// multi-second `wait` after a navigation ("launch, wait 5000, stop") to let a
/// page/app appear, which blindly burns the full guess even when the screen
/// settled in a fraction of it. Clamping keeps a genuine pause honest while
/// capping the worst case; the Conductor's adaptive post-navigation settle (poll
/// the snapshot fingerprint, proceed on first change) recovers the rest.
const WAIT_CEIL_MS: u32 = 2500;

/// Clamp a model-emitted wait to [`WAIT_CEIL_MS`]. A short, deliberate pause is
/// preserved; a multi-second guess is bounded.
fn clamp_wait_ms(ms: u32) -> u32 {
    ms.min(WAIT_CEIL_MS)
}

/// Whether an action is a "script primitive" (OS/shell op) rather than an
/// accessibility-tree op, and so takes the [`Executor::step_script`] path.
fn is_script_primitive(action: &Action) -> bool {
    matches!(
        action,
        Action::Launch { .. }
            | Action::Uri { .. }
            | Action::Shell { .. }
            | Action::Wait { .. }
            | Action::Click { .. }
            | Action::FocusApp { .. }
            | Action::Clipboard { .. }
    )
}

/// A short, PHI-free confirmation reason shown before a gated script primitive
/// runs. Shell shows the exact command + shell so the user sees what will run.
fn script_confirm_reason(action: &Action) -> String {
    match action {
        Action::Shell { command, shell, .. } => format!("run {shell} command: {command}"),
        Action::Launch { target, .. } => format!("launch {target}"),
        Action::Uri { uri, .. } => format!("open {uri}"),
        Action::Clipboard {
            op: ClipboardOp::Set,
            ..
        } => "write to the clipboard".into(),
        Action::Clipboard {
            op: ClipboardOp::Get,
            ..
        } => "read the clipboard".into(),
        other => format!("{} requires confirmation", other.kind()),
    }
}

/// Whether `command` contains a contiguous >=12-character span that also appears
/// in the snapshot's on-screen text — a strong signal the command was laundered
/// from injected screen content rather than the user's spoken intent.
fn command_echoes_screen(command: &str, snapshot: &Snapshot) -> bool {
    const MIN: usize = 12;
    let screen: String = snapshot
        .elements
        .iter()
        .map(|e| e.name.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    if screen.trim().is_empty() {
        return false;
    }
    let cmd: Vec<char> = command.to_lowercase().chars().collect();
    if cmd.len() < MIN {
        return false;
    }
    (0..=cmd.len() - MIN).any(|start| {
        let window: String = cmd[start..start + MIN].iter().collect();
        // Ignore windows that are mostly whitespace (weak matches).
        window.trim().len() >= MIN && screen.contains(&window)
    })
}

/// A cheap, PHI-free fingerprint of a snapshot (app, window title, element count)
/// for the "did the screen change between steps" trace signal.
fn snapshot_fingerprint(snap: &Snapshot) -> String {
    format!("{}|{}|{}", snap.app, snap.window_title, snap.elements.len())
}

/// A short trace label + PHI-safe detail for one step's [`Flow`] outcome. Only our
/// own reason/error strings are surfaced (never a model-authored ask_user prompt).
fn describe_flow(flow: &Flow) -> (&'static str, String) {
    match flow {
        Flow::Continue(o) | Flow::Halt(o) | Flow::Pause(o) => match o {
            StepOutcome::Done { verified: true, .. } => ("done", String::new()),
            StepOutcome::Done {
                verified: false, ..
            } => ("done_unverified", String::new()),
            StepOutcome::NeedsConfirm { reason, .. } => ("needs_confirm", reason.clone()),
            StepOutcome::NeedsAskUser { .. } => ("needs_ask_user", String::new()),
            StepOutcome::Denied { reason, .. } => ("denied", reason.clone()),
            StepOutcome::Failed { error, .. } => ("failed", error.clone()),
            StepOutcome::Aborted => ("aborted", String::new()),
        },
        Flow::Aborted => ("aborted", String::new()),
        Flow::Stop => ("stop", String::new()),
    }
}

/// The default number of vertical wheel notches a `Scroll` with no explicit
/// `amount` performs — a modest downward nudge, enough to pull the next band of
/// list results into view without overshooting the whole page.
const DEFAULT_SCROLL_NOTCHES: i32 = 3;

/// Translate a [`Action::Scroll`]'s `amount` into vertical wheel notches for the
/// backend `scroll` primitive. `amount` is a signed notch count (positive = down,
/// negative = up); a zero/absent `amount` defaults to
/// [`DEFAULT_SCROLL_NOTCHES`] downward.
fn scroll_dy(amount: i32) -> i32 {
    if amount == 0 {
        DEFAULT_SCROLL_NOTCHES
    } else {
        amount
    }
}

/// Above this length, a `type` action is pasted (clipboard + Ctrl+V) instead of
/// typed key-by-key, to stay clear of the UIA type watchdog. Multi-line text is
/// always pasted regardless of length (keystroke `\n` becomes a page break in some
/// editors). Short single-line text is still typed.
const TYPE_PASTE_THRESHOLD: usize = 120;

/// Whether a `type` action's text should be pasted rather than typed. True for any
/// text that contains a newline or exceeds [`TYPE_PASTE_THRESHOLD`] bytes.
fn should_paste(text: &str) -> bool {
    text.len() > TYPE_PASTE_THRESHOLD || text.contains('\n')
}

/// The maximum byte length of one `type_text` chunk. Long text is split into
/// pieces no larger than this before being sent to the backend.
const TYPE_CHUNK_BYTES: usize = 500;

/// Split `text` into chunks of at most [`TYPE_CHUNK_BYTES`] bytes, cutting only on
/// UTF-8 char boundaries and never between a `\r` and its following `\n`. Short
/// text (including "") returns a single chunk, preserving the original single-call
/// behavior. Newlines within the text are preserved exactly.
fn chunk_type_text(text: &str) -> Vec<&str> {
    if text.len() <= TYPE_CHUNK_BYTES {
        return vec![text];
    }
    let bytes = text.as_bytes();
    let mut chunks: Vec<&str> = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + TYPE_CHUNK_BYTES).min(text.len());
        if end < text.len() {
            // Back up to the nearest char boundary at or before the cap.
            while end > start && !text.is_char_boundary(end) {
                end -= 1;
            }
            // Never split a CRLF: if the cut lands right after '\r' and the next
            // byte is '\n', move the '\r' to the next chunk with its '\n'.
            if end > start && bytes[end - 1] == b'\r' && end < text.len() && bytes[end] == b'\n' {
                end -= 1;
            }
            // Guarantee forward progress (degenerate: a single char wider than the
            // cap can't happen for UTF-8 <=4 bytes, but stay safe).
            if end == start {
                end = start + 1;
                while end < text.len() && !text.is_char_boundary(end) {
                    end += 1;
                }
            }
        }
        chunks.push(&text[start..end]);
        start = end;
    }
    chunks
}

/// Return `action` with its target path replaced by `path` (for the target-bearing
/// variants); other actions are returned unchanged.
fn retarget(action: &Action, path: String) -> Action {
    match action {
        Action::Focus { .. } => Action::Focus { target: path },
        Action::Invoke { .. } => Action::Invoke { target: path },
        Action::Scroll { amount, .. } => Action::Scroll {
            target: Some(path),
            amount: *amount,
        },
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::element::{ActionPattern, Bounds, Role, Snapshot, UiElement};
    use crate::act::mock_backend::MockBackend;

    fn el(path: &str, role: Role, name: &str) -> UiElement {
        UiElement {
            path: path.to_string(),
            role,
            name: name.to_string(),
            description: String::new(),
            value_len: 0,
            states: Vec::new(),
            bounds: Some(Bounds {
                x: 0,
                y: 0,
                w: 10,
                h: 10,
            }),
            patterns: vec![ActionPattern::Invoke],
        }
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            app: "Editor".into(),
            window_title: "Untitled".into(),
            focused: Some("#/1".into()),
            pointer: None,
            selection_text_len: 0,
            elements: vec![
                el("#/1", Role::TextField, "Message"),
                // A deliberately NON-destructive control: the runtime classifier
                // must let plain buttons through. Destructive controls are covered
                // by their own tests below.
                el("#/2", Role::Button, "Next"),
            ],
        }
    }

    fn executor(backend: Arc<dyn AccessibilityBackend>) -> Executor {
        Executor::new(backend, CapabilityGate::new(), None, KillSwitch::new())
    }

    /// An executor whose gate grants AppLaunch (as the live Conductor does), so a
    /// `Launch` runs instead of pausing on Confirm — needed to exercise the focus
    /// guard, which only arms after a successful launch/switch.
    fn launching_executor(backend: Arc<dyn AccessibilityBackend>) -> Executor {
        use crate::act::capability::Capability;
        let mut gate = CapabilityGate::new();
        gate.grant(Capability::AppLaunch);
        Executor::new(backend, gate, None, KillSwitch::new())
    }

    fn snapshot_for_app(app: &str) -> Snapshot {
        Snapshot {
            app: app.into(),
            window_title: "w".into(),
            focused: Some("#/1".into()),
            pointer: None,
            selection_text_len: 0,
            elements: vec![el("#/1", Role::TextField, "Message")],
        }
    }

    #[tokio::test]
    async fn launch_with_http_url_opens_as_uri_not_app() {
        // The planner sometimes emits {"op":"launch","target":"https://…"}; a URL
        // cannot be started as an application, so it must route to the URI opener
        // instead of failing with "Failed to launch application 'https://…'".
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot_for_app("Editor"))
                .build(),
        );
        let mut exec = launching_executor(backend.clone());
        let url = "https://www.youtube.com/results?search_query=Hotel+California";
        let plan = ActionPlan::new(vec![Action::Launch {
            target: url.into(),
            origin: Origin::WorldKnowledge,
        }]);

        let result = exec.execute_plan(plan).await.unwrap();
        assert!(result.completed, "URL should open via open_uri");
        assert!(backend.launched().is_empty(), "must not app-launch a URL");
        assert_eq!(backend.opened_uris(), vec![url.to_string()]);
    }

    #[tokio::test]
    async fn launch_with_bare_app_name_still_launches() {
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot_for_app("Editor"))
                .build(),
        );
        let mut exec = launching_executor(backend.clone());
        let plan = ActionPlan::new(vec![Action::Launch {
            target: "notepad".into(),
            origin: Origin::WorldKnowledge,
        }]);

        exec.execute_plan(plan).await.unwrap();
        assert_eq!(backend.launched(), vec!["notepad".to_string()]);
        assert!(backend.opened_uris().is_empty());
    }

    #[tokio::test]
    async fn focus_guard_aborts_type_when_launched_app_is_not_foreground() {
        // Foreground stays "Editor" (mock snapshot is static, so focus_app can't
        // move it) after launching Chrome — exactly the race that typed into the
        // terminal. The guard must refuse to type.
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot_for_app("Editor"))
                .build(),
        );
        let mut exec = launching_executor(backend.clone());

        let plan = ActionPlan::new(vec![
            Action::Launch {
                target: "Google Chrome".into(),
                origin: Origin::TaskIntent,
            },
            Action::Type {
                text: "youtube.com".into(),
                clear: false,
            },
        ]);

        let result = exec.execute_plan(plan).await.unwrap();

        assert!(
            !result.completed,
            "must not complete: guard aborts the Type"
        );
        assert_eq!(backend.launched(), vec!["Google Chrome".to_string()]);
        assert!(
            backend.typed().is_empty(),
            "the guard must never send keystrokes to the wrong window"
        );
        assert!(result
            .outcomes
            .iter()
            .any(|o| matches!(o, StepOutcome::Failed { .. })));
    }

    #[tokio::test]
    async fn focus_guard_allows_type_when_launched_app_is_foreground() {
        // Foreground already IS Chrome, so the guard passes and the type lands.
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot_for_app("Chrome"))
                .build(),
        );
        let mut exec = launching_executor(backend.clone());

        let plan = ActionPlan::new(vec![
            Action::Launch {
                target: "Google Chrome".into(),
                origin: Origin::TaskIntent,
            },
            Action::Type {
                text: "hello".into(),
                clear: false,
            },
        ]);

        let result = exec.execute_plan(plan).await.unwrap();

        assert!(
            result.completed,
            "guard passes when the target is foreground"
        );
        assert_eq!(backend.typed(), vec!["hello".to_string()]);
    }

    #[tokio::test]
    async fn focus_guard_inert_without_a_launch() {
        // No launch/switch in the plan → guard stays inert → a plain type against
        // the current window works unchanged (a "type this" with the app already up).
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot_for_app("Editor"))
                .build(),
        );
        let mut exec = executor(backend.clone());

        let plan = ActionPlan::new(vec![Action::Type {
            text: "hi".into(),
            clear: false,
        }]);

        let result = exec.execute_plan(plan).await.unwrap();
        assert!(result.completed);
        assert_eq!(backend.typed(), vec!["hi".to_string()]);
    }

    #[test]
    fn should_paste_covers_multiline_and_long_text() {
        assert!(should_paste("a\nb"), "any newline pastes");
        assert!(should_paste(&"x".repeat(200)), "long text pastes");
        assert!(
            !should_paste("Cal California"),
            "short single line is typed"
        );
        assert!(
            !should_paste(""),
            "empty text keeps the single-call type path"
        );
    }

    #[tokio::test]
    async fn large_multiline_type_is_pasted_not_typed() {
        // A long / multi-line `type` (a written report) must paste via clipboard +
        // Ctrl+V, never char-type: char typing blows past the UIA type watchdog and
        // turns newlines into page breaks in some editors.
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot_for_app("Editor"))
                .build(),
        );
        let mut exec = executor(backend.clone());
        let report = "Line one\nLine two\nLine three".to_string();
        let plan = ActionPlan::new(vec![Action::Type {
            text: report.clone(),
            clear: false,
        }]);

        let result = exec.execute_plan(plan).await.unwrap();
        assert!(result.completed);
        assert_eq!(backend.clipboard_sets(), vec![report]);
        assert!(
            backend
                .keys()
                .iter()
                .any(|k| k.eq_ignore_ascii_case("ctrl+v")),
            "pasted text is committed with Ctrl+V"
        );
        assert!(
            backend.typed().is_empty(),
            "a pasted block must not also be char-typed"
        );
    }

    #[tokio::test]
    async fn focus_guard_persists_across_continuing_batches() {
        // The closed-loop fix (#2): a launch in one batch must keep the guard armed
        // for a `Type` in a LATER batch. Using `execute_plan_continuing` (as the
        // novel loop does), the `expected_app` set by batch 1's launch survives into
        // batch 2 — so the type into the wrong (still-Editor) foreground is refused.
        // Contrast with `execute_plan_with_context`, which resets per call.
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot_for_app("Editor"))
                .build(),
        );
        let mut exec = launching_executor(backend.clone());
        // A new goal starts inert.
        exec.reset_focus_guard();

        // Batch 1: launch (arms the guard for "Google Chrome").
        let b1 = exec
            .execute_plan_continuing(
                ActionPlan::new(vec![Action::Launch {
                    target: "Google Chrome".into(),
                    origin: Origin::TaskIntent,
                }]),
                "open chrome and type",
            )
            .await
            .unwrap();
        assert!(b1.completed);
        assert_eq!(backend.launched(), vec!["Google Chrome".to_string()]);

        // Batch 2: type. Foreground is still "Editor", so the guard armed in batch 1
        // must refuse the keystroke.
        let b2 = exec
            .execute_plan_continuing(
                ActionPlan::new(vec![Action::Type {
                    text: "youtube.com".into(),
                    clear: false,
                }]),
                "open chrome and type",
            )
            .await
            .unwrap();
        assert!(
            !b2.completed,
            "guard armed in batch 1 must survive into batch 2"
        );
        assert!(
            backend.typed().is_empty(),
            "the guard must never send keystrokes to the wrong window across batches"
        );
        assert!(b2
            .outcomes
            .iter()
            .any(|o| matches!(o, StepOutcome::Failed { .. })));
    }

    #[tokio::test]
    async fn reset_focus_guard_starts_a_new_goal_inert() {
        // After a goal armed the guard, `reset_focus_guard` clears the latch so the
        // NEXT goal's type against the current window is unaffected (the guard does
        // not leak an expected app across goals).
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot_for_app("Editor"))
                .build(),
        );
        let mut exec = launching_executor(backend.clone());

        // Goal 1 arms the guard for Chrome.
        exec.execute_plan_continuing(
            ActionPlan::new(vec![Action::Launch {
                target: "Google Chrome".into(),
                origin: Origin::TaskIntent,
            }]),
            "goal one",
        )
        .await
        .unwrap();

        // New goal: reset, then a plain type must land (guard inert again).
        exec.reset_focus_guard();
        let b = exec
            .execute_plan_continuing(
                ActionPlan::new(vec![Action::Type {
                    text: "hi".into(),
                    clear: false,
                }]),
                "goal two",
            )
            .await
            .unwrap();
        assert!(b.completed, "a reset guard leaves a plain type unguarded");
        assert_eq!(backend.typed(), vec!["hi".to_string()]);
    }

    #[tokio::test]
    async fn execute_plan_with_context_resets_focus_guard_per_call() {
        // The single-plan path must keep resetting the guard: even if a prior
        // continuing batch armed it, `execute_plan_with_context` starts inert.
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot_for_app("Editor"))
                .build(),
        );
        let mut exec = launching_executor(backend.clone());

        // Arm the guard for Chrome via a continuing batch.
        exec.execute_plan_continuing(
            ActionPlan::new(vec![Action::Launch {
                target: "Google Chrome".into(),
                origin: Origin::TaskIntent,
            }]),
            "arm it",
        )
        .await
        .unwrap();

        // A fresh single-plan command resets the latch, so this type lands.
        let result = exec
            .execute_plan_with_context(
                ActionPlan::new(vec![Action::Type {
                    text: "hi".into(),
                    clear: false,
                }]),
                "type hi",
            )
            .await
            .unwrap();
        assert!(result.completed, "single-plan path starts the guard inert");
        assert_eq!(backend.typed(), vec!["hi".to_string()]);
    }

    #[tokio::test]
    async fn three_step_allow_plan_runs_in_order_and_completes() {
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());

        let plan = ActionPlan::new(vec![
            Action::Focus {
                target: "#/1".into(),
            },
            Action::Type {
                text: "hello".into(),
                clear: false,
            },
            Action::Invoke {
                target: "#/2".into(),
            },
        ]);

        let result = exec.execute_plan(plan).await.unwrap();

        assert!(result.completed, "pure-allow plan must complete");
        assert_eq!(result.outcomes.len(), 3);
        assert!(result
            .outcomes
            .iter()
            .all(|o| matches!(o, StepOutcome::Done { verified: true, .. })));

        // Calls were recorded in plan order.
        assert_eq!(backend.focused_targets(), vec!["#/1".to_string()]);
        assert_eq!(backend.typed(), vec!["hello".to_string()]);
        assert_eq!(backend.invoked(), vec!["#/2".to_string()]);
    }

    #[tokio::test]
    async fn audit_log_records_one_row_per_step() {
        let path = std::env::temp_dir().join(format!(
            "flowrad_exec_audit_{}_{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let audit = AuditLog::open(&path).unwrap();

        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = Executor::new(
            backend,
            CapabilityGate::new(),
            Some(audit),
            KillSwitch::new(),
        );

        let plan = ActionPlan::new(vec![
            Action::Focus {
                target: "#/1".into(),
            },
            Action::Invoke {
                target: "#/2".into(),
            },
        ]);
        let result = exec.execute_plan(plan).await.unwrap();
        assert!(result.completed);

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2, "one audit row per executed step");
        for line in lines {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(v["result"], "ok");
            assert_eq!(v["decisions"][0], "allow");
        }

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn kill_switch_tripped_before_run_aborts() {
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let kill = KillSwitch::new();
        kill.trip();
        let mut exec = Executor::new(backend.clone(), CapabilityGate::new(), None, kill);

        let plan = ActionPlan::new(vec![Action::Invoke {
            target: "#/2".into(),
        }]);
        let result = exec.execute_plan(plan).await.unwrap();

        assert!(!result.completed);
        assert!(matches!(result.outcomes.as_slice(), [StepOutcome::Aborted]));
        // No backend action was performed.
        assert!(backend.invoked().is_empty());
        assert!(backend.focused_targets().is_empty());
    }

    #[tokio::test]
    async fn elevated_app_is_denied_and_no_action_runs() {
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot())
                .elevated(true)
                .build(),
        );
        let mut exec = executor(backend.clone());

        let plan = ActionPlan::new(vec![Action::Invoke {
            target: "#/2".into(),
        }]);
        let result = exec.execute_plan(plan).await.unwrap();

        assert!(!result.completed);
        match result.outcomes.as_slice() {
            [StepOutcome::Denied { reason, .. }] => {
                assert!(reason.contains("elevated"), "reason was: {reason}");
            }
            other => panic!("expected a single Denied outcome, got {other:?}"),
        }
        // Elevation is checked before any action call.
        assert!(backend.invoked().is_empty());
    }

    #[tokio::test]
    async fn stale_target_reground_repairs_the_path() {
        // The plan's target "Next" is not a live path, but grounding maps that
        // name to the Next button (#/2). The re-grounded action then runs.
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());

        let plan = ActionPlan::new(vec![Action::Invoke {
            target: "Next".into(),
        }]);
        let result = exec.execute_plan(plan).await.unwrap();

        assert!(result.completed, "re-grounded target should run");
        assert_eq!(backend.invoked(), vec!["#/2".to_string()]);
    }

    #[tokio::test]
    async fn missing_target_fails_the_step() {
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());

        let plan = ActionPlan::new(vec![Action::Invoke {
            target: "nonexistent widget".into(),
        }]);
        let result = exec.execute_plan(plan).await.unwrap();

        assert!(!result.completed);
        assert!(matches!(
            result.outcomes.as_slice(),
            [StepOutcome::Failed { .. }]
        ));
        assert!(backend.invoked().is_empty());
    }

    #[tokio::test]
    async fn scroll_action_calls_backend_scroll_with_mapped_amount() {
        // #1: `Action::Scroll` is no longer a no-op — it drives the backend `scroll`
        // primitive. A zero/absent amount defaults to a downward nudge; a signed
        // amount passes through (negative = up).
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());
        let plan = ActionPlan::new(vec![
            Action::Scroll {
                target: None,
                amount: 0,
            },
            Action::Scroll {
                target: None,
                amount: -2,
            },
        ]);
        let result = exec.execute_plan(plan).await.unwrap();
        assert!(result.completed, "scroll actions run to completion");
        assert_eq!(
            backend.scrolls(),
            vec![(0, DEFAULT_SCROLL_NOTCHES), (0, -2)],
            "amount 0 defaults to a downward nudge; a signed amount passes through"
        );
    }

    fn snapshot_with_offscreen_result() -> Snapshot {
        // A YouTube-style result present in the tree but scrolled below the fold.
        let mut result = el("#/5", Role::Link, "Eagles - Hotel California");
        result.states = vec![ElementState::Offscreen];
        Snapshot {
            app: "Chrome".into(),
            window_title: "YouTube".into(),
            focused: Some("#/1".into()),
            pointer: None,
            selection_text_len: 0,
            elements: vec![el("#/1", Role::TextField, "Search"), result],
        }
    }

    #[tokio::test]
    async fn offscreen_invoke_scrolls_into_view_before_acting() {
        // #3a: invoking a target marked `offscreen` first brings it into view, so
        // the invoke lands on a real control instead of scrolled-out empty space.
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot_with_offscreen_result())
                .build(),
        );
        let mut exec = executor(backend.clone());
        let plan = ActionPlan::new(vec![Action::Invoke {
            target: "#/5".into(),
        }]);
        let result = exec.execute_plan(plan).await.unwrap();
        assert!(result.completed);
        assert_eq!(
            backend.scrolled_into_view(),
            vec!["#/5".to_string()],
            "the offscreen target is scrolled into view first"
        );
        assert_eq!(backend.invoked(), vec!["#/5".to_string()]);
    }

    #[tokio::test]
    async fn onscreen_invoke_does_not_scroll_into_view() {
        // The offscreen-reach step is inert for an on-screen control: a plain
        // invoke must not pay a spurious scroll-into-view.
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());
        let plan = ActionPlan::new(vec![Action::Invoke {
            target: "#/2".into(),
        }]);
        let result = exec.execute_plan(plan).await.unwrap();
        assert!(result.completed);
        assert!(
            backend.scrolled_into_view().is_empty(),
            "an on-screen target is not scrolled into view"
        );
        assert_eq!(backend.invoked(), vec!["#/2".to_string()]);
    }

    #[test]
    fn scroll_dy_maps_amount_to_notches() {
        assert_eq!(scroll_dy(0), DEFAULT_SCROLL_NOTCHES, "0 defaults to down");
        assert_eq!(scroll_dy(5), 5, "positive passes through (down)");
        assert_eq!(scroll_dy(-3), -3, "negative passes through (up)");
    }

    #[test]
    fn chunk_type_text_short_is_single_chunk() {
        assert_eq!(chunk_type_text(""), vec![""]);
        assert_eq!(chunk_type_text("hello"), vec!["hello"]);
    }

    #[test]
    fn chunk_type_text_splits_long_text_and_rejoins_exactly() {
        let text = "x".repeat(1200) + "\n" + &"y".repeat(1200);
        let chunks = chunk_type_text(&text);
        assert!(
            chunks.len() >= 3,
            "expected multiple chunks: {}",
            chunks.len()
        );
        assert!(chunks.iter().all(|c| c.len() <= TYPE_CHUNK_BYTES));
        // Reassembling the chunks must reproduce the input byte-for-byte.
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn chunk_type_text_never_splits_a_crlf() {
        // A CRLF placed exactly at the 500-byte cap must stay together.
        let mut text = "a".repeat(TYPE_CHUNK_BYTES - 1);
        text.push('\r');
        text.push('\n');
        text.push_str(&"b".repeat(10));
        let chunks = chunk_type_text(&text);
        assert_eq!(chunks.concat(), text);
        for c in &chunks {
            // No chunk ends in a lone '\r' whose '\n' was pushed to the next chunk.
            assert!(!c.ends_with('\r'), "chunk split a CRLF: {c:?}");
        }
    }

    #[test]
    fn chunk_type_text_respects_utf8_boundaries() {
        // Multi-byte chars packed past the cap must never split mid-codepoint
        // (str slicing on a non-boundary would panic — reaching concat() proves it).
        let text = "é".repeat(400); // 800 bytes
        let chunks = chunk_type_text(&text);
        assert!(chunks.len() >= 2);
        assert_eq!(chunks.concat(), text);
    }

    #[tokio::test]
    async fn type_long_text_is_pasted_in_one_shot() {
        // Long text (even single-line) now pastes rather than char-types, to stay
        // clear of the UIA type watchdog. The chunking helper is still exercised by
        // `chunk_type_text` unit tests and the short-text typed path.
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());
        let long = "z".repeat(1300);
        let plan = ActionPlan::new(vec![Action::Type {
            text: long.clone(),
            clear: false,
        }]);
        let result = exec.execute_plan(plan).await.unwrap();
        assert!(result.completed);
        assert_eq!(backend.clipboard_sets(), vec![long]);
        assert!(backend
            .keys()
            .iter()
            .any(|k| k.eq_ignore_ascii_case("ctrl+v")));
        assert!(backend.typed().is_empty(), "long text is pasted, not typed");
    }

    #[tokio::test]
    async fn type_with_clear_selects_all_first() {
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());

        let plan = ActionPlan::new(vec![Action::Type {
            text: "new".into(),
            clear: true,
        }]);
        let result = exec.execute_plan(plan).await.unwrap();

        assert!(result.completed);
        assert_eq!(backend.keys(), vec!["ctrl+a".to_string()]);
        assert_eq!(backend.typed(), vec!["new".to_string()]);
    }

    #[tokio::test]
    async fn stop_ends_plan_as_completed() {
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend);

        let plan = ActionPlan::new(vec![
            Action::Key {
                combo: "ctrl+s".into(),
            },
            Action::Stop,
        ]);
        let result = exec.execute_plan(plan).await.unwrap();

        assert!(result.completed);
        // Only the Key step recorded an outcome; Stop ends without one.
        assert_eq!(result.outcomes.len(), 1);
    }

    #[tokio::test]
    async fn resume_confirm_deny_refuses() {
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());

        let remaining = ActionPlan::new(vec![Action::Invoke {
            target: "#/2".into(),
        }]);
        let result = exec
            .resume_after_user(remaining, UserDecision::ConfirmDeny)
            .await
            .unwrap();

        assert!(!result.completed);
        assert!(matches!(
            result.outcomes.as_slice(),
            [StepOutcome::Denied { .. }]
        ));
        assert!(backend.invoked().is_empty());
    }

    #[tokio::test]
    async fn resume_cancel_aborts() {
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend);

        let remaining = ActionPlan::new(vec![Action::Invoke {
            target: "#/2".into(),
        }]);
        let result = exec
            .resume_after_user(remaining, UserDecision::Cancel)
            .await
            .unwrap();

        assert!(!result.completed);
        assert!(matches!(result.outcomes.as_slice(), [StepOutcome::Aborted]));
    }

    #[tokio::test]
    async fn resume_confirm_allow_runs_preapproved_head() {
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());

        let remaining = ActionPlan::new(vec![Action::Invoke {
            target: "#/2".into(),
        }]);
        let result = exec
            .resume_after_user(remaining, UserDecision::ConfirmAllow)
            .await
            .unwrap();

        assert!(result.completed);
        assert_eq!(backend.invoked(), vec!["#/2".to_string()]);
    }

    #[tokio::test]
    async fn ask_user_action_pauses_the_plan() {
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend);

        let plan = ActionPlan::new(vec![
            Action::AskUser {
                question: "Which file?".into(),
                choices: vec!["A".into(), "B".into()],
            },
            Action::Stop,
        ]);
        let result = exec.execute_plan(plan).await.unwrap();

        assert!(!result.completed);
        match result.outcomes.as_slice() {
            [StepOutcome::NeedsAskUser { prompt, options }] => {
                assert_eq!(prompt, "Which file?");
                assert_eq!(options.len(), 2);
                assert_eq!(options[0].label, "A");
            }
            other => panic!("expected NeedsAskUser, got {other:?}"),
        }
    }

    // --- Destructive-action runtime safety (must-fixes #2, #3, #5) -------------

    fn snapshot_with_delete() -> Snapshot {
        Snapshot {
            app: "Editor".into(),
            window_title: "Untitled".into(),
            focused: Some("#/1".into()),
            pointer: None,
            selection_text_len: 0,
            elements: vec![
                el("#/1", Role::TextField, "Message"),
                el("#/9", Role::Button, "Delete"),
            ],
        }
    }

    #[tokio::test]
    async fn destructive_target_invoke_pauses_for_confirmation() {
        // Invoking a "Delete" control must pause even though the capability gate
        // allows a11y invoke by default: the runtime classifier is the net.
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot_with_delete())
                .build(),
        );
        let mut exec = executor(backend.clone());

        let plan = ActionPlan::new(vec![Action::Invoke {
            target: "#/9".into(),
        }]);
        let result = exec.execute_plan(plan).await.unwrap();

        assert!(!result.completed);
        match result.outcomes.as_slice() {
            [StepOutcome::NeedsConfirm { reason, .. }] => {
                assert!(reason.contains("destructive_target"), "reason: {reason}");
            }
            other => panic!("expected NeedsConfirm, got {other:?}"),
        }
        // Nothing was executed before the user confirmed.
        assert!(backend.invoked().is_empty());
    }

    #[tokio::test]
    async fn destructive_confirm_then_allow_runs_the_same_control() {
        // Full round-trip: pause on the destructive control, then a ConfirmAllow
        // that re-resolves to the SAME (path, name) executes it.
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot_with_delete())
                .build(),
        );
        let mut exec = executor(backend.clone());

        let plan = ActionPlan::new(vec![Action::Invoke {
            target: "#/9".into(),
        }]);
        let paused = exec.execute_plan(plan.clone()).await.unwrap();
        assert!(matches!(
            paused.outcomes.as_slice(),
            [StepOutcome::NeedsConfirm { .. }]
        ));
        assert!(backend.invoked().is_empty());

        let resumed = exec
            .resume_after_user(plan, UserDecision::ConfirmAllow)
            .await
            .unwrap();
        assert!(resumed.completed, "matching pre-approval should execute");
        assert_eq!(backend.invoked(), vec!["#/9".to_string()]);
    }

    #[tokio::test]
    async fn destructive_preapproval_reconfirms_when_target_changed() {
        // The user confirmed a control with a different name than what now sits at
        // the resolved path; the pre-approval must NOT transfer — re-confirm.
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot_with_delete())
                .build(),
        );
        let mut exec = executor(backend.clone());
        // Simulate a stale approval pinned to a different-looking control.
        exec.confirmed_target = Some(("#/9".into(), "Archive".into()));

        let plan = ActionPlan::new(vec![Action::Invoke {
            target: "#/9".into(),
        }]);
        let result = exec
            .resume_after_user(plan, UserDecision::ConfirmAllow)
            .await
            .unwrap();

        assert!(!result.completed, "changed target must re-confirm, not run");
        assert!(matches!(
            result.outcomes.as_slice(),
            [StepOutcome::NeedsConfirm { .. }]
        ));
        assert!(backend.invoked().is_empty());
    }

    #[tokio::test]
    async fn submit_enter_on_destructive_focus_confirms() {
        // Fast-path "submit" -> Key Enter with no target. The executor resolves the
        // FOCUSED control ("Delete") and the classifier forces a confirmation.
        let mut snap = snapshot_with_delete();
        snap.focused = Some("#/9".into());
        let backend = Arc::new(MockBackend::builder().snapshot(snap).build());
        let mut exec = executor(backend.clone());

        let plan = ActionPlan::new(vec![Action::Key {
            combo: "Enter".into(),
        }]);
        let result = exec.execute_plan(plan).await.unwrap();

        assert!(!result.completed);
        assert!(matches!(
            result.outcomes.as_slice(),
            [StepOutcome::NeedsConfirm { .. }]
        ));
        // The key was never pressed.
        assert!(backend.keys().is_empty());
    }

    #[tokio::test]
    async fn submit_enter_on_benign_focus_runs() {
        // Same fast-path Enter, but focused on a plain text field: no confirmation.
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());

        let plan = ActionPlan::new(vec![Action::Key {
            combo: "Enter".into(),
        }]);
        let result = exec.execute_plan(plan).await.unwrap();

        assert!(result.completed);
        assert_eq!(backend.keys(), vec!["Enter".to_string()]);
    }

    // --- Verify-failure halts the plan (must-fix #4) ---------------------------

    #[tokio::test]
    async fn verify_failure_halts_and_skips_later_steps() {
        // Focus #/2 while the (static) snapshot keeps focus on #/1: the focus
        // post-condition fails, so the plan HALTS and the following Invoke #/1
        // never runs.
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());

        let plan = ActionPlan::new(vec![
            Action::Focus {
                target: "#/2".into(),
            },
            Action::Invoke {
                target: "#/1".into(),
            },
        ]);
        let result = exec.execute_plan(plan).await.unwrap();

        assert!(!result.completed);
        assert!(
            matches!(result.outcomes.as_slice(), [StepOutcome::Failed { .. }]),
            "expected a single Failed outcome, got {:?}",
            result.outcomes
        );
        // The focus was attempted but the later invoke was skipped.
        assert_eq!(backend.focused_targets(), vec!["#/2".to_string()]);
        assert!(backend.invoked().is_empty());
    }

    // ---- Script primitives (launch / uri / shell / wait / focus_app / clipboard) ----

    #[test]
    fn clamp_wait_bounds_long_model_waits_but_keeps_short_ones() {
        // A short, deliberate pause is preserved exactly.
        assert_eq!(clamp_wait_ms(0), 0);
        assert_eq!(clamp_wait_ms(800), 800);
        assert_eq!(clamp_wait_ms(WAIT_CEIL_MS), WAIT_CEIL_MS);
        // A multi-second guess ("wait 5000" after a navigation) is capped.
        assert_eq!(clamp_wait_ms(5000), WAIT_CEIL_MS);
        assert_eq!(clamp_wait_ms(15_000), WAIT_CEIL_MS);
        assert_eq!(clamp_wait_ms(u32::MAX), WAIT_CEIL_MS);
        assert!(WAIT_CEIL_MS <= 2500, "ceiling stays a sane sub-3s bound");
    }

    #[tokio::test]
    async fn wait_and_focus_app_are_allowed_and_execute() {
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());
        let plan = ActionPlan::new(vec![
            Action::Wait { ms: 1 },
            Action::FocusApp {
                name: "Chrome".into(),
            },
        ]);
        let result = exec.execute_plan(plan).await.unwrap();
        assert!(result.completed, "wait + focus_app are Allow and complete");
        assert_eq!(backend.focused_apps(), vec!["Chrome".to_string()]);
    }

    #[tokio::test]
    async fn launch_pauses_for_confirmation_and_does_not_run() {
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());
        let plan = ActionPlan::new(vec![Action::Launch {
            target: "spotify".into(),
            origin: Origin::WorldKnowledge,
        }]);
        let result = exec.execute_plan(plan).await.unwrap();
        assert!(!result.completed);
        assert!(matches!(
            result.outcomes.last(),
            Some(StepOutcome::NeedsConfirm { .. })
        ));
        assert!(
            backend.launched().is_empty(),
            "must not launch before confirm"
        );
    }

    #[tokio::test]
    async fn shell_runs_only_after_confirmation() {
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot())
                .shell_output(0, "10.0.0.1")
                .build(),
        );
        let mut exec = executor(backend.clone());
        let shell = Action::Shell {
            command: "ipconfig".into(),
            shell: "cmd".into(),
            origin: Origin::WorldKnowledge,
        };
        // First pass pauses for confirmation without running.
        let paused = exec
            .execute_plan(ActionPlan::new(vec![shell.clone()]))
            .await
            .unwrap();
        assert!(matches!(
            paused.outcomes.last(),
            Some(StepOutcome::NeedsConfirm { .. })
        ));
        assert!(backend.ran_shells().is_empty());
        // Confirming runs it exactly once.
        let done = exec
            .resume_after_user(ActionPlan::new(vec![shell]), UserDecision::ConfirmAllow)
            .await
            .unwrap();
        assert!(done.completed);
        assert_eq!(
            backend.ran_shells(),
            vec![("ipconfig".to_string(), "cmd".to_string())]
        );
    }

    #[tokio::test]
    async fn dangerous_shell_is_denied_before_confirm() {
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());
        let plan = ActionPlan::new(vec![Action::Shell {
            command: "format c: /y".into(),
            shell: "cmd".into(),
            origin: Origin::WorldKnowledge,
        }]);
        let result = exec.execute_plan(plan).await.unwrap();
        assert!(!result.completed);
        assert!(
            matches!(result.outcomes.as_slice(), [StepOutcome::Denied { .. }]),
            "a destructive command is Denied, never offered as a confirm: {:?}",
            result.outcomes
        );
        assert!(backend.ran_shells().is_empty());
    }

    #[tokio::test]
    async fn screen_originated_shell_is_denied() {
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());
        let plan = ActionPlan::new(vec![Action::Shell {
            command: "echo hi".into(),
            shell: "powershell".into(),
            origin: Origin::Screen,
        }]);
        let result = exec.execute_plan(plan).await.unwrap();
        assert!(matches!(
            result.outcomes.as_slice(),
            [StepOutcome::Denied { .. }]
        ));
        assert!(backend.ran_shells().is_empty());
    }

    #[tokio::test]
    async fn dangerous_uri_scheme_is_denied() {
        let backend = Arc::new(MockBackend::builder().snapshot(snapshot()).build());
        let mut exec = executor(backend.clone());
        let plan = ActionPlan::new(vec![Action::Uri {
            uri: "file:///c:/windows/system32/x".into(),
            origin: Origin::WorldKnowledge,
        }]);
        let result = exec.execute_plan(plan).await.unwrap();
        assert!(matches!(
            result.outcomes.as_slice(),
            [StepOutcome::Denied { .. }]
        ));
        assert!(backend.opened_uris().is_empty());
    }

    #[tokio::test]
    async fn shell_echoing_on_screen_text_is_denied() {
        // A snapshot whose control name carries a long token; a shell command that
        // echoes that token is treated as laundered-from-screen and refused.
        let snap = Snapshot {
            app: "App".into(),
            window_title: "W".into(),
            focused: None,
            pointer: None,
            selection_text_len: 0,
            elements: vec![el("#/1", Role::Text, "verysecrettoken_abcdef")],
        };
        let backend = Arc::new(MockBackend::builder().snapshot(snap).build());
        let mut exec = executor(backend.clone());
        let plan = ActionPlan::new(vec![Action::Shell {
            command: "echo verysecrettoken_abcdef".into(),
            shell: "powershell".into(),
            origin: Origin::WorldKnowledge,
        }]);
        let result = exec.execute_plan(plan).await.unwrap();
        assert!(
            matches!(result.outcomes.as_slice(), [StepOutcome::Denied { .. }]),
            "command echoing on-screen text is denied: {:?}",
            result.outcomes
        );
        assert!(backend.ran_shells().is_empty());
    }

    #[tokio::test]
    async fn shell_nonzero_exit_fails_the_plan() {
        let backend = Arc::new(
            MockBackend::builder()
                .snapshot(snapshot())
                .shell_output(1, "not recognized")
                .build(),
        );
        let mut exec = executor(backend.clone());
        let shell = Action::Shell {
            command: "frobnicate".into(),
            shell: "cmd".into(),
            origin: Origin::WorldKnowledge,
        };
        exec.execute_plan(ActionPlan::new(vec![shell.clone()]))
            .await
            .unwrap();
        let done = exec
            .resume_after_user(ActionPlan::new(vec![shell]), UserDecision::ConfirmAllow)
            .await
            .unwrap();
        assert!(!done.completed);
        assert!(matches!(
            done.outcomes.last(),
            Some(StepOutcome::Failed { .. })
        ));
    }
}
