//! The planner's LLM transport, abstracted so the planner is testable without a
//! network.
//!
//! [`LlmClient`] is the seam: production uses [`GeminiLlmClient`] (the same cloud
//! `generateContent` transport as native STT, provider kept proprietary in
//! user-facing strings); tests inject a fixture client that returns canned JSON.

use std::time::Duration;

use async_trait::async_trait;

use crate::error::AppError;

/// The per-request timeout for Act's follow-up LLM calls (selection routing,
/// planner, answer) on both the Gemini and Cerebras transports.
///
/// The multi-step planner call can legitimately run 12-13s on a slow first hit,
/// so a 12s cap turned transient slowness into a hard "planner timed out" failure.
/// Raised to 25s to give a slow-but-succeeding call room to finish; the planner
/// still gets one timeout retry on top of this (see `planner.rs`).
pub const FOLLOWUP_LLM_TIMEOUT: Duration = Duration::from_secs(25);

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
            timeout: FOLLOWUP_LLM_TIMEOUT,
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
            // Minimize model "thinking" so flash / flash-lite return the structured
            // plan with the lowest latency (budget 0 = thinking off). Act selection
            // and planning are extraction/format tasks, not deep reasoning — the
            // thinking phase was a large chunk of the slow Gemini round-trips.
            "thinkingConfig": { "thinkingBudget": 0 },
        });
        if let Some(schema) = schema {
            let mut sanitized = schema.clone();
            sanitize_gemini_schema(&mut sanitized);
            generation_config["responseSchema"] = sanitized;
        }
        let body = serde_json::json!({
            "systemInstruction": { "parts": [{ "text": system }] },
            "contents": [{ "role": "user", "parts": [{ "text": user }] }],
            "generationConfig": generation_config,
        });

        tracing::debug!(model = %self.model, "Act LLM request");
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
            let snippet = raw[..truncate_at].to_string();
            // A bad model id (404) or auth/quota (403/429) here means Act silently
            // does nothing — the error only reaches the HUD. Log it so it is
            // visible in the terminal too.
            tracing::warn!(
                status = status.as_u16(),
                model = %self.model,
                body = %snippet,
                "Act LLM call failed"
            );
            return Err(AppError::Api {
                status: status.as_u16(),
                body: snippet,
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

/// Follow-up transport: an OpenAI-compatible `/chat/completions` provider, used
/// for Act's text-only follow-up calls (selection routing, planner, answer) when
/// the user opts into a faster model. Cerebras (`gpt-oss-120b`) is the first
/// option — very high tokens/sec, so the follow-ups return sooner than Gemini.
/// The FIRST/audio call always stays on Gemini (see `stt/gemini.rs`); this client
/// is text-only.
pub struct CerebrasLlmClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    timeout: std::time::Duration,
}

impl CerebrasLlmClient {
    /// The public Cerebras OpenAI-compatible endpoint base.
    pub const DEFAULT_BASE_URL: &'static str = "https://api.cerebras.ai/v1";

    pub fn new(client: reqwest::Client, api_key: String, model: String) -> Self {
        Self {
            client,
            api_key,
            model,
            base_url: Self::DEFAULT_BASE_URL.to_string(),
            timeout: FOLLOWUP_LLM_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    #[cfg(test)]
    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl LlmClient for CerebrasLlmClient {
    async fn generate_json(
        &self,
        system: &str,
        user: &str,
        schema: Option<&serde_json::Value>,
    ) -> Result<String, AppError> {
        if self.api_key.trim().is_empty() {
            return Err(AppError::Auth("Act follow-up API key is empty".into()));
        }
        // OpenAI-style JSON mode requires the word "json" somewhere in the
        // messages; the schema (when present) is folded into the system prompt so
        // the model both satisfies that constraint and sees the expected shape.
        let system_prompt = match schema {
            Some(schema) => format!(
                "{system}\n\nRespond with a single JSON object that conforms to this JSON Schema:\n{schema}"
            ),
            None => format!("{system}\n\nRespond with a single JSON object."),
        };
        let body = serde_json::json!({
            "model": self.model,
            "temperature": 0.0,
            "response_format": { "type": "json_object" },
            // gpt-oss models default to "medium" reasoning; Act selection/planning is
            // structured extraction, not deep reasoning, so pin the floor ("low") to
            // cut latency on the follow-up path. Mirrors Gemini's thinkingBudget=0.
            "reasoning_effort": "low",
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user },
            ],
        });

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        tracing::debug!(model = %self.model, "Act follow-up LLM request (Cerebras)");
        let resp = self
            .client
            .post(&url)
            .bearer_auth(self.api_key.trim())
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
            let snippet = raw[..truncate_at].to_string();
            tracing::warn!(
                status = status.as_u16(),
                model = %self.model,
                body = %snippet,
                "Act follow-up LLM call failed (Cerebras)"
            );
            return Err(AppError::Api {
                status: status.as_u16(),
                body: snippet,
            });
        }

        let v: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| AppError::Config(e.to_string()))?;
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default();
        // Some OpenAI-compatible models still wrap JSON in a ```json fence despite
        // json_object mode; strip it so the downstream parser gets bare JSON.
        let text = strip_json_fence(content).to_string();
        if text.is_empty() {
            return Err(AppError::Config("Act follow-up returned no content".into()));
        }
        Ok(text)
    }
}

/// Strip a leading ```json / ``` fence and trailing ``` from a model response,
/// returning the bare inner text. A no-op when there is no fence.
fn strip_json_fence(s: &str) -> &str {
    let t = s.trim();
    let t = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))
        .unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t);
    t.trim()
}

/// Gemini's `responseSchema` accepts only a restricted OpenAPI 3.0 subset and
/// returns HTTP 400 on JSON-Schema keywords it doesn't recognise (most notably
/// `additionalProperties`, but also `$schema`/`$ref`/`$defs`/`definitions`/
/// `patternProperties`). Recursively strip those keys so any planner or
/// selection schema is accepted; the remaining keywords (`type`, `properties`,
/// `items`, `enum`, `required`, …) are all supported.
fn sanitize_gemini_schema(value: &mut serde_json::Value) {
    const UNSUPPORTED: &[&str] = &[
        "additionalProperties",
        "$schema",
        "$id",
        "$ref",
        "$defs",
        "definitions",
        "patternProperties",
    ];
    match value {
        serde_json::Value::Object(map) => {
            for key in UNSUPPORTED {
                map.remove(*key);
            }
            for child in map.values_mut() {
                sanitize_gemini_schema(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                sanitize_gemini_schema(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod cerebras_tests {
    use super::*;

    #[tokio::test]
    async fn empty_api_key_is_rejected_before_any_request() {
        let client = CerebrasLlmClient::new(
            reqwest::Client::new(),
            "   ".to_string(),
            "gpt-oss-120b".to_string(),
        )
        .with_base_url("http://127.0.0.1:0/v1".to_string())
        .with_timeout(std::time::Duration::from_millis(50));
        let err = client
            .generate_json("system", "user", None)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Auth(_)), "got: {err:?}");
    }
}

#[cfg(test)]
mod schema_tests {
    use super::sanitize_gemini_schema;

    #[test]
    fn strip_json_fence_handles_fenced_and_bare() {
        use super::strip_json_fence;
        assert_eq!(strip_json_fence("{\"a\":1}"), "{\"a\":1}");
        assert_eq!(strip_json_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_json_fence("```\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_json_fence("  {\"a\":1}  "), "{\"a\":1}");
    }

    #[test]
    fn strips_additional_properties_recursively() {
        let mut schema = serde_json::json!({
            "type": "object",
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "additionalProperties": false,
            "properties": {
                "missions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": { "type": "string" },
                        "properties": { "id": { "type": "string" } }
                    }
                }
            }
        });
        sanitize_gemini_schema(&mut schema);
        let flat = schema.to_string();
        assert!(!flat.contains("additionalProperties"), "got: {flat}");
        assert!(!flat.contains("$schema"), "got: {flat}");
        // Supported keywords survive.
        assert!(flat.contains("properties"));
        assert!(flat.contains("\"id\""));
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
