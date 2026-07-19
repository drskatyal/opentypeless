//! The platform accessibility + input backend contract.
//!
//! Windows (UIA), macOS (AX) and the test [`MockBackend`](super::mock_backend)
//! all implement this. The executor and snapshot service depend only on this
//! trait, never on a concrete platform, so the OS-agnostic Act core is fully
//! testable off-platform.

use async_trait::async_trait;

use crate::error::AppError;

use super::element::{ElementPath, Snapshot};

/// The result of running a shell command via [`AccessibilityBackend::run_shell`].
#[derive(Debug, Clone)]
pub struct ShellOutput {
    /// The process exit code (0 = success).
    pub exit_code: i32,
    /// Captured standard output (best-effort, may be truncated by the backend).
    pub stdout: String,
}

#[async_trait]
pub trait AccessibilityBackend: Send + Sync {
    /// Read a fresh L0+L1 snapshot of the focused window.
    async fn snapshot(&self) -> Result<Snapshot, AppError>;

    /// Whether the focused app runs at a higher integrity level than us and thus
    /// cannot be driven (Windows: elevated / admin). We detect and decline.
    async fn focused_app_is_elevated(&self) -> Result<bool, AppError>;

    /// Move keyboard focus to an element.
    async fn focus(&self, target: &ElementPath) -> Result<(), AppError>;

    /// Invoke an element's default action via its accessibility pattern.
    async fn invoke(&self, target: &ElementPath) -> Result<(), AppError>;

    /// Set an element's value directly via the accessibility `SetValue` pattern.
    async fn set_value(&self, target: &ElementPath, value: &str) -> Result<(), AppError>;

    /// Synthesize typed text at the current caret.
    async fn type_text(&self, text: &str) -> Result<(), AppError>;

    /// Synthesize a key combo such as `"meta+Enter"` or `"ctrl+c"`.
    async fn key_combo(&self, combo: &str) -> Result<(), AppError>;

    /// Launch / start an application or executable by name or path.
    async fn launch(&self, target: &str) -> Result<(), AppError>;

    /// Open a URI (URL or app scheme) via the OS handler.
    async fn open_uri(&self, uri: &str) -> Result<(), AppError>;

    /// Run a shell command in `shell`, returning its exit code and captured stdout.
    async fn run_shell(&self, command: &str, shell: &str) -> Result<ShellOutput, AppError>;

    /// Bring a named application's window to the foreground. Returns whether a
    /// matching window was found and focused.
    async fn focus_app(&self, name: &str) -> Result<bool, AppError>;

    /// Foreground lock: make sure the window that is about to be acted on is a
    /// legitimate target before a click / type / key / invoke / focus lands.
    ///
    /// The real problem this solves (Windows, `npm run tauri dev`): after a
    /// coordinate click the OS foreground repeatedly flips to the DEV CONSOLE that
    /// launched the app (`WindowsTerminal` / `cmd` / `powershell`), so the next
    /// snapshot reads the terminal and the planner misfires against the wrong
    /// window. This method inspects the *current* foreground process; if it is our
    /// own console/exe (never a valid target) or simply not the intended app, it
    /// brings `app_hint` forward and re-checks with a bounded retry.
    ///
    /// Returns `Ok(true)` when a legitimate target is confirmed foreground (or the
    /// platform has nothing to guard), `Ok(false)` when it could not get one in
    /// front (e.g. the console is stuck and there is no hint to switch to). The
    /// default is a no-op `Ok(true)` so the mock and macOS backends are unaffected;
    /// only the Windows backend overrides it.
    async fn ensure_foreground(&self, _app_hint: Option<&str>) -> Result<bool, AppError> {
        Ok(true)
    }

    /// Read the system clipboard's current text.
    async fn clipboard_get(&self) -> Result<String, AppError>;

    /// Overwrite the system clipboard with `text`.
    async fn clipboard_set(&self, text: &str) -> Result<(), AppError>;

    /// Capture a PNG screenshot of the foreground window for the screen-aware
    /// (`hybrid` / `vision`) plan modes. Returns `Ok(None)` when the platform has
    /// no capture implementation (the default), so callers degrade to the a11y
    /// `tree` mode rather than failing. See `docs/act-screen-aware-design.md`.
    async fn capture_screen(&self) -> Result<Option<Vec<u8>>, AppError> {
        Ok(None)
    }

    /// Click at an absolute screen coordinate (logical pixels). Used by the
    /// `vision` mode, whose plans target coordinates rather than element paths.
    /// The default is unsupported so a backend that hasn't implemented pointer
    /// synthesis fails the step cleanly instead of silently no-oping.
    async fn click_point(&self, _x: i32, _y: i32) -> Result<(), AppError> {
        Err(AppError::Config(
            "coordinate click is not supported by this backend".into(),
        ))
    }

    /// Scroll the foreground window / focused scrollable region by `dx`, `dy`
    /// wheel notches. Vertical: `dy > 0` scrolls DOWN (reveals content below the
    /// fold), `dy < 0` scrolls UP. Horizontal: `dx > 0` scrolls RIGHT, `dx < 0`
    /// LEFT. This is how below-the-fold search results / list items marked
    /// `offscreen` in a snapshot are brought within reach.
    ///
    /// The default is a no-op success so the mock and any backend without pointer
    /// synthesis still compile and degrade cleanly (a `Scroll` action simply does
    /// nothing rather than failing the plan). The Windows backend overrides it.
    async fn scroll(&self, _dx: i32, _dy: i32) -> Result<(), AppError> {
        Ok(())
    }

    /// Best-effort: bring `target` into view if it is scrolled offscreen, so a
    /// following invoke/focus lands on a real, on-screen control. The default is a
    /// no-op success (a backend without a scroll-into-view primitive still acts in
    /// place — an a11y invoke-by-path works regardless of viewport position). The
    /// Windows backend overrides it with the UIA scroll-into-view primitive.
    async fn scroll_into_view(&self, _target: &ElementPath) -> Result<(), AppError> {
        Ok(())
    }

    fn name(&self) -> &str;
}

/// Process/window names that must NEVER be treated as a valid Act target: our own
/// dev console (the `npm run tauri dev` terminal), the shells it can run under, and
/// the app's own executable. When one of these is foreground, the guard re-focuses
/// the intended app instead of snapshotting/acting on it. Compared against a
/// normalized (lowercased, `.exe`-stripped, whitespace-free) name via `contains`,
/// so a stem like `powershell` matches a "Windows PowerShell" title too.
pub const EXCLUDED_FOREGROUND_STEMS: &[&str] = &[
    "windowsterminal",
    "conhost",
    "powershell",
    "pwsh",
    "cmd",
    "opentypeless",
];

/// Whether a foreground process/window name is one of our own or a dev console
/// ([`EXCLUDED_FOREGROUND_STEMS`]) and therefore never a legitimate Act target.
pub fn is_excluded_foreground(name: &str) -> bool {
    let normalized = super::focus_guard::normalize_app_name(name);
    if normalized.is_empty() {
        return false;
    }
    EXCLUDED_FOREGROUND_STEMS
        .iter()
        .any(|stem| normalized.contains(stem))
}

/// The pure foreground-lock decision, factored out of the Win32 layer so it is
/// unit-testable off-platform (the `GetForegroundWindow`/`SetForegroundWindow`
/// calls that feed it are not).
///
/// Given the current foreground app/window title (`current_app`), its process
/// executable stem (`current_proc`), and the app the command intends to act on
/// (`target_hint`), decide whether the guard must re-focus rather than act on
/// whatever is in front:
///
/// * If the foreground is our own console/exe (either the title OR the process
///   stem matches [`EXCLUDED_FOREGROUND_STEMS`]) → always refocus. This is the dev
///   console stealing focus after a click; it can never bless itself as the target.
/// * Else, with a target intended, refocus only when the foreground is not that
///   target (matched fuzzily against both the title and the process stem, so
///   "Google Chrome" ~ "chrome.exe").
/// * Else (a normal app in front, no specific target) → leave it; a plain
///   "copy that" against the current window must be unaffected.
pub fn should_refocus(current_app: &str, current_proc: &str, target_hint: Option<&str>) -> bool {
    // The dev console / our own window is never a valid target, regardless of any
    // hint — refocus (or, with no hint, signal the caller the foreground is bad).
    if is_excluded_foreground(current_app) || is_excluded_foreground(current_proc) {
        return true;
    }

    match target_hint {
        // A specific target is intended: refocus unless it is already in front.
        Some(hint) => {
            !super::focus_guard::apps_match(current_app, hint)
                && !super::focus_guard::apps_match(current_proc, hint)
        }
        // No intended target and a normal app in front: nothing to do.
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_dev_console_is_always_excluded() {
        // The exact processes from the bug's run log — the terminal that ran
        // `npm run tauri dev` and the shells it launches under.
        assert!(is_excluded_foreground("WindowsTerminal"));
        assert!(is_excluded_foreground("windowsterminal.exe"));
        assert!(is_excluded_foreground("cmd"));
        assert!(is_excluded_foreground("Windows PowerShell"));
        assert!(is_excluded_foreground("powershell.exe"));
        assert!(is_excluded_foreground("pwsh"));
        assert!(is_excluded_foreground("conhost.exe"));
        // Our own app window.
        assert!(is_excluded_foreground("opentypeless"));
        assert!(is_excluded_foreground("OpenTypeless.exe"));
    }

    #[test]
    fn real_apps_are_not_excluded() {
        assert!(!is_excluded_foreground("Google Chrome"));
        assert!(!is_excluded_foreground("chrome.exe"));
        assert!(!is_excluded_foreground("Spotify"));
        assert!(!is_excluded_foreground("Notepad"));
        assert!(!is_excluded_foreground(""));
    }

    #[test]
    fn console_foreground_refocuses_even_with_no_hint() {
        // The core bug: the dev console flipped to the front after a click. Even
        // without a hint the guard must reject it as a target.
        assert!(should_refocus("Windows PowerShell", "powershell", None));
        assert!(should_refocus(
            r"C:\WINDOWS\system32\cmd.exe (windowsterminal)",
            "windowsterminal",
            Some("Google Chrome"),
        ));
    }

    #[test]
    fn console_foreground_refocuses_toward_target() {
        // Console in front but Chrome is the intended app → refocus Chrome.
        assert!(should_refocus("Windows PowerShell", "powershell", Some("Google Chrome")));
    }

    #[test]
    fn correct_target_already_front_is_a_noop() {
        // The intended app is already foreground (matched via title or proc stem):
        // no refocus, so we don't thrash a window that's already correct.
        assert!(!should_refocus("Google Chrome", "chrome", Some("Google Chrome")));
        assert!(!should_refocus("New Tab - Google Chrome", "chrome", Some("chrome.exe")));
        assert!(!should_refocus("Spotify Premium", "spotify", Some("Spotify")));
    }

    #[test]
    fn wrong_app_front_with_hint_refocuses() {
        // A normal-but-wrong app is in front and we know the intended target.
        assert!(should_refocus("Notepad", "notepad", Some("Google Chrome")));
    }

    #[test]
    fn normal_app_front_without_hint_is_a_noop() {
        // No intended target and an ordinary app in front → leave it (a plain
        // "copy that" against the current window must not be disturbed).
        assert!(!should_refocus("Notepad", "notepad", None));
        assert!(!should_refocus("Google Chrome", "chrome", None));
    }

    #[test]
    fn hint_matched_against_proc_when_title_differs() {
        // Spotify titles its window with the track, so the title won't match the
        // hint — the process stem must still recognize it as already-foreground.
        assert!(!should_refocus(
            "Red Hot Chili Peppers - Can't Stop",
            "spotify",
            Some("Spotify"),
        ));
    }
}
