#![allow(dead_code)]
// ============================================================================
// Recupere — Slack Space Scanner
// ============================================================================
// Scans unallocated space between file data and cluster boundaries, plus
// entirely unallocated clusters, for remnants of deleted files.
// This catches data that filesystem analyzers miss because the metadata
// was overwritten but the data blocks weren't.
// ============================================================================

use crate::types::ByteRun;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const SCAN_CHUNK_SIZE: usize = 4096;
const MIN_INTERESTING_SIZE: usize = 32;

/// Candidate found in slack/unallocated space.
#[derive(Debug, Clone)]
pub struct SlackCandidate {
    pub offset: u64,
    pub size_bytes: u64,
    pub data_type: &'static str,
    pub extension: &'static str,
    pub byte_runs: Vec<ByteRun>,
    pub confidence: u8,
}

/// Scan unallocated regions of a disk image for file signatures.
/// Takes a list of allocated byte ranges (from the filesystem analyzer)
/// and scans everything else for known file headers.
pub fn scan_unallocated_space(
    image_path: &Path,
    image_size: u64,
    allocated_ranges: &[(u64, u64)], // (start, end) of allocated space
    max_candidates: usize,
) -> Result<Vec<SlackCandidate>, String> {
    let mut reader =
        File::open(image_path).map_err(|e| format!("Cannot open image for slack scanning: {e}"))?;

    // Build sorted list of free ranges
    let mut free_ranges = compute_free_ranges(image_size, allocated_ranges);
    // Sort by size descending — scan largest gaps first for best ROI
    free_ranges.sort_by(|a, b| (b.1 - b.0).cmp(&(a.1 - a.0)));

    let mut candidates = Vec::new();

    for (range_start, range_end) in &free_ranges {
        if candidates.len() >= max_candidates {
            break;
        }

        let range_size = range_end - range_start;
        if range_size < MIN_INTERESTING_SIZE as u64 {
            continue;
        }

        // Scan this free range for file signatures
        let mut offset = *range_start;
        while offset < *range_end && candidates.len() < max_candidates {
            let chunk_size = SCAN_CHUNK_SIZE.min((range_end - offset) as usize);
            let mut buf = vec![0u8; chunk_size];

            if let Err(e) = reader.seek(SeekFrom::Start(offset)) {
                tracing::warn!("slack scan: seek to {offset} failed: {e}; aborting this range");
                break;
            }
            let bytes_read = match reader.read(&mut buf) {
                Ok(n) => n,
                Err(e) => {
                    tracing::warn!("slack scan: read at {offset} failed: {e}; aborting this range");
                    break;
                }
            };
            if bytes_read < MIN_INTERESTING_SIZE {
                break;
            }
            buf.truncate(bytes_read);

            // Skip all-zero chunks (TRIM-zeroed)
            if buf.iter().all(|&b| b == 0) {
                offset += bytes_read as u64;
                continue;
            }

            // Check for known file signatures at the start of each sector boundary
            for sector_offset in (0..bytes_read).step_by(512) {
                if sector_offset + 16 > bytes_read {
                    break;
                }

                let sector = &buf[sector_offset..];
                if let Some((data_type, extension, estimated_size)) = identify_signature(sector) {
                    let abs_offset = offset + sector_offset as u64;
                    let actual_size = estimated_size.min((range_end - abs_offset) as usize);

                    if actual_size >= MIN_INTERESTING_SIZE {
                        candidates.push(SlackCandidate {
                            offset: abs_offset,
                            size_bytes: actual_size as u64,
                            data_type,
                            extension,
                            byte_runs: vec![ByteRun::physical(abs_offset, actual_size as u64)],
                            confidence: compute_confidence(sector, actual_size, estimated_size),
                        });
                    }
                }
            }

            offset += bytes_read as u64;
        }
    }

    Ok(candidates)
}

/// Compute the free (unallocated) ranges given the total size and allocated ranges.
fn compute_free_ranges(total_size: u64, allocated: &[(u64, u64)]) -> Vec<(u64, u64)> {
    let mut sorted: Vec<(u64, u64)> = allocated.to_vec();
    sorted.sort_by_key(|&(start, _)| start);

    // Merge overlapping ranges
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for (start, end) in sorted {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }

    // Compute gaps between merged ranges
    let mut free = Vec::new();
    let mut cursor = 0u64;

    for (start, end) in &merged {
        if *start > cursor {
            free.push((cursor, *start));
        }
        cursor = *end;
    }

    if cursor < total_size {
        free.push((cursor, total_size));
    }

    free
}

/// Identify a file signature at the start of a buffer.
/// Returns (data_type, extension, estimated_max_size) or None.
fn identify_signature(buf: &[u8]) -> Option<(&'static str, &'static str, usize)> {
    if buf.len() < 4 {
        return None;
    }

    // JPEG
    if buf.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(("image", "jpg", 16 * 1024 * 1024));
    }
    // PNG
    if buf.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some(("image", "png", 16 * 1024 * 1024));
    }
    // PDF
    if buf.starts_with(b"%PDF") {
        return Some(("document", "pdf", 64 * 1024 * 1024));
    }
    // ZIP/DOCX/XLSX
    if buf.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
        return Some(("archive", "zip", 64 * 1024 * 1024));
    }
    // GIF
    if buf.starts_with(b"GIF8") {
        return Some(("image", "gif", 8 * 1024 * 1024));
    }
    // MP4/MOV (ftyp at offset 4)
    if buf.len() >= 8 && &buf[4..8] == b"ftyp" {
        return Some(("video", "mp4", 512 * 1024 * 1024));
    }
    // RIFF (AVI/WAV)
    if buf.starts_with(b"RIFF") && buf.len() >= 12 {
        if &buf[8..12] == b"AVI " {
            return Some(("video", "avi", 512 * 1024 * 1024));
        }
        if &buf[8..12] == b"WAVE" {
            return Some(("audio", "wav", 256 * 1024 * 1024));
        }
    }
    // MKV/WebM
    if buf.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some(("video", "mkv", 512 * 1024 * 1024));
    }
    // MP3 (ID3)
    if buf.starts_with(b"ID3") {
        return Some(("audio", "mp3", 64 * 1024 * 1024));
    }
    // MP3 (sync word)
    if buf[0] == 0xFF && (buf[1] & 0xE0) == 0xE0 {
        return Some(("audio", "mp3", 32 * 1024 * 1024));
    }
    // OLE2 (DOC/XLS/PPT)
    if buf.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        return Some(("document", "doc", 64 * 1024 * 1024));
    }
    // SQLite
    if buf.starts_with(b"SQLite format 3") {
        return Some(("database", "sqlite", 256 * 1024 * 1024));
    }
    // ELF
    if buf.starts_with(&[0x7F, b'E', b'L', b'F']) {
        return Some(("executable", "elf", 256 * 1024 * 1024));
    }
    // PE/EXE
    if buf.starts_with(&[0x4D, 0x5A]) {
        return Some(("executable", "exe", 256 * 1024 * 1024));
    }
    // FLAC
    if buf.starts_with(b"fLaC") {
        return Some(("audio", "flac", 256 * 1024 * 1024));
    }
    // OGG
    if buf.starts_with(b"OggS") {
        return Some(("audio", "ogg", 128 * 1024 * 1024));
    }
    // BMP
    if buf.starts_with(&[0x42, 0x4D]) && buf.len() >= 6 {
        let size = u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]) as usize;
        if size > 26 && size < 64 * 1024 * 1024 {
            return Some(("image", "bmp", size));
        }
    }
    // TIFF (LE)
    if buf.starts_with(&[0x49, 0x49, 0x2A, 0x00]) {
        return Some(("image", "tiff", 64 * 1024 * 1024));
    }
    // TIFF (BE)
    if buf.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]) {
        return Some(("image", "tiff", 64 * 1024 * 1024));
    }
    // RAR
    if buf.starts_with(&[0x52, 0x61, 0x72, 0x21]) {
        return Some(("archive", "rar", 512 * 1024 * 1024));
    }
    // 7Z
    if buf.starts_with(&[0x37, 0x7A, 0xBC, 0xAF]) {
        return Some(("archive", "7z", 512 * 1024 * 1024));
    }

    None
}

fn compute_confidence(header: &[u8], actual_size: usize, estimated_size: usize) -> u8 {
    let mut score = 50u8;

    // Longer header match = more confidence
    if header.len() >= 8 {
        score += 10;
    }
    if header.len() >= 16 {
        score += 5;
    }

    // Size ratio: closer to estimated = better
    if actual_size >= estimated_size {
        score += 15;
    } else if actual_size >= estimated_size / 2 {
        score += 8;
    }

    // Non-zero content after header = more confidence
    let non_zero = header[4..header.len().min(64)]
        .iter()
        .filter(|&&b| b != 0)
        .count();
    if non_zero > 20 {
        score += 10;
    }

    score.min(90)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_free_ranges_identifies_gaps() {
        let free = compute_free_ranges(1000, &[(100, 200), (300, 400), (600, 800)]);
        assert_eq!(free, vec![(0, 100), (200, 300), (400, 600), (800, 1000)]);
    }

    #[test]
    fn compute_free_ranges_handles_overlapping() {
        let free = compute_free_ranges(1000, &[(100, 300), (200, 400)]);
        assert_eq!(free, vec![(0, 100), (400, 1000)]);
    }

    #[test]
    fn identify_signature_detects_jpeg() {
        let buf = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0, 0, 0, 0, 0, 0,
        ];
        let result = identify_signature(&buf);
        assert!(result.is_some());
        assert_eq!(result.unwrap().1, "jpg");
    }

    #[test]
    fn identify_signature_detects_mp4() {
        let buf = [
            0x00, 0x00, 0x00, 0x20, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm', 0, 0, 0, 0,
        ];
        let result = identify_signature(&buf);
        assert!(result.is_some());
        assert_eq!(result.unwrap().1, "mp4");
    }
}
