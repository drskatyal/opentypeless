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
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use async_trait::async_trait;

use terminator::{AutomationError, Desktop, UIElement as TElement, UIElementAttributes};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::error::AppError;

use super::backend::{AccessibilityBackend, ShellOutput};
use super::element::{ActionPattern, Bounds, ElementPath, ElementState, Role, Snapshot, UiElement};
use super::uia_broker;

/// Map an internal `anyhow` error to the crate's `AppError` at the trait boundary.
fn to_app_err(err: anyhow::Error) -> AppError {
    AppError::Config(err.to_string())
}

/// Maximum number of nodes captured in a single snapshot.
const DEFAULT_NODE_CAP: usize = 200;
/// Maximum subtree depth walked below the focused window.
const DEFAULT_MAX_DEPTH: usize = 16;
/// Cooperative wall-clock budget for a full snapshot walk.
const DEFAULT_SNAPSHOT_BUDGET: Duration = Duration::from_secs(4);

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
    node_cap: usize,
    max_depth: usize,
    snapshot_budget: Duration,
}

impl Default for WindowsUiaBackend {
    fn default() -> Self {
        Self {
            node_cap: DEFAULT_NODE_CAP,
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
        let cap = self.node_cap;
        let max_depth = self.max_depth;
        let budget = self.snapshot_budget;

        // The build itself is cooperatively bounded by `budget`; the broker
        // watchdog adds a small margin so a wedged COM call still returns control
        // to the caller (leaving the stuck worker to be recreated).
        run_op(budget + WATCHDOG_MARGIN, "snapshot", move |desktop| {
            build_snapshot(desktop, cap, max_depth, budget)
        })
        .await
        .map_err(to_app_err)
    }

    async fn focus(&self, path: &ElementPath) -> std::result::Result<(), AppError> {
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
        let text = text.to_string();
        run_op(OP_WATCHDOG, "type_text", move |desktop| {
            let focused = desktop.focused_element().map_err(to_anyhow)?;
            focused.type_text(&text, false).map_err(to_anyhow)
        })
        .await
        .map_err(to_app_err)
    }

    async fn key_combo(&self, combo: &str) -> std::result::Result<(), AppError> {
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
        let target = target.to_string();
        run_op(OP_WATCHDOG, "launch", move |desktop| {
            desktop.open_application(&target).map_err(to_anyhow)?;
            Ok(())
        })
        .await
        .map_err(to_app_err)
    }

    async fn open_uri(&self, uri: &str) -> std::result::Result<(), AppError> {
        let uri = uri.to_string();
        run_op(OP_WATCHDOG, "open_uri", move |desktop| {
            desktop.open_url(&uri, None).map_err(to_anyhow)?;
            Ok(())
        })
        .await
        .map_err(to_app_err)
    }

    async fn run_shell(
        &self,
        command: &str,
        shell: &str,
    ) -> std::result::Result<ShellOutput, AppError> {
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
        let name = name.to_string();
        run_op(OP_WATCHDOG, "focus_app", move |desktop| {
            Ok(desktop.activate_application(&name).is_ok())
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

    fn name(&self) -> &str {
        "windows-terminator"
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

/// Walk the FOREGROUND window's subtree into a [`Snapshot`].
///
/// Scoping: we resolve the focused element and climb to its containing window
/// (`window_root`), then walk only that subtree bounded by `cap` / `max_depth`.
/// This deliberately avoids a full-desktop walk (every top-level window of every
/// process) — the foreground window is the only thing the user can act on. If a
/// future Terminator API exposes the active/foreground window directly (e.g. a
/// `Desktop`-level accessor), it could replace the `focused_element().window()`
/// climb below; until then this is the narrowest scope the used API supports.
///
/// `desktop` is the worker's persistent session, so no per-snapshot COM/UIA
/// bring-up is paid here anymore.
fn build_snapshot(
    desktop: &Desktop,
    cap: usize,
    max_depth: usize,
    budget: Duration,
) -> Result<Snapshot> {
    let walk_start = Instant::now();
    let deadline = walk_start + budget;

    let focused = desktop.focused_element().map_err(to_anyhow)?;
    let focused_id = focused.id();
    let root = window_root(&focused);

    // Window / app context (best-effort; never carries element values).
    let window_title = root.attributes().name.clone().unwrap_or_default();
    let app = probe(deadline, || focused.application())
        .flatten()
        .and_then(|a| a.attributes().name)
        .unwrap_or_else(|| window_title.clone());

    let mut elements: Vec<UiElement> = Vec::new();
    let mut focused_path: Option<String> = None;

    // Iterative depth-first walk carrying the child-index path for each node.
    let mut stack: Vec<(TElement, String, usize)> = vec![(root, "#".to_string(), 0)];
    while let Some((element, path, depth)) = stack.pop() {
        if elements.len() >= cap || Instant::now() >= deadline {
            break;
        }

        // Identity-match the focused element by its stable id (cheap, cached),
        // avoiding a per-node focus probe.
        let node_id = element.id();
        let is_focused = node_id.is_some() && node_id == focused_id;
        if is_focused && focused_path.is_none() {
            focused_path = Some(path.clone());
        }

        let attributes = element.attributes();
        elements.push(map_element(
            deadline,
            &element,
            &attributes,
            &path,
            is_focused,
        ));

        if depth < max_depth {
            if let Some(children) = probe(deadline, || element.children()) {
                // Push in reverse so the 1-based first child is popped first.
                for (index, child) in children.into_iter().enumerate().rev() {
                    let child_path = format!("{path}/{}", index + 1);
                    stack.push((child, child_path, depth + 1));
                }
            }
        }
    }

    // snapshot node count + duration: the headline metric for tuning node_cap /
    // max_depth against real windows.
    tracing::debug!(
        target: uia_broker::TIMING_TARGET,
        op = "snapshot",
        nodes = elements.len(),
        node_cap = cap,
        max_depth,
        walk_ms = walk_start.elapsed().as_millis() as u64,
        hit_cap = elements.len() >= cap,
        "uia timing: foreground-window snapshot walk complete"
    );

    Ok(Snapshot {
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
    })
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

/// Translate a combination such as `ctrl+c` or `meta+Enter` into the `send_keys`
/// grammar Terminator's `press_key` forwards to: modifiers are held over the
/// primary key, for example `{CTRL}c` or `{WIN}{ENTER}`.
fn translate_combo(combo: &str) -> Result<String> {
    let mut out = String::new();
    let mut primary: Option<String> = None;

    for raw in combo.split('+') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => out.push_str("{CTRL}"),
            "shift" => out.push_str("{SHIFT}"),
            "alt" | "option" => out.push_str("{ALT}"),
            "meta" | "super" | "win" | "windows" | "cmd" | "command" => out.push_str("{WIN}"),
            _ => primary = Some(token.to_string()),
        }
    }

    let key = primary.ok_or_else(|| anyhow!("no primary key in combo '{combo}'"))?;
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
    fn translates_multiple_modifiers() {
        assert_eq!(
            translate_combo("ctrl+shift+Tab").unwrap(),
            "{CTRL}{SHIFT}{TAB}"
        );
    }

    #[test]
    fn rejects_modifier_only_combo() {
        assert!(translate_combo("ctrl+alt").is_err());
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
