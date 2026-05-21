pub mod clipboard;
pub mod keyboard;

use anyhow::Result;
use async_trait::async_trait;
use crate::error::UserError;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputMode {
    Keyboard,
    Clipboard,
}

#[async_trait]
pub trait TextOutput: Send + Sync {
    async fn type_text(&self, text: &str) -> Result<()>;
    fn mode(&self) -> OutputMode;
}

pub fn create_output(mode: OutputMode) -> Box<dyn TextOutput> {
    match mode {
        OutputMode::Keyboard => Box::new(keyboard::KeyboardOutput::new()),
        OutputMode::Clipboard => Box::new(clipboard::ClipboardOutput::new()),
    }
}

/// Try keyboard output first. On failure, fall back to clipboard.
/// Returns Ok(Some(UserError)) if fell back to clipboard (warning for frontend).
/// Returns Ok(None) if primary output succeeded.
/// Returns Err if both keyboard and clipboard failed.
pub async fn output_with_fallback(
    text: &str,
    mode: OutputMode,
) -> Result<Option<UserError>, String> {
    if mode == OutputMode::Clipboard {
        let output = create_output(OutputMode::Clipboard);
        return output.type_text(text).await.map_err(|e| e.to_string()).map(|_| None);
    }

    // Try keyboard first
    let keyboard = create_output(OutputMode::Keyboard);
    match keyboard.type_text(text).await {
        Ok(()) => Ok(None),
        Err(kb_err) => {
            tracing::warn!("Keyboard output failed: {}, falling back to clipboard", kb_err);
            let clipboard = create_output(OutputMode::Clipboard);
            match clipboard.type_text(text).await {
                Ok(()) => {
                    Ok(Some(UserError {
                        code: "output_fallback_clipboard".to_string(),
                        details: Some(kb_err.to_string()),
                        retry_count: 0,
                    }))
                }
                Err(cb_err) => {
                    Err(format!("Both keyboard ({}) and clipboard ({}) output failed", kb_err, cb_err))
                }
            }
        }
    }
}
