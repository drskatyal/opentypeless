# Incorporating the Gemini Computer Use tool

Status: **research + plan, no code.** How the official Gemini Computer Use (CU)
tool fits OpenTypeless, and the recommended way to adopt it. TL;DR: **add it as a
fallback actuator behind the existing vision seam — do not replace the UIA
engine.**

## What the CU tool is (July 2026)

The model sees a screenshot + goal and returns a **`function_call`** naming one UI
action; your client executes it and sends back a **`function_result`** (a fresh
screenshot) for the next step. Client-side loop, same shape as our ReAct loop.

- **Model:** `gemini-3.5-flash` is the recommended CU model (legacy
  `gemini-2.5-computer-use-preview-*` is retired). `gemini-3.6-flash` is the newer
  flash but CU is documented against 3.5-flash — pin the CU calls to the
  documented CU model, independent of our planner tier.
- **Action set:** `click_at`, `type_text_at`, `scroll_document`, `scroll_at`,
  `hover_at`, `drag_and_drop`, `key_combination`, `wait_5_seconds`, `search`,
  `navigate`, `go_back`, `go_forward`. Each action carries an **`intent`** field
  (the model's reasoning) — useful for our progress toasts.
- **Coordinates:** **normalized 0–1000 grid** — `pixel_x = x/1000 * viewport_w`.
  This is *exactly what we already emit and denormalize* (task #29,
  `planner.rs::denormalize_clicks`), so the coordinate translation is already
  built.
- **Safety:** actions may carry a **`safety_decision`** of `allowed` /
  `require_confirmation` / `blocked`, plus configurable policy categories.
- **Environments:** browser, mobile, **desktop** — but desktop is Gemini CU's
  *weakest* surface (it's browser-anchored, descended from Project Mariner).

## Why it's a clean fit for OpenTypeless (not a rewrite)

Four seams already exist:

| CU concept | Existing seam |
|---|---|
| screenshot→action→screenshot loop | `Conductor::novel_loop` (already this shape) |
| 0–1000 coords → pixels | `planner.rs::denormalize_clicks` (identical grid) |
| pluggable model transport | `LlmClient` trait + `vision_llm` on `Planner` |
| vision perception mode | `Perception::Vision` / `is_multimodal()` gate |
| `safety_decision: require_confirmation` | `CapabilityGate` + the confirm/resume pause flow |
| action → OS input | executor `click_point` / `type_text` / `scroll` / `key_combo` |

So CU is **behind our existing `PlanSource` / vision seam**, not a parallel stack.

## The recommended shape: CU as the tree-last fallback actuator

Our escalation ladder stays **CLI/API → UIA → vision**. CU slots in as a *better
vision tier* for the targets UIA can't see:

```
1. shell / API recipe            (deterministic, invisible)
2. UIA pattern (invoke/set_value) (background, exact — PRIMARY, keep)
3. our own 0–1000 vision planner  (current fallback)
   └─►  Gemini CU tool            (fallback-of-the-fallback: canvas apps, games,
                                    owner-drawn PACS viewers, weird Electron —
                                    anything with an empty/uninformative UIA tree)
```

Why fallback and not primary: on Windows *desktop* our UIA-first approach is
faster and more reliable than any pixel-loop (CU included), and CU's desktop
environment is its weakest. But when the UIA tree is empty (a canvas the model
must reason about visually), CU brings **maintained prompt-injection screening,
intent-annotated actions, and Google-tuned grounding** that beat our hand-rolled
vision prompt.

## Build plan (phased, behind a flag)

**Phase 1 — transport + action mapping.**
- Add a `GeminiComputerUseClient` implementing `LlmClient` (or a sibling trait
  `ComputerUseClient`) that sends the CU tool declaration + screenshot and parses
  the returned `function_call` + `safety_decision` + `intent`.
- Map the CU action set onto our `Action` enum:
  - `click_at` → `Action::Click{x,y}` (already 0–1000 → denormalize as today)
  - `type_text_at` → focus-at-coord then `Action::Type`
  - `scroll_document`/`scroll_at` → `Action::Scroll`
  - `key_combination` → `Action::Key`
  - `navigate`/`go_back`/`go_forward` → our `uri`/browser ops
  - `drag_and_drop`/`hover_at` → **new** executor primitives (only these are net-new)
- `wait_5_seconds` → our adaptive `settle_after_nav` (don't burn a flat 5s).

**Phase 2 — routing.** A new `PlanMode`/`Perception` variant `ComputerUse`, chosen
by the planner only when the UIA snapshot is empty/uninformative for the current
window (the exact non-UIA condition). Everything else stays on tree/hybrid.

**Phase 3 — safety bridge.** Map `safety_decision: require_confirmation` onto the
existing confirm pause (`StepOutcome::NeedsConfirm` → `AwaitingConfirm`), so CU's
safety verdicts flow through the *same* on-screen prompt window we just built. Our
`CapabilityGate` still has the final say — CU's `allowed` never bypasses a gated
capability.

**Phase 4 — verify on Windows.** CU desktop is weak; measure it against our own
vision planner on a canvas-app task set before defaulting anything to it.

## Caveats to design around

- **Latency:** CU is a full screenshot-per-step loop — same cost as our vision
  mode. Pair it with the deferred screenshot downscale/JPEG work
  (`act-background-and-screenshot-design.md`) or each CU step pays the full-res
  upload tax. And remember the **denormalize-vs-image-size trap** documented there
  — CU coords are 0–1000 so they're safe, but a downscaled screenshot still needs
  the real screen size for any of our *own* pixel math.
- **Desktop weakness:** keep UIA primary; use CU only where the tree is empty.
- **Cost/keys:** CU billing is image-prefill heavy; gate it behind the same
  provider/key config as the planner, and only invoke on the non-UIA fallback path.
- **Model pinning:** pin CU calls to the documented CU model (`gemini-3.5-flash`)
  even though our planner default is now `gemini-3.6-flash` — they're chosen
  independently.

## One-line recommendation
Adopt CU as the **non-UIA fallback actuator** behind the vision seam: reuse our
0–1000 denormalization, route CU `safety_decision` through the existing confirm
window, keep UIA primary. Net-new code is small (one client, an action map, two
new executor primitives) precisely because the grid, the loop, and the safety
pause already exist.
