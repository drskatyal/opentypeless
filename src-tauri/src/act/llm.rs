//! The planner's LLM transport, abstracted so the planner is testable without a
//! network.
//!
//! [`LlmClient`] is the seam: production uses [`GeminiLlmClient`] (the same cloud
//! `generateContent` transport as native STT, provider kept proprietary in
//! user-facing strings); tests inject a fixture client that returns canned JSON.

use std::time::Duration;

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};

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

    /// Whether this transport can actually SEE an attached screenshot. Only a
    /// multimodal transport ([`GeminiLlmClient`]) returns `true`; the text-only
    /// transports (Cerebras `gpt-oss-120b`, the test fixture) return `false`.
    ///
    /// The planner uses this to avoid a silent failure mode: in `hybrid` / `vision`
    /// plan modes a text-only client would be handed a coordinate-click prompt for a
    /// screenshot it cannot see, and would click blindly. When this is `false` the
    /// planner degrades the perception to `tree` instead.
    fn is_multimodal(&self) -> bool {
        false
    }

    /// Like [`generate_json`](Self::generate_json), but with an optional PNG
    /// screenshot for the `hybrid` / `vision` plan modes. The default **ignores**
    /// the image and delegates to the text path, so a text-only transport
    /// (Cerebras `gpt-oss-120b`, the test fixture) can never "see" — only a
    /// multimodal transport ([`GeminiLlmClient`]) overrides this to attach the
    /// image. See `docs/act-screen-aware-design.md`.
    async fn generate_json_multimodal(
        &self,
        system: &str,
        user: &str,
        _image_png: Option<&[u8]>,
        schema: Option<&serde_json::Value>,
    ) -> Result<String, AppError> {
        self.generate_json(system, user, schema).await
    }
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
    fn is_multimodal(&self) -> bool {
        true
    }

    async fn generate_json(
        &self,
        system: &str,
        user: &str,
        schema: Option<&serde_json::Value>,
    ) -> Result<String, AppError> {
        self.generate_inner(system, user, None, schema).await
    }

    async fn generate_json_multimodal(
        &self,
        system: &str,
        user: &str,
        image_png: Option<&[u8]>,
        schema: Option<&serde_json::Value>,
    ) -> Result<String, AppError> {
        self.generate_inner(system, user, image_png, schema).await
    }
}

impl GeminiLlmClient {
    /// Shared request path for the text and multimodal calls. When `image_png` is
    /// `Some`, a PNG `inlineData` part is attached alongside the user text.
    async fn generate_inner(
        &self,
        system: &str,
        user: &str,
        image_png: Option<&[u8]>,
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
        let mut parts = vec![serde_json::json!({ "text": user })];
        if let Some(png) = image_png {
            parts.push(serde_json::json!({
                "inlineData": { "mimeType": "image/png", "data": STANDARD.encode(png) }
            }));
        }
        let body = serde_json::json!({
            "systemInstruction": { "parts": [{ "text": system }] },
            "contents": [{ "role": "user", "parts": parts }],
            "generationConfig": generation_config,
        });

        tracing::debug!(model = %self.model, has_image = image_png.is_some(), "Act LLM request");
        // Full prompt capture for the in-app Diagnostics panel (crate::diag). The
        // system prompt is large and static, so log only its length; the user
        // message is the dynamic, useful part, so log it in full (truncated for
        // sanity). Screenshots are never logged (only a flag).
        tracing::debug!(
            target: "opentypeless_lib::act::llm::io",
            model = %self.model,
            has_image = image_png.is_some(),
            system_len = system.len(),
            user = %truncate_for_log(user, 6000),
            "Act LLM prompt (Gemini)"
        );
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
        tracing::debug!(
            target: "opentypeless_lib::act::llm::io",
            model = %self.model,
            response = %truncate_for_log(&text, 6000),
            "Act LLM response (Gemini)"
        );
        Ok(text)
    }
}

/// Truncate a prompt/response for logging so a huge tree snapshot doesn't flood
/// the diagnostics buffer. Cuts on a char boundary and marks the elision.
fn truncate_for_log(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let end = s
        .char_indices()
        .take_while(|&(i, _)| i < max)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    format!("{}… [+{} bytes]", &s[..end], s.len() - end)
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
        tracing::debug!(
            target: "opentypeless_lib::act::llm::io",
            model = %self.model,
            system_len = system_prompt.len(),
            user = %truncate_for_log(user, 6000),
            "Act LLM prompt (Cerebras)"
        );
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
        tracing::debug!(
            target: "opentypeless_lib::act::llm::io",
            model = %self.model,
            response = %truncate_for_log(&text, 6000),
            "Act LLM response (Cerebras)"
        );
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
