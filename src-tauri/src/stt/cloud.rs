use async_trait::async_trait;

use crate::error::{managed_cloud_error, AppError};
use crate::with_desktop_client_version;

use super::whisper_compat::WhisperCompatProvider;
use super::{SttConfig, SttProvider, TranscriptEvent};

/// Cloud STT provider that proxies audio through the talkmore-web API.
/// Auth token is passed via the api_key field. Quota is enforced server-side.
pub struct CloudSttProvider {
    stt_config: Option<SttConfig>,
    audio_buffer: Vec<u8>,
    client: reqwest::Client,
    api_base_url: String,
}

/// Max audio buffer: ~24 MB PCM ≈ 12.5 min at 16kHz 16-bit mono.
const MAX_AUDIO_BYTES: usize = 24 * 1024 * 1024;

fn contains_quota_marker(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("quota")
        || value.contains("limit exceeded")
        || value.contains("usage exceeded")
        || value.contains("byok")
}

fn forbidden_error_message(value: &serde_json::Value) -> Option<String> {
    value
        .get("error")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("message").and_then(|v| v.as_str()))
        .or_else(|| {
            value
                .get("error")
                .and_then(|v| v.get("message"))
                .and_then(|v| v.as_str())
        })
        .map(String::from)
}

fn quota_message_from_value(value: &serde_json::Value) -> Option<String> {
    for field in ["code", "error_code", "type"] {
        if value
            .get(field)
            .and_then(|v| v.as_str())
            .is_some_and(contains_quota_marker)
        {
            return Some(
                forbidden_error_message(value)
                    .unwrap_or_else(|| "Cloud STT quota exceeded".to_string()),
            );
        }
    }

    for field in ["error", "message"] {
        if let Some(item) = value.get(field) {
            if let Some(message) = item.as_str() {
                if contains_quota_marker(message) {
                    return Some(message.to_string());
                }
            } else if let Some(message) = quota_message_from_value(item) {
                return Some(message);
            }
        }
    }

    None
}

fn cloud_stt_forbidden_error(body: &str) -> AppError {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok();

    if let Some(value) = parsed.as_ref() {
        if let Some(message) = quota_message_from_value(value) {
            return AppError::Quota(message);
        }
    }

    AppError::Auth("Cloud STT access denied".to_string())
}

impl CloudSttProvider {
    pub fn new(api_base_url: String) -> Self {
        Self {
            stt_config: None,
            audio_buffer: Vec::new(),
            client: reqwest::Client::new(),
            api_base_url,
        }
    }

    pub fn with_client(api_base_url: String, client: reqwest::Client) -> Self {
        Self {
            stt_config: None,
            audio_buffer: Vec::new(),
            client,
            api_base_url,
        }
    }
}

#[async_trait]
impl SttProvider for CloudSttProvider {
    async fn connect(&mut self, config: &SttConfig) -> Result<(), AppError> {
        if config.api_key.is_empty() {
            return Err(AppError::Auth(
                "Cloud STT: session token is missing. Please sign in first.".to_string(),
            ));
        }
        self.stt_config = Some(config.clone());
        self.audio_buffer.clear();
        tracing::info!("Cloud STT provider ready (buffering mode)");
        Ok(())
    }

    async fn send_audio(&mut self, chunk: &[u8]) -> Result<(), AppError> {
        if self.audio_buffer.len() + chunk.len() > MAX_AUDIO_BYTES {
            return Err(AppError::Config(
                "Cloud STT: audio exceeds maximum length (~12 min)".to_string(),
            ));
        }
        self.audio_buffer.extend_from_slice(chunk);
        Ok(())
    }

    async fn recv_transcript(&mut self) -> Result<Option<TranscriptEvent>, AppError> {
        // Cloud STT is file-upload based and returns the final transcript from
        // disconnect(); keep this future pending so the pipeline select loop
        // waits for audio chunks instead of busy-spinning while recording.
        std::future::pending().await
    }

    async fn disconnect(&mut self) -> Result<Option<String>, AppError> {
        let config = match &self.stt_config {
            Some(c) => c.clone(),
            None => return Ok(None),
        };

        if self.audio_buffer.is_empty() {
            tracing::info!("Cloud STT: no audio buffered, skipping");
            return Ok(None);
        }

        let audio_len_secs = self.audio_buffer.len() as f64 / (config.sample_rate as f64 * 2.0);
        let wav_data = WhisperCompatProvider::build_wav(&self.audio_buffer, config.sample_rate);
        self.audio_buffer.clear();
        tracing::info!(
            "Cloud STT: sending {:.1}s of audio for transcription",
            audio_len_secs
        );

        let mut attempt = 0u32;
        loop {
            let file_part = reqwest::multipart::Part::bytes(wav_data.clone())
                .file_name("audio.wav")
                .mime_str("audio/wav")
                .map_err(|e| AppError::Config(e.to_string()))?;

            let mut form = reqwest::multipart::Form::new().part("audio", file_part);

            if let Some(ref lang) = config.language {
                form = form.text("language", lang.clone());
            }
            if let Some(operation_id) = config.operation_id.as_deref() {
                form = form
                    .text("operationId", operation_id.to_string())
                    .text("stageKey", format!("{operation_id}:stt"))
                    .text("requestType", "voice_pipeline")
                    .text("clientVersion", crate::desktop_client_version());
            }

            let resp_result = with_desktop_client_version(
                self.client
                    .post(format!("{}/api/proxy/stt", self.api_base_url)),
            )
            .header("Authorization", format!("Bearer {}", config.api_key))
            .multipart(form)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await;

            match resp_result {
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();

                    if status.is_success() {
                        let v: serde_json::Value = serde_json::from_str(&body)
                            .map_err(|e| AppError::Config(e.to_string()))?;
                        let text = v["text"].as_str().unwrap_or("").trim().to_string();

                        tracing::info!("Cloud STT transcription: {} chars", text.len());

                        return Ok(if text.is_empty() { None } else { Some(text) });
                    } else if let Some(error) = managed_cloud_error(status.as_u16(), &body) {
                        return Err(error);
                    } else if status.as_u16() == 403 {
                        return Err(cloud_stt_forbidden_error(&body));
                    } else if status.as_u16() >= 500 && attempt < 2 {
                        let truncate_at = body
                            .char_indices()
                            .take_while(|&(i, _)| i < 200)
                            .last()
                            .map(|(i, c)| i + c.len_utf8())
                            .unwrap_or(body.len());
                        tracing::warn!(
                            "Cloud STT server error {} (attempt {}/3): {}",
                            status,
                            attempt + 1,
                            &body[..truncate_at]
                        );
                        attempt += 1;
                        tokio::time::sleep(std::time::Duration::from_millis(
                            1000 * 2u64.pow(attempt - 1),
                        ))
                        .await;
                        continue;
                    } else {
                        let truncate_at = body
                            .char_indices()
                            .take_while(|&(i, _)| i < 200)
                            .last()
                            .map(|(i, c)| i + c.len_utf8())
                            .unwrap_or(body.len());
                        let sanitized = &body[..truncate_at];
                        tracing::error!("Cloud STT HTTP {}: {}", status, sanitized);
                        return Err(AppError::Api {
                            status: status.as_u16(),
                            body: sanitized.to_string(),
                        });
                    }
                }
                Err(e) if e.is_timeout() && attempt < 2 => {
                    tracing::warn!("Cloud STT timeout (attempt {}/3)", attempt + 1);
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(
                        1000 * 2u64.pow(attempt - 1),
                    ))
                    .await;
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    fn name(&self) -> &str {
        "Cloud"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_error_uses_quota_code() {
        let err = cloud_stt_forbidden_error(r#"{"code":"stt_quota_exceeded","error":"limit hit"}"#);
        assert!(matches!(err, AppError::Quota(_)));
    }

    #[test]
    fn forbidden_error_uses_quota_message() {
        let err = cloud_stt_forbidden_error(
            r#"{"error":"STT quota exceeded. Please switch to BYOK mode."}"#,
        );
        assert!(matches!(err, AppError::Quota(_)));
    }

    #[test]
    fn forbidden_error_uses_nested_quota_message() {
        let err = cloud_stt_forbidden_error(
            r#"{"error":{"code":"stt_quota_exceeded","message":"STT quota exceeded"}}"#,
        );
        assert!(matches!(err, AppError::Quota(_)));
    }

    #[test]
    fn forbidden_error_empty_body_is_auth_not_quota() {
        let err = cloud_stt_forbidden_error("");
        assert!(matches!(err, AppError::Auth(_)));
    }

    #[test]
    fn forbidden_error_unknown_json_is_auth_not_quota() {
        let err = cloud_stt_forbidden_error(r#"{"error":"Forbidden"}"#);
        assert!(matches!(err, AppError::Auth(_)));
    }

    #[tokio::test]
    async fn recv_transcript_waits_for_buffered_cloud_provider() {
        let mut provider = CloudSttProvider::new("https://example.test".to_string());

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(20),
            provider.recv_transcript(),
        )
        .await;

        assert!(result.is_err());
    }
}
