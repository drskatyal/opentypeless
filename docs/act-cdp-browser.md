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

### `is_browser_task` — the concrete decision (feature `cdp-browser`)

The heuristic is implemented as a pure, unit-tested function in
`act/browser.rs`:

```rust
pub fn is_browser_task(foreground_app: &str, goal: &str) -> bool
```

It returns `true` only when **both** conditions hold:

1. **Browser surface.** `foreground_app` normalizes to a known browser. This
   reuses the exact browser-name matching the live UIA path already uses: the
   shared helper `conductor::app_is_browser` (which backs `foreground_is_browser`)
   normalizes the app name (lowercase, `.exe`-stripped) and matches it against
   `BROWSER_APP_STEMS` (chrome, chromium, msedge, brave, firefox, opera, vivaldi,
   safari). Both paths therefore agree on what "a browser" is.
2. **Web-content goal.** A light keyword/verb check on the goal:
   - **Browser-chrome / OS-window** goals are matched *first* and route to UIA:
     "open a new tab", "close the window", "minimize", "incognito", "bookmark",
     "downloads", "zoom in", "quit chrome", … These work on UIA today and are not
     DOM content.
   - Otherwise a positive **web-content** signal is required — verbs/nouns like
     *play, watch, video, result, link, click, search, scroll, type, send,
     message, post, subscribe, sign in, submit, buy, first/second/third*, … — to
     return `true`.
   - **Ambiguous** goals with no clear signal (e.g. "do the thing", "continue")
     return `false` and fall back to UIA.

The function is exercised by unit tests in the same file: browser + web goal →
`true`; non-browser foreground → `false`; browser + clearly-OS goal → `false`;
ambiguous → `false`; and case-insensitivity.

The concrete, commented dispatch stub that would call it lives in
`act/browser.rs` under `ROUTER INTEGRATION POINT`. It is **not** wired into the
live conductor; the whole module compiles only under `--features cdp-browser`, so
default builds and tests are unchanged.

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

### Two modes: `act` (LLM) and `links` (deterministic)

The task carries an optional `mode` field (default `"act"`):

```text
stdin <- { "intent": "...", "url"?: "...", "timeoutMs"?: 60000,
           "mode"?: "act" | "links", "select"?: <number | string> }
```

- **`act`** (default, unchanged): `observe(intent)` then `act(intent)` — Stagehand
  plans the intent with the LLM and executes precise CDP clicks/typing on the real
  DOM node. Requires `GEMINI_API_KEY`.

- **`links`** (new, **deterministic — no LLM**): enumerate the active tab's anchor
  links from the DOM (`document.querySelectorAll('a')`, equivalent to a CDP
  `Runtime.evaluate`), collect each `{ text, href }` (resolved absolute, http/https
  only, in DOM order), pick one, and **navigate the tab directly to its href**.
  Does **not** require `GEMINI_API_KEY`. The `select` hint chooses the target:
  - a **1-based position** (`2` or `"2"`) → the Nth extracted link (e.g. "play the
    first result" → `select: 1`);
  - **text** (`"lofi hip hop"`) → the best text match among the links;
  - **omitted** → best text match against the `intent`, falling back to the first
    link.

  The result adds the chosen href and the full candidate list for auditing:

  ```text
  stdout -> { "ok": true, "detail": "navigated to <text>",
              "chosenHref": "https://…", "candidates": [ { "index": 1, "text": "…", "href": "…" }, … ] }
  ```

  This is the robust path for "play the Nth result / click the link that says X":
  no screenshot, no LLM planning, no coordinate math — just DOM anchor text/href
  and a direct navigation. The `act` path remains available for interactions that
  are not a plain link navigation (typing into a box, clicking a non-anchor
  control, sending a chat message).

## Whose Chrome / dedicated profile

FlowRad drives a **dedicated, persistent Chrome profile**
(`--user-data-dir=~/.flowrad/chrome-profile`, `--remote-debugging-port=9222`),
**not** the user's everyday browser. This isolates automation from the user's real
session (tabs, cookies, extensions), lets a one-time login (Google, Grok/X) persist
across runs, and gives us a predictable debugging port. See the README for the
full rationale.

> **Caveat — attaching to the user's own Chrome.** CDP can only talk to a Chrome
> that was **started with `--remote-debugging-port`**. A normally-launched Chrome
> exposes no CDP endpoint, so there is no way to attach to the user's existing
> everyday browser after the fact. To attach (via `FLOWRAD_CDP_URL`) that Chrome
> must have been launched with the flag (and, in practice, a distinct
> `--user-data-dir`, since Chrome refuses a second debugging session on a profile
> already open in a running instance). This is the other reason FlowRad launches
> its **own** Chrome with the flag set rather than reaching into the user's — the
> default path (no `FLOWRAD_CDP_URL`, sidecar launches the FlowRad Chrome itself)
> is the one that "just works".

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
