# Act — Screen-Aware Modes (tree / hybrid / vision)

Three interchangeable *perception* modes behind one toggle, sharing the **same**
Conductor, sub-agent orchestration, safety layer, and closed loop. Only how a
turn is **grounded and executed** changes; planning, capability-gating, the kill
switch, and the multi-agent lanes are reused unchanged.

## The three modes

| Mode | Perception | Model | Action target | Strength | Weakness |
|---|---|---|---|---|---|
| `tree` (today) | a11y UIA snapshot → text `GroundingPacket` | text (Cerebras/Gemini) | element **path** | fast, precise on good trees | empty/bad trees (games, canvas, Electron) |
| `hybrid` | a11y snapshot **+ screenshot with Set-of-Marks** (numbered boxes from element `Bounds`) | vision (Gemini) | **mark → element path** (reuse `Bounds::center`) | most accurate; grounded picks | needs both a screenshot and a usable tree |
| `vision` | screenshot only | vision (Gemini) | **coordinate** click/type | works with NO tree at all | weakest grounding (raw coords) |

Ranked accuracy: **hybrid ≥ tree > vision**. Vision's job is the long tail where
the tree is empty. **Cerebras is text-only** → hybrid/vision planning runs on
Gemini (multimodal); `tree` keeps Cerebras as the fast default.

## What's reused vs new

Reused: `Conductor`, `selection` (routing), the foreground/background sub-agent
lanes, `Blackboard`/whiteboard, `CapabilityGate`, kill switch, closed loop
(`run_novel`), destructive classifier. The whiteboard already carries
`already_open`; element `Bounds` + `center()` already exist (a click-from-bounds
fallback is present today).

New pieces (phased):

**Phase 1 — platform-agnostic scaffolding (compiles + tests on Linux/mock)**
- `PlanMode` enum (`Tree | Hybrid | Vision`) + `act_plan_mode` config field + a
  Settings segmented control.
- `LlmClient::generate_json_multimodal(system, user, image: Option<&[u8]>, schema)`
  — Gemini `generateContent` with an `inlineData` image part; text path unchanged.
- `Action::Click { x, y }` primitive + executor path (uses the existing pointer /
  bounds-center click infra) gated by a new `Capability::PointerClick`.
- A `capture_screen()` backend method on `AccessibilityBackend` (returns
  `Option<Vec<u8>>` PNG); mock + non-Windows return `None`; Windows stubbed.
- Scoreboard: one structured `act scoreboard mode=… goal=… ok=… ms=…` log line
  per command (goal-level success + latency), so the three modes are comparable.

**Phase 2 — Windows capture + grounding (needs the Windows build to tune)**
- Windows `capture_screen` (foreground-window grab; `xcap`/DXGI).
- Set-of-Marks overlay: draw numbered boxes from `Bounds` onto the screenshot for
  `hybrid`; the model returns a mark number → resolve to the element path.
- `vision` coordinate execution + a coordinate-space sanity clamp to the window.
- Auto-fallback routing: try `tree`; if the snapshot is empty / times out, fall
  back to `vision` for that turn (probably the real product default).

**Phase 3 — scoreboard harness**
- A fixed suite (the 82 actions + ~20 multi-step goals) run through all three
  modes, emitting a success/latency table so "which mode wins per app" is a number.

## Multi-agent invariant (unchanged in all three modes)

One orchestrator (Conductor) owns focus; it decomposes the utterance into missions
and runs a task-agent per mission. Foreground-lane agents serialize through the
focus lock (only one drives the keyboard/mouse at a time); background-lane agents
(LLM/URL/read-only) run in parallel. A mode only changes how each task-agent
*sees and acts* — not how many agents run or how they're scheduled. See
`act-multiagent-design.md`.

## Safety notes specific to vision

- Coordinate clicks are still gated + destructive-classified; a click at a
  coordinate the model invented is bounded by the focus guard (must be the
  expected app) and the kill switch.
- The screenshot is treated as UNTRUSTED data (same as SCREEN_CONTEXT): on-screen
  text in the image can be an injection attempt and never becomes a command.
