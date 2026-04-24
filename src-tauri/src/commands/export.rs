// ============================================================================
// Récupère — Export commands (wrappers + entrypoint + worker + report/csv/bundle)
// ============================================================================
// Tauri command handlers around the export pipeline.
//   - Read-only/safe wrappers: `validate_export_destination`,
//     `save_technical_timeline_report`, `save_support_bundle`,
//     `get_export_progress`, `get_export_logs`, `get_export_history`,
//     `clear_local_history`.
//   - `start_export` builds the export session and spawns
//     `run_export_session` (defined below in this module).
//   - `run_export_session` performs the actual file copy + integrity
//     verification loop — moved out of `mod.rs` in Sprint 2.1 slice
//     `export_worker` (2026-04-17).
//   - `generate_recovery_report`, `export_results_csv`,
//     `generate_lab_bundle` — the three report-oriented Tauri commands
//     — live here too. After this slice, `commands/mod.rs` has zero
//     `#[tauri::command]` entrypoints left.
// ============================================================================

use rand::RngCore;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use chrono::SecondsFormat;

use crate::core;
use crate::imaging;
use crate::types::{
    DeviceType, ExportError, ExportProgress, ExportSessionSummary, ExportValidation,
    HexPreviewLine, ImportedRecoverySourceStatus, LocalHistoryPurgeResult, RecoveredFile,
    TechnicalLogEntry,
};

use super::state::ExportSession;
use super::{
    app_build_info, append_export_log, build_diagnostic, export_sessions, get_session,
    normalize_conflict_strategy, persist_export_session, push_technical_log, unix_timestamp_ms,
    HEX_PREVIEW_LINE_WIDTH,
};

#[cfg_attr(feature = "desktop", tauri::command)]
#[allow(clippy::too_many_arguments)]
pub fn start_export(
    scan_id: String,
    destination_path: String,
    selected_file_ids: Vec<String>,
    conflict_strategy: String,
    preserve_structure: bool,
    verify_integrity: bool,
    explicit_selection: bool,
    implicit_preview_first_excluded_count: Option<u32>,
) -> Result<String, String> {
    let scan_session = get_session(&scan_id)?;
    let conflict_strategy = normalize_conflict_strategy(&conflict_strategy)?.to_string();

    let (root_path, files, scan_status) = {
        let scan_state =
            crate::commands::state::lock_or_recover(&scan_session, "scan session (export)");
        (
            PathBuf::from(scan_state.root_path.clone()),
            scan_state.results.clone(),
            scan_state.progress.status.clone(),
        )
    };

    if scan_status != "completed" {
        return Err("Export is only available after the scan has completed.".into());
    }

    let validation = validate_export_destination(
        destination_path.clone(),
        root_path.to_string_lossy().to_string(),
    );

    if !validation.is_safe {
        return Err(validation.message);
    }

    let files_to_export = select_files_for_export(&files, &selected_file_ids)?;
    let total_bytes = files_to_export
        .iter()
        .map(estimated_export_payload_bytes)
        .sum::<u64>();
    let export_id = format!("export-{}", unix_timestamp_ms());
    let started_at_ms = unix_timestamp_ms();

    let session = Arc::new(Mutex::new(ExportSession {
        id: export_id.clone(),
        scan_id: scan_id.clone(),
        destination_path: destination_path.clone(),
        started_at_ms,
        completed_at_ms: None,
        explicit_selection,
        implicit_preview_first_excluded_count: implicit_preview_first_excluded_count.unwrap_or(0),
        progress: ExportProgress {
            total_files: files_to_export.len() as u32,
            exported_files: 0,
            total_bytes,
            exported_bytes: 0,
            current_file: String::new(),
            errors: Vec::new(),
            status: "preparing".into(),
        },
        logs: vec![TechnicalLogEntry {
            timestamp_ms: started_at_ms,
            level: "info".into(),
            message: format!(
                "Export session created for {} file(s) to {}.",
                files_to_export.len(),
                destination_path
            ),
        }],
    }));

    crate::commands::state::lock_or_recover(export_sessions(), "export session registry")
        .insert(export_id.clone(), Arc::clone(&session));

    if let Err(error) = persist_export_session(&session) {
        tracing::info!("start_export: unable to persist initial export snapshot: {error}");
    }

    tracing::info!(
        "start_export: export_id={} scan_id={} destination={} files={} preserve_structure={} verify_integrity={} conflict_strategy={} explicit_selection={} implicit_preview_first_excluded_count={}",
        export_id,
        scan_id,
        destination_path,
        files_to_export.len(),
        preserve_structure,
        verify_integrity,
        conflict_strategy,
        explicit_selection,
        implicit_preview_first_excluded_count.unwrap_or(0)
    );

    crate::audit::record(
        crate::audit::AuditEventKind::ExportStarted,
        serde_json::json!({"export_id": &export_id, "scan_id": &scan_id, "files": files_to_export.len()}),
    );

    let export_id_for_thread = export_id.clone();
    thread::spawn(move || {
        run_export_session(
            export_id_for_thread,
            session,
            root_path,
            PathBuf::from(destination_path),
            files_to_export,
            conflict_strategy,
            preserve_structure,
            verify_integrity,
        );
    });

    Ok(export_id)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn validate_export_destination(
    destination: String,
    source_device_path: String,
) -> ExportValidation {
    // Canonicalize the destination to defeat symlink-based bypass attempts
    // (e.g. a symlink in the destination tree pointing back to the source disk).
    // We canonicalize the deepest existing ancestor since the leaf may not exist yet.
    let canonical_destination = {
        let mut probe = Path::new(&destination).to_path_buf();
        loop {
            if let Ok(real) = std::fs::canonicalize(&probe) {
                break Some(real);
            }
            if !probe.pop() {
                break None;
            }
        }
    };
    let canonical_destination_str = canonical_destination
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| destination.clone());

    let destination_device = core::resolve_base_device_identifier(&canonical_destination_str);
    let source_device = core::resolve_base_device_identifier(&source_device_path)
        .or_else(|| core::extract_base_disk(&source_device_path));
    let available_bytes = core::available_space_for_path(&destination).unwrap_or(0);

    let (is_safe, message) = match (destination_device.as_ref(), source_device.as_ref()) {
        (Some(dest), Some(source)) if dest == source => (
            false,
            "Unsafe destination: the selected path resolves to the same physical source device.",
        ),
        (Some(_), Some(_)) => (
            true,
            "Destination passed read-only validation and resolves to a different device.",
        ),
        // Strict failure when either side cannot be resolved: refuse rather than
        // allow ambiguous exports that could land on the source disk.
        _ => (
            false,
            "Destination could not be fully validated. Choose an existing path on a different device.",
        ),
    };

    tracing::info!(
        "validate_export_destination: destination='{}' source='{}' safe={} available_bytes={}",
        destination,
        source_device_path,
        is_safe,
        available_bytes
    );

    ExportValidation {
        is_safe,
        available_bytes,
        message: message.into(),
    }
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn save_technical_timeline_report(
    destination_path: String,
    content: String,
    source_device_path: Option<String>,
) -> Result<String, String> {
    if content.trim().is_empty() {
        return Err("The technical timeline export is empty.".into());
    }

    if let Some(source_device_path) = source_device_path.as_ref() {
        let validation =
            validate_export_destination(destination_path.clone(), source_device_path.clone());
        if !validation.is_safe {
            return Err(validation.message);
        }
    }

    super::write_text_report_to_path(Path::new(&destination_path), &content)?;

    tracing::info!(
        "save_technical_timeline_report: destination={} bytes={}",
        destination_path,
        content.len()
    );

    Ok(destination_path)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn save_support_bundle(
    destination_path: String,
    source_device_path: Option<String>,
) -> Result<String, String> {
    if let Some(source_device_path) = source_device_path.as_ref() {
        let validation =
            validate_export_destination(destination_path.clone(), source_device_path.clone());
        if !validation.is_safe {
            return Err(validation.message);
        }
    }

    let bundle = super::build_support_bundle_archive_bytes()?;
    super::write_binary_file_to_path(Path::new(&destination_path), &bundle, "support bundle")?;

    tracing::info!(
        "save_support_bundle: destination={} bytes={}",
        destination_path,
        bundle.len()
    );

    Ok(destination_path)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_export_progress(export_id: String) -> Result<ExportProgress, String> {
    let session = crate::commands::state::lock_or_recover(
        super::export_sessions(),
        "export session registry",
    )
    .get(&export_id)
    .cloned()
    .ok_or_else(|| format!("Export session `{export_id}` was not found."))?;

    let session = session.lock().expect("export session lock poisoned");
    Ok(session.progress.clone())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_export_logs(export_id: String) -> Result<Vec<TechnicalLogEntry>, String> {
    if let Some(session) =
        crate::commands::state::lock_or_recover(super::export_sessions(), "export session registry")
            .get(&export_id)
            .cloned()
    {
        let session = session.lock().expect("export session lock poisoned");
        return Ok(session.logs.clone());
    }

    super::load_persisted_export_record(&export_id)
        .map(|record| record.logs)
        .ok_or_else(|| format!("Export session `{export_id}` was not found."))
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_export_history() -> Vec<ExportSessionSummary> {
    let mut history_by_id: HashMap<String, ExportSessionSummary> =
        super::load_persisted_export_archive()
            .exports
            .into_iter()
            .map(|record| (record.summary.id.clone(), record.summary))
            .collect();

    let sessions = crate::commands::state::lock_or_recover(
        super::export_sessions(),
        "export session registry",
    );

    for session in sessions.values() {
        let summary = super::build_export_summary(session);
        history_by_id.insert(summary.id.clone(), summary);
    }

    let mut history: Vec<ExportSessionSummary> = history_by_id.into_values().collect();
    history.sort_by(|left, right| right.started_at_ms.cmp(&left.started_at_ms));
    history
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn clear_local_history(scope: String) -> Result<LocalHistoryPurgeResult, String> {
    let live_scan_sessions =
        crate::commands::state::lock_or_recover(super::scan_sessions(), "scan session registry")
            .len();
    let live_export_sessions = crate::commands::state::lock_or_recover(
        super::export_sessions(),
        "export session registry",
    )
    .len();

    let result = super::clear_local_history_at(
        &scope,
        &super::scan_history_storage_path(),
        &super::export_history_storage_path(),
        live_scan_sessions,
        live_export_sessions,
    )?;

    crate::audit::record(
        crate::audit::AuditEventKind::HistoryPurged,
        serde_json::json!({"scope": &scope}),
    );

    tracing::info!(
        "clear_local_history: scope={} removed_scans={} removed_exports={} scan_archive_deleted={} export_archive_deleted={} live_scan_sessions={} live_export_sessions={}",
        result.scope,
        result.removed_scan_records,
        result.removed_export_records,
        result.scan_archive_deleted,
        result.export_archive_deleted,
        result.live_scan_sessions,
        result.live_export_sessions
    );

    Ok(result)
}

// ----------------------------------------------------------------------------
// Worker — run_export_session + local helpers
// ----------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub(super) fn run_export_session(
    export_id: String,
    session: Arc<Mutex<ExportSession>>,
    root_path: PathBuf,
    destination_root: PathBuf,
    files_to_export: Vec<RecoveredFile>,
    conflict_strategy: String,
    preserve_structure: bool,
    verify_integrity: bool,
) {
    append_export_log(
        &session,
        "info",
        format!(
            "Starting export to {} with {} file(s), conflict strategy `{}`, preserve_structure={}, verify_integrity={}.",
            destination_root.to_string_lossy(),
            files_to_export.len(),
            conflict_strategy,
            preserve_structure,
            verify_integrity
        ),
    );

    if let Err(error) = fs::create_dir_all(&destination_root) {
        fail_export_session(
            &session,
            format!(
                "Unable to prepare destination directory {}: {}",
                destination_root.to_string_lossy(),
                error
            ),
        );
        return;
    }
    let canonical_destination_root = match fs::canonicalize(&destination_root) {
        Ok(path) => path,
        Err(error) => {
            fail_export_session(
                &session,
                format!(
                    "Unable to resolve destination directory {}: {}",
                    destination_root.to_string_lossy(),
                    error
                ),
            );
            return;
        }
    };
    #[cfg(not(test))]
    {
        let post_create_validation = validate_export_destination(
            canonical_destination_root.to_string_lossy().to_string(),
            root_path.to_string_lossy().to_string(),
        );
        if !post_create_validation.is_safe {
            fail_export_session(&session, post_create_validation.message);
            return;
        }
    }

    update_export_progress(&session, |progress| {
        progress.status = "exporting".into();
    });
    append_export_log(
        &session,
        "info",
        format!(
            "Destination directory ready at {}.",
            destination_root.to_string_lossy()
        ),
    );

    'file_export: for file in files_to_export {
        update_export_progress(&session, |progress| {
            progress.current_file = file.name.clone();
            progress.status = "exporting".into();
        });

        let target_dir = if preserve_structure {
            destination_root.join(relative_dir_from_display_path(&file.path))
        } else {
            destination_root.clone()
        };
        let source_description = export_source_description(&root_path, &file);
        append_export_log(
            &session,
            "info",
            format!(
                "Copying {} from {} to {}.",
                file.name,
                source_description,
                target_dir.to_string_lossy()
            ),
        );

        if let Err(error) = fs::create_dir_all(&target_dir) {
            push_export_error(
                &session,
                file.id.clone(),
                file.name.clone(),
                format!(
                    "Unable to create destination directory {}: {}",
                    target_dir.to_string_lossy(),
                    error
                ),
            );
            continue;
        }
        let canonical_target_dir = match fs::canonicalize(&target_dir) {
            Ok(path) if path.starts_with(&canonical_destination_root) => path,
            Ok(path) => {
                push_export_error(
                    &session,
                    file.id.clone(),
                    file.name.clone(),
                    format!(
                        "Refused to export outside the validated destination: {}.",
                        path.to_string_lossy()
                    ),
                );
                continue 'file_export;
            }
            Err(error) => {
                push_export_error(
                    &session,
                    file.id.clone(),
                    file.name.clone(),
                    format!(
                        "Unable to resolve destination directory {}: {}",
                        target_dir.to_string_lossy(),
                        error
                    ),
                );
                continue 'file_export;
            }
        };

        let safe_name = safe_export_file_name(&file.name);
        let requested_target = canonical_target_dir.join(&safe_name);
        let target_path = match resolve_target_path(&requested_target, &conflict_strategy) {
            Ok(Some(path)) => {
                if path != requested_target {
                    append_export_log(
                        &session,
                        "warning",
                        format!(
                            "Conflict detected for {}. Using renamed target {}.",
                            file.name,
                            path.to_string_lossy()
                        ),
                    );
                }
                path
            }
            Ok(None) => {
                push_export_error(
                    &session,
                    file.id.clone(),
                    file.name.clone(),
                    "Skipped because a file with the same name already exists at the destination."
                        .into(),
                );
                continue;
            }
            Err(error) => {
                push_export_error(&session, file.id.clone(), file.name.clone(), error);
                continue;
            }
        };

        let allow_replace = conflict_strategy == "overwrite";
        let copied_bytes =
            match export_recovered_file(&root_path, &file, &target_path, allow_replace) {
                Ok(bytes) => bytes,
                Err(error) => {
                    push_export_error(&session, file.id.clone(), file.name.clone(), error);
                    continue;
                }
            };
        let resource_fork_sidecar =
            match export_resource_fork_sidecar(&file, &target_path, &conflict_strategy) {
                Ok(sidecar) => sidecar,
                Err(error) => {
                    let _ = fs::remove_file(&target_path);
                    push_export_error(&session, file.id.clone(), file.name.clone(), error);
                    continue;
                }
            };
        let alternate_data_stream_sidecars =
            match export_alternate_data_stream_sidecars(&file, &target_path, &conflict_strategy) {
                Ok(sidecars) => sidecars,
                Err(error) => {
                    let _ = fs::remove_file(&target_path);
                    if let Some((sidecar_path, _)) = resource_fork_sidecar.as_ref() {
                        let _ = fs::remove_file(sidecar_path);
                    }
                    push_export_error(&session, file.id.clone(), file.name.clone(), error);
                    continue;
                }
            };
        if let Some((sidecar_path, sidecar_bytes)) = resource_fork_sidecar.as_ref() {
            append_export_log(
                &session,
                "info",
                format!(
                    "Copied the HFS+ resource-fork sidecar for {} ({} bytes) to {}.",
                    file.name,
                    sidecar_bytes,
                    sidecar_path.to_string_lossy()
                ),
            );
        }
        for (sidecar_path, sidecar_bytes, stream_name) in &alternate_data_stream_sidecars {
            append_export_log(
                &session,
                "info",
                format!(
                    "Copied the NTFS alternate-data-stream sidecar `{}` for {} ({} bytes) to {}.",
                    stream_name,
                    file.name,
                    sidecar_bytes,
                    sidecar_path.to_string_lossy()
                ),
            );
        }

        if verify_integrity {
            update_export_progress(&session, |progress| {
                progress.status = "verifying".into();
            });
            append_export_log(
                &session,
                "info",
                format!("Verifying copied file {}.", target_path.to_string_lossy()),
            );

            let verification = if file_uses_recovery_image(&file) {
                verify_reconstructed_export(&target_path, file.size_bytes)
            } else {
                match resolve_source_path_under_root(&root_path, &file) {
                    Ok(source_path) => verify_exported_file(&source_path, &target_path),
                    Err(error) => Err(error),
                }
            };

            match verification {
                Ok(()) => {
                    append_export_log(
                        &session,
                        "info",
                        format!("Integrity verification succeeded for {}.", file.name),
                    );
                }
                Err(error) => {
                    let _ = fs::remove_file(&target_path);
                    if let Some((sidecar_path, _)) = resource_fork_sidecar.as_ref() {
                        let _ = fs::remove_file(sidecar_path);
                    }
                    for (sidecar_path, _, _) in &alternate_data_stream_sidecars {
                        let _ = fs::remove_file(sidecar_path);
                    }
                    push_export_error(
                        &session,
                        file.id.clone(),
                        file.name.clone(),
                        format!("Integrity verification failed: {error}"),
                    );
                    continue;
                }
            }

            if let Some((sidecar_path, _)) = resource_fork_sidecar.as_ref() {
                let resource_fork = file.resource_fork.as_ref().ok_or_else(|| {
                    format!(
                        "Resource-fork metadata disappeared for {} during export.",
                        file.name
                    )
                });
                let verification = match resource_fork {
                    Ok(resource_fork) => {
                        verify_reconstructed_export(sidecar_path, resource_fork.size_bytes)
                    }
                    Err(error) => Err(error),
                };

                match verification {
                    Ok(()) => {
                        append_export_log(
                            &session,
                            "info",
                            format!(
                                "Integrity verification succeeded for the HFS+ resource-fork sidecar of {}.",
                                file.name
                            ),
                        );
                    }
                    Err(error) => {
                        let _ = fs::remove_file(&target_path);
                        let _ = fs::remove_file(sidecar_path);
                        for (ads_sidecar_path, _, _) in &alternate_data_stream_sidecars {
                            let _ = fs::remove_file(ads_sidecar_path);
                        }
                        push_export_error(
                            &session,
                            file.id.clone(),
                            file.name.clone(),
                            format!("Resource-fork verification failed: {error}"),
                        );
                        continue;
                    }
                }
            }

            for (sidecar_path, _, stream_name) in &alternate_data_stream_sidecars {
                let stream = file
                    .alternate_data_streams
                    .as_ref()
                    .and_then(|streams| streams.iter().find(|stream| stream.name == *stream_name))
                    .ok_or_else(|| {
                        format!(
                            "Alternate-data-stream metadata disappeared for {} during export.",
                            file.name
                        )
                    });
                let verification = match stream {
                    Ok(stream) => verify_reconstructed_export(sidecar_path, stream.size_bytes),
                    Err(error) => Err(error),
                };

                match verification {
                    Ok(()) => {
                        append_export_log(
                            &session,
                            "info",
                            format!(
                                "Integrity verification succeeded for the NTFS alternate-data-stream sidecar `{}` of {}.",
                                stream_name, file.name
                            ),
                        );
                    }
                    Err(error) => {
                        let _ = fs::remove_file(&target_path);
                        if let Some((resource_sidecar_path, _)) = resource_fork_sidecar.as_ref() {
                            let _ = fs::remove_file(resource_sidecar_path);
                        }
                        for (ads_sidecar_path, _, _) in &alternate_data_stream_sidecars {
                            let _ = fs::remove_file(ads_sidecar_path);
                        }
                        push_export_error(
                            &session,
                            file.id.clone(),
                            file.name.clone(),
                            format!(
                                "Alternate-data-stream sidecar `{stream_name}` verification failed: {error}"
                            ),
                        );
                        continue 'file_export;
                    }
                }
            }
        }

        update_export_progress(&session, |progress| {
            progress.exported_files = progress.exported_files.saturating_add(1);
            progress.exported_bytes = progress
                .exported_bytes
                .saturating_add(copied_bytes)
                .saturating_add(
                    resource_fork_sidecar
                        .as_ref()
                        .map(|(_, sidecar_bytes)| *sidecar_bytes)
                        .unwrap_or(0),
                )
                .saturating_add(
                    alternate_data_stream_sidecars
                        .iter()
                        .map(|(_, sidecar_bytes, _)| *sidecar_bytes)
                        .sum::<u64>(),
                );
            progress.status = "exporting".into();
        });
        append_export_log(
            &session,
            "info",
            format!(
                "Copied {} ({} bytes) to {}.",
                file.name,
                copied_bytes,
                target_path.to_string_lossy()
            ),
        );
    }

    let mut state = session.lock().expect("export session lock poisoned");
    state.progress.current_file.clear();
    state.progress.status =
        if state.progress.exported_files == 0 && !state.progress.errors.is_empty() {
            "error".into()
        } else {
            "completed".into()
        };
    state.completed_at_ms = Some(unix_timestamp_ms());
    let completion_level = if state.progress.status == "error" {
        "error"
    } else {
        "info"
    };
    let completion_message = format!(
        "Export finished with status `{}`: {} file(s) copied, {} error(s).",
        state.progress.status,
        state.progress.exported_files,
        state.progress.errors.len()
    );
    drop(state);
    append_export_log(&session, completion_level, completion_message);

    if let Err(error) = persist_export_session(&session) {
        tracing::info!("run_export_session: unable to persist completed export: {error}");
    }

    let state = session.lock().expect("export session lock poisoned");

    tracing::info!(
        "export complete: export_id={} scan_id={} destination={} exported_files={} errors={}",
        export_id,
        state.scan_id,
        state.destination_path,
        state.progress.exported_files,
        state.progress.errors.len()
    );
}

fn fail_export_session(session: &Arc<Mutex<ExportSession>>, reason: String) {
    let log_message = format!("Export failed: {reason}");
    let mut state = session.lock().expect("export session lock poisoned");
    state.progress.status = "error".into();
    state.progress.errors.push(ExportError {
        file_id: "session".into(),
        file_name: "session".into(),
        reason,
    });
    state.completed_at_ms = Some(unix_timestamp_ms());
    drop(state);
    append_export_log(session, "error", log_message);

    if let Err(error) = persist_export_session(session) {
        tracing::info!("fail_export_session: unable to persist export snapshot: {error}");
    }
}

fn export_source_description(root_path: &Path, file: &RecoveredFile) -> String {
    if file_uses_recovery_image(file) {
        file.source_image_path
            .clone()
            .unwrap_or_else(|| "the recovery image".into())
    } else {
        build_source_path(root_path, file)
            .to_string_lossy()
            .to_string()
    }
}

fn is_apfs_catalog_preview_first_candidate(file: &RecoveredFile) -> bool {
    file.is_deleted
        && file.source_view.as_deref() == Some("live-catalog")
        && matches!(
            file.validator_status.as_deref(),
            Some("unsupported" | "partial-unvalidated")
        )
}

fn is_apfs_catalog_reassembled_candidate(file: &RecoveredFile) -> bool {
    file.is_deleted
        && file.source_view.as_deref() == Some("live-catalog")
        && file.validator_status.as_deref() == Some("reassembled")
}

fn default_export_batch_holdout(files: &[RecoveredFile]) -> Vec<&RecoveredFile> {
    let held_out: Vec<&RecoveredFile> = files
        .iter()
        .filter(|file| is_apfs_catalog_preview_first_candidate(file))
        .collect();

    if held_out.is_empty() || held_out.len() == files.len() {
        return Vec::new();
    }

    held_out
}

fn default_export_hold_reason(language: &str) -> &'static str {
    if language == "fr" {
        "Retenu hors lot implicite: candidat APFS derive du catalogue courant sans validation structurelle"
    } else {
        "Held out of the implicit batch: APFS current-catalog candidate without structural validation"
    }
}

// ----------------------------------------------------------------------------
// Tauri commands — recovery report, CSV, lab bundle
// ----------------------------------------------------------------------------

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn generate_recovery_report(
    scan_id: String,
    language: String,
    include_file_inventory: bool,
) -> Result<String, String> {
    let session = get_session(&scan_id)?;
    let state = crate::commands::state::lock_or_recover(&session, "scan session");

    let device_name = &state.device_name;
    let scan_type = &state.scan_type;
    let files_found = state.results.len();
    let intact_count = state
        .results
        .iter()
        .filter(|f| f.integrity == "intact")
        .count();
    let partial_count = state
        .results
        .iter()
        .filter(|f| f.integrity == "partial" || f.integrity == "fragmented")
        .count();
    let corrupt_count = state
        .results
        .iter()
        .filter(|f| f.integrity == "corrupt")
        .count();
    let total_bytes: u64 = state.results.iter().map(|f| f.size_bytes).sum();
    let deleted_count = state.results.iter().filter(|f| f.is_deleted).count();
    let carved_count = state
        .results
        .iter()
        .filter(|f| f.recovery_method == "carving")
        .count();
    let imported_source_status = imported_source_status_for_scan_session(&state.device_id);
    let title = if language == "fr" {
        "Rapport de recuperation"
    } else {
        "Recovery Report"
    };
    let build_info = app_build_info();
    let app_identity = format!("{} v{}", build_info.product_name, build_info.app_version);
    let gen_label = if language == "fr" {
        "Genere par"
    } else {
        "Generated by"
    };
    let generated_label = if language == "fr" {
        "Genere"
    } else {
        "Generated"
    };
    let device_label = if language == "fr" {
        "Peripherique"
    } else {
        "Device"
    };
    let scan_label = if language == "fr" {
        "Type de scan"
    } else {
        "Scan type"
    };
    let files_label = if language == "fr" {
        "Fichiers trouves"
    } else {
        "Files found"
    };
    let size_label = if language == "fr" {
        "Taille totale"
    } else {
        "Total size"
    };
    let footer_note = if language == "fr" {
        "Ce rapport a ete genere automatiquement par Recupere. Les scores de recuperation sont des estimations et ne constituent pas une garantie."
    } else {
        "This report was generated automatically by Recupere. Recovery scores are estimates and should not be considered guarantees."
    };
    let scan_id_label = if language == "fr" {
        "ID de scan"
    } else {
        "Scan ID"
    };
    let provenance_title = if language == "fr" {
        "Provenance de la source"
    } else {
        "Source Provenance"
    };
    let provenance_kind_label = if language == "fr" {
        "Type de source"
    } else {
        "Source kind"
    };
    let provenance_format_label = "Format";
    let provenance_analysis_label = if language == "fr" {
        "Chemin d'analyse"
    } else {
        "Analysis path"
    };
    let provenance_prepared_label = if language == "fr" {
        "Preparation terminee"
    } else {
        "Preparation complete"
    };
    let provenance_display_label = if language == "fr" {
        "Nom enregistre"
    } else {
        "Registered name"
    };
    let provenance_size_label = if language == "fr" {
        "Taille logique"
    } else {
        "Logical size"
    };
    let provenance_source_label = if language == "fr" {
        "Source importee d'origine"
    } else {
        "Original imported source"
    };
    let provenance_mode_label = if language == "fr" {
        "Mode d'analyse"
    } else {
        "Analysis mode"
    };
    let default_export_title = if language == "fr" {
        "Posture du lot d'export par defaut"
    } else {
        "Default Export Batch Posture"
    };
    let default_export_included_label = if language == "fr" {
        "Dans le lot implicite"
    } else {
        "In implicit batch"
    };
    let default_export_held_out_label = if language == "fr" {
        "Retenus hors lot implicite"
    } else {
        "Held out of implicit batch"
    };
    let default_export_reason_label = if language == "fr" { "Raison" } else { "Reason" };
    let ai_triage_title = if language == "fr" {
        "Triage IA local"
    } else {
        "Local AI Triage"
    };
    let ai_export_now_label = if language == "fr" {
        "Export immédiat"
    } else {
        "Export now"
    };
    let ai_preview_label = if language == "fr" {
        "Vérifier par aperçu"
    } else {
        "Verify with preview"
    };
    let ai_complex_label = if language == "fr" {
        "Revue complexe"
    } else {
        "Complex review"
    };
    let ai_unstable_label = if language == "fr" {
        "Instables"
    } else {
        "Unstable"
    };
    let ai_apfs_preview_first_label = if language == "fr" {
        "APFS à prévisualiser d'abord"
    } else {
        "APFS preview-first"
    };
    let ai_apfs_reassembled_label = if language == "fr" {
        "APFS réassemblés"
    } else {
        "APFS reassembled"
    };

    let mut method_counts = HashMap::<&str, usize>::new();
    for file in &state.results {
        *method_counts
            .entry(file.recovery_method.as_str())
            .or_insert(0) += 1;
    }

    let avg_score = if files_found > 0 {
        state
            .results
            .iter()
            .map(|f| f.recovery_score as u64)
            .sum::<u64>()
            / files_found as u64
    } else {
        0
    };

    let journal_count = state.results.iter().filter(|f| f.journal_derived).count();
    let default_export_holdout = default_export_batch_holdout(&state.results);
    let default_export_included_count = files_found.saturating_sub(default_export_holdout.len());
    let ai_recovery_brief =
        crate::ai::build_scan_recovery_brief(&scan_id, &state.results, &state.logs);

    let timestamp = current_timestamp_rfc3339();

    let intact_pct = if files_found > 0 {
        (intact_count * 100) / files_found
    } else {
        0
    };
    let partial_pct = if files_found > 0 {
        (partial_count * 100) / files_found
    } else {
        0
    };
    let corrupt_pct = if files_found > 0 {
        100usize
            .saturating_sub(intact_pct)
            .saturating_sub(partial_pct)
    } else {
        0
    };

    let mut html = format!(
        r#"<!DOCTYPE html>
<html lang="{language}">
<head><meta charset="UTF-8"><title>{title} — {scan_id}</title>
<style>
body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; max-width: 960px; margin: 0 auto; padding: 40px 20px; color: #1a1d27; background: #fafbfc; }}
h1 {{ color: #3B82F6; border-bottom: 2px solid #3B82F6; padding-bottom: 12px; }}
h2 {{ color: #5F6578; margin-top: 32px; }}
table {{ width: 100%; border-collapse: collapse; margin: 16px 0; }}
th, td {{ padding: 8px 12px; border: 1px solid #e2e8f0; text-align: left; font-size: 14px; }}
th {{ background: #f7fafc; font-weight: 600; }}
.stat-row {{ display: flex; flex-wrap: wrap; gap: 8px; margin: 16px 0; }}
.stat {{ flex: 1; min-width: 120px; background: #fff; padding: 16px 20px; border-radius: 12px; text-align: center; box-shadow: 0 1px 3px rgba(0,0,0,0.08); }}
.stat-value {{ font-size: 28px; font-weight: 700; display: block; }}
.stat-label {{ font-size: 12px; color: #5F6578; margin-top: 4px; }}
.badge-intact {{ background: #ECFDF5; color: #059669; padding: 2px 10px; border-radius: 12px; font-weight: 600; font-size: 13px; }}
.badge-partial {{ background: #FFFBEB; color: #D97706; padding: 2px 10px; border-radius: 12px; font-weight: 600; font-size: 13px; }}
.badge-corrupt {{ background: #FEF2F2; color: #DC2626; padding: 2px 10px; border-radius: 12px; font-weight: 600; font-size: 13px; }}
.badge-deleted {{ background: #EFF6FF; color: #2563EB; padding: 2px 10px; border-radius: 12px; font-weight: 600; font-size: 13px; }}
.section {{ background: #fff; border-radius: 12px; padding: 24px; margin: 16px 0; box-shadow: 0 1px 3px rgba(0,0,0,0.08); }}
.pie {{ width: 140px; height: 140px; border-radius: 50%; margin: 0 auto; background: conic-gradient(#10B981 0% {intact_pct}%, #F59E0B {intact_pct}% {}%, #EF4444 {}% 100%); }}
.pie-legend {{ display: flex; gap: 16px; justify-content: center; margin-top: 12px; font-size: 13px; }}
.pie-legend span {{ display: flex; align-items: center; gap: 4px; }}
.pie-legend .dot {{ width: 10px; height: 10px; border-radius: 50%; display: inline-block; }}
.method-bar {{ display: flex; align-items: center; gap: 8px; margin: 4px 0; }}
.method-bar .bar {{ height: 20px; background: #3B82F6; border-radius: 4px; min-width: 2px; }}
.method-bar .label {{ font-size: 13px; min-width: 100px; }}
.method-bar .count {{ font-size: 13px; color: #5F6578; }}
footer {{ margin-top: 48px; padding-top: 16px; border-top: 1px solid #e2e8f0; font-size: 12px; color: #9ca3af; text-align: center; }}
</style></head>
<body>
<h1>{title}</h1>
<p><strong>{gen_label}:</strong> {app_identity} &mdash; {timestamp}</p>

<div class="stat-row">
<div class="stat"><span class="stat-value">{files_found}</span><span class="stat-label">{files_label}</span></div>
<div class="stat"><span class="stat-value" style="color:#059669">{intact_count}</span><span class="stat-label">Intact</span></div>
<div class="stat"><span class="stat-value" style="color:#D97706">{partial_count}</span><span class="stat-label">Partial</span></div>
<div class="stat"><span class="stat-value" style="color:#DC2626">{corrupt_count}</span><span class="stat-label">Corrupt</span></div>
<div class="stat"><span class="stat-value" style="color:#2563EB">{deleted_count}</span><span class="stat-label">Deleted</span></div>
<div class="stat"><span class="stat-value">{avg_score}</span><span class="stat-label">Avg Score</span></div>
</div>

<div class="section">
<h2 style="margin-top:0">{device_label} &amp; Scan</h2>
<table>
<tr><th>{device_label}</th><td>{device_name}</td></tr>
<tr><th>{scan_label}</th><td>{scan_type}</td></tr>
<tr><th>{size_label}</th><td>{total_bytes} bytes</td></tr>
<tr><th>Deleted</th><td>{deleted_count}</td></tr>
<tr><th>Carved</th><td>{carved_count}</td></tr>
<tr><th>Journal Derived</th><td>{journal_count}</td></tr>
</table>
</div>

<div style="display:flex;gap:24px;flex-wrap:wrap;">
<div class="section" style="flex:1;min-width:200px;">
<h2 style="margin-top:0">Integrity Distribution</h2>
<div class="pie"></div>
<div class="pie-legend">
<span><span class="dot" style="background:#10B981"></span> Intact {intact_pct}%</span>
<span><span class="dot" style="background:#F59E0B"></span> Partial {partial_pct}%</span>
<span><span class="dot" style="background:#EF4444"></span> Corrupt {corrupt_pct}%</span>
</div>
</div>

<div class="section" style="flex:1;min-width:280px;">
<h2 style="margin-top:0">Recovery Methods</h2>
"#,
        intact_pct + partial_pct,
        intact_pct + partial_pct
    );

    let max_method = method_counts.values().copied().max().unwrap_or(1).max(1);
    let mut sorted_methods: Vec<_> = method_counts.iter().collect();
    sorted_methods.sort_by(|a, b| b.1.cmp(a.1));
    for (method, count) in &sorted_methods {
        let bar_pct = (**count * 100) / max_method;
        html.push_str(&format!(
            "<div class=\"method-bar\"><span class=\"label\">{}</span><div class=\"bar\" style=\"width:{}%\"></div><span class=\"count\">{}</span></div>\n",
            html_escape(method), bar_pct.max(2), count
        ));
    }
    html.push_str("</div>\n</div>\n");

    html.push_str(&format!(
        "<div class=\"section\"><h2 style=\"margin-top:0\">{}</h2><table>\
<tr><th>{}</th><td>{}</td></tr>\
<tr><th>{}</th><td>{}</td></tr>\
<tr><th>{}</th><td>{}</td></tr>\
<tr><th>{}</th><td>{}</td></tr>\
<tr><th>{}</th><td>{}</td></tr>\
<tr><th>{}</th><td>{}</td></tr>\
</table></div>\n",
        ai_triage_title,
        ai_export_now_label,
        ai_recovery_brief.counts.export_now,
        ai_preview_label,
        ai_recovery_brief.counts.verify_with_preview,
        ai_complex_label,
        ai_recovery_brief.counts.complex_recovery_review,
        ai_unstable_label,
        ai_recovery_brief.counts.unstable,
        ai_apfs_preview_first_label,
        ai_recovery_brief.counts.apfs_catalog_preview_first,
        ai_apfs_reassembled_label,
        ai_recovery_brief.counts.apfs_catalog_reassembled,
    ));

    if let Some(imported) = &imported_source_status {
        let analysis_path = imported
            .analysis_path
            .as_deref()
            .unwrap_or(imported.source_path.as_str());
        html.push_str(&format!(
            "<div class=\"section\"><h2 style=\"margin-top:0\">{}</h2><table>\
<tr><th>{}</th><td>{}</td></tr>\
<tr><th>{}</th><td>{}</td></tr>\
<tr><th>{}</th><td>{}</td></tr>\
<tr><th>{}</th><td>{} bytes</td></tr>\
<tr><th>{}</th><td style=\"font-family:monospace;font-size:12px\">{}</td></tr>\
<tr><th>{}</th><td style=\"font-family:monospace;font-size:12px\">{}</td></tr>\
<tr><th>{}</th><td>{}</td></tr>\
<tr><th>{}</th><td>{}</td></tr>\
</table></div>\n",
            provenance_title,
            provenance_kind_label,
            html_escape(imported_source_kind_label(imported, &language)),
            provenance_display_label,
            html_escape(&imported.display_name),
            provenance_format_label,
            html_escape(&imported.source_format),
            provenance_size_label,
            imported.logical_size_bytes,
            provenance_source_label,
            html_escape(&imported.source_path),
            provenance_analysis_label,
            html_escape(analysis_path),
            provenance_mode_label,
            html_escape(imported_source_trace_analysis_mode(imported, &language)),
            provenance_prepared_label,
            imported.prepared
        ));
    }

    html.push_str(&format!(
        "<div class=\"section\"><h2 style=\"margin-top:0\">{}</h2><table>\
<tr><th>{}</th><td>{}</td></tr>\
<tr><th>{}</th><td>{}</td></tr>\
<tr><th>{}</th><td>{}</td></tr>\
<tr><th>{}</th><td>{}</td></tr>\
</table>",
        default_export_title,
        files_label,
        files_found,
        default_export_included_label,
        default_export_included_count,
        default_export_held_out_label,
        default_export_holdout.len(),
        ai_apfs_reassembled_label,
        state
            .results
            .iter()
            .filter(|file| is_apfs_catalog_reassembled_candidate(file))
            .count()
    ));

    if !default_export_holdout.is_empty() {
        html.push_str(&format!(
            "<p style=\"font-size:14px;color:#5F6578;margin-top:16px\">{}</p>\
<table><tr><th>Name</th><th>Path</th><th>Score</th><th>{}</th></tr>",
            html_escape(default_export_hold_reason(&language)),
            default_export_reason_label
        ));
        for file in &default_export_holdout {
            html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(&file.name),
                html_escape(&file.path),
                file.recovery_score,
                html_escape(default_export_hold_reason(&language))
            ));
        }
        html.push_str("</table>");
    }

    html.push_str("</div>\n");

    if include_file_inventory {
        let inv_title = if language == "fr" {
            "Inventaire des fichiers"
        } else {
            "File Inventory"
        };
        html.push_str(&format!("<div class=\"section\"><h2 style=\"margin-top:0\">{inv_title}</h2>\n<table>\n<tr><th>Name</th><th>Size</th><th>Integrity</th><th>Score</th><th>Method</th><th>SHA-256</th></tr>\n"));
        for file in &state.results {
            let hash = file.sha256.as_deref().unwrap_or("&mdash;");
            let badge_class = match file.integrity.as_str() {
                "intact" => "badge-intact",
                "partial" | "fragmented" => "badge-partial",
                _ => "badge-corrupt",
            };
            html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td><span class=\"{}\">{}</span></td><td>{}</td><td>{}</td><td style=\"font-size:10px;font-family:monospace\">{}</td></tr>\n",
                html_escape(&file.name), file.size_bytes, badge_class, file.integrity,
                file.recovery_score, file.recovery_method, hash
            ));
        }
        html.push_str("</table></div>\n");
    }

    html.push_str(&format!(
        r#"<footer>
<p>{footer_note}</p>
<p>{scan_id_label}: <code>{scan_id}</code> &mdash; {generated_label}: {timestamp}</p>
</footer>
</body></html>"#
    ));

    let report_dir = app_reports_dir();
    fs::create_dir_all(&report_dir).map_err(|e| format!("Cannot create report directory: {e}"))?;
    let report_path = report_dir.join(format!("report-{scan_id}-{}.html", random_hex_suffix()));
    write_private_file_atomically(&report_path, html.as_bytes(), "recovery report")?;
    Ok(report_path.to_string_lossy().to_string())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn export_results_csv(scan_id: String) -> Result<String, String> {
    let session = get_session(&scan_id)?;
    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    let imported_source_status = imported_source_status_for_scan_session(&state.device_id);
    let raid_imported_source = imported_source_status
        .as_ref()
        .is_some_and(is_reconstructed_raid_source);
    let imported_display_name = imported_source_status
        .as_ref()
        .map(|status| status.display_name.as_str())
        .unwrap_or("");
    let imported_source_kind = imported_source_status
        .as_ref()
        .map(|status| imported_source_kind_label(status, "en"))
        .unwrap_or("");
    let source_format = imported_source_status
        .as_ref()
        .map(|status| status.source_format.as_str())
        .unwrap_or("");
    let logical_size_bytes = imported_source_status
        .as_ref()
        .map(|status| status.logical_size_bytes)
        .unwrap_or(0);
    let original_source_path = imported_source_status
        .as_ref()
        .map(|status| status.source_path.as_str())
        .unwrap_or("");
    let source_available = imported_source_status
        .as_ref()
        .map(|status| status.source_available)
        .unwrap_or(false);
    let requires_preparation = imported_source_status
        .as_ref()
        .map(|status| status.requires_preparation)
        .unwrap_or(false);
    let prepared = imported_source_status
        .as_ref()
        .map(|status| status.prepared)
        .unwrap_or(false);
    let cache_path = imported_source_status
        .as_ref()
        .and_then(|status| status.cache_path.as_deref())
        .unwrap_or("");
    let analysis_mode = imported_source_status
        .as_ref()
        .map(|status| imported_source_trace_analysis_mode(status, "en"))
        .unwrap_or("");
    let analysis_path = imported_source_status
        .as_ref()
        .and_then(|status| {
            status
                .analysis_path
                .as_deref()
                .or(Some(status.source_path.as_str()))
        })
        .unwrap_or("");
    let default_export_holdout = default_export_batch_holdout(&state.results);

    let mut csv = String::from(
        "Name,Path,Extension,Size (bytes),Integrity,Recovery Score,Method,SHA-256,Deleted,Journal Derived,Default Export Held Back,Default Export Hold Reason,APFS Reassembled,Imported Source Name,Imported Source Kind,Source Format,Logical Size (bytes),Original Source Path,Source Available,Requires Preparation,Prepared,Derived Local Cache Path,Analysis Mode,Analysis Source Path,Reconstructed RAID Source\n",
    );

    for file in &state.results {
        let sha = file.sha256.as_deref().unwrap_or("");
        let held_back = default_export_holdout.iter().any(|held| held.id == file.id);
        let hold_reason = if held_back {
            default_export_hold_reason("en")
        } else {
            ""
        };
        let apfs_reassembled = is_apfs_catalog_reassembled_candidate(file);
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            csv_cell(&file.name),
            csv_cell(&file.path),
            csv_cell(&file.extension),
            file.size_bytes,
            csv_cell(&file.integrity),
            file.recovery_score,
            csv_cell(&file.recovery_method),
            csv_cell(sha),
            file.is_deleted,
            file.journal_derived,
            held_back,
            csv_cell(hold_reason),
            apfs_reassembled,
            csv_cell(imported_display_name),
            csv_cell(imported_source_kind),
            csv_cell(source_format),
            logical_size_bytes,
            csv_cell(original_source_path),
            source_available,
            requires_preparation,
            prepared,
            csv_cell(cache_path),
            csv_cell(analysis_mode),
            csv_cell(analysis_path),
            raid_imported_source,
        ));
    }

    let report_dir = app_reports_dir();
    fs::create_dir_all(&report_dir).map_err(|e| format!("Cannot create report directory: {e}"))?;
    let path = report_dir.join(format!("results-{scan_id}-{}.csv", random_hex_suffix()));
    write_private_file_atomically(&path, csv.as_bytes(), "CSV report")?;
    Ok(path.to_string_lossy().to_string())
}

fn csv_cell(value: &str) -> String {
    let mut safe = value.replace('"', "\"\"");
    if matches!(
        safe.chars().next(),
        Some('=' | '+' | '-' | '@' | '\t' | '\r')
    ) {
        safe.insert(0, '\'');
    }
    format!("\"{safe}\"")
}

fn app_reports_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("recupere")
        .join("reports")
}

fn current_timestamp_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn random_hex_suffix() -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn write_private_file_atomically(path: &Path, bytes: &[u8], label: &str) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| {
        format!(
            "The selected {label} destination {} has no writable parent directory.",
            path.to_string_lossy()
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "Unable to create {label} directory {}: {}",
            parent.to_string_lossy(),
            error
        )
    })?;

    let temp_path = temporary_sibling_path(path)?;
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp_path).map_err(|error| {
            format!(
                "Unable to create temporary {label} {}: {}",
                temp_path.to_string_lossy(),
                error
            )
        })?;
        file.write_all(bytes)
            .map_err(|error| format!("Unable to write {label}: {error}"))?;
        file.flush()
            .map_err(|error| format!("Unable to flush {label}: {error}"))?;
    }

    finalize_temporary_file(&temp_path, path, false)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn imported_source_status_for_scan_session(
    device_id: &str,
) -> Option<ImportedRecoverySourceStatus> {
    core::detect_devices()
        .into_iter()
        .find(|device| device.id == device_id && matches!(device.device_type, DeviceType::Image))
        .and_then(|device| {
            crate::imported_sources::get_imported_source_status(Path::new(&device.device_path))
                .ok()
                .flatten()
        })
}

fn is_reconstructed_raid_source(imported: &ImportedRecoverySourceStatus) -> bool {
    imported
        .source_format
        .trim()
        .to_ascii_uppercase()
        .starts_with("RAID")
}

fn imported_source_kind_label(
    imported: &ImportedRecoverySourceStatus,
    language: &str,
) -> &'static str {
    let format = imported.source_format.trim().to_ascii_uppercase();
    if format.starts_with("RAID") {
        if language == "fr" {
            "Image d'analyse RAID reconstruite"
        } else {
            "Reconstructed RAID analysis image"
        }
    } else if format == "E01" {
        if language == "fr" {
            "Image forensic de preuve"
        } else {
            "Forensic evidence image"
        }
    } else if matches!(format.as_str(), "VMDK" | "VHD" | "VHDX") {
        if language == "fr" {
            "Conteneur de disque virtuel"
        } else {
            "Virtual disk container"
        }
    } else if matches!(format.as_str(), "RAW" | "IMG" | "DD" | "BIN") {
        if language == "fr" {
            "Image disque brute"
        } else {
            "Raw disk image"
        }
    } else if language == "fr" {
        "Source importee"
    } else {
        "Imported source"
    }
}

fn imported_source_trace_analysis_mode(
    imported: &ImportedRecoverySourceStatus,
    language: &str,
) -> &'static str {
    if !imported.requires_preparation {
        if language == "fr" {
            "Lecture directe en lecture seule"
        } else {
            "Direct read-only source"
        }
    } else if imported.prepared {
        if language == "fr" {
            "Cache local prepare"
        } else {
            "Prepared local cache"
        }
    } else if language == "fr" {
        "Analyse poussee bloquee en attente de preparation"
    } else {
        "Deeper analysis blocked until preparation completes"
    }
}

fn imported_source_trace_json(imported: &ImportedRecoverySourceStatus) -> serde_json::Value {
    serde_json::json!({
        "display_name": imported.display_name,
        "source_kind": imported_source_kind_label(imported, "en"),
        "source_format": imported.source_format,
        "logical_size_bytes": imported.logical_size_bytes,
        "source_path": imported.source_path,
        "source_available": imported.source_available,
        "requires_preparation": imported.requires_preparation,
        "prepared": imported.prepared,
        "cache_path": imported.cache_path,
        "cache_size_bytes": imported.cache_size_bytes,
        "analysis_path": imported.analysis_path.clone().unwrap_or_else(|| imported.source_path.clone()),
        "analysis_mode": imported_source_trace_analysis_mode(imported, "en"),
    })
}

fn sanitize_bundle_name(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '-',
        })
        .collect();
    sanitized.trim_matches('-').to_string()
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn generate_lab_bundle(
    device_id: String,
    destination_path: String,
    active_scan_id: Option<String>,
    active_export_id: Option<String>,
) -> Result<String, String> {
    let device = core::detect_devices()
        .into_iter()
        .find(|d| d.id == device_id)
        .ok_or_else(|| format!("Device {device_id} not found"))?;
    let diagnostic = build_diagnostic(&device);
    let generated_at = current_timestamp_rfc3339();
    let build_info = app_build_info();
    let audit_report = crate::audit::verify_trail();
    let recent_traces = crate::telemetry::recent_traces(Some(100));
    let analysis_path = crate::imported_sources::resolve_analysis_source_path_if_imported(
        Path::new(&device.device_path),
    )
    .map_err(|error| format!("Cannot resolve analysis path for lab bundle: {error}"))?
    .unwrap_or_else(|| PathBuf::from(&device.device_path));
    let analysis_path_string = analysis_path.to_string_lossy().to_string();
    let raid_metadata = crate::raid::detect_raid_metadata(&analysis_path).map(|metadata| {
        serde_json::json!({
            "level": format!("{:?}", metadata.level),
            "member_count": metadata.member_count,
            "stripe_size_bytes": metadata.stripe_size_bytes,
            "superblock_version": metadata.superblock_version,
            "data_offset_bytes": metadata.data_offset_bytes,
        })
    });
    let encryption_info = crate::encryption::detect_encryption_type(
        &analysis_path_string,
        device.is_encrypted.unwrap_or(false),
    );
    let imported_source_status = if matches!(device.device_type, crate::types::DeviceType::Image) {
        crate::imported_sources::get_imported_source_status(Path::new(&device.device_path))
            .unwrap_or(None)
    } else {
        None
    };
    let smart_report = if matches!(device.device_type, crate::types::DeviceType::Image) {
        None
    } else {
        Some(core::get_smart_report(&device.device_path, &device.id))
    };
    let active_scan_results = active_scan_id.as_ref().and_then(|scan_id| {
        crate::commands::state::lock_or_recover(super::scan_sessions(), "scan session registry")
            .get(scan_id)
            .cloned()
            .map(|session| {
                crate::commands::state::lock_or_recover(&session, "scan session")
                    .results
                    .clone()
            })
    });
    let active_scan_logs = active_scan_id.as_ref().and_then(|scan_id| {
        let live_session = crate::commands::state::lock_or_recover(
            super::scan_sessions(),
            "scan session registry",
        )
        .get(scan_id)
        .cloned();
        if let Some(session) = live_session {
            return Some(
                crate::commands::state::lock_or_recover(&session, "scan session")
                    .logs
                    .clone(),
            );
        }
        super::load_persisted_scan_record(scan_id).map(|record| record.logs)
    });
    let active_scan_context = active_scan_id.as_ref().and_then(|scan_id| {
        let live_session = crate::commands::state::lock_or_recover(
            super::scan_sessions(),
            "scan session registry",
        )
        .get(scan_id)
        .cloned();
        if let Some(session) = live_session {
            let summary = super::build_scan_summary(&session);
            let logs = crate::commands::state::lock_or_recover(&session, "scan session")
                .logs
                .clone();
            return Some(serde_json::json!({
                "summary": summary,
                "logs": logs,
                "source": "live",
            }));
        }
        super::load_persisted_scan_record(scan_id).map(|record| {
            serde_json::json!({
                "summary": record.summary,
                "logs": record.logs,
                "source": "persisted",
            })
        })
    });
    let active_export_context = active_export_id.as_ref().and_then(|export_id| {
        let live_session = crate::commands::state::lock_or_recover(
            super::export_sessions(),
            "export session registry",
        )
        .get(export_id)
        .cloned();
        if let Some(session) = live_session {
            let summary = super::build_export_summary(&session);
            let logs = crate::commands::state::lock_or_recover(&session, "export session")
                .logs
                .clone();
            return Some(serde_json::json!({
                "summary": summary,
                "logs": logs,
                "source": "live",
            }));
        }
        super::load_persisted_export_record(export_id).map(|record| {
            serde_json::json!({
                "summary": record.summary,
                "logs": record.logs,
                "source": "persisted",
            })
        })
    });
    let default_export_holdout = active_scan_results
        .as_ref()
        .map(|results| default_export_batch_holdout(results))
        .unwrap_or_default();
    let default_export_reassembled: Vec<&RecoveredFile> = active_scan_results
        .as_ref()
        .map(|results| {
            results
                .iter()
                .filter(|file| is_apfs_catalog_reassembled_candidate(file))
                .collect()
        })
        .unwrap_or_default();
    let active_scan_apfs_reassembled = active_scan_results
        .as_ref()
        .map(|_| default_export_reassembled.len())
        .unwrap_or(0);
    let ai_recovery_brief = active_scan_results.as_ref().map(|results| {
        crate::ai::build_scan_recovery_brief(
            active_scan_id.as_deref().unwrap_or("live-scan"),
            results,
            active_scan_logs.as_deref().unwrap_or(&[]),
        )
    });

    let mut report = String::new();
    report.push_str("=== RECUPERE PRE-LAB RECOVERY BUNDLE ===\n\n");
    report.push_str(&format!("Generated: {}\n", generated_at));
    report.push_str(&format!(
        "Device: {} ({})\n",
        device.name, device.device_path
    ));
    report.push_str(&format!(
        "Type: {:?} | Filesystem: {:?}\n",
        device.device_type, device.filesystem
    ));
    report.push_str(&format!("Capacity: {} bytes\n", device.capacity_bytes));
    report.push_str(&format!(
        "Status: {:?} | Risk: {:?}\n",
        device.status, device.risk_level
    ));
    report.push_str(&format!(
        "SMART: {:?} | TRIM: {:?} | Encrypted: {:?}\n",
        device.smart_available, device.is_trim_enabled, device.is_encrypted
    ));
    report.push_str(&format!(
        "Serial: {}\n",
        device.serial.as_deref().unwrap_or("unknown")
    ));
    report.push_str(&format!(
        "Model: {}\n\n",
        device.model.as_deref().unwrap_or("unknown")
    ));
    report.push_str(&format!("Analysis Path: {}\n", analysis_path_string));
    report.push_str(&format!(
        "Build: {} {} ({}/{})\n\n",
        build_info.product_name,
        build_info.app_version,
        build_info.operating_system,
        build_info.architecture
    ));

    report.push_str("=== DIAGNOSTIC RESULTS ===\n");
    report.push_str(&format!("Verdict: {}\n", diagnostic.verdict));
    report.push_str(&format!("Details: {}\n", diagnostic.verdict_details));
    report.push_str(&format!(
        "Recoverability Score: {}/100\n",
        diagnostic.recoverability_score
    ));
    report.push_str(&format!("Loss Type: {}\n", diagnostic.loss_type));
    report.push_str(&format!("Imaging Ready: {}\n", diagnostic.imaging_ready));
    report.push_str(&format!(
        "Imaging Requires Elevation: {}\n\n",
        diagnostic.imaging_requires_elevation
    ));

    report.push_str("=== RISK FACTORS ===\n");
    for rf in &diagnostic.risk_factors {
        report.push_str(&format!(
            "- [{}] {} ({:?})\n",
            rf.id, rf.title_key, rf.severity
        ));
    }

    report.push_str("\n=== RECOMMENDATIONS FOR LAB ===\n");
    for rec in &diagnostic.recommendations {
        report.push_str(&format!(
            "- [P{}] {} ({})\n",
            rec.priority, rec.title_key, rec.rec_type
        ));
    }

    report.push_str("\n=== LIMITATIONS OBSERVED ===\n");
    for lim in &diagnostic.limitations {
        report.push_str(&format!("- {}\n", lim));
    }

    report.push_str("\n=== POTENTIAL VOLUMES ===\n");
    for vol in &diagnostic.potential_volumes {
        report.push_str(&format!(
            "- {} ({:?}) at offset 0x{:X}, confidence: {}%\n",
            vol.label, vol.filesystem, vol.start_offset, vol.confidence_score
        ));
    }

    report.push_str("\n=== ENCRYPTION / RAID / SMART ===\n");
    report.push_str(&format!(
        "Encryption: {:?}\n",
        encryption_info.encryption_type
    ));
    report.push_str(&format!(
        "Encryption detected: {}\n",
        encryption_info.detected
    ));
    report.push_str(&format!(
        "Unlock supported: {}\n",
        encryption_info.can_unlock
    ));
    report.push_str(&format!("Encryption note: {}\n", encryption_info.message));
    if let Some(raid) = &raid_metadata {
        report.push_str(&format!("RAID metadata: {}\n", raid));
    } else {
        report.push_str("RAID metadata: none detected\n");
    }
    if let Some(smart) = &smart_report {
        report.push_str(&format!(
            "SMART overall health: {} | temperature: {:?} | pending sectors: {:?}\n",
            smart.overall_health, smart.temperature_celsius, smart.pending_sectors
        ));
    } else {
        report.push_str("SMART: unavailable or not applicable\n");
    }

    if let Some(imported) = &imported_source_status {
        report.push_str("\n=== IMPORTED SOURCE STATUS ===\n");
        report.push_str(&format!("Registered name: {}\n", imported.display_name));
        report.push_str(&format!(
            "Source kind: {}\n",
            imported_source_kind_label(imported, "en")
        ));
        report.push_str(&format!("Format: {}\n", imported.source_format));
        report.push_str(&format!(
            "Logical size: {} byte(s)\n",
            imported.logical_size_bytes
        ));
        report.push_str(&format!("Original source path: {}\n", imported.source_path));
        report.push_str(&format!("Prepared: {}\n", imported.prepared));
        report.push_str(&format!(
            "Requires preparation: {}\n",
            imported.requires_preparation
        ));
        report.push_str(&format!(
            "Source available: {}\n",
            imported.source_available
        ));
        report.push_str(&format!(
            "Analysis mode: {}\n",
            imported_source_trace_analysis_mode(imported, "en")
        ));
        if let Some(path) = &imported.cache_path {
            report.push_str(&format!("Derived local cache path: {path}\n"));
        }
        report.push_str(&format!(
            "Prepared analysis path: {}\n",
            imported
                .analysis_path
                .as_deref()
                .unwrap_or(imported.source_path.as_str())
        ));
    }

    report.push_str("\n=== DEFAULT EXPORT BATCH POSTURE ===\n");
    if let Some(results) = &active_scan_results {
        report.push_str(&format!(
            "Results visible in live scan session: {}\n",
            results.len()
        ));
        report.push_str(&format!(
            "Held out of implicit export batch: {}\n",
            default_export_holdout.len()
        ));
        report.push_str(&format!(
            "APFS reassembled in current result set: {}\n",
            active_scan_apfs_reassembled
        ));
        if !default_export_holdout.is_empty() {
            report.push_str(&format!("Reason: {}\n", default_export_hold_reason("en")));
            for file in &default_export_holdout {
                report.push_str(&format!(
                    "- {} | {} | score {}\n",
                    file.name, file.path, file.recovery_score
                ));
            }
        }
        if !default_export_reassembled.is_empty() {
            report.push_str("APFS reassembled candidates in this result set:\n");
            for file in &default_export_reassembled {
                report.push_str(&format!(
                    "- {} | {} | score {}\n",
                    file.name, file.path, file.recovery_score
                ));
            }
        }
    } else {
        report.push_str(
            "Detailed default export holdout view unavailable because no live scan results were attached to this bundle.\n",
        );
    }

    report.push_str("\n=== LOCAL AI RECOVERY TRIAGE ===\n");
    if let Some(brief) = &ai_recovery_brief {
        report.push_str(&format!("Strategy title: {}\n", brief.strategy_title));
        report.push_str(&format!(
            "Safe export strategy: {}\n",
            brief.safe_export_strategy
        ));
        report.push_str(&format!(
            "Complexity summary: {}\n",
            brief.complexity_summary
        ));
        report.push_str(&format!("Export now: {}\n", brief.counts.export_now));
        report.push_str(&format!(
            "Verify with preview: {}\n",
            brief.counts.verify_with_preview
        ));
        report.push_str(&format!(
            "Complex review: {}\n",
            brief.counts.complex_recovery_review
        ));
        report.push_str(&format!("Unstable: {}\n", brief.counts.unstable));
        report.push_str(&format!(
            "APFS preview-first: {}\n",
            brief.counts.apfs_catalog_preview_first
        ));
        report.push_str(&format!(
            "APFS reassembled: {}\n",
            brief.counts.apfs_catalog_reassembled
        ));
    } else {
        report.push_str(
            "AI recovery brief unavailable because no live scan results were attached to this bundle.\n",
        );
    }

    report.push_str("\n=== AUDIT / TRACE CONTEXT ===\n");
    report.push_str(&format!(
        "Audit records: {} | verified: {} | unverified legacy: {}\n",
        audit_report.total, audit_report.ok, audit_report.unverified
    ));
    report.push_str(&format!("Broken audit records: {}\n", audit_report.broken));
    report.push_str(&format!(
        "First broken audit record: {}\n",
        audit_report
            .first_broken_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "none".into())
    ));
    report.push_str(&format!(
        "Audit chain tip hash: {}\n",
        audit_report.chain_tip_hash
    ));
    report.push_str(&format!(
        "Audit genesis previous hash: {}\n",
        audit_report.genesis_prev_hash
    ));
    report.push_str(&format!(
        "Recent traces captured: {}\n",
        recent_traces.len()
    ));
    report.push_str(&format!(
        "Active scan context: {}\n",
        active_scan_id.as_deref().unwrap_or("none")
    ));
    report.push_str(&format!(
        "Active export context: {}\n",
        active_export_id.as_deref().unwrap_or("none")
    ));

    if let Some(scan_context) = &active_scan_context {
        report.push_str("\n=== ACTIVE SCAN CONTEXT ===\n");
        report.push_str(
            &serde_json::to_string_pretty(scan_context)
                .map_err(|e| format!("Cannot serialize active scan context: {e}"))?,
        );
        report.push('\n');
    }

    if let Some(export_context) = &active_export_context {
        report.push_str("\n=== ACTIVE EXPORT CONTEXT ===\n");
        report.push_str(
            &serde_json::to_string_pretty(export_context)
                .map_err(|e| format!("Cannot serialize active export context: {e}"))?,
        );
        report.push('\n');
    }

    report.push_str("\n=== NOTES FOR LAB TECHNICIAN ===\n");
    report.push_str("- This bundle was generated by Recupere Recovery Intelligence Platform\n");
    report.push_str("- The device was accessed in READ-ONLY mode only\n");
    report.push_str("- No write operations were performed on the source device\n");
    report.push_str("- All analysis was non-destructive\n");

    let dest = PathBuf::from(&destination_path);
    let bundle_dir = dest.join(format!(
        "lab-bundle-{}-{}",
        sanitize_bundle_name(&device_id),
        unix_timestamp_ms()
    ));
    fs::create_dir_all(&bundle_dir)
        .map_err(|e| format!("Cannot create lab bundle directory: {e}"))?;

    let report_path = bundle_dir.join("summary.txt");
    fs::write(&report_path, report.as_bytes())
        .map_err(|e| format!("Cannot write lab summary: {e}"))?;

    let manifest = serde_json::json!({
        "generated_at": generated_at,
        "build_info": build_info,
        "device": device,
        "analysis_path": analysis_path_string,
        "diagnostic": diagnostic,
        "raid_metadata": raid_metadata,
        "encryption_info": encryption_info,
        "smart_report": smart_report,
        "imported_source_status": imported_source_status,
        "imported_source_trace": imported_source_status
            .as_ref()
            .map(imported_source_trace_json),
        "default_export_batch": {
            "results_visible_in_live_scan": active_scan_results.as_ref().map(|results| results.len()),
            "held_out_count": default_export_holdout.len(),
            "apfs_reassembled_count": active_scan_apfs_reassembled,
            "hold_reason": if default_export_holdout.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(default_export_hold_reason("en").into())
            },
            "held_out_files": default_export_holdout.iter().map(|file| serde_json::json!({
                "id": file.id,
                "name": file.name,
                "path": file.path,
                "recovery_score": file.recovery_score,
                "source_view": file.source_view,
                "validator_status": file.validator_status,
            })).collect::<Vec<_>>(),
            "reassembled_files": default_export_reassembled.iter().map(|file| serde_json::json!({
                "id": file.id,
                "name": file.name,
                "path": file.path,
                "recovery_score": file.recovery_score,
                "source_view": file.source_view,
                "validator_status": file.validator_status,
            })).collect::<Vec<_>>(),
        },
        "ai_recovery_brief": ai_recovery_brief,
        "audit_chain": audit_report,
        "recent_traces": recent_traces,
        "active_scan_context": active_scan_context,
        "active_export_context": active_export_context,
    });
    let manifest_path = bundle_dir.join("manifest.json");
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| format!("Cannot serialize lab manifest: {e}"))?;
    fs::write(&manifest_path, manifest_bytes)
        .map_err(|e| format!("Cannot write lab manifest: {e}"))?;

    if let Some(brief) = &ai_recovery_brief {
        let ai_brief_path = bundle_dir.join("ai-recovery-brief.json");
        let ai_brief_bytes = serde_json::to_vec_pretty(brief)
            .map_err(|e| format!("Cannot serialize AI recovery brief: {e}"))?;
        fs::write(&ai_brief_path, ai_brief_bytes)
            .map_err(|e| format!("Cannot write AI recovery brief: {e}"))?;
    }

    let trace_log = crate::telemetry::recent_traces(Some(100))
        .into_iter()
        .map(|trace| {
            format!(
                "{} [{}] {} {}\n",
                trace.timestamp_ms, trace.level, trace.target, trace.message
            )
        })
        .collect::<String>();
    let trace_path = bundle_dir.join("recent-traces.log");
    fs::write(&trace_path, trace_log.as_bytes())
        .map_err(|e| format!("Cannot write recent traces log: {e}"))?;

    Ok(bundle_dir.to_string_lossy().to_string())
}

// ----------------------------------------------------------------------------
// Helpers migrated from `commands/mod.rs` (Sprint 2.1 slice `export_helpers`).
// Used by run_export_session, the #[tauri::command] entrypoints above, and
// sibling modules (`repair_cmd.rs`, `file_preview.rs`) via
// `pub(super) use export::{...};` re-export in `mod.rs`.
// ----------------------------------------------------------------------------

pub(crate) fn select_files_for_export(
    files: &[RecoveredFile],
    selected_file_ids: &[String],
) -> Result<Vec<RecoveredFile>, String> {
    if selected_file_ids.is_empty() {
        return Ok(files.to_vec());
    }

    let selected: Vec<RecoveredFile> = selected_file_ids
        .iter()
        .filter_map(|id| files.iter().find(|file| &file.id == id).cloned())
        .collect();

    if selected.len() != selected_file_ids.len() {
        return Err("Some selected files are no longer available for export.".into());
    }

    Ok(selected)
}

pub(crate) fn file_uses_recovery_image(file: &RecoveredFile) -> bool {
    file.source_image_path.is_some()
}

pub(crate) fn infer_auxiliary_asset_preview(
    bytes: &[u8],
) -> Option<(&'static str, &'static str, &'static str)> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(("image", "jpg", "image/jpeg"));
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1A\n") {
        return Some(("image", "png", "image/png"));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(("image", "gif", "image/gif"));
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(("image", "webp", "image/webp"));
    }
    if bytes.starts_with(b"%PDF-") {
        return Some(("pdf", "pdf", "application/pdf"));
    }

    None
}

pub(crate) fn build_hex_preview_lines(start_offset: u64, bytes: &[u8]) -> Vec<HexPreviewLine> {
    bytes
        .chunks(HEX_PREVIEW_LINE_WIDTH)
        .enumerate()
        .map(|(index, chunk)| {
            let offset = start_offset + (index as u64 * HEX_PREVIEW_LINE_WIDTH as u64);
            let hex = format_hex_column(chunk);
            let ascii = chunk
                .iter()
                .map(|byte| match byte {
                    0x20..=0x7E => char::from(*byte),
                    _ => '.',
                })
                .collect();

            HexPreviewLine { offset, hex, ascii }
        })
        .collect()
}

fn format_hex_column(bytes: &[u8]) -> String {
    let mut cells: Vec<String> = bytes.iter().map(|byte| format!("{byte:02X}")).collect();
    while cells.len() < HEX_PREVIEW_LINE_WIDTH {
        cells.push("  ".into());
    }

    let (left, right) = cells.split_at(8);
    format!("{}  {}", left.join(" "), right.join(" "))
}

pub(crate) fn update_export_progress(
    session: &Arc<Mutex<ExportSession>>,
    update: impl FnOnce(&mut ExportProgress),
) {
    let mut state = session.lock().expect("export session lock poisoned");
    update(&mut state.progress);
}

pub(crate) fn push_export_error(
    session: &Arc<Mutex<ExportSession>>,
    file_id: String,
    file_name: String,
    reason: String,
) {
    tracing::info!("export warning: {} ({})", file_name, reason);
    let mut state = session.lock().expect("export session lock poisoned");
    let log_message = format!("{file_name}: {reason}");
    state.progress.errors.push(ExportError {
        file_id,
        file_name,
        reason,
    });
    if state.progress.exported_files == 0 {
        state.progress.status = "error".into();
    }
    push_technical_log(&mut state.logs, "warning", log_message);
    drop(state);

    if let Err(error) = persist_export_session(session) {
        tracing::info!("push_export_error: unable to persist export snapshot: {error}");
    }
}

pub(crate) fn build_source_path(root_path: &Path, file: &RecoveredFile) -> PathBuf {
    let mut source_path = root_path.to_path_buf();
    let relative_parent = relative_dir_from_display_path(&file.path);
    if !relative_parent.as_os_str().is_empty() {
        source_path.push(relative_parent);
    }
    source_path.push(safe_export_file_name(&file.name));
    source_path
}

pub(crate) fn resolve_source_path_under_root(
    root_path: &Path,
    file: &RecoveredFile,
) -> Result<PathBuf, String> {
    let source_path = build_source_path(root_path, file);
    let canonical_root = fs::canonicalize(root_path).map_err(|error| {
        format!(
            "Unable to resolve source root {}: {}",
            root_path.to_string_lossy(),
            error
        )
    })?;
    let canonical_source = fs::canonicalize(&source_path).map_err(|error| {
        format!(
            "Unable to resolve source file {}: {}",
            source_path.to_string_lossy(),
            error
        )
    })?;

    if !canonical_source.starts_with(&canonical_root) {
        return Err(format!(
            "Refused to read source file outside the scan root: {}.",
            canonical_source.to_string_lossy()
        ));
    }

    Ok(canonical_source)
}

pub(crate) fn estimated_export_payload_bytes(file: &RecoveredFile) -> u64 {
    file.size_bytes
        .saturating_add(
            file.resource_fork
                .as_ref()
                .map(|fork| fork.size_bytes)
                .unwrap_or(0),
        )
        .saturating_add(
            file.alternate_data_streams
                .as_ref()
                .map(|streams| streams.iter().map(|stream| stream.size_bytes).sum::<u64>())
                .unwrap_or(0),
        )
}

fn resource_fork_sidecar_path(target_path: &Path) -> Result<PathBuf, String> {
    let file_name = target_path.file_name().ok_or_else(|| {
        format!(
            "Unable to derive a resource-fork sidecar path for {}.",
            target_path.to_string_lossy()
        )
    })?;
    Ok(target_path.with_file_name(format!("{}.resource-fork.bin", file_name.to_string_lossy())))
}

fn sanitize_sidecar_component(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for character in value.chars() {
        let safe = matches!(
            character,
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.'
        );
        sanitized.push(if safe { character } else { '_' });
    }

    let trimmed = sanitized.trim_matches('_').trim_matches('.');
    if trimmed.is_empty() {
        "stream".into()
    } else {
        trimmed.to_string()
    }
}

fn alternate_data_stream_sidecar_path(
    target_path: &Path,
    stream_name: &str,
) -> Result<PathBuf, String> {
    let file_name = target_path.file_name().ok_or_else(|| {
        format!(
            "Unable to derive an alternate-data-stream sidecar path for {}.",
            target_path.to_string_lossy()
        )
    })?;
    let safe_stream_name = sanitize_sidecar_component(stream_name);
    Ok(target_path.with_file_name(format!(
        "{}.ads.{}.bin",
        file_name.to_string_lossy(),
        safe_stream_name
    )))
}

pub(crate) fn is_text_previewable_extension(extension: &str) -> bool {
    matches!(extension, "txt" | "md" | "log" | "json" | "csv")
}

pub(crate) fn is_document_previewable_extension(extension: &str) -> bool {
    matches!(extension, "docx" | "xlsx" | "pptx")
}

pub(crate) fn asset_preview_kind(extension: &str) -> Option<&'static str> {
    match extension {
        "jpg" | "jpeg" | "png" | "gif" | "webp" => Some("image"),
        "pdf" => Some("pdf"),
        _ => None,
    }
}

pub(crate) fn export_recovered_file(
    root_path: &Path,
    file: &RecoveredFile,
    target_path: &Path,
    allow_replace: bool,
) -> Result<u64, String> {
    let temp_path = temporary_sibling_path(target_path)?;
    let result = export_recovered_file_to_path(root_path, file, &temp_path);
    let written = match result {
        Ok(written) => written,
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }
    };
    finalize_temporary_file(&temp_path, target_path, allow_replace)?;
    Ok(written)
}

fn export_recovered_file_to_path(
    root_path: &Path,
    file: &RecoveredFile,
    target_path: &Path,
) -> Result<u64, String> {
    if file_uses_recovery_image(file) {
        let image_path = file.source_image_path.as_ref().ok_or_else(|| {
            format!(
                "Recovery image source is missing for {}. Re-run the recovery scan before exporting.",
                file.name
            )
        })?;
        let byte_runs = file.byte_runs.as_ref().ok_or_else(|| {
            format!(
                "Recovery image byte ranges are missing for {}. Re-run the recovery scan before exporting.",
                file.name
            )
        })?;

        imaging::materialize_byte_runs(
            Path::new(image_path),
            byte_runs,
            file.size_bytes,
            target_path,
        )
        .map_err(|error| {
            format!(
                "Unable to reconstruct {} from the recovery image into {}: {}",
                file.name,
                target_path.to_string_lossy(),
                error
            )
        })
    } else {
        let source_path = resolve_source_path_under_root(root_path, file)?;
        fs::copy(&source_path, target_path).map_err(|error| {
            format!(
                "Unable to copy {} to {}: {}",
                source_path.to_string_lossy(),
                target_path.to_string_lossy(),
                error
            )
        })
    }
}

pub(crate) fn export_resource_fork_sidecar(
    file: &RecoveredFile,
    target_path: &Path,
    conflict_strategy: &str,
) -> Result<Option<(PathBuf, u64)>, String> {
    let resource_fork = match file.resource_fork.as_ref() {
        Some(resource_fork) => resource_fork,
        None => return Ok(None),
    };

    let image_path = file.source_image_path.as_ref().ok_or_else(|| {
        format!(
            "Recovery image source is missing for the resource fork of {}.",
            file.name
        )
    })?;
    let requested_path = resource_fork_sidecar_path(target_path)?;
    let sidecar_path = match resolve_target_path(&requested_path, conflict_strategy)? {
        Some(path) => path,
        None => {
            return Err(format!(
                "Skipped the resource-fork sidecar for {} because a companion file already exists at the destination.",
                file.name
            ))
        }
    };

    let temp_path = temporary_sibling_path(&sidecar_path)?;
    let written = match imaging::materialize_byte_runs(
        Path::new(image_path),
        &resource_fork.byte_runs,
        resource_fork.size_bytes,
        &temp_path,
    ) {
        Ok(written) => written,
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            return Err(format!(
                "Unable to export the HFS+ resource-fork sidecar for {} into {}: {}",
                file.name,
                sidecar_path.to_string_lossy(),
                error
            ));
        }
    };
    finalize_temporary_file(&temp_path, &sidecar_path, conflict_strategy == "overwrite")?;

    Ok(Some((sidecar_path, written)))
}

pub(crate) fn export_alternate_data_stream_sidecars(
    file: &RecoveredFile,
    target_path: &Path,
    conflict_strategy: &str,
) -> Result<Vec<(PathBuf, u64, String)>, String> {
    let streams = match file.alternate_data_streams.as_ref() {
        Some(streams) if !streams.is_empty() => streams,
        _ => return Ok(Vec::new()),
    };

    let image_path = file.source_image_path.as_ref().ok_or_else(|| {
        format!(
            "Recovery image source is missing for the alternate data streams of {}.",
            file.name
        )
    })?;

    let mut exported = Vec::with_capacity(streams.len());
    for stream in streams {
        let requested_path = alternate_data_stream_sidecar_path(target_path, &stream.name)?;
        let sidecar_path = match resolve_target_path(&requested_path, conflict_strategy)? {
            Some(path) => path,
            None => {
                return Err(format!(
                    "Skipped the alternate-data-stream sidecar `{}` for {} because a companion file already exists at the destination.",
                    stream.name, file.name
                ))
            }
        };

        let temp_path = temporary_sibling_path(&sidecar_path)?;
        let written = match imaging::materialize_byte_runs(
            Path::new(image_path),
            &stream.byte_runs,
            stream.size_bytes,
            &temp_path,
        ) {
            Ok(written) => written,
            Err(error) => {
                let _ = fs::remove_file(&temp_path);
                return Err(format!(
                    "Unable to export the NTFS alternate-data-stream sidecar `{}` for {} into {}: {}",
                    stream.name,
                    file.name,
                    sidecar_path.to_string_lossy(),
                    error
                ));
            }
        };
        finalize_temporary_file(&temp_path, &sidecar_path, conflict_strategy == "overwrite")?;

        exported.push((sidecar_path, written, stream.name.clone()));
    }

    Ok(exported)
}

pub(crate) fn relative_dir_from_display_path(display_path: &str) -> PathBuf {
    let mut safe = PathBuf::new();
    for component in Path::new(display_path).components() {
        match component {
            Component::Normal(segment) => safe.push(segment),
            Component::CurDir | Component::ParentDir | Component::RootDir => {}
            Component::Prefix(_) => {}
        }
    }
    safe
}

pub(crate) fn safe_export_file_name(file_name: &str) -> String {
    let leaf = Path::new(file_name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("recovered-file");
    let sanitized = leaf
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            character if character.is_control() => '_',
            character => character,
        })
        .collect::<String>();
    let trimmed = sanitized.trim_matches(['.', ' ']);
    if trimmed.is_empty() {
        "recovered-file".to_string()
    } else if is_windows_reserved_file_name(trimmed) {
        match trimmed.split_once('.') {
            Some((stem, extension)) => format!("{stem}_.{extension}"),
            None => format!("{trimmed}_"),
        }
    } else {
        trimmed.to_string()
    }
}

fn is_windows_reserved_file_name(file_name: &str) -> bool {
    let stem = file_name
        .split_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(file_name)
        .trim_end_matches(['.', ' '])
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

pub(crate) fn resolve_target_path(
    target_path: &Path,
    conflict_strategy: &str,
) -> Result<Option<PathBuf>, String> {
    if !target_path.exists() {
        return Ok(Some(target_path.to_path_buf()));
    }

    match conflict_strategy {
        "rename" => Ok(Some(unique_target_path(target_path))),
        "skip" => Ok(None),
        "overwrite" => {
            if symlink_metadata_is_file_or_symlink(target_path)? {
                Ok(Some(target_path.to_path_buf()))
            } else {
                Err(format!(
                    "Unable to overwrite {} because it is not a regular file.",
                    target_path.to_string_lossy()
                ))
            }
        }
        other => Err(format!("Unsupported conflict strategy `{other}`.")),
    }
}

fn symlink_metadata_is_file_or_symlink(path: &Path) -> Result<bool, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "Unable to inspect existing destination {}: {}",
            path.to_string_lossy(),
            error
        )
    })?;
    let file_type = metadata.file_type();
    Ok(file_type.is_file() || file_type.is_symlink())
}

fn temporary_sibling_path(target_path: &Path) -> Result<PathBuf, String> {
    let parent = target_path.parent().ok_or_else(|| {
        format!(
            "The target {} has no writable parent directory.",
            target_path.to_string_lossy()
        )
    })?;
    let file_name = target_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("export");

    for _ in 0..16 {
        let candidate = parent.join(format!(".{file_name}.{}.tmp", random_hex_suffix()));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(format!(
        "Unable to allocate a temporary path next to {}.",
        target_path.to_string_lossy()
    ))
}

fn finalize_temporary_file(
    temp_path: &Path,
    target_path: &Path,
    allow_replace: bool,
) -> Result<(), String> {
    if let Ok(metadata) = fs::symlink_metadata(target_path) {
        let file_type = metadata.file_type();
        if file_type.is_dir() {
            let _ = fs::remove_file(temp_path);
            return Err(format!(
                "Refused to replace directory {}.",
                target_path.to_string_lossy()
            ));
        }
        if !allow_replace {
            let _ = fs::remove_file(temp_path);
            return Err(format!(
                "Refused to replace existing destination {}.",
                target_path.to_string_lossy()
            ));
        }
        fs::remove_file(target_path).map_err(|error| {
            let _ = fs::remove_file(temp_path);
            format!(
                "Unable to replace existing destination {}: {}",
                target_path.to_string_lossy(),
                error
            )
        })?;
    }

    fs::rename(temp_path, target_path).map_err(|error| {
        let _ = fs::remove_file(temp_path);
        format!(
            "Unable to finalize {} from temporary file {}: {}",
            target_path.to_string_lossy(),
            temp_path.to_string_lossy(),
            error
        )
    })
}

fn unique_target_path(target_path: &Path) -> PathBuf {
    let parent = target_path.parent().unwrap_or_else(|| Path::new(""));
    let stem = target_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("export");
    let extension = target_path.extension().and_then(|value| value.to_str());

    let mut counter = 1;
    loop {
        let candidate_name = match extension {
            Some(extension) if !extension.is_empty() => format!("{stem} ({counter}).{extension}"),
            _ => format!("{stem} ({counter})"),
        };
        let candidate = parent.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
        counter += 1;
    }
}

pub(crate) fn verify_exported_file(source_path: &Path, target_path: &Path) -> Result<(), String> {
    let source_metadata = fs::metadata(source_path)
        .map_err(|error| format!("unable to read source metadata: {error}"))?;
    let target_metadata = fs::metadata(target_path)
        .map_err(|error| format!("unable to read destination metadata: {error}"))?;

    if source_metadata.len() != target_metadata.len() {
        return Err(format!(
            "size mismatch (source {} bytes, destination {} bytes)",
            source_metadata.len(),
            target_metadata.len()
        ));
    }

    let source_hash = sha256_file(source_path)?;
    let target_hash = sha256_file(target_path)?;
    if source_hash != target_hash {
        return Err("sha256 mismatch between source and destination".into());
    }

    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| {
        format!(
            "unable to open {} for SHA-256 verification: {}",
            path.to_string_lossy(),
            error
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            format!(
                "unable to read {} for SHA-256 verification: {}",
                path.to_string_lossy(),
                error
            )
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn verify_reconstructed_export(
    target_path: &Path,
    expected_size: u64,
) -> Result<(), String> {
    let target_metadata = fs::metadata(target_path)
        .map_err(|error| format!("unable to read reconstructed destination metadata: {error}"))?;

    if target_metadata.len() != expected_size {
        return Err(format!(
            "size mismatch (expected {} bytes, destination {} bytes)",
            expected_size,
            target_metadata.len()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod export_tests {
    use super::default_export_batch_holdout;
    use crate::types::RecoveredFile;

    fn sample_file(id: &str) -> RecoveredFile {
        RecoveredFile {
            id: id.into(),
            name: format!("{id}.txt"),
            path: "/docs".into(),
            extension: "txt".into(),
            size_bytes: 128,
            integrity: "intact".into(),
            recovery_score: 90,
            recovery_method: "reconstruction".into(),
            preview_available: true,
            ..Default::default()
        }
    }

    #[test]
    fn default_export_batch_holdout_only_holds_apfs_preview_first_when_safer_files_exist() {
        let safe = sample_file("safe");
        let mut apfs = sample_file("apfs");
        apfs.is_deleted = true;
        apfs.source_view = Some("live-catalog".into());
        apfs.validator_status = Some("unsupported".into());

        let mixed = [safe, apfs.clone()];
        let held_out = default_export_batch_holdout(&mixed);
        assert_eq!(held_out.len(), 1);
        assert_eq!(held_out[0].id, apfs.id);

        let all_preview_first = [apfs];
        let held_out_all = default_export_batch_holdout(&all_preview_first);
        assert!(held_out_all.is_empty());
    }

    #[test]
    fn default_export_batch_holdout_includes_partial_unvalidated_apfs_candidates() {
        let safe = sample_file("safe");
        let mut apfs = sample_file("apfs-partial");
        apfs.is_deleted = true;
        apfs.source_view = Some("live-catalog".into());
        apfs.validator_status = Some("partial-unvalidated".into());

        let mixed = [safe, apfs.clone()];
        let held_out = default_export_batch_holdout(&mixed);
        assert_eq!(held_out.len(), 1);
        assert_eq!(held_out[0].id, apfs.id);
    }
}
