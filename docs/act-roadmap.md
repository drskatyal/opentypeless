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

## Agents task board — the "parallel agents checking tasks off" UX (Hey Clicky-inspired)

North-star reference: Farza (@FarzaTV)'s **Hey Clicky** product demos (heyclicky.com;
OSS foundation github.com/farzaa/clicky). Its agents surface, reverse-engineered
from the demo videos (X posts 2048203459976188261, 2055774393243230387,
2054397864009408889, 2051454940326097220):

- **Agents panel = responsive CARD GRID** (not Kanban columns / lanes / plain
  checklist). Each card: title, one-line streaming status/result, a thin progress
  bar while running, a state **badge** (Running / Done), a small preview
  thumbnail/icon, and an **"Open Agent"** button.
- **State = color at a glance**: blue = running/active, green/teal = done, (infer
  gray = pending, red = failed). Whole-card tint or left accent.
- **Live toasts/overlays**: non-modal floating cards mirroring active agents,
  stream the agent's *thoughts* in natural language (not a % or step list), thin
  progress bar, badge. On Done → summary replaces the stream + 2–3 **"suggested
  next"** action chips (open the result, continue by voice). Hover = "watch it
  think" (expands the reasoning stream). Multiple toasts coexist = parallelism.
- **Per-agent state machine**: Pending → Running (stream updates) → Done | Failed.
  Completion is a calm badge flip + summary + CTAs (no confetti, no strike-through);
  result artifacts auto-surface (file opens, site opens, ticket created + link).
- **Parallelism conveyed by**: N cards in the grid + N floating toasts, each with
  its own independent stream and a distinct accent color; "Open Agent" per card so
  one deep-dive never blocks the others. No single shared terminal log.
- **Aesthetic**: dark glass panels, rounded ~12–16px, SF-Pro-like type, vibrant
  per-agent accent colors, smooth 200–400ms transitions, non-focus-stealing.

Mapping to FlowRad Mic (build plan):
1. Backend: the Conductor emits a per-task lifecycle over ACT_EVENT with STABLE
   task ids — TaskSpawned{id,label} → TaskProgress{id,text} → TaskDone{id,ok,
   summary} / TaskFailed{id,error}. Today missions run sequentially; the board
   works in that model (queue = live checklist that checks off) and evolves to
   real parallel missions where independent.
2. Frontend: an **Agents board** (card grid) in the HUD/a panel + a live toast
   layer, following the states/colors above, with a check-off animation on Done.
3. Later: run independent missions concurrently (true parallel agents).

## Remaining before "done"

- [ ] **Live Windows E2E on real UIA** — the last mile; can't be simulated.
      Start with deterministic seeds (launch app, ms-settings:/shell: URIs, key
      combos, web URLs) — they bypass UIA resolution and are highest-confidence.
- [ ] Confidence-gated selection (above).
- [ ] Learn-by-recording V1 (above).
- [ ] First-run discoverability nudge for the dual-hotkey (Act vs Dictation).
- [ ] Optional: TTS for talk-back answers (behind a setting).
