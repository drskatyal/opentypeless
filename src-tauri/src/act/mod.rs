//! Act mode — voice-operated, capability-sandboxed OS automation.
//!
//! Phase 0 foundation. This subsystem is built and tested in isolation and is
//! NOT yet wired into the live dictation pipeline, so it cannot destabilize
//! Transcribe mode. Windows is the first platform; macOS AX follows.
//!
//! The design and the locked decisions live in
//! `docs/flowrad-act-architecture.md`.

pub mod action;
pub mod answer;
pub mod audit;
pub mod backend;
pub mod blackboard;
pub mod capability;
pub mod conductor;
pub mod destructive;
#[cfg(test)]
mod e2e_sim;
pub mod element;
pub mod events;
pub mod executor;
pub mod fastpath;
pub mod flow;
pub mod flow_registry;
pub mod flow_runner;
pub mod focus_guard;
pub mod grounding;
pub mod grounding_packet;
pub mod killswitch;
pub mod llm;
pub mod mock_backend;
pub mod plan_mode;
pub mod planner;
pub mod seed;
pub mod selection;
pub mod session;
pub mod shell_policy;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
mod uia_broker;
#[cfg(windows)]
pub mod windows;

use std::sync::Arc;

use backend::AccessibilityBackend;

/// Construct the platform accessibility backend for Act.
///
/// Windows uses the UIA/Terminator backend, macOS the native AX backend; every
/// other platform gets the mock (the command layer refuses to arm there).
pub fn create_backend() -> Arc<dyn AccessibilityBackend> {
    #[cfg(windows)]
    {
        Arc::new(windows::WindowsUiaBackend::new())
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(macos::MacBackend::new())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Arc::new(mock_backend::MockBackend::default())
    }
}

/// Whether Act's accessibility backend is functional on this platform.
pub const fn act_supported() -> bool {
    cfg!(windows) || cfg!(target_os = "macos")
}
