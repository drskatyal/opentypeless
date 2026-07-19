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
    /// Transforms applied, in order, wherever this slot is substituted — so a
    /// value can be URL-encoded in a `uri` step yet trimmed-plain in a
    /// `set_value` step. See [`SlotFilter`]. Empty = substitute verbatim.
    #[serde(default)]
    pub filters: Vec<SlotFilter>,
}

/// A value transform applied at substitution time. Kept small and total —
/// unknown filters would be a schema error, so the set is closed and typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotFilter {
    /// Strip leading/trailing whitespace.
    Trim,
    /// Collapse internal runs of whitespace to single spaces (implies trim).
    Squish,
    /// Lowercase (Unicode-aware).
    Lower,
    /// Percent-encode for use inside a URL query/path component.
    Urlencode,
    /// Escape for safe inclusion inside a PowerShell **single-quoted** string
    /// literal: every single quote is doubled (`'` → `''`), PowerShell's only
    /// in-literal escape. The recipe MUST surround the token with single quotes
    /// (`'{name}'`); this keeps an untrusted slot value from breaking out of that
    /// literal and injecting further commands. No other character is special
    /// inside a single-quoted PowerShell string (backticks, `$`, `;`, `|`, `&`
    /// are all literal), so doubling the quote is sufficient.
    ///
    /// Serialized as `psquote` (one word, matching the inline-filter spelling
    /// `{name|psquote}`) rather than the `snake_case` default `ps_quote`.
    #[serde(rename = "psquote")]
    PsQuote,
}

impl SlotFilter {
    /// Apply this transform to a substituted value.
    pub fn apply(self, s: &str) -> String {
        match self {
            SlotFilter::Trim => s.trim().to_string(),
            SlotFilter::Squish => s.split_whitespace().collect::<Vec<_>>().join(" "),
            SlotFilter::Lower => s.to_lowercase(),
            SlotFilter::Urlencode => urlencode_component(s),
            SlotFilter::PsQuote => s.replace('\'', "''"),
        }
    }
}

/// Percent-encode a string for safe inclusion in a URL component (RFC 3986
/// unreserved set stays literal; everything else becomes `%XX`). No external
/// crate — the rule is small and this keeps the primitive dependency-free.
fn urlencode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn default_slot_type() -> String {
    "string".to_string()
}
fn default_true() -> bool {
    true
}

/// A semantic selector set. Resolution tries these in order of specificity and
/// re-resolves live at replay, so no single brittle handle decides the target.
///
/// Prefer *stable* handles — `automation_id_any` and `class_name_any` survive
/// localization and copy changes; `name_any` is a fallback, not the anchor.
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
    /// Any of these stable automation ids matches (most robust when present) —
    /// scored *above* an English-name match, since ids survive localization and
    /// copy changes. Prefer these as the anchor; `name_any` is the fallback.
    #[serde(default)]
    pub automation_id_any: Vec<String>,
    /// Accessibility patterns the control must support, e.g. `value`, `invoke`.
    #[serde(default)]
    pub patterns: Vec<String>,
    /// Required control state gates. A control failing any asserted gate is
    /// rejected outright (a disabled/offscreen control is never a valid target).
    #[serde(default)]
    pub state: StateGate,
    /// When several controls match equally, take the nth (0-based) in tree
    /// order. `None` means "must be unique or best-scored", not "first".
    #[serde(default)]
    pub nth: Option<usize>,
    /// Resolve to an element bound by an earlier step (`FlowStep::bind`) instead
    /// of searching — for acting on a specific row already chosen by a
    /// `pick_result`. Mutually exclusive in spirit with the search fields.
    #[serde(default)]
    pub element_ref: Option<String>,
    /// Scope the live search *under* an earlier-bound element (its subtree),
    /// e.g. click the "More" button within the message row already bound.
    #[serde(default)]
    pub within_ref: Option<String>,
    /// Where to search: `app` (the target window) or `focused`.
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_scope() -> String {
    "app".to_string()
}

/// Toggle-state requirements a candidate must satisfy to be a legal target, so a
/// flow can target "the checkbox *only when unchecked*" or "the *selected* tab".
/// `None` = don't care; `Some(true)`/`Some(false)` = assert the state.
///
/// These cover the states the safety filter does *not* own: enabled + on-screen
/// are always required (a disabled or offscreen control is rejected outright,
/// regardless of any gate here), so they are deliberately not expressible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StateGate {
    /// Assert the control is (not) checked (checkbox / toggle).
    #[serde(default)]
    pub checked: Option<bool>,
    /// Assert the control is (not) selected (tab / list item / radio).
    #[serde(default)]
    pub selected: Option<bool>,
    /// Assert the control is (not) expanded (tree item / combo box).
    #[serde(default)]
    pub expanded: Option<bool>,
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

/// How a `pick_result` step chooses among candidate rows, and what to do at the
/// edges. This separates *choosing* a result from *acting* on it: a
/// `pick_result` binds the winner (via [`FlowStep::bind`]); a later step acts on
/// it through `element_ref`. Choosing is where flows most often go wrong, so the
/// thresholds and the no-match / tie behaviour are explicit, not implicit.
//
// No `Eq`: `min_score`/`tie_margin` are `f32`. `PartialEq` is enough for tests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PickSpec {
    /// Slot-templated terms the desired row should match (e.g. `{song}`,
    /// `{sender}`). Scored against each candidate's accessible name.
    #[serde(default)]
    pub match_terms: Vec<String>,
    /// Rows whose name contains any of these are rejected outright — e.g.
    /// `["Sponsored","Ad","Promoted"]` so an ad row never wins.
    #[serde(default)]
    pub negative_patterns: Vec<String>,
    /// Minimum score (0.0–1.0) the best candidate must reach to be accepted.
    #[serde(default = "default_min_score")]
    pub min_score: f32,
    /// The best must beat the runner-up by at least this margin, else it's a
    /// tie (ambiguous) — guards against picking one of two near-identical rows.
    #[serde(default = "default_tie_margin")]
    pub tie_margin: f32,
    /// What to do when nothing clears `min_score`.
    #[serde(default)]
    pub if_none: PickFallback,
    /// What to do when the top rows tie within `tie_margin`.
    #[serde(default)]
    pub if_ambiguous: PickFallback,
}

fn default_min_score() -> f32 {
    0.5
}
fn default_tie_margin() -> f32 {
    0.15
}

/// What a `pick_result` does when it can't confidently pick one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickFallback {
    /// Fail the step (honours the step's `on_fail`). Safe default — never guess.
    #[default]
    Fail,
    /// Surface the candidates to the user and wait for a numbered pick.
    Ask,
    /// Take the top-scored row anyway (only for low-stakes, reversible picks).
    TakeBest,
}

/// One semantic step of a leaf flow.
//
// No `Eq`: holds an optional [`PickSpec`], which carries `f32` thresholds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Selection parameters for a `pick_result` step (ignored otherwise).
    #[serde(default)]
    pub pick: Option<PickSpec>,
    /// Bind this step's resolved element/result under a name so later steps can
    /// reference it (`target.element_ref` / `target.within_ref`). PHI-free — a
    /// name, never a value.
    #[serde(default)]
    pub bind: Option<String>,
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
//
// No `Eq`: contains [`FlowStep`]s, which transitively hold `f32` thresholds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// Maximum characters of a card's description rendered into the drawer index.
/// The description is the routing signal, but the planner only needs its gist —
/// the whole (fenced) index ships on every selection call, so a terse cap keeps
/// that per-command prompt small. Every command id and its slots are always kept
/// in full; only the free-text description is shortened.
const CARD_DESC_MAX: usize = 44;

/// Compact a card description for the drawer index: collapse whitespace, drop any
/// parenthetical "(e.g. …)" example clause, and cap length. This is purely a
/// rendering shrink — the stored [`FlowFile::description`] is untouched.
fn terse_description(desc: &str) -> String {
    // Strip parenthesized clauses (usually verbose "(e.g. …)" examples).
    let mut stripped = String::with_capacity(desc.len());
    let mut depth = 0usize;
    for c in desc.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => stripped.push(c),
            _ => {}
        }
    }
    // Collapse any run of whitespace (incl. the gaps a stripped clause left) to one.
    let collapsed = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > CARD_DESC_MAX {
        collapsed
            .chars()
            .take(CARD_DESC_MAX - 1)
            .collect::<String>()
            + "…"
    } else {
        collapsed
    }
}

impl FlowCard {
    /// Render one card as a single prompt line inside the (fenced) drawer index.
    /// Description is treated as data; callers wrap the whole index in an
    /// UNTRUSTED block so a malicious file name can't act as an instruction. The
    /// description is rendered terse ([`terse_description`]) to keep the index —
    /// which ships on every selection call — small; id and slots are kept in full.
    pub fn to_prompt_line(&self) -> String {
        let slots = if self.slots.is_empty() {
            String::new()
        } else {
            format!(" slots=[{}]", self.slots.join(","))
        };
        format!(
            "{} — {}{}",
            self.id,
            terse_description(&self.description),
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
                filters: vec![],
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
                pick: None,
                bind: None,
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
    fn drawer_line_is_terse_but_keeps_id_and_slots() {
        // A verbose card with a parenthetical example clause and a long, multi-line
        // description. The rendered index line must drop the "(e.g. …)" clause and
        // cap length, while keeping the command id and every slot intact.
        let mut c = leaf().card();
        c.description =
            "open or launch any application by name\n(e.g. Spotify, Outlook, Notepad, Calculator, and many more)"
                .into();
        let verbose_len = format!(
            "{} — {} slots=[{}]",
            c.id,
            c.description.replace('\n', " "),
            c.slots.join(",")
        )
        .chars()
        .count();

        let line = c.to_prompt_line();
        // Id and slots survive in full.
        assert!(line.starts_with("play_song — "));
        assert!(line.contains("slots=[song]"));
        // The verbose example clause is gone.
        assert!(!line.contains("(e.g."));
        assert!(!line.contains("Outlook"));
        // And the whole line is shorter than the untrimmed rendering.
        assert!(
            line.chars().count() < verbose_len,
            "trimmed line ({}) must be shorter than verbose ({verbose_len})",
            line.chars().count()
        );
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
    fn slot_filters_apply_in_order() {
        assert_eq!(SlotFilter::Trim.apply("  hi  "), "hi");
        assert_eq!(SlotFilter::Squish.apply("a   b\t c"), "a b c");
        assert_eq!(SlotFilter::Lower.apply("HeLLo"), "hello");
        assert_eq!(SlotFilter::Urlencode.apply("a b&c"), "a%20b%26c");
        // Unreserved chars survive urlencode untouched.
        assert_eq!(SlotFilter::Urlencode.apply("A-z_0.9~"), "A-z_0.9~");
    }

    #[test]
    fn psquote_doubles_single_quotes_and_leaves_others_literal() {
        // A benign value with spaces is untouched (spaces are literal inside a
        // single-quoted PowerShell string) — so a folder name stays one path.
        assert_eq!(SlotFilter::PsQuote.apply("My New Folder"), "My New Folder");
        // A single quote is doubled — the only escape a single-quoted literal has.
        assert_eq!(SlotFilter::PsQuote.apply("O'Brien"), "O''Brien");
        // An injection attempt cannot break out: once wrapped as '<escaped>' the
        // whole thing stays a literal. Doubling the quotes keeps the surrounding
        // quotes balanced so the command after it is never parsed as code.
        let hostile = "'; Remove-Item C:\\ -Recurse; '";
        let escaped = SlotFilter::PsQuote.apply(hostile);
        assert_eq!(escaped, "''; Remove-Item C:\\ -Recurse; ''");
        let wrapped = format!("'{escaped}'");
        // Balanced single quotes: an even count means no dangling opener escapes
        // the literal (the recipe supplies the outer pair).
        assert_eq!(wrapped.matches('\'').count() % 2, 0);
    }

    #[test]
    fn pick_spec_defaults_are_conservative() {
        let json = r#"{ "match_terms": ["{song}"] }"#;
        let p: PickSpec = serde_json::from_str(json).unwrap();
        assert_eq!(p.min_score, 0.5);
        assert_eq!(p.tie_margin, 0.15);
        assert_eq!(p.if_none, PickFallback::Fail);
        assert_eq!(p.if_ambiguous, PickFallback::Fail);
    }

    #[test]
    fn selector_ref_and_state_gates_roundtrip() {
        let json = r#"{
            "role": "check_box",
            "within_ref": "msg_row",
            "state": { "checked": false, "selected": true }
        }"#;
        let s: Selector = serde_json::from_str(json).unwrap();
        assert_eq!(s.within_ref.as_deref(), Some("msg_row"));
        assert_eq!(s.state.checked, Some(false));
        assert_eq!(s.state.selected, Some(true));
        assert_eq!(s.state.expanded, None);
        assert!(s.element_ref.is_none());
        assert!(s.nth.is_none());
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
