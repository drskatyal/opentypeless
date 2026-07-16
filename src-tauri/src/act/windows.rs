//! Windows UI Automation (UIA) accessibility backend.
//!
//! TODO(act-phase0): implement [`AccessibilityBackend`](super::backend) over the
//! `uiautomation` crate — focused-window snapshot (L0+L1), invoke / set-value /
//! focus patterns, elevated-target detection (detect-and-decline), and input
//! synthesis via `enigo`. Stub only: kept dependency-free so the baseline
//! compiles on the Windows CI job before the real backend lands.
