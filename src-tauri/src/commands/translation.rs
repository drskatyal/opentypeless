use tauri::Emitter;

use crate::{pipeline::PipelineHandle, storage};

#[tauri::command]
pub async fn set_active_translation_target(
    app: tauri::AppHandle,
    code: String,
    pipeline: tauri::State<'_, PipelineHandle>,
    config_manager: tauri::State<'_, storage::ConfigManager>,
) -> Result<storage::TranslationConfig, String> {
    let code = code.trim().to_ascii_lowercase();
    let mut config = config_manager
        .load()
        .await
        .map_err(|error| error.to_string())?;
    if !config.translation.targets.contains(&code) {
        return Err("translation_target_not_configured".to_string());
    }

    let previous_operation_target = pipeline.switch_active_translation_target(code.clone())?;
    let previous_config = config.clone();
    config.translation.active_target = code.clone();
    config.target_lang = code.clone();
    if let Err(error) = config_manager.save(&config).await {
        let _ = pipeline.switch_active_translation_target(previous_operation_target);
        let _ = config_manager.save(&previous_config).await;
        return Err(error.to_string());
    }

    let _ = app.emit("translation:target-changed", &code);
    let _ = app.emit(
        "config:patch",
        serde_json::json!({
            "target_lang": code,
            "translation": config.translation.clone(),
        }),
    );
    Ok(config.translation)
}
