# Typeless Competitive Roadmap Spec

Date: 2026-06-27
Repo: `tover0314-w/opentypeless`
Authoring context: requested after `v0.1.41` release; local desktop quota copy changes are in progress.

## 1. Executive Summary

We are building OpenTypeless from a reliable cross-platform AI dictation tool into a polished voice-writing assistant that can compete with Typeless on everyday workflows. The near-term goal is not to clone every Typeless surface. The goal is to close the highest-value gaps: clearer words-based subscription UX, stronger dictation polish, command-based selected-text editing, Ask Anything workflows, app-specific writing behavior, and lightweight personalization while preserving OpenTypeless' open-source, BYOK, and privacy-first positioning.

The recommended path is:

1. **M0: Words quota and clarity cleanup**: make subscription UX use `words/month` rather than STT hours or LLM tokens, and show subscribed users remaining cloud words on the home page.
2. **M1: Dictation parity**: improve first-run experience, home copy, language variants, recording limits, and polish quality evaluation.
3. **M2: Voice commands and selected-text editing**: promote current selected-text context into a first-class "Speak to edit" mode with predictable command behavior.
4. **M3: Ask Anything**: add open questions, selected-text Q&A, optional search actions, and an answer panel without forcing app switching.
5. **M4: Personalization**: add user style profiles, per-app style rules, auto dictionary suggestions, and correction learning.
6. **M5: Teams/security/commercial polish**: only after core workflows feel trustworthy.

## 2. Source Research Summary

Sources checked on 2026-06-27:

- Typeless homepage: https://www.typeless.com/
- Typeless pricing: https://www.typeless.com/pricing
- Typeless key features help page: https://www.typeless.com/help/quickstart/key-features
- Typeless Ask Anything page: https://www.typeless.com/ask-anything
- Typeless downloads page: https://www.typeless.com/downloads
- Typeless macOS Voice Superpowers release note: https://www.typeless.com/help/release-notes/macos/voice-superpowers
- Typeless macOS Translation mode release note: https://www.typeless.com/help/release-notes/macos/translation-mode
- Typeless macOS personalization release note: https://www.typeless.com/help/release-notes/macos/personalized-smarter
- Typeless language variants release note: https://www.typeless.com/help/release-notes/macos/more-language-variants-supported
- Typeless dictation limit troubleshooting: https://www.typeless.com/help/troubleshooting/dictation-limit

Observed Typeless positioning:

- "Speak naturally" becomes polished messages, emails, and documents in real time.
- Speed claim is framed as roughly `220 wpm` voice keyboard vs `45 wpm` QWERTY typing.
- Works across Mac, Windows, iOS, and Android.
- Core dictation claims: removes filler words, removes repetition, handles mid-sentence self-corrections, auto-formats lists and steps, supports 100+ languages, translates as you speak.
- Personalization claims: personalized style and tone, personal dictionary, different tones for each app.
- Ask Anything claims: select text and speak commands to rewrite, summarize, translate, explain, search, draft, or answer in a pop-up panel.
- Pricing is words-based:
  - Free: `8,000 words/week`.
  - Pro: unlimited words, enhanced accuracy, priority access, team management, prioritized requests, early access.
- Security messaging: zero cloud data retention, never trained on user data, on-device history storage; pricing page also advertises compliance/security items such as HIPAA, GDPR, and ISO 27001.

## 3. Current OpenTypeless Baseline

Existing strengths in this repo:

- Cross-platform desktop app via Tauri for macOS, Windows, and Linux.
- BYOK first, plus optional Cloud Pro quota.
- Global hotkey, tray, floating capsule, history, i18n, onboarding.
- Multiple STT providers: Deepgram, AssemblyAI, Volcengine/Doubao, GLM-ASR, Whisper-compatible providers, cloud proxy.
- Multiple LLM providers and cloud LLM.
- LLM polish prompt already removes fillers, repetitions, false starts, formats lists, preserves language, supports dictionary terms, translation, selected-text mode, and app type add-ons.
- Foreground app detection exists on macOS and Windows and maps apps into broad types: email, chat, code, document, general.
- Personal dictionary exists with word and pronunciation fields.
- Selected text can be captured through clipboard sentinel flow and passed to the LLM.
- Cloud subscription status already exposes:
  - `cloudWordsUsed`
  - `cloudWordsLimit`
  - `cloudWordsResetAt`
  - legacy STT seconds and LLM tokens fields.

Key gaps vs Typeless:

- Product copy still leaks technical quota units in some surfaces: STT hours and LLM tokens.
- Home page currently emphasizes recordings and used quota, not "words remaining."
- Selected-text mode is hidden inside settings; it is not a clear user-facing workflow.
- No distinct Ask Anything answer panel for open questions.
- No command intent router; the prompt handles many cases but all modes share one pipeline.
- App-specific behavior is broad by type, not per app, per domain, or user-configurable.
- Personalization is manual custom prompt only; no learned style profile or correction loop.
- Personal dictionary is manual; no automatic suggestions from repeated corrections or failed terms.
- Translation exists, but not as a dedicated mode with its own shortcut and first-class mental model.
- No built-in quality evaluation suite for "sounds typed, not transcribed" outputs.
- Linux support is a differentiator vs Typeless, but selected-text and app-context parity will be harder on Linux desktop environments.

## 4. Product Principles

1. **Words over provider units**: users should see words/month, words remaining, and reset date. STT seconds and LLM tokens are implementation details.
2. **Voice in place**: the product should work where the cursor or selected text already is. Avoid sending users to a separate editor unless the task needs an answer panel.
3. **Command predictability**: "dictate," "rewrite selection," "ask about selection," and "open question/search" should be separable internally even if they use one shortcut.
4. **Privacy-first personalization**: style learning and dictionary suggestions should default to local storage and be explainable.
5. **BYOK remains a feature, not a fallback**: Cloud Pro can be easier, but BYOK should stay credible and visible.
6. **Cross-platform honesty**: if a workflow is weaker on Linux/Wayland, show it clearly rather than pretending parity.

## 5. User Personas

### Primary: Flow Writer

Role: founder, PM, engineer, student, creator, or knowledge worker who writes all day.

Jobs:

- Dictate Slack/email/docs/prompts faster than typing.
- Say messy thoughts and receive clean, sendable text.
- Rewrite selected text without copy/paste.
- Keep context inside the current app.

Pain:

- Raw dictation is awkward, unpunctuated, and too literal.
- ChatGPT-style workflows require copy/paste and tab switching.
- Subscription quotas are confusing when shown as minutes/tokens.

### Secondary: Multilingual Operator

Jobs:

- Speak in one language and output in another.
- Mix languages naturally.
- Preserve names, technical terms, product terms, and regional spelling.

Pain:

- Traditional STT breaks on mixed-language terms and proper nouns.
- Translation often sounds translated rather than native.

### Secondary: Privacy-Conscious Power User

Jobs:

- Use own API keys or local services.
- Keep history and personalization local.
- Understand what cloud mode sends and retains.

Pain:

- Commercial dictation tools are opaque about data handling.
- Cloud-only tools create lock-in and pricing anxiety.

## 6. Success Metrics

Primary metrics:

- Activation: % of new users completing first successful dictation within 10 minutes.
- Repeated value: % of active users with 5+ dictations in first 7 days.
- Output acceptance: % of sessions not cancelled, retried, or manually edited immediately after output. First implementation can use proxy metrics: repeat same field within 30 seconds, manual copy fallback, or explicit thumbs-up/down later.
- Cloud clarity: % of signed-in cloud users who can identify remaining words and reset date without opening Account.

Secondary metrics:

- Selected-text command usage per weekly active user.
- Dictionary terms added or accepted.
- Translation mode usage.
- Average time from hotkey release to final output.
- Quota exhaustion support tickets.

Guardrails:

- No regression in local BYOK setup.
- No new mandatory cloud account for core BYOK flows.
- No unexpected storage of selected text or voice beyond existing history settings.
- No cross-platform release breakage for macOS, Windows, Linux.

## 7. Scope And Phasing

## M0: Words Quota And Clarity Cleanup

Status: partially implemented in current local work.

### Problem

Current desktop copy mixes user-facing quota concepts:

- `10h/month` for STT.
- `~5M tokens/month` for LLM.
- Cloud words fields exist but are not the main home-page subscription mental model.

This makes OpenTypeless feel less mature than Typeless, whose pricing and product surfaces use words.

### Requirements

- Home page for signed-in subscribed users with `cloudWordsLimit > 0` must show:
  - plan name
  - words remaining as the primary number
  - used/limit progress bar
  - reset date if available
- Upgrade/settings copy must avoid STT hours or LLM tokens for Cloud Pro marketing.
- Copy should use `words/month` or `words/m` once exact plan limit is available.
- Account and Upgrade can keep detailed progress bars, but labels should prioritize cloud words when `cloudWordsLimit > 0`.

### Implementation Notes

Current code paths:

- `src/lib/api.ts`: `SubscriptionStatus` already has `cloudWordsUsed`, `cloudWordsLimit`, `cloudWordsResetAt`.
- `src/stores/authStore.ts`: state already stores cloud words.
- `src/components/HomePage/index.tsx`: add a `RemainingWords` block before quota bar.
- `src/i18n/locales/*.json`: add `home.wordsRemaining` and `home.wordsReset`; update Pro copy.

Future server/API improvement:

- Add plan metadata endpoint or extend `/api/subscription/status`:

```ts
type SubscriptionStatus = {
  cloudWordsUsed: number
  cloudWordsLimit: number
  cloudWordsResetAt: string | null
  planCloudWordsLimit?: number
  planBillingPeriod?: 'week' | 'month' | 'lifetime'
}
```

This allows upgrade screens for non-subscribed users to say exact values such as `200,000 words/m` instead of generic `cloud words/month`.

### Acceptance Criteria

- Pro/AppSumo users see remaining cloud words on Home without opening Account.
- Remaining is clamped to `0` if usage exceeds limit.
- BYOK/free users do not see a misleading paid quota card.
- No locale displays `10h/month`, `~5M tokens/month`, or equivalent technical quota copy for Pro marketing surfaces.
- Existing API tests and build pass.

## M1: Dictation Parity Foundation

### Problem

OpenTypeless already cleans transcripts, but the product needs a sharper promise: "speak naturally; get text that reads typed." Typeless wins by making this promise visible and consistent.

### Requirements

1. **Home/onboarding value proposition**
   - Replace vague onboarding/home text with a speed and outcome-oriented message.
   - Suggested copy:
     - English: `Speak naturally. OpenTypeless turns your voice into polished text across apps.`
     - Optional speed label: `Up to 4x faster than typing` only if we are comfortable with the claim.
   - Do not overclaim exact `wpm` unless measured in our app.

2. **Polish quality evaluation suite**
   - Add fixture-based tests for:
     - filler removal
     - repetition removal
     - self-correction
     - numbered lists
     - bullet lists
     - mixed Chinese/English
     - emails vs chat vs documents
     - selected-text instructions
   - Store fixtures in `src-tauri/src/llm` or `tests/fixtures/polish`.

3. **Language variants**
   - Extend `LANGUAGES` or STT config with variants:
     - English US/UK/AU
     - Spanish ES/MX
     - Portuguese BR/PT
     - Chinese simplified/traditional output preference only if it maps to provider or prompt safely.
   - Avoid forcing script conversion unless explicitly selected.

4. **Recording limit UX**
   - Current app has max recording duration in settings. Make remaining time visible only near the end.
   - If limit is reached, save partial transcript to history and show a clear status.

### Acceptance Criteria

- Users understand the first-run promise in under 10 seconds.
- Polish prompt test suite catches regressions.
- Language variant selection does not break existing `multi` auto-detect.
- Recording timeouts never silently lose transcript content.

## M2: Speak To Edit Selected Text

### Problem

The backend has selected-text capture and prompt support, but users do not perceive it as a core feature. Typeless treats selected-text editing as a hero workflow.

### Solution

Make selected-text editing a first-class mode:

- If text is selected and selected-text context is enabled, OpenTypeless interprets the voice input as a command.
- Common commands:
  - rewrite shorter/longer
  - make more formal/casual
  - fix grammar/spelling
  - translate selection
  - turn into bullet list
  - summarize
  - explain
- For edit commands, replace selected text in place.
- For read-only/ask commands, show answer panel instead of attempting replacement.

### Technical Design

Add an intent layer before LLM prompt selection:

```ts
type VoiceMode =
  | 'dictate'
  | 'translate'
  | 'edit_selection'
  | 'ask_selection'
  | 'ask_anything'
  | 'search'

type CommandIntent = {
  mode: VoiceMode
  confidence: number
  selectedTextPresent: boolean
  shouldReplaceSelection: boolean
  targetLanguage?: string
  action?: 'rewrite' | 'summarize' | 'explain' | 'translate' | 'draft' | 'search'
}
```

MVP can classify with deterministic heuristics in Rust or TypeScript:

- selected text + verbs like `rewrite`, `make`, `shorten`, `fix`, `translate` -> `edit_selection`
- selected text + `summarize`, `explain`, `what does this mean` -> `ask_selection`
- no selected text + `search ... on Google/YouTube/Amazon/GitHub` -> `search`
- no selected text + open question -> `ask_anything`
- default -> `dictate`

Later versions can use a small LLM classifier, but deterministic rules are safer for v1.

### UI

- Settings > AI Polish:
  - `Selected text commands`
  - `Replace selected text when command edits it`
  - `Show answer panel for read-only answers`
- Capsule state:
  - selected text detected: show a small "Selection" hint.
  - open answer: show "Answer ready" or open answer panel.
- History:
  - store mode and app name.
  - avoid storing selected text by default; store output and command only unless user enables full context history.

### Acceptance Criteria

- Selecting text and saying "make this shorter" replaces selection with shorter text.
- Selecting text and saying "summarize this" opens answer panel or inserts summary based on explicit setting.
- Clipboard is restored after selected-text capture.
- Prompt injection in selected text remains ignored.
- Works on macOS and Windows. Linux support documented as best effort.

## M3: Ask Anything

### Problem

Typeless expands beyond dictation into a system-level voice AI assistant. OpenTypeless can compete by adding a privacy-conscious Ask Anything mode that works with current app context.

### MVP Scope

Ask Anything v1 supports:

- Open questions by voice.
- Selected-text Q&A.
- Drafting into active field.
- Optional web search actions for explicit search commands.

Out of scope for v1:

- Autonomous multi-step browsing.
- Background agents.
- Deep integrations with Gmail, Slack, Notion APIs.
- Mobile parity.

### UX

Two output types:

1. **In-place output**
   - For draft/rewrite/translate commands.
   - The result goes into the current field or replaces selection.

2. **Answer panel**
   - For "explain this," "what does this mean," "brainstorm ideas," "what is..." style answers.
   - The panel should be compact, dismissible, copyable, and optionally insert answer into active field.

### Search Actions

Search should be explicit:

- `search React tutorials on YouTube`
- `search standing desk on Amazon`
- `search this error on Google`

Implementation:

- Parse target provider from command.
- Open URL through Tauri opener.
- Do not silently browse arbitrary pages or send selected text to search unless command asks for it.

### Backend/API

For Cloud mode, Ask Anything likely needs a managed LLM endpoint and optional search endpoint.

BYOK mode:

- Open questions can use configured LLM provider.
- Search URL actions do not require LLM if deterministic.
- Live web answer synthesis should be disabled unless user configures a provider/tool that supports it.

### Acceptance Criteria

- User can ask an open question from any app and see a pop-up answer.
- User can ask about highlighted text without switching tabs.
- User can insert or copy an answer.
- Search actions open correct URLs.
- Cloud/BYOK capability differences are visible and non-confusing.

## M4: Personalization And Per-App Tone

### Problem

OpenTypeless currently has:

- manual dictionary
- custom polish prompt
- coarse app type add-ons

Typeless' stronger promise is that output sounds like the user and changes tone by app.

### Solution

Add layered style control:

1. **Global writing preference**
   - Existing custom polish prompt becomes the visible "Default style."
   - Examples: concise, direct, friendly, professional, no emojis, preserve Markdown.

2. **Per-app style profiles**
   - Rule-based profiles by app name and optional window title/domain.
   - Examples:
     - Slack: casual and concise.
     - Gmail/Outlook: professional and complete.
     - VS Code/Cursor: preserve code, prompts, logs, technical terms.
     - Notion/Docs: structured paragraphs and Markdown.

3. **Personal dictionary suggestions**
   - Suggest terms from repeated user edits, corrected output, or low-confidence transcript segments.
   - User must accept suggestions before they enter dictionary.

4. **Style learning**
   - Optional local profile derived from accepted outputs and corrections.
   - Store locally by default.
   - Cloud sync only if explicitly enabled.

### Data Model

SQLite additions:

```sql
CREATE TABLE app_style_profiles (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  app_matcher TEXT NOT NULL,
  window_matcher TEXT,
  tone TEXT NOT NULL DEFAULT 'auto',
  output_format TEXT NOT NULL DEFAULT 'auto',
  custom_prompt TEXT NOT NULL DEFAULT '',
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE dictionary_suggestions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  word TEXT NOT NULL,
  pronunciation TEXT,
  source TEXT NOT NULL,
  evidence_count INTEGER NOT NULL DEFAULT 1,
  status TEXT NOT NULL DEFAULT 'pending',
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

Config additions:

```ts
type PersonalizationConfig = {
  styleLearningEnabled: boolean
  autoSuggestDictionary: boolean
  includeAppContext: boolean
  cloudSyncPersonalization: boolean
}
```

### Acceptance Criteria

- User can create a Slack-specific style and see it affect Slack output only.
- Existing app type behavior still works when no custom profile matches.
- Dictionary suggestions require explicit user acceptance.
- Turning off style learning stops new learning and removes learned profile from prompts.

## M5: Commercial And Team Features

Do not prioritize until M1-M4 feel good.

Potential scope:

- Team management and seats.
- Team dictionary.
- Team style templates.
- Admin retention controls.
- Audit-friendly privacy documentation.
- Enterprise deployment docs.

OpenTypeless differentiator should remain:

- open source
- BYOK
- Linux support
- transparent local storage
- provider choice

## 8. Competitive Gap Matrix

| Capability | Typeless observed | OpenTypeless today | Recommended priority |
| --- | --- | --- | --- |
| Polished dictation | Strong hero claim; filler/repetition/self-correction/formatting | Strong prompt foundation | P0: eval suite and copy |
| Words quota UX | Free words/week, Pro unlimited words | Cloud words API exists; UI mixed with hours/tokens | P0: words-first UI |
| Cross-app desktop | Mac/Windows | Mac/Windows/Linux | Keep; Linux as differentiator |
| Mobile | iOS/Android | None | Later/out of scope |
| Personal dictionary | manual and automatic claims | manual dictionary | P2: suggestions |
| Per-app tone | explicit feature | coarse app type prompt add-ons | P2 |
| Selected-text editing | first-class | backend support, hidden UX | P1 |
| Ask Anything | hero product | selected-text prompt can be extended | P1/P2 |
| Web/search actions | advertised | none | P2 with explicit commands |
| Translation mode | dedicated shortcut | supported in settings, not as mode | P1 |
| Personalization | learns style | manual custom prompt | P2/P3 |
| Security posture | zero retention, compliance claims | privacy-first/BYOK/open source | P1 docs/UX clarity |

## 9. Engineering Milestones

### Milestone 0.1.42: Words UX Patch

Files:

- `src/components/HomePage/index.tsx`
- `src/i18n/locales/*.json`
- optional tests for home quota rendering.

Definition of done:

- Home shows remaining words for subscribed cloud users.
- Pro copy uses words/month vocabulary.
- Build and relevant tests pass.

### Milestone 0.2.0: Dictation Quality And Mode Clarity

Files likely touched:

- `src-tauri/src/llm/prompt.rs`
- `src-tauri/src/pipeline.rs`
- `src-tauri/src/storage/mod.rs`
- `src/components/Onboarding/*`
- `src/components/HomePage/index.tsx`
- `src/components/Settings/LlmPane.tsx`
- `src/i18n/locales/*.json`

Deliverables:

- Polish fixture suite.
- Improved onboarding/home promise.
- Language variant model.
- Translation mode shortcut spec and UI.

### Milestone 0.3.0: Speak To Edit

Deliverables:

- Command intent model.
- Selected-text command UX.
- Replacement vs answer-panel behavior.
- History mode metadata.
- Tests for prompt injection and clipboard restoration.

### Milestone 0.4.0: Ask Anything

Deliverables:

- Answer panel.
- Open question mode.
- Selected-text Q&A.
- Search URL actions.
- BYOK/cloud capability messaging.

### Milestone 0.5.0: Personalization

Deliverables:

- Per-app style profile CRUD.
- Prompt composition by matched profile.
- Dictionary suggestions.
- Optional local style learning.

## 10. Risks And Mitigations

### Risk: "Unlimited words" is commercially expensive

Typeless can price Pro as unlimited. OpenTypeless currently has a low-price Pro option. We should not promise unlimited unless cost modeling supports it.

Mitigation:

- Use clear finite `xxx words/m` for OpenTypeless Cloud Pro.
- Keep BYOK unlimited as a differentiator.
- Show remaining words prominently.

### Risk: Ask Anything conflicts with privacy-first positioning

Ask Anything may send selected text to cloud LLMs.

Mitigation:

- Default BYOK behavior remains explicit.
- Show a first-use privacy notice for selected-text cloud commands.
- Do not store selected text by default.
- Add local-only mode where possible.

### Risk: Intent router makes wrong choices

Replacing selection when the user expected an answer is high-risk.

Mitigation:

- Start with conservative heuristics.
- Use answer panel for ambiguous commands.
- Add undo/copy affordances.
- Keep history entry with original selected text disabled by default.

### Risk: Linux selected-text parity is hard

Wayland limits global hotkeys and synthetic input.

Mitigation:

- Treat Linux as best effort for selection commands.
- Make clipboard copy-only behavior explicit on Wayland.
- Continue offering tray/app button fallback.

### Risk: Personalization feels creepy

Automatic style learning can surprise users.

Mitigation:

- Opt-in for learning.
- Local storage by default.
- Clear reset/export controls.
- Explain what is learned in plain language.

## 11. Open Questions For Product Discussion

1. What exact Cloud Pro quota should the app advertise as `xxx words/m`?
2. Do we want to compete with Typeless' "unlimited words" directly, or position OpenTypeless as lower-cost finite cloud quota + unlimited BYOK?
3. Should Ask Anything be available in BYOK-only mode first, Cloud mode first, or both?
4. Should selected-text commands replace text automatically, or require a preview for the first few uses?
5. Should OpenTypeless add an answer panel, or insert all answers into the current field?
6. Do we want web search actions in the desktop app, given privacy and implementation complexity?
7. Should personalization be local-only for v1?
8. Is mobile companion app part of the competitive target, or explicitly out of scope for the next 2-3 releases?
9. Which languages/variants matter most for the next release?
10. How bold should marketing copy be: "4x faster" vs "speak faster than typing" vs measured in-app stats?

## 12. Recommended Next Conversation

To converge the spec, decide these in order:

1. **Quota positioning**: exact `words/m`, finite vs unlimited, free quota.
2. **First hero workflow**: dictation quality, selected-text editing, or Ask Anything.
3. **Privacy stance**: what cloud mode may send/store, and how clearly we disclose it.
4. **Release slice**: whether `0.1.42` should only contain words quota UX, or also onboarding copy.

My recommendation:

- Ship `0.1.42` as the quota/copy release.
- Start `0.2.0` with selected-text editing as the flagship feature because the codebase already has most of the hard plumbing.
- Put full Ask Anything behind a feature flag until answer panel, search actions, and privacy copy are crisp.
