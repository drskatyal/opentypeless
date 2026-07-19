//! The action schema.
//!
//! The planner (or a local fast-path) emits an [`ActionPlan`]; the executor runs
//! each [`Action`] against the current snapshot, gated by the capability layer.
//! Targets are element paths resolved against the live snapshot at execution
//! time — never pixel coordinates (except an explicit click fallback the
//! executor derives from element bounds).

use serde::{Deserialize, Serialize};

use super::element::ElementPath;

/// Where a "script primitive" argument (a launch target, URI, or shell command)
/// came from. This is a provenance hint the safety layer uses: an argument that
/// originated from the trusted task intent is treated differently from one lifted
/// off the screen (which may be attacker-controlled) or from world knowledge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Derived from the user's own spoken task intent (most trusted).
    TaskIntent,
    /// Supplied by the model from its general world knowledge.
    #[default]
    WorldKnowledge,
    /// Lifted from on-screen content (least trusted — possible injection).
    Screen,
}

/// Which clipboard operation a [`Action::Clipboard`] performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardOp {
    /// Read the current clipboard text.
    Get,
    /// Overwrite the clipboard with the provided text.
    Set,
}

/// The default shell for [`Action::Shell`] when the planner omits one.
fn default_shell() -> String {
    "powershell".into()
}

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
        /// Schema-only-enforced clients (Cerebras json_object) sometimes omit this;
        /// default to no preset choices (a free-form question) rather than failing
        /// the whole plan parse with "missing field `choices`".
        #[serde(default)]
        choices: Vec<String>,
    },
    /// Launch / start an application or executable by name or path.
    ///
    /// Schema-only-enforced clients (Cerebras json_object) sometimes emit the
    /// executable under `command` (mirroring `shell`) or `name`/`app` rather than
    /// `target`; accept those aliases so the whole plan doesn't fail to parse with
    /// "missing field `target`".
    Launch {
        #[serde(alias = "command", alias = "name", alias = "app")]
        target: String,
        #[serde(default)]
        origin: Origin,
    },
    /// Open a URI (a URL or app scheme) via the OS handler.
    Uri {
        uri: String,
        #[serde(default)]
        origin: Origin,
    },
    /// Run a shell command. Always the highest-risk primitive.
    Shell {
        command: String,
        #[serde(default = "default_shell")]
        shell: String,
        #[serde(default)]
        origin: Origin,
    },
    /// Click at an absolute screen coordinate (logical pixels). Emitted only by
    /// the `vision` plan mode, whose grounding is the screenshot rather than the
    /// accessibility tree; every other mode targets element paths.
    Click { x: i32, y: i32 },
    /// Pause the plan for a fixed number of milliseconds.
    Wait { ms: u32 },
    /// Bring a named application's window to the foreground.
    ///
    /// Schema-only-enforced clients (Cerebras json_object) frequently emit the
    /// app name under `target` (mirroring `launch`/`focus`) rather than `name`;
    /// accept both so the whole plan doesn't fail to parse with
    /// "missing field `name`".
    FocusApp {
        #[serde(alias = "target")]
        name: String,
    },
    /// Read or write the system clipboard.
    ///
    /// The clipboard sub-op is serialized as `clip_op` rather than `op`: the
    /// enum's own discriminant already occupies the `op` key, so reusing it here
    /// would emit a duplicate JSON key and break the roundtrip.
    Clipboard {
        #[serde(rename = "clip_op")]
        op: ClipboardOp,
        #[serde(default)]
        text: String,
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
            Action::Launch { .. } => "launch",
            Action::Uri { .. } => "uri",
            Action::Shell { .. } => "shell",
            Action::Click { .. } => "click",
            Action::Wait { .. } => "wait",
            Action::FocusApp { .. } => "focus_app",
            Action::Clipboard { .. } => "clipboard",
            Action::Stop => "stop",
        }
    }

    /// The element path this action targets, if any (for local repair / logging).
    ///
    /// The new script primitives (Launch/Uri/Shell/Wait/FocusApp/Clipboard) do
    /// not act on an accessibility element path, so they return `None`.
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

    /// The provenance of this action's argument, for the script primitives that
    /// carry one (Launch/Uri/Shell). All other actions return `None`.
    pub fn origin(&self) -> Option<Origin> {
        match self {
            Action::Launch { origin, .. }
            | Action::Uri { origin, .. }
            | Action::Shell { origin, .. } => Some(*origin),
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

    #[test]
    fn origin_defaults_to_world_knowledge() {
        assert_eq!(Origin::default(), Origin::WorldKnowledge);
    }

    fn roundtrip(a: &Action) -> Action {
        let json = serde_json::to_string(a).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn launch_roundtrips_and_defaults_origin() {
        let a: Action = serde_json::from_str(r#"{"op":"launch","target":"spotify"}"#).unwrap();
        assert_eq!(
            a,
            Action::Launch {
                target: "spotify".into(),
                origin: Origin::WorldKnowledge,
            }
        );
        assert_eq!(a.kind(), "launch");
        assert_eq!(a.origin(), Some(Origin::WorldKnowledge));
        assert_eq!(a.target(), None);
        assert_eq!(roundtrip(&a), a);

        let explicit: Action =
            serde_json::from_str(r#"{"op":"launch","target":"notepad","origin":"task_intent"}"#)
                .unwrap();
        assert_eq!(explicit.origin(), Some(Origin::TaskIntent));
        assert_eq!(roundtrip(&explicit), explicit);
    }

    #[test]
    fn uri_roundtrips_and_defaults_origin() {
        let a: Action =
            serde_json::from_str(r#"{"op":"uri","uri":"https://example.com"}"#).unwrap();
        assert_eq!(
            a,
            Action::Uri {
                uri: "https://example.com".into(),
                origin: Origin::WorldKnowledge,
            }
        );
        assert_eq!(a.kind(), "uri");
        assert_eq!(a.origin(), Some(Origin::WorldKnowledge));
        assert_eq!(roundtrip(&a), a);

        let from_screen: Action =
            serde_json::from_str(r#"{"op":"uri","uri":"file:///etc","origin":"screen"}"#).unwrap();
        assert_eq!(from_screen.origin(), Some(Origin::Screen));
    }

    #[test]
    fn shell_roundtrips_defaults_shell_and_origin() {
        let a: Action = serde_json::from_str(r#"{"op":"shell","command":"ipconfig"}"#).unwrap();
        assert_eq!(
            a,
            Action::Shell {
                command: "ipconfig".into(),
                shell: "powershell".into(),
                origin: Origin::WorldKnowledge,
            }
        );
        assert_eq!(a.kind(), "shell");
        assert_eq!(a.origin(), Some(Origin::WorldKnowledge));
        assert_eq!(roundtrip(&a), a);

        let cmd: Action = serde_json::from_str(
            r#"{"op":"shell","command":"dir","shell":"cmd","origin":"task_intent"}"#,
        )
        .unwrap();
        assert_eq!(
            cmd,
            Action::Shell {
                command: "dir".into(),
                shell: "cmd".into(),
                origin: Origin::TaskIntent,
            }
        );
    }

    #[test]
    fn click_roundtrips() {
        let a: Action = serde_json::from_str(r#"{"op":"click","x":420,"y":137}"#).unwrap();
        assert_eq!(a, Action::Click { x: 420, y: 137 });
        assert_eq!(a.kind(), "click");
        assert_eq!(a.origin(), None);
        assert_eq!(a.target(), None);
        assert_eq!(roundtrip(&a), a);
    }

    #[test]
    fn wait_roundtrips() {
        let a: Action = serde_json::from_str(r#"{"op":"wait","ms":250}"#).unwrap();
        assert_eq!(a, Action::Wait { ms: 250 });
        assert_eq!(a.kind(), "wait");
        assert_eq!(a.origin(), None);
        assert_eq!(a.target(), None);
        assert_eq!(roundtrip(&a), a);
    }

    #[test]
    fn focus_app_roundtrips() {
        let a: Action = serde_json::from_str(r#"{"op":"focus_app","name":"Chrome"}"#).unwrap();
        assert_eq!(
            a,
            Action::FocusApp {
                name: "Chrome".into()
            }
        );
        assert_eq!(a.kind(), "focus_app");
        assert_eq!(a.origin(), None);
        assert_eq!(roundtrip(&a), a);
    }

    #[test]
    fn focus_app_accepts_target_alias() {
        // Cerebras json_object often emits `target` instead of `name`; the alias
        // keeps the whole plan from failing to parse.
        let a: Action = serde_json::from_str(r#"{"op":"focus_app","target":"spotify"}"#).unwrap();
        assert_eq!(
            a,
            Action::FocusApp {
                name: "spotify".into()
            }
        );
        assert_eq!(roundtrip(&a), a);
    }

    #[test]
    fn launch_accepts_command_name_and_app_aliases() {
        // Cerebras json_object emits the executable under `command` (mirroring
        // `shell`), `name`, or `app` instead of `target`; the aliases keep the plan
        // from failing to parse with "missing field `target`".
        for raw in [
            r#"{"op":"launch","command":"winword"}"#,
            r#"{"op":"launch","name":"winword"}"#,
            r#"{"op":"launch","app":"winword"}"#,
            r#"{"op":"launch","target":"winword"}"#,
        ] {
            let a: Action = serde_json::from_str(raw).unwrap_or_else(|e| panic!("{raw}: {e}"));
            assert_eq!(
                a,
                Action::Launch {
                    target: "winword".into(),
                    origin: Origin::default(),
                },
                "parsed from {raw}"
            );
        }
    }

    #[test]
    fn clipboard_roundtrips_get_and_set() {
        let get: Action = serde_json::from_str(r#"{"op":"clipboard","clip_op":"get"}"#).unwrap();
        assert_eq!(
            get,
            Action::Clipboard {
                op: ClipboardOp::Get,
                text: String::new(),
            }
        );
        assert_eq!(get.kind(), "clipboard");
        assert_eq!(get.origin(), None);
        assert_eq!(get.target(), None);
        assert_eq!(roundtrip(&get), get);

        let set: Action =
            serde_json::from_str(r#"{"op":"clipboard","clip_op":"set","text":"hello"}"#).unwrap();
        assert_eq!(
            set,
            Action::Clipboard {
                op: ClipboardOp::Set,
                text: "hello".into(),
            }
        );
        assert_eq!(roundtrip(&set), set);
    }
}
