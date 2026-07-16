//! The platform accessibility + input backend contract.
//!
//! Windows (UIA), macOS (AX) and the test [`MockBackend`](super::mock_backend)
//! all implement this. The executor and snapshot service depend only on this
//! trait, never on a concrete platform, so the OS-agnostic Act core is fully
//! testable off-platform.

use async_trait::async_trait;

use crate::error::AppError;

use super::element::{ElementPath, Snapshot};

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

    fn name(&self) -> &str;
}
