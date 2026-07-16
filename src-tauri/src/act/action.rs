//! The action schema.
//!
//! The planner (or a local fast-path) emits an [`ActionPlan`]; the executor runs
//! each [`Action`] against the current snapshot, gated by the capability layer.
//! Targets are element paths resolved against the live snapshot at execution
//! time — never pixel coordinates (except an explicit click fallback the
//! executor derives from element bounds).

use serde::{Deserialize, Serialize};

use super::element::ElementPath;

/// A single primitive action. `op` is the discriminant on the wire so the
/// planner can emit `{"op":"focus","target":"#/1/4/2"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Action {
    /// Move focus to an element.
    Focus { target: ElementPath },
    /// Type text at the current caret. `clear` first selects-all + deletes.
    Type {
        text: String,
        #[serde(default)]
        clear: bool,
    },
    /// Invoke an element's default action (press a button, open a menu).
    Invoke { target: ElementPath },
    /// Press a key combo, e.g. `"meta+Enter"`, `"ctrl+c"`.
    Key { combo: String },
    /// Scroll an element (or the focused window if `target` is None).
    Scroll {
        #[serde(default)]
        target: Option<ElementPath>,
        #[serde(default)]
        amount: i32,
    },
    /// Walk a menu by item names, e.g. `["File", "Export", "PDF"]`.
    SelectMenu { path: Vec<String> },
    /// Ask the user to disambiguate; halts the plan until answered.
    AskUser {
        question: String,
        choices: Vec<String>,
    },
    /// Stop the current plan / session.
    Stop,
}

impl Action {
    /// A short, stable kind string for logging and capability lookup.
    pub fn kind(&self) -> &'static str {
        match self {
            Action::Focus { .. } => "focus",
            Action::Type { .. } => "type",
            Action::Invoke { .. } => "invoke",
            Action::Key { .. } => "key",
            Action::Scroll { .. } => "scroll",
            Action::SelectMenu { .. } => "select_menu",
            Action::AskUser { .. } => "ask_user",
            Action::Stop => "stop",
        }
    }

    /// The element path this action targets, if any (for local repair / logging).
    pub fn target(&self) -> Option<&str> {
        match self {
            Action::Focus { target } | Action::Invoke { target } => Some(target),
            Action::Scroll {
                target: Some(target),
                ..
            } => Some(target),
            _ => None,
        }
    }
}

/// A structured plan emitted by the planner or a fast-path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionPlan {
    pub actions: Vec<Action>,
    #[serde(default)]
    pub confidence: f32,
}

impl ActionPlan {
    pub fn new(actions: Vec<Action>) -> Self {
        Self {
            actions,
            confidence: 1.0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_roundtrips_with_op_tag() {
        let a = Action::Focus {
            target: "#/1/4/2".into(),
        };
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(json, r##"{"op":"focus","target":"#/1/4/2"}"##);
        let back: Action = serde_json::from_str(&json).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn type_defaults_clear_false() {
        let a: Action = serde_json::from_str(r#"{"op":"type","text":"hi"}"#).unwrap();
        assert_eq!(
            a,
            Action::Type {
                text: "hi".into(),
                clear: false
            }
        );
    }

    #[test]
    fn plan_parses_from_model_shape() {
        let plan: ActionPlan = serde_json::from_str(
            r##"{"actions":[{"op":"focus","target":"#/1"},{"op":"type","text":"x"},{"op":"invoke","target":"#/2"}],"confidence":0.86}"##,
        )
        .unwrap();
        assert_eq!(plan.actions.len(), 3);
        assert_eq!(plan.actions[2].target(), Some("#/2"));
        assert_eq!(plan.actions[1].kind(), "type");
    }
}
