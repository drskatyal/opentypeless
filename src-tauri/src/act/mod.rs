//! Act mode — voice-operated, capability-sandboxed OS automation.
//!
//! Phase 0 foundation. This subsystem is built and tested in isolation and is
//! NOT yet wired into the live dictation pipeline, so it cannot destabilize
//! Transcribe mode. Windows is the first platform; macOS AX follows.
//!
//! The design and the locked decisions live in
//! `docs/flowrad-act-architecture.md`.

pub mod action;
pub mod audit;
pub mod backend;
pub mod capability;
pub mod element;
pub mod fastpath;
pub mod grounding;
pub mod killswitch;
pub mod mock_backend;

#[cfg(windows)]
pub mod windows;
