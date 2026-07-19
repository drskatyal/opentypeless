//! The Act kill switch — a global abort the agent itself can never steer.
//!
//! The switch is a shared atomic flag plus a [`tokio::sync::Notify`]: [`trip`]
//! flips the flag and wakes any executor parked in [`wait_tripped`] immediately,
//! so an in-flight plan aborts without polling latency. The agent has no path to
//! reset it (self-control ban); only the host/global-hotkey layer calls
//! [`reset`].
//!
//! [`trip`]: KillSwitch::trip
//! [`reset`]: KillSwitch::reset
//! [`wait_tripped`]: KillSwitch::wait_tripped

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::Notify;

#[derive(Debug, Default)]
struct Inner {
    aborted: AtomicBool,
    notify: Notify,
}

/// A cheap, cloneable abort flag shared with the executor. Clones share one
/// underlying state, so tripping any handle aborts them all.
#[derive(Debug, Clone, Default)]
pub struct KillSwitch {
    inner: Arc<Inner>,
}

impl KillSwitch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Trip the switch: set the flag, then wake every waiter. The flag is stored
    /// before waiters are notified so any waiter woken here observes `true`.
    pub fn trip(&self) {
        self.inner.aborted.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    pub fn is_tripped(&self) -> bool {
        self.inner.aborted.load(Ordering::SeqCst)
    }

    /// Clear the switch so a new session can arm. Not reachable by the agent.
    pub fn reset(&self) {
        self.inner.aborted.store(false, Ordering::SeqCst);
    }

    /// Resolve as soon as the switch is (or becomes) tripped.
    ///
    /// Registers with the notifier *before* the final flag check so a concurrent
    /// [`trip`](KillSwitch::trip) between the check and the await cannot be lost.
    pub async fn wait_tripped(&self) {
        // Fast path: already tripped, no need to arm the notifier.
        if self.is_tripped() {
            return;
        }
        let notified = self.inner.notify.notified();
        tokio::pin!(notified);
        // Enable registers this waiter without consuming it, closing the wakeup
        // race with a `trip` that runs before we await.
        notified.as_mut().enable();
        if self.is_tripped() {
            return;
        }
        notified.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn trip_sets_reset_clears() {
        let ks = KillSwitch::new();
        assert!(!ks.is_tripped());
        ks.trip();
        assert!(ks.is_tripped());
        ks.reset();
        assert!(!ks.is_tripped());
    }

    #[test]
    fn clones_share_one_state() {
        let a = KillSwitch::new();
        let b = a.clone();
        a.trip();
        assert!(b.is_tripped(), "clone must observe the trip");
        b.reset();
        assert!(!a.is_tripped(), "reset on a clone must clear the original");
    }

    #[tokio::test]
    async fn wait_tripped_returns_immediately_if_already_tripped() {
        let ks = KillSwitch::new();
        ks.trip();
        // Should resolve without hanging.
        tokio::time::timeout(Duration::from_secs(1), ks.wait_tripped())
            .await
            .expect("wait_tripped must return immediately when already tripped");
    }

    #[tokio::test]
    async fn wait_tripped_resolves_after_trip() {
        let ks = KillSwitch::new();
        let waiter = ks.clone();
        let handle = tokio::spawn(async move {
            waiter.wait_tripped().await;
        });

        // Give the waiter a moment to park, then trip from another handle.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!ks.is_tripped());
        ks.trip();

        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("waiter must wake within the timeout after trip")
            .expect("waiter task should not panic");
    }
}
