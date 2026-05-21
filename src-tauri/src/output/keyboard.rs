use anyhow::Result;
use async_trait::async_trait;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use super::{OutputMode, TextOutput};

/// Maximum characters per enigo.text() call to avoid input buffer overflow.
const TYPE_CHUNK_SIZE: usize = 200;
/// Delay between typing chunks.
const TYPE_CHUNK_DELAY_MS: u64 = 5;

/// Check if keyboard simulation is reliable on this platform.
/// Returns Ok(()) if fine, or Err with a reason string for the caller.
pub fn check_keyboard_available() -> std::result::Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
        if session == "wayland" {
            return Err("wayland_unsupported".to_string());
        }
        if session == "x11" || session.is_empty() {
            if std::process::Command::new("which")
                .arg("xdotool")
                .output()
                .map(|o| !o.status.success())
                .unwrap_or(true)
            {
                return Err("xdotool_missing".to_string());
            }
        }
    }
    let _ = (); // suppress unused warning on non-Linux
    Ok(())
}

pub struct KeyboardOutput;

impl Default for KeyboardOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardOutput {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TextOutput for KeyboardOutput {
    async fn type_text(&self, text: &str) -> Result<()> {
        let text = text.to_string();
        tokio::task::spawn_blocking(move || {
            let mut enigo = Enigo::new(&Settings::default())
                .map_err(|e| anyhow::anyhow!("Failed to create Enigo: {:?}", e))?;

            let lines: Vec<&str> = text.split('\n').collect();
            for (i, line) in lines.iter().enumerate() {
                if !line.is_empty() {
                    for chunk in line.chars().collect::<Vec<_>>().chunks(TYPE_CHUNK_SIZE) {
                        let s: String = chunk.iter().collect();
                        enigo
                            .text(&s)
                            .map_err(|e| anyhow::anyhow!("Failed to type text: {:?}", e))?;
                        std::thread::sleep(std::time::Duration::from_millis(TYPE_CHUNK_DELAY_MS));
                    }
                }
                if i < lines.len() - 1 {
                    enigo
                        .key(Key::Shift, Direction::Press)
                        .map_err(|e| anyhow::anyhow!("Key error: {:?}", e))?;
                    enigo
                        .key(Key::Return, Direction::Click)
                        .map_err(|e| anyhow::anyhow!("Key error: {:?}", e))?;
                    enigo
                        .key(Key::Shift, Direction::Release)
                        .map_err(|e| anyhow::anyhow!("Key error: {:?}", e))?;
                }
            }

            Ok(())
        })
        .await?
    }

    fn mode(&self) -> OutputMode {
        OutputMode::Keyboard
    }
}
