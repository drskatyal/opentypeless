# OpenTypeless Typeless Feature Completion P0/P1 Spec

Date: 2026-07-11

Status: Implemented product and engineering specification, revision 2.1

Canonical scope: OpenTypeless desktop plus the TalkMore cloud/account service

Baselines:

- `opentypeless` at `origin/main@1e5df0a`
- `talkmore` at `origin/main@dd7d023`

This document supersedes the earlier context-intelligence draft and fixture document. It is the product, implementation, and release source of truth for the P0/P1 work described below. Revision 1.1 incorporated the product review decisions on release slicing, detector latency, model capability, intent safety, password-reset redirects, cross-runtime session invalidation, restrained desktop UI, and measurable release fixtures. Revision 1.2 added a scalable built-in app registry, broader global and China-market app coverage, and explicit local custom app mappings without adding an app-management dashboard. Revision 2.0 records the implementation completed on `codex/typeless-p0-p1`, resolves the OAuth-only password flow against the final server contract, and adds exact user-visible outcomes, data boundaries, automated evidence, rollout controls, and remaining deployment validation. Revision 2.1 moves password mutation into an existing-style modal and adds a compact representative-app logo row beside context adaptation after packaged-desktop review.

## 1. Executive Summary

OpenTypeless already has the foundations of a strong cross-platform voice writing product: global dictation, optional AI polish, selected-text editing, Ask Anything, translation, local history, dictionary and correction rules, reusable scenes, BYOK providers, managed cloud providers, and desktop support on macOS, Windows, and Linux.

The remaining gap versus Typeless is not a collection of missing settings. The core gap is that OpenTypeless still behaves mostly like a configurable transcription pipeline, while Typeless increasingly behaves like a context-aware writing layer that understands where the user is writing, what result they intend, which parts of speech are instructions or corrections, and where the result should appear.

The primary product promise of this initiative is:

> The same spoken thought should become different ready-to-use writing in Gmail, Slack, Docs, GitHub, a code editor, or an unknown app, while preserving the same underlying facts and explicit user intent.

App recognition is therefore not a logo feature. Logos are optional feedback. The product value comes from automatically binding a safe writing policy to the detected context so users do not need to switch scenes or clean up the same transcript differently in every app.

Coverage must scale through data, not through one-off pipeline branches. Adding another app means adding a deterministic matcher, semantic-family mapping, optional small structured override, icon key, and fixtures to one local registry. It must not require a new LLM call, a standalone prompt, or a new UI surface.

This initiative makes OpenTypeless context-aware by default while preserving its differentiators:

- local-first history and settings
- BYOK and self-hosting
- explicit provider choice
- restrained desktop UI
- no silent web browsing
- no content-based telemetry
- user control over scenes and personalization

The initiative also closes the account recovery and password-management gap across `talkmore` and the desktop client. Account integrity is a separate P0 release track and must not be coupled to the context-intelligence rollout.

### 1.1 Revision 2.0 Implementation Result

The complete P0 and P1 functional scope in this specification is implemented in isolated OpenTypeless and TalkMore worktrees. The final implementation contains:

- 33 release-blocking reference app profiles and 38 extended profiles across all ten semantic families
- a background detector and O(1) recording-start snapshot read
- deterministic English, Simplified Chinese, and Traditional Chinese voice-intent routing
- secure password recovery, password change/set, token rotation, and managed-cloud session invalidation
- one-to-five translation targets with in-recording switching
- transactional dictionary import/export and explicit correction creation
- explicit device-local app-to-family/scene mappings
- up to three Dictate, Ask, and Translate shortcuts per action
- restrained desktop UI additions inside existing History, AI Polish, General, Dictionary, Translation, and Account surfaces

All repository-level automated tests, lint gates, and production builds pass at the commits recorded in Section 23. Cross-platform physical app recognition, native shortcut behavior, live transactional email delivery, and managed-model semantic evaluation remain deployment validation rather than unimplemented product behavior.

## 2. Evidence and Competitive Baseline

Official Typeless sources reviewed:

- Key features: https://www.typeless.com/help/quickstart/key-features
- macOS release notes: https://www.typeless.com/help/release-notes/macos
- Android release notes: https://www.typeless.com/help/release-notes/android
- Ask Anything: https://www.typeless.com/ask-anything
- Multiple translation targets: https://www.typeless.com/help/release-notes/macos/set-multiple-target-languages
- Voice editing: https://www.typeless.com/help/release-notes/macos/voice-superpowers

Official product surfaces reviewed while expanding the app registry:

- Lark/Feishu: https://www.larksuite.com/
- DingTalk: https://www.dingtalk.com/en
- WPS Office: https://www.wps.com/
- Tencent Docs: https://docs.qq.com/
- Shimo: https://shimo.im/
- Yuque: https://www.yuque.com/
- Jira and Confluence: https://www.atlassian.com/software/confluence/use-cases/confluence-jira

Better Auth sources reviewed for the cloud/account track:

- Email and password: https://better-auth.com/docs/authentication/email-password
- Email configuration: https://better-auth.com/docs/concepts/email
- Session management: https://better-auth.com/docs/concepts/session-management
- User and account linking: https://better-auth.com/docs/concepts/users-accounts

Confirmed Typeless capabilities relevant to this scope:

1. Removes filler and unnecessary repetition.
2. Keeps only the final intent after immediate or late self-correction.
3. Uses side notes as context rather than always placing them in output.
4. Reorders loosely spoken ideas into useful written structure.
5. Adapts tone and formatting to the active app.
6. Rewrites, summarizes, explains, translates, drafts, and searches by voice.
7. Places drafts in the active field and informational answers in a compact panel.
8. Supports multiple translation targets and in-flow target switching.
9. Supports dictionary search and bulk import.
10. Learns or suggests vocabulary from corrections.
11. Supports multiple shortcuts for external keyboards.
12. Keeps the normal experience visually lightweight.

Capabilities explicitly not treated as P0/P1 parity targets:

- iOS or Android keyboard products
- swipe-to-type keyboards
- enterprise administration
- referral and affiliate programs
- a 58-language UI expansion
- broad autonomous web browsing

## 3. Problem Statement

### 3.1 Primary Problem

Users can configure OpenTypeless to produce good text, but the product often needs manual setup to behave appropriately in the current task. Gmail in a browser can be classified as a generic browser. Slack, Google Docs, Notion, GitHub, support tools, and social composers can receive the same general polish behavior. Users must compensate with manual scene selection or custom prompts.

### 3.2 Secondary Problems

- Speech corrections and side comments can survive into the final text.
- Voice commands are split between normal dictation, selected-text keyword routing, and Ask Anything instead of sharing one intent model.
- Translation supports one configured target at a time.
- Dictionary management lacks search, edit, import, export, and a safe correction-learning workflow.
- Personal style exists as manual scenes, but it is not naturally associated with context.
- Account users can sign up and subscribe but cannot recover or change a password.
- A cloud session can expire or be revoked while the desktop still appears signed in.

### 3.3 Why This Matters

The product promise is text that is ready to use in the app where the user is already working. A transcription that is technically accurate but has the wrong tone, retains a discarded thought, routes to the wrong output surface, or cannot be recovered after a forgotten password breaks that promise.

## 4. Target Users and Jobs

### 4.1 Primary Users

Knowledge workers who dictate repeatedly across several desktop apps:

- founders and product managers moving between Gmail, Slack, Docs, Notion, Linear, and browsers
- developers dictating prompts, issues, reviews, and technical notes
- multilingual professionals switching between work-chat and email languages
- privacy-conscious users choosing BYOK or local models

### 4.2 Secondary Users

- customer support workers writing repeated ticket replies
- writers and students using selected-text explanation and summarization
- managed-cloud subscribers who need a complete account lifecycle

### 4.3 Jobs to Be Done

- When I speak naturally, turn my final thought into text I can send without cleanup.
- When I move between apps, adapt the writing without asking me to change a mode every time.
- When I select text and speak, infer whether I want to edit it or understand it.
- When I translate, let me switch among the languages I use without returning to settings.
- When OpenTypeless repeatedly misses a term, make correction easy without silently learning private content.
- When I lose access to my password, let me recover my paid account safely.

## 5. Product Principles

1. Context should be automatic, but user intent must win.
2. An active user-selected scene overrides automatic style adaptation.
3. Safety and semantic fidelity override style, scene, and personalization.
4. Low-confidence command detection falls back to normal dictation.
5. No P0 feature adds an extra LLM request to every dictation.
6. Browser URL paths, query strings, and selected text are not stored in history.
7. Search opens only explicit, allow-listed destinations.
8. The desktop UI stays compact and work-focused.
9. `talkmore` is the account and managed-cloud source of truth.
10. BYOK and local-only paths remain first-class.
11. Raw process names, window titles, and browser hosts are untrusted classifier inputs, never prompt instructions.
12. Release-quality semantic guarantees apply only to the managed reference model and an explicit certified-model matrix; arbitrary BYOK and local models remain best effort.

## 6. Current State

### 6.1 OpenTypeless Desktop

Already present:

- global dictation and Ask shortcuts
- hold and toggle recording modes
- streaming/partial transcript display
- selected-text capture and keyword routing
- Ask popup with selected-text context
- explicit search routing for Google, YouTube, Amazon, and GitHub
- translation shortcut and single target language
- dictionary terms and correction rules
- built-in and local custom scenes
- local history and retention controls
- BYOK, local, and managed-cloud providers
- macOS, Windows, and Linux platform capability handling

Confirmed gaps:

- `window_title` is collected, but app classification is primarily process-name based.
- Browser-hosted products often degrade to a general browser context.
- Prompt context is limited to coarse Email, Chat, Document, Code, and General add-ons.
- normal dictation and Ask do not share a first-class intent router.
- normal dictation does not reliably support `DraftInsert` commands.
- Ask defaults to a nondestructive popup and does not consistently route drafts into the active field.
- dictionary UI supports add/remove/toggle but not a full maintenance workflow.
- only one translation target is stored.
- one shortcut binding is stored for each role.

### 6.2 TalkMore Cloud and Account Service

Already present:

- Better Auth email/password login
- email verification through Resend
- Google and GitHub OAuth
- bearer sessions for the desktop client
- account linking
- subscription, license, cloud quota, backup, and proxy APIs
- explicit desktop CORS/trusted origins

Confirmed gaps:

- `emailAndPassword.sendResetPassword` is not configured.
- no reset-password web page exists.
- web and desktop login views have no Forgot password action.
- web and desktop account views have no password action.
- desktop auth state has no centralized invalid-session handling.
- desktop password change does not rotate the bearer token because the flow does not exist.
- verification email HTML still displays `TalkMore` instead of `OpenTypeless` in the template heading.
- the desktop resolves Better Auth `1.6.17`, while TalkMore currently resolves `1.4.19`; the cross-version client/server contract is not covered by an end-to-end auth test.

## 7. Priority and Release Slices

P0 is split into independently shippable tracks. Each track has its own release gate and may reach production without waiting for the other two. A combined P0 milestone is a roadmap label, not a requirement that all tracks deploy together.

### P0-A: Context-Aware Writing

1. Context Detector V2.
2. Semantic context profiles and small app-specific overrides.
3. Automatic context prompt binding.
4. Thought-aware dictation and self-correction fixtures.
5. Restrained context feedback in History and AI Polish settings.

### P0-B: Voice Intent and Output Placement

1. Shared intent model.
2. Draft insertion.
3. selected-text rewrite versus answer routing.
4. explicit search routing and false-positive protection.

### P0-C: Account Integrity

1. Forgot-password request and email.
2. Web reset-password completion.
3. Web and desktop password change.
4. session revocation and desktop bearer-token rotation.
5. automatic desktop sign-out after cloud-session invalidation.

Recommended release order:

1. P0-C Account Integrity, because it closes an existing account-safety gap and is independent of dictation behavior.
2. P0-A Context-Aware Writing, behind `context_adaptation_enabled` with an immediate local kill switch.
3. P0-B Voice Intent and Output Placement, after conservative false-positive fixtures pass for every enabled locale.

P1:

1. Multiple translation targets and in-flow switching.
2. Dictionary search, edit, import, export, and correction suggestions.
3. Explicit local style profiles, context-to-scene assignments, and custom app mappings.
4. Multiple shortcut bindings per action.

P2 or later:

1. Automatic observation of post-insertion edits.
2. Silent personal-style learning from history.
3. mobile keyboards.
4. field-level browser understanding through an extension.
5. enterprise administration.

## 8. Cross-Repository Architecture

### 8.1 Ownership

`opentypeless` owns:

- foreground context collection
- local context classification fallback
- voice mode and intent routing
- prompt composition for BYOK/local providers
- desktop UI and local history
- desktop bearer-token storage and Rust token synchronization

`talkmore` owns:

- Better Auth configuration and account data
- reset tokens and password writes
- verification, reset, and security email delivery
- web reset and account screens
- managed-cloud prompt execution
- cloud quota metadata and server-side security controls

Shared contract:

- desktop and cloud prompt paths must receive the same normalized context and intent metadata
- prompt paths may receive only internal context profile identifiers, semantic families, and allow-listed app override identifiers; they must never receive raw process names, window titles, or browser hosts as instructions
- account/session APIs must return enough information for desktop token rotation
- neither repository may log audio, transcript, selected text, or full prompt content in production diagnostics
- the TalkMore server endpoint shape is authoritative; both lockfiles must be pinned and their compatibility covered by contract tests before release

### 8.2 Runtime Flow

```text
OS focus/browser signals
  -> asynchronous Context Detector V2
  -> ContextSnapshot cache

Recording hotkey
  -> read cached ContextProfile without blocking microphone activation

Audio
  -> STT
  -> transcript
  -> VoiceIntent router
  -> prompt composer
       safety + operation + thought rules + context + user scene + dictionary
  -> BYOK/local LLM or TalkMore cloud LLM
  -> output placement
       insert | replace selection | popup | open allow-listed URL
  -> local history diagnostics
```

## 9. P0-A Requirements: Context-Aware Writing

### 9.1 Context Detector V2

#### Required Model

```rust
pub enum ContextFamily {
    Email,
    WorkChat,
    PersonalChat,
    Document,
    ProjectManagement,
    DeveloperCollaboration,
    PromptOrCode,
    Support,
    Social,
    General,
}

pub enum ContextSource {
    NativeProcess,
    BrowserDomain,
    WindowTitle,
    Fallback,
}

pub struct ContextProfile {
    pub id: String,
    pub family: ContextFamily,
    pub app_label: String,
    pub source: ContextSource,
    pub confidence: f32,
}

pub enum AppReleaseTier {
    Reference,
    Extended,
}

pub enum NativeMatcher {
    BundleId(&'static str),
    Executable(&'static str),
    ProcessAlias(&'static str),
}

pub enum WebMatcher {
    ExactHost(&'static str),
    HostSuffix(&'static str),
}

pub struct TitleMatcher {
    pub anchored_marker: &'static str,
    pub required_browser_or_process: &'static str,
}

pub struct AppProfileDefinition {
    pub id: &'static str,
    pub label: &'static str,
    pub family: ContextFamily,
    pub override_id: Option<&'static str>,
    pub native_matchers: &'static [NativeMatcher],
    pub web_matchers: &'static [WebMatcher],
    pub title_matchers: &'static [TitleMatcher],
    pub icon_key: &'static str,
    pub release_tier: AppReleaseTier,
}

pub struct AppStyleOverride {
    pub artifact_kind: ArtifactKind,
    pub formality: Formality,
    pub density: Density,
    pub markup: MarkupPolicy,
    pub list_behavior: ListBehavior,
}
```

Raw detection signals use a separate internal-only `ContextSignals` value containing normalized process identity, window title, and optional browser host. The browser value may contain only the normalized host and must never contain path, query, fragment, user information, or a full URL.

`ContextSignals` is untrusted input. Window title and browser host are discarded after deterministic classification. A separate ephemeral `TargetAppGuard` may retain only the normalized executable/bundle identity and process ID needed to verify focus before insertion; it is destroyed when the operation completes. Neither structure may be sent to an LLM, TalkMore, history, analytics, or normal production logs. `app_label` must come from the internal profile map rather than a raw title or process string.

The app registry is a compile-time local data source under `app_detector/profiles.rs`. It is not downloaded at runtime. Built-in overrides are structured policy deltas, not independent free-form prompts. Adding an app cannot introduce arbitrary prompt text outside the shared, reviewed policy composer.

#### P0 Reference Coverage

Reference-tier profiles are release-blocking. Each profile has exact matcher fixtures, forbidden near-match fixtures, a mapped label/icon, family behavior tests, and an optional small app override.

| Category                | Reference Apps and Surfaces                              | Family                 | Default Writing Behavior                             |
| ----------------------- | -------------------------------------------------------- | ---------------------- | ---------------------------------------------------- |
| Email                   | Gmail, Outlook, Apple Mail                               | Email                  | complete email body; natural professional tone       |
| Work chat               | Slack, Teams, Lark/Feishu, DingTalk, WeCom               | WorkChat               | concise work message; low formatting                 |
| Personal messaging      | Messages, WhatsApp, Telegram, WeChat                     | PersonalChat           | compact natural message; lighter formality           |
| Documents and knowledge | Google Docs, Notion, Word, WPS Office, Confluence, Yuque | Document               | coherent prose or supported lightweight structure    |
| Project collaboration   | Linear, Jira, Figma                                      | ProjectManagement      | concrete update, decision, status, or next action    |
| Developer collaboration | GitHub, GitLab                                           | DeveloperCollaboration | precise Markdown-friendly technical collaboration    |
| Code editors            | Cursor, VS Code, JetBrains IDEs                          | PromptOrCode           | preserve code, identifiers, paths, and commands      |
| AI assistants           | ChatGPT, Claude, Gemini                                  | PromptOrCode           | explicit, structured prompt text                     |
| Customer support        | Zendesk, Intercom                                        | Support                | empathetic, actionable reply without invented policy |
| Social                  | LinkedIn, X                                              | Social                 | platform-appropriate post or comment                 |
| Unknown                 | unknown browser, unknown native app                      | General                | neutral clean dictation                              |

#### Extended Built-In Coverage

Extended-tier entries use the shared family policy unless a reviewed override is justified. They may ship when their positive and negative matcher fixtures pass, but absence of one extended entry does not block the P0-A release.

| Category                | Extended Apps and Surfaces                                       | Family                 |
| ----------------------- | ---------------------------------------------------------------- | ---------------------- |
| Email                   | Spark, Superhuman, Thunderbird                                   | Email                  |
| Work chat               | Mattermost, Rocket.Chat, Zoom Team Chat                          | WorkChat               |
| Personal messaging      | Signal, Discord, QQ                                              | PersonalChat           |
| Documents and notes     | Pages, Obsidian, Craft, Coda, Tencent Docs, Shimo, Dropbox Paper | Document               |
| Project and task work   | Asana, Trello, ClickUp, monday.com, Todoist, Things              | ProjectManagement      |
| Developer collaboration | Bitbucket, Azure DevOps                                          | DeveloperCollaboration |
| Code editors            | Xcode, Zed, Sublime Text, Android Studio                         | PromptOrCode           |
| AI assistants           | Perplexity, Microsoft Copilot, Grok                              | PromptOrCode           |
| Customer support/CRM    | Freshdesk, Salesforce Service Cloud, HubSpot Service             | Support                |
| Social                  | Reddit                                                           | Social                 |
| Publishing              | Medium, Substack, WordPress                                      | Document               |

This list is a committed coverage target, not permission to add unverified brand guesses. Exact bundle IDs, executable aliases, hosts, and title markers are checked against installed builds or official web surfaces when fixtures are authored.

#### Registry Matching Rules

Classification precedence:

1. explicit local user mapping
2. exact browser host or reviewed dot-boundary host suffix when the foreground process is a supported browser
3. exact native bundle identifier or executable identity
4. reviewed native process alias
5. allow-listed, anchored title fallback with lower confidence
6. safe semantic-family fallback
7. `general.browser` or `general.native`

Requirements:

- profile IDs use stable `{family}.{slug}` identifiers such as `email.gmail`, `chat.slack`, `message.wechat`, `doc.wps`, `project.linear`, `dev.github`, `code.cursor`, `prompt.claude`, and `support.zendesk`
- exact host match wins over suffix match; suffix rules match only at a dot boundary
- paths, query strings, arbitrary page text, and unreviewed regular expressions are not matcher inputs
- duplicate profile IDs, conflicting exact matchers, unreachable matchers, and missing labels/icons fail registry validation tests
- build exact-host, suffix, bundle-ID, executable, and alias lookup indexes once at startup; do not linearly compose or scan prompt fragments on every recording
- a title matcher cannot classify an unknown browser as Email, WorkChat, PersonalChat, Support, Social, or ProjectManagement from a weak substring
- registry size does not change the number of LLM requests or the prompt-layer count
- the prompt receives only family policy plus the selected structured `override_id`

#### Hybrid Classification Rule

Detection may identify an exact app or site, but prompts are built from a stable semantic family plus a small app override. Do not maintain a complete independent prompt for every brand.

Example:

- Slack uses `WorkChat` rules plus a small `slack` override.
- Gmail uses `Email` rules plus a small `gmail` override.
- unknown mail clients may use `Email` without an app override.
- Lark/Feishu native defaults to `WorkChat`; an allow-listed high-confidence document title may select `Document`, but OpenTypeless does not inspect a private document URL path to force that distinction.
- WPS Office uses `Document` behavior across its writing surfaces rather than pretending to know the active field or file type.
- Figma uses `ProjectManagement` collaboration behavior; without field-level DOM access, OpenTypeless does not claim to distinguish a canvas label from a comment field.

#### Platform Strategy

Recording-start architecture:

- maintain a background `ContextSnapshot` cache instead of spawning detection work on the hotkey critical path
- update the cache on foreground-app changes and while a supported browser remains foregrounded
- use OS-native focus notifications or hooks where available; a permission-safe poll may be used as fallback
- do not poll a supported browser adapter more than once per second unless an OS focus event invalidates the snapshot
- a recording start performs an O(1) cache read only
- if no snapshot exists or the latest snapshot is older than two seconds, use `General` for that recording and schedule a nonblocking refresh
- browser adapter failure, missing permission, detector startup, or a stale cache never delays microphone activation
- store the snapshot captured for the recording so History and Settings can show the context used for that output rather than the app currently showing Settings

macOS:

- replace recording-start `osascript` subprocesses with a background native detector or cached adapter
- for Safari, Chrome, Edge, Brave, and Arc, attempt active-tab host retrieval through a known-browser adapter outside the recording-start critical path
- check browser Automation authorization without prompting; never trigger a new browser-control permission dialog at app startup or recording start
- when authorization is unavailable, use process/title classification until a future explicit permission action is added to the existing permissions UI
- fail closed to title/process heuristics when automation permission is unavailable

Windows:

- use foreground executable and window title first
- use supported browser title/domain adapters only where reliable
- do not make UI Automation a release blocker for the first P0 slice

Linux:

- use process and title heuristics
- do not claim reliable browser-domain detection under Wayland
- preserve current capability warnings and fallback behavior

#### Acceptance Criteria

- Gmail in Chrome or Safari does not classify as General when host or a high-confidence title is available.
- Slack native and Slack web map to the same semantic family.
- GitHub and a code editor use different profile IDs but compatible technical preservation rules.
- every reference-tier profile passes at least one exact positive matcher and three forbidden near-match fixtures for every matcher type it declares.
- every shipped extended-tier entry passes registry validation and positive/negative matcher fixtures even when it has no app-specific override.
- representative Email, WorkChat, PersonalChat, Document, ProjectManagement, DeveloperCollaboration, PromptOrCode, Support, and Social profiles are manually verified on each platform where a supported surface exists.
- an unknown website does not become Email, WorkChat, PersonalChat, ProjectManagement, Support, or Social based on a weak substring.
- context collection failure never blocks dictation.
- the recording-start cache read is below 5ms p95 in the release performance test.
- raw titles, process strings, browser hosts, full URLs, and selected text are not sent to prompts or stored by default; the only persistence exception is the explicit device-local P1 matcher defined in Sections 12.3 and 14.1.

### 9.2 Automatic Context Prompt Binding

Add `context_adaptation_enabled: bool`, default `true` for new and existing users.

Precedence:

1. safety and semantic-fidelity invariants
2. operation and output-placement contract
3. translation/language contract
4. thought-aware dictation rules
5. semantic context family
6. app-specific override
7. built-in polish style
8. explicit personal style profile, if enabled
9. scene assigned through an explicit custom app mapping, if present
10. active manually selected scene
11. explicit custom polish prompt

Later layers may refine style but must not override safety, requested operation, language, or factual fidelity.

An active manually selected scene wins over a custom mapped scene and automatic context style when they conflict. A custom mapped scene applies only when no manual scene is active. Context still supplies technical preservation and output-format safety, but it must not rewrite the user's chosen tone.

Built-in app overrides may adjust only artifact kind, formality, density, markup, and list behavior. They may not introduce a recipient, greeting, sign-off, deadline, assignee, issue state, policy, commitment, hashtag, emoji, or factual claim that the user did not provide.

#### Context Behavior Examples

Email:

- produce an email body, not a subject line, unless the user explicitly asks for a subject
- use complete sentences and natural paragraph breaks
- do not invent a greeting, recipient name, sign-off, deadline, or commitment

Work chat:

- prefer one to three short paragraphs
- avoid greetings and signatures unless spoken
- use bullets only when the content is naturally a list

Personal chat:

- prefer one or two compact natural messages
- avoid workplace status language, headings, greetings, and signatures unless spoken
- do not add emojis, slang, or intimacy that the user did not express

Developer collaboration:

- preserve identifiers, filenames, commands, code spans, issue numbers, and Markdown
- prefer problem, evidence, and next-step structure when the speech supports it
- do not turn dictation into code unless the user asks for code

Project management:

- make status, decision, blocker, owner, and next action explicit only when the speech provides them
- preserve issue IDs, dates, assignees, and state names exactly
- do not invent a ticket title, owner, priority, due date, estimate, or status transition

Support:

- acknowledge the user's issue without inventing policy or resolution
- preserve concrete steps and ownership

Social:

- respect the spoken claim and tone
- do not add hashtags, emojis, engagement bait, or unsupported claims unless requested

### 9.3 Thought-Aware Dictation

P0 must cover distinct semantic behaviors instead of treating all of them as generic polish.

#### Required Behaviors

Filler removal:

- remove hesitation sounds and empty discourse fillers
- retain meaningful discourse markers such as `however`, `actually`, or `for example`

Repetition removal:

- remove accidental repeated words and restarted fragments
- retain intentional rhetorical repetition

Immediate self-correction:

- `meet at seven, actually three` -> `Meet at three.`
- `send it to Sarah, no, Sam` -> `Send it to Sam.`

Late self-correction:

- a later explicit correction may replace an earlier fact even after intervening speech
- preserve all unrelated content

Side-note handling:

- explicit meta phrases such as `for context, do not include this` may guide the output without appearing in it
- ordinary parenthetical information must not be dropped merely because it sounds secondary

Out-of-order organization:

- organize steps, lists, and priority order when the speaker explicitly indicates order
- do not reorder a narrative or argument without clear evidence

Described term resolution:

- resolve a described name only from the current utterance, selected context, personal dictionary, correction rules, or an explicit trusted context source
- never search silently
- never invent an exact name when evidence is missing
- when uncertain, preserve the spoken description rather than hallucinating

#### Implementation Constraint

P0 uses the existing polish request. It must not add a second LLM classification or reasoning request to every dictation.

If AI polish is disabled, OpenTypeless returns the STT result and does not claim thought-aware semantic cleanup.

#### Model Capability Contract

The semantic quality gates in this section apply to:

1. the managed-cloud reference model and pinned prompt version used for the release
2. BYOK or local model/version pairs explicitly listed in a certified-model fixture matrix

Arbitrary custom Base URLs, unlisted BYOK models, and local models remain supported but are best effort. They do not inherit a product claim of 100 percent critical-fact preservation or full thought-aware cleanup merely because the request is OpenAI-compatible.

The desktop must not add a heavy new UI for this distinction. Settings may show one concise compatibility note after model selection or connection testing. Unsupported does not mean blocked: users may continue, while diagnostics identify the selected provider and model so failures can be reproduced without logging content.

#### Acceptance Criteria

- all critical names, dates, numbers, negations, URLs, identifiers, and commitments are preserved unless explicitly corrected
- discarded alternatives do not appear in output
- an ambiguous `actually` phrase is punctuated rather than always treated as deletion
- low-confidence reordering keeps original order
- fixture output contains no invented facts

## 10. P0-B Requirements: Voice Intent and Output Placement

### 10.1 Shared Intent Model

Create a shared router used by normal dictation, selected-text dictation, Translate, and Ask.

```rust
pub enum VoiceMode {
    Dictate,
    Ask,
    Translate,
}

pub enum VoiceIntentKind {
    DictateInsert,
    DraftInsert,
    RewriteSelection,
    TranslateInsert,
    TranslateSelection,
    AskSelection,
    OpenQuestion,
    Search,
}

pub enum VoiceOutputPlacement {
    InsertAtCursor,
    ReplaceSelection,
    PopupAnswer,
    OpenUrl,
}

pub struct VoiceIntent {
    pub kind: VoiceIntentKind,
    pub placement: VoiceOutputPlacement,
    pub confidence: f32,
    pub search_provider: Option<String>,
}
```

### 10.2 Routing Rules

Normal dictation without selection:

- default to `DictateInsert`
- route to `DraftInsert` only when the normalized utterance begins with an exact allow-listed command prefix for the detected speech locale, followed by a non-empty draft payload
- English P0 prefixes include forms such as `draft`, `write`, `compose`, and `reply with`; Chinese P0 prefixes include explicit imperative forms such as `写一封`, `起草`, `帮我写`, and `回复说`
- a command word appearing later in a sentence, inside a quotation, after a negation, or as the subject of discussion is normal dictation
- low confidence falls back to `DictateInsert`

Normal dictation with selected text:

- explicit rewrite/tone/length/format commands -> `RewriteSelection`
- explicit translation -> `TranslateSelection`
- informational question -> `AskSelection` and popup
- ambiguous speech -> nondestructive popup or normal dictation, never destructive replacement

Ask without selection:

- direct question -> `OpenQuestion` and popup
- explicit draft command -> `DraftInsert` in the previously active field
- explicit allow-listed search -> `Search`

Ask with selection:

- summarize, explain, compare, or factual question -> `AskSelection` and popup
- P0 keeps Ask nondestructive for ambiguous rewrite commands
- destructive replacement remains available through the dictation/edit-selection flow

Translate:

- Translate mode without selection -> `TranslateInsert`
- explicit translate command with selection -> `TranslateSelection`

### 10.3 Search Safety

P0 allow list:

- Google
- YouTube
- Amazon
- GitHub

Requirements:

- the command must explicitly name search intent
- a mention such as `I should search for this later` is normal dictation
- queries are URL-encoded
- no arbitrary URL or shell command may be generated
- OpenTypeless does not fetch or summarize search results in P0

### 10.4 Router Implementation

- deterministic routing in P0
- use token- or boundary-aware locale grammars, not unrestricted substring matching
- each grammar contains positive prefixes, required payload rules, negation guards, quotation/reported-speech guards, and near-miss fixtures
- English, Simplified Chinese, and Traditional Chinese command grammars are enabled for the first P0-B release
- other speech locales default to nondestructive `DictateInsert`, `OpenQuestion`, or the explicit Translate mode until that locale's grammar and release fixtures pass
- grammar selection uses the explicit STT language when configured; in automatic-language mode, only an exact enabled-language command prefix with an unambiguous script/boundary match may activate a command route
- UI localization remains required for all ten desktop UI locales; UI locale coverage does not imply that destructive voice-command routing is enabled for the same spoken language
- no additional LLM call solely for routing
- router kind and confidence may be stored as diagnostics
- selected text and full utterance content are not stored as router telemetry
- `DraftInsert`, `RewriteSelection`, `TranslateSelection`, and `Search` are individually feature-flagged so one unsafe route can be disabled without disabling dictation

### 10.5 Acceptance Criteria

- `draft a follow-up email about tomorrow's launch` inserts a draft in Gmail.
- `I need to draft a follow-up email tomorrow` remains normal dictation.
- `do not draft this yet` and `she said "draft a reply"` remain normal dictation.
- `make this warmer` replaces selected email text when using dictation/edit mode.
- `what does this mean?` with selection opens a compact answer.
- `search React tutorials on YouTube` opens an encoded YouTube search.
- Ask and dictation share the same intent type definitions.
- focus restoration is tested before any insertion or replacement.
- every enabled command locale passes its positive, negated, quoted, ambiguous, and code/identifier blocker fixtures with zero destructive false positives.

## 11. P0-C Requirements: Account Integrity

### 11.1 Source of Truth

`talkmore` owns password-reset tokens, password hashes, account links, sessions, and email delivery. The desktop must not implement a parallel reset-token system.

### 11.2 Forgot Password

Entry points:

- TalkMore web login
- OpenTypeless desktop Account login view

Flow:

1. User enters an email.
2. Web or desktop calls a TalkMore-owned wrapper endpoint with `{ email, locale }`; the client cannot supply `redirectTo` or another callback URL.
3. TalkMore validates the locale against the deployed locale list and constructs the canonical same-origin callback `https://www.opentypeless.com/{locale}/reset-password`.
4. TalkMore invokes Better Auth `requestPasswordReset` server-side with that canonical callback.
5. UI always displays the same success response regardless of account existence.
6. TalkMore sends an OpenTypeless-branded email through Resend when the account exists.
7. The email link passes through Better Auth token validation.
8. The user lands on the localized TalkMore reset page with a validated token.
9. The user enters and confirms a new password.
10. TalkMore calls `resetPassword` and revokes existing sessions.
11. Success returns the user to login. No reset token is passed to the desktop app.

Security requirements:

- token lifetime: one hour
- token is one-time use
- generic response for unknown emails
- preserve and verify Better Auth rate limiting
- the wrapper applies a rate limit at least as strict as the underlying Better Auth route using client IP plus a one-way normalized-email key; a server-to-server loopback address must not collapse or bypass caller limits
- disable the public Better Auth `/request-password-reset` path through `disabledPaths` or an equivalent server guard; the TalkMore wrapper is the only public reset-request entry point
- the wrapper may call the server-side Better Auth API internally, but no browser or desktop request can bypass it and submit `redirectTo` to the default endpoint
- the wrapper ignores unknown request fields and never redirects to a client-provided origin
- production reset callbacks use a dedicated exact allow list containing only canonical OpenTypeless HTTPS reset pages; broad Better Auth/CORS trusted origins, Tauri origins, and production-enabled localhost origins are never accepted as reset callbacks
- localhost reset callbacks may exist only in a development-only configuration that cannot be enabled in a production build
- configure Better Auth `advanced.backgroundTasks.handler` with the deployment platform's request-lifetime primitive so `runInBackgroundOrAwait` can finish Resend delivery after the response without being terminated
- email delivery success and failure are covered by an integration test against the configured background-task handler
- `revokeSessionsOnPasswordReset: true`
- reset tokens necessarily travel through the Better Auth email/callback URL, but are never written to application logs, analytics, local storage, desktop history, or persisted page state
- Better Auth logger output is wrapped or sanitized so unknown-account reset attempts do not log the submitted email address or token-bearing URLs
- the TalkMore reset page reads the callback token into memory and immediately removes it from the visible address with `history.replaceState`
- the reset page is `noindex` and uses a no-referrer policy

### 11.3 Change Password

Credential account:

- require current password
- require new password and confirmation
- enforce Better Auth minimum and maximum length
- default `revokeOtherSessions` to `true`

OAuth-only account:

- do not show a current-password form that must fail
- show a restrained `Set password` action
- require the authenticated OAuth email to be verified; if it is not verified, send verification email and block password creation
- after verification, call the TalkMore-owned `/api/opentypeless/auth/set-password` server endpoint
- the endpoint requires an authenticated session, verifies `emailVerified`, verifies that no credential provider already exists, and invokes Better Auth `setPassword` server-side
- clients never call the server-only Better Auth `setPassword` API directly and never receive a password hash

Linked OAuth plus credential account:

- show `Change password`
- changing the password does not unlink Google or GitHub

Account capability detection:

- web and desktop call Better Auth `listAccounts` after authentication
- a returned `credential` provider maps to `present`; clients never request or inspect a password hash
- social providers without a credential account map to `none`
- loading or endpoint failure maps to `unknown` and hides password mutation actions rather than guessing
- provider IDs may be used for UI behavior but are not written to dictation history

### 11.4 Desktop Token Rotation

When desktop calls `changePassword({ revokeOtherSessions: true })`, Better Auth returns a new session token.

The desktop must:

1. replace `localStorage.session_token`
2. call the existing Rust `set_session_token` command with the new token
3. refresh the session and subscription state
4. keep the current desktop signed in
5. reject the old token in tests

### 11.5 Invalid Session Handling

Add one centralized cross-runtime desktop auth invalidation path.

TalkMore authenticated cloud endpoints return a stable JSON error envelope:

```json
{
  "error": {
    "code": "AUTH_SESSION_INVALID",
    "message": "Session expired"
  }
}
```

Required machine codes:

- `AUTH_SESSION_INVALID`: a supplied OpenTypeless bearer session is expired, revoked, or invalid; HTTP `401`
- `AUTH_REQUIRED`: no OpenTypeless bearer session was supplied; HTTP `401`
- `QUOTA_EXCEEDED`: the account is valid but the managed-cloud quota blocks the request; HTTP `403`

Managed-cloud STT, LLM, Ask, subscription, backup, and other authenticated routes must preserve these codes. They must not require the desktop to infer account state from a status code or human-readable message.

Rust adds a dedicated `CloudSessionInvalid` error distinct from provider-key `Auth` and quota errors. When a managed-cloud response contains `AUTH_SESSION_INVALID`, Rust emits one `auth:session-invalid` Tauri event. A BYOK/local upstream `401` never creates this error or event.

The frontend owns one idempotent `invalidateCloudSession()` action used by both the Tauri event and TypeScript API responses. Concurrent failures collapse into one transition and one notification.

On an authenticated cloud response that proves the session is invalid:

- remove the local token
- clear the Rust token
- reset managed-cloud account and quota state
- retain BYOK configuration and local history
- show one concise `Session expired. Sign in again.` notification
- do not repeatedly show the same notification for every failed request

A provider-key `401` in BYOK mode must not sign the user out of the OpenTypeless account.

`AUTH_REQUIRED` may show the normal sign-in requirement but does not claim a session was revoked. `QUOTA_EXCEEDED` preserves the signed-in state and follows existing quota UI.

### 11.6 Email and Brand Consistency

- reset, verification, and password-changed emails use `OpenTypeless`
- remove the remaining `TalkMore` heading from transactional email HTML
- include plain-text fallback content
- include token-expiry wording
- never include subscription or quota details in a password email
- use Better Auth `onPasswordReset` for the post-reset security notification
- use a server-side Better Auth after hook for successful `/change-password` notifications; do not trust a separate client-triggered notification request
- a notification-delivery failure does not roll back a successful password reset or change

### 11.7 Acceptance Criteria

- web and desktop can request a reset without revealing whether an email exists
- a client-supplied reset callback, production localhost callback, and non-HTTPS callback are rejected or ignored before Better Auth receives the request
- a direct public request to Better Auth `/request-password-reset` returns `404` or an equivalent non-operational response
- an expired or reused token cannot reset a password
- password reset revokes old browser and desktop sessions
- desktop change-password rotates and persists its token
- OAuth-only users can establish a password only from a verified authenticated account through the TalkMore server wrapper
- a revoked desktop session is cleared on the next authenticated cloud request
- simultaneous Rust and TypeScript invalid-session responses produce one state reset and one notification
- local-only/BYOK use remains available after cloud sign-out

## 12. P1 Requirements

### 12.1 Multiple Translation Targets

Data:

```rust
pub struct TranslationConfig {
    pub targets: Vec<String>,
    pub active_target: String,
}
```

Requirements:

- migrate existing `target_lang` into a one-item `targets` list
- allow up to five unique targets
- permit reorder and removal, but never leave the list empty
- switch active target during Translate recording
- apply app context to translated output so a translated email reads like an email and translated chat reads like chat
- changing the active target for one run also becomes the next default

### 12.2 Dictionary Workflow

Requirements:

- local search across dictionary entries and correction rules
- edit word, pronunciation, wrong phrase, and corrected phrase
- bulk import UTF-8 TXT/CSV and the existing scene/dictionary bundle format where applicable
- export dictionary and correction rules without API keys or unrelated settings
- deduplicate normalized entries
- parse imports with structured TXT/CSV parsers, enforce documented columns, cap file size and row count, and report accepted/skipped/error counts without partially corrupting existing data
- quote and escape exported CSV values so spreadsheet software cannot interpret dictionary text as a formula
- from History, offer `Create correction` using raw and polished text only after explicit user action
- suggestions remain local and are never auto-added

Automatic monitoring of what the user types after OpenTypeless inserts text is P2.

### 12.3 Explicit Personal Style

P1 reuses Scenes instead of adding a second style-management system.

```rust
pub enum UserAppMatcher {
    NativeBundleId(String),
    NativeExecutable(String),
    ExactWebHost(String),
}

pub struct CustomAppMapping {
    pub id: String,
    pub label: String,
    pub matcher: UserAppMatcher,
    pub family: ContextFamily,
    pub scene_id: Option<String>,
}
```

Requirements:

- optionally assign a local scene to a semantic context family
- optionally map the latest detected native app or exact web host to a semantic family and, separately, to a local scene
- create a mapping only after an explicit user action; automatic detection never silently promotes a raw signal into persistent configuration
- confirmation shows the matcher type and an editable short label derived from the current OS app display name or exact host; saving the matcher and label requires one explicit confirmation
- sanitize the saved label, cap it at 40 characters, and never derive it from a window title, path, page title, or document name
- persist only an exact normalized bundle ID, executable identity, or web host selected by the user; never persist a window title, URL path/query, page text, or arbitrary regular expression
- custom mappings have classifier precedence over the built-in registry and can be edited, disabled, deleted, or reset to automatic behavior
- retain the latest mapping candidate in memory only until the next recording or app exit when the user does not save it
- user assignment overrides the built-in context tone
- semantic-family-to-scene assignments may sync only when the user explicitly includes settings in cloud backup
- custom app matchers remain device-local and are always excluded from cloud backup because bundle IDs, executable identities, and hosts are platform-specific context identifiers
- no silent training or style extraction from history

### 12.4 Multiple Shortcut Bindings

Requirements:

- up to three bindings each for Dictate, Ask, and Translate
- migrate each existing scalar binding into index zero of a new ordered binding list; index zero remains the primary binding for display and backward-compatible export
- detect conflicts before save
- use existing shortcut capture rows and icon actions
- do not add a separate shortcuts dashboard
- all bindings for one action share that action's existing hold/toggle behavior
- global shortcuts and platform-native shortcuts may coexist in a list
- native choices remain limited to the triggers the current monitors can represent: `Fn`, `Fn+Space`, and `Fn+LeftShift` on macOS; `RightAlt`, `RightAlt+Space`, and `RightAlt+LeftShift` on Windows
- the same trigger cannot be assigned to two actions, and unsupported native triggers are never serialized on another platform
- migration, downgrade-safe parsing, conflict detection, registration rollback, and restart persistence require automated coverage

## 13. Desktop UI Specification

### 13.1 Design Baseline

All changes must follow the existing desktop visual language:

- compact left navigation and settings sidebar
- 11px to 15px interface text
- existing background, border, accent, error, success, and warning tokens
- existing `6px` to `14px` radii according to the component already being extended
- existing `jelly-btn`, toggle, input, row, toast, and floating-note patterns
- Lucide icons where an icon exists
- stable dimensions with no content-driven capsule resizing

Do not introduce a new visual system while implementing this spec.

### 13.2 Global UI Constraints

Not allowed:

- new dashboard or context-management page
- large recommendation cards
- cards nested inside cards
- instructional feature copy in the working UI
- app-context confidence percentages in normal UI
- full URLs or selected-text previews
- remote image or logo loading
- a larger idle capsule
- a multi-turn Ask chat timeline

Allowed:

- a 14px to 16px local app logo or generic glyph
- one short app/context label
- compact inline forms and menus
- existing toast and error patterns
- a short tooltip/title for an ambiguous icon

### 13.3 Capsule

Idle state:

- no visual change
- keep the existing 36px circular capsule and OpenTypeless logo

Dictate recording/processing:

- do not increase height
- P0 does not add an app logo to the active capsule because the existing leading position communicates recording or processing state
- retain the recording dot, waveform, processing indicator, timer, and cancel action without displacement
- do not show long labels such as `Developer Collaboration`

Translate recording:

- show a compact active-language chip such as `JA` or `EN`
- chip opens a small menu containing configured translation targets
- menu uses current floating-menu styling
- target switching must not restart recording

Ask:

- retain current Ask icon, title, timer, thinking state, and floating note
- do not add app-context copy unless needed in a tooltip

### 13.4 History

Modify the existing 11px metadata line:

```text
[14px logo] Gmail  |  14:32  |  cloud
```

Requirements:

- no new card or additional metadata panel
- unknown context shows a generic AppWindow glyph and `General`
- search may match the short context label
- technical profile ID, confidence, domain, and intent confidence stay out of normal UI
- metadata order is `[logo] Context label · time · provider`; do not repeat the raw process name
- keep the row to one line with ellipsis; at narrow widths hide provider first, then truncate the context label

### 13.5 Settings: AI Polish and Scenes

P0 adds two compact rows to AI Polish settings using existing controls:

1. `Adapt writing to current app` toggle.
2. `Last dictation context` read-only row with a 14px logo/glyph and short mapped label.

The adaptation row also shows one restrained, noninteractive logo line for Gmail, Slack, Lark, WeChat, Google Docs, Notion, GitHub, and Cursor, followed by `+63`. Each 16px local logo has an accessible app name and tooltip. The line communicates representative coverage only; it is not a gallery, selector, or link, and it dims with the disabled toggle.

The row displays the snapshot actually used by the most recent dictation, not the app currently showing Settings. Hide the row when no snapshot exists. Do not add a live app monitor, timestamp, confidence value, domain, or explanatory card.

Scenes receives no additional P0 row. In P1, the `Last dictation context` row may expose one overflow action, `Use a different writing style...`, when an in-memory mapping candidate exists. The action opens a small existing-style menu for short label confirmation, semantic family, and optional scene assignment.

If custom mappings exist, the same menu may open one compact flat-list dialog to edit, disable, or delete only user-created mappings. Do not show the complete built-in registry, an app gallery, a logo grid, per-app cards, or a new navigation destination.

### 13.6 Settings: Dictionary

- add one search input using the History search/input pattern
- add one compact toolbar above the existing add-entry fields; place search at the left and Import/Export icon buttons at the right
- clicking an entry opens an inline edit state or existing-size dialog
- keep dictionary entries as one flat list; do not turn each word into a large card
- empty search and import-error states use current secondary text and toast patterns

### 13.7 Settings: Translation

- replace the single target selector with a compact ordered target list in P1
- use icon controls for add, remove, and reorder
- avoid explanatory cards
- show active target with the existing select/menu styling

### 13.8 Account: Signed Out

- add a text action `Forgot password?` aligned below the password field
- clicking it replaces the form content with a compact email request form in the same `max-w-[340px]` container
- success state says to check email and does not reveal account existence
- Back returns to Sign In
- no new navigation item

### 13.9 Account: Signed In

Extend the existing account information area with one Password row:

- credential account -> `Change password`
- OAuth-only account -> `Set password`
- linked account -> `Change password`

The action opens a centered existing-style modal, never an inline expanding form. The account page remains one compact `Security` row with one text action. The modal is at most 380px wide with a 10px radius, existing border/backdrop/shadow tokens, a 14px title, current/new/confirm fields as appropriate, and compact footer actions. It must not add a Security dashboard.

The modal focuses the first field, closes on Escape or backdrop click while idle, restores focus to the trigger, keeps validation and server errors inside the dialog, and cannot close while a password mutation is in progress.

After success:

- close the form
- show one success toast
- keep the current desktop session active after token rotation

### 13.10 App Logo Policy

- prefer an OS-provided native app icon when available without new permission prompts
- bundled brand marks may be used only as small local nominative identifiers
- registry support does not depend on a bundled brand asset; an app may ship with the correct profile and a family glyph
- custom mappings use the current OS-provided icon when safely available or the selected family's generic glyph
- never fetch logos at runtime
- use Lucide `AppWindow`, `Mail`, `MessageSquare`, `FileText`, `Code`, or `Headphones` fallbacks
- logos are not controls unless they have an explicit tooltip and action

### 13.11 Accessibility and Layout

- every icon button has `aria-label` and `title`
- keyboard focus order follows visual order
- password errors are associated with the relevant field
- Account inputs have persistent visible or visually hidden labels rather than relying on placeholders
- Account inputs use correct `autocomplete` values including `name`, `email`, `current-password`, and `new-password`
- the Sign In/Sign Up switcher exposes tab or equivalent selected-state semantics
- generic password-reset success and failure states use an appropriate polite `aria-live` region
- menus close on Escape and restore focus
- text does not overflow at the minimum supported desktop window size
- all new strings exist in all ten desktop locale files before release
- no font size scales with viewport width

## 14. Privacy and Cloud Contract

### 14.1 Local Data

- context profile and short app label may be stored with history when history is enabled
- raw process names, raw window titles, and browser hosts are classifier-only inputs and are not stored by default
- the only persistence exception is an explicit P1 custom mapping, which stores the user-approved normalized bundle ID, executable identity, or exact host plus the confirmed short label locally; it never stores a window/page title, path, query, document name, or page text
- full URL, selected text, and reset tokens are never stored
- personal style and dictionary suggestions remain local unless explicitly backed up
- custom app mappings are always excluded from TalkMore backup; only matcher-free family-to-scene assignments may be included through the existing explicit settings-backup choice

History storage migration:

- add `context_profile_id` and `context_label` fields; new entries write only mapped values such as `email.gmail` and `Gmail`
- keep target-process identity only in the in-memory `TargetAppGuard` used for focus restoration; do not reuse it as history metadata
- frontend events expose only the mapped context label/profile; target-process identity remains inside Rust
- in one transaction, map known legacy `app_name` values to the new fields, map unknown non-empty values to `general.native` / `General`, and clear the legacy raw value after successful migration
- older databases and exports remain readable, and migration rollback leaves the previous table intact

### 14.2 TalkMore Data

TalkMore may store:

- user, account, session, subscription, entitlement, and quota records
- operation IDs and numeric usage metadata needed for idempotent quota accounting

TalkMore must not persist or log:

- uploaded audio
- raw transcript
- polished output
- selected text
- Ask question or answer content
- full LLM messages
- password values or reset tokens
- raw process names, window titles, or browser hosts
- submitted email values or token-bearing links from password-reset diagnostic logs

Upstream STT/LLM provider handling must be documented separately. OpenTypeless must not claim end-to-end zero retention unless the selected provider and account contract support that claim.

### 14.3 Diagnostics

Allowed production diagnostics:

- stage name
- duration
- provider ID
- response status
- character/word counts
- context profile ID
- intent kind
- truncated boolean flags

Disallowed production diagnostics:

- content bodies
- selected text
- browser paths or query strings
- authorization headers
- session tokens
- email reset links

## 15. Success Metrics and Release Gates

This product is local-first, so P0 release decisions use content-free quality gates rather than mandatory user-content analytics.

### 15.1 Primary Quality Gates

- all semantic-output gates run against the pinned managed reference model and prompt version; certified BYOK/local model pairs run the same suite before entering the certified matrix
- the local registry validates with unique IDs, no conflicting exact matchers, no unreachable entries, and complete labels/icons
- P0 contains at least 30 reference app surfaces across Email, WorkChat, PersonalChat, Document, ProjectManagement, DeveloperCollaboration, PromptOrCode, Support, and Social
- supported context extended-corpus accuracy: at least 95 percent
- unknown-site false-positive classification: at most 2 percent in an unknown-context corpus containing at least 100 cases
- every context-output blocker preserves the shared fact set while satisfying its profile's required style and forbidden-adaptation invariants
- explicit intent true-positive rate: at least 95 percent in each enabled command locale, not only in an aggregate total
- destructive intent false positives: zero in the release blocker set
- critical fact preservation: 100 percent for names, numbers, dates, negations, identifiers, and commitments in blocker fixtures
- no additional LLM request in normal P0 dictation
- prompt composition includes exactly one semantic-family policy and at most one structured app override; adding registry entries does not increase prompt layers for unrelated apps

Every release report records desktop commit, TalkMore commit, operating system, model/provider identifier, prompt version, STT fixture source, temperature, corpus revision, and pass/fail counts. A percentage without the corpus size and revision is not a release result.

### 15.2 Performance Guardrails

- context detection adds no network request
- reading the cached context snapshot is at most 5ms p95 on supported native paths
- context refresh work never executes synchronously on the recording-start critical path
- indexed registry lookup remains below 2ms p95 in a synthetic 500-entry registry benchmark
- background context monitoring averages below 1 percent CPU over a five-minute idle/focus-switch fixture on the release baseline machines
- microphone activation p95 regresses by no more than 20ms from the Phase 0 platform baseline
- prompt composition remains local and sub-millisecond relative to provider latency
- no regression to microphone activation, cancellation, or focus restoration
- account recovery does not affect BYOK startup or dictation

### 15.3 UI Guardrails

- idle capsule dimensions unchanged
- active capsule height unchanged
- no horizontal overflow in Settings, History, Ask, or Account at the supported minimum window size
- locale parity test passes

### 15.4 Account Gates

- reset request, valid reset, expired token, reused token, credential change, OAuth-only set password, token rotation, and session invalidation all have automated coverage
- generic forgot-password response is identical for existing and unknown accounts at the UI contract level
- reset and change never log credentials or tokens
- production reset requests cannot redirect to localhost, a Tauri origin, an arbitrary trusted origin, HTTP, or an unsupported locale path
- the default public Better Auth reset-request route is disabled; only the canonical TalkMore wrapper is operational
- managed-cloud invalid-session, missing-session, quota, and BYOK provider-auth failures are distinguishable by machine contract

## 16. Required Acceptance Fixtures

The tables below are mandatory blocker examples, not the complete statistical corpus used for Section 15 percentages. Before implementation begins, check in versioned, content-safe fixture manifests with the following minimums:

- context: at least 300 cases across supported native apps and browsers, including every reference-tier profile and at least 100 unknown, conflicting, suffix-boundary, or adversarial contexts
- intent: at least 100 cases per enabled command locale, with at least half covering negation, quotation, discussion, code, identifiers, or other near misses
- thought-aware output: at least 50 blocker cases covering every critical-fact category and at least 100 extended cases
- account: deterministic integration cases for canonical callbacks, malicious callbacks, timing-independent UI responses, background email completion, token rotation, and concurrent invalid-session responses

Fixture files contain synthetic content only. Each manifest declares its revision and expected profile, intent, placement, or invariant so the test runner can report exact denominators.

### 16.1 Context

| Foreground Surface  | Available Signal                       | Expected Profile                    |
| ------------------- | -------------------------------------- | ----------------------------------- |
| Gmail in Chrome     | `mail.google.com`                      | `email.gmail`                       |
| Gmail in Safari     | title contains Gmail, host unavailable | `email.gmail` with lower confidence |
| Slack native        | process Slack                          | `chat.slack`                        |
| Slack web           | `app.slack.com`                        | `chat.slack`                        |
| Lark/Feishu native  | verified bundle/executable             | `chat.lark`                         |
| DingTalk native     | verified bundle/executable             | `chat.dingtalk`                     |
| WeChat native       | verified bundle/executable             | `message.wechat`                    |
| Google Docs         | `docs.google.com`                      | `doc.google_docs`                   |
| WPS Office          | verified bundle/executable             | `doc.wps`                           |
| Yuque web           | verified exact official host           | `doc.yuque`                         |
| Linear web          | verified exact official host           | `project.linear`                    |
| Figma native/web    | verified bundle or exact official host | `project.figma`                     |
| GitHub pull request | `github.com`                           | `dev.github`                        |
| GitLab              | verified bundle or exact official host | `dev.gitlab`                        |
| Cursor              | process Cursor                         | `code.cursor`                       |
| JetBrains IDE       | verified product bundle/executable     | `code.jetbrains`                    |
| Gemini              | verified exact official host           | `prompt.gemini`                     |
| Zendesk tenant      | normalized `*.zendesk.com` host        | `support.zendesk`                   |
| LinkedIn composer   | `linkedin.com`                         | `social.linkedin`                   |
| `not-slack.example` | unknown host                           | `general.browser`                   |
| unknown browser     | unknown host                           | `general.browser`                   |

Context-output blocker matrix:

Run the same synthetic semantic payload through every profile: `Tell Sam the launch is Friday, actually Monday; Monday is final; ask Sam to confirm.` Every result must contain Sam, Monday, and the confirmation request; must discard Friday as the corrected date; and must add no unspoken recipient, deadline, promise, issue number, or other fact.

| Context     | Required Style Invariant                                                       | Forbidden Adaptation                                       |
| ----------- | ------------------------------------------------------------------------------ | ---------------------------------------------------------- |
| Gmail       | complete, polite email-body sentences with natural paragraphing                | invented subject, greeting, recipient, or sign-off         |
| Outlook     | concise professional email body                                                | invented formality, policy, commitment, or attachment      |
| Slack       | one to three compact conversational paragraphs                                 | email greeting/signature or unnecessary headings           |
| Teams       | compact work-formal message                                                    | email chrome or unsupported organizational language        |
| Lark/Feishu | compact work message with a clear confirmation request                         | document template, email chrome, or invented owner         |
| WeChat      | one or two natural personal messages                                           | workplace status template, forced slang, or invented emoji |
| Google Docs | coherent document prose                                                        | chat shorthand or invented document title                  |
| Notion      | concise block-friendly prose; list only when the content supports one          | decorative template or unrelated headings                  |
| WPS Office  | coherent office-document prose                                                 | assumptions about file type, recipient, or document title  |
| Linear      | concise project update with an explicit requested action                       | invented assignee, priority, due date, estimate, or status |
| GitHub      | precise Markdown-friendly update with a clear requested action                 | invented issue/PR number, command, code, or technical fact |
| Cursor      | preserve spoken technical tokens and otherwise remain a direct writing request | generating code or renaming identifiers unless requested   |
| Gemini      | explicit prompt-like instruction preserving all supplied constraints           | invented role, context, tool, or desired output format     |
| Zendesk     | clear actionable message without unsupported policy or resolution              | invented empathy claim, refund, ownership, or SLA          |
| LinkedIn    | polished professional post/comment phrasing                                    | hashtags, engagement bait, or unsupported public claim     |
| General     | neutral clean dictation                                                        | app-specific greeting, structure, or tone assumptions      |

### 16.2 Thought-Aware Dictation

| Raw Speech                                                                          | Expected Result                                             |
| ----------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `Let's meet at seven, actually three`                                               | `Let's meet at three.`                                      |
| `Send this to Sarah, no, Sam`                                                       | `Send this to Sam.`                                         |
| `The deadline is Friday. One more thing... actually make the deadline Monday.`      | Monday replaces Friday; unrelated content remains.          |
| `Call it Open Type Less, one word, OpenTypeless`                                    | `Call it OpenTypeless.`                                     |
| `I do not want to cancel`                                                           | Negation remains unchanged.                                 |
| `Not bad, actually pretty good`                                                     | Both clauses remain; `actually` is not misread as deletion. |
| `For context only, do not include this: Sam is new. Tell Sam the launch is Monday.` | Output contains the message, not the explicit meta note.    |
| `The third step is deploy. First run tests. Second build the app.`                  | Ordered list 1 tests, 2 build, 3 deploy.                    |

### 16.3 Intent

| Utterance                                         | Selection | Mode      | Expected Intent  | Placement        |
| ------------------------------------------------- | --------- | --------- | ---------------- | ---------------- |
| `draft a follow-up email about tomorrow's launch` | no        | Dictate   | DraftInsert      | InsertAtCursor   |
| `I need to draft a follow-up email tomorrow`      | no        | Dictate   | DictateInsert    | InsertAtCursor   |
| `make this warmer`                                | yes       | Dictate   | RewriteSelection | ReplaceSelection |
| `what does this mean?`                            | yes       | Dictate   | AskSelection     | PopupAnswer      |
| `summarize this`                                  | yes       | Ask       | AskSelection     | PopupAnswer      |
| `search React tutorials on YouTube`               | no        | Ask       | Search           | OpenUrl          |
| `I should search YouTube later`                   | no        | Dictate   | DictateInsert    | InsertAtCursor   |
| any speech                                        | no        | Translate | TranslateInsert  | InsertAtCursor   |

### 16.4 Translation, Dictionary, and Custom App Mapping

| Case                              | Expected Result                                |
| --------------------------------- | ---------------------------------------------- |
| migrate existing `target_lang=ja` | targets contains `ja`; active is `ja`          |
| switch EN to JA while recording   | current run and next default use JA            |
| add duplicate target              | no duplicate row                               |
| edit dictionary entry             | same entry ID; updated values                  |
| import duplicate terms            | normalized deduplication                       |
| create correction from History    | explicit preview before save                   |
| map latest unknown exact host     | explicit family/scene mapping persists locally |
| dismiss mapping candidate         | candidate disappears without persistence       |
| delete custom app mapping         | built-in registry or General applies again     |
| attempt to persist title/path     | rejected; only exact approved matcher is saved |
| backup excludes app mappings      | no bundle ID, executable, or host is uploaded  |

### 16.5 Account

| Case                               | Expected Result                                               |
| ---------------------------------- | ------------------------------------------------------------- |
| unknown email reset request        | generic success; no email enumeration                         |
| credential user reset              | one-hour email link; password updates                         |
| OAuth-only set password            | verified signed-in user creates credential without unlinking OAuth |
| direct Better Auth reset request   | disabled public path; wrapper is the only operational entry   |
| client supplies localhost callback | callback ignored/rejected; canonical HTTPS callback is used   |
| client supplies Tauri callback     | callback ignored/rejected; no reset token reaches desktop     |
| unsupported locale callback        | locale falls back to the canonical supported default          |
| background reset email             | delivery completes after response through configured handler  |
| expired token                      | localized invalid/expired state; no password change           |
| reused token                       | rejected                                                      |
| desktop password change            | current password required; new bearer token persisted         |
| browser reset after desktop login  | old desktop token rejected; desktop clears managed-cloud auth |
| concurrent invalid cloud requests  | one local reset and one session-expired notification          |
| managed-cloud quota returns 403    | quota state shown; account remains signed in                  |
| BYOK provider returns 401          | provider error only; OpenTypeless account remains signed in   |

## 17. Dependencies and Risks

### 17.1 Dependencies

- OS/browser APIs for foreground context
- existing Tauri permissions and platform capability reporting
- Better Auth password and session endpoints
- Resend delivery and OpenTypeless domain configuration
- web deployment of the TalkMore reset page before exposing desktop Forgot password
- complete desktop and TalkMore locale keys for new account states
- a pinned, contract-tested Better Auth client/server version combination

### 17.2 Risks and Mitigations

Browser detection becomes brittle:

- use semantic families and normalized host rules
- keep title matching as lower-confidence fallback
- unknown contexts remain General
- refresh detection in the background and read a cache at recording start

The app registry becomes a hard-to-review collection of brand exceptions:

- use one typed registry schema and shared semantic-family policies
- allow at most one small structured override per app
- reject arbitrary built-in prompt strings, duplicate matchers, and unreachable entries in validation tests
- keep reference and extended tiers explicit so long-tail additions do not silently expand P0 release scope

Superapps expose chat, docs, tasks, and support inside one process or host:

- classify the app-level default conservatively
- use only reviewed high-confidence title markers for a sub-surface
- never inspect URL paths, page text, or DOM content in P0/P1
- let the user create an explicit local mapping when app-level behavior is not the desired family

Window titles or browser pages inject prompt instructions:

- treat every raw signal as untrusted classifier input
- send only internal profile/family/override identifiers to prompt composition
- discard raw signals after classification and exclude them from history, TalkMore, and production logs

Context style changes user meaning:

- hard semantic-fidelity prompt layer
- critical-fact fixtures
- user scene override
- context adaptation toggle

An arbitrary BYOK/local model cannot follow semantic-fidelity rules:

- apply release claims to the pinned managed reference model and certified model matrix
- keep unlisted models usable as best effort
- report provider/model identifiers in content-free diagnostics and fixture results

Intent router executes an accidental command:

- boundary-aware locale grammars with explicit prefixes and payload requirements
- negation, quotation, discussion, code, and identifier guards
- low-confidence fallback
- destructive false-positive blocker tests
- Ask remains nondestructive when ambiguous

Personalization creates privacy concerns:

- P1 is explicit and local
- custom app mappings persist only a user-approved exact bundle/executable/host and never a title or URL path
- custom app matchers never enter settings backup; matcher-free family-to-scene assignments remain opt-in
- no silent history training
- no post-insertion monitoring in P0/P1

Password reset leaks account existence:

- generic UI contract
- Better Auth synthetic work for unknown accounts
- nonblocking serverless-safe email delivery

Password reset redirects a token to a broadly trusted desktop or localhost origin:

- accept no callback URL from web or desktop clients
- construct a canonical same-origin HTTPS callback in TalkMore
- maintain a dedicated reset-callback allow list separate from CORS and Better Auth trusted origins
- test production configuration against localhost, Tauri, HTTP, and unsupported paths

Desktop loses its current session after password change:

- consume returned replacement token before refreshing account data
- integration test localStorage and Rust token state

Better Auth client/server versions drift:

- treat TalkMore's server contract as authoritative
- pin both dependency resolutions for the release
- run request-reset, reset, list-accounts, change-password, bearer-session, and token-rotation contract tests before any version upgrade
- do not bundle an unrelated Better Auth server upgrade into this feature unless its migration and regression suite pass independently

Cloud reset leaves stale desktop UI:

- stable TalkMore machine error code
- dedicated Rust error and Tauri event
- one idempotent frontend invalidation action
- one notification and complete managed-cloud state reset

## 18. Delivery Plan

### Phase 0: Freeze Fixtures and Baselines

- check in the Section 16 blocker fixtures and minimum-size versioned corpora
- freeze the reference-tier registry manifest and validate every declared matcher against a positive and forbidden near-match set
- pin the managed reference model, prompt version, temperature, and initial certified-model matrix
- convert fixtures into Rust, Vitest, and TalkMore tests with exact denominators
- record current p50/p95 recording-start and completion timings
- record current capsule/window dimensions
- confirm no content logging in cloud routes

### Phase 1: Context Foundation

- ContextProfile model
- typed app registry, validator, reference profiles, and verified extended mappings
- background ContextSnapshot cache and OS focus refresh
- browser/native classifiers outside the recording-start critical path
- semantic family prompts and app overrides
- context history diagnostics
- thought-aware prompt rules

Ship behind `context_adaptation_enabled` with the default enabled.

### Phase 2: Account Integrity, Parallel Track

TalkMore first:

- canonical request-password-reset wrapper and dedicated callback allow list
- reset email sender and template
- reset page
- Better Auth reset/session options, sanitized logger, and background-task handler
- stable authenticated-cloud error envelope and machine codes
- web Forgot/Change actions
- Better Auth cross-version contract tests and pinned dependency resolution

Desktop second:

- Forgot request UI
- account-type-aware Password row
- change-password flow
- token rotation, dedicated Rust cloud-session error/event, and idempotent frontend invalidation

This phase can ship independently from Context Foundation.

### Phase 3: Intent Router

- shared router types
- English and Simplified/Traditional Chinese boundary-aware grammars
- normal dictation integration
- selected-text integration
- Ask draft/search routing
- output placement and focus restoration
- false-positive release tests

### Phase 4: P1 Workflow Improvements

- translation target list and capsule chip
- dictionary search/edit/import/export
- context-to-scene assignments and explicit local custom app mappings
- multiple shortcuts
- ordered shortcut-list storage migration and platform-native trigger coverage

## 19. Engineering File Map

### 19.1 OpenTypeless Backend

Modify or create:

- `src-tauri/src/app_detector/mod.rs`
- `src-tauri/src/app_detector/profiles.rs` (new)
- `src-tauri/src/app_detector/cache.rs` (new)
- `src-tauri/src/intent.rs` (new)
- `src-tauri/src/llm/prompt.rs`
- `src-tauri/src/llm/mod.rs`
- `src-tauri/src/pipeline.rs`
- `src-tauri/src/selection.rs`
- `src-tauri/src/commands/ask.rs`
- `src-tauri/src/commands/dictionary.rs`
- `src-tauri/src/storage/mod.rs`
- `src-tauri/src/error.rs`
- `src-tauri/src/lib.rs`

### 19.2 OpenTypeless Frontend

Modify or create:

- `src/stores/appStore.ts`
- `src/stores/authStore.ts`
- `src/lib/auth-client.ts`
- `src/lib/auth-invalidation.ts` (new)
- `src/lib/api.ts`
- `src/lib/tauri.ts`
- `src/components/Capsule/*`
- `src/components/History/index.tsx`
- `src/components/Settings/ScenesPane.tsx`
- `src/components/Settings/DictionaryPane.tsx`
- `src/components/Settings/LlmPane.tsx`
- `src/components/Settings/GeneralPane.tsx`
- `src/components/AccountPage/index.tsx`
- all files under `src/i18n/locales/`

### 19.3 TalkMore

Modify or create:

- `src/lib/auth.ts`
- `src/lib/opentypeless-auth-errors.ts` (new)
- `src/lib/email.ts`
- `src/lib/email-template.ts`
- `src/app/[locale]/login/LoginContent.tsx`
- `src/app/[locale]/account/AccountContent.tsx`
- `src/app/[locale]/reset-password/page.tsx` (new)
- `src/app/[locale]/reset-password/ResetPasswordContent.tsx` (new)
- `src/app/api/opentypeless/auth/request-password-reset/route.ts` (new)
- TalkMore message locale files
- auth/account integration tests

### 19.4 Required Test Areas

- context classification by process, title, and host
- registry uniqueness, matcher conflict, dot-boundary suffix, tier, label/icon, and forbidden near-match validation
- reference and shipped extended app matcher fixtures
- explicit custom mapping creation, precedence, reset, local persistence, and backup exclusion/inclusion
- background context refresh, stale-cache fallback, and recording-start cache latency
- raw-signal prompt-injection and persistence blockers
- prompt layering and semantic invariants
- managed reference and certified-model matrix gates
- thought-aware fixed fixtures
- intent routing and false positives
- output focus restoration
- storage migration
- desktop locale parity
- capsule and settings layout
- TalkMore password reset and session revocation
- TalkMore canonical reset callback and malicious redirect rejection
- TalkMore background email completion and reset-log redaction
- TalkMore reset-page token stripping, `noindex`, and no-referrer behavior
- desktop token rotation and cross-Rust/TypeScript invalid-session behavior
- BYOK 401 isolation

## 20. Non-Goals

- mobile keyboard implementation
- browser extension
- field-level DOM inspection
- guaranteed chat-versus-doc-versus-task detection inside a superapp when only one process/host is available
- silent or autonomous web browsing
- arbitrary website actions
- multi-turn Ask chat
- automatic style learning from private history
- monitoring edits after insertion
- cloud-only personalization
- replacing the provider system
- large capsule or dashboard redesign
- enterprise administration
- account deletion or email change in this initiative

Account deletion and email change remain valid future account-lifecycle work, but they are not required to close the password-recovery gap.

## 21. Decision Log

Resolved:

1. Use exact app/site detection mapped to semantic context families with small app overrides.
2. Enable context adaptation by default, with a restrained toggle.
3. Keep active user scenes authoritative over automatic style.
4. Do not add an extra LLM request to every dictation.
5. Use deterministic intent routing in P0.
6. Keep explicit search allow-listed and non-browsing.
7. Treat TalkMore as the cloud/account source of truth.
8. Complete password reset on the web; allow request initiation from web and desktop.
9. Allow password change in web and desktop, with desktop bearer-token rotation.
10. Preserve the existing desktop visual language and add no new major surface.
11. Use tiny local app logos/glyphs only where they fit existing rows and dimensions.
12. Keep personal style explicit and local in P1.
13. Keep context detection off the recording-start critical path and read a background snapshot cache.
14. Treat raw process, title, and host signals as untrusted classifier-only data.
15. Apply semantic release claims to a pinned managed reference model and certified model matrix; keep other models best effort.
16. Enable deterministic destructive/draft routing only for locale grammars that pass their own release corpus.
17. Use a TalkMore wrapper that constructs the only permitted password-reset callback; never accept a client callback.
18. Distinguish managed-cloud session invalidation from BYOK provider authentication with a machine error contract, dedicated Rust event, and idempotent frontend action.
19. Show `Last dictation context`, not a live `Current context`, and place it in AI Polish settings.
20. Do not add an app logo to the active P0 capsule; preserve recording and processing state indicators.
21. Scale built-in app coverage through one typed local registry with shared family policies and at most one structured override per app.
22. Separate release-blocking reference profiles from verified extended family mappings so long-tail coverage does not create an unbounded P0 gate.
23. Let users explicitly map a long-tail app or exact host to a family/scene in P1 without storing titles, paths, or page content and without adding an app-management dashboard.
24. Keep the Account security row collapsed and open password mutation only in an existing-style modal.
25. Show representative app coverage as eight small local logos plus `+63`, without making the logos interactive or adding an app gallery.

Non-blocking implementation choices:

- exact OS icon extraction implementation per platform

These choices must satisfy Section 13 and do not change product scope.

## 22. Definition of Done

### 22.1 P0-A: Context-Aware Writing

P0-A may ship independently when:

- the asynchronous detector and snapshot cache pass stale, unavailable-permission, cold-start, app-switch, and browser-switch cases without delaying microphone activation.
- the registry contains at least 30 reference app surfaces across all required families and passes uniqueness, conflict, suffix-boundary, label/icon, and unreachable-entry validation.
- every reference profile and every shipped extended profile passes its declared positive and forbidden near-match fixtures.
- Context Detector V2 passes every blocker and the versioned extended-corpus thresholds.
- the same-payload context-output matrix visibly differs by app family while preserving the identical required fact set.
- only internal profile/family/override identifiers enter prompts; raw process, title, and host signal blockers pass.
- legacy History app metadata migrates transactionally to mapped profile/label fields, while focus restoration continues to use only the ephemeral Rust target guard.
- the managed reference model and prompt version pass all critical-fact, correction, and thought-aware blocker cases.
- certified model entries independently pass the same suite; unlisted models are not represented as certified.
- Gmail, Slack, Lark/Feishu, WeChat, WPS Office, Linear, GitHub, Cursor, Gemini, Zendesk, LinkedIn, and an unknown browser are manually verified on each supported platform where the relevant surface exists.
- AI Polish and History UI remain within existing row and window dimensions, and the active capsule is unchanged.
- P0-A Rust, frontend, locale, privacy, and layout tests pass.

### 22.2 P0-B: Voice Intent and Output Placement

P0-B may ship independently when:

- each enabled spoken-language grammar passes its own minimum-size positive and near-miss corpus.
- destructive intent false positives are zero in every release blocker set.
- disabled or unsupported spoken languages fall back nondestructively.
- output placement, focus restoration, URL encoding, allow-listed search, and per-route kill switches pass automated tests.
- P0-B Rust, frontend, and locale tests pass.

### 22.3 P0-C: Account Integrity

P0-C may ship independently when:

- password reset and change work end to end through TalkMore web and desktop.
- the reset wrapper ignores client callbacks and production rejects localhost, Tauri, HTTP, and unsupported callback paths.
- the default public Better Auth request-reset path is disabled and cannot bypass the wrapper.
- background reset email delivery completes under the deployed serverless task handler.
- expired/reused reset token, verified OAuth-only set-password, token rotation, and session revocation cases pass.
- managed-cloud invalid session, missing session, quota, and BYOK provider authentication remain machine-distinguishable.
- concurrent invalid-session failures clear managed-cloud state once without deleting BYOK/local configuration or history.
- no production log includes reset-request email values, audio, transcript, selected text, prompt content, authorization data, or reset tokens.
- P0-C TalkMore, desktop, cross-version auth-contract, locale, and accessibility tests pass.

### 22.4 Combined P0 Milestone

The roadmap may mark combined P0 complete after P0-A, P0-B, and P0-C have each met their independent Definition of Done. Combined status does not block an already qualified track from shipping.

### 22.5 P1

P1 is complete when:

- translation targets can be configured and switched during recording.
- dictionary entries can be searched, edited, imported, exported, and converted from explicit History corrections.
- context-to-scene personalization is explicit and optionally backed up; custom app mappings are explicit, removable, device-local, and never uploaded.
- the compact mapping menu and flat user-mapping dialog work without exposing the full built-in registry or adding a navigation destination.
- multiple shortcut bindings migrate, register, persist across restart, roll back on registration failure, and work without conflict or a separate UI surface.

## 23. Revision 2.1 Implementation and Verification Record

### 23.1 Implemented Revisions

The initial feature implementation was completed in isolated worktrees without changing either original checkout. Revision 2.1 was completed on the same branches.

| Repository | Branch | Initial Feature Head | Baseline |
| --- | --- | --- | --- |
| OpenTypeless desktop | `codex/typeless-p0-p1` | `932745a` | `origin/main@e76ee86` |
| TalkMore cloud/web | `codex/typeless-p0-p1` | `7fce777` | `origin/main@dd7d023` |

The initial OpenTypeless implementation contains 25 scoped commits covering account integrity, context adaptation, typed intent routing, translation, dictionary workflows, local mappings, multiple shortcuts, UI, fixtures, and release-gate fixes. Revision 2.1 adds focused specification, desktop UI, packaged-app verification, and test-runner stabilization commits on the same branch. The TalkMore branch contains six scoped commits covering account recovery, stable cloud-auth errors, desktop metadata validation, backup privacy, and test stabilization.

### 23.2 User-Visible Outcome

| User action | Result after this implementation |
| --- | --- |
| Dictate in Gmail or Outlook | Produces a complete professional email body with natural paragraphs, without inventing a greeting, recipient, subject, or sign-off. |
| Dictate in Slack, Teams, Lark, DingTalk, or WeCom | Produces a shorter work-chat message with low formatting overhead. |
| Dictate in Messages, WhatsApp, Telegram, or WeChat | Produces a compact natural personal message without forcing workplace structure or invented emoji. |
| Dictate in Docs, Notion, Word, WPS, Confluence, or Yuque | Produces coherent document prose and uses lists only when the speech supports them. |
| Dictate in Linear, Jira, Figma, GitHub, GitLab, Cursor, VS Code, or JetBrains | Preserves issue IDs, identifiers, paths, commands, Markdown, and technical wording while using the relevant project, collaboration, or prompt style. |
| Use an unknown app | Falls back to neutral General behavior; it is not guessed into a sensitive family from a weak substring. |
| Say an exact draft command | Inserts a generated draft into the guarded active target when the locale grammar and route flag allow it. |
| Ask about selected text | Opens the compact answer surface and does not destructively replace the selection. |
| Give an explicit selected-text rewrite or translation command | Replaces the selection only after target/focus verification. |
| Ask for an allow-listed search | Opens an encoded Google, YouTube, Amazon, or GitHub search; mentions and negated/quoted commands remain text. |
| Configure Translate | Stores one to five unique targets, supports reordering/removal, and switches the active target from the recording capsule without restarting recording. |
| Maintain vocabulary | Searches and edits terms/rules, previews TXT/CSV/JSON import, commits atomically, exports safe JSON/CSV, and creates corrections from History only after confirmation. |
| Use a long-tail or incorrectly classified app | Saves an explicit exact bundle ID, executable, or host to a family and optional scene on this device; the mapping can be disabled, edited, deleted, or reset. |
| Attach multiple keyboards or use alternate triggers | Stores up to three ordered bindings for each of Dictate, Ask, and Translate, with global conflict checks and legacy-primary compatibility. |
| Forget a password | Requests a generic OpenTypeless reset email from web or desktop and completes reset on the localized HTTPS web page. |
| Change or set a password | Credential users provide the current password and receive a rotated session; verified OAuth-only users can set a credential without unlinking OAuth. |
| Lose or revoke a cloud session | The next managed-cloud request signs out cloud state once, keeps local history and BYOK settings, and shows one concise notification. |

### 23.3 Final Context Contract

The detector starts in the background and writes an immutable `ContextSnapshot` cache. Recording start reads the cache and never performs browser automation or process discovery synchronously. A missing or stale snapshot returns General and schedules refresh.

Built-in coverage is data-driven:

- 33 `Reference` profiles with bundled 14px to 16px local WebP marks
- 38 `Extended` profiles using family glyphs unless a reviewed asset is present
- 71 total built-in profiles
- exact host, dot-boundary suffix, bundle ID, executable, process alias, and anchored title indexes
- registry validation for duplicate IDs, conflicting exact matchers, malformed profiles, and matcher reachability
- positive and forbidden-near-match tests for every matcher declared by a shipped profile
- a synthetic 500-profile indexed lookup benchmark with a 2ms p95 ceiling

Classification precedence is implemented as:

1. enabled explicit local mapping
2. exact host or reviewed host suffix for a supported browser
3. native bundle ID or executable
4. reviewed process alias
5. anchored title fallback tied to a known process/host
6. safe family fallback
7. `general.browser` or `general.native`

Only the following normalized values can cross from detection into prompt composition or History:

```text
profile_id, family, mapped label, mapped icon key, structured override_id
```

Raw process names, executable paths, window titles, browser paths, queries, fragments, and page content are excluded. The exact process/bundle identity used by `TargetAppGuard` remains ephemeral in Rust and is used only for output-placement safety.

### 23.4 Final Prompt and Intent Contract

The prompt composer applies one shared thought-aware policy, exactly one semantic-family policy, and at most one structured app override. The manual scene and explicit custom prompt remain authoritative for tone, but cannot override operation, language, safety, or semantic-fidelity constraints. No context feature adds a second model request.

The deterministic intent router is shared by Dictate, Ask, Translate, and selected-text flows. Release grammars and fixture counts are:

| Spoken locale | Versioned cases | Supported route behavior |
| --- | ---: | --- |
| English | 277 | draft, rewrite, translation, question, and allow-listed search |
| Simplified Chinese | 262 | draft, rewrite, translation, question, and allow-listed search |
| Traditional Chinese | 262 | draft, rewrite, translation, question, and allow-listed search |

Each corpus includes positive commands plus negated, quoted, discussed, ambiguous, code-like, identifier, empty-payload, wrong-mode, and disabled-flag cases. Unsupported spoken locales use nondestructive fallback behavior.

Output placement is a typed executor contract:

| Intent | Placement |
| --- | --- |
| `DictateInsert`, `DraftInsert`, `TranslateInsert` | `InsertAtCursor` |
| `RewriteSelection`, `TranslateSelection` | `ReplaceSelection` |
| `AskSelection`, `OpenQuestion` | `PopupAnswer` |
| `Search` | `OpenUrl` through the provider allow list |

The executor restores and validates the captured target before insert/replace. A target mismatch fails closed rather than writing into the newly focused app.

### 23.5 Final Account and Cloud Contract

TalkMore is the only password and managed-session authority. The implemented public OpenTypeless account endpoints are:

```text
POST /api/opentypeless/auth/request-password-reset
POST /api/opentypeless/auth/set-password
```

The reset wrapper accepts only `{ email, locale }`, normalizes the email, ignores unknown redirect fields, constructs the callback from `BETTER_AUTH_URL`, and returns the same `202 {"ok":true}` body for an accepted request whether or not the account exists. It applies per-IP and SHA-256 normalized-email buckets. The public Better Auth `/request-password-reset` path is disabled.

Password reset uses a one-hour token, revokes sessions, strips the token from the visible reset-page URL, and sends OpenTypeless-branded HTML and text email. Reset, verification, and password-change log values pass through token/URL/email sanitization. Next.js `after()` is configured as the Better Auth background-task lifetime hook.

Password capability is `present`, `none`, or `unknown`, derived from Better Auth account providers. Mutation actions remain hidden when capability is unknown. Verified OAuth-only accounts use the authenticated TalkMore wrapper, which rejects unverified users and users that already have a credential provider before calling the server-side password API.

Managed authenticated APIs use the stable envelope:

```json
{
  "error": {
    "code": "AUTH_SESSION_INVALID | AUTH_REQUIRED | QUOTA_EXCEEDED",
    "message": "..."
  }
}
```

The desktop maps `AUTH_SESSION_INVALID` to a dedicated Rust error and one `auth:session-invalid` event. A single idempotent frontend transition clears the browser token and Rust token, resets account/quota state, and retains local data and BYOK credentials. An upstream BYOK provider `401` is never interpreted as OpenTypeless account invalidation.

### 23.6 Final P1 Data Contracts

Translation configuration is backward compatible:

```text
translation.targets: ordered unique list, minimum 1, maximum 5
translation.active_target: member of targets
target_lang: legacy mirror of active_target
```

Dictionary import has the following hard limits and guarantees:

- UTF-8 only
- TXT, structured CSV, and JSON/backup-bundle input
- 1 MiB maximum input size
- 10,000 maximum rows
- Unicode normalization before deduplication
- preview report before mutation
- one database transaction for commit and rollback on any write failure
- CSV quoting plus formula-prefix neutralization for `=`, `+`, `-`, and `@`
- export contains dictionary terms and correction rules only

Custom app mappings are persisted in a dedicated local `context-mappings.json`, separate from normal synchronized settings. Candidate values are generation-scoped and expire when a later recording replaces them or the app exits. Persisted matcher types are limited to:

```text
native_bundle_id | native_executable | exact_web_host
```

Family-to-scene assignments may enter explicit settings backup. Exact app mappings, candidate values, process identities, executable identities, bundle IDs, and hosts are removed by both the desktop allow-list serializer and the TalkMore recursive backup sanitizer.

The shortcut contract keeps existing scalar bindings as compatibility mirrors while storing ordered lists:

```text
dictationBindings: ordered list, length 1 to 3
askBindings: ordered list, length 0 to 3
translateBindings: ordered list, length 0 to 3
```

Save validates duplicate bindings across all primary and secondary roles, validates native trigger support for the current platform, registers the complete candidate set, and rolls back to the previous registration/configuration if any registration fails.

### 23.7 Desktop UI Delta

No route, navigation destination, dashboard, app gallery, or large card was added.

| Existing surface | Implemented change |
| --- | --- |
| Capsule | Idle state and active height remain unchanged; Translate adds a stable compact language chip and existing-style target menu. |
| History | Existing metadata line adds a small local logo/glyph and mapped context label; provider hides first at narrow width; correction creation uses an explicit small dialog. |
| AI Polish | Adds the existing-style context-adaptation toggle, an eight-logo representative coverage line with `+63`, last-dictation context row, and concise model capability status. |
| Scenes/context | Last-context overflow opens a compact mapping form; only user-created mappings appear in one flat management dialog. |
| Dictionary | Adds one search field, compact Import/Export icon actions, inline maintenance, preview dialog, and unchanged flat-list density. |
| Translation settings | Replaces one selector with an ordered compact target list using icon actions. |
| General shortcuts | Reuses existing rows and capture controls; secondary bindings are inline and capped at three. |
| Account signed out | Adds `Forgot password?` and swaps the same 340px form container to request/success/back states. |
| Account signed in | Adds one Password row whose Change/Set action opens a compact 380px modal; no inline form or Security page. |

Reference app logos are local nominative identifiers only. Unknown and extended profiles fall back to the existing Lucide family glyphs. No logo is loaded remotely, and no app logo displaces recording, processing, timer, waveform, or cancel state in the active capsule.

All new desktop strings are present in the ten existing locale files. TalkMore account strings are present in all 33 existing web locale files.

### 23.8 Privacy Matrix

| Data | Memory | Desktop persistence | TalkMore request | TalkMore persistence/backup |
| --- | --- | --- | --- | --- |
| Mapped profile ID/family/label/icon | yes | History when enabled | normalized metadata only | validated for request use; not content telemetry |
| Raw window title | classifier only | no | no | no |
| Browser host | classifier only | only after explicit local mapping | no | always stripped |
| URL path/query/fragment | no | no | no | no |
| Bundle ID/executable matcher | target guard or candidate | only explicit local mapping | no | always stripped |
| Selected text | active operation only | no | only when required for the chosen operation | not logged, persisted, or backed up by this feature |
| Reset token/password/session token | active auth flow only | bearer session in existing desktop `localStorage` plus Rust runtime state; reset token is web-memory-only and removed from the URL | required auth endpoint only | never logs/backups |
| Family-to-scene assignment | yes | settings | only through explicit settings backup | allowed after sanitization |

### 23.9 Automated Verification Evidence

The following gates were run in the isolated feature worktrees across 2026-07-11 and 2026-07-12:

| Repository | Command | Result |
| --- | --- | --- |
| OpenTypeless Rust | `cargo fmt --all` | completed |
| OpenTypeless Rust | `cargo test -q` | 434 passed, 0 failed |
| OpenTypeless frontend | `npm test -- --reporter=dot` | 36 files, 320 passed, 0 failed |
| OpenTypeless frontend | `npm run lint` | passed; three pre-existing warnings only |
| OpenTypeless frontend | `npm run build` | production Vite build passed |
| OpenTypeless macOS bundle | `npm run tauri -- build --debug --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'` | Debug `.app` bundle passed |
| TalkMore | `npm test -- --reporter=dot` | 69 files, 316 passed, 0 failed |
| TalkMore | `npm run build` | Next.js 16 production build passed; 5,487 static pages generated |

TalkMore production build used the existing local Vercel production environment only for the build process. No environment value was copied, printed, modified, or committed.

Automated coverage includes registry validation and matching, snapshot staleness and refresh, raw-signal privacy, prompt layering, intent corpora, output placement and target guards, translation migration/switching, dictionary parsing/rollback/export, mapping candidate lifetime and persistence, backup sanitization at both boundaries, shortcut migration/conflict/rollback, account reset/set/change contracts, token rotation, concurrent invalidation, locale parity, and compact component interaction.

Revision 2.1 packaged-desktop verification on 2026-07-12 covered the normal 900x700 window and the configured 720x480 minimum. The Account security row remained collapsed; the password dialog opened without page reflow, focused the correct first field, closed on Escape, restored trigger focus, stayed fully inside the minimum viewport, and rendered above the existing update notification. AI Polish rendered all eight representative local marks plus `+63` without horizontal overflow. No password was submitted and no macOS permission was changed.

### 23.10 Release Controls and Rollback

- `context_adaptation_enabled` disables automatic app style while preserving normal polish.
- `voice_routing_flags.draft_insert`, `.rewrite_selection`, `.translate_selection`, and `.search` disable individual command routes independently.
- deleting or disabling a local app mapping immediately restores built-in classification.
- removing a family-to-scene assignment restores automatic family policy.
- legacy `target_lang`, `hotkey`, and `ask_hotkey` mirrors allow older readers to retain primary behavior.
- failed shortcut registration restores both the previous registered shortcuts and the previous saved configuration.
- account recovery is a TalkMore-owned track and does not gate local/BYOK dictation startup.
- managed-cloud invalidation clears only managed identity/quota state and is reversible through sign-in.

### 23.11 Deployment Validation Still Required

The implementation and repository gates are complete. The following checks require production services or physical operating-system surfaces and therefore remain release-operator gates:

1. Verify one live Resend reset and password-change email on the production OpenTypeless domain, including one-hour expiry and one-time reuse rejection.
2. Run the reference app recognition matrix on release macOS and Windows machines, plus Linux X11/Wayland fallback checks where supported.
3. Verify Fn and RightAlt native combinations with actual external keyboards and confirm focus restoration in representative native and browser apps.
4. Run the thought-aware same-payload matrix against the pinned managed model/prompt version and record model, temperature, corpus revision, and exact pass counts.
5. Repeat the completed macOS 900x700 and 720x480 visual checks on the Windows release runtime and any supported Linux packaging target before those platforms ship.

These checks do not require additional product UI or cloud data collection. A failed check blocks release of the affected track and uses the controls in Section 23.10; it does not silently weaken classifier, intent, privacy, or account contracts.
