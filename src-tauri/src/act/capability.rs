//! OS-capability sandbox — the real safety boundary (enforced in Rust, never in
//! the prompt).
//!
//! TODO(act-phase0): full capability set, the Action→Capability mapping, the
//! grant/confirm/deny policy table, optional app-scope, and the destructive
//! classifier. Stub only.

use super::action::Action;

/// The gate's ruling on an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Confirm,
    Deny,
}

/// Enforces the capability policy over actions before the executor runs them.
#[derive(Debug, Default)]
pub struct CapabilityGate;

impl CapabilityGate {
    pub fn new() -> Self {
        Self
    }

    /// Placeholder policy — replaced in Phase 0.
    pub fn evaluate(&self, _action: &Action) -> Decision {
        Decision::Confirm
    }
}
