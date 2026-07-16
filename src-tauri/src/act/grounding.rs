//! Grounding — resolve a spoken target to a concrete element using the snapshot,
//! without vision.
//!
//! TODO(act-phase0): the ordered resolution stack (deictic/focus-relative →
//! role+name → ordinal → state), returning a unique match, an ambiguous set for
//! numbered-overlay disambiguation, or nothing. Stub only.

use super::element::{ElementPath, Snapshot};

/// The outcome of trying to resolve a spoken reference to an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Grounded {
    /// A single confident match.
    One(ElementPath),
    /// Several plausible matches — disambiguate with a numbered overlay.
    Ambiguous(Vec<ElementPath>),
    /// No match.
    None,
}

/// Resolve a spoken phrase against the snapshot. Placeholder — replaced in
/// Phase 0.
pub fn resolve(_snapshot: &Snapshot, _phrase: &str) -> Grounded {
    Grounded::None
}
