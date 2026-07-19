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
