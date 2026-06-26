use crate::pipeline;
use crate::platform;
use crate::storage;
use crate::tray;
use crate::HotkeyRegistrationError;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

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
    let new_shortcut =
        crate::parse_hotkey(&hotkey).ok_or_else(|| format!("Invalid hotkey: {}", hotkey))?;

    // Unregister all existing shortcuts, then register the new one
    // (the global handler from with_handler is still active)
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string())?;
    if let Err(e) = app.global_shortcut().register(new_shortcut) {
        let message = e.to_string();
        *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(message.clone());
        return Err(message);
    }
    *hotkey_error.0.lock().unwrap_or_else(|e| e.into_inner()) = None;

    // Save updated hotkey to config
    let mut config = config_state.load().await.map_err(|e| e.to_string())?;
    config.hotkey = hotkey;
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
    let shortcut = crate::parse_hotkey(&config.hotkey).unwrap_or_else(crate::default_shortcut);
    // Ensure clean state, then register
    let _ = app.global_shortcut().unregister_all();
    app.global_shortcut()
        .register(shortcut)
        .map_err(|e| e.to_string())
}
