//! Tauri commands backing the in-app Diagnostics log panel. See `crate::diag`.

use crate::diag::{self, LogEntry};

/// Return recent diagnostics log entries (oldest first). `limit == 0` (or
/// omitted) returns the whole buffer. Used to seed the panel when it opens; live
/// updates then arrive over the `diag://log` event.
#[tauri::command]
pub fn diag_log_dump(limit: Option<usize>) -> Vec<LogEntry> {
    diag::dump(limit.unwrap_or(0))
}

/// Empty the in-app diagnostics buffer.
#[tauri::command]
pub fn diag_log_clear() {
    diag::clear();
}
