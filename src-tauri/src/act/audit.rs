//! Local, append-only audit log of Act activity.
//!
//! TODO(act-phase0): entry types (timestamp, transcript, snapshot hash, actions,
//! capability decisions, result), an append-only writer, and redaction so no PHI
//! is persisted. Stub only.

/// One audited Act event. Fields filled out in Phase 0.
#[derive(Debug, Default)]
pub struct AuditEntry;
