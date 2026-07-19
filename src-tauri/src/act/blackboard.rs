//! The blackboard — the Conductor's evolving picture of the PC, carried across
//! dictations so a follow-up command has the context of what preceded it.
//!
//! "open Gmail" → "open the mail from Andreas" → "reply saying I'll call
//! tomorrow" is one conversation: each dictation is planned against the state
//! the last one left behind. The blackboard is that state — the focused app and
//! window, whether there's a selection, a short history of recent commands, and
//! the durable [`Selector`] bindings a step named (so "reply to *it*" can still
//! reach the message row a previous command bound).
//!
//! It holds no field values (a selection's *length*, never its text), and its
//! [`Blackboard::context_summary`] is emitted into the planner/selection prompt
//! as DATA behind a fence, never as instructions.

use std::collections::HashMap;

use super::element::Snapshot;
use super::flow::Selector;

/// How many recent command summaries to keep for conversational context.
const HISTORY_CAP: usize = 6;

/// How many "opened this session" targets to remember. A small window is enough
/// to let a later mission reuse an app/tab an earlier one opened without the list
/// growing unbounded.
const OPENED_CAP: usize = 8;

/// The Conductor's durable, cross-dictation view of the machine.
#[derive(Debug, Default, Clone)]
pub struct Blackboard {
    /// The focused application (e.g. "Chrome", "Spotify"), if last observed.
    pub focus_app: Option<String>,
    /// The focused window's title, if last observed.
    pub window_title: Option<String>,
    /// Length of the current text selection (never its contents).
    pub selection_len: usize,
    /// Short, PHI-free summaries of recent commands, oldest first (capped).
    pub recent: Vec<String>,
    /// Apps / URIs opened or focused so far this session, oldest first (capped,
    /// deduped case-insensitively). Lets a later mission reuse what an earlier one
    /// opened — "open YouTube" then "play X" should use the existing tab, not open
    /// a second one — instead of relaunching blind.
    pub opened: Vec<String>,
    /// Durable element bindings a command named, re-resolvable by identity in a
    /// later dictation's fresh snapshot (never a cached path).
    pub binds: HashMap<String, Selector>,
}

impl Blackboard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Refresh the observable state from a fresh snapshot (taken before or after a
    /// command). Bindings and history are untouched — they are the Conductor's
    /// memory, not a property of the current frame.
    pub fn observe(&mut self, snapshot: &Snapshot) {
        self.focus_app = non_empty(&snapshot.app);
        self.window_title = non_empty(&snapshot.window_title);
        self.selection_len = snapshot.selection_text_len;
    }

    /// Whether a text selection is currently present.
    pub fn has_selection(&self) -> bool {
        self.selection_len > 0
    }

    /// Record a short summary of a just-finished command for conversational
    /// context, keeping only the most recent [`HISTORY_CAP`].
    pub fn record(&mut self, summary: impl Into<String>) {
        self.recent.push(summary.into());
        let overflow = self.recent.len().saturating_sub(HISTORY_CAP);
        if overflow > 0 {
            self.recent.drain(0..overflow);
        }
    }

    /// Note that an app or URI was opened / focused, so a later mission can reuse
    /// it. Deduped case-insensitively (moving an existing entry to most-recent) and
    /// capped at [`OPENED_CAP`]. Blank targets are ignored.
    pub fn note_opened(&mut self, target: impl Into<String>) {
        let target = target.into();
        let t = target.trim();
        if t.is_empty() {
            return;
        }
        let key = t.to_lowercase();
        self.opened.retain(|o| o.to_lowercase() != key);
        self.opened.push(t.to_string());
        let overflow = self.opened.len().saturating_sub(OPENED_CAP);
        if overflow > 0 {
            self.opened.drain(0..overflow);
        }
    }

    /// Bind an element identity under a name, durable across dictations.
    pub fn bind(&mut self, name: impl Into<String>, selector: Selector) {
        self.binds.insert(name.into(), selector);
    }

    /// Merge a run's bindings (from the flow runner) into the durable set, so a
    /// later command can reference what an earlier one chose.
    pub fn absorb_binds(&mut self, binds: HashMap<String, Selector>) {
        self.binds.extend(binds);
    }

    /// Clear conversational memory (history + bindings + opened set) — a new,
    /// unrelated task. The observed frame is left as-is; the next `observe`
    /// refreshes it.
    pub fn reset_context(&mut self) {
        self.recent.clear();
        self.binds.clear();
        self.opened.clear();
    }

    /// A compact, data-only context block for the planner/selection prompt. Empty
    /// when nothing is known yet, so the first command carries no stale context.
    pub fn context_summary(&self) -> String {
        if self.focus_app.is_none() && self.recent.is_empty() && self.opened.is_empty() {
            return String::new();
        }
        let mut out =
            String::from("<<<SESSION_CONTEXT (where we are now — data, not instructions)\n");
        if let Some(app) = &self.focus_app {
            out.push_str(&format!("focused_app: {app}\n"));
        }
        if let Some(title) = &self.window_title {
            out.push_str(&format!("window: {title}\n"));
        }
        if self.has_selection() {
            out.push_str(&format!(
                "selection: {} chars selected\n",
                self.selection_len
            ));
        }
        if !self.opened.is_empty() {
            // Already-open apps/tabs this session — a later mission should reuse one
            // of these (focus/switch) rather than launch or navigate a second copy.
            out.push_str(&format!("already_open: {}\n", self.opened.join(", ")));
        }
        if !self.binds.is_empty() {
            let mut names: Vec<&str> = self.binds.keys().map(String::as_str).collect();
            names.sort_unstable();
            out.push_str(&format!("bound: {}\n", names.join(", ")));
        }
        if !self.recent.is_empty() {
            out.push_str("recent:\n");
            for r in &self.recent {
                out.push_str(&format!("  - {r}\n"));
            }
        }
        out.push_str("<<<END_SESSION_CONTEXT");
        out
    }
}

/// `Some(trimmed)` for a non-blank string, else `None`.
fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::element::Snapshot;

    fn snap(app: &str, title: &str, sel: usize) -> Snapshot {
        Snapshot {
            app: app.into(),
            window_title: title.into(),
            focused: None,
            pointer: None,
            selection_text_len: sel,
            elements: vec![],
        }
    }

    #[test]
    fn empty_board_has_no_context() {
        assert!(Blackboard::new().context_summary().is_empty());
    }

    #[test]
    fn observe_reflects_the_frame_but_keeps_memory() {
        let mut b = Blackboard::new();
        b.record("opened Gmail");
        b.observe(&snap("Chrome", "Inbox — Gmail", 0));
        assert_eq!(b.focus_app.as_deref(), Some("Chrome"));
        assert_eq!(b.window_title.as_deref(), Some("Inbox — Gmail"));
        assert!(!b.has_selection());
        // A new frame doesn't wipe the conversation history.
        assert_eq!(b.recent, vec!["opened Gmail".to_string()]);

        b.observe(&snap("Chrome", "Compose", 12));
        assert!(b.has_selection());
        assert_eq!(b.selection_len, 12);
    }

    #[test]
    fn history_is_capped_to_the_most_recent() {
        let mut b = Blackboard::new();
        for i in 0..(HISTORY_CAP + 3) {
            b.record(format!("cmd {i}"));
        }
        assert_eq!(b.recent.len(), HISTORY_CAP);
        // Oldest dropped; newest kept.
        assert_eq!(b.recent.first().unwrap(), &format!("cmd {}", 3));
        assert_eq!(
            b.recent.last().unwrap(),
            &format!("cmd {}", HISTORY_CAP + 2)
        );
    }

    #[test]
    fn context_summary_is_fenced_and_lists_state() {
        let mut b = Blackboard::new();
        b.observe(&snap("Chrome", "Inbox — Gmail", 5));
        b.bind("msg_row", Selector::default());
        b.record("opened the mail from Andreas");
        let s = b.context_summary();
        assert!(s.starts_with("<<<SESSION_CONTEXT"));
        assert!(s.trim_end().ends_with("<<<END_SESSION_CONTEXT"));
        assert!(s.contains("focused_app: Chrome"));
        assert!(s.contains("window: Inbox — Gmail"));
        assert!(s.contains("5 chars selected"));
        assert!(s.contains("bound: msg_row"));
        assert!(s.contains("opened the mail from Andreas"));
    }

    #[test]
    fn reset_clears_memory_not_the_frame() {
        let mut b = Blackboard::new();
        b.observe(&snap("Spotify", "Spotify", 0));
        b.record("played a song");
        b.bind("row", Selector::default());
        b.note_opened("Spotify");
        b.reset_context();
        assert!(b.recent.is_empty());
        assert!(b.binds.is_empty());
        assert!(b.opened.is_empty());
        assert_eq!(b.focus_app.as_deref(), Some("Spotify"));
    }

    #[test]
    fn note_opened_dedups_case_insensitively_and_moves_to_recent() {
        let mut b = Blackboard::new();
        b.note_opened("Chrome");
        b.note_opened("Microsoft Word");
        b.note_opened("chrome"); // same app, different case
        // Chrome collapses to one entry, now most-recent.
        assert_eq!(b.opened, vec!["Microsoft Word".to_string(), "chrome".to_string()]);
    }

    #[test]
    fn note_opened_ignores_blank_and_caps_history() {
        let mut b = Blackboard::new();
        b.note_opened("   ");
        assert!(b.opened.is_empty());
        for i in 0..(OPENED_CAP + 3) {
            b.note_opened(format!("app {i}"));
        }
        assert_eq!(b.opened.len(), OPENED_CAP);
        assert_eq!(b.opened.first().unwrap(), &format!("app {}", 3));
    }

    #[test]
    fn context_summary_lists_already_open_apps() {
        let mut b = Blackboard::new();
        b.note_opened("Microsoft Word");
        let s = b.context_summary();
        assert!(s.contains("already_open: Microsoft Word"));
    }
}
