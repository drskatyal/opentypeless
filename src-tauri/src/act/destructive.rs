//! Runtime destructive-action classifier — the last-line, in-executor safety net.
//!
//! The planner's policy check and the [`CapabilityGate`](super::capability) both
//! run earlier, but a fast-path Enter/Space press or a re-grounded target can
//! still land on a destructive control WITHOUT a model round-trip. This pure,
//! testable classifier runs INSIDE the executor after the target is resolved, so
//! it sees the real control name the user is about to activate and can force an
//! explicit confirmation before an irreversible side effect happens.
//!
//! Only "activating" actions matter — pressing a button ([`Action::Invoke`]) or a
//! bare Enter/Space "click" on the focused control ([`Action::Key`]). Focusing,
//! typing, scrolling, and asking never trigger a side effect on their own and are
//! always allowed through here (the capability gate still governs them).

use super::action::Action;

/// Words that mark a destructive / irreversible / high-consequence intent.
///
/// Shared with the planner's defense-in-depth policy check (see
/// [`planner`](super::planner)) so the two lists can never drift apart.
pub const DESTRUCTIVE_WORDS: &[&str] = &[
    "delete",
    "remove",
    "discard",
    "trash",
    "empty",
    "erase",
    "destroy",
    "send",
    "submit",
    "pay",
    "purchase",
    "buy",
    "transfer",
    "checkout",
    "uninstall",
    "format",
    "reset",
    "clear",
    "overwrite",
    "quit",
    "shutdown",
    "sign out",
    "log out",
    "signout",
    "logout",
    "revoke",
    "unsubscribe",
    "permanently",
    "block",
    "ban",
];

/// Bare activator words — controls whose only job is to *confirm* a pending
/// action ("Yes", "OK", "Continue"). Activating one is dangerous only when the
/// surrounding spoken intent is itself destructive, so these gate on the
/// transcript hint rather than on the control name alone.
const CONFIRM_ACTIVATORS: &[&str] = &[
    "yes", "ok", "okay", "confirm", "continue", "proceed", "apply",
];

/// The classifier's verdict for a single action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destructive {
    /// Safe to run without an extra confirmation.
    Allow,
    /// Pause and require explicit user confirmation; carries a short reason code
    /// suitable for the audit log and the `Confirm` event.
    Confirm(String),
}

/// Classify one resolved action against the destructive-safety policy.
///
/// - `action` — the action about to run (already re-grounded to a live target).
/// - `resolved_name` / `resolved_desc` — the accessibility name/description of the
///   control this action will activate (for [`Action::Key`] this is the FOCUSED
///   element, resolved by the executor from the fresh snapshot).
/// - `transcript_hint` — the trusted spoken transcript that produced the plan, used
///   only to disambiguate bare confirm-activators ("Yes"/"OK").
///
/// Returns [`Destructive::Confirm`] when the action should pause for the user, and
/// [`Destructive::Allow`] otherwise. Non-activating actions always return `Allow`.
pub fn classify(
    action: &Action,
    resolved_name: &str,
    resolved_desc: &str,
    transcript_hint: &str,
) -> Destructive {
    // Only actions that actually activate a control can cause an irreversible
    // side effect; everything else passes through.
    if !is_activating(action) {
        return Destructive::Allow;
    }

    // 1. The control the user is about to activate is itself destructive.
    let haystack = format!("{resolved_name} {resolved_desc}");
    if contains_destructive(&haystack) {
        return Destructive::Confirm(format!("destructive_target:{resolved_name}"));
    }

    // 2. A bare confirm-activator ("Yes"/"OK"/"Continue") is dangerous only when
    //    the spoken intent that led here was itself destructive.
    if is_confirm_activator(resolved_name) && contains_destructive(transcript_hint) {
        return Destructive::Confirm("confirm_activator_with_destructive_intent".into());
    }

    Destructive::Allow
}

/// True when `action` actually *activates* a control — invoking it, or a bare
/// Enter/Space "click" on the focused control. Focus/Type/Scroll/SelectMenu/
/// AskUser/Stop and any modified key chord are not activating here.
fn is_activating(action: &Action) -> bool {
    match action {
        Action::Invoke { .. } => true,
        Action::Key { combo } => is_activation_key(combo),
        _ => false,
    }
}

/// Parse a key combo string and report whether it is a bare Enter or Space press —
/// the two keys that "click" a focused control. A modified chord (`ctrl+enter`,
/// `shift+space`, …) is deliberately NOT treated as an activation here: it usually
/// means something app-specific rather than "press the focused button".
fn is_activation_key(combo: &str) -> bool {
    let mut parts = combo
        .split('+')
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty());
    let Some(key) = parts.next() else {
        return false;
    };
    // A second segment means a modifier is present -> not a bare activation.
    if parts.next().is_some() {
        return false;
    }
    matches!(key.as_str(), "enter" | "return" | "space" | "spacebar")
}

/// True if `text` (lowercased) word-or-substring matches any destructive word.
fn contains_destructive(text: &str) -> bool {
    let t = text.to_lowercase();
    DESTRUCTIVE_WORDS.iter().any(|w| t.contains(w))
}

/// True if `name` is a bare confirm-activator, by whole-name or whole-word match.
fn is_confirm_activator(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    CONFIRM_ACTIVATORS
        .iter()
        .any(|w| n == *w || n.split_whitespace().any(|tok| tok == *w))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invoke() -> Action {
        Action::Invoke {
            target: "#/1".into(),
        }
    }

    fn key(combo: &str) -> Action {
        Action::Key {
            combo: combo.into(),
        }
    }

    // --- Branch 1: destructive target name ------------------------------------

    #[test]
    fn destructive_target_name_confirms() {
        assert_eq!(
            classify(&invoke(), "Delete", "", ""),
            Destructive::Confirm("destructive_target:Delete".into())
        );
        // Case-insensitive and works on the description too.
        assert_eq!(
            classify(&invoke(), "Trash", "", ""),
            Destructive::Confirm("destructive_target:Trash".into())
        );
        assert!(matches!(
            classify(&invoke(), "Do it", "permanently erase everything", ""),
            Destructive::Confirm(_)
        ));
        // "Send" / "Submit" are destructive.
        assert!(matches!(
            classify(&invoke(), "Send", "", ""),
            Destructive::Confirm(_)
        ));
    }

    #[test]
    fn benign_target_is_allowed() {
        assert_eq!(classify(&invoke(), "Next", "", ""), Destructive::Allow);
        assert_eq!(
            classify(&invoke(), "Save", "", "delete"),
            Destructive::Allow
        );
    }

    // --- Branch 2: confirm-activator + destructive transcript -----------------

    #[test]
    fn confirm_activator_with_destructive_intent_confirms() {
        assert_eq!(
            classify(&invoke(), "Yes", "", "delete the whole folder"),
            Destructive::Confirm("confirm_activator_with_destructive_intent".into())
        );
        assert_eq!(
            classify(&invoke(), "OK", "", "permanently remove my account"),
            Destructive::Confirm("confirm_activator_with_destructive_intent".into())
        );
    }

    #[test]
    fn confirm_activator_without_destructive_intent_is_allowed() {
        // "Yes" with a harmless transcript is fine.
        assert_eq!(
            classify(&invoke(), "Yes", "", "reply that i'll be there"),
            Destructive::Allow
        );
        // Empty transcript never trips branch 2.
        assert_eq!(classify(&invoke(), "Continue", "", ""), Destructive::Allow);
    }

    // --- Enter / Space activation on a focused control ------------------------

    #[test]
    fn enter_or_space_on_destructive_focus_confirms() {
        assert!(matches!(
            classify(&key("Enter"), "Delete", "", ""),
            Destructive::Confirm(_)
        ));
        assert!(matches!(
            classify(&key("space"), "Uninstall", "", ""),
            Destructive::Confirm(_)
        ));
    }

    #[test]
    fn enter_on_benign_focus_is_allowed() {
        assert_eq!(
            classify(&key("Enter"), "Message", "", ""),
            Destructive::Allow
        );
    }

    #[test]
    fn modified_chord_is_not_an_activation() {
        // ctrl+enter / shift+space are not bare activations, so even a destructive
        // focus name passes through here (the gate still governs the keypress).
        assert_eq!(
            classify(&key("ctrl+enter"), "Delete", "", ""),
            Destructive::Allow
        );
        assert_eq!(
            classify(&key("shift+space"), "Delete", "", ""),
            Destructive::Allow
        );
        assert_eq!(
            classify(&key("ctrl+c"), "Delete", "", ""),
            Destructive::Allow
        );
    }

    // --- Non-activating pass-through ------------------------------------------

    #[test]
    fn non_activating_actions_pass_through() {
        let focus = Action::Focus {
            target: "#/1".into(),
        };
        let type_ = Action::Type {
            text: "delete everything".into(),
            clear: false,
        };
        let scroll = Action::Scroll {
            target: None,
            amount: 1,
        };
        // Even with a destructive name/description, non-activating actions are
        // never classified as needing confirmation.
        assert_eq!(
            classify(&focus, "Delete", "destroy", "delete"),
            Destructive::Allow
        );
        assert_eq!(classify(&type_, "Delete", "", "delete"), Destructive::Allow);
        assert_eq!(
            classify(&scroll, "Delete", "", "delete"),
            Destructive::Allow
        );
        assert_eq!(
            classify(&Action::Stop, "Delete", "", "delete"),
            Destructive::Allow
        );
    }

    #[test]
    fn activation_key_parsing() {
        assert!(is_activation_key("Enter"));
        assert!(is_activation_key("return"));
        assert!(is_activation_key(" space "));
        assert!(is_activation_key("Spacebar"));
        assert!(!is_activation_key("ctrl+enter"));
        assert!(!is_activation_key("Tab"));
        assert!(!is_activation_key("ctrl+c"));
        assert!(!is_activation_key(""));
    }
}
