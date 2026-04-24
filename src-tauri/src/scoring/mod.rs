// ============================================================================
// Récupère — Centralized Scoring Engine
// ============================================================================
// All recoverability, confidence, and recovery-score calculations live here
// so that commands/, ai/, and carving/ share a single source of truth.
// ============================================================================

use crate::types::{
    AiRecoveryCounts, DetectedDevice, DeviceStatus, DeviceType, DiagnosticResult, FilesystemType,
    RecoveredFile, RiskLevel, TechnicalLogEntry,
};

// ---------------------------------------------------------------------------
// Diagnostic recoverability score (0-88)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn recoverability_score(
    risk_level: &RiskLevel,
    device_status: &DeviceStatus,
    device_type: &DeviceType,
    is_trim_enabled: Option<bool>,
    is_encrypted: Option<bool>,
    filesystem: &FilesystemType,
    smart_available: Option<bool>,
    has_potential_lost_volume: bool,
) -> u8 {
    let mut score = 72_i32;

    score -= risk_penalty(risk_level);
    score -= status_penalty(device_status);

    if matches!(device_type, DeviceType::Ssd | DeviceType::Nvme) && is_trim_enabled.unwrap_or(false)
    {
        score -= 12;
    }

    if is_encrypted.unwrap_or(false) {
        score -= 20;
    }

    if matches!(filesystem, FilesystemType::Unknown) {
        score -= 12;
    } else {
        score += 6;
    }

    if smart_available == Some(false) {
        score -= 4;
    }

    if has_potential_lost_volume {
        score += 6;
    }

    score.clamp(5, 88) as u8
}

// ---------------------------------------------------------------------------
// Risk / status penalty helpers
// ---------------------------------------------------------------------------

pub fn risk_penalty(level: &RiskLevel) -> i32 {
    match level {
        RiskLevel::Low => 0,
        RiskLevel::Medium => 6,
        RiskLevel::High => 14,
        RiskLevel::Critical => 24,
    }
}

pub fn status_penalty(status: &DeviceStatus) -> i32 {
    match status {
        DeviceStatus::Healthy => 0,
        DeviceStatus::Degraded => 8,
        DeviceStatus::Failing => 22,
        DeviceStatus::Unresponsive => 32,
        DeviceStatus::Unknown => 6,
    }
}

// ---------------------------------------------------------------------------
// AI advisory confidence (34-93)
// ---------------------------------------------------------------------------

pub fn advisory_confidence(device: &DetectedDevice, diagnostic: &DiagnosticResult) -> u8 {
    let mut score = 84_i32;

    if diagnostic.loss_type == "unknown" {
        score -= 16;
    }

    score -= match device.status {
        DeviceStatus::Healthy => 0,
        DeviceStatus::Degraded => 8,
        DeviceStatus::Failing => 15,
        DeviceStatus::Unresponsive => 22,
        DeviceStatus::Unknown => 6,
    };

    if matches!(device.device_type, DeviceType::Ssd | DeviceType::Nvme)
        && device.is_trim_enabled.unwrap_or(false)
    {
        score -= 12;
    }

    if device.is_encrypted.unwrap_or(false) {
        score -= 18;
    }

    if diagnostic.imaging_block_reason.is_some() {
        score -= 8;
    }

    if !diagnostic.potential_volumes.is_empty() {
        score += 4;
    }

    if diagnostic.recoverability_score >= 70 {
        score += 3;
    } else if diagnostic.recoverability_score <= 35 {
        score -= 6;
    }

    score.clamp(34, 93) as u8
}

// ---------------------------------------------------------------------------
// Recovery brief confidence (28-94)
// ---------------------------------------------------------------------------

pub fn recovery_brief_confidence(
    files: &[RecoveredFile],
    logs: &[TechnicalLogEntry],
    counts: &AiRecoveryCounts,
) -> u8 {
    if files.is_empty() {
        return 36;
    }

    let warning_or_error_logs = logs
        .iter()
        .filter(|entry| entry.level == "warning" || entry.level == "error")
        .count() as i32;

    let mut score = 72_i32;
    score += (counts.export_now as i32 / 4).min(12);
    score -= (counts.unstable as i32 / 2).min(18);
    score -= (counts.fragmented as i32 / 3).min(14);
    score -= (counts.carved as i32 / 4).min(10);
    score -= (counts.complex_recovery_review as i32 / 2).min(14);
    score -= (counts.compressed as i32 * 3).min(9);
    score -= (counts.snapshot_derived as i32 * 2).min(8);
    score -= (counts.journal_derived as i32 * 2).min(8);
    score -= (warning_or_error_logs * 3).min(18);

    if counts.previewable > 0 {
        score += 4;
    }

    score.clamp(28, 94) as u8
}

// ---------------------------------------------------------------------------
// Carving recovery score
// ---------------------------------------------------------------------------

pub fn carving_recovery_score(integrity: &str, footer_found: bool, format: &str) -> u8 {
    match (integrity, footer_found, format) {
        ("intact", true, "zip") => 71,
        ("intact", true, _) => 76,
        ("fragmented", _, _) => 44,
        ("corrupt", _, _) => 18,
        (_, false, "zip") => 35,
        _ => 39,
    }
}

// ---------------------------------------------------------------------------
// Recovery complexity classification
// ---------------------------------------------------------------------------

pub fn classify_recovery_complexity(
    integrity: &str,
    assembly_segment_count: u8,
    gap_count: u8,
    validator_status: &str,
) -> &'static str {
    if gap_count >= 2
        || assembly_segment_count >= 3
        || matches!(validator_status, "failed" | "partial-unvalidated")
        || matches!(integrity, "corrupt")
    {
        return "high";
    }

    if gap_count > 0
        || assembly_segment_count > 1
        || matches!(integrity, "fragmented" | "partial")
        || matches!(
            validator_status,
            "reassembled" | "zip-validated" | "office-validated"
        )
    {
        return "medium";
    }

    "low"
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_known_fs_device_scores_high() {
        let score = recoverability_score(
            &RiskLevel::Low,
            &DeviceStatus::Healthy,
            &DeviceType::Hdd,
            Some(false),
            Some(false),
            &FilesystemType::Ntfs,
            Some(true),
            false,
        );
        assert_eq!(score, 78);
    }

    #[test]
    fn encrypted_trim_ssd_scores_low() {
        let score = recoverability_score(
            &RiskLevel::High,
            &DeviceStatus::Degraded,
            &DeviceType::Ssd,
            Some(true),
            Some(true),
            &FilesystemType::Apfs,
            Some(true),
            false,
        );
        assert!(score <= 40);
    }

    #[test]
    fn risk_penalty_values() {
        assert_eq!(risk_penalty(&RiskLevel::Low), 0);
        assert_eq!(risk_penalty(&RiskLevel::Critical), 24);
    }

    #[test]
    fn status_penalty_values() {
        assert_eq!(status_penalty(&DeviceStatus::Healthy), 0);
        assert_eq!(status_penalty(&DeviceStatus::Unresponsive), 32);
    }

    #[test]
    fn carving_score_intact_jpeg() {
        assert_eq!(carving_recovery_score("intact", true, "jpeg"), 76);
    }

    #[test]
    fn carving_score_intact_zip() {
        assert_eq!(carving_recovery_score("intact", true, "zip"), 71);
    }

    #[test]
    fn carving_score_partial() {
        assert_eq!(carving_recovery_score("partial", false, "jpeg"), 39);
    }

    #[test]
    fn carving_score_corrupt() {
        assert_eq!(carving_recovery_score("corrupt", false, "pdf"), 18);
    }

    #[test]
    fn complexity_low_for_intact() {
        assert_eq!(
            classify_recovery_complexity("intact", 1, 0, "validated"),
            "low"
        );
    }

    #[test]
    fn complexity_medium_for_fragmented() {
        assert_eq!(
            classify_recovery_complexity("fragmented", 2, 1, "reassembled"),
            "medium"
        );
    }

    #[test]
    fn complexity_high_for_corrupt() {
        assert_eq!(
            classify_recovery_complexity("corrupt", 1, 0, "failed"),
            "high"
        );
    }
}

// ---------------------------------------------------------------------------
// Property-based invariants (proptest)
// ---------------------------------------------------------------------------
//
// Scoring functions are rule-based heuristics, so their exact output per input
// is covered by the unit tests above. What we also need is a guarantee that
// *no* legal combination of inputs can break the declared output range — a
// future rule that accidentally overflows [5, 88] would ship a UI showing
// a recoverability score of 120% or 0%. Proptest explores the input space
// (default 256 cases per property) and shrinks to a minimal counterexample
// on failure.
//
// Each property is the smallest assertion that actually detects a regression:
// range bounds for every scoring fn + one monotonicity sanity check that
// shipping a known-degraded device cannot produce a higher score than the
// same device reported as healthy.

#[cfg(test)]
mod properties {
    use super::*;
    use crate::types::{
        AiRecoveryCounts, DetectedDevice, DeviceStatus, DeviceType, DiagnosticResult,
        FilesystemType, PotentialVolume, Recommendation, RecoveredFile, RiskLevel,
        TechnicalLogEntry,
    };
    use proptest::prelude::*;

    // Strategy helpers --------------------------------------------------------

    fn risk_strategy() -> impl Strategy<Value = RiskLevel> {
        prop_oneof![
            Just(RiskLevel::Low),
            Just(RiskLevel::Medium),
            Just(RiskLevel::High),
            Just(RiskLevel::Critical),
        ]
    }

    fn status_strategy() -> impl Strategy<Value = DeviceStatus> {
        prop_oneof![
            Just(DeviceStatus::Healthy),
            Just(DeviceStatus::Degraded),
            Just(DeviceStatus::Failing),
            Just(DeviceStatus::Unresponsive),
            Just(DeviceStatus::Unknown),
        ]
    }

    fn device_type_strategy() -> impl Strategy<Value = DeviceType> {
        prop_oneof![
            Just(DeviceType::Hdd),
            Just(DeviceType::Ssd),
            Just(DeviceType::Nvme),
            Just(DeviceType::Usb),
            Just(DeviceType::Sd),
            Just(DeviceType::External),
            Just(DeviceType::Image),
        ]
    }

    fn filesystem_strategy() -> impl Strategy<Value = FilesystemType> {
        prop_oneof![
            Just(FilesystemType::Ntfs),
            Just(FilesystemType::Fat32),
            Just(FilesystemType::Exfat),
            Just(FilesystemType::Apfs),
            Just(FilesystemType::HfsPlus),
            Just(FilesystemType::Ext4),
            Just(FilesystemType::Unknown),
        ]
    }

    fn synthetic_device(status: DeviceStatus, device_type: DeviceType) -> DetectedDevice {
        DetectedDevice {
            id: "proptest".into(),
            name: "proptest".into(),
            device_path: "/dev/null".into(),
            device_type,
            filesystem: FilesystemType::Unknown,
            capacity_bytes: 1_000_000,
            used_bytes: 0,
            status,
            risk_level: RiskLevel::Low,
            serial: None,
            model: None,
            is_trim_enabled: Some(false),
            is_encrypted: Some(false),
            smart_available: Some(true),
            partitions: Vec::new(),
        }
    }

    fn synthetic_diagnostic(recoverability: u8, loss_type: &str) -> DiagnosticResult {
        DiagnosticResult {
            device_id: "proptest".into(),
            recoverability_score: recoverability,
            loss_type: loss_type.into(),
            probable_causes: Vec::new(),
            risk_factors: Vec::new(),
            recommendations: Vec::<Recommendation>::new(),
            limitations: Vec::new(),
            imaging_ready: false,
            imaging_requires_elevation: false,
            imaging_profile: "standard".into(),
            imaging_profile_reason_key: String::new(),
            imaging_source_path: None,
            imaging_block_reason: None,
            potential_volumes_inspected: false,
            potential_volumes_notice: None,
            potential_volumes: Vec::new(),
            verdict: String::new(),
            verdict_details: String::new(),
        }
    }

    fn synthetic_potential_volume() -> PotentialVolume {
        PotentialVolume {
            id: "pv".into(),
            label: "pv".into(),
            filesystem: FilesystemType::Unknown,
            start_offset: 0,
            size_bytes: None,
            confidence_score: 50,
            detection_method: "synthetic".into(),
            notes: Vec::new(),
        }
    }

    // Properties --------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, .. ProptestConfig::default() })]

        #[test]
        fn recoverability_score_always_in_documented_range(
            risk in risk_strategy(),
            status in status_strategy(),
            device_type in device_type_strategy(),
            trim in proptest::option::of(any::<bool>()),
            encrypted in proptest::option::of(any::<bool>()),
            fs in filesystem_strategy(),
            smart in proptest::option::of(any::<bool>()),
            has_potential_lost_volume in any::<bool>(),
        ) {
            let score = recoverability_score(
                &risk,
                &status,
                &device_type,
                trim,
                encrypted,
                &fs,
                smart,
                has_potential_lost_volume,
            );
            prop_assert!(
                (5..=88).contains(&score),
                "recoverability_score produced {} outside [5, 88]",
                score
            );
        }

        #[test]
        fn degraded_status_cannot_score_higher_than_healthy(
            risk in risk_strategy(),
            device_type in device_type_strategy(),
            trim in proptest::option::of(any::<bool>()),
            encrypted in proptest::option::of(any::<bool>()),
            fs in filesystem_strategy(),
            smart in proptest::option::of(any::<bool>()),
            has_potential_lost_volume in any::<bool>(),
        ) {
            let healthy = recoverability_score(
                &risk, &DeviceStatus::Healthy, &device_type,
                trim, encrypted, &fs, smart, has_potential_lost_volume,
            );
            for worse in [
                DeviceStatus::Degraded,
                DeviceStatus::Failing,
                DeviceStatus::Unresponsive,
            ] {
                let degraded = recoverability_score(
                    &risk, &worse, &device_type,
                    trim, encrypted, &fs, smart, has_potential_lost_volume,
                );
                prop_assert!(
                    degraded <= healthy,
                    "{worse:?} ({degraded}) scored higher than Healthy ({healthy})"
                );
            }
        }

        #[test]
        fn advisory_confidence_always_in_documented_range(
            status in status_strategy(),
            device_type in device_type_strategy(),
            trim in proptest::option::of(any::<bool>()),
            encrypted in proptest::option::of(any::<bool>()),
            recoverability in 0u8..=100,
            imaging_block in proptest::option::of("[a-z_]{3,16}"),
            has_lost_volume in any::<bool>(),
            loss_type in proptest::sample::select(vec!["deletion", "format", "corruption", "unknown"]),
        ) {
            let mut device = synthetic_device(status, device_type);
            device.is_trim_enabled = trim;
            device.is_encrypted = encrypted;
            let mut diag = synthetic_diagnostic(recoverability, loss_type);
            diag.imaging_block_reason = imaging_block;
            if has_lost_volume {
                diag.potential_volumes.push(synthetic_potential_volume());
            }
            let score = advisory_confidence(&device, &diag);
            prop_assert!(
                (34..=93).contains(&score),
                "advisory_confidence produced {} outside [34, 93]",
                score
            );
        }

        #[test]
        fn carving_recovery_score_always_in_documented_range(
            integrity in proptest::sample::select(vec!["intact", "fragmented", "corrupt", "partial", "other"]),
            footer_found in any::<bool>(),
            format in proptest::sample::select(vec!["zip", "jpeg", "png", "pdf", "mp4", "other"]),
        ) {
            let score = carving_recovery_score(integrity, footer_found, format);
            prop_assert!(
                (18..=76).contains(&score),
                "carving_recovery_score produced {} outside [18, 76] for ({}, {}, {})",
                score, integrity, footer_found, format
            );
        }

        #[test]
        fn classify_recovery_complexity_returns_known_tier(
            integrity in proptest::sample::select(vec!["intact", "fragmented", "partial", "corrupt"]),
            segments in 0u8..=32,
            gaps in 0u8..=32,
            validator in proptest::sample::select(vec![
                "validated", "reassembled", "zip-validated", "office-validated",
                "failed", "partial-unvalidated", "unknown",
            ]),
        ) {
            let tier = classify_recovery_complexity(integrity, segments, gaps, validator);
            prop_assert!(
                matches!(tier, "low" | "medium" | "high"),
                "classify_recovery_complexity returned unknown tier {:?}",
                tier
            );
        }

        #[test]
        fn recovery_brief_confidence_always_in_documented_range(
            file_count in 0usize..=256,
            warning_count in 0usize..=128,
            error_count in 0usize..=128,
            export_now in 0u32..=200,
            unstable in 0u32..=200,
            fragmented in 0u32..=200,
            carved in 0u32..=200,
            complex_review in 0u32..=200,
            compressed in 0u32..=10,
            snapshot in 0u32..=10,
            journal in 0u32..=10,
            previewable in 0u32..=200,
        ) {
            let files = vec![RecoveredFile::default(); file_count];
            let mut logs: Vec<TechnicalLogEntry> = Vec::new();
            logs.extend((0..warning_count).map(|_| TechnicalLogEntry {
                timestamp_ms: 0,
                level: "warning".into(),
                message: "synthetic".into(),
            }));
            logs.extend((0..error_count).map(|_| TechnicalLogEntry {
                timestamp_ms: 0,
                level: "error".into(),
                message: "synthetic".into(),
            }));

            let counts = AiRecoveryCounts {
                export_now,
                verify_with_preview: 0,
                complex_recovery_review: complex_review,
                review_first: 0,
                unstable,
                deleted: 0,
                carved,
                fragmented,
                previewable,
                compressed,
                snapshot_derived: snapshot,
                journal_derived: journal,
                apfs_catalog_preview_first: 0,
                apfs_catalog_reassembled: 0,
            };

            let score = recovery_brief_confidence(&files, &logs, &counts);
            prop_assert!(
                (28..=94).contains(&score),
                "recovery_brief_confidence produced {} outside [28, 94]",
                score
            );
        }
    }
}
