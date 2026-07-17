//! Persistent single-threaded UI Automation (UIA) session broker.
//!
//! COM/UIA objects created through Terminator have **thread affinity**: a
//! [`terminator::Desktop`] and every `UIElement` it hands out may only be used
//! from the OS thread that created them. The original backend paid for that on
//! every action — it constructed a fresh `Desktop` (re-initializing COM/UIA)
//! inside each `spawn_blocking` closure, then threw it away. That is the "one
//! COM bring-up per action" bottleneck this module removes.
//!
//! Instead, ONE long-lived dedicated OS thread ("the UIA worker") initializes
//! COM once (implicitly, when it constructs its single `Desktop` — see the
//! assumption note below) and then serves *every* backend operation. Callers
//! submit a boxed closure that borrows `&Desktop`; the closure runs on the
//! worker thread and returns only plain owned (`Send`) data across a
//! [`tokio::sync::oneshot`] channel. **No COM object ever crosses a thread
//! boundary** — the `Desktop` and all elements are created, used, and dropped on
//! the worker.
//!
//! ## Async bridge
//!
//! The `AccessibilityBackend` trait is `async`. [`execute`] is the bridge: it
//! (1) creates a `oneshot` channel, (2) boxes the caller's closure together with
//! the `oneshot::Sender`, (3) pushes the job down an `std::sync::mpsc` channel to
//! the worker, and (4) `await`s the `oneshot` receiver under a
//! `tokio::time::timeout` watchdog. Nothing blocks the async executor: the only
//! synchronous work (the `mpsc::send`) is on an unbounded channel and never
//! parks.
//!
//! ## Watchdog / recovery
//!
//! A synchronous UIA call cannot be preempted from another thread, so if an op
//! wedges (e.g. a hung provider) the worker thread is stuck forever. The
//! per-op `tokio::time::timeout` bounds the *caller*; on expiry (or if the
//! worker panics and drops the `oneshot::Sender`) the broker marks the worker
//! dead and lazily spawns a **fresh** thread + `Desktop` on the next op. The
//! wedged thread and its `Desktop` are abandoned (leaked) rather than
//! force-killed — killing an OS thread mid-COM-call is unsound. This keeps
//! "persistent" from ever meaning "unrecoverable".
//!
//! ## Assumptions a Windows reviewer should confirm
//!
//! * **COM initialization.** We rely on `Desktop::new_default()` establishing
//!   whatever COM apartment Terminator/`uiautomation` needs on the *calling*
//!   thread (the worker). The original code depended on the same behavior — it
//!   called `Desktop::new_default()` on arbitrary `spawn_blocking` threads — so
//!   this is not a new assumption, only a narrowed one (now exactly one thread).
//!   We deliberately do **not** call `CoInitializeEx` ourselves to avoid an
//!   `RPC_E_CHANGED_MODE` conflict with Terminator's own apartment choice.
//! * **Apartment model.** `uiautomation` typically runs MTA. The task brief
//!   calls this "the STA thread"; functionally what matters is that it is a
//!   single, stable thread that owns every UIA object, which is correct for both
//!   STA and MTA. We make no STA-specific assumption (no message pump is run).

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use terminator::Desktop;
use tokio::sync::oneshot;

/// `tracing` target shared by every "uia timing" line this backend emits, so a
/// Windows operator can filter with `RUST_LOG=act::uia_timing=debug` to measure
/// p50/p95 on a real machine.
pub(crate) const TIMING_TARGET: &str = "act::uia_timing";

/// A unit of work for the UIA worker. It borrows the worker's persistent
/// `Desktop`, runs on-thread, and is responsible for delivering its own result
/// (each job captures its own typed `oneshot::Sender`), so the channel stays
/// monomorphic and no `Any`/downcast is needed.
type Job = Box<dyn FnOnce(&Desktop) + Send + 'static>;

/// Why a broker dispatch failed. Op-level (terminator) errors are carried
/// *inside* the closure's own return type; these variants are only about the
/// transport/worker lifecycle.
#[derive(Debug)]
pub(crate) enum BrokerError {
    /// The op did not finish within its watchdog budget; the worker was
    /// abandoned and will be recreated on the next op.
    Timeout(Duration),
    /// The worker thread died (panicked, or failed to construct its `Desktop`)
    /// before answering.
    WorkerDied,
    /// The worker thread could not be started at all.
    Spawn(String),
}

impl fmt::Display for BrokerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrokerError::Timeout(d) => write!(f, "uia op exceeded watchdog ({d:?})"),
            BrokerError::WorkerDied => write!(f, "uia worker thread died before answering"),
            BrokerError::Spawn(e) => write!(f, "could not start uia worker thread: {e}"),
        }
    }
}

impl std::error::Error for BrokerError {}

/// The process-wide persistent UIA session.
struct Broker {
    /// Job sender for the *current* worker. `None` before the first op and after
    /// the worker is declared dead/hung; the next op respawns it lazily.
    sender: Mutex<Option<Sender<Job>>>,
    /// Monotonic worker id, for log correlation across recreations.
    generation: AtomicU64,
}

static BROKER: OnceLock<Broker> = OnceLock::new();

fn broker() -> &'static Broker {
    BROKER.get_or_init(|| Broker {
        sender: Mutex::new(None),
        generation: AtomicU64::new(0),
    })
}

impl Broker {
    /// Lock helper that tolerates a poisoned mutex. We only ever hold this lock
    /// for cheap, panic-free work (clone a sender, spawn a thread), so poisoning
    /// is not expected; recovering the guard keeps a stray panic elsewhere from
    /// permanently wedging the broker.
    fn lock_sender(&self) -> std::sync::MutexGuard<'_, Option<Sender<Job>>> {
        self.sender.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Return a live job sender, spawning a fresh worker (and `Desktop`) if none
    /// is currently registered.
    fn ensure_worker(&self) -> Result<Sender<Job>, BrokerError> {
        let mut guard = self.lock_sender();
        if let Some(sender) = guard.as_ref() {
            return Ok(sender.clone());
        }
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let sender = spawn_worker(generation)?;
        *guard = Some(sender.clone());
        Ok(sender)
    }

    /// Mark the current worker as unusable so the next op respawns it. Called
    /// after a watchdog timeout or a dead worker; the stuck/dead thread is
    /// abandoned.
    fn invalidate(&self) {
        *self.lock_sender() = None;
    }
}

/// Spawn a fresh UIA worker thread and return its job sender. The `Desktop` is
/// constructed on the worker itself (thread affinity), so a construction failure
/// surfaces later as a failed `mpsc::send` / dead worker rather than here.
fn spawn_worker(generation: u64) -> Result<Sender<Job>, BrokerError> {
    let (tx, rx) = mpsc::channel::<Job>();
    thread::Builder::new()
        .name(format!("uia-worker-{generation}"))
        .spawn(move || worker_main(generation, rx))
        .map_err(|e| BrokerError::Spawn(e.to_string()))?;
    Ok(tx)
}

/// The worker thread body: build the single persistent `Desktop`, then serve
/// jobs until the channel closes (broker dropped its sender = worker retired).
fn worker_main(generation: u64, rx: mpsc::Receiver<Job>) {
    let init_start = Instant::now();
    let desktop = match Desktop::new_default() {
        Ok(desktop) => desktop,
        Err(err) => {
            // Leave `rx` to drop; the broker still holds a (now stale) sender,
            // and the next op's `mpsc::send` will fail, triggering a respawn.
            tracing::warn!(
                target: TIMING_TARGET,
                generation,
                error = %err,
                "uia timing: worker failed to construct Desktop; session not started"
            );
            return;
        }
    };
    // desktop-acquire: paid ONCE per worker lifetime instead of once per op.
    tracing::debug!(
        target: TIMING_TARGET,
        generation,
        acquire_ms = init_start.elapsed().as_millis() as u64,
        "uia timing: desktop-acquire (persistent session ready)"
    );

    while let Ok(job) = rx.recv() {
        job(&desktop);
    }

    tracing::debug!(
        target: TIMING_TARGET,
        generation,
        "uia timing: worker channel closed; retiring session"
    );
}

/// Push a job to the current worker, respawning once if the stored sender turns
/// out to be stale (worker exited between ops).
fn submit(job: Job) -> Result<(), BrokerError> {
    let b = broker();
    let sender = b.ensure_worker()?;
    match sender.send(job) {
        Ok(()) => Ok(()),
        Err(mpsc::SendError(job)) => {
            // Receiver gone: the worker exited. Drop the stale sender and try a
            // freshly spawned worker exactly once.
            b.invalidate();
            let sender = b.ensure_worker()?;
            sender.send(job).map_err(|_| BrokerError::WorkerDied)
        }
    }
}

/// Run `f` on the persistent UIA worker thread and await its result under a
/// per-op `watchdog`.
///
/// `f` receives the worker's long-lived `&Desktop` and must return only `Send`
/// owned data (never a COM object). `op` is a short static label used purely for
/// the "uia timing" instrumentation.
///
/// On watchdog expiry or worker death the current worker is abandoned and the
/// next call transparently spawns a new thread + `Desktop`.
pub(crate) async fn execute<T, F>(
    watchdog: Duration,
    op: &'static str,
    f: F,
) -> Result<T, BrokerError>
where
    F: FnOnce(&Desktop) -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = oneshot::channel::<T>();

    let job: Job = Box::new(move |desktop: &Desktop| {
        let exec_start = Instant::now();
        let result = f(desktop);
        // action: the on-worker execution time for this op (excludes queue wait).
        tracing::debug!(
            target: TIMING_TARGET,
            op,
            exec_ms = exec_start.elapsed().as_millis() as u64,
            "uia timing: action executed on worker"
        );
        // If the caller already timed out and dropped `rx`, discard the result.
        let _ = tx.send(result);
    });

    let dispatch_start = Instant::now();
    submit(job)?;

    match tokio::time::timeout(watchdog, rx).await {
        Ok(Ok(value)) => {
            tracing::debug!(
                target: TIMING_TARGET,
                op,
                total_ms = dispatch_start.elapsed().as_millis() as u64,
                "uia timing: op completed (queue + exec)"
            );
            Ok(value)
        }
        Ok(Err(_recv_closed)) => {
            broker().invalidate();
            tracing::warn!(
                target: TIMING_TARGET,
                op,
                "uia timing: worker died before answering; recreating on next op"
            );
            Err(BrokerError::WorkerDied)
        }
        Err(_elapsed) => {
            broker().invalidate();
            tracing::warn!(
                target: TIMING_TARGET,
                op,
                watchdog_ms = watchdog.as_millis() as u64,
                "uia timing: op exceeded watchdog; abandoning worker"
            );
            Err(BrokerError::Timeout(watchdog))
        }
    }
}
