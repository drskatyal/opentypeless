//! In-app diagnostics log: an in-memory ring buffer that mirrors every `tracing`
//! event (the same INFO/DEBUG/WARN/ERROR lines that print to the terminal) so the
//! frontend can show them inside the app — errors, actions, and LLM I/O — without
//! the user needing a console attached.
//!
//! Two sinks receive every captured event:
//!   1. a bounded ring buffer ([`dump`] reads it, [`clear`] empties it), so a panel
//!      opened after the fact still sees recent history; and
//!   2. a live Tauri event ([`DIAG_EVENT`]) emitted per entry, so an open panel
//!      updates in real time.
//!
//! The capture layer only sees this crate's events (the global `EnvFilter` in
//! `lib.rs` enables `opentypeless=debug` and nothing else), so wry/tauri internals
//! never reach it and there is no logging recursion.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

/// The Tauri event channel each captured log entry is emitted on.
pub const DIAG_EVENT: &str = "diag://log";

/// Ring-buffer capacity. ~4k lines is several minutes of busy Act activity and a
/// few MB at most; older lines roll off the front.
const CAP: usize = 4000;

/// One captured log line, shaped for the frontend console.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    /// Monotonic sequence number (stable ordering + React keys).
    pub seq: u64,
    /// Unix milliseconds when the event was captured.
    pub ts_ms: u64,
    /// `ERROR` / `WARN` / `INFO` / `DEBUG` / `TRACE`.
    pub level: String,
    /// The event's module target, e.g. `opentypeless_lib::act::conductor`.
    pub target: String,
    /// The rendered message plus any structured fields, e.g.
    /// `Act selection resolved count=1 missions=["open_flow:play_video"]`.
    pub message: String,
}

static BUFFER: OnceLock<Mutex<VecDeque<LogEntry>>> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);
static APP: OnceLock<AppHandle> = OnceLock::new();

fn buffer() -> &'static Mutex<VecDeque<LogEntry>> {
    BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(CAP)))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Register the app handle so captured entries can be emitted live to the
/// frontend. Called once from `setup`; entries captured before this is set still
/// land in the ring buffer and reach the panel via [`dump`].
pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// Snapshot the most recent `limit` entries (oldest first). `limit == 0` returns
/// the whole buffer.
pub fn dump(limit: usize) -> Vec<LogEntry> {
    let buf = buffer().lock().unwrap_or_else(|e| e.into_inner());
    if limit == 0 || limit >= buf.len() {
        buf.iter().cloned().collect()
    } else {
        buf.iter().skip(buf.len() - limit).cloned().collect()
    }
}

/// Empty the ring buffer.
pub fn clear() {
    buffer().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

fn push(entry: LogEntry) {
    {
        let mut buf = buffer().lock().unwrap_or_else(|e| e.into_inner());
        if buf.len() >= CAP {
            buf.pop_front();
        }
        buf.push_back(entry.clone());
    }
    if let Some(app) = APP.get() {
        // Best-effort live push; a closed webview just drops it.
        let _ = app.emit(DIAG_EVENT, &entry);
    }
}

/// Collects an event's `message` field and any structured key=value fields into a
/// single human-readable line, matching how the terminal formatter renders them.
#[derive(Default)]
struct LineVisitor {
    message: String,
    fields: String,
}

impl LineVisitor {
    fn record(&mut self, name: &str, value: String) {
        if name == "message" {
            self.message = value;
        } else {
            if !self.fields.is_empty() {
                self.fields.push(' ');
            }
            self.fields.push_str(name);
            self.fields.push('=');
            self.fields.push_str(&value);
        }
    }

    fn into_line(self) -> String {
        match (self.message.is_empty(), self.fields.is_empty()) {
            (true, true) => String::new(),
            (false, true) => self.message,
            (true, false) => self.fields,
            (false, false) => format!("{} {}", self.message, self.fields),
        }
    }
}

impl Visit for LineVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.record(field.name(), value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.record(field.name(), format!("{value:?}"));
    }
}

/// A `tracing` layer that mirrors every event into the in-app diagnostics buffer.
pub struct DiagLayer;

impl<S: Subscriber> Layer<S> for DiagLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut visitor = LineVisitor::default();
        event.record(&mut visitor);
        push(LogEntry {
            seq: SEQ.fetch_add(1, Ordering::Relaxed),
            ts_ms: now_ms(),
            level: meta.level().as_str().to_string(),
            target: meta.target().to_string(),
            message: visitor.into_line(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_dumps_and_clears() {
        clear();
        for i in 0..3 {
            push(LogEntry {
                seq: i,
                ts_ms: 0,
                level: "INFO".into(),
                target: "t".into(),
                message: format!("m{i}"),
            });
        }
        let all = dump(0);
        assert_eq!(all.len(), 3);
        assert_eq!(all.first().unwrap().message, "m0");
        assert_eq!(all.last().unwrap().message, "m2");

        let tail = dump(2);
        assert_eq!(tail.len(), 2);
        assert_eq!(tail.first().unwrap().message, "m1");

        clear();
        assert!(dump(0).is_empty());
    }

    #[test]
    fn line_visitor_joins_message_and_fields() {
        let mut v = LineVisitor::default();
        v.record("message", "Act selection resolved".into());
        v.record("count", "1".into());
        assert_eq!(v.into_line(), "Act selection resolved count=1");
    }
}
