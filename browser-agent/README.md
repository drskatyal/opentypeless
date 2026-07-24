# FlowRad browser-agent (SPIKE)

A small Node/TypeScript **sidecar** that drives a **CDP-controlled Chrome** with
[Stagehand v3](https://www.npmjs.com/package/@browserbasehq/stagehand) to run one
browser task at a time. It is the browser half of an experiment to make Act mode's
web tasks reliable.

> **Status: spike.** This is new, isolated, and **not wired into the live
> conductor.** The Rust side (`src-tauri/src/act/browser.rs`) is behind the
> `cdp-browser` Cargo feature (default OFF). Design notes:
> [`../docs/act-cdp-browser.md`](../docs/act-cdp-browser.md).

## Why a dedicated Chrome over CDP (the "whose Chrome" rationale)

In real runs, browser tasks — playing a YouTube result, driving Grok's chat box —
are the flakiest part of the UIA/AX path. UIA **flattens** web content: the
accessibility tree collapses the DOM, element bounds are approximate, and so
coordinate clicks land in the wrong place and typing steals focus.

Driving Chrome directly over the **Chrome DevTools Protocol** (the approach behind
Perplexity Comet, Stagehand, and browser-use) fixes this: Stagehand sees the
**real DOM**, resolves a target to an actual node, and issues **precise CDP clicks
and key events** — no flattening, no coordinate guessing, no focus theft.

We deliberately drive a **dedicated, persistent FlowRad Chrome profile**, not the
user's everyday browser:

- **Isolation** — automation never fights the user's real session, tabs, extensions,
  or cookies, and a runaway task can't touch their logged-in banking tab.
- **Persistence** — a login FlowRad performs once (Google, Grok/X) lives in the
  FlowRad profile and survives across runs, so tasks don't re-auth every time.
- **A clean debugging port** — the FlowRad Chrome owns `--remote-debugging-port`
  on a known port without colliding with a user-launched Chrome.

The profile lives at `--user-data-dir` (default `~/.flowrad/chrome-profile`) and
Chrome is launched with `--remote-debugging-port=9222` (both overridable, below).

## Protocol

One task per process invocation. Line-delimited JSON over stdio:

```text
stdin  <-  { "intent": "play the first result", "url": "https://youtube.com/results?search_query=lofi", "timeoutMs": 60000 }
stdout ->  { "ok": true, "detail": "clicked the first video", "actions": [ ... ] }
```

- **`intent`** (required) — natural-language goal for this turn.
- **`url`** (optional) — navigate here before acting.
- **`timeoutMs`** (optional) — per-task ceiling; falls back to `FLOWRAD_TASK_TIMEOUT_MS`.
- **`mode`** (optional) — `"act"` (default) or `"links"` (see below).
- **`select`** (optional, `"links"` mode) — a 1-based position (`2` / `"2"`) for the
  Nth link, or text (`"lofi hip hop"`) for the best text match.

On failure the result line is `{ "ok": false, "detail": "...", "error": "..." }`.
The host relies on this **result line**, not the process exit code (we still exit
non-zero on failure for humans). All logging goes to **stderr**, so **stdout stays
a clean single-line result channel**.

### `act` mode (default, LLM)

Each run does `observe(intent)` (proves the DOM is grounded and surfaces candidate
actions) then `act(intent)` (Gemini plans, Stagehand executes precise CDP
clicks/typing). Requires `GEMINI_API_KEY`.

### `links` mode (deterministic, no LLM)

Enumerate the active tab's anchor links from the DOM
(`document.querySelectorAll('a')`), collect each `{ text, href }` (resolved
absolute, http/https, in DOM order), pick one via `select` (or the best text match
against `intent`, falling back to the first link), and **navigate the tab directly
to that href**. No LLM, no `GEMINI_API_KEY` needed. The result adds the chosen href
and the extracted candidate list:

```text
stdin  <-  { "intent": "play the first result", "url": "https://www.youtube.com/results?search_query=lofi", "mode": "links", "select": 1 }
stdout ->  { "ok": true, "detail": "navigated to ...", "chosenHref": "https://...", "candidates": [ { "index": 1, "text": "...", "href": "..." }, ... ] }
```

## Environment variables

| Variable                    | Default                        | Purpose |
| --------------------------- | ------------------------------ | ------- |
| `GEMINI_API_KEY`            | — (**required**)               | Google Generative AI key; Stagehand uses it to plan each turn. |
| `FLOWRAD_BROWSER_MODEL`     | `google/gemini-3.6-flash`      | Planner model. The `google/` prefix routes to Gemini; bare `gemini-3.5-flash` also works. |
| `FLOWRAD_CDP_PORT`          | `9222`                         | DevTools remote-debugging port for the launched Chrome. |
| `FLOWRAD_CDP_URL`           | — (unset ⇒ launch Chrome)      | If set (e.g. `http://127.0.0.1:9222`), **attach** to an already-running Chrome instead of launching one. |
| `FLOWRAD_USER_DATA_DIR`     | `~/.flowrad/chrome-profile`    | Dedicated, persistent FlowRad Chrome profile. |
| `FLOWRAD_CHROME_PATH`       | Stagehand default              | Explicit Chrome/Chromium binary. |
| `FLOWRAD_HEADLESS`          | `false`                        | `true` runs Chrome without a window (CI). |
| `FLOWRAD_TASK_TIMEOUT_MS`   | `60000`                        | Default per-task timeout. |

## Install & run

Requires Node `^20.19.0 || >=22.12.0`.

```sh
cd browser-agent
npm install
npm run build          # tsc -> dist/index.js   (or: npm run typecheck)

# One task, launching the FlowRad Chrome on port 9222:
export GEMINI_API_KEY=...   # required
echo '{"intent":"play the first result","url":"https://www.youtube.com/results?search_query=lofi"}' \
  | node dist/index.js

# During development (no build step):
echo '{"intent":"scroll down"}' | npm run dev

# Deterministic links mode (no GEMINI_API_KEY needed): navigate to the 1st result.
echo '{"intent":"play the first result","url":"https://www.youtube.com/results?search_query=lofi","mode":"links","select":1}' \
  | node dist/index.js
```

To attach to a Chrome you launched yourself:

```sh
google-chrome --remote-debugging-port=9222 --user-data-dir="$HOME/.flowrad/chrome-profile" &
FLOWRAD_CDP_URL=http://127.0.0.1:9222 node dist/index.js < task.json
```

## Cross-platform

Nothing here is Windows-specific: CDP is identical on macOS and Linux, so the same
sidecar drives Chrome on a Mac (point `FLOWRAD_CHROME_PATH` at Chrome if needed).
This is a key reason the CDP path is attractive — unlike the UIA backend it is not
Windows-only.

## Not wired in

The Rust host that would spawn this sidecar
([`../src-tauri/src/act/browser.rs`](../src-tauri/src/act/browser.rs)) is gated
behind the `cdp-browser` feature and is **not called from the live Act loop**. See
the design doc for how the router would eventually classify a task as "browser" and
hand it here.
