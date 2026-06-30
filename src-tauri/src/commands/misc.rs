use crate::pipeline;
use crate::platform;
use crate::storage;
use crate::tray;
use crate::AskHotkeyCache;
use crate::HotkeyRegistrationError;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

fn register_configured_shortcuts(
    app: &tauri::AppHandle,
    config: &storage::AppConfig,
) -> Result<(), String> {
    let dictation_shortcut = crate::parse_hotkey(&config.hotkey)
        .ok_or_else(|| format!("Invalid hotkey: {}", config.hotkey))?;
    let ask_shortcut = crate::parse_hotkey(&config.ask_hotkey)
        .ok_or_else(|| format!("Invalid Ask hotkey: {}", config.ask_hotkey))?;

    app.global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string())?;
    app.global_shortcut()
        .register(dictation_shortcut)
        .map_err(|e| e.to_string())?;

    if dictation_shortcut.key != ask_shortcut.key || dictation_shortcut.mods != ask_shortcut.mods {
        app.global_shortcut()
            .register(ask_shortcut)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
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
pub async fn update_hotkey(
    app: tauri::AppHandle,
    config_state: tauri::State<'_, storage::ConfigManager>,
    hotkey_error: tauri::State<'_, HotkeyRegistrationError>,
    hotkey: String,
) -> Result<(), String> {
    let mut config = config_state.load().await.map_err(|e| e.to_string())?;
    config.hotkey = hotkey;
    config.normalize_values();

    if let Err(e) = register_configured_shortcuts(&app, &config) {
        let message = e.to_string();
        *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(message.clone());
        return Err(message);
    }
    *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = None;

    config_state
        .save(&config)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn update_ask_hotkey(
    app: tauri::AppHandle,
    config_state: tauri::State<'_, storage::ConfigManager>,
    hotkey_error: tauri::State<'_, HotkeyRegistrationError>,
    ask_cache: tauri::State<'_, AskHotkeyCache>,
    hotkey: String,
) -> Result<(), String> {
    let mut config = config_state.load().await.map_err(|e| e.to_string())?;
    config.ask_hotkey = hotkey;
    config.normalize_values();

    if let Err(e) = register_configured_shortcuts(&app, &config) {
        let message = e.to_string();
        *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(message.clone());
        return Err(message);
    }
    *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *ask_cache.0.lock().unwrap_or_else(|e| e.into_inner()) = config.ask_hotkey.clone();

    config_state
        .save(&config)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Temporarily unregister all global shortcuts so the webview can capture key events.
#[tauri::command]
pub fn pause_hotkey(app: tauri::AppHandle) -> Result<(), String> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string())
}

/// Re-register the current hotkey from config after recording is done.
#[tauri::command]
pub async fn resume_hotkey(
    app: tauri::AppHandle,
    config_state: tauri::State<'_, storage::ConfigManager>,
) -> Result<(), String> {
    let config = config_state.load().await.map_err(|e| e.to_string())?;
    register_configured_shortcuts(&app, &config)
}
