// ============================================================================
// IPC Command Handlers — Honest desktop bridge
// ============================================================================
// SAFETY: All commands are read-only on the source. This build supports:
//   - standalone local read-only imaging
//   - mounted-volume catalog scans for currently accessible files
//   - FAT32, exFAT, NTFS, ext4, HFS+, and APFS deleted-file MVP workflows via a local read-only image
//
// Module organisation: this file is being progressively split per-domain.
// See `docs/refactor-commands.md` for the staged plan. Sub-modules are
// re-exported via `pub use` so that `lib.rs::generate_handler!` and any
// other caller can keep referencing `commands::<fn>` unchanged.
// ============================================================================

mod ai;
mod audit;
mod device;
mod export;
mod file_preview;
mod filesystem_memory_cmd;
mod imaging_cmd;
mod license;
mod repair_cmd;
mod runtime;
mod scan;
mod state;
mod support_bundle;
mod validation;
pub use ai::*;
pub use audit::*;
pub use device::*;
pub use export::*;
pub use file_preview::*;
pub use filesystem_memory_cmd::*;
pub use imaging_cmd::*;
pub use license::*;
pub use repair_cmd::*;
pub use runtime::*;
pub use scan::*;
pub(crate) use validation::{normalize_conflict_strategy, normalize_scan_type};

// Helpers moved to `commands/export.rs` (Sprint 2.1 slice `export_helpers`,
// 2026-04-17). Re-exposed at `super::` so `repair_cmd.rs`, `file_preview.rs`
// and `scan.rs` keep reaching them by their bare names through the existing
// `super::<fn>` call sites. `format_hex_column`, `resource_fork_sidecar_path`,
// `sanitize_sidecar_component`, `alternate_data_stream_sidecar_path` and
// `unique_target_path` are private helpers of `export.rs` and are not
// re-exported.
pub(super) use export::{
    asset_preview_kind, build_hex_preview_lines, build_source_path, file_uses_recovery_image,
    infer_auxiliary_asset_preview, is_document_previewable_extension,
    is_text_previewable_extension,
};

pub(super) use scan::{
    best_supported_potential_volume, guess_mime_type, guided_supported_potential_volume_candidate,
    run_deleted_apfs_scan, run_deleted_exfat_scan, run_deleted_ext4_scan, run_deleted_fat32_scan,
    run_deleted_hfsplus_scan, run_deleted_ntfs_scan, run_inventory_scan, run_potential_volume_scan,
    run_signature_carving_scan, supported_potential_volume_filesystem,
};

// Additional export + scan helpers only referenced from the inline `tests`
// module below. Kept out of the sibling re-export block so `cargo check --lib`
// does not warn about them being unused in non-test builds.
#[cfg(test)]
use export::{
    relative_dir_from_display_path, resolve_target_path, safe_export_file_name,
    verify_exported_file,
};
#[cfg(test)]
use scan::compute_progress;

#[cfg(test)]
use crate::types::*;
#[cfg(test)]
use std::{fs, path::Path};

pub(super) const HEX_PREVIEW_LINE_WIDTH: usize = 16;

// File preview builders moved to `commands/file_preview.rs`
// (Sprint 2.1 Pass D). Re-exposed for the few prod/test sites in
// `mod.rs` that still call them under their bare names.
pub(super) use file_preview::{
    build_file_auxiliary_hex_preview, build_file_auxiliary_preview, build_file_hex_preview,
    build_file_preview, save_file_auxiliary_payload_to_path,
};

// Runtime capabilities + build identity moved to `commands/runtime.rs`
// (Sprint 2.1 Pass A). Re-exposed here so the rest of `mod.rs` (support
// bundle builder, recovery report) and neighbouring modules (`scan.rs`,
// `ai.rs`) keep calling them by their bare names via
// `super::app_build_info()` etc. The `APP_PRODUCT_NAME` /
// `APP_BUNDLE_IDENTIFIER` constants are only referenced from the test
// module, so they are imported there (see `tests::`).
pub(super) use runtime::{app_build_info, runtime_capabilities};

// Heuristic diagnostic builder moved to `commands/device.rs` (Sprint 2.1
// Pass B). Re-exposed so the support-bundle builder, `generate_recovery_report`
// and `ai.rs` keep calling `build_diagnostic(...)` via their existing
// `super::` / bare-name path.
pub(super) use device::{build_diagnostic, filesystem_label};

// Imaging helpers (ImagingSourcePlan, resolve_imaging_source_plan, imaging
// profile/artifact helpers, progress/report readers, macOS privileged recovery
// orchestrator including `create_read_only_image_with_optional_elevation`)
// moved to `commands/imaging_cmd/helpers.rs` (Sprint 5, Chantier 76 slice
// `imaging_helpers`). Re-exposed at `super::` so the scan workers still in
// this file, sibling modules (`scan.rs`, `device.rs`), and the inline tests
// keep reaching them via their bare names.
pub(super) use support_bundle::build_support_bundle_archive_bytes;

pub(super) use imaging_cmd::helpers::{
    append_imaging_artifact_issue_logs, apply_imaging_artifact_issue_metrics,
    apply_imaging_artifact_session_details, create_read_only_image_with_optional_elevation,
    imaging_profile_for_session, inspect_potential_volumes_for_diagnostic,
    recommended_imaging_profile, recommended_imaging_profile_reason_key,
    resolve_imaging_source_plan, resolved_imaging_source_path, ImagingSourcePlan,
};

// Session registries, lifecycle, persistence, summaries, logs and
// timestamp utilities moved to `commands/state.rs` (Sprint 2.1 Pass C).
pub(super) use state::{
    append_export_log, append_scan_log, build_export_summary, build_scan_summary,
    clear_local_history_at, elapsed_seconds, export_history_storage_path, export_sessions,
    fail_scan_session, finalize_cancelled_scan, get_session, initialize_scan_session,
    load_persisted_export_archive, load_persisted_export_record, load_persisted_scan_archive,
    load_persisted_scan_record, persist_export_session, persist_scan_session, push_technical_log,
    request_scan_cancel, request_scan_pause, request_scan_resume, scan_history_storage_path,
    scan_sessions, unix_timestamp_ms, update_progress, wait_for_scan_permission,
    write_binary_file_to_path, write_text_report_to_path,
};

// `get_runtime_capabilities` and `get_app_build_info` are defined in
// `commands/runtime.rs` and re-exported via `pub use runtime::*;`.
// `get_devices`, `get_diagnostic`, `get_smart_report`, `detect_raid_metadata`
// and `get_encryption_info` are defined in `commands/device.rs` and
// re-exported here for the desktop handler. Sensitive unlock helpers also live
// in that module, but are intentionally not registered in the Tauri handler.

// AI commands (`get_ai_advisory`, `get_scan_ai_brief`, `ai_autopilot_scan`,
// `get_gemma_*`, `start_gemma_pull`, `get_gemma_pull_progress`,
// `classify_scan_files`, `predict_scan_recovery`, `generate_narrative_report`,
// `suggest_file_reconstruction`, `build_cloud_ai_prompt`, `run_gemma_analysis`,
// `chat_with_ai`, `smart_select_by_category`, `search_file_by_name`) are
// defined in `commands/ai.rs` and re-exported via `pub use ai::*;`.

// `start_imaging` is defined in `commands/imaging_cmd.rs`.

// Scan commands (`start_scan`, `start_potential_volume_scan`,
// `get_scan_progress`, `get_results`, `pause_scan`, `resume_scan`,
// `cancel_scan`, `get_scan_history`, `get_scan_logs`,
// `generate_imaging_session_report`) are defined in `commands/scan.rs`.
//
// File preview commands (`get_file_preview`, `get_file_hex_preview`,
// `get_file_auxiliary_preview`, `get_file_auxiliary_hex_preview`,
// `save_file_auxiliary_payload`) are defined in `commands/file_preview.rs`.
// Export commands (`validate_export_destination`,
// `save_technical_timeline_report`, `save_support_bundle`,
// `get_export_progress`, `get_export_logs`, `get_export_history`,
// `clear_local_history`) are defined in `commands/export.rs`.

// `start_export` is now defined in `commands/export.rs`.

// `normalize_scan_type` moved to `commands/validation.rs` (plan I5 slice 1).

// Export helpers (select_files_for_export, file_uses_recovery_image,
// infer_auxiliary_asset_preview, build_hex_preview_lines, format_hex_column,
// update_export_progress, push_export_error, build_source_path,
// estimated_export_payload_bytes, resource_fork_sidecar_path,
// sanitize_sidecar_component, alternate_data_stream_sidecar_path,
// is_text_previewable_extension, is_document_previewable_extension,
// asset_preview_kind, export_recovered_file, export_resource_fork_sidecar,
// export_alternate_data_stream_sidecars, relative_dir_from_display_path,
// resolve_target_path, unique_target_path, verify_exported_file,
// verify_reconstructed_export) moved to `commands/export.rs` — Sprint 2.1
// slice `export_helpers` (2026-04-17). Re-exposed below via
// `pub(super) use export::{...};` so sibling modules (`repair_cmd.rs`,
// `file_preview.rs`, `scan.rs`) keep reaching them through the existing
// `super::<fn>` paths.

// Scan helpers (register_scan_error, compute_progress, display_parent_path,
// guess_mime_type, is_previewable_extension) moved to `commands/scan.rs`
// Sprint 2.1 slice `scan_helpers` (2026-04-17). Re-exposed at `super::` via
// `pub(super) use scan::{...};` below so the scan workers that still live in
// this file, the #[cfg(test)] block, and `file_preview.rs` keep reaching them
// through their bare names.

// `build_diagnostic` and its local helpers (filesystem_label,
// deleted_entry_recovery_label, deleted_entry_recommendation_type,
// device_type_label) moved to `commands/device.rs` in Sprint 2.1 Pass B.
// The imaging/volume probes they rely on (resolved_imaging_source_path,
// resolve_imaging_source_plan, recommended_imaging_profile*,
// inspect_potential_volumes_for_diagnostic) now live in
// `commands/imaging_cmd/helpers.rs` and are reached via `super::` from
// `device.rs` through the `pub(super) use imaging_cmd::helpers::{...}`
// re-export above. The potential-volume ranking helpers
// (supported_potential_volume_filesystem, best_supported_potential_volume,
// guided_supported_potential_volume_candidate) still live in this file.

// Device metadata helpers such as `detect_raid_metadata` and
// `get_encryption_info` are defined in `commands/device.rs`.

// All AI commands (ai_autopilot_scan, get_gemma_*, classify_scan_files,
// predict_scan_recovery, generate_narrative_report,
// suggest_file_reconstruction, build_cloud_ai_prompt, run_gemma_analysis,
// chat_with_ai, smart_select_by_category, search_file_by_name) and the
// AI helpers (build_ai_file_input(s), require_gemma_ready) are defined in
// `commands/ai.rs` and re-exported via `pub use ai::*;`.
// `get_smart_report` is now defined in `commands/device.rs`.
// `get_audit_trail` is now defined in `commands/audit.rs` and re-exported
// at the top of this module via `pub use audit::*;`.

#[cfg(test)]
mod tests;
