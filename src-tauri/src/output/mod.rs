pub mod clipboard;
pub mod keyboard;

use crate::error::{AppError, UserError};
use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputMode {
    Keyboard,
    Clipboard,
}

#[async_trait]
pub trait TextOutput: Send + Sync {
    async fn type_text(&self, text: &str) -> Result<(), AppError>;
    fn mode(&self) -> OutputMode;
}

pub fn create_output(mode: OutputMode, app_handle: &tauri::AppHandle) -> Box<dyn TextOutput> {
    match mode {
        OutputMode::Keyboard => Box::new(keyboard::KeyboardOutput::new(app_handle)),
        OutputMode::Clipboard => Box::new(clipboard::ClipboardOutput::new()),
    }
}

fn clipboard_warning_for_platform() -> Option<UserError> {
    if crate::platform::is_wayland_session() {
        Some(UserError {
            code: "output_wayland_clipboard_copy_only".to_string(),
            details: None,
            retry_count: 0,
        })
    } else {
        None
    }
}

/// Try keyboard output first. On failure, fall back to clipboard.
/// Returns Ok(Some(UserError)) if fell back to clipboard (warning for frontend).
/// Returns Ok(None) if primary output succeeded.
/// Returns Err if both keyboard and clipboard failed.
pub async fn output_with_fallback(
    app_handle: &tauri::AppHandle,
    text: &str,
    mode: OutputMode,
) -> Result<Option<UserError>, String> {
    if mode == OutputMode::Clipboard {
        let output = create_output(OutputMode::Clipboard, app_handle);
        return output
            .type_text(text)
            .await
            .map_err(|e| e.to_string())
            .map(|_| clipboard_warning_for_platform());
    }

    // Try keyboard first
    let keyboard = create_output(OutputMode::Keyboard, app_handle);
    match keyboard.type_text(text).await {
        Ok(()) => Ok(None),
        Err(kb_err) => {
            tracing::warn!(
                "Keyboard output failed: {}, falling back to clipboard",
                kb_err
            );
            let clipboard = create_output(OutputMode::Clipboard, app_handle);
            match clipboard.type_text(text).await {
                Ok(()) => Ok(clipboard_warning_for_platform().or(Some(UserError {
                    code: "output_fallback_clipboard".to_string(),
                    details: Some(kb_err.to_string()),
                    retry_count: 0,
                }))),
                Err(cb_err) => Err(format!(
                    "Both keyboard ({}) and clipboard ({}) output failed",
                    kb_err, cb_err
                )),
            }
        }
    }
}
