# OpenLess 深度学习与 OpenTypeless 补齐实施 Spec

Date: 2026-07-06
Owner: OpenTypeless desktop
Reference: `Open-Less/openless` local clone at `/Users/bytedance/个人项目/openless`
Target: `tover0314-w/opentypeless` local repo at `/Users/bytedance/个人项目/opentypeless`

## 1. Executive Summary

我们不是要把 OpenTypeless 改成 OpenLess。这里把 OpenLess 当作参考实现，学习它已经踩过坑的桌面基础设施：凭据、热键、插入、流式输出、本地 ASR、划词工作流、设置诊断、local-first style packs。

说人话，OpenTypeless 要补齐这些东西：

1. **Secrets 不要再存配置文件**：BYOK API keys 要从 `AppConfig/settings.json` 迁到系统凭据库，并且迁移不能丢 key。
2. **Hotkey 要升级成系统**：不是多加几个快捷键，而是要有绑定校验、冲突检测、按下/松开边沿、安装状态、平台能力、失败回滚和录制器。
3. **插入层要更可靠**：补剪贴板恢复、粘贴快捷键、Windows 插入策略、结构化失败提示，避免用户文本丢失。
4. **流式输出要真正落到目标 App**：现在更多是 UI 里看到流，真正插入仍要等完整结果。
5. **本地 ASR 要产品化**：`custom-whisper` 有用但不够。用户需要本地 provider、诊断、模型状态和失败回退。
6. **划词、Ask、翻译要合成一套 command workflow**：选中文字后，替换、解释、总结、翻译要走不同安全路径。
7. **Scenes 要从 prompt list 变成 portable style system**：补内置模板、metadata、examples、import/export、validation。
8. **权限和诊断要可见**：麦克风、辅助功能、热键安装、剪贴板/插入、Linux 限制、provider 测试都要能看懂。

Recommended build order:

1. Credential Vault
2. Hotkey System Upgrade
3. Insertion Layer + Clipboard Restore
4. Streaming Insertion
5. Local ASR MVP
6. Selected Text Command Router
7. Scenes/Style Packs
8. Diagnostics/History/Microphone polish

The first two tracks should be treated as foundation work. If secrets and hotkeys stay weak, adding local ASR or selected-text workflows will amplify support issues.

## 2. Evidence Map

### OpenLess Files Studied

Hotkey and coordinator:

- `openless-all/app/src-tauri/src/hotkey.rs`
- `openless-all/app/src-tauri/src/combo_hotkey.rs`
- `openless-all/app/src-tauri/src/qa_hotkey.rs`
- `openless-all/app/src-tauri/src/global_hotkey_runtime.rs`
- `openless-all/app/src-tauri/src/shortcut_binding.rs`
- `openless-all/app/src-tauri/src/coordinator/hotkey_loops.rs`
- `openless-all/app/src-tauri/src/coordinator_state.rs`
- `openless-all/app/src-tauri/src/coordinator/dictation.rs`
- `openless-all/app/src-tauri/src/coordinator/qa.rs`
- `openless-all/app/src-tauri/src/commands/hotkeys.rs`
- `openless-all/app/src-tauri/src/commands/settings.rs`
- `openless-all/app/src/components/ShortcutRecorder.tsx`
- `openless-all/app/src/state/HotkeySettingsContext.tsx`
- `openless-all/app/src/lib/hotkey.ts`

Settings, permissions, insertion:

- `openless-all/app/src/pages/settings/RecordingInputSection.tsx`
- `openless-all/app/src/pages/settings/PermissionsSection.tsx`
- `openless-all/app/src-tauri/src/commands/permissions_cmds.rs`
- `openless-all/app/src-tauri/src/insertion.rs`
- `openless-all/app/src-tauri/src/unicode_keystroke.rs`
- `openless-all/app/src-tauri/src/permissions.rs`
- `openless-all/app/src-tauri/src/types.rs`

Credentials:

- `openless-all/app/src-tauri/src/persistence/credentials.rs`
- `openless-all/app/src-tauri/src/commands/credentials.rs`
- `openless-all/app/src-tauri/src/commands/providers.rs`

Local ASR:

- `openless-all/app/src-tauri/src/asr/local/mod.rs`
- `openless-all/app/src-tauri/src/asr/local/models.rs`
- `openless-all/app/src-tauri/src/asr/local/download.rs`
- `openless-all/app/src-tauri/src/asr/local/apple_speech_provider.rs`
- `openless-all/app/src-tauri/src/asr/local/qwen_engine.rs`
- `openless-all/app/src-tauri/src/asr/local/foundry_provider.rs`
- `openless-all/app/src-tauri/src/asr/local/sherpa_provider.rs`
- `openless-all/app/src/pages/settings/LocalModelSection.tsx`
- `openless-all/app/src/pages/LocalAsr/index.tsx`

Selected text, QA, translation, style:

- `openless-all/app/src-tauri/src/selection.rs`
- `openless-all/app/src/pages/SelectionAsk.tsx`
- `openless-all/app/src/pages/Translation.tsx`
- `openless-all/app/src-tauri/src/polish/prompt_compose.rs`
- `openless-all/app/src-tauri/src/persistence/style_pack.rs`
- `openless-all/app/src-tauri/src/commands/style_packs.rs`

### OpenTypeless Files Compared

- `src-tauri/src/storage/mod.rs`
- `src-tauri/src/pipeline.rs`
- `src-tauri/src/hotkey.rs`
- `src-tauri/src/output/keyboard.rs`
- `src-tauri/src/output/clipboard.rs`
- `src-tauri/src/output/mod.rs`
- `src-tauri/src/stt/*`
- `src-tauri/src/llm/*`
- `src-tauri/src/commands/*`
- `src/components/Settings/SttPane.tsx`
- `src/components/Settings/LlmPane.tsx`
- `src/components/Settings/ScenesPane.tsx`
- `src/components/AskPanel/AskPanel.tsx`
- `src/stores/appStore.ts`
- `src/lib/constants.ts`
- `docs/2026-06-27-typeless-competitive-roadmap-spec.md`
- `docs/2026-06-30-local-first-custom-scenes-spec.md`

## 3. Current OpenTypeless Baseline

### What OpenTypeless Already Does Well

- BYOK and Cloud are both first-class product paths.
- Account, subscription, cloud quota, upgrade, and AppSumo/lifetime logic exist.
- Multiple STT providers exist: Deepgram, AssemblyAI, Volcengine Doubao, GLM-ASR, OpenAI Whisper, Groq Whisper, SiliconFlow, custom Whisper, Cloud.
- Multiple LLM providers exist: OpenAI-compatible presets, Cloud, Ollama, OpenRouter, Gemini-compatible path, etc.
- Ask Anything exists as a separate short Q&A flow.
- Selected text can be captured and passed to polish.
- Local custom scenes now exist in current worktree.
- UI localization breadth is stronger than OpenLess.
- Linux support is part of the product.
- GitHub workflows/docs/community surface is relatively strong.

### What OpenTypeless Currently Lacks

- No OS-backed credential vault for BYOK API keys.
- Hotkey model is still string-oriented and role-limited.
- Hotkey registration failure is visible but not deeply modeled as a recoverable subsystem.
- No robust shortcut collision model across future roles.
- No side-specific modifier support.
- No clear pressed/released edge abstraction independent from UI mode logic.
- No safe rollback when changing hotkeys fails.
- No clipboard restore.
- No Windows insertion strategy choices.
- No streaming insertion into target app.
- No built-in local ASR engine/model manager.
- Ask Anything does not consume selected text as a first-class context.
- Selected-text edit vs selected-text answer is not a strongly separated workflow.
- Scenes are still lighter than OpenLess style packs.
- Permission and diagnostics UI is less complete.

## 4. Product Principles

1. **Trust before breadth**: A dictation app that loses clipboard contents, misses key release, or exposes API keys will lose users faster than it gains them with extra providers.
2. **Hotkeys are infrastructure**: Treat hotkeys like an input runtime with health, roles, collisions, and lifecycle. Do not treat them as strings in settings.
3. **Output must be recoverable**: If insertion fails, the text must be available and the user must know what happened.
4. **Local-first should be honest**: Local ASR should not mean "go read docs and run another server" only. That can remain one path, but not the whole product story.
5. **Command intent must be explicit**: Editing selected text and asking about selected text are different jobs. The app must not guess destructively.
6. **Settings should reflect capability**: Show only what makes sense on the current platform, and explain unavailable states.
7. **Cloud stays additive**: Cloud can be easier; it should not become required for local scenes, BYOK, hotkeys, or local ASR.
8. **Less is more UI**: Learning OpenLess does not mean copying its surface area. Any UI change must preserve OpenTypeless' current quiet, premium, lightweight feel. Prefer progressive disclosure, compact controls, and existing visual language over new pages, heavy cards, dense panels, or explanatory walls.

## 4.1 UI Guardrails

This spec allows deep infrastructure changes, but it should not cause a large visual redesign.

Hard rules:

1. **No big layout reset**
   - Do not redesign the whole Settings app.
   - Do not add a new dashboard-style home just to expose new capabilities.
   - Do not move primary navigation unless a separate design spec approves it.

2. **Use progressive disclosure**
   - Advanced controls should sit behind collapsible sections, inline advanced rows, or small secondary actions.
   - Default users should see simple choices; power users can expand details.
   - Diagnostics should summarize first, then reveal details.

3. **Keep controls minimal**
   - Prefer existing toggles, selects, segmented controls, and compact buttons.
   - Avoid large explanatory cards.
   - Avoid nested cards.
   - Avoid decorative surfaces, hero sections, gradients, and visual noise.

4. **Preserve premium feel**
   - Keep spacing calm and consistent with the existing Settings panes.
   - Keep typography compact inside settings and panels.
   - Use concise labels, not paragraph-length UI copy.
   - Use tooltips or one-line helper text for complex controls.

5. **No feature dumping**
   - Do not expose every OpenLess capability as a visible top-level setting.
   - Hide platform-irrelevant controls.
   - Hide experimental controls unless the user opts into advanced/local/beta sections.

6. **UI acceptance gate**
   - Any implementation touching UI must include a before/after screenshot review.
   - The reviewer should be able to say: "This still feels like OpenTypeless, only more capable."

Implication by area:

- Credential Vault: mostly invisible. Password fields become "saved/replace/clear"; no new large credential manager page.
- Hotkey System: add a compact recorder and status row; avoid a giant shortcut matrix unless placed under advanced.
- Insertion: keep basic mode simple; advanced paste/Windows options collapse.
- Streaming Insert: one toggle in advanced insertion settings.
- Local ASR: show one local section, with model details collapsed.
- Ask/Selected Text: keep the panel light; do not turn it into a chat app.
- Scenes: keep My Scenes/Built-ins/Cloud Packs simple; metadata lives in details.
- Diagnostics: one compact health section with pills and actions, not a monitoring dashboard.

## 5. Personas And Jobs

### Primary Persona: Daily Cross-App Writer

Uses dictation in Slack, email, docs, browser forms, notes, code review comments.

Jobs:

- Press a hotkey and speak without thinking about the app.
- Get polished text into the current cursor.
- Avoid losing clipboard or focus.
- Use hold-to-talk or toggle depending on preference.

Pain today:

- If output fails or appears late, trust drops.
- If hotkey behavior is inconsistent, they stop using it.

### Primary Persona: BYOK Privacy User

Uses own STT/LLM keys, maybe local Whisper or Ollama.

Jobs:

- Keep keys protected.
- Keep history local or disabled.
- Prefer local ASR when possible.

Pain today:

- "Stored locally" is weaker than "stored in system credential vault."
- Local ASR requires external setup.

### Secondary Persona: Selection Power User

Selects text and wants to rewrite, summarize, translate, or ask about it.

Jobs:

- "Rewrite this shorter" replaces selection.
- "What does this mean?" opens answer panel.
- "Translate this to English" returns translated text.

Pain today:

- selected-text context is hidden in settings.
- Ask Anything is separate and does not understand selected text.

### Secondary Persona: Windows/Linux User

Needs reliable hotkeys and insertion despite platform limitations.

Jobs:

- Know when hotkey is installed.
- Choose paste/SendInput style if needed.
- Understand Wayland limitations.

Pain today:

- Platform caveats are not actionable enough.

## 6. Strategic Context

OpenTypeless already has a stronger "open product with cloud option" story than OpenLess. The gap is lower-level reliability. Closing that gap makes existing product investments more valuable:

- Cloud Pro conversion improves if first-run dictation is reliable.
- BYOK story becomes credible if keys are protected.
- Ask/scenes/local ASR become usable if hotkeys and insertion are robust.
- Support burden drops when diagnostics are visible.

OpenLess is useful here because it has already solved several hard desktop app problems:

- global hotkeys across platforms,
- platform-specific input insertion,
- keychain credential storage,
- local ASR model lifecycle,
- selected-text QA flow,
- state-machine races during start/stop,
- settings persistence with side effects and rollback.

## 7. Release Slice Overview

### Release 1: Foundation Trust

Includes:

- Credential Vault
- Hotkey System Upgrade
- Basic insertion layer refactor with clipboard restore

Why first:

- These are prerequisites for safe local ASR, selected-text workflows, and streaming insertion.

### Release 2: Fast And Reliable Output

Includes:

- Windows insertion mode
- streaming insertion MVP
- richer permission/diagnostic panel

### Release 3: Local And Command Workflows

Includes:

- Local ASR MVP
- Selected Text Command Router
- Ask with selected text
- translation mode as a role, not only a setting

### Release 4: Style And Power User Layer

Includes:

- Scenes to Style Packs
- import/export
- history/debug controls
- microphone device UX polish

## 8. Track A: Credential Vault

Priority: P0

### Plain-Language Goal

API keys must not live in the normal settings JSON. Existing users should not lose their keys. The UI should say "saved in system credential storage" only when that is true.

### OpenLess Lessons

OpenLess `persistence/credentials.rs` teaches:

- use OS keyring as normal path;
- treat plaintext JSON only as migration source;
- store a structured v1 JSON payload, not only one key;
- include active ASR/LLM provider in vault snapshot;
- chunk payload for Windows Credential Manager size limits;
- use stable keychain account names to avoid repeated macOS prompts;
- keep process cache to avoid repeated Keychain reads;
- validate extra headers and reject reserved headers;
- delete legacy plaintext after successful verified migration.

### Current OpenTypeless Gap

`AppConfig` contains:

- `stt_api_key`
- `stt_custom_api_key`
- `llm_api_key`

These are serialized through `ConfigManager::save()` into `settings.json`.

### Requirements

#### A1. Data Model

Add a backend credential store with namespaces:

- `stt`
- `llm`
- future `cloud` if needed

Credential entries:

```rust
pub struct CredentialRecord {
    pub provider: String,
    pub secret_kind: String,
    pub value: String,
    pub updated_at: String,
}
```

For practical storage, a single JSON root can be stored in the keychain:

```json
{
  "version": 1,
  "active": {
    "stt": "glm-asr",
    "llm": "openrouter"
  },
  "providers": {
    "stt": {
      "glm-asr": { "apiKey": "..." },
      "custom-whisper": { "apiKey": "..." }
    },
    "llm": {
      "openrouter": { "apiKey": "..." }
    }
  }
}
```

#### A2. Config Migration

On startup:

1. Load `AppConfig`.
2. Detect non-empty legacy secret fields.
3. Write each secret into vault.
4. Read back and verify.
5. Clear secret fields in config.
6. Save config.
7. Mark migration success.

If any vault write or verify fails:

- keep old config intact;
- show a recoverable warning;
- do not silently discard keys.

#### A3. Frontend Contract

Frontend should no longer bind password inputs directly to config secret fields.

Expose:

```ts
type CredentialStatus = {
  namespace: 'stt' | 'llm'
  provider: string
  hasSecret: boolean
  updatedAt: string | null
  storage: 'os-vault' | 'session-only' | 'legacy-warning'
}
```

Commands:

- `get_credential_status(namespace, provider)`
- `set_credential(namespace, provider, value)`
- `clear_credential(namespace, provider)`
- `migrate_legacy_credentials`

#### A4. Provider Runtime

STT/LLM provider creation and connection tests must receive secrets from vault. Do not require frontend to send API key values after initial save.

#### A5. Linux Policy

Linux must not silently fall back to plaintext. If Secret Service is unavailable:

- show `session-only` as default option, or
- offer explicit opt-in insecure local storage with warning.

### Acceptance Criteria

- Existing keys survive migration.
- `settings.json` no longer contains non-empty API keys after successful migration.
- STT test, LLM test, model fetch, live dictation, and live polish work after migration.
- UI can show "Saved" without showing the actual secret.
- Clearing a key disables that provider's BYOK path.
- Unit tests cover migration success, migration failure, empty legacy fields, malformed config, and provider switch.

### Out Of Scope

- Full enterprise secret policy.
- Cloud account token migration unless separately decided.

## 9. Track B: Hotkey System Upgrade

Priority: P0

### Plain-Language Goal

Hotkeys should become a reliable input subsystem. The app should know which shortcut does what, whether it is installed, whether it conflicts, and whether it can work on this platform.

### OpenLess Lessons

OpenLess hotkey system has several pieces worth learning:

- `ShortcutBinding { primary, modifiers }` for user-facing shortcuts.
- `HotkeyTrigger` for legacy/single-modifier shortcuts like right Option or right Ctrl.
- `HotkeyCapability` describes platform support.
- `HotkeyStatus` reports starting/installed/failed.
- `HotkeyMonitor` emits edge events: pressed, released, cancelled.
- `ComboHotkeyMonitor` supports pressed and released for combination keys, enabling hold mode.
- `QaHotkeyMonitor` emits only pressed because QA is a toggle.
- `global_hotkey_runtime` shares one global manager and routes events by hotkey id.
- macOS global-hotkey registration must run on main thread.
- supervisors retry registration and expose status.
- settings persistence detects which hotkey changed and refreshes only affected monitor.
- shortcut recording temporarily suppresses active global hotkeys using `shortcut_recording_active`.
- collision checks reject duplicate role shortcuts before save.
- side-specific modifiers are allowed only where the backend can actually distinguish them.

### Current OpenTypeless Gap

OpenTypeless currently has:

- `hotkey` string, e.g. `Option+/` or `Ctrl+/`
- `ask_hotkey` string
- hold/toggle mode
- Tauri global shortcut plugin callback
- hotkey registration error cache

This is serviceable, but too shallow for:

- selected-text command hotkeys,
- translation hotkey,
- future scene switching,
- robust hold mode,
- platform-specific capabilities,
- user-friendly validation.

### Requirements

#### B1. New Data Model

Add typed hotkey bindings:

```ts
type ShortcutBinding = {
  primary: string
  modifiers: string[]
}

type HotkeyRole =
  | 'dictation'
  | 'ask'
  | 'translate'
  | 'editSelection'
  | 'switchScene'
  | 'openApp'

type HotkeyMode = 'hold' | 'toggle'

type HotkeyConfig = {
  dictation: ShortcutBinding
  ask: ShortcutBinding | null
  translate: ShortcutBinding | null
  editSelection: ShortcutBinding | null
  switchScene: ShortcutBinding | null
  openApp: ShortcutBinding | null
  dictationMode: HotkeyMode
}
```

Rust equivalent:

```rust
pub struct ShortcutBinding {
    pub primary: String,
    pub modifiers: Vec<String>,
}

pub enum HotkeyRole {
    Dictation,
    Ask,
    Translate,
    EditSelection,
    SwitchScene,
    OpenApp,
}

pub enum HotkeyEvent {
    Pressed(HotkeyRole),
    Released(HotkeyRole),
    Cancelled(HotkeyRole),
}
```

#### B2. Migration

Migrate existing:

- `hotkey: string` to `hotkeys.dictation`
- `ask_hotkey: string` to `hotkeys.ask`
- `hotkey_mode` to `hotkeys.dictationMode`

Keep old fields for one release if needed, but new code should read the typed shape.

#### B3. Shortcut Parser

Implement parser/formatter for:

- `Option+/`
- `Command+.`
- `Ctrl+Shift+;`
- `Space`
- `F1` to `F12`
- symbol keys: `.`, `/`, `;`, `,`, `-`, `=`, `[`, `]`

Normalize:

- macOS `cmd`, `option`, `ctrl`, `shift`
- Windows/Linux `ctrl`, `alt`, `shift`, `super`

#### B4. Validation

Validation must reject:

- empty primary;
- unsupported modifiers;
- unsupported primary;
- duplicate modifiers;
- no-op shortcut;
- shortcuts reserved by OS if known;
- shortcuts unsupported by current platform;
- role collisions.

Collision matrix:

- dictation cannot equal ask;
- dictation cannot equal translate;
- dictation cannot equal editSelection;
- ask cannot equal editSelection if both enabled;
- translate cannot equal switchScene/openApp;
- disabled role does not participate.

#### B5. Pressed/Released Edge Handling

Dictation hold mode requires both pressed and released.

Rules:

- Pressed while idle starts session.
- Released while listening stops session.
- Released while starting sets `pending_stop`.
- Pressed while processing is ignored or cancels only if explicit cancel command exists.
- Duplicate pressed events while already held are ignored.

This mirrors OpenLess `SessionState` and `pending_stop` handling.

#### B6. Hotkey Status

Expose:

```ts
type HotkeyStatus = {
  role: HotkeyRole
  adapter: 'tauriGlobalShortcut' | 'nativeHook' | 'unavailable'
  state: 'starting' | 'installed' | 'failed' | 'disabled'
  message: string | null
  lastError: { code: string; message: string } | null
}
```

Settings should show status per active role, or at least dictation/ask.

#### B7. Platform Capability

Expose:

```ts
type HotkeyCapability = {
  platform: 'macos' | 'windows' | 'linux' | 'unknown'
  sessionType: 'x11' | 'wayland' | 'unknown'
  supportsGlobalHotkey: boolean
  supportsHoldMode: boolean
  supportsReleasedEdge: boolean
  supportsSideSpecificModifiers: boolean
  requiresAccessibilityPermission: boolean
  statusHint: string | null
}
```

On Linux Wayland, do not pretend parity.

#### B8. Shortcut Recorder

Build a typed `ShortcutRecorder`:

- focus capture area;
- show current binding as keycaps;
- capture modifiers and primary;
- allow Esc cancel;
- validate before save;
- display backend validation error;
- disable global hotkey handling while recording;
- support disable button for optional roles.

#### B9. Save Flow And Rollback

When user changes a hotkey:

1. Validate locally.
2. Send to backend.
3. Backend validates collisions.
4. Backend attempts register/update.
5. If register succeeds, persist config.
6. If register fails, do not lose old shortcut.
7. UI refreshes from backend truth.

This differs from optimistic config-only updates.

#### B10. Supervisor

Add retry loop for failed installation:

- startup state: `starting`
- retry every 3s for first attempts;
- keep last error;
- stop retry if role disabled;
- wake/retry when settings change.

### User Stories

**Story B1: Configure dictation shortcut**

As a user, I can record a new dictation shortcut and know immediately if it is accepted.

Acceptance:

- recorder captures shortcut;
- backend validates it;
- conflict errors are shown;
- old shortcut remains active if new one fails.

**Story B2: Hold-to-talk never gets stuck**

As a hold-mode user, releasing the shortcut should stop recording even if startup was still in progress.

Acceptance:

- release during starting queues pending stop;
- no stuck recording after quick tap;
- tests cover pressed/released race.

**Story B3: Ask shortcut cannot collide**

As a user, I cannot accidentally assign Ask and Dictation to the same shortcut.

Acceptance:

- UI blocks it;
- backend also blocks it;
- test covers collision.

**Story B4: Understand hotkey health**

As a user, I can see whether the hotkey is installed or failed.

Acceptance:

- settings/permissions page shows installed/starting/failed;
- failure includes actionable message.

### Acceptance Criteria

- Existing `hotkey` and `ask_hotkey` users migrate without changing behavior.
- Dictation works in hold and toggle mode after migration.
- Ask hotkey still works.
- Changing hotkey while app is running updates runtime without restart.
- Hotkey recorder does not trigger real dictation while recording.
- Unit tests cover parser, formatter, collision matrix, migration, and start/stop state.

### Out Of Scope

- Native side-specific modifier hook in first implementation unless Tauri plugin cannot satisfy released-edge requirements.
- Linux fcitx hotkey plugin.
- Less Computer hotkeys.

## 10. Track C: Insertion Layer And Clipboard Safety

Priority: P0

### Plain-Language Goal

When the app outputs text, it should either insert it correctly or leave the text safely available and explain what happened. It should not overwrite the user's clipboard without recovery.

### OpenLess Lessons

OpenLess insertion stack includes:

- `TextInserter` with platform-specific behavior.
- clipboard restore plan with delayed restore.
- paste shortcut selection.
- Windows `SendInput` Unicode path.
- Linux fcitx commit path.
- `InsertStatus` to report inserted/copied/failed.
- streaming keystroke path separated from clipboard insertion.

### Requirements

#### C1. Insertion Strategy

Replace `OutputMode = keyboard | clipboard` with:

```ts
type InsertionStrategy =
  | 'auto'
  | 'keyboard'
  | 'clipboardPaste'
  | 'clipboardCopyOnly'
  | 'windowsSendInput'
```

Keep legacy `output_mode` migration:

- `keyboard` -> `auto`
- `clipboard` -> `clipboardPaste`

#### C2. Clipboard Restore

Add:

```ts
restoreClipboardAfterPaste: boolean
pasteShortcut: 'ctrlV' | 'ctrlShiftV' | 'shiftInsert'
```

Behavior:

- before paste, capture current clipboard text if possible;
- write output text;
- simulate paste;
- after delay, restore original clipboard if it still looks safe to restore;
- if paste fails, leave output text in clipboard.

#### C3. Structured Insert Result

Return:

```ts
type InsertResult = {
  status: 'inserted' | 'copiedFallback' | 'failed' | 'partiallyInserted'
  strategyUsed: InsertionStrategy
  charsInserted: number
  charsCopied: number
  warningCode: string | null
  message: string | null
}
```

#### C4. Windows Strategy

Add Windows settings:

- Auto
- SendInput Unicode
- Clipboard paste

If using SendInput:

- newline mode: `enter`, `shiftEnter`, `crlf`
- fallback to clipboard if zero chars inserted
- report partial insertion if any.

#### C5. Linux Strategy

For now:

- X11 keyboard path remains supported if current implementation works.
- Wayland is copy-only unless a reliable path exists.
- Settings should say this plainly.

#### C6. macOS Strategy

Keep main-thread requirements where needed.

If secure input or accessibility prevents insertion:

- do not claim success;
- copy fallback if possible;
- show warning.

### Acceptance Criteria

- Clipboard restore works on macOS and Windows.
- Clipboard is not restored if paste failed before target received text.
- Insert result is emitted to UI.
- Existing keyboard/clipboard settings migrate.
- Unit tests cover clipboard restore plan, insert result mapping, and fallback decisions.

## 11. Track D: Streaming Insertion

Priority: P1 after Track C

### Plain-Language Goal

After the user stops speaking, text should start appearing as the LLM produces it, not only after the entire response is finished.

### OpenLess Lessons

OpenLess `run_streaming_polish` teaches:

- streaming insertion is an insertion behavior, not just UI display;
- switch macOS input source to ABC to avoid IME interception;
- type deltas through a background task;
- flush chunks every small interval;
- collect typed text for history consistency;
- if zero chars typed, fall back to one-shot insertion;
- if partial chars typed, history should match what user actually saw;
- optional final clipboard save.

### Requirements

#### D1. Config

Add:

```ts
streamingInsert: boolean
streamingInsertSaveClipboard: boolean
```

Default:

- off for first beta, or on only for known-safe provider/platform combinations.

#### D2. Eligibility

Streaming insert is eligible when:

- polish enabled;
- provider supports LLM streaming;
- insertion strategy supports streaming;
- not selected-text replacement in MVP;
- not translation mode requiring non-local reversible conversion;
- app is not in known secure input state.

#### D3. Runtime

Pipeline:

1. STT completes final transcript.
2. LLM polish stream starts.
3. On each delta, send text to insertion worker.
4. Worker coalesces chunks.
5. Worker inserts chunks in order.
6. On finish, report actual inserted text.
7. History stores actual text, not imagined full LLM text if partial failure occurred.

#### D4. Failure Handling

- zero chars inserted -> one-shot fallback with full polished text.
- partial insert -> do not duplicate; mark partial warning.
- LLM stream fails before first delta -> fallback one-shot if possible.
- user cancels -> stop accepting deltas.

### Acceptance Criteria

- Text visibly begins inserting before full LLM response completes.
- No duplicate text after fallback.
- History matches visible inserted text.
- Can disable streaming from settings.
- Tests cover zero-char fallback, partial failure, cancellation, unsupported provider.

## 12. Track E: Local ASR Productization

Priority: P1

### Plain-Language Goal

OpenTypeless should support local speech recognition as a product feature, not only as "bring your own local server."

### OpenLess Lessons

OpenLess local ASR has:

- platform-gated providers;
- Apple Speech on macOS;
- Qwen3 model-managed local ASR;
- Foundry Local Whisper on Windows;
- Sherpa ONNX local on Windows;
- model registry;
- download state;
- mirror selection;
- ready sentinel;
- model delete;
- keep-loaded duration;
- enable confirmation;
- disable escape hatch.

### Current OpenTypeless Gap

OpenTypeless `custom-whisper` is useful but requires users to run another service. It does not provide:

- model download;
- local engine status;
- platform-gated local provider choices;
- local provider test;
- model disk usage;
- safe fallback if local engine fails.

### Requirements

#### E1. Provider Taxonomy

Settings should group STT providers:

- Managed Cloud
- BYOK Cloud Providers
- Local Endpoint
- Built-in Local Engine

#### E2. Phase 1: Local Endpoint Hardening

Improve `custom-whisper`:

- label as `Local / Custom Whisper`;
- endpoint validation;
- sample transcription test;
- auth optional;
- docs link;
- clear errors for unreachable endpoint, bad model, timeout.

#### E3. Phase 2: First Built-In Local Provider

Recommendation:

- macOS first: Apple Speech because it avoids model download complexity;
- keep Qwen3/Whisper model-managed path as next step.

Alternative:

- if Windows user base is priority, Foundry/Sherpa can come first.

#### E4. Local Provider Config

Add:

```ts
localAsrProvider: 'appleSpeech' | 'qwen3' | 'foundryWhisper' | 'sherpaOnnx' | null
localAsrActiveModel: string
localAsrMirror: 'huggingface' | 'hfMirror'
localAsrKeepLoadedSeconds: number
localAsrModelsBaseDir: string
```

#### E5. Model Manager

For model-managed providers:

- list models;
- fetch remote file info;
- show total bytes;
- download with progress;
- cancel download;
- delete model;
- mark ready via sentinel;
- show downloaded bytes;
- choose active model.

#### E6. Safe Provider Switching

When enabling local ASR:

1. Show confirmation: local model may be large/experimental.
2. Check platform support.
3. Check model availability or guide download.
4. Switch active provider.
5. If provider init fails, roll back to previous provider.

### Acceptance Criteria

- At least one built-in local ASR provider can be selected without API key.
- Unsupported platforms do not show unusable toggles as active choices.
- Local provider test works.
- Failed local provider activation rolls back.
- Users can return to previous cloud/BYOK provider.
- Local endpoint remains supported.

## 13. Track F: Selected Text, Ask, And Translation Command Router

Priority: P1

### Plain-Language Goal

When text is selected, the app should understand whether the user wants to edit that text or ask about it.

### OpenLess Lessons

OpenLess has:

- separate selection capture module;
- QA panel state machine;
- QA hotkey toggles panel;
- dictation hotkey inside QA records a question;
- focus target capture so QA window does not steal source app context;
- selected text truncation;
- QA messages history inside panel;
- default QA history off.

### Requirements

#### F1. Command Intents

Add:

```ts
type CommandIntent =
  | 'dictate'
  | 'editSelection'
  | 'askSelection'
  | 'summarizeSelection'
  | 'translateSelection'
  | 'openQuestion'
```

#### F2. Routing Inputs

Router uses:

- selected text exists or not;
- which hotkey triggered flow;
- active panel state;
- transcript command words;
- user default mode.

#### F3. Safe Output Split

Replacement intents:

- editSelection
- translateSelection when user expects replacement

Answer-panel intents:

- askSelection
- summarizeSelection if configured as answer
- openQuestion

Do not insert answer text into target app unless user explicitly requests replacement.

#### F4. Ask With Selection

Ask should accept:

- voice question;
- selected text context;
- optional front app context.

Answer panel should show:

- answer;
- selected text summary or source indicator;
- copy button;
- optional pin.

#### F5. Privacy

Selected text limit:

- default max 4000 chars.
- if truncated, tell user.
- do not save QA history unless setting enabled.

### Acceptance Criteria

- Select text + ask "what does this mean" opens answer panel.
- Select text + ask "make this shorter" replaces selection or outputs replacement.
- Ask without selection still works.
- Selected text capture failure gives clear fallback.
- Prompt tests cover destructive vs non-destructive intent.

## 14. Track G: Scenes To Style Packs

Priority: P1/P2

### Plain-Language Goal

Scenes should become reusable, portable writing behaviors, not only a few prompt templates.

### OpenLess Lessons

OpenLess style packs include:

- metadata;
- examples;
- tags;
- author/version;
- compatibility;
- origin;
- import/export;
- built-ins;
- active style pack id;
- runtime sync with preferences.

### Requirements

#### G1. Schema

Extend scene schema:

```ts
type StylePack = {
  id: string
  name: string
  description: string
  promptTemplate: string
  examples: Array<{ input: string; output: string }>
  tags: string[]
  version: string
  author: string | null
  source: 'builtin' | 'custom' | 'cloud' | 'imported'
  recommendedModel: string | null
  createdAt: string
  updatedAt: string
}
```

#### G2. Built-Ins

Add built-ins:

- Clean Dictation
- Meeting Notes
- Professional Email
- Support Reply
- Technical Explanation
- Code Comment
- Product Spec Notes

#### G3. Import/Export

- export all custom scenes/style packs to JSON;
- import one or many;
- validate size and fields;
- resolve id collisions;
- never overwrite without confirmation.

#### G4. Runtime Diagnostics

For each dictation history entry, store:

- active scene id;
- active scene source;
- active scene name;
- prompt length;
- whether prompt was truncated.

### Acceptance Criteria

- Local scenes work signed out.
- Built-ins are useful immediately.
- Import/export works.
- Scene prompt validation exists.
- Upgrade copy does not imply local self-authored scenes are paid-only.

## 15. Track H: Permissions, Diagnostics, And Settings Side Effects

Priority: P1/P2

### Plain-Language Goal

The app should tell users what is broken and how to fix it.

### OpenLess Lessons

OpenLess permissions page shows:

- microphone permission;
- accessibility permission;
- hotkey status;
- Windows IME status;
- network status;
- platform capabilities;
- system settings shortcuts;
- low-frequency refresh to avoid privacy indicator flicker.

OpenLess settings persistence also:

- compares previous and next prefs;
- refreshes only affected hotkey monitors;
- rolls back side effects if save fails;
- emits `prefs:changed` for other windows.

### Requirements

#### H1. Diagnostics Panel

Add checks:

- microphone;
- accessibility/input automation;
- global hotkey installation;
- clipboard write;
- insertion strategy;
- Linux session type;
- network for cloud/provider tests;
- local ASR model status.

#### H2. Status Types

Use:

```ts
type DiagnosticStatus = 'ok' | 'warning' | 'error' | 'notApplicable' | 'checking'
```

Each row includes:

- status;
- short message;
- action button if available;
- last checked timestamp.

#### H3. Settings Side Effects

For settings that affect runtime:

- hotkeys refresh hotkey runtime;
- microphone setting refreshes tray/menu if any;
- local ASR provider syncs active provider;
- insertion mode updates output runtime;
- UI language refreshes tray labels.

Write these as explicit side effects, not incidental frontend state changes.

#### H4. Save Rollback

If a setting has OS side effects and persistence fails:

- roll back side effect where possible;
- refresh UI from backend truth;
- show failure toast.

### Acceptance Criteria

- User can diagnose why hotkey does not work.
- User can diagnose why text was copied but not inserted.
- Permission checks do not constantly trigger microphone privacy indicator.
- Failed setting save does not leave UI and backend diverged.

## 16. Track I: History, Privacy, And Debug Recording

Priority: P2

### Requirements

Add:

- history retention days;
- history max entries;
- "do not save history";
- QA history separate toggle;
- optional debug audio recording;
- max debug audio entries;
- delete all debug recordings;
- repolish/retry from history if raw text exists.

Defaults:

- history on with current behavior, but retention configurable;
- QA history off by default;
- debug audio off by default.

Acceptance:

- privacy-sensitive user can disable history.
- debug audio is never recorded unless explicitly enabled.
- clear history deletes relevant data.

## 17. Implementation Plan

### M0: Preparation

Deliverables:

- finalize typed config schema;
- decide vault backend;
- decide first local ASR provider;
- decide whether streaming insert is beta/opt-in.

### M1: Credential Vault

Files likely touched:

- `src-tauri/Cargo.toml`
- `src-tauri/src/storage/mod.rs`
- new `src-tauri/src/credentials.rs`
- new `src-tauri/src/commands/credentials.rs`
- `src-tauri/src/commands/stt.rs`
- `src-tauri/src/commands/llm.rs`
- `src-tauri/src/pipeline.rs`
- `src/stores/appStore.ts`
- `src/components/Settings/SttPane.tsx`
- `src/components/Settings/LlmPane.tsx`

Tests:

- migration;
- vault read/write/delete;
- provider runtime secret resolution.

### M2: Hotkey System

Files likely touched:

- `src-tauri/src/hotkey.rs`
- new `src-tauri/src/hotkey/binding.rs`
- new `src-tauri/src/hotkey/runtime.rs`
- `src-tauri/src/commands/misc.rs`
- `src-tauri/src/commands/config.rs`
- `src-tauri/src/lib.rs`
- `src/stores/appStore.ts`
- new `src/components/ShortcutRecorder.tsx`
- `src/components/Settings/GeneralPane.tsx`
- `src/components/Settings/LlmPane.tsx`
- i18n locale files

Tests:

- parse/format shortcuts;
- migration;
- role collisions;
- hold/toggle state transitions;
- save rollback behavior.

### M3: Insertion Layer

Files likely touched:

- `src-tauri/src/output/mod.rs`
- `src-tauri/src/output/clipboard.rs`
- `src-tauri/src/output/keyboard.rs`
- new `src-tauri/src/output/insertion.rs`
- `src-tauri/src/pipeline.rs`
- `src/components/Settings/GeneralPane.tsx`
- `src/stores/appStore.ts`

Tests:

- insert result mapping;
- clipboard restore plan;
- legacy output mode migration.

### M4: Streaming Insertion

Files likely touched:

- `src-tauri/src/pipeline.rs`
- `src-tauri/src/llm/openai.rs`
- `src-tauri/src/llm/cloud.rs`
- `src-tauri/src/output/insertion.rs`
- maybe new `src-tauri/src/output/streaming.rs`

Tests:

- delta aggregation;
- cancellation;
- zero-char fallback;
- partial insert history.

### M5: Local ASR MVP

Files likely touched:

- `src-tauri/src/stt/mod.rs`
- new `src-tauri/src/stt/local/*`
- `src-tauri/src/commands/stt.rs`
- `src-tauri/src/storage/mod.rs`
- `src/components/Settings/SttPane.tsx`
- `src/lib/constants.ts`

Tests:

- provider gating;
- local provider test;
- provider activation rollback.

### M6: Selected Text Router

Files likely touched:

- `src-tauri/src/pipeline.rs`
- `src-tauri/src/commands/ask.rs`
- `src-tauri/src/llm/prompt.rs`
- maybe new `src-tauri/src/commands/selection.rs`
- `src/components/AskPanel/AskPanel.tsx`
- `src/components/Settings/LlmPane.tsx`

Tests:

- intent classification;
- selected text truncation;
- destructive vs non-destructive prompts.

### M7: Scenes/Diagnostics/History

Files likely touched:

- `src/lib/scenes/*`
- `src/components/Settings/ScenesPane.tsx`
- `src-tauri/src/storage/mod.rs`
- `src-tauri/src/commands/history.rs`
- diagnostics commands and settings UI.

## 18. Detailed Acceptance Checklist

### Credential Vault Checklist

- [ ] Existing users keep keys.
- [ ] New keys never enter `settings.json`.
- [ ] Provider tests work without frontend passing key every time.
- [ ] Linux fallback is explicit.
- [ ] Clear key works.

### Hotkey Checklist

- [ ] Dictation hotkey migrates.
- [ ] Ask hotkey migrates.
- [ ] Hold mode uses released edge.
- [ ] Release during starting stops after startup completes.
- [ ] Shortcut recorder validates before save.
- [ ] Hotkey conflicts are rejected.
- [ ] Hotkey install status is visible.
- [ ] Changing hotkey does not require restart.
- [ ] Failed change preserves old working shortcut.

### Insertion Checklist

- [ ] Clipboard restore works.
- [ ] Paste failure leaves text available.
- [ ] Insert result is shown/logged.
- [ ] Windows strategy can be selected.
- [ ] Wayland copy-only is explicit.

### Streaming Checklist

- [ ] Starts inserting before full LLM completion.
- [ ] Disable toggle exists.
- [ ] No duplicate fallback.
- [ ] Partial failure history matches screen.

### Local ASR Checklist

- [ ] Local endpoint has diagnostics.
- [ ] One built-in local provider exists.
- [ ] Provider activation is platform-gated.
- [ ] Model status or OS provider status is visible.
- [ ] Failed local provider does not break cloud/BYOK.

### Selected Text Checklist

- [ ] Edit selection replaces.
- [ ] Ask selection opens answer panel.
- [ ] Ask without selection still works.
- [ ] QA history off by default.
- [ ] Selected text truncation is visible.

## 19. Risks And Mitigations

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Keychain migration fails | Users lose BYOK setup | verify write before clearing plaintext |
| Hotkey plugin cannot provide release edge reliably | Hold mode remains flaky | add native hook only for roles requiring release edge |
| Hotkey change leaves no working shortcut | Activation/support regression | register new first, persist only after success |
| Clipboard restore races user copying new text | User clipboard surprise | restore only if current clipboard still equals inserted text or latest restore token |
| Streaming insert corrupts field | Hard to undo | opt-in beta; no selected-text streaming in MVP |
| Local ASR downloads too heavy | Support and bandwidth burden | start with Apple Speech/local endpoint hardening |
| Selected-text router chooses destructive action wrongly | User text overwritten | conservative routing; answer panel for ambiguity |
| Diagnostics become noisy | Users ignore warnings | show only actionable top-level status |

## 20. Open Questions

1. Should hotkey system use Tauri global shortcut plugin only, or add native hooks for released edge if plugin behavior is insufficient?
2. Should dictation default remain `Option+/` / `Ctrl+/`, or should we introduce OpenLess-like single modifier presets?
3. Should first local ASR be Apple Speech or model-managed Whisper/Qwen?
4. Should streaming insert be off by default until enough dogfood data exists?
5. Should selected-text "summarize" default to answer panel or replacement?
6. Should scenes become fully OpenLess-compatible style packs or only use a similar internal shape?
7. Should Cloud session token move to the credential vault or remain in memory/session storage?

## 21. Recommended Immediate Next Step

Write and implement a narrower spec first:

**Credential Vault + Hotkey System Foundation**

Why:

- It fixes the two highest-risk trust gaps.
- It creates the config/runtime shape needed for insertion, local ASR, and command router.
- It can be tested without adding new provider complexity.

Definition of done for that first implementation:

- no plaintext BYOK keys after migration;
- typed hotkey config exists;
- dictation and Ask hotkeys migrate;
- shortcut recorder validates and saves;
- collision detection exists;
- hotkey health is visible in settings;
- hold/toggle behavior is covered by tests.
