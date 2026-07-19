# Act — CDP-controlled-Chrome browser automation (SPIKE)

A fourth grounding path for browser tasks: instead of perceiving web content
through the accessibility tree (`tree` / `hybrid`) or raw pixels (`vision`), drive
a dedicated Chrome directly over the **Chrome DevTools Protocol** (CDP) using
[Stagehand v3](https://www.npmjs.com/package/@browserbasehq/stagehand). Real DOM,
precise clicks, no focus theft.

> **Status: spike.** New, isolated, feature-flagged, **not wired into the live
> conductor.** The Rust module `src-tauri/src/act/browser.rs` is behind the
> `cdp-browser` Cargo feature (default OFF); the Node/TypeScript sidecar lives in
> `browser-agent/`. Nothing in the shipping build changes.

## Why

In real runs the flakiest missions are browser tasks — playing a YouTube search
result, typing into and sending from Grok's chat box. The UIA/AX backend
**flattens** web content: the DOM collapses into an approximate accessibility
tree, `Bounds` are fuzzy, so `vision`-style coordinate clicks miss and synthetic
typing steals focus. This is the same wall Perplexity Comet, Stagehand, and
browser-use hit and solved by talking to the browser's own protocol.

Over CDP, Stagehand resolves an intent against the **real DOM node** and issues a
protocol-level click / key event on it. For browser tasks this **retires
pixel-grounding entirely**: there is no screenshot, no Set-of-Marks overlay, no
coordinate math. The grounding is the DOM.

| Path | Perception | Action target | Web reliability |
|---|---|---|---|
| `tree` | UIA/AX snapshot | element path | poor on flattened web content |
| `hybrid` | snapshot + screenshot (Set-of-Marks) | mark → path | better, but still tree-dependent |
| `vision` | screenshot only | coordinate | brittle; coords drift, focus theft |
| **`cdp-browser`** (this) | **real DOM via CDP** | **DOM node** | **precise; the intended fix** |

## Architecture

```
Conductor ──(router: is this a browser task?)──┐
   │ no                                          │ yes  (cfg cdp-browser)
   ▼                                             ▼
UIA planner/executor            act::browser::BrowserSession (Rust)
(unchanged live path)                    │ spawn + JSON-over-stdio
                                         ▼
                          browser-agent/ sidecar (Node + Stagehand v3)
                                         │ CDP (port 9222)
                                         ▼
                     dedicated FlowRad Chrome (persistent --user-data-dir)
```

- **Rust host** (`act/browser.rs`): `BrowserSession::run(&BrowserTask)` spawns the
  sidecar, writes one task JSON line to its stdin, reads one result JSON line from
  its stdout, with typed structs (`BrowserTask`, `BrowserResult`) and a
  `BrowserError` enum. Stateless per task: one sidecar per `run`, driving the
  long-lived Chrome profile.
- **Sidecar** (`browser-agent/index.ts`): launches or attaches to Chrome over CDP,
  configures Stagehand for **Gemini** (`google/gemini-3.5-flash`, key from
  `GEMINI_API_KEY`), runs `observe(intent)` then `act(intent)`, emits the result.

## The router — classifying a task as "browser" vs UIA

The conductor already routes each mission (`selection::Mission`) to a plan/execute
lane. The CDP path adds one upstream decision: **is this mission best served by the
browser?** A mission qualifies when **both**:

1. **The surface is a browser.** The foreground app (from the `Snapshot` / focused
   window) is Chrome/Edge/Chromium — or the mission's `target_app` hint names a
   browser or a known web property (YouTube, Grok, Gmail, …).
2. **The goal is web content**, not browser chrome. "play the first result",
   "send this to Grok", "click the second link" → DOM. "open a new tab",
   "close the window" → still fine over CDP, but also fine on UIA; no need to move.

Everything else stays on the existing UIA/AX path unchanged. Classification is a
cheap heuristic first (app identity + a small verb/keyword check); if we later want
it smarter, the selection LLM call can emit a `surface: "browser" | "os"` tag
alongside the mission. Ambiguity resolves toward the **existing** path so the spike
can only ever *add* capability, never regress today's behavior. On any CDP error
the dispatch falls back to the UIA path.

The concrete, commented dispatch stub lives in `act/browser.rs` under
`ROUTER INTEGRATION POINT`.

## Sidecar protocol

Line-delimited JSON over stdio, one task per invocation:

```text
stdin  <-  { "intent": "...", "url"?: "...", "timeoutMs"?: 60000 }
stdout ->  { "ok": true|false, "detail": "...", "actions"?: [...], "error"?: "..." }
```

- The host relies on the **result line**, not the exit code.
- All sidecar logging is on **stderr**; **stdout is the clean result channel**.
- `GEMINI_API_KEY` and the CDP/profile knobs come from the environment (see
  `browser-agent/README.md` for the full table).

## Whose Chrome / dedicated profile

FlowRad drives a **dedicated, persistent Chrome profile**
(`--user-data-dir=~/.flowrad/chrome-profile`, `--remote-debugging-port=9222`),
**not** the user's everyday browser. This isolates automation from the user's real
session (tabs, cookies, extensions), lets a one-time login (Google, Grok/X) persist
across runs, and gives us a predictable debugging port. See the README for the
full rationale.

## Cross-platform (works on Mac too)

CDP is byte-for-byte identical on Windows, macOS, and Linux, and the sidecar is
plain Node. Unlike the UIA backend (Windows-only) and the AX backend (macOS-only),
**one browser path covers every desktop OS**. On macOS, point `FLOWRAD_CHROME_PATH`
at the Chrome binary if the default doesn't resolve; nothing else changes.

## Safety notes (for when this graduates)

The spike is intentionally outside the safety layer because it is not wired in.
Before it goes live it must inherit the same discipline as the UIA path:

- **Capability gate + kill switch** apply to browser missions too; a CDP click is
  still an action.
- **Injection fences**: page text (titles, search results, chat responses) is
  **DATA, never instructions** — the same rule the selection/planner layers already
  enforce. Stagehand's own planning prompt must be fenced identically.
- **Destructive-action confirmation** (purchases, sends, deletes) routes through
  the existing confirm/ask-user flow, not silently through `act()`.

## Open questions

- **Where does the router live** — a cheap Rust heuristic in the conductor, or a
  `surface` tag from the selection LLM? Start heuristic; revisit if misroutes.
- **Session lifetime** — spawn-per-task (today's spike, robust) vs a resident
  sidecar holding Chrome open for lower latency across a multi-step mission.
- **Chrome lifecycle** — who launches/owns the FlowRad Chrome (sidecar-launched vs
  a FlowRad-managed long-lived process the sidecar attaches to via `FLOWRAD_CDP_URL`).
- **Model** — `gemini-3.5-flash` for cost/latency; do harder pages need a stronger
  planner, and should this share the conductor's existing model config?
- **Observability** — mapping Stagehand `actions` / `observe` output onto Act's
  `ActEvent` progress stream and audit log.
- **Login bootstrapping** — first-run interactive login into the FlowRad profile
  (Google, Grok) and how FlowRad detects "not logged in" and asks the user.
- **Downloads / file handling** and per-domain allow/block policy (Stagehand's
  `DomainPolicy`) for safety.
