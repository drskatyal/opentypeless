//! Windows accessibility backend for "Act mode", built on the Terminator SDK.
//!
//! This module is compiled only on Windows (`#[cfg(windows)]`, applied where the
//! module is declared). It delegates reading and acting to `terminator` (crate
//! `terminator-rs`, published by mediar-ai), a Playwright-style desktop
//! automation SDK layered on Windows UI Automation. Terminator supplies the
//! element tree, focus/invoke/set-value primitives, and input synthesis, so no
//! raw `uiautomation` or `enigo` plumbing is needed here.
//!
//! Design notes:
//!
//! * All Terminator work happens on ONE persistent UIA worker thread, owned by
//!   [`super::uia_broker`]. A `Desktop` and its element handles wrap COM
//!   interfaces with **thread affinity**, so the `Desktop` is constructed once
//!   and every op is dispatched to that thread as a `FnOnce(&Desktop)` closure
//!   that returns only plain owned data. This replaces the previous
//!   "`Desktop::new_default()` per action" (one COM/UIA bring-up per op) while
//!   still keeping the `async_trait` futures `Send` — no COM object is ever
//!   moved across a thread boundary or held across an `.await`. See
//!   [`super::uia_broker`] for the dispatch/watchdog/recovery design.
//! * Every op is bounded by a watchdog (see [`run_op`]); if a synchronous UIA
//!   call wedges the worker, the broker abandons it and the next op transparently
//!   spawns a fresh thread + `Desktop`, so "persistent" never means
//!   "unrecoverable".
//! * The raw text value of a control is never retained. Where a length is
//!   needed, the value is read locally, its `char` count is taken, and the
//!   string is dropped immediately (PHI safety).
//! * Timeout guard: synchronous UI Automation calls cannot be preempted from the
//!   calling thread, so the guard is cooperative. A wall-clock deadline is
//!   checked before each element probe (`probe`) and before descending, so a
//!   slow subtree is abandoned rather than walked; individual probe errors are
//!   treated as "skip this property". `snapshot` also wraps the whole blocking
//!   walk in a `tokio::time::timeout`, returning control to the caller even if a
//!   COM call is still wedged (the blocking thread is left to unwind).

use std::ffi::c_void;
use std::ptr;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use async_trait::async_trait;

use terminator::{AutomationError, Browser, Desktop, UIElement as TElement, UIElementAttributes};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_WHEEL, MOUSEINPUT,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, SW_SHOWNORMAL,
};

use crate::error::AppError;

use super::backend::{should_refocus, AccessibilityBackend, ShellOutput};
use super::element::{ActionPattern, Bounds, ElementPath, ElementState, Role, Snapshot, UiElement};
use super::uia_broker;

/// Map an internal `anyhow` error to the crate's `AppError` at the trait boundary.
fn to_app_err(err: anyhow::Error) -> AppError {
    AppError::Config(err.to_string())
}

/// Maximum number of *emitted* elements per snapshot. Kept in lock-step with the
/// grounding budget (`grounding_packet::DEFAULT_MAX_ELEMENTS`) so the walk stops
/// as soon as it has produced everything the planner will actually see — no work
/// is spent building elements that grounding would immediately discard.
const DEFAULT_ELEMENT_CAP: usize = super::grounding_packet::DEFAULT_MAX_ELEMENTS;
/// Hard ceiling on nodes *visited* (traversed) during a single walk. Structural
/// containers (Pane/Group/Document) are walked for their descendants but not
/// emitted, so this bounds the number of `attributes()`/`children()` cross-process
/// calls independently of how many actionable controls are found.
const DEFAULT_VISIT_CAP: usize = 600;
/// Maximum subtree depth walked below the focused window.
const DEFAULT_MAX_DEPTH: usize = 12;
/// Cooperative wall-clock budget for a full snapshot walk. A single wedged UIA
/// provider call cannot be preempted cooperatively (only the broker watchdog can),
/// so this is the budget for a *healthy* walk; the structural changes below keep a
/// normal walk well under it.
const DEFAULT_SNAPSHOT_BUDGET: Duration = Duration::from_secs(3);

/// How long a walked snapshot stays reusable for the same foreground window. Two
/// snapshots of the same HWND within this window (with no intervening mutating op,
/// which invalidates the cache) are served from the first walk instead of paying a
/// second slow cross-process read. Kept short so a UI change driven from outside
/// this backend cannot serve stale grounding for long.
const SNAPSHOT_CACHE_TTL: Duration = Duration::from_millis(750);

/// Watchdog budget for a single element/action op on the UIA worker. Generous
/// enough for a slow but healthy provider; anything past this is treated as a
/// wedged worker and triggers recreation (see [`super::uia_broker`]).
const OP_WATCHDOG: Duration = Duration::from_secs(10);
/// Extra margin added on top of an op's own internal timeout before the broker
/// watchdog fires, so a legitimately long op (e.g. `run_shell`) is never killed
/// by the watchdog first.
const WATCHDOG_MARGIN: Duration = Duration::from_secs(3);
/// Internal wall-clock timeout for `run_shell`'s child command.
const SHELL_TIMEOUT: Duration = Duration::from_secs(15);
/// Max captured shell output, in `char`s.
const SHELL_MAX_OUTPUT_CHARS: usize = 8192;

/// Terminator-backed Windows implementation of [`AccessibilityBackend`].
#[derive(Debug, Clone)]
pub struct WindowsUiaBackend {
    /// Max emitted (actionable) elements — the walk early-stops here.
    element_cap: usize,
    /// Max nodes traversed — bounds cross-process calls on chrome-heavy trees.
    visit_cap: usize,
    max_depth: usize,
    snapshot_budget: Duration,
}

impl Default for WindowsUiaBackend {
    fn default() -> Self {
        Self {
            element_cap: DEFAULT_ELEMENT_CAP,
            visit_cap: DEFAULT_VISIT_CAP,
            max_depth: DEFAULT_MAX_DEPTH,
            snapshot_budget: DEFAULT_SNAPSHOT_BUDGET,
        }
    }
}

impl WindowsUiaBackend {
    /// Create a backend with default limits.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AccessibilityBackend for WindowsUiaBackend {
    async fn snapshot(&self) -> std::result::Result<Snapshot, AppError> {
        let element_cap = self.element_cap;
        let visit_cap = self.visit_cap;
        let max_depth = self.max_depth;
        let budget = self.snapshot_budget;

        // The build itself is cooperatively bounded by `budget`; the broker
        // watchdog adds a small margin so a wedged COM call still returns control
        // to the caller (leaving the stuck worker to be recreated).
        run_op(budget + WATCHDOG_MARGIN, "snapshot", move |desktop| {
            build_snapshot(desktop, element_cap, visit_cap, max_depth, budget)
        })
        .await
        .map_err(to_app_err)
    }

    async fn focus(&self, path: &ElementPath) -> std::result::Result<(), AppError> {
        // Any op that can change the on-screen UI invalidates the reuse cache so
        // the next snapshot re-reads the (now changed) foreground window.
        invalidate_snapshot_cache();
        let path = path.to_string();
        run_op(OP_WATCHDOG, "focus", move |desktop| {
            let element = resolve(desktop, &path)?;
            // Prefer programmatic focus; fall back to a click when the control
            // cannot be focused directly.
            element
                .focus()
                .or_else(|_| element.click().map(|_| ()))
                .map_err(to_anyhow)
        })
        .await
        .map_err(to_app_err)
    }

    async fn invoke(&self, path: &ElementPath) -> std::result::Result<(), AppError> {
        invalidate_snapshot_cache();
        let path = path.to_string();
        run_op(OP_WATCHDOG, "invoke", move |desktop| {
            let element = resolve(desktop, &path)?;
            // Prefer the Invoke pattern; fall back to a click when unsupported.
            element
                .invoke()
                .or_else(|_| element.click().map(|_| ()))
                .map_err(to_anyhow)
        })
        .await
        .map_err(to_app_err)
    }

    async fn set_value(
        &self,
        path: &ElementPath,
        value: &str,
    ) -> std::result::Result<(), AppError> {
        invalidate_snapshot_cache();
        let path = path.to_string();
        let value = value.to_string();
        run_op(OP_WATCHDOG, "set_value", move |desktop| {
            let element = resolve(desktop, &path)?;
            match element.set_value(&value) {
                Ok(()) => Ok(()),
                Err(_) => {
                    // No Value pattern: focus and type the value directly.
                    let _ = element.focus();
                    element.type_text(&value, false).map_err(to_anyhow)
                }
            }
        })
        .await
        .map_err(to_app_err)
    }

    async fn type_text(&self, text: &str) -> std::result::Result<(), AppError> {
        invalidate_snapshot_cache();
        let text = text.to_string();
        run_op(OP_WATCHDOG, "type_text", move |desktop| {
            let focused = desktop.focused_element().map_err(to_anyhow)?;
            focused.type_text(&text, false).map_err(to_anyhow)
        })
        .await
        .map_err(to_app_err)
    }

    async fn key_combo(&self, combo: &str) -> std::result::Result<(), AppError> {
        invalidate_snapshot_cache();
        let combo = combo.to_string();
        run_op(OP_WATCHDOG, "key_combo", move |desktop| {
            let keys = translate_combo(&combo)?;
            let focused = desktop.focused_element().map_err(to_anyhow)?;
            focused.press_key(&keys).map_err(to_anyhow)
        })
        .await
        .map_err(to_app_err)
    }

    async fn focused_app_is_elevated(&self) -> std::result::Result<bool, AppError> {
        // Any failure is a soft "not elevated": this is only a detect-and-decline
        // hint, never a security boundary.
        run_op(OP_WATCHDOG, "focused_app_is_elevated", |desktop| {
            Ok(foreground_is_elevated(desktop))
        })
        .await
        .map_err(to_app_err)
    }

    // Script primitives. These delegate to Terminator (launch / activate / open
    // URL / run) and arboard (clipboard). The executor has already run the shell
    // Deny classifier, the origin/injection guards, and the capability-gate
    // confirm before any of these are called — this layer just performs the
    // vetted OS action. No elevation is ever requested.
    //
    // Known v1 limitation: run_shell has a 15s wall-clock timeout but the kill
    // switch cannot reap a shell child mid-run (the blocking thread + child keep
    // going until the command or the timeout completes). A confirmed, Deny-vetted,
    // non-elevated command running to completion is low-risk; a Job Object that
    // reaps the whole child tree on abort is a planned hardening.

    async fn launch(&self, target: &str) -> std::result::Result<(), AppError> {
        invalidate_snapshot_cache();
        let target = target.to_string();
        run_op(OP_WATCHDOG, "launch", move |desktop| {
            // Prefer a fast, fire-and-forget ShellExecuteW launch. It resolves
            // common apps via the App Paths registry (chrome, spotify, notepad),
            // returns the instant the launch is *issued*, and lets the closed loop
            // re-observe to find the new window. Terminator's `open_application`
            // instead WAITS for the app window via UIA, which blows past the 10s
            // watchdog on a cold start (this is exactly the "uia broker [launch]:
            // exceeded watchdog (10s)" failure on Spotify's first launch). Fall
            // back to it only when the shell can't resolve the name.
            if shell_open(&target).is_ok() {
                return Ok(());
            }
            desktop.open_application(&target).map_err(to_anyhow)?;
            Ok(())
        })
        .await
        .map_err(to_app_err)
    }

    async fn open_uri(&self, uri: &str) -> std::result::Result<(), AppError> {
        let uri = uri.to_string();
        run_op(OP_WATCHDOG, "open_uri", move |desktop| {
            let trimmed = uri.trim();
            let lower = trimmed.to_ascii_lowercase();
            let is_http = lower.starts_with("http://") || lower.starts_with("https://");

            // Does the URI carry an explicit non-web scheme (`shell:`, `ms-settings:`,
            // `file:`, `mailto:`, …)? A scheme is `letters[+-.]*` before the first `:`,
            // where the char after `:` is NOT a digit (so `host:port` is not mistaken
            // for a scheme).
            let has_uri_scheme = trimmed.find(':').is_some_and(|i| {
                let scheme = &trimmed[..i];
                let after = trimmed[i + 1..].chars().next();
                !scheme.is_empty()
                    && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                    && scheme
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
                    && !after.is_some_and(|c| c.is_ascii_digit())
            });

            // A bare, scheme-less web address ("youtube.com", "www.x.com/p") is the
            // common spoken form. Normalize it to https:// so it takes the browser
            // path instead of falling through to `shell_open` (which can't open a
            // bare domain → ShellExecuteW code 2). (B3)
            let web_url = if is_http {
                Some(trimmed.to_string())
            } else if !has_uri_scheme
                && trimmed.contains('.')
                && !trimmed.chars().any(char::is_whitespace)
            {
                Some(format!("https://{trimmed}"))
            } else {
                None
            };

            let Some(url) = web_url else {
                // Non-web schemes (`shell:`, `ms-settings:`, `file:`, `mailto:`, …)
                // open no browser window. terminator's `open_url` hands the URI to
                // ShellExecuteW and then *searches for a browser window with the
                // page title* — which never appears for a folder/settings URI, so it
                // spins to the watchdog (or mismatches an unrelated browser window).
                // Hand off directly to the shell instead: it returns as soon as the
                // registered handler is launched. (B5)
                return shell_open(trimmed);
            };

            // Web URL. Try the OS default browser first; if there is no default
            // association (ShellExecuteW error 2) fall back to Chrome then Edge,
            // which ShellExecuteW resolves via the App Paths registry. Each miss
            // returns fast — the ShellExecuteW call fails *before* any window hunt —
            // so http opens robustly regardless of default-browser config. (B3)
            let attempts = [None, Some(Browser::Chrome), Some(Browser::Edge)];
            let mut last_err: Option<AutomationError> = None;
            for browser in attempts {
                match desktop.open_url(&url, browser) {
                    Ok(_) => return Ok(()),
                    Err(e) => last_err = Some(e),
                }
            }
            Err(to_anyhow(last_err.expect("attempts is non-empty")))
        })
        .await
        .map_err(to_app_err)
    }

    async fn run_shell(
        &self,
        command: &str,
        shell: &str,
    ) -> std::result::Result<ShellOutput, AppError> {
        invalidate_snapshot_cache();
        let command = command.to_string();
        let shell = shell.to_string();
        // The child has its own `SHELL_TIMEOUT`; the broker watchdog sits a
        // margin above it so the watchdog never pre-empts a healthy command.
        run_op(
            SHELL_TIMEOUT + WATCHDOG_MARGIN,
            "run_shell",
            move |desktop| {
                // Terminator's `run` is async; drive it on a private current-thread
                // runtime so the non-Send Desktop / COM handles never cross an await.
                // (The UIA worker is a plain OS thread with no ambient tokio runtime,
                // so `block_on` here is safe.)
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| anyhow!("act.run_shell runtime: {e}"))?;
                let out = rt
                    .block_on(async {
                        tokio::time::timeout(
                            SHELL_TIMEOUT,
                            desktop.run(&command, Some(&shell), None),
                        )
                        .await
                    })
                    .map_err(|_| {
                        anyhow!("shell command timed out after {}s", SHELL_TIMEOUT.as_secs())
                    })?
                    .map_err(to_anyhow)?;
                let mut stdout = out.stdout;
                if !out.stderr.trim().is_empty() {
                    if !stdout.is_empty() {
                        stdout.push('\n');
                    }
                    stdout.push_str(&out.stderr);
                }
                if stdout.chars().count() > SHELL_MAX_OUTPUT_CHARS {
                    stdout = stdout
                        .chars()
                        .take(SHELL_MAX_OUTPUT_CHARS)
                        .collect::<String>()
                        + "…[output truncated]";
                }
                Ok(ShellOutput {
                    exit_code: out.exit_status.unwrap_or(-1),
                    stdout,
                })
            },
        )
        .await
        .map_err(to_app_err)
    }

    async fn focus_app(&self, name: &str) -> std::result::Result<bool, AppError> {
        invalidate_snapshot_cache();
        let name = name.to_string();
        run_op(OP_WATCHDOG, "focus_app", move |desktop| {
            Ok(desktop.activate_application(&name).is_ok())
        })
        .await
        .map_err(to_app_err)
    }

    async fn ensure_foreground(
        &self,
        app_hint: Option<&str>,
    ) -> std::result::Result<bool, AppError> {
        // Bringing a window forward changes the foreground, and the snapshot cache
        // is keyed by foreground HWND — drop it so the next snapshot re-reads the
        // (now corrected) window rather than the console that had stolen focus.
        invalidate_snapshot_cache();
        let hint = app_hint.map(str::to_string);
        run_op(OP_WATCHDOG, "ensure_foreground", move |desktop| {
            Ok(ensure_foreground_win(desktop, hint.as_deref()))
        })
        .await
        .map_err(to_app_err)
    }

    async fn clipboard_get(&self) -> std::result::Result<String, AppError> {
        // Clipboard ops don't touch the `Desktop`, but they still run on the UIA
        // worker so every OS interaction is serialized on one stable thread
        // (arboard opens/closes the clipboard per call; a consistent thread and
        // apartment avoids cross-thread clipboard ownership surprises).
        run_op(OP_WATCHDOG, "clipboard_get", |_desktop| {
            let mut clipboard =
                arboard::Clipboard::new().map_err(|e| anyhow!("clipboard open: {e}"))?;
            clipboard
                .get_text()
                .map_err(|e| anyhow!("clipboard read: {e}"))
        })
        .await
        .map_err(to_app_err)
    }

    async fn clipboard_set(&self, text: &str) -> std::result::Result<(), AppError> {
        let text = text.to_string();
        run_op(OP_WATCHDOG, "clipboard_set", move |_desktop| {
            let mut clipboard =
                arboard::Clipboard::new().map_err(|e| anyhow!("clipboard open: {e}"))?;
            clipboard
                .set_text(text)
                .map_err(|e| anyhow!("clipboard write: {e}"))?;
            Ok(())
        })
        .await
        .map_err(to_app_err)
    }

    async fn capture_screen(&self) -> std::result::Result<Option<Vec<u8>>, AppError> {
        // Screen capture is a GDI/DXGI grab, not a UIA op, so it runs on a blocking
        // pool thread rather than the single UIA worker (keeping the worker free
        // for element ops). Returns a PNG of the primary monitor for the vision /
        // hybrid plan modes.
        let png = tokio::task::spawn_blocking(|| -> std::result::Result<Vec<u8>, String> {
            use image::ImageEncoder;
            let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
            let monitor = monitors
                .iter()
                .find(|m| m.is_primary().unwrap_or(false))
                .or_else(|| monitors.first())
                .ok_or_else(|| "no monitor available".to_string())?;
            let img = monitor.capture_image().map_err(|e| e.to_string())?;
            let (w, h) = (img.width(), img.height());
            let mut png = Vec::new();
            image::codecs::png::PngEncoder::new(&mut png)
                .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
                .map_err(|e| e.to_string())?;
            Ok(png)
        })
        .await
        .map_err(|e| AppError::Config(format!("capture join error: {e}")))?
        .map_err(AppError::Config)?;
        Ok(Some(png))
    }

    async fn click_point(&self, x: i32, y: i32) -> std::result::Result<(), AppError> {
        // A coordinate left-click for `vision` mode. Invalidates the snapshot cache
        // (it may change the UI) and runs on the UIA worker like other input ops.
        invalidate_snapshot_cache();
        run_op(OP_WATCHDOG, "click_point", move |desktop| {
            desktop
                .click_at_coordinates(x as f64, y as f64)
                .map_err(to_anyhow)?;
            Ok(())
        })
        .await
        .map_err(to_app_err)
    }

    async fn scroll(&self, dx: i32, dy: i32) -> std::result::Result<(), AppError> {
        // Best-effort mouse-wheel scroll of the foreground window, so below-the-fold
        // content (search results, long lists) can be brought within reach. Runs on
        // the UIA worker (COM/STA-initialized, like the other input ops) and
        // invalidates the snapshot cache because the viewport moves. No `Desktop`
        // call is needed — the wheel is synthesized with `SendInput` and delivered
        // to the focused window.
        invalidate_snapshot_cache();
        run_op(OP_WATCHDOG, "scroll", move |_desktop| {
            send_wheel(dx, dy);
            Ok(())
        })
        .await
        .map_err(to_app_err)
    }

    async fn scroll_into_view(&self, path: &ElementPath) -> std::result::Result<(), AppError> {
        // Preferred offscreen-reach: terminator's `scroll_into_view` finds a
        // scrollable ancestor and drives the UIA ScrollItem/Scroll pattern (with
        // key fallbacks), re-checking bounds until the element is within the
        // viewport. The executor calls this best-effort before invoking an
        // `offscreen` target; surface a real error here so the caller can log it.
        invalidate_snapshot_cache();
        let path = path.to_string();
        run_op(OP_WATCHDOG, "scroll_into_view", move |desktop| {
            let element = resolve(desktop, &path)?;
            element.scroll_into_view().map_err(to_anyhow)
        })
        .await
        .map_err(to_app_err)
    }

    fn name(&self) -> &str {
        "windows-terminator"
    }
}

/// One mouse-wheel notch, in the `WHEEL_DELTA` units `SendInput` expects.
const WHEEL_DELTA: i32 = 120;

/// Synthesize mouse-wheel input for the foreground window. `dy > 0` scrolls the
/// content DOWN and `dx > 0` scrolls RIGHT, matching the
/// [`AccessibilityBackend::scroll`] contract. Windows' vertical-wheel convention
/// is the inverse — a POSITIVE `mouseData` scrolls up (away from the user) — so a
/// downward scroll is sent as a negative delta; the horizontal axis is already
/// right-positive.
fn send_wheel(dx: i32, dy: i32) {
    if dy != 0 {
        send_wheel_event(MOUSEEVENTF_WHEEL, -dy.saturating_mul(WHEEL_DELTA));
    }
    if dx != 0 {
        send_wheel_event(MOUSEEVENTF_HWHEEL, dx.saturating_mul(WHEEL_DELTA));
    }
}

/// Send a single wheel `INPUT` carrying `flags` (the vertical or horizontal wheel
/// event) and a signed `delta` in `WHEEL_DELTA` units.
fn send_wheel_event(flags: u32, delta: i32) {
    // SAFETY: a single, fully-initialized `INPUT` describing a mouse-wheel event.
    // `mi.mouseData` carries the signed wheel delta reinterpreted as u32 (the
    // documented ABI for wheel input); every other field is zeroed. `SendInput`
    // copies the struct for the duration of the call and retains nothing, and
    // `cbsize` is the exact size of one `INPUT`. Runs on the UIA worker, a normal
    // input-capable OS thread.
    unsafe {
        let mut input: INPUT = std::mem::zeroed();
        input.r#type = INPUT_MOUSE;
        input.Anonymous.mi = MOUSEINPUT {
            dx: 0,
            dy: 0,
            mouseData: delta as u32,
            dwFlags: flags,
            time: 0,
            dwExtraInfo: 0,
        };
        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Dispatch one op to the persistent UIA worker thread (see
/// [`super::uia_broker`]) and flatten the transport error into the op's own
/// `Result`.
///
/// `f` receives the worker's long-lived `&Desktop` and returns the op's
/// `Result<T>`; the whole call is bounded by `watchdog`. A broker-level failure
/// (watchdog timeout, dead worker) surfaces as an `Err` here, and the worker is
/// recreated on the next op.
async fn run_op<T, F>(watchdog: Duration, op: &'static str, f: F) -> Result<T>
where
    F: FnOnce(&Desktop) -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    match uia_broker::execute(watchdog, op, f).await {
        Ok(inner) => inner,
        Err(broker_err) => Err(anyhow!("uia broker [{op}]: {broker_err}")),
    }
}

/// Convert a Terminator error into an `anyhow` error.
fn to_anyhow(err: AutomationError) -> anyhow::Error {
    anyhow!("terminator error: {err}")
}

/// Issue a single guarded Terminator probe. Returns `None` when the deadline has
/// already passed (skip the call) or when the call itself fails.
fn probe<T, F>(deadline: Instant, f: F) -> Option<T>
where
    F: FnOnce() -> std::result::Result<T, AutomationError>,
{
    if Instant::now() >= deadline {
        return None;
    }
    f().ok()
}

/// Process-wide cache of the last successfully walked snapshot, keyed by the
/// foreground window handle. See [`cached_snapshot_for`] / [`store_snapshot`].
struct CachedSnapshot {
    /// Foreground `HWND` (as an `isize` key) the snapshot was taken for.
    hwnd: isize,
    /// When the walk that produced it completed.
    at: Instant,
    snap: Snapshot,
}

static SNAPSHOT_CACHE: OnceLock<Mutex<Option<CachedSnapshot>>> = OnceLock::new();

fn snapshot_cache() -> &'static Mutex<Option<CachedSnapshot>> {
    SNAPSHOT_CACHE.get_or_init(|| Mutex::new(None))
}

/// The current foreground window handle as an `isize` key, or `0` if none.
///
/// `GetForegroundWindow` has no preconditions and is safe to call from any
/// thread; it is only ever used here as a cheap cache key / change-detector, so a
/// stale or null value simply forces a fresh walk.
fn foreground_hwnd() -> isize {
    // SAFETY: `GetForegroundWindow` takes no arguments and is callable from any
    // thread with no initialization; a null return is handled by the caller.
    unsafe { GetForegroundWindow() as isize }
}

/// Return the cached snapshot iff it is for the same foreground window and still
/// within [`SNAPSHOT_CACHE_TTL`]. Any mutating op clears the cache (see the
/// `invalidate_snapshot_cache` calls in the trait impl), so a hit means the UI has
/// not been touched by this backend since the last walk.
fn cached_snapshot_for(hwnd: isize) -> Option<Snapshot> {
    let guard = snapshot_cache().lock().unwrap_or_else(|p| p.into_inner());
    match guard.as_ref() {
        Some(c) if c.hwnd == hwnd && c.at.elapsed() < SNAPSHOT_CACHE_TTL => Some(c.snap.clone()),
        _ => None,
    }
}

/// Store a freshly walked snapshot for reuse by [`cached_snapshot_for`].
fn store_snapshot(hwnd: isize, snap: Snapshot) {
    let mut guard = snapshot_cache().lock().unwrap_or_else(|p| p.into_inner());
    *guard = Some(CachedSnapshot {
        hwnd,
        at: Instant::now(),
        snap,
    });
}

/// Drop any cached snapshot. Called before every op that can change the on-screen
/// UI so the next `snapshot()` re-reads the live window rather than serving stale
/// grounding.
fn invalidate_snapshot_cache() {
    *snapshot_cache().lock().unwrap_or_else(|p| p.into_inner()) = None;
}

/// Walk the FOREGROUND window's subtree into a [`Snapshot`].
///
/// Latency strategy (the snapshot used to blow past its watchdog on Chrome/
/// Electron windows, leaving the planner with empty grounding):
///
/// * **Foreground scope.** We resolve the focused element and climb to its
///   containing window (`window_root`), walking only that subtree — never the
///   full desktop root.
/// * **Reuse cache.** If the foreground `HWND` is unchanged and no mutating op has
///   run since the last walk (TTL-bounded), the previous snapshot is returned
///   without touching UIA at all.
/// * **Actionable-only emission + early stop.** Structural containers (Pane/Group/
///   Document) are traversed for their descendants but not emitted; the walk stops
///   as soon as `element_cap` actionable controls (== the grounding budget) are
///   collected, and never visits more than `visit_cap` nodes. This is what keeps
///   the number of cross-process COM calls (and thus the chance of hitting a slow
///   provider) small.
/// * **Probe only what we keep.** The expensive per-node state probes
///   (`is_selected`/`is_toggled`/`is_visible`/`get_value`) run only for emitted
///   controls, not for the structural nodes we merely walk through.
///
/// A future single-call UIA `CacheRequest`/`BuildUpdatedCache` (to bulk-fetch all
/// properties in one cross-process round trip) would require the `uiautomation`/
/// `windows` COM crates, which are not direct dependencies today; this keeps to the
/// `terminator` API already in use.
///
/// `desktop` is the worker's persistent session, so no per-snapshot COM/UIA
/// bring-up is paid here anymore.
fn build_snapshot(
    desktop: &Desktop,
    element_cap: usize,
    visit_cap: usize,
    max_depth: usize,
    budget: Duration,
) -> Result<Snapshot> {
    let walk_start = Instant::now();

    // Fast path: reuse the previous walk for an unchanged foreground window.
    let fg_hwnd = foreground_hwnd();
    if fg_hwnd != 0 {
        if let Some(cached) = cached_snapshot_for(fg_hwnd) {
            tracing::debug!(
                target: uia_broker::TIMING_TARGET,
                op = "snapshot",
                cache = "hit",
                nodes = cached.elements.len(),
                walk_ms = walk_start.elapsed().as_millis() as u64,
                "uia timing: foreground-window snapshot served from cache"
            );
            return Ok(cached);
        }
    }

    let deadline = walk_start + budget;

    let focused = desktop.focused_element().map_err(to_anyhow)?;
    let focused_id = focused.id();
    let root = window_root(&focused);

    // Window / app context (best-effort; never carries element values).
    let window_title = root.attributes().name.clone().unwrap_or_default();
    let app_name = probe(deadline, || focused.application())
        .flatten()
        .and_then(|a| a.attributes().name)
        .unwrap_or_else(|| window_title.clone());

    // The reported app name is often just the window title, which for some apps
    // is not the app's identity (Spotify titles its window with the playing
    // track). Fold in the foreground process's executable stem so the focus
    // guard's substring match can still recognize the app. `process_id` reuses
    // the already-resolved focused element (no extra COM round-trip).
    let app_stem = focused.process_id().ok().and_then(process_exe_stem);
    let app = app_identity(&app_name, app_stem.as_deref());

    let mut elements: Vec<UiElement> = Vec::new();
    let mut focused_path: Option<String> = None;
    let mut visited: usize = 0;

    // Iterative depth-first walk carrying the child-index path for each node.
    let mut stack: Vec<(TElement, String, usize)> = vec![(root, "#".to_string(), 0)];
    while let Some((element, path, depth)) = stack.pop() {
        // Early stop: enough actionable elements for grounding, node budget spent,
        // or wall-clock deadline reached.
        if elements.len() >= element_cap || visited >= visit_cap || Instant::now() >= deadline {
            break;
        }
        visited += 1;

        // Identity-match the focused element by its stable id (cheap, cached),
        // avoiding a per-node focus probe.
        let node_id = element.id();
        let is_focused = node_id.is_some() && node_id == focused_id;
        if is_focused && focused_path.is_none() {
            focused_path = Some(path.clone());
        }

        // `attributes()` yields the role/name in one shot; only emit (and pay the
        // full per-node state probes in `map_element`) for actionable controls and
        // the focused element. Structural chrome is still traversed below.
        let attributes = element.attributes();
        if is_focused || is_interactable_role(attributes.role.as_str()) {
            elements.push(map_element(
                deadline,
                &element,
                &attributes,
                &path,
                is_focused,
            ));
        }

        if depth < max_depth {
            if let Some(children) = probe(deadline, || element.children()) {
                // Push in reverse so the 1-based first child is popped first. Paths
                // stay 1-based over the *full* child list so `resolve` can walk them
                // back even though structural children are not emitted.
                for (index, child) in children.into_iter().enumerate().rev() {
                    let child_path = format!("{path}/{}", index + 1);
                    stack.push((child, child_path, depth + 1));
                }
            }
        }
    }

    // snapshot emitted/visited counts + duration: the headline metric for tuning
    // element_cap / visit_cap / max_depth against real windows.
    tracing::debug!(
        target: uia_broker::TIMING_TARGET,
        op = "snapshot",
        cache = "miss",
        nodes = elements.len(),
        visited,
        element_cap,
        visit_cap,
        max_depth,
        walk_ms = walk_start.elapsed().as_millis() as u64,
        hit_cap = elements.len() >= element_cap,
        "uia timing: foreground-window snapshot walk complete"
    );

    let snapshot = Snapshot {
        app,
        window_title,
        focused: focused_path,
        // Terminator resolves the focused element, not a pointer hit-test, so no
        // pointer element is reported by this backend yet.
        pointer: None,
        // Terminator exposes no text-selection accessor, so a precise selection
        // length is not available from this backend.
        selection_text_len: 0,
        elements,
    };

    if fg_hwnd != 0 {
        store_snapshot(fg_hwnd, snapshot.clone());
    }

    Ok(snapshot)
}

/// Whether a Terminator role string (a UI Automation `ControlType` name) is an
/// actionable control worth emitting into the snapshot. Structural containers
/// (Pane/Group/Document/Text/etc.) are intentionally excluded — they are walked
/// for their descendants but never spend an `element_cap` slot. Kept in sync with
/// the roles/patterns [`UiElement::is_interactive`] recognizes, so every emitted
/// element lands in grounding's interactive-first bucket.
fn is_interactable_role(role: &str) -> bool {
    matches!(
        role,
        "Button"
            | "SplitButton"
            | "MenuItem"
            | "Hyperlink"
            | "Tab"
            | "TabItem"
            | "ListItem"
            | "DataItem"
            | "CheckBox"
            | "RadioButton"
            | "ComboBox"
            | "Edit"
            | "Slider"
            | "Spinner"
            | "TreeItem"
    )
}

/// Map a single Terminator element to our [`UiElement`], guarding every probe.
fn map_element(
    deadline: Instant,
    element: &TElement,
    attributes: &UIElementAttributes,
    path: &str,
    is_focused: bool,
) -> UiElement {
    let role_str = attributes.role.as_str();
    let role = map_role(role_str);

    let name = attributes.name.clone().filter(|s| !s.is_empty());
    let description = attributes.description.clone().filter(|s| !s.is_empty());

    let bounds = attributes
        .bounds
        .map(to_bounds)
        .or_else(|| probe(deadline, || element.bounds()).map(to_bounds));

    // State probes, each guarded; results are reused for pattern detection.
    let enabled = attributes
        .enabled
        .or_else(|| probe(deadline, || element.is_enabled()));
    let selected = probe(deadline, || element.is_selected());
    let toggled = probe(deadline, || element.is_toggled());
    let visible = probe(deadline, || element.is_visible());

    let mut states = Vec::new();
    if is_focused {
        states.push(ElementState::Focused);
    }
    match enabled {
        Some(true) => states.push(ElementState::Enabled),
        Some(false) => states.push(ElementState::Disabled),
        None => {}
    }
    if selected == Some(true) {
        states.push(ElementState::Selected);
    }
    if visible == Some(false) {
        states.push(ElementState::Offscreen);
    }

    let value_len = value_length(deadline, element, attributes, role);

    // Pattern detection. Terminator's snapshot attributes do not enumerate the
    // supported UI Automation control patterns, so this mixes solid probe
    // signals (Toggle/SelectionItem/Value) with role-based heuristics
    // (Invoke/ExpandCollapse/Scroll).
    let mut patterns = Vec::new();
    if role_supports_invoke(role_str) {
        patterns.push(ActionPattern::Invoke);
    }
    if value_len > 0 || matches!(role, Role::TextField | Role::ComboBox | Role::Document) {
        patterns.push(ActionPattern::SetValue);
    }
    if toggled.is_some() {
        patterns.push(ActionPattern::Toggle);
    }
    if selected.is_some() {
        patterns.push(ActionPattern::Select);
    }
    if matches!(role_str, "ComboBox" | "TreeItem") {
        patterns.push(ActionPattern::Expand);
    }
    if matches!(
        role_str,
        "List" | "Tree" | "Document" | "Pane" | "Table" | "DataGrid"
    ) {
        patterns.push(ActionPattern::Scroll);
    }

    UiElement {
        path: path.to_string(),
        role,
        name: name.unwrap_or_default(),
        description: description.unwrap_or_default(),
        value_len,
        states,
        bounds,
        patterns,
    }
}

/// Convert Terminator's `(x, y, width, height)` f64 tuple to our [`Bounds`].
fn to_bounds(bounds: (f64, f64, f64, f64)) -> Bounds {
    let (x, y, width, height) = bounds;
    Bounds {
        x: x as i32,
        y: y as i32,
        w: width as i32,
        h: height as i32,
    }
}

/// Length (in `char`s) of a control's value. The value string is never retained.
fn value_length(
    deadline: Instant,
    element: &TElement,
    attributes: &UIElementAttributes,
    role: Role,
) -> usize {
    if let Some(value) = &attributes.value {
        return value.chars().count();
    }
    if matches!(role, Role::TextField | Role::ComboBox | Role::Document) {
        if let Some(Some(value)) = probe(deadline, || element.get_value()) {
            // `value` is measured and dropped at the end of this scope.
            return value.chars().count();
        }
    }
    0
}

/// Map a Terminator role string (a UI Automation control-type name) onto our
/// normalized [`Role`].
fn map_role(role: &str) -> Role {
    match role {
        "Button" | "SplitButton" => Role::Button,
        "CheckBox" => Role::CheckBox,
        "ComboBox" => Role::ComboBox,
        "Edit" => Role::TextField,
        "Hyperlink" => Role::Link,
        "Image" => Role::Image,
        "List" | "DataGrid" => Role::List,
        "ListItem" | "DataItem" => Role::ListItem,
        "Menu" => Role::Menu,
        "MenuBar" => Role::MenuBar,
        "MenuItem" => Role::MenuItem,
        "ProgressBar" => Role::ProgressBar,
        "RadioButton" => Role::RadioButton,
        "ScrollBar" => Role::ScrollBar,
        "Slider" => Role::Slider,
        "Spinner" => Role::Spinner,
        "Tab" => Role::Tab,
        "TabItem" => Role::TabItem,
        "Text" => Role::Text,
        "ToolBar" => Role::Toolbar,
        "Tree" => Role::Tree,
        "TreeItem" => Role::TreeItem,
        "Group" => Role::Group,
        "Table" => Role::Table,
        "TitleBar" => Role::TitleBar,
        "Window" => Role::Window,
        "Pane" => Role::Pane,
        "Document" => Role::Document,
        "Separator" => Role::Separator,
        _ => Role::Unknown,
    }
}

/// Roles that are typically activatable via the Invoke pattern.
fn role_supports_invoke(role: &str) -> bool {
    matches!(
        role,
        "Button"
            | "SplitButton"
            | "MenuItem"
            | "Hyperlink"
            | "TabItem"
            | "ListItem"
            | "CheckBox"
            | "RadioButton"
    )
}

/// Resolve the containing window (or application) of the focused element,
/// falling back to the focused element itself.
fn window_root(focused: &TElement) -> TElement {
    if let Ok(Some(window)) = focused.window() {
        return window;
    }
    if let Ok(Some(application)) = focused.application() {
        return application;
    }
    focused.clone()
}

/// Resolve a `#/1/4/2`-style path back to a live element under the focused
/// window.
fn resolve(desktop: &Desktop, path: &str) -> Result<TElement> {
    let resolve_start = Instant::now();
    let focused = desktop.focused_element().map_err(to_anyhow)?;
    let mut current = window_root(&focused);

    let mut parts = path.split('/');
    match parts.next() {
        Some("#") => {}
        _ => return Err(anyhow!("invalid path root in '{path}'")),
    }

    let mut depth = 0usize;
    for token in parts {
        if token.is_empty() {
            continue;
        }
        let index: usize = token
            .parse()
            .map_err(|_| anyhow!("invalid path segment '{token}' in '{path}'"))?;
        if index == 0 {
            return Err(anyhow!("path segments are 1-based, got 0 in '{path}'"));
        }
        let children = current.children().map_err(to_anyhow)?;
        current = children
            .into_iter()
            .nth(index - 1)
            .ok_or_else(|| anyhow!("child index {index} out of range in '{path}'"))?;
        depth += 1;
    }

    // element-resolve: time to walk from the foreground window down to the target
    // element. Retained on the worker; the element itself never leaves the thread.
    tracing::debug!(
        target: uia_broker::TIMING_TARGET,
        op = "element-resolve",
        depth,
        resolve_ms = resolve_start.elapsed().as_millis() as u64,
        "uia timing: element-resolve"
    );

    Ok(current)
}

/// Open a non-web URI (`shell:`, `ms-settings:`, `file:`, `mailto:`, …) through
/// the shell's registered handler and return immediately.
///
/// Unlike terminator's `open_url`, this performs no post-launch "find the browser
/// window" search — so a folder or settings URI, which opens no browser at all,
/// hands off to Explorer / the Settings app and returns at once instead of
/// spinning to the op watchdog.
fn shell_open(uri: &str) -> Result<()> {
    // NUL-terminated UTF-16 for the ShellExecuteW wide-string parameters.
    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    let file: Vec<u16> = uri.encode_utf16().chain(std::iter::once(0)).collect();

    // SAFETY: `verb` and `file` are valid NUL-terminated UTF-16 buffers that
    // outlive the call; ShellExecuteW borrows them only for its duration and
    // retains nothing. hwnd / parameters / directory are null (no parent window,
    // no arguments, default working directory). This runs on the UIA worker,
    // which is COM (STA) initialized — the apartment ShellExecuteW expects.
    let hinstance = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    // ShellExecuteW returns a pseudo-HINSTANCE that is > 32 on success; any value
    // <= 32 is an error code (e.g. 2 = no association / file not found).
    if (hinstance as isize) <= 32 {
        return Err(anyhow!(
            "ShellExecuteW failed to open '{uri}' (code {})",
            hinstance as isize
        ));
    }
    Ok(())
}

/// Translate a combination such as `ctrl+c` or `meta+Enter` into the `send_keys`
/// grammar Terminator's `press_key` forwards to: modifiers are held over the
/// primary key, for example `{CTRL}c` or `{WIN}{ENTER}`.
fn translate_combo(combo: &str) -> Result<String> {
    let mut out = String::new();
    let mut primary: Option<String> = None;
    let mut modifiers = 0usize;

    for raw in combo.split('+') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => {
                out.push_str("{CTRL}");
                modifiers += 1;
            }
            "shift" => {
                out.push_str("{SHIFT}");
                modifiers += 1;
            }
            "alt" | "option" => {
                out.push_str("{ALT}");
                modifiers += 1;
            }
            "meta" | "super" | "win" | "windows" | "cmd" | "command" => {
                out.push_str("{WIN}");
                modifiers += 1;
            }
            _ => primary = Some(token.to_string()),
        }
    }

    let Some(key) = primary else {
        // A lone modifier tap — exactly one modifier and no primary key (e.g. `win`
        // to open Start, or `alt`) — has nothing to be held over, so the single
        // modifier token (`{WIN}`, `{ALT}`, …) IS the keypress. Send it as-is
        // instead of rejecting it. Two-or-more modifiers with no key (`ctrl+alt`)
        // stay an error: there's no meaningful "tap" for a bare chord. (B4)
        if modifiers == 1 {
            return Ok(out);
        }
        return Err(anyhow!("no primary key in combo '{combo}'"));
    };
    out.push_str(&key_token(&key));
    Ok(out)
}

/// Render a single primary key token in `send_keys` grammar. Named keys become
/// brace tokens; single characters pass through (with the grammar's reserved
/// characters escaped).
fn key_token(key: &str) -> String {
    let named = match key.to_ascii_lowercase().as_str() {
        "enter" | "return" => Some("{ENTER}"),
        "tab" => Some("{TAB}"),
        "space" => Some("{SPACE}"),
        "esc" | "escape" => Some("{ESC}"),
        "backspace" => Some("{BACKSPACE}"),
        "delete" | "del" => Some("{DELETE}"),
        "home" => Some("{HOME}"),
        "end" => Some("{END}"),
        "pageup" | "pgup" => Some("{PAGEUP}"),
        "pagedown" | "pgdn" => Some("{PAGEDOWN}"),
        "up" => Some("{UP}"),
        "down" => Some("{DOWN}"),
        "left" => Some("{LEFT}"),
        "right" => Some("{RIGHT}"),
        "insert" | "ins" => Some("{INSERT}"),
        "f1" => Some("{F1}"),
        "f2" => Some("{F2}"),
        "f3" => Some("{F3}"),
        "f4" => Some("{F4}"),
        "f5" => Some("{F5}"),
        "f6" => Some("{F6}"),
        "f7" => Some("{F7}"),
        "f8" => Some("{F8}"),
        "f9" => Some("{F9}"),
        "f10" => Some("{F10}"),
        "f11" => Some("{F11}"),
        "f12" => Some("{F12}"),
        _ => None,
    };
    if let Some(token) = named {
        return token.to_string();
    }

    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        // A single character: escape the grammar's reserved characters.
        (Some(c), None) => match c {
            '{' => "{{}".to_string(),
            '}' => "{}}".to_string(),
            '(' => "{(}".to_string(),
            ')' => "{)}".to_string(),
            other => other.to_string(),
        },
        // An unrecognized multi-character token: wrap it so `send_keys` can try
        // to resolve it as a named key (for example `PRINT`, `PAUSE`).
        _ => format!("{{{}}}", key.to_uppercase()),
    }
}

/// Combine the reported window/app title with the foreground process's
/// executable `stem` into a single app-identity string. The stem is appended
/// only when the title does not already contain it (case-insensitively), so a
/// title like "Google Chrome" with stem `chrome` is returned unchanged while a
/// track title with stem `spotify` becomes `"<track> (spotify)"`. With no stem
/// the title is returned verbatim.
///
/// Pure string logic (no Win32) so it is unit-testable without an OS.
fn app_identity(title: &str, stem: Option<&str>) -> String {
    match stem {
        Some(stem) if !title.to_lowercase().contains(stem) => format!("{title} ({stem})"),
        _ => title.to_string(),
    }
}

/// The executable stem (basename without a trailing ".exe", lowercased, e.g.
/// `spotify`) of the process identified by `pid`. Any failure — a zero PID, a
/// null handle, a failed query, or an empty result — resolves to `None`.
fn process_exe_stem(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }

        // Flag 0 requests the Win32 path form; `len` is in/out (buffer capacity
        // on entry, characters written excluding the NUL on success).
        let mut buffer = [0u16; 260];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut len);
        CloseHandle(handle);

        if ok == 0 || len == 0 {
            return None;
        }

        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        let name = path
            .rsplit('\\')
            .next()
            .unwrap_or(path.as_str())
            .to_lowercase();
        let stem = name.strip_suffix(".exe").unwrap_or(&name);
        if stem.is_empty() {
            return None;
        }
        Some(stem.to_string())
    }
}

/// Extra activate-and-settle attempts after the first foreground check, so the
/// guard tries up to `FOREGROUND_RETRIES` times to pull the intended app forward.
const FOREGROUND_RETRIES: u32 = 2;
/// Bounded wait for focus to settle after asking the OS to bring a window forward,
/// before re-reading the foreground.
const FOREGROUND_SETTLE: Duration = Duration::from_millis(300);

/// Read the current foreground window's `(title, process-exe-stem)` via Win32.
/// Both are best-effort: an empty value means "unknown", which the pure
/// [`should_refocus`] treats as non-excluded and non-matching. The window title
/// feeds the app side of the decision; the process stem (via the existing
/// [`process_exe_stem`] helper) is the reliable identity that recognizes a console
/// or our own exe.
fn foreground_identity() -> (String, String) {
    // SAFETY: `GetForegroundWindow` takes no arguments and is callable from any
    // thread; a null HWND yields an empty identity handled by the caller.
    // `GetWindowTextW` / `GetWindowThreadProcessId` borrow the HWND and the
    // out-buffers only for the duration of the call and retain nothing; the title
    // slice is bounded by the returned length, and the PID is read into a local.
    let (title, pid) = unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return (String::new(), String::new());
        }
        let mut buffer = [0u16; 512];
        let len = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        let title = if len > 0 {
            String::from_utf16_lossy(&buffer[..len as usize])
        } else {
            String::new()
        };
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        (title, pid)
    };
    // `process_exe_stem` opens/closes its own process handle internally.
    let proc = process_exe_stem(pid).unwrap_or_default();
    (title, proc)
}

/// Foreground-lock implementation (see [`AccessibilityBackend::ensure_foreground`]).
///
/// Reads the live foreground via Win32, runs it through the pure [`should_refocus`]
/// decision, and — when the front window is our dev console / own exe or simply the
/// wrong app — asks the OS to bring `hint` forward, re-checking up to
/// [`FOREGROUND_RETRIES`] times. Returns whether a legitimate target is foreground
/// at the end.
///
/// The bring-forward uses terminator's `activate_application` (the same primitive
/// [`AccessibilityBackend::focus_app`] uses) rather than a bare `SetForegroundWindow`:
/// under Windows' foreground-lock rules a raw `SetForegroundWindow` from a
/// background process is frequently ignored (it only flashes the taskbar), whereas
/// `activate_application` restores + raises the target window reliably.
fn ensure_foreground_win(desktop: &Desktop, hint: Option<&str>) -> bool {
    let (app, proc) = foreground_identity();
    if !should_refocus(&app, &proc, hint) {
        // A legitimate target (or an acceptable current window) is already in front.
        return true;
    }

    // The foreground is our console/exe or the wrong app. Without an intended
    // target there is no window to switch to, so report that we could not secure a
    // valid foreground and let the caller log it.
    let Some(target) = hint else {
        tracing::warn!(
            foreground_app = %app,
            foreground_proc = %proc,
            "act foreground guard: console/own window is foreground with no target to switch to"
        );
        return false;
    };

    for attempt in 1..=FOREGROUND_RETRIES {
        let _ = desktop.activate_application(target);
        std::thread::sleep(FOREGROUND_SETTLE);
        let (app, proc) = foreground_identity();
        if !should_refocus(&app, &proc, hint) {
            return true;
        }
        tracing::warn!(
            target,
            attempt,
            foreground_app = %app,
            foreground_proc = %proc,
            "act foreground guard: target still not foreground after activate"
        );
    }
    false
}

/// Whether the focused application's process runs at a higher integrity level
/// than this process. Any failure resolves to `false`.
///
/// Uses the worker's persistent `desktop` (COM affinity) to read the foreground
/// PID; the token/SID integrity comparison below is plain Win32 and
/// thread-agnostic.
fn foreground_is_elevated(desktop: &Desktop) -> bool {
    let pid = match desktop
        .focused_element()
        .and_then(|element| element.process_id())
    {
        Ok(pid) if pid > 0 => pid,
        _ => return false,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let foreground_rid = process_integrity_rid(handle);
        CloseHandle(handle);

        // GetCurrentProcess returns a pseudo-handle that must not be closed.
        let our_rid = process_integrity_rid(GetCurrentProcess());

        matches!((foreground_rid, our_rid), (Some(f), Some(o)) if f > o)
    }
}

/// Read a process token's mandatory integrity level RID.
///
/// # Safety
///
/// `process` must be a valid process handle opened with at least
/// `PROCESS_QUERY_LIMITED_INFORMATION` access, or a process pseudo-handle.
unsafe fn process_integrity_rid(process: HANDLE) -> Option<u32> {
    let mut token: HANDLE = ptr::null_mut();
    if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
        return None;
    }

    // First call sizes the buffer; it is expected to fail with the length set.
    let mut needed: u32 = 0;
    GetTokenInformation(token, TokenIntegrityLevel, ptr::null_mut(), 0, &mut needed);
    if needed == 0 {
        CloseHandle(token);
        return None;
    }

    let mut buffer = vec![0u8; needed as usize];
    let ok = GetTokenInformation(
        token,
        TokenIntegrityLevel,
        buffer.as_mut_ptr() as *mut c_void,
        needed,
        &mut needed,
    );
    CloseHandle(token);
    if ok == 0 {
        return None;
    }

    let label = &*(buffer.as_ptr() as *const TOKEN_MANDATORY_LABEL);
    let sid = label.Label.Sid;
    if sid.is_null() {
        return None;
    }

    let count_ptr = GetSidSubAuthorityCount(sid);
    if count_ptr.is_null() {
        return None;
    }
    let count = *count_ptr;
    if count == 0 {
        return None;
    }

    let rid_ptr = GetSidSubAuthority(sid, (count - 1) as u32);
    if rid_ptr.is_null() {
        return None;
    }
    Some(*rid_ptr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_simple_combo() {
        assert_eq!(translate_combo("ctrl+c").unwrap(), "{CTRL}c");
    }

    #[test]
    fn translates_meta_enter() {
        assert_eq!(translate_combo("meta+Enter").unwrap(), "{WIN}{ENTER}");
    }

    #[test]
    fn app_identity_appends_stem_when_title_lacks_it() {
        // Spotify's window title is the playing track, not the app name.
        let app = app_identity("Red Hot Chili Peppers - Can't Stop", Some("spotify"));
        assert!(
            app.to_lowercase().contains("spotify"),
            "expected stem folded in, got {app:?}"
        );
    }

    #[test]
    fn app_identity_does_not_double_append_present_stem() {
        // "Google Chrome" already contains "chrome" — leave it unchanged.
        assert_eq!(
            app_identity("Google Chrome", Some("chrome")),
            "Google Chrome"
        );
    }

    #[test]
    fn app_identity_without_stem_returns_title() {
        assert_eq!(app_identity("Notepad", None), "Notepad");
    }

    #[test]
    fn translates_multiple_modifiers() {
        assert_eq!(
            translate_combo("ctrl+shift+Tab").unwrap(),
            "{CTRL}{SHIFT}{TAB}"
        );
    }

    #[test]
    fn rejects_modifier_only_combo() {
        // Two-plus modifiers with no primary key is still an error (no meaningful tap).
        assert!(translate_combo("ctrl+alt").is_err());
    }

    #[test]
    fn translates_lone_modifier_tap() {
        // A single lone modifier (e.g. the Windows key to open Start) taps that key.
        assert_eq!(translate_combo("win").unwrap(), "{WIN}");
        assert_eq!(translate_combo("alt").unwrap(), "{ALT}");
    }

    #[test]
    fn maps_common_roles() {
        assert_eq!(map_role("Edit"), Role::TextField);
        assert_eq!(map_role("Hyperlink"), Role::Link);
        assert_eq!(map_role("ToolBar"), Role::Toolbar);
        assert_eq!(map_role("SomethingElse"), Role::Unknown);
    }

    #[test]
    fn backend_name_is_stable() {
        assert_eq!(WindowsUiaBackend::new().name(), "windows-terminator");
    }

    #[test]
    fn interactable_roles_are_actionable_not_structural() {
        // Actionable controls are emitted; structural containers are only walked.
        for role in ["Button", "Edit", "Hyperlink", "MenuItem", "CheckBox", "Tab"] {
            assert!(is_interactable_role(role), "{role} should be interactable");
        }
        for role in ["Pane", "Group", "Document", "Text", "TitleBar", "Window"] {
            assert!(
                !is_interactable_role(role),
                "{role} should be structural-only"
            );
        }
    }

    #[test]
    fn element_cap_tracks_grounding_budget() {
        // The walk must not stop before it has produced everything grounding shows.
        assert_eq!(
            DEFAULT_ELEMENT_CAP,
            super::super::grounding_packet::DEFAULT_MAX_ELEMENTS
        );
    }

    /// Every `key` step in the built-in seed drawer must translate to a valid
    /// Windows send-keys sequence — validated here against the REAL translator on
    /// the Windows CI runner, so a shipped seed can never carry an unpressable
    /// combo. (Runs only on Windows, where this module compiles.)
    #[test]
    fn every_seed_key_combo_translates_on_windows() {
        for flow in crate::act::seed::builtin_flows() {
            for step in &flow.steps {
                if step.action == "key" {
                    let combo = step.value.as_deref().unwrap_or_default();
                    let translated = translate_combo(combo);
                    assert!(
                        translated.is_ok(),
                        "seed {} key combo {:?} is not translatable: {:?}",
                        flow.id,
                        combo,
                        translated.err()
                    );
                }
            }
        }
    }
}
