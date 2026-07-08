# OpenTypeless x OpenLess 差异补齐 Spec

Date: 2026-07-06
Target repo: `tover0314-w/opentypeless`
Reference repo: `Open-Less/openless`

## 1. Executive Summary

OpenLess 和 OpenTypeless 不是同一个方向的简单替代品。

OpenLess 更像一个已经投入大量平台工程的「语音到当前光标」系统：本地 ASR、跨平台插入、流式输出、剪贴板恢复、Style Pack、Windows/macOS/Android 细节、远程手机麦克风、甚至 Less Computer 编程代理都做得很深。

OpenTypeless 更像一个已经具备产品闭环的开放语音助手：BYOK + Cloud、账号/订阅/额度、Ask Anything、更多 UI 语言、Linux 支持、文档和社区包装、Typeless 竞品路线图都更成体系。

推荐策略不是照搬 OpenLess，而是把 OpenLess 证明过的底层能力补进 OpenTypeless，同时保留 OpenTypeless 的开放、商业化和跨平台定位。

优先级结论：

1. **P0: Secrets 安全迁移**：API key 现在仍随 `AppConfig` 进入 `settings.json`，必须迁到 OS keychain / secret service，并提供旧配置迁移。
2. **P0: 插入层可靠性**：补剪贴板恢复、插入方式设置、Windows/macOS fallback、输出失败诊断；这是语音输入工具的信任底线。
3. **P0/P1: 流式插入**：OpenTypeless 已能接收 LLM stream chunk，但实际输出仍等完整结果；应把 polish stream 接到插入层，降低「松开热键后等待」体感。
4. **P1: Local ASR 产品化**：OpenTypeless 有 `custom-whisper`，但没有内置模型管理/本地引擎；应分阶段补 Apple Speech、Qwen3/Whisper、Windows Foundry/Sherpa。
5. **P1: Selected Text + Ask 收敛**：当前 selected-text polish 和 Ask Anything 是两条弱关联链路；应升级成可预测的 edit / ask / translate / summarize command router。
6. **P1/P2: Scenes 升级为 Style Packs**：保留 local-first scenes，但补 built-in 数量、metadata、examples、import/export、诊断；先不做完整 marketplace backend。
7. **暂不追**：Android、远程手机麦克风、Less Computer/coding agent、完整 marketplace 商业系统。

## 2. Source Baseline

本 spec 基于本地读取两个代码库和文档，不基于未验证的二手描述。

OpenLess baseline:

- Local path: `/Users/bytedance/个人项目/openless`
- Commit inspected: `7de0c04` (`docs: 用协调沉降定律重构 README 能力/增强叙事,凸显开屏即授权完基础设施`)
- App path: `openless-all/app`
- Package/Cargo version: `1.3.14`
- Rough source size read: about `90k` TypeScript/Rust LOC, `161` Tauri commands, `116` Rust files.

OpenTypeless baseline:

- Local path: `/Users/bytedance/个人项目/opentypeless`
- Commit HEAD inspected: `a83456f` (`Update README visual tour`)
- Package/Cargo version: `0.1.42`
- Rough source size read: about `23k` TypeScript/Rust LOC, `32` Tauri commands, `34` Rust files.
- Worktree note: local worktree is dirty; this spec reflects the currently checked-out files, including local custom scenes work already present.

Key local evidence paths:

- OpenTypeless config/secrets: `src-tauri/src/storage/mod.rs`, `src/stores/appStore.ts`
- OpenTypeless pipeline/output: `src-tauri/src/pipeline.rs`, `src-tauri/src/output/*`
- OpenTypeless providers: `src-tauri/src/stt/*`, `src-tauri/src/llm/*`, `src/components/Settings/*`
- OpenTypeless existing roadmap: `docs/2026-06-27-typeless-competitive-roadmap-spec.md`, `docs/2026-06-30-local-first-custom-scenes-spec.md`
- OpenLess credentials: `openless-all/app/src-tauri/src/persistence/credentials.rs`
- OpenLess insertion: `openless-all/app/src-tauri/src/insertion.rs`, `openless-all/app/src-tauri/src/unicode_keystroke.rs`
- OpenLess local ASR: `openless-all/app/src-tauri/src/asr/local/*`
- OpenLess streaming polish: `openless-all/app/src-tauri/src/coordinator/polish_flow.rs`, `openless-all/app/src-tauri/src/coordinator/dictation.rs`
- OpenLess style packs: `openless-all/app/src-tauri/src/persistence/style_pack.rs`, `openless-all/app/src-tauri/src/commands/style_packs.rs`
- OpenLess settings UI: `openless-all/app/src/pages/settings/*`

## 3. Product Positioning Difference

| Area | OpenTypeless today | OpenLess today | Spec decision |
| --- | --- | --- | --- |
| Core promise | Open-source alternative to Wispr Flow/Superwhisper; BYOK + Cloud; voice input, rewriting, Ask | Voice to usable written text at current cursor; platform-first reliability | Keep OpenTypeless positioning; borrow platform reliability |
| Commercial loop | Account, Cloud Pro/Lifetime, cloud words, upgrade flow | Less visible in inspected code; more local/BYOK/tooling oriented | Keep OpenTypeless commercial loop |
| Provider breadth | Strong LLM/STT provider list, OpenAI-compatible, Cloud proxy, custom Whisper | Strong presets plus Gemini native, Codex OAuth, local ASR, model list validation | Add local providers and stronger validation; avoid removing existing providers |
| Local ASR | `custom-whisper` HTTP endpoint only | Apple Speech, Qwen3, Foundry Local Whisper, Sherpa ONNX | Add staged local ASR, do not require bundled model at first |
| Output insertion | Enigo keyboard, clipboard paste fallback, Linux Wayland warning | Clipboard restore, Unicode SendInput, macOS input source switching, streaming insert, platform fallback | Build a dedicated insertion layer |
| Ask/selected text | Ask is standalone short Q&A; selected-text context only in dictation/polish | QA panel, selection capture, hotkeys, selected text based flows | Unify command router |
| Style/scenes | Local custom scenes are now present; built-ins are limited; cloud packs additive | Style pack store with metadata, examples, import/export, marketplace hints | Extend scenes toward style packs without marketplace dependency |
| Secrets | API keys are fields on `AppConfig` | OS keyring vault, chunking, migration from legacy plaintext | P0 fix |
| Docs/i18n | Strong README translations, docs, GitHub workflows | Strong usage/release docs, fewer UI locales | Keep OpenTypeless external strength |
| Android/remote/agent | Not in scope | Android plan, remote input LAN server, Less Computer agent | Explicitly defer |

## 4. Problem Statement

OpenTypeless already has a credible product shell, but the lower-level trust layer is behind OpenLess in the places users feel most:

- Whether API keys are actually protected.
- Whether text appears in the right field reliably.
- Whether the clipboard is preserved.
- Whether output starts quickly enough.
- Whether privacy users can stay local.
- Whether selected text commands behave predictably.

These gaps are not cosmetic. They directly affect activation, retention, support burden, and whether OpenTypeless can claim privacy-first BYOK with confidence.

## 5. Goals

1. Make BYOK secrets materially safer than plaintext config storage.
2. Improve output reliability across macOS, Windows, and Linux without losing current keyboard/clipboard modes.
3. Reduce perceived latency by inserting streamed polished output when safe.
4. Add a first-class local ASR path while preserving cloud and custom endpoint choices.
5. Turn selected-text context and Ask into a coherent command system.
6. Upgrade Scenes into a portable local-first style system.
7. Keep the OpenTypeless advantages: Linux support, BYOK, Cloud subscription, broad provider support, existing docs/i18n.
8. Preserve the existing quiet, premium UI feel: no large visual redesign, no feature dumping, and no heavy settings panels. All new controls should use progressive disclosure and minimal existing patterns.

## 6. Non-Goals

- Do not clone OpenLess branding, wording, or product scope wholesale.
- Do not build Android support in this program.
- Do not build the remote phone microphone/LAN input server now.
- Do not build Less Computer/coding agent now.
- Do not require sign-in for local scenes, BYOK, or local ASR.
- Do not block current Cloud Pro/Lifetime flows on local ASR work.
- Do not remove existing provider presets unless a provider is broken and separately deprecated.
- Do not make broad UI/layout changes as part of capability parity. UI changes must stay minimal, compact, and consistent with the current OpenTypeless visual system.

## 7. Recommended Program

### Track A: Credential Vault

Priority: P0

#### Current State

OpenTypeless stores secret fields inside `AppConfig`:

- `stt_api_key`
- `stt_custom_api_key`
- `llm_api_key`

`ConfigManager::save()` serializes `AppConfig` into Tauri store `settings.json`. The UI says keys are stored locally, which is true geographically but not sufficiently precise for a privacy/security claim.

OpenLess has a dedicated credentials vault using OS keyring, with plaintext legacy migration, chunking for Windows credential size limits, stable account names to reduce macOS Keychain prompts, process cache, and active provider snapshots.

#### Requirements

1. Add a `CredentialsVault` abstraction in Rust.
2. Store BYOK API keys outside `AppConfig`.
3. Keep non-secret provider config in `AppConfig`.
4. Migrate existing plaintext keys on first load after upgrade.
5. Clear migrated key fields from `settings.json`.
6. Provide Tauri commands:
   - `get_credential_status`
   - `set_stt_api_key`
   - `set_stt_custom_api_key`
   - `set_llm_api_key`
   - `delete_provider_credential`
   - optional `export_credential_diagnostics`
7. Frontend should show saved/empty status without needing to rehydrate full secret values.
8. Connection tests and live provider creation must read keys from vault, not from serialized config.

#### Implementation Notes

Recommended shape:

```rust
pub struct ProviderCredentialStatus {
    pub provider: String,
    pub has_secret: bool,
    pub updated_at: Option<String>,
}

pub trait CredentialsStore {
    fn get(&self, namespace: &str, provider: &str) -> Result<Option<String>>;
    fn set(&self, namespace: &str, provider: &str, secret: &str) -> Result<()>;
    fn remove(&self, namespace: &str, provider: &str) -> Result<()>;
}
```

Use `keyring` or equivalent OS-backed storage:

- macOS: Keychain
- Windows: Credential Manager
- Linux: Secret Service when available

Linux fallback must be explicit. If no secret service is available, the app should either:

- ask the user to keep secrets session-only, or
- use an opt-in legacy local config fallback with a clear warning.

Default fallback must not silently write plaintext secrets.

#### Acceptance Criteria

- After upgrade, existing `settings.json` no longer contains non-empty BYOK secrets.
- Existing users do not need to re-enter keys after migration succeeds.
- If migration fails, the app surfaces a recoverable error and does not delete the original key.
- Provider tests work for STT/LLM/custom Whisper after migration.
- Unit tests cover plaintext migration, empty keys, provider switching, deletion, and malformed legacy config.
- Manual verification covers macOS and Windows at minimum; Linux Secret Service behavior is documented.

### Track B: Dedicated Insertion Layer

Priority: P0

#### Current State

OpenTypeless has:

- `KeyboardOutput` using Enigo.
- `ClipboardOutput` that writes clipboard and simulates paste.
- fallback from keyboard to clipboard.
- platform capability warning for Linux Wayland.

Missing vs OpenLess:

- restore previous clipboard after paste.
- configurable paste shortcut.
- Windows Unicode SendInput mode.
- macOS input source/IME hardening.
- secure input detection.
- streaming output path.
- richer insertion diagnostics.

#### Requirements

1. Replace the binary `keyboard` / `clipboard` mental model with an insertion strategy:
   - `keyboard`
   - `clipboard_paste`
   - `clipboard_copy_only`
   - `unicode_sendinput` on Windows
   - future `ime_commit` / `tsf` if implemented
2. Add settings:
   - restore clipboard after paste
   - paste shortcut override
   - Windows insertion mode
   - newline behavior
   - fallback policy
3. Preserve current default behavior unless platform diagnostics indicate a safer default.
4. When clipboard paste is used, restore the original clipboard if enabled.
5. Emit structured warnings for:
   - keyboard unavailable
   - clipboard paste fallback
   - Wayland copy-only
   - secure input / inaccessible target
   - paste shortcut failed
6. Keep history write independent from insertion success, but mark insertion failure in the UI.

#### Acceptance Criteria

- Pasting text no longer permanently overwrites the clipboard when restore is enabled.
- Windows users can switch away from slow/fragile typed output.
- macOS insertion remains on main thread where required.
- Linux Wayland behavior remains honest: copy-only unless a reliable automation path exists.
- All output failures include a machine-readable code plus human-readable detail.
- Existing `output_mode` values migrate to the new insertion config.

### Track C: Streaming Polish Insertion

Priority: P0/P1

#### Current State

OpenTypeless `llm/openai.rs` already receives streamed chunks and calls a callback. The UI can append polished chunks, but final insertion happens after the complete response in `pipeline.rs`.

OpenLess streams polished deltas into a background typer, drains typed text, and falls back to one-shot insertion if streaming is unsupported.

#### Requirements

1. Add `streaming_insert_enabled` to config.
2. Implement streaming insertion only after STT final transcript is available.
3. Enable streaming for providers that support reliable chunk callbacks:
   - OpenAI-compatible providers
   - Cloud LLM if the proxy supports compatible streaming
4. Fallback to current one-shot output when:
   - provider does not stream
   - selected-text edit needs atomic replacement
   - output target is unsafe
   - insertion errors before first chunk
5. Do not stream raw ASR partials into target apps.
6. Keep displayed UI chunks and inserted chunks consistent.
7. Support cancellation between chunks.

#### Acceptance Criteria

- For supported providers, visible insertion starts before the full LLM response completes.
- If streaming fails before any inserted text, one-shot output still happens.
- If streaming partially succeeds, the app does not duplicate the already inserted prefix.
- Streaming can be disabled from settings.
- Tests cover chunk aggregation, cancellation, unsupported provider fallback, and partial failure.

### Track D: Local ASR Productization

Priority: P1

#### Current State

OpenTypeless supports `custom-whisper`, including a Speaches preset, which can point to a local HTTP server. This is useful but not the same as a productized local ASR experience.

OpenLess supports local ASR as first-class providers:

- Apple Speech on macOS.
- Qwen3 local ASR on macOS through native engine/model management.
- Windows Foundry Local Whisper.
- Windows Sherpa ONNX local.

#### Phased Approach

Phase 1: Local endpoint hardening

- Keep `custom-whisper`.
- Rename/position it clearly as `Local / Custom Whisper`.
- Add setup diagnostics:
  - endpoint reachable
  - model configured
  - auth optional
  - latency benchmark
  - sample transcription test
- Add docs for Speaches/local Whisper.

Phase 2: Low-friction OS local providers

- macOS: Apple Speech provider.
- Windows: Foundry Local Whisper if current Windows dependency and licensing story is acceptable.
- Keep these opt-in.

Phase 3: Model-managed local ASR

- Qwen3 or Whisper model download/management.
- Model registry, disk size display, delete/re-download.
- Keep model files outside app binary.
- Add clear CPU/GPU/performance expectations.

#### Requirements

1. Provider UI must distinguish:
   - Cloud
   - BYOK remote providers
   - Local HTTP endpoint
   - Built-in local engine
2. Local provider selection must not require an API key.
3. First activation should show model size/performance/privacy copy.
4. Failed local engine initialization must fall back to previous provider, not leave the app unusable.
5. Model download must be resumable or clearly retryable.

#### Acceptance Criteria

- A user can choose at least one local ASR option without creating a cloud account or entering an API key.
- Provider test works for local engines.
- Local provider status is visible in settings.
- Unsupported platforms do not show unusable local providers as primary options.
- Local ASR errors do not break cloud/BYOK providers.

### Track E: Selected Text + Ask Command Router

Priority: P1

#### Current State

OpenTypeless has two related but separate flows:

- Selected text can be captured during normal dictation and passed to the polish prompt.
- Ask Anything records a voice question and returns a short answer, but does not use selected text context.

OpenLess has deeper selection capture and a QA panel flow. It also captures focus target before opening UI so the panel does not steal the target app context.

#### Requirements

1. Introduce command intent types:
   - `dictate`
   - `edit_selection`
   - `ask_selection`
   - `summarize_selection`
   - `translate_selection`
   - `open_question`
2. Add a lightweight router that uses:
   - whether selected text exists
   - which hotkey was used
   - transcript command verbs
   - user setting defaults
3. Split answer flows from replacement flows:
   - edit/translate selection should replace or paste text.
   - ask/summarize should open an answer panel and not overwrite text.
4. Add selected text to Ask context when present and allowed.
5. Add max selected-text length, truncation notice, and privacy copy.
6. Keep selected-text capture disabled by default until permissions and UX are reliable enough, or make first-use consent explicit.

#### Acceptance Criteria

- User can select text, speak "rewrite this shorter", and get replacement text.
- User can select text, speak "what does this mean", and get an answer panel without replacing the selection.
- User can ask an open question with no selection.
- Router decisions are logged locally as structured diagnostics without storing extra sensitive text.
- Prompt tests cover edit vs ask vs summarize ambiguity.

### Track F: Scenes To Style Packs

Priority: P1/P2

#### Current State

OpenTypeless has local custom scenes in current worktree:

- `custom_scenes`
- `active_scene`
- `ScenesPane` with create/edit/delete/duplicate/activate.
- Built-in scenes in `src/lib/scenes/builtinScenes.ts`, currently limited.
- Cloud scene packs still exist as additive content.

OpenLess style packs have richer metadata:

- id/name/description/author/version/kind/base mode
- prompt
- examples
- tags
- icon
- recommended model
- compatibility
- import/export/reset
- origin metadata

#### Requirements

1. Complete the local-first scenes spec already started in `docs/2026-06-30-local-first-custom-scenes-spec.md`.
2. Add built-ins beyond the current starter set:
   - clean dictation
   - meeting notes
   - professional email
   - support reply
   - code comment / technical note
3. Extend scene schema gradually:
   - tags
   - examples
   - version
   - source/origin
   - optional recommended model
4. Add import/export JSON.
5. Add runtime diagnostics:
   - active scene id/name
   - prompt length
   - source type
   - validation warnings
6. Keep cloud packs additive and optional.

#### Acceptance Criteria

- Local scenes work fully signed out.
- Built-in scenes are useful when no account/cloud packs exist.
- Users can export/import scenes without database surgery.
- Scene prompt validation prevents empty, huge, or malformed prompts.
- Upgrade copy does not imply self-authored local scenes are paid-only.

### Track G: Diagnostics, History, And Recording Tools

Priority: P2

#### Current State

OpenTypeless has SQLite history with max retention count and a clear-history command. It does not expose history retention settings, audio debug recording, re-transcription, heatmap/stats, or detailed permission diagnosis at OpenLess depth.

#### Requirements

1. Add history retention settings:
   - keep last N entries
   - keep for N days
   - never save history
2. Add optional debug recording:
   - disabled by default
   - local only
   - obvious delete controls
3. Add retranscribe/repolish from history when raw audio/debug mode exists.
4. Add microphone device selection.
5. Add permission diagnostics:
   - microphone
   - accessibility/input automation
   - clipboard
   - global hotkey
   - Linux session type
6. Add lightweight local stats only if it helps product decisions and can be disabled.

#### Acceptance Criteria

- Privacy-sensitive users can disable history.
- Debug audio never records unless explicitly enabled.
- Permission diagnostics give actionable next steps.
- No telemetry is added without a separate analytics/privacy spec.

## 8. Implementation Phasing

Recommended order:

1. **M0: Decisions and migrations**
   - Decide vault backend/fallback policy.
   - Decide first built-in local ASR provider.
   - Freeze config migration shape.
2. **M1: Credential vault**
   - Add vault abstraction.
   - Migrate plaintext keys.
   - Update provider tests/runtime to read vault.
3. **M2: Insertion layer**
   - Clipboard restore.
   - Structured output warnings.
   - Windows insertion mode.
   - Settings UI.
4. **M3: Streaming insertion MVP**
   - Stream LLM deltas into insertion layer.
   - Fallback and cancellation.
   - Provider support gates.
5. **M4: Local ASR MVP**
   - Harden local endpoint.
   - Add one built-in local provider.
   - Add local model/provider UI.
6. **M5: Selected text command router**
   - Intent routing.
   - Ask with selection.
   - Replacement vs answer panel separation.
7. **M6: Scenes/style packs**
   - More built-ins.
   - Metadata.
   - Import/export.
8. **M7: Diagnostics/history**
   - Retention.
   - Device selection.
   - Debug recording.

## 9. File Impact Map

Likely Rust changes:

- `src-tauri/Cargo.toml`
  - add `keyring` or chosen vault dependency.
  - add local ASR dependencies only in later milestone.
- `src-tauri/src/storage/mod.rs`
  - remove secrets from persisted `AppConfig`.
  - add config migration helpers.
  - add history retention settings later.
- `src-tauri/src/storage/credentials.rs` or `src-tauri/src/credentials.rs`
  - new credential vault.
- `src-tauri/src/commands/config.rs`
  - expose credential commands or split into `commands/credentials.rs`.
- `src-tauri/src/commands/stt.rs`
  - read vault during provider tests.
- `src-tauri/src/commands/llm.rs`
  - read vault during provider tests/model listing.
- `src-tauri/src/pipeline.rs`
  - resolve provider secrets from vault.
  - connect streaming polish to output layer.
  - route command intents.
- `src-tauri/src/output/*`
  - add insertion strategy abstraction and clipboard restore.
- `src-tauri/src/selection.rs`
  - likely new module if selected-text capture is promoted.
- `src-tauri/src/stt/local/*`
  - new local ASR engines/providers.
- `src-tauri/src/llm/prompt.rs`
  - add command-specific prompts and selected-text ask prompts.

Likely frontend changes:

- `src/stores/appStore.ts`
  - remove secret values from `AppConfig` shape.
  - add credential status state.
  - add insertion and streaming settings.
- `src/components/Settings/SttPane.tsx`
  - credential status/save/delete flow.
  - local provider UI.
- `src/components/Settings/LlmPane.tsx`
  - credential status/save/delete flow.
  - streaming insert setting.
  - Ask selected-text UX.
- `src/components/Settings/GeneralPane.tsx`
  - insertion/clipboard/hotkey/permission diagnostics.
- `src/components/Settings/ScenesPane.tsx`
  - style pack metadata, import/export.
- `src/components/AskPanel/AskPanel.tsx`
  - selected-text aware answer panel and history.
- `src/lib/constants.ts`
  - provider taxonomy and defaults.
- `src/i18n/locales/*.json`
  - settings, diagnostics, credential migration, local ASR copy.

Test targets:

- Rust unit tests for credential migration, provider config resolution, insertion fallback, prompt routing.
- Frontend tests for settings panes and dirty/save behavior after secrets leave config.
- Manual smoke tests for macOS, Windows, Linux X11, Linux Wayland.

## 10. Risks And Mitigations

| Risk | Why it matters | Mitigation |
| --- | --- | --- |
| Keychain prompts annoy users | macOS Keychain ACL prompts can repeat if account names change | stable service/account names; process cache; minimal reads |
| Linux secret storage is inconsistent | Secret Service may not be available on minimal desktops | explicit session-only or opt-in fallback; do not silently plaintext |
| Config migration loses keys | High trust damage | two-phase migration: write vault, verify read, then clear plaintext |
| Streaming insertion corrupts target text | Partial insertion is harder to undo | default off or beta; fallback before first chunk; do not stream selected-text replacement initially |
| Local ASR bloats app/support surface | Models and native libs increase complexity | staged rollout; model download outside binary; platform-gated UI |
| Selected-text router surprises users | Replacing text when user expected answer is destructive | separate answer vs replacement flows; preview/confirm option for early builds |
| Chasing OpenLess agent/mobile scope dilutes roadmap | Those are large separate products | explicitly out-of-scope until core dictation trust is solved |

## 11. Success Metrics

Primary:

- 0 non-empty API key fields remain in persisted config after migration.
- First successful dictation completion rate improves or does not regress.
- Median time from hotkey release to first inserted character decreases for streaming-supported providers.
- Output fallback warnings decrease over time on macOS/Windows.
- Selected-text command success rate is measurable and support tickets do not spike.

Secondary:

- Local ASR activation rate among privacy/BYOK users.
- Percentage of users with clipboard restore enabled.
- Scene activation and custom scene creation rate.
- Ask with selection usage.
- Provider test success/failure distribution by provider.

Guardrails:

- No mandatory cloud account for BYOK/local flows.
- No regression in Cloud Pro subscription/words UX.
- No silent plaintext secret fallback.
- No unexpected selected-text/audio retention beyond explicit history/debug settings.

## 12. Open Questions

1. Which built-in local ASR provider should ship first?
   - Fastest low-risk path: Apple Speech on macOS plus hardened local Whisper endpoint.
   - Stronger privacy/product path: Qwen3 or Whisper model management.
   - Better Windows parity path: Foundry/Sherpa first.
2. Should streaming insertion be default-on after beta, or opt-in permanently?
3. Should selected-text replacement require confirmation in early versions?
4. Should Cloud credentials/session token also move into the vault, or remain separate session state?
5. How much OpenLess style pack compatibility is desired?
   - internal schema inspiration only
   - import-compatible subset
   - full compatibility
6. Does OpenTypeless want to claim local-first privacy for all BYOK providers, or only for local ASR/LLM paths?

## 13. Recommended Next Decision

Start with **Track A + Track B**.

Reason:

- Secrets and insertion reliability are foundational.
- They reduce risk before adding local ASR or more command surfaces.
- They do not conflict with existing Cloud, Typeless roadmap, or local scenes work.
- They create the infrastructure needed for streaming insertion and selected-text commands.

Concrete next implementation spec should be:

`Credential Vault And Config Migration Implementation Plan`

It should define exact store names, migration sequence, command signatures, frontend state shape, and tests before code changes begin.
