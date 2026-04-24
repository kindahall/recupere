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
use rand::RngCore;

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
        let source_path = super::resolve_source_path_under_root(&root_path, &file)?;
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
pub fn save_repaired_file(
    asset_path: String,
    destination_path: String,
    source_device_path: Option<String>,
) -> Result<String, String> {
    let src = Path::new(&asset_path);
    if !src.exists() {
        return Err("Repaired asset is no longer available — re-run the repair.".into());
    }

    if let Some(source_device_path) = source_device_path.as_ref() {
        let validation = super::validate_export_destination(
            destination_path.clone(),
            source_device_path.clone(),
        );
        if !validation.is_safe {
            return Err(validation.message);
        }
    }

    let destination = Path::new(&destination_path);
    reject_unsafe_repair_destination(destination)?;
    let temp_path = repaired_temp_path(destination)?;
    fs::copy(src, &temp_path).map_err(|e| format!("Cannot write repaired file temp copy: {e}"))?;

    if destination.exists() {
        fs::remove_file(destination)
            .map_err(|e| format!("Cannot replace existing repaired-file destination: {e}"))?;
    }
    fs::rename(&temp_path, destination).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        format!("Cannot finalize repaired file: {e}")
    })?;
    Ok(destination_path)
}

fn reject_unsafe_repair_destination(destination: &Path) -> Result<(), String> {
    let parent = destination.parent().ok_or_else(|| {
        format!(
            "The repaired-file destination {} has no writable parent directory.",
            destination.to_string_lossy()
        )
    })?;
    if !parent.exists() {
        return Err(format!(
            "The repaired-file destination directory {} does not exist.",
            parent.to_string_lossy()
        ));
    }

    if let Ok(metadata) = fs::symlink_metadata(destination) {
        let file_type = metadata.file_type();
        if file_type.is_dir() || is_special_file_type(&file_type) {
            return Err(format!(
                "Refused to write repaired bytes to unsafe destination {}.",
                destination.to_string_lossy()
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_special_file_type(file_type: &fs::FileType) -> bool {
    use std::os::unix::fs::FileTypeExt;
    file_type.is_block_device()
        || file_type.is_char_device()
        || file_type.is_fifo()
        || file_type.is_socket()
}

#[cfg(not(unix))]
fn is_special_file_type(_: &fs::FileType) -> bool {
    false
}

fn repaired_temp_path(destination: &Path) -> Result<PathBuf, String> {
    let parent = destination.parent().ok_or_else(|| {
        format!(
            "The repaired-file destination {} has no writable parent directory.",
            destination.to_string_lossy()
        )
    })?;
    let file_name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("repaired");

    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    let suffix: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(parent.join(format!(".{file_name}.{suffix}.tmp")))
}
