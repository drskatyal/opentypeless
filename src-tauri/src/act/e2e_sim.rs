//! End-to-end *simulation* of the Conductor over the real seed drawer.
//!
//! This is as close to a Windows E2E as we can get without a real UIA desktop:
//! it drives the fully-assembled stack — selection → mission loop → flow runner
//! → capability gate → backend — through the flagship user scenarios, over a
//! [`MockBackend`] primed with realistic snapshots (a Spotify results list, a
//! browser, a file window) and canned selection calls. It asserts the exact OS
//! primitives the stack would issue (URIs opened, keys pressed, rows invoked),
//! so a regression in any layer shows up here. Real-hardware validation still
//! needs the user's Windows machine; the companion `windows.rs` test checks the
//! seeds' key combos against the actual Windows translator on the Windows runner.

#![cfg(test)]

use std::sync::Arc;

use super::backend::AccessibilityBackend;
use super::capability::{Capability, CapabilityGate};
use super::conductor::{Conductor, ConductorState};
use super::element::{ActionPattern, Bounds, Role, Snapshot, UiElement};
use super::events::ActEvent;
use super::executor::{Executor, UserDecision};
use super::flow::{
    FlowFile, FlowKind, FlowStatus, FlowStep, OnFail, PickFallback, PickSpec, Selector,
};
use super::flow_registry::FlowRegistry;
use super::flow_runner::FlowRunner;
use super::killswitch::KillSwitch;
use super::llm::test_support::FixtureLlmClient;
use super::mock_backend::MockBackend;
use super::planner::Planner;

fn el(path: &str, role: Role, name: &str) -> UiElement {
    UiElement {
        path: path.into(),
        role,
        name: name.into(),
        description: String::new(),
        value_len: 0,
        states: vec![],
        bounds: Some(Bounds {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        }),
        patterns: vec![ActionPattern::Invoke, ActionPattern::SetValue],
    }
}

fn scene(app: &str, title: &str, elements: Vec<UiElement>) -> Snapshot {
    Snapshot {
        app: app.into(),
        window_title: title.into(),
        focused: None,
        pointer: None,
        selection_text_len: 0,
        elements,
    }
}

/// Build a Conductor over the real seed drawer plus any extra flows, with the
/// same gate the live app uses (AppLaunch + NetNavigate granted; shell stays
/// confirm), a fixture LLM returning `responses` in order, and `backend`.
fn sim_conductor(
    extra: Vec<FlowFile>,
    responses: Vec<Result<String, crate::error::AppError>>,
    backend: Arc<MockBackend>,
) -> Conductor {
    let llm = Arc::new(FixtureLlmClient::new(responses));
    let mut gate = CapabilityGate::new();
    gate.grant(Capability::AppLaunch);
    gate.grant(Capability::NetNavigate);

    let mut flows: Vec<FlowFile> = super::seed::builtin_flows();
    flows.extend(extra);
    let registry = FlowRegistry::from_files(flows);

    let runner = FlowRunner::new(
        backend.clone() as Arc<dyn AccessibilityBackend>,
        gate.clone(),
        KillSwitch::new(),
    );
    let planner = Planner::new(llm.clone(), "fast".into());
    let executor = Executor::new(
        backend.clone() as Arc<dyn AccessibilityBackend>,
        gate,
        None::<crate::act::audit::AuditLog>,
        KillSwitch::new(),
    );
    Conductor::new(
        registry,
        llm,
        runner,
        planner,
        executor,
        backend as Arc<dyn AccessibilityBackend>,
    )
}

fn ok(json: &str) -> Result<String, crate::error::AppError> {
    Ok(json.to_string())
}

#[tokio::test]
async fn scenario_open_a_folder_then_copy() {
    // "open my downloads and copy this" -> two deterministic leaves in order.
    let backend = Arc::new(MockBackend::new(scene("Explorer", "Downloads", vec![])));
    let responses = vec![ok(r#"{"missions":[
            {"type":"open_flow","id":"open_downloads","slots":{}},
            {"type":"open_flow","id":"copy","slots":{}}
        ]}"#)];
    let mut c = sim_conductor(vec![], responses, backend.clone());
    c.arm();

    let events = c
        .on_transcript("open my downloads and copy this".into())
        .await
        .unwrap();
    assert_eq!(backend.opened_uris(), vec!["shell:Downloads"]);
    assert_eq!(backend.keys(), vec!["ctrl+c".to_string()]);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, ActEvent::Result { ok: true, .. }))
            .count(),
        2
    );
    assert!(matches!(c.state(), ConductorState::Armed));
}

#[tokio::test]
async fn scenario_web_search_is_url_encoded() {
    // "search google for cheap flights to tokyo" -> a urlencoded query in the URL.
    let backend = Arc::new(MockBackend::new(scene("Chrome", "New Tab", vec![])));
    let responses = vec![ok(
        r#"{"missions":[{"type":"open_flow","id":"google_search","slots":{"query":"cheap flights to tokyo"}}]}"#,
    )];
    let mut c = sim_conductor(vec![], responses, backend.clone());
    c.arm();

    c.on_transcript("search google for cheap flights to tokyo".into())
        .await
        .unwrap();
    assert_eq!(
        backend.opened_uris(),
        vec!["https://www.google.com/search?q=cheap%20flights%20to%20tokyo"]
    );
}

#[tokio::test]
async fn scenario_open_app_launches_by_name() {
    let backend = Arc::new(MockBackend::new(scene("Desktop", "", vec![])));
    let responses = vec![ok(
        r#"{"missions":[{"type":"open_flow","id":"open_app","slots":{"app":"Spotify"}}]}"#,
    )];
    let mut c = sim_conductor(vec![], responses, backend.clone());
    c.arm();

    c.on_transcript("open spotify".into()).await.unwrap();
    assert_eq!(backend.launched(), vec!["Spotify".to_string()]);
}

#[tokio::test]
async fn scenario_pick_a_search_result_over_a_realistic_list() {
    // A Spotify-like results list; a custom pick flow selects the matching row and
    // rejects the sponsored one. Simulates the in-app selection a branch's planner
    // would drive, but deterministically here.
    let backend = Arc::new(MockBackend::new(scene(
        "Spotify",
        "Search",
        vec![
            el(
                "#/1",
                Role::ListItem,
                "Sponsored — Hotel California ringtone",
            ),
            el("#/2", Role::ListItem, "Hotel California — Eagles"),
            el("#/3", Role::ListItem, "Take It Easy — Eagles"),
        ],
    )));
    let pick_flow = FlowFile {
        id: "play_top_result".into(),
        name: "Play the top result".into(),
        description: "play the best-matching search result".into(),
        aliases: vec![],
        kind: FlowKind::Leaf,
        app_scope: vec![],
        preconditions: vec![],
        slots: vec![super::flow::Slot {
            name: "song".into(),
            kind: "query".into(),
            required: true,
            examples: vec![],
            default: None,
            filters: vec![],
        }],
        steps: vec![FlowStep {
            id: "s1".into(),
            intent: "play the result".into(),
            action: "pick_result".into(),
            target: Some(Selector {
                role: Some("list_item".into()),
                ..Default::default()
            }),
            value: None,
            pick: Some(PickSpec {
                match_terms: vec!["{song}".into()],
                negative_patterns: vec!["Sponsored".into()],
                min_score: 0.5,
                tie_margin: 0.15,
                if_none: PickFallback::Fail,
                if_ambiguous: PickFallback::Fail,
            }),
            bind: None,
            wait_before: None,
            postcondition: None,
            on_fail: OnFail::Abort,
        }],
        branch_context: None,
        verify: None,
        status: FlowStatus::Smoke,
        version: 1,
        health: Default::default(),
    };
    let responses = vec![ok(
        r#"{"missions":[{"type":"open_flow","id":"play_top_result","slots":{"song":"Hotel California"}}]}"#,
    )];
    let mut c = sim_conductor(vec![pick_flow], responses, backend.clone());
    c.arm();

    let events = c
        .on_transcript("play hotel california".into())
        .await
        .unwrap();
    assert_eq!(
        backend.invoked(),
        vec!["#/2".to_string()],
        "the real result, not the ad"
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));
}

#[tokio::test]
async fn scenario_shell_flow_pauses_for_confirm_then_resumes() {
    // A shell primitive is Confirm-always. A custom flow issues one; the Conductor
    // pauses, the user approves, and it completes — the safety pause end to end.
    let backend = Arc::new(MockBackend::new(scene("Desktop", "", vec![])));
    let shell_flow = FlowFile {
        id: "flush_dns".into(),
        name: "Flush DNS".into(),
        description: "flush the DNS resolver cache".into(),
        aliases: vec![],
        kind: FlowKind::Leaf,
        app_scope: vec![],
        preconditions: vec![],
        slots: vec![],
        steps: vec![FlowStep {
            id: "s1".into(),
            intent: "flush dns".into(),
            action: "launch".into(),
            target: None,
            // Launch is granted in sim_conductor, so force a pause via a shell-less
            // stand-in: use a value that still needs confirm? Launch is granted, so
            // use `focus_app` (granted) won't pause. Use a fresh gate below instead.
            value: Some("ipconfig".into()),
            pick: None,
            bind: None,
            wait_before: None,
            postcondition: None,
            on_fail: OnFail::Abort,
        }],
        branch_context: None,
        verify: None,
        status: FlowStatus::Smoke,
        version: 1,
        health: Default::default(),
    };
    // Build a conductor whose gate does NOT grant AppLaunch, so `launch` pauses.
    let llm = Arc::new(FixtureLlmClient::new(vec![ok(
        r#"{"missions":[{"type":"open_flow","id":"flush_dns","slots":{}}]}"#,
    )]));
    let mut flows = super::seed::builtin_flows();
    flows.push(shell_flow);
    let registry = FlowRegistry::from_files(flows);
    let runner = FlowRunner::new(
        backend.clone() as Arc<dyn AccessibilityBackend>,
        CapabilityGate::new(),
        KillSwitch::new(),
    );
    let planner = Planner::new(llm.clone(), "fast".into());
    let executor = Executor::new(
        backend.clone() as Arc<dyn AccessibilityBackend>,
        CapabilityGate::new(),
        None::<crate::act::audit::AuditLog>,
        KillSwitch::new(),
    );
    let mut c = Conductor::new(
        registry,
        llm,
        runner,
        planner,
        executor,
        backend.clone() as Arc<dyn AccessibilityBackend>,
    );
    c.arm();

    let events = c.on_transcript("flush my dns".into()).await.unwrap();
    assert!(events.iter().any(|e| matches!(e, ActEvent::Confirm { .. })));
    assert!(matches!(c.state(), ConductorState::AwaitingConfirm));
    assert!(
        backend.launched().is_empty(),
        "nothing runs before approval"
    );

    let after = c.decide(UserDecision::ConfirmAllow).await.unwrap();
    assert_eq!(backend.launched(), vec!["ipconfig".to_string()]);
    assert!(after
        .iter()
        .any(|e| matches!(e, ActEvent::Result { ok: true, .. })));
    assert!(matches!(c.state(), ConductorState::Armed));
}
