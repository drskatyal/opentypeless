//! Serde payloads emitted to the frontend over Tauri events as Act runs.
//!
//! Event names live in [`ACT_EVENT`]. Payloads are PHI-safe: they carry short
//! human summaries and option labels (control names), never element values.

use serde::{Deserialize, Serialize};

/// The Tauri event channel Act state/results are emitted on.
pub const ACT_EVENT: &str = "act://event";

/// A disambiguation option offered to the user (numbered-overlay pick).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskOption {
    pub index: usize,
    pub label: String,
    pub path: String,
}

/// Everything Act tells the UI. `kind` is the wire discriminant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActEvent {
    /// Session state changed (idle / armed / planning / executing / awaiting_*).
    State { state: String },
    /// A live, human-readable progress line for the activity indicator, e.g.
    /// "opening play_song", "launching Spotify", "searching Hotel California".
    /// PHI-free by construction (labels, not values).
    Step { label: String },
    /// A medium-risk action needs the user's explicit confirmation.
    Confirm { summary: String, reason: String },
    /// The target was ambiguous; the user must pick one.
    AskUser {
        prompt: String,
        options: Vec<AskOption>,
    },
    /// A spoken/displayed answer to a question — talk-back mode, where the user
    /// asked something ("what's on my screen?") rather than commanding an action.
    Say { text: String },
    /// A plan finished (ok = fully completed).
    Result { ok: bool, summary: String },
    /// A recoverable error (bad plan, timeout, unsupported platform, …).
    Error { message: String },
    /// A mission entered the queue — one card on the Agents board. `id` is stable
    /// for the life of the command (e.g. "t0"); `label` is a short human title (a
    /// flow name, a goal, or a question). `status` distinguishes a `queued` card
    /// (spawned up front, before it runs) from one that is already `running`. An
    /// absent status means `running` (back-compat with the old lazy spawn).
    TaskSpawned {
        id: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// A live status line for a running task's card. PHI-free (labels, not
    /// values), same discipline as [`ActEvent::Step`].
    TaskProgress { id: String, text: String },
    /// A task finished — the card flips to Done ✓ or Failed. Mirrors the
    /// mission's [`ActEvent::Result`]/[`ActEvent::Say`] outcome.
    TaskResult {
        id: String,
        ok: bool,
        summary: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_roundtrips_with_kind_tag() {
        let e = ActEvent::Confirm {
            summary: "Click Send".into(),
            reason: "sends a message".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"kind\":\"confirm\""));
        assert_eq!(serde_json::from_str::<ActEvent>(&json).unwrap(), e);
    }

    #[test]
    fn ask_user_options_roundtrip() {
        let e = ActEvent::AskUser {
            prompt: "Which Delete?".into(),
            options: vec![
                AskOption {
                    index: 1,
                    label: "Delete".into(),
                    path: "#/1".into(),
                },
                AskOption {
                    index: 2,
                    label: "Delete".into(),
                    path: "#/2".into(),
                },
            ],
        };
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<ActEvent>(&json).unwrap(), e);
    }

    #[test]
    fn task_events_roundtrip_with_snake_case_kinds() {
        let spawned = ActEvent::TaskSpawned {
            id: "t0".into(),
            label: "Open Gmail".into(),
            status: None,
        };
        let json = serde_json::to_string(&spawned).unwrap();
        assert!(json.contains("\"kind\":\"task_spawned\""));
        // An absent status is omitted from the wire (back-compat) and round-trips.
        assert!(!json.contains("status"));
        assert_eq!(serde_json::from_str::<ActEvent>(&json).unwrap(), spawned);

        // A queued spawn carries its status on the wire and round-trips.
        let queued = ActEvent::TaskSpawned {
            id: "t1".into(),
            label: "Open Gmail".into(),
            status: Some("queued".into()),
        };
        let json = serde_json::to_string(&queued).unwrap();
        assert!(json.contains("\"status\":\"queued\""));
        assert_eq!(serde_json::from_str::<ActEvent>(&json).unwrap(), queued);

        let progress = ActEvent::TaskProgress {
            id: "t0".into(),
            text: "Working…".into(),
        };
        let json = serde_json::to_string(&progress).unwrap();
        assert!(json.contains("\"kind\":\"task_progress\""));
        assert_eq!(serde_json::from_str::<ActEvent>(&json).unwrap(), progress);

        let result = ActEvent::TaskResult {
            id: "t0".into(),
            ok: true,
            summary: "Done: Open Gmail".into(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"kind\":\"task_result\""));
        assert_eq!(serde_json::from_str::<ActEvent>(&json).unwrap(), result);
    }
}
