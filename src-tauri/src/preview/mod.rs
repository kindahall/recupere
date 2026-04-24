#![allow(dead_code)]
use crate::{imaging, types::ByteRun};
use quick_xml::{events::Event, Reader};
use std::{
    env,
    fs::{self, File},
    io::{BufReader, Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use zip::{result::ZipError, ZipArchive};

const TEXT_PREVIEW_LIMIT_BYTES: u64 = 64 * 1024;
const HEX_PREVIEW_MAX_BYTES: u64 = 1024;
const XLSX_PREVIEW_MAX_SHEETS: usize = 3;
const PPTX_PREVIEW_MAX_SLIDES: usize = 6;

/// Hard ceiling on the size of an image we are willing to decode for preview
/// generation. Anti-DoS guard: a malicious or corrupted PNG/WEBP can be a
/// few MB on disk and decompress to gigabytes of pixels, OOM-ing the app.
const MAX_PREVIEW_IMAGE_BYTES: usize = 50 * 1024 * 1024; // 50 MB

/// Hard ceiling on the resolution of a decoded image. The `image` crate
/// allocates `width * height * bytes_per_pixel` so a 32k × 32k image alone
/// would request ~4 GB of RAM.
const MAX_PREVIEW_IMAGE_DIMENSION: u32 = 16384;

/// Default ceiling applied when a caller materialises an on-disk preview from
/// a source path without specifying its own per-file limit. Callers that need
/// a different ceiling can override it via
/// [`materialize_asset_preview_from_path_with_limit`], which takes the cap as
/// an explicit argument. The value is tuned so large video/audio headers
/// still decode but a 10 GB disk-image doesn't end up in `$TEMP`.
pub const DEFAULT_MATERIALIZE_MAX_BYTES: u64 = 128 * 1024 * 1024; // 128 MB

/// Ceiling on the aggregate size of the preview workspace directory. Once
/// exceeded, the LRU sweep in [`enforce_preview_workspace_quota`] evicts the
/// oldest materialised assets (ordered by modified time) until the directory
/// is back under this threshold. Env-var override
/// `RECUPERE_PREVIEW_QUOTA_BYTES` is honoured so tests can use a tiny quota
/// without patching the constant.
pub const PREVIEW_WORKSPACE_QUOTA_BYTES: u64 = 500 * 1024 * 1024; // 500 MB

fn preview_workspace_quota_bytes() -> u64 {
    env::var("RECUPERE_PREVIEW_QUOTA_BYTES")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(PREVIEW_WORKSPACE_QUOTA_BYTES)
}

/// Decode an image from an in-memory byte slice, enforcing both a byte-size
/// guard (input) and a dimension guard (decoded output) to prevent
/// decompression-bomb DoS. Returns the decoded `DynamicImage` on success.
fn decode_image_with_limits(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    if bytes.len() > MAX_PREVIEW_IMAGE_BYTES {
        return Err(format!(
            "Preview skipped: image exceeds {} MB",
            MAX_PREVIEW_IMAGE_BYTES / (1024 * 1024)
        ));
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("Cannot detect image format: {e}"))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_PREVIEW_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_PREVIEW_IMAGE_DIMENSION);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|e| format!("Cannot decode image: {e}"))
}

pub fn read_text_preview_from_path(path: &Path) -> Result<(String, bool), String> {
    let file = File::open(path).map_err(|error| {
        format!(
            "Unable to open preview source {}: {error}",
            path.to_string_lossy()
        )
    })?;
    read_text_preview_from_reader(BufReader::new(file))
}

pub fn read_text_preview_from_image(
    image_path: &Path,
    byte_runs: &[ByteRun],
    bytes_to_materialize: u64,
) -> Result<(String, bool), String> {
    let bytes_to_read = bytes_to_materialize.min(TEXT_PREVIEW_LIMIT_BYTES + 1);
    let bytes = imaging::read_byte_runs(image_path, byte_runs, bytes_to_read)?;
    Ok(decode_text_preview_bytes(&bytes))
}

pub fn read_text_preview_from_image_if_text_like(
    image_path: &Path,
    byte_runs: &[ByteRun],
    bytes_to_materialize: u64,
) -> Result<Option<(String, bool)>, String> {
    let bytes_to_read = bytes_to_materialize.min(TEXT_PREVIEW_LIMIT_BYTES + 1);
    let bytes = imaging::read_byte_runs(image_path, byte_runs, bytes_to_read)?;
    if !looks_like_text_preview_bytes(&bytes) {
        return Ok(None);
    }
    Ok(Some(decode_text_preview_bytes(&bytes)))
}

pub fn read_document_preview_from_path(
    path: &Path,
    extension: &str,
) -> Result<(String, bool), String> {
    let file = File::open(path).map_err(|error| {
        format!(
            "Unable to open preview source {}: {error}",
            path.to_string_lossy()
        )
    })?;
    read_document_preview_from_reader(file, extension, &path.to_string_lossy())
}

pub fn read_document_preview_from_image(
    image_path: &Path,
    byte_runs: &[ByteRun],
    bytes_to_materialize: u64,
    scan_id: &str,
    file_id: &str,
    extension: &str,
) -> Result<(String, bool), String> {
    let preview_path = materialize_asset_preview(
        image_path,
        byte_runs,
        bytes_to_materialize,
        scan_id,
        file_id,
        extension,
    )?;

    read_document_preview_from_path(&preview_path, extension)
}

pub fn read_document_preview_from_image_if_supported(
    image_path: &Path,
    byte_runs: &[ByteRun],
    bytes_to_materialize: u64,
    scan_id: &str,
    file_id: &str,
) -> Result<Option<(String, bool, &'static str)>, String> {
    let preview_path = materialize_asset_preview(
        image_path,
        byte_runs,
        bytes_to_materialize,
        scan_id,
        file_id,
        "zip",
    )?;
    let Some(extension) = detect_document_preview_extension_from_path(&preview_path)? else {
        return Ok(None);
    };

    let (content, truncated) = read_document_preview_from_path(&preview_path, extension)?;
    Ok(Some((content, truncated, extension)))
}

pub fn materialize_asset_preview(
    image_path: &Path,
    byte_runs: &[ByteRun],
    bytes_to_materialize: u64,
    scan_id: &str,
    file_id: &str,
    extension: &str,
) -> Result<PathBuf, String> {
    let capped = bytes_to_materialize.min(DEFAULT_MATERIALIZE_MAX_BYTES);
    let preview_path = preview_asset_path(scan_id, file_id, extension);
    imaging::materialize_byte_runs(image_path, byte_runs, capped, &preview_path)?;
    enforce_preview_workspace_quota(&preview_path)?;
    Ok(preview_path)
}

/// Materialise a file from disk into the preview workspace, applying a
/// per-file byte ceiling. When `max_bytes` is `None` we apply
/// [`DEFAULT_MATERIALIZE_MAX_BYTES`] — no caller is ever allowed to copy a
/// source file verbatim into the preview workspace, so the `None` arm cannot
/// blow past the quota. After materialisation we enforce
/// [`PREVIEW_WORKSPACE_QUOTA_BYTES`] via LRU eviction; if the new asset would
/// itself exceed the quota, the LRU sweep deletes the newly-written file too
/// and surfaces an error so the caller can degrade (e.g. skip the preview).
pub fn materialize_asset_preview_from_path(
    source_path: &Path,
    scan_id: &str,
    file_id: &str,
    extension: &str,
    max_bytes: Option<u64>,
) -> Result<PathBuf, String> {
    let effective_limit = max_bytes.unwrap_or(DEFAULT_MATERIALIZE_MAX_BYTES);

    let preview_path = preview_asset_path(scan_id, file_id, extension);
    if let Some(parent) = preview_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Unable to create preview workspace {}: {error}",
                parent.to_string_lossy()
            )
        })?;
    }

    let mut source = File::open(source_path).map_err(|error| {
        format!(
            "Unable to open preview source {}: {error}",
            source_path.to_string_lossy()
        )
    })?;
    let mut destination = File::create(&preview_path).map_err(|error| {
        format!(
            "Unable to create preview asset {}: {error}",
            preview_path.to_string_lossy()
        )
    })?;

    let copy_result = {
        let mut limited = (&mut source).take(effective_limit);
        std::io::copy(&mut limited, &mut destination)
    };

    if let Err(error) = copy_result {
        // Best-effort cleanup so a partial write doesn't leak into the
        // workspace — the LRU would evict it eventually, but we'd rather
        // leave the quota untouched on failures.
        let _ = fs::remove_file(&preview_path);
        return Err(format!(
            "Unable to materialize preview asset {}: {error}",
            preview_path.to_string_lossy()
        ));
    }

    destination.flush().map_err(|error| {
        let _ = fs::remove_file(&preview_path);
        format!(
            "Unable to finalize preview asset {}: {error}",
            preview_path.to_string_lossy()
        )
    })?;

    enforce_preview_workspace_quota(&preview_path)?;
    Ok(preview_path)
}

/// Walk the preview workspace directory and delete the oldest files (by
/// `mtime`) until the aggregate size is back under the configured quota. The
/// file at `fresh_path` is protected from eviction unless the quota is so
/// small that even the just-written asset doesn't fit — in which case we
/// delete it and return an error so the caller can degrade.
pub(crate) fn enforce_preview_workspace_quota(fresh_path: &Path) -> Result<(), String> {
    let workspace = preview_workspace_dir();
    let quota = preview_workspace_quota_bytes();
    enforce_preview_workspace_quota_at(&workspace, fresh_path, quota)
}

/// Test- and bench-friendly variant of
/// [`enforce_preview_workspace_quota`] that takes the workspace directory
/// and the quota as explicit arguments. The public entrypoint reads both
/// from module-level state (env var + constant) and delegates here. Public
/// so the `preview_quota_budget` criterion benchmark can drive it without
/// leaking env-var state across threads.
pub fn enforce_preview_workspace_quota_at(
    workspace: &Path,
    fresh_path: &Path,
    quota: u64,
) -> Result<(), String> {
    if quota == 0 {
        return Ok(());
    }

    let mut entries: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
    let read_dir = match fs::read_dir(workspace) {
        Ok(iter) => iter,
        Err(_) => return Ok(()),
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !metadata.is_file() {
            continue;
        }
        let mtime = metadata
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        entries.push((path, metadata.len(), mtime));
    }

    let total: u64 = entries.iter().map(|(_, size, _)| size).sum();
    if total <= quota {
        return Ok(());
    }

    // Oldest first so the LRU sweep evicts stale assets before recent ones.
    entries.sort_by(|a, b| a.2.cmp(&b.2));

    let mut remaining = total;
    let fresh_canonical = fresh_path.canonicalize().ok();
    for (path, size, _) in entries {
        if remaining <= quota {
            break;
        }
        // Skip the freshly-written asset unless it's the ONLY entry, in which
        // case we have to evict it to respect the quota.
        let is_fresh = fresh_canonical
            .as_ref()
            .map(|canon| path.canonicalize().ok().as_ref() == Some(canon))
            .unwrap_or_else(|| path == fresh_path);
        if is_fresh {
            continue;
        }
        if fs::remove_file(&path).is_ok() {
            remaining = remaining.saturating_sub(size);
        }
    }

    if remaining > quota {
        // The fresh file alone exceeds the quota. Remove it and tell the
        // caller — callers that care can surface a "skip preview" message.
        let _ = fs::remove_file(fresh_path);
        return Err(format!(
            "Preview skipped: asset would exceed the workspace quota of {} MB.",
            (quota / (1024 * 1024)).max(1)
        ));
    }

    Ok(())
}

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "tiff", "tif", "webp", "ico",
];

pub fn is_image_previewable(extension: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&extension.to_lowercase().as_str())
}

pub fn generate_image_preview(
    image_path: &Path,
    byte_runs: &[ByteRun],
    bytes_to_materialize: u64,
    scan_id: &str,
    file_id: &str,
    extension: &str,
) -> Result<PathBuf, String> {
    // Materialize bytes to temp file first
    let preview_path = preview_asset_path(scan_id, file_id, extension);
    imaging::materialize_byte_runs(image_path, byte_runs, bytes_to_materialize, &preview_path)?;

    // Try to decode for validation only — apply the same DoS guards as the
    // thumbnail path. We still return the raw asset path on decode failure
    // (the front-end will fall back to a hex preview).
    let bytes = std::fs::read(&preview_path)
        .map_err(|e| format!("Cannot read materialized preview: {e}"))?;
    let _ = decode_image_with_limits(&bytes);
    Ok(preview_path)
}

pub fn generate_image_thumbnail(
    image_path: &Path,
    byte_runs: &[ByteRun],
    bytes_to_materialize: u64,
    scan_id: &str,
    file_id: &str,
) -> Result<PathBuf, String> {
    let temp_path = preview_asset_path(scan_id, file_id, "raw");
    imaging::materialize_byte_runs(image_path, byte_runs, bytes_to_materialize, &temp_path)?;

    let bytes = std::fs::read(&temp_path)
        .map_err(|e| format!("Cannot read materialized file for thumbnail: {e}"))?;
    let _ = std::fs::remove_file(&temp_path);

    let img = decode_image_with_limits(&bytes)?;
    let thumbnail = img.thumbnail(128, 128);
    let thumb_path = preview_asset_path(scan_id, file_id, "thumb.png");
    thumbnail
        .save(&thumb_path)
        .map_err(|e| format!("Cannot save thumbnail: {e}"))?;
    Ok(thumb_path)
}

/// Extract readable text from a PDF file for preview purposes.
/// Uses a simple heuristic: finds text between BT/ET markers and
/// extracts Tj/TJ operator strings.
pub fn extract_pdf_text_preview(bytes: &[u8]) -> Result<String, String> {
    let content = String::from_utf8_lossy(bytes);
    let mut text = String::new();
    let mut in_text_block = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "BT" {
            in_text_block = true;
            continue;
        }
        if trimmed == "ET" {
            in_text_block = false;
            if !text.is_empty() {
                text.push('\n');
            }
            continue;
        }
        if in_text_block {
            // Extract text from Tj operator: (text) Tj
            if let Some(start) = trimmed.find('(') {
                if let Some(end) = trimmed.rfind(')') {
                    if end > start {
                        text.push_str(&trimmed[start + 1..end]);
                        text.push(' ');
                    }
                }
            }
            // Extract text from TJ operator: [(text)] TJ
            if trimmed.ends_with("TJ") || trimmed.ends_with("Tj") {
                let bracket_content = trimmed.replace("TJ", "").replace("Tj", "");
                for segment in bracket_content.split('(') {
                    if let Some(end) = segment.find(')') {
                        text.push_str(&segment[..end]);
                    }
                }
            }
        }
    }

    let result = text.trim().to_string();
    if result.is_empty() {
        Err("No extractable text found in PDF.".into())
    } else {
        Ok(result
            .chars()
            .take(TEXT_PREVIEW_LIMIT_BYTES as usize)
            .collect())
    }
}

pub fn read_hex_preview_from_path(
    path: &Path,
    start_offset: u64,
    requested_bytes: u64,
) -> Result<Vec<u8>, String> {
    let mut file = File::open(path).map_err(|error| {
        format!(
            "Unable to open preview source {}: {error}",
            path.to_string_lossy()
        )
    })?;
    file.seek(SeekFrom::Start(start_offset)).map_err(|error| {
        format!(
            "Unable to seek preview source {}: {error}",
            path.to_string_lossy()
        )
    })?;

    let bytes_to_read = requested_bytes.clamp(1, HEX_PREVIEW_MAX_BYTES);
    let mut limited = file.take(bytes_to_read);
    let mut bytes = Vec::with_capacity(bytes_to_read as usize);
    limited.read_to_end(&mut bytes).map_err(|error| {
        format!(
            "Unable to read preview bytes from {}: {error}",
            path.to_string_lossy()
        )
    })?;
    Ok(bytes)
}

pub fn read_hex_preview_from_image(
    image_path: &Path,
    byte_runs: &[ByteRun],
    total_size_bytes: u64,
    start_offset: u64,
    requested_bytes: u64,
) -> Result<Vec<u8>, String> {
    if start_offset > total_size_bytes {
        return Err(format!(
            "Hex preview offset {start_offset} exceeds the available file size {total_size_bytes}."
        ));
    }

    let bytes_to_read = requested_bytes
        .clamp(1, HEX_PREVIEW_MAX_BYTES)
        .min(total_size_bytes.saturating_sub(start_offset));

    imaging::read_byte_runs_range(image_path, byte_runs, start_offset, bytes_to_read)
}

pub fn preview_asset_path(scan_id: &str, file_id: &str, extension: &str) -> PathBuf {
    let sanitized_scan_id = sanitize_segment(scan_id);
    let sanitized_file_id = sanitize_segment(file_id);
    let sanitized_extension = sanitize_segment(extension);
    let file_name = if sanitized_extension.is_empty() {
        format!("{sanitized_scan_id}-{sanitized_file_id}.preview")
    } else {
        format!("{sanitized_scan_id}-{sanitized_file_id}.{sanitized_extension}")
    };
    preview_workspace_dir().join(file_name)
}

fn preview_workspace_dir() -> PathBuf {
    if let Some(path) = env::var_os("RECUPERE_PREVIEW_PATH") {
        return PathBuf::from(path);
    }

    if cfg!(test) {
        return env::temp_dir()
            .join(format!("recupere-test-{}", std::process::id()))
            .join("previews");
    }

    env::temp_dir().join("recupere-workspace").join("previews")
}

fn read_document_preview_from_reader(
    reader: impl Read + Seek,
    extension: &str,
    source_label: &str,
) -> Result<(String, bool), String> {
    let mut archive = ZipArchive::new(reader).map_err(|error| {
        format!("Unable to open {extension} preview source {source_label}: {error}")
    })?;

    let content = match extension {
        "docx" => extract_docx_preview(&mut archive)?,
        "xlsx" => extract_xlsx_preview(&mut archive)?,
        "pptx" => extract_pptx_preview(&mut archive)?,
        _ => {
            return Err(format!(
                "Document preview is not implemented for the extension {extension}."
            ))
        }
    };

    Ok(limit_preview_text(&content))
}

fn detect_document_preview_extension_from_path(
    path: &Path,
) -> Result<Option<&'static str>, String> {
    let file = File::open(path).map_err(|error| {
        format!(
            "Unable to open preview source {}: {error}",
            path.to_string_lossy()
        )
    })?;
    detect_document_preview_extension_from_reader(file)
}

fn detect_document_preview_extension_from_reader(
    reader: impl Read + Seek,
) -> Result<Option<&'static str>, String> {
    let archive = match ZipArchive::new(reader) {
        Ok(archive) => archive,
        Err(ZipError::InvalidArchive(_))
        | Err(ZipError::UnsupportedArchive(_))
        | Err(ZipError::FileNotFound) => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Unable to inspect preview archive contents for document detection: {error}"
            ))
        }
    };

    let mut has_xlsx = false;
    let mut has_pptx = false;
    for name in archive.file_names() {
        if name == "word/document.xml" {
            return Ok(Some("docx"));
        }
        if name == "xl/workbook.xml"
            || (name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml"))
        {
            has_xlsx = true;
        }
        if name == "ppt/presentation.xml"
            || (name.starts_with("ppt/slides/slide") && name.ends_with(".xml"))
        {
            has_pptx = true;
        }
    }

    if has_xlsx {
        return Ok(Some("xlsx"));
    }
    if has_pptx {
        return Ok(Some("pptx"));
    }

    Ok(None)
}

fn read_text_preview_from_reader(reader: impl Read) -> Result<(String, bool), String> {
    let mut buffer = Vec::with_capacity((TEXT_PREVIEW_LIMIT_BYTES + 1) as usize);
    let mut limited = reader.take(TEXT_PREVIEW_LIMIT_BYTES + 1);
    limited
        .read_to_end(&mut buffer)
        .map_err(|error| format!("Unable to read preview text bytes: {error}"))?;
    Ok(decode_text_preview_bytes(&buffer))
}

fn decode_text_preview_bytes(bytes: &[u8]) -> (String, bool) {
    let truncated = bytes.len() as u64 > TEXT_PREVIEW_LIMIT_BYTES;
    let content_bytes = if truncated {
        &bytes[..TEXT_PREVIEW_LIMIT_BYTES as usize]
    } else {
        bytes
    };
    let content = String::from_utf8_lossy(content_bytes).to_string();
    (content, truncated)
}

fn looks_like_text_preview_bytes(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }

    let content_bytes = if bytes.len() as u64 > TEXT_PREVIEW_LIMIT_BYTES {
        &bytes[..TEXT_PREVIEW_LIMIT_BYTES as usize]
    } else {
        bytes
    };

    if content_bytes.contains(&0) {
        return false;
    }

    let suspicious = content_bytes
        .iter()
        .filter(|byte| matches!(**byte, 0x01..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F))
        .count();
    let suspicious_ratio = suspicious as f32 / content_bytes.len() as f32;
    suspicious_ratio <= 0.10
}

fn extract_docx_preview<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<String, String> {
    let document_xml = read_archive_entry_bytes(archive, "word/document.xml")?
        .ok_or_else(|| "Unable to build DOCX preview: word/document.xml is missing.".to_string())?;

    let preview = extract_docx_text_from_xml(&document_xml)?;
    if preview.trim().is_empty() {
        return Err("DOCX preview did not expose readable text content.".into());
    }

    Ok(preview)
}

fn extract_xlsx_preview<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<String, String> {
    let shared_strings = match read_archive_entry_bytes(archive, "xl/sharedStrings.xml")? {
        Some(bytes) => parse_xlsx_shared_strings(&bytes)?,
        None => Vec::new(),
    };

    let mut sheet_names: Vec<String> = archive
        .file_names()
        .filter(|name| name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml"))
        .map(str::to_string)
        .collect();
    sheet_names.sort();

    if sheet_names.is_empty() {
        return Err("Unable to build XLSX preview: no worksheet XML was found.".into());
    }

    let mut sections = Vec::new();
    for sheet_name in sheet_names.iter().take(XLSX_PREVIEW_MAX_SHEETS) {
        let Some(sheet_xml) = read_archive_entry_bytes(archive, sheet_name)? else {
            continue;
        };
        let sheet_preview = extract_xlsx_sheet_from_xml(&sheet_xml, &shared_strings)?;
        if !sheet_preview.trim().is_empty() {
            sections.push(sheet_preview);
        }
    }

    if sections.is_empty() {
        return Err("XLSX preview did not expose readable worksheet content.".into());
    }

    Ok(sections.join("\n\n"))
}

fn extract_pptx_preview<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<String, String> {
    let mut slide_names: Vec<(u32, String)> = archive
        .file_names()
        .filter(|name| name.starts_with("ppt/slides/slide") && name.ends_with(".xml"))
        .filter_map(|name| {
            pptx_slide_index_from_entry_name(name).map(|index| (index, name.to_string()))
        })
        .collect();
    slide_names.sort_by_key(|(index, _)| *index);

    if slide_names.is_empty() {
        return Err("Unable to build PPTX preview: no slide XML was found.".into());
    }

    let mut sections = Vec::new();
    for (slide_index, slide_name) in slide_names.iter().take(PPTX_PREVIEW_MAX_SLIDES) {
        let Some(slide_xml) = read_archive_entry_bytes(archive, slide_name)? else {
            continue;
        };
        let slide_preview = extract_pptx_text_from_xml(&slide_xml, "slide")?;
        let notes_preview = read_archive_entry_bytes(
            archive,
            &format!("ppt/notesSlides/notesSlide{slide_index}.xml"),
        )?
        .map(|notes_xml| extract_pptx_text_from_xml(&notes_xml, "speaker notes"))
        .transpose()?;

        let section = build_pptx_preview_section(&slide_preview, notes_preview.as_deref());
        if !section.trim().is_empty() {
            sections.push(section);
        }
    }

    if sections.is_empty() {
        return Err("PPTX preview did not expose readable slide content.".into());
    }

    Ok(sections.join("\n\n"))
}

fn build_pptx_preview_section(slide_text: &str, notes_text: Option<&str>) -> String {
    let slide_text = slide_text.trim();
    let notes_text = notes_text.unwrap_or_default().trim();

    match (slide_text.is_empty(), notes_text.is_empty()) {
        (true, true) => String::new(),
        (false, true) => slide_text.to_string(),
        (true, false) => format!("[Speaker Notes]\n{notes_text}"),
        (false, false) => format!("{slide_text}\n\n[Speaker Notes]\n{notes_text}"),
    }
}

fn read_archive_entry_bytes<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    entry_name: &str,
) -> Result<Option<Vec<u8>>, String> {
    match archive.by_name(entry_name) {
        Ok(mut file) => {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).map_err(|error| {
                format!("Unable to read preview archive entry {entry_name}: {error}")
            })?;
            Ok(Some(bytes))
        }
        Err(ZipError::FileNotFound) => Ok(None),
        Err(error) => Err(format!(
            "Unable to access preview archive entry {entry_name}: {error}"
        )),
    }
}

fn extract_docx_text_from_xml(xml: &[u8]) -> Result<String, String> {
    let mut reader = Reader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut output = String::new();
    let mut inside_text_node = false;

    loop {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                b"t" => inside_text_node = true,
                b"tab" => output.push('\t'),
                b"br" | b"cr" => push_preview_newline(&mut output),
                _ => {}
            },
            Ok(Event::Empty(event)) => match event.local_name().as_ref() {
                b"tab" => output.push('\t'),
                b"br" | b"cr" => push_preview_newline(&mut output),
                _ => {}
            },
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                b"t" => inside_text_node = false,
                b"p" | b"tr" => push_preview_newline(&mut output),
                b"tc" => {
                    if !output.ends_with('\n') && !output.ends_with('\t') {
                        output.push('\t');
                    }
                }
                _ => {}
            },
            Ok(Event::Text(event)) if inside_text_node => {
                let content = event
                    .xml_content()
                    .map_err(|error| format!("Unable to decode DOCX preview text: {error}"))?;
                output.push_str(&content);
            }
            Ok(Event::CData(event)) if inside_text_node => {
                output.push_str(&String::from_utf8_lossy(event.as_ref()));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(format!("Unable to parse DOCX preview XML: {error}"));
            }
        }
    }

    Ok(normalize_preview_text(output))
}

fn extract_pptx_text_from_xml(xml: &[u8], source_label: &str) -> Result<String, String> {
    let mut reader = Reader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut output = String::new();
    let mut inside_text_node = false;

    loop {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                b"t" => inside_text_node = true,
                b"br" => push_preview_newline(&mut output),
                _ => {}
            },
            Ok(Event::Empty(event)) => {
                if event.local_name().as_ref() == b"br" {
                    push_preview_newline(&mut output);
                }
            }
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                b"t" => inside_text_node = false,
                b"p" => push_preview_newline(&mut output),
                _ => {}
            },
            Ok(Event::Text(event)) if inside_text_node => {
                let content = event.xml_content().map_err(|error| {
                    format!("Unable to decode PPTX {source_label} preview text: {error}")
                })?;
                output.push_str(&content);
            }
            Ok(Event::CData(event)) if inside_text_node => {
                output.push_str(&String::from_utf8_lossy(event.as_ref()));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(format!(
                    "Unable to parse PPTX {source_label} preview XML: {error}"
                ));
            }
        }
    }

    Ok(normalize_preview_text(output))
}

fn pptx_slide_index_from_entry_name(entry_name: &str) -> Option<u32> {
    let file_name = entry_name
        .rsplit('/')
        .next()
        .unwrap_or(entry_name)
        .strip_suffix(".xml")?;
    file_name.strip_prefix("slide")?.parse::<u32>().ok()
}

fn parse_xlsx_shared_strings(xml: &[u8]) -> Result<Vec<String>, String> {
    let mut reader = Reader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut values = Vec::new();
    let mut current = String::new();
    let mut inside_string = false;
    let mut inside_text_node = false;

    loop {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                b"si" => {
                    current.clear();
                    inside_string = true;
                }
                b"t" if inside_string => inside_text_node = true,
                _ => {}
            },
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                b"t" => inside_text_node = false,
                b"si" => {
                    values.push(current.clone());
                    current.clear();
                    inside_string = false;
                }
                _ => {}
            },
            Ok(Event::Text(event)) if inside_text_node => {
                let content = event
                    .xml_content()
                    .map_err(|error| format!("Unable to decode XLSX shared string: {error}"))?;
                current.push_str(&content);
            }
            Ok(Event::CData(event)) if inside_text_node => {
                current.push_str(&String::from_utf8_lossy(event.as_ref()));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(format!("Unable to parse XLSX shared strings: {error}"));
            }
        }
    }

    Ok(values)
}

fn extract_xlsx_sheet_from_xml(xml: &[u8], shared_strings: &[String]) -> Result<String, String> {
    let mut reader = Reader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut output = String::new();
    let mut row_cells: Vec<String> = Vec::new();
    let mut current_cell_type: Option<String> = None;
    let mut current_cell_column: Option<usize> = None;
    let mut current_cell_value = String::new();
    let mut inside_value = false;
    let mut inside_inline_text = false;

    loop {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => match event.local_name().as_ref() {
                b"row" => row_cells.clear(),
                b"c" => {
                    current_cell_type = None;
                    current_cell_column = None;
                    current_cell_value.clear();
                    for attribute in event.attributes().with_checks(false) {
                        let attribute = attribute.map_err(|error| {
                            format!("Unable to parse XLSX cell attribute: {error}")
                        })?;
                        match attribute.key.as_ref() {
                            b"t" => {
                                current_cell_type =
                                    Some(String::from_utf8_lossy(attribute.value.as_ref()).into());
                            }
                            b"r" => {
                                current_cell_column = column_index_from_cell_reference(
                                    &String::from_utf8_lossy(attribute.value.as_ref()),
                                );
                            }
                            _ => {}
                        }
                    }
                }
                b"v" => inside_value = true,
                b"t" if current_cell_type.as_deref() == Some("inlineStr") => {
                    inside_inline_text = true;
                }
                _ => {}
            },
            Ok(Event::End(event)) => match event.local_name().as_ref() {
                b"v" => inside_value = false,
                b"t" => inside_inline_text = false,
                b"c" => {
                    if let Some(column) = current_cell_column {
                        while row_cells.len() + 1 < column {
                            row_cells.push(String::new());
                        }
                    }
                    row_cells.push(resolve_xlsx_cell_value(
                        current_cell_type.as_deref(),
                        &current_cell_value,
                        shared_strings,
                    ));
                    current_cell_type = None;
                    current_cell_column = None;
                    current_cell_value.clear();
                }
                b"row" => {
                    let row = trim_trailing_empty_cells(row_cells.as_slice());
                    if !row.is_empty() {
                        if !output.is_empty() {
                            output.push('\n');
                        }
                        output.push_str(&row.join("\t"));
                    }
                    row_cells.clear();
                }
                _ => {}
            },
            Ok(Event::Text(event)) if inside_value || inside_inline_text => {
                let content = event
                    .xml_content()
                    .map_err(|error| format!("Unable to decode XLSX cell value: {error}"))?;
                current_cell_value.push_str(&content);
            }
            Ok(Event::CData(event)) if inside_value || inside_inline_text => {
                current_cell_value.push_str(&String::from_utf8_lossy(event.as_ref()));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(format!("Unable to parse XLSX worksheet XML: {error}"));
            }
        }
    }

    Ok(normalize_preview_text(output))
}

fn resolve_xlsx_cell_value(
    cell_type: Option<&str>,
    raw_value: &str,
    shared_strings: &[String],
) -> String {
    let value = raw_value.trim();
    match cell_type {
        Some("s") => value
            .parse::<usize>()
            .ok()
            .and_then(|index| shared_strings.get(index).cloned())
            .unwrap_or_default(),
        Some("b") => match value {
            "1" => "TRUE".into(),
            "0" => "FALSE".into(),
            _ => value.into(),
        },
        _ => value.into(),
    }
}

fn column_index_from_cell_reference(reference: &str) -> Option<usize> {
    let column = reference
        .chars()
        .take_while(|character| character.is_ascii_alphabetic())
        .fold(0usize, |accumulator, character| {
            (accumulator * 26) + (character.to_ascii_uppercase() as usize) - ('A' as usize) + 1
        });

    (column > 0).then_some(column)
}

fn trim_trailing_empty_cells(row: &[String]) -> &[String] {
    let end = row
        .iter()
        .rposition(|cell| !cell.is_empty())
        .map(|index| index + 1)
        .unwrap_or(0);
    &row[..end]
}

fn normalize_preview_text(content: String) -> String {
    content
        .lines()
        .map(|line| line.trim_end_matches([' ', '\t']))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn limit_preview_text(content: &str) -> (String, bool) {
    if content.len() <= TEXT_PREVIEW_LIMIT_BYTES as usize {
        return (content.to_string(), false);
    }

    let mut end = TEXT_PREVIEW_LIMIT_BYTES as usize;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }

    (content[..end].to_string(), true)
}

fn push_preview_newline(output: &mut String) {
    if !output.ends_with('\n') {
        output.push('\n');
    }
}

fn sanitize_segment(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ByteRun;
    use std::{
        fs,
        io::{Cursor, Write},
    };
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    #[test]
    fn read_text_preview_from_path_truncates_large_files() {
        let root = env::temp_dir().join(format!("recupere-preview-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("preview test workspace should exist");
        let path = root.join("large.txt");
        fs::write(&path, "A".repeat((TEXT_PREVIEW_LIMIT_BYTES + 10) as usize))
            .expect("large preview source should be written");

        let (content, truncated) =
            read_text_preview_from_path(&path).expect("text preview should be readable");
        assert!(truncated);
        assert_eq!(content.len(), TEXT_PREVIEW_LIMIT_BYTES as usize);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn materialize_asset_preview_reconstructs_deleted_bytes() {
        let root = env::temp_dir().join(format!(
            "recupere-preview-asset-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("preview test workspace should exist");
        let image_path = root.join("source.img");
        fs::write(&image_path, b"0123456789abcdefghij")
            .expect("preview source image should be written");

        let preview_path = materialize_asset_preview(
            &image_path,
            &[ByteRun {
                offset: 10,
                length: 10,
                zero_fill: false,
                ..Default::default()
            }],
            10,
            "scan-preview",
            "file-preview",
            "txt",
        )
        .expect("asset preview should be materialized");

        assert_eq!(
            fs::read(&preview_path).expect("materialized preview should exist"),
            b"abcdefghij"
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(preview_path);
    }

    #[test]
    fn read_document_preview_from_path_extracts_docx_text() {
        let root =
            env::temp_dir().join(format!("recupere-preview-docx-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("preview docx workspace should exist");
        let path = root.join("case-note.docx");
        fs::write(
            &path,
            build_zip_bytes(&[
                (
                    "word/document.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
                    <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                      <w:body>
                        <w:p><w:r><w:t>Bonjour</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>Monde</w:t></w:r></w:p>
                        <w:p><w:r><w:t>Deuxieme ligne</w:t></w:r></w:p>
                      </w:body>
                    </w:document>"#,
                ),
            ]),
        )
        .expect("synthetic DOCX should be written");

        let (content, truncated) =
            read_document_preview_from_path(&path, "docx").expect("docx preview should parse");

        assert_eq!(content, "Bonjour\tMonde\nDeuxieme ligne");
        assert!(!truncated);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_document_preview_from_path_extracts_xlsx_rows() {
        let root =
            env::temp_dir().join(format!("recupere-preview-xlsx-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("preview xlsx workspace should exist");
        let path = root.join("inventory.xlsx");
        fs::write(
            &path,
            build_zip_bytes(&[
                (
                    "xl/sharedStrings.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
                    <sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                      <si><t>Client</t></si>
                      <si><t>Montant</t></si>
                    </sst>"#,
                ),
                (
                    "xl/worksheets/sheet1.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
                    <worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                      <sheetData>
                        <row r="1">
                          <c r="A1" t="s"><v>0</v></c>
                          <c r="B1" t="s"><v>1</v></c>
                        </row>
                        <row r="2">
                          <c r="A2" t="inlineStr"><is><t>Alpha</t></is></c>
                          <c r="B2"><v>42</v></c>
                        </row>
                      </sheetData>
                    </worksheet>"#,
                ),
            ]),
        )
        .expect("synthetic XLSX should be written");

        let (content, truncated) =
            read_document_preview_from_path(&path, "xlsx").expect("xlsx preview should parse");

        assert_eq!(content, "Client\tMontant\nAlpha\t42");
        assert!(!truncated);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_document_preview_from_path_extracts_pptx_text() {
        let root =
            env::temp_dir().join(format!("recupere-preview-pptx-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("preview pptx workspace should exist");
        let path = root.join("deck.pptx");
        fs::write(
            &path,
            build_zip_bytes(&[
                (
                    "ppt/presentation.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
                    <p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"></p:presentation>"#,
                ),
                (
                    "ppt/slides/slide1.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
                    <p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                           xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                      <p:cSld>
                        <p:spTree>
                          <p:sp><p:txBody><a:p><a:r><a:t>Incident Review</a:t></a:r></a:p></p:txBody></p:sp>
                          <p:sp><p:txBody><a:p><a:r><a:t>Disk 4 unstable</a:t></a:r></a:p></p:txBody></p:sp>
                        </p:spTree>
                      </p:cSld>
                    </p:sld>"#,
                ),
            ]),
        )
        .expect("synthetic PPTX should be written");

        let (content, truncated) =
            read_document_preview_from_path(&path, "pptx").expect("pptx preview should parse");

        assert_eq!(content, "Incident Review\nDisk 4 unstable");
        assert!(!truncated);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_document_preview_from_path_extracts_pptx_speaker_notes() {
        let root = env::temp_dir().join(format!(
            "recupere-preview-pptx-notes-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("preview pptx notes workspace should exist");
        let path = root.join("deck-notes.pptx");
        fs::write(
            &path,
            build_zip_bytes(&[
                (
                    "ppt/presentation.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
                    <p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"></p:presentation>"#,
                ),
                (
                    "ppt/slides/slide1.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
                    <p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                           xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                      <p:cSld><p:spTree>
                        <p:sp><p:txBody><a:p><a:r><a:t>Incident Review</a:t></a:r></a:p></p:txBody></p:sp>
                      </p:spTree></p:cSld>
                    </p:sld>"#,
                ),
                (
                    "ppt/notesSlides/notesSlide1.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
                    <p:notes xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                             xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                      <p:cSld><p:spTree>
                        <p:sp><p:txBody><a:p><a:r><a:t>Ask the user to stop writing to disk 4.</a:t></a:r></a:p></p:txBody></p:sp>
                      </p:spTree></p:cSld>
                    </p:notes>"#,
                ),
            ]),
        )
        .expect("synthetic PPTX with notes should be written");

        let (content, truncated) = read_document_preview_from_path(&path, "pptx")
            .expect("pptx notes preview should parse");

        assert_eq!(
            content,
            "Incident Review\n\n[Speaker Notes]\nAsk the user to stop writing to disk 4."
        );
        assert!(!truncated);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_document_preview_from_image_extracts_reconstructed_docx_text() {
        let root = env::temp_dir().join(format!(
            "recupere-preview-docx-image-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("preview docx image workspace should exist");
        let image_path = root.join("source.img");
        let docx_bytes = build_zip_bytes(&[(
            "word/document.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
              <w:body><w:p><w:r><w:t>Recovered note</w:t></w:r></w:p></w:body>
            </w:document>"#,
        )]);
        fs::write(&image_path, &docx_bytes).expect("preview source image should be written");

        let (content, truncated) = read_document_preview_from_image(
            &image_path,
            &[ByteRun {
                offset: 0,
                length: docx_bytes.len() as u64,
                zero_fill: false,
                ..Default::default()
            }],
            docx_bytes.len() as u64,
            "scan-docx-image",
            "file-docx-image",
            "docx",
        )
        .expect("docx preview from image should parse");

        assert_eq!(content, "Recovered note");
        assert!(!truncated);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_document_preview_from_image_if_supported_detects_docx_payloads() {
        let root = env::temp_dir().join(format!(
            "recupere-preview-docx-aux-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("preview docx aux workspace should exist");
        let image_path = root.join("source.img");
        let docx_bytes = build_zip_bytes(&[(
            "word/document.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
              <w:body><w:p><w:r><w:t>Auxiliary DOCX</w:t></w:r></w:p></w:body>
            </w:document>"#,
        )]);
        fs::write(&image_path, &docx_bytes).expect("preview aux DOCX source should be written");

        let preview = read_document_preview_from_image_if_supported(
            &image_path,
            &[ByteRun::physical(0, docx_bytes.len() as u64)],
            docx_bytes.len() as u64,
            "scan-aux-docx-image",
            "file-aux-docx-image",
        )
        .expect("auxiliary DOCX preview detection should succeed")
        .expect("DOCX payload should be detected");

        assert_eq!(preview.0, "Auxiliary DOCX");
        assert!(!preview.1);
        assert_eq!(preview.2, "docx");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_document_preview_from_image_if_supported_detects_pptx_payloads() {
        let root = env::temp_dir().join(format!(
            "recupere-preview-pptx-aux-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("preview pptx aux workspace should exist");
        let image_path = root.join("source.img");
        let pptx_bytes = build_zip_bytes(&[
            (
                "ppt/presentation.xml",
                r#"<?xml version="1.0" encoding="UTF-8"?>
                <p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"></p:presentation>"#,
            ),
            (
                "ppt/slides/slide1.xml",
                r#"<?xml version="1.0" encoding="UTF-8"?>
                <p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                       xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                  <p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Auxiliary PPTX</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld>
                </p:sld>"#,
            ),
        ]);
        fs::write(&image_path, &pptx_bytes).expect("preview aux PPTX source should be written");

        let preview = read_document_preview_from_image_if_supported(
            &image_path,
            &[ByteRun::physical(0, pptx_bytes.len() as u64)],
            pptx_bytes.len() as u64,
            "scan-aux-pptx-image",
            "file-aux-pptx-image",
        )
        .expect("auxiliary PPTX preview detection should succeed")
        .expect("PPTX payload should be detected");

        assert_eq!(preview.0, "Auxiliary PPTX");
        assert!(!preview.1);
        assert_eq!(preview.2, "pptx");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_document_preview_from_image_if_supported_keeps_pptx_speaker_notes() {
        let root = env::temp_dir().join(format!(
            "recupere-preview-pptx-aux-notes-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("preview pptx aux notes workspace should exist");
        let image_path = root.join("source.img");
        let pptx_bytes = build_zip_bytes(&[
            (
                "ppt/presentation.xml",
                r#"<?xml version="1.0" encoding="UTF-8"?>
                <p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"></p:presentation>"#,
            ),
            (
                "ppt/slides/slide1.xml",
                r#"<?xml version="1.0" encoding="UTF-8"?>
                <p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                       xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                  <p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Auxiliary PPTX</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld>
                </p:sld>"#,
            ),
            (
                "ppt/notesSlides/notesSlide1.xml",
                r#"<?xml version="1.0" encoding="UTF-8"?>
                <p:notes xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                         xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                  <p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Preserve notes in export.</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld>
                </p:notes>"#,
            ),
        ]);
        fs::write(&image_path, &pptx_bytes)
            .expect("preview aux PPTX notes source should be written");

        let preview = read_document_preview_from_image_if_supported(
            &image_path,
            &[ByteRun::physical(0, pptx_bytes.len() as u64)],
            pptx_bytes.len() as u64,
            "scan-aux-pptx-notes-image",
            "file-aux-pptx-notes-image",
        )
        .expect("auxiliary PPTX notes preview detection should succeed")
        .expect("PPTX payload with notes should be detected");

        assert_eq!(
            preview.0,
            "Auxiliary PPTX\n\n[Speaker Notes]\nPreserve notes in export."
        );
        assert!(!preview.1);
        assert_eq!(preview.2, "pptx");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_document_preview_from_image_if_supported_ignores_non_office_archives() {
        let root = env::temp_dir().join(format!(
            "recupere-preview-zip-aux-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("preview aux ZIP workspace should exist");
        let image_path = root.join("source.img");
        let zip_bytes = build_zip_bytes(&[("notes/readme.txt", "Just a generic zip")]);
        fs::write(&image_path, &zip_bytes).expect("preview aux ZIP source should be written");

        let preview = read_document_preview_from_image_if_supported(
            &image_path,
            &[ByteRun::physical(0, zip_bytes.len() as u64)],
            zip_bytes.len() as u64,
            "scan-aux-zip-image",
            "file-aux-zip-image",
        )
        .expect("auxiliary ZIP detection should succeed");

        assert!(preview.is_none());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_document_preview_from_path_rejects_invalid_archive() {
        let root = env::temp_dir().join(format!(
            "recupere-preview-invalid-doc-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("preview invalid doc workspace should exist");
        let path = root.join("broken.docx");
        fs::write(&path, b"not a zip archive").expect("invalid archive should be written");

        let error = read_document_preview_from_path(&path, "docx")
            .expect_err("invalid archive should fail cleanly");
        assert!(error.contains("Unable to open docx preview source"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_text_preview_from_image_if_text_like_accepts_textual_bytes() {
        let root = env::temp_dir().join(format!(
            "recupere-preview-aux-text-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("preview aux text workspace should exist");
        let image_path = root.join("source.img");
        fs::write(&image_path, b"[ZoneTransfer]\nZoneId=3\n")
            .expect("preview aux text source should be written");

        let preview =
            read_text_preview_from_image_if_text_like(&image_path, &[ByteRun::physical(0, 24)], 24)
                .expect("text-like preview should read");

        let (content, truncated) = preview.expect("preview should be considered text-like");
        assert_eq!(content, "[ZoneTransfer]\nZoneId=3\n");
        assert!(!truncated);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_text_preview_from_image_if_text_like_rejects_binary_bytes() {
        let root = env::temp_dir().join(format!(
            "recupere-preview-aux-binary-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("preview aux binary workspace should exist");
        let image_path = root.join("source.img");
        fs::write(&image_path, [0_u8, 0x01, 0x02, 0x7F, b'A', b'B', b'C'])
            .expect("preview aux binary source should be written");

        let preview =
            read_text_preview_from_image_if_text_like(&image_path, &[ByteRun::physical(0, 7)], 7)
                .expect("binary-like preview should read");

        assert!(preview.is_none());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn enforce_preview_workspace_quota_evicts_oldest_until_under_limit() {
        use std::time::{Duration, SystemTime};
        let workspace = env::temp_dir().join(format!(
            "recupere-preview-quota-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&workspace);
        fs::create_dir_all(&workspace).expect("preview quota workspace should exist");

        // Three 10-byte files with strictly-increasing mtimes. The quota is
        // 20 bytes so the oldest (first) must be evicted.
        let now = SystemTime::now();
        let paths: Vec<PathBuf> = (0..3)
            .map(|i| {
                let path = workspace.join(format!("file-{i}.bin"));
                fs::write(&path, vec![b'x'; 10]).expect("asset write should succeed");
                let mtime = now - Duration::from_secs(60 - (i as u64 * 10));
                set_file_mtime_for_test(&path, mtime);
                path
            })
            .collect();

        let fresh = paths[2].clone();
        enforce_preview_workspace_quota_at(&workspace, &fresh, 20)
            .expect("quota enforcement should succeed when LRU makes room");

        assert!(!paths[0].exists(), "oldest file should be evicted");
        assert!(paths[1].exists(), "second file should survive");
        assert!(fresh.exists(), "fresh file must be preserved");

        let _ = fs::remove_dir_all(&workspace);
    }

    #[test]
    fn enforce_preview_workspace_quota_rejects_oversize_fresh_asset() {
        let workspace = env::temp_dir().join(format!(
            "recupere-preview-quota-oversize-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&workspace);
        fs::create_dir_all(&workspace).expect("preview quota workspace should exist");
        let fresh = workspace.join("huge.bin");
        fs::write(&fresh, vec![b'y'; 1024 * 1024]).expect("fresh oversize asset should be written");

        let err = enforce_preview_workspace_quota_at(&workspace, &fresh, 1024)
            .expect_err("oversize fresh asset should be rejected");
        assert!(err.contains("Preview skipped"), "got: {err}");
        assert!(
            !fresh.exists(),
            "fresh asset that exceeds the quota must be deleted by the LRU sweep"
        );

        let _ = fs::remove_dir_all(&workspace);
    }

    fn set_file_mtime_for_test(path: &Path, target: std::time::SystemTime) {
        // std::fs::File::set_modified (stable since 1.75) is the cross-
        // platform way to stamp a test file with a deterministic mtime.
        let file = File::options()
            .write(true)
            .open(path)
            .expect("test asset should be opened for mtime update");
        file.set_modified(target)
            .expect("test asset mtime should be settable");
    }

    fn build_zip_bytes(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        let mut archive = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

        for (path, content) in entries {
            archive
                .start_file(path, options)
                .expect("zip entry should start");
            archive
                .write_all(content.as_bytes())
                .expect("zip entry bytes should be written");
        }

        archive.finish().expect("zip archive should finish");
        cursor.into_inner()
    }
}
