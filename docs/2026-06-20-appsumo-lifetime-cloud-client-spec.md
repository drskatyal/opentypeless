# OpenTypeless AppSumo Lifetime Cloud Client Spec

Date: 2026-06-20
Project: `opentypeless`
Companion spec: `talkmore/docs/2026-06-20-appsumo-lifetime-cloud-api-spec.md`
Status: proposed for implementation after AppSumo pricing approval

## 1. Executive Summary

OpenTypeless needs a client-side AppSumo readiness pass so lifetime buyers understand their cloud benefit as monthly cloud words, not STT hours or LLM tokens. The desktop app should keep BYOK as the unlimited/private path, while the managed cloud path becomes a clear AppSumo entitlement with a single usage meter, predictable over-quota behavior, and wording that matches the AppSumo listing.

The launch-safe AppSumo offer is a lifetime deal with three license tiers:

| Tier           | Price | Cloud words per month | Positioning                   |
| -------------- | ----: | --------------------: | ----------------------------- |
| License Tier 1 |   $59 |               200,000 | Solo daily dictation          |
| License Tier 2 |  $149 |               700,000 | Power users and heavy writing |
| License Tier 3 |  $299 |             1,250,000 | Founder/operator heavy usage  |

This is competitive with current AppSumo voice writing deals without overexposing cloud COGS. WriteVoice lists $59/$139/$249 for 200,000/500,000/1,250,000 monthly dictation words. AI Dictation stacks $49 codes with 200k/400k/800k/1.2m words across the first four code levels. VoiceDash listed $59/$149/$299/$499 with 200k/600k/1.4m/3m monthly words and team features. Letterly is the outlier with $89+ lifetime codes and no word caps, but its economics and positioning are different enough that OpenTypeless should not copy "unlimited cloud" without stronger local/offline cost controls. A stretch Tier 3 of $329-349 for 1,500,000 words can be tested only after payout and COGS are proven.

## 2. Current Client Baseline

Reviewed against local `main`, which is aligned with `origin/main` at commit `1cfda30 docs: improve github conversion`. The worktree has unrelated local changes, so this spec should be implemented in a fresh branch without reverting existing edits.

Relevant implementation today:

1. `src/lib/api.ts`
   - `SubscriptionStatus` only supports `plan: 'free' | 'pro'`.
   - Status payload is split into `sttSecondsUsed`, `sttSecondsLimit`, `llmTokensUsed`, and `llmTokensLimit`.
   - `proxyLlm()` only sends `messages`; no mode metadata is sent to TalkMore.
2. `src/stores/authStore.ts`
   - Auth state mirrors the token/hour quota model.
   - Quota warnings fire separately for STT and LLM at 90 percent.
3. `src/components/AccountPage/index.tsx`
   - Account details show Free/Pro only.
   - Quota card shows two bars: STT hours/minutes and LLM k tokens.
   - Subscription management assumes Pro subscription portal for all paid users.
4. `src/components/UpgradePage/index.tsx`
   - Upgrade card shows `$4.99 / month`.
   - Pro quota card uses STT hours and LLM tokens.
5. `src/components/Settings/LlmPane.tsx`
   - Cloud LLM card says "Cloud LLM (Pro)" and uses Pro subscription copy.
6. `src-tauri/src/llm/cloud.rs`
   - Builds system prompt and messages locally, then sends `{ messages, stream }` to `/api/proxy/llm`.
   - Detects 403 quota errors and maps them to `AppError::LlmQuota`.
   - Does not send app type, selected text mode, translation state, or quota-intent metadata as separate fields.
7. `src-tauri/src/stt/cloud.rs`
   - Uploads WAV audio to `/api/proxy/stt`.
   - Detects quota-shaped 403 responses and maps them to `AppError::Quota`.
8. `src-tauri/src/commands/stt.rs`
   - Cloud STT and Cloud LLM connection tests call `/api/subscription/status`.
   - Both cloud tests currently treat only `plan === "pro"` as active cloud access, so AppSumo plans would fail connection checks unless this is changed.
9. `src/components/Onboarding/LlmSetupStep.tsx` and `src/components/Onboarding/SttSetupStep.tsx`
   - Onboarding still renders API-key-oriented fields and disables test buttons when API keys are empty.
   - Cloud onboarding must use sign-in plus entitlement status, not local API keys.
10. `src-tauri/src/llm/prompt.rs`

- Prompt already has useful product differentiation: app-aware formatting, custom dictionary, translation, selected-text command mode, and prompt-injection protection.
- Ordinary transcription is correctly treated as content only; spoken commands should execute only when selected-text mode has actual selected text.

## 3. Product Decision Review

The AppSumo offer should not be implemented as "current Pro, but lifetime." That shortcut would create two problems:

1. The user-facing value would stay confusing because the app would still show hours and tokens.
2. Lifetime buyers, monthly subscribers, refund states, and future upgrades would all share one `pro` flag, making entitlement bugs likely.

Client decision:

1. Treat AppSumo as a distinct paid source: `source: 'appsumo'`.
2. Treat words/month as the canonical user-facing quota.
3. Keep BYOK visually separate and unlimited.
4. Hide exact cloud model names in the product UI.
5. Keep current monthly Pro subscription available until a separate pricing decision replaces it.

## 4. Goals

1. Show AppSumo license tier, cloud words used, cloud words limit, and reset date in Account and Upgrade surfaces.
2. Replace customer-facing "STT hours" and "LLM k tokens" with one cloud words meter for managed cloud usage.
3. Make over-quota behavior understandable: cloud quota used up, switch to BYOK or wait for reset.
4. Preserve backward compatibility with the existing TalkMore status response during rollout.
5. Keep selected-text mode behavior explicit and testable.
6. Send enough request metadata to TalkMore so the server can route models and quota accurately without parsing prompts only.
7. Avoid promising unlimited managed cloud usage in the desktop UI.

## 5. Non-Goals

1. Do not implement AppSumo license activation in the desktop app unless the web flow is unavailable. Activation should primarily happen on opentypeless.com.
2. Do not remove BYOK providers.
3. Do not expose model names, token counts, or provider internals to end users.
4. Do not implement team seats in OpenTypeless for this release.
5. Do not add offline/local model features as part of this AppSumo client pass.

## 6. Target Users

Primary user:

1. AppSumo buyer who wants "it just works" cloud dictation and AI polish after signing in.
2. Uses OpenTypeless in normal desktop text fields across email, docs, chat, and browser apps.
3. Thinks in words/month, not seconds, tokens, or provider models.

Secondary user:

1. Existing open-source/BYOK user.
2. Wants unlimited/private usage with their own API keys.
3. Should not feel forced into cloud or AppSumo.

## 7. User Stories and Requirements

### Story 1: AppSumo buyer sees their actual license

As an AppSumo buyer, I want Account to show my lifetime license tier and word quota, so I can trust the app is connected to my purchase.

Acceptance criteria:

1. Account page displays plan names:
   - `Free`
   - `Pro`
   - `AppSumo Tier 1`
   - `AppSumo Tier 2`
   - `AppSumo Tier 3`
2. AppSumo accounts show `Lifetime license`, not `Renews`.
3. AppSumo accounts show `Resets on <date>` for monthly cloud words.
4. Monthly subscribers can still access the billing portal.
5. AppSumo users do not see a generic "Manage subscription" portal button unless TalkMore returns a valid license management URL.

### Story 2: User sees one cloud words meter

As a cloud user, I want to see one monthly cloud words meter, so I know how much of my included AppSumo usage remains.

Acceptance criteria:

1. Account and Upgrade pages show:
   - `Cloud words this month`
   - `<used> / <limit> words`
   - reset date when available
2. The desktop app no longer shows `quotaHours` or `quotaTokens` for AppSumo plans.
3. Legacy Pro users may still receive server-side STT/LLM fields, but the UI should prefer `cloudWordsUsed` and `cloudWordsLimit` when present.
4. At 90 percent cloud word usage, show one warning toast.
5. At 100 percent, show one over-quota toast and do not show separate STT/LLM warnings.

### Story 3: User understands BYOK is separate

As a power user, I want BYOK to remain unlimited and private, so I can keep using OpenTypeless after managed cloud quota is used.

Acceptance criteria:

1. Account or Upgrade page includes a short line:
   - `BYOK usage is unlimited and does not count toward cloud words.`
2. Settings should keep local API key providers visible.
3. Over-quota errors include a direct action to open Settings and switch STT/LLM provider to BYOK.
4. BYOK copy must not imply AppSumo includes unlimited managed cloud usage.

### Story 4: Settings copy matches the AppSumo offer

As a user choosing providers, I want cloud mode copy to match the plan I bought, so I understand why sign-in matters.

Acceptance criteria:

1. STT cloud card and LLM cloud card copy use "Cloud words" and "included monthly usage".
2. Free user copy says cloud trial/credit, not Pro-only.
3. Paid AppSumo user copy says cloud is active.
4. Monthly Pro user copy still says cloud is active.
5. The UI does not mention Gemini, Groq, OpenRouter, tokens, or hours in the customer-facing plan card.

### Story 5: Client sends mode metadata for server routing

As the cloud API, I need metadata about the request mode, so I can route model fallback and quota correctly.

Acceptance criteria:

1. `CloudLlmProvider` sends a backward-compatible request body:
   ```json
   {
     "messages": [],
     "stream": true,
     "context": {
       "operationId": "018f9a9b-7c3e-7b1a-a2f3-2e1b8a6a8f10",
       "appType": "email",
       "hasSelectedText": true,
       "translateEnabled": false,
       "targetLang": "en",
       "rawTextChars": 123,
       "selectedTextChars": 456
     }
   }
   ```
2. TalkMore must continue to accept old clients without `context`.
3. The context values must not include the actual selected text or transcript outside the existing `messages` payload.
4. `operationId` is generated once per recording/voice pipeline and reused for the matching Cloud STT and Cloud LLM calls, so TalkMore can settle customer-facing cloud words once.
5. Rust `AppType` must be explicitly mapped to lower-case strings: `email`, `chat`, `code`, `document`, `general`.
6. Tests cover the serialized context for selected-text and non-selected-text flows.

### Story 6: Quota errors are actionable

As a user, I want cloud quota errors to tell me what happened and what to do next.

Acceptance criteria:

1. STT quota errors show:
   - `Cloud words used up. Switch to BYOK or wait until your monthly reset.`
2. LLM quota errors show the same cloud words framing.
3. The error UI should include a Settings action where the current notification/toast pattern allows it.
4. Auth errors remain separate from quota errors.
5. Empty or unknown 403 bodies remain auth errors, matching current `cloud.rs` behavior.

### Story 7: Cloud onboarding and connection tests support AppSumo

As an AppSumo buyer, I want the first-run setup and connection tests to recognize my lifetime license, so I do not get blocked after activation.

Acceptance criteria:

1. Cloud STT and Cloud LLM connection tests use a shared paid-cloud access rule instead of `plan === "pro"`.
2. A paid-cloud plan is active when either `source: "creem"` has a positive `cloudWordsLimit`, or `source: "appsumo"` has a positive `cloudWordsLimit` and `licenseStatus: "active"`.
3. Onboarding cloud provider steps do not require a local API key.
4. If the user is not signed in, onboarding routes them to account sign-in before cloud testing.
5. If cloud quota is exhausted, onboarding explains BYOK or monthly reset instead of showing a generic connection failure.
6. Tests cover Free, Pro, AppSumo Tier 1, refunded AppSumo, and unknown plan responses.

## 8. API Contract

OpenTypeless should accept the new TalkMore status shape:

```ts
type SubscriptionStatus = {
  plan: 'free' | 'pro' | 'appsumo_tier1' | 'appsumo_tier2' | 'appsumo_tier3'
  source: 'free' | 'creem' | 'appsumo'
  displayName: string
  subscriptionEnd: string | null
  subscriptionStatus: string | null
  licenseStatus?: 'active' | 'refunded' | 'deactivated' | 'pending'
  cloudWordsUsed: number
  cloudWordsLimit: number
  cloudWordsResetAt: string | null
  byokUnlimited: true

  // Backward compatibility during rollout:
  sttSecondsUsed?: number
  sttSecondsLimit?: number
  llmTokensUsed?: number
  llmTokensLimit?: number
}
```

Client compatibility rules:

1. If `cloudWordsLimit` exists, use cloud words UI.
2. If it does not exist, fall back to the current STT/LLM bars.
3. Unknown plan IDs should not automatically unlock cloud. The client may display `Paid plan detected` only when `source` is `creem` with `cloudWordsLimit > 0`, or when `source` is `appsumo` with `cloudWordsLimit > 0` and `licenseStatus === "active"`.
4. `plan === 'pro'` must not be the only paid check. Add a helper like `hasManagedCloudAccess(status)`.

## 9. UI Copy

Recommended English strings:

1. `account.cloudWords`: `Cloud words`
2. `account.cloudWordsThisMonth`: `Cloud words this month`
3. `account.cloudWordsUsage`: `{{used}} / {{limit}} words`
4. `account.cloudWordsReset`: `Resets {{date}}`
5. `account.lifetimeLicense`: `Lifetime license`
6. `account.byokUnlimited`: `BYOK usage is unlimited and does not count toward cloud words.`
7. `account.cloudQuotaWarning`: `You have used 90% of your monthly cloud words.`
8. `errors.cloud_quota_exceeded`: `Cloud words used up. Switch to BYOK or wait until your monthly reset.`
9. `settings.cloudLlmActive`: `Cloud AI is active for your plan.`
10. `settings.cloudSttActive`: `Cloud speech recognition is active for your plan.`

Avoid these in AppSumo-facing surfaces:

1. `5M tokens`
2. `10 hours`
3. `Gemini`
4. `Groq`
5. `OpenRouter`

## 10. Implementation Plan

### Phase 1: Types and entitlement helpers

Files:

1. `src/lib/api.ts`
2. `src/stores/authStore.ts`
3. `src/lib/constants.ts`

Work:

1. Expand `SubscriptionStatus`.
2. Add `PlanId`, `PlanSource`, and `isPaidCloudPlan()` helper.
3. Add cloud word fields to `AuthState`.
4. Keep legacy STT/LLM fields until TalkMore is fully migrated.
5. Replace `plan === 'pro'` checks in cloud-access UI with the helper.

Tests:

1. Unit test status parsing for legacy Pro.
2. Unit test status parsing for AppSumo Tier 1/2/3.
3. Unit test unknown paid plan fallback.

### Phase 2: Account and Upgrade UI

Files:

1. `src/components/AccountPage/index.tsx`
2. `src/components/UpgradePage/index.tsx`
3. `src/i18n/locales/*.json`

Work:

1. Replace AppSumo paid quota display with one cloud words bar.
2. Keep legacy fallback for old status payloads.
3. Display lifetime license and reset date for AppSumo users.
4. Keep monthly subscription portal for Creem Pro only.
5. Update pricing card copy if the current screen is used for AppSumo promotion. Otherwise, leave AppSumo purchase CTA on web and use desktop Upgrade as account status only.

Tests:

1. Render AppSumo Tier 1 status.
2. Render monthly Pro status.
3. Render free status.
4. Verify no `k tokens` or `hours` text appears for AppSumo status.

### Phase 3: Settings copy and quota errors

Files:

1. `src/components/Settings/LlmPane.tsx`
2. `src/components/Settings/SttPane.tsx`
3. `src/i18n/locales/*.json`
4. `src/i18n/__tests__/errors.test.ts`

Work:

1. Update cloud provider copy from Pro-only to paid cloud access.
2. Use AppSumo-compatible quota wording.
3. Add a generic cloud quota exhausted error translation if TalkMore moves to `cloud_quota_exceeded`.
4. Keep `stt_quota_exceeded` and `llm_quota_exceeded` aliases for backward compatibility.

Tests:

1. Existing error key coverage continues passing.
2. New cloud quota key exists in all locales.
3. Cloud pane shows active state for AppSumo plans.

### Phase 4: LLM request metadata

Files:

1. `src-tauri/src/llm/cloud.rs`
2. `src-tauri/src/llm/mod.rs`
3. `src-tauri/src/pipeline.rs`
4. `src-tauri/src/stt/cloud.rs`

Work:

1. Add a `context` object to the cloud LLM request body.
2. Generate an `operationId` once per recording and pass it to both Cloud STT and Cloud LLM.
3. Serialize app type, selected-text state, translation state, and length metrics.
4. Do not include secrets or duplicate raw user text in the metadata object.
5. Keep server compatibility by preserving `messages` and `stream`.

Tests:

1. Rust unit test for cloud LLM request body when selected text exists.
2. Rust unit test for request body without selected text.
3. Rust unit test that one pipeline reuses the same `operationId` for STT and LLM cloud calls.
4. Regression test that ordinary spoken commands remain content outside selected-text mode.

### Phase 5: Cloud onboarding and connection tests

Files:

1. `src-tauri/src/commands/stt.rs`
2. `src/components/Onboarding/LlmSetupStep.tsx`
3. `src/components/Onboarding/SttSetupStep.tsx`
4. `src/lib/tauri.ts`
5. onboarding and settings tests

Work:

1. Replace cloud `plan === "pro"` checks with the same paid-cloud rule used by the frontend store.
2. For cloud providers, bypass local API key requirements and test against signed-in entitlement status.
3. Show quota/auth-specific onboarding messages instead of one generic failure state.
4. Add tests for cloud onboarding with Free, Pro, AppSumo, refunded AppSumo, and quota-exhausted responses.

### Phase 6: Manual verification

Manual checks:

1. Free signed-in user sees free cloud credit.
2. AppSumo Tier 1 user sees `200,000 cloud words/month`.
3. AppSumo Tier 2 user sees `700,000 cloud words/month`.
4. AppSumo Tier 3 user sees `1,250,000 cloud words/month`.
5. BYOK user can dictate with no cloud usage change.
6. Cloud quota 403 shows quota error, not auth error.
7. Selected-text rewrite works and ordinary transcription does not execute spoken commands.
8. Cloud onboarding and connection tests pass for AppSumo plans without requiring API keys.

## 11. Success Metrics

Primary metric:

1. AppSumo activation rate: percentage of redeemed buyers who sign in to the desktop app and complete one successful cloud dictation within 24 hours.

Secondary metrics:

1. Cloud quota confusion: fewer support messages mentioning "tokens", "hours", or "what is my limit".
2. BYOK recovery: percentage of over-quota users who switch to BYOK instead of churning.
3. Cloud request success rate: STT and LLM success rates remain above 98 percent excluding user cancellations and quota/auth errors.
4. Median voice-to-polished-text latency remains competitive with the current cloud path.

Guardrail metrics:

1. AppSumo cloud COGS per active buyer.
2. 95th percentile monthly cloud usage per buyer.
3. Quota bypass or race-condition incidents.
4. Refund/chargeback rate.

## 12. Risks and Mitigations

| Risk                                       | Impact                                     | Mitigation                                                                                               |
| ------------------------------------------ | ------------------------------------------ | -------------------------------------------------------------------------------------------------------- |
| Users compare to unlimited Letterly        | OpenTypeless may look capped               | Position BYOK as unlimited and privacy-friendly; position managed cloud as fair monthly included usage.  |
| Lifetime heavy users max out quota forever | Negative unit economics                    | Enforce monthly reset, no rollover, request-level limits, abuse controls, and model routing in TalkMore. |
| Old clients cannot parse AppSumo plans     | Paid users see wrong state                 | Keep old fields and add tolerant client parsing before TalkMore switches.                                |
| Showing model names creates support burden | Users ask for provider-specific guarantees | Hide model names; state that managed cloud routes to suitable production models.                         |
| AppSumo refund not reflected in client     | Refunded users keep cloud access           | TalkMore spec owns license status; client must honor `licenseStatus !== 'active'`.                       |

## 13. Open Questions

1. Should the desktop app include a license redemption field, or should activation be web-only?
2. Should the Upgrade page remain monthly Pro-focused, or become a generic "Cloud plan" screen after AppSumo launch?
3. Do AppSumo buyers get cloud backup and scene packs, or only managed STT/LLM cloud usage?
4. Should Tier 2/3 get any visible non-quota differentiation, or keep all tiers as one feature set with larger usage?

## 14. Source Notes

Competitor and pricing references checked on 2026-06-20:

1. WriteVoice AppSumo listing: https://appsumo.com/products/writevoice/
2. AI Dictation AppSumo listing: https://appsumo.com/products/ai-dictation/
3. VoiceDash AppSumo listing: https://appsumo.com/products/voicedash/
4. Letterly AppSumo listing: https://appsumo.com/products/letterly/
5. Gemini API pricing: https://ai.google.dev/gemini-api/docs/pricing
6. Groq pricing: https://groq.com/pricing
