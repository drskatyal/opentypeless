//! Tauri command surface for Act mode.
//!
//! The Act engine (`crate::act`) is platform- and transport-agnostic; this module
//! is the thin glue that owns a single [`ActSession`] as Tauri-managed state,
//! (re)builds it from the live config when the user toggles Act on, and forwards
//! the UI's confirm / pick / abort decisions into the engine, emitting the
//! resulting [`ActEvent`]s back over the [`ACT_EVENT`] channel.
//!
//! The dictation pipeline forks into the armed session in `pipeline.rs`; see the
//! "Act fork" there.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::act::capability::CapabilityGate;
use crate::act::executor::{Executor, UserDecision};
use crate::act::killswitch::KillSwitch;
use crate::act::llm::GeminiLlmClient;
use crate::act::planner::Planner;
use crate::act::session::{ActMode, ActSession};
use crate::act::{self};
use crate::storage::{self, ConfigManager};

/// Tauri-managed Act runtime: the live session (armed lazily on enable) plus the
/// shared HTTP client used to (re)build its planner transport.
pub struct ActState {
    /// The current session, or `None` when Act is off. Behind an async mutex so a
    /// command can hold it across the awaits inside the engine.
    pub session: tokio::sync::Mutex<Option<ActSession>>,
    /// The shared HTTP client, reused for the planner's cloud transport.
    pub client: reqwest::Client,
}

impl ActState {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            session: tokio::sync::Mutex::new(None),
            client,
        }
    }
}

/// Map the configured planner tier onto a concrete model id. "precise" trades
/// latency for a stronger planner; anything else uses the fast default.
fn model_for_tier(tier: &str) -> &'static str {
    if tier == "precise" {
        "gemini-3.5-flash"
    } else {
        "gemini-3.1-flash-lite"
    }
}

/// Build a fresh [`ActSession`] from the current config. Act planning is an LLM
/// task, so it reuses the LLM credentials (`llm_api_key`).
fn build_session(client: reqwest::Client, config: &storage::AppConfig) -> ActSession {
    let api_key = config.llm_api_key.clone();
    let model = model_for_tier(&config.act_model_tier).to_string();
    let planner = Planner::new(
        Arc::new(GeminiLlmClient::new(client, api_key, model)),
        config.act_model_tier.clone(),
    );
    let backend = act::create_backend();
    let executor = Executor::new(
        backend.clone(),
        CapabilityGate::new(),
        None,
        KillSwitch::new(),
    );
    let mode = ActMode::from_stt_mode(&config.stt_mode);
    ActSession::new(planner, executor, backend, mode)
}

/// Turn Act on (build + arm a session) or off (disarm + drop it).
///
/// `act_enabled` itself is persisted by the frontend via `update_config`; this
/// command only manages the runtime session so it does not double-write config.
#[tauri::command]
pub async fn act_set_enabled(
    state: State<'_, ActState>,
    config: State<'_, ConfigManager>,
    enabled: bool,
) -> Result<(), String> {
    if enabled && !act::act_supported() {
        return Err("Act mode is only available on Windows in this version.".to_string());
    }

    let mut guard = state.session.lock().await;
    if enabled {
        let cfg = config.load().await.map_err(|e| e.to_string())?;
        let mut session = build_session(state.client.clone(), &cfg);
        session.arm();
        *guard = Some(session);
    } else {
        if let Some(session) = guard.as_mut() {
            session.disarm();
        }
        *guard = None;
    }
    Ok(())
}

/// The current session state name ("idle" when Act is off).
#[tauri::command]
pub async fn act_get_state(state: State<'_, ActState>) -> Result<String, String> {
    let guard = state.session.lock().await;
    Ok(guard
        .as_ref()
        .map(|s| s.state().name().to_string())
        .unwrap_or_else(|| "idle".to_string()))
}

/// Forward the user's answer to a Confirm / ask_user pause into the engine and
/// emit the resulting events.
#[tauri::command]
pub async fn act_user_decision(
    app: AppHandle,
    state: State<'_, ActState>,
    decision: String,
    index: Option<usize>,
) -> Result<(), String> {
    let user_decision = match decision.as_str() {
        "confirm_allow" => UserDecision::ConfirmAllow,
        "confirm_deny" => UserDecision::ConfirmDeny,
        "cancel" => UserDecision::Cancel,
        "ask_user_pick" => UserDecision::AskUserPick {
            index: index.unwrap_or(0),
        },
        other => return Err(format!("Unknown Act decision: {other}")),
    };

    let mut guard = state.session.lock().await;
    let session = guard
        .as_mut()
        .ok_or_else(|| "Act session is not active".to_string())?;
    let events = session
        .on_user_decision(user_decision)
        .await
        .map_err(|e| e.to_string())?;
    for event in &events {
        let _ = app.emit(act::events::ACT_EVENT, event);
    }
    Ok(())
}

/// Trip the kill switch and reset the session to its armed baseline.
#[tauri::command]
pub async fn act_abort(state: State<'_, ActState>) -> Result<(), String> {
    let mut guard = state.session.lock().await;
    if let Some(session) = guard.as_mut() {
        session.abort();
    }
    Ok(())
}
