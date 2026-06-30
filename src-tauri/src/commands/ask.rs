use crate::audio::{AudioCaptureHandle, AudioConfig};
use crate::pipeline::PipelineState;
use crate::storage;
use crate::stt::{self, SttConfig, TranscriptEvent};
use crate::{api_base_url, with_desktop_client_version, SessionTokenStore};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tauri::Emitter;
use tokio::sync::Notify;

pub const ASK_MAX_QUESTION_CHARS: usize = 500;
pub const ASK_OUTPUT_TOKEN_LIMIT: u32 = 80;
const ASK_STT_FINALIZE_TIMEOUT_SECS: u64 = 12;

#[derive(Default)]
pub struct AskDictationState(Arc<Mutex<AskDictationStateInner>>);

#[derive(Default)]
struct AskDictationStateInner {
    starting: bool,
    stop_after_start: bool,
    session: Option<AskDictationSession>,
    processing: bool,
    pending_message: Option<PendingAskMessage>,
}

impl AskDictationState {
    pub fn is_recording(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .session
            .is_some()
    }

    pub fn is_busy(&self) -> bool {
        let guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        guard.starting || guard.session.is_some() || guard.processing
    }

    pub fn is_starting(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).starting
    }

    pub(crate) fn try_begin_starting(&self) -> bool {
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if guard.starting || guard.session.is_some() || guard.processing {
            return false;
        }
        guard.starting = true;
        guard.stop_after_start = false;
        true
    }

    fn clear_starting(&self) {
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        guard.starting = false;
        guard.stop_after_start = false;
    }

    fn set_processing(&self, processing: bool) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).processing = processing;
    }

    pub fn set_pending_result(&self, result: AskDictationResult) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending_message = Some(PendingAskMessage::Result(result));
    }

    pub fn set_pending_error(&self, message: String) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending_message = Some(PendingAskMessage::Error(message));
    }

    fn take_pending_message(&self) -> Option<PendingAskMessage> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending_message
            .take()
    }

    pub fn request_stop_after_start(&self) -> bool {
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if !guard.starting {
            return false;
        }
        guard.stop_after_start = true;
        true
    }

    pub fn take_stop_after_start(&self) -> bool {
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let should_stop = guard.stop_after_start;
        guard.stop_after_start = false;
        should_stop
    }

    fn abort_starting_or_recording(&self) -> (Option<AskDictationSession>, bool) {
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let was_starting = guard.starting;
        guard.starting = false;
        guard.stop_after_start = false;
        (guard.session.take(), was_starting)
    }
}

pub struct AskDictationSession {
    handle: AudioCaptureHandle,
    operation_id: String,
    transcript: Arc<Mutex<String>>,
    error: Arc<Mutex<Option<String>>>,
    done: Arc<Notify>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskDictationResult {
    question: String,
    answer: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "kind", content = "payload", rename_all = "camelCase")]
pub enum PendingAskMessage {
    Result(AskDictationResult),
    Error(String),
}

fn emit_capsule_state(app: &tauri::AppHandle, state: PipelineState) {
    let _ = app.emit("pipeline:state", state);
}

fn show_ask_error_window(app: &tauri::AppHandle, message: &str) {
    match crate::show_ask_popup_window(app) {
        Ok(window) => {
            let _ = window.emit("ask:error", message);
        }
        Err(error) => {
            tracing::error!("Failed to show Ask error window: {}", error);
        }
    }
}

fn should_surface_async_recording_error(
    session_operation_id: Option<&str>,
    processing: bool,
    operation_id: &str,
) -> bool {
    !processing && session_operation_id == Some(operation_id)
}

fn surface_async_recording_error(
    app: &tauri::AppHandle,
    state: &Arc<Mutex<AskDictationStateInner>>,
    operation_id: &str,
    message: String,
) {
    let session = {
        let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
        if !should_surface_async_recording_error(
            guard
                .session
                .as_ref()
                .map(|session| session.operation_id.as_str()),
            guard.processing,
            operation_id,
        ) {
            return;
        }

        guard.pending_message = Some(PendingAskMessage::Error(message.clone()));
        guard.session.take()
    };

    if let Some(mut session) = session {
        session.handle.stop();
    }
    emit_capsule_state(app, PipelineState::Idle);
    show_ask_error_window(app, &message);
}

fn synthetic_operation_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (now >> 96) as u32,
        (now >> 80) as u16,
        (now >> 64) as u16,
        (now >> 48) as u16,
        now & 0x0000_ffff_ffff_ffff_ffffu128
    )
}

pub fn validate_ask_question(question: &str) -> Result<String, String> {
    let trimmed = question.trim().to_string();
    if trimmed.is_empty() {
        return Err("Question is required".to_string());
    }
    if trimmed.chars().count() > ASK_MAX_QUESTION_CHARS {
        return Err(format!(
            "Question is too long (max {} characters)",
            ASK_MAX_QUESTION_CHARS
        ));
    }
    Ok(trimmed)
}

fn validate_ask_answer(answer: &str) -> Result<String, String> {
    let trimmed = answer.trim().to_string();
    if trimmed.is_empty() {
        return Err("Ask returned an empty answer. Please try again.".to_string());
    }
    Ok(trimmed)
}

fn ask_messages(question: &str) -> Vec<serde_json::Value> {
    vec![
        json!({
            "role": "system",
            "content": "Answer clearly and directly in the same language as the user. Keep the answer under 40 words. Do not use web search, external browsing, or selected-text context."
        }),
        json!({ "role": "user", "content": question }),
    ]
}

pub fn build_byok_ask_body(question: &str, model: &str) -> Result<serde_json::Value, String> {
    let question = validate_ask_question(question)?;
    let mut body = json!({
        "model": model,
        "messages": ask_messages(&question),
        "max_tokens": ASK_OUTPUT_TOKEN_LIMIT,
        "temperature": 0.2,
        "stream": false
    });

    if model.starts_with("glm-") {
        if let Some(obj) = body.as_object_mut() {
            obj.insert(
                "thinking".to_string(),
                json!({
                    "type": "enabled"
                }),
            );
            obj.insert("temperature".to_string(), json!(1.0));
            obj.insert("top_p".to_string(), json!(0.95));
        }
    }

    Ok(body)
}

fn should_use_byok(config: &storage::AppConfig) -> bool {
    if config.llm_provider == "cloud" {
        return false;
    }
    if config.llm_base_url.trim().is_empty() || config.llm_model.trim().is_empty() {
        return false;
    }
    !config.llm_api_key.trim().is_empty() || config.llm_provider == "ollama"
}

fn should_use_cloud(config: &storage::AppConfig) -> bool {
    config.llm_provider == "cloud"
}

fn build_ask_stt_config(
    config: &storage::AppConfig,
    api_key: String,
    operation_id: String,
) -> SttConfig {
    SttConfig {
        api_key,
        language: if config.stt_language == "multi" {
            None
        } else {
            Some(config.stt_language.clone())
        },
        smart_format: true,
        sample_rate: 16000,
        resource_id: if config.stt_provider == stt::volcengine::VOLCENGINE_DOUBAO_PROVIDER {
            Some(config.stt_volcengine_resource_id.clone())
        } else {
            None
        },
        operation_id: Some(operation_id),
    }
}

fn ask_stt_api_key(config: &storage::AppConfig, token_store: &SessionTokenStore) -> String {
    if config.stt_provider == "cloud" {
        return token_store
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
    }
    if config.stt_provider == stt::config::CUSTOM_WHISPER_PROVIDER {
        return config.stt_custom_api_key.clone();
    }
    config.stt_api_key.clone()
}

fn cloud_auth_required_message() -> String {
    "Sign in to use Cloud Ask, or switch to BYOK.".to_string()
}

fn map_audio_capture_error(message: &str) -> String {
    let normalized = message.to_ascii_lowercase();
    if normalized.contains("permission")
        || normalized.contains("access denied")
        || normalized.contains("not authorized")
    {
        return "Microphone permission is required.".to_string();
    }

    if normalized.contains("no input device")
        || normalized.contains("default input")
        || normalized.contains("device")
    {
        return "Microphone unavailable. Check your input device.".to_string();
    }

    "Microphone unavailable. Check your input device.".to_string()
}

fn append_final_transcript(transcript: &Arc<Mutex<String>>, text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return transcript.lock().unwrap_or_else(|e| e.into_inner()).clone();
    }

    let mut current = transcript.lock().unwrap_or_else(|e| e.into_inner());
    if !current.trim().is_empty() && !current.ends_with(' ') {
        current.push(' ');
    }
    current.push_str(text);
    current.trim().to_string()
}

async fn answer_question(
    config: &storage::AppConfig,
    client: &reqwest::Client,
    token_store: &SessionTokenStore,
    question: &str,
    operation_id: Option<&str>,
) -> Result<String, String> {
    if should_use_byok(config) {
        return ask_via_byok(client, config, question).await;
    }

    if should_use_cloud(config) {
        return ask_via_cloud(client, token_store, question, operation_id).await;
    }

    Err("Configure a BYOK LLM provider or choose Cloud LLM to use Ask.".to_string())
}

fn response_error(status: reqwest::StatusCode, text: String) -> String {
    let sanitized: String = text.chars().take(200).collect();
    format!("Ask request failed ({}): {}", status.as_u16(), sanitized)
}

fn cloud_response_error(status: reqwest::StatusCode, text: String) -> String {
    let parsed = serde_json::from_str::<serde_json::Value>(&text).ok();
    let message = parsed
        .as_ref()
        .and_then(extract_cloud_error_message)
        .or_else(|| {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });

    if status.as_u16() == 401 {
        return cloud_auth_required_message();
    }

    if status.as_u16() == 403 {
        let quota_message = message
            .as_deref()
            .filter(|value| contains_quota_marker(value))
            .map(ToString::to_string);
        return quota_message.unwrap_or_else(cloud_auth_required_message);
    }

    if status.as_u16() >= 500 {
        return "Ask service error. Please try again.".to_string();
    }

    let sanitized: String = message
        .unwrap_or_else(|| "Ask request failed. Please try again.".to_string())
        .chars()
        .take(160)
        .collect();
    format!("Ask request failed ({}): {}", status.as_u16(), sanitized)
}

fn contains_quota_marker(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("quota")
        || value.contains("limit exceeded")
        || value.contains("usage exceeded")
        || value.contains("cloud words")
        || value.contains("byok")
}

fn extract_cloud_error_message(value: &serde_json::Value) -> Option<String> {
    for field in ["error", "message"] {
        match value.get(field) {
            Some(serde_json::Value::String(message)) => return Some(message.clone()),
            Some(nested) => {
                if let Some(message) = extract_cloud_error_message(nested) {
                    return Some(message);
                }
            }
            None => {}
        }
    }

    None
}

async fn ask_via_byok(
    client: &reqwest::Client,
    config: &storage::AppConfig,
    question: &str,
) -> Result<String, String> {
    let parsed =
        url::Url::parse(&config.llm_base_url).map_err(|e| format!("Invalid LLM base URL: {e}"))?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err("LLM base URL must use http or https scheme".to_string());
    }

    let url = format!(
        "{}/chat/completions",
        config.llm_base_url.trim_end_matches('/')
    );
    let mut request = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&build_byok_ask_body(question, &config.llm_model)?)
        .timeout(std::time::Duration::from_secs(30));

    if !config.llm_api_key.trim().is_empty() {
        request = request.header("Authorization", format!("Bearer {}", config.llm_api_key));
    }

    let resp = request.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(response_error(status, text));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    extract_byok_ask_answer(&body)
}

fn extract_byok_ask_answer(body: &serde_json::Value) -> Result<String, String> {
    let message = &body["choices"][0]["message"];
    if let Some(content) = message["content"].as_str() {
        if !content.trim().is_empty() {
            return validate_ask_answer(content);
        }
    }

    validate_ask_answer(message["reasoning_content"].as_str().unwrap_or(""))
}

async fn ask_via_cloud(
    client: &reqwest::Client,
    token_store: &SessionTokenStore,
    question: &str,
    operation_id: Option<&str>,
) -> Result<String, String> {
    let token = token_store
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if token.trim().is_empty() {
        return Err(cloud_auth_required_message());
    }

    let operation_id = operation_id
        .map(str::to_string)
        .unwrap_or_else(synthetic_operation_id);
    let stage_key = format!("{operation_id}:ask");
    let body = json!({
        "question": question,
        "context": {
            "operationId": operation_id,
            "stageKey": stage_key,
            "requestType": "ask_anything",
            "clientVersion": crate::desktop_client_version()
        }
    });

    let resp =
        with_desktop_client_version(client.post(format!("{}/api/proxy/ask", api_base_url())))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(45))
            .send()
            .await
            .map_err(|e| e.to_string())?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(cloud_response_error(status, text));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    validate_ask_answer(body["answer"].as_str().unwrap_or(""))
}

#[tauri::command]
pub async fn ask_anything(
    question: String,
    config_state: tauri::State<'_, storage::ConfigManager>,
    token_store: tauri::State<'_, SessionTokenStore>,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<String, String> {
    let question = validate_ask_question(&question)?;
    let config = config_state.load().await.map_err(|e| e.to_string())?;

    answer_question(&config, &client, &token_store, &question, None).await
}

#[tauri::command]
pub async fn start_ask_dictation(
    app: tauri::AppHandle,
    state: tauri::State<'_, AskDictationState>,
    config_state: tauri::State<'_, storage::ConfigManager>,
    token_store: tauri::State<'_, SessionTokenStore>,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<(), String> {
    if !state.try_begin_starting() {
        return Ok(());
    }

    start_reserved_ask_dictation(app, state, config_state, token_store, client).await
}

pub(crate) async fn start_reserved_ask_dictation(
    app: tauri::AppHandle,
    state: tauri::State<'_, AskDictationState>,
    config_state: tauri::State<'_, storage::ConfigManager>,
    token_store: tauri::State<'_, SessionTokenStore>,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<(), String> {
    let result = async {
        let config = config_state.load().await.map_err(|e| e.to_string())?;
        let stt_api_key = ask_stt_api_key(&config, &token_store);
        if stt::config::stt_provider_requires_api_key(&config.stt_provider)
            && stt_api_key.is_empty()
        {
            return Err(
                "STT API key is not configured. Please set it in Settings -> Speech Recognition."
                    .to_string(),
            );
        }

        let custom_whisper_config = if config.stt_provider == stt::config::CUSTOM_WHISPER_PROVIDER
        {
            Some(stt::config::build_custom_whisper_config(
                &config.stt_custom_base_url,
                &config.stt_custom_model,
            )?)
        } else {
            None
        };
        let operation_id = synthetic_operation_id();
        let stt_config = build_ask_stt_config(&config, stt_api_key, operation_id.clone());
        let mut provider = stt::create_provider(
            &config.stt_provider,
            custom_whisper_config,
            Some(client.inner().clone()),
        )
        .map_err(|e| e.to_string())?;
        provider
            .connect(&stt_config)
            .await
            .map_err(|e| e.to_string())?;

        let (handle, mut audio_rx) = AudioCaptureHandle::start(AudioConfig::default())
            .map_err(|e| map_audio_capture_error(&e.to_string()))?;
        let mut handle = Some(handle);
        let transcript = Arc::new(Mutex::new(String::new()));
        let error = Arc::new(Mutex::new(None::<String>));
        let done = Arc::new(Notify::new());
        let task_operation_id = operation_id.clone();

        let should_discard_started_resources = {
            let mut guard = state.0.lock().unwrap_or_else(|e| e.into_inner());
            if !guard.starting || guard.session.is_some() || guard.processing {
                guard.starting = false;
                guard.stop_after_start = false;
                true
            } else {
                guard.starting = false;
                guard.session = Some(AskDictationSession {
                    handle: handle.take().expect("Ask audio handle was already consumed"),
                    operation_id,
                    transcript: transcript.clone(),
                    error: error.clone(),
                    done: done.clone(),
                });
                false
            }
        };

        if should_discard_started_resources {
            if let Some(mut handle) = handle {
                handle.stop();
            }
            let _ = provider.disconnect().await;
            return Ok(());
        }

        emit_capsule_state(&app, PipelineState::Recording);
        let state_inner = state.0.clone();

        tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    chunk = audio_rx.recv() => {
                        match chunk {
                            Some(data) => {
                                if let Err(e) = provider.send_audio(&data).await {
                                    let message = e.to_string();
                                    *error.lock().unwrap_or_else(|err| err.into_inner()) = Some(message.clone());
                                    surface_async_recording_error(
                                        &app,
                                        &state_inner,
                                        &task_operation_id,
                                        message,
                                    );
                                    break;
                                }
                            }
                            None => {
                                match provider.disconnect().await {
                                    Ok(Some(text)) => {
                                        let current = append_final_transcript(&transcript, &text);
                                        let _ = app.emit("ask:final", current);
                                    }
                                    Ok(None) => {}
                                    Err(e) => {
                                        let message = e.to_string();
                                        *error.lock().unwrap_or_else(|err| err.into_inner()) = Some(message.clone());
                                        surface_async_recording_error(
                                            &app,
                                            &state_inner,
                                            &task_operation_id,
                                            message,
                                        );
                                    }
                                }
                                break;
                            }
                        }
                    }
                    event = provider.recv_transcript() => {
                        match event {
                            Ok(Some(TranscriptEvent::Partial { text })) => {
                                let _ = app.emit("ask:partial", text);
                            }
                            Ok(Some(TranscriptEvent::Final { text, .. })) => {
                                let current = append_final_transcript(&transcript, &text);
                                let _ = app.emit("ask:final", current);
                            }
                            Ok(Some(TranscriptEvent::Error { message })) => {
                                *error.lock().unwrap_or_else(|err| err.into_inner()) = Some(message.clone());
                                surface_async_recording_error(
                                    &app,
                                    &state_inner,
                                    &task_operation_id,
                                    message,
                                );
                                break;
                            }
                            Err(e) => {
                                let message = e.to_string();
                                *error.lock().unwrap_or_else(|err| err.into_inner()) = Some(message.clone());
                                surface_async_recording_error(
                                    &app,
                                    &state_inner,
                                    &task_operation_id,
                                    message,
                                );
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }

            done.notify_waiters();
        });

        Ok(())
    }
    .await;

    if result.is_err() {
        state.clear_starting();
    }
    result
}

#[tauri::command]
pub async fn stop_ask_dictation(
    app: tauri::AppHandle,
    state: tauri::State<'_, AskDictationState>,
    config_state: tauri::State<'_, storage::ConfigManager>,
    token_store: tauri::State<'_, SessionTokenStore>,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<AskDictationResult, String> {
    let mut session = {
        let mut guard = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if guard.processing {
            return Err("Ask is already processing".to_string());
        }
        let session = guard
            .session
            .take()
            .ok_or_else(|| "Ask dictation is not recording".to_string())?;
        guard.stop_after_start = false;
        guard.processing = true;
        session
    };

    let result = async {
        session.handle.stop();
        emit_capsule_state(&app, PipelineState::Polishing);

        let finalize_timed_out = tokio::select! {
            _ = session.done.notified() => false,
            _ = tokio::time::sleep(std::time::Duration::from_secs(ASK_STT_FINALIZE_TIMEOUT_SECS)) => {
                true
            }
        };

        if finalize_timed_out {
            let transcript = session
                .transcript
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(message) = session
                .error
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
            {
                return Err(message);
            }
            if transcript.trim().is_empty() {
                return Err("No speech detected. Please try again.".to_string());
            }
            tracing::warn!(
                "Ask STT finalize timed out; continuing with collected transcript"
            );
        }

        if let Some(message) = session
            .error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Err(message);
        }

        let question = validate_ask_question(
            &session
                .transcript
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
        )?;

        let config = config_state.load().await.map_err(|e| e.to_string())?;
        let answer = answer_question(
            &config,
            &client,
            &token_store,
            &question,
            Some(&session.operation_id),
        )
        .await?;

        Ok(AskDictationResult { question, answer })
    }
    .await;

    state.set_processing(false);
    match &result {
        Ok(_) => emit_capsule_state(&app, PipelineState::Outputting),
        Err(message) => {
            emit_capsule_state(&app, PipelineState::Idle);
            let _ = app.emit("ask:error", message.clone());
        }
    }

    result
}

#[tauri::command]
pub fn abort_ask_dictation(
    app: tauri::AppHandle,
    state: tauri::State<'_, AskDictationState>,
) -> Result<(), String> {
    let (session, was_starting) = state.abort_starting_or_recording();
    if let Some(mut session) = session {
        session.handle.stop();
        emit_capsule_state(&app, PipelineState::Idle);
    } else if was_starting {
        emit_capsule_state(&app, PipelineState::Idle);
    }
    Ok(())
}

#[tauri::command]
pub fn take_pending_ask_message(
    state: tauri::State<'_, AskDictationState>,
) -> Result<Option<PendingAskMessage>, String> {
    Ok(state.take_pending_message())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_body_uses_low_output_cap_and_no_web_search() {
        let body = build_byok_ask_body("What is OpenTypeless?", "test-model").unwrap();

        assert_eq!(body["model"], "test-model");
        assert_eq!(body["max_tokens"], ASK_OUTPUT_TOKEN_LIMIT);
        assert_eq!(body["stream"], false);

        let messages = body["messages"].as_array().unwrap();
        let system_prompt = messages[0]["content"].as_str().unwrap();
        assert!(system_prompt.contains("40 words"));
        assert!(system_prompt.contains("Do not use web search"));
    }

    #[test]
    fn byok_ask_body_enables_glm_thinking_mode() {
        let body = build_byok_ask_body("What is OpenTypeless?", "glm-4.7").unwrap();

        assert_eq!(body["max_tokens"], ASK_OUTPUT_TOKEN_LIMIT);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["temperature"], 1.0);
        assert_eq!(body["top_p"], 0.95);
    }

    #[test]
    fn byok_ask_answer_falls_back_to_reasoning_content() {
        let body = json!({
            "choices": [
                {
                    "message": {
                        "content": "",
                        "reasoning_content": "Use Command+Period to ask."
                    }
                }
            ]
        });

        assert_eq!(
            extract_byok_ask_answer(&body).unwrap(),
            "Use Command+Period to ask."
        );
    }

    #[test]
    fn ask_question_validation_rejects_empty_or_oversized_questions() {
        assert!(validate_ask_question("   ").is_err());
        assert!(validate_ask_question(&"x".repeat(ASK_MAX_QUESTION_CHARS + 1)).is_err());
        assert_eq!(
            validate_ask_question("  Explain polish mode.  ").unwrap(),
            "Explain polish mode."
        );
    }

    #[test]
    fn ask_answer_validation_rejects_empty_answers() {
        assert!(validate_ask_answer("").is_err());
        assert!(validate_ask_answer("   ").is_err());
        assert_eq!(
            validate_ask_answer("  A concise answer.  ").unwrap(),
            "A concise answer."
        );
    }

    #[test]
    fn ask_dictation_stt_config_uses_cloud_token_and_multi_language() {
        let config = storage::AppConfig {
            stt_provider: "cloud".to_string(),
            stt_language: "multi".to_string(),
            ..Default::default()
        };

        let stt_config = build_ask_stt_config(
            &config,
            "session-token".to_string(),
            "operation-1".to_string(),
        );

        assert_eq!(stt_config.api_key, "session-token");
        assert_eq!(stt_config.language, None);
        assert_eq!(stt_config.operation_id.as_deref(), Some("operation-1"));
    }

    #[test]
    fn cloud_ask_errors_are_short_and_actionable() {
        let quota = cloud_response_error(
            reqwest::StatusCode::FORBIDDEN,
            r#"{"code":"cloud_quota_exceeded","error":"Cloud words used up. Please switch to BYOK mode or wait until reset."}"#.to_string(),
        );
        assert_eq!(
            quota,
            "Cloud words used up. Please switch to BYOK mode or wait until reset."
        );

        let auth = cloud_response_error(reqwest::StatusCode::UNAUTHORIZED, String::new());
        assert_eq!(auth, "Sign in to use Cloud Ask, or switch to BYOK.");

        let service = cloud_response_error(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "upstream failed".to_string(),
        );
        assert_eq!(service, "Ask service error. Please try again.");
    }

    #[test]
    fn byok_ask_errors_do_not_use_cloud_auth_copy() {
        let message = response_error(reqwest::StatusCode::UNAUTHORIZED, "bad key".to_string());
        assert_eq!(message, "Ask request failed (401): bad key");
    }

    #[test]
    fn audio_capture_errors_are_user_readable() {
        assert_eq!(
            map_audio_capture_error("No input device available"),
            "Microphone unavailable. Check your input device."
        );
        assert_eq!(
            map_audio_capture_error("permission denied"),
            "Microphone permission is required."
        );
    }

    #[test]
    fn pending_ask_message_is_consumed_once() {
        let state = AskDictationState::default();
        state.set_pending_result(AskDictationResult {
            question: "What is OpenTypeless?".to_string(),
            answer: "A voice app.".to_string(),
        });

        match state.take_pending_message().unwrap() {
            PendingAskMessage::Result(result) => {
                assert_eq!(result.answer, "A voice app.");
            }
            PendingAskMessage::Error(_) => panic!("expected result"),
        }
        assert!(state.take_pending_message().is_none());

        state.set_pending_error("No speech detected. Please try again.".to_string());
        match state.take_pending_message().unwrap() {
            PendingAskMessage::Error(message) => {
                assert_eq!(message, "No speech detected. Please try again.");
            }
            PendingAskMessage::Result(_) => panic!("expected error"),
        }
    }

    #[test]
    fn async_stt_errors_surface_only_for_active_recording_session() {
        assert!(should_surface_async_recording_error(
            Some("operation-1"),
            false,
            "operation-1"
        ));
        assert!(!should_surface_async_recording_error(
            Some("operation-1"),
            true,
            "operation-1"
        ));
        assert!(!should_surface_async_recording_error(
            Some("operation-1"),
            false,
            "operation-2"
        ));
        assert!(!should_surface_async_recording_error(
            None,
            false,
            "operation-1"
        ));
    }

    #[test]
    fn ask_starting_state_blocks_duplicate_starts() {
        let state = AskDictationState::default();

        assert!(state.try_begin_starting());
        assert!(state.is_starting());
        assert!(state.is_busy());
        assert!(!state.is_recording());
        assert!(!state.try_begin_starting());

        state.clear_starting();

        assert!(!state.is_starting());
        assert!(!state.is_busy());
    }

    #[test]
    fn ask_starting_state_tracks_stop_after_start() {
        let state = AskDictationState::default();

        assert!(!state.request_stop_after_start());

        assert!(state.try_begin_starting());
        assert!(state.request_stop_after_start());
        assert!(state.take_stop_after_start());
        assert!(!state.take_stop_after_start());

        assert!(state.request_stop_after_start());
        state.clear_starting();
        assert!(!state.request_stop_after_start());
        assert!(!state.take_stop_after_start());
    }

    #[test]
    fn aborting_ask_does_not_clear_processing_stage() {
        let state = AskDictationState::default();
        state.set_processing(true);

        let (_session, was_starting) = state.abort_starting_or_recording();

        assert!(!was_starting);
        assert!(state.is_busy());

        state.set_processing(false);
        assert!(!state.is_busy());
    }
}
