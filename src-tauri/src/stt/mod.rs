pub mod apple_speech;
pub mod assemblyai;
pub mod cloud;
pub mod config;
pub mod deepgram;
pub mod gemini;
pub mod volcengine;
pub mod whisper_compat;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

use whisper_compat::{WhisperCompatConfig, WhisperCompatProvider};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttConfig {
    pub api_key: String,
    pub language: Option<String>,
    pub smart_format: bool,
    pub sample_rate: u32,
    pub resource_id: Option<String>,
    pub operation_id: Option<String>,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            language: None,
            smart_format: true,
            sample_rate: 16000,
            resource_id: None,
            operation_id: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TranscriptEvent {
    Partial { text: String },
    Final { text: String, confidence: f32 },
    SpeechStarted,
    SpeechEnded,
    Error { message: String },
}

#[async_trait]
pub trait SttProvider: Send + Sync {
    async fn connect(&mut self, config: &SttConfig) -> Result<(), AppError>;
    async fn send_audio(&mut self, chunk: &[u8]) -> Result<(), AppError>;
    async fn recv_transcript(&mut self) -> Result<Option<TranscriptEvent>, AppError>;
    /// Disconnect and optionally return a final transcript (for file-based providers).
    async fn disconnect(&mut self) -> Result<Option<String>, AppError>;
    fn name(&self) -> &str;
}

pub fn create_provider(
    provider_name: &str,
    custom_whisper_config: Option<WhisperCompatConfig>,
    client: Option<reqwest::Client>,
    gemini_model: Option<String>,
) -> Result<Box<dyn SttProvider>, AppError> {
    match provider_name {
        "cloud" => {
            let api_base_url = crate::api_base_url();
            Ok(match client {
                Some(ref c) => Box::new(cloud::CloudSttProvider::with_client(
                    api_base_url,
                    c.clone(),
                )),
                None => Box::new(cloud::CloudSttProvider::new(api_base_url)),
            })
        }
        "assemblyai" => Ok(Box::new(assemblyai::AssemblyAiProvider::new())),
        "gemini" => {
            // An empty selection falls back to the provider's default model.
            let model = gemini_model.filter(|m| !m.trim().is_empty());
            Ok(match client {
                Some(ref c) => Box::new(gemini::GeminiSttProvider::with_client(model, c.clone())),
                None => Box::new(gemini::GeminiSttProvider::new(model)),
            })
        }
        "deepgram" => Ok(Box::new(deepgram::DeepgramProvider::new())),
        apple_speech::APPLE_SPEECH_PROVIDER => {
            Ok(Box::new(apple_speech::AppleSpeechProvider::new()))
        }
        volcengine::VOLCENGINE_DOUBAO_PROVIDER => {
            Ok(Box::new(volcengine::VolcengineDoubaoProvider::new()))
        }
        config::CUSTOM_WHISPER_PROVIDER => {
            let wc = custom_whisper_config.ok_or_else(|| {
                AppError::Config("Local / Custom Whisper is missing base URL or model".to_string())
            })?;
            Ok(match client {
                Some(ref c) => Box::new(WhisperCompatProvider::with_client(wc, c.clone())),
                None => Box::new(WhisperCompatProvider::new(wc)),
            })
        }
        name => {
            // All Whisper-compatible providers share the same HTTP upload logic.
            // Config is centralised in config::build_known_whisper_config.
            let wc = config::build_known_whisper_config(name)
                .ok_or_else(|| AppError::Config(format!("Unknown STT provider: {}", name)))?;
            Ok(match client {
                Some(ref c) => Box::new(WhisperCompatProvider::with_client(wc, c.clone())),
                None => Box::new(WhisperCompatProvider::new(wc)),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_whisper_requires_explicit_config() {
        let result = create_provider(config::CUSTOM_WHISPER_PROVIDER, None, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn custom_whisper_uses_explicit_config() {
        let cfg = config::build_custom_whisper_config(
            "http://localhost:8000/v1",
            "Systran/faster-whisper-large-v3",
        )
        .unwrap();

        let provider =
            create_provider(config::CUSTOM_WHISPER_PROVIDER, Some(cfg), None, None).unwrap();
        assert_eq!(provider.name(), config::CUSTOM_WHISPER_PROVIDER);
    }

    #[test]
    fn creates_volcengine_doubao_realtime_provider() {
        let provider = create_provider("volcengine-doubao", None, None, None).unwrap();
        assert_eq!(provider.name(), "Volcengine Doubao Realtime ASR");
    }

    #[test]
    fn creates_apple_speech_builtin_local_provider() {
        let provider = create_provider("apple-speech", None, None, None).unwrap();
        assert_eq!(provider.name(), "Apple Speech");
    }

    #[test]
    fn creates_gemini_provider() {
        let provider = create_provider("gemini", None, None, None).unwrap();
        assert_eq!(provider.name(), "gemini");
    }

    #[test]
    fn creates_gemini_provider_with_selected_model() {
        // A blank model must not override the default; a real one must be honored.
        let default_model = create_provider("gemini", None, None, Some("  ".to_string())).unwrap();
        assert_eq!(default_model.name(), "gemini");

        let selected = create_provider(
            "gemini",
            None,
            None,
            Some("gemini-3.1-flash-lite".to_string()),
        )
        .unwrap();
        assert_eq!(selected.name(), "gemini");
    }

    #[test]
    fn unknown_stt_provider_returns_error() {
        let result = create_provider("not-a-provider", None, None, None);
        assert!(result.is_err());
    }
}
