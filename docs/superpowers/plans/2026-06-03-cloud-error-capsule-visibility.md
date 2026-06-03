# Cloud Error and Capsule Visibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix Cloud STT error reporting and make the desktop capsule hide while idle without losing settings sync across webviews.

**Architecture:** Add structured Rust errors for quota/no-speech, latch STT failures by session, and route all frontend display through i18n keys. Add a dedicated persisted config patch path for capsule visibility and selected cross-webview fields so capsule, main window, and tray stay in sync without stale full-config writes.

**Tech Stack:** Rust/Tauri 2, React 19, Zustand, i18next, Vitest, Cargo tests.

---

## File Structure

- Modify `src-tauri/src/error.rs`: add `AppError::Quota`, keep user-error mapping centralized.
- Modify `src-tauri/src/stt/cloud.rs`: classify Cloud STT 403 responses with a testable helper.
- Modify `src-tauri/src/pipeline.rs`: add session-aware STT error latch and structured no-speech error emission.
- Modify `src-tauri/src/storage/mod.rs`: support new-install vs legacy config defaults for `capsule_auto_hide`.
- Modify `src-tauri/src/commands/config.rs`: add partial capsule visibility command and full-save patch emission.
- Modify `src-tauri/src/lib.rs`: register the new Tauri command and tray event.
- Modify `src-tauri/src/tray.rs`: read `app_config.ui_language`, add capsule visibility item labels, and call shared save helper.
- Modify `src/lib/tauri.ts`: expose `setCapsuleAutoHide`.
- Modify `src/stores/appStore.ts`: add `applyPersistedConfigPatch`.
- Modify `src/hooks/useTauriEvents.ts`: listen for `config:patch`, apply i18n/localStorage updates, and preserve dirty settings.
- Modify `src/hooks/useCapsuleResize.ts`: replace transition-only auto-hide logic with computed visibility reconciliation.
- Create `src/hooks/__tests__/useCapsuleResize.test.ts`: test pure visibility decisions.
- Modify `src/components/Capsule/CapsuleContextMenu.tsx`: add hide/show item using the partial command.
- Modify `src/components/Capsule/__tests__/CapsuleContextMenu.test.tsx`: cover labels and command call.
- Modify `src/i18n/locales/*.json`: add `errors` keys and menu/onboarding copy.
- Create `src/i18n/__tests__/errors.test.ts`: enforce locale error coverage.
- Modify `src/components/Onboarding/DoneStep.tsx`: update copy for hidden-by-default capsule.
- Modify existing tests in `src/stores/__tests__/appStore.test.ts` and `src/components/Settings/__tests__/Settings.test.tsx`: cover patch behavior and settings-save sync.

---

### Task 1: Rust Cloud STT Error Classification

**Files:**
- Modify: `src-tauri/src/error.rs`
- Modify: `src-tauri/src/stt/cloud.rs`
- Test: `src-tauri/src/error.rs`
- Test: `src-tauri/src/stt/cloud.rs`

- [ ] **Step 1: Write failing tests for quota mapping**

Add these tests to the existing `#[cfg(test)] mod tests` in `src-tauri/src/error.rs`:

```rust
#[test]
fn test_quota_is_not_retryable() {
    let err = AppError::Quota("quota exceeded".to_string());
    assert!(!err.is_retryable());
}

#[test]
fn test_quota_maps_to_quota_code() {
    let err = AppError::Quota("quota exceeded".to_string());
    let ue = err.to_user_error();
    assert_eq!(ue.code, "stt_quota_exceeded");
    assert_eq!(ue.details.as_deref(), Some("quota exceeded"));
}
```

- [ ] **Step 2: Run Rust error tests and verify RED**

Run:

```powershell
cd src-tauri
cargo test error::tests::test_quota --lib
```

Expected: FAIL because `AppError::Quota` does not exist.

- [ ] **Step 3: Implement `AppError::Quota`**

Update `src-tauri/src/error.rs`:

```rust
pub enum AppError {
    Network(String),
    Timeout(Duration),
    Api { status: u16, body: String },
    Auth(String),
    Quota(String),
    Output(String),
    Config(String),
}
```

Add the new match arms:

```rust
AppError::Quota(_) => false,
```

```rust
AppError::Quota(msg) => ("stt_quota_exceeded".to_string(), Some(msg.clone())),
```

```rust
AppError::Quota(msg) => write!(f, "Quota error: {}", msg),
```

- [ ] **Step 4: Verify quota tests pass**

Run:

```powershell
cd src-tauri
cargo test error::tests::test_quota --lib
```

Expected: PASS.

- [ ] **Step 5: Write failing tests for Cloud 403 classification**

Add helper tests to `src-tauri/src/stt/cloud.rs`:

```rust
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
fn forbidden_error_empty_body_is_auth_not_quota() {
    let err = cloud_stt_forbidden_error("");
    assert!(matches!(err, AppError::Auth(_)));
}

#[test]
fn forbidden_error_unknown_json_is_auth_not_quota() {
    let err = cloud_stt_forbidden_error(r#"{"error":"Forbidden"}"#);
    assert!(matches!(err, AppError::Auth(_)));
}
```

- [ ] **Step 6: Run Cloud classification tests and verify RED**

Run:

```powershell
cd src-tauri
cargo test stt::cloud::tests::forbidden_error --lib
```

Expected: FAIL because `cloud_stt_forbidden_error` does not exist.

- [ ] **Step 7: Implement classification helper and use it**

Add near `MAX_AUDIO_BYTES` in `src-tauri/src/stt/cloud.rs`:

```rust
fn contains_quota_marker(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("quota")
        || value.contains("limit exceeded")
        || value.contains("usage exceeded")
        || value.contains("byok")
}

fn cloud_stt_forbidden_error(body: &str) -> AppError {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok();

    if let Some(value) = parsed.as_ref() {
        for field in ["code", "error_code", "type"] {
            if value
                .get(field)
                .and_then(|v| v.as_str())
                .is_some_and(contains_quota_marker)
            {
                let details = value
                    .get("error")
                    .or_else(|| value.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Cloud STT quota exceeded")
                    .to_string();
                return AppError::Quota(details);
            }
        }

        for field in ["error", "message"] {
            if let Some(message) = value.get(field).and_then(|v| v.as_str()) {
                if contains_quota_marker(message) {
                    return AppError::Quota(message.to_string());
                }
            }
        }
    }

    AppError::Auth("Cloud STT access denied".to_string())
}
```

Replace the 403 branch:

```rust
} else if status.as_u16() == 403 {
    return Err(cloud_stt_forbidden_error(&body));
}
```

- [ ] **Step 8: Verify classification tests pass**

Run:

```powershell
cd src-tauri
cargo test stt::cloud::tests::forbidden_error --lib
```

Expected: PASS.

- [ ] **Step 9: Commit Task 1**

```powershell
git add src-tauri/src/error.rs src-tauri/src/stt/cloud.rs
git commit -m "fix: classify cloud stt quota errors"
```

---

### Task 2: Pipeline STT Error Latching and Structured No-Speech

**Files:**
- Modify: `src-tauri/src/pipeline.rs`
- Test: `src-tauri/src/pipeline.rs`

- [ ] **Step 1: Write failing helper tests**

Add these pure helper tests in `src-tauri/src/pipeline.rs` under existing tests:

```rust
#[test]
fn session_error_latch_returns_matching_error() {
    let latch = Mutex::new(Some((
        7,
        UserError {
            code: "stt_quota_exceeded".to_string(),
            details: Some("quota".to_string()),
            retry_count: 0,
        },
    )));

    let err = take_matching_stt_error(&latch, 7).unwrap();
    assert_eq!(err.code, "stt_quota_exceeded");
    assert!(latch.lock().unwrap().is_none());
}

#[test]
fn session_error_latch_ignores_stale_error() {
    let latch = Mutex::new(Some((
        6,
        UserError {
            code: "stt_quota_exceeded".to_string(),
            details: Some("quota".to_string()),
            retry_count: 0,
        },
    )));

    assert!(take_matching_stt_error(&latch, 7).is_none());
    assert!(latch.lock().unwrap().is_some());
}

#[test]
fn no_speech_user_error_has_localizable_code() {
    let err = no_speech_user_error();
    assert_eq!(err.code, "stt_no_speech_detected");
    assert_eq!(err.retry_count, 0);
}
```

Also add:

```rust
use crate::error::UserError;
```

to the test module if needed.

- [ ] **Step 2: Run pipeline helper tests and verify RED**

Run:

```powershell
cd src-tauri
cargo test pipeline::tests::session_error_latch pipeline::tests::no_speech_user_error --lib
```

Expected: FAIL because helpers do not exist.

- [ ] **Step 3: Add latch field and helper functions**

In `PipelineHandle`, add:

```rust
stt_error: Arc<Mutex<Option<(u64, crate::error::UserError)>>>,
```

Initialize in `new()`:

```rust
stt_error: Arc::new(Mutex::new(None)),
```

Add pure helpers near `should_finalize_stt_task`:

```rust
fn no_speech_user_error() -> crate::error::UserError {
    crate::error::UserError {
        code: "stt_no_speech_detected".to_string(),
        details: None,
        retry_count: 0,
    }
}

fn take_matching_stt_error(
    stt_error: &Mutex<Option<(u64, crate::error::UserError)>>,
    session_id: u64,
) -> Option<crate::error::UserError> {
    let mut guard = stt_error.lock().unwrap_or_else(|e| e.into_inner());
    if guard
        .as_ref()
        .is_some_and(|(latched_session_id, _)| *latched_session_id == session_id)
    {
        return guard.take().map(|(_, error)| error);
    }
    None
}
```

- [ ] **Step 4: Latch provider errors only for active sessions**

Before each STT task spawn, clone:

```rust
let stt_error_ref = self.stt_error.clone();
```

When starting a new recording after the idle-to-recording CAS succeeds, clear the latch:

```rust
*self.stt_error.lock().unwrap_or_else(|e| e.into_inner()) = None;
```

In `Some(Err(e))` disconnect branch, inside `should_finalize_stt_task(...)`:

```rust
let user_error = e.to_user_error();
*stt_error_ref.lock().unwrap_or_else(|e| e.into_inner()) =
    Some((stt_control.id, user_error.clone()));
let _ = app_handle.emit("pipeline:error", user_error);
```

In `TranscriptEvent::Error { message }`, inside `should_finalize_stt_task(...)`:

```rust
let user_error = crate::error::AppError::Config(message.clone()).to_user_error();
*stt_error_ref.lock().unwrap_or_else(|e| e.into_inner()) =
    Some((stt_control.id, user_error.clone()));
let _ = app_handle.emit("pipeline:error", user_error);
```

In `Err(e)` from `provider.recv_transcript()`, add an active-session branch instead of only logging and breaking:

```rust
if should_finalize_stt_task(
    abort_flag_ref.as_ref(),
    active_session_id_ref.as_ref(),
    stt_control.id,
) {
    let user_error = e.to_user_error();
    *stt_error_ref.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((stt_control.id, user_error.clone()));
    let _ = app_handle.emit("pipeline:error", user_error);
}
```

- [ ] **Step 5: Stop no-speech from overwriting latched errors**

At the start of `wait_for_stt` after waiting for `stt_control.done.notified()`, add:

```rust
if let Some(stt_control) = stt_control.as_ref() {
    if take_matching_stt_error(&self.stt_error, stt_control.id).is_some() {
        self.set_state(PipelineState::Idle);
        return Ok(None);
    }
}
```

Replace the plain no-speech string emission:

```rust
let _ = self.app_handle.emit("pipeline:error", no_speech_user_error());
```

- [ ] **Step 6: Verify pipeline tests pass**

Run:

```powershell
cd src-tauri
cargo test pipeline::tests::session_error_latch pipeline::tests::no_speech_user_error --lib
```

Expected: PASS.

- [ ] **Step 7: Run broader Rust tests for touched modules**

Run:

```powershell
cd src-tauri
cargo test pipeline::tests stt::cloud::tests error::tests --lib
```

Expected: PASS.

- [ ] **Step 8: Commit Task 2**

```powershell
git add src-tauri/src/pipeline.rs
git commit -m "fix: preserve stt provider errors"
```

---

### Task 3: Locale Error Coverage

**Files:**
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/zh.json`
- Modify: `src/i18n/locales/ja.json`
- Modify: `src/i18n/locales/ko.json`
- Modify: `src/i18n/locales/fr.json`
- Modify: `src/i18n/locales/de.json`
- Modify: `src/i18n/locales/es.json`
- Modify: `src/i18n/locales/pt.json`
- Modify: `src/i18n/locales/ru.json`
- Modify: `src/i18n/locales/it.json`
- Create: `src/i18n/__tests__/errors.test.ts`

- [ ] **Step 1: Write failing locale coverage test**

Create `src/i18n/__tests__/errors.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import en from '../locales/en.json'
import zh from '../locales/zh.json'
import ja from '../locales/ja.json'
import ko from '../locales/ko.json'
import fr from '../locales/fr.json'
import de from '../locales/de.json'
import es from '../locales/es.json'
import pt from '../locales/pt.json'
import ru from '../locales/ru.json'
import it from '../locales/it.json'

const locales = { en, zh, ja, ko, fr, de, es, pt, ru, it }

const requiredErrorKeys = [
  'stt_timeout',
  'stt_invalid_key',
  'stt_failed',
  'stt_quota_exceeded',
  'stt_no_speech_detected',
  'output_fallback_clipboard',
] as const

describe('localized error messages', () => {
  it('defines all structured error keys for every locale', () => {
    for (const [locale, messages] of Object.entries(locales)) {
      expect(messages).toHaveProperty('errors')
      for (const key of requiredErrorKeys) {
        const value = messages.errors?.[key as keyof typeof messages.errors]
        expect(value, `${locale}.${key}`).toEqual(expect.any(String))
        expect(value.trim(), `${locale}.${key}`).not.toBe('')
      }
    }
  })
})
```

- [ ] **Step 2: Run locale test and verify RED**

Run:

```powershell
npm test -- src/i18n/__tests__/errors.test.ts
```

Expected: FAIL because locale files do not define `errors`.

- [ ] **Step 3: Add error keys to locale files**

Add this `errors` object near the top-level `capsule` or `settings` object in each locale file.

English:

```json
"errors": {
  "stt_timeout": "Speech recognition timed out. Please try again.",
  "stt_invalid_key": "Cloud speech access failed. Please sign in again or check your API key.",
  "stt_failed": "Speech recognition failed. {{details}}",
  "stt_quota_exceeded": "Cloud speech quota exceeded. Upgrade to Pro or switch to BYOK mode.",
  "stt_no_speech_detected": "No speech detected. Please try again.",
  "output_fallback_clipboard": "Text output failed. The result was copied to clipboard instead. {{details}}"
}
```

Chinese:

```json
"errors": {
  "stt_timeout": "语音识别超时，请重试。",
  "stt_invalid_key": "云端语音访问失败，请重新登录或检查 API Key。",
  "stt_failed": "语音识别失败。{{details}}",
  "stt_quota_exceeded": "云端语音识别额度已用完。请升级到 Pro 或切换到 BYOK 模式。",
  "stt_no_speech_detected": "未检测到语音，请重试。",
  "output_fallback_clipboard": "文本输出失败，结果已改为复制到剪贴板。{{details}}"
}
```

Japanese:

```json
"errors": {
  "stt_timeout": "音声認識がタイムアウトしました。もう一度お試しください。",
  "stt_invalid_key": "クラウド音声アクセスに失敗しました。再度サインインするか、APIキーを確認してください。",
  "stt_failed": "音声認識に失敗しました。{{details}}",
  "stt_quota_exceeded": "クラウド音声認識の利用枠を超えました。Proにアップグレードするか、BYOKモードに切り替えてください。",
  "stt_no_speech_detected": "音声が検出されませんでした。もう一度お試しください。",
  "output_fallback_clipboard": "テキスト出力に失敗しました。代わりに結果をクリップボードにコピーしました。{{details}}"
}
```

Korean:

```json
"errors": {
  "stt_timeout": "음성 인식 시간이 초과되었습니다. 다시 시도해 주세요.",
  "stt_invalid_key": "클라우드 음성 접근에 실패했습니다. 다시 로그인하거나 API 키를 확인해 주세요.",
  "stt_failed": "음성 인식에 실패했습니다. {{details}}",
  "stt_quota_exceeded": "클라우드 음성 인식 할당량을 모두 사용했습니다. Pro로 업그레이드하거나 BYOK 모드로 전환해 주세요.",
  "stt_no_speech_detected": "음성이 감지되지 않았습니다. 다시 시도해 주세요.",
  "output_fallback_clipboard": "텍스트 출력에 실패했습니다. 대신 결과를 클립보드에 복사했습니다. {{details}}"
}
```

French:

```json
"errors": {
  "stt_timeout": "La reconnaissance vocale a expiré. Veuillez réessayer.",
  "stt_invalid_key": "L'accès vocal cloud a échoué. Reconnectez-vous ou vérifiez votre clé API.",
  "stt_failed": "La reconnaissance vocale a échoué. {{details}}",
  "stt_quota_exceeded": "Le quota de reconnaissance vocale cloud est dépassé. Passez à Pro ou utilisez le mode BYOK.",
  "stt_no_speech_detected": "Aucune parole détectée. Veuillez réessayer.",
  "output_fallback_clipboard": "La sortie de texte a échoué. Le résultat a été copié dans le presse-papiers. {{details}}"
}
```

German:

```json
"errors": {
  "stt_timeout": "Die Spracherkennung ist abgelaufen. Bitte versuchen Sie es erneut.",
  "stt_invalid_key": "Der Cloud-Sprachzugriff ist fehlgeschlagen. Melden Sie sich erneut an oder prüfen Sie Ihren API-Schlüssel.",
  "stt_failed": "Die Spracherkennung ist fehlgeschlagen. {{details}}",
  "stt_quota_exceeded": "Das Kontingent für Cloud-Spracherkennung ist aufgebraucht. Wechseln Sie zu Pro oder in den BYOK-Modus.",
  "stt_no_speech_detected": "Keine Sprache erkannt. Bitte versuchen Sie es erneut.",
  "output_fallback_clipboard": "Die Textausgabe ist fehlgeschlagen. Das Ergebnis wurde stattdessen in die Zwischenablage kopiert. {{details}}"
}
```

Spanish:

```json
"errors": {
  "stt_timeout": "El reconocimiento de voz agotó el tiempo de espera. Inténtalo de nuevo.",
  "stt_invalid_key": "El acceso de voz en la nube falló. Inicia sesión de nuevo o revisa tu clave API.",
  "stt_failed": "El reconocimiento de voz falló. {{details}}",
  "stt_quota_exceeded": "Se agotó la cuota de reconocimiento de voz en la nube. Actualiza a Pro o cambia al modo BYOK.",
  "stt_no_speech_detected": "No se detectó voz. Inténtalo de nuevo.",
  "output_fallback_clipboard": "La salida de texto falló. El resultado se copió al portapapeles. {{details}}"
}
```

Portuguese:

```json
"errors": {
  "stt_timeout": "O reconhecimento de fala expirou. Tente novamente.",
  "stt_invalid_key": "O acesso à fala na nuvem falhou. Entre novamente ou verifique sua chave de API.",
  "stt_failed": "O reconhecimento de fala falhou. {{details}}",
  "stt_quota_exceeded": "A cota de reconhecimento de fala na nuvem acabou. Atualize para Pro ou mude para o modo BYOK.",
  "stt_no_speech_detected": "Nenhuma fala detectada. Tente novamente.",
  "output_fallback_clipboard": "A saída de texto falhou. O resultado foi copiado para a área de transferência. {{details}}"
}
```

Russian:

```json
"errors": {
  "stt_timeout": "Время распознавания речи истекло. Попробуйте еще раз.",
  "stt_invalid_key": "Доступ к облачной речи не удался. Войдите снова или проверьте API-ключ.",
  "stt_failed": "Распознавание речи не удалось. {{details}}",
  "stt_quota_exceeded": "Квота облачного распознавания речи исчерпана. Обновитесь до Pro или переключитесь в режим BYOK.",
  "stt_no_speech_detected": "Речь не обнаружена. Попробуйте еще раз.",
  "output_fallback_clipboard": "Не удалось вывести текст. Результат скопирован в буфер обмена. {{details}}"
}
```

Italian:

```json
"errors": {
  "stt_timeout": "Il riconoscimento vocale è scaduto. Riprova.",
  "stt_invalid_key": "L'accesso vocale cloud non è riuscito. Accedi di nuovo o controlla la tua chiave API.",
  "stt_failed": "Il riconoscimento vocale non è riuscito. {{details}}",
  "stt_quota_exceeded": "La quota di riconoscimento vocale cloud è esaurita. Passa a Pro o usa la modalità BYOK.",
  "stt_no_speech_detected": "Nessun parlato rilevato. Riprova.",
  "output_fallback_clipboard": "L'output del testo non è riuscito. Il risultato è stato copiato negli appunti. {{details}}"
}
```

- [ ] **Step 4: Verify locale test passes**

Run:

```powershell
npm test -- src/i18n/__tests__/errors.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit Task 3**

```powershell
git add src/i18n
git commit -m "fix: localize structured error messages"
```

---

### Task 4: Config Migration, Patch Commands, and Tray Sync

**Files:**
- Modify: `src-tauri/src/storage/mod.rs`
- Modify: `src-tauri/src/commands/config.rs`
- Modify: `src-tauri/src/tray.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/storage/mod.rs`
- Test: `src-tauri/src/tray.rs`
- Test: `src-tauri/src/commands/config.rs`

- [ ] **Step 1: Write failing config default tests**

Add to `src-tauri/src/storage/mod.rs` tests:

```rust
#[test]
fn app_config_new_install_defaults_capsule_auto_hide_true() {
    let config = AppConfig::new_install_default();
    assert!(config.capsule_auto_hide);
}

#[test]
fn app_config_existing_missing_capsule_auto_hide_defaults_false() {
    let value = serde_json::json!({
        "stt_provider": "deepgram",
        "stt_api_key": "hosted-secret"
    });

    let config = AppConfig::from_stored_value(value).unwrap();

    assert_eq!(config.stt_provider, "deepgram");
    assert_eq!(config.stt_api_key, "hosted-secret");
    assert!(!config.capsule_auto_hide);
}

#[test]
fn app_config_existing_explicit_capsule_auto_hide_is_preserved() {
    let value = serde_json::json!({
        "capsule_auto_hide": true
    });

    let config = AppConfig::from_stored_value(value).unwrap();

    assert!(config.capsule_auto_hide);
}
```

- [ ] **Step 2: Run storage tests and verify RED**

Run:

```powershell
cd src-tauri
cargo test storage::tests::app_config_ --lib
```

Expected: FAIL because helper constructors do not exist.

- [ ] **Step 3: Implement storage helpers**

In `impl AppConfig`, add:

```rust
impl AppConfig {
    pub fn new_install_default() -> Self {
        Self {
            capsule_auto_hide: true,
            ..Self::default()
        }
    }

    pub fn from_stored_value(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        let has_capsule_auto_hide = value
            .as_object()
            .is_some_and(|object| object.contains_key("capsule_auto_hide"));
        let mut config: Self = serde_json::from_value(value)?;
        if !has_capsule_auto_hide {
            config.capsule_auto_hide = false;
        }
        Ok(config)
    }
}
```

Update `ConfigManager::load()`:

```rust
Some(val) => AppConfig::from_stored_value(val.clone()).unwrap_or_else(|_| AppConfig::new_install_default()),
None => AppConfig::new_install_default(),
```

- [ ] **Step 4: Verify storage tests pass**

Run:

```powershell
cd src-tauri
cargo test storage::tests::app_config_ --lib
```

Expected: PASS.

- [ ] **Step 5: Add config patch helper and command tests**

In `src-tauri/src/commands/config.rs`, create pure helper tests for patch detection:

```rust
#[test]
fn config_patch_includes_capsule_auto_hide_change() {
    let mut next = storage::AppConfig::default();
    let previous = next.clone();
    next.capsule_auto_hide = !previous.capsule_auto_hide;

    let patch = config_patch_between(&previous, &next);

    assert_eq!(patch["capsule_auto_hide"], next.capsule_auto_hide);
}

#[test]
fn config_patch_includes_ui_language_change() {
    let previous = storage::AppConfig::default();
    let mut next = previous.clone();
    next.ui_language = "zh".to_string();

    let patch = config_patch_between(&previous, &next);

    assert_eq!(patch["ui_language"], "zh");
}

#[test]
fn config_patch_includes_max_recording_seconds_change() {
    let previous = storage::AppConfig::default();
    let mut next = previous.clone();
    next.max_recording_seconds = 45;

    let patch = config_patch_between(&previous, &next);

    assert_eq!(patch["max_recording_seconds"], 45);
}
```

- [ ] **Step 6: Run config helper tests and verify RED**

Run:

```powershell
cd src-tauri
cargo test commands::config::tests::config_patch --lib
```

Expected: FAIL because `config_patch_between` does not exist.

- [ ] **Step 7: Implement patch helper and command**

Add imports:

```rust
use serde_json::{json, Map, Value};
use tauri::Emitter;
```

Add helper:

```rust
fn config_patch_between(previous: &storage::AppConfig, next: &storage::AppConfig) -> Value {
    let mut patch = Map::new();
    if previous.capsule_auto_hide != next.capsule_auto_hide {
        patch.insert("capsule_auto_hide".to_string(), json!(next.capsule_auto_hide));
    }
    if previous.max_recording_seconds != next.max_recording_seconds {
        patch.insert(
            "max_recording_seconds".to_string(),
            json!(next.max_recording_seconds),
        );
    }
    if previous.ui_language != next.ui_language {
        patch.insert("ui_language".to_string(), json!(next.ui_language));
    }
    Value::Object(patch)
}

fn emit_config_patch(app: &tauri::AppHandle, patch: &Value) {
    if patch.as_object().is_some_and(|object| !object.is_empty()) {
        let _ = app.emit("config:patch", patch.clone());
    }
}
```

Change `update_config` signature to include app:

```rust
pub async fn update_config(
    app: tauri::AppHandle,
    state: tauri::State<'_, storage::ConfigManager>,
    cache: tauri::State<'_, HotkeyModeCache>,
    close_tray_cache: tauri::State<'_, CloseToTrayCache>,
    config: storage::AppConfig,
) -> Result<(), String>
```

Before save:

```rust
let previous = state.load().await.map_err(|e| e.to_string())?;
let patch = config_patch_between(&previous, &config);
```

After save:

```rust
emit_config_patch(&app, &patch);
if patch.get("ui_language").is_some() || patch.get("capsule_auto_hide").is_some() {
    crate::refresh_tray(&app);
}
```

Add command:

```rust
#[tauri::command]
pub async fn set_capsule_auto_hide(
    app: tauri::AppHandle,
    state: tauri::State<'_, storage::ConfigManager>,
    enabled: bool,
) -> Result<(), String> {
    let mut config = state.load().await.map_err(|e| e.to_string())?;
    if config.capsule_auto_hide == enabled {
        return Ok(());
    }
    config.capsule_auto_hide = enabled;
    state.save(&config).await.map_err(|e| e.to_string())?;
    let patch = serde_json::json!({ "capsule_auto_hide": enabled });
    emit_config_patch(&app, &patch);
    crate::refresh_tray(&app);
    Ok(())
}
```

- [ ] **Step 8: Register the command**

In `src-tauri/src/lib.rs` add:

```rust
commands::config::set_capsule_auto_hide,
```

to the `tauri::generate_handler!` list.

- [ ] **Step 9: Fix tray language lookup and add visibility labels**

In `src-tauri/src/tray.rs`, extend `TrayLabels`:

```rust
hide_capsule_when_idle: &'static str,
keep_capsule_visible: &'static str,
```

Add these exact fields to every `TrayLabels` match arm:

```rust
// zh
hide_capsule_when_idle: "空闲时隐藏胶囊",
keep_capsule_visible: "保持胶囊可见",

// ja
hide_capsule_when_idle: "待機中はカプセルを非表示",
keep_capsule_visible: "カプセルを表示したままにする",

// ko
hide_capsule_when_idle: "유휴 시 캡슐 숨기기",
keep_capsule_visible: "캡슐 항상 표시",

// fr
hide_capsule_when_idle: "Masquer la capsule au repos",
keep_capsule_visible: "Garder la capsule visible",

// de
hide_capsule_when_idle: "Kapsel im Leerlauf ausblenden",
keep_capsule_visible: "Kapsel sichtbar lassen",

// es
hide_capsule_when_idle: "Ocultar cápsula en reposo",
keep_capsule_visible: "Mantener cápsula visible",

// pt
hide_capsule_when_idle: "Ocultar cápsula em repouso",
keep_capsule_visible: "Manter cápsula visível",

// ru
hide_capsule_when_idle: "Скрывать капсулу в простое",
keep_capsule_visible: "Оставлять капсулу видимой",

// it
hide_capsule_when_idle: "Nascondi capsula quando inattiva",
keep_capsule_visible: "Mantieni capsula visibile",

// default/en
hide_capsule_when_idle: "Hide Capsule When Idle",
keep_capsule_visible: "Keep Capsule Visible",
```

Replace language lookup with:

```rust
let lang = app
    .store("settings.json")
    .ok()
    .and_then(|s| s.get("app_config"))
    .and_then(|v| v.get("ui_language").and_then(|v| v.as_str()).map(String::from))
    .unwrap_or_else(|| "en".to_string());
```

Add a `capsule_auto_hide: bool` parameter to `build_tray_menu` and pass the loaded value from `refresh_tray`. Add item:

```rust
let capsule_visibility = MenuItem::with_id(
    app,
    "toggle_capsule_auto_hide",
    if capsule_auto_hide {
        labels.keep_capsule_visible
    } else {
        labels.hide_capsule_when_idle
    },
    true,
    None::<&str>,
)?;
```

Place it near the recording item:

```rust
&show_hide, &sep1, &record, &capsule_visibility, &sep2, ...
```

In `src-tauri/src/lib.rs` tray menu event handler:

```rust
"toggle_capsule_auto_hide" => {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let config_state = handle.state::<storage::ConfigManager>();
        let enabled = config_state
            .load()
            .await
            .map(|config| !config.capsule_auto_hide)
            .unwrap_or(true);
        if let Err(e) = commands::config::set_capsule_auto_hide(handle.clone(), config_state, enabled).await {
            tracing::error!("Tray capsule visibility toggle failed: {}", e);
        }
    });
}
```

- [ ] **Step 10: Verify Rust config/tray tests**

Run:

```powershell
cd src-tauri
cargo test storage::tests commands::config::tests tray::tests --lib
```

Expected: PASS.

- [ ] **Step 11: Commit Task 4**

```powershell
git add src-tauri/src/storage/mod.rs src-tauri/src/commands/config.rs src-tauri/src/lib.rs src-tauri/src/tray.rs
git commit -m "fix: sync persisted capsule visibility"
```

---

### Task 5: Frontend Config Patch Sync

**Files:**
- Modify: `src/stores/appStore.ts`
- Modify: `src/stores/__tests__/appStore.test.ts`
- Modify: `src/hooks/useTauriEvents.ts`
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: Write failing store patch tests**

Add to `src/stores/__tests__/appStore.test.ts`:

```ts
it('applyPersistedConfigPatch updates config and savedConfig for patched fields', () => {
  const saved = { ...getState().config, capsule_auto_hide: false }
  getState().setSavedConfig(saved)
  getState().applyPersistedConfigPatch({ capsule_auto_hide: true })

  expect(getState().config.capsule_auto_hide).toBe(true)
  expect(getState().savedConfig?.capsule_auto_hide).toBe(true)
})

it('applyPersistedConfigPatch preserves unrelated dirty fields', () => {
  const saved = { ...getState().config, theme: 'system' as const, capsule_auto_hide: false }
  getState().setSavedConfig(saved)
  getState().updateConfig({ theme: 'dark' })

  getState().applyPersistedConfigPatch({ capsule_auto_hide: true })

  expect(getState().config.theme).toBe('dark')
  expect(getState().savedConfig?.theme).toBe('system')
  expect(getState().config.capsule_auto_hide).toBe(true)
  expect(getState().savedConfig?.capsule_auto_hide).toBe(true)
})

it('applyPersistedConfigPatch lets persisted patch win for the same dirty field', () => {
  const saved = { ...getState().config, capsule_auto_hide: false }
  getState().setSavedConfig(saved)
  getState().updateConfig({ capsule_auto_hide: true })

  getState().applyPersistedConfigPatch({ capsule_auto_hide: false })

  expect(getState().config.capsule_auto_hide).toBe(false)
  expect(getState().savedConfig?.capsule_auto_hide).toBe(false)
})
```

- [ ] **Step 2: Run store tests and verify RED**

Run:

```powershell
npm test -- src/stores/__tests__/appStore.test.ts
```

Expected: FAIL because `applyPersistedConfigPatch` does not exist.

- [ ] **Step 3: Implement store action**

Update `AppState` in `src/stores/appStore.ts`:

```ts
applyPersistedConfigPatch: (patch: Partial<AppConfig>) => void
```

Add implementation:

```ts
applyPersistedConfigPatch: (patch) =>
  set((s) => ({
    config: { ...s.config, ...patch },
    savedConfig: s.savedConfig ? { ...s.savedConfig, ...patch } : s.savedConfig,
  })),
```

- [ ] **Step 4: Expose Tauri command**

In `src/lib/tauri.ts` add:

```ts
export async function setCapsuleAutoHide(enabled: boolean): Promise<void> {
  return invoke('set_capsule_auto_hide', { enabled })
}
```

- [ ] **Step 5: Listen for `config:patch` and update i18n**

In `src/hooks/useTauriEvents.ts`, import:

```ts
import i18n from '../i18n'
import type { AppConfig } from '../stores/appStore'
```

Destructure:

```ts
applyPersistedConfigPatch,
```

Add listener:

```ts
addListener<Partial<AppConfig>>('config:patch', (patch) => {
  applyPersistedConfigPatch(patch)
  if (patch.ui_language) {
    i18n.changeLanguage(patch.ui_language)
    localStorage.setItem('ui_language', patch.ui_language)
  }
})
```

Add `applyPersistedConfigPatch` to the effect dependency list.

- [ ] **Step 6: Verify store tests pass**

Run:

```powershell
npm test -- src/stores/__tests__/appStore.test.ts
```

Expected: PASS.

- [ ] **Step 7: Commit Task 5**

```powershell
git add src/stores/appStore.ts src/stores/__tests__/appStore.test.ts src/hooks/useTauriEvents.ts src/lib/tauri.ts
git commit -m "fix: apply persisted config patches"
```

---

### Task 6: Capsule Menu and Auto-Hide Visibility Logic

**Files:**
- Modify: `src/hooks/useCapsuleResize.ts`
- Create: `src/hooks/__tests__/useCapsuleResize.test.ts`
- Modify: `src/components/Capsule/CapsuleContextMenu.tsx`
- Create: `src/components/Capsule/__tests__/CapsuleContextMenu.test.tsx`
- Modify: `src/i18n/locales/*.json`

- [ ] **Step 1: Write failing visibility pure-function tests**

Create `src/hooks/__tests__/useCapsuleResize.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { getCapsuleVisibility } from '../useCapsuleResize'

describe('getCapsuleVisibility', () => {
  it('hides idle capsule when auto-hide is enabled', () => {
    expect(
      getCapsuleVisibility({
        capsuleAutoHide: true,
        contextMenuOpen: false,
        capsuleExpanded: false,
        hasError: false,
        pipelineState: 'idle',
      }),
    ).toBe(false)
  })

  it('shows idle capsule when an error appears', () => {
    expect(
      getCapsuleVisibility({
        capsuleAutoHide: true,
        contextMenuOpen: false,
        capsuleExpanded: false,
        hasError: true,
        pipelineState: 'idle',
      }),
    ).toBe(true)
  })

  it('shows active capsule while recording', () => {
    expect(
      getCapsuleVisibility({
        capsuleAutoHide: true,
        contextMenuOpen: false,
        capsuleExpanded: false,
        hasError: false,
        pipelineState: 'recording',
      }),
    ).toBe(true)
  })
})
```

- [ ] **Step 2: Run visibility tests and verify RED**

Run:

```powershell
npm test -- src/hooks/__tests__/useCapsuleResize.test.ts
```

Expected: FAIL because `getCapsuleVisibility` is not exported.

- [ ] **Step 3: Export visibility helper and update hook**

In `src/hooks/useCapsuleResize.ts`, add:

```ts
export interface CapsuleVisibilityInput {
  capsuleAutoHide: boolean
  contextMenuOpen: boolean
  capsuleExpanded: boolean
  hasError: boolean
  pipelineState: PipelineState
}

export function getCapsuleVisibility({
  capsuleAutoHide,
  contextMenuOpen,
  capsuleExpanded,
  hasError,
  pipelineState,
}: CapsuleVisibilityInput): boolean {
  return (
    !capsuleAutoHide ||
    contextMenuOpen ||
    capsuleExpanded ||
    hasError ||
    pipelineState !== 'idle'
  )
}
```

Inside the effect, compute:

```ts
const shouldShow = getCapsuleVisibility({
  capsuleAutoHide,
  contextMenuOpen,
  capsuleExpanded,
  hasError,
  pipelineState,
})
```

After sizing/positioning, reconcile visibility:

```ts
if (shouldShow) {
  await win.show().catch(() => {})
} else {
  await win.hide().catch(() => {})
}
```

Keep the first-mount positioning logic, but remove the old `leftIdle` / `becameIdle` early return so idle-to-idle config changes and idle errors are handled.

- [ ] **Step 4: Verify visibility tests pass**

Run:

```powershell
npm test -- src/hooks/__tests__/useCapsuleResize.test.ts
```

Expected: PASS.

- [ ] **Step 5: Write failing capsule menu tests**

Create `src/components/Capsule/__tests__/CapsuleContextMenu.test.tsx`:

```tsx
import React from 'react'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useAppStore } from '../../../stores/appStore'
import { CapsuleContextMenu } from '../CapsuleContextMenu'
import { setCapsuleAutoHide } from '../../../lib/tauri'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        'capsule.menu.hideWhenIdle': 'Hide capsule when idle',
        'capsule.menu.keepVisible': 'Keep capsule visible',
        'capsule.menu.openMainWindow': 'Open Main Window',
        'capsule.menu.settings': 'Settings',
        'capsule.menu.history': 'History',
        'capsule.menu.account': 'Account',
        'capsule.menu.upgrade': 'Upgrade',
        'capsule.menu.exit': 'Exit',
      })[key] ?? key,
  }),
}))

vi.mock('../../../lib/tauri', () => ({
  setCapsuleAutoHide: vi.fn().mockResolvedValue(undefined),
}))

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  useAppStore.setState(useAppStore.getInitialState())
})

describe('CapsuleContextMenu', () => {
  it('shows hide action when auto-hide is disabled', () => {
    render(<CapsuleContextMenu onClose={vi.fn()} />)
    expect(screen.getByRole('menuitem', { name: /hide capsule when idle/i })).toBeInTheDocument()
  })

  it('calls partial capsule visibility command', async () => {
    const onClose = vi.fn()
    render(<CapsuleContextMenu onClose={onClose} />)
    fireEvent.click(screen.getByRole('menuitem', { name: /hide capsule when idle/i }))
    await waitFor(() => expect(setCapsuleAutoHide).toHaveBeenCalledWith(true))
    expect(onClose).toHaveBeenCalled()
  })

  it('shows keep visible action when auto-hide is enabled', () => {
    useAppStore.getState().updateConfig({ capsule_auto_hide: true })
    render(<CapsuleContextMenu onClose={vi.fn()} />)
    expect(screen.getByRole('menuitem', { name: /keep capsule visible/i })).toBeInTheDocument()
  })
})
```

- [ ] **Step 6: Run capsule menu tests and verify RED**

Run:

```powershell
npm test -- src/components/Capsule/__tests__/CapsuleContextMenu.test.tsx
```

Expected: FAIL because the menu item and command call do not exist.

- [ ] **Step 7: Implement menu item**

In `src/components/Capsule/CapsuleContextMenu.tsx`, import:

```ts
import { Settings, History, LogOut, CircleUser, Crown, AppWindow, Eye, EyeOff } from 'lucide-react'
import { setCapsuleAutoHide } from '../../lib/tauri'
import { useAppStore } from '../../stores/appStore'
```

Inside component:

```ts
const capsuleAutoHide = useAppStore((s) => s.config.capsule_auto_hide)
```

Add item before Settings:

```ts
{
  icon: capsuleAutoHide ? Eye : EyeOff,
  label: capsuleAutoHide ? t('capsule.menu.keepVisible') : t('capsule.menu.hideWhenIdle'),
  onClick: async () => {
    await setCapsuleAutoHide(!capsuleAutoHide)
    onClose()
  },
},
```

Add locale keys under `capsule.menu` in every locale:

```json
"hideWhenIdle": "Hide capsule when idle",
"keepVisible": "Keep capsule visible"
```

Use these exact values:

```json
{
  "en": {
    "hideWhenIdle": "Hide capsule when idle",
    "keepVisible": "Keep capsule visible"
  },
  "zh": {
    "hideWhenIdle": "空闲时隐藏胶囊",
    "keepVisible": "保持胶囊可见"
  },
  "ja": {
    "hideWhenIdle": "待機中はカプセルを非表示",
    "keepVisible": "カプセルを表示したままにする"
  },
  "ko": {
    "hideWhenIdle": "유휴 시 캡슐 숨기기",
    "keepVisible": "캡슐 항상 표시"
  },
  "fr": {
    "hideWhenIdle": "Masquer la capsule au repos",
    "keepVisible": "Garder la capsule visible"
  },
  "de": {
    "hideWhenIdle": "Kapsel im Leerlauf ausblenden",
    "keepVisible": "Kapsel sichtbar lassen"
  },
  "es": {
    "hideWhenIdle": "Ocultar cápsula en reposo",
    "keepVisible": "Mantener cápsula visible"
  },
  "pt": {
    "hideWhenIdle": "Ocultar cápsula em repouso",
    "keepVisible": "Manter cápsula visível"
  },
  "ru": {
    "hideWhenIdle": "Скрывать капсулу в простое",
    "keepVisible": "Оставлять капсулу видимой"
  },
  "it": {
    "hideWhenIdle": "Nascondi capsula quando inattiva",
    "keepVisible": "Mantieni capsula visibile"
  }
}
```

- [ ] **Step 8: Verify capsule tests pass**

Run:

```powershell
npm test -- src/hooks/__tests__/useCapsuleResize.test.ts src/components/Capsule/__tests__/CapsuleContextMenu.test.tsx
```

Expected: PASS.

- [ ] **Step 9: Commit Task 6**

```powershell
git add src/hooks/useCapsuleResize.ts src/hooks/__tests__/useCapsuleResize.test.ts src/components/Capsule/CapsuleContextMenu.tsx src/components/Capsule/__tests__/CapsuleContextMenu.test.tsx src/i18n/locales
git commit -m "fix: expose idle capsule visibility controls"
```

---

### Task 7: Onboarding Copy, Integration Verification, and Release Readiness

**Files:**
- Modify: `src/components/Onboarding/DoneStep.tsx`
- Modify: `src/i18n/locales/*.json`
- Test: `src/components/Settings/__tests__/Settings.test.tsx`

- [ ] **Step 1: Update onboarding completion copy**

Replace the completion subtitle key use in `DoneStep.tsx` with a key that matches hidden-by-default behavior:

```tsx
<p className="text-[13px] text-text-secondary mt-1">
  {t('onboarding.done.capsuleAppearsWhenRecording')}
</p>
```

Replace `clickCapsuleSub` and `dragToRepositionSub` with text that qualifies the capsule as visible only during active work. Change the right-click tip description in `DoneStep.tsx` from `t('onboarding.done.rightClickMenuSub')` to `t('onboarding.done.restoreCapsuleSub')`.

For English, update/add these locale keys under `onboarding.done`:

```json
"capsuleAppearsWhenRecording": "The capsule appears while recording and processing.",
"clickCapsuleSub": "When visible, click the capsule to toggle recording.",
"dragToRepositionSub": "When visible, drag the capsule anywhere on screen.",
"restoreCapsuleSub": "Use the tray or Settings to keep the capsule visible while idle."
```

Use these exact values for all locales:

```json
{
  "en": {
    "capsuleAppearsWhenRecording": "The capsule appears while recording and processing.",
    "clickCapsuleSub": "When visible, click the capsule to toggle recording.",
    "dragToRepositionSub": "When visible, drag the capsule anywhere on screen.",
    "restoreCapsuleSub": "Use the tray or Settings to keep the capsule visible while idle."
  },
  "zh": {
    "capsuleAppearsWhenRecording": "胶囊会在录音和处理时出现。",
    "clickCapsuleSub": "胶囊可见时，点击它可切换录音状态。",
    "dragToRepositionSub": "胶囊可见时，可将它拖到屏幕任意位置。",
    "restoreCapsuleSub": "可通过托盘或设置让胶囊在空闲时保持可见。"
  },
  "ja": {
    "capsuleAppearsWhenRecording": "カプセルは録音中と処理中に表示されます。",
    "clickCapsuleSub": "表示されているときは、カプセルをクリックして録音を切り替えます。",
    "dragToRepositionSub": "表示されているときは、カプセルを画面上の好きな場所へドラッグできます。",
    "restoreCapsuleSub": "トレイまたは設定から、待機中もカプセルを表示したままにできます。"
  },
  "ko": {
    "capsuleAppearsWhenRecording": "캡슐은 녹음 및 처리 중에 나타납니다.",
    "clickCapsuleSub": "캡슐이 보일 때 클릭하면 녹음을 전환할 수 있습니다.",
    "dragToRepositionSub": "캡슐이 보일 때 화면 어디로든 드래그할 수 있습니다.",
    "restoreCapsuleSub": "트레이 또는 설정에서 유휴 중에도 캡슐을 계속 표시할 수 있습니다."
  },
  "fr": {
    "capsuleAppearsWhenRecording": "La capsule apparaît pendant l'enregistrement et le traitement.",
    "clickCapsuleSub": "Quand elle est visible, cliquez sur la capsule pour activer ou arrêter l'enregistrement.",
    "dragToRepositionSub": "Quand elle est visible, faites glisser la capsule où vous voulez sur l'écran.",
    "restoreCapsuleSub": "Utilisez la barre d'état ou les paramètres pour garder la capsule visible au repos."
  },
  "de": {
    "capsuleAppearsWhenRecording": "Die Kapsel erscheint während Aufnahme und Verarbeitung.",
    "clickCapsuleSub": "Wenn sie sichtbar ist, klicken Sie auf die Kapsel, um die Aufnahme umzuschalten.",
    "dragToRepositionSub": "Wenn sie sichtbar ist, ziehen Sie die Kapsel an eine beliebige Stelle auf dem Bildschirm.",
    "restoreCapsuleSub": "Über Tray oder Einstellungen können Sie die Kapsel im Leerlauf sichtbar lassen."
  },
  "es": {
    "capsuleAppearsWhenRecording": "La cápsula aparece durante la grabación y el procesamiento.",
    "clickCapsuleSub": "Cuando esté visible, haz clic en la cápsula para alternar la grabación.",
    "dragToRepositionSub": "Cuando esté visible, arrastra la cápsula a cualquier lugar de la pantalla.",
    "restoreCapsuleSub": "Usa la bandeja o Configuración para mantener la cápsula visible en reposo."
  },
  "pt": {
    "capsuleAppearsWhenRecording": "A cápsula aparece durante a gravação e o processamento.",
    "clickCapsuleSub": "Quando estiver visível, clique na cápsula para alternar a gravação.",
    "dragToRepositionSub": "Quando estiver visível, arraste a cápsula para qualquer lugar da tela.",
    "restoreCapsuleSub": "Use a bandeja ou as Configurações para manter a cápsula visível em repouso."
  },
  "ru": {
    "capsuleAppearsWhenRecording": "Капсула появляется во время записи и обработки.",
    "clickCapsuleSub": "Когда капсула видна, нажмите на нее, чтобы переключить запись.",
    "dragToRepositionSub": "Когда капсула видна, перетащите ее в любое место на экране.",
    "restoreCapsuleSub": "Используйте трей или настройки, чтобы оставлять капсулу видимой в простое."
  },
  "it": {
    "capsuleAppearsWhenRecording": "La capsula appare durante la registrazione e l'elaborazione.",
    "clickCapsuleSub": "Quando è visibile, fai clic sulla capsula per attivare o interrompere la registrazione.",
    "dragToRepositionSub": "Quando è visibile, trascina la capsula ovunque sullo schermo.",
    "restoreCapsuleSub": "Usa il tray o le Impostazioni per mantenere la capsula visibile quando è inattiva."
  }
}
```

- [ ] **Step 2: Add settings save sync test**

In `src/components/Settings/__tests__/Settings.test.tsx`, add this store-level regression for full-save patch semantics:

```tsx
it('persisted capsule visibility patch does not erase unrelated dirty settings', async () => {
  renderSettings()

  act(() => {
    useAppStore.getState().updateConfig({ theme: 'dark' })
    useAppStore.getState().applyPersistedConfigPatch({ capsule_auto_hide: true })
  })

  expect(useAppStore.getState().config.theme).toBe('dark')
  expect(useAppStore.getState().config.capsule_auto_hide).toBe(true)
})
```

- [ ] **Step 3: Run frontend test suite**

Run:

```powershell
npm test
```

Expected: PASS.

- [ ] **Step 4: Run frontend static checks**

Run:

```powershell
npm run build
npm run lint
npm run format:check
```

Expected: all PASS.

- [ ] **Step 5: Run Rust test suite and static checks**

Run:

```powershell
cd src-tauri
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Expected: all PASS.

- [ ] **Step 6: Run final diff checks**

Run from repo root:

```powershell
git diff --check
git status --short
```

Expected: no whitespace errors. `git status --short` shows only intended files before commit, then clean after commit.

- [ ] **Step 7: Commit Task 7**

```powershell
git add src/components/Onboarding/DoneStep.tsx src/i18n/locales src/components/Settings/__tests__/Settings.test.tsx
git commit -m "fix: align onboarding with hidden capsule behavior"
```

---

## Plan Self-Review

Spec coverage:

- Cloud STT quota/auth distinction is covered in Tasks 1 and 3.
- No-speech structured localization and overwrite prevention are covered in Tasks 2 and 3.
- Capsule idle hiding, right-click control, and recovery through tray are covered in Tasks 4 and 6.
- Cross-webview patch sync and dirty-setting protection are covered in Tasks 4 and 5.
- New-install vs legacy default behavior is covered in Task 4.
- Tray language lookup and refresh after persisted language changes are covered in Task 4.
- Onboarding copy is covered in Task 7.

Placeholder scan:

- This plan contains no placeholder steps, no deferred requirements, and no unresolved file ownership.

Type consistency:

- The Rust command is consistently named `set_capsule_auto_hide`.
- The frontend wrapper is consistently named `setCapsuleAutoHide`.
- The frontend persisted patch action is consistently named `applyPersistedConfigPatch`.
- The sync event is consistently named `config:patch`.
