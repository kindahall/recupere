// ============================================================================
// Récupère — Imaging helpers (Sprint 5, Chantier 76 slice `imaging_helpers`)
// ============================================================================
// Extraction mécanique des helpers imaging qui vivaient auparavant dans
// `commands/mod.rs`. Les signatures restent identiques, seule la visibilité
// est promue en `pub(crate)` pour permettre la ré-exportation
// `pub(super) use imaging_cmd::{...}` depuis `commands/mod.rs` (les appelants
// frères — `scan.rs`, `device.rs`, `imaging_cmd/mod.rs` — continuent de
// résoudre `super::<fn>` via la ré-exportation).
// ============================================================================

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::{core, imaging, partitioning, types::*};

use super::super::state::{
    self, append_scan_log, elapsed_seconds, update_progress, InventoryScanSession,
};
use super::privileged_macos::privileged_imager_executable_path;

#[cfg(target_os = "macos")]
use super::privileged_macos::{
    build_macos_privileged_imager_script, build_privileged_imaging_failure,
    create_privileged_helper_temp_dir,
};

#[derive(Debug, Clone)]
pub(crate) enum ImagingSourcePlan {
    Direct {
        source_path: PathBuf,
    },
    #[cfg(target_os = "macos")]
    ElevatedMacOs {
        source_path: PathBuf,
        executable_path: PathBuf,
    },
}

impl ImagingSourcePlan {
    pub(crate) fn source_path(&self) -> &Path {
        match self {
            Self::Direct { source_path } => source_path.as_path(),
            #[cfg(target_os = "macos")]
            Self::ElevatedMacOs { source_path, .. } => source_path.as_path(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn requires_elevation(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            matches!(self, Self::ElevatedMacOs { .. })
        }

        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }
}

pub(crate) fn resolved_imaging_source_path(device: &DetectedDevice) -> Result<PathBuf, String> {
    if let Some(imported_analysis_path) =
        crate::imported_sources::resolve_analysis_source_path_if_imported(Path::new(
            &device.device_path,
        ))?
    {
        return Ok(imported_analysis_path);
    }

    Ok(core::preferred_imaging_source_path(&device.device_path))
}

fn is_raw_device_path(source_path: &Path) -> bool {
    source_path.to_string_lossy().starts_with("/dev/")
}

fn is_permission_denied_imaging_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("does not have permission")
        || lower.contains("permission denied")
        || lower.contains("operation not permitted")
        || lower.contains("os error 13")
        || lower.contains("(os error 1)")
}

pub(crate) fn imaging_requires_elevation_fallback(
    source_path: &Path,
    validation_error: &str,
    privileged_fallback_available: bool,
) -> bool {
    privileged_fallback_available
        && is_raw_device_path(source_path)
        && is_permission_denied_imaging_error(validation_error)
}

pub(crate) fn recommended_imaging_profile(device: &DetectedDevice) -> imaging::ImagingProfile {
    if matches!(
        device.status,
        DeviceStatus::Degraded | DeviceStatus::Failing | DeviceStatus::Unresponsive
    ) || matches!(device.risk_level, RiskLevel::High | RiskLevel::Critical)
    {
        imaging::ImagingProfile::Cautious
    } else {
        imaging::ImagingProfile::Standard
    }
}

pub(crate) fn recommended_imaging_profile_reason_key(device: &DetectedDevice) -> &'static str {
    if matches!(
        device.status,
        DeviceStatus::Failing | DeviceStatus::Unresponsive
    ) {
        "imaging.profile_reason_failing"
    } else if matches!(device.status, DeviceStatus::Degraded) {
        "imaging.profile_reason_degraded"
    } else if matches!(device.risk_level, RiskLevel::High | RiskLevel::Critical) {
        "imaging.profile_reason_risk"
    } else {
        "imaging.profile_reason_standard"
    }
}

pub(crate) fn append_imaging_profile_log(session: &Arc<Mutex<InventoryScanSession>>) {
    let (profile, reason_key) = {
        let state = state::lock_or_recover(session, "scan session");
        (
            state.imaging_profile,
            state.imaging_profile_reason_key.clone(),
        )
    };

    if profile != Some(imaging::ImagingProfile::Cautious) {
        return;
    }

    let reason = match reason_key.as_deref() {
        Some("imaging.profile_reason_failing") => {
            "the source reports a failing or unresponsive hardware state"
        }
        Some("imaging.profile_reason_degraded") => "the source reports a degraded hardware state",
        Some("imaging.profile_reason_risk") => "the source is currently classified as high risk",
        _ => "the current source requires extra caution",
    };
    append_scan_log(
        session,
        "warning",
        format!(
            "Using the cautious read-only imaging profile ({reason}). Reads will use smaller chunks and limited retries instead of maximum throughput."
        ),
    );
}

pub(crate) fn imaging_profile_for_session(
    session: &Arc<Mutex<InventoryScanSession>>,
) -> imaging::ImagingProfile {
    state::lock_or_recover(session, "scan session")
        .imaging_profile
        .unwrap_or(imaging::ImagingProfile::Standard)
}

pub(crate) fn imaging_unreadable_error_count(artifact: &imaging::ImageArtifact) -> u32 {
    u32::try_from(artifact.unreadable_ranges_count).unwrap_or(u32::MAX)
}

pub(crate) fn append_imaging_artifact_issue_logs(
    session: &Arc<Mutex<InventoryScanSession>>,
    artifact: &imaging::ImageArtifact,
    context_label: &str,
) {
    if artifact.retry_passes_completed > 0 {
        append_scan_log(
            session,
            "info",
            format!(
                "{} ran {} targeted cautious rescue pass(es) over previously unreadable ranges, alternating direction, trimming edges, probing central islands, zooming around newly recovered pockets, splitting partial progress into finer local retries, prioritizing smaller residual gaps, and micro-scraping with finer blocks when possible.",
                context_label, artifact.retry_passes_completed
            ),
        );
    }

    if artifact.rescued_after_retry_bytes > 0 {
        append_scan_log(
            session,
            "info",
            format!(
                "{} recovered {} byte(s) during targeted cautious rescue passes after the initial sweep.",
                context_label, artifact.rescued_after_retry_bytes
            ),
        );
    }

    if artifact.unreadable_bytes == 0 {
        return;
    }

    append_scan_log(
        session,
        "warning",
        format!(
            "{} completed with {} unreadable source segment(s) neutralized as zero-filled gaps ({} bytes total).",
            context_label,
            artifact.unreadable_ranges_count,
            artifact.unreadable_bytes
        ),
    );

    for range in &artifact.unreadable_range_samples {
        append_scan_log(
            session,
            "warning",
            format!(
                "Unreadable source segment zero-filled at offset 0x{:X} ({} bytes).",
                range.start_offset, range.length
            ),
        );
    }
}

pub(crate) fn apply_imaging_artifact_issue_metrics(
    progress: &mut ScanProgress,
    artifact: &imaging::ImageArtifact,
) {
    progress.unreadable_ranges_count = artifact.unreadable_ranges_count;
    progress.unreadable_bytes = artifact.unreadable_bytes;
    progress.unreadable_ranges = artifact
        .unreadable_ranges
        .iter()
        .map(|range| crate::types::ImagingMapRange {
            start_offset: range.start_offset,
            length: range.length,
        })
        .collect();
    progress.errors_count = progress
        .errors_count
        .max(imaging_unreadable_error_count(artifact));
}

pub(crate) fn apply_imaging_artifact_session_details(
    session: &Arc<Mutex<InventoryScanSession>>,
    artifact: &imaging::ImageArtifact,
) {
    let mut state = state::lock_or_recover(session, "scan session");
    state.imaging_unreadable_ranges = artifact
        .unreadable_ranges
        .iter()
        .map(|range| crate::types::ImagingMapRange {
            start_offset: range.start_offset,
            length: range.length,
        })
        .collect();
    state.imaging_rescued_after_retry_bytes = artifact.rescued_after_retry_bytes;
    state.imaging_retry_passes_completed = artifact.retry_passes_completed;
}

pub(crate) fn resolve_imaging_source_plan(
    device: &DetectedDevice,
) -> Result<ImagingSourcePlan, String> {
    let source_path = resolved_imaging_source_path(device)?;
    match core::validate_imaging_source_readable(&source_path) {
        Ok(()) => Ok(ImagingSourcePlan::Direct { source_path }),
        Err(error) => {
            let privileged_fallback_available = privileged_imager_executable_path().is_some();
            if imaging_requires_elevation_fallback(
                &source_path,
                &error,
                privileged_fallback_available,
            ) {
                #[cfg(target_os = "macos")]
                {
                    return Ok(ImagingSourcePlan::ElevatedMacOs {
                        source_path,
                        executable_path: privileged_imager_executable_path()
                            .expect("privileged imager executable path should be cached"),
                    });
                }
            }

            Err(error)
        }
    }
}

pub(crate) fn update_image_acquisition_progress(
    session: &Arc<Mutex<InventoryScanSession>>,
    copied_bytes: u64,
    total_bytes: u64,
    started_at: &SystemTime,
) {
    let percent = if total_bytes == 0 {
        95.0
    } else {
        ((copied_bytes as f64 / total_bytes as f64) * 96.0).clamp(2.0, 98.0)
    };
    update_progress(session, |progress| {
        progress.bytes_scanned = copied_bytes;
        progress.percent_complete = percent as f32;
        progress.elapsed_seconds = elapsed_seconds(started_at);
    });
}

pub(crate) fn read_u64_report(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[cfg(target_os = "macos")]
pub(crate) fn read_image_artifact_report(path: &Path) -> Option<imaging::ImageArtifact> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

#[cfg(target_os = "macos")]
fn try_unmount_macos_device(session: &Arc<Mutex<InventoryScanSession>>, source_device_path: &Path) {
    let path_str = source_device_path.to_string_lossy().to_string();
    if !path_str.starts_with("/dev/") {
        return;
    }
    let unmount_partition = std::process::Command::new("diskutil")
        .arg("unmount")
        .arg(&path_str)
        .output();
    if let Ok(output) = unmount_partition.as_ref() {
        if output.status.success() {
            append_scan_log(
                session,
                "info",
                format!("Unmounted {path_str} so the device can be imaged read-only."),
            );
            return;
        }
    }
    if let Some(stripped) = path_str.strip_prefix("/dev/") {
        let whole_disk = stripped.trim_start_matches('r');
        let mut whole = String::new();
        for ch in whole_disk.chars() {
            if ch == 's' && whole.starts_with("disk") && whole.len() > 4 {
                break;
            }
            whole.push(ch);
        }
        let whole_path = format!("/dev/{whole}");
        let _ = std::process::Command::new("diskutil")
            .arg("unmountDisk")
            .arg(&whole_path)
            .output()
            .ok()
            .map(|output| {
                if output.status.success() {
                    append_scan_log(
                        session,
                        "info",
                        format!("Unmounted {whole_path} so the device can be imaged read-only."),
                    );
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    append_scan_log(
                        session,
                        "warning",
                        format!(
                            "Could not unmount {whole_path} cleanly: {stderr}. Continuing anyway."
                        ),
                    );
                }
            });
    }
}

#[cfg(target_os = "macos")]
fn run_macos_privileged_image_acquisition_for_recovery(
    session_id: &str,
    session: &Arc<Mutex<InventoryScanSession>>,
    executable_path: &Path,
    source_device_path: &Path,
    destination_path: &Path,
    profile: imaging::ImagingProfile,
    progress_cb: &mut dyn FnMut(u64),
) -> Result<imaging::ImageArtifact, String> {
    try_unmount_macos_device(session, source_device_path);

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

    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let output = std::process::Command::new("osascript")
            .arg("-e")
            .arg(applescript)
            .output();
        let _ = sender.send(output);
    });

    loop {
        match receiver.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(output_result) => {
                if let Some(copied_bytes) = read_u64_report(&progress_file) {
                    progress_cb(copied_bytes);
                }
                let outcome = match output_result {
                    Ok(output) if output.status.success() => {
                        let artifact =
                            read_image_artifact_report(&summary_file).unwrap_or_else(|| {
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
                            });
                        if artifact.bytes_copied > 0 {
                            progress_cb(artifact.bytes_copied);
                        }
                        append_scan_log(
                            session,
                            "info",
                            format!(
                                "Administrator-approved read-only image written to {}.",
                                destination_path.to_string_lossy()
                            ),
                        );
                        Ok(artifact)
                    }
                    Ok(output) => Err(build_privileged_imaging_failure(&output, &error_file)),
                    Err(error) => Err(format!(
                        "Unable to request administrator approval for read-only imaging: {error}"
                    )),
                };
                let _ = fs::remove_dir_all(&helper_temp_dir);
                return outcome;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Some(copied_bytes) = read_u64_report(&progress_file) {
                    progress_cb(copied_bytes);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                let _ = fs::remove_dir_all(&helper_temp_dir);
                return Err(
                    "The privileged imaging helper stopped unexpectedly before completion.".into(),
                );
            }
        }
    }
}

pub(crate) fn inspect_potential_volumes_for_diagnostic(
    imaging_source_plan: &Result<ImagingSourcePlan, String>,
) -> (bool, Option<String>, Vec<PotentialVolume>) {
    match imaging_source_plan {
        Ok(ImagingSourcePlan::Direct { source_path }) => {
            match partitioning::inspect_potential_volumes(source_path) {
                Ok(volumes) => (true, None, volumes),
                Err(error) => (
                    true,
                    Some(format!(
                        "Potential lost-volume inspection could not complete on the current source: {error}"
                    )),
                    Vec::new(),
                ),
            }
        }
        Ok(plan) if plan.requires_elevation() => (
            false,
            Some(
                "Potential lost-volume inspection needs direct raw-device access in the current process. On this macOS host, create an image first or approve imaging before deeper lost-partition analysis."
                    .into(),
            ),
            Vec::new(),
        ),
        Err(error) => (
            false,
            Some(format!(
                "Potential lost-volume inspection is unavailable for this source. {error}"
            )),
            Vec::new(),
        ),
        #[allow(unreachable_patterns)]
        _ => (false, None, Vec::new()),
    }
}

/// Image a source for recovery, transparently elevating via the macOS privileged
/// imager helper when the direct read-only attempt fails with EACCES on a raw
/// device. On any other platform, or when the privileged helper is unavailable,
/// this is equivalent to `imaging::create_read_only_image`.
pub(crate) fn create_read_only_image_with_optional_elevation(
    session_id: &str,
    session: &Arc<Mutex<InventoryScanSession>>,
    source_device_path: &Path,
    profile: imaging::ImagingProfile,
    progress_cb: &mut dyn FnMut(u64),
) -> Result<imaging::ImageArtifact, String> {
    #[cfg(not(target_os = "macos"))]
    let _ = session;

    if profile == imaging::ImagingProfile::Cautious {
        append_imaging_profile_log(session);
    }

    match imaging::create_read_only_image_with_profile(
        session_id,
        source_device_path,
        profile,
        progress_cb,
    ) {
        Ok(artifact) => Ok(artifact),
        Err(error) => {
            #[cfg(target_os = "macos")]
            {
                if is_raw_device_path(source_device_path)
                    && is_permission_denied_imaging_error(&error)
                {
                    if let Some(executable_path) = privileged_imager_executable_path() {
                        append_scan_log(
                            session,
                            "warning",
                            format!(
                                "Direct read-only access to {} was denied. Requesting administrator approval to image the source.",
                                source_device_path.to_string_lossy()
                            ),
                        );
                        let destination_path = imaging::workspace_image_path_for_scan(session_id);
                        return run_macos_privileged_image_acquisition_for_recovery(
                            session_id,
                            session,
                            &executable_path,
                            source_device_path,
                            &destination_path,
                            profile,
                            progress_cb,
                        );
                    }
                }
            }
            Err(error)
        }
    }
}
