use crate::{
    analyzers::hfsplus,
    types::{FilesystemType, PotentialVolume},
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const LOGICAL_SECTOR_SIZE: u64 = 512;
const MIB: u64 = 1024 * 1024;
const SIGNATURE_SCAN_LIMIT_BYTES: u64 = 128 * MIB;
const APFS_MIN_BLOCK_SIZE: u64 = 4096;
const APFS_MAX_BLOCK_SIZE: u64 = 65536;
const APFS_NXSB_MAGIC: &[u8; 4] = b"NXSB";
const GPT_APFS_PARTITION_TYPE: [u8; 16] = [
    0xEF, 0x57, 0x34, 0x7C, 0x00, 0x00, 0xAA, 0x11, 0xAA, 0x11, 0x00, 0x30, 0x65, 0x43, 0xEC, 0xAC,
];

#[derive(Debug, Clone)]
struct BootSectorMatch {
    filesystem: FilesystemType,
    size_bytes: Option<u64>,
    notes: Vec<String>,
}

#[derive(Debug, Clone)]
struct GptHeader {
    entries_lba: u64,
    entry_count: u32,
    entry_size: u32,
}

pub fn inspect_potential_volumes(source_path: &Path) -> Result<Vec<PotentialVolume>, String> {
    let mut reader = File::open(source_path).map_err(|error| {
        format!(
            "Unable to open the raw source {} for conservative partition inspection: {}",
            source_path.to_string_lossy(),
            error
        )
    })?;
    let source_size = reader
        .metadata()
        .map_err(|error| format!("Unable to inspect the raw source metadata: {error}"))?
        .len();
    let source_size = (source_size > 0).then_some(source_size);

    let mut volumes = BTreeMap::<u64, PotentialVolume>::new();

    if let Ok(mbr_volumes) = parse_mbr_partitions(&mut reader, source_size) {
        for volume in mbr_volumes {
            merge_volume(&mut volumes, volume);
        }
    }

    if let Ok(gpt_volumes) = parse_gpt_partitions(&mut reader, 1, "gpt", 96, source_size) {
        for volume in gpt_volumes {
            merge_volume(&mut volumes, volume);
        }
    }

    let backup_header_lba = source_size
        .and_then(|size| size.checked_div(LOGICAL_SECTOR_SIZE))
        .and_then(|sectors| sectors.checked_sub(1));
    if let Some(lba) = backup_header_lba {
        if let Ok(gpt_backup_volumes) =
            parse_gpt_partitions(&mut reader, lba, "gpt-backup", 92, source_size)
        {
            for volume in gpt_backup_volumes {
                merge_volume(&mut volumes, volume);
            }
        }
    }

    for offset in signature_scan_offsets(source_size) {
        if volumes.contains_key(&offset) {
            continue;
        }

        if let Some(signature_match) = detect_boot_sector(&mut reader, offset, source_size)? {
            merge_volume(
                &mut volumes,
                PotentialVolume {
                    id: format!("pv-{:x}", offset),
                    label: format!(
                        "Potential {} volume @ 0x{:X}",
                        filesystem_label(&signature_match.filesystem),
                        offset
                    ),
                    filesystem: signature_match.filesystem,
                    start_offset: offset,
                    size_bytes: signature_match.size_bytes,
                    confidence_score: 74,
                    detection_method: "boot-signature".into(),
                    notes: signature_match.notes,
                },
            );
        }
    }

    Ok(volumes.into_values().collect())
}

fn parse_mbr_partitions(
    reader: &mut File,
    source_size: Option<u64>,
) -> Result<Vec<PotentialVolume>, String> {
    let sector = read_sector(reader, 0)?;
    if sector[510] != 0x55 || sector[511] != 0xAA {
        return Err("No valid MBR signature found.".into());
    }

    let mut volumes = Vec::new();
    for index in 0..4 {
        let offset = 446 + index * 16;
        let entry = &sector[offset..offset + 16];
        let partition_type = entry[4];
        let start_lba = le_u32(&entry[8..12]) as u64;
        let sector_count = le_u32(&entry[12..16]) as u64;

        if partition_type == 0 || start_lba == 0 || sector_count == 0 || partition_type == 0xEE {
            continue;
        }

        let start_offset = start_lba.saturating_mul(LOGICAL_SECTOR_SIZE);
        let size_bytes = sector_count.saturating_mul(LOGICAL_SECTOR_SIZE);
        if source_size.is_some_and(|size| start_offset >= size) {
            continue;
        }

        let signature_match = detect_boot_sector(reader, start_offset, source_size)?;
        let filesystem = signature_match
            .as_ref()
            .map(|candidate| candidate.filesystem.clone())
            .unwrap_or_else(|| filesystem_from_mbr_type(partition_type));
        let mut notes = vec![format!(
            "Found in the MBR partition table with type 0x{:02X}.",
            partition_type
        )];
        if let Some(candidate) = signature_match {
            notes.extend(candidate.notes);
        }

        volumes.push(PotentialVolume {
            id: format!("pv-mbr-{index}"),
            label: format!("MBR partition {} @ 0x{:X}", index + 1, start_offset),
            filesystem,
            start_offset,
            size_bytes: Some(size_bytes),
            confidence_score: 90,
            detection_method: "mbr".into(),
            notes,
        });
    }

    if volumes.is_empty() {
        return Err("No usable MBR partition entry found.".into());
    }

    Ok(volumes)
}

fn parse_gpt_partitions(
    reader: &mut File,
    header_lba: u64,
    detection_method: &str,
    confidence_score: u8,
    source_size: Option<u64>,
) -> Result<Vec<PotentialVolume>, String> {
    let header_offset = header_lba.saturating_mul(LOGICAL_SECTOR_SIZE);
    let header_bytes = read_sector(reader, header_offset)?;
    if &header_bytes[0..8] != b"EFI PART" {
        return Err("No valid GPT header signature found.".into());
    }

    let header = GptHeader {
        entries_lba: le_u64(&header_bytes[72..80]),
        entry_count: le_u32(&header_bytes[80..84]).min(256),
        entry_size: le_u32(&header_bytes[84..88]),
    };
    if header.entry_count == 0 || header.entry_size < 128 || header.entry_size > 4096 {
        return Err("The GPT header does not expose usable partition entry metadata.".into());
    }

    let entries_offset = header.entries_lba.saturating_mul(LOGICAL_SECTOR_SIZE);
    let total_entries_bytes = header.entry_count as u64 * header.entry_size as u64;
    let entries = read_exact_at(reader, entries_offset, total_entries_bytes as usize)?;

    let mut volumes = Vec::new();
    for index in 0..header.entry_count as usize {
        let entry_offset = index * header.entry_size as usize;
        let entry = &entries[entry_offset..entry_offset + header.entry_size as usize];
        if entry[0..16].iter().all(|byte| *byte == 0) {
            continue;
        }

        let first_lba = le_u64(&entry[32..40]);
        let last_lba = le_u64(&entry[40..48]);
        if first_lba == 0 || last_lba < first_lba {
            continue;
        }

        let start_offset = first_lba.saturating_mul(LOGICAL_SECTOR_SIZE);
        if source_size.is_some_and(|size| start_offset >= size) {
            continue;
        }

        let size_bytes = (last_lba - first_lba + 1).saturating_mul(LOGICAL_SECTOR_SIZE);
        let signature_match = detect_boot_sector(reader, start_offset, source_size)?;
        let filesystem = signature_match
            .as_ref()
            .map(|candidate| candidate.filesystem.clone())
            .unwrap_or_else(|| {
                filesystem_from_gpt_type(&entry[0..16]).unwrap_or(FilesystemType::Unknown)
            });
        let entry_name = decode_utf16_name(&entry[56..128])
            .unwrap_or_else(|| format!("GPT partition {}", index + 1));
        let mut notes = vec![format!(
            "Found in the {} partition table at LBA {}.",
            detection_method, first_lba
        )];
        if matches!(filesystem, FilesystemType::Apfs) {
            notes.push(
                "The GPT partition type matches an APFS container. This build can attempt conservative visible-file cataloging from a recovered local slice, but deleted APFS recovery is not implemented yet."
                    .into(),
            );
        }
        if let Some(candidate) = signature_match {
            notes.extend(candidate.notes);
        }

        volumes.push(PotentialVolume {
            id: format!("pv-{detection_method}-{index}"),
            label: entry_name,
            filesystem,
            start_offset,
            size_bytes: Some(size_bytes),
            confidence_score,
            detection_method: detection_method.into(),
            notes,
        });
    }

    if volumes.is_empty() {
        return Err("No usable GPT partition entry found.".into());
    }

    Ok(volumes)
}

fn signature_scan_offsets(source_size: Option<u64>) -> Vec<u64> {
    let mut offsets = Vec::new();
    let max_offset = source_size
        .map(|size| size.min(SIGNATURE_SCAN_LIMIT_BYTES))
        .unwrap_or(SIGNATURE_SCAN_LIMIT_BYTES);
    let mut current = MIB;
    while current < max_offset {
        offsets.push(current);
        current = current.saturating_add(MIB);
    }
    offsets
}

fn detect_boot_sector(
    reader: &mut File,
    offset: u64,
    source_size: Option<u64>,
) -> Result<Option<BootSectorMatch>, String> {
    if let Some(candidate) = detect_apfs_container(reader, offset, source_size)? {
        return Ok(Some(candidate));
    }

    if let Some(candidate) = detect_hfsplus_volume(reader, offset, source_size)? {
        return Ok(Some(candidate));
    }

    if source_size.is_some_and(|size| offset.saturating_add(512) > size) {
        return Ok(None);
    }

    let sector = read_sector(reader, offset)?;
    if sector[510] != 0x55 || sector[511] != 0xAA {
        return Ok(None);
    }

    let remaining_bytes = source_size.map(|size| size.saturating_sub(offset));

    if let Some(candidate) = detect_ntfs_boot_sector(&sector, remaining_bytes) {
        return Ok(Some(candidate));
    }
    if let Some(candidate) = detect_exfat_boot_sector(&sector, remaining_bytes) {
        return Ok(Some(candidate));
    }
    if let Some(candidate) = detect_fat32_boot_sector(&sector, remaining_bytes) {
        return Ok(Some(candidate));
    }

    Ok(None)
}

fn detect_apfs_container(
    reader: &mut File,
    offset: u64,
    source_size: Option<u64>,
) -> Result<Option<BootSectorMatch>, String> {
    if source_size.is_some_and(|size| offset.saturating_add(APFS_MIN_BLOCK_SIZE) > size) {
        return Ok(None);
    }

    let bytes = read_exact_at(reader, offset, APFS_MIN_BLOCK_SIZE as usize)?;
    if &bytes[32..36] != APFS_NXSB_MAGIC {
        return Ok(None);
    }

    let block_size = le_u32(&bytes[36..40]) as u64;
    let block_count = le_u64(&bytes[40..48]);
    if block_count == 0
        || !(APFS_MIN_BLOCK_SIZE..=APFS_MAX_BLOCK_SIZE).contains(&block_size)
        || !is_power_of_two(block_size)
    {
        return Ok(None);
    }

    let size_bytes = match block_size.checked_mul(block_count) {
        Some(size_bytes) if size_bytes > 0 => size_bytes,
        _ => return Ok(None),
    };
    if source_size.is_some_and(|size| offset.saturating_add(size_bytes) > size) {
        return Ok(None);
    }

    Ok(Some(BootSectorMatch {
        filesystem: FilesystemType::Apfs,
        size_bytes: Some(size_bytes),
        notes: vec![
            "Validated by an APFS container superblock (NXSB).".into(),
            format!("Detected APFS block size: {} bytes.", block_size),
            format!("Estimated APFS container size: {} bytes.", size_bytes),
            "This MVP can attempt conservative visible-file cataloging from a recovered APFS local slice, but deleted-file recovery is not implemented yet.".into(),
        ],
    }))
}

fn detect_hfsplus_volume(
    reader: &mut File,
    offset: u64,
    source_size: Option<u64>,
) -> Result<Option<BootSectorMatch>, String> {
    let remaining_bytes = source_size.map(|size| size.saturating_sub(offset));
    let layout = match hfsplus::inspect_volume_layout(reader, offset, remaining_bytes)? {
        Some(layout) => layout,
        None => return Ok(None),
    };

    let size_bytes = layout.block_size as u64 * layout.total_blocks as u64;
    if size_bytes == 0 || remaining_bytes.is_some_and(|remaining| size_bytes > remaining) {
        return Ok(None);
    }

    let mut notes = vec![
        "Validated by an HFS+ volume header signature.".into(),
        format!("Estimated HFS+ volume size: {} bytes.", size_bytes),
    ];
    if layout.wrapped {
        notes.push(
            "The detected HFS+ volume is embedded behind an HFS wrapper and will be analyzed from its embedded offset."
                .into(),
        );
    }

    Ok(Some(BootSectorMatch {
        filesystem: FilesystemType::HfsPlus,
        size_bytes: Some(size_bytes),
        notes,
    }))
}

fn detect_ntfs_boot_sector(sector: &[u8], remaining_bytes: Option<u64>) -> Option<BootSectorMatch> {
    if &sector[3..11] != b"NTFS    " {
        return None;
    }

    let bytes_per_sector = le_u16(&sector[11..13]) as u64;
    let sectors_per_cluster = sector[13] as u64;
    let total_sectors = le_u64(&sector[40..48]);
    let mft_lcn = le_u64(&sector[48..56]);

    if !is_valid_bytes_per_sector(bytes_per_sector)
        || !is_power_of_two(sectors_per_cluster)
        || total_sectors == 0
        || mft_lcn == 0
    {
        return None;
    }

    let size_bytes = total_sectors.checked_mul(bytes_per_sector)?;
    if size_bytes == 0 || remaining_bytes.is_some_and(|remaining| size_bytes > remaining) {
        return None;
    }

    Some(BootSectorMatch {
        filesystem: FilesystemType::Ntfs,
        size_bytes: Some(size_bytes),
        notes: vec![
            "Validated by an NTFS boot-sector signature.".into(),
            format!("Estimated NTFS volume size: {} bytes.", size_bytes),
        ],
    })
}

fn detect_exfat_boot_sector(
    sector: &[u8],
    remaining_bytes: Option<u64>,
) -> Option<BootSectorMatch> {
    if &sector[3..11] != b"EXFAT   " {
        return None;
    }

    let volume_length_sectors = le_u64(&sector[72..80]);
    let bytes_per_sector_shift = sector[108];
    let sectors_per_cluster_shift = sector[109];
    let cluster_count = le_u32(&sector[92..96]);

    if !(9..=12).contains(&bytes_per_sector_shift)
        || sectors_per_cluster_shift > 25
        || cluster_count == 0
    {
        return None;
    }

    let bytes_per_sector = 1_u64 << bytes_per_sector_shift;
    let size_bytes = volume_length_sectors.checked_mul(bytes_per_sector)?;
    if size_bytes == 0 || remaining_bytes.is_some_and(|remaining| size_bytes > remaining) {
        return None;
    }

    Some(BootSectorMatch {
        filesystem: FilesystemType::Exfat,
        size_bytes: Some(size_bytes),
        notes: vec![
            "Validated by an exFAT boot-sector signature.".into(),
            format!("Estimated exFAT volume size: {} bytes.", size_bytes),
        ],
    })
}

fn detect_fat32_boot_sector(
    sector: &[u8],
    remaining_bytes: Option<u64>,
) -> Option<BootSectorMatch> {
    if &sector[82..90] != b"FAT32   " {
        return None;
    }

    let bytes_per_sector = le_u16(&sector[11..13]) as u64;
    let sectors_per_cluster = sector[13] as u64;
    let reserved_sector_count = le_u16(&sector[14..16]);
    let fat_count = sector[16];
    let total_sectors_16 = le_u16(&sector[19..21]) as u64;
    let total_sectors_32 = le_u32(&sector[32..36]) as u64;
    let fat_size_16 = le_u16(&sector[22..24]) as u64;
    let fat_size_32 = le_u32(&sector[36..40]) as u64;
    let root_cluster = le_u32(&sector[44..48]);

    if !is_valid_bytes_per_sector(bytes_per_sector)
        || !is_power_of_two(sectors_per_cluster)
        || reserved_sector_count == 0
        || fat_count == 0
        || root_cluster < 2
    {
        return None;
    }

    let total_sectors = if total_sectors_16 > 0 {
        total_sectors_16
    } else {
        total_sectors_32
    };
    let fat_size = if fat_size_16 > 0 {
        fat_size_16
    } else {
        fat_size_32
    };
    if total_sectors == 0 || fat_size == 0 {
        return None;
    }

    let size_bytes = total_sectors.checked_mul(bytes_per_sector)?;
    if size_bytes == 0 || remaining_bytes.is_some_and(|remaining| size_bytes > remaining) {
        return None;
    }

    Some(BootSectorMatch {
        filesystem: FilesystemType::Fat32,
        size_bytes: Some(size_bytes),
        notes: vec![
            "Validated by a FAT32 boot-sector signature.".into(),
            format!("Estimated FAT32 volume size: {} bytes.", size_bytes),
        ],
    })
}

fn merge_volume(volumes: &mut BTreeMap<u64, PotentialVolume>, mut incoming: PotentialVolume) {
    if let Some(existing) = volumes.get_mut(&incoming.start_offset) {
        if matches!(existing.filesystem, FilesystemType::Unknown)
            && !matches!(incoming.filesystem, FilesystemType::Unknown)
        {
            existing.filesystem = incoming.filesystem;
        }

        if existing.size_bytes.is_none() {
            existing.size_bytes = incoming.size_bytes.take();
        }

        existing.confidence_score = existing.confidence_score.max(incoming.confidence_score);

        if !existing
            .detection_method
            .contains(&incoming.detection_method)
        {
            existing.detection_method = format!(
                "{}+{}",
                existing.detection_method, incoming.detection_method
            );
        }

        for note in incoming.notes {
            if !existing.notes.contains(&note) {
                existing.notes.push(note);
            }
        }
        return;
    }

    volumes.insert(incoming.start_offset, incoming);
}

fn read_sector(reader: &mut File, offset: u64) -> Result<[u8; 512], String> {
    let mut sector = [0_u8; 512];
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|error| format!("Unable to seek the inspected source: {error}"))?;
    reader
        .read_exact(&mut sector)
        .map_err(|error| format!("Unable to read the inspected source: {error}"))?;
    Ok(sector)
}

fn read_exact_at(reader: &mut File, offset: u64, length: usize) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0_u8; length];
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|error| format!("Unable to seek partition metadata: {error}"))?;
    reader
        .read_exact(&mut bytes)
        .map_err(|error| format!("Unable to read partition metadata: {error}"))?;
    Ok(bytes)
}

fn decode_utf16_name(bytes: &[u8]) -> Option<String> {
    let mut words = Vec::new();
    for chunk in bytes.chunks_exact(2) {
        let word = le_u16(chunk);
        if word == 0 {
            break;
        }
        words.push(word);
    }

    let name = String::from_utf16_lossy(&words).trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn filesystem_from_mbr_type(partition_type: u8) -> FilesystemType {
    match partition_type {
        0x0B | 0x0C => FilesystemType::Fat32,
        0xAF => FilesystemType::HfsPlus,
        0x07 => FilesystemType::Unknown,
        _ => FilesystemType::Unknown,
    }
}

fn filesystem_from_gpt_type(bytes: &[u8]) -> Option<FilesystemType> {
    if bytes.len() < 16 {
        return None;
    }

    if bytes[0..16] == GPT_APFS_PARTITION_TYPE {
        return Some(FilesystemType::Apfs);
    }

    None
}

fn filesystem_label(filesystem: &FilesystemType) -> &'static str {
    match filesystem {
        FilesystemType::Ntfs => "NTFS",
        FilesystemType::Fat32 => "FAT32",
        FilesystemType::Exfat => "exFAT",
        FilesystemType::Apfs => "APFS",
        FilesystemType::HfsPlus => "HFS+",
        FilesystemType::Ext4 => "EXT4",
        FilesystemType::Unknown => "Unknown",
    }
}

fn is_valid_bytes_per_sector(value: u64) -> bool {
    matches!(value, 512 | 1024 | 2048 | 4096)
}

fn is_power_of_two(value: u64) -> bool {
    value > 0 && (value & (value - 1)) == 0
}

fn le_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn le_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs, path::PathBuf};

    #[test]
    fn detects_fat32_boot_signature_without_partition_table() {
        let image_path = temp_fixture_path("partition-fat32-signature.img");
        let mut bytes = vec![0_u8; (4 * MIB) as usize];
        write_fat32_boot_sector(&mut bytes, MIB as usize, 6144);
        fs::write(&image_path, bytes).expect("fixture should be written");

        let volumes = inspect_potential_volumes(&image_path).expect("signature scan should work");
        let candidate = volumes
            .iter()
            .find(|volume| volume.start_offset == MIB)
            .expect("FAT32 candidate should exist");
        assert!(matches!(candidate.filesystem, FilesystemType::Fat32));
        assert_eq!(candidate.detection_method, "boot-signature");

        let _ = fs::remove_file(image_path);
    }

    #[test]
    fn parses_mbr_partition_and_refines_fat32_signature() {
        let image_path = temp_fixture_path("partition-mbr-fat32.img");
        let start_lba = 2048_u32;
        let sector_count = 8192_u32;
        let start_offset = start_lba as usize * LOGICAL_SECTOR_SIZE as usize;
        let mut bytes =
            vec![0_u8; (start_offset as u64 + sector_count as u64 * LOGICAL_SECTOR_SIZE) as usize];
        write_mbr_partition(&mut bytes, 0, 0x0C, start_lba, sector_count);
        write_fat32_boot_sector(&mut bytes, start_offset, sector_count);
        fs::write(&image_path, bytes).expect("fixture should be written");

        let volumes = inspect_potential_volumes(&image_path).expect("MBR inspection should work");
        let candidate = volumes
            .iter()
            .find(|volume| volume.start_offset == start_offset as u64)
            .expect("MBR FAT32 candidate should exist");
        assert_eq!(candidate.detection_method, "mbr");
        assert!(candidate
            .notes
            .iter()
            .any(|note| note.contains("Validated by a FAT32 boot-sector signature")));

        let _ = fs::remove_file(image_path);
    }

    #[test]
    fn parses_gpt_partition_and_refines_ntfs_signature() {
        let image_path = temp_fixture_path("partition-gpt-ntfs.img");
        let start_lba = 2048_u64;
        let last_lba = 8191_u64;
        let total_sectors = 12288_u64;
        let start_offset = start_lba as usize * LOGICAL_SECTOR_SIZE as usize;
        let mut bytes = vec![0_u8; (total_sectors * LOGICAL_SECTOR_SIZE) as usize];

        write_protective_mbr(&mut bytes, total_sectors as u32 - 1);
        write_gpt_header(&mut bytes, total_sectors, 2, 128, 128);
        write_gpt_partition_entry(&mut bytes, 0, 2, 128, start_lba, last_lba, "Recovered NTFS");
        write_ntfs_boot_sector(&mut bytes, start_offset, last_lba - start_lba + 1);
        fs::write(&image_path, bytes).expect("fixture should be written");

        let volumes = inspect_potential_volumes(&image_path).expect("GPT inspection should work");
        let candidate = volumes
            .iter()
            .find(|volume| volume.start_offset == start_lba * LOGICAL_SECTOR_SIZE)
            .expect("GPT NTFS candidate should exist");
        assert!(candidate.detection_method.contains("gpt"));
        assert!(matches!(candidate.filesystem, FilesystemType::Ntfs));
        assert_eq!(candidate.label, "Recovered NTFS");

        let _ = fs::remove_file(image_path);
    }

    #[test]
    fn detects_hfsplus_volume_header_without_partition_table() {
        let image_path = temp_fixture_path("partition-hfsplus-signature.img");
        let offset = MIB as usize;
        let hfsplus_image = hfsplus::synthetic_visible_hfsplus_image_for_tests(b"hello hfs");
        let mut bytes = vec![0_u8; offset + hfsplus_image.len()];
        bytes[offset..offset + hfsplus_image.len()].copy_from_slice(&hfsplus_image);
        fs::write(&image_path, bytes).expect("fixture should be written");

        let volumes = inspect_potential_volumes(&image_path).expect("signature scan should work");
        let candidate = volumes
            .iter()
            .find(|volume| volume.start_offset == MIB)
            .expect("HFS+ candidate should exist");
        assert!(matches!(candidate.filesystem, FilesystemType::HfsPlus));
        assert_eq!(candidate.detection_method, "boot-signature");
        assert!(candidate
            .notes
            .iter()
            .any(|note| note.contains("HFS+ volume header signature")));

        let _ = fs::remove_file(image_path);
    }

    #[test]
    fn detects_apfs_container_superblock_without_partition_table() {
        let image_path = temp_fixture_path("partition-apfs-signature.img");
        let offset = MIB as usize;
        let block_size = 4096_u32;
        let block_count = 4096_u64;
        let size_bytes = block_size as usize * block_count as usize;
        let mut bytes = vec![0_u8; offset + size_bytes];
        write_apfs_container_superblock(&mut bytes, offset, block_size, block_count);
        fs::write(&image_path, bytes).expect("fixture should be written");

        let volumes = inspect_potential_volumes(&image_path).expect("signature scan should work");
        let candidate = volumes
            .iter()
            .find(|volume| volume.start_offset == MIB)
            .expect("APFS candidate should exist");
        assert!(matches!(candidate.filesystem, FilesystemType::Apfs));
        assert_eq!(candidate.detection_method, "boot-signature");
        assert!(candidate
            .notes
            .iter()
            .any(|note| note.contains("APFS container superblock")));

        let _ = fs::remove_file(image_path);
    }

    #[test]
    fn parses_gpt_partition_and_keeps_apfs_type_hint_without_superblock() {
        let image_path = temp_fixture_path("partition-gpt-apfs.img");
        let start_lba = 2048_u64;
        let last_lba = 8191_u64;
        let total_sectors = 12288_u64;
        let mut bytes = vec![0_u8; (total_sectors * LOGICAL_SECTOR_SIZE) as usize];

        write_protective_mbr(&mut bytes, total_sectors as u32 - 1);
        write_gpt_header(&mut bytes, total_sectors, 2, 128, 128);
        write_gpt_partition_entry_with_type(
            &mut bytes,
            0,
            2,
            128,
            start_lba,
            last_lba,
            "Recovered APFS",
            GPT_APFS_PARTITION_TYPE,
        );
        fs::write(&image_path, bytes).expect("fixture should be written");

        let volumes = inspect_potential_volumes(&image_path).expect("GPT inspection should work");
        let candidate = volumes
            .iter()
            .find(|volume| volume.start_offset == start_lba * LOGICAL_SECTOR_SIZE)
            .expect("GPT APFS candidate should exist");
        assert!(candidate.detection_method.contains("gpt"));
        assert!(matches!(candidate.filesystem, FilesystemType::Apfs));
        assert!(candidate
            .notes
            .iter()
            .any(|note| note.contains("GPT partition type matches an APFS container")));

        let _ = fs::remove_file(image_path);
    }

    fn temp_fixture_path(name: &str) -> PathBuf {
        env::temp_dir().join(format!("recupere-{}-{}", std::process::id(), name))
    }

    fn write_protective_mbr(bytes: &mut [u8], total_sectors_minus_one: u32) {
        write_mbr_partition(bytes, 0, 0xEE, 1, total_sectors_minus_one);
    }

    fn write_mbr_partition(
        bytes: &mut [u8],
        index: usize,
        partition_type: u8,
        start_lba: u32,
        sector_count: u32,
    ) {
        let offset = 446 + index * 16;
        bytes[offset + 4] = partition_type;
        bytes[offset + 8..offset + 12].copy_from_slice(&start_lba.to_le_bytes());
        bytes[offset + 12..offset + 16].copy_from_slice(&sector_count.to_le_bytes());
        bytes[510] = 0x55;
        bytes[511] = 0xAA;
    }

    fn write_fat32_boot_sector(bytes: &mut [u8], offset: usize, total_sectors: u32) {
        let sector = &mut bytes[offset..offset + 512];
        sector[11..13].copy_from_slice(&512_u16.to_le_bytes());
        sector[13] = 8;
        sector[14..16].copy_from_slice(&32_u16.to_le_bytes());
        sector[16] = 2;
        sector[32..36].copy_from_slice(&total_sectors.to_le_bytes());
        sector[36..40].copy_from_slice(&128_u32.to_le_bytes());
        sector[44..48].copy_from_slice(&2_u32.to_le_bytes());
        sector[82..90].copy_from_slice(b"FAT32   ");
        sector[510] = 0x55;
        sector[511] = 0xAA;
    }

    fn write_ntfs_boot_sector(bytes: &mut [u8], offset: usize, total_sectors: u64) {
        let sector = &mut bytes[offset..offset + 512];
        sector[3..11].copy_from_slice(b"NTFS    ");
        sector[11..13].copy_from_slice(&512_u16.to_le_bytes());
        sector[13] = 8;
        sector[40..48].copy_from_slice(&total_sectors.to_le_bytes());
        sector[48..56].copy_from_slice(&4_u64.to_le_bytes());
        sector[64] = (-10_i8) as u8;
        sector[510] = 0x55;
        sector[511] = 0xAA;
    }

    fn write_gpt_header(
        bytes: &mut [u8],
        total_sectors: u64,
        entries_lba: u64,
        entry_count: u32,
        entry_size: u32,
    ) {
        let header = &mut bytes[512..1024];
        header[0..8].copy_from_slice(b"EFI PART");
        header[8..12].copy_from_slice(&0x0001_0000_u32.to_le_bytes());
        header[12..16].copy_from_slice(&92_u32.to_le_bytes());
        header[24..32].copy_from_slice(&1_u64.to_le_bytes());
        header[32..40].copy_from_slice(&(total_sectors - 1).to_le_bytes());
        header[72..80].copy_from_slice(&entries_lba.to_le_bytes());
        header[80..84].copy_from_slice(&entry_count.to_le_bytes());
        header[84..88].copy_from_slice(&entry_size.to_le_bytes());
        header[510] = 0x55;
        header[511] = 0xAA;
    }

    fn write_gpt_partition_entry(
        bytes: &mut [u8],
        index: usize,
        entries_lba: u64,
        entry_size: usize,
        first_lba: u64,
        last_lba: u64,
        name: &str,
    ) {
        write_gpt_partition_entry_with_type(
            bytes,
            index,
            entries_lba,
            entry_size,
            first_lba,
            last_lba,
            name,
            [
                0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44, 0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26,
                0x99, 0xC7,
            ],
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn write_gpt_partition_entry_with_type(
        bytes: &mut [u8],
        index: usize,
        entries_lba: u64,
        entry_size: usize,
        first_lba: u64,
        last_lba: u64,
        name: &str,
        partition_type: [u8; 16],
    ) {
        let offset = entries_lba as usize * LOGICAL_SECTOR_SIZE as usize + index * entry_size;
        let entry = &mut bytes[offset..offset + entry_size];
        entry[0..16].copy_from_slice(&partition_type);
        entry[32..40].copy_from_slice(&first_lba.to_le_bytes());
        entry[40..48].copy_from_slice(&last_lba.to_le_bytes());
        for (index, code_unit) in name.encode_utf16().enumerate() {
            let start = 56 + index * 2;
            entry[start..start + 2].copy_from_slice(&code_unit.to_le_bytes());
        }
    }

    fn write_apfs_container_superblock(
        bytes: &mut [u8],
        offset: usize,
        block_size: u32,
        block_count: u64,
    ) {
        let block = &mut bytes[offset..offset + block_size as usize];
        block[32..36].copy_from_slice(APFS_NXSB_MAGIC);
        block[36..40].copy_from_slice(&block_size.to_le_bytes());
        block[40..48].copy_from_slice(&block_count.to_le_bytes());
    }
}
