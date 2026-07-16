//! The Act kill switch — a global abort the agent itself can never steer.
//!
//! TODO(act-phase0): wire to a global hotkey, cancel in-flight actions, release
//! held modifiers, and guarantee a <100ms abort. Stub only.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A cheap, clonable abort flag shared with the executor.
#[derive(Debug, Clone, Default)]
pub struct KillSwitch {
    aborted: Arc<AtomicBool>,
}

impl KillSwitch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn trip(&self) {
        self.aborted.store(true, Ordering::SeqCst);
    }

    pub fn is_tripped(&self) -> bool {
        self.aborted.load(Ordering::SeqCst)
    }

    pub fn reset(&self) {
        self.aborted.store(false, Ordering::SeqCst);
    }
}
