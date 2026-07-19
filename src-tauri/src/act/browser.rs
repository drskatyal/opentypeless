//! CDP-controlled-Chrome browser automation (SPIKE — feature `cdp-browser`).
//!
//! This module is the Rust half of an isolated experiment: instead of driving
//! web content through UIA/AX (which flattens the DOM, so coordinate clicks miss
//! and focus gets stolen), it hands a browser task to a Node/TypeScript sidecar
//! (`browser-agent/`) that drives a dedicated Chrome over the DevTools Protocol
//! via Stagehand v3. Real DOM, precise clicks, a persistent FlowRad profile.
//!
//! **Status: scaffold only.** It is gated behind the `cdp-browser` Cargo feature
//! (default OFF) and is NOT called from the live conductor / executor / planner.
//! The intended wiring point is marked below (see `ROUTER INTEGRATION POINT`).
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
}

impl BrowserTask {
    /// A task that just acts on whatever page the FlowRad Chrome is already on.
    pub fn new(intent: impl Into<String>) -> Self {
        Self {
            intent: intent.into(),
            url: None,
            timeout_ms: None,
        }
    }

    /// Navigate to `url` first, then act.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
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
// ROUTER INTEGRATION POINT (not wired — spike).
// ===========================================================================
//
// When this graduates from a spike, the conductor's router (super::selection /
// super::conductor) would classify a mission as "browser" (foreground app is a
// browser AND the goal is web content — see docs/act-cdp-browser.md for the
// classifier) and dispatch it here instead of onto the UIA planner/executor:
//
// ```ignore
// // inside the conductor's per-mission dispatch, GATED on cfg(feature = "cdp-browser"):
// if super::browser::is_browser_task(&mission, &snapshot) {
//     let session = super::browser::BrowserSession::from_env();
//     let task = super::browser::BrowserTask::new(goal).with_url(target_url);
//     match session.run(&task) {
//         Ok(res)  => { /* emit ActEvent::Progress(res.detail); continue the loop */ }
//         Err(err) => { /* fall back to the UIA path, or surface the error */ }
//     }
//     return; // handled by CDP; skip the UIA planner/executor for this mission
// }
// ```
//
// Nothing above calls `run` today. The live path (executor.rs, planner.rs,
// selection.rs, conductor.rs) is untouched by this module.

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
}
