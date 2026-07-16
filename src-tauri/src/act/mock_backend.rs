//! In-memory [`AccessibilityBackend`] for tests and non-Windows default builds.
//!
//! It serves a scripted [`Snapshot`], reports a seeded elevation flag, and —
//! crucially — **records every call** (invoked/focused targets, `set_value`
//! pairs, typed text, key combos) in order, so the executor and grounding can be
//! asserted against end-to-end off-platform. All trait methods succeed.

use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::AppError;

use super::backend::AccessibilityBackend;
use super::element::{ElementPath, Snapshot};

/// The ordered log of calls made against a [`MockBackend`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Recorded {
    invoked: Vec<ElementPath>,
    focused: Vec<ElementPath>,
    set_values: Vec<(ElementPath, String)>,
    typed: Vec<String>,
    keys: Vec<String>,
}

/// A scriptable backend backed by an in-memory snapshot that records calls.
#[derive(Debug, Default)]
pub struct MockBackend {
    snapshot: Snapshot,
    elevated: bool,
    calls: Mutex<Recorded>,
}

impl MockBackend {
    /// A backend serving `snapshot`, not elevated, with an empty call log.
    pub fn new(snapshot: Snapshot) -> Self {
        Self {
            snapshot,
            elevated: false,
            calls: Mutex::new(Recorded::default()),
        }
    }

    /// Start configuring a backend.
    pub fn builder() -> MockBackendBuilder {
        MockBackendBuilder::default()
    }

    fn calls(&self) -> std::sync::MutexGuard<'_, Recorded> {
        self.calls.lock().expect("mock backend call log poisoned")
    }

    /// Targets passed to [`AccessibilityBackend::invoke`], in call order.
    pub fn invoked(&self) -> Vec<ElementPath> {
        self.calls().invoked.clone()
    }

    /// Targets passed to [`AccessibilityBackend::focus`], in call order.
    pub fn focused_targets(&self) -> Vec<ElementPath> {
        self.calls().focused.clone()
    }

    /// `(target, value)` pairs passed to [`AccessibilityBackend::set_value`].
    pub fn set_values(&self) -> Vec<(ElementPath, String)> {
        self.calls().set_values.clone()
    }

    /// Text passed to [`AccessibilityBackend::type_text`], in call order.
    pub fn typed(&self) -> Vec<String> {
        self.calls().typed.clone()
    }

    /// Combos passed to [`AccessibilityBackend::key_combo`], in call order.
    pub fn keys(&self) -> Vec<String> {
        self.calls().keys.clone()
    }
}

/// Builder that seeds the served [`Snapshot`] and the `elevated` flag.
#[derive(Debug, Default)]
pub struct MockBackendBuilder {
    snapshot: Snapshot,
    elevated: bool,
}

impl MockBackendBuilder {
    /// Seed the snapshot the backend serves.
    pub fn snapshot(mut self, snapshot: Snapshot) -> Self {
        self.snapshot = snapshot;
        self
    }

    /// Seed the value returned by `focused_app_is_elevated`.
    pub fn elevated(mut self, elevated: bool) -> Self {
        self.elevated = elevated;
        self
    }

    /// Finish building the backend.
    pub fn build(self) -> MockBackend {
        MockBackend {
            snapshot: self.snapshot,
            elevated: self.elevated,
            calls: Mutex::new(Recorded::default()),
        }
    }
}

#[async_trait]
impl AccessibilityBackend for MockBackend {
    async fn snapshot(&self) -> Result<Snapshot, AppError> {
        Ok(self.snapshot.clone())
    }

    async fn focused_app_is_elevated(&self) -> Result<bool, AppError> {
        Ok(self.elevated)
    }

    async fn focus(&self, target: &ElementPath) -> Result<(), AppError> {
        self.calls().focused.push(target.clone());
        Ok(())
    }

    async fn invoke(&self, target: &ElementPath) -> Result<(), AppError> {
        self.calls().invoked.push(target.clone());
        Ok(())
    }

    async fn set_value(&self, target: &ElementPath, value: &str) -> Result<(), AppError> {
        self.calls()
            .set_values
            .push((target.clone(), value.to_string()));
        Ok(())
    }

    async fn type_text(&self, text: &str) -> Result<(), AppError> {
        self.calls().typed.push(text.to_string());
        Ok(())
    }

    async fn key_combo(&self, combo: &str) -> Result<(), AppError> {
        self.calls().keys.push(combo.to_string());
        Ok(())
    }

    fn name(&self) -> &str {
        "mock"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn records_calls_in_order() {
        let backend = MockBackend::default();

        backend.focus(&"#/1".to_string()).await.unwrap();
        backend.type_text("hello").await.unwrap();
        backend.key_combo("ctrl+a").await.unwrap();
        backend.type_text("world").await.unwrap();
        backend.invoke(&"#/2".to_string()).await.unwrap();
        backend.key_combo("Enter").await.unwrap();
        backend
            .set_value(&"#/3".to_string(), "value")
            .await
            .unwrap();

        assert_eq!(backend.focused_targets(), vec!["#/1".to_string()]);
        assert_eq!(backend.invoked(), vec!["#/2".to_string()]);
        assert_eq!(
            backend.typed(),
            vec!["hello".to_string(), "world".to_string()]
        );
        assert_eq!(
            backend.keys(),
            vec!["ctrl+a".to_string(), "Enter".to_string()]
        );
        assert_eq!(
            backend.set_values(),
            vec![("#/3".to_string(), "value".to_string())]
        );
    }

    #[tokio::test]
    async fn builder_seeds_snapshot_and_elevation() {
        let snap = Snapshot {
            app: "Editor".into(),
            window_title: "Untitled".into(),
            ..Snapshot::default()
        };
        let backend = MockBackend::builder()
            .snapshot(snap.clone())
            .elevated(true)
            .build();

        assert_eq!(backend.snapshot().await.unwrap(), snap);
        assert!(backend.focused_app_is_elevated().await.unwrap());
    }

    #[tokio::test]
    async fn defaults_are_empty_and_unelevated() {
        let backend = MockBackend::default();
        assert!(!backend.focused_app_is_elevated().await.unwrap());
        assert!(backend.invoked().is_empty());
        assert!(backend.typed().is_empty());
        assert!(backend.keys().is_empty());
        assert!(backend.set_values().is_empty());
        assert!(backend.focused_targets().is_empty());
    }
}
