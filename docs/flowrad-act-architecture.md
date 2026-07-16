# FlowRad Mic — "Act Mode" Architecture Plan

> Status: design / proposal. This document is the detailed architectural plan for
> **Act mode** — the second half of FlowRad Mic alongside **Transcribe mode**.
> It was produced with a **council of agents** (an adversarial design pass, a
> constructive design pass, and a dedicated open-source landscape audit) and then
> reconciled against the existing codebase. It supersedes nothing in
> `flowrad-cowork-spec.md`; it makes that spec concrete and adds the latency
> budget, the accessibility access model, and the two voice-control state machines.

## 0. What Act mode is (and is not)

**Transcribe mode** turns speech into text in whatever field is focused. **Act
mode** turns speech into *actions on the computer*: "open Chrome and go to
railway.app", "set the date field to tomorrow", "reply to this saying I'll be
late". It is for clinicians **and** everyone.

The governing thesis, agreed by the whole council:

> Act mode is a **voice-operated, capability-sandboxed accessibility RPA engine**,
> with a cloud LLM (Gemini) used only for **intent → structured plan** when a
> local fast-path misses — **not** a screenshot-vision "computer use" agent with a
> microphone bolted on.

Non-goals (explicit):

- **No screenshots by default.** Vision is a narrow, opt-in, PHI-redacted
  fallback for accessibility-blind surfaces — never the happy path. Screenshotting
  a clinician's screen ships patient names, other windows, and notifications to a
  model; that is incompatible with our privacy promise.
- **We automate the OS, never a specific IDE.** No Cursor/VS Code puppeteering.
  "Cursor" in our design always means the **mouse cursor / caret**, i.e. where the
  user is pointing or typing.
- **The model is never the safety boundary.** Enforcement lives in Rust, at the
  OS-capability level. Command-approval UX is secondary.

## 1. The council

| Stream | Role | Headline conclusion |
|--------|------|---------------------|
| Landscape audit | Survey OSS building blocks, a11y-tree vs vision | The Rust/accessibility path already has proof points — reuse them, don't rebuild |
| Adversarial design | Stress-test the a11y-first approach, latency, safety | "Only Gemini is the bottleneck" is achievable **only after** you industrialize an a11y snapshot cache + local fast-paths |
| Codebase reconciliation | Map to existing pipeline/VAD/hotkeys | Act reuses the existing VAD, hotkey modes, and STT plumbing wholesale |

## 2. Open-source landscape audit

The dividing line is **screenshot-vision** (send pixels to a VLM, act on
coordinates — high latency, ships every pixel) vs **accessibility-tree** (query
the OS a11y API, act on element handles — millisecond queries, selectable/
redactable, DPI-independent). We are firmly accessibility-tree-first.

### Computer-use / OS agents

| Repo | Approach | Verdict |
|------|----------|---------|
| **mediar-ai/terminator** (Rust, Windows UIA, active) | **A11y-tree** | **Closest to our target.** "Playwright for Windows computer use," screenshot-free, ms-level UIA queries. Copy its element-locator model + tool schema. Windows-first. |
| **mediar-ai/MacosUseSDK** + **mcp-server-macos-use** (Swift) | **A11y-tree (AX)** | The macOS half of the same pattern. FFI or port. |
| **mediar-ai/fazm** (Swift) | **A11y-tree + voice** | Voice-controlled macOS AX agent that "actually clicks buttons in Slack/Linear/Notion." **Proof the voice+a11y product is viable.** |
| bytedance/UI-TARS(-desktop) (~38k) | Screenshot/VLM | Most polished OSS vision stack — **wrong** latency/privacy/failure model for us. Reference for grounding research only. |
| simular-ai/Agent-S / Agent-S2 | Hybrid (a11y + screenshot) | Good a11y-tree **serialization** patterns; research planner is too slow. |
| trycua/cua (~20k) | Hybrid, VM-centric | Built around VMs/sandboxes, not local-host control. |
| OpenAdaptAI/OpenAdapt | Hybrid record/replay | Uses pywinauto + macOS AX; ideas for hooks, not a live agent. |
| Skyvern (~22k, AGPL) | Browser DOM + vision | Browser-only and **AGPL (viral)** — avoid for an OS tool. |
| anthropics computer-use-demo, openai cua-sample | Screenshot | Canonical vision harnesses; confirm the pixel path's cost profile. Reference, not core. |

### Accessibility libraries (the preferred no-screenshot path)

| Repo | Platform | Use |
|------|----------|-----|
| **leexgone/uiautomation-rs** | Windows UIA (Rust) | **Native-Rust Windows path.** Build the Windows executor on this. |
| **tmandry/AXSwift**, **MacPaw/macapptree**, Hammerspoon `hs.axuielement` | macOS AX | Cleanest AX clients + an LLM-oriented tree-serialization schema to mirror. |
| pywinauto / FlaUI / yinkaisheng UIA | Windows | API-design + control-pattern references (Python/.NET). |
| yury/cidre, objc2-application-services | macOS AX from Rust | Pure-Rust AX (early) — alternative to a Swift FFI shim. |
| AccessKit | — | **Wrong direction**: exposes *our own* app's a11y to screen readers; does not read other apps. |

### Input synthesis

| Repo | Use |
|------|-----|
| **enigo-rs/enigo** (Rust, cross-platform) | **Default synthesis layer** (Win SendInput / macOS CGEvent / Linux X11) when an AX `Invoke`/`SetValue` pattern isn't available. |
| Narsil/rdev | Global input listen/simulate for push-to-talk hotkey capture. |
| nut.js, RobotJS | Node-bound; nut.js has a restrictive-license caveat — avoid depending. |

### Voice-control precedents

- **farzaa/clicky (HeyClicky)** — press-and-hold → ScreenCaptureKit grab → Claude
  vision → TTS. **Screenshot-based; our direct competitor.** Two modes (fast
  dictation vs slow screen-aware) — validates the two-modality UX, but its pixel
  path is exactly the latency/PHI tax we avoid.
- **Talon Voice** — grammar-based command routing that resolves fixed commands
  **without an LLM round-trip**; push-to-talk / continuous / noise modes. **Copy
  the local-grammar fast path.**
- **Aqua Voice** — reads on-screen text as STT context for accuracy. Precedent for
  using the a11y tree as *STT context*, not just for actions.

### What to reuse vs never copy

**Reuse:** Terminator + uiautomation-rs (Windows), MacosUseSDK/AXSwift +
macapptree schema (macOS), enigo (synthesis), Talon-style local grammar, Voice
Control / Voice Access **numbered-overlay** disambiguation UX, RPA verify/repair
discipline.

**Never copy:** continuous screenshot→VLM→click loops as the primary controller;
"give the model the whole desktop every step"; unrestricted shell as the
integration backbone; pixel-only coordinates without element identity; trusting
the model as the safety boundary; app-specific IDE deep integrations as the
product center.

**Whitespace we fill:** no single repo unifies Windows + macOS accessibility under
one Rust/Tauri roof with a voice loop and a Talon-style local fast path. That is
Act mode.

## 3. System architecture

Act mode slots into the existing Tauri v2 layout (Rust host + React UI) and reuses
the current mic capture, VAD segmenter, hotkey modes, and STT provider.

```
React UI                         Rust host
--------                         ---------
Act session HUD                  hotkeys / VAD               (existing)
numbered overlays (disambig)     mic capture + segmenter     (existing)
confirm / undo toast             stt provider (Gemini)       (existing)
audit log viewer                 a11y_snapshot_service       (new)
capability manager               platform_uia (Windows)      (new)
                                 platform_ax  (macOS)        (new)
                                 input_synthesizer (enigo)   (new)
                                 grounding_resolver          (new)
                                 fastpath_router             (new)
                                 planner (Gemini, structured)(new)
                                 capability_gate             (new)
                                 action_executor (+ verify)  (new)
                                 kill_switch + audit_log      (new)
```

**Threading model**
- **Snapshot worker** — dedicated thread; maintains a cached, versioned snapshot
  of the focused window's accessibility subtree. Never blocks the UI.
- **Executor** — a serial queue; one action plan at a time. The kill switch clears
  the queue and releases held modifiers.
- **Planner / STT** — async, with the cancellation tokens already used by the VAD
  worker plumbed through.

## 4. Accessibility access model (per OS)

The primitive is: **read** the focused element / caret / element subtree, and
**act** by invoking accessibility patterns first, synthesizing input only as a
fallback.

### Windows — UI Automation (UIA)
- Read: `GetFocusedElement`, scoped `FindFirst`/`FindAll` on the **focused window
  only** (never the full desktop). Cache `Name, ControlType, BoundingRectangle,
  AutomationId, IsEnabled`, available patterns, and the `TextPattern` caret.
- Act: prefer control patterns — `Invoke`, `Value.SetValue`, `SelectionItem`,
  `Toggle`, `ExpandCollapse`, `Scroll` — over synthetic clicks. Fall back to
  enigo click at the element's DPI-corrected bounds center.
- Cost/hazards: your process is almost always **out-of-proc** → COM marshalling.
  Treat any uncached full-window walk as ~100–400ms p50, multi-second p95. Put a
  hard per-call timeout (e.g. 200–500ms) and a per-plan budget on every UIA call;
  a hung UI thread must degrade to a spoken error, not a freeze.
- Integrity: a medium-integrity process can't drive elevated apps. MVP detects and
  says so ("that app is running as admin — I can't control it").

### macOS — Accessibility (AXUIElement)
- Read: `kAXFocusedUIElement`, batch `AXUIElementCopyMultipleAttributeValues`
  (avoid attribute storms), `kAXSelectedTextRange` for caret. Invalidate on
  `kAXFocusedUIElementChangedNotification`.
- Act: prefer `AXPress`, `AXSetValue`, `AXConfirm`, `AXSetFocus`; CGEvent click
  only as fallback.
- Permissions (TCC): **Accessibility** (required) + **Input Monitoring** (for
  synthesis) + optionally **Screen Recording** (only if vision fallback is ever
  enabled). First call can block on the TCC prompt; handle grant + revocation in
  onboarding.
- Rust reality: macOS AX in Rust is the biggest engineering gap. Realistic path is
  a thin **Swift FFI shim** over AXSwift/MacosUseSDK, or the early `cidre` /
  `objc2-application-services` bindings. Normalize its output to the same element
  schema as Windows.

### Linux — AT-SPI (nice-to-have)
- AT-SPI2 via the `atspi` crate. Least-covered target; ship after Win + macOS.

### Normalized element schema
Both OSes emit one schema so the planner sees a single shape:
`{ path_id, role, name, description, value_len, states[], bounds, patterns[] }`.
Caret/selection is normalized across UIA `TextPattern` and AX
`kAXSelectedTextRange`.

## 5. Grounding — speech → element, without vision

Ordered resolution stack (cheap/deterministic first):

1. **Deictic / focus-relative** ("this", "here", "that field") → caret position,
   else focused element, else pointer hit-test element. **This is the first-class
   caret/pointer primitive** from the prior spec, always included in context.
2. **Role + name match** ("Submit button", "search box") — fuzzy match on
   name/description/help, filtered by role.
3. **Ordinal / structural** ("second field", "last tab") over a stable
   **reading/focus-order** candidate list (visible interactive only).
4. **State filters** ("the selected row", "the enabled Save").
5. **App landmarks** — a small data map (browser address bar, back) via reliable
   automation ids. Data, not IDE puppeteering.
6. **Numbered-overlay disambiguation** — when top candidates tie, draw `1..k`
   labels on those elements via a transparent always-on-top window; user says
   "two" or presses 2. Faster and more accurate than another model round-trip.
   (Stolen shamelessly from macOS Voice Control / Windows Voice Access.)
7. **Confirmation** for low-confidence or destructive targets.

**What the planner receives** is a compact *grounding packet* — focus, pointer,
selection, and a token-capped list of visible interactive candidates with
`path_id/role/name/state` — **not** a raw tree dump. More tree ≠ better grounding.

**Action schema (MVP)** — structured output only, resolved against the *current*
snapshot by the local executor:

```json
{
  "actions": [
    { "op": "focus",  "target": "#/1/4/2" },
    { "op": "type",   "text": "hello", "clear": false },
    { "op": "invoke", "target": "#/1/4/9" },
    { "op": "key",    "combo": "meta+Enter" },
    { "op": "ask_user", "question": "Which Delete?", "choices": ["1","2","3"] }
  ],
  "confidence": 0.86
}
```

If a `target` path is stale, the executor does **local repair** (re-snapshot the
focused subtree, re-match) before failing soft — it does not go back to Gemini for
mechanical failures, only semantic ones.

## 6. The two voice-control modalities (both required)

Act is driven by the **same mode-aware hotkeys already shipped** for Transcribe:

### 6a. Hold-to-talk (Batch) — the accurate one
Manual endpointing: the user holds the key, speaks the whole command, releases.
Release → transcribe the whole clip → plan → act. Because the user controls the
start and end precisely, there is no VAD mis-segmentation, so this is the **most
accurate** and the recommended default for high-stakes actions.

```
IDLE ──hold key──▶ LISTENING ──release──▶ TRANSCRIBE ──▶ PLAN
  ▲                                                        │
  └────────────── verify / confirm / undo ◀── EXECUTE ◀────┘
```

### 6b. Hands-free (VAD session) — the ergonomic one
Press once to **arm** a session; the energy-VAD segmenter (already built) cuts
each utterance into a command; press again to **stop**. No hold. Each segment runs
the plan→act loop; the session stays armed between commands.

```
IDLE ──press──▶ ARMED ──VAD segment──▶ [TRANSCRIBE ▶ PLAN ▶ EXECUTE ▶ verify] ──▶ ARMED
  ▲                                                                                 │
  └────────────────────────────── press (stop) / kill switch ◀──────────────────────┘
```

This mirrors the hotkey rule already enforced in code
(`AppConfig::effective_hotkey_mode`): **Batch allows hold or toggle; VAD is
toggle-only** because hold-to-talk is meaningless when the VAD is doing the
endpointing. Batch is the safer default for Act; VAD wins on ergonomics.

**Confirmation / undo** works in both: destructive actions require a spoken
"confirm" or a button (timeout cancels); every executed plan is reversible where
the OS allows (`undo`, close, restore focus) and always logged.

## 7. Latency budget — engineering "only Gemini is the bottleneck"

Honest per-stage budget (fast Gemini tier):

| Stage | Batch (hold) | VAD (hands-free) | Notes |
|-------|--------------|------------------|-------|
| Endpoint (key-up / VAD trailing silence) | ~0–30ms | 100–400ms | VAD silence is **user-perceived** latency — tune it |
| Audio → STT (Gemini) | 200–800ms | similar per segment | streamed; overlaps with context build |
| Build grounding packet (L0+L1) | **5–40ms cached** / 80–400ms cold | same | **the make-or-break stage** |
| **Gemini plan (fast tier)** | **400–1500ms** | same | the true bottleneck once local work is amortized |
| Parse → executor | 1–10ms | same | deterministic |
| Input synthesis (key/click) | 5–30ms + 50–200ms focus settle | same | OS input queue |
| Post-action verify | 50–300ms | same | needed for robustness |

**Verdict (from the adversarial pass):** "only Gemini is the bottleneck" is
**achievable on the modal happy path, and only if** you (1) prewarm and scope the
a11y snapshot, (2) stream STT overlapping with context build, and (3) let common
commands skip Gemini. It is **not** achievable on first-touch of a heavy app
(cold snapshot), virtualized-list hunts, multi-step plans (latency = sum of
steps), elevated targets, or any vision fallback. The doc must not pretend
otherwise, and the UX should say "working… step 2/4" for multi-step plans.

**Techniques to keep only Gemini on the critical path:**
1. **Snapshot service, not live walks.** Cache a versioned per-app snapshot;
   invalidate on focus/structure-changed/resize events. Hot path reads the cache.
2. **Snapshot tiers.** L0 = focused element + parent chain + selection (always,
   ~10–40ms). L1 = interactive controls in the focused window, depth-capped
   (cached). L2 = search expansion / virtualized scroll (on demand).
3. **Prewarm** on session arm / app-focus change (debounced) so utterance-end
   finds context already warm.
4. **Overlap** STT with the snapshot refresh; assemble the prompt when both ready.
5. **Local deterministic fast-paths (bypass Gemini entirely)** — a tiny local
   intent classifier for `copy/paste/cut/undo/redo/select all/save/new tab/close
   tab/next field/submit/stop` and app launch via the OS search index. Target
   <300ms after STT. This is the Talon lesson.
6. **Speculative execution** only for safe, non-destructive prep (e.g. "focus
   address bar") on high-confidence partial transcripts; cancel on revision.
7. **Compress the tree for the LLM** — send role/name/state/short-path, cap tokens
   hard (~2–6k chars).

**Target metrics:** p50 utterance-end → first action start (excl. Gemini) < 80ms
cached; grounding accuracy on a uniquely-named control > 90% on allowlisted apps;
kill switch aborts < 100ms.

## 8. Safety & trust model

### Capability sandbox (primary, enforced in Rust)
Capabilities are process-enforced; the LLM cannot "talk its way" past them.

| Capability | Examples | Default |
|------------|----------|---------|
| `input.keyboard` / `input.mouse` | type, chords, click | session grant |
| `a11y.read` / `a11y.invoke` | tree, values / buttons, menus | session grant |
| `clipboard.read/write` | paste injection | explicit |
| `fs.user_docs` | open/save dialogs | limited |
| `fs.destructive` | delete, trash | deny + confirm |
| `net.navigate` | open URLs | confirm external |
| `app.launch` | start processes | allowlist or confirm |
| `system.power` | shutdown/sleep | deny |
| `vision.capture` | screenshot | opt-in |
| `agent.self` | quit Act, mute | always |

Optional **app scope**: only the frontmost app, or user-pinned apps; system
settings blocked by default. Command-approval prompts are UX sugar, **not** the
boundary (a confused agent can be phished into clicking "Allow").

### Prompt injection — spoken audio and on-screen text are DATA
Attack surface: on-screen "ignore previous instructions…", crafted window
titles/filenames, notification-toast injection, background-audio commands ("delete
all files" from a TV), homoglyph/RTL control names, confused-deputy clicks on real
OS security dialogs, clipboard "paste and run" bait.

Defenses:
- Immutable system policy; UI tree and transcript wrapped as untrusted
  (`<untrusted_ui>`, `<untrusted_speech>`).
- **Structured outputs only**; no free-form shell in Act MVP (shell, if ever, is a
  separate explicit capability + mode).
- **OS-security-surface denylist** — never drive UAC / password prompts / Secure
  Desktop / browser permission popovers / auth dialogs; detect by process/window
  class and refuse.
- **Local destructive classifier** — delete/send/pay/overwrite/share always force
  confirm regardless of model confidence.

### Kill switch, audit, hygiene
- **Kill switch** — a global hotkey the agent cannot steer; stops the VAD session,
  cancels in-flight actions, releases Shift/Ctrl/Alt/Meta, suppresses synthesis;
  aborts < 100ms. Optional local panic-phrase spotter (never cloud-only).
- **Audit log** — local, append-only: timestamp, transcript, tree hash, actions,
  capability checks, result. No PHI upload by default; user-exportable.
- **Rate limits** — max actions/sec and clicks/min to bound runaway loops.
- **Self-control ban** — the agent cannot disable the kill switch, audit, or grant
  itself capabilities.

## 9. Failure matrix & app capability registry

Accessibility trees lie or vanish for Electron/CEF (sparse/role-soup),
custom-drawn/canvas/games (no tree), virtualized lists (only visible rows), some
web content (shadow DOM, cross-origin iframes), and under stale-tree/focus-race
conditions. We maintain a per-app **capability matrix** (`full | partial |
vision-only | unsupported`) and surface it honestly ("limited control in
Discord"). Virtualized lists get an `ensure_visible(matcher)` scroll-and-re-snapshot
protocol with a bounded step budget.

## 10. Vision — narrow, opt-in, PHI-safe fallback (stub in MVP)

Justified **only** when the tree has no matching named target, the user refers to a
pure visual attribute ("the blue button", "the chart peak"), or a canvas/game has
no AX peers. Rules: default off; crop to the active window (never full desktop);
session or per-capture consent with a PHI banner; no disk persistence; **grounding
only** (one capture per failed grounding), never a continuous VLM control loop; map
the vision hit back to an AX node and act via `Invoke` where possible.

## 11. Phased delivery roadmap

- **Phase 0 (foundation)** — executor + capability gate + kill switch + audit;
  manual JSON actions; a11y snapshot L0/L1 on **one** OS first (pick Windows for
  the mature Rust UIA story, then port). No LLM yet.
- **Phase 1 (voice → act)** — wire STT + local fast-paths + Gemini structured
  plans; verbs `focus/type/invoke/key/clear/scroll/select_menu/ask_user/stop`;
  polish Batch (hold-to-talk).
- **Phase 2 (accuracy)** — numbered-overlay disambiguation; ordinals; verify/repair
  loop; harden the VAD session; ship the app capability matrix.
- **Phase 3 (parity)** — second OS (macOS AX via Swift FFI); virtualized
  scroll-search; clipboard + navigation capabilities.
- **Phase 4 (escapes)** — opt-in vision grounding fallback; optional signed
  elevated helper (maybe never).

## 12. How it plugs into the current codebase

Act reuses, not rebuilds:
- **Mic + VAD** — the energy-VAD segmenter already emits per-utterance finals via a
  cancellation-safe worker; Act consumes those as command boundaries in VAD mode.
- **Hotkeys** — `effective_hotkey_mode` already forces toggle-only in VAD and
  allows hold-or-toggle in Batch; Act inherits it directly.
- **STT** — the native Gemini provider (fast tier default, precise tier backup) is
  the same transport; Act adds a planner call after the transcript.
- **Pipeline** — the executor is a new sibling to the text-insertion path, gated by
  a new "Act" pipeline state.

New crates/FFI: `uiautomation-rs` (Windows), a Swift AX shim or `cidre` (macOS),
`enigo` (synthesis). New Rust modules: `a11y_snapshot_service`, `platform_uia`,
`platform_ax`, `grounding_resolver`, `fastpath_router`, `planner`,
`capability_gate`, `action_executor`, `kill_switch`, `audit_log`.

## 13. Open decisions for the product owner

1. **Primary OS first** — Windows (mature Rust UIA via `uiautomation-rs`, our
   recommendation) vs macOS (bigger clinician base but AX-in-Rust gap needs a Swift
   shim).
2. **Default Act modality** — recommend **Batch/hold-to-talk** as the default for
   accuracy, VAD as the opt-in power mode.
3. **Local fast-path model** — keyword+slot to start; a tiny on-device intent model
   later.
4. **Vision fallback** — ship the stub disabled; decide if/when to enable for
   accessibility-blind apps, given the PHI posture.
5. **Elevated-app control** — likely "detect and decline" indefinitely; a signed
   helper is a large surface for marginal gain.

---

*Council method: an adversarial design pass and a dedicated open-source landscape
audit were run in parallel and reconciled against the shipped code. A third
constructive-design stream did not return in time and is not reflected here; its
findings can be folded in as an addendum without changing the thesis.*
