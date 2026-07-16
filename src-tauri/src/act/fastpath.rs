//! Local command grammar — the fast-path that resolves fixed verbs to actions
//! WITHOUT a model round-trip, so only open-ended intents pay the Gemini cost.
//!
//! TODO(act-phase0): the keyword+slot grammar for copy/paste/cut/undo/redo/
//! select-all/save/new-tab/close-tab/next-field/submit/stop and app launch,
//! returning `Some(ActionPlan)` on a hit and `None` on a miss. Stub only.

use super::action::ActionPlan;

/// Attempts to resolve a transcript locally. `None` means "escalate to the
/// planner (Gemini)".
pub fn resolve(_transcript: &str) -> Option<ActionPlan> {
    None
}
