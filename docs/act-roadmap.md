# Act / Conductor — roadmap & design notes

Durable memory for in-flight ideas and deferred work on the voice-control
assistant (Act mode + the Conductor/drawer). Not a spec — a captured backlog so
decisions survive across sessions.

## Confidence-gated selection (accuracy improvement — agreed direction)

Today the selection LLM picks *which* flow and fills slots, and the Conductor
just runs it — no confidence gate at the routing layer, so a 55/45 call or a
half-heard slot is a silent guess. `pick_result` already gates *row* selection
(`min_score` + `tie_margin` → `Fail|Ask|TakeBest`), but nothing gates *flow +
slot* selection, which is the higher-leverage layer (a wrong flow = wrong task).

**Direction (prefer structural signals over raw LLM self-confidence, which is
poorly calibrated):**

1. Structural first: is each *required* slot actually present in the transcript?
   Did the model return one clear flow or several plausible ones?
2. Selection returns a coarse per-mission `confidence` band (high/med/low) +
   required-slot presence. Conductor gates:
   - high → run
   - medium → run but announce / quick-confirm
   - low, or missing required slot → **clarification path**: ask, offering the
     top 2–3 candidate flows as a numbered pick (reuse the existing
     NeedsChoice / AskUser HUD).
3. Keep `pick_result`'s deterministic score as-is (the good kind of confidence).
4. Be conservative about per-*action* LLM confidence in the planner — highest
   miscalibration risk, lowest ROI; the executor already gates risky actions.

This is the schema-v2 selection-contract item deferred earlier
("required-slot presence → clarifications path, flow+slot confidence").
Suggested: run the calibration/threshold design past the council (Grok+GPT)
before implementing.

## Learn-by-recording (planned, council-reviewed — build later)

"Record this flow" → perform once → LLM names + parameterizes → save to drawer.
Council plan captured; V1 non-negotiables:
- Two artifacts: a private, encrypted, short-TTL `RecordingTrace` (holds
  literals, never enters the drawer) vs. the public `FlowFile` (approved slot
  examples only). Never record password-field values; redact secret-shaped
  strings.
- Hybrid capture: input hooks (when/order/typed chars) + UIA focus/invoke
  events (what element) + snapshot-diff on idle (side-effects). Never
  coordinates-only. Emit top-3 ranked semantic selectors per action.
- Generalize with one LLM call bracketed by deterministic pre-compress +
  post-validate; under-slot bias; leaf-vs-branch decision; forbid shell.
- **Mandatory human review** before save; learned flows are Smoke and gated on
  replay; Conductor won't auto-route to a not-yet-enabled learned flow.
- V1 single-app preferred; multi-app/virtualized-list/selector-repair later.

## Remaining before "done"

- [ ] **Live Windows E2E on real UIA** — the last mile; can't be simulated.
      Start with deterministic seeds (launch app, ms-settings:/shell: URIs, key
      combos, web URLs) — they bypass UIA resolution and are highest-confidence.
- [ ] Confidence-gated selection (above).
- [ ] Learn-by-recording V1 (above).
- [ ] First-run discoverability nudge for the dual-hotkey (Act vs Dictation).
- [ ] Optional: TTS for talk-back answers (behind a setting).
