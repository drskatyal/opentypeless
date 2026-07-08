use async_trait::async_trait;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::error::AppError;

use super::{InsertResult, InsertionStrategy, OutputMode, TextOutput};

/// Maximum characters per enigo.text() call to avoid input buffer overflow.
const TYPE_CHUNK_SIZE: usize = 200;
/// Delay between typing chunks.
const TYPE_CHUNK_DELAY_MS: u64 = 5;
/// Base timeout for macOS main-thread keyboard output.
#[cfg(target_os = "macos")]
const MACOS_TYPE_BASE_TIMEOUT_SECS: u64 = 30;
/// Maximum timeout for macOS main-thread keyboard output.
#[cfg(target_os = "macos")]
const MACOS_TYPE_MAX_TIMEOUT_SECS: u64 = 300;

/// Check if keyboard simulation is reliable on this platform.
/// Returns Ok(()) if fine, or Err with a reason string for the caller.
pub fn check_keyboard_available() -> std::result::Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let session = crate::platform::current_session_type();
        if session == "wayland" {
            return Err("wayland_unsupported".to_string());
        }
        if (session == "x11" || session.is_empty())
            && std::process::Command::new("which")
                .arg("xdotool")
                .output()
                .map(|o| !o.status.success())
                .unwrap_or(true)
        {
            return Err("xdotool_missing".to_string());
        }
    }
    let _ = (); // suppress unused warning on non-Linux
    Ok(())
}

pub struct KeyboardOutput {
    #[cfg(target_os = "macos")]
    app_handle: tauri::AppHandle,
}

impl KeyboardOutput {
    pub fn new(app_handle: &tauri::AppHandle) -> Self {
        #[cfg(target_os = "macos")]
        {
            Self {
                app_handle: app_handle.clone(),
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = app_handle;
            Self {}
        }
    }

    #[cfg(target_os = "macos")]
    async fn type_text_on_main_thread(&self, text: &str) -> Result<(), AppError> {
        let text = text.to_string();
        let timeout = macos_type_timeout(&text);
        let app_handle = self.app_handle.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();

        app_handle
            .run_on_main_thread(move || {
                let result = type_text_sync(&text);
                let _ = tx.send(result);
            })
            .map_err(|e| {
                AppError::Output(format!(
                    "Failed to schedule keyboard output on main thread: {}",
                    e
                ))
            })?;

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(AppError::Output(
                "Main thread keyboard output task was dropped".to_string(),
            )),
            Err(_) => Err(AppError::Output(format!(
                "Main thread keyboard output timed out after {:.0}s",
                timeout.as_secs_f64()
            ))),
        }
    }
}

#[async_trait]
impl TextOutput for KeyboardOutput {
    async fn type_text(&self, text: &str) -> Result<InsertResult, AppError> {
        let chars_inserted = text.chars().count();

        #[cfg(target_os = "macos")]
        {
            self.type_text_on_main_thread(text).await?;
            return Ok(InsertResult::inserted(
                InsertionStrategy::Keyboard,
                chars_inserted,
            ));
        }

        #[cfg(not(target_os = "macos"))]
        {
            let text = text.to_string();
            tokio::task::spawn_blocking(move || type_text_sync(&text))
                .await
                .map_err(|e| AppError::Output(format!("Spawn blocking error: {}", e)))??;
            Ok(InsertResult::inserted(
                InsertionStrategy::Keyboard,
                chars_inserted,
            ))
        }
    }

    fn mode(&self) -> OutputMode {
        OutputMode::Keyboard
    }
}

fn type_text_sync(text: &str) -> Result<(), AppError> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| AppError::Output(format!("Failed to create Enigo: {:?}", e)))?;

    let lines: Vec<&str> = text.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        if !line.is_empty() {
            for chunk in line.chars().collect::<Vec<_>>().chunks(TYPE_CHUNK_SIZE) {
                let s: String = chunk.iter().collect();
                enigo
                    .text(&s)
                    .map_err(|e| AppError::Output(format!("Failed to type text: {:?}", e)))?;
                std::thread::sleep(std::time::Duration::from_millis(TYPE_CHUNK_DELAY_MS));
            }
        }
        if i < lines.len() - 1 {
            enigo
                .key(Key::Shift, Direction::Press)
                .map_err(|e| AppError::Output(format!("Key error: {:?}", e)))?;
            enigo
                .key(Key::Return, Direction::Click)
                .map_err(|e| AppError::Output(format!("Key error: {:?}", e)))?;
            enigo
                .key(Key::Shift, Direction::Release)
                .map_err(|e| AppError::Output(format!("Key error: {:?}", e)))?;
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_type_timeout(text: &str) -> std::time::Duration {
    let char_count = text.chars().count();
    let chunk_count = if char_count == 0 {
        0
    } else {
        ((char_count - 1) / TYPE_CHUNK_SIZE) + 1
    };
    let seconds =
        (MACOS_TYPE_BASE_TIMEOUT_SECS + chunk_count as u64).min(MACOS_TYPE_MAX_TIMEOUT_SECS);

    std::time::Duration::from_secs(seconds)
}
