//! OS-capability sandbox — the real safety boundary (enforced in Rust, never in
//! the prompt).
//!
//! Every [`Action`] is mapped to exactly one [`Capability`]; the
//! [`CapabilityGate`] then rules on that capability against a fixed default
//! policy table (from the architecture doc's capability table), a per-session
//! grant set, and an optional frontmost-app scope. The LLM cannot "talk its way"
//! past this — a confused or injected plan still hits the same Rust-enforced
//! table. Command-approval UX is secondary; this is the boundary.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::action::{Action, ClipboardOp};

/// An OS capability an action needs. Capabilities are process-enforced; the model
/// never sees or influences the table below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Synthesize typed text / key chords.
    InputKeyboard,
    /// Synthesize mouse movement / clicks.
    InputMouse,
    /// Read the accessibility tree and element values.
    A11yRead,
    /// Invoke accessibility patterns (press buttons, open menus, set focus).
    A11yInvoke,
    /// Read the system clipboard.
    ClipboardRead,
    /// Write / inject into the system clipboard.
    ClipboardWrite,
    /// Drive open/save dialogs over the user's documents.
    FsUserDocs,
    /// Delete / trash / overwrite files — always dangerous.
    FsDestructive,
    /// Open URLs / navigate to external destinations.
    NetNavigate,
    /// Launch / start processes.
    AppLaunch,
    /// Execute an arbitrary shell command — the highest-risk surface. Always
    /// Confirm in v1; a grant can never soften it to Allow.
    ShellExec,
    /// Move / focus / arrange application windows.
    WindowManage,
    /// Shut down / sleep / restart the machine — always dangerous.
    SystemPower,
    /// Capture the screen (opt-in vision fallback).
    VisionCapture,
    /// Control the agent itself (ask the user, stop the session).
    AgentSelf,
}

impl Capability {
    /// Capabilities that can never be upgraded to [`Decision::Allow`], no matter
    /// what the user grants. These are the destructive / system-power surfaces the
    /// architecture doc pins to "deny" — a grant can at most soften them, never
    /// open them.
    fn is_never_allowable(self) -> bool {
        matches!(
            self,
            Capability::FsDestructive | Capability::SystemPower | Capability::ShellExec
        )
    }
}

/// The single capability an action requires.
///
/// Type/Key are keyboard synthesis; Focus/Invoke/SelectMenu drive accessibility
/// invoke patterns; Scroll/Click are pointer (mouse-wheel / mouse-button) input;
/// AskUser/Stop are agent-self control.
pub fn required_capability(action: &Action) -> Capability {
    match action {
        Action::Type { .. } | Action::Key { .. } => Capability::InputKeyboard,
        Action::Focus { .. } | Action::Invoke { .. } | Action::SelectMenu { .. } => {
            Capability::A11yInvoke
        }
        Action::Launch { .. } => Capability::AppLaunch,
        // Scroll synthesizes a mouse wheel; a coordinate Click synthesizes a mouse
        // button. Both are pointer input, allowed by default (like a11y invoke).
        Action::Click { .. } | Action::Scroll { .. } => Capability::InputMouse,
        Action::Uri { .. } => Capability::NetNavigate,
        Action::Shell { .. } => Capability::ShellExec,
        Action::FocusApp { .. } => Capability::WindowManage,
        Action::Wait { .. } => Capability::AgentSelf,
        Action::Clipboard {
            op: ClipboardOp::Get,
            ..
        } => Capability::ClipboardRead,
        Action::Clipboard {
            op: ClipboardOp::Set,
            ..
        } => Capability::ClipboardWrite,
        Action::AskUser { .. } | Action::Stop => Capability::AgentSelf,
    }
}

/// The gate's ruling on an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// Run it without asking.
    Allow,
    /// Run it only after explicit user confirmation.
    Confirm,
    /// Refuse it outright.
    Deny,
}

/// Enforces the capability policy over actions before the executor runs them.
///
/// State:
/// - a fixed default policy table (see [`CapabilityGate::default_decision`]),
/// - a per-session `granted` set that softens a capability one step
///   (Confirm → Allow, or a denied-by-default opt-in like vision Deny → Confirm),
///   except the never-allowable destructive/system capabilities,
/// - an optional frontmost-app allowlist: when set, actionable capabilities are
///   denied unless the frontmost app is on the list. `agent.self` is never
///   scoped away, so Stop / AskUser always work.
#[derive(Debug, Default, Clone)]
pub struct CapabilityGate {
    granted: HashSet<Capability>,
    frontmost_app: Option<String>,
    app_allowlist: Option<HashSet<String>>,
}

impl CapabilityGate {
    /// A gate with safe defaults: input + a11y read/invoke granted;
    /// clipboard/fs.user_docs/net.navigate/app.launch require confirmation;
    /// fs.destructive and system.power denied; vision denied until opted in;
    /// agent.self always allowed. No app scope (all frontmost apps allowed).
    pub fn new() -> Self {
        Self::default()
    }

    /// The fixed default ruling for a capability, before any grant or app scope.
    /// This is the architecture doc's capability table, in Rust.
    fn default_decision(cap: Capability) -> Decision {
        use Capability::*;
        match cap {
            // Session grant — the everyday automation surface.
            InputKeyboard | InputMouse | A11yRead | A11yInvoke => Decision::Allow,
            // Window management is a low-risk convenience — allowed by default.
            WindowManage => Decision::Allow,
            // Explicit / limited — confirm each time until granted for the session.
            ClipboardRead | ClipboardWrite | FsUserDocs | NetNavigate | AppLaunch => {
                Decision::Confirm
            }
            // Shell is confirm-every-time and never grantable up to Allow (pinned
            // by `is_never_allowable`).
            ShellExec => Decision::Confirm,
            // Denied by default; vision is opt-in (a grant softens it to Confirm).
            VisionCapture => Decision::Deny,
            // Never permitted from voice in this build.
            FsDestructive | SystemPower => Decision::Deny,
            // The agent may always control itself.
            AgentSelf => Decision::Allow,
        }
    }

    /// Grant a capability for the current session, softening its default ruling by
    /// one step. Never-allowable capabilities (destructive / system power) are
    /// unaffected — the agent can never grant itself those.
    pub fn grant(&mut self, cap: Capability) {
        self.granted.insert(cap);
    }

    /// Revoke a previously granted capability.
    pub fn revoke(&mut self, cap: Capability) {
        self.granted.remove(&cap);
    }

    /// Set (or clear) the frontmost app used for app-scope checks.
    pub fn set_frontmost_app(&mut self, app: Option<String>) {
        self.frontmost_app = app;
    }

    /// Set (or clear) the app allowlist. When `Some`, actionable capabilities are
    /// denied unless the frontmost app is on the list.
    pub fn set_app_allowlist(&mut self, apps: Option<HashSet<String>>) {
        self.app_allowlist = apps;
    }

    /// Rule on an action.
    pub fn evaluate(&self, action: &Action) -> Decision {
        self.evaluate_capability(required_capability(action))
    }

    /// Rule on a capability directly (used by the action path and by callers that
    /// need to check a capability the action mapping doesn't yet surface, e.g.
    /// clipboard / vision).
    pub fn evaluate_capability(&self, cap: Capability) -> Decision {
        // Agent-self is always permitted and never blocked by app scope, so the
        // kill switch / disambiguation can always run.
        if cap == Capability::AgentSelf {
            return Decision::Allow;
        }

        // App-scope hook: outside the allowlisted frontmost app, refuse to act.
        if let Some(allow) = &self.app_allowlist {
            let in_scope = self
                .frontmost_app
                .as_deref()
                .is_some_and(|app| allow.contains(app));
            if !in_scope {
                return Decision::Deny;
            }
        }

        let base = Self::default_decision(cap);
        self.apply_grant(cap, base)
    }

    /// Soften `base` by one step if the capability has been granted this session,
    /// never touching the never-allowable capabilities.
    fn apply_grant(&self, cap: Capability, base: Decision) -> Decision {
        if !self.granted.contains(&cap) || cap.is_never_allowable() {
            return base;
        }
        match base {
            Decision::Deny => Decision::Confirm,
            Decision::Confirm => Decision::Allow,
            Decision::Allow => Decision::Allow,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_action() -> Action {
        Action::Type {
            text: "hi".into(),
            clear: false,
        }
    }

    use super::super::action::{ClipboardOp, Origin};

    /// Every action kind paired with the decision the default gate should return.
    fn all_action_kinds() -> Vec<(Action, Decision)> {
        vec![
            (type_action(), Decision::Allow),
            (
                Action::Key {
                    combo: "ctrl+c".into(),
                },
                Decision::Allow,
            ),
            (
                Action::Focus {
                    target: "#/1".into(),
                },
                Decision::Allow,
            ),
            (
                Action::Invoke {
                    target: "#/1".into(),
                },
                Decision::Allow,
            ),
            (
                Action::Scroll {
                    target: None,
                    amount: 1,
                },
                Decision::Allow,
            ),
            (
                Action::SelectMenu {
                    path: vec!["File".into()],
                },
                Decision::Allow,
            ),
            (
                Action::AskUser {
                    question: "?".into(),
                    choices: vec![],
                },
                Decision::Allow,
            ),
            (Action::Stop, Decision::Allow),
            // New script primitives.
            (
                Action::Launch {
                    target: "spotify".into(),
                    origin: Origin::default(),
                },
                Decision::Confirm,
            ),
            (
                Action::Uri {
                    uri: "https://example.com".into(),
                    origin: Origin::default(),
                },
                Decision::Confirm,
            ),
            (
                Action::Shell {
                    command: "ipconfig".into(),
                    shell: "powershell".into(),
                    origin: Origin::default(),
                },
                Decision::Confirm,
            ),
            (Action::Wait { ms: 100 }, Decision::Allow),
            (
                Action::FocusApp {
                    name: "Chrome".into(),
                },
                Decision::Allow,
            ),
            (
                Action::Clipboard {
                    op: ClipboardOp::Get,
                    text: String::new(),
                },
                Decision::Confirm,
            ),
            (
                Action::Clipboard {
                    op: ClipboardOp::Set,
                    text: "hi".into(),
                },
                Decision::Confirm,
            ),
        ]
    }

    #[test]
    fn script_primitives_map_to_expected_capability() {
        assert_eq!(
            required_capability(&Action::Launch {
                target: "x".into(),
                origin: Origin::default(),
            }),
            Capability::AppLaunch
        );
        assert_eq!(
            required_capability(&Action::Uri {
                uri: "x".into(),
                origin: Origin::default(),
            }),
            Capability::NetNavigate
        );
        assert_eq!(
            required_capability(&Action::Shell {
                command: "x".into(),
                shell: "cmd".into(),
                origin: Origin::default(),
            }),
            Capability::ShellExec
        );
        assert_eq!(
            required_capability(&Action::FocusApp { name: "x".into() }),
            Capability::WindowManage
        );
        assert_eq!(
            required_capability(&Action::Wait { ms: 1 }),
            Capability::AgentSelf
        );
        assert_eq!(
            required_capability(&Action::Clipboard {
                op: ClipboardOp::Get,
                text: String::new(),
            }),
            Capability::ClipboardRead
        );
        assert_eq!(
            required_capability(&Action::Clipboard {
                op: ClipboardOp::Set,
                text: "x".into(),
            }),
            Capability::ClipboardWrite
        );
    }

    #[test]
    fn every_action_maps_to_expected_capability() {
        assert_eq!(
            required_capability(&type_action()),
            Capability::InputKeyboard
        );
        assert_eq!(
            required_capability(&Action::Key {
                combo: "ctrl+c".into()
            }),
            Capability::InputKeyboard
        );
        assert_eq!(
            required_capability(&Action::Focus {
                target: "#/1".into()
            }),
            Capability::A11yInvoke
        );
        assert_eq!(
            required_capability(&Action::Invoke {
                target: "#/1".into()
            }),
            Capability::A11yInvoke
        );
        assert_eq!(
            required_capability(&Action::Scroll {
                target: None,
                amount: 0
            }),
            Capability::InputMouse
        );
        assert_eq!(
            required_capability(&Action::SelectMenu { path: vec![] }),
            Capability::A11yInvoke
        );
        assert_eq!(
            required_capability(&Action::AskUser {
                question: "?".into(),
                choices: vec![]
            }),
            Capability::AgentSelf
        );
        assert_eq!(required_capability(&Action::Stop), Capability::AgentSelf);
    }

    #[test]
    fn default_gate_rules_each_action_kind() {
        // Input + a11y invoke + agent-self + window-manage are allowed by default;
        // the explicit/limited surfaces (launch, uri, shell, clipboard) confirm.
        let gate = CapabilityGate::new();
        for (action, expected) in all_action_kinds() {
            assert_eq!(
                gate.evaluate(&action),
                expected,
                "action {:?} should rule {:?} by default",
                action.kind(),
                expected
            );
        }
    }

    #[test]
    fn default_policy_table_matches_architecture_doc() {
        let gate = CapabilityGate::new();
        use Capability::*;
        use Decision::*;
        assert_eq!(gate.evaluate_capability(InputKeyboard), Allow);
        assert_eq!(gate.evaluate_capability(InputMouse), Allow);
        assert_eq!(gate.evaluate_capability(A11yRead), Allow);
        assert_eq!(gate.evaluate_capability(A11yInvoke), Allow);
        assert_eq!(gate.evaluate_capability(ClipboardRead), Confirm);
        assert_eq!(gate.evaluate_capability(ClipboardWrite), Confirm);
        assert_eq!(gate.evaluate_capability(FsUserDocs), Confirm);
        assert_eq!(gate.evaluate_capability(NetNavigate), Confirm);
        assert_eq!(gate.evaluate_capability(AppLaunch), Confirm);
        assert_eq!(gate.evaluate_capability(ShellExec), Confirm);
        assert_eq!(gate.evaluate_capability(WindowManage), Allow);
        assert_eq!(gate.evaluate_capability(FsDestructive), Deny);
        assert_eq!(gate.evaluate_capability(SystemPower), Deny);
        assert_eq!(gate.evaluate_capability(VisionCapture), Deny);
        assert_eq!(gate.evaluate_capability(AgentSelf), Allow);
    }

    #[test]
    fn granting_upgrades_confirm_to_allow() {
        let mut gate = CapabilityGate::new();
        assert_eq!(
            gate.evaluate_capability(Capability::ClipboardWrite),
            Decision::Confirm
        );
        gate.grant(Capability::ClipboardWrite);
        assert_eq!(
            gate.evaluate_capability(Capability::ClipboardWrite),
            Decision::Allow
        );
        gate.revoke(Capability::ClipboardWrite);
        assert_eq!(
            gate.evaluate_capability(Capability::ClipboardWrite),
            Decision::Confirm
        );
    }

    #[test]
    fn vision_is_opt_in_only_to_confirm() {
        let mut gate = CapabilityGate::new();
        assert_eq!(
            gate.evaluate_capability(Capability::VisionCapture),
            Decision::Deny
        );
        // Opting in softens Deny to Confirm (per-capture consent), never straight
        // to Allow.
        gate.grant(Capability::VisionCapture);
        assert_eq!(
            gate.evaluate_capability(Capability::VisionCapture),
            Decision::Confirm
        );
    }

    #[test]
    fn destructive_and_system_can_never_be_allowed() {
        let mut gate = CapabilityGate::new();
        gate.grant(Capability::FsDestructive);
        gate.grant(Capability::SystemPower);
        // Even granted, these never reach Allow.
        assert_ne!(
            gate.evaluate_capability(Capability::FsDestructive),
            Decision::Allow
        );
        assert_ne!(
            gate.evaluate_capability(Capability::SystemPower),
            Decision::Allow
        );
        assert_eq!(
            gate.evaluate_capability(Capability::FsDestructive),
            Decision::Deny
        );
        assert_eq!(
            gate.evaluate_capability(Capability::SystemPower),
            Decision::Deny
        );
    }

    #[test]
    fn shell_exec_stays_confirm_even_after_grant() {
        let mut gate = CapabilityGate::new();
        assert_eq!(
            gate.evaluate_capability(Capability::ShellExec),
            Decision::Confirm
        );
        // Shell is never-allowable: a grant cannot soften Confirm to Allow.
        gate.grant(Capability::ShellExec);
        assert_eq!(
            gate.evaluate_capability(Capability::ShellExec),
            Decision::Confirm
        );
    }

    #[test]
    fn app_scope_denies_outside_allowlist_but_not_agent_self() {
        let mut gate = CapabilityGate::new();
        gate.set_app_allowlist(Some(HashSet::from(["Chrome".to_string()])));

        gate.set_frontmost_app(Some("Notepad".to_string()));
        assert_eq!(gate.evaluate(&type_action()), Decision::Deny);
        // Agent-self control still works even out of scope (kill switch path).
        assert_eq!(gate.evaluate(&Action::Stop), Decision::Allow);

        gate.set_frontmost_app(Some("Chrome".to_string()));
        assert_eq!(gate.evaluate(&type_action()), Decision::Allow);

        // No frontmost app known → treated as out of scope.
        gate.set_frontmost_app(None);
        assert_eq!(gate.evaluate(&type_action()), Decision::Deny);
    }
}
