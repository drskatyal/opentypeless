//! The planner's LLM transport, abstracted so the planner is testable without a
//! network.
//!
//! [`LlmClient`] is the seam: production uses [`GeminiLlmClient`] (the same cloud
//! `generateContent` transport as native STT, provider kept proprietary in
//! user-facing strings); tests inject a fixture client that returns canned JSON.

use async_trait::async_trait;

use crate::error::AppError;

/// Turns a (system, user) prompt pair into a JSON string response. The `schema`
/// is an optional JSON Schema the transport may pass through to constrain output.
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn generate_json(
        &self,
        system: &str,
        user: &str,
        schema: Option<&serde_json::Value>,
    ) -> Result<String, AppError>;
}

/// Production transport: cloud `generateContent` with `responseMimeType:
/// application/json`, temperature 0, and an optional `responseSchema`.
pub struct GeminiLlmClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
    timeout: std::time::Duration,
}

impl GeminiLlmClient {
    pub fn new(client: reqwest::Client, api_key: String, model: String) -> Self {
        Self {
            client,
            api_key,
            model,
            timeout: std::time::Duration::from_secs(12),
        }
    }

    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl LlmClient for GeminiLlmClient {
    async fn generate_json(
        &self,
        system: &str,
        user: &str,
        schema: Option<&serde_json::Value>,
    ) -> Result<String, AppError> {
        if self.api_key.trim().is_empty() {
            return Err(AppError::Auth("Act planner API key is empty".into()));
        }
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        );
        let mut generation_config = serde_json::json!({
            "temperature": 0.0,
            "responseMimeType": "application/json",
        });
        if let Some(schema) = schema {
            generation_config["responseSchema"] = schema.clone();
        }
        let body = serde_json::json!({
            "systemInstruction": { "parts": [{ "text": system }] },
            "contents": [{ "role": "user", "parts": [{ "text": user }] }],
            "generationConfig": generation_config,
        });

        let resp = self
            .client
            .post(&url)
            .header("x-goog-api-key", self.api_key.trim())
            .json(&body)
            .timeout(self.timeout)
            .send()
            .await?;

        let status = resp.status();
        let raw = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            let truncate_at = raw
                .char_indices()
                .take_while(|&(i, _)| i < 200)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(raw.len());
            return Err(AppError::Api {
                status: status.as_u16(),
                body: raw[..truncate_at].to_string(),
            });
        }

        let v: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| AppError::Config(e.to_string()))?;
        let text = v["candidates"][0]["content"]["parts"]
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p["text"].as_str())
                    .collect::<String>()
            })
            .unwrap_or_default()
            .trim()
            .to_string();
        if text.is_empty() {
            return Err(AppError::Config("Act planner returned no content".into()));
        }
        Ok(text)
    }
}

#[cfg(test)]
pub mod test_support {
    use super::*;
    use std::sync::Mutex;

    /// A fixture [`LlmClient`] that records prompts and returns canned responses
    /// in order, so the planner can be tested without a network.
    pub struct FixtureLlmClient {
        responses: Mutex<Vec<Result<String, AppError>>>,
        pub calls: Mutex<Vec<(String, String)>>,
    }

    impl FixtureLlmClient {
        pub fn new(responses: Vec<Result<String, AppError>>) -> Self {
            Self {
                responses: Mutex::new(responses),
                calls: Mutex::new(Vec::new()),
            }
        }

        pub fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl LlmClient for FixtureLlmClient {
        async fn generate_json(
            &self,
            system: &str,
            user: &str,
            _schema: Option<&serde_json::Value>,
        ) -> Result<String, AppError> {
            self.calls
                .lock()
                .unwrap()
                .push((system.to_string(), user.to_string()));
            let mut r = self.responses.lock().unwrap();
            if r.is_empty() {
                return Err(AppError::Config("fixture exhausted".into()));
            }
            r.remove(0)
        }
    }
}
