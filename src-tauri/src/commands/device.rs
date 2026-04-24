// ============================================================================
// Récupère — Device commands
// ============================================================================
// Command handlers that surface the storage devices attached to the
// host (detection, diagnostic, SMART, RAID, encryption). The detection and
// diagnostic flows exposed to the desktop UI stay read-only on the source
// media. Sensitive unlock helpers remain in this module for controlled
// internal/lab use, but are not exposed through the general desktop handler.
//
// The heuristic diagnostic builder `build_diagnostic` also lives here
// (Sprint 2.1 Pass B). It still reaches back into `commands::mod` for the
// imaging/volume probes that have not been extracted yet; those are marked
// `pub(super)` over there.
// ============================================================================

use crate::core;
use crate::types::{
    DetectedDevice, DeviceStatus, DeviceType, DiagnosticResult, FilesystemType,
    ImportedRecoverySourceStatus, Recommendation, RiskFactor, RiskLevel, SmartReport,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RaidAnalysisBuildRequest {
    pub level: String,
    pub ordered_member_device_ids: Vec<Option<String>>,
    pub stripe_size_bytes: u64,
    pub data_offset_bytes: u64,
}

struct ResolvedRaidBuildConfig {
    level: crate::raid::RaidLevel,
    ordered_member_paths: Vec<PathBuf>,
    missing_member_indexes: Vec<usize>,
    stripe_size_bytes: u64,
    data_offset_bytes: u64,
    expected_member_count: usize,
}

fn validate_raid_build_configuration(
    level: crate::raid::RaidLevel,
    expected_member_count: usize,
    member_slots: &[Option<String>],
    stripe_size_bytes: u64,
    data_offset_bytes: u64,
) -> serde_json::Value {
    let missing_member_count = member_slots.iter().filter(|slot| slot.is_none()).count();
    let populated_member_count = member_slots.len().saturating_sub(missing_member_count);
    let mut warnings = Vec::new();

    let duplicate_count = {
        let mut seen = std::collections::HashSet::new();
        member_slots
            .iter()
            .flatten()
            .filter(|member_id| !seen.insert((*member_id).to_string()))
            .count()
    };
    if duplicate_count > 0 {
        warnings.push("The same RAID member is selected more than once.".to_string());
    }
    if stripe_size_bytes == 0 {
        warnings.push("Stripe size must be greater than zero.".to_string());
    }
    if member_slots.len() != expected_member_count {
        warnings.push(format!(
            "This candidate expects {expected_member_count} slot(s), but the current build uses {}.",
            member_slots.len()
        ));
    }
    if populated_member_count == 0 {
        warnings.push("Select at least one readable RAID member.".to_string());
    }
    if data_offset_bytes > 16 * 1024 * 1024 {
        warnings.push(
            "The configured data offset is unusually high; verify the array metadata carefully."
                .to_string(),
        );
    }

    let (supported, variant, confidence, message) = match level {
        crate::raid::RaidLevel::Raid0 | crate::raid::RaidLevel::Jbod => {
            if missing_member_count > 0 {
                (
                    false,
                    "unsupported",
                    "low",
                    "RAID 0 / JBOD cannot be rebuilt safely with missing members.",
                )
            } else {
                (
                    warnings.is_empty(),
                    "supported",
                    if warnings.is_empty() {
                        "high"
                    } else {
                        "medium"
                    },
                    "Full array configuration detected.",
                )
            }
        }
        crate::raid::RaidLevel::Raid1 => {
            if missing_member_count >= expected_member_count {
                (
                    false,
                    "unsupported",
                    "low",
                    "At least one readable mirror member is required.",
                )
            } else if missing_member_count > 0 {
                (
                    duplicate_count == 0 && stripe_size_bytes > 0 && member_slots.len() == expected_member_count,
                    "degraded",
                    "medium",
                    "Degraded RAID 1 reconstruction is possible from the remaining mirror member(s).",
                )
            } else {
                (
                    warnings.is_empty(),
                    "supported",
                    if warnings.is_empty() {
                        "high"
                    } else {
                        "medium"
                    },
                    "Full mirror set detected.",
                )
            }
        }
        crate::raid::RaidLevel::Raid5 => {
            if missing_member_count > 1 {
                (
                    false,
                    "unsupported",
                    "low",
                    "RAID 5 currently supports at most one missing member.",
                )
            } else if missing_member_count == 1 {
                (
                    duplicate_count == 0
                        && stripe_size_bytes > 0
                        && member_slots.len() == expected_member_count,
                    "degraded",
                    "medium",
                    "Degraded RAID 5 reconstruction is supported with one missing member.",
                )
            } else {
                (
                    warnings.is_empty(),
                    "supported",
                    if warnings.is_empty() {
                        "high"
                    } else {
                        "medium"
                    },
                    "Full RAID 5 member set detected.",
                )
            }
        }
        crate::raid::RaidLevel::Raid6 => {
            if missing_member_count > 2 {
                (
                    false,
                    "unsupported",
                    "low",
                    "RAID 6 cannot be rebuilt with more than two missing members.",
                )
            } else if missing_member_count >= 1 {
                (
                    duplicate_count == 0 && stripe_size_bytes > 0 && member_slots.len() == expected_member_count,
                    "degraded",
                    "medium",
                    "Degraded RAID 6 reconstruction is supported within the current two-missing-member tolerance.",
                )
            } else {
                (
                    warnings.is_empty(),
                    "supported",
                    if warnings.is_empty() {
                        "high"
                    } else {
                        "medium"
                    },
                    "Full RAID 6 member set detected.",
                )
            }
        }
    };

    serde_json::json!({
        "supported": supported,
        "variant": variant,
        "confidence": confidence,
        "missing_member_count": missing_member_count,
        "message": message,
        "warnings": warnings,
    })
}

fn resolve_raid_build_config(
    devices: &[DetectedDevice],
    selected_device: &DetectedDevice,
    candidate: &crate::raid::RaidCandidate,
    request: Option<RaidAnalysisBuildRequest>,
) -> Result<ResolvedRaidBuildConfig, String> {
    if let Some(request) = request {
        if request.ordered_member_device_ids.is_empty() {
            return Err(
                "Select at least one RAID slot before building an analysis image.".to_string(),
            );
        }
        let level = parse_raid_level(&request.level)?;
        let mut ordered_member_paths = Vec::with_capacity(request.ordered_member_device_ids.len());
        let mut missing_member_indexes = Vec::new();
        for (index, member_id) in request.ordered_member_device_ids.iter().enumerate() {
            if let Some(member_id) = member_id {
                let member_device = devices
                    .iter()
                    .find(|candidate| candidate.id == *member_id)
                    .ok_or_else(|| format!("RAID member `{member_id}` is no longer available."))?;
                ordered_member_paths.push(analysis_path_for_device(member_device)?);
            } else {
                missing_member_indexes.push(index);
                ordered_member_paths.push(analysis_path_for_device(selected_device)?);
            }
        }
        return Ok(ResolvedRaidBuildConfig {
            level,
            ordered_member_paths,
            missing_member_indexes,
            stripe_size_bytes: request.stripe_size_bytes,
            data_offset_bytes: request.data_offset_bytes,
            expected_member_count: candidate.expected_member_count as usize,
        });
    }

    if candidate.members.len() != candidate.expected_member_count as usize {
        return Err(format!(
            "This RAID candidate is incomplete (detected {} member(s) out of {} expected). Connect the remaining members before building an analysis image.",
            candidate.members.len(),
            candidate.expected_member_count
        ));
    }

    let metadata = crate::raid::detect_raid_metadata(&candidate.members[0])
        .ok_or_else(|| "Unable to confirm RAID metadata for the selected candidate.".to_string())?;
    Ok(ResolvedRaidBuildConfig {
        level: candidate.level,
        ordered_member_paths: candidate.members.clone(),
        missing_member_indexes: Vec::new(),
        stripe_size_bytes: candidate.stripe_size_bytes,
        data_offset_bytes: metadata.data_offset_bytes,
        expected_member_count: candidate.expected_member_count as usize,
    })
}

fn parse_raid_level(label: &str) -> Result<crate::raid::RaidLevel, String> {
    match label {
        "Raid0" | "RAID0" | "raid0" => Ok(crate::raid::RaidLevel::Raid0),
        "Raid1" | "RAID1" | "raid1" => Ok(crate::raid::RaidLevel::Raid1),
        "Raid5" | "RAID5" | "raid5" => Ok(crate::raid::RaidLevel::Raid5),
        "Raid6" | "RAID6" | "raid6" => Ok(crate::raid::RaidLevel::Raid6),
        "Jbod" | "JBOD" | "jbod" => Ok(crate::raid::RaidLevel::Jbod),
        other => Err(format!("Unsupported RAID level `{other}`.")),
    }
}

fn analysis_path_for_device(device: &DetectedDevice) -> Result<PathBuf, String> {
    if let Some(path) = crate::imported_sources::resolve_analysis_source_path_if_imported(
        Path::new(&device.device_path),
    )? {
        return Ok(path);
    }

    Ok(PathBuf::from(&device.device_path))
}

fn imported_source_path_for_device(device_id: &str) -> Result<PathBuf, String> {
    let device = core::detect_devices()
        .into_iter()
        .find(|candidate| candidate.id == device_id)
        .ok_or_else(|| format!("Device {device_id} not found"))?;

    if !matches!(device.device_type, DeviceType::Image) {
        return Err(format!(
            "Device {device_id} is not an imported recovery source."
        ));
    }

    Ok(PathBuf::from(device.device_path))
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_devices() -> Vec<DetectedDevice> {
    tracing::info!("get_devices: detecting storage devices in read-only mode");
    core::detect_devices()
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_diagnostic(device_id: String) -> Result<DiagnosticResult, String> {
    let device = core::detect_devices()
        .into_iter()
        .find(|candidate| candidate.id == device_id)
        .ok_or_else(|| {
            format!("Device `{device_id}` was not found. Refresh detected devices and try again.")
        })?;

    tracing::info!(
        "get_diagnostic: building heuristic diagnostic for {} ({})",
        device.name,
        device.device_path
    );

    crate::audit::record(
        crate::audit::AuditEventKind::DeviceSelected,
        serde_json::json!({"device_id": device_id, "device_name": &device.name}),
    );

    Ok(build_diagnostic(&device))
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_smart_report(device_id: String) -> Result<SmartReport, String> {
    let device = core::detect_devices()
        .into_iter()
        .find(|d| d.id == device_id)
        .ok_or_else(|| format!("Device {device_id} not found"))?;
    if matches!(device.device_type, DeviceType::Image) {
        return Ok(SmartReport {
            device_id,
            overall_health: "unavailable".into(),
            temperature_celsius: None,
            power_on_hours: None,
            reallocated_sectors: None,
            pending_sectors: None,
            attributes: Vec::new(),
            error_log_count: None,
        });
    }
    Ok(core::get_smart_report(&device.device_path, &device_id))
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn detect_raid_metadata(device_id: String) -> Result<Option<serde_json::Value>, String> {
    let device = core::detect_devices()
        .into_iter()
        .find(|d| d.id == device_id)
        .ok_or_else(|| format!("Device {device_id} not found"))?;

    let path = analysis_path_for_device(&device)?;
    match crate::raid::detect_raid_metadata(&path) {
        Some(metadata) => Ok(Some(serde_json::json!({
            "level": format!("{:?}", metadata.level),
            "member_count": metadata.member_count,
            "stripe_size_bytes": metadata.stripe_size_bytes,
            "superblock_version": metadata.superblock_version,
            "data_offset_bytes": metadata.data_offset_bytes,
        }))),
        None => Ok(None),
    }
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn scan_raid_candidates(device_id: String) -> Result<Vec<serde_json::Value>, String> {
    let devices = core::detect_devices();
    let selected_device = devices
        .iter()
        .find(|candidate| candidate.id == device_id)
        .ok_or_else(|| format!("Device {device_id} not found"))?;

    let mut analysable_devices = Vec::new();
    let mut analysis_paths = Vec::new();
    for device in &devices {
        if let Ok(path) = analysis_path_for_device(device) {
            analysis_paths.push(path.clone());
            analysable_devices.push((device, path));
        }
    }

    let selected_path = analysis_path_for_device(selected_device)?;
    let candidates = crate::raid::scan_multi_disk_raid_candidates(&analysis_paths);

    Ok(candidates
        .into_iter()
        .filter_map(|candidate| {
            let members = analysable_devices
                .iter()
                .filter(|(_, path)| candidate.members.iter().any(|member| member == path))
                .map(|(device, _)| {
                    serde_json::json!({
                        "device_id": device.id,
                        "device_name": device.name,
                        "device_path": device.device_path,
                    })
                })
                .collect::<Vec<_>>();

            if !candidate
                .members
                .iter()
                .any(|member| member == &selected_path)
            {
                return None;
            }

            Some(serde_json::json!({
                "level": format!("{:?}", candidate.level),
                "expected_member_count": candidate.expected_member_count,
                "detected_member_count": candidate.members.len(),
                "stripe_size_bytes": candidate.stripe_size_bytes,
                "superblock_version": candidate.superblock_version,
                "members": members,
            }))
        })
        .collect())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn validate_raid_analysis_build(
    device_id: String,
    request: RaidAnalysisBuildRequest,
) -> Result<serde_json::Value, String> {
    let devices = core::detect_devices();
    let selected_device = devices
        .iter()
        .find(|candidate| candidate.id == device_id)
        .ok_or_else(|| format!("Device {device_id} not found"))?;

    let mut analysis_paths = Vec::new();
    for device in &devices {
        if let Ok(path) = analysis_path_for_device(device) {
            analysis_paths.push(path);
        }
    }

    let selected_path = analysis_path_for_device(selected_device)?;
    let candidate = crate::raid::scan_multi_disk_raid_candidates(&analysis_paths)
        .into_iter()
        .find(|candidate| {
            candidate
                .members
                .iter()
                .any(|member| member == &selected_path)
        })
        .ok_or_else(|| {
            format!(
                "No coherent RAID candidate including {} is currently available.",
                selected_device.name
            )
        })?;

    let level = parse_raid_level(&request.level)?;
    Ok(validate_raid_build_configuration(
        level,
        candidate.expected_member_count as usize,
        &request.ordered_member_device_ids,
        request.stripe_size_bytes,
        request.data_offset_bytes,
    ))
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn build_raid_analysis_image(
    device_id: String,
    request: Option<RaidAnalysisBuildRequest>,
) -> Result<serde_json::Value, String> {
    let devices = core::detect_devices();
    let selected_device = devices
        .iter()
        .find(|candidate| candidate.id == device_id)
        .ok_or_else(|| format!("Device {device_id} not found"))?;

    let mut analysable_devices = Vec::new();
    let mut analysis_paths = Vec::new();
    for device in &devices {
        if let Ok(path) = analysis_path_for_device(device) {
            analysis_paths.push(path.clone());
            analysable_devices.push((device, path));
        }
    }

    let selected_path = analysis_path_for_device(selected_device)?;
    let candidate = crate::raid::scan_multi_disk_raid_candidates(&analysis_paths)
        .into_iter()
        .find(|candidate| {
            candidate
                .members
                .iter()
                .any(|member| member == &selected_path)
        })
        .ok_or_else(|| {
            format!(
                "No coherent RAID candidate including {} is currently available.",
                selected_device.name
            )
        })?;

    let validation = if let Some(ref request) = request {
        let level = parse_raid_level(&request.level)?;
        validate_raid_build_configuration(
            level,
            candidate.expected_member_count as usize,
            &request.ordered_member_device_ids,
            request.stripe_size_bytes,
            request.data_offset_bytes,
        )
    } else {
        serde_json::json!({
            "supported": candidate.members.len() == candidate.expected_member_count as usize,
            "variant": "supported",
            "confidence": "high",
            "missing_member_count": 0,
            "message": "Full array configuration detected.",
            "warnings": []
        })
    };

    if !validation
        .get("supported")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return Err(validation
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or("The RAID configuration is not supported.")
            .to_string());
    }

    let resolved = resolve_raid_build_config(&devices, selected_device, &candidate, request)?;

    let mut hasher = Sha256::new();
    hasher.update(format!("{:?}", resolved.level).as_bytes());
    hasher.update(candidate.superblock_version.as_bytes());
    hasher.update(resolved.stripe_size_bytes.to_le_bytes());
    hasher.update(resolved.data_offset_bytes.to_le_bytes());
    for member in &resolved.ordered_member_paths {
        hasher.update(member.to_string_lossy().as_bytes());
    }
    for missing_index in &resolved.missing_member_indexes {
        hasher.update(format!("missing:{missing_index}").as_bytes());
    }
    let digest = hasher.finalize();
    let key: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let destination_path =
        crate::imported_sources::generated_analysis_artifact_dir("raid-analysis")
            .join(format!("raid-{}.img", key));

    let copied = crate::raid::materialize_raid_image(
        crate::raid::RaidConfig {
            level: resolved.level,
            member_paths: resolved.ordered_member_paths.clone(),
            stripe_size_bytes: resolved.stripe_size_bytes,
            data_offset_bytes: resolved.data_offset_bytes,
            missing_members: resolved.missing_member_indexes.clone(),
        },
        &destination_path,
        &mut |_copied| Ok(()),
    )?;

    let display_name = format!(
        "{} analysis ({})",
        format!("{:?}", resolved.level).replace("Raid", "RAID "),
        selected_device.name
    );
    let source_format = format!("{:?}", resolved.level).replace("Raid", "RAID");
    let record = crate::imported_sources::register_generated_recovery_source(
        &destination_path,
        &display_name,
        &source_format,
        copied,
    )?;

    crate::audit::record(
        crate::audit::AuditEventKind::SettingsChanged,
        serde_json::json!({
            "kind": "raid_analysis_image_built",
            "source_device_id": device_id,
            "level": format!("{:?}", resolved.level),
            "member_count": resolved.ordered_member_paths.len().saturating_sub(resolved.missing_member_indexes.len()),
            "missing_member_count": resolved.missing_member_indexes.len(),
            "output_path": record.path,
        }),
    );

    let imported_device_id = core::detect_devices()
        .into_iter()
        .find(|device| device.device_path == record.path)
        .map(|device| device.id)
        .ok_or_else(|| {
            "The RAID analysis image was built but could not be registered as a device.".to_string()
        })?;

    Ok(serde_json::json!({
        "device_id": imported_device_id,
        "path": record.path,
        "display_name": record.display_name,
        "level": source_format,
        "member_count": resolved.ordered_member_paths.len().saturating_sub(resolved.missing_member_indexes.len()),
        "expected_member_count": resolved.expected_member_count,
    }))
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_encryption_info(device_id: String) -> Result<crate::encryption::EncryptionInfo, String> {
    let device = core::detect_devices()
        .into_iter()
        .find(|d| d.id == device_id)
        .ok_or_else(|| format!("Device {device_id} not found"))?;
    let analysis_path = analysis_path_for_device(&device)?;
    let analysis_path_string = analysis_path.to_string_lossy().to_string();
    Ok(crate::encryption::detect_encryption_type(
        &analysis_path_string,
        device.is_encrypted.unwrap_or(false),
    ))
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn import_recovery_source(path: String) -> Result<ImportedRecoverySourceStatus, String> {
    let record = crate::imported_sources::import_recovery_source(Path::new(&path))?;
    tracing::info!(
        "import_recovery_source: registered {} ({})",
        record.display_name,
        record.path
    );
    crate::audit::record(
        crate::audit::AuditEventKind::SettingsChanged,
        serde_json::json!({
            "kind": "imported_recovery_source_added",
            "path": record.path,
            "format": record.source_format,
            "logical_size_bytes": record.logical_size_bytes,
        }),
    );
    crate::imported_sources::get_imported_source_status(Path::new(&record.path))?.ok_or_else(|| {
        "The imported recovery source was registered but no status is available.".into()
    })
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_imported_recovery_source_status(
    device_id: String,
) -> Result<Option<ImportedRecoverySourceStatus>, String> {
    let source_path = imported_source_path_for_device(&device_id)?;
    crate::imported_sources::get_imported_source_status(&source_path)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn prepare_imported_recovery_source(
    device_id: String,
) -> Result<ImportedRecoverySourceStatus, String> {
    let source_path = imported_source_path_for_device(&device_id)?;
    let status = crate::imported_sources::prepare_imported_source(&source_path)?;
    tracing::info!(
        "prepare_imported_recovery_source: prepared {} ({})",
        status.source_format,
        status.source_path
    );
    crate::audit::record(
        crate::audit::AuditEventKind::SettingsChanged,
        serde_json::json!({
            "kind": "imported_recovery_source_prepared",
            "path": status.source_path,
            "format": status.source_format,
            "analysis_path": status.analysis_path,
        }),
    );
    Ok(status)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remove_imported_recovery_source(path: String) -> Result<(), String> {
    crate::imported_sources::remove_imported_recovery_source(Path::new(&path))?;
    tracing::info!("remove_imported_recovery_source: removed {}", path);
    crate::audit::record(
        crate::audit::AuditEventKind::SettingsChanged,
        serde_json::json!({
            "kind": "imported_recovery_source_removed",
            "path": path,
        }),
    );
    Ok(())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn unlock_encrypted_device(device_id: String, _password: String) -> Result<String, String> {
    let device = core::detect_devices()
        .into_iter()
        .find(|d| d.id == device_id)
        .ok_or_else(|| format!("Device {device_id} not found"))?;
    crate::audit::record(
        crate::audit::AuditEventKind::SettingsChanged,
        serde_json::json!({
            "kind": "encrypted_volume_unlock_refused",
            "device_id": device_id,
            "device_path": device.device_path,
            "succeeded": false,
        }),
    );
    Err(
        "Encrypted volume unlock is disabled in this build because it can modify system state. \
         Create a read-only image first, then scan the image."
            .into(),
    )
}

pub fn bruteforce_encrypted_device(
    device_id: String,
    extra_passwords: Vec<String>,
) -> Result<crate::encryption::bruteforce::BruteForceResult, String> {
    use std::sync::{atomic::AtomicBool, Arc};
    let device = core::detect_devices()
        .into_iter()
        .find(|d| d.id == device_id)
        .ok_or_else(|| format!("Device {device_id} not found"))?;
    let cancel = Arc::new(AtomicBool::new(false));
    Ok(crate::encryption::bruteforce::run_dictionary_attack(
        &device.device_path,
        &extra_passwords,
        cancel,
        |_p| {},
    ))
}

// ----------------------------------------------------------------------------
// Heuristic diagnostic builder (Sprint 2.1 Pass B)
// ----------------------------------------------------------------------------
// `build_diagnostic` takes the metadata + SMART signal + imaging source plan
// resolved for a detected device and produces the localised advisory shown
// to the user (risk factors, recommendations, limitations, verdict). It does
// not read user data — every decision is based on `DetectedDevice` metadata,
// the resolved imaging plan, and the potential-volume probes. Callers:
// `get_diagnostic` above, `generate_recovery_report`, the AI advisory, and
// the support-bundle manifest.

pub(crate) fn build_diagnostic(device: &DetectedDevice) -> DiagnosticResult {
    let mounted_scan_available = core::primary_mount_path(device).is_some();
    let deleted_recovery_fs_label = if mounted_scan_available {
        deleted_entry_recovery_label(&device.filesystem)
    } else {
        None
    };
    let deleted_recovery_available = deleted_recovery_fs_label.is_some();
    let imaging_source_path = super::resolved_imaging_source_path(device).ok();
    let imaging_source_plan = super::resolve_imaging_source_plan(device);
    let imaging_ready = imaging_source_plan.is_ok();
    let imaging_profile = super::recommended_imaging_profile(device);
    let imaging_profile_reason_key = super::recommended_imaging_profile_reason_key(device);
    let signature_carving_available = imaging_ready;
    let imaging_requires_elevation = imaging_source_plan
        .as_ref()
        .map(|plan| plan.requires_elevation())
        .unwrap_or(false);
    let (potential_volumes_inspected, potential_volumes_notice, potential_volumes) =
        super::inspect_potential_volumes_for_diagnostic(&imaging_source_plan);
    let imaging_block_reason = imaging_source_plan.as_ref().err().cloned();
    let encryption_info = crate::encryption::detect_encryption_type(
        &device.device_path,
        device.is_encrypted.unwrap_or(false),
    );
    let encryption_pre_unlock_blocked =
        encryption_info.detected && encryption_info.workflow_state == "pre_unlock_blocked";
    let has_potential_lost_volume = !potential_volumes.is_empty();
    let has_apfs_potential_volume = potential_volumes
        .iter()
        .any(|volume| matches!(volume.filesystem, FilesystemType::Apfs));
    let apfs_snapshots_present = detect_apfs_local_snapshots(device);
    let supported_potential_volume_count = potential_volumes
        .iter()
        .filter(|volume| super::supported_potential_volume_filesystem(&volume.filesystem))
        .count();
    let best_supported_volume = super::best_supported_potential_volume(&potential_volumes);
    let guided_supported_volume =
        super::guided_supported_potential_volume_candidate(&potential_volumes);

    let mut observed_conditions = vec![
        "diagnostic.observed.metadata_only".into(),
        "diagnostic.observed.device_type_fs".into(),
    ];

    if core::primary_mount_path(device).is_some() {
        observed_conditions.push("diagnostic.observed.mount_available".into());
    } else {
        observed_conditions.push("diagnostic.observed.no_mount".into());
    }

    if matches!(
        &device.status,
        DeviceStatus::Degraded | DeviceStatus::Failing | DeviceStatus::Unresponsive
    ) {
        observed_conditions.push("diagnostic.observed.status_caution".into());
    }

    if has_potential_lost_volume {
        observed_conditions.push("diagnostic.observed.potential_volumes_found".into());
        if has_apfs_potential_volume {
            observed_conditions.push("diagnostic.observed.apfs_candidate".into());
        }
        if apfs_snapshots_present == Some(true) {
            observed_conditions.push("diagnostic.observed.apfs_snapshots_present".into());
        }
    } else if potential_volumes_inspected {
        observed_conditions.push("diagnostic.observed.no_volumes_found".into());
    } else if potential_volumes_notice.is_some() {
        observed_conditions.push("diagnostic.observed.volumes_notice".into());
    }

    let metadata_only_desc_key = if deleted_recovery_fs_label.is_some() {
        "risk.metadata_only.description_with_deleted"
    } else if signature_carving_available {
        "risk.metadata_only.description_with_carving"
    } else {
        "risk.metadata_only.description_basic"
    };

    let mut risk_factors = vec![RiskFactor {
        id: "metadata-only".into(),
        severity: RiskLevel::Medium,
        title_key: "risk.metadata_only.title".into(),
        description_key: metadata_only_desc_key.into(),
    }];

    if matches!(&device.device_type, DeviceType::Ssd | DeviceType::Nvme)
        && device.is_trim_enabled.unwrap_or(false)
    {
        risk_factors.push(RiskFactor {
            id: "trim".into(),
            severity: RiskLevel::High,
            title_key: "risk.trim.title".into(),
            description_key: "risk.trim.description".into(),
        });
    }

    if encryption_info.detected {
        risk_factors.push(RiskFactor {
            id: "encryption".into(),
            severity: RiskLevel::High,
            title_key: "risk.encryption.title".into(),
            description_key: "risk.encryption.description".into(),
        });
    }

    if matches!(
        &device.status,
        DeviceStatus::Failing | DeviceStatus::Unresponsive
    ) {
        risk_factors.push(RiskFactor {
            id: "health".into(),
            severity: RiskLevel::Critical,
            title_key: "risk.health.title".into(),
            description_key: "risk.health.description".into(),
        });
    } else if matches!(&device.status, DeviceStatus::Degraded) {
        risk_factors.push(RiskFactor {
            id: "degraded".into(),
            severity: RiskLevel::Medium,
            title_key: "risk.degraded.title".into(),
            description_key: "risk.degraded.description".into(),
        });
    }

    if matches!(&device.filesystem, FilesystemType::Unknown) {
        risk_factors.push(RiskFactor {
            id: "unknown-fs".into(),
            severity: RiskLevel::Medium,
            title_key: "risk.unknown_fs.title".into(),
            description_key: "risk.unknown_fs.description".into(),
        });
    }

    if has_potential_lost_volume {
        risk_factors.push(RiskFactor {
            id: "potential-lost-volume".into(),
            severity: RiskLevel::Medium,
            title_key: "risk.potential_lost_volume.title".into(),
            description_key: "risk.potential_lost_volume.description".into(),
        });
    }

    let score = crate::scoring::recoverability_score(
        &device.risk_level,
        &device.status,
        &device.device_type,
        device.is_trim_enabled,
        device.is_encrypted,
        &device.filesystem,
        device.smart_available,
        has_potential_lost_volume,
    ) as i32;

    let high_risk = matches!(&device.risk_level, RiskLevel::High | RiskLevel::Critical)
        || matches!(
            &device.status,
            DeviceStatus::Failing | DeviceStatus::Unresponsive
        );
    if imaging_requires_elevation {
        observed_conditions.push("diagnostic.observed.elevation_required".into());
    }

    let mut recommendations = Vec::new();

    if high_risk {
        recommendations.push(Recommendation {
            id: "stop-usage".into(),
            rec_type: "stop-usage".into(),
            priority: 1,
            title_key: "recommendation.stop_usage.title".into(),
            description_key: "recommendation.stop_usage.description".into(),
            is_recommended: true,
            target_potential_volume_id: None,
            target_potential_volume_label: None,
            target_potential_volume_filesystem: None,
            target_potential_volume_start_offset: None,
            target_potential_volume_size_bytes: None,
        });
    }

    let image_first_desc_key = if encryption_pre_unlock_blocked {
        "recommendation.image_first.description_encrypted"
    } else if imaging_block_reason.is_some() {
        "recommendation.image_first.description_blocked"
    } else if imaging_requires_elevation {
        "recommendation.image_first.description_elevation"
    } else if deleted_recovery_fs_label.is_some() {
        "recommendation.image_first.description_with_deleted"
    } else {
        "recommendation.image_first.description_default"
    };

    recommendations.push(Recommendation {
        id: "image-first".into(),
        rec_type: "image-first".into(),
        priority: if high_risk { 2 } else { 1 },
        title_key: "recommendation.image_first.title".into(),
        description_key: image_first_desc_key.into(),
        is_recommended: imaging_ready,
        target_potential_volume_id: None,
        target_potential_volume_label: None,
        target_potential_volume_filesystem: None,
        target_potential_volume_start_offset: None,
        target_potential_volume_size_bytes: None,
    });

    if signature_carving_available {
        recommendations.push(Recommendation {
            id: "scan-signature-carving".into(),
            rec_type: "scan-signature-carving".into(),
            priority: if deleted_recovery_available { 3 } else { 2 },
            title_key: "recommendation.scan_signature_carving.title".into(),
            description_key: "recommendation.scan_signature_carving.description".into(),
            is_recommended: !deleted_recovery_available
                && !high_risk
                && !encryption_pre_unlock_blocked,
            target_potential_volume_id: None,
            target_potential_volume_label: None,
            target_potential_volume_filesystem: None,
            target_potential_volume_start_offset: None,
            target_potential_volume_size_bytes: None,
        });
    }

    if let Some(volume) = guided_supported_volume.or(best_supported_volume) {
        let volume_fs_label = filesystem_label(&volume.filesystem);
        recommendations.push(Recommendation {
            id: format!("scan-lost-volume-{}", volume.id),
            rec_type: "scan-lost-volume".into(),
            priority: if high_risk { 3 } else { 2 },
            title_key: "recommendation.scan_lost_volume.title".into(),
            description_key: if supported_potential_volume_count > 1 {
                "recommendation.scan_lost_volume.description_multiple".into()
            } else {
                "recommendation.scan_lost_volume.description_single".into()
            },
            is_recommended: imaging_ready
                && guided_supported_volume.is_some()
                && !encryption_pre_unlock_blocked,
            target_potential_volume_id: Some(volume.id.clone()),
            target_potential_volume_label: Some(volume.label.clone()),
            target_potential_volume_filesystem: Some(volume_fs_label.to_lowercase()),
            target_potential_volume_start_offset: Some(volume.start_offset),
            target_potential_volume_size_bytes: volume.size_bytes,
        });
    }

    if has_potential_lost_volume {
        recommendations.push(Recommendation {
            id: "review-potential-volumes".into(),
            rec_type: "review-potential-volumes".into(),
            priority: if high_risk { 3 } else { 2 },
            title_key: "recommendation.review_potential_volumes.title".into(),
            description_key: if has_apfs_potential_volume {
                "recommendation.review_potential_volumes.description_apfs".into()
            } else {
                "recommendation.review_potential_volumes.description_default".into()
            },
            is_recommended: supported_potential_volume_count == 0
                && !encryption_pre_unlock_blocked
                && (!mounted_scan_available
                    || matches!(
                        &device.filesystem,
                        FilesystemType::Unknown | FilesystemType::Apfs
                    )),
            target_potential_volume_id: None,
            target_potential_volume_label: None,
            target_potential_volume_filesystem: None,
            target_potential_volume_start_offset: None,
            target_potential_volume_size_bytes: None,
        });
    }

    if mounted_scan_available {
        if let Some(fs_label) = deleted_recovery_fs_label {
            recommendations.push(Recommendation {
                id: format!("scan-deleted-{}", fs_label.to_lowercase()),
                rec_type: deleted_entry_recommendation_type(&device.filesystem)
                    .unwrap_or("scan-deleted")
                    .into(),
                priority: if high_risk { 2 } else { 1 },
                title_key: "recommendation.scan_deleted.title".into(),
                description_key: "recommendation.scan_deleted.description".into(),
                is_recommended: !encryption_pre_unlock_blocked,
                target_potential_volume_id: None,
                target_potential_volume_label: None,
                target_potential_volume_filesystem: None,
                target_potential_volume_start_offset: None,
                target_potential_volume_size_bytes: None,
            });
        }

        recommendations.push(Recommendation {
            id: "scan-now".into(),
            rec_type: if high_risk {
                "scan-deep".into()
            } else {
                "scan-quick".into()
            },
            priority: if high_risk { 4 } else { 3 },
            title_key: if high_risk {
                "recommendation.scan_deep.title".into()
            } else {
                "recommendation.scan_quick.title".into()
            },
            description_key: if deleted_recovery_fs_label.is_some() {
                "recommendation.scan_now.description_with_deleted".into()
            } else {
                "recommendation.scan_now.description_default".into()
            },
            is_recommended: !high_risk
                && !deleted_recovery_available
                && !encryption_pre_unlock_blocked,
            target_potential_volume_id: None,
            target_potential_volume_label: None,
            target_potential_volume_filesystem: None,
            target_potential_volume_start_offset: None,
            target_potential_volume_size_bytes: None,
        });
    } else {
        recommendations.push(Recommendation {
            id: "wait-mount".into(),
            rec_type: "scan-quick".into(),
            priority: 3,
            title_key: "recommendation.wait_mount.title".into(),
            description_key: "recommendation.wait_mount.description".into(),
            is_recommended: false,
            target_potential_volume_id: None,
            target_potential_volume_label: None,
            target_potential_volume_filesystem: None,
            target_potential_volume_start_offset: None,
            target_potential_volume_size_bytes: None,
        });
    }

    if device.is_encrypted.unwrap_or(false) || matches!(&device.status, DeviceStatus::Unresponsive)
    {
        recommendations.push(Recommendation {
            id: "professional-help".into(),
            rec_type: "professional-help".into(),
            priority: 4,
            title_key: "recommendation.professional_help.title".into(),
            description_key: "recommendation.professional_help.description".into(),
            is_recommended: false,
            target_potential_volume_id: None,
            target_potential_volume_label: None,
            target_potential_volume_filesystem: None,
            target_potential_volume_start_offset: None,
            target_potential_volume_size_bytes: None,
        });
    }

    let mut limitations = if deleted_recovery_fs_label.is_some() {
        vec![
            "diagnostic.limitation.deleted_recovery_limited".into(),
            "diagnostic.limitation.scores_estimate_deleted".into(),
        ]
    } else {
        vec![
            "diagnostic.limitation.catalog_only".into(),
            "diagnostic.limitation.scores_estimate_metadata".into(),
        ]
    };

    if signature_carving_available {
        limitations.push("diagnostic.limitation.carving_limited".into());
    }

    if matches!(&device.filesystem, FilesystemType::Ext4) {
        limitations.push("diagnostic.limitation.ext4_mvp".into());
    }

    if matches!(&device.filesystem, FilesystemType::HfsPlus) {
        limitations.push("diagnostic.limitation.hfsplus_mvp".into());
    }

    if matches!(&device.filesystem, FilesystemType::Apfs) {
        limitations.push("diagnostic.limitation.apfs_mvp".into());
        limitations.push("diagnostic.limitation.apfs_snapshot_gap".into());
        if apfs_snapshots_present == Some(true) {
            limitations.push("diagnostic.limitation.apfs_snapshots_present_unavailable".into());
        }
    }

    if has_potential_lost_volume {
        limitations.push("diagnostic.limitation.lost_volume_conservative".into());
    }

    if has_apfs_potential_volume {
        limitations.push("diagnostic.limitation.apfs_support_limited".into());
        limitations.push("diagnostic.limitation.apfs_snapshot_gap".into());
        if apfs_snapshots_present == Some(true) {
            limitations.push("diagnostic.limitation.apfs_snapshots_present_unavailable".into());
        }
    }

    if potential_volumes_inspected {
        limitations.push("diagnostic.limitation.lost_volume_mvp".into());
    }

    if let Some(notice) = potential_volumes_notice.as_ref() {
        limitations.push(notice.clone());
    }

    if !mounted_scan_available {
        limitations.push("diagnostic.limitation.no_mount".into());
    }

    if imaging_requires_elevation {
        limitations.push("diagnostic.limitation.elevation_required".into());
    }

    if let Some(reason) = imaging_block_reason.as_ref() {
        limitations.push(reason.clone());
    }

    if matches!(&device.device_type, DeviceType::Ssd | DeviceType::Nvme)
        && device.is_trim_enabled.unwrap_or(false)
    {
        limitations.push("diagnostic.limitation.trim_erased".into());
    }

    if encryption_info.detected {
        limitations.push("diagnostic.limitation.encryption_required".into());
    }
    if encryption_pre_unlock_blocked {
        limitations.push("diagnostic.limitation.encryption_locked_scan_blocked".into());
        if matches!(&device.filesystem, FilesystemType::Apfs)
            || has_apfs_potential_volume
            || encryption_info.encryption_type == crate::encryption::EncryptionType::FileVault2
        {
            limitations.push("diagnostic.limitation.apfs_pre_unlock_gap".into());
        }
    }

    if high_risk {
        limitations.push("diagnostic.limitation.high_risk_image_first".into());
    }

    let verdict = if score < 20 || matches!(&device.status, DeviceStatus::Unresponsive) {
        "lab"
    } else if score < 40
        || encryption_info.detected
        || matches!(&device.status, DeviceStatus::Failing)
    {
        "critical"
    } else if score < 65
        || matches!(&device.risk_level, RiskLevel::High)
        || (matches!(&device.device_type, DeviceType::Ssd | DeviceType::Nvme)
            && device.is_trim_enabled.unwrap_or(false))
    {
        "risky"
    } else {
        "simple"
    };

    let verdict_details: String = match verdict {
        "simple" => "Recovery is straightforward. You can safely recover your files here without professional help.".into(),
        "risky" => "Recovery is possible but requires caution. Create a disk image first and proceed carefully.".into(),
        "critical" => "This is a critical case. Stop using the device immediately. Image it once then analyze the image. Consider professional help if files are irreplaceable.".into(),
        "lab" => "Professional lab recovery is strongly recommended. Further attempts here risk permanent data loss. Create an image if possible and take it to a specialist.".into(),
        _ => String::new(),
    };

    DiagnosticResult {
        device_id: device.id.clone(),
        recoverability_score: score as u8,
        loss_type: if has_potential_lost_volume
            && (!mounted_scan_available || matches!(&device.filesystem, FilesystemType::Unknown))
        {
            "partition-lost".into()
        } else {
            "unknown".into()
        },
        probable_causes: observed_conditions,
        risk_factors,
        recommendations,
        limitations,
        imaging_ready,
        imaging_requires_elevation,
        imaging_profile: imaging_profile.as_str().into(),
        imaging_profile_reason_key: imaging_profile_reason_key.into(),
        imaging_source_path: imaging_source_path.map(|path| path.to_string_lossy().to_string()),
        imaging_block_reason,
        potential_volumes_inspected,
        potential_volumes_notice,
        potential_volumes,
        verdict: verdict.into(),
        verdict_details,
    }
}

#[allow(dead_code)]
fn device_type_label(device_type: &DeviceType) -> &'static str {
    match device_type {
        DeviceType::Hdd => "HDD",
        DeviceType::Ssd => "SSD",
        DeviceType::Nvme => "NVMe",
        DeviceType::Usb => "USB media",
        DeviceType::Sd => "SD card",
        DeviceType::External => "external storage",
        DeviceType::Image => "disk image",
    }
}

fn detect_apfs_local_snapshots(device: &DetectedDevice) -> Option<bool> {
    if !matches!(device.filesystem, FilesystemType::Apfs) {
        return None;
    }

    let mount_path = core::primary_mount_path(device)?;
    detect_apfs_local_snapshots_for_mount(&mount_path)
}

#[cfg(target_os = "macos")]
fn detect_apfs_local_snapshots_for_mount(mount_path: &Path) -> Option<bool> {
    let output = std::process::Command::new("diskutil")
        .args(["apfs", "listSnapshots", &mount_path.to_string_lossy()])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    parse_apfs_snapshot_listing(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "macos"))]
fn detect_apfs_local_snapshots_for_mount(_mount_path: &Path) -> Option<bool> {
    None
}

fn parse_apfs_snapshot_listing(output: &str) -> Option<bool> {
    let normalized = output.to_ascii_lowercase();

    if normalized.contains("no snapshots") || normalized.contains("found no snapshots") {
        return Some(false);
    }

    if output.contains("Snapshot UUID:")
        || output.contains("XID:")
        || normalized.contains("snapshot for disk")
        || normalized.contains("snapshots for disk")
    {
        return Some(true);
    }

    None
}

pub(crate) fn filesystem_label(filesystem: &FilesystemType) -> &'static str {
    match filesystem {
        FilesystemType::Ntfs => "NTFS",
        FilesystemType::Fat32 => "FAT32",
        FilesystemType::Exfat => "exFAT",
        FilesystemType::Apfs => "APFS",
        FilesystemType::HfsPlus => "HFS+",
        FilesystemType::Ext4 => "ext4",
        FilesystemType::Unknown => "unknown",
    }
}

fn deleted_entry_recovery_label(filesystem: &FilesystemType) -> Option<&'static str> {
    match filesystem {
        FilesystemType::Ntfs => Some("NTFS"),
        FilesystemType::Fat32 => Some("FAT32"),
        FilesystemType::Exfat => Some("exFAT"),
        FilesystemType::Ext4 => Some("EXT4"),
        FilesystemType::Apfs => Some("APFS"),
        _ => None,
    }
}

fn deleted_entry_recommendation_type(filesystem: &FilesystemType) -> Option<&'static str> {
    match filesystem {
        FilesystemType::Ntfs => Some("scan-deleted-ntfs"),
        FilesystemType::Fat32 => Some("scan-deleted-fat32"),
        FilesystemType::Exfat => Some("scan-deleted-exfat"),
        FilesystemType::Ext4 => Some("scan-deleted-ext4"),
        FilesystemType::HfsPlus => Some("scan-deleted-hfsplus"),
        FilesystemType::Apfs => Some("scan-deleted-apfs"),
        _ => None,
    }
}

#[cfg(test)]
mod device_tests {
    use super::parse_apfs_snapshot_listing;

    #[test]
    fn parse_apfs_snapshot_listing_detects_present_snapshots() {
        let listing = "Snapshots for disk1s5s1 (1 found)\n|\n+-- Snapshot UUID: ABCD\n    XID: 123";
        assert_eq!(parse_apfs_snapshot_listing(listing), Some(true));
    }

    #[test]
    fn parse_apfs_snapshot_listing_detects_absence() {
        let listing = "No snapshots for disk1s5s1";
        assert_eq!(parse_apfs_snapshot_listing(listing), Some(false));
    }
}
