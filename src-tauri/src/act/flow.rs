//! The "drawer" file schema — a saved task flow.
//!
//! The drawer is a registry of named files. The planner sees only each file's
//! *card* (`id · name · description · slot hints`) in its cached prompt; it picks
//! files by id and fills their slots. Opening a file loads the full [`FlowFile`]
//! locally:
//!
//! * a [`FlowKind::Leaf`] file is a ready semantic recipe — run its [`FlowStep`]s
//!   directly, zero further model calls.
//! * a [`FlowKind::Branch`] file needs reasoning — its `branch_context` is handed
//!   back to the planner, which loops as the task's complexity demands.
//!
//! Steps target controls by *semantic selector sets* (role + name synonyms +
//! automation id + required patterns), never pixels, so a flow survives the window
//! moving or minor UI drift. Values are slot-templated (`{song}`). Every flow
//! carries a lifecycle status and health counters so a flow that stops verifying
//! is quarantined rather than left to click the wrong thing.

use serde::{Deserialize, Serialize};

/// Whether a file executes directly or expands into more planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowKind {
    /// A deterministic recipe — run the steps, no further model call.
    Leaf,
    /// Needs the planner to reason with `branch_context` and loop.
    Branch,
}

/// A saved flow's trust state. A flow is only trusted after it has verified on
/// the real target; repeated verify failures quarantine it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowStatus {
    /// Authored but neither statically nor runtime checked. Default.
    #[default]
    Draft,
    /// Statically checked only (allowlisted URI / known-good shortcut, no
    /// selector steps) but never executed on Windows. What web-researched
    /// deterministic flows get — reliable-by-construction, but NOT proven at
    /// runtime, so it never wears the `verified` badge.
    Smoke,
    /// Executed on the real target with the objective verify all green. Only a
    /// real run promotes to this — authoring never sets it (audit: "verified"
    /// must mean runtime-verified, or health/promotion is a lie).
    Verified,
    /// Failed verification repeatedly; skipped in favor of a fresh plan.
    Quarantined,
}

/// A variable the user fills by voice, e.g. the song title in "play {song}".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    pub name: String,
    /// Free-form type hint for the planner (`string`, `person`, `query`, …).
    #[serde(default = "default_slot_type")]
    pub kind: String,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub default: Option<String>,
}

fn default_slot_type() -> String {
    "string".to_string()
}
fn default_true() -> bool {
    true
}

/// A semantic selector set. Resolution tries these in order of specificity and
/// re-resolves live at replay, so no single brittle handle decides the target.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Selector {
    /// Accessibility control type, e.g. `edit`, `button`, `listitem`.
    #[serde(default)]
    pub role: Option<String>,
    /// Any of these control names / labels matches (synonyms + localizations).
    #[serde(default)]
    pub name_any: Vec<String>,
    /// The name contains this (slot-templated) substring — for result rows.
    #[serde(default)]
    pub name_contains: Option<String>,
    /// Any of these stable automation ids matches (most robust when present).
    #[serde(default)]
    pub automation_id_any: Vec<String>,
    /// Accessibility patterns the control must support, e.g. `value`, `invoke`.
    #[serde(default)]
    pub patterns: Vec<String>,
    /// Where to search: `app` (the target window) or `focused`.
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_scope() -> String {
    "app".to_string()
}

/// A precondition that must hold (or be made to hold) before a flow runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Precondition {
    /// The app must be running; launch it by name/alias/path if not.
    AppRunning { app: String },
    /// The app must be foreground.
    FocusApp { app: String },
    /// Bail to a fresh plan if a login/auth wall is detected.
    Authenticated,
}

/// A wait predicate — flows wait on UI *conditions*, never fixed sleeps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitSpec {
    /// e.g. `target_exists`, `results_present`, `value_contains`.
    pub predicate: String,
    #[serde(default)]
    pub selector: Option<Selector>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u32,
}

fn default_timeout_ms() -> u32 {
    5000
}

/// What to do when a step fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnFail {
    /// Stop the flow and report (default — never amplify a wrong state).
    #[default]
    Abort,
    /// Retry the step once after a stable-UI wait.
    RetryOnce,
    /// Drop out of the flow and replan the remaining goal from primitives.
    Replan,
}

/// One semantic step of a leaf flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowStep {
    pub id: String,
    /// Human/audit description of the step's intent (PHI-free).
    #[serde(default)]
    pub intent: String,
    /// The primitive verb: `launch` `uri` `focus_app` `focus` `set_value`
    /// `invoke` `key` `pick_result` `wait`.
    pub action: String,
    /// The control this step targets (none for `launch`/`uri`/`key`/`wait`).
    #[serde(default)]
    pub target: Option<Selector>,
    /// A slot-templated literal for `set_value` / `uri` / `launch` / `key`.
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub wait_before: Option<WaitSpec>,
    /// A predicate that must hold after the step (else the step failed).
    #[serde(default)]
    pub postcondition: Option<WaitSpec>,
    #[serde(default)]
    pub on_fail: OnFail,
}

/// The objective verification for a whole flow (outcome, not "click succeeded").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifySpec {
    /// e.g. `uia_text_contains`, `now_playing_contains`, `url_contains`.
    pub predicate: String,
    /// Slot-templated terms the observed state must contain.
    #[serde(default)]
    pub terms: Vec<String>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u32,
}

/// PHI-free health counters used to promote or quarantine a flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FlowHealth {
    #[serde(default)]
    pub success_count: u32,
    #[serde(default)]
    pub fail_count: u32,
}

/// A saved task file — the unit the drawer stores and the planner opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowFile {
    pub id: String,
    pub name: String,
    /// One line the planner reads to pick this file. Untrusted user data — the
    /// prompt fences it, never treats it as an instruction.
    pub description: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub kind: FlowKind,
    /// Apps this flow is scoped to (by process/window name).
    #[serde(default)]
    pub app_scope: Vec<String>,
    #[serde(default)]
    pub preconditions: Vec<Precondition>,
    #[serde(default)]
    pub slots: Vec<Slot>,
    /// Steps for a [`FlowKind::Leaf`]. Empty for a branch.
    #[serde(default)]
    pub steps: Vec<FlowStep>,
    /// Context handed to the planner for a [`FlowKind::Branch`]. Empty for a leaf.
    #[serde(default)]
    pub branch_context: Option<String>,
    #[serde(default)]
    pub verify: Option<VerifySpec>,
    #[serde(default)]
    pub status: FlowStatus,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub health: FlowHealth,
}

/// The compact index entry the planner sees for a file — never the whole file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowCard {
    pub id: String,
    pub name: String,
    pub description: String,
    pub slots: Vec<String>,
}

impl FlowFile {
    /// The drawer-index card for this file: id, name, description, slot names.
    pub fn card(&self) -> FlowCard {
        FlowCard {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            slots: self.slots.iter().map(|s| s.name.clone()).collect(),
        }
    }

    /// A flow is offerable to the planner unless it has been quarantined.
    pub fn is_selectable(&self) -> bool {
        self.status != FlowStatus::Quarantined
    }

    /// Record a successful verified run; promotes a draft or smoke-checked flow
    /// to runtime-verified (only a real run can reach `Verified`).
    pub fn record_success(&mut self) {
        self.health.success_count = self.health.success_count.saturating_add(1);
        self.health.fail_count = 0;
        if matches!(self.status, FlowStatus::Draft | FlowStatus::Smoke) {
            self.status = FlowStatus::Verified;
        }
    }

    /// Record a verify failure; quarantine after two consecutive failures.
    pub fn record_failure(&mut self) {
        self.health.fail_count = self.health.fail_count.saturating_add(1);
        if self.health.fail_count >= 2 {
            self.status = FlowStatus::Quarantined;
        }
    }
}

impl FlowCard {
    /// Render one card as a single prompt line inside the (fenced) drawer index.
    /// Description is treated as data; callers wrap the whole index in an
    /// UNTRUSTED block so a malicious file name can't act as an instruction.
    pub fn to_prompt_line(&self) -> String {
        let slots = if self.slots.is_empty() {
            String::new()
        } else {
            format!(" slots=[{}]", self.slots.join(","))
        };
        format!(
            "{} — {}{}",
            self.id,
            self.description.replace('\n', " "),
            slots
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf() -> FlowFile {
        FlowFile {
            id: "play_song".into(),
            name: "Play a song".into(),
            description: "play a track in the music app".into(),
            aliases: vec!["play music".into()],
            kind: FlowKind::Leaf,
            app_scope: vec!["Spotify".into()],
            preconditions: vec![Precondition::AppRunning {
                app: "Spotify".into(),
            }],
            slots: vec![Slot {
                name: "song".into(),
                kind: "query".into(),
                required: true,
                examples: vec!["Hotel California".into()],
                default: None,
            }],
            steps: vec![FlowStep {
                id: "s1".into(),
                intent: "focus search".into(),
                action: "set_value".into(),
                target: Some(Selector {
                    role: Some("edit".into()),
                    name_any: vec!["Search".into()],
                    scope: "app".into(),
                    ..Default::default()
                }),
                value: Some("{song}".into()),
                wait_before: None,
                postcondition: None,
                on_fail: OnFail::Abort,
            }],
            branch_context: None,
            verify: Some(VerifySpec {
                predicate: "now_playing_contains".into(),
                terms: vec!["{song}".into()],
                timeout_ms: 4000,
            }),
            status: FlowStatus::Draft,
            version: 1,
            health: FlowHealth::default(),
        }
    }

    #[test]
    fn card_carries_id_desc_and_slot_names_only() {
        let c = leaf().card();
        assert_eq!(c.id, "play_song");
        assert_eq!(c.slots, vec!["song".to_string()]);
        assert!(c.to_prompt_line().contains("play_song"));
        assert!(c.to_prompt_line().contains("slots=[song]"));
    }

    #[test]
    fn draft_promotes_on_success_and_quarantines_on_repeated_failure() {
        let mut f = leaf();
        f.record_success();
        assert_eq!(f.status, FlowStatus::Verified);
        f.record_failure();
        assert_eq!(f.status, FlowStatus::Verified); // one failure is not fatal
        f.record_failure();
        assert_eq!(f.status, FlowStatus::Quarantined);
        assert!(!f.is_selectable());
    }

    #[test]
    fn file_roundtrips_through_json() {
        let f = leaf();
        let json = serde_json::to_string(&f).unwrap();
        let back: FlowFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn serde_defaults_fill_a_minimal_file() {
        // A hand-authored file need only specify the essentials.
        let json = r#"{
            "id": "open_bluetooth",
            "name": "Open Bluetooth settings",
            "description": "open the Bluetooth settings page",
            "kind": "leaf",
            "steps": [{ "id":"s1", "action":"uri", "value":"ms-settings:bluetooth" }]
        }"#;
        let f: FlowFile = serde_json::from_str(json).unwrap();
        assert_eq!(f.status, FlowStatus::Draft);
        assert_eq!(f.version, 0);
        assert!(f.slots.is_empty());
        assert_eq!(f.steps[0].on_fail, OnFail::Abort);
    }
}
