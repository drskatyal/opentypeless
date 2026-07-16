//! The executor: runs a validated [`ActionPlan`] over an [`AccessibilityBackend`].
//!
//! Per action: capability gate -> kill-switch check -> resolve the target against
//! a FRESH snapshot (re-ground on a stale path) -> execute (prefer a11y invoke)
//! -> verify -> audit. Confirm and ask_user pause the plan and are resumed via
//! [`Executor::resume_after_user`].

use std::sync::Arc;

use super::action::{Action, ActionPlan};
use super::audit::{AuditEntry, AuditLog};
use super::backend::AccessibilityBackend;
use super::capability::{CapabilityGate, Decision};
use super::destructive::{self, Destructive};
use super::element::{ElementPath, Snapshot};
use super::events::AskOption;
use super::grounding::{self, Grounded};
use super::killswitch::KillSwitch;

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
        // A fresh command clears any pre-approval left over from a prior one.
        self.transcript = transcript.to_string();
        self.confirmed_target = None;
        self.run(plan.actions, false).await
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

            match self.step(action, preapproved).await? {
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

        // 4. Resolve the target against the live snapshot; re-ground if stale.
        let action = match self.resolve_target(&action, &snapshot) {
            TargetResolution::Ready(a) => a,
            TargetResolution::Ambiguous(options) => {
                let prompt = format!("Which one? ({})", action.kind());
                self.audit_step(&action, decision, "paused: ask_user");
                return Ok(Flow::Pause(StepOutcome::NeedsAskUser { prompt, options }));
            }
            TargetResolution::Gone => {
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
                if *clear {
                    match guarded!(self.backend.key_combo("ctrl+a")) {
                        Ok(()) => guarded!(self.backend.type_text(text)),
                        err => err,
                    }
                } else {
                    guarded!(self.backend.type_text(text))
                }
            }
            Action::Scroll { .. } => {
                // Best-effort: the backend trait has no cross-platform scroll
                // primitive yet, so treat scroll as a no-op success.
                Ok(())
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
            // TODO(script-primitives): dispatch + verify + injection guards. The
            // next agent wires these to the backend (launch/open_uri/run_shell/
            // focus_app/clipboard), routes each through the shell_policy Deny
            // classifier + capability gate, and adds origin-aware injection
            // guards. Until then they fail closed so the plan halts rather than
            // silently doing nothing.
            Action::Launch { .. }
            | Action::Uri { .. }
            | Action::Shell { .. }
            | Action::Wait { .. }
            | Action::FocusApp { .. }
            | Action::Clipboard { .. } => {
                return Ok(Execution::Failed("not yet wired".into()));
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
        match self.step(resolved, false).await? {
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
}
