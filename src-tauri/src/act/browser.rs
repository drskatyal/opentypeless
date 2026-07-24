//! CDP-controlled-Chrome browser automation (SPIKE — feature `cdp-browser`).
//!
//! This module is the Rust half of an isolated experiment: instead of driving
//! web content through UIA/AX (which flattens the DOM, so coordinate clicks miss
//! and focus gets stolen), it hands a browser task to a Node/TypeScript sidecar
//! (`browser-agent/`) that drives a dedicated Chrome over the DevTools Protocol
//! via Stagehand v3. Real DOM, precise clicks, a persistent FlowRad profile.
//!
//! **Status: wired behind the `cdp-browser` Cargo feature (default OFF).** When a
//! build enables the feature, the conductor's Novel-mission dispatch
//! ([`super::conductor::Conductor`]) routes a browser page-content goal here —
//! via the pure [`is_browser_task`] decision and the [`browser_task_for`] mapping —
//! and falls back to the UIA planner on any CDP failure. A DEFAULT build never
//! compiles this module, so today's behavior is unchanged.
//! Design notes and the router classification rules live in
//! `docs/act-cdp-browser.md`; the sidecar protocol lives in
//! `browser-agent/README.md`.
//!
//! Protocol: one task per sidecar invocation. We write a single JSON line to the
//! child's stdin and read a single JSON result line from its stdout:
//!
//! ```text
//! stdin  -> { "intent": "play the first result", "url": "https://youtube.com", "timeoutMs": 60000 }
//! stdout <- { "ok": true, "detail": "clicked the first video", "actions": [ ... ] }
//! ```

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Default DevTools remote-debugging port for the FlowRad Chrome.
const DEFAULT_CDP_PORT: u16 = 9222;

/// Default per-task ceiling handed to the sidecar (milliseconds).
const DEFAULT_TASK_TIMEOUT_MS: u64 = 60_000;

/// One browser task, serialized to the sidecar's stdin as a single JSON line.
///
/// The field names/casing here are the wire contract the Node sidecar
/// (`browser-agent/index.ts`) reads: `intent`, `url`, `timeoutMs`, `mode`
/// (`"act"` | `"links"`), and `select` (a 1-based number or a text hint).
#[derive(Debug, Clone, Serialize)]
pub struct BrowserTask {
    /// Natural-language goal for this turn ("play the first result", "type the
    /// prompt into the chat box and send it").
    pub intent: String,
    /// Optional URL to navigate to before acting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Optional per-task timeout override (milliseconds).
    #[serde(rename = "timeoutMs", skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Which sidecar driver to use: `"act"` (LLM-planned CDP acting, the default
    /// when omitted) or `"links"` (deterministic DOM anchor extract + navigate).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// `"links"`-mode selection hint: a 1-based position (a JSON number) or a text
    /// match (a JSON string). Ignored by the sidecar in `"act"` mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub select: Option<serde_json::Value>,
}

impl BrowserTask {
    /// A task that just acts on whatever page the FlowRad Chrome is already on.
    pub fn new(intent: impl Into<String>) -> Self {
        Self {
            intent: intent.into(),
            url: None,
            timeout_ms: None,
            mode: None,
            select: None,
        }
    }

    /// Navigate to `url` first, then act.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Switch this task to the deterministic `"links"` driver (DOM anchor
    /// extraction + navigate-by-href, no LLM).
    pub fn links_mode(mut self) -> Self {
        self.mode = Some("links".to_string());
        self
    }

    /// `"links"`-mode: pick the Nth extracted anchor (1-based).
    pub fn with_select_index(mut self, n: u32) -> Self {
        self.select = Some(serde_json::Value::from(n));
        self
    }

    /// `"links"`-mode: pick the best text match among the extracted anchors.
    pub fn with_select_text(mut self, text: impl Into<String>) -> Self {
        self.select = Some(serde_json::Value::from(text.into()));
        self
    }
}

/// The single JSON result line the sidecar writes to stdout.
#[derive(Debug, Clone, Deserialize)]
pub struct BrowserResult {
    /// Whether Stagehand reported the act as successful.
    pub ok: bool,
    /// Human-readable summary of what happened (or why it failed).
    #[serde(default)]
    pub detail: String,
    /// Stagehand's structured actions, when the turn produced any.
    #[serde(default)]
    pub actions: Vec<serde_json::Value>,
    /// The href the tab was navigated to (`"links"` mode); used to enrich the
    /// success summary. Absent in `"act"` mode.
    #[serde(default, rename = "chosenHref")]
    pub chosen_href: Option<String>,
    /// Present only on failure.
    #[serde(default)]
    pub error: Option<String>,
}

/// Anything that can go wrong running a browser task.
#[derive(Debug)]
pub enum BrowserError {
    /// The sidecar process could not be spawned (Node missing, bad path).
    Spawn(std::io::Error),
    /// Writing the task or reading the result failed.
    Io(std::io::Error),
    /// Task serialization / result deserialization failed.
    Serde(serde_json::Error),
    /// The sidecar produced no result line (crashed before emitting one).
    NoResult,
    /// The sidecar ran the task but reported failure.
    Failed(BrowserResult),
}

impl std::fmt::Display for BrowserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrowserError::Spawn(e) => write!(f, "failed to spawn browser sidecar: {e}"),
            BrowserError::Io(e) => write!(f, "browser sidecar I/O error: {e}"),
            BrowserError::Serde(e) => write!(f, "browser sidecar protocol error: {e}"),
            BrowserError::NoResult => write!(f, "browser sidecar produced no result line"),
            BrowserError::Failed(r) => {
                write!(f, "browser task failed: {}", r.detail)?;
                if let Some(err) = &r.error {
                    write!(f, " ({err})")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for BrowserError {}

/// How to launch and talk to the sidecar. Every field has an env-backed default
/// (mirroring the env vars the sidecar itself reads) so the host can inject a
/// per-machine configuration without code changes.
#[derive(Debug, Clone)]
pub struct BrowserConfig {
    /// The command used to run the sidecar, e.g. `node`.
    pub node_command: String,
    /// Path to the built sidecar entrypoint (`browser-agent/dist/index.js`).
    pub script_path: PathBuf,
    /// DevTools remote-debugging port for the FlowRad Chrome.
    pub cdp_port: u16,
    /// Attach to an already-running Chrome at this CDP endpoint instead of
    /// letting the sidecar launch one. `None` => sidecar launches Chrome.
    pub cdp_url: Option<String>,
    /// Dedicated, persistent FlowRad Chrome profile directory.
    pub user_data_dir: Option<PathBuf>,
    /// Default per-task timeout when a `BrowserTask` doesn't set one.
    pub task_timeout: Duration,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        let node_command = std::env::var("FLOWRAD_BROWSER_NODE").unwrap_or_else(|_| "node".into());
        let script_path = std::env::var("FLOWRAD_BROWSER_AGENT_SCRIPT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("browser-agent/dist/index.js"));
        let cdp_port = std::env::var("FLOWRAD_CDP_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_CDP_PORT);
        let cdp_url = std::env::var("FLOWRAD_CDP_URL")
            .ok()
            .filter(|s| !s.is_empty());
        let user_data_dir = std::env::var("FLOWRAD_USER_DATA_DIR")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        Self {
            node_command,
            script_path,
            cdp_port,
            cdp_url,
            user_data_dir,
            task_timeout: Duration::from_millis(DEFAULT_TASK_TIMEOUT_MS),
        }
    }
}

/// A handle that runs one browser task per call by spawning the sidecar.
///
/// Deliberately stateless w.r.t. the child process: each [`run`](Self::run)
/// spawns, talks, and reaps a fresh sidecar (which drives the persistent Chrome
/// profile), so a crashed task can never wedge a long-lived subprocess. A future
/// iteration could hold the Chrome open and keep the sidecar resident; that is
/// out of scope for the spike.
pub struct BrowserSession {
    config: BrowserConfig,
}

impl BrowserSession {
    /// Build a session from an explicit config.
    pub fn new(config: BrowserConfig) -> Self {
        Self { config }
    }

    /// Build a session from env-backed defaults (see [`BrowserConfig::default`]).
    pub fn from_env() -> Self {
        Self::new(BrowserConfig::default())
    }

    /// Run one browser task end to end: spawn the sidecar, hand it the task,
    /// read back its single JSON result line.
    ///
    /// The sidecar reads `GEMINI_API_KEY` from its own environment; this method
    /// forwards the host process environment as-is (plus the CDP/profile knobs),
    /// so the key must be present in the host env.
    pub fn run(&self, task: &BrowserTask) -> Result<BrowserResult, BrowserError> {
        let mut task = task.clone();
        if task.timeout_ms.is_none() {
            task.timeout_ms = Some(self.config.task_timeout.as_millis() as u64);
        }
        let payload = serde_json::to_string(&task).map_err(BrowserError::Serde)?;

        let mut cmd = Command::new(&self.config.node_command);
        cmd.arg(&self.config.script_path)
            .env("FLOWRAD_CDP_PORT", self.config.cdp_port.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(url) = &self.config.cdp_url {
            cmd.env("FLOWRAD_CDP_URL", url);
        }
        if let Some(dir) = &self.config.user_data_dir {
            cmd.env("FLOWRAD_USER_DATA_DIR", dir);
        }

        let mut child = cmd.spawn().map_err(BrowserError::Spawn)?;

        // Write the task line, then drop stdin so the sidecar's stdin reaches EOF.
        {
            let mut stdin = child.stdin.take().ok_or(BrowserError::NoResult)?;
            stdin
                .write_all(payload.as_bytes())
                .and_then(|_| stdin.write_all(b"\n"))
                .map_err(BrowserError::Io)?;
        }

        // Read the first non-empty JSON line from stdout.
        let stdout = child.stdout.take().ok_or(BrowserError::NoResult)?;
        let mut reader = BufReader::new(stdout);
        let mut result: Option<BrowserResult> = None;
        let mut line = String::new();
        loop {
            line.clear();
            let n = reader.read_line(&mut line).map_err(BrowserError::Io)?;
            if n == 0 {
                break; // EOF
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            result = Some(serde_json::from_str(trimmed).map_err(BrowserError::Serde)?);
            break;
        }

        // Reap the child so we never leak a zombie, regardless of outcome.
        let _ = child.wait();

        match result {
            Some(r) if r.ok => Ok(r),
            Some(r) => Err(BrowserError::Failed(r)),
            None => Err(BrowserError::NoResult),
        }
    }
}

// ===========================================================================
// Router decision (pure, unit-tested).
// ===========================================================================

/// Goal keywords/verbs that identify **page content** work — the stuff a DOM
/// path serves best. Matched case-insensitively via `contains`.
const WEB_CONTENT_KEYWORDS: &[&str] = &[
    "play",
    "watch",
    "video",
    "result",
    "link",
    "click",
    "search",
    "scroll",
    "type",
    "send",
    "message",
    "post",
    "comment",
    "reply",
    "like",
    "subscribe",
    "follow",
    "share",
    "sign in",
    "log in",
    "login",
    "fill",
    "submit",
    "form",
    "select",
    "choose",
    "read",
    "article",
    "add to cart",
    "checkout",
    "buy",
    "purchase",
    "review",
    "navigate",
    "go to",
    "visit",
    "browse",
    "first",
    "second",
    "third",
    "top result",
    "the page",
];

/// Goal keywords that identify **browser-chrome / OS-window** management rather
/// than page content. These stay on the UIA path: they work there today, and a
/// browser-chrome action (new tab, close window) is not DOM content. Matched
/// before the web keywords so an ambiguous "open a new tab" resolves to `false`.
const OS_CHROME_KEYWORDS: &[&str] = &[
    "new tab",
    "close tab",
    "close the tab",
    "new window",
    "close window",
    "close the window",
    "minimize",
    "maximize",
    "restore the window",
    "incognito",
    "private window",
    "bookmark",
    "add to favorites",
    "downloads",
    "history",
    "clear history",
    "settings",
    "preferences",
    "extension",
    "zoom in",
    "zoom out",
    "reset zoom",
    "full screen",
    "fullscreen",
    "print the page",
    "quit",
    "restart the browser",
    "update chrome",
];

/// Pure router decision: should this mission be driven over the CDP/DOM browser
/// path instead of the UIA/AX tree?
///
/// Returns `true` only when **both** hold (see `docs/act-cdp-browser.md`):
///
/// 1. **The surface is a browser** — `foreground_app` normalizes to a known
///    browser (shared with the live UIA path via
///    [`super::conductor::app_is_browser`]).
/// 2. **The goal is web content** — a light verb/keyword heuristic. Browser-chrome
///    / OS-window goals ("open a new tab", "close the window") and anything with
///    no clear web signal are treated as **not** browser work.
///
/// When the classification is ambiguous the function returns `false`, so the
/// mission falls back to the existing UIA/AX path. This makes the spike strictly
/// additive: it can only ever route *more* work to CDP, never regress today's
/// behavior. On any downstream CDP error the dispatch is expected to fall back to
/// UIA as well.
pub fn is_browser_task(foreground_app: &str, goal: &str) -> bool {
    if !super::conductor::app_is_browser(foreground_app) {
        return false;
    }
    let g = goal.trim().to_ascii_lowercase();
    if g.is_empty() {
        return false;
    }
    // Browser-chrome / OS-window management wins first: keep it on UIA.
    if OS_CHROME_KEYWORDS.iter().any(|kw| g.contains(kw)) {
        return false;
    }
    // Otherwise, route to CDP only on a positive web-content signal; ambiguous
    // goals fall back to UIA.
    WEB_CONTENT_KEYWORDS.iter().any(|kw| g.contains(kw))
}

// ===========================================================================
// Mission -> BrowserTask mapping (pure, unit-tested).
// ===========================================================================

/// Known site name -> canonical URL, for a goal/target that NAMES a site without
/// spelling out its URL ("play the first result on YouTube").
const KNOWN_SITES: &[(&str, &str)] = &[
    ("youtube", "https://www.youtube.com"),
    ("google", "https://www.google.com"),
    ("gmail", "https://mail.google.com"),
    ("grok", "https://grok.com"),
    ("chatgpt", "https://chatgpt.com"),
    ("reddit", "https://www.reddit.com"),
    ("twitter", "https://twitter.com"),
    ("amazon", "https://www.amazon.com"),
    ("wikipedia", "https://www.wikipedia.org"),
    ("github", "https://github.com"),
];

/// Nouns that mark an ordinal selection as picking a RESULT/link rather than a
/// stray number in the goal text.
const RESULT_NOUNS: &[&str] = &[
    "result", "link", "video", "item", "option", "song", "track", "one", "hit",
];

/// Map a Novel mission (a natural-language `goal`, plus an optional `target_app`
/// hint) onto a concrete [`BrowserTask`] for the CDP sidecar. PURE — no I/O — so
/// the decision is unit-tested in isolation. Rules, in order:
///
/// 1. **URL** — an explicit `http(s)://` or bare domain in the goal wins; else a
///    known site NAMED by the goal or the target app supplies one. If found, the
///    task navigates there first (`with_url`).
/// 2. **"Nth result" → deterministic links mode** — a "play/open/click the
///    FIRST/second/top … result/link/video" style goal maps to `mode: "links"`
///    with a 1-based numeric `select`, so the sidecar extracts anchors and
///    navigates by href with NO LLM.
/// 3. **Otherwise** — an `"act"` task (mode omitted) with `intent = goal`, letting
///    the sidecar plan the turn.
pub fn browser_task_for(goal: &str, target_app: Option<&str>) -> BrowserTask {
    let g = goal.trim();
    let lower = g.to_ascii_lowercase();

    let mut task = BrowserTask::new(g);
    if let Some(url) = extract_url(g).or_else(|| site_url(&lower, target_app)) {
        task = task.with_url(url);
    }

    // "play/open/click the FIRST/Nth result|link|video" → deterministic links mode.
    if let Some(n) = ordinal_result(&lower) {
        return task.links_mode().with_select_index(n);
    }
    task
}

/// A human-readable summary of a successful browser result, folding in the
/// navigated href (links mode) when the sidecar reported one.
pub fn browser_summary(result: &BrowserResult) -> String {
    let base = result.detail.trim();
    let base = if base.is_empty() { "Done" } else { base };
    match result.chosen_href.as_deref() {
        Some(href) if !href.is_empty() => format!("{base} ({href})"),
        _ => base.to_string(),
    }
}

/// Pull the first explicit URL / bare domain out of a goal string, preserving the
/// original case (paths can be case-sensitive). A bare domain gains an `https://`.
fn extract_url(goal: &str) -> Option<String> {
    for raw in goal.split_whitespace() {
        // Trim surrounding punctuation ("visit example.com." / "(https://x.io)").
        let tok = raw.trim_matches(|c: char| {
            !(c.is_ascii_alphanumeric()
                || matches!(c, ':' | '/' | '.' | '-' | '_' | '?' | '=' | '&' | '#' | '%'))
        });
        if tok.starts_with("http://") || tok.starts_with("https://") {
            return Some(tok.to_string());
        }
        if looks_like_domain(tok) {
            return Some(format!("https://{tok}"));
        }
    }
    None
}

/// Whether a token looks like a bare host/domain (`youtube.com`, `foo.co.uk/x`).
fn looks_like_domain(tok: &str) -> bool {
    if tok.is_empty() || tok.contains(' ') {
        return false;
    }
    // Consider only the host portion for the TLD check.
    let host = tok
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(tok)
        .trim_end_matches('.');
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let tld = *labels.last().unwrap();
    if !(2..=6).contains(&tld.len()) || !tld.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    labels
        .iter()
        .all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

/// A known site's URL when the (lowercased) goal or the target app names it.
fn site_url(lower: &str, target_app: Option<&str>) -> Option<String> {
    let app = target_app.unwrap_or("").to_ascii_lowercase();
    KNOWN_SITES
        .iter()
        .find(|(name, _)| lower.contains(name) || app.contains(name))
        .map(|(_, url)| (*url).to_string())
}

/// The 1-based ordinal of a "play the FIRST/Nth result" style goal, or `None`
/// when the goal isn't selecting an ordinal result/link.
fn ordinal_result(lower: &str) -> Option<u32> {
    let n = ordinal_number(lower)?;
    RESULT_NOUNS
        .iter()
        .any(|noun| lower.contains(noun))
        .then_some(n)
}

/// Parse a leading ordinal ("first", "2nd", "top") to a 1-based number.
fn ordinal_number(lower: &str) -> Option<u32> {
    const WORDS: &[(&str, u32)] = &[
        ("first", 1),
        ("second", 2),
        ("third", 3),
        ("fourth", 4),
        ("fifth", 5),
        ("1st", 1),
        ("2nd", 2),
        ("3rd", 3),
        ("4th", 4),
        ("5th", 5),
    ];
    if lower.contains("top ") || lower.ends_with("top") {
        return Some(1);
    }
    WORDS
        .iter()
        .find(|(w, _)| lower.contains(w))
        .map(|(_, n)| *n)
}

// ===========================================================================
// ROUTER INTEGRATION POINT (wired, feature-gated).
// ===========================================================================
//
// The conductor's Novel-mission dispatch (`super::conductor::Conductor::run_mission`,
// GATED on `cfg(feature = "cdp-browser")`) reads the foreground app from a snapshot
// and, when `is_browser_task(foreground, goal)` holds (foreground is a browser AND
// the goal is web content — see docs/act-cdp-browser.md), calls
// `Conductor::run_browser_task`, which:
//
//   1. maps the goal to a `BrowserTask` via the pure `browser_task_for` helper,
//   2. runs `BrowserSession::from_env().run(&task)` on the blocking pool, and
//   3. on success emits `ActEvent::Result`, or on ANY failure falls back to the
//      UIA `novel_loop` so a CDP hiccup never dead-ends a command.
//
// A DEFAULT build (feature off) does not compile this module or that branch, so the
// live UIA path (executor.rs, planner.rs, selection.rs, conductor.rs) is unchanged.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_serializes_to_expected_wire_shape() {
        let task = BrowserTask::new("play the first result")
            .with_url("https://youtube.com/results?q=lofi");
        let json = serde_json::to_string(&task).unwrap();
        // intent + url present; timeout omitted until defaulted by run().
        assert!(json.contains("\"intent\":\"play the first result\""));
        assert!(json.contains("\"url\":\"https://youtube.com/results?q=lofi\""));
        assert!(!json.contains("timeoutMs"));
    }

    #[test]
    fn task_without_url_omits_optional_fields() {
        let json = serde_json::to_string(&BrowserTask::new("scroll down")).unwrap();
        assert_eq!(json, "{\"intent\":\"scroll down\"}");
    }

    #[test]
    fn result_deserializes_success_line() {
        let line = r#"{"ok":true,"detail":"clicked the first video","actions":[]}"#;
        let r: BrowserResult = serde_json::from_str(line).unwrap();
        assert!(r.ok);
        assert_eq!(r.detail, "clicked the first video");
        assert!(r.error.is_none());
    }

    #[test]
    fn result_deserializes_failure_line() {
        let line = r#"{"ok":false,"detail":"browser task threw","error":"boom"}"#;
        let r: BrowserResult = serde_json::from_str(line).unwrap();
        assert!(!r.ok);
        assert_eq!(r.error.as_deref(), Some("boom"));
    }

    #[test]
    fn config_default_has_sane_port_and_script() {
        let cfg = BrowserConfig::default();
        assert!(cfg.cdp_port >= 1024);
        assert!(cfg.script_path.to_string_lossy().ends_with("index.js"));
    }

    // --- router: is_browser_task -------------------------------------------

    #[test]
    fn router_browser_plus_web_goal_is_true() {
        // Foreground is a browser AND the goal is clearly page content.
        for (app, goal) in [
            ("chrome.exe", "play the first result"),
            ("Google Chrome", "click the second link"),
            ("msedge.exe", "send this message to Grok"),
            ("Brave Browser", "search for lofi and play the top result"),
            ("firefox", "scroll down and like the video"),
            ("Vivaldi", "type the prompt into the chat box and submit"),
        ] {
            assert!(
                is_browser_task(app, goal),
                "{app:?} + {goal:?} should route to CDP"
            );
        }
    }

    #[test]
    fn router_non_browser_foreground_is_false() {
        // Even a perfectly web-shaped goal stays on UIA when the surface isn't a
        // browser.
        for app in [
            "Spotify",
            "Notepad",
            "Microsoft Word",
            "Slack",
            "",
            "Ledger",
        ] {
            assert!(
                !is_browser_task(app, "play the first result"),
                "{app:?} is not a browser; must not route to CDP"
            );
        }
    }

    #[test]
    fn router_browser_plus_os_goal_is_false() {
        // Browser-chrome / OS-window management belongs on UIA even in a browser.
        for goal in [
            "open a new tab",
            "close the window",
            "minimize the browser",
            "open an incognito window",
            "bookmark this page",
            "open the downloads folder",
            "zoom in",
            "quit chrome",
        ] {
            assert!(
                !is_browser_task("chrome.exe", goal),
                "OS/chrome goal {goal:?} must stay on UIA"
            );
        }
    }

    #[test]
    fn router_ambiguous_goal_falls_back_to_uia() {
        // No clear web signal => ambiguous => false (fall back to UIA).
        for goal in ["", "   ", "do the thing", "help me", "continue"] {
            assert!(
                !is_browser_task("chrome.exe", goal),
                "ambiguous goal {goal:?} must fall back to UIA"
            );
        }
    }

    #[test]
    fn router_is_case_insensitive_on_goal() {
        assert!(is_browser_task("chrome.exe", "PLAY the First Result"));
        assert!(!is_browser_task("chrome.exe", "Open A New TAB"));
    }

    // --- mapping: browser_task_for -----------------------------------------

    #[test]
    fn map_play_first_result_on_youtube_is_links_mode() {
        let task = browser_task_for("play the first result on youtube", None);
        assert_eq!(task.mode.as_deref(), Some("links"));
        assert_eq!(task.select, Some(serde_json::Value::from(1u32)));
        assert_eq!(task.url.as_deref(), Some("https://www.youtube.com"));
        let json = serde_json::to_string(&task).unwrap();
        // The exact wire shape the sidecar reads: links driver + numeric select.
        assert!(json.contains("\"mode\":\"links\""), "json: {json}");
        assert!(json.contains("\"select\":1"), "json: {json}");
    }

    #[test]
    fn map_nth_result_picks_that_index() {
        let task = browser_task_for("click the second link", None);
        assert_eq!(task.mode.as_deref(), Some("links"));
        assert_eq!(task.select, Some(serde_json::Value::from(2u32)));
        // No site named → no navigation, just act on the current tab's links.
        assert!(task.url.is_none(), "url: {:?}", task.url);
    }

    #[test]
    fn map_top_result_is_index_one() {
        let task = browser_task_for("open the top result", None);
        assert_eq!(task.mode.as_deref(), Some("links"));
        assert_eq!(task.select, Some(serde_json::Value::from(1u32)));
    }

    #[test]
    fn map_open_bare_domain_is_a_url_act_task() {
        let task = browser_task_for("open example.com", None);
        assert_eq!(task.url.as_deref(), Some("https://example.com"));
        assert!(task.mode.is_none(), "plain open must not be links mode");
        assert!(task.select.is_none());
        // Serialized: url present, no mode/select/timeout.
        let json = serde_json::to_string(&task).unwrap();
        assert!(
            json.contains("\"url\":\"https://example.com\""),
            "json: {json}"
        );
        assert!(!json.contains("mode"), "json: {json}");
        assert!(!json.contains("select"), "json: {json}");
    }

    #[test]
    fn map_explicit_https_url_is_preserved_with_case() {
        let task = browser_task_for("go to https://YouTube.com/watch?v=AbCdEf", None);
        assert_eq!(
            task.url.as_deref(),
            Some("https://YouTube.com/watch?v=AbCdEf")
        );
    }

    #[test]
    fn map_target_app_supplies_the_site_url() {
        // The goal names no site, but the target-app hint does.
        let task = browser_task_for("send this message", Some("Grok"));
        assert_eq!(task.url.as_deref(), Some("https://grok.com"));
        assert!(task.mode.is_none(), "no ordinal → act mode");
    }

    #[test]
    fn map_plain_act_goal_keeps_intent_and_no_mode() {
        let task = browser_task_for("type the prompt into the chat box and send it", None);
        assert_eq!(task.intent, "type the prompt into the chat box and send it");
        assert!(task.mode.is_none());
        assert!(task.select.is_none());
        assert!(task.url.is_none());
    }

    #[test]
    fn map_ordinal_without_result_noun_is_not_links() {
        // "first" with no result/link/video noun is not a result selection.
        let task = browser_task_for("say hello first", None);
        assert!(task.mode.is_none(), "no result noun → not links mode");
    }

    // --- summary: browser_summary ------------------------------------------

    #[test]
    fn summary_includes_chosen_href_when_present() {
        let line = r#"{"ok":true,"detail":"navigated to Lofi","chosenHref":"https://youtu.be/x"}"#;
        let r: BrowserResult = serde_json::from_str(line).unwrap();
        assert_eq!(
            browser_summary(&r),
            "navigated to Lofi (https://youtu.be/x)"
        );
    }

    #[test]
    fn summary_falls_back_to_detail_or_done() {
        let r: BrowserResult =
            serde_json::from_str(r#"{"ok":true,"detail":"clicked play"}"#).unwrap();
        assert_eq!(browser_summary(&r), "clicked play");
        let empty: BrowserResult = serde_json::from_str(r#"{"ok":true,"detail":""}"#).unwrap();
        assert_eq!(browser_summary(&empty), "Done");
    }
}
