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
}
