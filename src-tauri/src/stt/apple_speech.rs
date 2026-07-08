use crate::error::AppError;

use super::{SttConfig, SttProvider, TranscriptEvent};

pub const APPLE_SPEECH_PROVIDER: &str = "apple-speech";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppleSpeechAvailability {
    pub platform_supported: bool,
    pub authorization_status: AppleSpeechAuthorizationStatus,
    pub locale: Option<String>,
    pub recognizer_available: Option<bool>,
    pub ready: bool,
    pub issue_code: Option<String>,
    pub issue_message: Option<String>,
}

impl AppleSpeechAvailability {
    pub fn from_parts(
        platform_supported: bool,
        authorization_status: AppleSpeechAuthorizationStatus,
        locale: Option<String>,
        recognizer_available: Option<bool>,
    ) -> Self {
        let (ready, issue_code, issue_message) = if !platform_supported {
            (
                false,
                Some("unsupported_platform".to_string()),
                Some("Apple Speech is only available on macOS".to_string()),
            )
        } else {
            match authorization_status {
                AppleSpeechAuthorizationStatus::Authorized => match recognizer_available {
                    Some(true) => (true, None, None),
                    Some(false) => (
                        false,
                        Some("speech_language_unavailable".to_string()),
                        Some(
                            "Apple Speech is not currently available for this language".to_string(),
                        ),
                    ),
                    None => (
                        false,
                        Some("speech_recognizer_unavailable".to_string()),
                        Some(
                            "Apple Speech recognizer availability could not be checked".to_string(),
                        ),
                    ),
                },
                AppleSpeechAuthorizationStatus::NotDetermined => (
                    false,
                    Some("speech_permission_not_determined".to_string()),
                    Some("Speech Recognition permission has not been requested".to_string()),
                ),
                AppleSpeechAuthorizationStatus::Denied => (
                    false,
                    Some("speech_permission_denied".to_string()),
                    Some("Speech Recognition permission is denied for OpenTypeless".to_string()),
                ),
                AppleSpeechAuthorizationStatus::Restricted => (
                    false,
                    Some("speech_permission_restricted".to_string()),
                    Some("Speech Recognition is restricted on this Mac".to_string()),
                ),
                AppleSpeechAuthorizationStatus::Unsupported => (
                    false,
                    Some("unsupported_platform".to_string()),
                    Some("Apple Speech is only available on macOS".to_string()),
                ),
                AppleSpeechAuthorizationStatus::Unknown => (
                    false,
                    Some("speech_permission_unknown".to_string()),
                    Some("Speech Recognition permission status is unknown".to_string()),
                ),
            }
        };

        Self {
            platform_supported,
            authorization_status,
            locale,
            recognizer_available,
            ready,
            issue_code,
            issue_message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AppleSpeechAuthorizationStatus {
    Unsupported,
    NotDetermined,
    Denied,
    Restricted,
    Authorized,
    Unknown,
}

impl AppleSpeechAuthorizationStatus {
    pub fn from_macos_raw(status: i64) -> Self {
        match status {
            0 => Self::NotDetermined,
            1 => Self::Denied,
            2 => Self::Restricted,
            3 => Self::Authorized,
            _ => Self::Unknown,
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::sync::mpsc;
    use std::time::Duration;

    use anyhow::{anyhow, bail, Context, Result};
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};

    use super::{
        AppError, AppleSpeechAuthorizationStatus, AppleSpeechAvailability, SttConfig, SttProvider,
        TranscriptEvent,
    };

    const RECOGNITION_WAIT: Duration = Duration::from_secs(60);
    const AUTHORIZATION_WAIT: Duration = Duration::from_secs(30);
    const AVAILABILITY_WAIT: Duration = Duration::from_secs(3);
    const AVAILABILITY_POLL: Duration = Duration::from_millis(100);

    pub struct AppleSpeechProvider {
        config: Option<SttConfig>,
        audio_buffer: Vec<u8>,
    }

    impl Default for AppleSpeechProvider {
        fn default() -> Self {
            Self::new()
        }
    }

    impl AppleSpeechProvider {
        pub fn new() -> Self {
            Self {
                config: None,
                audio_buffer: Vec::new(),
            }
        }
    }

    #[async_trait::async_trait]
    impl SttProvider for AppleSpeechProvider {
        async fn connect(&mut self, config: &SttConfig) -> Result<(), AppError> {
            self.config = Some(config.clone());
            self.audio_buffer.clear();
            Ok(())
        }

        async fn send_audio(&mut self, chunk: &[u8]) -> Result<(), AppError> {
            self.audio_buffer.extend_from_slice(chunk);
            Ok(())
        }

        async fn recv_transcript(&mut self) -> Result<Option<TranscriptEvent>, AppError> {
            std::future::pending().await
        }

        async fn disconnect(&mut self) -> Result<Option<String>, AppError> {
            let config = match self.config.clone() {
                Some(config) => config,
                None => return Ok(None),
            };

            if self.audio_buffer.is_empty() {
                return Ok(None);
            }

            let pcm = std::mem::take(&mut self.audio_buffer);
            let sample_rate = config.sample_rate;
            let locale = apple_locale_for_language(config.language.as_deref());
            let result = tauri::async_runtime::spawn_blocking(move || {
                transcribe_pcm_blocking(&pcm, sample_rate, locale.as_deref())
            })
            .await
            .map_err(|e| AppError::Config(format!("Apple Speech task failed: {e}")))?;

            result.map_err(|e| AppError::Config(e.to_string()))
        }

        fn name(&self) -> &str {
            "Apple Speech"
        }
    }

    pub fn is_available_on_current_platform() -> bool {
        true
    }

    pub fn apple_speech_availability(language: Option<&str>) -> AppleSpeechAvailability {
        let locale = apple_locale_for_language(language);
        let authorization_status =
            current_authorization_status().unwrap_or(AppleSpeechAuthorizationStatus::Unknown);
        let recognizer_available =
            if authorization_status == AppleSpeechAuthorizationStatus::Authorized {
                recognizer_available(locale.as_deref()).ok()
            } else {
                None
            };

        AppleSpeechAvailability::from_parts(
            true,
            authorization_status,
            locale,
            recognizer_available,
        )
    }

    pub fn request_apple_speech_authorization() -> Result<AppleSpeechAuthorizationStatus, AppError>
    {
        request_authorization_blocking().map_err(|e| AppError::Config(e.to_string()))
    }

    fn transcribe_pcm_blocking(
        pcm: &[u8],
        sample_rate: u32,
        locale: Option<&str>,
    ) -> Result<Option<String>> {
        ensure_authorized()?;

        let wav = crate::stt::whisper_compat::WhisperCompatProvider::build_wav(pcm, sample_rate);
        let path = std::env::temp_dir().join(format!(
            "opentypeless-apple-speech-{}-{}.wav",
            std::process::id(),
            unique_suffix()
        ));
        std::fs::write(&path, &wav)
            .with_context(|| format!("write temporary WAV {}", path.display()))?;
        let _cleanup = TempFileGuard(&path);
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow!("temporary WAV path is not valid UTF-8: {}", path.display()))?;
        let text = recognize_file(path_str, locale)?;
        let text = text.trim().to_string();
        Ok((!text.is_empty()).then_some(text))
    }

    fn current_authorization_status() -> Result<AppleSpeechAuthorizationStatus> {
        let cls = speech_recognizer_class()?;
        let status: i64 = unsafe { msg_send![cls, authorizationStatus] };
        Ok(AppleSpeechAuthorizationStatus::from_macos_raw(status))
    }

    fn request_authorization_blocking() -> Result<AppleSpeechAuthorizationStatus> {
        let cls = speech_recognizer_class()?;
        let status = current_authorization_status()?;
        match status {
            AppleSpeechAuthorizationStatus::Authorized => return Ok(status),
            AppleSpeechAuthorizationStatus::Denied => {
                bail!("Speech Recognition permission is denied for OpenTypeless")
            }
            AppleSpeechAuthorizationStatus::Restricted => {
                bail!("Speech Recognition is restricted on this Mac")
            }
            AppleSpeechAuthorizationStatus::NotDetermined => {}
            AppleSpeechAuthorizationStatus::Unknown => {
                bail!("Unknown Speech Recognition authorization status")
            }
            AppleSpeechAuthorizationStatus::Unsupported => {
                bail!("Apple Speech is only available on macOS")
            }
        }

        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |granted_status: i64| {
            let _ = tx.send(AppleSpeechAuthorizationStatus::from_macos_raw(
                granted_status,
            ));
        });
        let _: () = unsafe { msg_send![cls, requestAuthorization: &*block] };

        match rx.recv_timeout(AUTHORIZATION_WAIT) {
            Ok(AppleSpeechAuthorizationStatus::Authorized) => {
                Ok(AppleSpeechAuthorizationStatus::Authorized)
            }
            Ok(AppleSpeechAuthorizationStatus::Denied) => {
                bail!("Speech Recognition permission is denied for OpenTypeless")
            }
            Ok(AppleSpeechAuthorizationStatus::Restricted) => {
                bail!("Speech Recognition is restricted on this Mac")
            }
            Ok(other) => bail!("Speech Recognition was not authorized, status: {other:?}"),
            Err(error) => bail!("Timed out waiting for Speech Recognition authorization: {error}"),
        }
    }

    fn ensure_authorized() -> Result<()> {
        request_authorization_blocking().map(|_| ())
    }

    fn recognize_file(wav_path: &str, locale: Option<&str>) -> Result<String> {
        let recognizer = create_recognizer(locale)?;
        wait_until_available(recognizer)?;
        let url = file_url(wav_path)?;
        let request = create_url_request(url)?;
        configure_on_device(recognizer, request);

        let (tx, rx) = mpsc::channel::<RecognitionOutcome>();
        let block = RcBlock::new(move |result: *mut AnyObject, error: *mut AnyObject| {
            let outcome = build_outcome(result, error);
            if outcome.is_terminal() {
                let _ = tx.send(outcome);
            }
        });

        let _task: *mut AnyObject = unsafe {
            msg_send![
                recognizer,
                recognitionTaskWithRequest: request,
                resultHandler: &*block
            ]
        };

        match rx.recv_timeout(RECOGNITION_WAIT) {
            Ok(RecognitionOutcome::Final(text)) => Ok(text),
            Ok(RecognitionOutcome::Failed(message)) => bail!("Apple Speech failed: {message}"),
            Ok(RecognitionOutcome::Pending) => unreachable!("pending outcomes are not sent"),
            Err(error) => bail!("Timed out waiting for Apple Speech result: {error}"),
        }
    }

    fn wait_until_available(recognizer: *mut AnyObject) -> Result<()> {
        let deadline = std::time::Instant::now() + AVAILABILITY_WAIT;
        loop {
            let available: Bool = unsafe { msg_send![recognizer, isAvailable] };
            if available.as_bool() {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                bail!("Apple Speech is not currently available for this language");
            }
            std::thread::sleep(AVAILABILITY_POLL);
        }
    }

    fn recognizer_available(locale: Option<&str>) -> Result<bool> {
        let recognizer = create_recognizer(locale)?;
        let available: Bool = unsafe { msg_send![recognizer, isAvailable] };
        Ok(available.as_bool())
    }

    fn configure_on_device(recognizer: *mut AnyObject, request: *mut AnyObject) {
        let supports: Bool = unsafe { msg_send![recognizer, supportsOnDeviceRecognition] };
        if supports.as_bool() {
            let _: () =
                unsafe { msg_send![request, setRequiresOnDeviceRecognition: Bool::new(true)] };
        }
    }

    enum RecognitionOutcome {
        Pending,
        Final(String),
        Failed(String),
    }

    impl RecognitionOutcome {
        fn is_terminal(&self) -> bool {
            !matches!(self, RecognitionOutcome::Pending)
        }
    }

    fn build_outcome(result: *mut AnyObject, error: *mut AnyObject) -> RecognitionOutcome {
        if !error.is_null() {
            return RecognitionOutcome::Failed(ns_error_description(error));
        }
        if result.is_null() {
            return RecognitionOutcome::Failed("empty recognition result".to_string());
        }
        let is_final: Bool = unsafe { msg_send![result, isFinal] };
        if !is_final.as_bool() {
            return RecognitionOutcome::Pending;
        }
        let transcription: *mut AnyObject = unsafe { msg_send![result, bestTranscription] };
        if transcription.is_null() {
            return RecognitionOutcome::Final(String::new());
        }
        let formatted: *mut AnyObject = unsafe { msg_send![transcription, formattedString] };
        RecognitionOutcome::Final(ns_string_to_rust(formatted))
    }

    fn speech_recognizer_class() -> Result<&'static AnyClass> {
        AnyClass::get("SFSpeechRecognizer")
            .ok_or_else(|| anyhow!("SFSpeechRecognizer requires macOS Speech.framework"))
    }

    fn create_recognizer(locale: Option<&str>) -> Result<*mut AnyObject> {
        let cls = speech_recognizer_class()?;
        let recognizer: *mut AnyObject = match locale.and_then(ns_locale) {
            Some(ns_locale) => unsafe {
                let alloc: *mut AnyObject = msg_send![cls, alloc];
                msg_send![alloc, initWithLocale: ns_locale]
            },
            None => unsafe {
                let alloc: *mut AnyObject = msg_send![cls, alloc];
                msg_send![alloc, init]
            },
        };
        if recognizer.is_null() {
            bail!("Apple Speech recognizer could not be created for this language");
        }
        Ok(recognizer)
    }

    fn ns_locale(identifier: &str) -> Option<*mut AnyObject> {
        let ns_id = ns_string_from_str(identifier).ok()?;
        let cls = AnyClass::get("NSLocale")?;
        let locale: *mut AnyObject = unsafe { msg_send![cls, localeWithLocaleIdentifier: ns_id] };
        (!locale.is_null()).then_some(locale)
    }

    pub fn apple_locale_for_language(language: Option<&str>) -> Option<String> {
        let locale = match language.unwrap_or_default().trim() {
            "zh" | "zh-CN" | "cmn-Hans-CN" => "zh-CN",
            "zh-TW" | "cmn-Hant-TW" => "zh-TW",
            "en" | "en-US" => "en-US",
            "ja" | "ja-JP" => "ja-JP",
            "ko" | "ko-KR" => "ko-KR",
            "fr" | "fr-FR" => "fr-FR",
            "de" | "de-DE" => "de-DE",
            "es" | "es-ES" => "es-ES",
            "it" | "it-IT" => "it-IT",
            "pt" | "pt-BR" => "pt-BR",
            "ru" | "ru-RU" => "ru-RU",
            "ar" | "ar-SA" => "ar-SA",
            "vi" | "vi-VN" => "vi-VN",
            "th" | "th-TH" => "th-TH",
            "hi" | "hi-IN" => "hi-IN",
            _ => return None,
        };
        Some(locale.to_string())
    }

    fn file_url(path: &str) -> Result<*mut AnyObject> {
        let ns_path = ns_string_from_str(path)?;
        let cls = AnyClass::get("NSURL").ok_or_else(|| anyhow!("NSURL class is unavailable"))?;
        let url: *mut AnyObject = unsafe { msg_send![cls, fileURLWithPath: ns_path] };
        if url.is_null() {
            bail!("could not create file URL for {path}");
        }
        Ok(url)
    }

    fn create_url_request(url: *mut AnyObject) -> Result<*mut AnyObject> {
        let cls = AnyClass::get("SFSpeechURLRecognitionRequest")
            .ok_or_else(|| anyhow!("SFSpeechURLRecognitionRequest class is unavailable"))?;
        let request: *mut AnyObject = unsafe {
            let alloc: *mut AnyObject = msg_send![cls, alloc];
            msg_send![alloc, initWithURL: url]
        };
        if request.is_null() {
            bail!("could not create SFSpeechURLRecognitionRequest");
        }
        Ok(request)
    }

    fn ns_string_from_str(value: &str) -> Result<*mut AnyObject> {
        let c = std::ffi::CString::new(value).context("string contains NUL")?;
        let cls =
            AnyClass::get("NSString").ok_or_else(|| anyhow!("NSString class is unavailable"))?;
        let ns: *mut AnyObject = unsafe { msg_send![cls, stringWithUTF8String: c.as_ptr()] };
        if ns.is_null() {
            bail!("could not create NSString");
        }
        Ok(ns)
    }

    fn ns_string_to_rust(ns: *mut AnyObject) -> String {
        if ns.is_null() {
            return String::new();
        }
        let ptr: *const std::os::raw::c_char = unsafe { msg_send![ns, UTF8String] };
        if ptr.is_null() {
            return String::new();
        }
        unsafe { std::ffi::CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }

    fn ns_error_description(error: *mut AnyObject) -> String {
        if error.is_null() {
            return "unknown error".to_string();
        }
        let desc: *mut AnyObject = unsafe { msg_send![error, localizedDescription] };
        let message = ns_string_to_rust(desc);
        if message.is_empty() {
            "unknown error".to_string()
        } else {
            message
        }
    }

    fn unique_suffix() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    struct TempFileGuard<'a>(&'a std::path::Path);

    impl Drop for TempFileGuard<'_> {
        fn drop(&mut self) {
            if let Err(error) = std::fs::remove_file(self.0) {
                tracing::warn!(
                    "Failed to remove Apple Speech temporary WAV {}: {error}",
                    self.0.display()
                );
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn empty_buffer_disconnect_returns_no_transcript_without_authorization() {
            let mut provider = AppleSpeechProvider::new();
            provider.connect(&SttConfig::default()).await.unwrap();

            assert_eq!(provider.disconnect().await.unwrap(), None);
        }

        #[test]
        fn maps_stt_language_to_apple_locale() {
            assert_eq!(
                apple_locale_for_language(Some("zh")).as_deref(),
                Some("zh-CN")
            );
            assert_eq!(
                apple_locale_for_language(Some("en")).as_deref(),
                Some("en-US")
            );
            assert_eq!(apple_locale_for_language(Some("multi")), None);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::{
        AppError, AppleSpeechAuthorizationStatus, AppleSpeechAvailability, SttConfig, SttProvider,
        TranscriptEvent,
    };

    pub struct AppleSpeechProvider;

    impl AppleSpeechProvider {
        pub fn new() -> Self {
            Self
        }
    }

    #[async_trait::async_trait]
    impl SttProvider for AppleSpeechProvider {
        async fn connect(&mut self, _config: &SttConfig) -> Result<(), AppError> {
            Err(AppError::Config(
                "Apple Speech is only available on macOS".to_string(),
            ))
        }

        async fn send_audio(&mut self, _chunk: &[u8]) -> Result<(), AppError> {
            Ok(())
        }

        async fn recv_transcript(&mut self) -> Result<Option<TranscriptEvent>, AppError> {
            Ok(None)
        }

        async fn disconnect(&mut self) -> Result<Option<String>, AppError> {
            Ok(None)
        }

        fn name(&self) -> &str {
            "Apple Speech"
        }
    }

    pub fn is_available_on_current_platform() -> bool {
        false
    }

    pub fn apple_speech_availability(_language: Option<&str>) -> AppleSpeechAvailability {
        AppleSpeechAvailability::from_parts(
            false,
            AppleSpeechAuthorizationStatus::Unsupported,
            None,
            None,
        )
    }

    pub fn request_apple_speech_authorization() -> Result<AppleSpeechAuthorizationStatus, AppError>
    {
        Ok(AppleSpeechAuthorizationStatus::Unsupported)
    }
}

pub use platform::{
    apple_speech_availability, is_available_on_current_platform,
    request_apple_speech_authorization, AppleSpeechProvider,
};

#[cfg(test)]
mod availability_tests {
    use super::*;

    #[test]
    fn apple_speech_authorization_status_maps_macos_raw_values() {
        assert_eq!(
            AppleSpeechAuthorizationStatus::from_macos_raw(0),
            AppleSpeechAuthorizationStatus::NotDetermined
        );
        assert_eq!(
            AppleSpeechAuthorizationStatus::from_macos_raw(1),
            AppleSpeechAuthorizationStatus::Denied
        );
        assert_eq!(
            AppleSpeechAuthorizationStatus::from_macos_raw(2),
            AppleSpeechAuthorizationStatus::Restricted
        );
        assert_eq!(
            AppleSpeechAuthorizationStatus::from_macos_raw(3),
            AppleSpeechAuthorizationStatus::Authorized
        );
        assert_eq!(
            AppleSpeechAuthorizationStatus::from_macos_raw(99),
            AppleSpeechAuthorizationStatus::Unknown
        );
    }

    #[test]
    fn apple_speech_availability_requires_authorized_and_available_recognizer() {
        let denied = AppleSpeechAvailability::from_parts(
            true,
            AppleSpeechAuthorizationStatus::Denied,
            Some("en-US".to_string()),
            None,
        );
        assert!(!denied.ready);
        assert_eq!(
            denied.issue_code.as_deref(),
            Some("speech_permission_denied")
        );

        let unavailable_language = AppleSpeechAvailability::from_parts(
            true,
            AppleSpeechAuthorizationStatus::Authorized,
            Some("zz-ZZ".to_string()),
            Some(false),
        );
        assert!(!unavailable_language.ready);
        assert_eq!(
            unavailable_language.issue_code.as_deref(),
            Some("speech_language_unavailable")
        );

        let ready = AppleSpeechAvailability::from_parts(
            true,
            AppleSpeechAuthorizationStatus::Authorized,
            Some("en-US".to_string()),
            Some(true),
        );
        assert!(ready.ready);
        assert_eq!(ready.issue_code, None);
    }
}
