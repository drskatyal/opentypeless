//! How Act perceives the screen when planning a turn.
//!
//! The three modes share the same Conductor, sub-agent lanes, safety layer and
//! closed loop; only *grounding + execution* differ. See
//! `docs/act-screen-aware-design.md`.

/// The perception mode for an Act planning turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanMode {
    /// Accessibility-tree snapshot only; actions target element paths. The fast,
    /// precise default that works when the tree is good.
    #[default]
    Tree,
    /// Accessibility tree **plus** a screenshot with Set-of-Marks (numbered boxes
    /// drawn from element `Bounds`); the model picks a mark, resolved back to an
    /// element path. Most accurate.
    Hybrid,
    /// Screenshot only; the model returns coordinate actions ([`super::action::Action::Click`]).
    /// The fallback for apps with no usable tree (games, canvas, remote desktop).
    Vision,
}

impl PlanMode {
    /// Parse the persisted `act_plan_mode` config string. Unknown / empty values
    /// fall back to [`PlanMode::Tree`] so a bad config never disables Act.
    pub fn from_config(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "hybrid" => PlanMode::Hybrid,
            "vision" => PlanMode::Vision,
            _ => PlanMode::Tree,
        }
    }

    /// The stable config/wire string for this mode.
    pub fn as_str(self) -> &'static str {
        match self {
            PlanMode::Tree => "tree",
            PlanMode::Hybrid => "hybrid",
            PlanMode::Vision => "vision",
        }
    }

    /// Whether this mode needs a screenshot (so the Conductor should capture one
    /// before planning). `Tree` never does.
    pub fn needs_screenshot(self) -> bool {
        matches!(self, PlanMode::Hybrid | PlanMode::Vision)
    }

    /// Whether this mode still uses the accessibility tree for grounding. `Vision`
    /// does not; the other two do.
    pub fn uses_tree(self) -> bool {
        matches!(self, PlanMode::Tree | PlanMode::Hybrid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_modes_and_defaults_unknown_to_tree() {
        assert_eq!(PlanMode::from_config("tree"), PlanMode::Tree);
        assert_eq!(PlanMode::from_config("Hybrid"), PlanMode::Hybrid);
        assert_eq!(PlanMode::from_config(" VISION "), PlanMode::Vision);
        assert_eq!(PlanMode::from_config("nonsense"), PlanMode::Tree);
        assert_eq!(PlanMode::from_config(""), PlanMode::Tree);
        assert_eq!(PlanMode::default(), PlanMode::Tree);
    }

    #[test]
    fn roundtrips_via_as_str() {
        for m in [PlanMode::Tree, PlanMode::Hybrid, PlanMode::Vision] {
            assert_eq!(PlanMode::from_config(m.as_str()), m);
        }
    }

    #[test]
    fn capability_flags() {
        assert!(!PlanMode::Tree.needs_screenshot());
        assert!(PlanMode::Hybrid.needs_screenshot());
        assert!(PlanMode::Vision.needs_screenshot());
        assert!(PlanMode::Tree.uses_tree());
        assert!(PlanMode::Hybrid.uses_tree());
        assert!(!PlanMode::Vision.uses_tree());
    }
}
