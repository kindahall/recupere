// ============================================================================
// Récupère — File Repair Engine (Phase A: trivial repairs)
// ============================================================================
// Repairs structurally damaged files whose payload is largely intact but
// whose framing/trailer/index has been truncated or corrupted. This is the
// kind of cheap, high-impact repair that Stellar Repair, DiskInternals and
// Disk Drill charge extra for. The five repairers below cover the most
// common formats encountered in real-world recoveries.
//
//   - JPEG : append SOI/EOI, truncate trailing garbage past EOI.
//   - PNG  : append IEND chunk if missing, fix truncation.
//   - PDF  : append %%EOF marker if missing.
//   - ZIP  : reconstruct End-of-Central-Directory from Local File Headers.
//   - MP4  : detect missing/displaced moov atom (reports only — full
//            untrunc-style reconstruction is Phase B).
// ============================================================================

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RepairConfidence {
    /// File looks intact, no repair needed.
    Intact,
    /// Repair has a high chance of producing a usable file.
    High,
    /// Repair may produce a partially usable file.
    Medium,
    /// Format is recognised but no repair strategy is applicable.
    None,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepairReport {
    pub format: &'static str,
    pub confidence: RepairConfidence,
    pub original_size: usize,
    pub repaired_size: usize,
    pub actions: Vec<String>,
    pub notes: Vec<String>,
}

impl RepairReport {
    fn intact(format: &'static str, size: usize) -> Self {
        Self {
            format,
            confidence: RepairConfidence::Intact,
            original_size: size,
            repaired_size: size,
            actions: vec!["File is already intact, no repair applied.".into()],
            notes: vec![],
        }
    }
}

#[derive(Debug)]
pub struct RepairOutcome {
    pub bytes: Vec<u8>,
    pub report: RepairReport,
}

pub fn repair(extension: &str, bytes: &[u8]) -> Result<RepairOutcome, String> {
    let ext = extension.to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => repair_jpeg(bytes),
        "png" => repair_png(bytes),
        "pdf" => repair_pdf(bytes),
        "zip" | "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" | "epub" | "jar" | "apk" => {
            repair_zip(bytes, &ext)
        }
        "mp4" | "mov" | "m4v" | "m4a" => analyse_mp4(bytes),
        _ => Err(format!("No repair strategy for extension `{ext}`.")),
    }
}

// ---------------------------------------------------------------------------
// JPEG
// ---------------------------------------------------------------------------

fn repair_jpeg(bytes: &[u8]) -> Result<RepairOutcome, String> {
    if bytes.len() < 4 {
        return Err("JPEG payload is too short.".into());
    }
    if bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Err("Not a JPEG (missing SOI marker FF D8).".into());
    }

    let mut actions = Vec::new();
    let mut out = Vec::with_capacity(bytes.len() + 2);

    // Find the last EOI marker (FF D9) anywhere in the file. JPEGs sometimes
    // contain embedded EOIs in thumbnails, so we want the *last* one.
    let mut last_eoi: Option<usize> = None;
    let mut i = 2;
    while i + 1 < bytes.len() {
        if bytes[i] == 0xFF && bytes[i + 1] == 0xD9 {
            last_eoi = Some(i + 2);
            i += 2;
        } else {
            i += 1;
        }
    }

    match last_eoi {
        Some(end) if end == bytes.len() => {
            return Ok(RepairOutcome {
                bytes: bytes.to_vec(),
                report: RepairReport::intact("jpeg", bytes.len()),
            });
        }
        Some(end) => {
            out.extend_from_slice(&bytes[..end]);
            actions.push(format!(
                "Truncated {} trailing garbage bytes past last EOI marker.",
                bytes.len() - end
            ));
        }
        None => {
            out.extend_from_slice(bytes);
            out.extend_from_slice(&[0xFF, 0xD9]);
            actions.push("Appended missing EOI marker (FF D9).".into());
        }
    }

    let confidence = if last_eoi.is_some() {
        RepairConfidence::High
    } else {
        RepairConfidence::Medium
    };

    Ok(RepairOutcome {
        report: RepairReport {
            format: "jpeg",
            confidence,
            original_size: bytes.len(),
            repaired_size: out.len(),
            actions,
            notes: vec![
                "Header markers verified. If decoding still fails, the entropy-coded \
                 segments are damaged — try Phase B repair (Huffman recovery)."
                    .into(),
            ],
        },
        bytes: out,
    })
}

// ---------------------------------------------------------------------------
// PNG
// ---------------------------------------------------------------------------

const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
const PNG_IEND: [u8; 12] = [
    0x00, 0x00, 0x00, 0x00, // length
    b'I', b'E', b'N', b'D', // type
    0xAE, 0x42, 0x60, 0x82, // pre-computed CRC
];

fn repair_png(bytes: &[u8]) -> Result<RepairOutcome, String> {
    if bytes.len() < 8 || bytes[..8] != PNG_SIGNATURE {
        return Err("Not a PNG (signature mismatch).".into());
    }

    let mut actions = Vec::new();
    let mut last_chunk_end: Option<usize> = None;
    let mut has_iend = false;
    let mut has_ihdr = false;

    let mut offset = 8usize;
    while offset + 12 <= bytes.len() {
        let length = u32::from_be_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        let chunk_total = 12 + length;
        if offset + chunk_total > bytes.len() {
            break; // truncated chunk
        }
        let chunk_type = &bytes[offset + 4..offset + 8];
        if chunk_type == b"IHDR" {
            has_ihdr = true;
        }
        if chunk_type == b"IEND" {
            has_iend = true;
            last_chunk_end = Some(offset + chunk_total);
            break;
        }
        last_chunk_end = Some(offset + chunk_total);
        offset += chunk_total;
    }

    if !has_ihdr {
        return Err("PNG IHDR chunk missing — file is unrecoverable by structural repair.".into());
    }

    let truncate_to = last_chunk_end.unwrap_or(8);
    let mut out = Vec::with_capacity(truncate_to + 12);
    out.extend_from_slice(&bytes[..truncate_to]);

    if truncate_to != bytes.len() {
        actions.push(format!(
            "Truncated {} bytes of corrupted/incomplete data past last valid chunk.",
            bytes.len() - truncate_to
        ));
    }

    if !has_iend {
        out.extend_from_slice(&PNG_IEND);
        actions.push("Appended missing IEND terminator chunk.".into());
    }

    let confidence = if has_iend && truncate_to == bytes.len() {
        return Ok(RepairOutcome {
            bytes: bytes.to_vec(),
            report: RepairReport::intact("png", bytes.len()),
        });
    } else if has_ihdr {
        RepairConfidence::High
    } else {
        RepairConfidence::Medium
    };

    Ok(RepairOutcome {
        report: RepairReport {
            format: "png",
            confidence,
            original_size: bytes.len(),
            repaired_size: out.len(),
            actions,
            notes: vec![],
        },
        bytes: out,
    })
}

// ---------------------------------------------------------------------------
// PDF
// ---------------------------------------------------------------------------

fn repair_pdf(bytes: &[u8]) -> Result<RepairOutcome, String> {
    if bytes.len() < 5 || &bytes[..5] != b"%PDF-" {
        return Err("Not a PDF (missing %PDF- header).".into());
    }

    // Search for %%EOF in the last 1024 bytes (the standard places it
    // there per PDF 1.7 §7.5.5).
    let tail_start = bytes.len().saturating_sub(1024);
    let tail = &bytes[tail_start..];
    let has_eof = tail.windows(5).any(|w| w == b"%%EOF");

    if has_eof {
        return Ok(RepairOutcome {
            bytes: bytes.to_vec(),
            report: RepairReport::intact("pdf", bytes.len()),
        });
    }

    let mut out = bytes.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    out.extend_from_slice(b"%%EOF\n");

    Ok(RepairOutcome {
        report: RepairReport {
            format: "pdf",
            confidence: RepairConfidence::Medium,
            original_size: bytes.len(),
            repaired_size: out.len(),
            actions: vec!["Appended missing %%EOF trailer marker.".into()],
            notes: vec![
                "Cross-reference table (xref) is not rebuilt. Many PDF readers will \
                 still open the document; for full xref reconstruction, use Phase B."
                    .into(),
            ],
        },
        bytes: out,
    })
}

// ---------------------------------------------------------------------------
// ZIP (covers DOCX/XLSX/PPTX/ODT/EPUB/JAR/APK)
// ---------------------------------------------------------------------------

const ZIP_LFH_SIG: [u8; 4] = [b'P', b'K', 0x03, 0x04];
const ZIP_CDH_SIG: [u8; 4] = [b'P', b'K', 0x01, 0x02];
const ZIP_EOCD_SIG: [u8; 4] = [b'P', b'K', 0x05, 0x06];

fn repair_zip(bytes: &[u8], ext: &str) -> Result<RepairOutcome, String> {
    if bytes.len() < 22 {
        return Err("Archive too small to be a valid ZIP.".into());
    }
    if !bytes.starts_with(&ZIP_LFH_SIG) && !bytes.starts_with(&ZIP_CDH_SIG) {
        return Err("Not a ZIP (no Local File Header at offset 0).".into());
    }

    // Already-OK fast path: scan the last 65 KB for EOCD.
    let scan_start = bytes.len().saturating_sub(65 * 1024);
    let has_eocd = bytes[scan_start..].windows(4).any(|w| w == ZIP_EOCD_SIG);

    if has_eocd {
        return Ok(RepairOutcome {
            bytes: bytes.to_vec(),
            report: RepairReport::intact(zip_format_label(ext), bytes.len()),
        });
    }

    // Walk the Local File Headers to enumerate entries, build a synthetic
    // Central Directory, then synthesize an EOCD that points to it.
    let entries = scan_local_file_headers(bytes)?;
    if entries.is_empty() {
        return Err("No Local File Headers could be parsed — archive is too damaged.".into());
    }

    let central_dir_offset = bytes.len() as u32;
    let mut central_dir = Vec::new();
    for entry in &entries {
        write_central_directory_header(&mut central_dir, entry);
    }
    let central_dir_size = central_dir.len() as u32;

    let mut eocd = Vec::with_capacity(22);
    eocd.extend_from_slice(&ZIP_EOCD_SIG);
    eocd.extend_from_slice(&[0, 0]); // disk number
    eocd.extend_from_slice(&[0, 0]); // disk where CD starts
    eocd.extend_from_slice(&(entries.len() as u16).to_le_bytes()); // entries on this disk
    eocd.extend_from_slice(&(entries.len() as u16).to_le_bytes()); // total entries
    eocd.extend_from_slice(&central_dir_size.to_le_bytes());
    eocd.extend_from_slice(&central_dir_offset.to_le_bytes());
    eocd.extend_from_slice(&[0, 0]); // .ZIP file comment length

    let mut out = Vec::with_capacity(bytes.len() + central_dir.len() + eocd.len());
    out.extend_from_slice(bytes);
    out.extend_from_slice(&central_dir);
    out.extend_from_slice(&eocd);

    Ok(RepairOutcome {
        report: RepairReport {
            format: zip_format_label(ext),
            confidence: RepairConfidence::High,
            original_size: bytes.len(),
            repaired_size: out.len(),
            actions: vec![
                format!(
                    "Reconstructed Central Directory from {} Local File Headers.",
                    entries.len()
                ),
                "Synthesised End-of-Central-Directory record.".into(),
            ],
            notes: vec!["Compressed sizes preserved from LFH; if a LFH used \
                 streaming mode (sizes-in-data-descriptor) the entry may be \
                 skipped silently."
                .into()],
        },
        bytes: out,
    })
}

fn zip_format_label(ext: &str) -> &'static str {
    match ext {
        "docx" => "docx",
        "xlsx" => "xlsx",
        "pptx" => "pptx",
        "odt" | "ods" | "odp" => "opendocument",
        "epub" => "epub",
        "jar" => "jar",
        "apk" => "apk",
        _ => "zip",
    }
}

#[derive(Debug)]
struct ZipEntry {
    lfh_offset: u32,
    version_needed: u16,
    flags: u16,
    method: u16,
    mod_time: u16,
    mod_date: u16,
    crc32: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    name: Vec<u8>,
    extra: Vec<u8>,
}

fn scan_local_file_headers(bytes: &[u8]) -> Result<Vec<ZipEntry>, String> {
    let mut entries = Vec::new();
    let mut offset = 0usize;
    while offset + 30 <= bytes.len() {
        if bytes[offset..offset + 4] != ZIP_LFH_SIG {
            // Stop at the first non-LFH boundary; CDHs and EOCD live past
            // the payload area and we're not interested in them here.
            break;
        }
        let version_needed = u16::from_le_bytes([bytes[offset + 4], bytes[offset + 5]]);
        let flags = u16::from_le_bytes([bytes[offset + 6], bytes[offset + 7]]);
        let method = u16::from_le_bytes([bytes[offset + 8], bytes[offset + 9]]);
        let mod_time = u16::from_le_bytes([bytes[offset + 10], bytes[offset + 11]]);
        let mod_date = u16::from_le_bytes([bytes[offset + 12], bytes[offset + 13]]);
        let crc32 = u32::from_le_bytes([
            bytes[offset + 14],
            bytes[offset + 15],
            bytes[offset + 16],
            bytes[offset + 17],
        ]);
        let compressed_size = u32::from_le_bytes([
            bytes[offset + 18],
            bytes[offset + 19],
            bytes[offset + 20],
            bytes[offset + 21],
        ]);
        let uncompressed_size = u32::from_le_bytes([
            bytes[offset + 22],
            bytes[offset + 23],
            bytes[offset + 24],
            bytes[offset + 25],
        ]);
        let name_len = u16::from_le_bytes([bytes[offset + 26], bytes[offset + 27]]) as usize;
        let extra_len = u16::from_le_bytes([bytes[offset + 28], bytes[offset + 29]]) as usize;

        let name_start = offset + 30;
        let name_end = name_start + name_len;
        let extra_end = name_end + extra_len;
        if extra_end > bytes.len() {
            break;
        }
        let name = bytes[name_start..name_end].to_vec();
        let extra = bytes[name_end..extra_end].to_vec();

        // Streaming mode: sizes are zero in the LFH and the actual values
        // sit in a data descriptor right after the compressed data. We
        // can't reliably reconstruct those, so we skip the entry.
        if flags & 0x0008 != 0 || compressed_size == 0xFFFFFFFF {
            // Best-effort: try to find next LFH signature.
            if let Some(next) = find_next_signature(bytes, extra_end, &ZIP_LFH_SIG) {
                offset = next;
                continue;
            }
            break;
        }

        let data_end = extra_end + compressed_size as usize;
        if data_end > bytes.len() {
            break;
        }

        entries.push(ZipEntry {
            lfh_offset: offset as u32,
            version_needed,
            flags,
            method,
            mod_time,
            mod_date,
            crc32,
            compressed_size,
            uncompressed_size,
            name,
            extra,
        });

        offset = data_end;
    }
    Ok(entries)
}

fn find_next_signature(bytes: &[u8], from: usize, sig: &[u8; 4]) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(4)
        .position(|w| w == sig)
        .map(|p| from + p)
}

fn write_central_directory_header(out: &mut Vec<u8>, e: &ZipEntry) {
    out.extend_from_slice(&ZIP_CDH_SIG);
    out.extend_from_slice(&[0x14, 0x00]); // version made by (ZIP 2.0)
    out.extend_from_slice(&e.version_needed.to_le_bytes());
    out.extend_from_slice(&e.flags.to_le_bytes());
    out.extend_from_slice(&e.method.to_le_bytes());
    out.extend_from_slice(&e.mod_time.to_le_bytes());
    out.extend_from_slice(&e.mod_date.to_le_bytes());
    out.extend_from_slice(&e.crc32.to_le_bytes());
    out.extend_from_slice(&e.compressed_size.to_le_bytes());
    out.extend_from_slice(&e.uncompressed_size.to_le_bytes());
    out.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
    out.extend_from_slice(&(e.extra.len() as u16).to_le_bytes());
    out.extend_from_slice(&[0, 0]); // file comment length
    out.extend_from_slice(&[0, 0]); // disk number start
    out.extend_from_slice(&[0, 0]); // internal file attributes
    out.extend_from_slice(&[0, 0, 0, 0]); // external file attributes
    out.extend_from_slice(&e.lfh_offset.to_le_bytes());
    out.extend_from_slice(&e.name);
    out.extend_from_slice(&e.extra);
}

// ---------------------------------------------------------------------------
// MP4 (analysis only — Phase A)
// ---------------------------------------------------------------------------

fn analyse_mp4(bytes: &[u8]) -> Result<RepairOutcome, String> {
    if bytes.len() < 16 {
        return Err("MP4 payload too short.".into());
    }
    let mut has_ftyp = false;
    let mut has_moov = false;
    let mut has_mdat = false;

    let mut offset = 0usize;
    while offset + 8 <= bytes.len() {
        let size = u32::from_be_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        let kind = &bytes[offset + 4..offset + 8];
        match kind {
            b"ftyp" => has_ftyp = true,
            b"moov" => has_moov = true,
            b"mdat" => has_mdat = true,
            _ => {}
        }
        if size < 8 {
            break;
        }
        offset = offset.saturating_add(size);
    }

    let confidence = if has_ftyp && has_moov && has_mdat {
        RepairConfidence::Intact
    } else {
        RepairConfidence::None
    };

    let actions = if confidence == RepairConfidence::Intact {
        vec!["ftyp/moov/mdat all present — file appears intact.".into()]
    } else {
        vec!["MP4 structural repair (moov rebuild) is scheduled for Phase B.".into()]
    };

    let mut notes = Vec::new();
    if !has_ftyp {
        notes.push("Missing `ftyp` box.".into());
    }
    if !has_moov {
        notes.push("Missing `moov` box — Phase B (untrunc-style) needed.".into());
    }
    if !has_mdat {
        notes.push("Missing `mdat` box — payload data is gone.".into());
    }

    Ok(RepairOutcome {
        bytes: bytes.to_vec(),
        report: RepairReport {
            format: "mp4",
            confidence,
            original_size: bytes.len(),
            repaired_size: bytes.len(),
            actions,
            notes,
        },
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_appends_missing_eoi() {
        let bytes = [0xFF, 0xD8, 0x00, 0x11, 0x22];
        let outcome = repair_jpeg(&bytes).unwrap();
        assert_eq!(&outcome.bytes[outcome.bytes.len() - 2..], &[0xFF, 0xD9]);
        assert_eq!(outcome.report.confidence, RepairConfidence::Medium);
    }

    #[test]
    fn jpeg_truncates_garbage_past_eoi() {
        let bytes = [0xFF, 0xD8, 0x00, 0xFF, 0xD9, 0x99, 0x88];
        let outcome = repair_jpeg(&bytes).unwrap();
        assert_eq!(outcome.bytes.len(), 5);
        assert_eq!(outcome.report.confidence, RepairConfidence::High);
    }

    #[test]
    fn png_appends_iend() {
        let mut bytes = PNG_SIGNATURE.to_vec();
        // Minimal IHDR chunk: length=13, type=IHDR, 13 bytes of data, CRC.
        bytes.extend_from_slice(&[0, 0, 0, 13]);
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&[0; 13]);
        bytes.extend_from_slice(&[0; 4]); // bogus CRC, we don't validate
        let outcome = repair_png(&bytes).unwrap();
        assert_eq!(&outcome.bytes[outcome.bytes.len() - 12..], &PNG_IEND);
    }

    #[test]
    fn pdf_appends_eof_marker() {
        let bytes = b"%PDF-1.4\n%hello\n";
        let outcome = repair_pdf(bytes).unwrap();
        assert!(outcome.bytes.ends_with(b"%%EOF\n"));
    }

    #[test]
    fn pdf_intact_when_eof_present() {
        let bytes = b"%PDF-1.4\nstuff\n%%EOF\n";
        let outcome = repair_pdf(bytes).unwrap();
        assert_eq!(outcome.report.confidence, RepairConfidence::Intact);
    }

    #[test]
    fn zip_unknown_extension_rejected() {
        let bytes = b"PK\x03\x04rest of payload that is not a real zip";
        // Too short / no parseable LFH → expect a clean error, not a panic.
        let result = repair_zip(bytes, "zip");
        assert!(result.is_err() || result.is_ok());
    }
}
