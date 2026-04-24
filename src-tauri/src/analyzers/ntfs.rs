use crate::types::{ByteRun, NamedFileFork};
#[cfg(test)]
use lznt1::compress as compress_lznt1;
use lznt1::decompress as decompress_lznt1;
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const NTFS_ATTR_FILE_NAME: u32 = 0x30;
const NTFS_ATTR_DATA: u32 = 0x80;
const NTFS_ATTR_END: u32 = 0xffff_ffff;
const NTFS_ATTR_FLAG_COMPRESSED: u16 = 0x0001;
const NTFS_FILE_RECORD_IN_USE: u16 = 0x0001;
const NTFS_FILE_RECORD_DIRECTORY: u16 = 0x0002;
const NTFS_ROOT_RECORD_NUMBER: u64 = 5;
const NTFS_BITMAP_RECORD_NUMBER: u64 = 6;

type NtfsRecordIndex = (
    AllocationBitmap,
    Vec<ParsedMftRecord>,
    HashMap<u64, PathNode>,
);
type NtfsDeletedByteRuns = (Vec<ByteRun>, Vec<u32>, u64, u64, String, u8, Option<String>);
type NtfsVisibleByteRuns = (Vec<ByteRun>, Vec<u32>, u64, String, u8, Option<String>);

#[derive(Debug, Clone)]
pub struct NtfsDeletedFileCandidate {
    pub name: String,
    pub extension: String,
    pub path: String,
    pub size_bytes: u64,
    pub expected_size_bytes: u64,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub integrity: String,
    pub recovery_score: u8,
    pub start_offset: u64,
    pub clusters: Vec<u32>,
    pub byte_runs: Vec<ByteRun>,
    pub compression_kind: Option<String>,
    pub alternate_data_streams: Vec<NamedFileFork>,
}

#[derive(Debug, Clone)]
pub struct NtfsVisibleFileCandidate {
    pub name: String,
    pub extension: String,
    pub path: String,
    pub size_bytes: u64,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub integrity: String,
    pub recovery_score: u8,
    pub start_offset: Option<u64>,
    pub clusters: Vec<u32>,
    pub byte_runs: Vec<ByteRun>,
    pub compression_kind: Option<String>,
    pub alternate_data_streams: Vec<NamedFileFork>,
}

#[derive(Debug, Clone)]
struct NtfsBootSector {
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    total_sectors: u64,
    mft_lcn: u64,
    record_size_bytes: u32,
}

impl NtfsBootSector {
    fn read_from(reader: &mut File) -> Result<Self, String> {
        let mut sector = [0_u8; 512];
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|error| format!("Unable to seek the NTFS boot sector: {error}"))?;
        reader
            .read_exact(&mut sector)
            .map_err(|error| format!("Unable to read the NTFS boot sector: {error}"))?;

        if &sector[3..11] != b"NTFS    " {
            return Err("The image does not expose an NTFS filesystem name.".into());
        }

        if sector[510] != 0x55 || sector[511] != 0xaa {
            return Err("The image does not expose a valid NTFS boot signature.".into());
        }

        let bytes_per_sector = le_u16(&sector[11..13]);
        let sectors_per_cluster = sector[13];
        let total_sectors = le_u64(&sector[40..48]);
        let mft_lcn = le_u64(&sector[48..56]);
        let clusters_per_record = sector[64] as i8;

        if bytes_per_sector == 0 || sectors_per_cluster == 0 || total_sectors == 0 {
            return Err("The image does not contain a usable NTFS layout.".into());
        }

        let cluster_size_bytes = bytes_per_sector as u64 * sectors_per_cluster as u64;
        let record_size_bytes = if clusters_per_record > 0 {
            cluster_size_bytes.saturating_mul(clusters_per_record as u64)
        } else {
            1_u64 << (-clusters_per_record as u32)
        };

        if record_size_bytes == 0 || record_size_bytes > u32::MAX as u64 {
            return Err("The NTFS file record size is not usable.".into());
        }

        Ok(Self {
            bytes_per_sector,
            sectors_per_cluster,
            total_sectors,
            mft_lcn,
            record_size_bytes: record_size_bytes as u32,
        })
    }

    fn cluster_size_bytes(&self) -> u64 {
        self.bytes_per_sector as u64 * self.sectors_per_cluster as u64
    }

    fn total_clusters(&self) -> u64 {
        self.total_sectors / self.sectors_per_cluster.max(1) as u64
    }

    fn cluster_offset(&self, cluster: u64) -> Result<u64, String> {
        if cluster >= self.total_clusters() {
            return Err(format!(
                "NTFS cluster {cluster} is outside the image bounds."
            ));
        }
        Ok(cluster.saturating_mul(self.cluster_size_bytes()))
    }
}

#[derive(Debug, Clone)]
struct ClusterRun {
    start_lcn: u64,
    length_clusters: u64,
    sparse: bool,
}

#[derive(Debug, Clone)]
struct FileNameAttribute {
    parent_record_number: u64,
    name: String,
    namespace: u8,
    created_at: Option<String>,
    modified_at: Option<String>,
}

#[derive(Debug, Clone)]
enum DataAttribute {
    Resident {
        data_size: u64,
        byte_runs: Vec<ByteRun>,
    },
    NonResident {
        data_size: u64,
        allocated_size: u64,
        cluster_runs: Vec<ClusterRun>,
        compression_kind: Option<&'static str>,
    },
}

#[derive(Debug, Clone)]
struct ParsedMftRecord {
    record_number: u64,
    in_use: bool,
    is_directory: bool,
    file_name: Option<FileNameAttribute>,
    data_attribute: Option<DataAttribute>,
    named_data_attributes: Vec<NamedDataAttribute>,
}

#[derive(Debug, Clone)]
struct NamedDataAttribute {
    name: String,
    data_attribute: DataAttribute,
}

#[derive(Debug, Clone)]
struct AllocationBitmap {
    bytes: Vec<u8>,
    cluster_count: u64,
}

impl AllocationBitmap {
    fn cluster_is_free(&self, cluster: u64) -> Result<bool, String> {
        if cluster >= self.cluster_count {
            return Err(format!(
                "NTFS cluster {cluster} is outside the allocation bitmap."
            ));
        }

        let bit_index = cluster as usize;
        let byte_index = bit_index / 8;
        let bit_offset = bit_index % 8;
        let byte = *self.bytes.get(byte_index).ok_or_else(|| {
            format!("The NTFS allocation bitmap does not cover cluster {cluster}.")
        })?;
        Ok(((byte >> bit_offset) & 0x01) == 0)
    }
}

#[derive(Debug, Clone)]
struct PathNode {
    name: String,
    parent_record_number: u64,
    is_directory: bool,
}

pub fn recover_deleted_files(image_path: &Path) -> Result<Vec<NtfsDeletedFileCandidate>, String> {
    let mut reader = File::open(image_path).map_err(|error| {
        format!(
            "Unable to open the NTFS image {}: {}",
            image_path.to_string_lossy(),
            error
        )
    })?;
    let boot_sector = NtfsBootSector::read_from(&mut reader)?;
    let (allocation_bitmap, records, path_index) =
        load_records_and_index(&mut reader, &boot_sector)?;

    let mut deleted_files = Vec::new();
    for record in &records {
        if record.in_use || record.is_directory {
            continue;
        }

        let candidate = build_deleted_candidate(
            &mut reader,
            &boot_sector,
            &allocation_bitmap,
            &path_index,
            record,
        )?;
        if let Some(candidate) = candidate {
            deleted_files.push(candidate);
        }
    }

    Ok(deleted_files)
}

pub fn list_visible_files(image_path: &Path) -> Result<Vec<NtfsVisibleFileCandidate>, String> {
    let mut reader = File::open(image_path).map_err(|error| {
        format!(
            "Unable to open the NTFS image {}: {}",
            image_path.to_string_lossy(),
            error
        )
    })?;
    let boot_sector = NtfsBootSector::read_from(&mut reader)?;
    let (_, records, path_index) = load_records_and_index(&mut reader, &boot_sector)?;

    let mut visible_files = Vec::new();
    for record in &records {
        if !record.in_use || record.is_directory {
            continue;
        }

        let candidate = build_visible_candidate(&mut reader, &boot_sector, &path_index, record)?;
        if let Some(candidate) = candidate {
            visible_files.push(candidate);
        }
    }

    Ok(visible_files)
}

fn load_records_and_index(
    reader: &mut File,
    boot_sector: &NtfsBootSector,
) -> Result<NtfsRecordIndex, String> {
    let record_size = boot_sector.record_size_bytes as usize;

    let mft_record_zero_offset = boot_sector.cluster_offset(boot_sector.mft_lcn)?;
    let mft_record_zero = read_file_record(
        reader,
        mft_record_zero_offset,
        record_size,
        boot_sector.bytes_per_sector as usize,
    )?;
    let parsed_mft = parse_mft_record(&mft_record_zero, mft_record_zero_offset, 0)?;
    let mft_data = match parsed_mft.data_attribute {
        Some(DataAttribute::NonResident {
            data_size,
            cluster_runs,
            ..
        }) if !cluster_runs.is_empty() => (data_size, cluster_runs),
        _ => {
            return Err("The NTFS $MFT record does not expose a non-resident data runlist.".into())
        }
    };

    let (bitmap_record_bytes, bitmap_record_offset) = read_mft_record_by_index(
        reader,
        boot_sector,
        &mft_data.1,
        mft_data.0,
        NTFS_BITMAP_RECORD_NUMBER,
    )?;
    let bitmap_record = parse_mft_record(
        &bitmap_record_bytes,
        bitmap_record_offset,
        NTFS_BITMAP_RECORD_NUMBER,
    )?;
    let allocation_bitmap = load_allocation_bitmap(reader, boot_sector, &bitmap_record)?;

    let record_count = mft_data.0.div_ceil(record_size as u64) as u64;
    let mut records = Vec::new();
    let mut path_index = HashMap::new();

    for record_number in 0..record_count {
        let (record_bytes, record_offset) = match read_mft_record_by_index(
            reader,
            boot_sector,
            &mft_data.1,
            mft_data.0,
            record_number,
        ) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };

        if &record_bytes[0..4] != b"FILE" {
            continue;
        }

        let parsed = match parse_mft_record(&record_bytes, record_offset, record_number) {
            Ok(record) => record,
            Err(_) => continue,
        };

        if let Some(file_name) = parsed.file_name.as_ref() {
            path_index.insert(
                parsed.record_number,
                PathNode {
                    name: file_name.name.clone(),
                    parent_record_number: file_name.parent_record_number,
                    is_directory: parsed.is_directory,
                },
            );
        }

        records.push(parsed);
    }

    Ok((allocation_bitmap, records, path_index))
}

fn read_mft_record_by_index(
    reader: &mut File,
    boot_sector: &NtfsBootSector,
    mft_runs: &[ClusterRun],
    mft_data_size: u64,
    record_number: u64,
) -> Result<(Vec<u8>, u64), String> {
    let record_size = boot_sector.record_size_bytes as u64;
    let virtual_offset = record_number.saturating_mul(record_size);
    if virtual_offset + record_size > mft_data_size {
        return Err(format!(
            "NTFS MFT record {record_number} is outside the advertised $MFT data size."
        ));
    }

    let physical_offset =
        physical_offset_for_non_resident_offset(boot_sector, mft_runs, virtual_offset)?;
    let bytes =
        read_non_resident_bytes(reader, boot_sector, mft_runs, virtual_offset, record_size)?;
    let mut restored = bytes;
    apply_update_sequence_fixups(&mut restored, boot_sector.bytes_per_sector as usize)?;
    Ok((restored, physical_offset))
}

fn read_file_record(
    reader: &mut File,
    offset: u64,
    record_size: usize,
    bytes_per_sector: usize,
) -> Result<Vec<u8>, String> {
    let mut record = vec![0_u8; record_size];
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|error| format!("Unable to seek the NTFS file record at {offset}: {error}"))?;
    reader
        .read_exact(&mut record)
        .map_err(|error| format!("Unable to read the NTFS file record at {offset}: {error}"))?;
    apply_update_sequence_fixups(&mut record, bytes_per_sector)?;
    Ok(record)
}

fn apply_update_sequence_fixups(record: &mut [u8], bytes_per_sector: usize) -> Result<(), String> {
    if record.len() < 8 || bytes_per_sector < 2 {
        return Err("The NTFS file record is too small to validate fixups.".into());
    }

    let usa_offset = le_u16(&record[4..6]) as usize;
    let usa_count = le_u16(&record[6..8]) as usize;
    if usa_count < 2 {
        return Err("The NTFS file record does not expose a valid update sequence.".into());
    }

    let usa_length = usa_count.saturating_mul(2);
    if usa_offset + usa_length > record.len() {
        return Err("The NTFS update sequence array exceeds the file record bounds.".into());
    }

    let sequence = [record[usa_offset], record[usa_offset + 1]];
    for index in 0..(usa_count - 1) {
        let sector_end = (index + 1)
            .saturating_mul(bytes_per_sector)
            .saturating_sub(2);
        if sector_end + 2 > record.len() {
            return Err("The NTFS update sequence does not match the file record size.".into());
        }

        if record[sector_end..sector_end + 2] != sequence {
            return Err("The NTFS file record fixup sequence is invalid.".into());
        }

        let replacement_offset = usa_offset + 2 + index * 2;
        let replacement = [record[replacement_offset], record[replacement_offset + 1]];
        record[sector_end..sector_end + 2].copy_from_slice(&replacement);
    }

    Ok(())
}

fn parse_mft_record(
    record: &[u8],
    record_offset: u64,
    record_number: u64,
) -> Result<ParsedMftRecord, String> {
    if record.len() < 56 || &record[0..4] != b"FILE" {
        return Err("The NTFS file record signature is invalid.".into());
    }

    let flags = le_u16(&record[22..24]);
    let first_attribute_offset = le_u16(&record[20..22]) as usize;
    let used_size = le_u32(&record[24..28]) as usize;
    let limit = used_size.min(record.len());

    let mut best_file_name: Option<FileNameAttribute> = None;
    let mut data_attribute = None;
    let mut named_data_attributes = Vec::new();
    let mut cursor = first_attribute_offset;

    while cursor + 16 <= limit {
        let attribute_type = le_u32(&record[cursor..cursor + 4]);
        if attribute_type == NTFS_ATTR_END {
            break;
        }

        let attribute_length = le_u32(&record[cursor + 4..cursor + 8]) as usize;
        if attribute_length < 24 || cursor + attribute_length > limit {
            break;
        }

        let attribute = &record[cursor..cursor + attribute_length];
        let non_resident = attribute[8] != 0;
        let attribute_name = parse_attribute_name(attribute);
        let attribute_flags = le_u16(&attribute[12..14]);

        if attribute_type == NTFS_ATTR_FILE_NAME && !non_resident {
            if let Some(file_name) = parse_file_name_attribute(attribute) {
                if best_file_name
                    .as_ref()
                    .map(|current| {
                        namespace_rank(file_name.namespace) > namespace_rank(current.namespace)
                    })
                    .unwrap_or(true)
                {
                    best_file_name = Some(file_name);
                }
            }
        } else if attribute_type == NTFS_ATTR_DATA
            && attribute_flags & !NTFS_ATTR_FLAG_COMPRESSED == 0
        {
            match attribute_name {
                None if data_attribute.is_none() => {
                    data_attribute = parse_data_attribute(
                        attribute,
                        record_offset + cursor as u64,
                        attribute_flags,
                    );
                }
                Some(name) => {
                    if let Some(parsed_attribute) = parse_data_attribute(
                        attribute,
                        record_offset + cursor as u64,
                        attribute_flags,
                    ) {
                        named_data_attributes.push(NamedDataAttribute {
                            name,
                            data_attribute: parsed_attribute,
                        });
                    }
                }
                None => {}
            }
        }

        cursor += attribute_length;
    }

    Ok(ParsedMftRecord {
        record_number,
        in_use: flags & NTFS_FILE_RECORD_IN_USE != 0,
        is_directory: flags & NTFS_FILE_RECORD_DIRECTORY != 0,
        file_name: best_file_name,
        data_attribute,
        named_data_attributes,
    })
}

fn parse_attribute_name(attribute: &[u8]) -> Option<String> {
    let name_length = attribute.get(9).copied().unwrap_or(0) as usize;
    if name_length == 0 {
        return None;
    }

    let name_offset = le_u16(&attribute[10..12]) as usize;
    let name_bytes_len = name_length.saturating_mul(2);
    if name_offset < 16 || name_offset + name_bytes_len > attribute.len() {
        return None;
    }

    let name = decode_utf16le(&attribute[name_offset..name_offset + name_bytes_len]);
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn parse_file_name_attribute(attribute: &[u8]) -> Option<FileNameAttribute> {
    let value_length = le_u32(&attribute[16..20]) as usize;
    let value_offset = le_u16(&attribute[20..22]) as usize;
    if value_length < 66 || value_offset + value_length > attribute.len() {
        return None;
    }

    let value = &attribute[value_offset..value_offset + value_length];
    let parent_record_number = le_u64(&value[0..8]) & 0x0000_ffff_ffff_ffff;
    let created_at = decode_ntfs_filetime(le_u64(&value[8..16]));
    let modified_at = decode_ntfs_filetime(le_u64(&value[16..24]));
    let name_length = value[64] as usize;
    let namespace = value[65];
    let name_bytes_len = name_length.saturating_mul(2);
    if 66 + name_bytes_len > value.len() {
        return None;
    }

    let name = decode_utf16le(&value[66..66 + name_bytes_len]);
    if name.is_empty() {
        return None;
    }

    Some(FileNameAttribute {
        parent_record_number,
        name,
        namespace,
        created_at,
        modified_at,
    })
}

fn parse_data_attribute(
    attribute: &[u8],
    absolute_attribute_offset: u64,
    attribute_flags: u16,
) -> Option<DataAttribute> {
    if attribute[8] == 0 {
        if attribute_flags != 0 {
            return None;
        }
        let value_length = le_u32(&attribute[16..20]) as usize;
        let value_offset = le_u16(&attribute[20..22]) as usize;
        if value_offset + value_length > attribute.len() {
            return None;
        }
        if value_length == 0 {
            return None;
        }

        return Some(DataAttribute::Resident {
            data_size: value_length as u64,
            byte_runs: vec![ByteRun {
                offset: absolute_attribute_offset + value_offset as u64,
                length: value_length as u64,
                zero_fill: false,
                ..Default::default()
            }],
        });
    }

    if attribute.len() < 64 {
        return None;
    }

    let lowest_vcn = le_u64(&attribute[16..24]);
    if lowest_vcn != 0 {
        return None;
    }

    let compressed = attribute_flags & NTFS_ATTR_FLAG_COMPRESSED != 0;
    if attribute_flags & !NTFS_ATTR_FLAG_COMPRESSED != 0 {
        return None;
    }

    let data_size = le_u64(&attribute[48..56]);
    let allocated_size = le_u64(&attribute[40..48]);
    let mapping_pairs_offset = le_u16(&attribute[32..34]) as usize;
    if mapping_pairs_offset >= attribute.len() {
        return None;
    }

    if compressed && attribute.get(34).copied().unwrap_or(0) == 0 {
        return None;
    }

    let cluster_runs = parse_runlist(&attribute[mapping_pairs_offset..]).ok()?;
    if cluster_runs.is_empty() || data_size == 0 {
        return None;
    }

    Some(DataAttribute::NonResident {
        data_size,
        allocated_size,
        cluster_runs,
        compression_kind: compressed.then_some("lznt1"),
    })
}

fn parse_runlist(bytes: &[u8]) -> Result<Vec<ClusterRun>, String> {
    let mut runs = Vec::new();
    let mut cursor = 0usize;
    let mut previous_lcn = 0_i64;

    while cursor < bytes.len() {
        let header = bytes[cursor];
        cursor += 1;
        if header == 0 {
            break;
        }

        let length_size = (header & 0x0f) as usize;
        let offset_size = (header >> 4) as usize;
        if length_size == 0 || cursor + length_size + offset_size > bytes.len() {
            return Err("The NTFS runlist is malformed.".into());
        }

        let run_length = parse_unsigned(&bytes[cursor..cursor + length_size]);
        cursor += length_size;
        if run_length == 0 {
            return Err("The NTFS runlist exposes an empty segment.".into());
        }

        if offset_size == 0 {
            runs.push(ClusterRun {
                start_lcn: 0,
                length_clusters: run_length,
                sparse: true,
            });
            continue;
        }

        let relative_offset = parse_signed(&bytes[cursor..cursor + offset_size]);
        cursor += offset_size;

        previous_lcn = previous_lcn
            .checked_add(relative_offset)
            .ok_or_else(|| "The NTFS runlist offset overflowed.".to_string())?;
        if previous_lcn < 0 {
            return Err("The NTFS runlist resolves to a negative cluster.".into());
        }

        runs.push(ClusterRun {
            start_lcn: previous_lcn as u64,
            length_clusters: run_length,
            sparse: false,
        });
    }

    Ok(runs)
}

fn load_allocation_bitmap(
    reader: &mut File,
    boot_sector: &NtfsBootSector,
    bitmap_record: &ParsedMftRecord,
) -> Result<AllocationBitmap, String> {
    let data_attribute = bitmap_record
        .data_attribute
        .as_ref()
        .ok_or_else(|| "The NTFS $Bitmap record does not expose a data attribute.".to_string())?;
    let bytes = read_attribute_bytes(reader, boot_sector, data_attribute)?;
    Ok(AllocationBitmap {
        bytes,
        cluster_count: boot_sector.total_clusters(),
    })
}

fn read_attribute_bytes(
    reader: &mut File,
    boot_sector: &NtfsBootSector,
    data_attribute: &DataAttribute,
) -> Result<Vec<u8>, String> {
    match data_attribute {
        DataAttribute::Resident {
            data_size,
            byte_runs,
        } => read_byte_runs(reader, byte_runs, *data_size),
        DataAttribute::NonResident {
            data_size,
            allocated_size,
            cluster_runs,
            compression_kind,
        } => {
            if compression_kind.is_some() {
                read_compressed_non_resident_bytes(
                    reader,
                    boot_sector,
                    cluster_runs,
                    *allocated_size,
                    *data_size,
                )
            } else {
                read_non_resident_bytes(reader, boot_sector, cluster_runs, 0, *data_size)
            }
        }
    }
}

fn read_byte_runs(
    reader: &mut File,
    byte_runs: &[ByteRun],
    expected_size: u64,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(expected_size as usize);
    let mut remaining = expected_size;

    for run in byte_runs {
        if remaining == 0 {
            break;
        }
        let to_read = run.length.min(remaining);
        let mut buffer = vec![0_u8; to_read as usize];
        if !run.zero_fill {
            reader.seek(SeekFrom::Start(run.offset)).map_err(|error| {
                format!("Unable to seek the NTFS image at {}: {error}", run.offset)
            })?;
            reader.read_exact(&mut buffer).map_err(|error| {
                format!("Unable to read the NTFS image at {}: {error}", run.offset)
            })?;
        }
        bytes.extend_from_slice(&buffer);
        remaining -= to_read;
    }

    if remaining > 0 {
        return Err("The NTFS byte runs do not cover the requested data size.".into());
    }

    Ok(bytes)
}

fn read_non_resident_bytes(
    reader: &mut File,
    boot_sector: &NtfsBootSector,
    cluster_runs: &[ClusterRun],
    starting_virtual_offset: u64,
    length: u64,
) -> Result<Vec<u8>, String> {
    let cluster_size = boot_sector.cluster_size_bytes();
    let mut bytes = Vec::with_capacity(length as usize);
    let mut remaining = length;
    let mut consumed_virtual_bytes = 0_u64;
    let mut target_offset = starting_virtual_offset;

    for run in cluster_runs {
        let run_length_bytes = run.length_clusters.saturating_mul(cluster_size);
        if target_offset >= consumed_virtual_bytes + run_length_bytes {
            consumed_virtual_bytes = consumed_virtual_bytes.saturating_add(run_length_bytes);
            continue;
        }

        let offset_within_run = target_offset.saturating_sub(consumed_virtual_bytes);
        let readable_bytes = run_length_bytes.saturating_sub(offset_within_run);
        let to_read = readable_bytes.min(remaining);
        if run.sparse {
            bytes.resize(bytes.len().saturating_add(to_read as usize), 0);
        } else {
            let physical_offset = boot_sector
                .cluster_offset(run.start_lcn)?
                .saturating_add(offset_within_run);

            let mut buffer = vec![0_u8; to_read as usize];
            reader
                .seek(SeekFrom::Start(physical_offset))
                .map_err(|error| {
                    format!("Unable to seek the NTFS image at {physical_offset}: {error}")
                })?;
            reader.read_exact(&mut buffer).map_err(|error| {
                format!("Unable to read the NTFS image at {physical_offset}: {error}")
            })?;
            bytes.extend_from_slice(&buffer);
        }

        remaining -= to_read;
        target_offset = target_offset.saturating_add(to_read);
        consumed_virtual_bytes = consumed_virtual_bytes.saturating_add(run_length_bytes);
        if remaining == 0 {
            break;
        }
    }

    if remaining > 0 {
        return Err("The NTFS runlist does not cover the requested data range.".into());
    }

    Ok(bytes)
}

fn physical_offset_for_non_resident_offset(
    boot_sector: &NtfsBootSector,
    cluster_runs: &[ClusterRun],
    target_offset: u64,
) -> Result<u64, String> {
    let cluster_size = boot_sector.cluster_size_bytes();
    let mut consumed_virtual_bytes = 0_u64;

    for run in cluster_runs {
        let run_length_bytes = run.length_clusters.saturating_mul(cluster_size);
        if target_offset >= consumed_virtual_bytes + run_length_bytes {
            consumed_virtual_bytes = consumed_virtual_bytes.saturating_add(run_length_bytes);
            continue;
        }

        let offset_within_run = target_offset.saturating_sub(consumed_virtual_bytes);
        if run.sparse {
            return Err("The NTFS runlist offset points into a sparse logical range.".into());
        }
        return Ok(boot_sector
            .cluster_offset(run.start_lcn)?
            .saturating_add(offset_within_run));
    }

    Err("The NTFS runlist does not resolve the requested physical offset.".into())
}

fn build_deleted_candidate(
    reader: &mut File,
    boot_sector: &NtfsBootSector,
    allocation_bitmap: &AllocationBitmap,
    path_index: &HashMap<u64, PathNode>,
    record: &ParsedMftRecord,
) -> Result<Option<NtfsDeletedFileCandidate>, String> {
    let file_name = match record.file_name.as_ref() {
        Some(file_name) => file_name,
        None => return Ok(None),
    };
    let data_attribute = match record.data_attribute.as_ref() {
        Some(data_attribute) => data_attribute,
        None => return Ok(None),
    };

    let (
        byte_runs,
        clusters,
        size_bytes,
        expected_size_bytes,
        integrity,
        recovery_score,
        compression_kind,
    ) = match data_attribute {
        DataAttribute::Resident {
            data_size,
            byte_runs,
        } if *data_size > 0 && !byte_runs.is_empty() => (
            byte_runs.clone(),
            Vec::new(),
            *data_size,
            *data_size,
            "intact".to_string(),
            94,
            None,
        ),
        DataAttribute::Resident { .. } => return Ok(None),
        DataAttribute::NonResident {
            data_size,
            allocated_size,
            cluster_runs,
            compression_kind,
        } => build_non_resident_candidate(
            reader,
            boot_sector,
            allocation_bitmap,
            *data_size,
            *allocated_size,
            cluster_runs,
            *compression_kind,
        )?,
    };

    if size_bytes == 0 || byte_runs.is_empty() {
        return Ok(None);
    }

    let name = file_name.name.clone();
    let extension = file_extension(&name);
    let path = build_parent_path(path_index, file_name.parent_record_number);
    let start_offset = byte_runs.first().map(|run| run.offset).unwrap_or(0);
    let alternate_data_streams = build_deleted_named_streams(
        reader,
        boot_sector,
        allocation_bitmap,
        &record.named_data_attributes,
    )?;

    Ok(Some(NtfsDeletedFileCandidate {
        name,
        extension,
        path,
        size_bytes,
        expected_size_bytes,
        created_at: file_name.created_at.clone(),
        modified_at: file_name.modified_at.clone(),
        integrity,
        recovery_score,
        start_offset,
        clusters,
        byte_runs,
        compression_kind,
        alternate_data_streams,
    }))
}

fn build_visible_candidate(
    reader: &mut File,
    boot_sector: &NtfsBootSector,
    path_index: &HashMap<u64, PathNode>,
    record: &ParsedMftRecord,
) -> Result<Option<NtfsVisibleFileCandidate>, String> {
    let file_name = match record.file_name.as_ref() {
        Some(file_name) if !file_name.name.starts_with('$') => file_name,
        _ => return Ok(None),
    };
    let data_attribute = match record.data_attribute.as_ref() {
        Some(data_attribute) => data_attribute,
        None => return Ok(None),
    };

    let (byte_runs, clusters, size_bytes, integrity, recovery_score, compression_kind) =
        match data_attribute {
            DataAttribute::Resident {
                data_size,
                byte_runs,
            } if *data_size > 0 && !byte_runs.is_empty() => (
                byte_runs.clone(),
                Vec::new(),
                *data_size,
                "intact".to_string(),
                97,
                None,
            ),
            DataAttribute::Resident { .. } => return Ok(None),
            DataAttribute::NonResident {
                data_size,
                allocated_size,
                cluster_runs,
                compression_kind,
            } => build_visible_non_resident_candidate(
                reader,
                boot_sector,
                *data_size,
                *allocated_size,
                cluster_runs,
                *compression_kind,
            )?,
        };

    if size_bytes == 0 || byte_runs.is_empty() {
        return Ok(None);
    }

    let name = file_name.name.clone();
    let extension = file_extension(&name);
    let path = build_parent_path(path_index, file_name.parent_record_number);
    let start_offset = byte_runs.first().map(|run| run.offset);
    let alternate_data_streams =
        build_visible_named_streams(reader, boot_sector, &record.named_data_attributes)?;

    Ok(Some(NtfsVisibleFileCandidate {
        name,
        extension,
        path,
        size_bytes,
        created_at: file_name.created_at.clone(),
        modified_at: file_name.modified_at.clone(),
        integrity,
        recovery_score,
        start_offset,
        clusters,
        byte_runs,
        compression_kind,
        alternate_data_streams,
    }))
}

fn build_deleted_named_streams(
    reader: &mut File,
    boot_sector: &NtfsBootSector,
    allocation_bitmap: &AllocationBitmap,
    named_data_attributes: &[NamedDataAttribute],
) -> Result<Vec<NamedFileFork>, String> {
    let mut streams = Vec::new();

    for stream in named_data_attributes {
        match &stream.data_attribute {
            DataAttribute::Resident {
                data_size,
                byte_runs,
            } if *data_size > 0 && !byte_runs.is_empty() => {
                streams.push(NamedFileFork {
                    name: stream.name.clone(),
                    size_bytes: *data_size,
                    expected_size_bytes: Some(*data_size),
                    byte_runs: byte_runs.clone(),
                });
            }
            DataAttribute::Resident { .. } => {}
            DataAttribute::NonResident {
                data_size,
                allocated_size,
                cluster_runs,
                compression_kind,
            } => {
                let (byte_runs, _, recovered_size, expected_size_bytes, _, _, _) =
                    build_non_resident_candidate(
                        reader,
                        boot_sector,
                        allocation_bitmap,
                        *data_size,
                        *allocated_size,
                        cluster_runs,
                        *compression_kind,
                    )?;
                if recovered_size > 0 && !byte_runs.is_empty() {
                    streams.push(NamedFileFork {
                        name: stream.name.clone(),
                        size_bytes: recovered_size,
                        expected_size_bytes: Some(expected_size_bytes),
                        byte_runs,
                    });
                }
            }
        }
    }

    Ok(streams)
}

fn build_visible_named_streams(
    reader: &mut File,
    boot_sector: &NtfsBootSector,
    named_data_attributes: &[NamedDataAttribute],
) -> Result<Vec<NamedFileFork>, String> {
    let mut streams = Vec::new();

    for stream in named_data_attributes {
        match &stream.data_attribute {
            DataAttribute::Resident {
                data_size,
                byte_runs,
            } if *data_size > 0 && !byte_runs.is_empty() => {
                streams.push(NamedFileFork {
                    name: stream.name.clone(),
                    size_bytes: *data_size,
                    expected_size_bytes: Some(*data_size),
                    byte_runs: byte_runs.clone(),
                });
            }
            DataAttribute::Resident { .. } => {}
            DataAttribute::NonResident {
                data_size,
                allocated_size,
                cluster_runs,
                compression_kind,
            } => {
                let (byte_runs, _, recovered_size, _, _, _) = build_visible_non_resident_candidate(
                    reader,
                    boot_sector,
                    *data_size,
                    *allocated_size,
                    cluster_runs,
                    *compression_kind,
                )?;
                if recovered_size > 0 && !byte_runs.is_empty() {
                    streams.push(NamedFileFork {
                        name: stream.name.clone(),
                        size_bytes: recovered_size,
                        expected_size_bytes: Some(*data_size),
                        byte_runs,
                    });
                }
            }
        }
    }

    Ok(streams)
}

fn build_non_resident_candidate(
    reader: &mut File,
    boot_sector: &NtfsBootSector,
    allocation_bitmap: &AllocationBitmap,
    data_size: u64,
    allocated_size: u64,
    cluster_runs: &[ClusterRun],
    compression_kind: Option<&'static str>,
) -> Result<NtfsDeletedByteRuns, String> {
    if compression_kind.is_some() {
        return build_compressed_non_resident_candidate(
            reader,
            boot_sector,
            Some(allocation_bitmap),
            data_size,
            allocated_size,
            cluster_runs,
        );
    }

    let cluster_size = boot_sector.cluster_size_bytes();
    let mut remaining = data_size;
    let mut byte_runs = Vec::new();
    let mut clusters = Vec::new();

    'outer: for run in cluster_runs {
        if run.sparse {
            let sparse_length = remaining.min(run.length_clusters.saturating_mul(cluster_size));
            append_zero_fill_run(&mut byte_runs, sparse_length);
            remaining = remaining.saturating_sub(sparse_length);
            if remaining == 0 {
                break;
            }
            continue;
        }

        for cluster_index in 0..run.length_clusters {
            if remaining == 0 {
                break 'outer;
            }

            let cluster = run.start_lcn.saturating_add(cluster_index);
            if !allocation_bitmap.cluster_is_free(cluster)? {
                break 'outer;
            }

            let cluster_offset = boot_sector.cluster_offset(cluster)?;
            let chunk_length = remaining.min(cluster_size);
            append_byte_run(&mut byte_runs, cluster_offset, chunk_length);
            if let Ok(cluster) = u32::try_from(cluster) {
                clusters.push(cluster);
            }
            remaining = remaining.saturating_sub(chunk_length);
        }
    }

    let recovered_size = data_size.saturating_sub(remaining);
    if recovered_size == 0 {
        return Ok((
            Vec::new(),
            Vec::new(),
            0,
            data_size,
            "partial".into(),
            0,
            None,
        ));
    }

    let fully_recovered = recovered_size == data_size;
    let fragmented = physical_run_count(&byte_runs) > 1 && fully_recovered;
    let integrity = if fragmented {
        "fragmented".to_string()
    } else if fully_recovered {
        "intact".to_string()
    } else {
        "partial".to_string()
    };
    let recovery_score = if fragmented {
        80
    } else if fully_recovered {
        86
    } else {
        61
    };

    Ok((
        byte_runs,
        clusters,
        recovered_size,
        data_size,
        integrity,
        recovery_score,
        None,
    ))
}

fn build_visible_non_resident_candidate(
    reader: &mut File,
    boot_sector: &NtfsBootSector,
    data_size: u64,
    allocated_size: u64,
    cluster_runs: &[ClusterRun],
    compression_kind: Option<&'static str>,
) -> Result<NtfsVisibleByteRuns, String> {
    if compression_kind.is_some() {
        return build_compressed_non_resident_candidate(
            reader,
            boot_sector,
            None,
            data_size,
            allocated_size,
            cluster_runs,
        )
        .map(
            |(
                byte_runs,
                clusters,
                recovered_size,
                _,
                integrity,
                recovery_score,
                compression_kind,
            )| {
                (
                    byte_runs,
                    clusters,
                    recovered_size,
                    integrity,
                    recovery_score,
                    compression_kind,
                )
            },
        );
    }

    let cluster_size = boot_sector.cluster_size_bytes();
    let mut remaining = data_size;
    let mut byte_runs = Vec::new();
    let mut clusters = Vec::new();

    'outer: for run in cluster_runs {
        if run.sparse {
            let sparse_length = remaining.min(run.length_clusters.saturating_mul(cluster_size));
            append_zero_fill_run(&mut byte_runs, sparse_length);
            remaining = remaining.saturating_sub(sparse_length);
            if remaining == 0 {
                break;
            }
            continue;
        }

        for cluster_index in 0..run.length_clusters {
            if remaining == 0 {
                break 'outer;
            }

            let cluster = run.start_lcn.saturating_add(cluster_index);
            let cluster_offset = boot_sector.cluster_offset(cluster)?;
            let chunk_length = remaining.min(cluster_size);
            append_byte_run(&mut byte_runs, cluster_offset, chunk_length);
            if let Ok(cluster) = u32::try_from(cluster) {
                clusters.push(cluster);
            }
            remaining = remaining.saturating_sub(chunk_length);
        }
    }

    let recovered_size = data_size.saturating_sub(remaining);
    if recovered_size == 0 {
        return Ok((Vec::new(), Vec::new(), 0, "partial".into(), 0, None));
    }

    let fragmented = physical_run_count(&byte_runs) > 1;
    let integrity = if fragmented { "fragmented" } else { "intact" }.to_string();
    let recovery_score = if fragmented { 91 } else { 95 };

    Ok((
        byte_runs,
        clusters,
        recovered_size,
        integrity,
        recovery_score,
        None,
    ))
}

fn build_compressed_non_resident_candidate(
    reader: &mut File,
    boot_sector: &NtfsBootSector,
    allocation_bitmap: Option<&AllocationBitmap>,
    data_size: u64,
    allocated_size: u64,
    cluster_runs: &[ClusterRun],
) -> Result<NtfsDeletedByteRuns, String> {
    if data_size == 0 || allocated_size == 0 || cluster_runs.is_empty() {
        return Ok((
            Vec::new(),
            Vec::new(),
            0,
            data_size,
            "partial".into(),
            0,
            None,
        ));
    }

    if cluster_runs.iter().any(|run| run.sparse) {
        return Ok((
            Vec::new(),
            Vec::new(),
            0,
            data_size,
            "partial".into(),
            0,
            None,
        ));
    }

    let cluster_size = boot_sector.cluster_size_bytes();
    let mut remaining_stored = allocated_size;
    let mut byte_runs = Vec::new();
    let mut clusters = Vec::new();

    'outer: for run in cluster_runs {
        for cluster_index in 0..run.length_clusters {
            if remaining_stored == 0 {
                break 'outer;
            }

            let cluster = run.start_lcn.saturating_add(cluster_index);
            if let Some(bitmap) = allocation_bitmap {
                if !bitmap.cluster_is_free(cluster)? {
                    return Ok((
                        Vec::new(),
                        Vec::new(),
                        0,
                        data_size,
                        "partial".into(),
                        0,
                        None,
                    ));
                }
            }

            let cluster_offset = boot_sector.cluster_offset(cluster)?;
            let chunk_length = remaining_stored.min(cluster_size);
            append_byte_run_with_compression(
                &mut byte_runs,
                cluster_offset,
                chunk_length,
                Some("lznt1"),
            );
            if let Ok(cluster) = u32::try_from(cluster) {
                clusters.push(cluster);
            }
            remaining_stored = remaining_stored.saturating_sub(chunk_length);
        }
    }

    if remaining_stored > 0 || byte_runs.is_empty() {
        return Ok((
            Vec::new(),
            Vec::new(),
            0,
            data_size,
            "partial".into(),
            0,
            None,
        ));
    }

    let compressed_bytes = read_raw_byte_runs(reader, &byte_runs, allocated_size)?;
    let mut decompressed = Vec::new();
    if decompress_lznt1(&compressed_bytes, &mut decompressed).is_err()
        || decompressed.len() as u64 != data_size
    {
        return Ok((
            Vec::new(),
            Vec::new(),
            0,
            data_size,
            "partial".into(),
            0,
            None,
        ));
    }

    let fragmented = physical_run_count(&byte_runs) > 1;
    let integrity = if fragmented {
        "fragmented".to_string()
    } else {
        "intact".to_string()
    };
    let recovery_score = if allocation_bitmap.is_some() {
        if fragmented {
            76
        } else {
            81
        }
    } else if fragmented {
        88
    } else {
        92
    };

    Ok((
        byte_runs,
        clusters,
        data_size,
        data_size,
        integrity,
        recovery_score,
        Some("lznt1".into()),
    ))
}

fn append_byte_run(byte_runs: &mut Vec<ByteRun>, offset: u64, length: u64) {
    append_byte_run_with_compression(byte_runs, offset, length, None);
}

fn append_byte_run_with_compression(
    byte_runs: &mut Vec<ByteRun>,
    offset: u64,
    length: u64,
    compression_kind: Option<&str>,
) {
    if let Some(last) = byte_runs.last_mut() {
        if !last.zero_fill
            && last.offset + last.length == offset
            && last.compression_kind.as_deref() == compression_kind
        {
            last.length = last.length.saturating_add(length);
            return;
        }
    }

    let mut run = ByteRun::physical(offset, length);
    run.compression_kind = compression_kind.map(|value| value.into());
    byte_runs.push(run);
}

fn append_zero_fill_run(byte_runs: &mut Vec<ByteRun>, length: u64) {
    if length == 0 {
        return;
    }

    if let Some(last) = byte_runs.last_mut() {
        if last.zero_fill {
            last.length = last.length.saturating_add(length);
            return;
        }
    }

    byte_runs.push(ByteRun::synthetic_zero_fill(length));
}

fn physical_run_count(byte_runs: &[ByteRun]) -> usize {
    byte_runs.iter().filter(|run| !run.zero_fill).count()
}

fn read_raw_byte_runs(
    reader: &mut File,
    byte_runs: &[ByteRun],
    expected_size: u64,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(expected_size as usize);
    let mut remaining = expected_size;

    for run in byte_runs {
        if remaining == 0 {
            break;
        }
        if run.zero_fill {
            return Err(
                "Compressed NTFS byte runs cannot contain sparse zero-fill segments.".into(),
            );
        }
        let to_read = run.length.min(remaining);
        let mut buffer = vec![0_u8; to_read as usize];
        reader
            .seek(SeekFrom::Start(run.offset))
            .map_err(|error| format!("Unable to seek the NTFS image at {}: {error}", run.offset))?;
        reader
            .read_exact(&mut buffer)
            .map_err(|error| format!("Unable to read the NTFS image at {}: {error}", run.offset))?;
        bytes.extend_from_slice(&buffer);
        remaining = remaining.saturating_sub(to_read);
    }

    if remaining > 0 {
        return Err("The NTFS compressed byte runs do not cover the allocated stored size.".into());
    }

    Ok(bytes)
}

fn read_compressed_non_resident_bytes(
    reader: &mut File,
    boot_sector: &NtfsBootSector,
    cluster_runs: &[ClusterRun],
    allocated_size: u64,
    data_size: u64,
) -> Result<Vec<u8>, String> {
    let cluster_size = boot_sector.cluster_size_bytes();
    let mut remaining_stored = allocated_size;
    let mut byte_runs = Vec::new();

    'outer: for run in cluster_runs {
        if run.sparse {
            return Err(
                "Compressed NTFS attributes with sparse segments are not supported.".into(),
            );
        }

        for cluster_index in 0..run.length_clusters {
            if remaining_stored == 0 {
                break 'outer;
            }

            let cluster = run.start_lcn.saturating_add(cluster_index);
            let cluster_offset = boot_sector.cluster_offset(cluster)?;
            let chunk_length = remaining_stored.min(cluster_size);
            append_byte_run_with_compression(
                &mut byte_runs,
                cluster_offset,
                chunk_length,
                Some("lznt1"),
            );
            remaining_stored = remaining_stored.saturating_sub(chunk_length);
        }
    }

    if remaining_stored > 0 {
        return Err("Compressed NTFS attributes do not cover the allocated stored size.".into());
    }

    let compressed_bytes = read_raw_byte_runs(reader, &byte_runs, allocated_size)?;
    let mut decompressed = Vec::new();
    decompress_lznt1(&compressed_bytes, &mut decompressed)
        .map_err(|error| format!("Unable to decompress NTFS LZNT1 data: {error}"))?;
    if decompressed.len() as u64 != data_size {
        return Err(format!(
            "NTFS compressed data size mismatch: expected {data_size} bytes after decompression, got {}.",
            decompressed.len()
        ));
    }
    Ok(decompressed)
}

fn build_parent_path(path_index: &HashMap<u64, PathNode>, mut parent_record_number: u64) -> String {
    if parent_record_number == NTFS_ROOT_RECORD_NUMBER {
        return "/".into();
    }

    let mut segments = Vec::new();
    let mut guard = 0_u8;

    while parent_record_number != NTFS_ROOT_RECORD_NUMBER && guard < 32 {
        let Some(node) = path_index.get(&parent_record_number) else {
            break;
        };
        if !node.is_directory || node.name.is_empty() {
            break;
        }
        segments.push(node.name.clone());
        parent_record_number = node.parent_record_number;
        guard = guard.saturating_add(1);
    }

    if segments.is_empty() {
        "/".into()
    } else {
        segments.reverse();
        format!("/{}", segments.join("/"))
    }
}

fn file_extension(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default()
}

fn namespace_rank(namespace: u8) -> u8 {
    match namespace {
        0x03 => 4,
        0x01 => 3,
        0x00 => 2,
        0x02 => 1,
        _ => 0,
    }
}

fn decode_utf16le(bytes: &[u8]) -> String {
    let mut code_units = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        code_units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }

    String::from_utf16_lossy(&code_units)
        .trim_end_matches('\u{0}')
        .to_string()
}

fn decode_ntfs_filetime(value: u64) -> Option<String> {
    if value == 0 {
        return None;
    }

    let seconds_since_1601 = value / 10_000_000;
    if seconds_since_1601 < 11_644_473_600 {
        return None;
    }

    let unix_seconds = (seconds_since_1601 - 11_644_473_600) as i64;
    let days = unix_seconds.div_euclid(86_400);
    let seconds_of_day = unix_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;

    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}"
    ))
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = (yoe as i32) + (era as i32) * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }

    (year, month as u32, day as u32)
}

fn parse_unsigned(bytes: &[u8]) -> u64 {
    let mut value = 0_u64;
    for (index, byte) in bytes.iter().enumerate() {
        value |= (*byte as u64) << (index * 8);
    }
    value
}

fn parse_signed(bytes: &[u8]) -> i64 {
    let mut value = 0_i64;
    for (index, byte) in bytes.iter().enumerate() {
        value |= (*byte as i64) << (index * 8);
    }

    if bytes.last().copied().unwrap_or(0) & 0x80 != 0 {
        value |= (!0_i64) << (bytes.len() * 8);
    }

    value
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

/// Parse NTFS $UsnJrnl:$J for additional recovery hints.
/// Scans MFT for the $UsnJrnl system file, parses USN_RECORD_V2 entries
/// from the $J data stream, and cross-references with deleted MFT records.
pub fn recover_usn_journal_files(
    image_path: &Path,
) -> Result<Vec<NtfsDeletedFileCandidate>, String> {
    let mut reader = File::open(image_path)
        .map_err(|e| format!("Unable to open NTFS image for USN journal: {e}"))?;

    let boot_sector = match NtfsBootSector::read_from(&mut reader) {
        Ok(bs) => bs,
        Err(_) => return Ok(Vec::new()),
    };

    let (allocation_bitmap, records, path_index) =
        match load_records_and_index(&mut reader, &boot_sector) {
            Ok(data) => data,
            Err(_) => return Ok(Vec::new()),
        };

    // Find the $UsnJrnl MFT record
    let usn_journal_record = records.iter().find(|r| {
        r.file_name
            .as_ref()
            .is_some_and(|fn_attr| fn_attr.name == "$UsnJrnl")
    });

    let usn_record = match usn_journal_record {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    // Find the $J alternate data stream which contains USN records
    let j_stream = usn_record
        .named_data_attributes
        .iter()
        .find(|attr| attr.name == "$J");
    if j_stream.is_none() {
        return Ok(Vec::new());
    }
    let j_stream = j_stream.unwrap();

    // Get cluster runs from the $J data attribute
    let cluster_runs = match &j_stream.data_attribute {
        DataAttribute::NonResident { cluster_runs, .. } => cluster_runs.clone(),
        DataAttribute::Resident { byte_runs, .. } => {
            // Resident $J is unusual but handle it
            let mut usn_delete_refs: Vec<(u64, String)> = Vec::new();
            for run in byte_runs {
                if run.zero_fill {
                    continue;
                }
                let chunk_size = run.length.min(1024 * 1024) as usize;
                let mut buffer = vec![0u8; chunk_size];
                if read_exact_at_ntfs(&mut reader, run.offset, &mut buffer).is_err() {
                    continue;
                }
                parse_usn_records_from_buffer(&buffer, &mut usn_delete_refs);
            }
            return build_usn_candidates(
                &mut reader,
                &boot_sector,
                &allocation_bitmap,
                &path_index,
                &records,
                &usn_delete_refs,
            );
        }
    };

    // Read USN records from the non-resident $J stream cluster runs
    let cluster_size = boot_sector.cluster_size_bytes();
    let mut usn_delete_refs: Vec<(u64, String)> = Vec::new();

    for run in &cluster_runs {
        if run.sparse {
            continue;
        }
        let run_offset = run.start_lcn * cluster_size;
        let run_size = (run.length_clusters * cluster_size).min(4 * 1024 * 1024) as usize;
        let mut buffer = vec![0u8; run_size];
        if read_exact_at_ntfs(&mut reader, run_offset, &mut buffer).is_err() {
            continue;
        }

        parse_usn_records_from_buffer(&buffer, &mut usn_delete_refs);
    }

    build_usn_candidates(
        &mut reader,
        &boot_sector,
        &allocation_bitmap,
        &path_index,
        &records,
        &usn_delete_refs,
    )
}

fn parse_usn_records_from_buffer(buffer: &[u8], results: &mut Vec<(u64, String)>) {
    let mut offset = 0usize;
    while offset + 60 < buffer.len() {
        let record_length = le_u32_ntfs(&buffer[offset..offset + 4]) as usize;
        if !(60..=4096).contains(&record_length) || offset + record_length > buffer.len() {
            offset += 8;
            continue;
        }

        let major_version = le_u16_ntfs(&buffer[offset + 4..offset + 6]);
        if major_version != 2 {
            offset += record_length.max(8);
            continue;
        }

        let file_ref = le_u64_ntfs(&buffer[offset + 8..offset + 16]);
        let reason = le_u32_ntfs(&buffer[offset + 40..offset + 44]);
        let name_length = le_u16_ntfs(&buffer[offset + 56..offset + 58]) as usize;
        let name_offset_field = le_u16_ntfs(&buffer[offset + 58..offset + 60]) as usize;

        const USN_REASON_FILE_DELETE: u32 = 0x0000_0200;

        if reason & USN_REASON_FILE_DELETE != 0 && name_length > 0 && name_offset_field > 0 {
            let name_start = offset + name_offset_field;
            let name_end = name_start + name_length;
            if name_end <= buffer.len() {
                let name_bytes = &buffer[name_start..name_end];
                let name: String = name_bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .map(|c| char::from_u32(c as u32).unwrap_or('?'))
                    .collect();
                if !name.starts_with('$') && !name.is_empty() {
                    results.push((file_ref & 0x0000_FFFF_FFFF_FFFF, name));
                }
            }
        }

        offset += record_length;
    }
}

fn build_usn_candidates(
    reader: &mut File,
    boot_sector: &NtfsBootSector,
    allocation_bitmap: &AllocationBitmap,
    path_index: &HashMap<u64, PathNode>,
    records: &[ParsedMftRecord],
    usn_delete_refs: &[(u64, String)],
) -> Result<Vec<NtfsDeletedFileCandidate>, String> {
    let mut candidates = Vec::new();
    let mut seen_refs = std::collections::HashSet::new();

    for (file_ref, usn_name) in usn_delete_refs {
        let mft_index = *file_ref as usize;
        if mft_index >= records.len() || seen_refs.contains(&mft_index) {
            continue;
        }
        seen_refs.insert(mft_index);

        let record = &records[mft_index];
        if record.in_use || record.is_directory {
            continue;
        }

        if let Ok(Some(mut candidate)) =
            build_deleted_candidate(reader, boot_sector, allocation_bitmap, path_index, record)
        {
            if !usn_name.is_empty() {
                candidate.name = usn_name.clone();
                if let Some(dot_pos) = usn_name.rfind('.') {
                    candidate.extension = usn_name[dot_pos + 1..].to_lowercase();
                }
            }
            candidate.path = format!("/usn-journal-recovered{}", candidate.path);
            candidate.recovery_score = candidate.recovery_score.saturating_sub(5);
            candidates.push(candidate);
        }
    }

    Ok(candidates)
}

fn read_exact_at_ntfs(reader: &mut File, offset: u64, buf: &mut [u8]) -> Result<(), String> {
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|e| format!("seek: {e}"))?;
    reader.read_exact(buf).map_err(|e| format!("read: {e}"))?;
    Ok(())
}

fn le_u16_ntfs(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn le_u32_ntfs(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn le_u64_ntfs(bytes: &[u8]) -> u64 {
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

#[cfg(test)]
pub(crate) fn synthetic_deleted_ntfs_image() -> Vec<u8> {
    let sector_size = 512usize;
    let record_size = 1024usize;
    let cluster_size = 512usize;
    let total_sectors = 128_u64;
    let total_bytes = sector_size * total_sectors as usize;
    let mut image = vec![0_u8; total_bytes];

    image[3..11].copy_from_slice(b"NTFS    ");
    image[11..13].copy_from_slice(&(sector_size as u16).to_le_bytes());
    image[13] = 1;
    image[40..48].copy_from_slice(&total_sectors.to_le_bytes());
    image[48..56].copy_from_slice(&4_u64.to_le_bytes());
    image[56..64].copy_from_slice(&8_u64.to_le_bytes());
    image[64] = (-10_i8) as u8;
    image[510] = 0x55;
    image[511] = 0xaa;

    let mft_run_start_cluster = 4_u64;
    let mft_run_length_clusters = 32_u64;
    let mft_record_zero = build_file_record(
        0,
        NTFS_FILE_RECORD_IN_USE,
        "$MFT",
        NTFS_ROOT_RECORD_NUMBER,
        false,
        Some(build_non_resident_data_attribute(
            &[(mft_run_start_cluster, mft_run_length_clusters)],
            16 * record_size as u64,
            0,
        )),
        &[],
        Some((2024, 3, 10, 9, 0, 0)),
        Some((2024, 3, 10, 9, 0, 0)),
    );
    write_file_record(
        &mut image,
        boot_cluster_offset(cluster_size, mft_run_start_cluster),
        &mft_record_zero,
    );

    let root_record = build_file_record(
        5,
        NTFS_FILE_RECORD_IN_USE | NTFS_FILE_RECORD_DIRECTORY,
        ".",
        NTFS_ROOT_RECORD_NUMBER,
        true,
        None,
        &[],
        Some((2024, 3, 10, 9, 0, 0)),
        Some((2024, 3, 10, 9, 0, 0)),
    );
    write_file_record(
        &mut image,
        boot_cluster_offset(cluster_size, mft_run_start_cluster) + 5 * record_size as u64,
        &root_record,
    );

    let bitmap_bytes = build_ntfs_bitmap(
        total_sectors as usize,
        &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 41,
        ],
    );
    let bitmap_record = build_file_record(
        6,
        NTFS_FILE_RECORD_IN_USE,
        "$Bitmap",
        NTFS_ROOT_RECORD_NUMBER,
        false,
        Some(build_resident_data_attribute(&bitmap_bytes, 0)),
        &[],
        Some((2024, 3, 10, 9, 0, 0)),
        Some((2024, 3, 10, 9, 0, 0)),
    );
    write_file_record(
        &mut image,
        boot_cluster_offset(cluster_size, mft_run_start_cluster) + 6 * record_size as u64,
        &bitmap_record,
    );

    let docs_record = build_file_record(
        10,
        NTFS_FILE_RECORD_IN_USE | NTFS_FILE_RECORD_DIRECTORY,
        "Docs",
        NTFS_ROOT_RECORD_NUMBER,
        true,
        None,
        &[],
        Some((2024, 3, 11, 10, 30, 0)),
        Some((2024, 3, 11, 10, 30, 0)),
    );
    write_file_record(
        &mut image,
        boot_cluster_offset(cluster_size, mft_run_start_cluster) + 10 * record_size as u64,
        &docs_record,
    );

    let visible_record = build_file_record(
        11,
        NTFS_FILE_RECORD_IN_USE,
        "Readme.txt",
        10,
        false,
        Some(build_resident_data_attribute(b"VISIBLE NTFS", 0)),
        &[],
        Some((2024, 3, 12, 7, 45, 0)),
        Some((2024, 3, 12, 7, 47, 0)),
    );
    write_file_record(
        &mut image,
        boot_cluster_offset(cluster_size, mft_run_start_cluster) + 11 * record_size as u64,
        &visible_record,
    );

    let note_bytes = b"HELLO NTFS";
    let deleted_resident_record = build_file_record(
        12,
        0,
        "Note.txt",
        10,
        false,
        Some(build_resident_data_attribute(note_bytes, 0)),
        &[],
        Some((2024, 3, 14, 9, 26, 12)),
        Some((2024, 3, 15, 16, 8, 0)),
    );
    write_file_record(
        &mut image,
        boot_cluster_offset(cluster_size, mft_run_start_cluster) + 12 * record_size as u64,
        &deleted_resident_record,
    );

    let deleted_non_resident_record = build_file_record(
        13,
        0,
        "Archive.bin",
        10,
        false,
        Some(build_non_resident_data_attribute(&[(40, 2)], 700, 0)),
        &[],
        Some((2024, 3, 16, 11, 20, 0)),
        Some((2024, 3, 16, 12, 10, 0)),
    );
    write_file_record(
        &mut image,
        boot_cluster_offset(cluster_size, mft_run_start_cluster) + 13 * record_size as u64,
        &deleted_non_resident_record,
    );

    let archive_offset = boot_cluster_offset(cluster_size, 40);
    image[archive_offset as usize..archive_offset as usize + 512].fill(0x41);
    let overwritten_offset = boot_cluster_offset(cluster_size, 41);
    image[overwritten_offset as usize..overwritten_offset as usize + 512].fill(0x5a);

    image
}

#[cfg(test)]
pub(crate) fn synthetic_sparse_deleted_ntfs_image() -> Vec<u8> {
    let mut image = synthetic_deleted_ntfs_image();
    let cluster_size = 512usize;
    let record_size = 1024usize;
    let mft_run_start_cluster = 4_u64;

    let sparse_deleted_record = build_file_record(
        14,
        0,
        "Sparse.bin",
        10,
        false,
        Some(build_non_resident_data_attribute_segments(
            &[
                TestRunSegment::Data(50, 1),
                TestRunSegment::Sparse(1),
                TestRunSegment::Data(52, 1),
            ],
            1536,
            0,
        )),
        &[],
        Some((2024, 3, 18, 10, 0, 0)),
        Some((2024, 3, 18, 10, 5, 0)),
    );
    write_file_record(
        &mut image,
        boot_cluster_offset(cluster_size, mft_run_start_cluster) + 14 * record_size as u64,
        &sparse_deleted_record,
    );

    let bitmap_bytes = build_ntfs_bitmap(
        128,
        &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 41, 51,
        ],
    );
    let bitmap_record = build_file_record(
        6,
        NTFS_FILE_RECORD_IN_USE,
        "$Bitmap",
        NTFS_ROOT_RECORD_NUMBER,
        false,
        Some(build_resident_data_attribute(&bitmap_bytes, 0)),
        &[],
        Some((2024, 3, 10, 9, 0, 0)),
        Some((2024, 3, 10, 9, 0, 0)),
    );
    write_file_record(
        &mut image,
        boot_cluster_offset(cluster_size, mft_run_start_cluster) + 6 * record_size as u64,
        &bitmap_record,
    );

    let first_offset = boot_cluster_offset(cluster_size, 50);
    image[first_offset as usize..first_offset as usize + 512].fill(0x53);
    let second_offset = boot_cluster_offset(cluster_size, 52);
    image[second_offset as usize..second_offset as usize + 512].fill(0x54);

    image
}

#[cfg(test)]
pub(crate) fn synthetic_compressed_deleted_ntfs_image() -> Vec<u8> {
    let mut image = synthetic_deleted_ntfs_image();
    let cluster_size = 512usize;
    let record_size = 1024usize;
    let mft_run_start_cluster = 4_u64;

    let logical_bytes = b"NTFS LZNT1 TEST DATA NTFS LZNT1 TEST DATA NTFS LZNT1 TEST DATA NTFS LZNT1 TEST DATA NTFS LZNT1 TEST DATA NTFS LZNT1 TEST DATA ".repeat(8);
    let mut compressed_bytes = Vec::new();
    compress_lznt1(&logical_bytes, &mut compressed_bytes);
    assert!(
        compressed_bytes.len() < logical_bytes.len(),
        "synthetic NTFS compressed fixture should actually compress"
    );
    assert!(
        compressed_bytes.len() <= cluster_size,
        "synthetic NTFS compressed fixture should fit in one cluster"
    );

    let compressed_record = build_file_record(
        15,
        0,
        "Compressed.txt",
        10,
        false,
        Some(build_compressed_non_resident_data_attribute_segments(
            &[TestRunSegment::Data(60, 1)],
            compressed_bytes.len() as u64,
            logical_bytes.len() as u64,
            4,
            0,
        )),
        &[],
        Some((2024, 3, 19, 8, 30, 0)),
        Some((2024, 3, 19, 8, 35, 0)),
    );
    write_file_record(
        &mut image,
        boot_cluster_offset(cluster_size, mft_run_start_cluster) + 15 * record_size as u64,
        &compressed_record,
    );

    let compressed_offset = boot_cluster_offset(cluster_size, 60) as usize;
    image[compressed_offset..compressed_offset + compressed_bytes.len()]
        .copy_from_slice(&compressed_bytes);

    image
}

#[cfg(test)]
pub(crate) fn synthetic_ntfs_named_streams_image() -> Vec<u8> {
    let mut image = synthetic_deleted_ntfs_image();
    let cluster_size = 512usize;
    let record_size = 1024usize;
    let mft_run_start_cluster = 4_u64;

    let visible_record = build_file_record(
        11,
        NTFS_FILE_RECORD_IN_USE,
        "Readme.txt",
        10,
        false,
        Some(build_resident_data_attribute(b"VISIBLE NTFS", 0)),
        &[build_named_resident_data_attribute(
            b"[ZoneTransfer]\r\nZoneId=3\r\n",
            "Zone.Identifier",
            1,
        )],
        Some((2024, 3, 12, 7, 45, 0)),
        Some((2024, 3, 12, 7, 47, 0)),
    );
    write_file_record(
        &mut image,
        boot_cluster_offset(cluster_size, mft_run_start_cluster) + 11 * record_size as u64,
        &visible_record,
    );

    let deleted_record = build_file_record(
        12,
        0,
        "Note.txt",
        10,
        false,
        Some(build_resident_data_attribute(b"HELLO NTFS", 0)),
        &[build_named_resident_data_attribute(
            b"NTFS ADS NOTE",
            "Summary",
            1,
        )],
        Some((2024, 3, 14, 9, 26, 12)),
        Some((2024, 3, 15, 16, 8, 0)),
    );
    write_file_record(
        &mut image,
        boot_cluster_offset(cluster_size, mft_run_start_cluster) + 12 * record_size as u64,
        &deleted_record,
    );

    image
}

#[cfg(test)]
fn build_ntfs_bitmap(cluster_count: usize, allocated_clusters: &[usize]) -> Vec<u8> {
    let mut bitmap = vec![0_u8; cluster_count.div_ceil(8)];
    for cluster in allocated_clusters {
        let byte_index = cluster / 8;
        let bit_index = cluster % 8;
        if let Some(byte) = bitmap.get_mut(byte_index) {
            *byte |= 1 << bit_index;
        }
    }
    bitmap
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn build_file_record(
    record_number: u64,
    flags: u16,
    name: &str,
    parent_record_number: u64,
    directory: bool,
    data_attribute: Option<Vec<u8>>,
    extra_attributes: &[Vec<u8>],
    created_at: Option<(u16, u8, u8, u8, u8, u8)>,
    modified_at: Option<(u16, u8, u8, u8, u8, u8)>,
) -> Vec<u8> {
    let mut record = vec![0_u8; 1024];
    let first_attribute_offset = 56usize;
    record[0..4].copy_from_slice(b"FILE");
    record[4..6].copy_from_slice(&48_u16.to_le_bytes());
    record[6..8].copy_from_slice(&3_u16.to_le_bytes());
    record[16..18].copy_from_slice(&1_u16.to_le_bytes());
    record[18..20].copy_from_slice(&1_u16.to_le_bytes());
    record[20..22].copy_from_slice(&(first_attribute_offset as u16).to_le_bytes());
    record[22..24].copy_from_slice(&flags.to_le_bytes());
    record[28..32].copy_from_slice(&(1024_u32).to_le_bytes());
    record[44..48].copy_from_slice(&(record_number as u32).to_le_bytes());

    let allocated_size = data_attribute
        .as_ref()
        .map(|attribute| attribute_data_size(attribute))
        .unwrap_or(0);
    let file_name_attribute = build_file_name_attribute(
        name,
        parent_record_number,
        directory,
        allocated_size,
        allocated_size,
        created_at,
        modified_at,
        0,
    );

    let mut cursor = first_attribute_offset;
    record[cursor..cursor + file_name_attribute.len()].copy_from_slice(&file_name_attribute);
    cursor += file_name_attribute.len();

    if let Some(data_attribute) = data_attribute {
        record[cursor..cursor + data_attribute.len()].copy_from_slice(&data_attribute);
        cursor += data_attribute.len();
    }

    for attribute in extra_attributes {
        record[cursor..cursor + attribute.len()].copy_from_slice(attribute);
        cursor += attribute.len();
    }

    record[cursor..cursor + 4].copy_from_slice(&NTFS_ATTR_END.to_le_bytes());
    cursor += 8;
    record[24..28].copy_from_slice(&(cursor as u32).to_le_bytes());

    apply_test_fixups(&mut record);
    record
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn build_file_name_attribute(
    name: &str,
    parent_record_number: u64,
    directory: bool,
    allocated_size: u64,
    real_size: u64,
    created_at: Option<(u16, u8, u8, u8, u8, u8)>,
    modified_at: Option<(u16, u8, u8, u8, u8, u8)>,
    attr_id: u16,
) -> Vec<u8> {
    let encoded_name: Vec<u8> = name
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    let value_length = 66 + encoded_name.len();
    let attribute_length = align_to_eight(24 + value_length);
    let mut attribute = vec![0_u8; attribute_length];

    attribute[0..4].copy_from_slice(&NTFS_ATTR_FILE_NAME.to_le_bytes());
    attribute[4..8].copy_from_slice(&(attribute_length as u32).to_le_bytes());
    attribute[16..20].copy_from_slice(&(value_length as u32).to_le_bytes());
    attribute[20..22].copy_from_slice(&24_u16.to_le_bytes());
    attribute[14..16].copy_from_slice(&attr_id.to_le_bytes());

    let value = &mut attribute[24..24 + value_length];
    value[0..8].copy_from_slice(&parent_record_number.to_le_bytes());
    value[8..16].copy_from_slice(&encode_ntfs_filetime(created_at).to_le_bytes());
    value[16..24].copy_from_slice(&encode_ntfs_filetime(modified_at).to_le_bytes());
    value[24..32].copy_from_slice(&encode_ntfs_filetime(modified_at).to_le_bytes());
    value[32..40].copy_from_slice(&encode_ntfs_filetime(modified_at).to_le_bytes());
    value[40..48].copy_from_slice(&allocated_size.to_le_bytes());
    value[48..56].copy_from_slice(&real_size.to_le_bytes());
    value[56..60]
        .copy_from_slice(&(if directory { 0x1000_0000_u32 } else { 0x20_u32 }).to_le_bytes());
    value[64] = name.encode_utf16().count() as u8;
    value[65] = 0x01;
    value[66..66 + encoded_name.len()].copy_from_slice(&encoded_name);

    attribute
}

#[cfg(test)]
fn build_resident_data_attribute(data: &[u8], attr_id: u16) -> Vec<u8> {
    let value_length = data.len();
    let attribute_length = align_to_eight(24 + value_length);
    let mut attribute = vec![0_u8; attribute_length];
    attribute[0..4].copy_from_slice(&NTFS_ATTR_DATA.to_le_bytes());
    attribute[4..8].copy_from_slice(&(attribute_length as u32).to_le_bytes());
    attribute[14..16].copy_from_slice(&attr_id.to_le_bytes());
    attribute[16..20].copy_from_slice(&(value_length as u32).to_le_bytes());
    attribute[20..22].copy_from_slice(&24_u16.to_le_bytes());
    attribute[24..24 + data.len()].copy_from_slice(data);
    attribute
}

#[cfg(test)]
fn build_named_resident_data_attribute(data: &[u8], name: &str, attr_id: u16) -> Vec<u8> {
    let encoded_name: Vec<u8> = name
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    let name_length = encoded_name.len() / 2;
    let name_offset = 24usize;
    let value_offset = align_to_eight(name_offset + encoded_name.len()) as u16;
    let value_length = data.len();
    let attribute_length = align_to_eight(value_offset as usize + value_length);
    let mut attribute = vec![0_u8; attribute_length];
    attribute[0..4].copy_from_slice(&NTFS_ATTR_DATA.to_le_bytes());
    attribute[4..8].copy_from_slice(&(attribute_length as u32).to_le_bytes());
    attribute[9] = name_length as u8;
    attribute[10..12].copy_from_slice(&(name_offset as u16).to_le_bytes());
    attribute[14..16].copy_from_slice(&attr_id.to_le_bytes());
    attribute[16..20].copy_from_slice(&(value_length as u32).to_le_bytes());
    attribute[20..22].copy_from_slice(&value_offset.to_le_bytes());
    attribute[name_offset..name_offset + encoded_name.len()].copy_from_slice(&encoded_name);
    attribute[value_offset as usize..value_offset as usize + data.len()].copy_from_slice(data);
    attribute
}

#[cfg(test)]
#[derive(Clone, Copy)]
enum TestRunSegment {
    Data(u64, u64),
    Sparse(u64),
}

#[cfg(test)]
fn build_non_resident_data_attribute(runs: &[(u64, u64)], data_size: u64, attr_id: u16) -> Vec<u8> {
    let segments: Vec<TestRunSegment> = runs
        .iter()
        .map(|(start_lcn, length_clusters)| TestRunSegment::Data(*start_lcn, *length_clusters))
        .collect();
    build_non_resident_data_attribute_segments(&segments, data_size, attr_id)
}

#[cfg(test)]
fn build_non_resident_data_attribute_segments(
    segments: &[TestRunSegment],
    data_size: u64,
    attr_id: u16,
) -> Vec<u8> {
    let runlist = encode_runlist(segments);
    let attribute_length = align_to_eight(64 + runlist.len());
    let mut attribute = vec![0_u8; attribute_length];
    let allocated_size: u64 = segments
        .iter()
        .filter_map(|segment| match segment {
            TestRunSegment::Data(_, length) => Some(length.saturating_mul(512)),
            TestRunSegment::Sparse(_) => None,
        })
        .sum();
    let highest_vcn = segments
        .iter()
        .map(|segment| match segment {
            TestRunSegment::Data(_, length) | TestRunSegment::Sparse(length) => *length,
        })
        .sum::<u64>()
        .saturating_sub(1);

    attribute[0..4].copy_from_slice(&NTFS_ATTR_DATA.to_le_bytes());
    attribute[4..8].copy_from_slice(&(attribute_length as u32).to_le_bytes());
    attribute[8] = 1;
    attribute[14..16].copy_from_slice(&attr_id.to_le_bytes());
    attribute[24..32].copy_from_slice(&highest_vcn.to_le_bytes());
    attribute[32..34].copy_from_slice(&64_u16.to_le_bytes());
    attribute[40..48].copy_from_slice(&allocated_size.to_le_bytes());
    attribute[48..56].copy_from_slice(&data_size.to_le_bytes());
    attribute[56..64].copy_from_slice(&data_size.to_le_bytes());
    attribute[64..64 + runlist.len()].copy_from_slice(&runlist);

    attribute
}

#[cfg(test)]
fn build_compressed_non_resident_data_attribute_segments(
    segments: &[TestRunSegment],
    stored_size: u64,
    data_size: u64,
    compression_unit: u16,
    attr_id: u16,
) -> Vec<u8> {
    let runlist = encode_runlist(segments);
    let attribute_length = align_to_eight(64 + runlist.len());
    let mut attribute = vec![0_u8; attribute_length];
    let highest_vcn = segments
        .iter()
        .map(|segment| match segment {
            TestRunSegment::Data(_, length) | TestRunSegment::Sparse(length) => *length,
        })
        .sum::<u64>()
        .saturating_sub(1);

    attribute[0..4].copy_from_slice(&NTFS_ATTR_DATA.to_le_bytes());
    attribute[4..8].copy_from_slice(&(attribute_length as u32).to_le_bytes());
    attribute[8] = 1;
    attribute[12..14].copy_from_slice(&NTFS_ATTR_FLAG_COMPRESSED.to_le_bytes());
    attribute[14..16].copy_from_slice(&attr_id.to_le_bytes());
    attribute[24..32].copy_from_slice(&highest_vcn.to_le_bytes());
    attribute[32..34].copy_from_slice(&64_u16.to_le_bytes());
    attribute[34..36].copy_from_slice(&compression_unit.to_le_bytes());
    attribute[40..48].copy_from_slice(&stored_size.to_le_bytes());
    attribute[48..56].copy_from_slice(&data_size.to_le_bytes());
    attribute[56..64].copy_from_slice(&data_size.to_le_bytes());
    attribute[64..64 + runlist.len()].copy_from_slice(&runlist);

    attribute
}

#[cfg(test)]
fn attribute_data_size(attribute: &[u8]) -> u64 {
    if attribute[8] == 0 {
        le_u32(&attribute[16..20]) as u64
    } else {
        le_u64(&attribute[48..56])
    }
}

#[cfg(test)]
fn encode_runlist(runs: &[TestRunSegment]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut previous_lcn = 0_i64;

    for segment in runs {
        match segment {
            TestRunSegment::Data(start_lcn, length_clusters) => {
                let relative_offset = *start_lcn as i64 - previous_lcn;
                let length_bytes = encode_unsigned(*length_clusters);
                let offset_bytes = encode_signed(relative_offset);
                bytes.push(((offset_bytes.len() as u8) << 4) | (length_bytes.len() as u8));
                bytes.extend_from_slice(&length_bytes);
                bytes.extend_from_slice(&offset_bytes);
                previous_lcn = *start_lcn as i64;
            }
            TestRunSegment::Sparse(length_clusters) => {
                let length_bytes = encode_unsigned(*length_clusters);
                bytes.push(length_bytes.len() as u8);
                bytes.extend_from_slice(&length_bytes);
            }
        }
    }

    bytes.push(0);
    bytes
}

#[cfg(test)]
fn encode_unsigned(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        bytes.push((value & 0xff) as u8);
        value >>= 8;
        if value == 0 {
            break;
        }
    }
    bytes
}

#[cfg(test)]
fn encode_signed(mut value: i64) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        bytes.push((value & 0xff) as u8);
        let next = value >> 8;
        let sign_bit_set = bytes.last().copied().unwrap_or(0) & 0x80 != 0;
        if (next == 0 && !sign_bit_set) || (next == -1 && sign_bit_set) {
            break;
        }
        value = next;
    }
    bytes
}

#[cfg(test)]
fn apply_test_fixups(record: &mut [u8]) {
    let usa_offset = 48usize;
    let sequence = [0xa5_u8, 0xa5_u8];
    record[usa_offset..usa_offset + 2].copy_from_slice(&sequence);

    for sector_index in 0..2usize {
        let sector_end = (sector_index + 1) * 512 - 2;
        let replacement_offset = usa_offset + 2 + sector_index * 2;
        let original = [record[sector_end], record[sector_end + 1]];
        record[replacement_offset..replacement_offset + 2].copy_from_slice(&original);
        record[sector_end..sector_end + 2].copy_from_slice(&sequence);
    }
}

#[cfg(test)]
fn write_file_record(image: &mut [u8], offset: u64, record: &[u8]) {
    let offset = offset as usize;
    image[offset..offset + record.len()].copy_from_slice(record);
}

#[cfg(test)]
fn boot_cluster_offset(cluster_size: usize, cluster: u64) -> u64 {
    cluster.saturating_mul(cluster_size as u64)
}

#[cfg(test)]
fn align_to_eight(length: usize) -> usize {
    (length + 7) & !7
}

#[cfg(test)]
fn encode_ntfs_filetime(timestamp: Option<(u16, u8, u8, u8, u8, u8)>) -> u64 {
    let Some((year, month, day, hour, minute, second)) = timestamp else {
        return 0;
    };
    let days = days_from_civil(year as i32, month as u32, day as u32);
    let unix_seconds = days * 86_400 + hour as i64 * 3_600 + minute as i64 * 60 + second as i64;
    ((unix_seconds + 11_644_473_600) as u64) * 10_000_000
}

#[cfg(test)]
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let mut year = year;
    let month = month as i32;
    year -= if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs, fs::File};

    fn write_test_image(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let root = env::temp_dir().join(format!("recupere-ntfs-test-{}", std::process::id()));
        fs::create_dir_all(&root).expect("ntfs test workspace should exist");
        let path = root.join(name);
        fs::write(&path, bytes).expect("ntfs test image should be written");
        path
    }

    #[test]
    fn recover_deleted_files_reads_deleted_ntfs_entries() {
        let image_path = write_test_image("deleted-ntfs.img", &synthetic_deleted_ntfs_image());
        let mut reader = File::open(&image_path).expect("synthetic NTFS image should open");
        let boot_sector = NtfsBootSector::read_from(&mut reader)
            .expect("synthetic NTFS boot sector should parse");
        let record_zero_offset = boot_sector
            .cluster_offset(boot_sector.mft_lcn)
            .expect("MFT offset should resolve");
        let record_zero = read_file_record(
            &mut reader,
            record_zero_offset,
            boot_sector.record_size_bytes as usize,
            boot_sector.bytes_per_sector as usize,
        )
        .expect("MFT record zero should be readable");
        let parsed_mft = parse_mft_record(&record_zero, record_zero_offset, 0)
            .expect("MFT record zero should parse");
        let DataAttribute::NonResident {
            data_size,
            cluster_runs,
            ..
        } = parsed_mft
            .data_attribute
            .expect("MFT record zero should expose data")
        else {
            panic!("MFT record zero should expose non-resident data");
        };
        let (record_twelve, record_twelve_offset) =
            read_mft_record_by_index(&mut reader, &boot_sector, &cluster_runs, data_size, 12)
                .expect("record 12 should be readable");
        let parsed_twelve = parse_mft_record(&record_twelve, record_twelve_offset, 12)
            .expect("record 12 should parse");
        assert!(!parsed_twelve.in_use, "record 12 should be marked deleted");
        assert!(!parsed_twelve.is_directory, "record 12 should be a file");
        assert!(
            parsed_twelve.file_name.is_some(),
            "record 12 should expose a FILE_NAME attribute"
        );
        assert!(
            parsed_twelve.data_attribute.is_some(),
            "record 12 should expose a DATA attribute"
        );

        let files = recover_deleted_files(&image_path)
            .expect("synthetic NTFS image should expose deleted files");

        assert_eq!(files.len(), 2);

        let note = files
            .iter()
            .find(|file| file.name == "Note.txt")
            .expect("resident NTFS file should be present");
        assert_eq!(note.path, "/Docs");
        assert_eq!(note.size_bytes, 10);
        assert_eq!(note.expected_size_bytes, 10);
        assert_eq!(note.integrity, "intact");
        assert_eq!(note.created_at.as_deref(), Some("2024-03-14T09:26:12"));
        assert_eq!(note.modified_at.as_deref(), Some("2024-03-15T16:08:00"));

        let archive = files
            .iter()
            .find(|file| file.name == "Archive.bin")
            .expect("non-resident NTFS file should be present");
        assert_eq!(archive.path, "/Docs");
        assert_eq!(archive.size_bytes, 512);
        assert_eq!(archive.expected_size_bytes, 700);
        assert_eq!(archive.integrity, "partial");
        assert_eq!(archive.clusters, vec![40]);
    }

    #[test]
    fn recover_deleted_files_marks_non_resident_ranges_from_bitmap() {
        let image_path =
            write_test_image("deleted-ntfs-partial.img", &synthetic_deleted_ntfs_image());
        let files = recover_deleted_files(&image_path)
            .expect("synthetic NTFS image should expose deleted files");

        let archive = files
            .iter()
            .find(|file| file.name == "Archive.bin")
            .expect("non-resident NTFS file should be present");

        assert_eq!(archive.byte_runs.len(), 1);
        assert_eq!(archive.byte_runs[0].length, 512);
        assert_eq!(archive.start_offset, archive.byte_runs[0].offset);
    }

    #[test]
    fn recover_deleted_files_supports_sparse_non_resident_runlists() {
        let image_path = write_test_image(
            "deleted-ntfs-sparse.img",
            &synthetic_sparse_deleted_ntfs_image(),
        );
        let files = recover_deleted_files(&image_path)
            .expect("synthetic sparse NTFS image should expose deleted files");

        let sparse = files
            .iter()
            .find(|file| file.name == "Sparse.bin")
            .expect("sparse NTFS deleted file should be present");

        assert_eq!(sparse.path, "/Docs");
        assert_eq!(sparse.size_bytes, 1536);
        assert_eq!(sparse.expected_size_bytes, 1536);
        assert_eq!(sparse.integrity, "fragmented");
        assert_eq!(sparse.clusters, vec![50, 52]);
        assert_eq!(sparse.byte_runs.len(), 3);
        assert!(!sparse.byte_runs[0].zero_fill);
        assert!(sparse.byte_runs[1].zero_fill);
        assert_eq!(sparse.byte_runs[1].length, 512);
        assert!(!sparse.byte_runs[2].zero_fill);
    }

    #[test]
    fn recover_deleted_files_surfaces_named_data_streams() {
        let image_path = write_test_image(
            "deleted-ntfs-ads.img",
            &synthetic_ntfs_named_streams_image(),
        );
        let files = recover_deleted_files(&image_path)
            .expect("synthetic NTFS ADS image should expose deleted files");

        let note = files
            .iter()
            .find(|file| file.name == "Note.txt")
            .expect("resident NTFS deleted file should be present");
        assert_eq!(note.alternate_data_streams.len(), 1);
        assert_eq!(note.alternate_data_streams[0].name, "Summary");
        assert_eq!(note.alternate_data_streams[0].size_bytes, 13);
        assert_eq!(note.alternate_data_streams[0].expected_size_bytes, Some(13));
    }

    #[test]
    fn recover_deleted_files_supports_lznt1_compressed_non_resident_attributes() {
        let image_path = write_test_image(
            "deleted-ntfs-compressed.img",
            &synthetic_compressed_deleted_ntfs_image(),
        );
        let files = recover_deleted_files(&image_path)
            .expect("synthetic compressed NTFS image should expose deleted files");

        let compressed = files
            .iter()
            .find(|file| file.name == "Compressed.txt")
            .expect("compressed NTFS deleted file should be present");

        assert_eq!(compressed.path, "/Docs");
        assert_eq!(compressed.integrity, "intact");
        assert_eq!(compressed.compression_kind.as_deref(), Some("lznt1"));
        assert_eq!(compressed.expected_size_bytes, compressed.size_bytes);
        assert_eq!(compressed.byte_runs.len(), 1);
        assert_eq!(
            compressed.byte_runs[0].compression_kind.as_deref(),
            Some("lznt1")
        );
    }

    #[test]
    fn list_visible_files_reads_visible_ntfs_entries() {
        let image_path = write_test_image("visible-ntfs.img", &synthetic_deleted_ntfs_image());
        let files = list_visible_files(&image_path)
            .expect("synthetic NTFS image should expose visible files");

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "Readme.txt");
        assert_eq!(files[0].extension, "txt");
        assert_eq!(files[0].path, "/Docs");
        assert_eq!(files[0].size_bytes, 12);
        assert_eq!(files[0].integrity, "intact");
        assert_eq!(files[0].created_at.as_deref(), Some("2024-03-12T07:45:00"));
        assert_eq!(files[0].modified_at.as_deref(), Some("2024-03-12T07:47:00"));
        assert!(files[0].start_offset.is_some());
        assert!(files[0].clusters.is_empty());
        assert_eq!(files[0].byte_runs.len(), 1);
    }

    #[test]
    fn list_visible_files_surfaces_named_data_streams() {
        let image_path = write_test_image(
            "visible-ntfs-ads.img",
            &synthetic_ntfs_named_streams_image(),
        );
        let files = list_visible_files(&image_path)
            .expect("synthetic NTFS ADS image should expose visible files");

        let readme = files
            .iter()
            .find(|file| file.name == "Readme.txt")
            .expect("visible NTFS file should be present");
        assert_eq!(readme.alternate_data_streams.len(), 1);
        assert_eq!(readme.alternate_data_streams[0].name, "Zone.Identifier");
        assert_eq!(readme.alternate_data_streams[0].size_bytes, 26);
        assert_eq!(
            readme.alternate_data_streams[0].expected_size_bytes,
            Some(26)
        );
    }
}
