// ============================================================================
// Récupère — Shared session state + lifecycle + persistence
// ============================================================================
// Types, constants, live session registries, persistence, log helpers and
// timestamp utilities used by `commands::scan`, `commands::imaging`,
// `commands::export`, `commands::ai`, `commands::file_preview` and the
// support-bundle / recovery-report helpers in `commands::mod`. Everything
// is `pub(crate)` so neighbouring modules can keep calling these helpers
// through the re-export in `commands::mod`.
// ============================================================================

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

use serde::{Deserialize, Serialize};

use crate::imaging::{self, ImagingProfile};
use crate::types::{
    DetectedDevice, ExportProgress, ExportSessionSummary, ImagingMapRange,
    ImportedRecoverySourceStatus, LocalHistoryPurgeResult, RecoveredFile, ScanProgress,
    ScanSessionSummary, TechnicalLogEntry,
};

// ---------------------------------------------------------------------------
// Tunables
// ---------------------------------------------------------------------------

pub(super) const MAX_SESSION_LOGS: usize = 500;
pub(super) const MAX_PERSISTED_SCAN_RECORDS: usize = 250;
pub(super) const MAX_PERSISTED_EXPORT_RECORDS: usize = 250;
pub(super) const SCAN_CANCELLED_SENTINEL: &str = "__recupere_scan_cancelled__";

// ---------------------------------------------------------------------------
// Live session structs
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) struct InventoryScanSession {
    pub(super) id: String,
    pub(super) device_id: String,
    pub(super) device_name: String,
    pub(super) scan_type: String,
    pub(super) root_path: String,
    pub(super) started_at_ms: u64,
    pub(super) completed_at_ms: Option<u64>,
    pub(super) imaging_profile: Option<ImagingProfile>,
    pub(super) imaging_profile_reason_key: Option<String>,
    pub(super) progress: ScanProgress,
    pub(super) imaging_unreadable_ranges: Vec<ImagingMapRange>,
    pub(super) imaging_rescued_after_retry_bytes: u64,
    pub(super) imaging_retry_passes_completed: u8,
    pub(super) results: Vec<RecoveredFile>,
    pub(super) logs: Vec<TechnicalLogEntry>,
    pub(super) control: Arc<ScanControl>,
}

#[derive(Debug)]
pub(crate) struct ExportSession {
    pub(super) id: String,
    pub(super) scan_id: String,
    pub(super) destination_path: String,
    pub(super) started_at_ms: u64,
    pub(super) completed_at_ms: Option<u64>,
    pub(super) explicit_selection: bool,
    pub(super) implicit_preview_first_excluded_count: u32,
    pub(super) progress: ExportProgress,
    pub(super) logs: Vec<TechnicalLogEntry>,
}

// ---------------------------------------------------------------------------
// Scan pause/resume/cancel control block
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub(super) struct ScanControlState {
    pub(super) paused: bool,
    pub(super) cancelled: bool,
}

#[derive(Debug, Default)]
pub(crate) struct ScanControl {
    pub(super) state: Mutex<ScanControlState>,
    pub(super) condvar: Condvar,
}

// ---------------------------------------------------------------------------
// On-disk persisted records
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedScanRecord {
    pub(super) summary: ScanSessionSummary,
    pub(super) logs: Vec<TechnicalLogEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct PersistedScanArchive {
    pub(super) scans: Vec<PersistedScanRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct PersistedExportArchive {
    pub(super) exports: Vec<PersistedExportRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedExportRecord {
    pub(super) summary: ExportSessionSummary,
    pub(super) logs: Vec<TechnicalLogEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct LegacyPersistedExportArchive {
    pub(super) exports: Vec<ExportSessionSummary>,
}

// ---------------------------------------------------------------------------
// Poison-tolerant lock helper
// ---------------------------------------------------------------------------
//
// The 350+ `mutex.lock().expect("...")` call sites in commands/*.rs turn every
// poisoned lock (i.e. a thread panicked while holding the mutex) into a full
// app crash. Recovery workloads can't afford that — a failed worker thread
// must surface as a clear error in the affected session, not a segfault-like
// exit that loses every other in-flight scan/export.
//
// `lock_or_recover` grants the poisoned guard (`PoisonError::into_inner`) and
// logs the recovery so the event is auditable. Callers opt in by using this
// helper instead of `.lock().expect(...)`; new code should always prefer it.
// The legacy `.expect(...)` call sites are being migrated opportunistically;
// see the audit plan, Sprint 2.2.
pub(crate) fn lock_or_recover<'a, T>(
    mutex: &'a Mutex<T>,
    label: &'static str,
) -> MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poison) => {
            tracing::warn!(
                target: "recupere::lock_recovery",
                "poisoned lock recovered at {label}; continuing with last known state"
            );
            poison.into_inner()
        }
    }
}

#[derive(Debug, Default)]
struct SessionSourceProvenance {
    source_display_name: Option<String>,
    source_kind: Option<String>,
    source_format: Option<String>,
    source_analysis_path: Option<String>,
    source_available: Option<bool>,
    source_requires_preparation: Option<bool>,
    source_prepared: Option<bool>,
    reconstructed_raid_source: bool,
}

fn classify_imported_source_kind(status: &ImportedRecoverySourceStatus) -> &'static str {
    let source_format = status.source_format.trim().to_ascii_uppercase();

    if source_format.starts_with("RAID") {
        return "raid-analysis";
    }
    if source_format == "E01" {
        return "forensic-image";
    }
    if matches!(source_format.as_str(), "VMDK" | "VHD" | "VHDX") {
        return "virtual-disk";
    }
    if matches!(source_format.as_str(), "RAW" | "IMG" | "DD" | "BIN") {
        return "raw-image";
    }

    "generic-image"
}

fn build_scan_source_provenance(device_id: &str) -> SessionSourceProvenance {
    let Some(device) = crate::core::detect_devices()
        .into_iter()
        .find(|device| device.id == device_id)
    else {
        return SessionSourceProvenance::default();
    };

    let Ok(Some(status)) =
        crate::imported_sources::get_imported_source_status(Path::new(&device.device_path))
    else {
        return SessionSourceProvenance::default();
    };

    SessionSourceProvenance {
        source_display_name: Some(status.display_name.clone()),
        source_kind: Some(classify_imported_source_kind(&status).into()),
        source_format: Some(status.source_format.clone()),
        source_analysis_path: status
            .analysis_path
            .clone()
            .or_else(|| Some(status.source_path.clone())),
        source_available: Some(status.source_available),
        source_requires_preparation: Some(status.requires_preparation),
        source_prepared: Some(status.prepared),
        reconstructed_raid_source: classify_imported_source_kind(&status) == "raid-analysis",
    }
}

fn load_scan_summary_for_export_provenance(scan_id: &str) -> Option<ScanSessionSummary> {
    let live_session = lock_or_recover(scan_sessions(), "scan session registry")
        .get(scan_id)
        .cloned();
    if let Some(session) = live_session {
        return Some(build_scan_summary(&session));
    }

    load_persisted_scan_record(scan_id).map(|record| record.summary)
}

#[cfg(test)]
mod lock_helper_tests {
    use super::*;

    #[test]
    fn lock_or_recover_returns_guard_for_healthy_mutex() {
        let m = Mutex::new(42_u32);
        let guard = lock_or_recover(&m, "healthy");
        assert_eq!(*guard, 42);
    }

    #[test]
    fn lock_or_recover_keeps_serving_after_poison() {
        let mutex = Arc::new(Mutex::new(String::from("before-poison")));
        let clone = Arc::clone(&mutex);

        // Poison the mutex the only reliable way: let a thread panic while
        // holding the guard. `join()` returns `Err` because that thread
        // unwound, but the mutex stays alive and the Rust runtime flags it
        // as poisoned — exactly the state we need to verify the helper
        // handles without crashing the main thread.
        let poisoning_thread = std::thread::spawn(move || {
            let mut guard = clone.lock().expect("first acquire succeeds");
            *guard = String::from("in-flight-write");
            panic!("synthetic poison trigger");
        });
        let join_result = poisoning_thread.join();
        assert!(
            join_result.is_err(),
            "the poisoning thread must actually panic to poison the lock"
        );

        assert!(mutex.is_poisoned());
        let recovered = lock_or_recover(&mutex, "after-poison");
        assert_eq!(*recovered, "in-flight-write");
    }
}

// ===========================================================================
// Session registries, lifecycle, persistence, summaries, logs and timestamp
// utilities moved from `commands/mod.rs` in Sprint 2.1 Pass C. Everything
// here is `pub(crate)` so that neighbouring modules (`scan`, `export`,
// `ai`, `imaging_cmd`, `file_preview`) can keep calling them by their bare
// name via a re-export in `commands/mod.rs`.
// ===========================================================================

pub(crate) fn scan_sessions() -> &'static Mutex<HashMap<String, Arc<Mutex<InventoryScanSession>>>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, Arc<Mutex<InventoryScanSession>>>>> =
        OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn export_sessions() -> &'static Mutex<HashMap<String, Arc<Mutex<ExportSession>>>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, Arc<Mutex<ExportSession>>>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn initialize_scan_session(
    device: &DetectedDevice,
    scan_type: &str,
    root_path: &Path,
    total_bytes: u64,
    imaging_profile: Option<imaging::ImagingProfile>,
    imaging_profile_reason_key: Option<String>,
) -> (String, Arc<Mutex<InventoryScanSession>>) {
    let session_id = format!("scan-{}", unix_timestamp_ms());
    let started_at_ms = unix_timestamp_ms();
    let control = Arc::new(ScanControl::default());
    let session = Arc::new(Mutex::new(InventoryScanSession {
        id: session_id.clone(),
        device_id: device.id.clone(),
        device_name: device.name.clone(),
        scan_type: scan_type.to_string(),
        root_path: root_path.to_string_lossy().to_string(),
        started_at_ms,
        completed_at_ms: None,
        imaging_profile,
        imaging_profile_reason_key,
        progress: ScanProgress {
            status: "preparing".into(),
            stage: "initializing".into(),
            percent_complete: 0.0,
            bytes_scanned: 0,
            total_bytes,
            files_found: 0,
            errors_count: 0,
            elapsed_seconds: 0,
            resume_from_bytes: 0,
            unreadable_ranges_count: 0,
            unreadable_bytes: 0,
            rescued_after_retry_bytes: 0,
            retry_passes_completed: 0,
            unreadable_ranges: Vec::new(),
        },
        imaging_unreadable_ranges: Vec::new(),
        imaging_rescued_after_retry_bytes: 0,
        imaging_retry_passes_completed: 0,
        results: Vec::new(),
        logs: vec![TechnicalLogEntry {
            timestamp_ms: started_at_ms,
            level: "info".into(),
            message: format!(
                "Scan session created for {} on {}.",
                device.name,
                root_path.to_string_lossy()
            ),
        }],
        control,
    }));

    crate::commands::state::lock_or_recover(scan_sessions(), "scan session registry")
        .insert(session_id.clone(), Arc::clone(&session));

    if let Err(error) = persist_scan_session(&session) {
        tracing::info!(
            "initialize_scan_session: unable to persist initial session snapshot: {error}"
        );
    }

    (session_id, session)
}

pub(crate) fn get_session(scan_id: &str) -> Result<Arc<Mutex<InventoryScanSession>>, String> {
    crate::commands::state::lock_or_recover(scan_sessions(), "scan session registry")
        .get(scan_id)
        .cloned()
        .ok_or_else(|| format!("Scan session `{scan_id}` was not found."))
}

pub(crate) fn build_scan_summary(session: &Arc<Mutex<InventoryScanSession>>) -> ScanSessionSummary {
    let state = crate::commands::state::lock_or_recover(session, "scan session");
    let provenance = build_scan_source_provenance(&state.device_id);
    ScanSessionSummary {
        id: state.id.clone(),
        device_id: state.device_id.clone(),
        device_name: state.device_name.clone(),
        source_display_name: provenance.source_display_name,
        source_kind: provenance.source_kind,
        source_format: provenance.source_format,
        source_analysis_path: provenance.source_analysis_path,
        source_available: provenance.source_available,
        source_requires_preparation: provenance.source_requires_preparation,
        source_prepared: provenance.source_prepared,
        reconstructed_raid_source: provenance.reconstructed_raid_source,
        scan_type: state.scan_type.clone(),
        started_at_ms: state.started_at_ms,
        completed_at_ms: state.completed_at_ms,
        status: state.progress.status.clone(),
        files_found: state.progress.files_found,
        files_recovered: recovered_files_count(&state.results),
        duration_seconds: state.progress.elapsed_seconds,
        errors: state.progress.errors_count,
        bytes_copied: state.progress.bytes_scanned,
        total_bytes: state.progress.total_bytes,
        resume_from_bytes: state.progress.resume_from_bytes,
        unreadable_ranges_count: state.progress.unreadable_ranges_count,
        unreadable_bytes: state.progress.unreadable_bytes,
        rescued_after_retry_bytes: state.imaging_rescued_after_retry_bytes,
        retry_passes_completed: state.imaging_retry_passes_completed,
        unreadable_ranges: state.imaging_unreadable_ranges.clone(),
    }
}

pub(crate) fn build_export_summary(session: &Arc<Mutex<ExportSession>>) -> ExportSessionSummary {
    let state = session.lock().expect("export session lock poisoned");
    let scan_provenance = load_scan_summary_for_export_provenance(&state.scan_id);
    ExportSessionSummary {
        id: state.id.clone(),
        scan_id: state.scan_id.clone(),
        source_device_name: scan_provenance
            .as_ref()
            .map(|summary| summary.device_name.clone()),
        source_display_name: scan_provenance
            .as_ref()
            .and_then(|summary| summary.source_display_name.clone()),
        source_kind: scan_provenance
            .as_ref()
            .and_then(|summary| summary.source_kind.clone()),
        source_format: scan_provenance
            .as_ref()
            .and_then(|summary| summary.source_format.clone()),
        source_analysis_path: scan_provenance
            .as_ref()
            .and_then(|summary| summary.source_analysis_path.clone()),
        source_available: scan_provenance
            .as_ref()
            .and_then(|summary| summary.source_available),
        source_requires_preparation: scan_provenance
            .as_ref()
            .and_then(|summary| summary.source_requires_preparation),
        source_prepared: scan_provenance
            .as_ref()
            .and_then(|summary| summary.source_prepared),
        reconstructed_raid_source: scan_provenance
            .as_ref()
            .map(|summary| summary.reconstructed_raid_source)
            .unwrap_or(false),
        destination_path: state.destination_path.clone(),
        started_at_ms: state.started_at_ms,
        completed_at_ms: state.completed_at_ms,
        status: state.progress.status.clone(),
        total_files: state.progress.total_files,
        exported_files: state.progress.exported_files,
        total_bytes: state.progress.total_bytes,
        exported_bytes: state.progress.exported_bytes,
        explicit_selection: state.explicit_selection,
        implicit_preview_first_excluded_count: state.implicit_preview_first_excluded_count,
        errors: state.progress.errors.clone(),
    }
}

pub(crate) fn snapshot_export_record(session: &Arc<Mutex<ExportSession>>) -> PersistedExportRecord {
    let state = session.lock().expect("export session lock poisoned");
    let scan_provenance = load_scan_summary_for_export_provenance(&state.scan_id);
    PersistedExportRecord {
        summary: ExportSessionSummary {
            id: state.id.clone(),
            scan_id: state.scan_id.clone(),
            source_device_name: scan_provenance
                .as_ref()
                .map(|summary| summary.device_name.clone()),
            source_display_name: scan_provenance
                .as_ref()
                .and_then(|summary| summary.source_display_name.clone()),
            source_kind: scan_provenance
                .as_ref()
                .and_then(|summary| summary.source_kind.clone()),
            source_format: scan_provenance
                .as_ref()
                .and_then(|summary| summary.source_format.clone()),
            source_analysis_path: scan_provenance
                .as_ref()
                .and_then(|summary| summary.source_analysis_path.clone()),
            source_available: scan_provenance
                .as_ref()
                .and_then(|summary| summary.source_available),
            source_requires_preparation: scan_provenance
                .as_ref()
                .and_then(|summary| summary.source_requires_preparation),
            source_prepared: scan_provenance
                .as_ref()
                .and_then(|summary| summary.source_prepared),
            reconstructed_raid_source: scan_provenance
                .as_ref()
                .map(|summary| summary.reconstructed_raid_source)
                .unwrap_or(false),
            destination_path: state.destination_path.clone(),
            started_at_ms: state.started_at_ms,
            completed_at_ms: state.completed_at_ms,
            status: state.progress.status.clone(),
            total_files: state.progress.total_files,
            exported_files: state.progress.exported_files,
            total_bytes: state.progress.total_bytes,
            exported_bytes: state.progress.exported_bytes,
            explicit_selection: state.explicit_selection,
            implicit_preview_first_excluded_count: state.implicit_preview_first_excluded_count,
            errors: state.progress.errors.clone(),
        },
        logs: state.logs.clone(),
    }
}

pub(crate) fn snapshot_scan_record(
    session: &Arc<Mutex<InventoryScanSession>>,
) -> PersistedScanRecord {
    let state = crate::commands::state::lock_or_recover(session, "scan session");
    let provenance = build_scan_source_provenance(&state.device_id);
    PersistedScanRecord {
        summary: ScanSessionSummary {
            id: state.id.clone(),
            device_id: state.device_id.clone(),
            device_name: state.device_name.clone(),
            source_display_name: provenance.source_display_name,
            source_kind: provenance.source_kind,
            source_format: provenance.source_format,
            source_analysis_path: provenance.source_analysis_path,
            source_available: provenance.source_available,
            source_requires_preparation: provenance.source_requires_preparation,
            source_prepared: provenance.source_prepared,
            reconstructed_raid_source: provenance.reconstructed_raid_source,
            scan_type: state.scan_type.clone(),
            started_at_ms: state.started_at_ms,
            completed_at_ms: state.completed_at_ms,
            status: state.progress.status.clone(),
            files_found: state.progress.files_found,
            files_recovered: recovered_files_count(&state.results),
            duration_seconds: state.progress.elapsed_seconds,
            errors: state.progress.errors_count,
            bytes_copied: state.progress.bytes_scanned,
            total_bytes: state.progress.total_bytes,
            resume_from_bytes: state.progress.resume_from_bytes,
            unreadable_ranges_count: state.progress.unreadable_ranges_count,
            unreadable_bytes: state.progress.unreadable_bytes,
            rescued_after_retry_bytes: state.imaging_rescued_after_retry_bytes,
            retry_passes_completed: state.imaging_retry_passes_completed,
            unreadable_ranges: state.imaging_unreadable_ranges.clone(),
        },
        logs: state.logs.clone(),
    }
}

pub(crate) fn persist_scan_session(
    session: &Arc<Mutex<InventoryScanSession>>,
) -> Result<(), String> {
    let snapshot = snapshot_scan_record(session);
    let result = upsert_persisted_scan_record(snapshot);
    emit_scan_terminal_audit(session);
    result
}

pub(crate) fn persist_export_session(session: &Arc<Mutex<ExportSession>>) -> Result<(), String> {
    let snapshot = snapshot_export_record(session);
    let result = upsert_persisted_export_record(snapshot);
    emit_export_terminal_audit(session);
    result
}

// ---------------------------------------------------------------------------
// Terminal audit emission (Completed / Failed / Cancelled)
// ---------------------------------------------------------------------------
//
// Emission is centralized here because every success / failure / cancel path
// in `commands/` eventually calls `persist_scan_session` or
// `persist_export_session` to flush the final state to disk. Driving the
// audit event off the persistence hook keeps the audit trail in lock-step
// with the persisted archive.
//
// A global HashSet of already-emitted session ids prevents double-emission
// when the same session is persisted multiple times after entering its
// terminal state (which can happen when a worker writes one more log line
// and re-persists the session).

fn terminal_audit_state() -> &'static Mutex<HashSet<String>> {
    static EMITTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    EMITTED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn mark_terminal_emitted(key: String) -> bool {
    let mut guard = match terminal_audit_state().lock() {
        Ok(guard) => guard,
        Err(poison) => poison.into_inner(),
    };
    guard.insert(key)
}

fn emit_scan_terminal_audit(session: &Arc<Mutex<InventoryScanSession>>) {
    let (
        id,
        device_id,
        scan_type,
        status,
        files_found,
        errors_count,
        elapsed_seconds,
        completed_at_ms,
    ) = {
        let state = lock_or_recover(session, "scan session (audit hook)");
        (
            state.id.clone(),
            state.device_id.clone(),
            state.scan_type.clone(),
            state.progress.status.clone(),
            state.progress.files_found,
            state.progress.errors_count,
            state.progress.elapsed_seconds,
            state.completed_at_ms,
        )
    };

    let is_image = scan_type == "image";
    let kind = match status.as_str() {
        "completed" if is_image => crate::audit::AuditEventKind::ImagingCompleted,
        "completed" => crate::audit::AuditEventKind::ScanCompleted,
        "error" if is_image => crate::audit::AuditEventKind::ImagingFailed,
        "error" => crate::audit::AuditEventKind::ScanFailed,
        // Canceled scans already emit `ScanCanceled` at request time from
        // `commands::scan::cancel_scan`; don't duplicate it here.
        _ => return,
    };

    let key = format!("scan:{id}");
    if !mark_terminal_emitted(key) {
        return;
    }

    let details = serde_json::json!({
        "scan_id": id,
        "device_id": device_id,
        "scan_type": scan_type,
        "status": status,
        "files_found": files_found,
        "errors_count": errors_count,
        "elapsed_seconds": elapsed_seconds,
        "completed_at_ms": completed_at_ms,
    });

    crate::audit::record(kind, details);
}

fn emit_export_terminal_audit(session: &Arc<Mutex<ExportSession>>) {
    let (id, scan_id, status, exported_files, error_count, completed_at_ms) = {
        let state = lock_or_recover(session, "export session (audit hook)");
        (
            state.id.clone(),
            state.scan_id.clone(),
            state.progress.status.clone(),
            state.progress.exported_files,
            state.progress.errors.len(),
            state.completed_at_ms,
        )
    };

    let kind = match status.as_str() {
        "completed" => crate::audit::AuditEventKind::ExportCompleted,
        "error" => crate::audit::AuditEventKind::ExportFailed,
        _ => return,
    };

    let key = format!("export:{id}");
    if !mark_terminal_emitted(key) {
        return;
    }

    let details = serde_json::json!({
        "export_id": id,
        "scan_id": scan_id,
        "status": status,
        "exported_files": exported_files,
        "error_count": error_count,
        "completed_at_ms": completed_at_ms,
    });

    crate::audit::record(kind, details);
}

#[cfg(test)]
pub(crate) fn reset_terminal_audit_emission_for_tests() {
    if let Ok(mut guard) = terminal_audit_state().lock() {
        guard.clear();
    }
}

#[cfg(test)]
mod terminal_audit_tests {
    use super::{mark_terminal_emitted, reset_terminal_audit_emission_for_tests};

    #[test]
    fn mark_terminal_emitted_is_idempotent_per_key() {
        reset_terminal_audit_emission_for_tests();
        assert!(mark_terminal_emitted("scan:abc".into()));
        assert!(!mark_terminal_emitted("scan:abc".into()));
        assert!(mark_terminal_emitted("scan:def".into()));
        reset_terminal_audit_emission_for_tests();
        // After a reset we can re-emit for the same key again.
        assert!(mark_terminal_emitted("scan:abc".into()));
    }
}

pub(crate) fn append_scan_log(
    session: &Arc<Mutex<InventoryScanSession>>,
    level: &str,
    message: String,
) {
    let mut state = crate::commands::state::lock_or_recover(session, "scan session");
    push_technical_log(&mut state.logs, level, message);
    drop(state);

    if let Err(error) = persist_scan_session(session) {
        tracing::info!("append_scan_log: unable to persist session snapshot: {error}");
    }
}

pub(crate) fn append_export_log(session: &Arc<Mutex<ExportSession>>, level: &str, message: String) {
    let mut state = session.lock().expect("export session lock poisoned");
    push_technical_log(&mut state.logs, level, message);
    drop(state);

    if let Err(error) = persist_export_session(session) {
        tracing::info!("append_export_log: unable to persist export snapshot: {error}");
    }
}

pub(crate) fn push_technical_log(logs: &mut Vec<TechnicalLogEntry>, level: &str, message: String) {
    if logs.len() >= MAX_SESSION_LOGS {
        logs.remove(0);
    }
    logs.push(TechnicalLogEntry {
        timestamp_ms: unix_timestamp_ms(),
        level: level.to_string(),
        message,
    });
}

pub(crate) fn upsert_persisted_scan_record(record: PersistedScanRecord) -> Result<(), String> {
    upsert_persisted_scan_record_at(&scan_history_storage_path(), record)
}

pub(crate) fn upsert_persisted_export_record(record: PersistedExportRecord) -> Result<(), String> {
    upsert_persisted_export_record_at(&export_history_storage_path(), record)
}

pub(crate) fn load_persisted_scan_record(scan_id: &str) -> Option<PersistedScanRecord> {
    load_persisted_scan_archive()
        .scans
        .into_iter()
        .find(|record| record.summary.id == scan_id)
}

pub(crate) fn scan_control_handle(session: &Arc<Mutex<InventoryScanSession>>) -> Arc<ScanControl> {
    let state = crate::commands::state::lock_or_recover(session, "scan session");
    Arc::clone(&state.control)
}

pub(crate) fn scan_cancelled_error() -> String {
    SCAN_CANCELLED_SENTINEL.into()
}

pub(crate) fn is_scan_cancelled_error(error: &str) -> bool {
    error == SCAN_CANCELLED_SENTINEL
}

pub(crate) fn wait_for_scan_permission(
    session: &Arc<Mutex<InventoryScanSession>>,
) -> Result<(), String> {
    let control = scan_control_handle(session);
    let mut state = control.state.lock().expect("scan control lock poisoned");

    loop {
        if state.cancelled {
            return Err(scan_cancelled_error());
        }

        if !state.paused {
            return Ok(());
        }

        state = control
            .condvar
            .wait(state)
            .expect("scan control condvar lock poisoned");
    }
}

pub(crate) fn finalize_cancelled_scan(session: &Arc<Mutex<InventoryScanSession>>) {
    let mut state = crate::commands::state::lock_or_recover(session, "scan session");
    let should_log = state.progress.status != "cancelled";
    state.progress.status = "cancelled".into();
    state.progress.stage = "finalizing".into();
    state.completed_at_ms = Some(state.completed_at_ms.unwrap_or_else(unix_timestamp_ms));
    if should_log {
        push_technical_log(
            &mut state.logs,
            "warning",
            "Scan canceled at user request.".into(),
        );
    }
    drop(state);

    if let Err(error) = persist_scan_session(session) {
        tracing::info!("finalize_cancelled_scan: unable to persist session snapshot: {error}");
    }
}

pub(crate) fn request_scan_pause(session: &Arc<Mutex<InventoryScanSession>>) -> Result<(), String> {
    {
        let state = crate::commands::state::lock_or_recover(session, "scan session");
        if matches!(
            state.progress.status.as_str(),
            "completed" | "error" | "cancelled"
        ) {
            return Err("Only an active scan can be paused.".into());
        }
        if state.progress.status == "paused" {
            return Ok(());
        }
    }

    let control = scan_control_handle(session);
    {
        let mut control_state = control.state.lock().expect("scan control lock poisoned");
        control_state.paused = true;
    }

    let mut state = crate::commands::state::lock_or_recover(session, "scan session");
    state.progress.status = "paused".into();
    push_technical_log(
        &mut state.logs,
        "warning",
        "Scan paused by user request.".into(),
    );
    drop(state);

    if let Err(error) = persist_scan_session(session) {
        tracing::info!("request_scan_pause: unable to persist session snapshot: {error}");
    }

    Ok(())
}

pub(crate) fn request_scan_resume(
    session: &Arc<Mutex<InventoryScanSession>>,
) -> Result<(), String> {
    {
        let state = crate::commands::state::lock_or_recover(session, "scan session");
        if matches!(
            state.progress.status.as_str(),
            "completed" | "error" | "cancelled"
        ) {
            return Err("Only a paused scan can be resumed.".into());
        }
        if state.progress.status != "paused" {
            return Ok(());
        }
    }

    let control = scan_control_handle(session);
    {
        let mut control_state = control.state.lock().expect("scan control lock poisoned");
        control_state.paused = false;
        control.condvar.notify_all();
    }

    let mut state = crate::commands::state::lock_or_recover(session, "scan session");
    state.progress.status = "scanning".into();
    push_technical_log(
        &mut state.logs,
        "info",
        "Scan resumed by user request.".into(),
    );
    drop(state);

    if let Err(error) = persist_scan_session(session) {
        tracing::info!("request_scan_resume: unable to persist session snapshot: {error}");
    }

    Ok(())
}

pub(crate) fn request_scan_cancel(
    session: &Arc<Mutex<InventoryScanSession>>,
) -> Result<(), String> {
    {
        let state = crate::commands::state::lock_or_recover(session, "scan session");
        if matches!(
            state.progress.status.as_str(),
            "completed" | "error" | "cancelled"
        ) {
            return Err("Only an active scan can be stopped.".into());
        }
    }

    let control = scan_control_handle(session);
    {
        let mut control_state = control.state.lock().expect("scan control lock poisoned");
        control_state.cancelled = true;
        control_state.paused = false;
        control.condvar.notify_all();
    }

    finalize_cancelled_scan(session);
    Ok(())
}

pub(crate) fn load_persisted_scan_archive() -> PersistedScanArchive {
    load_persisted_scan_archive_from(&scan_history_storage_path())
}

pub(crate) fn load_persisted_export_archive() -> PersistedExportArchive {
    load_persisted_export_archive_from(&export_history_storage_path())
}

pub(crate) fn load_persisted_export_record(export_id: &str) -> Option<PersistedExportRecord> {
    load_persisted_export_archive()
        .exports
        .into_iter()
        .find(|record| record.summary.id == export_id)
}

pub(crate) fn scan_history_storage_path() -> PathBuf {
    if let Some(path) = env::var_os("RECUPERE_HISTORY_PATH") {
        return PathBuf::from(path);
    }

    if cfg!(test) {
        return env::temp_dir()
            .join(format!("recupere-test-{}", std::process::id()))
            .join("scan-history.json");
    }

    app_data_dir().join("scan-history.json")
}

pub(crate) fn export_history_storage_path() -> PathBuf {
    if let Some(path) = env::var_os("RECUPERE_EXPORT_HISTORY_PATH") {
        return PathBuf::from(path);
    }

    if cfg!(test) {
        return env::temp_dir()
            .join(format!("recupere-test-{}", std::process::id()))
            .join("export-history.json");
    }

    app_data_dir().join("export-history.json")
}

pub(crate) fn load_persisted_scan_archive_from(path: &Path) -> PersistedScanArchive {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(_) => return PersistedScanArchive::default(),
    };

    serde_json::from_str::<PersistedScanArchive>(&contents).unwrap_or_else(|error| {
        tracing::info!(
            "load_persisted_scan_archive_from: unable to parse {}: {}",
            path.to_string_lossy(),
            error
        );
        PersistedScanArchive::default()
    })
}

pub(crate) fn load_persisted_export_archive_from(path: &Path) -> PersistedExportArchive {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(_) => return PersistedExportArchive::default(),
    };

    match serde_json::from_str::<PersistedExportArchive>(&contents) {
        Ok(archive) => archive,
        Err(current_error) => match serde_json::from_str::<LegacyPersistedExportArchive>(&contents)
        {
            Ok(legacy_archive) => PersistedExportArchive {
                exports: legacy_archive
                    .exports
                    .into_iter()
                    .map(|summary| PersistedExportRecord {
                        summary,
                        logs: Vec::new(),
                    })
                    .collect(),
            },
            Err(legacy_error) => {
                tracing::info!(
                    "load_persisted_export_archive_from: unable to parse {} as current ({}) or legacy ({}) format",
                    path.to_string_lossy(),
                    current_error,
                    legacy_error
                );
                PersistedExportArchive::default()
            }
        },
    }
}

pub(crate) fn clear_local_history_at(
    scope: &str,
    scan_path: &Path,
    export_path: &Path,
    live_scan_sessions: usize,
    live_export_sessions: usize,
) -> Result<LocalHistoryPurgeResult, String> {
    let normalized_scope = normalize_history_scope(scope)?;
    let mut result = LocalHistoryPurgeResult {
        scope: normalized_scope.into(),
        removed_scan_records: 0,
        removed_export_records: 0,
        scan_archive_deleted: false,
        export_archive_deleted: false,
        live_scan_sessions: live_scan_sessions as u32,
        live_export_sessions: live_export_sessions as u32,
    };

    if matches!(normalized_scope, "scan" | "all") {
        result.removed_scan_records =
            load_persisted_scan_archive_from(scan_path).scans.len() as u32;
        result.scan_archive_deleted = delete_local_archive(scan_path, "scan")?;
    }

    if matches!(normalized_scope, "export" | "all") {
        result.removed_export_records = load_persisted_export_archive_from(export_path)
            .exports
            .len() as u32;
        result.export_archive_deleted = delete_local_archive(export_path, "export")?;
    }

    Ok(result)
}

pub(crate) fn upsert_persisted_scan_record_at(
    path: &Path,
    record: PersistedScanRecord,
) -> Result<(), String> {
    let mut archive = load_persisted_scan_archive_from(path);

    if let Some(existing) = archive
        .scans
        .iter_mut()
        .find(|existing| existing.summary.id == record.summary.id)
    {
        *existing = record;
    } else {
        archive.scans.push(record);
    }

    archive
        .scans
        .sort_by(|left, right| right.summary.started_at_ms.cmp(&left.summary.started_at_ms));
    archive.scans.truncate(MAX_PERSISTED_SCAN_RECORDS);

    write_persisted_scan_archive_to(path, &archive)
}

pub(crate) fn upsert_persisted_export_record_at(
    path: &Path,
    record: PersistedExportRecord,
) -> Result<(), String> {
    let mut archive = load_persisted_export_archive_from(path);

    if let Some(existing) = archive
        .exports
        .iter_mut()
        .find(|existing| existing.summary.id == record.summary.id)
    {
        *existing = record;
    } else {
        archive.exports.push(record);
    }

    archive
        .exports
        .sort_by(|left, right| right.summary.started_at_ms.cmp(&left.summary.started_at_ms));
    archive.exports.truncate(MAX_PERSISTED_EXPORT_RECORDS);

    write_persisted_export_archive_to(path, &archive)
}

pub(crate) fn recovered_files_count(results: &[RecoveredFile]) -> u32 {
    results.iter().filter(|file| file.is_deleted).count() as u32
}

pub(crate) fn fail_scan_session(session: &Arc<Mutex<InventoryScanSession>>, reason: String) {
    if is_scan_cancelled_error(&reason) {
        finalize_cancelled_scan(session);
        return;
    }

    let already_cancelled = {
        let state = crate::commands::state::lock_or_recover(session, "scan session");
        state.progress.status == "cancelled"
    };
    if already_cancelled {
        return;
    }

    let mut state = crate::commands::state::lock_or_recover(session, "scan session");
    state.progress.status = "error".into();
    state.progress.stage = "finalizing".into();
    state.completed_at_ms = Some(unix_timestamp_ms());
    push_technical_log(&mut state.logs, "error", format!("Scan failed: {reason}"));
    drop(state);

    if let Err(error) = persist_scan_session(session) {
        tracing::info!("fail_scan_session: unable to persist session snapshot: {error}");
    }
}

pub(crate) fn update_progress(
    session: &Arc<Mutex<InventoryScanSession>>,
    update: impl FnOnce(&mut ScanProgress),
) {
    let mut state = crate::commands::state::lock_or_recover(session, "scan session");
    update(&mut state.progress);
}

pub(crate) fn elapsed_seconds(started_at: &SystemTime) -> u64 {
    SystemTime::now()
        .duration_since(*started_at)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(crate) fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn write_text_report_to_path(path: &Path, content: &str) -> Result<(), String> {
    if content.trim().is_empty() {
        return Err("The report content is empty.".into());
    }

    write_binary_file_to_path(path, content.as_bytes(), "technical report")
}

pub(crate) fn write_binary_file_to_path(
    path: &Path,
    bytes: &[u8],
    label: &str,
) -> Result<(), String> {
    if bytes.is_empty() {
        return Err(format!("The {label} content is empty."));
    }

    if path.is_dir() {
        return Err(format!(
            "The selected {label} destination {} is a directory.",
            path.to_string_lossy(),
        ));
    }

    let parent = path.parent().ok_or_else(|| {
        format!(
            "The selected {label} destination {} has no writable parent directory.",
            path.to_string_lossy(),
        )
    })?;

    if !parent.exists() {
        return Err(format!(
            "The selected {label} directory {} does not exist.",
            parent.to_string_lossy(),
        ));
    }

    let temp_path = path.with_extension("tmp");
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp_path).map_err(|error| {
            format!(
                "Unable to write temporary {label} {}: {}",
                temp_path.to_string_lossy(),
                error,
            )
        })?;
        use std::io::Write as _;
        file.write_all(bytes)
            .map_err(|error| format!("Unable to write temporary {label}: {error}"))?;
        file.flush()
            .map_err(|error| format!("Unable to flush temporary {label}: {error}"))?;
    }

    if path.exists() {
        fs::remove_file(path).map_err(|error| {
            format!(
                "Unable to replace existing {label} {}: {}",
                path.to_string_lossy(),
                error,
            )
        })?;
    }

    fs::rename(&temp_path, path).map_err(|error| {
        format!(
            "Unable to finalize {label} {}: {}",
            path.to_string_lossy(),
            error,
        )
    })
}

// Filesystem / env helpers promoted from commands/mod.rs in Sprint 2.1 Pass C

pub(crate) fn normalize_history_scope(scope: &str) -> Result<&'static str, String> {
    match scope {
        "scan" => Ok("scan"),
        "export" => Ok("export"),
        "all" => Ok("all"),
        other => Err(format!(
            "Unsupported history purge scope `{other}`. Use `scan`, `export`, or `all`."
        )),
    }
}

pub(crate) fn app_data_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        if let Some(home) = user_home_dir() {
            return home.join("Library/Application Support/recupere");
        }
    }

    if cfg!(target_os = "windows") {
        if let Some(app_data) = env::var_os("LOCALAPPDATA").or_else(|| env::var_os("APPDATA")) {
            return PathBuf::from(app_data).join("recupere");
        }
    }

    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(data_home).join("recupere");
    }

    if let Some(home) = user_home_dir() {
        return home.join(".local/share/recupere");
    }

    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".recupere")
}

pub(crate) fn user_home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
}

pub(crate) fn write_persisted_scan_archive_to(
    path: &Path,
    archive: &PersistedScanArchive,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Unable to create local history directory {}: {}",
                parent.to_string_lossy(),
                error
            )
        })?;
    }

    let payload = serde_json::to_string_pretty(archive)
        .map_err(|error| format!("Unable to serialize local scan archive: {error}"))?;
    let temp_path = path.with_extension("tmp");

    fs::write(&temp_path, payload).map_err(|error| {
        format!(
            "Unable to write temporary scan archive {}: {}",
            temp_path.to_string_lossy(),
            error
        )
    })?;

    fs::rename(&temp_path, path).map_err(|error| {
        format!(
            "Unable to finalize scan archive {}: {}",
            path.to_string_lossy(),
            error
        )
    })
}

pub(crate) fn write_persisted_export_archive_to(
    path: &Path,
    archive: &PersistedExportArchive,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Unable to create local export history directory {}: {}",
                parent.to_string_lossy(),
                error
            )
        })?;
    }

    let payload = serde_json::to_string_pretty(archive)
        .map_err(|error| format!("Unable to serialize local export archive: {error}"))?;
    let temp_path = path.with_extension("tmp");

    fs::write(&temp_path, payload).map_err(|error| {
        format!(
            "Unable to write temporary export archive {}: {}",
            temp_path.to_string_lossy(),
            error
        )
    })?;

    fs::rename(&temp_path, path).map_err(|error| {
        format!(
            "Unable to finalize export archive {}: {}",
            path.to_string_lossy(),
            error
        )
    })
}

pub(crate) fn delete_local_archive(path: &Path, label: &str) -> Result<bool, String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "Unable to delete local {label} archive {}: {}",
            path.to_string_lossy(),
            error
        )),
    }
}
