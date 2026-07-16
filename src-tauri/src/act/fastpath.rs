//! Local command grammar — the fast-path that resolves fixed verbs to actions
//! WITHOUT a model round-trip, so only open-ended intents pay the Gemini cost.
//!
//! This is the Talon lesson: a tiny keyword grammar for the highest-frequency
//! commands (copy/paste/cut/undo/redo/select-all/save/new-tab/close-tab/
//! next-field/submit/stop) resolves deterministically in microseconds and never
//! touches the planner. A miss returns `None` and escalates to Gemini.

use super::action::{Action, ActionPlan};

/// Normalize a raw transcript for matching: lowercase, trim, collapse internal
/// whitespace runs to single spaces, and strip trailing sentence punctuation.
fn normalize(transcript: &str) -> String {
    let lowered = transcript.to_lowercase();
    let collapsed = lowered.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .trim_end_matches([' ', '.', ',', '!', '?', ';', ':'])
        .to_string()
}

/// Build a single-key-combo plan.
fn key_plan(combo: &str) -> ActionPlan {
    ActionPlan::new(vec![Action::Key {
        combo: combo.to_string(),
    }])
}

/// Attempts to resolve a transcript locally. `None` means "escalate to the
/// planner (Gemini)".
pub fn resolve(transcript: &str) -> Option<ActionPlan> {
    let phrase = normalize(transcript);

    // Strip a few common filler suffixes so "copy that" == "copy" and
    // "select all text" == "select all".
    let core = strip_filler(phrase.as_str());

    let plan = match core {
        "copy" => key_plan("ctrl+c"),
        "paste" => key_plan("ctrl+v"),
        "cut" => key_plan("ctrl+x"),
        "undo" => key_plan("ctrl+z"),
        "redo" => key_plan("ctrl+y"),
        "select all" => key_plan("ctrl+a"),
        "save" => key_plan("ctrl+s"),
        "new tab" => key_plan("ctrl+t"),
        "close tab" => key_plan("ctrl+w"),
        "next field" => key_plan("Tab"),
        "submit" | "press enter" | "enter" => key_plan("Enter"),
        "stop" | "cancel" => ActionPlan::new(vec![Action::Stop]),
        _ => return None,
    };
    Some(plan)
}

/// Trim a small set of trailing filler words that don't change the verb's
/// meaning, so minor spoken variants collapse onto their canonical command.
fn strip_filler(phrase: &str) -> &str {
    const TRAILING: &[&str] = &[" that", " this", " it", " text", " please", " now"];
    let mut core = phrase;
    // Peel repeatedly so "copy that please" -> "copy".
    loop {
        let mut changed = false;
        for suffix in TRAILING {
            if let Some(stripped) = core.strip_suffix(suffix) {
                core = stripped;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    core
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo_of(plan: &ActionPlan) -> &str {
        match &plan.actions[0] {
            Action::Key { combo } => combo,
            other => panic!("expected key action, got {other:?}"),
        }
    }

    #[test]
    fn resolves_core_verbs_to_exact_combos() {
        let cases = [
            ("copy", "ctrl+c"),
            ("paste", "ctrl+v"),
            ("cut", "ctrl+x"),
            ("undo", "ctrl+z"),
            ("redo", "ctrl+y"),
            ("select all", "ctrl+a"),
            ("save", "ctrl+s"),
            ("new tab", "ctrl+t"),
            ("close tab", "ctrl+w"),
            ("next field", "Tab"),
            ("submit", "Enter"),
            ("press enter", "Enter"),
        ];
        for (transcript, combo) in cases {
            let plan = resolve(transcript).unwrap_or_else(|| panic!("{transcript} should resolve"));
            assert_eq!(plan.actions.len(), 1, "{transcript} is a single action");
            assert_eq!(combo_of(&plan), combo, "{transcript}");
        }
    }

    #[test]
    fn stop_and_cancel_emit_stop_action() {
        assert_eq!(resolve("stop").unwrap().actions, vec![Action::Stop]);
        assert_eq!(resolve("cancel").unwrap().actions, vec![Action::Stop]);
    }

    #[test]
    fn normalizes_case_whitespace_and_punctuation() {
        assert_eq!(combo_of(&resolve("  COPY  ").unwrap()), "ctrl+c");
        assert_eq!(combo_of(&resolve("Paste.").unwrap()), "ctrl+v");
        assert_eq!(combo_of(&resolve("Select   All!").unwrap()), "ctrl+a");
        assert_eq!(combo_of(&resolve("New Tab?").unwrap()), "ctrl+t");
    }

    #[test]
    fn accepts_minor_filler_variants() {
        assert_eq!(combo_of(&resolve("copy that").unwrap()), "ctrl+c");
        assert_eq!(combo_of(&resolve("paste it").unwrap()), "ctrl+v");
        assert_eq!(combo_of(&resolve("select all text").unwrap()), "ctrl+a");
        assert_eq!(combo_of(&resolve("copy that please").unwrap()), "ctrl+c");
        assert_eq!(combo_of(&resolve("save now").unwrap()), "ctrl+s");
    }

    #[test]
    fn unknown_text_escalates_to_none() {
        assert!(resolve("open railway dot app").is_none());
        assert!(resolve("reply saying i'll be late").is_none());
        assert!(resolve("").is_none());
        assert!(resolve("copier").is_none());
    }

    #[test]
    fn resolved_plan_is_full_confidence() {
        assert_eq!(resolve("copy").unwrap().confidence, 1.0);
    }
}
