// ============================================================================
// Récupère — Filesystem memory commands (Chantier 82 B4)
// ============================================================================
// Tauri bindings for the `filesystem_memory` module. All commands are
// read-only on the source and refuse to persist the snapshot store inside a
// scanned source path.
// ============================================================================

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::filesystem_memory::{
    capture_snapshot, compute_diff, load_all_snapshots, load_monitoring_policy,
    missing_file_insights, persist_snapshot, recovery_context_hints_for, rotate_snapshots,
    save_monitoring_policy, scheduler, store_file_path, CaptureOptions, FilesystemSnapshot,
    MissingFileInsight, MonitoringPolicy, RecoveryContextHint, RecoveryContextLookupInput,
    SnapshotDiff,
};

fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn canonicalize_target(target_path: &str) -> Result<PathBuf, String> {
    let trimmed = target_path.trim();
    if trimmed.is_empty() {
        return Err("Filesystem memory: the target path must not be empty.".into());
    }
    let buf = PathBuf::from(trimmed);
    match buf.canonicalize() {
        Ok(canon) => Ok(canon),
        Err(_) => Ok(buf),
    }
}

fn ensure_store_is_outside_source(target_path: &Path) -> Result<(), String> {
    let store_file = store_file_path()?;
    let store_dir = store_file
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or(store_file);
    let canonical_store = std::fs::canonicalize(&store_dir).unwrap_or(store_dir);
    let canonical_target =
        std::fs::canonicalize(target_path).unwrap_or_else(|_| target_path.to_path_buf());
    if canonical_store.starts_with(&canonical_target) {
        return Err(format!(
            "Refusing to snapshot {}: the filesystem-memory store lives under that path. \
             Move Récupère's data directory outside the source volume and retry.",
            canonical_target.to_string_lossy()
        ));
    }
    Ok(())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn create_filesystem_memory_snapshot(
    target_path: String,
    volume_fingerprint: Option<String>,
) -> Result<FilesystemSnapshot, String> {
    let target = canonicalize_target(&target_path)?;
    ensure_store_is_outside_source(&target)?;

    let snapshot_id = format!("fsm-{}", unix_timestamp_ms());
    tracing::info!(
        "create_filesystem_memory_snapshot: id={} target={}",
        snapshot_id,
        target.to_string_lossy()
    );

    let options = CaptureOptions {
        volume_fingerprint,
        ..CaptureOptions::default()
    };
    let snapshot = match capture_snapshot(&snapshot_id, &target, &options) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            crate::audit::record(
                crate::audit::AuditEventKind::FilesystemSnapshotFailed,
                serde_json::json!({
                    "snapshot_id": snapshot_id,
                    "target_path": target.to_string_lossy(),
                    "error": error,
                }),
            );
            return Err(error);
        }
    };
    persist_snapshot(&snapshot)?;
    rotate_snapshots(&snapshot.target_path)?;
    crate::audit::record(
        crate::audit::AuditEventKind::FilesystemSnapshotCaptured,
        serde_json::json!({
            "snapshot_id": snapshot.id,
            "target_path": snapshot.target_path,
            "files_indexed": snapshot.files_indexed,
            "status": snapshot.status,
            "errors_count": snapshot.errors.len(),
            "volume_fingerprint": snapshot.volume_fingerprint,
        }),
    );
    tracing::info!(
        "create_filesystem_memory_snapshot: id={} status={:?} files_indexed={} errors={}",
        snapshot.id,
        snapshot.status,
        snapshot.files_indexed,
        snapshot.errors.len()
    );
    Ok(snapshot)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn list_filesystem_memory_snapshots() -> Result<Vec<FilesystemSnapshot>, String> {
    let snapshots = load_all_snapshots()?;
    tracing::info!(
        "list_filesystem_memory_snapshots: returned {} snapshot(s)",
        snapshots.len()
    );
    Ok(snapshots)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn compute_filesystem_memory_diff(
    baseline_id: String,
    head_id: String,
) -> Result<SnapshotDiff, String> {
    tracing::info!(
        "compute_filesystem_memory_diff: baseline={} head={}",
        baseline_id,
        head_id
    );
    let snapshots = load_all_snapshots()?;
    let baseline = snapshots
        .iter()
        .find(|s| s.id == baseline_id)
        .ok_or_else(|| {
            format!("Filesystem memory: baseline snapshot `{baseline_id}` not found.")
        })?;
    let head = snapshots
        .iter()
        .find(|s| s.id == head_id)
        .ok_or_else(|| format!("Filesystem memory: head snapshot `{head_id}` not found."))?;
    let diff = match compute_diff(baseline, head) {
        Ok(diff) => diff,
        Err(error) => {
            crate::audit::record(
                crate::audit::AuditEventKind::FilesystemDiffFailed,
                serde_json::json!({
                    "baseline_id": baseline_id,
                    "head_id": head_id,
                    "target_path": baseline.target_path,
                    "error": error,
                }),
            );
            return Err(error);
        }
    };
    crate::audit::record(
        crate::audit::AuditEventKind::FilesystemDiffComputed,
        serde_json::json!({
            "baseline_id": baseline_id,
            "head_id": head_id,
            "target_path": diff.target_path,
            "changes_count": diff.changes.len(),
            "baseline_volume_fingerprint": baseline.volume_fingerprint,
            "head_volume_fingerprint": head.volume_fingerprint,
        }),
    );
    tracing::info!(
        "compute_filesystem_memory_diff: changes={} baseline={} head={}",
        diff.changes.len(),
        baseline_id,
        head_id
    );
    Ok(diff)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_filesystem_memory_missing_files(
    baseline_id: String,
    head_id: String,
) -> Result<Vec<MissingFileInsight>, String> {
    let snapshots = load_all_snapshots()?;
    let baseline = snapshots
        .iter()
        .find(|s| s.id == baseline_id)
        .ok_or_else(|| {
            format!("Filesystem memory: baseline snapshot `{baseline_id}` not found.")
        })?;
    let head = snapshots
        .iter()
        .find(|s| s.id == head_id)
        .ok_or_else(|| format!("Filesystem memory: head snapshot `{head_id}` not found."))?;
    let diff = compute_diff(baseline, head)?;
    Ok(missing_file_insights(
        &diff,
        baseline.captured_at_ms,
        head.captured_at_ms,
    ))
}

/// Return the persisted monitoring policy, defaulting to `Manual` when nothing
/// has been written yet. Paired with `save_filesystem_memory_policy`.
#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_filesystem_memory_policy() -> Result<MonitoringPolicy, String> {
    let policy = load_monitoring_policy()?;
    tracing::info!(
        "get_filesystem_memory_policy: {}",
        serde_json::to_string(&policy).unwrap_or_else(|_| "<unserializable>".into())
    );
    Ok(policy)
}

/// Persist a new monitoring policy and restart the passive scheduler so the UI
/// toggle takes effect immediately. The policy is normalised (interval floor)
/// before it hits the disk so the UI can never bypass the minimum interval.
#[cfg_attr(feature = "desktop", tauri::command)]
pub fn save_filesystem_memory_policy(policy: MonitoringPolicy) -> Result<MonitoringPolicy, String> {
    let normalized = policy.normalize();
    save_monitoring_policy(&normalized)?;
    scheduler::start_with_policy(normalized.clone());
    crate::audit::record(
        crate::audit::AuditEventKind::SettingsChanged,
        serde_json::json!({
            "setting": "filesystem_memory_policy",
            "policy": normalized,
        }),
    );
    tracing::info!(
        "save_filesystem_memory_policy: {}",
        serde_json::to_string(&normalized).unwrap_or_else(|_| "<unserializable>".into())
    );
    Ok(normalized)
}

/// Non-invasive enrichment: the UI sends a list of file names from the current
/// scan's results and we return a hint per name that matches a "missing" entry
/// in the diff. No RecoveredFile is mutated in the backend — the frontend
/// layers the hints on top when it renders the results card. This is how B5
/// ties the filesystem-memory layer back to the scan results without changing
/// core recovery logic.
#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_recovery_context_hints(
    baseline_id: String,
    head_id: String,
    recovered_files: Vec<RecoveryContextLookupInput>,
) -> Result<Vec<RecoveryContextHint>, String> {
    if recovered_files.is_empty() {
        return Ok(Vec::new());
    }
    let snapshots = load_all_snapshots()?;
    let baseline = snapshots
        .iter()
        .find(|s| s.id == baseline_id)
        .ok_or_else(|| {
            format!("Filesystem memory: baseline snapshot `{baseline_id}` not found.")
        })?;
    let head = snapshots
        .iter()
        .find(|s| s.id == head_id)
        .ok_or_else(|| format!("Filesystem memory: head snapshot `{head_id}` not found."))?;
    let diff = compute_diff(baseline, head)?;
    Ok(recovery_context_hints_for(
        &diff,
        &recovered_files,
        baseline.captured_at_ms,
        head.captured_at_ms,
    ))
}
