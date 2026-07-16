//! The drawer runner — replays a saved [`FlowFile`] over an
//! [`AccessibilityBackend`], zero further model calls for a leaf.
//!
//! Opening a leaf file yields a deterministic recipe: run each [`FlowStep`] in
//! order, re-resolving every target against a FRESH snapshot by *semantic
//! selector set* (role + name synonyms + automation id + required patterns), never
//! a pixel or a cached path, so a flow survives the window moving or minor UI
//! drift. A branch file is not run — its `branch_context` is surfaced back to the
//! caller, which loops the planner.
//!
//! Per step: kill-switch check -> progress label -> `wait_before` predicate poll
//! -> selector resolution -> capability gate -> backend execution (raced against
//! the kill switch) -> `postcondition` check honoring `on_fail`. After the steps,
//! the flow's objective `verify` decides whether the run truly succeeded.
//!
//! The gate is the real safety boundary: a `Confirm` ruling pauses the run and is
//! surfaced as [`FlowOutcome::NeedsConfirm`] for the session to resolve, exactly
//! like the executor.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::backend::AccessibilityBackend;
use super::capability::{Capability, CapabilityGate, Decision};
use super::element::{ActionPattern, ElementPath, ElementState, Role, Snapshot, UiElement};
use super::events::ActEvent;
use super::flow::{FlowFile, FlowKind, FlowStep, OnFail, Selector, VerifySpec, WaitSpec};
use super::killswitch::KillSwitch;

/// The score an element must reach to be accepted as a selector's target. A single
/// role match (or a single `name_contains` hit) clears it; noise scores zero.
const RESOLVE_THRESHOLD: i32 = 3;

/// How often the wait/verify pollers re-snapshot while waiting on a UI condition.
const POLL_INTERVAL: Duration = Duration::from_millis(40);

/// A short settle before a `RetryOnce` re-attempt, so a transient mid-drift UI can
/// stabilize before we act again.
const STABLE_WAIT: Duration = Duration::from_millis(150);

/// The upper bound on any single wait/verify, so a bad predicate can't hang a run.
const MAX_WAIT: Duration = Duration::from_secs(15);

/// The terminal result of replaying one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowOutcome {
    /// Every step ran; `verified` reflects the objective `verify` (true when the
    /// file has no verify to run against).
    Done { verified: bool },
    /// A step's capability ruled `Confirm`; the session pauses for the user and
    /// resumes the flow.
    NeedsConfirm { reason: String },
    /// A branch file was opened — its context (and the live slots) go back to the
    /// planner, which reasons and loops.
    Branch {
        context: String,
        slots: HashMap<String, String>,
    },
    /// A step failed (target gone, backend error, unmet postcondition, or a
    /// `Replan` bail). `step` is the step id; `error` is a PHI-free reason.
    Failed { step: String, error: String },
    /// The kill switch tripped mid-run.
    Aborted,
}

/// A hard runner failure (a backend the run cannot proceed without), distinct from
/// a per-flow [`FlowOutcome`]. Step-level failures are reported as
/// [`FlowOutcome::Failed`], not as this error.
#[derive(Debug)]
pub enum FlowRunError {
    /// The backend failed a call the runner depends on (e.g. taking a snapshot).
    Backend(String),
}

impl std::fmt::Display for FlowRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlowRunError::Backend(m) => write!(f, "flow runner backend error: {m}"),
        }
    }
}
impl std::error::Error for FlowRunError {}

/// The result of a single step attempt (resolve + gate + execute), before its
/// postcondition is judged.
enum Attempt {
    /// The backend call ran successfully.
    Ran,
    /// The capability gate ruled `Confirm`; pause for the user.
    Confirm(String),
    /// The kill switch tripped.
    Aborted,
    /// The step could not run (target gone, denied, backend error, missing value).
    Failed(String),
}

/// Where the step loop lands after fully handling one step (attempt + on_fail).
enum StepFlow {
    /// The step completed; go to the next one.
    Next,
    /// Pause the whole flow for a confirmation.
    Confirm(String),
    /// Abort the whole flow.
    Aborted,
    /// Stop the flow with a failure.
    Failed(String),
}

/// Replays saved flows over a backend, enforcing the capability gate and kill
/// switch. Separate from the executor, but shares its safety discipline.
pub struct FlowRunner {
    backend: Arc<dyn AccessibilityBackend>,
    gate: CapabilityGate,
    kill: KillSwitch,
}

impl FlowRunner {
    pub fn new(
        backend: Arc<dyn AccessibilityBackend>,
        gate: CapabilityGate,
        kill: KillSwitch,
    ) -> Self {
        Self {
            backend,
            gate,
            kill,
        }
    }

    /// The kill switch this runner races every backend await against.
    pub fn kill_switch(&self) -> KillSwitch {
        self.kill.clone()
    }

    /// Replay `file` with `slots` filled, emitting progress via `emit`.
    ///
    /// A branch file returns [`FlowOutcome::Branch`] immediately (the caller loops
    /// the planner). A leaf file runs its steps, then its objective `verify`.
    pub async fn run(
        &self,
        file: &FlowFile,
        slots: &HashMap<String, String>,
        emit: &impl Fn(ActEvent),
    ) -> Result<FlowOutcome, FlowRunError> {
        emit(ActEvent::Step {
            label: format!("opening {}", file.id),
        });

        if file.kind == FlowKind::Branch {
            return Ok(FlowOutcome::Branch {
                context: file.branch_context.clone().unwrap_or_default(),
                slots: slots.clone(),
            });
        }

        for step in &file.steps {
            // a. The agent can never steer past a trip.
            if self.kill.is_tripped() {
                return Ok(FlowOutcome::Aborted);
            }

            // b. A live, PHI-free progress line (intent when authored, else verb).
            let label = if step.intent.is_empty() {
                step.action.clone()
            } else {
                step.intent.clone()
            };
            emit(ActEvent::Step { label });

            // c. Wait on the UI condition (never a fixed sleep). A timed-out wait is
            //    not itself fatal — resolution below is the real gate.
            if let Some(wait) = &step.wait_before {
                self.wait_for(wait, slots).await;
            }

            match self.run_step(step, slots).await? {
                StepFlow::Next => continue,
                StepFlow::Confirm(reason) => return Ok(FlowOutcome::NeedsConfirm { reason }),
                StepFlow::Aborted => return Ok(FlowOutcome::Aborted),
                StepFlow::Failed(error) => {
                    return Ok(FlowOutcome::Failed {
                        step: step.id.clone(),
                        error,
                    })
                }
            }
        }

        // Objective verification: did the outcome actually happen? A file with no
        // verify is taken at face value.
        let verified = match &file.verify {
            Some(spec) => self.verify(spec, slots).await?,
            None => true,
        };
        Ok(FlowOutcome::Done { verified })
    }

    /// Run one step end to end: attempt it, judge its postcondition, and honor
    /// `on_fail` on any failure.
    async fn run_step(
        &self,
        step: &FlowStep,
        slots: &HashMap<String, String>,
    ) -> Result<StepFlow, FlowRunError> {
        // First attempt.
        let failure = match self.attempt(step, slots).await? {
            Attempt::Ran => self.postcondition_failure(step, slots).await,
            Attempt::Confirm(reason) => return Ok(StepFlow::Confirm(reason)),
            Attempt::Aborted => return Ok(StepFlow::Aborted),
            Attempt::Failed(error) => Some(error),
        };

        let Some(reason) = failure else {
            return Ok(StepFlow::Next);
        };

        // The step failed — honor its recovery policy.
        match step.on_fail {
            OnFail::Abort => Ok(StepFlow::Failed(reason)),
            // Replan bails out of the flow; the session replans the remaining goal.
            OnFail::Replan => Ok(StepFlow::Failed(format!("replan: {reason}"))),
            OnFail::RetryOnce => {
                // Let a mid-drift UI settle, then re-run the whole step once.
                if self.sleep_or_abort(STABLE_WAIT).await {
                    return Ok(StepFlow::Aborted);
                }
                match self.attempt(step, slots).await? {
                    Attempt::Ran => match self.postcondition_failure(step, slots).await {
                        Some(again) => Ok(StepFlow::Failed(again)),
                        None => Ok(StepFlow::Next),
                    },
                    Attempt::Confirm(r) => Ok(StepFlow::Confirm(r)),
                    Attempt::Aborted => Ok(StepFlow::Aborted),
                    Attempt::Failed(again) => Ok(StepFlow::Failed(again)),
                }
            }
        }
    }

    /// One attempt at a step: resolve its target against a fresh snapshot, gate the
    /// mapped capability, then run the backend primitive (raced against the kill
    /// switch). Does NOT judge the postcondition.
    async fn attempt(
        &self,
        step: &FlowStep,
        slots: &HashMap<String, String>,
    ) -> Result<Attempt, FlowRunError> {
        let action = step.action.as_str();

        // d. Resolve the target for the actions that address a control.
        let target: Option<ElementPath> = if let Some(sel) = &step.target {
            let snapshot = self.snapshot().await?;
            match self.resolve_selector(&snapshot, sel, slots) {
                Some(path) => Some(path),
                None => {
                    return Ok(Attempt::Failed(format!("no target for step {}", step.id)));
                }
            }
        } else {
            None
        };

        // e. Capability gate. Confirm pauses; Deny fails the step.
        match self.gate.evaluate_capability(capability_for(action)) {
            Decision::Allow => {}
            Decision::Confirm => {
                let reason = format!("{action} needs confirmation");
                return Ok(Attempt::Confirm(reason));
            }
            Decision::Deny => {
                return Ok(Attempt::Failed(format!("{action} denied by policy")));
            }
        }

        // f. Execute, racing the kill switch on every backend await.
        self.execute(step, target.as_deref(), slots).await
    }

    /// Perform the backend primitive for `action`. Slot tokens in `value` are
    /// substituted first. Every await is raced against the kill switch.
    async fn execute(
        &self,
        step: &FlowStep,
        target: Option<&str>,
        slots: &HashMap<String, String>,
    ) -> Result<Attempt, FlowRunError> {
        let action = step.action.as_str();
        let value = step.value.as_deref().map(|v| substitute(v, slots));

        macro_rules! guarded {
            ($fut:expr) => {
                tokio::select! {
                    biased;
                    _ = self.kill.wait_tripped() => return Ok(Attempt::Aborted),
                    res = $fut => res,
                }
            };
        }

        let result: Result<(), String> = match action {
            "launch" => match value {
                Some(v) => guarded!(self.backend.launch(&v)).map_err(|e| e.to_string()),
                None => return Ok(Attempt::Failed("launch step missing value".into())),
            },
            "uri" => match value {
                Some(v) => guarded!(self.backend.open_uri(&v)).map_err(|e| e.to_string()),
                None => return Ok(Attempt::Failed("uri step missing value".into())),
            },
            "focus_app" => match value {
                Some(v) => match guarded!(self.backend.focus_app(&v)) {
                    Ok(true) => Ok(()),
                    Ok(false) => Err(format!("could not focus \"{v}\"")),
                    Err(e) => Err(e.to_string()),
                },
                None => return Ok(Attempt::Failed("focus_app step missing value".into())),
            },
            "key" => match value {
                Some(v) => guarded!(self.backend.key_combo(&v)).map_err(|e| e.to_string()),
                None => return Ok(Attempt::Failed("key step missing value".into())),
            },
            "focus" => match target {
                Some(path) => {
                    let p = path.to_string();
                    guarded!(self.backend.focus(&p)).map_err(|e| e.to_string())
                }
                None => return Ok(Attempt::Failed("focus step has no resolved target".into())),
            },
            "set_value" => match (target, value) {
                (Some(path), Some(v)) => {
                    let p = path.to_string();
                    guarded!(self.backend.set_value(&p, &v)).map_err(|e| e.to_string())
                }
                (None, _) => return Ok(Attempt::Failed("set_value has no resolved target".into())),
                (_, None) => return Ok(Attempt::Failed("set_value step missing value".into())),
            },
            "invoke" | "pick_result" => match target {
                Some(path) => {
                    let p = path.to_string();
                    guarded!(self.backend.invoke(&p)).map_err(|e| e.to_string())
                }
                None => return Ok(Attempt::Failed(format!("{action} has no resolved target"))),
            },
            // A bare wait resolves entirely in `wait_before`; there is nothing to do.
            "wait" => Ok(()),
            other => return Ok(Attempt::Failed(format!("unknown action \"{other}\""))),
        };

        Ok(match result {
            Ok(()) => Attempt::Ran,
            Err(e) => Attempt::Failed(e),
        })
    }

    /// Judge a step's postcondition. Returns `Some(reason)` when the step declared a
    /// postcondition that never became true (the step failed), else `None`.
    async fn postcondition_failure(
        &self,
        step: &FlowStep,
        slots: &HashMap<String, String>,
    ) -> Option<String> {
        let post = step.postcondition.as_ref()?;
        if self.wait_for(post, slots).await {
            None
        } else {
            Some(format!("postcondition \"{}\" not met", post.predicate))
        }
    }

    /// Poll a wait predicate against fresh snapshots until it holds or its timeout
    /// elapses. Returns whether the predicate held. A tripped kill switch ends the
    /// wait early (returning `false`).
    async fn wait_for(&self, spec: &WaitSpec, slots: &HashMap<String, String>) -> bool {
        let budget = Duration::from_millis(spec.timeout_ms as u64).min(MAX_WAIT);
        let deadline = Instant::now() + budget;
        loop {
            if self.kill.is_tripped() {
                return false;
            }
            let Ok(snapshot) = self.snapshot().await else {
                return false;
            };
            if self.predicate_holds(&snapshot, spec, slots) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            if self.sleep_or_abort(POLL_INTERVAL).await {
                return false;
            }
        }
    }

    /// Evaluate a wait predicate over one snapshot.
    fn predicate_holds(
        &self,
        snapshot: &Snapshot,
        spec: &WaitSpec,
        slots: &HashMap<String, String>,
    ) -> bool {
        match spec.predicate.as_str() {
            // The named control exists (and is acceptable) right now.
            "target_exists" | "value_contains" => spec
                .selector
                .as_ref()
                .map(|sel| self.resolve_selector(snapshot, sel, slots).is_some())
                .unwrap_or(true),
            // Result rows have appeared: a matching selector, else any list item.
            "results_present" => match &spec.selector {
                Some(sel) => self.resolve_selector(snapshot, sel, slots).is_some(),
                None => snapshot.elements.iter().any(|e| e.role == Role::ListItem),
            },
            // Unknown predicate: do not block the flow on it.
            _ => true,
        }
    }

    /// Objective verification for the whole flow: every slot-substituted term must
    /// appear in the live snapshot's observable text (control names, descriptions,
    /// window title, app). Polled until it holds or the spec's timeout elapses.
    async fn verify(
        &self,
        spec: &VerifySpec,
        slots: &HashMap<String, String>,
    ) -> Result<bool, FlowRunError> {
        let budget = Duration::from_millis(spec.timeout_ms as u64).min(MAX_WAIT);
        let deadline = Instant::now() + budget;
        loop {
            if self.kill.is_tripped() {
                return Ok(false);
            }
            let snapshot = self.snapshot().await?;
            if verify_terms_present(&snapshot, spec, slots) {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            if self.sleep_or_abort(POLL_INTERVAL).await {
                return Ok(false);
            }
        }
    }

    /// Objective-aware selector resolution: score every element, hard-rejecting the
    /// unusable, and return the highest scorer above [`RESOLVE_THRESHOLD`].
    ///
    /// Scoring (additive, so the most specific control wins):
    /// * role match: `+3`
    /// * exact `name_any` (casefold) hit: `+5`
    /// * `automation_id_any` hit (path / description): `+4`
    /// * slot-substituted `name_contains` substring (casefold): `+4`
    /// * each required pattern the control supports: `+1`
    ///
    /// Hard rejects: password-like fields, disabled controls, offscreen controls.
    fn resolve_selector(
        &self,
        snapshot: &Snapshot,
        sel: &Selector,
        slots: &HashMap<String, String>,
    ) -> Option<ElementPath> {
        let needle = sel
            .name_contains
            .as_ref()
            .map(|c| substitute(c, slots).to_lowercase());

        let mut best: Option<(i32, &UiElement)> = None;
        for element in &snapshot.elements {
            if is_unusable(element) {
                continue;
            }
            let score = score_element(element, sel, needle.as_deref());
            if score < RESOLVE_THRESHOLD {
                continue;
            }
            if best.map(|(b, _)| score > b).unwrap_or(true) {
                best = Some((score, element));
            }
        }
        best.map(|(_, e)| e.path.clone())
    }

    /// Take a fresh snapshot, mapping a backend failure to a runner error.
    async fn snapshot(&self) -> Result<Snapshot, FlowRunError> {
        self.backend
            .snapshot()
            .await
            .map_err(|e| FlowRunError::Backend(e.to_string()))
    }

    /// Sleep `dur`, but wake early if the kill switch trips. Returns whether it was
    /// the kill switch that woke us (i.e. the caller should abort).
    async fn sleep_or_abort(&self, dur: Duration) -> bool {
        tokio::select! {
            biased;
            _ = self.kill.wait_tripped() => true,
            _ = tokio::time::sleep(dur) => false,
        }
    }
}

/// Map a flow step's primitive verb to the single OS capability it needs, so the
/// gate rules on it exactly as it would an equivalent [`super::action::Action`].
fn capability_for(action: &str) -> Capability {
    match action {
        "launch" => Capability::AppLaunch,
        "uri" => Capability::NetNavigate,
        "focus_app" => Capability::WindowManage,
        "key" => Capability::InputKeyboard,
        "focus" | "invoke" | "pick_result" | "set_value" => Capability::A11yInvoke,
        // `wait` (and any unmapped verb) is agent-self pacing — always allowed.
        _ => Capability::AgentSelf,
    }
}

/// Replace every `{slot}` token with its value; leave unknown tokens intact.
pub fn substitute(template: &str, slots: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let key = &after[..close];
                match slots.get(key) {
                    Some(value) => out.push_str(value),
                    // Unknown token — leave it verbatim.
                    None => {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            // No closing brace — the remainder is literal.
            None => {
                out.push_str(&rest[open..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Whether an element must never be resolved as a target.
fn is_unusable(element: &UiElement) -> bool {
    element.has_state(ElementState::Disabled)
        || element.has_state(ElementState::Offscreen)
        || looks_like_password(element)
}

/// The element model carries no secret flag, so a password field is recognized
/// heuristically by its label. Conservative — a wrong reject just means the
/// selector finds nothing and the step fails safe rather than typing into a secret.
fn looks_like_password(element: &UiElement) -> bool {
    let name = element.name.to_lowercase();
    let desc = element.description.to_lowercase();
    name.contains("password") || desc.contains("password")
}

/// Objective-aware score for one candidate against a selector.
fn score_element(element: &UiElement, sel: &Selector, needle: Option<&str>) -> i32 {
    let mut score = 0;
    let name_lc = element.name.to_lowercase();

    if let Some(role) = &sel.role {
        if role_matches(role, element.role) {
            score += 3;
        }
    }

    for candidate in &sel.name_any {
        if name_lc == candidate.to_lowercase() {
            score += 5;
            break;
        }
    }

    // The element model has no dedicated automation-id field; match the id against
    // the stable path and the (localization-free) description as a best effort.
    let path_lc = element.path.to_lowercase();
    let desc_lc = element.description.to_lowercase();
    for aid in &sel.automation_id_any {
        let aid_lc = aid.to_lowercase();
        if path_lc == aid_lc || desc_lc == aid_lc {
            score += 4;
            break;
        }
    }

    if let Some(needle) = needle {
        if !needle.is_empty() && name_lc.contains(needle) {
            score += 4;
        }
    }

    for pattern in &sel.patterns {
        if let Some(p) = pattern_from_str(pattern) {
            if element.patterns.contains(&p) {
                score += 1;
            }
        }
    }

    score
}

/// Whether a selector's role string (e.g. `edit`, `listitem`) names this role.
fn role_matches(sel_role: &str, role: Role) -> bool {
    let s = sel_role.trim().to_lowercase().replace([' ', '-'], "_");
    let aliases: &[&str] = match role {
        Role::Button => &["button", "push_button"],
        Role::TextField => &[
            "text_field",
            "textfield",
            "edit",
            "textbox",
            "text_box",
            "input",
            "searchbox",
            "search_box",
            "search",
        ],
        Role::CheckBox => &["check_box", "checkbox"],
        Role::RadioButton => &["radio_button", "radiobutton", "radio"],
        Role::ComboBox => &["combo_box", "combobox", "dropdown"],
        Role::List => &["list", "listbox", "list_box"],
        Role::ListItem => &["list_item", "listitem", "item", "option", "result"],
        Role::Menu => &["menu"],
        Role::MenuBar => &["menu_bar", "menubar"],
        Role::MenuItem => &["menu_item", "menuitem"],
        Role::Tab => &["tab"],
        Role::TabItem => &["tab_item", "tabitem"],
        Role::Link => &["link", "hyperlink"],
        Role::Window => &["window"],
        Role::Pane => &["pane"],
        Role::Group => &["group"],
        Role::Text => &["text", "label", "static"],
        Role::Image => &["image", "img"],
        Role::Slider => &["slider"],
        Role::Spinner => &["spinner"],
        Role::ProgressBar => &["progress_bar", "progressbar"],
        Role::ScrollBar => &["scroll_bar", "scrollbar"],
        Role::Toolbar => &["toolbar"],
        Role::TitleBar => &["title_bar", "titlebar"],
        Role::Separator => &["separator"],
        Role::Tree => &["tree"],
        Role::TreeItem => &["tree_item", "treeitem"],
        Role::Table => &["table", "grid"],
        Role::Row => &["row"],
        Role::Cell => &["cell", "grid_cell", "gridcell"],
        Role::Document => &["document"],
        Role::Unknown => &["unknown"],
    };
    aliases.contains(&s.as_str())
}

/// Map a selector's pattern string to an [`ActionPattern`].
fn pattern_from_str(pattern: &str) -> Option<ActionPattern> {
    match pattern
        .trim()
        .to_lowercase()
        .replace([' ', '-'], "_")
        .as_str()
    {
        "invoke" => Some(ActionPattern::Invoke),
        "value" | "set_value" | "setvalue" => Some(ActionPattern::SetValue),
        "toggle" => Some(ActionPattern::Toggle),
        "select" => Some(ActionPattern::Select),
        "expand" => Some(ActionPattern::Expand),
        "scroll" => Some(ActionPattern::Scroll),
        "focus" => Some(ActionPattern::Focus),
        _ => None,
    }
}

/// Whether every slot-substituted verify term appears in the snapshot's observable
/// text. An empty term is vacuously present.
fn verify_terms_present(
    snapshot: &Snapshot,
    spec: &VerifySpec,
    slots: &HashMap<String, String>,
) -> bool {
    let mut haystack = format!("{} {}", snapshot.app, snapshot.window_title).to_lowercase();
    for element in &snapshot.elements {
        haystack.push(' ');
        haystack.push_str(&element.name.to_lowercase());
        haystack.push(' ');
        haystack.push_str(&element.description.to_lowercase());
    }
    spec.terms.iter().all(|term| {
        let sub = substitute(term, slots).to_lowercase();
        sub.is_empty() || haystack.contains(&sub)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::element::Bounds;
    use crate::act::flow::{FlowHealth, FlowStatus};
    use crate::act::mock_backend::MockBackend;

    fn el(path: &str, role: Role, name: &str) -> UiElement {
        UiElement {
            path: path.into(),
            role,
            name: name.into(),
            description: String::new(),
            value_len: 0,
            states: Vec::new(),
            bounds: Some(Bounds {
                x: 0,
                y: 0,
                w: 10,
                h: 10,
            }),
            patterns: vec![ActionPattern::Invoke, ActionPattern::SetValue],
        }
    }

    fn snapshot(elements: Vec<UiElement>) -> Snapshot {
        Snapshot {
            app: "Spotify".into(),
            window_title: "Spotify".into(),
            focused: None,
            pointer: None,
            selection_text_len: 0,
            elements,
        }
    }

    fn base_file() -> FlowFile {
        FlowFile {
            id: "flow".into(),
            name: "flow".into(),
            description: "a flow".into(),
            aliases: vec![],
            kind: FlowKind::Leaf,
            app_scope: vec![],
            preconditions: vec![],
            slots: vec![],
            steps: vec![],
            branch_context: None,
            verify: None,
            status: FlowStatus::Draft,
            version: 1,
            health: FlowHealth::default(),
        }
    }

    fn step(id: &str, action: &str) -> FlowStep {
        FlowStep {
            id: id.into(),
            intent: String::new(),
            action: action.into(),
            target: None,
            value: None,
            wait_before: None,
            postcondition: None,
            on_fail: OnFail::Abort,
        }
    }

    /// A gate that grants the confirm-by-default surfaces (uri/launch) so a saved,
    /// user-authored flow replays without a per-step pause in tests.
    fn open_gate() -> CapabilityGate {
        let mut gate = CapabilityGate::new();
        gate.grant(Capability::NetNavigate);
        gate.grant(Capability::AppLaunch);
        gate
    }

    fn runner(backend: Arc<dyn AccessibilityBackend>, kill: KillSwitch) -> FlowRunner {
        FlowRunner::new(backend, open_gate(), kill)
    }

    fn no_slots() -> HashMap<String, String> {
        HashMap::new()
    }

    fn noop_emit(_: ActEvent) {}

    #[test]
    fn substitute_replaces_known_and_keeps_unknown() {
        let mut slots = HashMap::new();
        slots.insert("song".to_string(), "Hotel California".to_string());
        assert_eq!(substitute("{song}", &slots), "Hotel California");
        assert_eq!(
            substitute("play {song} now", &slots),
            "play Hotel California now"
        );
        assert_eq!(substitute("{unknown}", &slots), "{unknown}");
        assert_eq!(substitute("no tokens", &slots), "no tokens");
        assert_eq!(substitute("dangling {open", &slots), "dangling {open");
    }

    #[tokio::test]
    async fn leaf_uri_step_runs_and_records_the_uri() {
        let backend = Arc::new(MockBackend::new(snapshot(vec![])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut file = base_file();
        let mut s = step("s1", "uri");
        s.value = Some("ms-settings:bluetooth".into());
        file.steps = vec![s];

        let outcome = runner.run(&file, &no_slots(), &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Done { verified: true });
        assert_eq!(
            backend.opened_uris(),
            vec!["ms-settings:bluetooth".to_string()]
        );
    }

    #[tokio::test]
    async fn set_value_resolves_search_field_and_sets_slot_value() {
        let backend = Arc::new(MockBackend::new(snapshot(vec![
            el("#/1", Role::Button, "Home"),
            el("#/2", Role::TextField, "Search"),
        ])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut slots = HashMap::new();
        slots.insert("song".to_string(), "Hotel California".to_string());

        let mut file = base_file();
        let mut s = step("s1", "set_value");
        s.target = Some(Selector {
            role: Some("edit".into()),
            name_any: vec!["Search".into()],
            ..Default::default()
        });
        s.value = Some("{song}".into());
        file.steps = vec![s];

        let outcome = runner.run(&file, &slots, &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Done { verified: true });
        assert_eq!(
            backend.set_values(),
            vec![("#/2".to_string(), "Hotel California".to_string())]
        );
    }

    #[tokio::test]
    async fn pick_result_resolves_listitem_by_name_contains_and_verifies() {
        let backend = Arc::new(MockBackend::new(snapshot(vec![
            el("#/1", Role::ListItem, "Hotel California — Eagles"),
            el("#/2", Role::ListItem, "Take It Easy — Eagles"),
        ])));
        let runner = runner(backend.clone(), KillSwitch::new());

        let mut slots = HashMap::new();
        slots.insert("song".to_string(), "Hotel California".to_string());

        let mut file = base_file();
        let mut s = step("s1", "pick_result");
        s.target = Some(Selector {
            role: Some("listitem".into()),
            name_contains: Some("{song}".into()),
            ..Default::default()
        });
        file.steps = vec![s];
        file.verify = Some(VerifySpec {
            predicate: "now_playing_contains".into(),
            terms: vec!["{song}".into()],
            timeout_ms: 1000,
        });

        let outcome = runner.run(&file, &slots, &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Done { verified: true });
        assert_eq!(backend.invoked(), vec!["#/1".to_string()]);
    }

    #[tokio::test]
    async fn verify_failure_reports_unverified_done() {
        // The term never appears in the snapshot, so the objective verify fails.
        let backend = Arc::new(MockBackend::new(snapshot(vec![el(
            "#/1",
            Role::TextField,
            "Search",
        )])));
        let runner = runner(backend, KillSwitch::new());

        let mut file = base_file();
        file.verify = Some(VerifySpec {
            predicate: "now_playing_contains".into(),
            terms: vec!["Nothing Here".into()],
            timeout_ms: 50,
        });

        let outcome = runner.run(&file, &no_slots(), &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Done { verified: false });
    }

    #[tokio::test]
    async fn missing_target_fails_the_step() {
        // The snapshot has no edit control, so resolution finds nothing.
        let backend = Arc::new(MockBackend::new(snapshot(vec![el(
            "#/1",
            Role::Button,
            "Home",
        )])));
        let runner = runner(backend, KillSwitch::new());

        let mut file = base_file();
        let mut s = step("s1", "set_value");
        s.target = Some(Selector {
            role: Some("edit".into()),
            name_any: vec!["Search".into()],
            ..Default::default()
        });
        s.value = Some("hi".into());
        file.steps = vec![s];

        let outcome = runner.run(&file, &no_slots(), &noop_emit).await.unwrap();
        assert!(
            matches!(outcome, FlowOutcome::Failed { ref step, .. } if step == "s1"),
            "expected Failed for the missing target, got {outcome:?}"
        );
    }

    #[tokio::test]
    async fn tripped_kill_switch_aborts_before_any_step() {
        let backend = Arc::new(MockBackend::new(snapshot(vec![])));
        let kill = KillSwitch::new();
        kill.trip();
        let runner = runner(backend.clone(), kill);

        let mut file = base_file();
        let mut s = step("s1", "uri");
        s.value = Some("ms-settings:bluetooth".into());
        file.steps = vec![s];

        let outcome = runner.run(&file, &no_slots(), &noop_emit).await.unwrap();
        assert_eq!(outcome, FlowOutcome::Aborted);
        assert!(
            backend.opened_uris().is_empty(),
            "no backend call after a trip"
        );
    }

    #[tokio::test]
    async fn branch_file_surfaces_context_and_slots() {
        let backend = Arc::new(MockBackend::new(snapshot(vec![])));
        let runner = runner(backend, KillSwitch::new());

        let mut slots = HashMap::new();
        slots.insert("topic".to_string(), "quarterly report".to_string());

        let mut file = base_file();
        file.kind = FlowKind::Branch;
        file.branch_context = Some("draft an email about the topic".into());

        let outcome = runner.run(&file, &slots, &noop_emit).await.unwrap();
        match outcome {
            FlowOutcome::Branch {
                context,
                slots: out,
            } => {
                assert_eq!(context, "draft an email about the topic");
                assert_eq!(
                    out.get("topic").map(String::as_str),
                    Some("quarterly report")
                );
            }
            other => panic!("expected Branch, got {other:?}"),
        }
    }

    #[test]
    fn resolver_hard_rejects_disabled_and_prefers_specific() {
        let mut disabled = el("#/1", Role::TextField, "Search");
        disabled.states = vec![ElementState::Disabled];
        let enabled = el("#/2", Role::TextField, "Search");
        let snap = snapshot(vec![disabled, enabled]);

        let backend = Arc::new(MockBackend::new(snap.clone()));
        let runner = runner(backend, KillSwitch::new());
        let sel = Selector {
            role: Some("edit".into()),
            name_any: vec!["Search".into()],
            ..Default::default()
        };
        let path = runner.resolve_selector(&snap, &sel, &no_slots());
        assert_eq!(path.as_deref(), Some("#/2"), "disabled control is rejected");
    }

    #[test]
    fn resolver_rejects_password_fields() {
        let secret = el("#/1", Role::TextField, "Password");
        let snap = snapshot(vec![secret.clone()]);
        let backend = Arc::new(MockBackend::new(snap.clone()));
        let runner = runner(backend, KillSwitch::new());
        let sel = Selector {
            role: Some("edit".into()),
            name_contains: Some("pass".into()),
            ..Default::default()
        };
        assert!(
            runner.resolve_selector(&snap, &sel, &no_slots()).is_none(),
            "a password field must never resolve as a target"
        );
    }
}
