// ============================================================================
// Récupère — File preview commands
// ============================================================================
// Tauri command handlers that surface the content of recovered files: text,
// hex, image thumbnails, auxiliary payloads (e.g. compressed archive
// members). The heavy lifting lives in the `preview/` module; the helpers
// `build_file_preview`, `build_file_hex_preview`,
// `build_file_auxiliary_preview`, `build_file_auxiliary_hex_preview` and
// `save_file_auxiliary_payload_to_path` are defined in `commands/mod.rs`
// (`pub(super)`) because they still touch a number of file-local utilities.
// ============================================================================

use std::path::{Path, PathBuf};

use crate::imaging;
use crate::preview;
use crate::types::{ByteRun, FileHexPreview, FilePreview};

// Helpers and constants that still live in `commands/mod.rs`. The preview
// builders below reach them through this `super::` import so the bare-name
// call sites that existed before the Pass D extraction keep compiling.
use super::{
    asset_preview_kind, build_hex_preview_lines, file_uses_recovery_image, guess_mime_type,
    infer_auxiliary_asset_preview, is_document_previewable_extension,
    is_text_previewable_extension, resolve_source_path_under_root, HEX_PREVIEW_LINE_WIDTH,
};

/// Hard cap on how many bytes we materialise for video/audio asset previews.
/// Large enough for HTML5 metadata + a few seconds of playback, small enough
/// to keep disk + memory pressure bounded.
const MEDIA_ASSET_MAX_BYTES: u64 = 64 * 1024 * 1024; // 64 MB
const PDF_TEXT_PREVIEW_MAX_BYTES: u64 = 16 * 1024 * 1024; // 16 MB

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_file_preview(scan_id: String, file_id: String) -> Result<FilePreview, String> {
    let session = super::get_session(&scan_id)?;
    let (root_path, file) = {
        let session = crate::commands::state::lock_or_recover(&session, "scan session (preview)");
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

    super::build_file_preview(&scan_id, &root_path, &file)
}

/// Materialise a recovered media file (video or audio) to the preview-asset
/// cache and return the on-disk path. The frontend converts that path to a
/// `tauri://localhost/...` URL via `convertFileSrc` and feeds it to a HTML5
/// `<video>` / `<audio>` element so the user gets a real playable preview
/// straight from the gallery — no full-blown export needed.
#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_file_media_asset(scan_id: String, file_id: String) -> Result<String, String> {
    let session = super::get_session(&scan_id)?;
    let (root_path, file) = {
        let session = crate::commands::state::lock_or_recover(&session, "scan session (preview)");
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

    if !file.preview_available {
        return Err("Preview is not available for this file.".to_string());
    }

    let extension = file.extension.to_ascii_lowercase();

    if super::file_uses_recovery_image(&file) {
        let image_path = file.source_image_path.as_ref().ok_or_else(|| {
            format!(
                "Preview source image is missing for recovery-backed file {}.",
                file.name
            )
        })?;
        let byte_runs = file.byte_runs.as_ref().ok_or_else(|| {
            format!(
                "Preview byte ranges are missing for recovery-backed file {}.",
                file.name
            )
        })?;
        let bytes_to_materialize = file.size_bytes.min(MEDIA_ASSET_MAX_BYTES);
        let asset_path = preview::materialize_asset_preview(
            Path::new(image_path),
            byte_runs,
            bytes_to_materialize,
            &scan_id,
            &file.id,
            &extension,
        )?;
        return Ok(asset_path.to_string_lossy().to_string());
    }

    let source_path = super::resolve_source_path_under_root(&root_path, &file)?;
    if !source_path.exists() {
        return Err(format!(
            "Preview source {} is no longer accessible.",
            source_path.to_string_lossy()
        ));
    }
    let asset_path = preview::materialize_asset_preview_from_path(
        &source_path,
        &scan_id,
        &file.id,
        &extension,
        None,
    )?;
    Ok(asset_path.to_string_lossy().to_string())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_file_hex_preview(
    scan_id: String,
    file_id: String,
    start_offset: u64,
    bytes_to_read: u64,
) -> Result<FileHexPreview, String> {
    super::validate_hex_preview_request(bytes_to_read)?;
    let session = super::get_session(&scan_id)?;
    let (root_path, file) = {
        let session = crate::commands::state::lock_or_recover(&session, "scan session (preview)");
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

    super::build_file_hex_preview(&root_path, &file, start_offset, bytes_to_read)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_file_auxiliary_hex_preview(
    scan_id: String,
    file_id: String,
    auxiliary_kind: String,
    auxiliary_name: Option<String>,
    start_offset: u64,
    bytes_to_read: u64,
) -> Result<FileHexPreview, String> {
    super::validate_hex_preview_request(bytes_to_read)?;
    let session = super::get_session(&scan_id)?;
    let file = {
        let session = crate::commands::state::lock_or_recover(&session, "scan session (preview)");
        session
            .results
            .iter()
            .find(|candidate| candidate.id == file_id)
            .cloned()
            .ok_or_else(|| {
                format!("Recovered file `{file_id}` was not found in scan `{scan_id}`.")
            })?
    };

    super::build_file_auxiliary_hex_preview(
        &file,
        &auxiliary_kind,
        auxiliary_name.as_deref(),
        start_offset,
        bytes_to_read,
    )
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_file_auxiliary_preview(
    scan_id: String,
    file_id: String,
    auxiliary_kind: String,
    auxiliary_name: Option<String>,
) -> Result<FilePreview, String> {
    let session = super::get_session(&scan_id)?;
    let file = {
        let session = crate::commands::state::lock_or_recover(&session, "scan session (preview)");
        session
            .results
            .iter()
            .find(|candidate| candidate.id == file_id)
            .cloned()
            .ok_or_else(|| {
                format!("Recovered file `{file_id}` was not found in scan `{scan_id}`.")
            })?
    };

    super::build_file_auxiliary_preview(&scan_id, &file, &auxiliary_kind, auxiliary_name.as_deref())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn save_file_auxiliary_payload(
    scan_id: String,
    file_id: String,
    auxiliary_kind: String,
    auxiliary_name: Option<String>,
    destination_path: String,
    source_device_path: Option<String>,
) -> Result<String, String> {
    let session = super::get_session(&scan_id)?;
    let file = {
        let session = crate::commands::state::lock_or_recover(&session, "scan session (preview)");
        session
            .results
            .iter()
            .find(|candidate| candidate.id == file_id)
            .cloned()
            .ok_or_else(|| {
                format!("Recovered file `{file_id}` was not found in scan `{scan_id}`.")
            })?
    };

    if let Some(source_device_path) = source_device_path.as_ref() {
        let validation = super::validate_export_destination(
            destination_path.clone(),
            source_device_path.clone(),
        );
        if !validation.is_safe {
            return Err(validation.message);
        }
    }

    let written = super::save_file_auxiliary_payload_to_path(
        &file,
        &auxiliary_kind,
        auxiliary_name.as_deref(),
        Path::new(&destination_path),
    )?;

    tracing::info!(
        "save_file_auxiliary_payload: scan_id={} file_id={} kind={} destination={} bytes={}",
        scan_id,
        file_id,
        auxiliary_kind,
        destination_path,
        written
    );

    Ok(destination_path)
}

// ===========================================================================
// Preview builders moved from `commands/mod.rs` (Sprint 2.1 Pass D).
// Callers that live in this module or in other `commands::*` modules reach
// helpers that stayed behind (`super::build_source_path`,
// `super::file_uses_recovery_image`) through the `super::` path.
// ===========================================================================

use crate::types::RecoveredFile;

#[derive(Debug)]
struct AuxiliaryPayloadRef<'a> {
    target_label: String,
    preview_file_id: String,
    image_path: &'a str,
    total_size_bytes: u64,
    byte_runs: &'a [ByteRun],
}

pub(crate) fn build_file_preview(
    scan_id: &str,
    root_path: &Path,
    file: &RecoveredFile,
) -> Result<FilePreview, String> {
    if !file.preview_available {
        return Ok(unavailable_file_preview(
            file,
            "Preview is not available for this file type in the current build.",
        ));
    }

    let extension = file.extension.to_ascii_lowercase();

    if preview::is_image_previewable(&extension) {
        if file_uses_recovery_image(file) {
            let image_path = file.source_image_path.as_ref().ok_or_else(|| {
                format!(
                    "Preview source image is missing for recovery-backed file {}.",
                    file.name
                )
            })?;
            let byte_runs = file.byte_runs.as_ref().ok_or_else(|| {
                format!(
                    "Preview byte ranges are missing for recovery-backed file {}.",
                    file.name
                )
            })?;
            let asset_path = preview::generate_image_preview(
                Path::new(image_path),
                byte_runs,
                file.size_bytes,
                scan_id,
                &file.id,
                &extension,
            )?;
            return Ok(FilePreview {
                file_id: file.id.clone(),
                kind: "image".into(),
                mime_type: Some(format!(
                    "image/{}",
                    if extension == "jpg" {
                        "jpeg"
                    } else {
                        &extension
                    }
                )),
                text_content: None,
                asset_path: Some(asset_path.to_string_lossy().to_string()),
                truncated: false,
                message: None,
            });
        } else {
            let source_path = resolve_source_path_under_root(root_path, file)?;
            if !source_path.exists() {
                return Err(format!(
                    "Preview source {} is no longer accessible.",
                    source_path.to_string_lossy()
                ));
            }
            let asset_path = preview::materialize_asset_preview_from_path(
                &source_path,
                scan_id,
                &file.id,
                &extension,
                None,
            )?;
            return Ok(FilePreview {
                file_id: file.id.clone(),
                kind: "image".into(),
                mime_type: Some(format!(
                    "image/{}",
                    if extension == "jpg" {
                        "jpeg"
                    } else {
                        &extension
                    }
                )),
                text_content: None,
                asset_path: Some(asset_path.to_string_lossy().to_string()),
                truncated: false,
                message: None,
            });
        }
    }

    if is_document_previewable_extension(&extension) {
        let (content, truncated) = if file_uses_recovery_image(file) {
            let image_path = file.source_image_path.as_ref().ok_or_else(|| {
                format!(
                    "Preview source image is missing for recovery-backed file {}.",
                    file.name
                )
            })?;
            let byte_runs = file.byte_runs.as_ref().ok_or_else(|| {
                format!(
                    "Preview byte ranges are missing for recovery-backed file {}.",
                    file.name
                )
            })?;
            preview::read_document_preview_from_image(
                Path::new(image_path),
                byte_runs,
                file.size_bytes,
                scan_id,
                &file.id,
                &extension,
            )?
        } else {
            let source_path = resolve_source_path_under_root(root_path, file)?;
            preview::read_document_preview_from_path(&source_path, &extension)?
        };

        return Ok(FilePreview {
            file_id: file.id.clone(),
            kind: "text".into(),
            mime_type: file.mime_type.clone(),
            text_content: Some(content),
            asset_path: None,
            truncated,
            message: None,
        });
    }

    if is_text_previewable_extension(&extension) {
        let (content, truncated) = if file_uses_recovery_image(file) {
            let image_path = file.source_image_path.as_ref().ok_or_else(|| {
                format!(
                    "Preview source image is missing for recovery-backed file {}.",
                    file.name
                )
            })?;
            let byte_runs = file.byte_runs.as_ref().ok_or_else(|| {
                format!(
                    "Preview byte ranges are missing for recovery-backed file {}.",
                    file.name
                )
            })?;
            preview::read_text_preview_from_image(
                Path::new(image_path),
                byte_runs,
                file.size_bytes,
            )?
        } else {
            let source_path = resolve_source_path_under_root(root_path, file)?;
            preview::read_text_preview_from_path(&source_path)?
        };

        return Ok(FilePreview {
            file_id: file.id.clone(),
            kind: "text".into(),
            mime_type: file.mime_type.clone(),
            text_content: Some(content),
            asset_path: None,
            truncated,
            message: None,
        });
    }

    // PDF text extraction — try to pull readable text before falling back to raw asset
    if extension == "pdf" {
        let pdf_bytes_result: Result<Option<Vec<u8>>, String> =
            if file.size_bytes > PDF_TEXT_PREVIEW_MAX_BYTES {
                Ok(None)
            } else if file_uses_recovery_image(file) {
                match (file.source_image_path.as_ref(), file.byte_runs.as_ref()) {
                    (Some(image_path), Some(byte_runs)) => {
                        imaging::read_byte_runs(Path::new(image_path), byte_runs, file.size_bytes)
                            .map(Some)
                    }
                    _ => Err("Missing source image or byte runs for PDF preview.".into()),
                }
            } else {
                let source_path = resolve_source_path_under_root(root_path, file)?;
                std::fs::read(&source_path)
                    .map(Some)
                    .map_err(|e| format!("Cannot read PDF source: {e}"))
            };
        if let Ok(Some(bytes)) = pdf_bytes_result {
            if let Ok(text) = preview::extract_pdf_text_preview(&bytes) {
                return Ok(FilePreview {
                    file_id: file.id.clone(),
                    kind: "text".into(),
                    mime_type: Some("application/pdf".into()),
                    text_content: Some(text),
                    asset_path: None,
                    truncated: false,
                    message: None,
                });
            }
        }
        // Fall through to asset preview if text extraction fails
    }

    if let Some(kind) = asset_preview_kind(&extension) {
        let asset_path = if file_uses_recovery_image(file) {
            let image_path = file.source_image_path.as_ref().ok_or_else(|| {
                format!(
                    "Preview source image is missing for recovery-backed file {}.",
                    file.name
                )
            })?;
            let byte_runs = file.byte_runs.as_ref().ok_or_else(|| {
                format!(
                    "Preview byte ranges are missing for recovery-backed file {}.",
                    file.name
                )
            })?;
            preview::materialize_asset_preview(
                Path::new(image_path),
                byte_runs,
                file.size_bytes,
                scan_id,
                &file.id,
                &extension,
            )?
        } else {
            let source_path = resolve_source_path_under_root(root_path, file)?;
            if !source_path.exists() {
                return Err(format!(
                    "Preview source {} is no longer accessible.",
                    source_path.to_string_lossy()
                ));
            }
            preview::materialize_asset_preview_from_path(
                &source_path,
                scan_id,
                &file.id,
                &extension,
                None,
            )?
        };

        return Ok(FilePreview {
            file_id: file.id.clone(),
            kind: kind.into(),
            mime_type: file.mime_type.clone(),
            text_content: None,
            asset_path: Some(asset_path.to_string_lossy().to_string()),
            truncated: false,
            message: None,
        });
    }

    Ok(unavailable_file_preview(
        file,
        "Preview is not implemented for this format yet.",
    ))
}

fn resolve_auxiliary_payload<'a>(
    file: &'a RecoveredFile,
    auxiliary_kind: &str,
    auxiliary_name: Option<&str>,
) -> Result<AuxiliaryPayloadRef<'a>, String> {
    let (target_label, preview_file_id, total_size_bytes, byte_runs) = match auxiliary_kind {
        "resource-fork" => {
            let resource_fork = file.resource_fork.as_ref().ok_or_else(|| {
                format!(
                    "Recovered file `{}` does not expose a reconstructible HFS+ resource fork.",
                    file.name
                )
            })?;
            (
                "HFS+ resource fork".to_string(),
                format!("{}::resource-fork", file.id),
                resource_fork.size_bytes,
                resource_fork.byte_runs.as_slice(),
            )
        }
        "ads" => {
            let stream_name = auxiliary_name.ok_or_else(|| {
                format!(
                    "A named NTFS alternate data stream must be specified for {}.",
                    file.name
                )
            })?;
            let stream = file
                .alternate_data_streams
                .as_ref()
                .and_then(|streams| streams.iter().find(|stream| stream.name == stream_name))
                .ok_or_else(|| {
                    format!(
                        "Recovered file `{}` does not expose a reconstructible NTFS alternate data stream named `{stream_name}`.",
                        file.name
                    )
                })?;
            (
                format!("NTFS ADS `{stream_name}`"),
                format!("{}::ads::{stream_name}", file.id),
                stream.size_bytes,
                stream.byte_runs.as_slice(),
            )
        }
        other => {
            return Err(format!(
                "Unsupported auxiliary kind `{other}` for recovered file `{}`.",
                file.name
            ))
        }
    };

    let image_path = file.source_image_path.as_deref().ok_or_else(|| {
        format!(
            "Recovery image source is missing for the {target_label} of {}.",
            file.name
        )
    })?;

    Ok(AuxiliaryPayloadRef {
        target_label,
        preview_file_id,
        image_path,
        total_size_bytes,
        byte_runs,
    })
}

pub(crate) fn build_file_auxiliary_preview(
    scan_id: &str,
    file: &RecoveredFile,
    auxiliary_kind: &str,
    auxiliary_name: Option<&str>,
) -> Result<FilePreview, String> {
    let payload = resolve_auxiliary_payload(file, auxiliary_kind, auxiliary_name)?;

    let sniff_bytes = preview::read_hex_preview_from_image(
        Path::new(payload.image_path),
        payload.byte_runs,
        payload.total_size_bytes,
        0,
        64,
    )?;

    if let Some((kind, extension, mime_type)) = infer_auxiliary_asset_preview(&sniff_bytes) {
        let asset_path = preview::materialize_asset_preview(
            Path::new(payload.image_path),
            payload.byte_runs,
            payload.total_size_bytes,
            scan_id,
            &payload.preview_file_id,
            extension,
        )?;

        return Ok(FilePreview {
            file_id: payload.preview_file_id,
            kind: kind.into(),
            mime_type: Some(mime_type.into()),
            text_content: None,
            asset_path: Some(asset_path.to_string_lossy().to_string()),
            truncated: false,
            message: None,
        });
    }

    if let Some((content, truncated, extension)) =
        preview::read_document_preview_from_image_if_supported(
            Path::new(payload.image_path),
            payload.byte_runs,
            payload.total_size_bytes,
            scan_id,
            &payload.preview_file_id,
        )?
    {
        return Ok(FilePreview {
            file_id: payload.preview_file_id,
            kind: "text".into(),
            mime_type: guess_mime_type(extension),
            text_content: Some(content),
            asset_path: None,
            truncated,
            message: None,
        });
    }

    let preview = preview::read_text_preview_from_image_if_text_like(
        Path::new(payload.image_path),
        payload.byte_runs,
        payload.total_size_bytes,
    )?;

    match preview {
        Some((content, truncated)) => Ok(FilePreview {
            file_id: payload.preview_file_id,
            kind: "text".into(),
            mime_type: Some("text/plain".into()),
            text_content: Some(content),
            asset_path: None,
            truncated,
            message: None,
        }),
        None => Ok(FilePreview {
            file_id: payload.preview_file_id,
            kind: "unavailable".into(),
            mime_type: None,
            text_content: None,
            asset_path: None,
            truncated: false,
            message: Some(format!(
                "The {} of {} does not look like readable text or a supported preview asset in the current build. Use the hex viewer or export the raw sidecar instead.",
                payload.target_label,
                file.name
            )),
        }),
    }
}

pub(crate) fn build_file_hex_preview(
    root_path: &Path,
    file: &RecoveredFile,
    start_offset: u64,
    bytes_to_read: u64,
) -> Result<FileHexPreview, String> {
    if start_offset > file.size_bytes {
        return Err(format!(
            "Hex preview offset {start_offset} exceeds the available file size {} for {}.",
            file.size_bytes, file.name
        ));
    }

    let bytes = if start_offset == file.size_bytes {
        Vec::new()
    } else if file_uses_recovery_image(file) {
        let image_path = file.source_image_path.as_ref().ok_or_else(|| {
            format!(
                "Hex preview source image is missing for recovery-backed file {}.",
                file.name
            )
        })?;
        let byte_runs = file.byte_runs.as_ref().ok_or_else(|| {
            format!(
                "Hex preview byte ranges are missing for recovery-backed file {}.",
                file.name
            )
        })?;
        preview::read_hex_preview_from_image(
            Path::new(image_path),
            byte_runs,
            file.size_bytes,
            start_offset,
            bytes_to_read,
        )?
    } else {
        let source_path = resolve_source_path_under_root(root_path, file)?;
        preview::read_hex_preview_from_path(&source_path, start_offset, bytes_to_read)?
    };

    Ok(FileHexPreview {
        file_id: file.id.clone(),
        start_offset,
        bytes_read: bytes.len() as u64,
        total_size_bytes: file.size_bytes,
        line_width: HEX_PREVIEW_LINE_WIDTH as u8,
        has_more_before: start_offset > 0,
        has_more_after: start_offset + (bytes.len() as u64) < file.size_bytes,
        lines: build_hex_preview_lines(start_offset, &bytes),
    })
}

pub(crate) fn build_file_auxiliary_hex_preview(
    file: &RecoveredFile,
    auxiliary_kind: &str,
    auxiliary_name: Option<&str>,
    start_offset: u64,
    bytes_to_read: u64,
) -> Result<FileHexPreview, String> {
    let payload = resolve_auxiliary_payload(file, auxiliary_kind, auxiliary_name)?;

    let bytes = preview::read_hex_preview_from_image(
        Path::new(payload.image_path),
        payload.byte_runs,
        payload.total_size_bytes,
        start_offset,
        bytes_to_read,
    )?;

    Ok(FileHexPreview {
        file_id: file.id.clone(),
        start_offset,
        bytes_read: bytes.len() as u64,
        total_size_bytes: payload.total_size_bytes,
        line_width: HEX_PREVIEW_LINE_WIDTH as u8,
        has_more_before: start_offset > 0,
        has_more_after: start_offset.saturating_add(bytes.len() as u64) < payload.total_size_bytes,
        lines: build_hex_preview_lines(start_offset, &bytes),
    })
}

pub(crate) fn save_file_auxiliary_payload_to_path(
    file: &RecoveredFile,
    auxiliary_kind: &str,
    auxiliary_name: Option<&str>,
    destination_path: &Path,
) -> Result<u64, String> {
    let payload = resolve_auxiliary_payload(file, auxiliary_kind, auxiliary_name)?;

    if payload.total_size_bytes == 0 {
        return Err(format!(
            "The {} of {} is empty and cannot be exported as a raw sidecar payload.",
            payload.target_label, file.name
        ));
    }

    if destination_path.is_dir() {
        return Err(format!(
            "The selected auxiliary payload destination {} is a directory.",
            destination_path.to_string_lossy()
        ));
    }

    imaging::materialize_byte_runs(
        Path::new(payload.image_path),
        payload.byte_runs,
        payload.total_size_bytes,
        destination_path,
    )
    .map_err(|error| {
        format!(
            "Unable to save the {} of {} into {}: {}",
            payload.target_label,
            file.name,
            destination_path.to_string_lossy(),
            error
        )
    })
}

fn unavailable_file_preview(file: &RecoveredFile, message: &str) -> FilePreview {
    FilePreview {
        file_id: file.id.clone(),
        kind: "unavailable".into(),
        mime_type: file.mime_type.clone(),
        text_content: None,
        asset_path: None,
        truncated: false,
        message: Some(message.into()),
    }
}
