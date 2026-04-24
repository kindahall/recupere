// ============================================================================
// Récupère — Scan commands
// ============================================================================
// This module owns the scan entrypoints, the session lifecycle reads, and all
// the heavy recovery workers: FAT32/exFAT/NTFS deleted-entry workers
// (Sprint 5 slice `scan_deleted_fat_family`), ext4/HFS+/APFS deleted-entry
// workers (Sprint 5 slice `scan_deleted_unix_family`), plus inventory,
// signature-carving and potential-volume workers (Sprint 5 slice
// `scan_workers_tail`). Shared helpers (session lifecycle, imaging artifact
// plumbing, filesystem labels, image-snapshot orchestration) still live in
// `commands::state`, `commands::imaging_cmd::helpers`, `commands::device` and
// `commands::imaging_cmd` respectively and are reached through the explicit
// `use super::{...}` block below so this file stays mechanical.
// ============================================================================

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::SystemTime,
};

use crate::{
    analyzers::{apfs, exfat, ext4, fat32, hfsplus, ntfs},
    carving, core, imaging, partitioning,
    types::{
        ByteRun, FileFork, FilesystemType, ImagingMapRange, NamedFileFork, PotentialVolume,
        RecoveredFile, ScanProgress, ScanSessionSummary, TechnicalLogEntry,
    },
};
use chrono::{SecondsFormat, TimeZone, Utc};

use super::state::{InventoryScanSession, MAX_SESSION_LOGS};
use super::{
    append_imaging_artifact_issue_logs, append_scan_log, apply_imaging_artifact_issue_metrics,
    apply_imaging_artifact_session_details, create_read_only_image_with_optional_elevation,
    elapsed_seconds, fail_scan_session, filesystem_label, finalize_cancelled_scan, imaging_cmd,
    imaging_profile_for_session, persist_scan_session, unix_timestamp_ms, update_progress,
    wait_for_scan_permission, ImagingSourcePlan,
};

const QUICK_SCAN_MAX_DEPTH: usize = 2;

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn start_scan(device_id: String, scan_type: String) -> Result<String, String> {
    let normalized_scan_type = super::normalize_scan_type(&scan_type)?;

    let device = core::detect_devices()
        .into_iter()
        .find(|candidate| candidate.id == device_id)
        .ok_or_else(|| {
            format!("Device `{device_id}` was not found. Refresh detected devices and try again.")
        })?;

    let imaging_source_plan = if normalized_scan_type == "image"
        || normalized_scan_type == "signature-carving"
        || normalized_scan_type == "reconstruction"
    {
        Some(super::resolve_imaging_source_plan(&device)?)
    } else {
        None
    };

    let root_path = if let Some(plan) = imaging_source_plan.as_ref() {
        plan.source_path().to_path_buf()
    } else if normalized_scan_type == "carving" {
        if !matches!(
            device.filesystem,
            FilesystemType::Fat32
                | FilesystemType::Exfat
                | FilesystemType::Ntfs
                | FilesystemType::Ext4
                | FilesystemType::HfsPlus
                | FilesystemType::Apfs
        ) {
            return Err(
                "Deleted-file recovery MVP is currently limited to FAT32, exFAT, NTFS, ext4, HFS+, and APFS sources."
                    .into(),
            );
        }

        PathBuf::from(device.device_path.clone())
    } else {
        core::primary_mount_path(&device).ok_or_else(|| {
            "This device does not expose a mounted filesystem path that can be cataloged safely."
                .to_string()
        })?
    };

    if normalized_scan_type != "carving"
        && normalized_scan_type != "image"
        && normalized_scan_type != "signature-carving"
        && normalized_scan_type != "reconstruction"
        && !root_path.is_dir()
    {
        return Err("The selected scan target is not an accessible mounted directory.".into());
    }

    let total_bytes = if normalized_scan_type == "carving"
        || normalized_scan_type == "image"
        || normalized_scan_type == "signature-carving"
        || normalized_scan_type == "reconstruction"
    {
        device.capacity_bytes.max(1)
    } else if device.used_bytes > 0 {
        device.used_bytes
    } else {
        device.capacity_bytes.max(1)
    };
    let imaging_profile = if normalized_scan_type == "carving"
        || normalized_scan_type == "image"
        || normalized_scan_type == "signature-carving"
        || normalized_scan_type == "reconstruction"
    {
        Some(super::recommended_imaging_profile(&device))
    } else {
        None
    };
    let imaging_profile_reason_key =
        imaging_profile.map(|_| super::recommended_imaging_profile_reason_key(&device).to_string());
    let (session_id, session) = super::initialize_scan_session(
        &device,
        normalized_scan_type,
        &root_path,
        total_bytes,
        imaging_profile,
        imaging_profile_reason_key,
    );

    tracing::info!(
        "start_scan: session={} device={} root={} scan_type={}",
        session_id,
        device.name,
        root_path.to_string_lossy(),
        normalized_scan_type
    );

    crate::audit::record(
        crate::audit::AuditEventKind::ScanStarted,
        serde_json::json!({"scan_id": &session_id, "device_id": &device_id, "scan_type": normalized_scan_type}),
    );

    let session_id_for_thread = session_id.clone();
    let root_path_for_thread = root_path.clone();
    let imaging_source_plan_for_thread = imaging_source_plan.clone();
    let deleted_recovery_filesystem = device.filesystem.clone();
    thread::spawn(move || {
        if normalized_scan_type == "carving" {
            match deleted_recovery_filesystem {
                FilesystemType::Fat32 => super::run_deleted_fat32_scan(
                    session_id_for_thread,
                    session,
                    root_path_for_thread,
                    total_bytes,
                ),
                FilesystemType::Exfat => super::run_deleted_exfat_scan(
                    session_id_for_thread,
                    session,
                    root_path_for_thread,
                    total_bytes,
                ),
                FilesystemType::Ntfs => super::run_deleted_ntfs_scan(
                    session_id_for_thread,
                    session,
                    root_path_for_thread,
                    total_bytes,
                ),
                FilesystemType::Ext4 => super::run_deleted_ext4_scan(
                    session_id_for_thread,
                    session,
                    root_path_for_thread,
                    total_bytes,
                ),
                FilesystemType::HfsPlus => super::run_deleted_hfsplus_scan(
                    session_id_for_thread,
                    session,
                    root_path_for_thread,
                    total_bytes,
                ),
                FilesystemType::Apfs => super::run_deleted_apfs_scan(
                    session_id_for_thread,
                    session,
                    root_path_for_thread,
                    total_bytes,
                ),
                other => super::fail_scan_session(
                    &session,
                    format!(
                        "Deleted-file recovery MVP is not implemented for {} sources yet.",
                        super::filesystem_label(&other)
                    ),
                ),
            }
        } else if normalized_scan_type == "signature-carving"
            || normalized_scan_type == "reconstruction"
        {
            super::run_signature_carving_scan(
                session_id_for_thread,
                session,
                imaging_source_plan_for_thread
                    .expect("signature carving should keep the resolved source plan"),
                total_bytes,
            );
        } else if normalized_scan_type == "image" {
            super::imaging_cmd::run_image_acquisition(
                session_id_for_thread,
                session,
                imaging_source_plan_for_thread
                    .expect("image scan should keep the resolved source plan"),
                total_bytes,
            );
        } else {
            super::run_inventory_scan(
                session_id_for_thread,
                session,
                root_path_for_thread,
                normalized_scan_type,
            );
        }
    });

    Ok(session_id)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn start_potential_volume_scan(device_id: String, volume_id: String) -> Result<String, String> {
    let device = core::detect_devices()
        .into_iter()
        .find(|candidate| candidate.id == device_id)
        .ok_or_else(|| {
            format!("Device `{device_id}` was not found. Refresh detected devices and try again.")
        })?;

    let source_plan = super::resolve_imaging_source_plan(&device)?;
    let potential_volumes = partitioning::inspect_potential_volumes(source_plan.source_path())?;
    let volume = potential_volumes
        .into_iter()
        .find(|candidate| candidate.id == volume_id)
        .ok_or_else(|| {
            format!(
                "Potential volume `{volume_id}` is no longer available on the selected source. Refresh the diagnostic and try again."
            )
        })?;

    if !supported_potential_volume_filesystem(&volume.filesystem) {
        return Err(format!(
            "Potential volume `{}` uses `{}` which is not yet analyzable in this MVP. Supported filesystems are NTFS, FAT32, exFAT, HFS+, and APFS with conservative visible-file plus limited deleted-file cataloging.",
            volume.label,
            super::filesystem_label(&volume.filesystem)
        ));
    }

    let total_bytes = device
        .capacity_bytes
        .saturating_add(volume.size_bytes.unwrap_or(device.capacity_bytes / 2))
        .max(device.capacity_bytes)
        .max(1);
    let root_path = source_plan.source_path().to_path_buf();
    let (session_id, session) = super::initialize_scan_session(
        &device,
        "lost-volume",
        &root_path,
        total_bytes,
        Some(super::recommended_imaging_profile(&device)),
        Some(super::recommended_imaging_profile_reason_key(&device).to_string()),
    );

    tracing::info!(
        "start_potential_volume_scan: session={} device={} volume={} fs={} root={} offset={}",
        session_id,
        device.id,
        volume.id,
        super::filesystem_label(&volume.filesystem),
        root_path.to_string_lossy(),
        volume.start_offset
    );

    let session_id_for_thread = session_id.clone();
    thread::spawn(move || {
        super::run_potential_volume_scan(
            session_id_for_thread,
            session,
            source_plan,
            volume,
            total_bytes,
        );
    });

    Ok(session_id)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_scan_progress(scan_id: String) -> Result<ScanProgress, String> {
    let session = super::get_session(&scan_id)?;
    let session = crate::commands::state::lock_or_recover(&session, "scan session (scan)");
    Ok(session.progress.clone())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_results(scan_id: String) -> Result<Vec<RecoveredFile>, String> {
    let session = super::get_session(&scan_id)?;
    let session = crate::commands::state::lock_or_recover(&session, "scan session (scan)");
    Ok(session.results.clone())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn pause_scan(scan_id: String) -> Result<(), String> {
    let session = super::get_session(&scan_id)?;
    super::request_scan_pause(&session)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn resume_scan(scan_id: String) -> Result<(), String> {
    let session = super::get_session(&scan_id)?;
    super::request_scan_resume(&session)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn cancel_scan(scan_id: String) -> Result<(), String> {
    let session = super::get_session(&scan_id)?;
    crate::audit::record(
        crate::audit::AuditEventKind::ScanCanceled,
        serde_json::json!({"scan_id": &scan_id}),
    );
    super::request_scan_cancel(&session)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_scan_history() -> Vec<ScanSessionSummary> {
    let mut history_by_id: HashMap<String, ScanSessionSummary> =
        super::load_persisted_scan_archive()
            .scans
            .into_iter()
            .map(|record| (record.summary.id.clone(), record.summary))
            .collect();

    let sessions =
        crate::commands::state::lock_or_recover(super::scan_sessions(), "scan session registry");

    for session in sessions.values() {
        let summary = super::build_scan_summary(session);
        history_by_id.insert(summary.id.clone(), summary);
    }

    let mut history: Vec<ScanSessionSummary> = history_by_id.into_values().collect();
    history.sort_by(|left, right| right.started_at_ms.cmp(&left.started_at_ms));
    history
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_scan_logs(scan_id: String) -> Result<Vec<TechnicalLogEntry>, String> {
    if let Some(session) =
        crate::commands::state::lock_or_recover(super::scan_sessions(), "scan session registry")
            .get(&scan_id)
            .cloned()
    {
        let session = crate::commands::state::lock_or_recover(&session, "scan session (scan)");
        return Ok(session.logs.clone());
    }

    super::load_persisted_scan_record(&scan_id)
        .map(|record| record.logs)
        .ok_or_else(|| format!("Scan session `{scan_id}` was not found."))
}

fn session_uses_imaging(summary: &ScanSessionSummary) -> bool {
    summary.scan_type == "image" || summary.resume_from_bytes > 0 || summary.unreadable_bytes > 0
}

fn imaging_operator_status(summary: &ScanSessionSummary) -> &'static str {
    if summary.unreadable_bytes > 0 || summary.unreadable_ranges_count > 0 {
        return "degraded";
    }

    if summary.rescued_after_retry_bytes > 0 || summary.retry_passes_completed > 0 {
        return "rescued";
    }

    if summary.resume_from_bytes > 0 {
        return "resumed";
    }

    "stable"
}

fn imaging_operator_summary(summary: &ScanSessionSummary) -> (&'static str, &'static str) {
    match imaging_operator_status(summary) {
        "degraded" => (
            "Image completed with unrecovered source gaps.",
            "Keep the rescue map and incident report with the case, continue analysis on the image, and treat the zero-filled gaps as still-missing data.",
        ),
        "rescued" => (
            "Targeted cautious rescue passes recovered additional readable bytes after the initial sweep.",
            "Preserve both the image and the rescue map, then continue analysis on the resulting image instead of retrying the original source again.",
        ),
        "resumed" => (
            "Imaging resumed from a coherent local partial image without recording new unreadable gaps.",
            "Keep using the same local image destination and preserve this report so later work continues from the same known-good partial state.",
        ),
        _ => (
            "Stable copy path with no recorded unreadable source gaps or targeted rescue activity.",
            "Use this image as the working source for scan and export, and archive this report with the case notes.",
        ),
    }
}

fn format_timestamp_ms_rfc3339(timestamp_ms: u64) -> String {
    let seconds = (timestamp_ms / 1000) as i64;
    let nanos = ((timestamp_ms % 1000) as u32) * 1_000_000;
    Utc.timestamp_opt(seconds, nanos)
        .single()
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_else(|| format!("{timestamp_ms} ms"))
}

fn load_scan_summary_for_report(scan_id: &str) -> Option<ScanSessionSummary> {
    if let Some(session) =
        crate::commands::state::lock_or_recover(super::scan_sessions(), "scan session registry")
            .get(scan_id)
            .cloned()
    {
        return Some(super::build_scan_summary(&session));
    }

    super::load_persisted_scan_record(scan_id).map(|record| record.summary)
}

fn format_mapfile_hex(value: u64) -> String {
    format!("0x{value:016X}")
}

fn format_unreadable_range_line(range: &ImagingMapRange) -> String {
    let end_offset = range
        .start_offset
        .saturating_add(range.length.saturating_sub(1));
    format!(
        "{} - {} ({} bytes)",
        format_mapfile_hex(range.start_offset),
        format_mapfile_hex(end_offset),
        range.length
    )
}

fn append_unreadable_range_sample_report(report: &mut String, summary: &ScanSessionSummary) {
    if summary.unreadable_ranges.is_empty() {
        return;
    }

    let mut ranges = summary.unreadable_ranges.clone();
    ranges.sort_by(|left, right| {
        right
            .length
            .cmp(&left.length)
            .then_with(|| left.start_offset.cmp(&right.start_offset))
    });

    report.push_str("=== UNREADABLE RANGE SAMPLE ===\n");
    if let Some(largest_range) = ranges.first() {
        report.push_str(&format!(
            "Largest unreadable range: {}\n",
            format_unreadable_range_line(largest_range)
        ));
    }
    report.push_str(&format!(
        "Precise unreadable ranges persisted: {}\n",
        ranges.len()
    ));
    report.push_str("Sampled ranges:\n");
    for range in ranges.iter().take(8) {
        report.push_str(&format!("- {}\n", format_unreadable_range_line(range)));
    }
    if ranges.len() > 8 {
        report.push_str(&format!(
            "- ... {} additional precise range(s) omitted from this sample.\n",
            ranges.len() - 8
        ));
    }
    report.push('\n');
}

fn imaging_map_domain_end(summary: &ScanSessionSummary) -> u64 {
    summary.total_bytes.max(summary.bytes_copied)
}

fn imaging_source_preparation_state(summary: &ScanSessionSummary) -> Option<&'static str> {
    match (
        summary.source_requires_preparation,
        summary.source_prepared,
        summary.source_available,
    ) {
        (Some(true), Some(true), _) => Some("prepared-local-analysis"),
        (Some(true), Some(false), _) => Some("preparation-pending"),
        (Some(false), _, Some(true)) => Some("direct-analysis"),
        (Some(false), _, Some(false)) => Some("source-unavailable"),
        _ => None,
    }
}

fn append_imaging_source_provenance_report(report: &mut String, summary: &ScanSessionSummary) {
    if summary.source_display_name.is_none()
        && summary.source_kind.is_none()
        && summary.source_format.is_none()
        && summary.source_analysis_path.is_none()
        && summary.source_available.is_none()
        && summary.source_requires_preparation.is_none()
        && summary.source_prepared.is_none()
        && !summary.reconstructed_raid_source
    {
        return;
    }

    report.push_str("=== SOURCE PROVENANCE ===\n");
    if let Some(display_name) = summary.source_display_name.as_ref() {
        report.push_str(&format!("Registered source: {}\n", display_name));
    }
    if let Some(source_kind) = summary.source_kind.as_ref() {
        report.push_str(&format!("Source kind: {}\n", source_kind));
    }
    if let Some(source_format) = summary.source_format.as_ref() {
        report.push_str(&format!("Source format: {}\n", source_format));
    }
    if let Some(source_analysis_path) = summary.source_analysis_path.as_ref() {
        report.push_str(&format!("Analysis path: {}\n", source_analysis_path));
    }
    if let Some(source_available) = summary.source_available {
        report.push_str(&format!(
            "Source available: {}\n",
            if source_available { "yes" } else { "no" }
        ));
    }
    if let Some(preparation_state) = imaging_source_preparation_state(summary) {
        report.push_str(&format!("Preparation state: {}\n", preparation_state));
    }
    if summary.reconstructed_raid_source {
        report.push_str("Reconstructed RAID analysis source: yes\n");
    }
    report.push('\n');
}

fn append_imaging_source_provenance_mapfile(mapfile: &mut String, summary: &ScanSessionSummary) {
    if let Some(display_name) = summary.source_display_name.as_ref() {
        mapfile.push_str(&format!("# Registered source: {}\n", display_name));
    }
    if let Some(source_kind) = summary.source_kind.as_ref() {
        mapfile.push_str(&format!("# Source kind: {}\n", source_kind));
    }
    if let Some(source_format) = summary.source_format.as_ref() {
        mapfile.push_str(&format!("# Source format: {}\n", source_format));
    }
    if let Some(source_analysis_path) = summary.source_analysis_path.as_ref() {
        mapfile.push_str(&format!("# Analysis path: {}\n", source_analysis_path));
    }
    if let Some(source_available) = summary.source_available {
        mapfile.push_str(&format!(
            "# Source available: {}\n",
            if source_available { "yes" } else { "no" }
        ));
    }
    if let Some(preparation_state) = imaging_source_preparation_state(summary) {
        mapfile.push_str(&format!("# Preparation state: {}\n", preparation_state));
    }
    if summary.reconstructed_raid_source {
        mapfile.push_str("# Reconstructed RAID analysis source: yes\n");
    }
}

fn build_imaging_map_blocks(summary: &ScanSessionSummary) -> Result<Vec<(u64, u64, char)>, String> {
    if summary.unreadable_bytes > 0
        && summary.unreadable_ranges_count > 0
        && summary.unreadable_ranges.is_empty()
    {
        return Err(
            "This imaging session predates persisted rescue-map ranges and cannot produce a precise mapfile."
                .into(),
        );
    }

    let domain_end = imaging_map_domain_end(summary);
    if domain_end == 0 {
        return Ok(Vec::new());
    }

    let copied_end = summary.bytes_copied.min(domain_end);
    let mut ranges: Vec<ImagingMapRange> = summary
        .unreadable_ranges
        .iter()
        .filter(|range| range.length > 0)
        .cloned()
        .collect();
    ranges.sort_by_key(|range| range.start_offset);

    let mut blocks = Vec::new();
    let mut cursor = 0_u64;

    for range in ranges {
        if range.start_offset >= copied_end {
            break;
        }

        let range_start = range.start_offset.min(copied_end);
        let range_end = range
            .start_offset
            .saturating_add(range.length)
            .min(copied_end);

        if range_end <= cursor {
            continue;
        }

        if range_start > cursor {
            blocks.push((cursor, range_start.saturating_sub(cursor), '+'));
        }

        let bad_start = range_start.max(cursor);
        if range_end > bad_start {
            blocks.push((bad_start, range_end.saturating_sub(bad_start), '-'));
            cursor = range_end;
        }
    }

    if copied_end > cursor {
        blocks.push((cursor, copied_end.saturating_sub(cursor), '+'));
    }

    if domain_end > copied_end {
        blocks.push((copied_end, domain_end.saturating_sub(copied_end), '?'));
    }

    Ok(blocks
        .into_iter()
        .filter(|(_, length, _)| *length > 0)
        .collect())
}

fn build_imaging_rescue_map(summary: &ScanSessionSummary) -> Result<String, String> {
    let build_info = super::app_build_info();
    let blocks = build_imaging_map_blocks(summary)?;
    let current_status = if summary.status == "completed" {
        '+'
    } else {
        '?'
    };
    let current_pos = if summary.status == "completed" {
        0
    } else {
        summary.bytes_copied
    };
    let current_pass = u32::from(summary.retry_passes_completed.max(1));
    let mut mapfile = String::new();

    mapfile.push_str(&format!(
        "# Mapfile. Generated by {} {}\n",
        build_info.product_name, build_info.app_version
    ));
    mapfile.push_str("# ddrescue-style export generated from a Recupere imaging session.\n");
    mapfile.push_str(&format!("# Session ID: {}\n", summary.id));
    mapfile.push_str(&format!("# Device: {}\n", summary.device_name));
    append_imaging_source_provenance_mapfile(&mut mapfile, summary);
    mapfile.push_str(&format!(
        "# Generated: {}\n",
        Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
    ));
    mapfile.push_str(
        "# '+' = finished block, '-' = zero-filled unreadable block, '?' = not-yet-copied block.\n",
    );
    mapfile.push_str(
        "# Zero-filled unreadable blocks represent source bytes that were not reconstructed.\n",
    );
    mapfile.push_str("# current_pos  current_status  current_pass\n");
    mapfile.push_str(&format!(
        "{}     {}               {}\n",
        format_mapfile_hex(current_pos),
        current_status,
        current_pass
    ));
    mapfile.push_str("#      pos              size  status\n");
    for (start, length, status) in blocks {
        mapfile.push_str(&format!(
            "{}  {}  {}\n",
            format_mapfile_hex(start),
            format_mapfile_hex(length),
            status
        ));
    }

    Ok(mapfile)
}

fn build_imaging_session_report(
    summary: &ScanSessionSummary,
    logs: &[TechnicalLogEntry],
) -> String {
    let build_info = super::app_build_info();
    let mut report = String::new();

    report.push_str("=== RECUPERE IMAGING SESSION INCIDENT REPORT ===\n\n");
    report.push_str(&format!(
        "Generated: {}\n",
        Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
    ));
    report.push_str(&format!(
        "Product: {} {}\n",
        build_info.product_name, build_info.app_version
    ));
    report.push_str(&format!("Build profile: {}\n", build_info.build_profile));
    report.push_str(&format!("Runtime: {}\n\n", build_info.tauri_runtime));

    report.push_str("=== SESSION IDENTITY ===\n");
    report.push_str(&format!("Session ID: {}\n", summary.id));
    report.push_str(&format!("Device ID: {}\n", summary.device_id));
    report.push_str(&format!("Device Name: {}\n", summary.device_name));
    report.push_str(&format!("Scan Type: {}\n", summary.scan_type));
    report.push_str(&format!("Status: {}\n", summary.status));
    report.push_str(&format!(
        "Started: {}\n",
        format_timestamp_ms_rfc3339(summary.started_at_ms)
    ));
    report.push_str(&format!(
        "Completed: {}\n",
        summary
            .completed_at_ms
            .map(format_timestamp_ms_rfc3339)
            .unwrap_or_else(|| "in progress".into())
    ));
    report.push_str(&format!(
        "Duration: {} second(s)\n\n",
        summary.duration_seconds
    ));

    append_imaging_source_provenance_report(&mut report, summary);

    report.push_str("=== IMAGING SUMMARY ===\n");
    report.push_str(&format!("Imaging bytes copied: {}\n", summary.bytes_copied));
    report.push_str(&format!("Imaging total bytes: {}\n", summary.total_bytes));
    report.push_str(&format!(
        "Recovered files counted: {}\n",
        summary.files_recovered
    ));
    report.push_str(&format!("Files found: {}\n", summary.files_found));
    report.push_str(&format!("Recorded errors: {}\n", summary.errors));
    report.push_str(&format!(
        "Resumed from existing partial image: {}\n",
        summary.resume_from_bytes
    ));
    report.push_str(&format!(
        "Unreadable source segments: {}\n",
        summary.unreadable_ranges_count
    ));
    report.push_str(&format!(
        "Zero-filled unreadable bytes: {}\n",
        summary.unreadable_bytes
    ));
    report.push_str(&format!(
        "Targeted rescue passes completed: {}\n",
        summary.retry_passes_completed
    ));
    report.push_str(&format!(
        "Bytes recovered during targeted rescue: {}\n\n",
        summary.rescued_after_retry_bytes
    ));

    append_unreadable_range_sample_report(&mut report, summary);

    let (operator_summary, operator_next_step) = imaging_operator_summary(summary);
    report.push_str("=== OPERATOR HANDOFF ===\n");
    report.push_str(&format!(
        "Operator status: {}\n",
        imaging_operator_status(summary)
    ));
    report.push_str(&format!("Operator summary: {}\n", operator_summary));
    report.push_str(&format!("Safer next step: {}\n\n", operator_next_step));

    report.push_str("=== INTERPRETATION ===\n");
    if summary.resume_from_bytes > 0 {
        report.push_str(
            "- This session reused a coherent local partial image instead of restarting from zero.\n",
        );
    } else {
        report.push_str("- No partial-image resume was recorded for this session.\n");
    }

    if summary.unreadable_bytes > 0 {
        report.push_str(
            "- Unreadable source regions were neutralized as zero-filled gaps so the read-only imaging workflow could continue conservatively.\n",
        );
        report.push_str(
            "- These zero-filled bytes were not reconstructed and do not represent recovered source data.\n",
        );
    } else {
        report.push_str("- No unreadable source gaps were recorded for this session.\n");
    }

    if summary.retry_passes_completed > 0 {
        report.push_str(&format!(
            "- Targeted cautious rescue passes completed: {}. The pass sequence alternated traversal direction, trimmed range edges, probed central islands, zoomed around newly recovered pockets, split partial progress into finer local retries, prioritized smaller residual gaps, and micro-scraped with finer blocks where possible.\n",
            summary.retry_passes_completed
        ));
    }

    if summary.rescued_after_retry_bytes > 0 {
        report.push_str(&format!(
            "- {} byte(s) were recovered during targeted cautious rescue passes after the initial sweep.\n",
            summary.rescued_after_retry_bytes
        ));
    }
    report.push('\n');

    report.push_str("=== TECHNICAL LOGS ===\n");
    if logs.is_empty() {
        report.push_str("No technical log entry is available for this session.\n");
    } else {
        for entry in logs {
            report.push_str(&format!(
                "[{}] {} {}\n",
                format_timestamp_ms_rfc3339(entry.timestamp_ms),
                entry.level.to_uppercase(),
                entry.message
            ));
        }
    }

    report
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn generate_imaging_session_report(scan_id: String) -> Result<String, String> {
    let summary = load_scan_summary_for_report(&scan_id)
        .ok_or_else(|| format!("Scan session `{scan_id}` was not found."))?;

    if !session_uses_imaging(&summary) {
        return Err(format!(
            "Scan session `{scan_id}` does not expose an imaging session report."
        ));
    }

    let logs = get_scan_logs(scan_id)?;
    Ok(build_imaging_session_report(&summary, &logs))
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn generate_imaging_rescue_map(scan_id: String) -> Result<String, String> {
    let summary = load_scan_summary_for_report(&scan_id)
        .ok_or_else(|| format!("Scan session `{scan_id}` was not found."))?;

    if !session_uses_imaging(&summary) {
        return Err(format!(
            "Scan session `{scan_id}` does not expose an imaging rescue map."
        ));
    }

    build_imaging_rescue_map(&summary)
}

// ----------------------------------------------------------------------------
// Helpers migrated from `commands/mod.rs` (Sprint 2.1 slice `scan_helpers`,
// 2026-04-17). These are shared by the scan workers that still live in
// `mod.rs` until a later slice moves them too. Siblings reach them via
// `super::<fn>` thanks to the `pub(super) use scan::{...};` re-export in
// `mod.rs`.
// ----------------------------------------------------------------------------

pub(crate) fn register_scan_error(
    session: &Arc<Mutex<InventoryScanSession>>,
    started_at: &SystemTime,
    message: &str,
) {
    tracing::info!("scan warning: {message}");
    let mut state = crate::commands::state::lock_or_recover(session, "scan session");
    state.progress.errors_count = state.progress.errors_count.saturating_add(1);
    state.progress.elapsed_seconds = super::elapsed_seconds(started_at);
    if state.logs.len() >= MAX_SESSION_LOGS {
        state.logs.remove(0);
    }
    state.logs.push(TechnicalLogEntry {
        timestamp_ms: super::unix_timestamp_ms(),
        level: "warning".into(),
        message: message.to_string(),
    });
    drop(state);

    if let Err(error) = super::persist_scan_session(session) {
        tracing::info!("register_scan_error: unable to persist session snapshot: {error}");
    }
}

pub(crate) fn compute_progress(bytes_scanned: u64, total_bytes: u64, completed: bool) -> f32 {
    if completed {
        return 100.0;
    }

    if total_bytes == 0 {
        return 50.0;
    }

    let raw = (bytes_scanned as f64 / total_bytes as f64) * 100.0;
    raw.clamp(4.0, 99.0) as f32
}

pub(crate) fn display_parent_path(root: &Path, file_path: &Path) -> String {
    let parent = file_path.parent().unwrap_or(root);
    match parent.strip_prefix(root) {
        Ok(relative) if relative.as_os_str().is_empty() => "/".into(),
        Ok(relative) => format!("/{}", relative.to_string_lossy()),
        Err(_) => parent.to_string_lossy().to_string(),
    }
}

pub(crate) fn guess_mime_type(extension: &str) -> Option<String> {
    let mime = match extension {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "txt" | "md" | "log" => Some("text/plain"),
        "pdf" => Some("application/pdf"),
        "json" => Some("application/json"),
        "csv" => Some("text/csv"),
        "docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "xlsx" => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        "pptx" => Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        "mp4" => Some("video/mp4"),
        "mov" => Some("video/quicktime"),
        _ => None,
    };

    mime.map(str::to_string)
}

pub(crate) fn is_previewable_extension(extension: &str) -> bool {
    super::is_text_previewable_extension(extension)
        || super::is_document_previewable_extension(extension)
        || super::asset_preview_kind(extension).is_some()
}

pub(crate) fn run_deleted_fat32_scan(
    session_id: String,
    session: Arc<Mutex<InventoryScanSession>>,
    source_device_path: PathBuf,
    total_bytes: u64,
) {
    append_scan_log(
        &session,
        "info",
        format!(
            "Starting FAT32 deleted-file recovery from {}.",
            source_device_path.to_string_lossy()
        ),
    );

    update_progress(&session, |progress| {
        progress.status = "scanning".into();
        progress.stage = "reading-partition-table".into();
        progress.percent_complete = 2.0;
        progress.total_bytes = total_bytes.max(1);
    });
    append_scan_log(
        &session,
        "info",
        "Validating the FAT32 source path and preparing a local read-only image.".into(),
    );

    let started_at = SystemTime::now();

    update_progress(&session, |progress| {
        progress.stage = "analyzing-filesystem".into();
        progress.percent_complete = 4.0;
        progress.elapsed_seconds = 0;
    });
    append_scan_log(
        &session,
        "info",
        "Creating a local image snapshot before deleted-entry analysis.".into(),
    );

    let image_artifact = match create_read_only_image_with_optional_elevation(
        &session_id,
        &session,
        &source_device_path,
        imaging_profile_for_session(&session),
        &mut |copied_bytes| {
            let percent = if total_bytes == 0 {
                30.0
            } else {
                (copied_bytes as f64 / total_bytes as f64) * 45.0 + 4.0
            };
            update_progress(&session, |progress| {
                progress.bytes_scanned = copied_bytes;
                progress.percent_complete = percent.clamp(4.0, 55.0) as f32;
                progress.elapsed_seconds = elapsed_seconds(&started_at);
            });
        },
    ) {
        Ok(artifact) => artifact,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Local image created at {} ({} bytes).",
            image_artifact.path.to_string_lossy(),
            image_artifact.bytes_copied
        ),
    );
    append_imaging_artifact_issue_logs(&session, &image_artifact, "Local image snapshot");
    apply_imaging_artifact_session_details(&session, &image_artifact);

    update_progress(&session, |progress| {
        progress.stage = "scanning-deleted-entries".into();
        progress.bytes_scanned = image_artifact.bytes_copied;
        apply_imaging_artifact_issue_metrics(progress, &image_artifact);
        progress.percent_complete = 62.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let deleted_files = match fat32::recover_deleted_files(&image_artifact.path) {
        Ok(files) => files,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Detected {} deleted FAT32 entrie(s) that can be reconstructed conservatively.",
            deleted_files.len()
        ),
    );

    update_progress(&session, |progress| {
        progress.stage = "scoring-results".into();
        progress.percent_complete = 88.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let image_path = image_artifact.path.to_string_lossy().to_string();
    let results: Vec<RecoveredFile> = deleted_files
        .into_iter()
        .enumerate()
        .map(|(index, file)| RecoveredFile {
            id: format!("{session_id}-file-{}", index + 1),
            name: file.name,
            path: file.path,
            extension: file.extension.clone(),
            size_bytes: file.size_bytes,
            created_at: file.created_at,
            modified_at: file.modified_at,
            integrity: file.integrity,
            recovery_score: file.recovery_score,
            recovery_method: "reconstruction".into(),
            preview_available: is_previewable_extension(&file.extension),
            mime_type: guess_mime_type(&file.extension),
            expected_size_bytes: Some(file.expected_size_bytes),
            deleted_at: None,
            start_offset: Some(file.start_offset),
            clusters: Some(file.clusters),
            byte_runs: Some(file.byte_runs),
            resource_fork: None,
            alternate_data_streams: None,
            source_image_path: Some(image_path.clone()),
            is_deleted: true,
            source_view: Some("live-catalog".into()),
            ..Default::default()
        })
        .collect();

    let elapsed_seconds = elapsed_seconds(&started_at);
    let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
    state.results = results;
    state.progress.status = "completed".into();
    state.progress.stage = "finalizing".into();
    state.progress.elapsed_seconds = elapsed_seconds;
    state.progress.percent_complete = 100.0;
    state.progress.bytes_scanned = image_artifact.bytes_copied;
    state.progress.total_bytes = total_bytes.max(image_artifact.bytes_copied).max(1);
    state.progress.files_found = state.results.len() as u32;
    state.completed_at_ms = Some(unix_timestamp_ms());
    let completion_timestamp = state.completed_at_ms.unwrap_or_else(unix_timestamp_ms);
    let files_found = state.progress.files_found;
    if state.logs.len() >= MAX_SESSION_LOGS {
        state.logs.remove(0);
    }
    state.logs.push(TechnicalLogEntry {
        timestamp_ms: completion_timestamp,
        level: "info".into(),
        message: format!(
            "Deleted FAT32 recovery completed: {} file(s) reconstructed from the local image.",
            files_found
        ),
    });
    drop(state);

    if let Err(error) = persist_scan_session(&session) {
        tracing::info!("run_deleted_fat32_scan: unable to persist completed session: {error}");
    }
}

pub(crate) fn run_deleted_exfat_scan(
    session_id: String,
    session: Arc<Mutex<InventoryScanSession>>,
    source_device_path: PathBuf,
    total_bytes: u64,
) {
    append_scan_log(
        &session,
        "info",
        format!(
            "Starting exFAT deleted-file recovery from {}.",
            source_device_path.to_string_lossy()
        ),
    );

    update_progress(&session, |progress| {
        progress.status = "scanning".into();
        progress.stage = "reading-partition-table".into();
        progress.percent_complete = 2.0;
        progress.total_bytes = total_bytes.max(1);
    });
    append_scan_log(
        &session,
        "info",
        "Validating the exFAT source path and preparing a local read-only image.".into(),
    );

    let started_at = SystemTime::now();

    update_progress(&session, |progress| {
        progress.stage = "analyzing-filesystem".into();
        progress.percent_complete = 4.0;
        progress.elapsed_seconds = 0;
    });
    append_scan_log(
        &session,
        "info",
        "Creating a local image snapshot before deleted-entry analysis.".into(),
    );

    let image_artifact = match create_read_only_image_with_optional_elevation(
        &session_id,
        &session,
        &source_device_path,
        imaging_profile_for_session(&session),
        &mut |copied_bytes| {
            let percent = if total_bytes == 0 {
                30.0
            } else {
                (copied_bytes as f64 / total_bytes as f64) * 45.0 + 4.0
            };
            update_progress(&session, |progress| {
                progress.bytes_scanned = copied_bytes;
                progress.percent_complete = percent.clamp(4.0, 55.0) as f32;
                progress.elapsed_seconds = elapsed_seconds(&started_at);
            });
        },
    ) {
        Ok(artifact) => artifact,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Local image created at {} ({} bytes).",
            image_artifact.path.to_string_lossy(),
            image_artifact.bytes_copied
        ),
    );
    append_imaging_artifact_issue_logs(&session, &image_artifact, "Local image snapshot");
    apply_imaging_artifact_session_details(&session, &image_artifact);

    update_progress(&session, |progress| {
        progress.stage = "scanning-deleted-entries".into();
        progress.bytes_scanned = image_artifact.bytes_copied;
        apply_imaging_artifact_issue_metrics(progress, &image_artifact);
        progress.percent_complete = 62.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let deleted_files = match exfat::recover_deleted_files(&image_artifact.path) {
        Ok(files) => files,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Detected {} deleted exFAT entrie(s) that can be reconstructed conservatively.",
            deleted_files.len()
        ),
    );

    update_progress(&session, |progress| {
        progress.stage = "scoring-results".into();
        progress.percent_complete = 88.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let image_path = image_artifact.path.to_string_lossy().to_string();
    let results: Vec<RecoveredFile> = deleted_files
        .into_iter()
        .enumerate()
        .map(|(index, file)| RecoveredFile {
            id: format!("{session_id}-file-{}", index + 1),
            name: file.name,
            path: file.path,
            extension: file.extension.clone(),
            size_bytes: file.size_bytes,
            created_at: file.created_at,
            modified_at: file.modified_at,
            integrity: file.integrity,
            recovery_score: file.recovery_score,
            recovery_method: "reconstruction".into(),
            preview_available: is_previewable_extension(&file.extension),
            mime_type: guess_mime_type(&file.extension),
            expected_size_bytes: Some(file.expected_size_bytes),
            deleted_at: None,
            start_offset: Some(file.start_offset),
            clusters: Some(file.clusters),
            byte_runs: Some(file.byte_runs),
            resource_fork: None,
            alternate_data_streams: None,
            source_image_path: Some(image_path.clone()),
            is_deleted: true,
            source_view: Some("recovery-image".into()),
            ..Default::default()
        })
        .collect();

    let elapsed_seconds = elapsed_seconds(&started_at);
    let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
    state.results = results;
    state.progress.status = "completed".into();
    state.progress.stage = "finalizing".into();
    state.progress.elapsed_seconds = elapsed_seconds;
    state.progress.percent_complete = 100.0;
    state.progress.bytes_scanned = image_artifact.bytes_copied;
    state.progress.total_bytes = total_bytes.max(image_artifact.bytes_copied).max(1);
    state.progress.files_found = state.results.len() as u32;
    state.completed_at_ms = Some(unix_timestamp_ms());
    let completion_timestamp = state.completed_at_ms.unwrap_or_else(unix_timestamp_ms);
    let files_found = state.progress.files_found;
    if state.logs.len() >= MAX_SESSION_LOGS {
        state.logs.remove(0);
    }
    state.logs.push(TechnicalLogEntry {
        timestamp_ms: completion_timestamp,
        level: "info".into(),
        message: format!(
            "Deleted exFAT recovery completed: {} file(s) reconstructed from the local image.",
            files_found
        ),
    });
    drop(state);

    if let Err(error) = persist_scan_session(&session) {
        tracing::info!("run_deleted_exfat_scan: unable to persist completed session: {error}");
    }
}

pub(crate) fn run_deleted_ntfs_scan(
    session_id: String,
    session: Arc<Mutex<InventoryScanSession>>,
    source_device_path: PathBuf,
    total_bytes: u64,
) {
    append_scan_log(
        &session,
        "info",
        format!(
            "Starting NTFS deleted-file recovery from {}.",
            source_device_path.to_string_lossy()
        ),
    );

    update_progress(&session, |progress| {
        progress.status = "scanning".into();
        progress.stage = "reading-partition-table".into();
        progress.percent_complete = 2.0;
        progress.total_bytes = total_bytes.max(1);
    });
    append_scan_log(
        &session,
        "info",
        "Validating the NTFS source path and preparing a local read-only image.".into(),
    );

    let started_at = SystemTime::now();

    update_progress(&session, |progress| {
        progress.stage = "analyzing-filesystem".into();
        progress.percent_complete = 4.0;
        progress.elapsed_seconds = 0;
    });
    append_scan_log(
        &session,
        "info",
        "Creating a local image snapshot before deleted-entry analysis.".into(),
    );

    let image_artifact = match create_read_only_image_with_optional_elevation(
        &session_id,
        &session,
        &source_device_path,
        imaging_profile_for_session(&session),
        &mut |copied_bytes| {
            let percent = if total_bytes == 0 {
                30.0
            } else {
                (copied_bytes as f64 / total_bytes as f64) * 45.0 + 4.0
            };
            update_progress(&session, |progress| {
                progress.bytes_scanned = copied_bytes;
                progress.percent_complete = percent.clamp(4.0, 55.0) as f32;
                progress.elapsed_seconds = elapsed_seconds(&started_at);
            });
        },
    ) {
        Ok(artifact) => artifact,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Local image created at {} ({} bytes).",
            image_artifact.path.to_string_lossy(),
            image_artifact.bytes_copied
        ),
    );
    append_imaging_artifact_issue_logs(&session, &image_artifact, "Local image snapshot");
    apply_imaging_artifact_session_details(&session, &image_artifact);

    update_progress(&session, |progress| {
        progress.stage = "scanning-deleted-entries".into();
        progress.bytes_scanned = image_artifact.bytes_copied;
        apply_imaging_artifact_issue_metrics(progress, &image_artifact);
        progress.percent_complete = 62.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let deleted_files = match ntfs::recover_deleted_files(&image_artifact.path) {
        Ok(files) => files,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    let journal_files = match ntfs::recover_usn_journal_files(&image_artifact.path) {
        Ok(files) => files,
        Err(error) => {
            append_scan_log(
                &session,
                "warn",
                format!("USN journal replay failed (non-fatal): {error}"),
            );
            Vec::new()
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Detected {} deleted NTFS entrie(s) whose MFT metadata still exposes reconstructible data (plus {} from USN journal).",
            deleted_files.len(),
            journal_files.len()
        ),
    );

    update_progress(&session, |progress| {
        progress.stage = "scoring-results".into();
        progress.percent_complete = 88.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let image_path = image_artifact.path.to_string_lossy().to_string();
    let deleted_count = deleted_files.len();
    let mut results: Vec<RecoveredFile> = deleted_files
        .into_iter()
        .enumerate()
        .map(|(index, file)| {
            let compression_kind = file.compression_kind;
            let recovery_complexity = compression_kind.as_ref().map(|_| "medium".to_string());
            let validator_status = compression_kind.as_ref().map(|_| "validated".to_string());

            RecoveredFile {
                id: format!("{session_id}-file-{}", index + 1),
                name: file.name,
                path: file.path,
                extension: file.extension.clone(),
                size_bytes: file.size_bytes,
                created_at: file.created_at,
                modified_at: file.modified_at,
                integrity: file.integrity,
                recovery_score: file.recovery_score,
                recovery_method: "reconstruction".into(),
                preview_available: is_previewable_extension(&file.extension),
                mime_type: guess_mime_type(&file.extension),
                expected_size_bytes: Some(file.expected_size_bytes),
                deleted_at: None,
                start_offset: Some(file.start_offset),
                clusters: Some(file.clusters),
                byte_runs: Some(file.byte_runs),
                resource_fork: None,
                alternate_data_streams: Some(file.alternate_data_streams),
                source_image_path: Some(image_path.clone()),
                is_deleted: true,
                compression_kind,
                source_view: Some("recovery-image".into()),
                native_auxiliary_kind: Some("ads".into()),
                recovery_complexity,
                validator_status,
                ..Default::default()
            }
        })
        .collect();

    let journal_results: Vec<RecoveredFile> = journal_files
        .into_iter()
        .enumerate()
        .map(|(index, file)| {
            let compression_kind = file.compression_kind;
            let recovery_complexity = compression_kind.as_ref().map(|_| "medium".to_string());
            let validator_status = compression_kind.as_ref().map(|_| "validated".to_string());

            RecoveredFile {
                id: format!("{session_id}-file-{}", deleted_count + index + 1),
                name: file.name,
                path: file.path,
                extension: file.extension.clone(),
                size_bytes: file.size_bytes,
                created_at: file.created_at,
                modified_at: file.modified_at,
                integrity: file.integrity,
                recovery_score: file.recovery_score,
                recovery_method: "journal-replay".into(),
                preview_available: is_previewable_extension(&file.extension),
                mime_type: guess_mime_type(&file.extension),
                expected_size_bytes: Some(file.expected_size_bytes),
                deleted_at: None,
                start_offset: Some(file.start_offset),
                clusters: Some(file.clusters),
                byte_runs: Some(file.byte_runs),
                resource_fork: None,
                alternate_data_streams: Some(file.alternate_data_streams),
                source_image_path: Some(image_path.clone()),
                is_deleted: true,
                journal_derived: true,
                compression_kind,
                source_view: Some("journal".into()),
                native_auxiliary_kind: Some("ads".into()),
                recovery_complexity,
                validator_status,
                ..Default::default()
            }
        })
        .collect();

    results.extend(journal_results);

    // Attempt corrupted filesystem fallback if primary recovery found few results
    if deleted_count < 3 {
        append_scan_log(
            &session,
            "info",
            "Primary NTFS recovery found few results. Attempting MFT mirror fallback.".into(),
        );
        match crate::fallback::recover_ntfs_from_mft_mirror(&image_artifact.path) {
            Ok(fallback_candidates) => {
                let fallback_count = fallback_candidates.len();
                if fallback_count > 0 {
                    append_scan_log(
                        &session,
                        "info",
                        format!(
                            "Fallback recovery found {} additional candidate(s) from MFT mirror.",
                            fallback_count
                        ),
                    );
                    let base_index = results.len();
                    let fallback_results: Vec<RecoveredFile> = fallback_candidates
                        .into_iter()
                        .enumerate()
                        .map(|(index, candidate)| RecoveredFile {
                            id: format!(
                                "{session_id}-file-{}",
                                deleted_count + base_index + index + 1
                            ),
                            name: candidate.name,
                            path: candidate.path,
                            extension: candidate.extension.clone(),
                            size_bytes: candidate.size_bytes,
                            created_at: None,
                            modified_at: None,
                            integrity: candidate.integrity,
                            recovery_score: candidate.recovery_score,
                            recovery_method: candidate.recovery_method,
                            preview_available: is_previewable_extension(&candidate.extension),
                            mime_type: guess_mime_type(&candidate.extension),
                            expected_size_bytes: Some(candidate.size_bytes),
                            deleted_at: None,
                            start_offset: Some(candidate.start_offset),
                            clusters: None,
                            byte_runs: Some(candidate.byte_runs),
                            resource_fork: None,
                            alternate_data_streams: None,
                            source_image_path: Some(image_path.clone()),
                            is_deleted: true,
                            source_view: Some("fallback".into()),
                            native_auxiliary_kind: Some("ads".into()),
                            ..Default::default()
                        })
                        .collect();
                    results.extend(fallback_results);
                }
            }
            Err(e) => {
                append_scan_log(
                    &session,
                    "warning",
                    format!("Fallback recovery attempt failed: {e}"),
                );
            }
        }
    }

    // Correlate recovery sources for deduplication and promotion
    let filesystem_files: Vec<RecoveredFile> = results
        .iter()
        .filter(|f| f.recovery_method == "reconstruction")
        .cloned()
        .collect();
    let journal_corr_files: Vec<RecoveredFile> = results
        .iter()
        .filter(|f| f.journal_derived)
        .cloned()
        .collect();
    let other_files: Vec<RecoveredFile> = results
        .iter()
        .filter(|f| f.recovery_method != "reconstruction" && !f.journal_derived)
        .cloned()
        .collect();

    let correlation = crate::correlation::correlate_recovery_sources(
        filesystem_files,
        journal_corr_files,
        other_files,
    );

    append_scan_log(&session, "info", format!(
        "Correlation engine: {} files from {} sources, {} duplicates removed, {} promoted by journal confirmation.",
        correlation.files.len(),
        correlation.filesystem_count + correlation.journal_count + correlation.carved_count,
        correlation.deduplicated_count,
        correlation.promoted_count
    ));

    let results = correlation.files;

    let elapsed_seconds = elapsed_seconds(&started_at);
    let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
    state.results = results;
    state.progress.status = "completed".into();
    state.progress.stage = "finalizing".into();
    state.progress.elapsed_seconds = elapsed_seconds;
    state.progress.percent_complete = 100.0;
    state.progress.bytes_scanned = image_artifact.bytes_copied;
    state.progress.total_bytes = total_bytes.max(image_artifact.bytes_copied).max(1);
    state.progress.files_found = state.results.len() as u32;
    state.completed_at_ms = Some(unix_timestamp_ms());
    let completion_timestamp = state.completed_at_ms.unwrap_or_else(unix_timestamp_ms);
    let files_found = state.progress.files_found;
    if state.logs.len() >= MAX_SESSION_LOGS {
        state.logs.remove(0);
    }
    state.logs.push(TechnicalLogEntry {
        timestamp_ms: completion_timestamp,
        level: "info".into(),
        message: format!(
            "Deleted NTFS recovery completed: {} file(s) reconstructed from the local image.",
            files_found
        ),
    });
    drop(state);

    if let Err(error) = persist_scan_session(&session) {
        tracing::info!("run_deleted_ntfs_scan: unable to persist completed session: {error}");
    }
}

pub(crate) fn run_deleted_ext4_scan(
    session_id: String,
    session: Arc<Mutex<InventoryScanSession>>,
    source_device_path: PathBuf,
    total_bytes: u64,
) {
    append_scan_log(
        &session,
        "info",
        format!(
            "Starting ext4 deleted-file recovery from {}.",
            source_device_path.to_string_lossy()
        ),
    );

    update_progress(&session, |progress| {
        progress.status = "scanning".into();
        progress.stage = "reading-partition-table".into();
        progress.percent_complete = 2.0;
        progress.total_bytes = total_bytes.max(1);
    });
    append_scan_log(
        &session,
        "info",
        "Validating the ext4 source path and preparing a local read-only image.".into(),
    );

    let started_at = SystemTime::now();

    update_progress(&session, |progress| {
        progress.stage = "analyzing-filesystem".into();
        progress.percent_complete = 4.0;
        progress.elapsed_seconds = 0;
    });
    append_scan_log(
        &session,
        "info",
        "Creating a local image snapshot before conservative orphaned-inode analysis.".into(),
    );

    let image_artifact = match create_read_only_image_with_optional_elevation(
        &session_id,
        &session,
        &source_device_path,
        imaging_profile_for_session(&session),
        &mut |copied_bytes| {
            let percent = if total_bytes == 0 {
                30.0
            } else {
                (copied_bytes as f64 / total_bytes as f64) * 45.0 + 4.0
            };
            update_progress(&session, |progress| {
                progress.bytes_scanned = copied_bytes;
                progress.percent_complete = percent.clamp(4.0, 55.0) as f32;
                progress.elapsed_seconds = elapsed_seconds(&started_at);
            });
        },
    ) {
        Ok(artifact) => artifact,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Local image created at {} ({} bytes).",
            image_artifact.path.to_string_lossy(),
            image_artifact.bytes_copied
        ),
    );
    append_imaging_artifact_issue_logs(&session, &image_artifact, "Local image snapshot");
    apply_imaging_artifact_session_details(&session, &image_artifact);

    update_progress(&session, |progress| {
        progress.stage = "scanning-deleted-entries".into();
        progress.bytes_scanned = image_artifact.bytes_copied;
        apply_imaging_artifact_issue_metrics(progress, &image_artifact);
        progress.percent_complete = 62.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let deleted_files = match ext4::recover_deleted_files(&image_artifact.path) {
        Ok(files) => files,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    let journal_files = match ext4::recover_journal_files(&image_artifact.path) {
        Ok(files) => files,
        Err(error) => {
            append_scan_log(
                &session,
                "warn",
                format!("ext4 journal replay failed (non-fatal): {error}"),
            );
            Vec::new()
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Detected {} deleted ext4 inode candidate(s) with reconstructible free blocks (plus {} from journal).",
            deleted_files.len(),
            journal_files.len()
        ),
    );

    update_progress(&session, |progress| {
        progress.stage = "scoring-results".into();
        progress.percent_complete = 88.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let image_path = image_artifact.path.to_string_lossy().to_string();
    let deleted_count = deleted_files.len();
    let mut results: Vec<RecoveredFile> = deleted_files
        .into_iter()
        .enumerate()
        .map(|(index, file)| RecoveredFile {
            id: format!("{session_id}-file-{}", index + 1),
            name: file.name,
            path: file.path,
            extension: file.extension.clone(),
            size_bytes: file.size_bytes,
            created_at: None,
            modified_at: file.modified_at,
            integrity: file.integrity,
            recovery_score: file.recovery_score,
            recovery_method: "reconstruction".into(),
            preview_available: is_previewable_extension(&file.extension),
            mime_type: guess_mime_type(&file.extension),
            expected_size_bytes: Some(file.expected_size_bytes),
            deleted_at: file.deleted_at,
            start_offset: Some(file.start_offset),
            clusters: Some(file.clusters),
            byte_runs: Some(file.byte_runs),
            resource_fork: None,
            alternate_data_streams: None,
            source_image_path: Some(image_path.clone()),
            is_deleted: true,
            source_view: Some("recovery-image".into()),
            ..Default::default()
        })
        .collect();

    let journal_results: Vec<RecoveredFile> = journal_files
        .into_iter()
        .enumerate()
        .map(|(index, file)| RecoveredFile {
            id: format!("{session_id}-file-{}", deleted_count + index + 1),
            name: file.name,
            path: file.path,
            extension: file.extension.clone(),
            size_bytes: file.size_bytes,
            created_at: None,
            modified_at: file.modified_at,
            integrity: file.integrity,
            recovery_score: file.recovery_score,
            recovery_method: "journal-replay".into(),
            preview_available: is_previewable_extension(&file.extension),
            mime_type: guess_mime_type(&file.extension),
            expected_size_bytes: Some(file.expected_size_bytes),
            deleted_at: file.deleted_at,
            start_offset: Some(file.start_offset),
            clusters: Some(file.clusters),
            byte_runs: Some(file.byte_runs),
            resource_fork: None,
            alternate_data_streams: None,
            source_image_path: Some(image_path.clone()),
            is_deleted: true,
            journal_derived: true,
            source_view: Some("journal".into()),
            ..Default::default()
        })
        .collect();

    results.extend(journal_results);

    // Attempt corrupted filesystem fallback if primary recovery found few results
    if deleted_count < 3 {
        append_scan_log(
            &session,
            "info",
            "Primary ext4 recovery found few results. Attempting backup superblock fallback."
                .into(),
        );
        match crate::fallback::recover_ext4_from_backup_superblock(&image_artifact.path) {
            Ok(fallback_candidates) => {
                let fallback_count = fallback_candidates.len();
                if fallback_count > 0 {
                    append_scan_log(&session, "info", format!("Fallback recovery found {} additional candidate(s) from backup superblocks.", fallback_count));
                    let base_index = results.len();
                    let fallback_results: Vec<RecoveredFile> = fallback_candidates
                        .into_iter()
                        .enumerate()
                        .map(|(index, candidate)| RecoveredFile {
                            id: format!(
                                "{session_id}-file-{}",
                                deleted_count + base_index + index + 1
                            ),
                            name: candidate.name,
                            path: candidate.path,
                            extension: candidate.extension.clone(),
                            size_bytes: candidate.size_bytes,
                            created_at: None,
                            modified_at: None,
                            integrity: candidate.integrity,
                            recovery_score: candidate.recovery_score,
                            recovery_method: candidate.recovery_method,
                            preview_available: is_previewable_extension(&candidate.extension),
                            mime_type: guess_mime_type(&candidate.extension),
                            expected_size_bytes: Some(candidate.size_bytes),
                            deleted_at: None,
                            start_offset: Some(candidate.start_offset),
                            clusters: None,
                            byte_runs: Some(candidate.byte_runs),
                            resource_fork: None,
                            alternate_data_streams: None,
                            source_image_path: Some(image_path.clone()),
                            is_deleted: true,
                            source_view: Some("fallback".into()),
                            ..Default::default()
                        })
                        .collect();
                    results.extend(fallback_results);
                }
            }
            Err(e) => {
                append_scan_log(
                    &session,
                    "warning",
                    format!("Fallback recovery attempt failed: {e}"),
                );
            }
        }
    }

    // Correlate recovery sources for deduplication and promotion
    let filesystem_files: Vec<RecoveredFile> = results
        .iter()
        .filter(|f| f.recovery_method == "reconstruction")
        .cloned()
        .collect();
    let journal_corr_files: Vec<RecoveredFile> = results
        .iter()
        .filter(|f| f.journal_derived)
        .cloned()
        .collect();
    let other_files: Vec<RecoveredFile> = results
        .iter()
        .filter(|f| f.recovery_method != "reconstruction" && !f.journal_derived)
        .cloned()
        .collect();

    let correlation = crate::correlation::correlate_recovery_sources(
        filesystem_files,
        journal_corr_files,
        other_files,
    );

    append_scan_log(&session, "info", format!(
        "Correlation engine: {} files from {} sources, {} duplicates removed, {} promoted by journal confirmation.",
        correlation.files.len(),
        correlation.filesystem_count + correlation.journal_count + correlation.carved_count,
        correlation.deduplicated_count,
        correlation.promoted_count
    ));

    let results = correlation.files;

    let elapsed_seconds = elapsed_seconds(&started_at);
    let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
    state.results = results;
    state.progress.status = "completed".into();
    state.progress.stage = "finalizing".into();
    state.progress.elapsed_seconds = elapsed_seconds;
    state.progress.percent_complete = 100.0;
    state.progress.bytes_scanned = image_artifact.bytes_copied;
    state.progress.total_bytes = total_bytes.max(image_artifact.bytes_copied).max(1);
    state.progress.files_found = state.results.len() as u32;
    state.completed_at_ms = Some(unix_timestamp_ms());
    let completion_timestamp = state.completed_at_ms.unwrap_or_else(unix_timestamp_ms);
    let files_found = state.progress.files_found;
    if state.logs.len() >= MAX_SESSION_LOGS {
        state.logs.remove(0);
    }
    state.logs.push(TechnicalLogEntry {
        timestamp_ms: completion_timestamp,
        level: "info".into(),
        message: format!(
            "Deleted ext4 recovery completed: {} orphaned inode file(s) reconstructed from the local image.",
            files_found
        ),
    });
    drop(state);

    if let Err(error) = persist_scan_session(&session) {
        tracing::info!("run_deleted_ext4_scan: unable to persist completed session: {error}");
    }
}

pub(crate) fn run_deleted_hfsplus_scan(
    session_id: String,
    session: Arc<Mutex<InventoryScanSession>>,
    source_device_path: PathBuf,
    total_bytes: u64,
) {
    append_scan_log(
        &session,
        "info",
        format!(
            "Starting HFS+ deleted-file recovery from {}.",
            source_device_path.to_string_lossy()
        ),
    );

    update_progress(&session, |progress| {
        progress.status = "scanning".into();
        progress.stage = "reading-partition-table".into();
        progress.percent_complete = 2.0;
        progress.total_bytes = total_bytes.max(1);
    });
    append_scan_log(
        &session,
        "info",
        "Validating the HFS+ source path and preparing a local read-only image.".into(),
    );

    let started_at = SystemTime::now();

    update_progress(&session, |progress| {
        progress.stage = "analyzing-filesystem".into();
        progress.percent_complete = 4.0;
        progress.elapsed_seconds = 0;
    });
    append_scan_log(
        &session,
        "info",
        "Creating a local image snapshot before conservative HFS+ catalog-slack analysis.".into(),
    );

    let image_artifact = match create_read_only_image_with_optional_elevation(
        &session_id,
        &session,
        &source_device_path,
        imaging_profile_for_session(&session),
        &mut |copied_bytes| {
            let percent = if total_bytes == 0 {
                30.0
            } else {
                (copied_bytes as f64 / total_bytes as f64) * 45.0 + 4.0
            };
            update_progress(&session, |progress| {
                progress.bytes_scanned = copied_bytes;
                progress.percent_complete = percent.clamp(4.0, 55.0) as f32;
                progress.elapsed_seconds = elapsed_seconds(&started_at);
            });
        },
    ) {
        Ok(artifact) => artifact,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Local image created at {} ({} bytes).",
            image_artifact.path.to_string_lossy(),
            image_artifact.bytes_copied
        ),
    );
    append_imaging_artifact_issue_logs(&session, &image_artifact, "Local image snapshot");
    apply_imaging_artifact_session_details(&session, &image_artifact);

    update_progress(&session, |progress| {
        progress.stage = "scanning-deleted-entries".into();
        progress.bytes_scanned = image_artifact.bytes_copied;
        apply_imaging_artifact_issue_metrics(progress, &image_artifact);
        progress.percent_complete = 62.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let deleted_files = match hfsplus::recover_deleted_files(&image_artifact.path) {
        Ok(files) => files,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    let journal_files = match hfsplus::recover_journal_files(&image_artifact.path) {
        Ok(files) => files,
        Err(error) => {
            append_scan_log(
                &session,
                "warn",
                format!("HFS+ journal replay failed (non-fatal): {error}"),
            );
            Vec::new()
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Detected {} deleted HFS+ catalog record candidate(s) with reconstructible fork bytes (plus {} from journal).",
            deleted_files.len(),
            journal_files.len()
        ),
    );

    update_progress(&session, |progress| {
        progress.stage = "scoring-results".into();
        progress.percent_complete = 88.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let image_path = image_artifact.path.to_string_lossy().to_string();
    let deleted_count = deleted_files.len();
    let mut results: Vec<RecoveredFile> = deleted_files
        .into_iter()
        .enumerate()
        .map(|(index, file)| RecoveredFile {
            id: format!("{session_id}-file-{}", index + 1),
            name: file.name,
            path: file.path,
            extension: file.extension.clone(),
            size_bytes: file.size_bytes,
            created_at: file.created_at,
            modified_at: file.modified_at,
            integrity: file.integrity,
            recovery_score: file.recovery_score,
            recovery_method: "reconstruction".into(),
            preview_available: is_previewable_extension(&file.extension),
            mime_type: guess_mime_type(&file.extension),
            expected_size_bytes: file.expected_size_bytes,
            deleted_at: None,
            start_offset: file.start_offset,
            clusters: Some(Vec::new()),
            byte_runs: Some(file.byte_runs),
            resource_fork: file.resource_fork.clone(),
            alternate_data_streams: None,
            source_image_path: Some(image_path.clone()),
            is_deleted: true,
            source_view: Some("recovery-image".into()),
            native_auxiliary_kind: if file.resource_fork.is_some() {
                Some("resource-fork".into())
            } else {
                None
            },
            ..Default::default()
        })
        .collect();

    let journal_results: Vec<RecoveredFile> = journal_files
        .into_iter()
        .enumerate()
        .map(|(index, file)| RecoveredFile {
            id: format!("{session_id}-file-{}", deleted_count + index + 1),
            name: file.name,
            path: file.path,
            extension: file.extension.clone(),
            size_bytes: file.size_bytes,
            created_at: file.created_at,
            modified_at: file.modified_at,
            integrity: file.integrity,
            recovery_score: file.recovery_score,
            recovery_method: "journal-replay".into(),
            preview_available: is_previewable_extension(&file.extension),
            mime_type: guess_mime_type(&file.extension),
            expected_size_bytes: file.expected_size_bytes,
            deleted_at: None,
            start_offset: file.start_offset,
            clusters: Some(Vec::new()),
            byte_runs: Some(file.byte_runs),
            resource_fork: file.resource_fork.clone(),
            alternate_data_streams: None,
            source_image_path: Some(image_path.clone()),
            is_deleted: true,
            journal_derived: true,
            source_view: Some("journal".into()),
            native_auxiliary_kind: if file.resource_fork.is_some() {
                Some("resource-fork".into())
            } else {
                None
            },
            ..Default::default()
        })
        .collect();

    results.extend(journal_results);

    let elapsed_seconds = elapsed_seconds(&started_at);
    let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
    state.results = results;
    state.progress.status = "completed".into();
    state.progress.stage = "finalizing".into();
    state.progress.elapsed_seconds = elapsed_seconds;
    state.progress.percent_complete = 100.0;
    state.progress.bytes_scanned = image_artifact.bytes_copied;
    state.progress.total_bytes = total_bytes.max(image_artifact.bytes_copied).max(1);
    state.progress.files_found = state.results.len() as u32;
    state.completed_at_ms = Some(unix_timestamp_ms());
    let completion_timestamp = state.completed_at_ms.unwrap_or_else(unix_timestamp_ms);
    let files_found = state.progress.files_found;
    if state.logs.len() >= MAX_SESSION_LOGS {
        state.logs.remove(0);
    }
    state.logs.push(TechnicalLogEntry {
        timestamp_ms: completion_timestamp,
        level: "info".into(),
        message: format!(
            "Deleted HFS+ recovery completed: {} catalog record file(s) reconstructed from the local image.",
            files_found
        ),
    });
    drop(state);

    if let Err(error) = persist_scan_session(&session) {
        tracing::info!("run_deleted_hfsplus_scan: unable to persist completed session: {error}");
    }
}

pub(crate) fn run_deleted_apfs_scan(
    session_id: String,
    session: Arc<Mutex<InventoryScanSession>>,
    source_device_path: PathBuf,
    total_bytes: u64,
) {
    append_scan_log(
        &session,
        "info",
        format!(
            "Starting APFS deleted-file recovery from {}.",
            source_device_path.to_string_lossy()
        ),
    );

    update_progress(&session, |progress| {
        progress.status = "scanning".into();
        progress.stage = "reading-partition-table".into();
        progress.percent_complete = 2.0;
        progress.total_bytes = total_bytes.max(1);
    });
    append_scan_log(
        &session,
        "info",
        "Validating the APFS source path and preparing a local read-only image.".into(),
    );

    let started_at = SystemTime::now();

    update_progress(&session, |progress| {
        progress.stage = "analyzing-filesystem".into();
        progress.percent_complete = 4.0;
        progress.elapsed_seconds = 0;
    });
    append_scan_log(
        &session,
        "info",
        "Creating a local image snapshot before conservative APFS orphan-catalog analysis.".into(),
    );

    let image_artifact = match create_read_only_image_with_optional_elevation(
        &session_id,
        &session,
        &source_device_path,
        imaging_profile_for_session(&session),
        &mut |copied_bytes| {
            let percent = if total_bytes == 0 {
                30.0
            } else {
                (copied_bytes as f64 / total_bytes as f64) * 45.0 + 4.0
            };
            update_progress(&session, |progress| {
                progress.bytes_scanned = copied_bytes;
                progress.percent_complete = percent.clamp(4.0, 55.0) as f32;
                progress.elapsed_seconds = elapsed_seconds(&started_at);
            });
        },
    ) {
        Ok(artifact) => artifact,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Local image created at {} ({} bytes).",
            image_artifact.path.to_string_lossy(),
            image_artifact.bytes_copied
        ),
    );
    append_imaging_artifact_issue_logs(&session, &image_artifact, "Local image snapshot");
    apply_imaging_artifact_session_details(&session, &image_artifact);

    update_progress(&session, |progress| {
        progress.stage = "scanning-deleted-entries".into();
        progress.bytes_scanned = image_artifact.bytes_copied;
        apply_imaging_artifact_issue_metrics(progress, &image_artifact);
        progress.percent_complete = 62.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let deleted_files = match apfs::recover_deleted_files(&image_artifact.path) {
        Ok(files) => files,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Detected {} deleted APFS orphan candidate(s) with reconstructible extents from the current catalog state.",
            deleted_files.len()
        ),
    );

    update_progress(&session, |progress| {
        progress.stage = "scoring-results".into();
        progress.percent_complete = 88.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let image_path = image_artifact.path.to_string_lossy().to_string();
    let results: Vec<RecoveredFile> = deleted_files
        .into_iter()
        .enumerate()
        .map(|(index, file)| {
            let mut recovered = RecoveredFile {
                id: format!("{session_id}-file-{}", index + 1),
                name: file.name,
                path: file.path,
                extension: file.extension.clone(),
                size_bytes: file.size_bytes,
                created_at: file.created_at,
                modified_at: file.modified_at,
                integrity: file.integrity,
                recovery_score: file.recovery_score,
                recovery_method: "reconstruction".into(),
                preview_available: is_previewable_extension(&file.extension),
                mime_type: guess_mime_type(&file.extension),
                expected_size_bytes: file.expected_size_bytes,
                deleted_at: None,
                start_offset: file.start_offset,
                clusters: Some(Vec::new()),
                byte_runs: Some(file.byte_runs),
                resource_fork: None,
                alternate_data_streams: None,
                source_image_path: Some(image_path.clone()),
                is_deleted: true,
                source_view: Some("live-catalog".into()),
                ..Default::default()
            };
            apply_apfs_catalog_reconstruction_evidence(&mut recovered);
            recovered
        })
        .collect();

    let elapsed_seconds = elapsed_seconds(&started_at);
    let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
    state.results = results;
    state.progress.status = "completed".into();
    state.progress.stage = "finalizing".into();
    state.progress.elapsed_seconds = elapsed_seconds;
    state.progress.percent_complete = 100.0;
    state.progress.bytes_scanned = image_artifact.bytes_copied;
    state.progress.total_bytes = total_bytes.max(image_artifact.bytes_copied).max(1);
    state.progress.files_found = state.results.len() as u32;
    state.completed_at_ms = Some(unix_timestamp_ms());
    let completion_timestamp = state.completed_at_ms.unwrap_or_else(unix_timestamp_ms);
    let files_found = state.progress.files_found;
    if state.logs.len() >= MAX_SESSION_LOGS {
        state.logs.remove(0);
    }
    state.logs.push(TechnicalLogEntry {
        timestamp_ms: completion_timestamp,
        level: "info".into(),
        message: format!(
            "Deleted APFS recovery completed: {} orphaned catalog file(s) reconstructed from the local image.",
            files_found
        ),
    });
    drop(state);

    if let Err(error) = persist_scan_session(&session) {
        tracing::info!("run_deleted_apfs_scan: unable to persist completed session: {error}");
    }
}

fn potential_volume_source_snapshot_path(session_id: &str) -> PathBuf {
    imaging::workspace_image_path_for_scan(&format!("{session_id}-source"))
}

fn potential_volume_slice_path(session_id: &str) -> PathBuf {
    imaging::workspace_image_path_for_scan(&format!("{session_id}-slice"))
}

fn potential_volume_slice_length(
    source_image_path: &Path,
    volume: &PotentialVolume,
) -> Result<u64, String> {
    if let Some(size_bytes) = volume.size_bytes {
        return Ok(size_bytes.max(1));
    }

    let source_length = fs::metadata(source_image_path)
        .map_err(|error| format!("Unable to inspect the temporary source snapshot: {error}"))?
        .len();

    if volume.start_offset >= source_length {
        return Err(format!(
            "Potential volume `{}` starts beyond the temporary source snapshot bounds.",
            volume.label
        ));
    }

    Ok(source_length.saturating_sub(volume.start_offset))
}

fn rebase_slice_offset(volume_start_offset: u64, local_start_offset: Option<u64>) -> Option<u64> {
    local_start_offset.map(|offset| volume_start_offset.saturating_add(offset))
}

#[allow(clippy::too_many_arguments)]
fn recovered_file_from_slice(
    session_id: &str,
    index: usize,
    volume_start_offset: u64,
    slice_image_path: &str,
    name: String,
    path: String,
    extension: String,
    size_bytes: u64,
    created_at: Option<String>,
    modified_at: Option<String>,
    integrity: String,
    recovery_score: u8,
    recovery_method: &str,
    expected_size_bytes: Option<u64>,
    start_offset: Option<u64>,
    clusters: Vec<u32>,
    byte_runs: Vec<ByteRun>,
    is_deleted: bool,
    resource_fork: Option<FileFork>,
    alternate_data_streams: Option<Vec<NamedFileFork>>,
) -> RecoveredFile {
    let compression_kind = byte_runs
        .iter()
        .find_map(|run| run.compression_kind.clone());
    let validator_status = compression_kind.as_ref().map(|_| "validated".to_string());
    let recovery_complexity = compression_kind.as_ref().map(|_| "medium".to_string());
    let native_auxiliary_kind = match (
        resource_fork.is_some(),
        alternate_data_streams
            .as_ref()
            .is_some_and(|streams| !streams.is_empty()),
    ) {
        (true, true) => Some("mixed".into()),
        (true, false) => Some("resource-fork".into()),
        (false, true) => Some("ads".into()),
        (false, false) => None,
    };

    RecoveredFile {
        id: format!("{session_id}-file-{}", index + 1),
        name,
        path,
        extension: extension.clone(),
        size_bytes,
        created_at,
        modified_at,
        integrity,
        recovery_score,
        recovery_method: recovery_method.into(),
        preview_available: is_previewable_extension(&extension),
        mime_type: guess_mime_type(&extension),
        expected_size_bytes,
        deleted_at: None,
        start_offset: rebase_slice_offset(volume_start_offset, start_offset),
        clusters: Some(clusters),
        byte_runs: Some(byte_runs),
        resource_fork,
        alternate_data_streams,
        source_image_path: Some(slice_image_path.into()),
        is_deleted,
        compression_kind,
        source_view: Some("recovery-image".into()),
        native_auxiliary_kind,
        recovery_complexity,
        validator_status,
        ..Default::default()
    }
}

fn apply_apfs_catalog_reconstruction_evidence(recovered: &mut RecoveredFile) {
    let assembly_segment_count = recovered
        .byte_runs
        .as_ref()
        .map(|runs| runs.len().min(u8::MAX as usize) as u8)
        .filter(|count| *count > 0)
        .unwrap_or(1);
    let gap_count = 0_u8;
    let catalog_timestamps_complete =
        recovered.created_at.is_some() && recovered.modified_at.is_some();
    let validator_status = if recovered.integrity == "partial"
        || recovered.expected_size_bytes != Some(recovered.size_bytes)
    {
        "partial-unvalidated"
    } else if matches!(recovered.integrity.as_str(), "intact" | "fragmented")
        && !recovered.extension.trim().is_empty()
        && recovered.extension != "bin"
        && catalog_timestamps_complete
    {
        // This is still not a structural validator. It only means the
        // current APFS catalog still maps the full payload, the preview
        // bytes matched a known file signature, and the catalog still
        // exposes coherent timestamps for that orphaned inode.
        "reassembled"
    } else {
        "unsupported"
    };

    recovered.validator_status = Some(validator_status.into());
    recovered.assembly_segment_count = Some(assembly_segment_count);
    recovered.gap_count = Some(gap_count);
    recovered.recovery_complexity = Some(
        crate::scoring::classify_recovery_complexity(
            &recovered.integrity,
            assembly_segment_count,
            gap_count,
            validator_status,
        )
        .into(),
    );
}

pub(crate) fn run_potential_volume_scan(
    session_id: String,
    session: Arc<Mutex<InventoryScanSession>>,
    source_plan: ImagingSourcePlan,
    volume: PotentialVolume,
    total_bytes: u64,
) {
    let source_device_path = source_plan.source_path().to_path_buf();
    let volume_label = volume.label.clone();
    let volume_fs_label = filesystem_label(&volume.filesystem);

    append_scan_log(
        &session,
        "info",
        format!(
            "Starting recovered-volume analysis for {} ({}) from {}.",
            volume_label,
            volume_fs_label,
            source_device_path.to_string_lossy()
        ),
    );

    update_progress(&session, |progress| {
        progress.status = "scanning".into();
        progress.stage = "reading-partition-table".into();
        progress.percent_complete = 2.0;
        progress.total_bytes = total_bytes.max(1);
    });
    append_scan_log(
        &session,
        "info",
        format!(
            "Preparing potential volume {} at 0x{:X} for conservative read-only analysis.",
            volume_label, volume.start_offset
        ),
    );

    let started_at = SystemTime::now();
    let source_snapshot_path = potential_volume_source_snapshot_path(&session_id);
    update_progress(&session, |progress| {
        progress.stage = "creating-image".into();
        progress.percent_complete = 4.0;
        progress.elapsed_seconds = 0;
    });
    append_scan_log(
        &session,
        "info",
        "Creating a local read-only source snapshot before extracting the recovered volume slice."
            .into(),
    );

    let source_image_artifact = match imaging_cmd::create_local_image_snapshot(
        &session_id,
        &session,
        source_plan,
        total_bytes,
        &source_snapshot_path,
        &started_at,
    ) {
        Ok(artifact) => artifact,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Local source snapshot created at {} ({} bytes).",
            source_image_artifact.path.to_string_lossy(),
            source_image_artifact.bytes_copied
        ),
    );
    append_imaging_artifact_issue_logs(&session, &source_image_artifact, "Local source snapshot");
    apply_imaging_artifact_session_details(&session, &source_image_artifact);
    if source_image_artifact.resume_from_bytes > 0 {
        append_scan_log(
            &session,
            "info",
            format!(
                "The local source snapshot resumed from {} bytes already present in the partial image.",
                source_image_artifact.resume_from_bytes
            ),
        );
    }

    let slice_length = match potential_volume_slice_length(&source_image_artifact.path, &volume) {
        Ok(length) => length,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };
    let slice_path = potential_volume_slice_path(&session_id);

    update_progress(&session, |progress| {
        progress.stage = "analyzing-filesystem".into();
        progress.bytes_scanned = source_image_artifact.bytes_copied;
        progress.resume_from_bytes = source_image_artifact.resume_from_bytes;
        apply_imaging_artifact_issue_metrics(progress, &source_image_artifact);
        progress.percent_complete = 58.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });
    append_scan_log(
        &session,
        "info",
        format!(
            "Extracting the recovered volume slice locally (offset 0x{:X}, {} bytes).",
            volume.start_offset, slice_length
        ),
    );

    let slice_artifact = match imaging::create_read_only_image_slice_at_controlled(
        &slice_path,
        &source_image_artifact.path,
        volume.start_offset,
        Some(slice_length),
        &mut |copied_bytes| {
            wait_for_scan_permission(&session)?;
            let percent = if slice_length == 0 {
                76.0
            } else {
                (copied_bytes as f64 / slice_length as f64) * 18.0 + 58.0
            };
            update_progress(&session, |progress| {
                progress.bytes_scanned = source_image_artifact
                    .bytes_copied
                    .saturating_add(copied_bytes);
                progress.total_bytes = total_bytes
                    .max(
                        source_image_artifact
                            .bytes_copied
                            .saturating_add(slice_length),
                    )
                    .max(1);
                progress.percent_complete = percent.clamp(58.0, 76.0) as f32;
                progress.elapsed_seconds = elapsed_seconds(&started_at);
            });
            Ok(())
        },
    ) {
        Ok(artifact) => artifact,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Recovered volume slice created at {} ({} bytes).",
            slice_artifact.path.to_string_lossy(),
            slice_artifact.bytes_copied
        ),
    );

    let slice_image_path = slice_artifact.path.to_string_lossy().to_string();
    update_progress(&session, |progress| {
        progress.stage = "analyzing-filesystem".into();
        progress.bytes_scanned = source_image_artifact
            .bytes_copied
            .saturating_add(slice_artifact.bytes_copied);
        progress.percent_complete = 78.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });
    append_scan_log(
        &session,
        "info",
        format!(
            "Cataloging currently visible {} files from the recovered local slice.",
            volume_fs_label
        ),
    );

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let (visible_results, deleted_results) = match &volume.filesystem {
        FilesystemType::Fat32 => {
            let visible_files = match fat32::list_visible_files(&slice_artifact.path) {
                Ok(files) => files,
                Err(error) => {
                    fail_scan_session(&session, error);
                    return;
                }
            };
            let visible_results = visible_files
                .into_iter()
                .enumerate()
                .map(|(index, file)| {
                    recovered_file_from_slice(
                        &session_id,
                        index,
                        volume.start_offset,
                        &slice_image_path,
                        file.name,
                        file.path,
                        file.extension,
                        file.size_bytes,
                        file.created_at,
                        file.modified_at,
                        file.integrity,
                        file.recovery_score,
                        "filesystem",
                        Some(file.size_bytes),
                        file.start_offset,
                        file.clusters,
                        file.byte_runs,
                        false,
                        None,
                        None,
                    )
                })
                .collect::<Vec<_>>();
            update_progress(&session, |progress| {
                progress.stage = "scanning-deleted-entries".into();
                progress.percent_complete = 86.0;
                progress.elapsed_seconds = elapsed_seconds(&started_at);
            });
            append_scan_log(
                &session,
                "info",
                format!(
                    "Scanning deleted {} entries from the recovered local slice.",
                    volume_fs_label
                ),
            );
            if wait_for_scan_permission(&session).is_err() {
                finalize_cancelled_scan(&session);
                return;
            }
            let deleted_files = match fat32::recover_deleted_files(&slice_artifact.path) {
                Ok(files) => files,
                Err(error) => {
                    fail_scan_session(&session, error);
                    return;
                }
            };
            let deleted_results = deleted_files
                .into_iter()
                .enumerate()
                .map(|(index, file)| {
                    let mut recovered = recovered_file_from_slice(
                        &session_id,
                        visible_results.len() + index,
                        volume.start_offset,
                        &slice_image_path,
                        file.name,
                        file.path,
                        file.extension,
                        file.size_bytes,
                        file.created_at,
                        file.modified_at,
                        file.integrity,
                        file.recovery_score,
                        "reconstruction",
                        Some(file.expected_size_bytes),
                        Some(file.start_offset),
                        file.clusters,
                        file.byte_runs,
                        true,
                        None,
                        None,
                    );
                    recovered.source_view = Some("live-catalog".into());
                    apply_apfs_catalog_reconstruction_evidence(&mut recovered);
                    recovered
                })
                .collect::<Vec<_>>();
            (visible_results, deleted_results)
        }
        FilesystemType::Exfat => {
            let visible_files = match exfat::list_visible_files(&slice_artifact.path) {
                Ok(files) => files,
                Err(error) => {
                    fail_scan_session(&session, error);
                    return;
                }
            };
            let visible_results = visible_files
                .into_iter()
                .enumerate()
                .map(|(index, file)| {
                    recovered_file_from_slice(
                        &session_id,
                        index,
                        volume.start_offset,
                        &slice_image_path,
                        file.name,
                        file.path,
                        file.extension,
                        file.size_bytes,
                        file.created_at,
                        file.modified_at,
                        file.integrity,
                        file.recovery_score,
                        "filesystem",
                        Some(file.size_bytes),
                        file.start_offset,
                        file.clusters,
                        file.byte_runs,
                        false,
                        None,
                        None,
                    )
                })
                .collect::<Vec<_>>();
            update_progress(&session, |progress| {
                progress.stage = "scanning-deleted-entries".into();
                progress.percent_complete = 86.0;
                progress.elapsed_seconds = elapsed_seconds(&started_at);
            });
            append_scan_log(
                &session,
                "info",
                format!(
                    "Scanning deleted {} entries from the recovered local slice.",
                    volume_fs_label
                ),
            );
            if wait_for_scan_permission(&session).is_err() {
                finalize_cancelled_scan(&session);
                return;
            }
            let deleted_files = match exfat::recover_deleted_files(&slice_artifact.path) {
                Ok(files) => files,
                Err(error) => {
                    fail_scan_session(&session, error);
                    return;
                }
            };
            let deleted_results = deleted_files
                .into_iter()
                .enumerate()
                .map(|(index, file)| {
                    recovered_file_from_slice(
                        &session_id,
                        visible_results.len() + index,
                        volume.start_offset,
                        &slice_image_path,
                        file.name,
                        file.path,
                        file.extension,
                        file.size_bytes,
                        file.created_at,
                        file.modified_at,
                        file.integrity,
                        file.recovery_score,
                        "reconstruction",
                        Some(file.expected_size_bytes),
                        Some(file.start_offset),
                        file.clusters,
                        file.byte_runs,
                        true,
                        None,
                        None,
                    )
                })
                .collect::<Vec<_>>();
            (visible_results, deleted_results)
        }
        FilesystemType::Ntfs => {
            let visible_files = match ntfs::list_visible_files(&slice_artifact.path) {
                Ok(files) => files,
                Err(error) => {
                    fail_scan_session(&session, error);
                    return;
                }
            };
            let visible_results = visible_files
                .into_iter()
                .enumerate()
                .map(|(index, file)| {
                    let compression_kind = file.compression_kind.clone();
                    let mut recovered = recovered_file_from_slice(
                        &session_id,
                        index,
                        volume.start_offset,
                        &slice_image_path,
                        file.name,
                        file.path,
                        file.extension,
                        file.size_bytes,
                        file.created_at,
                        file.modified_at,
                        file.integrity,
                        file.recovery_score,
                        "filesystem",
                        Some(file.size_bytes),
                        file.start_offset,
                        file.clusters,
                        file.byte_runs,
                        false,
                        None,
                        Some(file.alternate_data_streams),
                    );
                    if compression_kind.is_some() {
                        recovered.compression_kind = compression_kind;
                        recovered.recovery_complexity = Some("medium".into());
                        recovered.validator_status = Some("validated".into());
                    }
                    recovered
                })
                .collect::<Vec<_>>();
            update_progress(&session, |progress| {
                progress.stage = "scanning-deleted-entries".into();
                progress.percent_complete = 86.0;
                progress.elapsed_seconds = elapsed_seconds(&started_at);
            });
            append_scan_log(
                &session,
                "info",
                format!(
                    "Scanning deleted {} entries from the recovered local slice.",
                    volume_fs_label
                ),
            );
            if wait_for_scan_permission(&session).is_err() {
                finalize_cancelled_scan(&session);
                return;
            }
            let deleted_files = match ntfs::recover_deleted_files(&slice_artifact.path) {
                Ok(files) => files,
                Err(error) => {
                    fail_scan_session(&session, error);
                    return;
                }
            };
            let deleted_results = deleted_files
                .into_iter()
                .enumerate()
                .map(|(index, file)| {
                    let compression_kind = file.compression_kind.clone();
                    let mut recovered = recovered_file_from_slice(
                        &session_id,
                        visible_results.len() + index,
                        volume.start_offset,
                        &slice_image_path,
                        file.name,
                        file.path,
                        file.extension,
                        file.size_bytes,
                        file.created_at,
                        file.modified_at,
                        file.integrity,
                        file.recovery_score,
                        "reconstruction",
                        Some(file.expected_size_bytes),
                        Some(file.start_offset),
                        file.clusters,
                        file.byte_runs,
                        true,
                        None,
                        Some(file.alternate_data_streams),
                    );
                    if compression_kind.is_some() {
                        recovered.compression_kind = compression_kind;
                        recovered.recovery_complexity = Some("medium".into());
                        recovered.validator_status = Some("validated".into());
                    }
                    recovered
                })
                .collect::<Vec<_>>();
            (visible_results, deleted_results)
        }
        FilesystemType::HfsPlus => {
            let visible_files = match hfsplus::list_visible_files(&slice_artifact.path) {
                Ok(files) => files,
                Err(error) => {
                    fail_scan_session(&session, error);
                    return;
                }
            };
            let visible_results = visible_files
                .into_iter()
                .enumerate()
                .map(|(index, file)| {
                    recovered_file_from_slice(
                        &session_id,
                        index,
                        volume.start_offset,
                        &slice_image_path,
                        file.name,
                        file.path,
                        file.extension,
                        file.size_bytes,
                        file.created_at,
                        file.modified_at,
                        file.integrity,
                        file.recovery_score,
                        "filesystem",
                        file.expected_size_bytes,
                        file.start_offset,
                        Vec::new(),
                        file.byte_runs,
                        false,
                        file.resource_fork,
                        None,
                    )
                })
                .collect::<Vec<_>>();
            update_progress(&session, |progress| {
                progress.stage = "scanning-deleted-entries".into();
                progress.percent_complete = 86.0;
                progress.elapsed_seconds = elapsed_seconds(&started_at);
            });
            append_scan_log(
                &session,
                "info",
                format!(
                    "Scanning deleted {} catalog records from the recovered local slice.",
                    volume_fs_label
                ),
            );
            if wait_for_scan_permission(&session).is_err() {
                finalize_cancelled_scan(&session);
                return;
            }
            let deleted_files = match hfsplus::recover_deleted_files(&slice_artifact.path) {
                Ok(files) => files,
                Err(error) => {
                    fail_scan_session(&session, error);
                    return;
                }
            };
            let deleted_results = deleted_files
                .into_iter()
                .enumerate()
                .map(|(index, file)| {
                    recovered_file_from_slice(
                        &session_id,
                        visible_results.len() + index,
                        volume.start_offset,
                        &slice_image_path,
                        file.name,
                        file.path,
                        file.extension,
                        file.size_bytes,
                        file.created_at,
                        file.modified_at,
                        file.integrity,
                        file.recovery_score,
                        "reconstruction",
                        file.expected_size_bytes,
                        file.start_offset,
                        Vec::new(),
                        file.byte_runs,
                        true,
                        file.resource_fork,
                        None,
                    )
                })
                .collect::<Vec<_>>();
            (visible_results, deleted_results)
        }
        FilesystemType::Apfs => {
            let visible_files = match apfs::list_visible_files(&slice_artifact.path) {
                Ok(files) => files,
                Err(error) => {
                    fail_scan_session(&session, error);
                    return;
                }
            };
            let visible_results = visible_files
                .into_iter()
                .enumerate()
                .map(|(index, file)| {
                    recovered_file_from_slice(
                        &session_id,
                        index,
                        volume.start_offset,
                        &slice_image_path,
                        file.name,
                        file.path,
                        file.extension,
                        file.size_bytes,
                        file.created_at,
                        file.modified_at,
                        file.integrity,
                        file.recovery_score,
                        "filesystem",
                        file.expected_size_bytes,
                        file.start_offset,
                        Vec::new(),
                        file.byte_runs,
                        false,
                        None,
                        None,
                    )
                })
                .collect::<Vec<_>>();
            update_progress(&session, |progress| {
                progress.stage = "scanning-deleted-entries".into();
                progress.percent_complete = 86.0;
                progress.elapsed_seconds = elapsed_seconds(&started_at);
            });
            append_scan_log(
                &session,
                "info",
                format!(
                    "Scanning deleted {} orphaned catalog inodes from the recovered local slice.",
                    volume_fs_label
                ),
            );
            if wait_for_scan_permission(&session).is_err() {
                finalize_cancelled_scan(&session);
                return;
            }
            let deleted_files = match apfs::recover_deleted_files(&slice_artifact.path) {
                Ok(files) => files,
                Err(error) => {
                    fail_scan_session(&session, error);
                    return;
                }
            };
            let deleted_results = deleted_files
                .into_iter()
                .enumerate()
                .map(|(index, file)| {
                    recovered_file_from_slice(
                        &session_id,
                        visible_results.len() + index,
                        volume.start_offset,
                        &slice_image_path,
                        file.name,
                        file.path,
                        file.extension,
                        file.size_bytes,
                        file.created_at,
                        file.modified_at,
                        file.integrity,
                        file.recovery_score,
                        "reconstruction",
                        file.expected_size_bytes,
                        file.start_offset,
                        Vec::new(),
                        file.byte_runs,
                        true,
                        None,
                        None,
                    )
                })
                .collect::<Vec<_>>();
            (visible_results, deleted_results)
        }
        _ => {
            fail_scan_session(
                &session,
                format!(
                    "Potential volume `{}` uses `{}` which is not yet analyzable in this MVP.",
                    volume.label, volume_fs_label
                ),
            );
            return;
        }
    };
    let visible_count = visible_results.len();
    let deleted_count = deleted_results.len();
    let results = visible_results
        .into_iter()
        .chain(deleted_results)
        .collect::<Vec<_>>();

    append_scan_log(
        &session,
        "info",
        format!(
            "Recovered-volume analysis found {} visible and {} deleted {} file candidate(s).",
            visible_count, deleted_count, volume_fs_label
        ),
    );

    update_progress(&session, |progress| {
        progress.stage = "scoring-results".into();
        progress.percent_complete = 92.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let elapsed_seconds = elapsed_seconds(&started_at);
    let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
    state.results = results;
    state.progress.status = "completed".into();
    state.progress.stage = "finalizing".into();
    state.progress.elapsed_seconds = elapsed_seconds;
    state.progress.percent_complete = 100.0;
    state.progress.bytes_scanned = source_image_artifact
        .bytes_copied
        .saturating_add(slice_artifact.bytes_copied);
    state.progress.total_bytes = total_bytes
        .max(
            source_image_artifact
                .bytes_copied
                .saturating_add(slice_artifact.bytes_copied),
        )
        .max(1);
    state.progress.files_found = state.results.len() as u32;
    state.completed_at_ms = Some(unix_timestamp_ms());
    let completion_timestamp = state.completed_at_ms.unwrap_or_else(unix_timestamp_ms);
    let files_found = state.progress.files_found;
    if state.logs.len() >= MAX_SESSION_LOGS {
        state.logs.remove(0);
    }
    state.logs.push(TechnicalLogEntry {
        timestamp_ms: completion_timestamp,
        level: "info".into(),
        message: format!(
            "Recovered-volume analysis completed: {} {} file(s) cataloged or reconstructed from the local slice image.",
            files_found,
            volume_fs_label
        ),
    });
    drop(state);

    if let Err(error) = persist_scan_session(&session) {
        tracing::info!("run_potential_volume_scan: unable to persist completed session: {error}");
    }
}

pub(crate) fn run_signature_carving_scan(
    session_id: String,
    session: Arc<Mutex<InventoryScanSession>>,
    source_plan: ImagingSourcePlan,
    total_bytes: u64,
) {
    let source_device_path = source_plan.source_path().to_path_buf();

    append_scan_log(
        &session,
        "info",
        format!(
            "Starting signature carving from {}.",
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
        "Creating a local read-only image snapshot before signature carving.".into(),
    );

    let started_at = SystemTime::now();
    let image_destination = imaging::workspace_image_path_for_scan(&session_id);
    let image_artifact = match imaging_cmd::create_local_image_snapshot(
        &session_id,
        &session,
        source_plan,
        total_bytes,
        &image_destination,
        &started_at,
    ) {
        Ok(artifact) => artifact,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Local image created at {} ({} bytes).",
            image_artifact.path.to_string_lossy(),
            image_artifact.bytes_copied
        ),
    );
    append_imaging_artifact_issue_logs(&session, &image_artifact, "Local image snapshot");
    apply_imaging_artifact_session_details(&session, &image_artifact);
    if image_artifact.resume_from_bytes > 0 {
        append_scan_log(
            &session,
            "info",
            format!(
                "The local image snapshot resumed from {} bytes already present in the partial image.",
                image_artifact.resume_from_bytes
            ),
        );
    }

    update_progress(&session, |progress| {
        progress.stage = "carving-signatures".into();
        progress.bytes_scanned = image_artifact.bytes_copied;
        progress.resume_from_bytes = image_artifact.resume_from_bytes;
        apply_imaging_artifact_issue_metrics(progress, &image_artifact);
        progress.percent_complete = 60.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });
    append_scan_log(
        &session,
        "info",
        "Searching the local image for known JPEG, PNG, PDF, and ZIP signatures.".into(),
    );

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let carved_files = match carving::carve_signatures(&image_artifact.path) {
        Ok(files) => files,
        Err(error) => {
            fail_scan_session(&session, error);
            return;
        }
    };

    append_scan_log(
        &session,
        "info",
        format!(
            "Detected {} carved file candidate(s) from the local image.",
            carved_files.len()
        ),
    );

    update_progress(&session, |progress| {
        progress.stage = "scoring-results".into();
        progress.percent_complete = 88.0;
        progress.elapsed_seconds = elapsed_seconds(&started_at);
    });

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let image_path = image_artifact.path.to_string_lossy().to_string();
    let results: Vec<RecoveredFile> = carved_files
        .into_iter()
        .enumerate()
        .map(|(index, file)| RecoveredFile {
            id: format!("{session_id}-file-{}", index + 1),
            name: file.name,
            path: "/carved".into(),
            extension: file.extension.clone(),
            size_bytes: file.size_bytes,
            created_at: None,
            modified_at: None,
            integrity: file.integrity,
            recovery_score: file.recovery_score,
            recovery_method: "carving".into(),
            preview_available: is_previewable_extension(&file.extension),
            mime_type: guess_mime_type(&file.extension),
            expected_size_bytes: None,
            deleted_at: None,
            start_offset: Some(file.start_offset),
            clusters: None,
            byte_runs: Some(file.byte_runs),
            resource_fork: None,
            alternate_data_streams: None,
            source_image_path: Some(image_path.clone()),
            is_deleted: true,
            source_view: Some("recovery-image".into()),
            recovery_complexity: Some(file.recovery_complexity),
            validator_status: Some(file.validator_status),
            assembly_segment_count: Some(file.assembly_segment_count),
            gap_count: Some(file.gap_count),
            ..Default::default()
        })
        .collect();

    let elapsed_seconds = elapsed_seconds(&started_at);
    let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
    state.results = results;
    state.progress.status = "completed".into();
    state.progress.stage = "finalizing".into();
    state.progress.elapsed_seconds = elapsed_seconds;
    state.progress.percent_complete = 100.0;
    state.progress.bytes_scanned = image_artifact.bytes_copied;
    state.progress.total_bytes = total_bytes.max(image_artifact.bytes_copied).max(1);
    state.progress.files_found = state.results.len() as u32;
    state.completed_at_ms = Some(unix_timestamp_ms());
    let completion_timestamp = state.completed_at_ms.unwrap_or_else(unix_timestamp_ms);
    let files_found = state.progress.files_found;
    if state.logs.len() >= MAX_SESSION_LOGS {
        state.logs.remove(0);
    }
    state.logs.push(TechnicalLogEntry {
        timestamp_ms: completion_timestamp,
        level: "info".into(),
        message: format!(
            "Signature carving completed: {} file(s) reconstructed from the local image.",
            files_found
        ),
    });
    drop(state);

    if let Err(error) = persist_scan_session(&session) {
        tracing::info!("run_signature_carving_scan: unable to persist completed session: {error}");
    }
}

pub(crate) fn run_inventory_scan(
    session_id: String,
    session: Arc<Mutex<InventoryScanSession>>,
    root_path: PathBuf,
    scan_type: &'static str,
) {
    append_scan_log(
        &session,
        "info",
        format!("Starting {scan_type} read-only catalog scan."),
    );

    update_progress(&session, |progress| {
        progress.status = "scanning".into();
        progress.stage = "reading-partition-table".into();
        progress.percent_complete = 2.0;
    });
    append_scan_log(
        &session,
        "info",
        "Reading partition metadata and validating the mounted source path.".into(),
    );

    let started_at = SystemTime::now();
    let depth_limit = if scan_type == "quick" {
        Some(QUICK_SCAN_MAX_DEPTH)
    } else {
        None
    };

    update_progress(&session, |progress| {
        progress.stage = "analyzing-filesystem".into();
        progress.percent_complete = 4.0;
        progress.elapsed_seconds = 0;
    });
    append_scan_log(
        &session,
        "info",
        if depth_limit.is_some() {
            format!(
                "Quick scan depth limit enabled at {} directory levels.",
                QUICK_SCAN_MAX_DEPTH
            )
        } else {
            "Deep scan will traverse all accessible directories on the mounted volume.".into()
        },
    );

    let mut stack = vec![(root_path.clone(), 0_usize)];
    let mut next_file_index: usize = 1;

    while let Some((directory, depth)) = stack.pop() {
        if wait_for_scan_permission(&session).is_err() {
            finalize_cancelled_scan(&session);
            return;
        }

        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                register_scan_error(
                    &session,
                    &started_at,
                    &format!(
                        "Unable to read directory {}: {}",
                        directory.to_string_lossy(),
                        error
                    ),
                );
                continue;
            }
        };

        for entry in entries {
            if wait_for_scan_permission(&session).is_err() {
                finalize_cancelled_scan(&session);
                return;
            }

            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    register_scan_error(
                        &session,
                        &started_at,
                        &format!("Directory entry read failed: {error}"),
                    );
                    continue;
                }
            };

            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    register_scan_error(
                        &session,
                        &started_at,
                        &format!(
                            "Unable to read metadata for {}: {}",
                            path.to_string_lossy(),
                            error
                        ),
                    );
                    continue;
                }
            };

            if metadata.file_type().is_symlink() {
                continue;
            }

            if metadata.is_dir() {
                if depth_limit.is_none_or(|limit| depth < limit) {
                    stack.push((path, depth + 1));
                }
                continue;
            }

            if !metadata.is_file() {
                continue;
            }

            let file_name = entry.file_name().to_string_lossy().to_string();
            let extension = path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_lowercase())
                .unwrap_or_default();
            let parent_display_path = display_parent_path(&root_path, &path);
            let file_size = metadata.len();
            let file_id = format!("{session_id}-file-{next_file_index}");
            next_file_index += 1;

            let recovered_file = RecoveredFile {
                id: file_id,
                name: file_name,
                path: parent_display_path,
                extension: extension.clone(),
                size_bytes: file_size,
                created_at: None,
                modified_at: None,
                integrity: "intact".into(),
                recovery_score: 100,
                recovery_method: "filesystem".into(),
                preview_available: is_previewable_extension(&extension),
                mime_type: guess_mime_type(&extension),
                expected_size_bytes: Some(file_size),
                deleted_at: None,
                start_offset: None,
                clusters: None,
                byte_runs: None,
                resource_fork: None,
                alternate_data_streams: None,
                source_image_path: None,
                is_deleted: false,
                source_view: Some("mounted-volume".into()),
                recovery_complexity: Some("low".into()),
                validator_status: Some("validated".into()),
                ..Default::default()
            };

            let elapsed_seconds = elapsed_seconds(&started_at);
            let mut checkpoint: Option<u32> = None;
            let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
            state.results.push(recovered_file);
            state.progress.files_found = state.results.len() as u32;
            state.progress.bytes_scanned = state.progress.bytes_scanned.saturating_add(file_size);
            state.progress.elapsed_seconds = elapsed_seconds;
            state.progress.percent_complete = compute_progress(
                state.progress.bytes_scanned,
                state.progress.total_bytes,
                false,
            );
            if state.progress.files_found.is_multiple_of(100) {
                checkpoint = Some(state.progress.files_found);
            }
            drop(state);

            if let Some(files_found) = checkpoint {
                append_scan_log(
                    &session,
                    "info",
                    format!("Catalog checkpoint reached: {files_found} files indexed."),
                );
            }
        }
    }

    if wait_for_scan_permission(&session).is_err() {
        finalize_cancelled_scan(&session);
        return;
    }

    let elapsed_seconds = elapsed_seconds(&started_at);
    let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
    state.progress.status = "completed".into();
    state.progress.stage = "finalizing".into();
    state.progress.elapsed_seconds = elapsed_seconds;
    state.progress.percent_complete = 100.0;
    state.completed_at_ms = Some(unix_timestamp_ms());
    let completion_timestamp = state.completed_at_ms.unwrap_or_else(unix_timestamp_ms);
    let files_found = state.progress.files_found;
    let errors_count = state.progress.errors_count;
    if state.logs.len() >= MAX_SESSION_LOGS {
        state.logs.remove(0);
    }
    state.logs.push(TechnicalLogEntry {
        timestamp_ms: completion_timestamp,
        level: "info".into(),
        message: format!(
            "Scan completed: {} files cataloged with {} access warnings.",
            files_found, errors_count
        ),
    });
    drop(state);

    if let Err(error) = persist_scan_session(&session) {
        tracing::info!("run_inventory_scan: unable to persist completed session: {error}");
    }

    let state = crate::commands::state::lock_or_recover(&session, "scan session");

    tracing::info!(
        "scan complete: session={} device={} root={} scan_type={} files_found={} errors={}",
        session_id,
        state.device_name,
        state.root_path,
        state.scan_type,
        state.progress.files_found,
        state.progress.errors_count
    );
}

fn supported_deleted_recovery_filesystem(filesystem: &FilesystemType) -> bool {
    matches!(
        filesystem,
        FilesystemType::Fat32
            | FilesystemType::Exfat
            | FilesystemType::Ntfs
            | FilesystemType::Ext4
            | FilesystemType::HfsPlus
            | FilesystemType::Apfs
    )
}

pub(crate) fn supported_potential_volume_filesystem(filesystem: &FilesystemType) -> bool {
    supported_deleted_recovery_filesystem(filesystem)
        || matches!(filesystem, FilesystemType::HfsPlus | FilesystemType::Apfs)
}

fn potential_volume_detection_rank(detection_method: &str) -> u8 {
    match detection_method {
        "gpt" => 3,
        "mbr" => 2,
        "gpt-backup" => 1,
        _ => 0,
    }
}

pub(crate) fn best_supported_potential_volume(
    potential_volumes: &[PotentialVolume],
) -> Option<&PotentialVolume> {
    potential_volumes
        .iter()
        .filter(|volume| supported_potential_volume_filesystem(&volume.filesystem))
        .max_by(|left, right| {
            left.confidence_score
                .cmp(&right.confidence_score)
                .then_with(|| {
                    potential_volume_detection_rank(&left.detection_method)
                        .cmp(&potential_volume_detection_rank(&right.detection_method))
                })
                .then_with(|| right.start_offset.cmp(&left.start_offset))
        })
}

pub(crate) fn guided_supported_potential_volume_candidate(
    potential_volumes: &[PotentialVolume],
) -> Option<&PotentialVolume> {
    let mut supported = potential_volumes
        .iter()
        .filter(|volume| supported_potential_volume_filesystem(&volume.filesystem))
        .collect::<Vec<_>>();

    supported.sort_by(|left, right| {
        right
            .confidence_score
            .cmp(&left.confidence_score)
            .then_with(|| {
                potential_volume_detection_rank(&right.detection_method)
                    .cmp(&potential_volume_detection_rank(&left.detection_method))
            })
            .then_with(|| left.start_offset.cmp(&right.start_offset))
    });

    let best = supported.first().copied()?;
    let second = supported.get(1).copied();
    if second.is_none() {
        return Some(best);
    }

    let second = second.expect("second candidate should exist when not early-returned");
    let confidence_gap = best
        .confidence_score
        .saturating_sub(second.confidence_score);
    let method_gap = potential_volume_detection_rank(&best.detection_method)
        .saturating_sub(potential_volume_detection_rank(&second.detection_method));

    let strong_confidence_winner = best.confidence_score >= 88 && confidence_gap >= 8;
    let very_strong_winner = best.confidence_score >= 94 && confidence_gap >= 6;
    let strong_partition_table_winner =
        best.confidence_score >= 90 && confidence_gap >= 4 && method_gap >= 2;

    if strong_confidence_winner || very_strong_winner || strong_partition_table_winner {
        Some(best)
    } else {
        None
    }
}

#[cfg(test)]
mod scan_tests {
    use super::apply_apfs_catalog_reconstruction_evidence;
    use crate::types::RecoveredFile;

    #[test]
    fn apply_apfs_catalog_reconstruction_evidence_marks_catalog_results_conservatively() {
        let mut recovered = RecoveredFile {
            integrity: "intact".into(),
            size_bytes: 128,
            expected_size_bytes: Some(128),
            ..Default::default()
        };

        apply_apfs_catalog_reconstruction_evidence(&mut recovered);

        assert_eq!(recovered.validator_status.as_deref(), Some("unsupported"));
        assert_eq!(recovered.recovery_complexity.as_deref(), Some("low"));
        assert_eq!(recovered.assembly_segment_count, Some(1));
        assert_eq!(recovered.gap_count, Some(0));
    }

    #[test]
    fn apply_apfs_catalog_reconstruction_evidence_marks_single_run_signature_backed_results_as_reassembled(
    ) {
        let mut recovered = RecoveredFile {
            integrity: "intact".into(),
            extension: "txt".into(),
            size_bytes: 128,
            expected_size_bytes: Some(128),
            created_at: Some("2026-04-21T09:00:00".into()),
            modified_at: Some("2026-04-21T09:05:00".into()),
            byte_runs: Some(vec![crate::types::ByteRun {
                offset: 4096,
                length: 128,
                zero_fill: false,
                compression_kind: None,
                source_view: None,
            }]),
            ..Default::default()
        };

        apply_apfs_catalog_reconstruction_evidence(&mut recovered);

        assert_eq!(recovered.validator_status.as_deref(), Some("reassembled"));
        assert_eq!(recovered.recovery_complexity.as_deref(), Some("medium"));
        assert_eq!(recovered.assembly_segment_count, Some(1));
        assert_eq!(recovered.gap_count, Some(0));
    }

    #[test]
    fn apply_apfs_catalog_reconstruction_evidence_marks_complete_multi_run_signature_backed_results_as_reassembled(
    ) {
        let mut recovered = RecoveredFile {
            integrity: "fragmented".into(),
            extension: "jpg".into(),
            size_bytes: 8192,
            expected_size_bytes: Some(8192),
            created_at: Some("2026-04-21T09:00:00".into()),
            modified_at: Some("2026-04-21T09:05:00".into()),
            byte_runs: Some(vec![
                crate::types::ByteRun {
                    offset: 4096,
                    length: 4096,
                    zero_fill: false,
                    compression_kind: None,
                    source_view: None,
                },
                crate::types::ByteRun {
                    offset: 16384,
                    length: 4096,
                    zero_fill: false,
                    compression_kind: None,
                    source_view: None,
                },
            ]),
            ..Default::default()
        };

        apply_apfs_catalog_reconstruction_evidence(&mut recovered);

        assert_eq!(recovered.validator_status.as_deref(), Some("reassembled"));
        assert_eq!(recovered.recovery_complexity.as_deref(), Some("medium"));
        assert_eq!(recovered.assembly_segment_count, Some(2));
        assert_eq!(recovered.gap_count, Some(0));
    }

    #[test]
    fn apply_apfs_catalog_reconstruction_evidence_keeps_signature_backed_results_unsupported_without_catalog_timestamps(
    ) {
        let mut recovered = RecoveredFile {
            integrity: "intact".into(),
            extension: "txt".into(),
            size_bytes: 256,
            expected_size_bytes: Some(256),
            byte_runs: Some(vec![crate::types::ByteRun {
                offset: 8192,
                length: 256,
                zero_fill: false,
                compression_kind: None,
                source_view: None,
            }]),
            ..Default::default()
        };

        apply_apfs_catalog_reconstruction_evidence(&mut recovered);

        assert_eq!(recovered.validator_status.as_deref(), Some("unsupported"));
        assert_eq!(recovered.recovery_complexity.as_deref(), Some("low"));
    }
}
