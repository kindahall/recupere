// ============================================================================
// Récupère — Audit trail commands
// ============================================================================
// Thin wrappers around `crate::audit::*`. Note that `clear_local_history`
// stays in `commands/mod.rs` for now because it touches the scan/export
// session registries which have not been extracted yet — see
// `docs/refactor-commands.md` sub-pass 4 (`state.rs`).
// ============================================================================

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_audit_trail() -> Vec<crate::audit::AuditRecord> {
    crate::audit::get_trail()
}

/// Walk the hash-chained audit trail and return per-record verification.
/// Exposed so support bundles can include proof that the trail was intact
/// when the bundle was generated; rewriting history invalidates every
/// successor hash and shows up here as a concrete `first_broken_id`.
#[cfg_attr(feature = "desktop", tauri::command)]
pub fn verify_audit_trail() -> crate::audit::AuditChainReport {
    crate::audit::verify_trail()
}

/// Return up to `limit` recent in-memory `tracing` events. Local-only, never
/// transmitted. Used by the Settings "diagnostics" section and the support
/// bundle. `limit=None` returns the full ring (capacity ~500).
#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_recent_traces(limit: Option<usize>) -> Vec<crate::telemetry::TraceEntry> {
    crate::telemetry::recent_traces(limit)
}

/// Clear the in-memory trace ring. Typically called by the UI before
/// reproducing a bug so the captured window is focused on the repro.
#[cfg_attr(feature = "desktop", tauri::command)]
pub fn clear_recent_traces() {
    crate::telemetry::clear_traces();
}
