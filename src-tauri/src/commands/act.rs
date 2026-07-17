//! Tauri command surface for Act mode.
//!
//! The Act engine (`crate::act`) is platform- and transport-agnostic; this module
//! is the thin glue that owns a single [`Conductor`] as Tauri-managed state,
//! (re)builds it (with the seed drawer) from the live config when the user
//! toggles Act on, and forwards the UI's confirm / pick / abort / undo decisions
//! into the engine, emitting the resulting `ActEvent`s back over the ACT_EVENT
//! channel.
//!
//! The dictation pipeline forks into the armed session in `pipeline.rs`; see the
//! "Act fork" there.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::act::capability::{Capability, CapabilityGate};
use crate::act::conductor::Conductor;
use crate::act::executor::{Executor, UserDecision};
use crate::act::flow_registry::FlowRegistry;
use crate::act::flow_runner::FlowRunner;
use crate::act::killswitch::KillSwitch;
use crate::act::llm::{CerebrasLlmClient, GeminiLlmClient, LlmClient};
use crate::act::planner::Planner;
use crate::act::{self, seed};
use crate::credentials::{
    resolve_cerebras_config_secret, resolve_llm_config_secret, resolve_stt_config_secret,
    CredentialSecretReader, SystemCredentialVault,
};
use crate::storage::{self, ConfigManager};

/// Tauri-managed Act runtime: the live [`Conductor`] (armed lazily on enable)
/// plus the shared HTTP client used to (re)build its LLM transport.
pub struct ActState {
    /// The current Conductor, or `None` when Act is off. Behind an async mutex so
    /// a command can hold it across the awaits inside the engine.
    pub session: tokio::sync::Mutex<Option<Conductor>>,
    /// A clone of the current command's [`KillSwitch`], shared with the live
    /// Conductor's runner AND executor (one switch drives both). Held behind a
    /// *std* (non-async) mutex, OUTSIDE the session lock, so `act_abort` can trip
    /// an in-flight command mid-execution without first acquiring the (long-held)
    /// session lock. Replaced on every enable with the freshly built switch.
    pub kill: std::sync::Mutex<KillSwitch>,
    /// Whether a *dedicated* Act hotkey is bound. When true, only Act-hotkey
    /// recordings route to Act (the dual-hotkey model); when false, the Act
    /// toggle routes every armed recording (the simple model). Set on enable.
    pub has_act_hotkey: std::sync::atomic::AtomicBool,
    /// The shared HTTP client, reused for the LLM cloud transport.
    pub client: reqwest::Client,
}

impl ActState {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            session: tokio::sync::Mutex::new(None),
            kill: std::sync::Mutex::new(KillSwitch::new()),
            has_act_hotkey: std::sync::atomic::AtomicBool::new(false),
            client,
        }
    }
}

/// The Gemini key Act should use. Act's selection + planner are Gemini calls, so
/// it reuses whichever Gemini key is configured.
///
/// At runtime the plaintext key fields on `AppConfig` are cleared — the real
/// secrets live in the OS credential vault (see `credentials.rs`), so this must
/// resolve through the vault exactly like the STT pipeline does, NOT read the
/// (empty) config fields. Preference: the dedicated LLM key **when the LLM
/// provider is Gemini**, else the STT key (the common single-Gemini-key setup).
/// A non-Gemini LLM key (e.g. an OpenAI polish key) would be wrong for Act's
/// Gemini calls, so it is deliberately skipped in favour of the STT key.
fn act_gemini_key<V: CredentialSecretReader>(config: &storage::AppConfig, vault: &V) -> String {
    if config.llm_provider.trim().eq_ignore_ascii_case("gemini") {
        let llm = resolve_llm_config_secret(config, vault).unwrap_or_default();
        if !llm.trim().is_empty() {
            return llm.trim().to_string();
        }
    }
    resolve_stt_config_secret(config, vault)
        .unwrap_or_default()
        .trim()
        .to_string()
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

/// The Cerebras model used for Act's follow-up calls — a large open model served
/// at very high tokens/sec, chosen for lower follow-up latency.
const CEREBRAS_FOLLOWUP_MODEL: &str = "gpt-oss-120b";

/// Build the LlmClient for Act's text-only FOLLOW-UP calls (selection routing,
/// planner, answer). When `act_followup_provider` is "cerebras" AND a Cerebras
/// key resolves from the vault, route those calls through the OpenAI-compatible
/// Cerebras endpoint for lower latency; otherwise fall back to Gemini. The
/// first/audio call is a separate path (`stt/gemini.rs`) and always stays Gemini.
fn build_followup_llm<V: CredentialSecretReader>(
    client: reqwest::Client,
    config: &storage::AppConfig,
    vault: &V,
) -> Arc<dyn LlmClient> {
    if config
        .act_followup_provider
        .trim()
        .eq_ignore_ascii_case("cerebras")
    {
        let key = resolve_cerebras_config_secret(config, vault).unwrap_or_default();
        if !key.trim().is_empty() {
            tracing::info!(
                model = CEREBRAS_FOLLOWUP_MODEL,
                "Act follow-up calls routed to Cerebras"
            );
            return Arc::new(CerebrasLlmClient::new(
                client,
                key.trim().to_string(),
                CEREBRAS_FOLLOWUP_MODEL.to_string(),
            ));
        }
        tracing::warn!(
            "Act follow-up provider is Cerebras but no Cerebras key is configured; using Gemini"
        );
    }
    let api_key = act_gemini_key(config, vault);
    let model = model_for_tier(&config.act_model_tier).to_string();
    tracing::info!(model = %model, "Act follow-up calls routed to Gemini");
    Arc::new(GeminiLlmClient::new(client, api_key, model))
}

/// The Conductor's capability gate. Opening apps, settings pages, and URLs is the
/// whole point of a voice assistant the user explicitly armed, so `AppLaunch` and
/// `NetNavigate` are granted (frictionless open). Shell execution stays Confirm
/// (never granted here) and destructive/system-power capabilities stay Deny.
fn conductor_gate() -> CapabilityGate {
    let mut gate = CapabilityGate::new();
    gate.grant(Capability::AppLaunch);
    gate.grant(Capability::NetNavigate);
    gate
}

/// Build a fresh [`Conductor`] from the current config, loaded with the built-in
/// seed drawer so it works out of the box. Selection + planning are LLM tasks, so
/// it reuses the LLM credentials (`llm_api_key`).
///
/// The caller-supplied `kill` is threaded into BOTH the flow runner and the
/// executor (one switch drives both), so the clone stored in [`ActState`] trips
/// the exact switch every in-flight command races against — lock-free abort.
fn build_conductor(
    client: reqwest::Client,
    config: &storage::AppConfig,
    kill: KillSwitch,
) -> Conductor {
    let llm: Arc<dyn LlmClient> = build_followup_llm(client, config, &SystemCredentialVault);
    let backend = act::create_backend();
    let gate = conductor_gate();

    let registry = FlowRegistry::from_files(seed::builtin_flows());
    let runner = FlowRunner::new(backend.clone(), gate.clone(), kill.clone());
    let planner = Planner::new(llm.clone(), config.act_model_tier.clone());
    let executor = Executor::new(backend.clone(), gate, None, kill);
    Conductor::new(registry, llm, runner, planner, executor, backend)
}

/// Build + arm a fresh Conductor from `config` and store it — plus its kill
/// switch and the dedicated-hotkey flag — into `state`. The single arming path
/// shared by the enable command and startup rehydration.
///
/// A fresh kill switch is minted per arm; its clone in [`ActState`] lets
/// `act_abort` trip the exact switch this Conductor's runner + executor race
/// against.
async fn arm_into(state: &ActState, config: &storage::AppConfig) {
    let kill = KillSwitch::new();
    let mut conductor = build_conductor(state.client.clone(), config, kill.clone());
    conductor.arm();
    let state_name = conductor.state().name();
    *state.kill.lock().unwrap_or_else(|e| e.into_inner()) = kill;
    let has_act_hotkey = config.hotkeys.act.is_some();
    state
        .has_act_hotkey
        .store(has_act_hotkey, std::sync::atomic::Ordering::SeqCst);
    *state.session.lock().await = Some(conductor);
    tracing::info!(
        conductor_state = state_name,
        has_act_hotkey,
        model_tier = %config.act_model_tier,
        "Act session built and armed"
    );
}

/// Rehydrate the in-memory Act session at startup from persisted config.
///
/// `act_enabled` survives restarts, but the live Conductor does not — it is
/// built only when the toggle is flipped. A launch with Act previously on must
/// therefore rebuild + arm it here, or the pipeline sees no armed session and
/// every recording falls back to plain dictation. No-op when Act is off,
/// unsupported, or no Gemini key is configured.
pub async fn rehydrate_if_enabled(state: &ActState, config: &storage::AppConfig) {
    if !config.act_enabled || !act::act_supported() {
        return;
    }
    if act_gemini_key(config, &SystemCredentialVault).is_empty() {
        tracing::warn!(
            "Act is enabled but no Gemini API key is configured; not arming on startup."
        );
        return;
    }
    arm_into(state, config).await;
    tracing::info!("Act rehydrated and armed from persisted config on startup.");
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
    tracing::info!(enabled, "act_set_enabled called");
    if enabled && !act::act_supported() {
        tracing::warn!("Act enable refused: not supported on this platform");
        return Err("Act mode is only available on Windows in this version.".to_string());
    }

    if enabled {
        // Preflight BEFORE touching the session: Act planning needs LLM
        // credentials, so refuse (and store nothing) when none are configured.
        let cfg = config.load().await.map_err(|e| e.to_string())?;
        let has_key = !act_gemini_key(&cfg, &SystemCredentialVault).is_empty();
        tracing::info!(
            has_gemini_key = has_key,
            act_hotkey_bound = cfg.hotkeys.act.is_some(),
            "Act enable preflight"
        );
        if !has_key {
            tracing::warn!("Act enable refused: no Gemini API key configured");
            return Err("Act mode requires a Gemini API key — add one in Settings.".to_string());
        }

        arm_into(&state, &cfg).await;
    } else {
        // Trip FIRST (no session lock) so a live executor is cancelled at its next
        // await rather than silently dropped, then disarm and drop the session.
        state.kill.lock().unwrap_or_else(|e| e.into_inner()).trip();
        let mut guard = state.session.lock().await;
        if let Some(session) = guard.as_mut() {
            session.disarm();
        }
        *guard = None;
        tracing::info!("Act session disarmed and dropped");
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

/// A drawer recipe, summarized for the Settings "what Act can do" list.
#[derive(serde::Serialize)]
pub struct ActFlowInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Example spoken phrases that trigger it.
    pub aliases: Vec<String>,
    /// "leaf" (deterministic) or "branch" (planner-assisted).
    pub kind: String,
    /// The slots the user fills by voice (e.g. the song, the query).
    pub slots: Vec<String>,
}

/// List the drawer recipes (built-in seed pack) so Settings can show the user
/// everything Act can do. Reads the same source the Conductor loads.
#[tauri::command]
pub async fn act_list_flows() -> Result<Vec<ActFlowInfo>, String> {
    use crate::act::flow::FlowKind;
    let flows = seed::builtin_flows()
        .into_iter()
        .map(|f| ActFlowInfo {
            id: f.id,
            name: f.name,
            description: f.description,
            aliases: f.aliases,
            kind: match f.kind {
                FlowKind::Leaf => "leaf",
                FlowKind::Branch => "branch",
            }
            .to_string(),
            slots: f.slots.into_iter().map(|s| s.name).collect(),
        })
        .collect();
    Ok(flows)
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
    let conductor = guard
        .as_mut()
        .ok_or_else(|| "Act session is not active".to_string())?;
    let events = conductor
        .decide(user_decision)
        .await
        .map_err(|e| e.to_string())?;
    for event in &events {
        let _ = app.emit(act::events::ACT_EVENT, event);
    }
    Ok(())
}

/// Undo the last edit — the focused app's own Ctrl+Z. Surfaced as its own command
/// so a dedicated "undo" hotkey / button can reach it without a dictation round.
#[tauri::command]
pub async fn act_undo(app: AppHandle, state: State<'_, ActState>) -> Result<(), String> {
    let mut guard = state.session.lock().await;
    let conductor = guard
        .as_mut()
        .ok_or_else(|| "Act session is not active".to_string())?;
    let events = conductor.undo().await.map_err(|e| e.to_string())?;
    for event in &events {
        let _ = app.emit(act::events::ACT_EVENT, event);
    }
    Ok(())
}

/// Trip the kill switch and reset the session to its armed baseline.
///
/// The trip happens on the ActState-held switch clone, WITHOUT the session lock,
/// so an in-flight command (which holds the session lock for its whole
/// plan+execute) is cancelled at its next backend await. Flipping the session
/// state to its baseline is then best-effort via `try_lock`: if the session is
/// busy we skip it — the executor's own abort path resets the state when the
/// tripped command unwinds.
#[tauri::command]
pub async fn act_abort(state: State<'_, ActState>) -> Result<(), String> {
    // 1. Trip first, lock-free, so a mid-execution command aborts immediately.
    state.kill.lock().unwrap_or_else(|e| e.into_inner()).trip();

    // 2. Best-effort state reset without blocking on a busy session.
    if let Ok(mut guard) = state.session.try_lock() {
        if let Some(session) = guard.as_mut() {
            session.abort();
        }
    }
    Ok(())
}
