# Act: background actuation + screenshot pipeline (design)

Status: **design / not yet implemented.** Companion to the shipped latency+safety
sprint (fastpath hoist, foreground hard-stop, per-stage timing). The two items
here are deferred out of that PR because each needs **Windows runtime
verification** and touches a coupling that unit tests can't catch. This doc is the
implementation spec.

---

## 1. Screenshot pipeline (Change 3) — deferred, with a coordinate trap

### Why it's not in the sprint
The audit is right that `capture_screen` (`windows.rs`) grabs the **full primary
monitor** and PNG-encodes RGBA8 at native resolution (3–8 MB on a 1440p/4K
display), which is slow to encode, slow to upload, and inflates Gemini's
image-prefill bill. But there is a **trap**:

> `denormalize_clicks` (`planner.rs:~174`, called at `~383`) scales the model's
> normalized `0–1000` click coordinates against the **screenshot's own pixel
> dimensions** (`png_dimensions(image)`), NOT the real screen size.

Today that's correct *only because the screenshot is the full monitor*, so
`image dims == screen dims`. **The moment you downscale the screenshot, the image
dims diverge from the real screen, and every vision click lands in the wrong place**
(e.g. a 1568-wide image on a 3840-wide screen → all clicks land in the left ~40%).
This is a silent, per-click regression — exactly the failure that unit tests pass
through and a user rages at.

### The correct implementation (two coupled changes, ship together)

**A. Decouple denormalization from the image size.** `denormalize_clicks` must
scale `0–1000` against the **real screen dimensions**, independent of the uploaded
image's pixels. Options, cheapest first:
1. Add a `screen: (u32, u32)` (real monitor size) to `PlanRequest` / `Perception`,
   set at capture time, and denormalize against that instead of `png_dimensions`.
2. Or keep denormalizing against `png_dimensions` but **stamp the original monitor
   dimensions into the returned capture** and thread them through — more plumbing,
   same effect.

Option 1 is the clean one. It also future-proofs foreground-window capture (below),
where image dims ≠ screen dims by construction.

**B. Then downscale + re-encode.** In `capture_screen`, after `capture_image()`:
```rust
use image::{DynamicImage, imageops::FilterType};
let dynimg = DynamicImage::ImageRgba8(img);          // xcap returns RgbaImage
const MAX_EDGE: u32 = 1568;                            // long-edge cap (Gemini-friendly)
let scaled = if dynimg.width().max(dynimg.height()) > MAX_EDGE {
    dynimg.resize(MAX_EDGE, MAX_EDGE, FilterType::Triangle) // preserves aspect
} else {
    dynimg
};
```
Encode choice:
- **JPEG q≈80** is the big win (10–20× smaller than PNG for a screenshot). Requires
  adding `"jpeg"` to the `image` crate features in `Cargo.toml` (currently
  `["png"]`, twice — the `[dependencies]` and the target-specific block), AND
  changing the hardcoded `"mimeType": "image/png"` at `llm.rs:~149` to
  `image/jpeg`. That mime is **shared with the macOS backend** — either switch
  macOS capture to JPEG too, or change `capture_screen` to return `(bytes, mime)`
  and thread the mime to the `inlineData` part. Returning `(Vec<u8>, &'static str)`
  is the clean seam.
- If you want zero mime churn first, **downscale-only, keep PNG** — still a large
  win (fewer pixels → smaller PNG + base64 + prefill), and it's the smallest
  diff once change (A) is in. Add JPEG in a follow-up.

**C. (Optional, later) Foreground-window-only capture.** Instead of the whole
monitor, capture just the active window's rect. Fewer pixels, and it matches the
HWND-keyed grounding model. Needs the foreground window rect (Win32
`GetForegroundWindow` + `GetWindowRect`) and — because the image is now a
sub-region — denormalization/anchoring against that rect's origin+size, which is
exactly why change (A) must land first.

### Verification (needs a Windows box)
1. Vision-mode click on a known target (e.g. a specific toolbar button) lands
   correctly at 4K, 1440p, and 1080p, and on a non-primary-DPI setup.
2. Gemini accepts the JPEG `inlineData` (it does; JPEG is universally supported).
3. Scoreboard `total_ms` for a vision turn drops materially vs. the PNG baseline.

---

## 2. Background actuation ladder (Change 5) — the "runs in the back" architecture

### The core fact
UIA control patterns act **cross-process without moving the cursor, without
focusing the window, without touching the user's input queue**:
- `InvokePattern.Invoke()` — buttons, links
- `ValuePattern.SetValue()` — text fields
- `ScrollItemPattern`, `ExpandCollapsePattern`, `SelectionItemPattern`

The executor's `invoke` / `set_value` / `scroll_into_view` are **already
background-safe**. What's missing is (a) the planner *preferring* them, and (b) a
policy that only "borrows" the foreground for the irreducible slice that needs
real input.

### The four-tier actuation policy (decided per action)
```
1. shell / API      → deterministic, invisible          (recipes exist today;
                       teach the planner to prefer them)
2. UIA pattern      → background invoke/set_value/scroll (executor has this)
3. foreground input → focus-arbitrated SendInput,        (bounded ~300ms,
                       real keystrokes / menus / drag       announced, then restore)
4. session isolate  → PiP RDP loopback desktop           (Pro/Enterprise only)
```
The **capability gate** already decides *whether* an action may run; this decides
*how intrusively* it runs. Perception + planning for N agents are read-only and
run fully parallel; only tier-3 foreground actions serialize through an arbitrator.

### Concrete pieces to build
- **`ActuationMode` enum** on `Action` (or derived by the executor per action):
  `Background | Foreground | Session`. Default `Background`.
- **Executor routing:** for `Click`-class intents, prefer resolving a UIA target
  and `invoke`-ing it (background) over a coordinate click (foreground). The
  coordinate click becomes the tier-3 fallback, gated by the foreground hard-stop
  the sprint just added.
- **Focus arbitrator:** extend `focus_guard` from "is the expected app in front?"
  into a serializer: hold the foreground only for the tier-3 slice of a plan,
  restore the user's prior foreground after, and never grab focus for an app the
  user is *actively typing in* (queue or PiP instead).
- **Planner preference:** one line in the contract — try shell/API, then UIA, then
  foreground — plus a small recipe library so the planner knows when a shell route
  exists (the `create_folder`/`copy_item`/… recipes are the seed).

### Verification (needs a Windows box)
- A multi-step web/file task completes with the user's cursor **never moving** and
  their focused window **unchanged**, progress visible only in the orbs/toasts.
- A task that genuinely needs a keystroke borrows the foreground for <500ms and
  restores it.

---

## 3. Microsoft UFO² — reference, not dependency

`github.com/microsoft/UFO` (MIT). A Windows "AgentOS" research prototype. **Do not
build on it or port its code** — it's Python/pywinauto/YAML FSM agents, and our
conductor/executor/safety stack is already ahead. Two things are worth reading:

- **`ufo/client/computer.py`** — the PiP/session abstraction. The trick isn't
  exotic: UFO² hosts Microsoft's own Terminal Services ActiveX control
  (`MsTscAx`, from `mstscax.dll`) in a window pointed at the **loopback address**,
  giving a real second session (own input queue, clipboard, device context)
  rendered in a resizable child window. Our tier-4 port: `windows-rs` → host
  `MsTscAx` in a native child HWND (or an embedded Tauri window) → connect to
  `localhost` → run a small Act-executor process **inside** that session → talk to
  it over authenticated named pipes. **Pro/Enterprise + virtualization only**
  (Windows Home cannot host the loopback session), so tier-4 is a capability probe
  at startup, not a requirement — tiers 1–3 cover Home.
- **Speculative multi-action batching** (`ufo/agents/`) — execute several planned
  actions before re-observing, ~51% fewer LLM calls. Maps directly onto our ReAct
  loop; worth reading before the next latency pass.

Net: UFO² validates the two bets we already made (UIA-first grounding, background
isolation). Steal the `MsTscAx` recipe for tier-4 when we get there; port concepts,
not files.

---

## Sequencing
1. **Shipped now:** fastpath hoist, foreground hard-stop, per-stage timing.
2. **Next (this doc, §1):** decouple denormalization from image size → downscale
   → JPEG. Windows-verified.
3. **Then (§2):** actuation-mode enum + executor UIA-preference + focus arbitrator
   (tiers 1–3). This is the "runs in the back" feature; Home-compatible.
4. **Later (§2 tier-4 / §3):** `MsTscAx` PiP session for true isolation on
   Pro/Enterprise; speculative batching for the next latency pass.
