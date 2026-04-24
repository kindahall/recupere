// ============================================================================
// Récupère — File repair commands
// ============================================================================
// Tauri command handlers exposing the `repair/` engine to the frontend.
// `repair_file`        : analyse a recovered file, run the appropriate
//                        repairer, write the repaired bytes to the preview
//                        asset cache, return the report + the asset path.
// `save_repaired_file` : copy a previously-repaired asset to a user-chosen
//                        destination on disk.
// ============================================================================

use std::fs;
use std::path::{Path, PathBuf};

use crate::imaging;
use crate::preview;
use crate::repair::{self, RepairReport};

#[derive(serde::Serialize)]
pub struct RepairCommandResult {
    pub report: RepairReport,
    pub asset_path: Option<String>,
}

const REPAIR_INPUT_MAX_BYTES: u64 = 256 * 1024 * 1024; // 256 MB

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn repair_file(scan_id: String, file_id: String) -> Result<RepairCommandResult, String> {
    let session = super::get_session(&scan_id)?;
    let (root_path, file) = {
        let session = crate::commands::state::lock_or_recover(&session, "scan session (repair)");
        let file = session
            .results
            .iter()
            .find(|candidate| candidate.id == file_id)
            .cloned()
            .ok_or_else(|| {
                format!("Recovered file `{file_id}` was not found in scan `{scan_id}`.")
            })?;
        (PathBuf::from(session.root_path.clone()), file)
    };

    let extension = file.extension.to_ascii_lowercase();
    if extension.is_empty() {
        return Err("Cannot repair a file with no extension.".into());
    }

    // Materialise (or read) the source bytes — capped to 256 MB so we never
    // load multi-gig payloads into memory by accident.
    let bytes_to_read = file.size_bytes.min(REPAIR_INPUT_MAX_BYTES);
    let bytes = if super::file_uses_recovery_image(&file) {
        let image_path = file.source_image_path.as_ref().ok_or_else(|| {
            format!(
                "Repair source image is missing for recovery-backed file {}.",
                file.name
            )
        })?;
        let byte_runs = file.byte_runs.as_ref().ok_or_else(|| {
            format!(
                "Repair byte ranges are missing for recovery-backed file {}.",
                file.name
            )
        })?;
        imaging::read_byte_runs(Path::new(image_path), byte_runs, bytes_to_read)?
    } else {
        let source_path = super::build_source_path(&root_path, &file);
        if !source_path.exists() {
            return Err(format!(
                "Repair source {} is no longer accessible.",
                source_path.to_string_lossy()
            ));
        }
        let mut bytes =
            fs::read(&source_path).map_err(|e| format!("Cannot read repair source: {e}"))?;
        if (bytes.len() as u64) > REPAIR_INPUT_MAX_BYTES {
            bytes.truncate(REPAIR_INPUT_MAX_BYTES as usize);
        }
        bytes
    };

    let outcome = repair::repair(&extension, &bytes)?;

    // Persist the repaired bytes to the preview asset cache so the
    // frontend can immediately re-render the file (image preview, video,
    // download, ...).
    let asset_path =
        preview::preview_asset_path(&scan_id, &file.id, &format!("repaired.{extension}"));
    if let Some(parent) = asset_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&asset_path, &outcome.bytes)
        .map_err(|e| format!("Cannot write repaired asset: {e}"))?;

    tracing::info!(
        "repair_file: scan_id={} file_id={} format={} confidence={:?} \
         original_size={} repaired_size={}",
        scan_id,
        file_id,
        outcome.report.format,
        outcome.report.confidence,
        outcome.report.original_size,
        outcome.report.repaired_size,
    );

    Ok(RepairCommandResult {
        report: outcome.report,
        asset_path: Some(asset_path.to_string_lossy().to_string()),
    })
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn save_repaired_file(asset_path: String, destination_path: String) -> Result<String, String> {
    let src = Path::new(&asset_path);
    if !src.exists() {
        return Err("Repaired asset is no longer available — re-run the repair.".into());
    }
    fs::copy(src, &destination_path).map_err(|e| format!("Cannot save repaired file: {e}"))?;
    Ok(destination_path)
}
