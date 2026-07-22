//! In-memory [`AccessibilityBackend`] for tests and non-Windows default builds.
//!
//! It serves a scripted [`Snapshot`], reports a seeded elevation flag, and —
//! crucially — **records every call** (invoked/focused targets, `set_value`
//! pairs, typed text, key combos) in order, so the executor and grounding can be
//! asserted against end-to-end off-platform. All trait methods succeed.

use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::AppError;

use super::backend::{AccessibilityBackend, ShellOutput};
use super::element::{ElementPath, Snapshot};

/// The ordered log of calls made against a [`MockBackend`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Recorded {
    invoked: Vec<ElementPath>,
    focused: Vec<ElementPath>,
    set_values: Vec<(ElementPath, String)>,
    typed: Vec<String>,
    keys: Vec<String>,
    launched: Vec<String>,
    opened_uris: Vec<String>,
    ran_shells: Vec<(String, String)>,
    focused_apps: Vec<String>,
    clipboard_sets: Vec<String>,
    scrolls: Vec<(i32, i32)>,
    scrolled_into_view: Vec<ElementPath>,
    clicks: Vec<(i32, i32)>,
}

/// A scriptable backend backed by an in-memory snapshot that records calls.
#[derive(Debug, Default)]
pub struct MockBackend {
    snapshot: Snapshot,
    elevated: bool,
    /// Text returned by `clipboard_get`.
    clipboard: String,
    /// `(exit_code, stdout)` returned by `run_shell`.
    shell_output: (i32, String),
    /// Value returned by `ensure_foreground` — `false` models "our own console /
    /// wrong app is in front with no valid target", which must make a coordinate
    /// `Click` refuse rather than click blindly.
    foreground_ok: bool,
    calls: Mutex<Recorded>,
}

impl MockBackend {
    /// A backend serving `snapshot`, not elevated, with an empty call log.
    pub fn new(snapshot: Snapshot) -> Self {
        Self {
            snapshot,
            elevated: false,
            clipboard: String::new(),
            shell_output: (0, String::new()),
            foreground_ok: true,
            calls: Mutex::new(Recorded::default()),
        }
    }

    /// Set the text `clipboard_get` will return.
    pub fn set_clipboard(&mut self, text: impl Into<String>) {
        self.clipboard = text.into();
    }

    /// Set the `(exit_code, stdout)` that `run_shell` will return.
    pub fn set_shell_output(&mut self, exit_code: i32, stdout: impl Into<String>) {
        self.shell_output = (exit_code, stdout.into());
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

    /// Targets passed to [`AccessibilityBackend::launch`], in call order.
    pub fn launched(&self) -> Vec<String> {
        self.calls().launched.clone()
    }

    /// `(x, y)` coordinates passed to [`AccessibilityBackend::click_point`].
    pub fn clicks(&self) -> Vec<(i32, i32)> {
        self.calls().clicks.clone()
    }

    /// URIs passed to [`AccessibilityBackend::open_uri`], in call order.
    pub fn opened_uris(&self) -> Vec<String> {
        self.calls().opened_uris.clone()
    }

    /// `(command, shell)` pairs passed to [`AccessibilityBackend::run_shell`].
    pub fn ran_shells(&self) -> Vec<(String, String)> {
        self.calls().ran_shells.clone()
    }

    /// App names passed to [`AccessibilityBackend::focus_app`], in call order.
    pub fn focused_apps(&self) -> Vec<String> {
        self.calls().focused_apps.clone()
    }

    /// Text values passed to [`AccessibilityBackend::clipboard_set`], in order.
    pub fn clipboard_sets(&self) -> Vec<String> {
        self.calls().clipboard_sets.clone()
    }

    /// `(dx, dy)` wheel-notch pairs passed to [`AccessibilityBackend::scroll`],
    /// in call order.
    pub fn scrolls(&self) -> Vec<(i32, i32)> {
        self.calls().scrolls.clone()
    }

    /// Targets passed to [`AccessibilityBackend::scroll_into_view`], in call order.
    pub fn scrolled_into_view(&self) -> Vec<ElementPath> {
        self.calls().scrolled_into_view.clone()
    }
}

/// Builder that seeds the served [`Snapshot`] and the `elevated` flag.
#[derive(Debug, Default)]
pub struct MockBackendBuilder {
    snapshot: Snapshot,
    elevated: bool,
    clipboard: String,
    shell_output: (i32, String),
    /// `None` builds a backend whose foreground guard passes (the common case);
    /// `Some(false)` models "no valid target window is foreground".
    foreground_ok: Option<bool>,
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

    /// Seed the text returned by `clipboard_get`.
    pub fn clipboard(mut self, text: impl Into<String>) -> Self {
        self.clipboard = text.into();
        self
    }

    /// Seed the `(exit_code, stdout)` returned by `run_shell`.
    pub fn shell_output(mut self, exit_code: i32, stdout: impl Into<String>) -> Self {
        self.shell_output = (exit_code, stdout.into());
        self
    }

    /// Seed the value returned by `ensure_foreground`. `false` models "our own
    /// console / the wrong app is in front with no valid target to switch to".
    pub fn foreground_ok(mut self, ok: bool) -> Self {
        self.foreground_ok = Some(ok);
        self
    }

    /// Finish building the backend.
    pub fn build(self) -> MockBackend {
        MockBackend {
            snapshot: self.snapshot,
            elevated: self.elevated,
            clipboard: self.clipboard,
            shell_output: self.shell_output,
            foreground_ok: self.foreground_ok.unwrap_or(true),
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

    async fn ensure_foreground(&self, _app_hint: Option<&str>) -> Result<bool, AppError> {
        Ok(self.foreground_ok)
    }

    async fn click_point(&self, x: i32, y: i32) -> Result<(), AppError> {
        self.calls().clicks.push((x, y));
        Ok(())
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

    async fn launch(&self, target: &str) -> Result<(), AppError> {
        self.calls().launched.push(target.to_string());
        Ok(())
    }

    async fn open_uri(&self, uri: &str) -> Result<(), AppError> {
        self.calls().opened_uris.push(uri.to_string());
        Ok(())
    }

    async fn run_shell(&self, command: &str, shell: &str) -> Result<ShellOutput, AppError> {
        self.calls()
            .ran_shells
            .push((command.to_string(), shell.to_string()));
        Ok(ShellOutput {
            exit_code: self.shell_output.0,
            stdout: self.shell_output.1.clone(),
        })
    }

    async fn focus_app(&self, name: &str) -> Result<bool, AppError> {
        self.calls().focused_apps.push(name.to_string());
        Ok(true)
    }

    async fn clipboard_get(&self) -> Result<String, AppError> {
        Ok(self.clipboard.clone())
    }

    async fn clipboard_set(&self, text: &str) -> Result<(), AppError> {
        self.calls().clipboard_sets.push(text.to_string());
        Ok(())
    }

    async fn scroll(&self, dx: i32, dy: i32) -> Result<(), AppError> {
        self.calls().scrolls.push((dx, dy));
        Ok(())
    }

    async fn scroll_into_view(&self, target: &ElementPath) -> Result<(), AppError> {
        self.calls().scrolled_into_view.push(target.clone());
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
    async fn records_script_primitive_calls() {
        let backend = MockBackend::default();

        backend.launch("spotify").await.unwrap();
        backend.open_uri("https://example.com").await.unwrap();
        backend.run_shell("ipconfig", "cmd").await.unwrap();
        backend.focus_app("Chrome").await.unwrap();
        backend.clipboard_set("hello").await.unwrap();

        assert_eq!(backend.launched(), vec!["spotify".to_string()]);
        assert_eq!(
            backend.opened_uris(),
            vec!["https://example.com".to_string()]
        );
        assert_eq!(
            backend.ran_shells(),
            vec![("ipconfig".to_string(), "cmd".to_string())]
        );
        assert_eq!(backend.focused_apps(), vec!["Chrome".to_string()]);
        assert_eq!(backend.clipboard_sets(), vec!["hello".to_string()]);
    }

    #[tokio::test]
    async fn seeded_clipboard_and_shell_output_are_returned() {
        let backend = MockBackend::builder()
            .clipboard("copied text")
            .shell_output(3, "some stdout")
            .build();

        assert_eq!(backend.clipboard_get().await.unwrap(), "copied text");
        let out = backend.run_shell("whoami", "powershell").await.unwrap();
        assert_eq!(out.exit_code, 3);
        assert_eq!(out.stdout, "some stdout");
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
        assert!(backend.launched().is_empty());
        assert!(backend.opened_uris().is_empty());
        assert!(backend.ran_shells().is_empty());
        assert!(backend.focused_apps().is_empty());
        assert!(backend.clipboard_sets().is_empty());
        assert!(backend.scrolls().is_empty());
        assert!(backend.scrolled_into_view().is_empty());
        assert_eq!(backend.clipboard_get().await.unwrap(), "");
    }

    #[tokio::test]
    async fn records_scroll_and_scroll_into_view_calls() {
        let backend = MockBackend::default();
        backend.scroll(0, 3).await.unwrap();
        backend.scroll(0, -1).await.unwrap();
        backend
            .scroll_into_view(&"#/2/1".to_string())
            .await
            .unwrap();

        assert_eq!(backend.scrolls(), vec![(0, 3), (0, -1)]);
        assert_eq!(backend.scrolled_into_view(), vec!["#/2/1".to_string()]);
    }
}
