#![allow(dead_code)]
// ============================================================================
// Recupere — Corrupted Filesystem Fallback Recovery
// ============================================================================
// When the primary filesystem analyzer fails (damaged superblock, corrupted
// MFT, broken catalog tree), this module attempts recovery through:
//
// 1. Superblock backup scanning (ext4 has backups at group boundaries)
// 2. MFT mirror recovery (NTFS stores MFT mirror at known locations)
// 3. Raw inode/MFT record scanning (brute-force search for valid records)
// 4. Filesystem structure reconstruction from remaining metadata
// ============================================================================

use crate::types::ByteRun;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

/// Result of a fallback recovery attempt.
#[derive(Debug, Clone)]
pub struct FallbackRecoveryCandidate {
    pub name: String,
    pub extension: String,
    pub path: String,
    pub size_bytes: u64,
    pub integrity: String,
    pub recovery_score: u8,
    pub start_offset: u64,
    pub byte_runs: Vec<ByteRun>,
    pub recovery_method: String,
    pub source_description: String,
}

/// Attempt ext4 recovery using backup superblocks.
/// ext4 stores superblock backups at the start of block groups that are
/// powers of 3, 5, and 7 (groups 0, 1, 3, 5, 7, 9, 25, 27, 49, ...).
pub fn recover_ext4_from_backup_superblock(
    image_path: &Path,
) -> Result<Vec<FallbackRecoveryCandidate>, String> {
    let mut reader =
        File::open(image_path).map_err(|e| format!("Cannot open image for ext4 fallback: {e}"))?;
    let file_size = reader
        .metadata()
        .map_err(|e| format!("Cannot read image metadata: {e}"))?
        .len();

    // Try common block sizes and backup superblock locations
    let block_sizes: &[u64] = &[1024, 2048, 4096];
    let backup_groups: &[u64] = &[1, 3, 5, 7, 9, 25, 27, 49, 81, 125, 243];

    for &block_size in block_sizes {
        let blocks_per_group = block_size * 8; // Default for most ext4
        for &group in backup_groups {
            let sb_offset =
                group * blocks_per_group * block_size + if block_size == 1024 { 1024 } else { 0 };
            if sb_offset + 1024 > file_size {
                continue;
            }

            let mut sb_bytes = [0u8; 1024];
            if reader.seek(SeekFrom::Start(sb_offset)).is_err() {
                continue;
            }
            if reader.read_exact(&mut sb_bytes).is_err() {
                continue;
            }

            // Check ext4 magic at offset 56
            let magic = u16::from_le_bytes([sb_bytes[56], sb_bytes[57]]);
            if magic != 0xef53 {
                continue;
            }

            // Valid backup superblock found — attempt recovery using it
            let inodes_per_group =
                u32::from_le_bytes([sb_bytes[40], sb_bytes[41], sb_bytes[42], sb_bytes[43]]);
            let inode_size = u16::from_le_bytes([sb_bytes[88], sb_bytes[89]]);
            let inode_size = if inode_size == 0 { 128u16 } else { inode_size };

            // Scan for valid inodes near the backup superblock's group
            let inode_table_offset = sb_offset + block_size * 2; // Approximate
            let candidates = scan_raw_inodes(
                &mut reader,
                file_size,
                inode_table_offset,
                inode_size as u64,
                inodes_per_group as usize,
                block_size,
                &format!("ext4-backup-group-{group}"),
            )?;

            if !candidates.is_empty() {
                return Ok(candidates);
            }
        }
    }

    Ok(Vec::new())
}

/// Attempt NTFS recovery using the MFT mirror ($MFTMirr).
/// NTFS stores a partial mirror of the MFT at a known location (typically
/// the middle of the volume).
pub fn recover_ntfs_from_mft_mirror(
    image_path: &Path,
) -> Result<Vec<FallbackRecoveryCandidate>, String> {
    let mut reader =
        File::open(image_path).map_err(|e| format!("Cannot open image for NTFS fallback: {e}"))?;
    let file_size = reader
        .metadata()
        .map_err(|e| format!("Cannot read image metadata: {e}"))?
        .len();

    // Read boot sector to find MFT mirror location
    let mut boot = [0u8; 512];
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|e| format!("Cannot seek boot sector: {e}"))?;
    reader
        .read_exact(&mut boot)
        .map_err(|e| format!("Cannot read boot sector: {e}"))?;

    // Check NTFS signature
    if &boot[3..7] != b"NTFS" {
        return Ok(Vec::new());
    }

    let bytes_per_sector = u16::from_le_bytes([boot[11], boot[12]]) as u64;
    let sectors_per_cluster = boot[13] as u64;
    if bytes_per_sector == 0 || sectors_per_cluster == 0 {
        return Ok(Vec::new());
    }
    let cluster_size = bytes_per_sector * sectors_per_cluster;

    // MFT mirror cluster is at offset 56 in boot sector (LE u64)
    let mft_mirror_cluster = u64::from_le_bytes([
        boot[56], boot[57], boot[58], boot[59], boot[60], boot[61], boot[62], boot[63],
    ]);

    let mirror_offset = mft_mirror_cluster * cluster_size;
    if mirror_offset + 4096 > file_size {
        return Ok(Vec::new());
    }

    // MFT mirror contains first 4 MFT records — scan for FILE records around it
    let mft_record_size = 1024u64; // Standard MFT record size
    let mut candidates = Vec::new();

    // Also scan from MFT mirror location outward for additional FILE records
    let scan_range = (mirror_offset..mirror_offset + 1024 * 1024).step_by(mft_record_size as usize);
    for offset in scan_range {
        if offset + mft_record_size > file_size {
            break;
        }

        let mut record = vec![0u8; mft_record_size as usize];
        if reader.seek(SeekFrom::Start(offset)).is_err() {
            continue;
        }
        if reader.read_exact(&mut record).is_err() {
            continue;
        }

        // Check for FILE signature
        if &record[0..4] != b"FILE" {
            continue;
        }

        // Parse basic MFT record fields
        let flags = u16::from_le_bytes([record[22], record[23]]);
        let is_in_use = flags & 0x01 != 0;
        let is_directory = flags & 0x02 != 0;

        if is_in_use || is_directory {
            continue; // We want deleted files
        }

        // Extract file size from DATA attribute if present
        let first_attr_offset = u16::from_le_bytes([record[20], record[21]]) as usize;
        if let Some(file_info) = extract_mft_file_info(&record, first_attr_offset, cluster_size) {
            candidates.push(FallbackRecoveryCandidate {
                name: file_info.name,
                extension: file_info.extension,
                path: "/mft-mirror-recovered".into(),
                size_bytes: file_info.size_bytes,
                integrity: "partial".into(),
                recovery_score: 55,
                start_offset: offset,
                byte_runs: file_info.byte_runs,
                recovery_method: "mft-mirror".into(),
                source_description: format!("MFT mirror at offset 0x{:X}", mirror_offset),
            });
        }
    }

    Ok(candidates)
}

/// Brute-force scan for raw inodes in an ext4 image.
fn scan_raw_inodes(
    reader: &mut File,
    file_size: u64,
    start_offset: u64,
    inode_size: u64,
    max_inodes: usize,
    block_size: u64,
    source: &str,
) -> Result<Vec<FallbackRecoveryCandidate>, String> {
    let mut candidates = Vec::new();
    let mut offset = start_offset;
    let max_offset = (start_offset + max_inodes as u64 * inode_size).min(file_size);

    while offset + inode_size <= max_offset && candidates.len() < 500 {
        let mut inode_bytes = vec![0u8; inode_size as usize];
        if reader.seek(SeekFrom::Start(offset)).is_err() {
            break;
        }
        if reader.read_exact(&mut inode_bytes).is_err() {
            break;
        }

        let mode = u16::from_le_bytes([inode_bytes[0], inode_bytes[1]]);
        let size = u32::from_le_bytes([
            inode_bytes[4],
            inode_bytes[5],
            inode_bytes[6],
            inode_bytes[7],
        ]) as u64;
        let dtime = u32::from_le_bytes([
            inode_bytes[20],
            inode_bytes[21],
            inode_bytes[22],
            inode_bytes[23],
        ]);

        // Regular file, has size, has deletion time
        let is_regular = mode & 0x8000 != 0;
        if is_regular && size > 0 && dtime > 0 && inode_bytes.len() >= 100 {
            // Try to extract extent or block pointers
            let flags = u32::from_le_bytes([
                inode_bytes[32],
                inode_bytes[33],
                inode_bytes[34],
                inode_bytes[35],
            ]);
            let has_extents = flags & 0x0008_0000 != 0;

            if has_extents {
                // Parse extent header at offset 40
                let extent_magic = u16::from_le_bytes([inode_bytes[40], inode_bytes[41]]);
                if extent_magic == 0xf30a {
                    let entries = u16::from_le_bytes([inode_bytes[42], inode_bytes[43]]) as usize;
                    let depth = u16::from_le_bytes([inode_bytes[46], inode_bytes[47]]);

                    if depth == 0 && entries > 0 && entries <= 4 {
                        let mut byte_runs = Vec::new();
                        let mut total_recoverable = 0u64;

                        for e in 0..entries {
                            let ext_offset = 52 + e * 12;
                            if ext_offset + 12 > inode_bytes.len() {
                                break;
                            }

                            let start_block = {
                                let lo = u32::from_le_bytes([
                                    inode_bytes[ext_offset + 4],
                                    inode_bytes[ext_offset + 5],
                                    inode_bytes[ext_offset + 6],
                                    inode_bytes[ext_offset + 7],
                                ]) as u64;
                                let hi = u16::from_le_bytes([
                                    inode_bytes[ext_offset + 2],
                                    inode_bytes[ext_offset + 3],
                                ]) as u64;
                                (hi << 32) | lo
                            };
                            let length = u16::from_le_bytes([
                                inode_bytes[ext_offset],
                                inode_bytes[ext_offset + 1],
                            ]) as u64;

                            if start_block > 0 && length > 0 {
                                let run_offset = start_block * block_size;
                                let run_length =
                                    (length * block_size).min(size - total_recoverable);
                                byte_runs.push(ByteRun::physical(run_offset, run_length));
                                total_recoverable += run_length;
                            }
                        }

                        if !byte_runs.is_empty() {
                            // Read first bytes to infer extension
                            let mut preview = [0u8; 16];
                            if let Ok(()) = {
                                reader
                                    .seek(SeekFrom::Start(byte_runs[0].offset))
                                    .and_then(|_| reader.read_exact(&mut preview))
                                    .map_err(|_| ())
                            } {
                                let ext = infer_extension_from_magic(&preview);
                                let name = format!("fallback-{:08X}.{}", offset, ext);
                                candidates.push(FallbackRecoveryCandidate {
                                    name,
                                    extension: ext.to_string(),
                                    path: format!("/{source}"),
                                    size_bytes: total_recoverable,
                                    integrity: if total_recoverable >= size {
                                        "intact"
                                    } else {
                                        "partial"
                                    }
                                    .into(),
                                    recovery_score: if total_recoverable >= size { 62 } else { 45 },
                                    start_offset: byte_runs[0].offset,
                                    byte_runs,
                                    recovery_method: "fallback-inode-scan".into(),
                                    source_description: format!(
                                        "Raw inode at 0x{:X} via {source}",
                                        offset
                                    ),
                                });
                            }
                        }
                    }
                }
            }
        }

        offset += inode_size;
    }

    Ok(candidates)
}

struct MftFileInfo {
    name: String,
    extension: String,
    size_bytes: u64,
    byte_runs: Vec<ByteRun>,
}

fn extract_mft_file_info(
    record: &[u8],
    first_attr_offset: usize,
    cluster_size: u64,
) -> Option<MftFileInfo> {
    let mut offset = first_attr_offset;
    let mut name = String::new();
    let mut extension = String::new();
    let mut size_bytes = 0u64;
    let mut byte_runs = Vec::new();

    while offset + 4 < record.len() {
        let attr_type = u32::from_le_bytes([
            record[offset],
            record[offset + 1],
            record[offset + 2],
            record[offset + 3],
        ]);
        if attr_type == 0xFFFF_FFFF || attr_type == 0 {
            break;
        }
        let attr_length = u32::from_le_bytes([
            record[offset + 4],
            record[offset + 5],
            record[offset + 6],
            record[offset + 7],
        ]) as usize;
        if attr_length < 16 || offset + attr_length > record.len() {
            break;
        }

        match attr_type {
            0x30 => {
                // FILE_NAME attribute
                let is_resident = record[offset + 8] == 0;
                if is_resident {
                    let content_offset =
                        u16::from_le_bytes([record[offset + 20], record[offset + 21]]) as usize;
                    let abs = offset + content_offset;
                    if abs + 66 < record.len() {
                        let name_len = record[abs + 64] as usize;
                        if abs + 66 + name_len * 2 <= record.len() {
                            let name_bytes = &record[abs + 66..abs + 66 + name_len * 2];
                            name = name_bytes
                                .chunks_exact(2)
                                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                                .map(|c| char::from_u32(c as u32).unwrap_or('?'))
                                .collect();
                            if let Some(dot) = name.rfind('.') {
                                extension = name[dot + 1..].to_lowercase();
                            }
                        }
                    }
                }
            }
            0x80 => {
                // DATA attribute
                let is_resident = record[offset + 8] == 0;
                if !is_resident && offset + 48 < record.len() {
                    size_bytes = u64::from_le_bytes([
                        record[offset + 48],
                        record[offset + 49],
                        record[offset + 50],
                        record[offset + 51],
                        record[offset + 52],
                        record[offset + 53],
                        record[offset + 54],
                        record[offset + 55],
                    ]);
                    // Parse data runs
                    let runs_offset =
                        u16::from_le_bytes([record[offset + 32], record[offset + 33]]) as usize;
                    let abs = offset + runs_offset;
                    let mut run_offset = abs;
                    let mut current_lcn: i64 = 0;

                    while run_offset < offset + attr_length && run_offset < record.len() {
                        let header_byte = record[run_offset];
                        if header_byte == 0 {
                            break;
                        }

                        let length_size = (header_byte & 0x0F) as usize;
                        let offset_size = ((header_byte >> 4) & 0x0F) as usize;
                        run_offset += 1;

                        if run_offset + length_size + offset_size > record.len() {
                            break;
                        }

                        let mut length_val = 0u64;
                        for i in 0..length_size {
                            length_val |= (record[run_offset + i] as u64) << (i * 8);
                        }
                        run_offset += length_size;

                        if offset_size > 0 {
                            let mut offset_val = 0i64;
                            for i in 0..offset_size {
                                offset_val |= (record[run_offset + i] as i64) << (i * 8);
                            }
                            // Sign extend
                            if offset_size < 8 && (record[run_offset + offset_size - 1] & 0x80) != 0
                            {
                                for i in offset_size..8 {
                                    offset_val |= 0xFFi64 << (i * 8);
                                }
                            }
                            run_offset += offset_size;
                            current_lcn += offset_val;

                            if current_lcn > 0 && length_val > 0 {
                                byte_runs.push(ByteRun::physical(
                                    current_lcn as u64 * cluster_size,
                                    length_val * cluster_size,
                                ));
                            }
                        } else {
                            // Sparse run
                            run_offset += offset_size;
                        }
                    }
                }
            }
            _ => {}
        }

        offset += attr_length;
    }

    if name.is_empty() || byte_runs.is_empty() {
        return None;
    }

    Some(MftFileInfo {
        name,
        extension,
        size_bytes,
        byte_runs,
    })
}

fn infer_extension_from_magic(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return "jpg";
    }
    if bytes.starts_with(b"\x89PNG") {
        return "png";
    }
    if bytes.starts_with(b"%PDF") {
        return "pdf";
    }
    if bytes.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
        return "zip";
    }
    if bytes.starts_with(b"GIF8") {
        return "gif";
    }
    if bytes.starts_with(b"RIFF") {
        return "avi";
    }
    if bytes.starts_with(b"ID3") {
        return "mp3";
    }
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        return "doc";
    }
    if bytes.starts_with(b"SQLite") {
        return "sqlite";
    }
    if bytes.starts_with(&[0x7F, 0x45, 0x4C, 0x46]) {
        return "elf";
    }
    if bytes.starts_with(&[0x4D, 0x5A]) {
        return "exe";
    }
    "bin"
}

/// Attempt FAT32 recovery by scanning for backup boot sector.
/// FAT32 stores a backup boot sector at sector 6.
pub fn recover_fat32_from_backup_boot(
    image_path: &Path,
) -> Result<Vec<FallbackRecoveryCandidate>, String> {
    let mut reader =
        File::open(image_path).map_err(|e| format!("Cannot open image for FAT32 fallback: {e}"))?;
    let file_size = reader
        .metadata()
        .map_err(|e| format!("Cannot read metadata: {e}"))?
        .len();

    // FAT32 backup boot sector at sector 6 (offset 3072)
    let backup_offset = 6 * 512;
    if backup_offset + 512 > file_size {
        return Ok(Vec::new());
    }

    let mut boot = [0u8; 512];
    reader
        .seek(SeekFrom::Start(backup_offset))
        .map_err(|e| format!("seek: {e}"))?;
    reader
        .read_exact(&mut boot)
        .map_err(|e| format!("read: {e}"))?;

    // Check for valid FAT32 boot signature (0x55AA at end)
    if boot[510] != 0x55 || boot[511] != 0xAA {
        return Ok(Vec::new());
    }

    // Check for FAT32 filesystem type string at offset 82
    let fs_type = &boot[82..90];
    if !fs_type.starts_with(b"FAT32") {
        return Ok(Vec::new());
    }

    let bytes_per_sector = u16::from_le_bytes([boot[11], boot[12]]) as u64;
    let sectors_per_cluster = boot[13] as u64;
    if bytes_per_sector == 0 || sectors_per_cluster == 0 {
        return Ok(Vec::new());
    }

    let cluster_size = bytes_per_sector * sectors_per_cluster;
    let root_cluster = u32::from_le_bytes([boot[44], boot[45], boot[46], boot[47]]) as u64;
    let reserved_sectors = u16::from_le_bytes([boot[14], boot[15]]) as u64;
    let fat_count = boot[16] as u64;
    let fat_size = u32::from_le_bytes([boot[36], boot[37], boot[38], boot[39]]) as u64;
    let data_start = (reserved_sectors + fat_count * fat_size) * bytes_per_sector;

    // Scan directory entries in root cluster area for deleted entries (0xE5 marker)
    let root_offset = data_start + (root_cluster - 2) * cluster_size;
    let mut candidates = Vec::new();
    let scan_size = (cluster_size * 16).min(file_size - root_offset) as usize;
    let mut dir_buf = vec![0u8; scan_size];

    if reader.seek(SeekFrom::Start(root_offset)).is_ok() && reader.read_exact(&mut dir_buf).is_ok()
    {
        for i in (0..dir_buf.len()).step_by(32) {
            if i + 32 > dir_buf.len() {
                break;
            }
            let entry = &dir_buf[i..i + 32];

            // 0xE5 = deleted entry marker
            if entry[0] != 0xE5 {
                continue;
            }
            // Skip directories and volume labels
            if entry[11] & 0x18 != 0 {
                continue;
            }

            let size = u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]) as u64;
            if size == 0 {
                continue;
            }

            let first_cluster = (u16::from_le_bytes([entry[20], entry[21]]) as u64) << 16
                | u16::from_le_bytes([entry[26], entry[27]]) as u64;
            if first_cluster < 2 {
                continue;
            }

            let file_offset = data_start + (first_cluster - 2) * cluster_size;
            if file_offset + size > file_size {
                continue;
            }

            // Reconstruct name (replace 0xE5 with _)
            let mut name_bytes = entry[0..8].to_vec();
            name_bytes[0] = b'_';
            let name = String::from_utf8_lossy(&name_bytes).trim().to_string();
            let ext_str = String::from_utf8_lossy(&entry[8..11]).trim().to_lowercase();
            let full_name = if ext_str.is_empty() {
                name.clone()
            } else {
                format!("{name}.{ext_str}")
            };

            candidates.push(FallbackRecoveryCandidate {
                name: full_name,
                extension: ext_str,
                path: "/fat32-fallback".into(),
                size_bytes: size,
                integrity: "partial".into(),
                recovery_score: 55,
                start_offset: file_offset,
                byte_runs: vec![ByteRun::physical(file_offset, size.min(cluster_size * 64))],
                recovery_method: "fat32-backup-boot".into(),
                source_description: "FAT32 backup boot sector recovery".into(),
            });

            if candidates.len() >= 500 {
                break;
            }
        }
    }

    Ok(candidates)
}

/// Attempt HFS+ recovery by scanning for alternate volume header.
/// HFS+ stores an alternate volume header at the second-to-last 512-byte sector.
pub fn recover_hfsplus_from_alternate_header(
    image_path: &Path,
) -> Result<Vec<FallbackRecoveryCandidate>, String> {
    let mut reader =
        File::open(image_path).map_err(|e| format!("Cannot open image for HFS+ fallback: {e}"))?;
    let file_size = reader
        .metadata()
        .map_err(|e| format!("Cannot read metadata: {e}"))?
        .len();

    if file_size < 2048 {
        return Ok(Vec::new());
    }

    // Alternate volume header is at offset (volume_size - 1024)
    let alt_offset = file_size - 1024;
    let mut header = [0u8; 512];
    reader
        .seek(SeekFrom::Start(alt_offset))
        .map_err(|e| format!("seek: {e}"))?;
    reader
        .read_exact(&mut header)
        .map_err(|e| format!("read: {e}"))?;

    // Check for HFS+ or HFSX signature
    let sig = u16::from_be_bytes([header[0], header[1]]);
    if sig != 0x482B && sig != 0x4858 {
        // H+ or HX
        return Ok(Vec::new());
    }

    let block_size = u32::from_be_bytes([header[40], header[41], header[42], header[43]]) as u64;
    if block_size == 0 {
        return Ok(Vec::new());
    }

    // Successfully found alternate header — this confirms a valid HFS+ volume
    // Use block_size to do a brute-force catalog scan around known offsets
    Ok(Vec::new()) // Catalog reconstruction would go here
}

/// Attempt APFS recovery by scanning for checkpoint superblocks.
/// APFS stores checkpoint superblocks at known offsets in the container.
pub fn recover_apfs_from_checkpoint(
    image_path: &Path,
) -> Result<Vec<FallbackRecoveryCandidate>, String> {
    let mut reader =
        File::open(image_path).map_err(|e| format!("Cannot open image for APFS fallback: {e}"))?;
    let file_size = reader
        .metadata()
        .map_err(|e| format!("Cannot read metadata: {e}"))?
        .len();

    if file_size < 4096 {
        return Ok(Vec::new());
    }

    // APFS container superblock magic: 'NXSB' at offset 32 of each block
    // Scan first few blocks for valid APFS superblocks
    let block_size = 4096u64;
    let max_blocks = (file_size / block_size).min(1024);

    for block_idx in 0..max_blocks {
        let offset = block_idx * block_size;
        let mut buf = [0u8; 64];
        if reader.seek(SeekFrom::Start(offset + 32)).is_err() {
            continue;
        }
        if reader.read_exact(&mut buf).is_err() {
            continue;
        }

        // Check for NXSB magic
        if &buf[0..4] == b"NXSB" {
            // Found a container superblock — checkpoint-based recovery could proceed here
            // Extract checkpoint descriptor area offset and scan for object maps
            return Ok(Vec::new()); // Detailed reconstruction would go here
        }
    }

    Ok(Vec::new())
}

/// Universal fallback: scan entire image for any valid filesystem structures.
/// This is the last resort when no specific filesystem can be identified.
pub fn scan_for_any_filesystem_structures(
    image_path: &Path,
) -> Result<Vec<FallbackRecoveryCandidate>, String> {
    let mut reader =
        File::open(image_path).map_err(|e| format!("Cannot open image for universal scan: {e}"))?;
    let file_size = reader
        .metadata()
        .map_err(|e| format!("Cannot read metadata: {e}"))?
        .len();

    let mut candidates = Vec::new();

    // Scan at 512-byte aligned offsets for known filesystem signatures
    let scan_step = 4096u64; // Align to common block size
    let max_scan = file_size.min(512 * 1024 * 1024); // Scan first 512MB

    let mut offset = 0u64;
    while offset < max_scan && candidates.len() < 200 {
        let mut buf = [0u8; 512];
        if reader.seek(SeekFrom::Start(offset)).is_err() {
            break;
        }
        if reader.read_exact(&mut buf).is_err() {
            break;
        }

        // Check for NTFS boot sector
        if &buf[3..7] == b"NTFS" && buf[510] == 0x55 && buf[511] == 0xAA {
            candidates.push(FallbackRecoveryCandidate {
                name: format!("ntfs-structure-at-{:08X}", offset),
                extension: "ntfs".into(),
                path: "/universal-scan".into(),
                size_bytes: 0,
                integrity: "unknown".into(),
                recovery_score: 30,
                start_offset: offset,
                byte_runs: vec![],
                recovery_method: "universal-structure-scan".into(),
                source_description: format!("NTFS boot sector at offset 0x{:X}", offset),
            });
        }

        // Check for ext4 superblock magic
        if offset >= 1024 {
            let mut sb = [0u8; 2];
            if reader.seek(SeekFrom::Start(offset + 56)).is_ok()
                && reader.read_exact(&mut sb).is_ok()
                && u16::from_le_bytes(sb) == 0xef53
            {
                candidates.push(FallbackRecoveryCandidate {
                    name: format!("ext4-structure-at-{:08X}", offset),
                    extension: "ext4".into(),
                    path: "/universal-scan".into(),
                    size_bytes: 0,
                    integrity: "unknown".into(),
                    recovery_score: 30,
                    start_offset: offset,
                    byte_runs: vec![],
                    recovery_method: "universal-structure-scan".into(),
                    source_description: format!("ext4 superblock at offset 0x{:X}", offset),
                });
            }
        }

        // Check for FAT32 boot sector
        if buf[510] == 0x55 && buf[511] == 0xAA && buf[82..90].starts_with(b"FAT32") {
            candidates.push(FallbackRecoveryCandidate {
                name: format!("fat32-structure-at-{:08X}", offset),
                extension: "fat32".into(),
                path: "/universal-scan".into(),
                size_bytes: 0,
                integrity: "unknown".into(),
                recovery_score: 30,
                start_offset: offset,
                byte_runs: vec![],
                recovery_method: "universal-structure-scan".into(),
                source_description: format!("FAT32 boot sector at offset 0x{:X}", offset),
            });
        }

        offset += scan_step;
    }

    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_extension_recognizes_common_formats() {
        assert_eq!(infer_extension_from_magic(&[0xFF, 0xD8, 0xFF, 0xE0]), "jpg");
        assert_eq!(infer_extension_from_magic(b"\x89PNG\r\n\x1a\n"), "png");
        assert_eq!(infer_extension_from_magic(b"%PDF-1.7"), "pdf");
        assert_eq!(infer_extension_from_magic(&[0x50, 0x4B, 0x03, 0x04]), "zip");
        assert_eq!(infer_extension_from_magic(&[0x00, 0x00, 0x00, 0x00]), "bin");
    }
}
