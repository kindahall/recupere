// ============================================================================
// Shared Rust Types — IPC Contracts
// ============================================================================

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceType {
    #[serde(rename = "hdd")]
    Hdd,
    #[serde(rename = "ssd")]
    Ssd,
    #[serde(rename = "nvme")]
    Nvme,
    #[serde(rename = "usb")]
    Usb,
    #[serde(rename = "sd")]
    Sd,
    #[serde(rename = "external")]
    External,
    #[serde(rename = "image")]
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilesystemType {
    #[serde(rename = "ntfs")]
    Ntfs,
    #[serde(rename = "fat32")]
    Fat32,
    #[serde(rename = "exfat")]
    Exfat,
    #[serde(rename = "apfs")]
    Apfs,
    #[serde(rename = "hfs+")]
    HfsPlus,
    #[serde(rename = "ext4")]
    Ext4,
    #[serde(rename = "unknown")]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskLevel {
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "critical")]
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceStatus {
    #[serde(rename = "healthy")]
    Healthy,
    #[serde(rename = "degraded")]
    Degraded,
    #[serde(rename = "failing")]
    Failing,
    #[serde(rename = "unresponsive")]
    Unresponsive,
    #[serde(rename = "unknown")]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Partition {
    pub id: String,
    pub label: String,
    pub filesystem: FilesystemType,
    pub size_bytes: u64,
    pub start_offset: u64,
    pub mount_path: Option<String>,
    pub is_mounted: bool,
    pub is_bootable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedDevice {
    pub id: String,
    pub name: String,
    pub device_path: String,
    pub device_type: DeviceType,
    pub filesystem: FilesystemType,
    pub capacity_bytes: u64,
    pub used_bytes: u64,
    pub status: DeviceStatus,
    pub risk_level: RiskLevel,
    pub serial: Option<String>,
    pub model: Option<String>,
    pub is_trim_enabled: Option<bool>,
    pub is_encrypted: Option<bool>,
    pub smart_available: Option<bool>,
    pub partitions: Vec<Partition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedRecoverySourceStatus {
    pub display_name: String,
    pub source_path: String,
    pub source_format: String,
    pub logical_size_bytes: u64,
    pub support_tier: String,
    pub support_note: String,
    pub safer_next_step: String,
    pub source_available: bool,
    pub requires_preparation: bool,
    pub prepared: bool,
    pub analysis_path: Option<String>,
    pub cache_path: Option<String>,
    pub cache_size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    pub device_detection: bool,
    pub heuristic_diagnostic: bool,
    pub ai_advisory: bool,
    pub optional_cloud_ai: bool,
    pub scan_engine: bool,
    pub imaging_engine: bool,
    pub results_browser: bool,
    pub export_validation: bool,
    pub export_engine: bool,
    pub technical_logs: bool,
    pub limited_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppBuildInfo {
    pub product_name: String,
    pub bundle_identifier: String,
    pub app_version: String,
    pub package_name: String,
    pub build_profile: String,
    pub operating_system: String,
    pub architecture: String,
    pub target_triple: String,
    pub tauri_runtime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticResult {
    pub device_id: String,
    pub recoverability_score: u8,
    pub loss_type: String,
    pub probable_causes: Vec<String>,
    pub risk_factors: Vec<RiskFactor>,
    pub recommendations: Vec<Recommendation>,
    pub limitations: Vec<String>,
    pub imaging_ready: bool,
    pub imaging_requires_elevation: bool,
    pub imaging_profile: String,
    pub imaging_profile_reason_key: String,
    pub imaging_source_path: Option<String>,
    pub imaging_block_reason: Option<String>,
    pub potential_volumes_inspected: bool,
    pub potential_volumes_notice: Option<String>,
    pub potential_volumes: Vec<PotentialVolume>,
    pub verdict: String,
    pub verdict_details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiAdvisory {
    pub device_id: String,
    pub mode: String,
    pub confidence_score: u8,
    pub summary: String,
    pub rationale: Vec<String>,
    pub cautions: Vec<String>,
    pub next_steps: Vec<String>,
    pub expert_notes: Vec<String>,
    pub recommended_action_type: Option<String>,
    pub recommended_action_title: Option<String>,
    pub cloud_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRecoveryCounts {
    pub export_now: u32,
    pub verify_with_preview: u32,
    pub complex_recovery_review: u32,
    pub review_first: u32,
    pub unstable: u32,
    pub deleted: u32,
    pub carved: u32,
    pub fragmented: u32,
    pub previewable: u32,
    pub compressed: u32,
    pub snapshot_derived: u32,
    pub journal_derived: u32,
    pub apfs_catalog_preview_first: u32,
    pub apfs_catalog_reassembled: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRecoveryBrief {
    pub scan_id: String,
    pub mode: String,
    pub confidence_score: u8,
    pub summary: String,
    pub strategy_title: String,
    pub strategy_reasoning: Vec<String>,
    pub evidence: Vec<String>,
    pub cautions: Vec<String>,
    pub next_steps: Vec<String>,
    pub expert_notes: Vec<String>,
    pub priority_order: Vec<String>,
    pub stability_reason: String,
    pub blocked_by: Vec<String>,
    pub safe_export_strategy: String,
    pub complexity_summary: String,
    pub counts: AiRecoveryCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskFactor {
    pub id: String,
    pub severity: RiskLevel,
    pub title_key: String,
    pub description_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub id: String,
    pub rec_type: String,
    pub priority: u8,
    pub title_key: String,
    pub description_key: String,
    pub is_recommended: bool,
    pub target_potential_volume_id: Option<String>,
    pub target_potential_volume_label: Option<String>,
    pub target_potential_volume_filesystem: Option<String>,
    pub target_potential_volume_start_offset: Option<u64>,
    pub target_potential_volume_size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PotentialVolume {
    pub id: String,
    pub label: String,
    pub filesystem: FilesystemType,
    pub start_offset: u64,
    pub size_bytes: Option<u64>,
    pub confidence_score: u8,
    pub detection_method: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub status: String,
    pub stage: String,
    pub percent_complete: f32,
    pub bytes_scanned: u64,
    pub total_bytes: u64,
    pub files_found: u32,
    pub errors_count: u32,
    pub elapsed_seconds: u64,
    #[serde(default)]
    pub resume_from_bytes: u64,
    #[serde(default)]
    pub unreadable_ranges_count: u64,
    #[serde(default)]
    pub unreadable_bytes: u64,
    #[serde(default)]
    pub rescued_after_retry_bytes: u64,
    #[serde(default)]
    pub retry_passes_completed: u8,
    #[serde(default)]
    pub unreadable_ranges: Vec<ImagingMapRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicalLogEntry {
    pub timestamp_ms: u64,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImagingMapRange {
    pub start_offset: u64,
    pub length: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSessionSummary {
    pub id: String,
    pub device_id: String,
    pub device_name: String,
    #[serde(default)]
    pub source_display_name: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub source_format: Option<String>,
    #[serde(default)]
    pub source_analysis_path: Option<String>,
    #[serde(default)]
    pub source_available: Option<bool>,
    #[serde(default)]
    pub source_requires_preparation: Option<bool>,
    #[serde(default)]
    pub source_prepared: Option<bool>,
    #[serde(default)]
    pub reconstructed_raid_source: bool,
    pub scan_type: String,
    pub started_at_ms: u64,
    pub completed_at_ms: Option<u64>,
    pub status: String,
    pub files_found: u32,
    pub files_recovered: u32,
    pub duration_seconds: u64,
    pub errors: u32,
    #[serde(default)]
    pub bytes_copied: u64,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub resume_from_bytes: u64,
    #[serde(default)]
    pub unreadable_ranges_count: u64,
    #[serde(default)]
    pub unreadable_bytes: u64,
    #[serde(default)]
    pub rescued_after_retry_bytes: u64,
    #[serde(default)]
    pub retry_passes_completed: u8,
    #[serde(default)]
    pub unreadable_ranges: Vec<ImagingMapRange>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecoveredFile {
    pub id: String,
    pub name: String,
    pub path: String,
    pub extension: String,
    pub size_bytes: u64,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub integrity: String,
    pub recovery_score: u8,
    pub recovery_method: String,
    pub preview_available: bool,
    pub mime_type: Option<String>,
    pub expected_size_bytes: Option<u64>,
    pub deleted_at: Option<String>,
    pub start_offset: Option<u64>,
    pub clusters: Option<Vec<u32>>,
    pub byte_runs: Option<Vec<ByteRun>>,
    pub resource_fork: Option<FileFork>,
    pub alternate_data_streams: Option<Vec<NamedFileFork>>,
    pub source_image_path: Option<String>,
    pub is_deleted: bool,
    pub compression_kind: Option<String>,
    pub source_view: Option<String>,
    pub native_auxiliary_kind: Option<String>,
    pub snapshot_xid: Option<u64>,
    pub recovery_complexity: Option<String>,
    pub validator_status: Option<String>,
    pub assembly_segment_count: Option<u8>,
    pub gap_count: Option<u8>,
    #[serde(default)]
    pub journal_derived: bool,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePreview {
    pub file_id: String,
    pub kind: String,
    pub mime_type: Option<String>,
    pub text_content: Option<String>,
    pub asset_path: Option<String>,
    pub truncated: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HexPreviewLine {
    pub offset: u64,
    pub hex: String,
    pub ascii: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHexPreview {
    pub file_id: String,
    pub start_offset: u64,
    pub bytes_read: u64,
    pub total_size_bytes: u64,
    pub line_width: u8,
    pub has_more_before: bool,
    pub has_more_after: bool,
    pub lines: Vec<HexPreviewLine>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ByteRun {
    pub offset: u64,
    pub length: u64,
    #[serde(default)]
    pub zero_fill: bool,
    #[serde(default)]
    pub compression_kind: Option<String>,
    #[serde(default)]
    pub source_view: Option<String>,
}

impl ByteRun {
    pub fn physical(offset: u64, length: u64) -> Self {
        Self {
            offset,
            length,
            zero_fill: false,
            compression_kind: None,
            source_view: None,
        }
    }

    pub fn synthetic_zero_fill(length: u64) -> Self {
        Self {
            offset: 0,
            length,
            zero_fill: true,
            compression_kind: None,
            source_view: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileFork {
    pub size_bytes: u64,
    pub expected_size_bytes: Option<u64>,
    pub byte_runs: Vec<ByteRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedFileFork {
    pub name: String,
    pub size_bytes: u64,
    pub expected_size_bytes: Option<u64>,
    pub byte_runs: Vec<ByteRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportValidation {
    pub is_safe: bool,
    pub available_bytes: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportError {
    pub file_id: String,
    pub file_name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportProgress {
    pub total_files: u32,
    pub exported_files: u32,
    pub total_bytes: u64,
    pub exported_bytes: u64,
    pub current_file: String,
    pub errors: Vec<ExportError>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportSessionSummary {
    pub id: String,
    pub scan_id: String,
    #[serde(default)]
    pub source_device_name: Option<String>,
    #[serde(default)]
    pub source_display_name: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub source_format: Option<String>,
    #[serde(default)]
    pub source_analysis_path: Option<String>,
    #[serde(default)]
    pub source_available: Option<bool>,
    #[serde(default)]
    pub source_requires_preparation: Option<bool>,
    #[serde(default)]
    pub source_prepared: Option<bool>,
    #[serde(default)]
    pub reconstructed_raid_source: bool,
    pub destination_path: String,
    pub started_at_ms: u64,
    pub completed_at_ms: Option<u64>,
    pub status: String,
    pub total_files: u32,
    pub exported_files: u32,
    pub total_bytes: u64,
    pub exported_bytes: u64,
    #[serde(default)]
    pub explicit_selection: bool,
    #[serde(default)]
    pub implicit_preview_first_excluded_count: u32,
    pub errors: Vec<ExportError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalHistoryPurgeResult {
    pub scope: String,
    pub removed_scan_records: u32,
    pub removed_export_records: u32,
    pub scan_archive_deleted: bool,
    pub export_archive_deleted: bool,
    pub live_scan_sessions: u32,
    pub live_export_sessions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartAttribute {
    pub id: u8,
    pub name: String,
    pub value: u8,
    pub worst: u8,
    pub threshold: u8,
    pub raw_value: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartReport {
    pub device_id: String,
    pub overall_health: String,
    pub temperature_celsius: Option<u8>,
    pub power_on_hours: Option<u64>,
    pub reallocated_sectors: Option<u64>,
    pub pending_sectors: Option<u64>,
    pub attributes: Vec<SmartAttribute>,
    pub error_log_count: Option<u32>,
}
