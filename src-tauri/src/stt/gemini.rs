use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};

use crate::error::AppError;

use super::{SttConfig, SttProvider, TranscriptEvent};

/// Max audio buffer: ~24 MB PCM ≈ 12.5 min at 16kHz 16-bit mono.
/// Gemini inline audio has to be sent in a single request, so keep it bounded.
const MAX_AUDIO_BYTES: usize = 24 * 1024 * 1024;

/// Default Gemini model used for native transcription. Any Flash model works.
const DEFAULT_MODEL: &str = "gemini-2.5-flash";

/// Native Gemini STT provider.
///
/// Gemini has no OpenAI Whisper-style `/audio/transcriptions` endpoint, so it
/// cannot be driven through the Whisper-compatible provider. Instead it
/// transcribes via `generateContent` with the audio sent as inline base64 data.
/// Like the other file-based providers it buffers PCM during recording and does
/// the actual transcription in `disconnect`.
pub struct GeminiSttProvider {
    model: String,
    stt_config: Option<SttConfig>,
    audio_buffer: Vec<u8>,
    client: reqwest::Client,
}

impl GeminiSttProvider {
    pub fn new(model: Option<String>) -> Self {
        Self::with_client(model, reqwest::Client::new())
    }

    pub fn with_client(model: Option<String>, client: reqwest::Client) -> Self {
        Self {
            model: model.unwrap_or_else(|| DEFAULT_MODEL.into()),
            stt_config: None,
            audio_buffer: Vec::new(),
            client,
        }
    }

    /// Build a WAV file from raw PCM 16-bit mono audio.
    fn build_wav(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
        let data_len = pcm.len() as u32;
        // 16-bit mono => 2 bytes per sample frame.
        let byte_rate = sample_rate * 2;
        let mut wav = Vec::with_capacity(44 + pcm.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // channels
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&byte_rate.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes()); // block align
        wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend_from_slice(pcm);
        wav
    }
}

#[async_trait]
impl SttProvider for GeminiSttProvider {
    async fn connect(&mut self, config: &SttConfig) -> Result<(), AppError> {
        if config.api_key.trim().is_empty() {
            return Err(AppError::Auth("Gemini API key is empty".into()));
        }
        self.stt_config = Some(config.clone());
        self.audio_buffer.clear();
        tracing::info!(
            "Gemini STT provider ready (buffering mode), model={}",
            self.model
        );
        Ok(())
    }

    async fn send_audio(&mut self, chunk: &[u8]) -> Result<(), AppError> {
        if self.audio_buffer.len() + chunk.len() > MAX_AUDIO_BYTES {
            return Err(AppError::Config(
                "Gemini: audio exceeds maximum length (~12 min)".into(),
            ));
        }
        self.audio_buffer.extend_from_slice(chunk);
        Ok(())
    }

    async fn recv_transcript(&mut self) -> Result<Option<TranscriptEvent>, AppError> {
        // File-based provider: transcription happens in disconnect(); keep this
        // future pending so the pipeline select loop does not busy-spin.
        std::future::pending().await
    }

    async fn disconnect(&mut self) -> Result<Option<String>, AppError> {
        let config = match &self.stt_config {
            Some(c) => c.clone(),
            None => return Ok(None),
        };

        if self.audio_buffer.is_empty() {
            tracing::info!("Gemini: no audio buffered, skipping");
            return Ok(None);
        }

        let audio_len_secs = self.audio_buffer.len() as f64 / (config.sample_rate as f64 * 2.0);
        let wav = Self::build_wav(&self.audio_buffer, config.sample_rate);
        self.audio_buffer.clear();
        tracing::info!(
            "Gemini: sending {:.1}s of audio for transcription",
            audio_len_secs
        );

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        );

        let body = serde_json::json!({
            "systemInstruction": {
                "parts": [{ "text": "Transcribe the audio verbatim. Return ONLY the transcript text." }]
            },
            "contents": [{
                "role": "user",
                "parts": [
                    { "text": "Transcribe this audio." },
                    { "inlineData": { "mimeType": "audio/wav", "data": STANDARD.encode(&wav) } }
                ]
            }],
            "generationConfig": { "temperature": 0.0 }
        });

        let resp = self
            .client
            .post(&url)
            .header("x-goog-api-key", config.api_key.trim())
            .json(&body)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await?;

        let status = resp.status();
        let raw = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            // Truncate at a valid UTF-8 char boundary to avoid panics on multi-byte chars.
            let truncate_at = raw
                .char_indices()
                .take_while(|&(i, _)| i < 200)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(raw.len());
            let sanitized = raw[..truncate_at].to_string();
            tracing::error!("Gemini STT HTTP {}: {}", status, sanitized);
            return Err(AppError::Api {
                status: status.as_u16(),
                body: sanitized,
            });
        }

        let v: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| AppError::Config(e.to_string()))?;
        let text = v["candidates"][0]["content"]["parts"]
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|part| part["text"].as_str())
                    .collect::<String>()
            })
            .unwrap_or_default()
            .trim()
            .to_string();

        tracing::info!("Gemini transcription: {} chars", text.len());
        Ok(if text.is_empty() { None } else { Some(text) })
    }

    fn name(&self) -> &str {
        "gemini"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_key(api_key: &str) -> SttConfig {
        SttConfig {
            api_key: api_key.to_string(),
            language: None,
            smart_format: true,
            sample_rate: 16000,
            resource_id: None,
            operation_id: None,
        }
    }

    #[tokio::test]
    async fn connect_rejects_empty_api_key() {
        let mut provider = GeminiSttProvider::new(None);
        let result = provider.connect(&config_with_key("   ")).await;
        assert!(matches!(result, Err(AppError::Auth(_))));
    }

    #[tokio::test]
    async fn connect_accepts_non_empty_api_key() {
        let mut provider = GeminiSttProvider::new(None);
        let result = provider.connect(&config_with_key("test-key")).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn disconnect_without_audio_returns_none() {
        let mut provider = GeminiSttProvider::new(None);
        provider
            .connect(&config_with_key("test-key"))
            .await
            .unwrap();
        let result = provider.disconnect().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn send_audio_rejects_oversized_buffer() {
        let mut provider = GeminiSttProvider::new(None);
        provider
            .connect(&config_with_key("test-key"))
            .await
            .unwrap();
        let chunk = vec![0u8; MAX_AUDIO_BYTES + 1];
        let result = provider.send_audio(&chunk).await;
        assert!(matches!(result, Err(AppError::Config(_))));
    }

    #[tokio::test]
    async fn recv_transcript_waits_for_file_based_provider() {
        let mut provider = GeminiSttProvider::new(None);
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(20),
            provider.recv_transcript(),
        )
        .await;
        assert!(result.is_err());
    }

    #[test]
    fn default_model_used_when_none() {
        let provider = GeminiSttProvider::new(None);
        assert_eq!(provider.model, DEFAULT_MODEL);
    }

    #[test]
    fn build_wav_has_valid_header() {
        let pcm = vec![0u8; 32];
        let wav = GeminiSttProvider::build_wav(&pcm, 16000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + pcm.len());
    }

    #[test]
    fn name_is_gemini() {
        let provider = GeminiSttProvider::new(None);
        assert_eq!(provider.name(), "gemini");
    }
}
