// ============================================================================
// Récupère — Imaging commands and workers
// ============================================================================
// This module now owns the imaging entrypoint plus the read-only image-copy
// workers. Shared cross-domain helpers (session persistence, progress helpers,
// macOS privileged helper script builders, ...) still live in `mod.rs` and are
// consumed through `pub(super)` visibility until the final state extraction.
// ============================================================================

use std::fs;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
#[cfg(target_os = "macos")]
use std::time::Duration;
use std::time::SystemTime;

use crate::{core, imaging, types::TechnicalLogEntry};

use super::state::{
    append_scan_log, elapsed_seconds, fail_scan_session, finalize_cancelled_scan,
    initialize_scan_session, persist_scan_session, unix_timestamp_ms, update_progress,
    wait_for_scan_permission, InventoryScanSession, MAX_SESSION_LOGS,
};
use super::validate_export_destination;
pub(super) mod helpers;
pub(super) mod privileged_macos;

use helpers::{
    append_imaging_artifact_issue_logs, append_imaging_profile_log, imaging_profile_for_session,
    imaging_unreadable_error_count, read_u64_report, recommended_imaging_profile,
    recommended_imaging_profile_reason_key, resolve_imaging_source_plan,
    update_image_acquisition_progress, ImagingSourcePlan,
};

#[cfg(target_os = "macos")]
use helpers::read_image_artifact_report;
#[cfg(target_os = "macos")]
use privileged_macos::{
    build_macos_privileged_imager_script, build_privileged_imaging_failure,
    create_privileged_helper_temp_dir,
};

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn start_imaging(
    device_id: String,
    destination_path: String,
    rescue_map_path: Option<String>,
) -> Result<String, String> {
    let trimmed_destination = destination_path.trim();
    if trimmed_destination.is_empty() {
        return Err("Choose a destination file before starting disk imaging.".into());
    }

    let device = core::detect_devices()
        .into_iter()
        .find(|candidate| candidate.id == device_id)
        .ok_or_else(|| {
            format!("Device `{device_id}` was not found. Refresh detected devices and try again.")
        })?;
    let source_plan = resolve_imaging_source_plan(&device)?;
    let source_path = source_plan.source_path().to_path_buf();

    let validation = validate_export_destination(
        trimmed_destination.to_string(),
        source_path.to_string_lossy().to_string(),
    );
    if !validation.is_safe {
        return Err(validation.message);
    }

    let root_path = source_path;
    let total_bytes = device.capacity_bytes.max(1);
    let imaging_profile = recommended_imaging_profile(&device);
    let imaging_profile_reason_key = recommended_imaging_profile_reason_key(&device).to_string();
    let (session_id, session) = initialize_scan_session(
        &device,
        "image",
        &root_path,
        total_bytes,
        Some(imaging_profile),
        Some(imaging_profile_reason_key),
    );

    tracing::info!(
        "start_imaging: session={} device={} source={} destination={} rescue_map={}",
        session_id,
        device.name,
        root_path.to_string_lossy(),
        trimmed_destination,
        rescue_map_path.as_deref().unwrap_or("<none>")
    );

    crate::audit::record(
        crate::audit::AuditEventKind::ImagingStarted,
        serde_json::json!({"scan_id": &session_id, "device_id": &device_id}),
    );

    let session_id_for_thread = session_id.clone();
    let destination_path_for_thread = PathBuf::from(trimmed_destination);
    let rescue_map_path_for_thread =
        rescue_map_path.and_then(|path| (!path.trim().is_empty()).then_some(PathBuf::from(path)));
    thread::spawn(move || {
        run_image_acquisition_to(
            session_id_for_thread,
            session,
            source_plan,
            total_bytes,
            destination_path_for_thread,
            rescue_map_path_for_thread,
        );
    });

    Ok(session_id)
}

#[cfg(target_os = "macos")]
#[allow(clippy::too_many_arguments)]
fn run_macos_privileged_image_acquisition(
    session_id: &str,
    session: &Arc<Mutex<InventoryScanSession>>,
    executable_path: &Path,
    source_device_path: &Path,
    total_bytes: u64,
    destination_path: &Path,
    profile: imaging::ImagingProfile,
    started_at: &SystemTime,
) -> Result<imaging::ImageArtifact, String> {
    append_scan_log(
        session,
        "warning",
        format!(
            "macOS requires administrator approval to read {} in strict read-only mode. The app will now request elevation for the physical device only.",
            source_device_path.to_string_lossy()
        ),
    );

    let helper_temp_dir = create_privileged_helper_temp_dir(session_id)?;
    let progress_file = helper_temp_dir.join("progress.txt");
    let error_file = helper_temp_dir.join("error.txt");
    let summary_file = helper_temp_dir.join("summary.json");
    let applescript = build_macos_privileged_imager_script(
        executable_path,
        source_device_path,
        destination_path,
        &progress_file,
        &error_file,
        &summary_file,
        profile,
    );
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let output = Command::new("osascript")
            .arg("-e")
            .arg(applescript)
            .output();
        let _ = sender.send(output);
    });

    loop {
        match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(output_result) => {
                if let Some(copied_bytes) = read_u64_report(&progress_file) {
                    update_image_acquisition_progress(
                        session,
                        copied_bytes,
                        total_bytes,
                        started_at,
                    );
                } else {
                    update_image_acquisition_progress(session, 0, total_bytes, started_at);
                }

                let outcome = match output_result {
                    Ok(output) if output.status.success() => Ok(read_image_artifact_report(
                        &summary_file,
                    )
                    .unwrap_or_else(|| {
                        let bytes_copied = read_u64_report(&progress_file).or_else(|| {
                            fs::metadata(destination_path)
                                .ok()
                                .map(|metadata| metadata.len())
                        });
                        imaging::ImageArtifact {
                            path: destination_path.to_path_buf(),
                            bytes_copied: bytes_copied.unwrap_or(0),
                            resume_from_bytes: 0,
                            unreadable_ranges_count: 0,
                            unreadable_bytes: 0,
                            unreadable_ranges: Vec::new(),
                            unreadable_range_samples: Vec::new(),
                            rescued_after_retry_bytes: 0,
                            retry_passes_completed: 0,
                        }
                    })),
                    Ok(output) => Err(build_privileged_imaging_failure(&output, &error_file)),
                    Err(error) => Err(format!(
                        "Unable to request administrator approval for read-only imaging: {error}"
                    )),
                };

                let _ = fs::remove_dir_all(&helper_temp_dir);
                return outcome;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                update_image_acquisition_progress(
                    session,
                    read_u64_report(&progress_file).unwrap_or(0),
                    total_bytes,
                    started_at,
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = fs::remove_dir_all(&helper_temp_dir);
                return Err(
                    "The privileged imaging helper stopped unexpectedly before completion.".into(),
                );
            }
        }
    }
}

pub(super) fn run_image_acquisition(
    session_id: String,
    session: Arc<Mutex<InventoryScanSession>>,
    source_plan: ImagingSourcePlan,
    total_bytes: u64,
) {
    run_image_acquisition_with_destination(
        session_id,
        session,
        source_plan,
        total_bytes,
        None,
        None,
    );
}

pub(super) fn run_image_acquisition_to(
    session_id: String,
    session: Arc<Mutex<InventoryScanSession>>,
    source_plan: ImagingSourcePlan,
    total_bytes: u64,
    destination_path: PathBuf,
    rescue_map_path: Option<PathBuf>,
) {
    run_image_acquisition_with_destination(
        session_id,
        session,
        source_plan,
        total_bytes,
        Some(destination_path),
        rescue_map_path,
    );
}

pub(super) fn create_local_image_snapshot(
    session_id: &str,
    session: &Arc<Mutex<InventoryScanSession>>,
    source_plan: ImagingSourcePlan,
    total_bytes: u64,
    destination_path: &Path,
    started_at: &SystemTime,
) -> Result<imaging::ImageArtifact, String> {
    let profile = imaging_profile_for_session(session);
    match source_plan {
        ImagingSourcePlan::Direct { source_path } => {
            imaging::create_read_only_image_at_controlled_with_profile(
                destination_path,
                &source_path,
                profile,
                &mut |copied_bytes| {
                    wait_for_scan_permission(session)?;
                    update_image_acquisition_progress(
                        session,
                        copied_bytes,
                        total_bytes,
                        started_at,
                    );
                    Ok(())
                },
            )
        }
        #[cfg(target_os = "macos")]
        ImagingSourcePlan::ElevatedMacOs {
            source_path,
            executable_path,
        } => run_macos_privileged_image_acquisition(
            session_id,
            session,
            &executable_path,
            &source_path,
            total_bytes,
            destination_path,
            profile,
            started_at,
        ),
    }
}

fn run_image_acquisition_with_destination(
    session_id: String,
    session: Arc<Mutex<InventoryScanSession>>,
    source_plan: ImagingSourcePlan,
    total_bytes: u64,
    destination_path: Option<PathBuf>,
    rescue_map_path: Option<PathBuf>,
) {
    let source_device_path = source_plan.source_path().to_path_buf();
    let explicit_destination = destination_path.is_some();
    let image_destination =
        destination_path.unwrap_or_else(|| imaging::workspace_image_path_for_scan(&session_id));

    append_scan_log(
        &session,
        "info",
        format!(
            "Starting standalone read-only imaging from {}.",
            source_device_path.to_string_lossy()
        ),
    );

    update_progress(&session, |progress| {
        progress.status = "scanning".into();
        progress.stage = "creating-image".into();
        progress.percent_complete = 2.0;
        progress.total_bytes = total_bytes.max(1);
    });
    append_scan_log(
        &session,
        "info",
        if explicit_destination {
            format!(
                "Creating a read-only image at the selected destination {}.",
                image_destination.to_string_lossy()
            )
        } else {
            "Creating a local read-only image in the protected workspace.".into()
        },
    );
    append_imaging_profile_log(&session);

    if let Some(mapfile_path) = rescue_map_path.as_ref() {
        append_scan_log(
            &session,
            "info",
            format!(
                "Attempting to import the external rescue map {} for the existing partial image at {}.",
                mapfile_path.to_string_lossy(),
                image_destination.to_string_lossy()
            ),
        );
        append_scan_log(
            &session,
            "warning",
            "External rescue maps only guide reuse of a matching local partial image. They do not reconstruct missing source bytes on their own.".into(),
        );

        match imaging::import_external_rescue_map_for_image_destination(
            &image_destination,
            &source_device_path,
            total_bytes,
            mapfile_path,
        ) {
            Ok(import_summary) => {
                append_scan_log(
                    &session,
                    "info",
                    format!(
                        "Imported external rescue map successfully: {} reusable byte(s), {} unresolved segment(s), {} unresolved byte(s). Recupere will restart its own cautious retry-pass counter from zero on top of this imported checkpoint.",
                        import_summary.resume_from_bytes,
                        import_summary.unreadable_ranges_count,
                        import_summary.unreadable_bytes
                    ),
                );
            }
            Err(error) => {
                fail_scan_session(&session, error);
                return;
            }
        }
    }

    let started_at = SystemTime::now();

    let image_artifact_result = create_local_image_snapshot(
        &session_id,
        &session,
        source_plan,
        total_bytes,
        &image_destination,
        &started_at,
    );

    let image_artifact = match image_artifact_result {
        Ok(artifact) => artifact,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };
    append_imaging_artifact_issue_logs(&session, &image_artifact, "Read-only imaging");

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let elapsed_seconds = elapsed_seconds(&started_at);
    let mut state = crate::commands::state::lock_or_recover(&session, "scan session (imaging)");
    state.progress.status = "completed".into();
    state.progress.stage = "finalizing".into();
    state.progress.elapsed_seconds = elapsed_seconds;
    state.progress.percent_complete = 100.0;
    state.progress.bytes_scanned = image_artifact.bytes_copied;
    state.progress.total_bytes = total_bytes.max(image_artifact.bytes_copied).max(1);
    state.progress.resume_from_bytes = image_artifact.resume_from_bytes;
    state.progress.unreadable_ranges_count = image_artifact.unreadable_ranges_count;
    state.progress.unreadable_bytes = image_artifact.unreadable_bytes;
    state.progress.rescued_after_retry_bytes = image_artifact.rescued_after_retry_bytes;
    state.progress.retry_passes_completed = image_artifact.retry_passes_completed;
    state.progress.unreadable_ranges = image_artifact
        .unreadable_ranges
        .iter()
        .map(|range| crate::types::ImagingMapRange {
            start_offset: range.start_offset,
            length: range.length,
        })
        .collect();
    state.progress.errors_count = imaging_unreadable_error_count(&image_artifact);
    state.imaging_unreadable_ranges = image_artifact
        .unreadable_ranges
        .iter()
        .map(|range| crate::types::ImagingMapRange {
            start_offset: range.start_offset,
            length: range.length,
        })
        .collect();
    state.imaging_rescued_after_retry_bytes = image_artifact.rescued_after_retry_bytes;
    state.imaging_retry_passes_completed = image_artifact.retry_passes_completed;
    state.progress.files_found = 0;
    state.completed_at_ms = Some(unix_timestamp_ms());
    let completion_timestamp = state.completed_at_ms.unwrap_or_else(unix_timestamp_ms);
    if image_artifact.resume_from_bytes > 0 {
        if state.logs.len() >= MAX_SESSION_LOGS {
            state.logs.remove(0);
        }
        state.logs.push(TechnicalLogEntry {
            timestamp_ms: completion_timestamp,
            level: "info".into(),
            message: format!(
                "Read-only imaging resumed from an existing partial local image ({} bytes already captured).",
                image_artifact.resume_from_bytes
            ),
        });
    }
    if state.logs.len() >= MAX_SESSION_LOGS {
        state.logs.remove(0);
    }
    state.logs.push(TechnicalLogEntry {
        timestamp_ms: completion_timestamp,
        level: "info".into(),
        message: format!(
            "Standalone read-only imaging completed: image saved at {} ({} bytes).",
            image_artifact.path.to_string_lossy(),
            image_artifact.bytes_copied
        ),
    });
    drop(state);

    if let Err(error) = persist_scan_session(&session) {
        tracing::info!("run_image_acquisition: unable to persist completed session: {error}");
    }
}
