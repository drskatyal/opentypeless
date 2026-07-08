use crate::hotkey::{HotkeySupervisor, HotkeySupervisorSnapshot, HotkeySupervisorState};
use crate::native_hotkey::{NativeHotkeyBinding, NativeHotkeyRuntime};
use crate::pipeline;
use crate::platform;
use crate::storage;
use crate::tray;
use crate::AskHotkeyCache;
use crate::HotkeyRegistrationError;
use crate::HotkeyRoleCache;
use serde::Serialize;
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticStatus {
    Ok,
    Warning,
    Error,
    NotApplicable,
    Checking,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticRow {
    pub id: String,
    pub status: DiagnosticStatus,
    pub message: String,
    pub action: Option<String>,
    pub last_checked_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SystemDiagnosticsReport {
    pub checked_at: String,
    pub rows: Vec<DiagnosticRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeResult {
    ok: bool,
    message: String,
}

impl ProbeResult {
    fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
        }
    }

    fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HotkeyBindingStatus {
    pub value: String,
    pub valid: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatusError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyRoleStatus {
    pub role: String,
    pub adapter: String,
    pub state: String,
    pub message: Option<String>,
    pub last_error: Option<HotkeyStatusError>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyCapability {
    pub platform: String,
    pub session_type: String,
    pub supports_global_hotkey: bool,
    pub supports_hold_mode: bool,
    pub supports_released_edge: bool,
    pub supports_side_specific_modifiers: bool,
    pub requires_accessibility_permission: bool,
    pub status_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HotkeyStatus {
    pub dictation: HotkeyBindingStatus,
    pub ask: HotkeyBindingStatus,
    pub conflict: bool,
    pub registration_error: Option<String>,
    pub roles: Vec<HotkeyRoleStatus>,
    pub capability: HotkeyCapability,
}

pub(crate) const HOTKEY_REGISTRATION_SUPERSEDED_ERROR: &str = "hotkey registration superseded";

pub(crate) fn run_if_hotkey_generation_current<F>(
    supervisor: &HotkeySupervisor,
    generation: u64,
    register: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    supervisor
        .run_if_current_generation(generation, register)
        .unwrap_or_else(|| Err(HOTKEY_REGISTRATION_SUPERSEDED_ERROR.to_string()))
}

pub(crate) fn register_configured_shortcuts(
    app: &tauri::AppHandle,
    config: &storage::AppConfig,
) -> Result<(), String> {
    register_configured_shortcuts_guarded(app, config)
}

pub(crate) fn register_configured_shortcuts_for_generation(
    app: &tauri::AppHandle,
    config: &storage::AppConfig,
    supervisor: &HotkeySupervisor,
    generation: u64,
) -> Result<(), String> {
    run_if_hotkey_generation_current(supervisor, generation, || {
        register_configured_shortcuts_guarded(app, config)
    })
}

fn register_configured_shortcuts_guarded(
    app: &tauri::AppHandle,
    config: &storage::AppConfig,
) -> Result<(), String> {
    let hotkeys = effective_hotkey_config(config);
    let plan =
        crate::hotkey::hotkey_registration_plan_from_config(&hotkeys).map_err(|e| e.to_string())?;

    app.global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string())?;

    for registered in &plan.global {
        app.global_shortcut()
            .register(registered.shortcut)
            .map_err(|e| e.to_string())?;
    }

    if let Some(native_runtime) = app.try_state::<NativeHotkeyRuntime>() {
        let native_bindings: Vec<NativeHotkeyBinding> = plan
            .native
            .iter()
            .map(|registered| NativeHotkeyBinding {
                role: registered.role,
                trigger: registered.trigger,
            })
            .collect();
        let handle = app.clone();
        native_runtime.install(
            native_bindings,
            Arc::new(move |event| {
                crate::hotkey::handle_hotkey_role_event(handle.clone(), event.role, event.state);
            }),
        )?;
    }

    if let Some(role_cache) = app.try_state::<HotkeyRoleCache>() {
        *role_cache.0.lock().unwrap_or_else(|e| e.into_inner()) = plan;
    }

    Ok(())
}

fn effective_hotkey_config(config: &storage::AppConfig) -> storage::HotkeyConfig {
    let typed_dictation = config.hotkeys.dictation.to_hotkey_string();
    let typed_ask = config
        .hotkeys
        .ask
        .as_ref()
        .and_then(storage::ShortcutBinding::to_hotkey_string);
    let ask_matches = typed_ask
        .as_deref()
        .map(|value| value == config.ask_hotkey)
        .unwrap_or_else(|| config.ask_hotkey.is_empty());

    if typed_dictation.as_deref() != Some(config.hotkey.as_str())
        || !ask_matches
        || config.hotkeys.dictation_mode != config.hotkey_mode
    {
        return storage::HotkeyConfig::from_legacy(
            &config.hotkey,
            &config.ask_hotkey,
            &config.hotkey_mode,
        );
    }

    config.hotkeys.clone()
}

#[cfg(test)]
fn hotkey_status_for(
    config: &storage::AppConfig,
    registration_error: Option<String>,
) -> HotkeyStatus {
    hotkey_status_for_with_capability(config, registration_error, platform::capabilities())
}

fn hotkey_capability_for(caps: &platform::PlatformCapabilities) -> HotkeyCapability {
    let is_linux_wayland = caps.os == "linux" && caps.session_type == "wayland";
    let supports_native_single_key = matches!(caps.os.as_str(), "macos" | "windows");
    HotkeyCapability {
        platform: caps.os.clone(),
        session_type: caps.session_type.clone(),
        supports_global_hotkey: !is_linux_wayland,
        supports_hold_mode: !is_linux_wayland,
        supports_released_edge: !is_linux_wayland || supports_native_single_key,
        supports_side_specific_modifiers: supports_native_single_key,
        requires_accessibility_permission: caps.os == "macos",
        status_hint: is_linux_wayland.then(|| "linuxWaylandLimited".to_string()),
    }
}

fn hotkey_role_status(
    role: &str,
    binding: Option<&storage::ShortcutBinding>,
    validation_error: Option<&crate::hotkey::HotkeyPairError>,
    registration_error: Option<&str>,
    supervisor: Option<&HotkeySupervisorSnapshot>,
    capability: &HotkeyCapability,
) -> HotkeyRoleStatus {
    let Some(binding) = binding else {
        return HotkeyRoleStatus {
            role: role.to_string(),
            adapter: "unavailable".to_string(),
            state: "disabled".to_string(),
            message: None,
            last_error: None,
        };
    };
    let adapter = if crate::hotkey::native_trigger_from_binding(binding).is_some() {
        "nativeHook"
    } else {
        "tauriGlobalShortcut"
    };

    if let Some(error) = validation_error {
        return HotkeyRoleStatus {
            role: role.to_string(),
            adapter: adapter.to_string(),
            state: "failed".to_string(),
            message: Some(error.to_string()),
            last_error: Some(HotkeyStatusError {
                code: "invalidBinding".to_string(),
                message: error.to_string(),
            }),
        };
    }

    if !crate::hotkey::binding_is_valid_for_registration(binding) {
        return HotkeyRoleStatus {
            role: role.to_string(),
            adapter: adapter.to_string(),
            state: "failed".to_string(),
            message: Some("Invalid shortcut binding".to_string()),
            last_error: Some(HotkeyStatusError {
                code: "invalidBinding".to_string(),
                message: "Invalid shortcut binding".to_string(),
            }),
        };
    }

    if let Some(snapshot) = supervisor {
        if snapshot.state == HotkeySupervisorState::Starting {
            return HotkeyRoleStatus {
                role: role.to_string(),
                adapter: adapter.to_string(),
                state: "starting".to_string(),
                message: snapshot.last_error.clone(),
                last_error: snapshot.last_error.as_ref().map(|error| HotkeyStatusError {
                    code: "registrationFailed".to_string(),
                    message: error.clone(),
                }),
            };
        }

        if snapshot.state == HotkeySupervisorState::Disabled {
            return HotkeyRoleStatus {
                role: role.to_string(),
                adapter: "unavailable".to_string(),
                state: "disabled".to_string(),
                message: None,
                last_error: None,
            };
        }
    }

    if let Some(error) = registration_error {
        return HotkeyRoleStatus {
            role: role.to_string(),
            adapter: adapter.to_string(),
            state: "failed".to_string(),
            message: Some(error.to_string()),
            last_error: Some(HotkeyStatusError {
                code: "registrationFailed".to_string(),
                message: error.to_string(),
            }),
        };
    }

    HotkeyRoleStatus {
        role: role.to_string(),
        adapter: adapter.to_string(),
        state: "installed".to_string(),
        message: capability.status_hint.clone(),
        last_error: None,
    }
}

#[cfg(test)]
fn hotkey_status_for_with_capability(
    config: &storage::AppConfig,
    registration_error: Option<String>,
    caps: platform::PlatformCapabilities,
) -> HotkeyStatus {
    hotkey_status_for_with_capability_and_supervisor(config, registration_error, caps, None)
}

fn hotkey_status_for_with_capability_and_supervisor(
    config: &storage::AppConfig,
    registration_error: Option<String>,
    caps: platform::PlatformCapabilities,
    supervisor: Option<HotkeySupervisorSnapshot>,
) -> HotkeyStatus {
    let hotkeys = effective_hotkey_config(config);
    let dictation_value = hotkeys
        .dictation
        .to_hotkey_string()
        .unwrap_or_else(|| config.hotkey.clone());
    let ask_value = hotkeys
        .ask
        .as_ref()
        .and_then(storage::ShortcutBinding::to_hotkey_string)
        .unwrap_or_else(|| config.ask_hotkey.clone());
    let validation_result = crate::hotkey::validate_hotkey_config(&hotkeys);
    let validation_error = validation_result.as_ref().err();
    let capability = hotkey_capability_for(&caps);
    let supervisor_error = supervisor
        .as_ref()
        .and_then(|state| state.last_error.clone());
    let effective_registration_error = registration_error.or(supervisor_error);
    let registration_error_ref = effective_registration_error.as_deref();
    let supervisor_ref = supervisor.as_ref();
    let role_bindings = [
        (
            crate::hotkey::HotkeyRole::Dictation.as_str(),
            Some(&hotkeys.dictation),
        ),
        (
            crate::hotkey::HotkeyRole::Ask.as_str(),
            hotkeys.ask.as_ref(),
        ),
        (
            crate::hotkey::HotkeyRole::TranslateSelection.as_str(),
            hotkeys.translate.as_ref(),
        ),
        (
            crate::hotkey::HotkeyRole::EditSelection.as_str(),
            hotkeys.edit_selection.as_ref(),
        ),
        (
            crate::hotkey::HotkeyRole::SwitchScene.as_str(),
            hotkeys.switch_scene.as_ref(),
        ),
        (
            crate::hotkey::HotkeyRole::OpenApp.as_str(),
            hotkeys.open_app.as_ref(),
        ),
    ];
    let roles = role_bindings
        .into_iter()
        .map(|(role, binding)| {
            hotkey_role_status(
                role,
                binding,
                validation_error,
                registration_error_ref,
                supervisor_ref,
                &capability,
            )
        })
        .collect();

    HotkeyStatus {
        dictation: HotkeyBindingStatus {
            value: dictation_value,
            valid: crate::hotkey::binding_is_valid_for_registration(&hotkeys.dictation),
        },
        ask: HotkeyBindingStatus {
            value: ask_value,
            valid: hotkeys.ask.is_none()
                || hotkeys
                    .ask
                    .as_ref()
                    .is_some_and(crate::hotkey::binding_is_valid_for_registration),
        },
        conflict: matches!(
            validation_result,
            Err(crate::hotkey::HotkeyPairError::ConflictingHotkeys)
        ),
        registration_error: effective_registration_error,
        roles,
        capability,
    }
}

fn diagnostic_row(
    id: &str,
    status: DiagnosticStatus,
    message: impl Into<String>,
    action: Option<&str>,
    checked_at: &str,
) -> DiagnosticRow {
    DiagnosticRow {
        id: id.to_string(),
        status,
        message: message.into(),
        action: action.map(str::to_string),
        last_checked_at: checked_at.to_string(),
    }
}

fn microphone_diagnostic_row(probe: ProbeResult, checked_at: &str) -> DiagnosticRow {
    let (status, action) = if probe.ok {
        (DiagnosticStatus::Ok, None)
    } else {
        (DiagnosticStatus::Error, Some("openSystemSoundSettings"))
    };
    diagnostic_row("microphone", status, probe.message, action, checked_at)
}

fn accessibility_diagnostic_row(
    config: &storage::AppConfig,
    caps: &platform::PlatformCapabilities,
    trusted: bool,
    checked_at: &str,
) -> DiagnosticRow {
    if !config_requires_accessibility_permission(config, caps) {
        return diagnostic_row(
            "accessibility",
            DiagnosticStatus::NotApplicable,
            "Not required for the current output mode",
            None,
            checked_at,
        );
    }

    if trusted {
        diagnostic_row(
            "accessibility",
            DiagnosticStatus::Ok,
            "Accessibility permission is granted",
            None,
            checked_at,
        )
    } else {
        diagnostic_row(
            "accessibility",
            DiagnosticStatus::Error,
            "Accessibility permission is required for the Fn shortcut or keyboard output",
            Some("openAccessibilitySettings"),
            checked_at,
        )
    }
}

fn config_requires_accessibility_permission(
    config: &storage::AppConfig,
    caps: &platform::PlatformCapabilities,
) -> bool {
    if caps.os != "macos" {
        return false;
    }

    config.output_mode == "keyboard" || config_uses_macos_native_hotkey(config)
}

fn config_uses_macos_native_hotkey(config: &storage::AppConfig) -> bool {
    let hotkeys = effective_hotkey_config(config);
    crate::hotkey::hotkey_registration_plan_from_config(&hotkeys)
        .map(|plan| {
            plan.native.iter().any(|registered| {
                registered.trigger == crate::native_hotkey::NativeHotkeyTrigger::Fn
            })
        })
        .unwrap_or(false)
}

fn hotkey_diagnostic_row(
    status: HotkeyStatus,
    caps: &platform::PlatformCapabilities,
    checked_at: &str,
) -> DiagnosticRow {
    if status.roles.iter().any(|role| role.state == "starting") {
        return diagnostic_row(
            "hotkey",
            DiagnosticStatus::Checking,
            "Global hotkeys are being registered",
            None,
            checked_at,
        );
    }

    if let Some(error) = status.registration_error {
        return diagnostic_row(
            "hotkey",
            DiagnosticStatus::Error,
            error,
            Some("reviewHotkeys"),
            checked_at,
        );
    }

    if status.conflict {
        return diagnostic_row(
            "hotkey",
            DiagnosticStatus::Error,
            "Dictation and Ask hotkeys conflict",
            Some("reviewHotkeys"),
            checked_at,
        );
    }

    if !status.dictation.valid || !status.ask.valid {
        return diagnostic_row(
            "hotkey",
            DiagnosticStatus::Error,
            "One or more hotkeys are invalid",
            Some("reviewHotkeys"),
            checked_at,
        );
    }

    if !caps.global_hotkey_reliable {
        return diagnostic_row(
            "hotkey",
            DiagnosticStatus::Warning,
            "Global hotkeys may be limited in this desktop session",
            None,
            checked_at,
        );
    }

    diagnostic_row(
        "hotkey",
        DiagnosticStatus::Ok,
        "Global hotkeys are configured",
        None,
        checked_at,
    )
}

fn clipboard_diagnostic_row(probe: ProbeResult, checked_at: &str) -> DiagnosticRow {
    let (status, action) = if probe.ok {
        (DiagnosticStatus::Ok, None)
    } else {
        (DiagnosticStatus::Error, Some("retryDiagnostics"))
    };
    let message = if probe.ok {
        format!("{} (text-only restore)", probe.message)
    } else {
        probe.message
    };
    diagnostic_row("clipboard", status, message, action, checked_at)
}

fn insertion_diagnostic_row(
    config: &storage::AppConfig,
    caps: &platform::PlatformCapabilities,
    checked_at: &str,
) -> DiagnosticRow {
    let uses_clipboard_paste = config.insertion_strategy == "clipboardPaste"
        || (config.insertion_strategy == "auto" && config.output_mode == "clipboard");
    let uses_keyboard = config.insertion_strategy == "keyboard"
        || config.insertion_strategy == "windowsSendInput"
        || (config.insertion_strategy == "auto" && config.output_mode == "keyboard");

    if uses_clipboard_paste && !caps.clipboard_auto_paste_reliable {
        return diagnostic_row(
            "insertion",
            DiagnosticStatus::Warning,
            "Clipboard paste automation may fall back to copy-only",
            None,
            checked_at,
        );
    }

    if uses_keyboard && !caps.keyboard_output_reliable {
        return diagnostic_row(
            "insertion",
            DiagnosticStatus::Warning,
            "Keyboard output may be limited in this desktop session",
            None,
            checked_at,
        );
    }

    diagnostic_row(
        "insertion",
        DiagnosticStatus::Ok,
        "Text insertion mode is compatible",
        None,
        checked_at,
    )
}

fn platform_diagnostic_row(
    caps: &platform::PlatformCapabilities,
    checked_at: &str,
) -> DiagnosticRow {
    if caps.os == "linux" && caps.session_type == "wayland" {
        return diagnostic_row(
            "platform",
            DiagnosticStatus::Warning,
            "Linux Wayland can restrict global shortcuts and input automation",
            None,
            checked_at,
        );
    }

    diagnostic_row(
        "platform",
        DiagnosticStatus::Ok,
        format!("{} / {}", caps.os, caps.session_type),
        None,
        checked_at,
    )
}

fn build_system_diagnostics_report(
    config: &storage::AppConfig,
    caps: platform::PlatformCapabilities,
    hotkey_status: HotkeyStatus,
    accessibility_trusted: bool,
    microphone_probe: ProbeResult,
    clipboard_probe: ProbeResult,
    checked_at: &str,
) -> SystemDiagnosticsReport {
    let rows = vec![
        microphone_diagnostic_row(microphone_probe, checked_at),
        accessibility_diagnostic_row(config, &caps, accessibility_trusted, checked_at),
        hotkey_diagnostic_row(hotkey_status, &caps, checked_at),
        clipboard_diagnostic_row(clipboard_probe, checked_at),
        insertion_diagnostic_row(config, &caps, checked_at),
        platform_diagnostic_row(&caps, checked_at),
    ];

    SystemDiagnosticsReport {
        checked_at: checked_at.to_string(),
        rows,
    }
}

fn probe_microphone_input() -> ProbeResult {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let Some(device) = host.default_input_device() else {
        return ProbeResult::err("No default microphone input was found");
    };

    match device.default_input_config() {
        Ok(config) => ProbeResult::ok(format!(
            "{} / {} Hz",
            device
                .name()
                .unwrap_or_else(|_| "Default microphone".to_string()),
            config.sample_rate().0
        )),
        Err(error) => ProbeResult::err(format!("Microphone input is unavailable: {error}")),
    }
}

fn probe_clipboard_write_restore() -> ProbeResult {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(clipboard) => clipboard,
        Err(error) => return ProbeResult::err(format!("Clipboard is unavailable: {error}")),
    };

    let Ok(original_text) = clipboard.get_text() else {
        return ProbeResult::ok("Clipboard is available");
    };

    let sentinel = format!(
        "opentypeless-diagnostic-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    if let Err(error) = clipboard.set_text(sentinel.clone()) {
        return ProbeResult::err(format!("Clipboard write failed: {error}"));
    }

    let read_back = clipboard.get_text().unwrap_or_default();
    let restore_result = clipboard.set_text(original_text);

    if let Err(error) = restore_result {
        return ProbeResult::err(format!("Clipboard restore failed: {error}"));
    }

    if read_back == sentinel {
        ProbeResult::ok("Clipboard write and text restore succeeded")
    } else {
        ProbeResult::err("Clipboard write verification failed")
    }
}

fn current_diagnostics_timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

#[tauri::command]
pub fn refresh_tray_labels(app: tauri::AppHandle) -> Result<(), String> {
    tray::refresh_tray(&app);
    Ok(())
}

#[tauri::command]
pub fn check_accessibility_permission() -> bool {
    pipeline::is_accessibility_trusted()
}

#[tauri::command]
pub fn request_accessibility_permission() -> bool {
    pipeline::request_accessibility_permission()
}

#[tauri::command]
pub fn get_platform_capabilities() -> platform::PlatformCapabilities {
    platform::capabilities()
}

#[tauri::command]
pub fn get_hotkey_registration_error(
    state: tauri::State<'_, HotkeyRegistrationError>,
) -> Option<String> {
    state.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[tauri::command]
pub async fn get_hotkey_status(
    config_state: tauri::State<'_, storage::ConfigManager>,
    hotkey_error: tauri::State<'_, HotkeyRegistrationError>,
    hotkey_supervisor: tauri::State<'_, HotkeySupervisor>,
) -> Result<HotkeyStatus, String> {
    let config = config_state.load().await.map_err(|e| e.to_string())?;
    let registration_error = hotkey_error
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    Ok(hotkey_status_for_with_capability_and_supervisor(
        &config,
        registration_error,
        platform::capabilities(),
        Some(hotkey_supervisor.snapshot()),
    ))
}

#[tauri::command]
pub async fn get_system_diagnostics(
    config_state: tauri::State<'_, storage::ConfigManager>,
    hotkey_error: tauri::State<'_, HotkeyRegistrationError>,
    hotkey_supervisor: tauri::State<'_, HotkeySupervisor>,
) -> Result<SystemDiagnosticsReport, String> {
    let config = config_state.load().await.map_err(|e| e.to_string())?;
    let registration_error = hotkey_error
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let caps = platform::capabilities();
    let hotkey_status = hotkey_status_for_with_capability_and_supervisor(
        &config,
        registration_error,
        caps.clone(),
        Some(hotkey_supervisor.snapshot()),
    );
    let accessibility_trusted = pipeline::is_accessibility_trusted();
    let checked_at = current_diagnostics_timestamp();

    Ok(build_system_diagnostics_report(
        &config,
        caps,
        hotkey_status,
        accessibility_trusted,
        probe_microphone_input(),
        probe_clipboard_write_restore(),
        &checked_at,
    ))
}

#[tauri::command]
pub async fn update_hotkey(
    app: tauri::AppHandle,
    config_state: tauri::State<'_, storage::ConfigManager>,
    hotkey_error: tauri::State<'_, HotkeyRegistrationError>,
    hotkey_supervisor: tauri::State<'_, HotkeySupervisor>,
    hotkey: String,
) -> Result<(), String> {
    let previous = config_state.load().await.map_err(|e| e.to_string())?;
    let mut config = previous.clone();
    config.hotkey = hotkey;
    config.normalize_values();

    let generation = hotkey_supervisor.wake_for_settings_change();
    if let Err(error) = crate::commands::config::refresh_hotkey_runtime_with_rollback(
        &previous,
        &config,
        |candidate| register_configured_shortcuts(&app, candidate),
    ) {
        if let Some(rollback_error) = error.rollback_error {
            hotkey_supervisor.record_registration_failure(generation, rollback_error.clone());
            *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(rollback_error);
        } else {
            hotkey_supervisor.record_registration_success(generation);
            *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        return Err(error.registration_error);
    }
    hotkey_supervisor.record_registration_success(generation);
    *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = None;

    if let Err(error) = config_state.save(&config).await.map_err(|e| e.to_string()) {
        let rollback_generation = hotkey_supervisor.wake_for_settings_change();
        if let Err(rollback_error) = register_configured_shortcuts(&app, &previous) {
            hotkey_supervisor
                .record_registration_failure(rollback_generation, rollback_error.clone());
            *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(rollback_error);
        } else {
            hotkey_supervisor.record_registration_success(rollback_generation);
            *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        return Err(error);
    }

    Ok(())
}

#[tauri::command]
pub async fn update_ask_hotkey(
    app: tauri::AppHandle,
    config_state: tauri::State<'_, storage::ConfigManager>,
    hotkey_error: tauri::State<'_, HotkeyRegistrationError>,
    hotkey_supervisor: tauri::State<'_, HotkeySupervisor>,
    ask_cache: tauri::State<'_, AskHotkeyCache>,
    hotkey: String,
) -> Result<(), String> {
    let previous = config_state.load().await.map_err(|e| e.to_string())?;
    let mut config = previous.clone();
    config.ask_hotkey = hotkey;
    config.normalize_values();

    let generation = hotkey_supervisor.wake_for_settings_change();
    if let Err(error) = crate::commands::config::refresh_hotkey_runtime_with_rollback(
        &previous,
        &config,
        |candidate| register_configured_shortcuts(&app, candidate),
    ) {
        if let Some(rollback_error) = error.rollback_error {
            hotkey_supervisor.record_registration_failure(generation, rollback_error.clone());
            *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(rollback_error);
        } else {
            hotkey_supervisor.record_registration_success(generation);
            *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        return Err(error.registration_error);
    }
    hotkey_supervisor.record_registration_success(generation);
    *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *ask_cache.0.lock().unwrap_or_else(|e| e.into_inner()) = config.ask_hotkey.clone();

    if let Err(error) = config_state.save(&config).await.map_err(|e| e.to_string()) {
        let rollback_generation = hotkey_supervisor.wake_for_settings_change();
        if let Err(rollback_error) = register_configured_shortcuts(&app, &previous) {
            hotkey_supervisor
                .record_registration_failure(rollback_generation, rollback_error.clone());
            *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(rollback_error);
        } else {
            hotkey_supervisor.record_registration_success(rollback_generation);
            *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        *ask_cache.0.lock().unwrap_or_else(|e| e.into_inner()) = previous.ask_hotkey;
        return Err(error);
    }

    Ok(())
}

/// Temporarily unregister all global shortcuts so the webview can capture key events.
#[tauri::command]
pub fn pause_hotkey(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(hotkey_supervisor) = app.try_state::<HotkeySupervisor>() {
        hotkey_supervisor.disable();
    }

    let result = app
        .global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string());

    if let Some(native_runtime) = app.try_state::<NativeHotkeyRuntime>() {
        native_runtime.pause();
    }

    result
}

/// Re-register the current hotkey from config after recording is done.
#[tauri::command]
pub async fn resume_hotkey(
    app: tauri::AppHandle,
    config_state: tauri::State<'_, storage::ConfigManager>,
    hotkey_error: tauri::State<'_, HotkeyRegistrationError>,
    hotkey_supervisor: tauri::State<'_, HotkeySupervisor>,
) -> Result<(), String> {
    let config = config_state.load().await.map_err(|e| e.to_string())?;
    let generation = hotkey_supervisor.begin_registration_attempt();
    match register_configured_shortcuts(&app, &config) {
        Ok(()) => {
            hotkey_supervisor.record_registration_success(generation);
            *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
        Err(error) => {
            hotkey_supervisor.record_registration_failure(generation, error.clone());
            *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(error.clone());
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_status_marks_valid_distinct_bindings() {
        let config = storage::AppConfig {
            hotkey: "Ctrl+/".to_string(),
            ask_hotkey: "Ctrl+.".to_string(),
            ..storage::AppConfig::default()
        };

        let status = hotkey_status_for(&config, None);

        assert!(status.dictation.valid);
        assert!(status.ask.valid);
        assert!(!status.conflict);
        assert_eq!(status.registration_error, None);
    }

    #[test]
    fn hotkey_status_marks_conflicting_bindings() {
        let config = storage::AppConfig {
            hotkey: "Ctrl+/".to_string(),
            ask_hotkey: "Control+Slash".to_string(),
            ..storage::AppConfig::default()
        };

        let status = hotkey_status_for(&config, Some("already registered".to_string()));

        assert!(status.dictation.valid);
        assert!(status.ask.valid);
        assert!(status.conflict);
        assert_eq!(
            status.registration_error,
            Some("already registered".to_string())
        );
    }

    #[test]
    fn hotkey_status_reports_installed_roles_and_capability() {
        let config = storage::AppConfig {
            hotkey: "Ctrl+/".to_string(),
            ask_hotkey: "Ctrl+.".to_string(),
            ..storage::AppConfig::default()
        };
        let caps = platform::PlatformCapabilities {
            os: "linux".to_string(),
            session_type: "x11".to_string(),
            global_hotkey_reliable: true,
            keyboard_output_reliable: true,
            clipboard_auto_paste_reliable: true,
        };

        let status = hotkey_status_for_with_capability(&config, None, caps);

        assert!(status.capability.supports_global_hotkey);
        assert!(status.capability.supports_hold_mode);
        assert!(status.capability.supports_released_edge);
        assert_eq!(status.roles.len(), 6);
        assert_eq!(status.roles[0].role, "dictation");
        assert_eq!(status.roles[0].adapter, "tauriGlobalShortcut");
        assert_eq!(status.roles[0].state, "installed");
        assert_eq!(status.roles[0].last_error, None);
        assert_eq!(status.roles[1].role, "ask");
        assert_eq!(status.roles[1].state, "installed");
        assert_eq!(status.roles[2].role, "translate");
        assert_eq!(status.roles[2].state, "disabled");
        assert_eq!(status.roles[5].role, "openApp");
        assert_eq!(status.roles[5].state, "disabled");
    }

    #[test]
    fn hotkey_status_capability_reports_native_support_on_macos_and_windows() {
        let macos = platform::PlatformCapabilities {
            os: "macos".to_string(),
            session_type: "unknown".to_string(),
            global_hotkey_reliable: true,
            keyboard_output_reliable: true,
            clipboard_auto_paste_reliable: true,
        };
        let windows = platform::PlatformCapabilities {
            os: "windows".to_string(),
            session_type: "unknown".to_string(),
            global_hotkey_reliable: true,
            keyboard_output_reliable: true,
            clipboard_auto_paste_reliable: true,
        };

        let macos_capability = hotkey_capability_for(&macos);
        let windows_capability = hotkey_capability_for(&windows);

        assert!(macos_capability.supports_side_specific_modifiers);
        assert!(macos_capability.requires_accessibility_permission);
        assert!(windows_capability.supports_side_specific_modifiers);
        assert!(!windows_capability.requires_accessibility_permission);
    }

    #[test]
    fn hotkey_status_includes_configured_advanced_roles() {
        let mut config = storage::AppConfig::default();
        config.hotkeys.dictation = storage::ShortcutBinding::from_hotkey("Ctrl+/").unwrap();
        config.hotkeys.translate = storage::ShortcutBinding::from_hotkey("Ctrl+Shift+T");
        config.hotkeys.edit_selection = storage::ShortcutBinding::from_hotkey("Ctrl+Shift+E");
        config.hotkeys.switch_scene = storage::ShortcutBinding::from_hotkey("Ctrl+Shift+S");
        config.hotkeys.open_app = storage::ShortcutBinding::from_hotkey("Ctrl+Shift+O");
        config.hotkey = config.hotkeys.dictation.to_hotkey_string().unwrap();
        config.ask_hotkey = config
            .hotkeys
            .ask
            .as_ref()
            .and_then(storage::ShortcutBinding::to_hotkey_string)
            .unwrap();
        let caps = platform::PlatformCapabilities {
            os: "linux".to_string(),
            session_type: "x11".to_string(),
            global_hotkey_reliable: true,
            keyboard_output_reliable: true,
            clipboard_auto_paste_reliable: true,
        };

        let status = hotkey_status_for_with_capability(&config, None, caps);
        let roles: Vec<&str> = status.roles.iter().map(|role| role.role.as_str()).collect();

        assert_eq!(
            roles,
            vec![
                "dictation",
                "ask",
                "translate",
                "editSelection",
                "switchScene",
                "openApp",
            ]
        );
        assert!(status.roles.iter().all(|role| role.state == "installed"));
    }

    #[test]
    fn hotkey_status_reports_native_hook_for_native_dictation() {
        let config = storage::AppConfig {
            hotkey: "RightAlt".to_string(),
            hotkey_mode: "toggle".to_string(),
            hotkeys: storage::HotkeyConfig::from_legacy("RightAlt", "Ctrl+.", "toggle"),
            ..storage::AppConfig::default()
        };
        let caps = platform::PlatformCapabilities {
            os: "windows".to_string(),
            session_type: "unknown".to_string(),
            global_hotkey_reliable: true,
            keyboard_output_reliable: true,
            clipboard_auto_paste_reliable: true,
        };

        let status = hotkey_status_for_with_capability(&config, None, caps);

        assert!(status.dictation.valid);
        assert_eq!(status.roles[0].role, "dictation");
        assert_eq!(status.roles[0].adapter, "nativeHook");
        assert_eq!(status.roles[0].state, "installed");
    }

    #[test]
    fn hotkey_supervisor_generation_guard_skips_register_closure_when_superseded() {
        let supervisor = HotkeySupervisor::default();
        let stale_generation = supervisor.snapshot().generation;
        supervisor.wake_for_settings_change();
        let mut called = false;

        let result = run_if_hotkey_generation_current(&supervisor, stale_generation, || {
            called = true;
            Ok(())
        });

        assert_eq!(result.unwrap_err(), HOTKEY_REGISTRATION_SUPERSEDED_ERROR);
        assert!(!called);
    }

    #[test]
    fn hotkey_status_reports_failed_roles_for_registration_error() {
        let config = storage::AppConfig {
            hotkey: "Ctrl+/".to_string(),
            ask_hotkey: "Ctrl+.".to_string(),
            ..storage::AppConfig::default()
        };
        let caps = platform::PlatformCapabilities {
            os: "linux".to_string(),
            session_type: "x11".to_string(),
            global_hotkey_reliable: true,
            keyboard_output_reliable: true,
            clipboard_auto_paste_reliable: true,
        };

        let status = hotkey_status_for_with_capability(
            &config,
            Some("already registered".to_string()),
            caps,
        );

        assert_eq!(status.roles[0].state, "failed");
        assert_eq!(
            status.roles[0]
                .last_error
                .as_ref()
                .map(|error| error.code.as_str()),
            Some("registrationFailed")
        );
        assert_eq!(status.roles[1].state, "failed");
    }

    #[test]
    fn hotkey_status_reports_starting_roles_while_supervisor_retries() {
        let config = storage::AppConfig {
            hotkey: "Ctrl+/".to_string(),
            ask_hotkey: "Ctrl+.".to_string(),
            ..storage::AppConfig::default()
        };
        let supervisor = HotkeySupervisor::default();
        let generation = supervisor.snapshot().generation;
        supervisor.record_registration_failure(generation, "already registered".to_string());
        supervisor.begin_registration_attempt();
        let caps = platform::PlatformCapabilities {
            os: "macos".to_string(),
            session_type: "unknown".to_string(),
            keyboard_output_reliable: true,
            clipboard_auto_paste_reliable: true,
            global_hotkey_reliable: true,
        };

        let status = hotkey_status_for_with_capability_and_supervisor(
            &config,
            Some("already registered".to_string()),
            caps,
            Some(supervisor.snapshot()),
        );

        assert_eq!(status.roles[0].state, "starting");
        assert_eq!(
            status.roles[0]
                .last_error
                .as_ref()
                .map(|error| error.code.as_str()),
            Some("registrationFailed")
        );
        assert_eq!(status.roles[1].state, "starting");
    }

    #[test]
    fn hotkey_diagnostics_report_checking_while_supervisor_is_starting() {
        let config = storage::AppConfig {
            hotkey: "Ctrl+/".to_string(),
            ask_hotkey: "Ctrl+.".to_string(),
            ..storage::AppConfig::default()
        };
        let supervisor = HotkeySupervisor::default();
        let caps = platform::PlatformCapabilities {
            os: "macos".to_string(),
            session_type: "unknown".to_string(),
            keyboard_output_reliable: true,
            clipboard_auto_paste_reliable: true,
            global_hotkey_reliable: true,
        };
        let status = hotkey_status_for_with_capability_and_supervisor(
            &config,
            None,
            caps.clone(),
            Some(supervisor.snapshot()),
        );

        let row = hotkey_diagnostic_row(status, &caps, "2026-07-06T00:00:00");

        assert_eq!(row.status, DiagnosticStatus::Checking);
        assert_eq!(row.action, None);
    }

    #[test]
    fn diagnostics_rows_cover_core_runtime_health_for_configured_native_hook() {
        let config = storage::AppConfig {
            hotkey: "RightAlt".to_string(),
            hotkey_mode: "toggle".to_string(),
            hotkeys: storage::HotkeyConfig::from_legacy("RightAlt", "Ctrl+.", "toggle"),
            ..storage::AppConfig::default()
        };
        let caps = platform::PlatformCapabilities {
            os: "windows".to_string(),
            session_type: "unknown".to_string(),
            global_hotkey_reliable: true,
            keyboard_output_reliable: true,
            clipboard_auto_paste_reliable: true,
        };
        let status = hotkey_status_for_with_capability(&config, None, caps.clone());

        let row = hotkey_diagnostic_row(status, &caps, "2026-07-06T00:00:00");

        assert_eq!(row.status, DiagnosticStatus::Ok);
        assert_eq!(row.action, None);
    }

    #[test]
    fn diagnostics_rows_cover_core_runtime_health_does_not_treat_native_hook_disabled_as_pending() {
        let caps = platform::PlatformCapabilities {
            os: "windows".to_string(),
            session_type: "unknown".to_string(),
            global_hotkey_reliable: true,
            keyboard_output_reliable: true,
            clipboard_auto_paste_reliable: true,
        };
        let status = HotkeyStatus {
            dictation: HotkeyBindingStatus {
                value: "RightAlt".to_string(),
                valid: true,
            },
            ask: HotkeyBindingStatus {
                value: "Ctrl+.".to_string(),
                valid: true,
            },
            conflict: false,
            registration_error: None,
            roles: vec![HotkeyRoleStatus {
                role: "dictation".to_string(),
                adapter: "nativeHook".to_string(),
                state: "disabled".to_string(),
                message: None,
                last_error: None,
            }],
            capability: hotkey_capability_for(&caps),
        };

        let row = hotkey_diagnostic_row(status, &caps, "2026-07-06T00:00:00");

        assert_eq!(row.status, DiagnosticStatus::Ok);
        assert_ne!(
            row.message,
            "Native hotkey runtime is not installed yet".to_string()
        );
    }

    #[test]
    fn hotkey_status_reports_disabled_optional_ask_role() {
        let mut config = storage::AppConfig::default();
        config.hotkeys.dictation = storage::ShortcutBinding::from_hotkey("Ctrl+/").unwrap();
        config.hotkey = "Ctrl+/".to_string();
        config.hotkeys.ask = None;
        config.ask_hotkey = String::new();
        let caps = platform::PlatformCapabilities {
            os: "linux".to_string(),
            session_type: "wayland".to_string(),
            global_hotkey_reliable: false,
            keyboard_output_reliable: false,
            clipboard_auto_paste_reliable: false,
        };

        let status = hotkey_status_for_with_capability(&config, None, caps);

        assert!(!status.capability.supports_global_hotkey);
        assert!(!status.capability.supports_released_edge);
        assert_eq!(
            status.capability.status_hint.as_deref(),
            Some("linuxWaylandLimited")
        );
        assert!(status.ask.valid);
        assert_eq!(status.roles[0].role, "dictation");
        assert_eq!(status.roles[0].state, "installed");
        assert_eq!(
            status.roles[0].message.as_deref(),
            Some("linuxWaylandLimited")
        );
        assert_eq!(status.roles[1].role, "ask");
        assert_eq!(status.roles[1].adapter, "unavailable");
        assert_eq!(status.roles[1].state, "disabled");
    }

    #[test]
    fn diagnostics_rows_cover_core_runtime_health() {
        let config = storage::AppConfig {
            insertion_strategy: "clipboardPaste".to_string(),
            output_mode: "clipboard".to_string(),
            ..storage::AppConfig::default()
        };
        let caps = platform::PlatformCapabilities {
            os: "linux".to_string(),
            session_type: "wayland".to_string(),
            global_hotkey_reliable: false,
            keyboard_output_reliable: false,
            clipboard_auto_paste_reliable: false,
        };
        let hotkey_status = hotkey_status_for(&config, Some("already registered".to_string()));

        let report = build_system_diagnostics_report(
            &config,
            caps,
            hotkey_status,
            false,
            ProbeResult::ok("Built-in microphone"),
            ProbeResult::ok("Clipboard write restored"),
            "2026-07-06T00:00:00",
        );

        let ids: Vec<&str> = report.rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "microphone",
                "accessibility",
                "hotkey",
                "clipboard",
                "insertion",
                "platform"
            ]
        );
        assert_eq!(report.rows[0].status, DiagnosticStatus::Ok);
        assert_eq!(report.rows[1].status, DiagnosticStatus::NotApplicable);
        assert_eq!(report.rows[2].status, DiagnosticStatus::Error);
        assert_eq!(report.rows[4].status, DiagnosticStatus::Warning);
    }

    #[test]
    fn diagnostics_marks_macos_accessibility_as_error_when_missing() {
        let config = storage::AppConfig::default();
        let caps = platform::PlatformCapabilities {
            os: "macos".to_string(),
            session_type: "unknown".to_string(),
            global_hotkey_reliable: true,
            keyboard_output_reliable: true,
            clipboard_auto_paste_reliable: true,
        };
        let hotkey_status = hotkey_status_for(&config, None);

        let report = build_system_diagnostics_report(
            &config,
            caps,
            hotkey_status,
            false,
            ProbeResult::ok("Built-in microphone"),
            ProbeResult::ok("Clipboard write restored"),
            "2026-07-06T00:00:00",
        );

        let accessibility = report
            .rows
            .iter()
            .find(|row| row.id == "accessibility")
            .unwrap();
        assert_eq!(accessibility.status, DiagnosticStatus::Error);
        assert_eq!(
            accessibility.action.as_deref(),
            Some("openAccessibilitySettings")
        );
    }

    #[test]
    fn diagnostics_marks_macos_fn_hotkey_accessibility_as_error_even_for_clipboard_output() {
        let mut config = storage::AppConfig {
            output_mode: "clipboard".to_string(),
            insertion_strategy: "clipboardPaste".to_string(),
            hotkey: "Fn".to_string(),
            hotkey_mode: "toggle".to_string(),
            ask_hotkey: "Command+.".to_string(),
            ..storage::AppConfig::default()
        };
        config.hotkeys = storage::HotkeyConfig::from_legacy("Fn", "Command+.", "toggle");
        let caps = platform::PlatformCapabilities {
            os: "macos".to_string(),
            session_type: "unknown".to_string(),
            global_hotkey_reliable: true,
            keyboard_output_reliable: true,
            clipboard_auto_paste_reliable: true,
        };
        let hotkey_status = hotkey_status_for_with_capability(&config, None, caps.clone());

        let report = build_system_diagnostics_report(
            &config,
            caps,
            hotkey_status,
            false,
            ProbeResult::ok("Built-in microphone"),
            ProbeResult::ok("Clipboard write restored"),
            "2026-07-06T00:00:00",
        );

        let accessibility = report
            .rows
            .iter()
            .find(|row| row.id == "accessibility")
            .unwrap();
        assert_eq!(accessibility.status, DiagnosticStatus::Error);
        assert_eq!(
            accessibility.action.as_deref(),
            Some("openAccessibilitySettings")
        );
    }

    #[test]
    fn clipboard_diagnostics_disclose_text_only_restore_scope() {
        let row = clipboard_diagnostic_row(
            ProbeResult::ok("Clipboard write and restore succeeded"),
            "2026-07-06T00:00:00",
        );

        assert_eq!(row.status, DiagnosticStatus::Ok);
        assert!(row.message.contains("text-only"));
    }
}
