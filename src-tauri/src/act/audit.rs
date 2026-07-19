//! Local, append-only audit log of Act activity.
//!
//! Every executed plan is logged to a local JSON-lines file: the spoken
//! transcript, a hash of the accessibility snapshot it ran against, the planned
//! actions, the per-action capability [`Decision`]s, and a result string. No PHI
//! is persisted — the snapshot is stored only as a hash, and the element schema
//! itself carries only value *lengths*, never raw values (see [`Snapshot`]). The
//! log is user-exportable and never uploaded by default.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::action::Action;
use super::capability::Decision;
use super::element::Snapshot;

/// FNV-1a 64-bit — a tiny, dependency-free hash used to fingerprint a snapshot
/// without storing its contents.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Unix-millis timestamp; a clock earlier than the epoch degrades to 0 rather
/// than panicking.
fn now_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// One audited Act event. Serialized as a single JSON line.
///
/// Deliberately PHI-free: it holds a `snapshot_hash` (never the snapshot), the
/// transcript, the actions, their decisions, and a result — nothing that can
/// carry a field's raw value.
#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    /// Milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
    /// The spoken command that produced this plan.
    pub transcript: String,
    /// FNV-1a hash of the serialized snapshot the plan ran against (value-free —
    /// the snapshot schema only carries value lengths, never values).
    pub snapshot_hash: u64,
    /// The plan that was evaluated / executed.
    pub actions: Vec<Action>,
    /// The gate's ruling for each action, positionally aligned with `actions`.
    pub decisions: Vec<Decision>,
    /// Human-readable outcome, e.g. `"ok"`, `"blocked: fs.destructive"`.
    pub result: String,
}

impl AuditEntry {
    /// Build an entry, stamping the current time and hashing the snapshot. The
    /// snapshot is consumed only to compute its hash; it is not retained.
    pub fn new(
        transcript: impl Into<String>,
        snapshot: &Snapshot,
        actions: Vec<Action>,
        decisions: Vec<Decision>,
        result: impl Into<String>,
    ) -> Self {
        let snapshot_hash = serde_json::to_vec(snapshot)
            .map(|bytes| fnv1a_64(&bytes))
            .unwrap_or(0);
        Self {
            timestamp_ms: now_unix_millis(),
            transcript: transcript.into(),
            snapshot_hash,
            actions,
            decisions,
            result: result.into(),
        }
    }
}

/// An append-only JSON-lines audit log.
#[derive(Debug)]
pub struct AuditLog {
    path: PathBuf,
    file: File,
}

impl AuditLog {
    /// Open (creating if needed) the log at `path` for appending.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self { path, file })
    }

    /// The file this log writes to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one entry as a JSON line and flush it to disk.
    pub fn append(&mut self, entry: &AuditEntry) -> io::Result<()> {
        let line = serde_json::to_string(entry).map_err(io::Error::other)?;
        // Guard against a serializer that ever emits a newline mid-record, which
        // would corrupt the one-entry-per-line contract.
        debug_assert!(!line.contains('\n'));
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::element::{Role, UiElement};

    fn unique_temp_path(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "flowrad_audit_{}_{}_{}.jsonl",
            std::process::id(),
            tag,
            nanos
        ))
    }

    /// A snapshot deliberately seeded with PHI-looking strings so the test can
    /// prove none of them reach the log (only the hash does).
    fn phi_snapshot() -> Snapshot {
        Snapshot {
            app: "Chart".into(),
            window_title: "Patient: John Doe".into(),
            focused: Some("#/1".into()),
            pointer: None,
            selection_text_len: 9,
            elements: vec![UiElement {
                path: "#/1".into(),
                role: Role::TextField,
                name: "Social Security Number".into(),
                description: String::new(),
                value_len: 11,
                states: vec![],
                bounds: None,
                patterns: vec![],
            }],
        }
    }

    #[test]
    fn appends_two_entries_and_round_trips() {
        let path = unique_temp_path("roundtrip");
        let snap = phi_snapshot();

        let e1 = AuditEntry::new(
            "click send",
            &snap,
            vec![Action::Invoke {
                target: "#/1".into(),
            }],
            vec![Decision::Allow],
            "ok",
        );
        let e2 = AuditEntry::new(
            "delete the file",
            &snap,
            vec![Action::Stop],
            vec![Decision::Deny],
            "blocked: fs.destructive",
        );

        {
            let mut log = AuditLog::open(&path).unwrap();
            log.append(&e1).unwrap();
            log.append(&e2).unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2, "expected exactly two JSON lines");

        let v1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        let v2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();

        assert_eq!(v1["transcript"], "click send");
        assert_eq!(v1["result"], "ok");
        assert_eq!(v1["actions"][0]["op"], "invoke");
        assert_eq!(v1["decisions"][0], "allow");
        assert!(v1["timestamp_ms"].as_u64().unwrap() > 0);
        assert!(v1["snapshot_hash"].as_u64().is_some());

        assert_eq!(v2["transcript"], "delete the file");
        assert_eq!(v2["result"], "blocked: fs.destructive");
        assert_eq!(v2["decisions"][0], "deny");

        // Same snapshot → identical hash both times.
        assert_eq!(v1["snapshot_hash"], v2["snapshot_hash"]);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn log_never_contains_snapshot_value_content() {
        let path = unique_temp_path("nophi");
        let snap = phi_snapshot();
        let entry = AuditEntry::new(
            "focus the field",
            &snap,
            vec![Action::Focus {
                target: "#/1".into(),
            }],
            vec![Decision::Allow],
            "ok",
        );

        {
            let mut log = AuditLog::open(&path).unwrap();
            log.append(&entry).unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        // The snapshot is stored only as a hash — none of its app/window/element
        // strings may appear in the log.
        assert!(
            !contents.contains("John Doe"),
            "window title leaked into log"
        );
        assert!(
            !contents.contains("Social Security Number"),
            "element name leaked into log"
        );
        assert!(
            !contents.contains("Patient"),
            "PHI substring leaked into log"
        );
        // What is present is benign: the transcript and the hash field.
        assert!(contents.contains("focus the field"));
        assert!(contents.contains("snapshot_hash"));

        let _ = std::fs::remove_file(&path);
    }
}
