//! In-memory [`AccessibilityBackend`] for tests and non-Windows default builds.
//!
//! TODO(act-phase0): record invoked actions, serve a scripted snapshot, and
//! model focus/value changes so the executor and grounding can be tested
//! end-to-end off-platform. Stub only.

use async_trait::async_trait;

use crate::error::AppError;

use super::backend::AccessibilityBackend;
use super::element::{ElementPath, Snapshot};

/// A scriptable backend backed by an in-memory snapshot.
#[derive(Debug, Default)]
pub struct MockBackend {
    snapshot: Snapshot,
}

impl MockBackend {
    pub fn new(snapshot: Snapshot) -> Self {
        Self { snapshot }
    }
}

#[async_trait]
impl AccessibilityBackend for MockBackend {
    async fn snapshot(&self) -> Result<Snapshot, AppError> {
        Ok(self.snapshot.clone())
    }

    async fn focused_app_is_elevated(&self) -> Result<bool, AppError> {
        Ok(false)
    }

    async fn focus(&self, _target: &ElementPath) -> Result<(), AppError> {
        Ok(())
    }

    async fn invoke(&self, _target: &ElementPath) -> Result<(), AppError> {
        Ok(())
    }

    async fn set_value(&self, _target: &ElementPath, _value: &str) -> Result<(), AppError> {
        Ok(())
    }

    async fn type_text(&self, _text: &str) -> Result<(), AppError> {
        Ok(())
    }

    async fn key_combo(&self, _combo: &str) -> Result<(), AppError> {
        Ok(())
    }

    fn name(&self) -> &str {
        "mock"
    }
}
