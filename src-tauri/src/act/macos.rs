//! macOS accessibility + input backend for "Act mode", built on the native
//! Accessibility (AX) API, Core Graphics event synthesis, and AppKit.
//!
//! Compiled only on macOS (`#[cfg(target_os = "macos")]`, applied where the
//! module is declared in `super`). It mirrors the STRUCTURE and contracts of the
//! Windows backend (`super::windows`) — same `AccessibilityBackend` trait, same
//! `#/index/index` element-path scheme, same PHI discipline (element values are
//! measured, never retained) — but reads the tree via `AXUIElement*` and drives
//! input via `CGEvent`.
//!
//! Design notes:
//!
//! * **Thread model / Send-safety.** `AXUIElementRef` and `CGEvent` are raw
//!   pointers / non-`Send` CF objects, so every op does all of its native work
//!   inside a `tokio::task::spawn_blocking` closure that returns only plain owned
//!   data (`Snapshot`, `ShellOutput`, `String`, `Vec<u8>`). No native handle ever
//!   crosses an `.await`, so the `async_trait` futures stay `Send`. This mirrors
//!   the "native handles never leave one thread" invariant the Windows backend
//!   gets from its UIA worker.
//! * **PHI safety.** A control's text value is read only to take its `char`
//!   count (`value_len`) and is dropped immediately; it is never stored in the
//!   snapshot. Secure text fields (`AXSecureTextField`) are never read at all.
//! * **Cooperative bound.** The walk is bounded by an element cap (== the
//!   grounding budget), a visited-node cap, a max depth, and a wall-clock
//!   deadline, exactly like the Windows walk. A single wedged AX provider call
//!   cannot be preempted cooperatively, so `snapshot()` additionally wraps the
//!   blocking walk in a `tokio::time::timeout` that returns control to the caller
//!   even if a synchronous AX call is still stuck (the blocking thread unwinds on
//!   its own).
//!
//! ## macOS permissions (TCC)
//!
//! Reading the AX tree and posting synthetic input BOTH require the
//! **Accessibility** grant (System Settings › Privacy & Security ›
//! Accessibility) for the OpenTypeless app / the terminal hosting it. Without it,
//! `AXIsProcessTrusted()` is false and `snapshot()` returns a clear error, and
//! `CGEvent` posts are silently dropped by the OS. `capture_screen()`
//! additionally requires the **Screen Recording** grant. These are per-app user
//! grants that cannot be assumed programmatically.

use std::ffi::CString;
use std::io::Read;
use std::os::raw::{c_char, c_void};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use async_trait::async_trait;

use accessibility_sys::{
    kAXErrorSuccess, kAXValueTypeCGPoint, kAXValueTypeCGSize, AXIsProcessTrusted,
    AXUIElementCopyAttributeValue, AXUIElementCreateApplication, AXUIElementPerformAction,
    AXUIElementRef, AXUIElementSetAttributeValue, AXValueGetValue, AXValueRef,
};

use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
use core_foundation_sys::base::{CFEqual, CFGetTypeID, CFRelease, CFRetain, CFTypeRef};
use core_foundation_sys::data::{CFDataGetBytePtr, CFDataGetLength};
use core_foundation_sys::number::{
    CFBooleanGetTypeID, CFBooleanGetValue, CFBooleanRef, CFNumberGetTypeID, CFNumberRef,
};
use core_foundation_sys::string::{CFStringGetTypeID, CFStringRef};

use core_graphics::display::CGDisplay;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::{CGPoint, CGSize};

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};

use crate::error::AppError;

use super::backend::{AccessibilityBackend, ShellOutput};
use super::element::{ActionPattern, Bounds, ElementPath, ElementState, Role, Snapshot, UiElement};

/// Max emitted (actionable) elements — the walk early-stops here. Kept in
/// lock-step with the grounding budget so we never build elements grounding
/// would discard (same rationale as the Windows backend).
const DEFAULT_ELEMENT_CAP: usize = super::grounding_packet::DEFAULT_MAX_ELEMENTS;
/// Hard ceiling on nodes traversed during a single walk.
const DEFAULT_VISIT_CAP: usize = 600;
/// Maximum subtree depth walked below the focused window.
const DEFAULT_MAX_DEPTH: usize = 12;
/// Cooperative wall-clock budget for a healthy snapshot walk.
const DEFAULT_SNAPSHOT_BUDGET: Duration = Duration::from_secs(3);
/// Extra margin on top of the walk budget before the outer `tokio::time::timeout`
/// abandons a wedged blocking walk.
const WATCHDOG_MARGIN: Duration = Duration::from_secs(3);
/// Wall-clock timeout for a `run_shell` child command.
const SHELL_TIMEOUT: Duration = Duration::from_secs(15);
/// Max captured shell output, in `char`s.
const SHELL_MAX_OUTPUT_CHARS: usize = 8192;

/// Native macOS implementation of [`AccessibilityBackend`].
#[derive(Debug, Clone)]
pub struct MacBackend {
    element_cap: usize,
    visit_cap: usize,
    max_depth: usize,
    snapshot_budget: Duration,
}

impl Default for MacBackend {
    fn default() -> Self {
        Self {
            element_cap: DEFAULT_ELEMENT_CAP,
            visit_cap: DEFAULT_VISIT_CAP,
            max_depth: DEFAULT_MAX_DEPTH,
            snapshot_budget: DEFAULT_SNAPSHOT_BUDGET,
        }
    }
}

impl MacBackend {
    /// Create a backend with default limits.
    pub fn new() -> Self {
        Self::default()
    }
}

/// Run a blocking closure on the tokio blocking pool and flatten the join error.
///
/// Every native op funnels through here so all `AXUIElementRef` / `CGEvent` work
/// happens on one blocking thread and returns only `Send` owned data.
async fn run_blocking<T, F>(f: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Config(format!("act macos task join error: {e}")))?
}

fn cfg_err(msg: impl Into<String>) -> AppError {
    AppError::Config(msg.into())
}

#[async_trait]
impl AccessibilityBackend for MacBackend {
    async fn snapshot(&self) -> Result<Snapshot, AppError> {
        let (element_cap, visit_cap, max_depth, budget) = (
            self.element_cap,
            self.visit_cap,
            self.max_depth,
            self.snapshot_budget,
        );
        // The walk is cooperatively bounded by `budget`; the outer timeout adds a
        // margin so a wedged synchronous AX call still returns control to the
        // caller (the stuck blocking thread is left to unwind on its own).
        let handle = tokio::task::spawn_blocking(move || {
            build_snapshot(element_cap, visit_cap, max_depth, budget)
        });
        match tokio::time::timeout(budget + WATCHDOG_MARGIN, handle).await {
            Ok(joined) => joined.map_err(|e| cfg_err(format!("act macos snapshot join: {e}")))?,
            Err(_) => Err(cfg_err("act macos snapshot timed out")),
        }
    }

    async fn focused_app_is_elevated(&self) -> Result<bool, AppError> {
        // macOS has no integrity-level analogue we detect-and-decline on; the AX
        // grant is the real gate (handled in `snapshot`). Never elevated here.
        Ok(false)
    }

    async fn focus(&self, target: &ElementPath) -> Result<(), AppError> {
        let path = target.clone();
        run_blocking(move || {
            let el = resolve(&path)?;
            if set_focused(el.as_ref())
                || perform(el.as_ref(), "AXRaise")
                || perform(el.as_ref(), "AXPress")
            {
                return Ok(());
            }
            Err(cfg_err(format!("could not focus element '{path}'")))
        })
        .await
    }

    async fn invoke(&self, target: &ElementPath) -> Result<(), AppError> {
        let path = target.clone();
        run_blocking(move || {
            let el = resolve(&path)?;
            if perform(el.as_ref(), "AXPress")
                || perform(el.as_ref(), "AXConfirm")
                || perform(el.as_ref(), "AXPick")
            {
                return Ok(());
            }
            Err(cfg_err(format!("could not invoke element '{path}'")))
        })
        .await
    }

    async fn set_value(&self, target: &ElementPath, value: &str) -> Result<(), AppError> {
        let path = target.clone();
        let value = value.to_string();
        run_blocking(move || {
            let el = resolve(&path)?;
            if set_ax_value(el.as_ref(), &value) {
                return Ok(());
            }
            // No settable AXValue: focus the control and type the text instead.
            let _ = set_focused(el.as_ref());
            synth_text(&value)
        })
        .await
    }

    async fn type_text(&self, text: &str) -> Result<(), AppError> {
        let text = text.to_string();
        run_blocking(move || synth_text(&text)).await
    }

    async fn key_combo(&self, combo: &str) -> Result<(), AppError> {
        let combo = combo.to_string();
        run_blocking(move || {
            let (flags, keycode) = parse_combo(&combo)?;
            synth_key(flags, keycode)
        })
        .await
    }

    async fn launch(&self, target: &str) -> Result<(), AppError> {
        let target = target.to_string();
        run_blocking(move || launch_app(&target)).await
    }

    async fn open_uri(&self, uri: &str) -> Result<(), AppError> {
        let uri = uri.to_string();
        run_blocking(move || open_uri_mac(&uri)).await
    }

    async fn run_shell(&self, command: &str, shell: &str) -> Result<ShellOutput, AppError> {
        let command = command.to_string();
        let shell = shell.to_string();
        run_blocking(move || run_shell_mac(&command, &shell)).await
    }

    async fn focus_app(&self, name: &str) -> Result<bool, AppError> {
        let name = name.to_string();
        run_blocking(move || Ok(activate_app(&name))).await
    }

    async fn clipboard_get(&self) -> Result<String, AppError> {
        // arboard drives NSPasteboard under the hood and is already a dependency
        // (the Windows backend uses it too), so the clipboard path is shared.
        run_blocking(|| {
            let mut clipboard =
                arboard::Clipboard::new().map_err(|e| cfg_err(format!("clipboard open: {e}")))?;
            match clipboard.get_text() {
                Ok(text) => Ok(text),
                // An empty / non-text clipboard is not an error for our purposes.
                Err(arboard::Error::ContentNotAvailable) => Ok(String::new()),
                Err(e) => Err(cfg_err(format!("clipboard read: {e}"))),
            }
        })
        .await
    }

    async fn clipboard_set(&self, text: &str) -> Result<(), AppError> {
        let text = text.to_string();
        run_blocking(move || {
            let mut clipboard =
                arboard::Clipboard::new().map_err(|e| cfg_err(format!("clipboard open: {e}")))?;
            clipboard
                .set_text(text)
                .map_err(|e| cfg_err(format!("clipboard write: {e}")))
        })
        .await
    }

    async fn capture_screen(&self) -> Result<Option<Vec<u8>>, AppError> {
        // A screen grab needs the Screen Recording TCC grant. On failure we return
        // Ok(None) so the planner degrades to `tree` mode rather than failing.
        let png = tokio::task::spawn_blocking(capture_png)
            .await
            .map_err(|e| cfg_err(format!("act macos capture join: {e}")))?;
        Ok(png)
    }

    async fn click_point(&self, x: i32, y: i32) -> Result<(), AppError> {
        run_blocking(move || synth_click(x, y)).await
    }

    fn name(&self) -> &str {
        "macos-ax"
    }
}

// ---------------------------------------------------------------------------
// Owned AXUIElement handle
// ---------------------------------------------------------------------------

/// An owned `AXUIElementRef` with CoreFoundation reference-count semantics: it
/// releases on drop and retains on clone, so the DFS walk can hold and move
/// elements without leaking or double-freeing.
struct AxElement(AXUIElementRef);

impl AxElement {
    /// Adopt a `Create`/`Copy`-rule reference (we already own +1; no extra
    /// retain). Returns `None` for a null pointer.
    unsafe fn from_create(ptr: AXUIElementRef) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            Some(AxElement(ptr))
        }
    }

    /// Adopt a `Get`-rule reference (borrowed): retain it so it outlives the
    /// container it came from.
    unsafe fn from_get(ptr: AXUIElementRef) -> Option<Self> {
        if ptr.is_null() {
            return None;
        }
        CFRetain(ptr as CFTypeRef);
        Some(AxElement(ptr))
    }

    fn as_ref(&self) -> AXUIElementRef {
        self.0
    }
}

impl Drop for AxElement {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a non-null CF object we hold one reference to.
        unsafe { CFRelease(self.0 as CFTypeRef) }
    }
}

impl Clone for AxElement {
    fn clone(&self) -> Self {
        // SAFETY: retaining a live CF object; the clone owns its own +1.
        unsafe { CFRetain(self.0 as CFTypeRef) };
        AxElement(self.0)
    }
}

// ---------------------------------------------------------------------------
// AX attribute helpers
// ---------------------------------------------------------------------------

/// Copy an attribute value as an owned `CFType`, or `None` on any AX error / null.
fn copy_attr(el: AXUIElementRef, name: &str) -> Option<CFType> {
    let attr = CFString::new(name);
    let mut value: CFTypeRef = std::ptr::null();
    // SAFETY: `attr` outlives the call; `value` receives a Copy-rule reference we
    // adopt via `wrap_under_create_rule`.
    let err = unsafe { AXUIElementCopyAttributeValue(el, attr.as_concrete_TypeRef(), &mut value) };
    if err != kAXErrorSuccess || value.is_null() {
        return None;
    }
    Some(unsafe { CFType::wrap_under_create_rule(value) })
}

/// Copy an attribute whose value is another AXUIElement (e.g. `AXFocusedWindow`).
fn copy_attr_element(el: AXUIElementRef, name: &str) -> Option<AxElement> {
    let value = copy_attr(el, name)?;
    // Retain the underlying element so it survives `value` being dropped.
    unsafe { AxElement::from_get(value.as_CFTypeRef() as AXUIElementRef) }
}

/// Copy a string-valued attribute (`AXTitle`, `AXRole`, `AXDescription`).
fn copy_attr_string(el: AXUIElementRef, name: &str) -> Option<String> {
    let value = copy_attr(el, name)?;
    let raw = value.as_CFTypeRef();
    unsafe {
        if CFGetTypeID(raw) == CFStringGetTypeID() {
            let s = CFString::wrap_under_get_rule(raw as CFStringRef);
            return Some(s.to_string());
        }
    }
    None
}

/// Copy a boolean-valued attribute (`AXEnabled`, `AXSelected`).
fn copy_attr_bool(el: AXUIElementRef, name: &str) -> Option<bool> {
    let value = copy_attr(el, name)?;
    let raw = value.as_CFTypeRef();
    unsafe {
        if CFGetTypeID(raw) == CFBooleanGetTypeID() {
            return Some(CFBooleanGetValue(raw as CFBooleanRef));
        }
    }
    None
}

/// A control's `AXValue`, normalized. The `String` is the live text content and
/// is only ever measured / used as a fallback label — never stored in a snapshot.
enum AxValue {
    Text(usize, String),
    Number(i64),
}

/// Read `AXValue`, distinguishing text (fields, static text), numbers (sliders),
/// and booleans (checkboxes, reported as 0/1).
fn read_ax_value(el: AXUIElementRef) -> Option<AxValue> {
    let value = copy_attr(el, "AXValue")?;
    let raw = value.as_CFTypeRef();
    unsafe {
        let tid = CFGetTypeID(raw);
        if tid == CFStringGetTypeID() {
            let text = CFString::wrap_under_get_rule(raw as CFStringRef).to_string();
            return Some(AxValue::Text(text.chars().count(), text));
        }
        if tid == CFNumberGetTypeID() {
            let n = CFNumber::wrap_under_get_rule(raw as CFNumberRef);
            return Some(AxValue::Number(n.to_i64().unwrap_or(0)));
        }
        if tid == CFBooleanGetTypeID() {
            let val = CFBooleanGetValue(raw as CFBooleanRef);
            return Some(AxValue::Number(if val { 1 } else { 0 }));
        }
    }
    None
}

/// Extract a `CGPoint` from an `AXValue` attribute (e.g. `AXPosition`).
fn read_point(el: AXUIElementRef, name: &str) -> Option<CGPoint> {
    let value = copy_attr(el, name)?;
    let ax = value.as_CFTypeRef() as AXValueRef;
    let mut p = CGPoint::new(0.0, 0.0);
    // SAFETY: `ax` is an AXValue of CGPoint type; we hand AXValueGetValue a
    // matching, correctly-sized destination.
    let ok = unsafe {
        AXValueGetValue(
            ax,
            kAXValueTypeCGPoint,
            &mut p as *mut CGPoint as *mut c_void,
        )
    };
    if ok {
        Some(p)
    } else {
        None
    }
}

/// Extract a `CGSize` from an `AXValue` attribute (e.g. `AXSize`).
fn read_size(el: AXUIElementRef, name: &str) -> Option<CGSize> {
    let value = copy_attr(el, name)?;
    let ax = value.as_CFTypeRef() as AXValueRef;
    let mut s = CGSize::new(0.0, 0.0);
    let ok =
        unsafe { AXValueGetValue(ax, kAXValueTypeCGSize, &mut s as *mut CGSize as *mut c_void) };
    if ok {
        Some(s)
    } else {
        None
    }
}

/// On-screen bounds from `AXPosition` + `AXSize`.
fn read_bounds(el: AXUIElementRef) -> Option<Bounds> {
    let pos = read_point(el, "AXPosition")?;
    let size = read_size(el, "AXSize")?;
    Some(Bounds {
        x: pos.x as i32,
        y: pos.y as i32,
        w: size.width as i32,
        h: size.height as i32,
    })
}

/// The children of an element as owned handles.
fn ax_children(el: AXUIElementRef) -> Vec<AxElement> {
    let attr = CFString::new("AXChildren");
    let mut value: CFTypeRef = std::ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(el, attr.as_concrete_TypeRef(), &mut value) };
    if err != kAXErrorSuccess || value.is_null() {
        return Vec::new();
    }
    let array = value as CFArrayRef;
    let count = unsafe { CFArrayGetCount(array) };
    let mut out = Vec::with_capacity(count.max(0) as usize);
    for i in 0..count {
        let item = unsafe { CFArrayGetValueAtIndex(array, i) } as AXUIElementRef;
        if let Some(child) = unsafe { AxElement::from_get(item) } {
            out.push(child);
        }
    }
    // Release the Copy-rule array (its elements were individually retained above).
    unsafe { CFRelease(value) };
    out
}

/// The application's first window, used as a last resort when neither
/// `AXFocusedWindow` nor `AXMainWindow` resolves.
fn first_window(app: AXUIElementRef) -> Option<AxElement> {
    let attr = CFString::new("AXWindows");
    let mut value: CFTypeRef = std::ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(app, attr.as_concrete_TypeRef(), &mut value) };
    if err != kAXErrorSuccess || value.is_null() {
        return None;
    }
    let array = value as CFArrayRef;
    let count = unsafe { CFArrayGetCount(array) };
    let result = if count > 0 {
        let item = unsafe { CFArrayGetValueAtIndex(array, 0) } as AXUIElementRef;
        unsafe { AxElement::from_get(item) }
    } else {
        None
    };
    unsafe { CFRelease(value) };
    result
}

/// Perform a named AX action (`AXPress`, `AXRaise`, …). Returns whether it
/// succeeded.
fn perform(el: AXUIElementRef, action: &str) -> bool {
    let a = CFString::new(action);
    unsafe { AXUIElementPerformAction(el, a.as_concrete_TypeRef()) == kAXErrorSuccess }
}

/// Set `AXFocused = true` on an element. Returns whether it succeeded.
fn set_focused(el: AXUIElementRef) -> bool {
    let attr = CFString::new("AXFocused");
    let t = CFBoolean::true_value();
    unsafe {
        AXUIElementSetAttributeValue(el, attr.as_concrete_TypeRef(), t.as_CFTypeRef())
            == kAXErrorSuccess
    }
}

/// Set `AXValue` to a string. Returns whether it succeeded (many controls have a
/// non-settable value, in which case the caller falls back to typing).
fn set_ax_value(el: AXUIElementRef, value: &str) -> bool {
    let attr = CFString::new("AXValue");
    let v = CFString::new(value);
    unsafe {
        AXUIElementSetAttributeValue(el, attr.as_concrete_TypeRef(), v.as_CFTypeRef())
            == kAXErrorSuccess
    }
}

// ---------------------------------------------------------------------------
// Frontmost app / window resolution
// ---------------------------------------------------------------------------

/// The frontmost application's pid and localized name, via NSWorkspace.
///
/// Called from a blocking-pool thread. Reading `frontmostApplication`'s pid /
/// name off the main thread is well-trodden and stable; nothing here mutates
/// AppKit UI state.
fn frontmost_pid_and_name() -> Option<(i32, String)> {
    let cls = AnyClass::get("NSWorkspace")?;
    // SAFETY: standard AppKit singleton + property reads; all returns are checked.
    unsafe {
        let workspace: *mut AnyObject = msg_send![cls, sharedWorkspace];
        if workspace.is_null() {
            return None;
        }
        let app: *mut AnyObject = msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let pid: i32 = msg_send![app, processIdentifier];
        let name_ns: *mut AnyObject = msg_send![app, localizedName];
        let name = ns_string_to_rust(name_ns);
        Some((pid, name))
    }
}

/// Resolve the frontmost app's target window (focused → main → first), falling
/// back to the application element itself. Shared by `build_snapshot` and
/// `resolve` so the `#/…` paths they produce and consume address the same root.
fn frontmost_window_root() -> Result<AxElement, AppError> {
    // The Accessibility TCC grant gates BOTH reading the tree and posting input.
    if !unsafe { AXIsProcessTrusted() } {
        return Err(cfg_err(
            "Accessibility permission not granted for OpenTypeless \
             (System Settings > Privacy & Security > Accessibility).",
        ));
    }
    let (pid, _name) =
        frontmost_pid_and_name().ok_or_else(|| cfg_err("no frontmost application"))?;
    let app_el = unsafe { AxElement::from_create(AXUIElementCreateApplication(pid)) }
        .ok_or_else(|| cfg_err("AXUIElementCreateApplication returned null"))?;
    let window = copy_attr_element(app_el.as_ref(), "AXFocusedWindow")
        .or_else(|| copy_attr_element(app_el.as_ref(), "AXMainWindow"))
        .or_else(|| first_window(app_el.as_ref()))
        .unwrap_or_else(|| app_el.clone());
    Ok(window)
}

// ---------------------------------------------------------------------------
// Snapshot walk
// ---------------------------------------------------------------------------

/// Walk the frontmost window's subtree into a [`Snapshot`].
///
/// Mirrors the Windows walk: iterative DFS carrying the 1-based child-index path,
/// bounded by an element cap (== grounding budget), a visited-node cap, a max
/// depth, and a wall-clock deadline. Structural containers are traversed for
/// their descendants but only emitted when actionable or focused, and the full
/// per-node property reads are paid only for emitted elements.
fn build_snapshot(
    element_cap: usize,
    visit_cap: usize,
    max_depth: usize,
    budget: Duration,
) -> Result<Snapshot, AppError> {
    if !unsafe { AXIsProcessTrusted() } {
        return Err(cfg_err(
            "Accessibility permission not granted for OpenTypeless \
             (System Settings > Privacy & Security > Accessibility).",
        ));
    }

    let (pid, app_name) =
        frontmost_pid_and_name().ok_or_else(|| cfg_err("no frontmost application"))?;
    let app_el = unsafe { AxElement::from_create(AXUIElementCreateApplication(pid)) }
        .ok_or_else(|| cfg_err("AXUIElementCreateApplication returned null"))?;

    let focused_el = copy_attr_element(app_el.as_ref(), "AXFocusedUIElement");
    let window = copy_attr_element(app_el.as_ref(), "AXFocusedWindow")
        .or_else(|| copy_attr_element(app_el.as_ref(), "AXMainWindow"))
        .or_else(|| first_window(app_el.as_ref()))
        .unwrap_or_else(|| app_el.clone());
    let window_title = copy_attr_string(window.as_ref(), "AXTitle").unwrap_or_default();

    let focused_raw = focused_el.as_ref().map(|e| e.as_ref());
    let deadline = Instant::now() + budget;

    let mut elements: Vec<UiElement> = Vec::new();
    let mut focused_path: Option<String> = None;
    let mut visited: usize = 0;

    // Iterative DFS carrying each node's `#/i/j` path. Root is the window at `#`.
    let mut stack: Vec<(AxElement, String, usize)> = vec![(window, "#".to_string(), 0)];
    while let Some((element, path, depth)) = stack.pop() {
        if elements.len() >= element_cap || visited >= visit_cap || Instant::now() >= deadline {
            break;
        }
        visited += 1;

        let role_str = copy_attr_string(element.as_ref(), "AXRole").unwrap_or_default();

        let is_focused = match focused_raw {
            Some(fr) => unsafe { CFEqual(element.as_ref() as CFTypeRef, fr as CFTypeRef) != 0 },
            None => false,
        };
        if is_focused && focused_path.is_none() {
            focused_path = Some(path.clone());
        }

        if is_focused || is_interactable_role(&role_str) {
            elements.push(map_element(element.as_ref(), &role_str, &path, is_focused));
        }

        if depth < max_depth {
            // Push children reversed so the 1-based first child pops first; paths
            // stay 1-based over the FULL child list so `resolve` can walk them back
            // even though structural children are not emitted.
            let children = ax_children(element.as_ref());
            for (index, child) in children.into_iter().enumerate().rev() {
                let child_path = format!("{path}/{}", index + 1);
                stack.push((child, child_path, depth + 1));
            }
        }
    }

    Ok(Snapshot {
        app: app_name,
        window_title,
        focused: focused_path,
        // AX resolves the focused element, not a pointer hit-test; no pointer
        // element is reported (matches the Windows backend).
        pointer: None,
        // No portable text-selection length accessor is used here.
        selection_text_len: 0,
        elements,
    })
}

/// Whether an AX role string is an actionable control worth emitting. Structural
/// containers (windows, groups, static text, scroll areas) are walked for their
/// descendants but never spend an element-cap slot — kept in sync with the roles
/// [`UiElement::is_interactive`] recognizes.
fn is_interactable_role(role: &str) -> bool {
    matches!(
        role,
        "AXButton"
            | "AXMenuButton"
            | "AXPopUpButton"
            | "AXComboBox"
            | "AXCheckBox"
            | "AXRadioButton"
            | "AXTextField"
            | "AXTextArea"
            | "AXSecureTextField"
            | "AXSlider"
            | "AXIncrementor"
            | "AXMenuItem"
            | "AXMenuBarItem"
            | "AXLink"
            | "AXTab"
            | "AXDisclosureTriangle"
    )
}

/// Roles typically activatable via the `AXPress` action.
fn role_supports_invoke(role: &str) -> bool {
    matches!(
        role,
        "AXButton"
            | "AXMenuButton"
            | "AXPopUpButton"
            | "AXMenuItem"
            | "AXMenuBarItem"
            | "AXLink"
            | "AXCheckBox"
            | "AXRadioButton"
            | "AXTab"
            | "AXDisclosureTriangle"
    )
}

/// Map an AX role string onto our normalized [`Role`].
fn map_role(role: &str) -> Role {
    match role {
        "AXButton" | "AXMenuButton" => Role::Button,
        "AXPopUpButton" | "AXComboBox" => Role::ComboBox,
        "AXTextField" | "AXTextArea" | "AXSecureTextField" => Role::TextField,
        "AXCheckBox" => Role::CheckBox,
        "AXRadioButton" => Role::RadioButton,
        "AXMenuItem" | "AXMenuBarItem" => Role::MenuItem,
        "AXMenu" => Role::Menu,
        "AXMenuBar" => Role::MenuBar,
        "AXTab" => Role::Tab,
        "AXTabGroup" => Role::Group,
        "AXLink" => Role::Link,
        "AXSlider" => Role::Slider,
        "AXIncrementor" => Role::Spinner,
        "AXStaticText" | "AXHeading" => Role::Text,
        "AXImage" => Role::Image,
        "AXList" => Role::List,
        "AXRow" => Role::Row,
        "AXCell" => Role::Cell,
        "AXTable" => Role::Table,
        "AXOutline" => Role::Tree,
        "AXDisclosureTriangle" => Role::Button,
        "AXGroup" | "AXSplitGroup" | "AXRadioGroup" => Role::Group,
        "AXScrollArea" => Role::Pane,
        "AXScrollBar" => Role::ScrollBar,
        "AXToolbar" => Role::Toolbar,
        "AXProgressIndicator" | "AXBusyIndicator" => Role::ProgressBar,
        "AXWindow" | "AXSheet" | "AXDrawer" => Role::Window,
        "AXWebArea" => Role::Document,
        _ => Role::Unknown,
    }
}

/// Build a [`UiElement`] for an emitted node, reading its label / value length /
/// bounds / states / patterns.
///
/// PHI: the element's value text is read only to take its `char` count and, for
/// non-editable label roles, as a name fallback; it is dropped at the end of this
/// function and never stored. Secure text fields are never read.
fn map_element(el: AXUIElementRef, role_str: &str, path: &str, is_focused: bool) -> UiElement {
    let role = map_role(role_str);
    let secure = role_str == "AXSecureTextField";

    let title = copy_attr_string(el, "AXTitle").filter(|s| !s.is_empty());
    let description = copy_attr_string(el, "AXDescription").filter(|s| !s.is_empty());

    let value = if secure { None } else { read_ax_value(el) };
    let value_len = match &value {
        Some(AxValue::Text(len, _)) => *len,
        _ => 0,
    };
    let value_is_set_number = matches!(&value, Some(AxValue::Number(n)) if *n != 0);

    let is_text_input = matches!(role, Role::TextField | Role::ComboBox);

    // name = AXTitle || AXDescription || (non-editable) AXValue text. Editable /
    // secure field contents are never promoted to the name (PHI-safety).
    let name = title
        .or_else(|| description.clone())
        .or_else(|| match &value {
            Some(AxValue::Text(_, s)) if !is_text_input && !secure => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default();

    let bounds = read_bounds(el);
    let enabled = copy_attr_bool(el, "AXEnabled");
    let selected = copy_attr_bool(el, "AXSelected");

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
    if matches!(role, Role::CheckBox | Role::RadioButton) && value_is_set_number {
        states.push(ElementState::Checked);
    }
    // AX has no portable "offscreen" flag; a zero-area rect is our proxy.
    if let Some(b) = &bounds {
        if b.w <= 0 || b.h <= 0 {
            states.push(ElementState::Offscreen);
        }
    }

    // Pattern detection is role/value based (no AX action enumeration), matching
    // the Windows backend's heuristics.
    let mut patterns = Vec::new();
    if role_supports_invoke(role_str) {
        patterns.push(ActionPattern::Invoke);
    }
    if is_text_input || value_len > 0 {
        patterns.push(ActionPattern::SetValue);
    }
    if matches!(role, Role::CheckBox | Role::RadioButton) {
        patterns.push(ActionPattern::Toggle);
    }
    if matches!(
        role,
        Role::ListItem | Role::MenuItem | Role::Tab | Role::Row | Role::Cell | Role::TreeItem
    ) {
        patterns.push(ActionPattern::Select);
    }
    if matches!(role, Role::ComboBox) {
        patterns.push(ActionPattern::Expand);
    }
    if matches!(
        role,
        Role::List
            | Role::Table
            | Role::Tree
            | Role::Document
            | Role::Group
            | Role::Pane
            | Role::ScrollBar
    ) {
        patterns.push(ActionPattern::Scroll);
    }

    UiElement {
        path: path.to_string(),
        role,
        name,
        description: description.unwrap_or_default(),
        value_len,
        states,
        bounds,
        patterns,
    }
}

/// Resolve a `#/i/j/k` path back to a live element under the frontmost window.
fn resolve(path: &str) -> Result<AxElement, AppError> {
    let mut current = frontmost_window_root()?;

    let mut parts = path.split('/');
    match parts.next() {
        Some("#") => {}
        _ => return Err(cfg_err(format!("invalid path root in '{path}'"))),
    }

    for token in parts {
        if token.is_empty() {
            continue;
        }
        let index: usize = token
            .parse()
            .map_err(|_| cfg_err(format!("invalid path segment '{token}' in '{path}'")))?;
        if index == 0 {
            return Err(cfg_err(format!(
                "path segments are 1-based, got 0 in '{path}'"
            )));
        }
        let children = ax_children(current.as_ref());
        current = children
            .into_iter()
            .nth(index - 1)
            .ok_or_else(|| cfg_err(format!("child index {index} out of range in '{path}'")))?;
    }

    Ok(current)
}

// ---------------------------------------------------------------------------
// Input synthesis (CGEvent)
// ---------------------------------------------------------------------------

/// Type arbitrary text at the caret via `CGEventKeyboardSetUnicodeString`.
fn synth_text(text: &str) -> Result<(), AppError> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| cfg_err("CGEventSource::new failed (Accessibility permission?)"))?;
    // A single keyboard event carrying the whole unicode string, posted down then
    // up. Keycode 0 is ignored when a unicode string is attached.
    let down = CGEvent::new_keyboard_event(source.clone(), 0u16, true)
        .map_err(|_| cfg_err("failed to create key-down event"))?;
    down.set_string(text);
    down.post(CGEventTapLocation::HID);
    let up = CGEvent::new_keyboard_event(source, 0u16, false)
        .map_err(|_| cfg_err("failed to create key-up event"))?;
    up.set_string(text);
    up.post(CGEventTapLocation::HID);
    Ok(())
}

/// Post a modifier + key chord.
fn synth_key(flags: CGEventFlags, keycode: u16) -> Result<(), AppError> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| cfg_err("CGEventSource::new failed (Accessibility permission?)"))?;
    let down = CGEvent::new_keyboard_event(source.clone(), keycode, true)
        .map_err(|_| cfg_err("failed to create key-down event"))?;
    down.set_flags(flags);
    down.post(CGEventTapLocation::HID);
    let up = CGEvent::new_keyboard_event(source, keycode, false)
        .map_err(|_| cfg_err("failed to create key-up event"))?;
    up.set_flags(flags);
    up.post(CGEventTapLocation::HID);
    Ok(())
}

/// A left-button click at an absolute screen coordinate.
fn synth_click(x: i32, y: i32) -> Result<(), AppError> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| cfg_err("CGEventSource::new failed (Accessibility permission?)"))?;
    let point = CGPoint::new(x as f64, y as f64);
    let down = CGEvent::new_mouse_event(
        source.clone(),
        CGEventType::LeftMouseDown,
        point,
        CGMouseButton::Left,
    )
    .map_err(|_| cfg_err("failed to create mouse-down event"))?;
    down.post(CGEventTapLocation::HID);
    let up = CGEvent::new_mouse_event(source, CGEventType::LeftMouseUp, point, CGMouseButton::Left)
        .map_err(|_| cfg_err("failed to create mouse-up event"))?;
    up.post(CGEventTapLocation::HID);
    Ok(())
}

/// Parse a combo such as `"ctrl+v"`, `"cmd+c"`, `"enter"`, `"escape"` into
/// `(modifier flags, primary keycode)`.
///
/// macOS note: the platform-standard shortcut modifier is **Command**, not
/// Control. We map exactly as spoken/typed — `"ctrl+c"` → Control+C, `"cmd+c"` /
/// `"meta+c"` → Command+C — and additionally treat `meta`/`super`/`win`/`command`
/// as Command. Flows authored for the mac should use `cmd+…` for copy/paste/etc.;
/// a literal `ctrl+…` is sent as Control and generally will NOT trigger the mac
/// clipboard shortcuts.
fn parse_combo(combo: &str) -> Result<(CGEventFlags, u16), AppError> {
    let mut flags = CGEventFlags::empty();
    let mut primary: Option<String> = None;
    let mut modifiers = 0usize;

    for raw in combo.split('+') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => {
                flags |= CGEventFlags::CGEventFlagControl;
                modifiers += 1;
            }
            "shift" => {
                flags |= CGEventFlags::CGEventFlagShift;
                modifiers += 1;
            }
            "alt" | "option" | "opt" => {
                flags |= CGEventFlags::CGEventFlagAlternate;
                modifiers += 1;
            }
            "meta" | "super" | "win" | "windows" | "cmd" | "command" => {
                flags |= CGEventFlags::CGEventFlagCommand;
                modifiers += 1;
            }
            _ => primary = Some(token.to_string()),
        }
    }

    let Some(key) = primary else {
        // A lone modifier tap (e.g. `cmd` alone) presses that modifier key.
        if modifiers == 1 {
            let code = if flags.contains(CGEventFlags::CGEventFlagCommand) {
                55 // kVK_Command
            } else if flags.contains(CGEventFlags::CGEventFlagShift) {
                56 // kVK_Shift
            } else if flags.contains(CGEventFlags::CGEventFlagAlternate) {
                58 // kVK_Option
            } else {
                59 // kVK_Control
            };
            return Ok((CGEventFlags::empty(), code));
        }
        return Err(cfg_err(format!("no primary key in combo '{combo}'")));
    };

    let code = keycode_for(&key)
        .ok_or_else(|| cfg_err(format!("unsupported key '{key}' in combo '{combo}'")))?;
    Ok((flags, code))
}

/// Map a key name (single letter/digit or a named key) to its US-ANSI virtual
/// keycode (`kVK_*`). Returns `None` for anything unmapped.
fn keycode_for(key: &str) -> Option<u16> {
    let code = match key.to_ascii_lowercase().as_str() {
        "a" => 0,
        "s" => 1,
        "d" => 2,
        "f" => 3,
        "h" => 4,
        "g" => 5,
        "z" => 6,
        "x" => 7,
        "c" => 8,
        "v" => 9,
        "b" => 11,
        "q" => 12,
        "w" => 13,
        "e" => 14,
        "r" => 15,
        "y" => 16,
        "t" => 17,
        "1" => 18,
        "2" => 19,
        "3" => 20,
        "4" => 21,
        "6" => 22,
        "5" => 23,
        "=" | "equal" => 24,
        "9" => 25,
        "7" => 26,
        "-" | "minus" => 27,
        "8" => 28,
        "0" => 29,
        "]" | "rightbracket" => 30,
        "o" => 31,
        "u" => 32,
        "[" | "leftbracket" => 33,
        "i" => 34,
        "p" => 35,
        "l" => 37,
        "j" => 38,
        "'" | "quote" => 39,
        "k" => 40,
        ";" | "semicolon" => 41,
        "\\" | "backslash" => 42,
        "," | "comma" => 43,
        "/" | "slash" => 44,
        "n" => 45,
        "m" => 46,
        "." | "period" => 47,
        "`" | "grave" => 50,
        "enter" | "return" => 36,
        "tab" => 48,
        "space" | " " => 49,
        // On mac the "delete" key IS backspace (kVK_Delete = 51).
        "backspace" | "delete" => 51,
        "esc" | "escape" => 53,
        "del" | "forwarddelete" => 117,
        "home" => 115,
        "end" => 119,
        "pageup" | "pgup" => 116,
        "pagedown" | "pgdn" => 121,
        "left" => 123,
        "right" => 124,
        "down" => 125,
        "up" => 126,
        "f1" => 122,
        "f2" => 120,
        "f3" => 99,
        "f4" => 118,
        "f5" => 96,
        "f6" => 97,
        "f7" => 98,
        "f8" => 100,
        "f9" => 101,
        "f10" => 109,
        "f11" => 103,
        "f12" => 111,
        _ => return None,
    };
    Some(code)
}

// ---------------------------------------------------------------------------
// Script primitives (launch / open / shell / activate / clipboard / capture)
// ---------------------------------------------------------------------------

/// Launch an app by name or bundle id via `/usr/bin/open`.
fn launch_app(target: &str) -> Result<(), AppError> {
    // `open -a <name>` resolves display names ("Safari") and app paths; if that
    // fails and the target looks like a bundle id, try `open -b`.
    if Command::new("/usr/bin/open")
        .arg("-a")
        .arg(target)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Ok(());
    }
    if target.contains('.')
        && Command::new("/usr/bin/open")
            .arg("-b")
            .arg(target)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    {
        return Ok(());
    }
    Err(cfg_err(format!("could not launch '{target}'")))
}

/// Open a URI via `/usr/bin/open`, normalizing a bare spoken domain
/// ("youtube.com") to `https://` so it takes the browser path (parity with the
/// Windows backend).
fn open_uri_mac(uri: &str) -> Result<(), AppError> {
    let trimmed = uri.trim();
    let lower = trimmed.to_ascii_lowercase();
    let is_http = lower.starts_with("http://") || lower.starts_with("https://");

    // An explicit non-web scheme? (`mailto:`, `file:`, `x-apple.systempreferences:`)
    let has_scheme = trimmed.find(':').is_some_and(|i| {
        let scheme = &trimmed[..i];
        let after = trimmed[i + 1..].chars().next();
        !scheme.is_empty()
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            && !after.is_some_and(|c| c.is_ascii_digit())
    });

    let target = if is_http {
        trimmed.to_string()
    } else if !has_scheme && trimmed.contains('.') && !trimmed.chars().any(char::is_whitespace) {
        format!("https://{trimmed}")
    } else {
        trimmed.to_string()
    };

    let status = Command::new("/usr/bin/open")
        .arg(&target)
        .status()
        .map_err(|e| cfg_err(format!("open failed: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(cfg_err(format!("open exited with status {status}")))
    }
}

/// Resolve a shell name/path to an absolute interpreter path (default zsh).
fn resolve_shell(shell: &str) -> String {
    let s = shell.trim();
    if s.is_empty() {
        return "/bin/zsh".to_string();
    }
    if s.contains('/') {
        return s.to_string();
    }
    match s.to_ascii_lowercase().as_str() {
        "zsh" => "/bin/zsh".to_string(),
        "bash" => "/bin/bash".to_string(),
        "sh" => "/bin/sh".to_string(),
        other => format!("/bin/{other}"),
    }
}

/// Run a shell command with a wall-clock timeout, capturing stdout + stderr.
///
/// Pipes are drained by reader threads so a chatty command cannot deadlock on a
/// full pipe buffer; on timeout the child is killed.
fn run_shell_mac(command: &str, shell: &str) -> Result<ShellOutput, AppError> {
    let shell_path = resolve_shell(shell);
    let mut child = Command::new(&shell_path)
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| cfg_err(format!("spawn shell '{shell_path}': {e}")))?;

    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let out_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(pipe) = stdout_pipe.as_mut() {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(pipe) = stderr_pipe.as_mut() {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(e) => return Err(cfg_err(format!("wait on shell child: {e}"))),
        }
        if start.elapsed() >= SHELL_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    let stdout_str = out_handle.join().unwrap_or_default();
    let stderr_str = err_handle.join().unwrap_or_default();

    let Some(status) = status else {
        return Err(cfg_err(format!(
            "shell command timed out after {}s",
            SHELL_TIMEOUT.as_secs()
        )));
    };

    let mut stdout = stdout_str;
    if !stderr_str.trim().is_empty() {
        if !stdout.is_empty() {
            stdout.push('\n');
        }
        stdout.push_str(&stderr_str);
    }
    if stdout.chars().count() > SHELL_MAX_OUTPUT_CHARS {
        stdout = stdout
            .chars()
            .take(SHELL_MAX_OUTPUT_CHARS)
            .collect::<String>()
            + "…[output truncated]";
    }

    Ok(ShellOutput {
        exit_code: status.code().unwrap_or(-1),
        stdout,
    })
}

/// Bring a named app to the foreground via NSWorkspace's running-applications
/// list, matching on localized name or bundle id. Returns whether one matched.
fn activate_app(name: &str) -> bool {
    let Some(cls) = AnyClass::get("NSWorkspace") else {
        return false;
    };
    let needle = name.trim().to_lowercase();
    if needle.is_empty() {
        return false;
    }
    unsafe {
        let workspace: *mut AnyObject = msg_send![cls, sharedWorkspace];
        if workspace.is_null() {
            return false;
        }
        let apps: *mut AnyObject = msg_send![workspace, runningApplications];
        if apps.is_null() {
            return false;
        }
        let count: usize = msg_send![apps, count];
        for i in 0..count {
            let app: *mut AnyObject = msg_send![apps, objectAtIndex: i];
            if app.is_null() {
                continue;
            }
            let ln: *mut AnyObject = msg_send![app, localizedName];
            let bid: *mut AnyObject = msg_send![app, bundleIdentifier];
            let lname = ns_string_to_rust(ln).to_lowercase();
            let bname = ns_string_to_rust(bid).to_lowercase();
            let matched = (!lname.is_empty() && lname.contains(&needle))
                || (!bname.is_empty() && bname.contains(&needle));
            if matched {
                // NSApplicationActivateIgnoringOtherApps = 1 << 1. Deprecated on
                // recent macOS but still functional; the simplest cross-version
                // activate.
                let options: usize = 1 << 1;
                let ok: Bool = msg_send![app, activateWithOptions: options];
                return ok.as_bool();
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Screen capture
// ---------------------------------------------------------------------------

/// Capture a PNG of the main display, or `None` on failure (caller degrades to
/// `tree` mode). Tries Core Graphics first, then the `screencapture` CLI.
fn capture_png() -> Option<Vec<u8>> {
    if let Some(png) = capture_via_cg() {
        return Some(png);
    }
    capture_via_screencapture()
}

/// Capture via `CGDisplayCreateImage` (deprecated on macOS 14+ but still
/// functional with the Screen Recording grant). BGRA + row padding are handled
/// when repacking into tight RGBA for the PNG encoder.
fn capture_via_cg() -> Option<Vec<u8>> {
    use image::ImageEncoder;

    let image = CGDisplay::main().image()?;
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return None;
    }
    let bytes_per_row = image.bytes_per_row();
    let bpp = (image.bits_per_pixel() / 8).max(1);

    let data = image.data();
    let ptr = unsafe { CFDataGetBytePtr(data.as_concrete_TypeRef()) };
    let len = unsafe { CFDataGetLength(data.as_concrete_TypeRef()) } as usize;
    if ptr.is_null() || len == 0 {
        return None;
    }
    let src = unsafe { std::slice::from_raw_parts(ptr, len) };

    let mut rgba = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let row_start = y * bytes_per_row;
        let row_end = row_start + width * bpp;
        if row_end > len {
            break;
        }
        for px in src[row_start..row_end].chunks_exact(bpp) {
            // Core Graphics display images are BGRA (little-endian ARGB8888).
            let b = px[0];
            let g = px[1];
            let r = px[2];
            let a = if bpp >= 4 { px[3] } else { 255 };
            rgba.push(r);
            rgba.push(g);
            rgba.push(b);
            rgba.push(a);
        }
    }

    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &rgba,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .ok()?;
    Some(png)
}

/// Fallback capture via `/usr/sbin/screencapture -x -t png`.
fn capture_via_screencapture() -> Option<Vec<u8>> {
    let path = std::env::temp_dir().join(format!("flowrad-capture-{}.png", std::process::id()));
    let status = Command::new("/usr/sbin/screencapture")
        .arg("-x")
        .arg("-t")
        .arg("png")
        .arg(&path)
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    Some(bytes)
}

// ---------------------------------------------------------------------------
// NSString helpers (raw objc2, matching the codebase convention in
// `stt/apple_speech.rs` — no objc2-app-kit dependency needed)
// ---------------------------------------------------------------------------

/// Convert an `NSString*` to a Rust `String` (empty on null).
fn ns_string_to_rust(ns: *mut AnyObject) -> String {
    if ns.is_null() {
        return String::new();
    }
    let ptr: *const c_char = unsafe { msg_send![ns, UTF8String] };
    if ptr.is_null() {
        return String::new();
    }
    unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// Build an autoreleased `NSString*` from a Rust `&str` (unused today but kept
/// alongside `ns_string_to_rust` for symmetry with the codebase helper set).
#[allow(dead_code)]
fn ns_string_from_str(value: &str) -> Option<*mut AnyObject> {
    let c = CString::new(value).ok()?;
    let cls = AnyClass::get("NSString")?;
    let ns: *mut AnyObject = unsafe { msg_send![cls, stringWithUTF8String: c.as_ptr()] };
    if ns.is_null() {
        None
    } else {
        Some(ns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_roles() {
        assert_eq!(map_role("AXButton"), Role::Button);
        assert_eq!(map_role("AXTextField"), Role::TextField);
        assert_eq!(map_role("AXPopUpButton"), Role::ComboBox);
        assert_eq!(map_role("AXStaticText"), Role::Text);
        assert_eq!(map_role("AXSomethingElse"), Role::Unknown);
    }

    #[test]
    fn interactable_roles_are_actionable_not_structural() {
        for role in [
            "AXButton",
            "AXTextField",
            "AXCheckBox",
            "AXMenuItem",
            "AXLink",
            "AXTab",
        ] {
            assert!(is_interactable_role(role), "{role} should be interactable");
        }
        for role in [
            "AXWindow",
            "AXGroup",
            "AXStaticText",
            "AXScrollArea",
            "AXToolbar",
        ] {
            assert!(
                !is_interactable_role(role),
                "{role} should be structural-only"
            );
        }
    }

    #[test]
    fn parses_ctrl_combo_as_control() {
        let (flags, code) = parse_combo("ctrl+v").unwrap();
        assert!(flags.contains(CGEventFlags::CGEventFlagControl));
        assert_eq!(code, 9); // kVK_ANSI_V
    }

    #[test]
    fn parses_cmd_and_meta_as_command() {
        let (f1, _) = parse_combo("cmd+c").unwrap();
        let (f2, _) = parse_combo("meta+c").unwrap();
        assert!(f1.contains(CGEventFlags::CGEventFlagCommand));
        assert!(f2.contains(CGEventFlags::CGEventFlagCommand));
    }

    #[test]
    fn parses_named_keys() {
        assert_eq!(parse_combo("enter").unwrap().1, 36);
        assert_eq!(parse_combo("escape").unwrap().1, 53);
        assert_eq!(parse_combo("shift+tab").unwrap().1, 48);
    }

    #[test]
    fn lone_modifier_taps_that_key() {
        let (flags, code) = parse_combo("cmd").unwrap();
        assert!(flags.is_empty());
        assert_eq!(code, 55); // kVK_Command
    }

    #[test]
    fn rejects_unknown_primary_key() {
        assert!(parse_combo("ctrl+£").is_err());
    }

    #[test]
    fn resolve_shell_defaults_to_zsh() {
        assert_eq!(resolve_shell(""), "/bin/zsh");
        assert_eq!(resolve_shell("bash"), "/bin/bash");
        assert_eq!(
            resolve_shell("/opt/homebrew/bin/fish"),
            "/opt/homebrew/bin/fish"
        );
    }

    #[test]
    fn backend_name_is_stable() {
        assert_eq!(MacBackend::new().name(), "macos-ax");
    }
}
