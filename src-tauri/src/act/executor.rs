//! The executor: runs a validated [`ActionPlan`] over an [`AccessibilityBackend`].
//!
//! Per action: capability gate -> kill-switch check -> resolve the target against
//! a FRESH snapshot (re-ground on a stale path) -> execute (prefer a11y invoke)
//! -> verify -> audit. Confirm and ask_user pause the plan and are resumed via
//! [`Executor::resume_after_user`]. TODO(act-phase1): the run loop is stubbed;
//! the types/signatures below are the frozen contract.

use std::sync::Arc;

use super::action::{Action, ActionPlan};
use super::audit::AuditLog;
use super::backend::AccessibilityBackend;
use super::capability::CapabilityGate;
use super::events::AskOption;
use super::killswitch::KillSwitch;

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

/// Runs plans over a backend, enforcing the capability gate and kill switch and
/// writing the audit log.
pub struct Executor {
    backend: Arc<dyn AccessibilityBackend>,
    gate: CapabilityGate,
    audit: Option<AuditLog>,
    kill: KillSwitch,
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
        }
    }

    pub fn kill_switch(&self) -> KillSwitch {
        self.kill.clone()
    }

    /// Execute a plan step by step. Returns early (completed=false) on the first
    /// Confirm / ask_user / Deny / Failed / Abort.
    pub async fn execute_plan(&mut self, plan: ActionPlan) -> Result<ExecResult, ExecError> {
        // TODO(act-phase1): the per-action gate/kill/snapshot/execute/verify/audit
        // loop. Stubbed to keep the contract compiling.
        let _ = (&self.backend, &self.gate, &mut self.audit, &self.kill, plan);
        Err(ExecError::Internal("executor not yet implemented".into()))
    }

    /// Resume the remaining plan after a Confirm/ask_user decision.
    pub async fn resume_after_user(
        &mut self,
        remaining: ActionPlan,
        decision: UserDecision,
    ) -> Result<ExecResult, ExecError> {
        let _ = (remaining, decision);
        Err(ExecError::Internal(
            "executor resume not yet implemented".into(),
        ))
    }
}
