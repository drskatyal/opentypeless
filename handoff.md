# OpenTypeless Desktop Settings Simplification Handoff

Updated: 2026-07-13 19:46:37 CST
Worktree: `/Users/bytedance/.config/superpowers/worktrees/opentypeless/desktop-settings-simplification`
Original checkout to avoid: `/Users/bytedance/个人项目/opentypeless`
Branch: `codex/desktop-settings-simplification` at `a04937a`, based on local `main` at `d188dbf`

## Constraints

- Preserve all uncommitted changes.
- Do not modify the original checkout.
- Do not merge, push, or commit unless explicitly asked.
- Current packaged test app:
  `/Users/bytedance/.config/superpowers/worktrees/opentypeless/desktop-settings-simplification/src-tauri/target/debug/bundle/macos/OpenTypeless.app`

## High-Level Status

The main feature work and review fixes are implemented. The final full frontend/Rust suites, lint, strict Clippy, production build, debug app bundle, and packaged native visual pass are green. This is not yet a signed release recommendation because Windows/Linux real-device smoke tests, macOS signed/notarized package behavior, Gmail Automation permission flow, account password backend flows, and updater end-to-end release flow still need real-environment verification.

## Open GitHub Issue Review

- `#76` Artix/Wayland crash: diagnostics and opt-in launch workarounds are implemented, but the reporter-specific crash is not confirmed fixed.
- `#75` iOS support: not implemented and outside this desktop spec.
- `#73` dictionary import/export: local import/export is implemented; trending-keyword import remains open.
- `#72` local-first paths: Ollama keyless polish/model fetch and Custom Whisper onboarding bugs are fixed in code and tests; real endpoint smoke tests remain.
- `#70` Fn push-to-talk: lower-level macOS Fn support and default binding are implemented; hardware smoke remains.
- `#67` Xiaomi MiMo ASR: not implemented.
- `#28` summary UI: Doubao STT exists, but the requested summary UI remains open.

## User-Visible Work Completed

### GitHub issue #72 and #76 follow-up

- Added spec:
  `docs/superpowers/specs/2026-07-13-local-provider-and-linux-launch-hardening-design.md`.
- Implemented #72 local provider fixes:
  - Added shared LLM key requirement helper.
  - Ollama is treated as keyless consistently.
  - Dictation AI polish no longer skips Ollama just because API key is empty.
  - Ask and dictation now use the same keyless/keyed provider decision.
  - LLM connection and benchmark paths allow keyless providers when base URL/model are configured.
  - Keyless LLM requests omit empty `Authorization: Bearer` headers.
  - AI Polish hides the API-key field for keyless providers and keeps Test beside Base URL.
  - Keyed providers cannot request models until a key is entered.
  - Onboarding no longer silently switches an existing `custom-whisper` setup to a cloud provider.
  - Onboarding Custom Whisper test now uses `stt_custom_api_key`, `stt_custom_base_url`, and `stt_custom_model`.
- Added #76 Linux launch hardening:
  - Logs safe Linux launch diagnostics before WebView creation.
  - Keeps existing NVIDIA + Wayland DMA-BUF workaround.
  - Adds opt-in env switches:
    - `OPENTYPELESS_DISABLE_WEBKIT_DMABUF=1`
    - `OPENTYPELESS_DISABLE_WEBKIT_COMPOSITING=1`
    - `OPENTYPELESS_FORCE_GDK_X11=1`
    - `OPENTYPELESS_FORCE_SOFTWARE_GL=1`
  - Does not claim Artix/AMD crash fixed; needs reporter logs/backtrace/strace.

### Settings simplification

- General settings now keeps hotkeys and dictation mode as separate compact sections.
- Low-frequency settings remain behind the existing `More` disclosure.
- macOS Accessibility grant flow no longer leaves stale Fn registration errors.
- Top Accessibility banner, General pane recovery, and onboarding permission grant now call `resumeHotkey()` after permission is granted.
- AI Polish and Speech Recognition Pro CTAs use an upgrade button and consistent card density.
- `common.upgrade` key usage was removed in favor of `nav.upgrade`.
- Config dirty-state comparison is semantic and key-order independent, so a cold backend load no longer shows a false `You have unsaved changes` bar.
- Scenes and Dictionary switch to stacked controls at the app's narrow desktop size while retaining compact horizontal rows at the normal window size.

### Scenes / App Writing Modes

- Old `SceneAssignmentsDialog` UI was removed.
- Scenes now centers on `App Writing Modes` by app family.
- Each family shows representative app logos, for example Gmail, Apple Mail, Slack, Lark, Google Docs, Notion, GitHub, Cursor, Zendesk, X, and LinkedIn.
- Default modes are concrete names such as Email Format, Work Chat, Document Notes, etc.; no more `Automatic` as the user-facing choice.
- Built-in default system scenes appear in My Scenes as editable/resettable defaults.
- Custom scenes remain creatable and assignable to app families.
- Global Scene activation is no longer exposed in the new UI.
- Exact-app override UI now follows default app writing mode or selects user custom scenes only; it no longer offers old built-in scene/family assignment choices.

### Prompt and scene behavior

- Email family prompt now asks for an email body, greeting when recipient is spoken, clear paragraphs, and light closing when appropriate.
- Work chat, personal chat, document, project management, developer collaboration, prompt/code, support, and social family rules have concise formatting contracts.
- Built-in Professional Email prompt now explicitly produces an email body.
- When a mapped/default/custom/active scene prompt exists, `[BUILTIN_POLISH_STYLE]` is skipped so Polish Style does not stack with scene-owned output shape.
- General/no-scene dictation still keeps Polish Style fallback.
- Chinese email draft intent detection was expanded for natural phrases like writing an email to Sara while retaining guarded negative/discussion cases.

### Permissions and Browser Access

- Added compact onboarding `Permissions` step.
- It shows only relevant rows:
  - Microphone
  - Text output
  - Browser apps
  - Apple Speech only when selected
- No Settings permission health/checklist section was added, per user request.
- Added `BrowserAccessStatus` through app context and history:
  - `available`
  - `needs_permission`
  - `not_applicable`
  - `unknown`
- Passive polling uses the non-prompting macOS Automation preflight API.
- Only macOS TCC denials (`-1743` / `-1744`) become `needs_permission`; unrelated AppleScript failures remain `unknown`.
- The top banner exposes a compact `Allow Browser Access` action only when access is actually needed.
- Clicking that action requests Automation access for the exact safe browser target; polling never raises a surprise system prompt.
- Raw URLs are not stored or displayed.
- AI Polish shows a short Browser Access hint only when context adaptation is enabled, latest context is `general.browser`, and URL read failed.
- History shows `Browser · needs browser access` only for that specific URL-read-failure fallback.

### Onboarding and language

- Onboarding includes App Writing Modes copy and the new compact Permissions step.
- Ask standalone webview now loads config on startup and applies `config.ui_language`, preventing English UI from showing Chinese Ask copy.
- All new strings were added across 10 locale files.
- Locale key parity verified with 725 keys.

### Capsule, Ask, and History fixes

- Ask recording and Ask thinking capsule titles use `whitespace-nowrap`, preventing Chinese `问答` from breaking into two vertical characters.
- macOS History clear no longer uses `window.confirm`; it now uses an in-app minimal confirmation block.
- Existing History feature still supports clearing all history. There is still no single-entry delete feature.

### Cross-platform default shortcuts

Current defaults:

- macOS:
  - Dictation: `Fn`
  - Ask: `Fn+Space`
  - Translate: `Fn+LeftShift`
  - Mode: `toggle`
- Windows:
  - Dictation: `Ctrl+/`
  - Ask: `Ctrl+.`
  - Translate: `Ctrl+Shift+/`
  - Mode: `hold`
- Linux:
  - Dictation: `Ctrl+/`
  - Ask: `Ctrl+.`
  - Translate: `Ctrl+Shift+/`
  - Mode: `hold`

RightAlt parsing/native support remains in lower-level code, but it is no longer the Windows default or recommended special chip.

### Account password flow review

- Account page security section shows:
  - `Change password` if credential account exists.
  - `Set password` for OAuth-only accounts.
  - No action while capability is unknown.
- Forgot password remains in signed-out auth flow.
- Password dialog has focused modal behavior and focus trap tests.
- Store logic covers:
  - Password reset request with locale.
  - Password change with token rotation.
  - OAuth-only set password.
  - Verification required before set password if account is unverified.
- This flow still requires real backend/email verification before release.

### Cloud backup and restore

- Device-local API keys and exact-app matcher material remain excluded from cloud backup/restore.
- `system_scene_overrides` is included so edited default writing modes survive cross-device restore.
- Restored settings are allow-list merged, persisted through Tauri, then reloaded from backend truth.
- History, dictionary entries, and correction rules are validated and restored into SQLite, not only the current Zustand session.
- History/dictionary/correction replacement uses one SQLite transaction and ignores incoming database IDs.
- Dictionary backups use the existing cloud `dictionary` slot as `{ entries, correction_rules }`; legacy array-only backups remain supported and preserve local correction rules.

## Important Files Changed

- `src/components/Settings/ScenesPane.tsx`
- `src/components/Settings/GeneralPane.tsx`
- `src/components/Settings/LlmPane.tsx`
- `src/components/Settings/SttPane.tsx`
- `src/components/Settings/AppStyleMappingDialog.tsx`
- `src/components/Settings/shared/useDirtyConfig.ts`
- `src/components/toast-service.ts`
- `src/components/Settings/SceneAssignmentsDialog.tsx` deleted
- `src/components/Onboarding/PermissionsStep.tsx` added
- `src/components/MainLayout/AccessibilityBanner.tsx`
- `src/components/Capsule/CapsuleAskRecording.tsx`
- `src/components/Capsule/CapsuleAskThinking.tsx`
- `src/components/History/index.tsx`
- `src/components/History/AppContextMeta.tsx`
- `src/App.tsx`
- `src/stores/appStore.ts`
- `src-tauri/src/storage/mod.rs`
- `src-tauri/src/commands/backup.rs`
- `src-tauri/src/app_detector/cache.rs`
- `src-tauri/src/app_detector/platform/macos.rs`
- `src-tauri/src/app_detector/types.rs`
- `src-tauri/src/llm/context_policy.rs`
- `src-tauri/src/llm/prompt.rs`
- `src-tauri/src/voice_intent/grammar/zh_hans.rs`
- `src/i18n/locales/*.json`
- `src/components/Onboarding/SttSetupStep.tsx`
- `src/components/Onboarding/__tests__/SttSetupStep.test.tsx`
- `src-tauri/src/commands/llm.rs`
- `src-tauri/src/commands/ask.rs`
- `src-tauri/src/llm/openai.rs`
- `src-tauri/src/lib.rs`
- `src/lib/backup-settings.ts`
- `src/components/AccountPage/index.tsx`
- `docs/superpowers/specs/2026-07-13-local-provider-and-linux-launch-hardening-design.md`

## Verification Completed

Final verification after the review fixes:

- `npm test -- --reporter=dot`
  - Passed: 41 files, 373 tests.
  - Notes: expected test-log noise from simulated vault failures, HTTP 429, update install failure, and existing framer-motion DOM prop mocks.
- `cargo test --manifest-path src-tauri/Cargo.toml`
  - Passed: 456 tests.
- `npm run lint`
  - Passed with zero warnings.
- `npm run format:check`
  - Passed after applying the repository's Prettier configuration to `src/`.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
  - Passed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
  - Passed.
- `npm run build`
  - Passed.
  - Notes: existing Vite dynamic import/chunk-size warnings remain.
- Locale key parity script
  - Passed: 725 leaf keys in each of 10 locale files.
- `git diff --check`
  - Passed.
- `npm run tauri -- build --debug --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'`
  - Passed.
  - Latest app path: `src-tauri/target/debug/bundle/macos/OpenTypeless.app`
- Packaged macOS native QA through Computer Use:
  - Passed cold-start Settings check: no false dirty bar.
  - Passed General, AI Polish, Scenes, Dictionary, Account, and Forgot password review at `900x700` and the app's narrow `720x480` size.
  - Confirmed app logos, stacked narrow layouts, compact normal layouts, and no observed text overlap or clipping.

## Release Readiness

- Proposed next version: `v1.1.49` (the tag is unused in both GitHub repositories).
- macOS and Linux signing/notarization secrets are configured in the CI repository.
- The Windows SignPath workflow builds and signs both MSI and NSIS packages, but the
  configured policy currently issues the SignPath OSS test certificate. It is useful
  for a dry run only and is not a publicly trusted production signature.
- The Windows workflow now rejects `publish_release=true` whenever the SignPath policy
  slug starts with `test-` or `test_`, and it requires Authenticode status `Valid` for
  production publication. This prevents a test-signed installer from being attached
  to a stable GitHub Release by mistake.
- A complete signed `v1.1.49` release remains blocked until the CI repository is given
  a production SignPath signing policy/certificate (or the standard Windows workflow
  receives a production PFX certificate and password).
- The owner repository's Actions jobs currently fail before executing any steps, so
  CI and signing validation must run from `toverwu-qaq/opentypeless`; release assets
  are still published to `tover0314-w/opentypeless` after all signing gates pass.

## Known Residual Risks / Not Fully Verified

- Gmail Browser Access / macOS Automation:
  - Non-prompting detection and explicit user-triggered grant paths exist and status is recorded.
  - Still needs real Chrome/Gmail test to confirm the system prompt and post-grant `Gmail / Email` classification.
- Windows:
  - Defaults restored to Ctrl shortcuts and covered in frontend platform tests.
  - Needs real Windows smoke test for global shortcut behavior and translation shortcut `Ctrl+Shift+/`.
  - A macOS-hosted Windows cross-target check was attempted but stopped inside `ring`'s C build because the MSVC SDK header `assert.h` is unavailable; this does not replace a Windows build.
- Linux:
  - Defaults set and Wayland limitations exist in code.
  - Needs real X11/Wayland smoke test.
- Issue #76:
  - Diagnostics and opt-in workarounds are implemented.
  - The Artix/AMD crash still requires reporter confirmation and native backtrace/strace.
- Issue #72:
  - Code/test fixes are implemented for keyless LLM and Custom Whisper onboarding.
  - A real Ollama smoke test and a real Custom Whisper endpoint smoke test are still recommended before closing.
- Signed macOS release:
  - Only debug app bundle verified.
  - Signed/notarized `/Applications` install and TCC behavior still need verification.
- Updater:
  - Plugin/config exist and prompt component has tests.
  - Full manifest/signature/download/install/restart flow not verified in this session.
- Account password:
  - UI/store tests pass and code path is reviewed.
  - Real backend Set password / Change password / Forgot password email flow still needs test accounts.
- History:
  - Clear all is fixed with in-app confirmation.
  - Single-entry delete is not implemented.
- Cloud restore:
  - Local transactional persistence is covered by tests; a real Pro account upload/download round trip against TalkMore still needs verification.
- Some older locale strings outside this feature may still be English fallback quality, even though key parity passes.

## Suggested QA Checklist

1. macOS Fn dictation:
   - Grant Accessibility from banner/onboarding.
   - Confirm Fn triggers capsule without restarting.
2. Ask:
   - English UI shows Ask, not Chinese `问答`.
   - Ask recording/thinking capsule does not wrap title.
3. Gmail:
   - Use Chrome/Gmail compose.
   - Confirm the in-app Browser Access action appears when needed and the macOS prompt appears only after clicking it.
   - After grant, History should show Gmail/Email.
   - Output should be email body format.
4. Scenes:
   - Review App Writing Modes rows and logos.
   - Edit/reset Email Format.
   - Assign custom scene to a family.
5. History:
   - Clear all history on macOS using in-app confirmation.
6. Account:
   - OAuth-only Set password.
   - Credential Change password.
   - Forgot password email.
7. Windows:
   - Dictation `Ctrl+/`.
   - Ask `Ctrl+.`
   - Translate `Ctrl+Shift+/`.
8. Linux:
   - Dictation `Ctrl+/`.
   - Ask `Ctrl+.`
   - Translate `Ctrl+Shift+/`.
   - Confirm Wayland copy/paste behavior.
9. Release:
   - Build signed macOS app.
   - Verify updater end-to-end.

## Notes For Next Agent

- Worktree is intentionally dirty and broad. Do not revert unrelated files.
- `handoff.md` itself is untracked and should remain as the handoff artifact unless the user asks otherwise.
- The user has explicitly authorized commit, push, CI/CD, signed release publication,
  and evidence-based issue replies/closures for this release task.
- If continuing QA, start from the latest debug package path above.
