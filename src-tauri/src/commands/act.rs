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

use tauri::{AppHandle, Emitter, Manager, State};

use crate::act::capability::{Capability, CapabilityGate};
use crate::act::conductor::{Conductor, ConductorState};
use crate::act::executor::{Executor, UserDecision};
use crate::act::flow_registry::FlowRegistry;
use crate::act::flow_runner::FlowRunner;
use crate::act::killswitch::KillSwitch;
use crate::act::llm::{CerebrasLlmClient, GeminiLlmClient, LlmClient, FOLLOWUP_LLM_TIMEOUT};
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
    /// Fingerprint of the follow-up LLM the *currently armed* Conductor was built
    /// with — `provider|model|hash(key)` (see [`followup_signature`]). Used to
    /// detect, on a settings/credential save, whether the effective follow-up
    /// provider/model/key actually changed so a live rebuild only happens when it
    /// did (never on unrelated saves). Empty when Act is off.
    pub followup_signature: std::sync::Mutex<String>,
}

impl ActState {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            session: tokio::sync::Mutex::new(None),
            kill: std::sync::Mutex::new(KillSwitch::new()),
            has_act_hotkey: std::sync::atomic::AtomicBool::new(false),
            client,
            followup_signature: std::sync::Mutex::new(String::new()),
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

/// Map the configured planner tier onto a concrete model id.
///
/// `precise` — the default — runs `gemini-3.6-flash`; `fast` runs
/// `gemini-3.5-flash-lite`. Both are Gemini 3.x "level" models, so their thinking
/// is pinned to `MINIMAL` (see `llm::thinking_config`) to keep Act's structured
/// selection/planning calls fast: Gemini 3 Flash DEFAULTS to HIGH thinking, which
/// blew the 25s selection timeout until the thinking level was set correctly.
/// (Cerebras `gpt-oss-120b` remains the fastest planner and is preferred with a
/// Cerebras key.)
fn model_for_tier(tier: &str) -> &'static str {
    match tier {
        "fast" => "gemini-3.5-flash-lite",
        // "precise" and any unknown value use the flagship flash tier.
        _ => "gemini-3.6-flash",
    }
}

/// The Cerebras model used for Act's follow-up calls — a large open model served
/// at very high tokens/sec, chosen for lower follow-up latency.
const CEREBRAS_FOLLOWUP_MODEL: &str = "gpt-oss-120b";

/// The concrete follow-up LLM selection (provider tag, model id, resolved key)
/// derived from config + vault. Single source of truth shared by
/// [`build_followup_llm`] (which constructs the client) and
/// [`followup_signature`] (which fingerprints it to detect live changes), so the
/// two can never drift apart.
struct FollowupChoice {
    /// "cerebras" or "gemini".
    provider: &'static str,
    /// The concrete model id passed to the client.
    model: String,
    /// The resolved API key for `provider`.
    key: String,
    /// True when the user selected Cerebras but no Cerebras key resolved, so the
    /// selection fell back to Gemini — surfaced as a warn by the builder.
    cerebras_fallback: bool,
}

/// Decide the effective follow-up provider/model/key. When
/// `act_followup_provider` is "cerebras" AND a Cerebras key resolves from the
/// vault, route follow-up calls through the OpenAI-compatible Cerebras endpoint
/// for lower latency; otherwise fall back to Gemini (the first/audio call is a
/// separate path in `stt/gemini.rs` and always stays Gemini).
fn resolve_followup_choice<V: CredentialSecretReader>(
    config: &storage::AppConfig,
    vault: &V,
) -> FollowupChoice {
    if config
        .act_followup_provider
        .trim()
        .eq_ignore_ascii_case("cerebras")
    {
        let key = resolve_cerebras_config_secret(config, vault).unwrap_or_default();
        if !key.trim().is_empty() {
            return FollowupChoice {
                provider: "cerebras",
                model: CEREBRAS_FOLLOWUP_MODEL.to_string(),
                key: key.trim().to_string(),
                cerebras_fallback: false,
            };
        }
        return FollowupChoice {
            provider: "gemini",
            model: model_for_tier(&config.act_model_tier).to_string(),
            key: act_gemini_key(config, vault),
            cerebras_fallback: true,
        };
    }
    FollowupChoice {
        provider: "gemini",
        model: model_for_tier(&config.act_model_tier).to_string(),
        key: act_gemini_key(config, vault),
        cerebras_fallback: false,
    }
}

/// A stable fingerprint of the effective Act session config —
/// `provider|model|plan_mode|hash(key)`. The key is hashed (never retained in
/// plaintext) so two saves that resolve to the same values compare equal and no
/// live rebuild is triggered; any real provider, model, key, OR perception-mode
/// change flips it (so switching Perception mode in Settings rebuilds the
/// Conductor with the new mode, instead of the change being ignored until
/// restart).
fn followup_signature<V: CredentialSecretReader>(config: &storage::AppConfig, vault: &V) -> String {
    use std::hash::{Hash, Hasher};
    let choice = resolve_followup_choice(config, vault);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    choice.key.hash(&mut hasher);
    format!(
        "{}|{}|{}|{:016x}",
        choice.provider,
        choice.model,
        act::plan_mode::PlanMode::from_config(&config.act_plan_mode).as_str(),
        hasher.finish()
    )
}

/// Build the LlmClient for Act's text-only FOLLOW-UP calls (selection routing,
/// planner, answer) from the selection in [`resolve_followup_choice`].
fn build_followup_llm<V: CredentialSecretReader>(
    client: reqwest::Client,
    config: &storage::AppConfig,
    vault: &V,
) -> Arc<dyn LlmClient> {
    let choice = resolve_followup_choice(config, vault);
    if choice.cerebras_fallback {
        tracing::warn!(
            "Act follow-up provider is Cerebras but no Cerebras key is configured; using Gemini"
        );
    }
    match choice.provider {
        "cerebras" => {
            tracing::info!(
                provider = "cerebras",
                model = %choice.model,
                timeout_secs = FOLLOWUP_LLM_TIMEOUT.as_secs(),
                "Act follow-up calls routed to Cerebras"
            );
            Arc::new(CerebrasLlmClient::new(client, choice.key, choice.model))
        }
        _ => {
            tracing::info!(
                provider = "gemini",
                model = %choice.model,
                timeout_secs = FOLLOWUP_LLM_TIMEOUT.as_secs(),
                "Act follow-up calls routed to Gemini"
            );
            Arc::new(GeminiLlmClient::new(client, choice.key, choice.model))
        }
    }
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
    let llm: Arc<dyn LlmClient> =
        build_followup_llm(client.clone(), config, &SystemCredentialVault);
    let backend = act::create_backend();
    let gate = conductor_gate();

    // A dedicated multimodal (Gemini) client for the planner's screenshot modes.
    // The follow-up `llm` above may be text-only (Cerebras), which cannot see the
    // hybrid/vision screenshot; giving the planner its own Gemini client lets those
    // modes work regardless of the follow-up provider. Built only when a Gemini key
    // resolves — otherwise `None`, and the planner's guard degrades screenshot modes
    // to tree (the correct fallback).
    let gemini_key = act_gemini_key(config, &SystemCredentialVault);
    let vision_llm: Option<Arc<dyn LlmClient>> = if gemini_key.is_empty() {
        tracing::debug!(
            "Act planner vision client not attached: no Gemini key (screenshot modes degrade to tree)"
        );
        None
    } else {
        tracing::info!("Act planner vision client attached (Gemini) for hybrid/vision modes");
        Some(Arc::new(GeminiLlmClient::new(
            client,
            gemini_key,
            model_for_tier(&config.act_model_tier).to_string(),
        )))
    };

    let registry = FlowRegistry::from_files(seed::builtin_flows());
    let runner = FlowRunner::new(backend.clone(), gate.clone(), kill.clone());
    let planner =
        Planner::new(llm.clone(), config.act_model_tier.clone()).with_vision_llm(vision_llm);
    let executor = Executor::new(backend.clone(), gate, None, kill);
    let mut conductor = Conductor::new(registry, llm, runner, planner, executor, backend);
    conductor.set_plan_mode(act::plan_mode::PlanMode::from_config(&config.act_plan_mode));
    conductor
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
    *state
        .followup_signature
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = followup_signature(config, &SystemCredentialVault);
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

/// Rebuild the live Act session's follow-up LLM in place when the effective
/// provider/model/key changed — no app restart.
///
/// Called after a settings save (provider tier via `update_config`) and after a
/// credential change (Cerebras/Gemini keys via `set_credential` /
/// `clear_credential`). The bug this fixes: the follow-up LlmClient was built
/// once at arm time, so switching Gemini <-> Cerebras (or rotating the key) only
/// took effect on restart.
///
/// Safety / no-clobber contract:
/// * No-op unless Act is enabled AND actually armed. When Act is off the session
///   is `None` and we do nothing (the next arm rebuilds fresh from config).
/// * The new selection is fingerprinted ([`followup_signature`]); if it matches
///   the armed one, nothing is rebuilt — unrelated settings saves never churn
///   the Conductor.
/// * The replacement Conductor is built BEFORE the session lock is taken (cheap:
///   it only constructs HTTP-backed clients), so the lock is held just for the
///   swap.
/// * The swap acquires the session mutex with `.lock().await`. An in-flight
///   command holds that same mutex for its whole plan+execute, so we wait for it
///   to finish and swap on the next idle — the running mission is preserved, not
///   dropped. The engine never re-enters the session lock (only `act.rs` /
///   `dev_relay.rs` touch it), so this cannot deadlock.
pub async fn refresh_followup_if_changed(state: &ActState, config: &storage::AppConfig) {
    if !config.act_enabled || !act::act_supported() {
        return;
    }
    let new_signature = followup_signature(config, &SystemCredentialVault);
    if *state
        .followup_signature
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        == new_signature
    {
        return;
    }

    // Build the replacement up front so the session lock is held only for the swap.
    let kill = KillSwitch::new();
    let mut conductor = build_conductor(state.client.clone(), config, kill.clone());
    conductor.arm();
    let state_name = conductor.state().name();

    // Acquire the session lock. If a command is running it holds this lock across
    // its whole execution; awaiting here lets it finish, THEN swaps.
    let mut guard = state.session.lock().await;
    match guard.as_ref() {
        None => {
            // Not actually armed (Act toggled off between our check and here, or the
            // session was temporarily borrowed by the dev relay). Do not resurrect it;
            // the next arm rebuilds fresh from config.
            return;
        }
        Some(existing) if *existing.state() != ConductorState::Armed => {
            // The session is mid-mission — actively working is impossible here (that
            // holds this same lock), so this means it is PAUSED awaiting a confirm /
            // pick, with a `pending` continuation stored. Swapping in a fresh
            // Conductor would silently drop that pending mission, and the user's next
            // decision would fail with "session is not active". Defer instead: leave
            // the running Conductor in place (the provider change applies to the next
            // command) and leave the stored signature stale so a later idle save
            // re-attempts the swap.
            tracing::info!(
                state = existing.state().name(),
                "Act follow-up change deferred: session is mid-mission (paused); \
                 will apply on the next command or re-arm"
            );
            return;
        }
        Some(_) => {}
    }
    *state.kill.lock().unwrap_or_else(|e| e.into_inner()) = kill;
    state.has_act_hotkey.store(
        config.hotkeys.act.is_some(),
        std::sync::atomic::Ordering::SeqCst,
    );
    *guard = Some(conductor);
    *state
        .followup_signature
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = new_signature;
    drop(guard);
    tracing::info!(
        conductor_state = state_name,
        "Act follow-up provider/key changed in settings; session rebuilt live (no restart)"
    );
}

/// Spawn a background [`refresh_followup_if_changed`] from the latest persisted
/// config. Used by the synchronous credential-save commands, which must not block
/// on a config load + keyring read; the vault write has already completed by the
/// time this runs, so the refresh reads the new key.
pub fn spawn_followup_refresh(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<ActState>() else {
            return;
        };
        let Some(config_manager) = app.try_state::<ConfigManager>() else {
            return;
        };
        let config = match config_manager.load().await {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!("Act follow-up refresh skipped: failed to load config: {error}");
                return;
            }
        };
        refresh_followup_if_changed(&state, &config).await;
    });
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
        return Err("Act mode is only available on Windows and macOS in this version.".to_string());
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
        state
            .followup_signature
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
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
