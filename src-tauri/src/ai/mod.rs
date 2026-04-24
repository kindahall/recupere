#![allow(dead_code)]
use crate::types::{
    AiAdvisory, AiRecoveryBrief, AiRecoveryCounts, DetectedDevice, DeviceType, DiagnosticResult,
    FilesystemType, Recommendation, RecoveredFile, RuntimeCapabilities, TechnicalLogEntry,
};

pub fn build_local_advisory(
    device: &DetectedDevice,
    diagnostic: &DiagnosticResult,
    capabilities: &RuntimeCapabilities,
) -> AiAdvisory {
    let recommended_action = select_recommended_action(&diagnostic.recommendations);
    let confidence_score = advisory_confidence_score(device, diagnostic);
    let summary = build_summary(device, diagnostic, recommended_action);
    let rationale = build_rationale(diagnostic, recommended_action);
    let cautions = build_cautions(diagnostic);
    let next_steps = build_next_steps(diagnostic, recommended_action);
    let expert_notes = build_expert_notes(device, diagnostic, capabilities);

    AiAdvisory {
        device_id: device.id.clone(),
        mode: "local".into(),
        confidence_score,
        summary,
        rationale,
        cautions,
        next_steps,
        expert_notes,
        recommended_action_type: recommended_action.map(|action| action.rec_type.clone()),
        recommended_action_title: recommended_action.map(|action| action.title_key.clone()),
        cloud_available: capabilities.optional_cloud_ai,
    }
}

pub fn build_scan_recovery_brief(
    scan_id: &str,
    files: &[RecoveredFile],
    logs: &[TechnicalLogEntry],
) -> AiRecoveryBrief {
    let counts = classify_recovery_counts(files);
    let confidence_score = recovery_brief_confidence(files, logs, &counts);
    let strategy_title = recovery_strategy_title(files, logs, &counts);
    let summary = recovery_summary(logs, &counts, &strategy_title);
    let strategy_reasoning = recovery_strategy_reasoning(files, logs, &counts);
    let evidence = recovery_evidence(files, logs, &counts);
    let cautions = recovery_cautions(files, logs, &counts);
    let next_steps = recovery_next_steps(files, logs, &counts);
    let expert_notes = recovery_expert_notes(files, logs, &counts);
    let priority_order = recovery_priority_order(&counts);
    let stability_reason = recovery_stability_reason(files, logs, &counts);
    let blocked_by = recovery_blocked_by(files, logs);
    let safe_export_strategy = recovery_safe_export_strategy(files, &counts);
    let complexity_summary = recovery_complexity_summary(files, &counts);

    AiRecoveryBrief {
        scan_id: scan_id.into(),
        mode: "local-results".into(),
        confidence_score,
        summary,
        strategy_title,
        strategy_reasoning,
        evidence,
        cautions,
        next_steps,
        expert_notes,
        priority_order,
        stability_reason,
        blocked_by,
        safe_export_strategy,
        complexity_summary,
        counts,
    }
}

fn select_recommended_action(recommendations: &[Recommendation]) -> Option<&Recommendation> {
    recommendations
        .iter()
        .filter(|recommendation| recommendation.is_recommended)
        .min_by_key(|recommendation| recommendation.priority)
        .or_else(|| {
            recommendations
                .iter()
                .min_by_key(|recommendation| recommendation.priority)
        })
}

fn advisory_confidence_score(device: &DetectedDevice, diagnostic: &DiagnosticResult) -> u8 {
    crate::scoring::advisory_confidence(device, diagnostic)
}

fn build_summary(
    _device: &DetectedDevice,
    diagnostic: &DiagnosticResult,
    recommended_action: Option<&Recommendation>,
) -> String {
    let recovery_band = match diagnostic.recoverability_score {
        80..=100 => "ai_advisory.summary.high",
        55..=79 => "ai_advisory.summary.moderate",
        30..=54 => "ai_advisory.summary.limited",
        _ => "ai_advisory.summary.low",
    };

    let strategy = recommended_action
        .map(|recommendation| recommendation.title_key.as_str())
        .unwrap_or("ai_advisory.summary.review_first");

    format!("{recovery_band}|{strategy}")
}

fn build_rationale(
    diagnostic: &DiagnosticResult,
    recommended_action: Option<&Recommendation>,
) -> Vec<String> {
    let mut rationale: Vec<String> = diagnostic.probable_causes.iter().take(2).cloned().collect();

    if let Some(action) = recommended_action {
        rationale.push(format!(
            "ai_advisory.rationale.prioritizes|{}|{}",
            action.title_key, action.description_key
        ));
    }

    if !diagnostic.potential_volumes.is_empty() {
        rationale.push("ai_advisory.rationale.potential_volumes".into());
    }

    if rationale.is_empty() {
        rationale.push("ai_advisory.rationale.default".into());
    }

    rationale
}

fn build_cautions(diagnostic: &DiagnosticResult) -> Vec<String> {
    let mut cautions: Vec<String> = diagnostic.limitations.iter().take(3).cloned().collect();

    if diagnostic.imaging_block_reason.is_some() {
        cautions.push("ai_advisory.caution.imaging_blocked".into());
    }

    if cautions.is_empty() {
        cautions.push("ai_advisory.caution.default".into());
    }

    cautions
}

fn build_next_steps(
    diagnostic: &DiagnosticResult,
    recommended_action: Option<&Recommendation>,
) -> Vec<String> {
    let mut ordered: Vec<&Recommendation> = diagnostic.recommendations.iter().collect();
    ordered.sort_by_key(|recommendation| recommendation.priority);

    let mut next_steps = Vec::new();
    if let Some(action) = recommended_action {
        next_steps.push(action.title_key.clone());
    }

    for recommendation in ordered {
        if next_steps.len() >= 3 {
            break;
        }

        if next_steps
            .iter()
            .any(|item| item == &recommendation.title_key)
        {
            continue;
        }

        next_steps.push(recommendation.title_key.clone());
    }

    if next_steps.is_empty() {
        next_steps.push("ai_advisory.next_steps.review_limitations".into());
    }

    next_steps
}

fn build_expert_notes(
    device: &DetectedDevice,
    diagnostic: &DiagnosticResult,
    capabilities: &RuntimeCapabilities,
) -> Vec<String> {
    let mut notes = vec![
        format!("ai_advisory.expert.device_path|{}", device.device_path),
        format!(
            "ai_advisory.expert.filesystem|{}",
            filesystem_label(&device.filesystem)
        ),
        "ai_advisory.expert.mode_local".into(),
    ];

    if let Some(path) = &diagnostic.imaging_source_path {
        notes.push(format!("ai_advisory.expert.imaging_source|{path}"));
    }

    if diagnostic.imaging_requires_elevation {
        notes.push("ai_advisory.expert.elevation_required".into());
    }

    notes.push(if capabilities.optional_cloud_ai {
        "ai_advisory.expert.cloud_yes".into()
    } else {
        "ai_advisory.expert.cloud_no".into()
    });

    notes
}

fn device_type_label(device_type: &DeviceType) -> &'static str {
    match device_type {
        DeviceType::Hdd => "HDD",
        DeviceType::Ssd => "SSD",
        DeviceType::Nvme => "NVMe",
        DeviceType::Usb => "USB",
        DeviceType::Sd => "SD card",
        DeviceType::External => "external",
        DeviceType::Image => "disk image",
    }
}

fn filesystem_label(filesystem: &FilesystemType) -> &'static str {
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

fn lowercase_first(value: &str) -> String {
    let mut characters = value.chars();
    match characters.next() {
        Some(first) => first.to_lowercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

fn classify_recovery_counts(files: &[RecoveredFile]) -> AiRecoveryCounts {
    let mut export_now = 0u32;
    let mut verify_with_preview = 0u32;
    let mut complex_recovery_review = 0u32;
    let mut unstable = 0u32;

    for file in files {
        if is_unstable_result(file) {
            unstable += 1;
        } else if is_complex_result(file) {
            complex_recovery_review += 1;
        } else if is_export_now_result(file) {
            export_now += 1;
        } else {
            verify_with_preview += 1;
        }
    }

    AiRecoveryCounts {
        export_now,
        verify_with_preview,
        complex_recovery_review,
        review_first: verify_with_preview.saturating_add(complex_recovery_review),
        unstable,
        deleted: files.iter().filter(|file| file.is_deleted).count() as u32,
        carved: files
            .iter()
            .filter(|file| file.recovery_method == "carving")
            .count() as u32,
        fragmented: files
            .iter()
            .filter(|file| file.integrity == "fragmented" || file.integrity == "partial")
            .count() as u32,
        previewable: files.iter().filter(|file| file.preview_available).count() as u32,
        compressed: files
            .iter()
            .filter(|file| file.compression_kind.is_some())
            .count() as u32,
        snapshot_derived: files
            .iter()
            .filter(|file| file.source_view.as_deref() == Some("snapshot"))
            .count() as u32,
        journal_derived: files
            .iter()
            .filter(|file| file.journal_derived || file.source_view.as_deref() == Some("journal"))
            .count() as u32,
        apfs_catalog_preview_first: files
            .iter()
            .filter(|file| is_apfs_catalog_metadata_review_result(file))
            .count() as u32,
        apfs_catalog_reassembled: files
            .iter()
            .filter(|file| is_apfs_catalog_reassembled_result(file))
            .count() as u32,
    }
}

fn is_apfs_catalog_metadata_review_result(file: &RecoveredFile) -> bool {
    file.is_deleted
        && file.source_view.as_deref() == Some("live-catalog")
        && matches!(
            file.validator_status.as_deref(),
            Some("unsupported" | "partial-unvalidated")
        )
}

fn is_apfs_catalog_reassembled_result(file: &RecoveredFile) -> bool {
    file.is_deleted
        && file.source_view.as_deref() == Some("live-catalog")
        && file.validator_status.as_deref() == Some("reassembled")
}

fn recovery_brief_confidence(
    files: &[RecoveredFile],
    logs: &[TechnicalLogEntry],
    counts: &AiRecoveryCounts,
) -> u8 {
    crate::scoring::recovery_brief_confidence(files, logs, counts)
}

fn recovery_strategy_title(
    files: &[RecoveredFile],
    logs: &[TechnicalLogEntry],
    counts: &AiRecoveryCounts,
) -> String {
    let warning_or_error_logs = logs
        .iter()
        .filter(|entry| entry.level == "warning" || entry.level == "error")
        .count();

    if warning_or_error_logs >= 2 {
        return "ai_recovery.strategy.stabilize".into();
    }
    if counts.complex_recovery_review > 0 {
        return "ai_recovery.strategy.review_complex".into();
    }
    if counts.unstable >= counts.export_now && counts.unstable > 0 {
        return "ai_recovery.strategy.review_unstable".into();
    }
    if counts.verify_with_preview > 0 || counts.fragmented > 0 || counts.carved > 0 {
        return "ai_recovery.strategy.preview_complex".into();
    }
    if files.iter().any(|file| file.is_deleted) {
        return "ai_recovery.strategy.export_deleted_first".into();
    }
    "ai_recovery.strategy.export_strongest".into()
}

fn recovery_summary(
    _logs: &[TechnicalLogEntry],
    _counts: &AiRecoveryCounts,
    strategy_title: &str,
) -> String {
    format!("ai_recovery.summary|{strategy_title}")
}

fn recovery_strategy_reasoning(
    _files: &[RecoveredFile],
    logs: &[TechnicalLogEntry],
    counts: &AiRecoveryCounts,
) -> Vec<String> {
    let mut items = Vec::new();

    if counts.export_now > 0 {
        items.push("ai_recovery.reasoning.export_now_intact".into());
    }
    if counts.verify_with_preview > 0 {
        items.push("ai_recovery.reasoning.verify_preview".into());
    }
    if counts.complex_recovery_review > 0 {
        items.push("ai_recovery.reasoning.complex_sensitive".into());
    }
    if counts.fragmented > 0 {
        items.push("ai_recovery.reasoning.fragmented_risk".into());
    }
    if counts.carved > 0 {
        items.push("ai_recovery.reasoning.carved_signature".into());
    }
    let warnings = logs
        .iter()
        .filter(|entry| entry.level == "warning" || entry.level == "error")
        .count();
    if warnings > 0 {
        items.push("ai_recovery.reasoning.warnings_observed".into());
    }
    if items.is_empty() {
        items.push("ai_recovery.reasoning.no_warnings".into());
    }

    items
}

fn recovery_evidence(
    files: &[RecoveredFile],
    logs: &[TechnicalLogEntry],
    _counts: &AiRecoveryCounts,
) -> Vec<String> {
    let mut items = vec![
        "ai_recovery.evidence.intact_vs_corrupt".into(),
        "ai_recovery.evidence.deleted_previewable".into(),
    ];

    let has_extensions = files.iter().any(|f| !f.extension.is_empty());
    if has_extensions {
        items.push("ai_recovery.evidence.top_file_groups".into());
    }
    if files.iter().any(is_apfs_catalog_reassembled_result) {
        items.push("ai_recovery.evidence.apfs_catalog_reassembled".into());
    }

    let errors = logs.iter().filter(|entry| entry.level == "error").count();
    if errors > 0 {
        items.push("ai_recovery.evidence.backend_errors".into());
    }

    items
}

fn recovery_cautions(
    files: &[RecoveredFile],
    logs: &[TechnicalLogEntry],
    counts: &AiRecoveryCounts,
) -> Vec<String> {
    let mut items = Vec::new();

    if counts.fragmented > 0 {
        items.push("ai_recovery.caution.fragmented_incomplete".into());
    }
    if counts.carved > 0 {
        items.push("ai_recovery.caution.carved_inconsistent".into());
    }
    if logs.iter().any(|entry| entry.level == "error") {
        items.push("ai_recovery.caution.backend_errors".into());
    }
    if counts.snapshot_derived > 0 {
        items.push("ai_recovery.caution.snapshot_verify".into());
    }
    if counts.journal_derived > 0 {
        items.push("ai_recovery.caution.journal_higher_risk".into());
    }
    if files.iter().any(is_apfs_catalog_metadata_review_result) {
        items.push("ai_recovery.caution.apfs_catalog_verify".into());
    }
    if files.iter().any(is_apfs_catalog_reassembled_result) {
        items.push("ai_recovery.caution.apfs_reassembled_verify".into());
    }
    if counts.compressed > 0 {
        items.push("ai_recovery.caution.compressed_batches".into());
    }
    if files
        .iter()
        .any(|file| file.expected_size_bytes.unwrap_or(file.size_bytes) > file.size_bytes)
    {
        items.push("ai_recovery.caution.truncated_payload".into());
    }
    if items.is_empty() {
        items.push("ai_recovery.caution.default".into());
    }

    items
}

fn recovery_next_steps(
    files: &[RecoveredFile],
    logs: &[TechnicalLogEntry],
    counts: &AiRecoveryCounts,
) -> Vec<String> {
    let mut steps = Vec::new();

    if counts.export_now > 0 {
        steps.push("ai_recovery.next.export_strongest".into());
    }
    if counts.previewable > 0 && (counts.fragmented > 0 || counts.carved > 0) {
        steps.push("ai_recovery.next.preview_before_export".into());
    }
    if logs
        .iter()
        .any(|entry| entry.level == "warning" || entry.level == "error")
    {
        steps.push("ai_recovery.next.review_logs".into());
    }
    if counts.complex_recovery_review > 0 {
        steps.push("ai_recovery.next.inspect_complex".into());
    }
    if files.iter().any(is_apfs_catalog_metadata_review_result) {
        steps.push("ai_recovery.next.verify_apfs_catalog_deleted".into());
    }
    if files.iter().any(|file| file.is_deleted) {
        steps.push("ai_recovery.next.prioritize_deleted_intact".into());
    }
    if steps.is_empty() {
        steps.push("ai_recovery.next.default".into());
    }

    steps.truncate(4);
    steps
}

fn recovery_expert_notes(
    files: &[RecoveredFile],
    logs: &[TechnicalLogEntry],
    counts: &AiRecoveryCounts,
) -> Vec<String> {
    let warning_or_error_logs = logs
        .iter()
        .filter(|entry| entry.level == "warning" || entry.level == "error")
        .count();
    let max_score = files
        .iter()
        .map(|file| file.recovery_score)
        .max()
        .unwrap_or(0);

    vec![
        format!("ai_recovery.expert.results_analyzed|{}", files.len()),
        format!("ai_recovery.expert.previewable|{}", counts.previewable),
        format!("ai_recovery.expert.deleted|{}", counts.deleted),
        format!("ai_recovery.expert.carved|{}", counts.carved),
        format!("ai_recovery.expert.compressed|{}", counts.compressed),
        format!(
            "ai_recovery.expert.snapshot_derived|{}",
            counts.snapshot_derived
        ),
        format!(
            "ai_recovery.expert.journal_derived|{}",
            counts.journal_derived
        ),
        format!(
            "ai_recovery.expert.apfs_catalog_review|{}",
            files
                .iter()
                .filter(|file| is_apfs_catalog_metadata_review_result(file))
                .count()
        ),
        format!(
            "ai_recovery.expert.apfs_catalog_reassembled|{}",
            files
                .iter()
                .filter(|file| is_apfs_catalog_reassembled_result(file))
                .count()
        ),
        format!("ai_recovery.expert.warning_logs|{}", warning_or_error_logs),
        format!("ai_recovery.expert.highest_score|{}", max_score),
    ]
}

fn is_export_now_result(file: &RecoveredFile) -> bool {
    file.integrity == "intact"
        && file.recovery_score >= 78
        && file.validator_status.as_deref() != Some("failed")
        && !is_complex_result(file)
}

fn is_unstable_result(file: &RecoveredFile) -> bool {
    file.integrity == "corrupt"
        || file.recovery_score < 35
        || file.validator_status.as_deref() == Some("failed")
}

fn is_complex_result(file: &RecoveredFile) -> bool {
    file.recovery_complexity.as_deref() == Some("high")
        || file.compression_kind.is_some()
        || matches!(file.source_view.as_deref(), Some("snapshot" | "journal"))
        || file.journal_derived
        || is_apfs_catalog_metadata_review_result(file)
        || file.assembly_segment_count.unwrap_or(1) > 1
        || file.gap_count.unwrap_or(0) > 0
        || file.validator_status.as_deref() == Some("reassembled")
}

fn recovery_priority_order(counts: &AiRecoveryCounts) -> Vec<String> {
    let mut order = Vec::new();
    if counts.export_now > 0 {
        order.push("export_now".into());
    }
    if counts.verify_with_preview > 0 {
        order.push("verify_with_preview".into());
    }
    if counts.complex_recovery_review > 0 {
        order.push("complex_recovery_review".into());
    }
    if counts.unstable > 0 {
        order.push("hold_unstable".into());
    }
    if order.is_empty() {
        order.push("verify_with_preview".into());
    }
    order
}

fn recovery_stability_reason(
    files: &[RecoveredFile],
    logs: &[TechnicalLogEntry],
    counts: &AiRecoveryCounts,
) -> String {
    if files.is_empty() {
        return "ai_recovery.stability.no_candidates".into();
    }
    if counts.unstable > 0 {
        return "ai_recovery.stability.unstable_present".into();
    }
    if counts.complex_recovery_review > 0 {
        return "ai_recovery.stability.complex_present".into();
    }
    if logs
        .iter()
        .any(|entry| entry.level == "warning" || entry.level == "error")
    {
        return "ai_recovery.stability.warnings_present".into();
    }
    "ai_recovery.stability.export_ready".into()
}

fn recovery_blocked_by(files: &[RecoveredFile], logs: &[TechnicalLogEntry]) -> Vec<String> {
    let mut blocked_by = Vec::new();

    if files
        .iter()
        .any(|file| file.source_view.as_deref() == Some("snapshot"))
    {
        blocked_by.push("ai_recovery.blocked.snapshot".into());
    }
    if files
        .iter()
        .any(|file| file.journal_derived || file.source_view.as_deref() == Some("journal"))
    {
        blocked_by.push("ai_recovery.blocked.journal".into());
    }
    if files.iter().any(|file| file.compression_kind.is_some()) {
        blocked_by.push("ai_recovery.blocked.compressed".into());
    }
    if files
        .iter()
        .any(|file| file.validator_status.as_deref() == Some("reassembled"))
    {
        blocked_by.push("ai_recovery.blocked.reassembly".into());
    }
    if files.iter().any(is_apfs_catalog_metadata_review_result) {
        blocked_by.push("ai_recovery.blocked.apfs_catalog_metadata".into());
    }
    if logs.iter().any(|entry| entry.level == "error") {
        blocked_by.push("ai_recovery.blocked.backend_errors".into());
    }

    blocked_by
}

fn recovery_safe_export_strategy(files: &[RecoveredFile], counts: &AiRecoveryCounts) -> String {
    if files.is_empty() {
        return "ai_recovery.export_strategy.wait_scan".into();
    }
    if counts.export_now > 0 && counts.complex_recovery_review == 0 && counts.unstable == 0 {
        return "ai_recovery.export_strategy.strongest_first".into();
    }
    if counts.complex_recovery_review > 0 {
        return "ai_recovery.export_strategy.low_complexity_only".into();
    }
    if counts.verify_with_preview > 0 {
        return "ai_recovery.export_strategy.preview_all".into();
    }
    "ai_recovery.export_strategy.hold_unstable".into()
}

fn recovery_complexity_summary(files: &[RecoveredFile], _counts: &AiRecoveryCounts) -> String {
    if files.iter().any(is_apfs_catalog_metadata_review_result) {
        return "ai_recovery.complexity_summary_apfs_catalog".into();
    }
    if files.iter().any(is_apfs_catalog_reassembled_result) {
        return "ai_recovery.complexity_summary_apfs_reassembled".into();
    }

    "ai_recovery.complexity_summary".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        DeviceStatus, RecoveredFile, RiskLevel, RuntimeCapabilities, TechnicalLogEntry,
    };

    fn sample_device() -> DetectedDevice {
        DetectedDevice {
            id: "dev-1".into(),
            name: "USB Case".into(),
            device_path: "/dev/disk-test".into(),
            device_type: DeviceType::Usb,
            filesystem: FilesystemType::Ntfs,
            capacity_bytes: 100,
            used_bytes: 50,
            status: DeviceStatus::Healthy,
            risk_level: RiskLevel::Medium,
            serial: None,
            model: None,
            is_trim_enabled: Some(false),
            is_encrypted: Some(false),
            smart_available: Some(false),
            partitions: Vec::new(),
        }
    }

    fn sample_diagnostic() -> DiagnosticResult {
        DiagnosticResult {
            device_id: "dev-1".into(),
            recoverability_score: 72,
            loss_type: "accidental-deletion".into(),
            probable_causes: vec!["Recent directory deletion was detected.".into()],
            risk_factors: Vec::new(),
            recommendations: vec![
                Recommendation {
                    id: "rec-1".into(),
                    rec_type: "scan-deleted-ntfs".into(),
                    priority: 1,
                    title_key: "recommendation.scan_deleted.title".into(),
                    description_key: "recommendation.scan_deleted.description".into(),
                    is_recommended: true,
                    target_potential_volume_id: None,
                    target_potential_volume_label: None,
                    target_potential_volume_filesystem: None,
                    target_potential_volume_start_offset: None,
                    target_potential_volume_size_bytes: None,
                },
                Recommendation {
                    id: "rec-2".into(),
                    rec_type: "image-first".into(),
                    priority: 2,
                    title_key: "recommendation.image_first.title".into(),
                    description_key: "recommendation.image_first.description_default".into(),
                    is_recommended: false,
                    target_potential_volume_id: None,
                    target_potential_volume_label: None,
                    target_potential_volume_filesystem: None,
                    target_potential_volume_start_offset: None,
                    target_potential_volume_size_bytes: None,
                },
            ],
            limitations: vec!["TRIM may already have erased some deleted SSD/NVMe blocks.".into()],
            imaging_ready: true,
            imaging_requires_elevation: false,
            imaging_source_path: Some("/dev/disk-test".into()),
            imaging_block_reason: None,
            imaging_profile: "standard".into(),
            imaging_profile_reason_key: "imaging.profile_reason_standard".into(),
            potential_volumes_inspected: false,
            potential_volumes_notice: None,
            potential_volumes: Vec::new(),
            verdict: "simple".into(),
            verdict_details: "Recovery is straightforward. You can safely recover your files here without professional help.".into(),
        }
    }

    fn sample_capabilities() -> RuntimeCapabilities {
        RuntimeCapabilities {
            device_detection: true,
            heuristic_diagnostic: true,
            ai_advisory: true,
            optional_cloud_ai: false,
            scan_engine: true,
            imaging_engine: true,
            results_browser: true,
            export_validation: true,
            export_engine: true,
            technical_logs: true,
            limited_capabilities: vec!["scanEngine".into(), "technicalLogs".into()],
        }
    }

    fn sample_recovered_file(
        id: &str,
        integrity: &str,
        score: u8,
        recovery_method: &str,
        is_deleted: bool,
        preview_available: bool,
    ) -> RecoveredFile {
        RecoveredFile {
            id: id.into(),
            name: format!("{id}.txt"),
            path: "/docs".into(),
            extension: "txt".into(),
            size_bytes: 512,
            created_at: None,
            modified_at: None,
            integrity: integrity.into(),
            recovery_score: score,
            recovery_method: recovery_method.into(),
            preview_available,
            mime_type: Some("text/plain".into()),
            expected_size_bytes: Some(700),
            deleted_at: None,
            start_offset: None,
            clusters: None,
            byte_runs: None,
            resource_fork: None,
            alternate_data_streams: None,
            source_image_path: Some("/tmp/image.raw".into()),
            is_deleted,
            ..Default::default()
        }
    }

    fn sample_logs() -> Vec<TechnicalLogEntry> {
        vec![
            TechnicalLogEntry {
                timestamp_ms: 1,
                level: "info".into(),
                message: "Scan started".into(),
            },
            TechnicalLogEntry {
                timestamp_ms: 2,
                level: "warning".into(),
                message: "Fragmented candidate detected".into(),
            },
        ]
    }

    #[test]
    fn build_local_advisory_prefers_recommended_action() {
        let advisory = build_local_advisory(
            &sample_device(),
            &sample_diagnostic(),
            &sample_capabilities(),
        );

        assert_eq!(advisory.mode, "local");
        assert_eq!(
            advisory.recommended_action_type.as_deref(),
            Some("scan-deleted-ntfs")
        );
        assert!(
            advisory
                .summary
                .contains("recommendation.scan_deleted.title"),
            "summary should mention the preferred strategy key"
        );
        assert!(!advisory.cloud_available);
    }

    #[test]
    fn build_local_advisory_reduces_confidence_for_unknown_and_encrypted_cases() {
        let mut device = sample_device();
        device.device_type = DeviceType::Nvme;
        device.is_trim_enabled = Some(true);
        device.is_encrypted = Some(true);
        device.status = DeviceStatus::Failing;

        let mut diagnostic = sample_diagnostic();
        diagnostic.loss_type = "unknown".into();

        let advisory = build_local_advisory(&device, &diagnostic, &sample_capabilities());
        assert!(
            advisory.confidence_score < 60,
            "confidence should drop on encrypted/trim/failing unknown cases"
        );
    }

    #[test]
    fn build_local_advisory_falls_back_to_generic_caution_when_needed() {
        let mut diagnostic = sample_diagnostic();
        diagnostic.limitations.clear();

        let advisory = build_local_advisory(&sample_device(), &diagnostic, &sample_capabilities());

        assert_eq!(advisory.cautions.len(), 1);
        assert!(advisory.cautions[0].contains("ai_advisory.caution.default"));
    }

    #[test]
    fn build_scan_recovery_brief_prioritizes_intact_results() {
        let files = vec![
            sample_recovered_file("strong", "intact", 92, "filesystem", true, true),
            sample_recovered_file("review", "partial", 61, "reconstruction", true, true),
            sample_recovered_file("risky", "corrupt", 18, "carving", true, false),
        ];

        let brief = build_scan_recovery_brief("scan-1", &files, &sample_logs());

        assert_eq!(brief.scan_id, "scan-1");
        assert_eq!(brief.mode, "local-results");
        assert_eq!(brief.counts.export_now, 1);
        assert_eq!(brief.counts.verify_with_preview, 1);
        assert_eq!(brief.counts.complex_recovery_review, 0);
        assert_eq!(brief.counts.review_first, 1);
        assert_eq!(brief.counts.unstable, 1);
        assert!(brief.summary.contains("ai_recovery.summary"));
        assert_eq!(
            brief.priority_order.first().map(String::as_str),
            Some("export_now")
        );
        assert!(brief
            .safe_export_strategy
            .contains("ai_recovery.export_strategy"));
        assert!(!brief.strategy_title.is_empty());
    }

    #[test]
    fn build_scan_recovery_brief_surfaces_complex_cautions() {
        let files = vec![
            sample_recovered_file("frag", "fragmented", 49, "reconstruction", true, true),
            sample_recovered_file("carved", "partial", 41, "carving", true, true),
        ];
        let logs = vec![TechnicalLogEntry {
            timestamp_ms: 3,
            level: "error".into(),
            message: "Read retry budget exceeded".into(),
        }];

        let brief = build_scan_recovery_brief("scan-2", &files, &logs);

        assert!(brief.counts.fragmented >= 2);
        assert!(brief
            .cautions
            .iter()
            .any(|item| item.contains("ai_recovery.caution.fragmented_incomplete")));
        assert!(brief
            .cautions
            .iter()
            .any(|item| item.contains("ai_recovery.caution.backend_errors")));
    }

    #[test]
    fn build_scan_recovery_brief_routes_apfs_catalog_metadata_results_to_complex_review() {
        let mut apfs_catalog =
            sample_recovered_file("apfs-catalog", "intact", 86, "reconstruction", true, true);
        apfs_catalog.source_view = Some("live-catalog".into());
        apfs_catalog.validator_status = Some("unsupported".into());
        apfs_catalog.recovery_complexity = Some("low".into());

        let brief = build_scan_recovery_brief("scan-apfs", &[apfs_catalog], &sample_logs());

        assert_eq!(brief.counts.export_now, 0);
        assert_eq!(brief.counts.complex_recovery_review, 1);
        assert_eq!(brief.counts.review_first, 1);
        assert_eq!(brief.counts.apfs_catalog_preview_first, 1);
        assert_eq!(brief.counts.apfs_catalog_reassembled, 0);
        assert!(brief
            .cautions
            .iter()
            .any(|item| item.contains("ai_recovery.caution.apfs_catalog_verify")));
        assert!(brief
            .next_steps
            .iter()
            .any(|item| item.contains("ai_recovery.next.verify_apfs_catalog_deleted")));
        assert!(brief
            .blocked_by
            .iter()
            .any(|item| item.contains("ai_recovery.blocked.apfs_catalog_metadata")));
        assert!(brief
            .complexity_summary
            .contains("ai_recovery.complexity_summary_apfs_catalog"));
        assert_eq!(
            brief.priority_order.first().map(String::as_str),
            Some("complex_recovery_review")
        );
    }

    #[test]
    fn build_scan_recovery_brief_reports_apfs_catalog_reassembled_separately() {
        let mut apfs_catalog = sample_recovered_file(
            "apfs-reassembled",
            "intact",
            86,
            "reconstruction",
            true,
            true,
        );
        apfs_catalog.source_view = Some("live-catalog".into());
        apfs_catalog.validator_status = Some("reassembled".into());
        apfs_catalog.recovery_complexity = Some("medium".into());

        let brief = build_scan_recovery_brief("scan-apfs-reassembled", &[apfs_catalog], &[]);

        assert_eq!(brief.counts.apfs_catalog_preview_first, 0);
        assert_eq!(brief.counts.apfs_catalog_reassembled, 1);
        assert!(brief
            .evidence
            .iter()
            .any(|item| item.contains("ai_recovery.evidence.apfs_catalog_reassembled")));
        assert!(brief
            .cautions
            .iter()
            .any(|item| item.contains("ai_recovery.caution.apfs_reassembled_verify")));
        assert!(brief
            .complexity_summary
            .contains("ai_recovery.complexity_summary_apfs_reassembled"));
    }
}
