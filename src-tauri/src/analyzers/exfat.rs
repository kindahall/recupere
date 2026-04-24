use crate::types::ByteRun;
use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const EXFAT_ENTRY_END: u8 = 0x00;
const EXFAT_ENTRY_IN_USE: u8 = 0x80;
const EXFAT_ENTRY_TYPE_MASK: u8 = 0x7f;
const EXFAT_ENTRY_TYPE_ALLOCATION_BITMAP: u8 = 0x01;
const EXFAT_ENTRY_TYPE_FILE: u8 = 0x05;
const EXFAT_ENTRY_TYPE_STREAM_EXTENSION: u8 = 0x40;
const EXFAT_ENTRY_TYPE_FILE_NAME: u8 = 0x41;
const EXFAT_ATTR_DIRECTORY: u16 = 0x0010;
const EXFAT_EOC_MIN: u32 = 0xffff_fff8;

#[derive(Debug, Clone)]
pub struct ExfatDeletedFileCandidate {
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
}

#[derive(Debug, Clone)]
pub struct ExfatVisibleFileCandidate {
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
}

#[derive(Debug, Clone)]
struct ExfatBootSector {
    fat_offset_sectors: u32,
    cluster_heap_offset_sectors: u32,
    cluster_count: u32,
    root_dir_first_cluster: u32,
    bytes_per_sector_shift: u8,
    sectors_per_cluster_shift: u8,
}

impl ExfatBootSector {
    fn read_from(reader: &mut File) -> Result<Self, String> {
        let mut sector = [0_u8; 512];
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|error| format!("Unable to seek the exFAT boot sector: {error}"))?;
        reader
            .read_exact(&mut sector)
            .map_err(|error| format!("Unable to read the exFAT boot sector: {error}"))?;

        if &sector[3..11] != b"EXFAT   " {
            return Err("The image does not expose an exFAT filesystem name.".into());
        }

        if sector[510] != 0x55 || sector[511] != 0xaa {
            return Err("The image does not expose a valid exFAT boot signature.".into());
        }

        let fat_offset_sectors = le_u32(&sector[80..84]);
        let cluster_heap_offset_sectors = le_u32(&sector[88..92]);
        let cluster_count = le_u32(&sector[92..96]);
        let root_dir_first_cluster = le_u32(&sector[96..100]);
        let bytes_per_sector_shift = sector[108];
        let sectors_per_cluster_shift = sector[109];

        if !(9..=12).contains(&bytes_per_sector_shift)
            || sectors_per_cluster_shift > 25
            || fat_offset_sectors == 0
            || cluster_heap_offset_sectors == 0
            || cluster_count < 1
            || root_dir_first_cluster < 2
        {
            return Err("The image does not contain a usable exFAT layout.".into());
        }

        Ok(Self {
            fat_offset_sectors,
            cluster_heap_offset_sectors,
            cluster_count,
            root_dir_first_cluster,
            bytes_per_sector_shift,
            sectors_per_cluster_shift,
        })
    }

    fn bytes_per_sector(&self) -> u64 {
        1_u64 << self.bytes_per_sector_shift
    }

    fn sectors_per_cluster(&self) -> u64 {
        1_u64 << self.sectors_per_cluster_shift
    }

    fn cluster_size_bytes(&self) -> u64 {
        self.bytes_per_sector() * self.sectors_per_cluster()
    }

    fn fat_offset_bytes(&self) -> u64 {
        self.fat_offset_sectors as u64 * self.bytes_per_sector()
    }

    fn cluster_offset(&self, cluster: u32) -> Result<u64, String> {
        validate_cluster(cluster, self.cluster_count)?;
        Ok((self.cluster_heap_offset_sectors as u64
            + (cluster as u64 - 2) * self.sectors_per_cluster())
            * self.bytes_per_sector())
    }
}

#[derive(Debug, Clone)]
struct AllocationBitmap {
    bytes: Vec<u8>,
    cluster_count: u32,
}

impl AllocationBitmap {
    fn cluster_is_free(&self, cluster: u32) -> Result<bool, String> {
        validate_cluster(cluster, self.cluster_count)?;
        let bit_index = (cluster - 2) as usize;
        let byte_index = bit_index / 8;
        let bit_offset = bit_index % 8;
        let byte = *self.bytes.get(byte_index).ok_or_else(|| {
            format!("The exFAT allocation bitmap does not cover cluster {cluster}.")
        })?;
        Ok(((byte >> bit_offset) & 0x01) == 0)
    }
}

#[derive(Debug, Clone)]
struct StreamEntry {
    allocation_possible: bool,
    no_fat_chain: bool,
    name_length: usize,
    first_cluster: u32,
    data_length: u64,
}

#[derive(Debug, Clone)]
struct FileEntryRecord {
    deleted: bool,
    directory: bool,
    display_name: String,
    extension: String,
    created_at: Option<String>,
    modified_at: Option<String>,
    allocation_possible: bool,
    no_fat_chain: bool,
    first_cluster: u32,
    data_length: u64,
}

pub fn recover_deleted_files(image_path: &Path) -> Result<Vec<ExfatDeletedFileCandidate>, String> {
    let mut reader = File::open(image_path).map_err(|error| {
        format!(
            "Unable to open the exFAT image {}: {}",
            image_path.to_string_lossy(),
            error
        )
    })?;
    let boot_sector = ExfatBootSector::read_from(&mut reader)?;
    let root_directory_bytes = read_root_directory_bytes(&mut reader, &boot_sector)?;
    let allocation_bitmap =
        load_allocation_bitmap(&mut reader, &boot_sector, &root_directory_bytes)?;
    let mut deleted_files = Vec::new();
    let mut visited_directories = HashSet::new();

    scan_directory(
        &mut reader,
        &boot_sector,
        &allocation_bitmap,
        DirectoryLocation::Root {
            start_cluster: boot_sector.root_dir_first_cluster,
        },
        "/",
        &mut visited_directories,
        &mut deleted_files,
    )?;

    Ok(deleted_files)
}

pub fn list_visible_files(image_path: &Path) -> Result<Vec<ExfatVisibleFileCandidate>, String> {
    let mut reader = File::open(image_path).map_err(|error| {
        format!(
            "Unable to open the exFAT image {}: {}",
            image_path.to_string_lossy(),
            error
        )
    })?;
    let boot_sector = ExfatBootSector::read_from(&mut reader)?;
    let root_directory_bytes = read_root_directory_bytes(&mut reader, &boot_sector)?;
    let allocation_bitmap =
        load_allocation_bitmap(&mut reader, &boot_sector, &root_directory_bytes)?;
    let mut visible_files = Vec::new();
    let mut visited_directories = HashSet::new();

    scan_visible_directory(
        &mut reader,
        &boot_sector,
        &allocation_bitmap,
        DirectoryLocation::Root {
            start_cluster: boot_sector.root_dir_first_cluster,
        },
        "/",
        &mut visited_directories,
        &mut visible_files,
    )?;

    Ok(visible_files)
}

#[derive(Debug, Clone)]
enum DirectoryLocation {
    Root {
        start_cluster: u32,
    },
    Child {
        start_cluster: u32,
        data_length: u64,
        no_fat_chain: bool,
    },
}

fn read_root_directory_bytes(
    reader: &mut File,
    boot_sector: &ExfatBootSector,
) -> Result<Vec<u8>, String> {
    let clusters = follow_cluster_chain(
        reader,
        boot_sector,
        boot_sector.root_dir_first_cluster,
        None,
    )?;
    read_clusters(reader, boot_sector, &clusters, None)
}

fn load_allocation_bitmap(
    reader: &mut File,
    boot_sector: &ExfatBootSector,
    root_directory_bytes: &[u8],
) -> Result<AllocationBitmap, String> {
    for chunk in root_directory_bytes.chunks_exact(32) {
        if chunk[0] == EXFAT_ENTRY_END {
            break;
        }

        if chunk[0] == (EXFAT_ENTRY_IN_USE | EXFAT_ENTRY_TYPE_ALLOCATION_BITMAP) {
            let first_cluster = le_u32(&chunk[20..24]);
            let data_length = le_u64(&chunk[24..32]);
            let required_clusters =
                required_clusters(data_length, boot_sector.cluster_size_bytes());
            let clusters = follow_cluster_chain(
                reader,
                boot_sector,
                first_cluster,
                Some(required_clusters.max(1)),
            )?;
            let bytes = read_clusters(reader, boot_sector, &clusters, Some(data_length))?;
            return Ok(AllocationBitmap {
                bytes,
                cluster_count: boot_sector.cluster_count,
            });
        }
    }

    Err("The exFAT root directory does not expose an allocation bitmap entry.".into())
}

fn scan_directory(
    reader: &mut File,
    boot_sector: &ExfatBootSector,
    allocation_bitmap: &AllocationBitmap,
    location: DirectoryLocation,
    current_path: &str,
    visited_directories: &mut HashSet<u32>,
    deleted_files: &mut Vec<ExfatDeletedFileCandidate>,
) -> Result<(), String> {
    let start_cluster = match &location {
        DirectoryLocation::Root { start_cluster } => *start_cluster,
        DirectoryLocation::Child { start_cluster, .. } => *start_cluster,
    };

    if !visited_directories.insert(start_cluster) {
        return Ok(());
    }

    let directory_bytes = read_directory_bytes(reader, boot_sector, &location)?;
    let mut entry_index = 0usize;

    while entry_index + 32 <= directory_bytes.len() {
        let primary = &directory_bytes[entry_index..entry_index + 32];
        let entry_type = primary[0];
        if entry_type == EXFAT_ENTRY_END {
            break;
        }

        if entry_type & EXFAT_ENTRY_TYPE_MASK != EXFAT_ENTRY_TYPE_FILE {
            entry_index += 32;
            continue;
        }

        let secondary_count = primary[1] as usize;
        let next_index = entry_index + 32 * (secondary_count + 1);
        if next_index > directory_bytes.len() {
            break;
        }

        let secondaries = &directory_bytes[entry_index + 32..next_index];
        if let Some(record) = parse_file_entry_set(primary, secondaries) {
            if record.deleted && !record.directory {
                deleted_files.push(build_deleted_candidate(
                    reader,
                    boot_sector,
                    allocation_bitmap,
                    current_path,
                    &record,
                )?);
            } else if !record.deleted
                && record.directory
                && record.allocation_possible
                && record.first_cluster >= 2
                && record.data_length > 0
            {
                let next_path = if current_path == "/" {
                    format!("/{}", record.display_name)
                } else {
                    format!("{current_path}/{}", record.display_name)
                };
                scan_directory(
                    reader,
                    boot_sector,
                    allocation_bitmap,
                    DirectoryLocation::Child {
                        start_cluster: record.first_cluster,
                        data_length: record.data_length,
                        no_fat_chain: record.no_fat_chain,
                    },
                    &next_path,
                    visited_directories,
                    deleted_files,
                )?;
            }
        }

        entry_index = next_index;
    }

    Ok(())
}

fn scan_visible_directory(
    reader: &mut File,
    boot_sector: &ExfatBootSector,
    _allocation_bitmap: &AllocationBitmap,
    location: DirectoryLocation,
    current_path: &str,
    visited_directories: &mut HashSet<u32>,
    visible_files: &mut Vec<ExfatVisibleFileCandidate>,
) -> Result<(), String> {
    let start_cluster = match &location {
        DirectoryLocation::Root { start_cluster } => *start_cluster,
        DirectoryLocation::Child { start_cluster, .. } => *start_cluster,
    };

    if !visited_directories.insert(start_cluster) {
        return Ok(());
    }

    let directory_bytes = read_directory_bytes(reader, boot_sector, &location)?;
    let mut entry_index = 0usize;

    while entry_index + 32 <= directory_bytes.len() {
        let primary = &directory_bytes[entry_index..entry_index + 32];
        let entry_type = primary[0];
        if entry_type == EXFAT_ENTRY_END {
            break;
        }

        if entry_type & EXFAT_ENTRY_TYPE_MASK != EXFAT_ENTRY_TYPE_FILE {
            entry_index += 32;
            continue;
        }

        let secondary_count = primary[1] as usize;
        let next_index = entry_index + 32 * (secondary_count + 1);
        if next_index > directory_bytes.len() {
            break;
        }

        let secondaries = &directory_bytes[entry_index + 32..next_index];
        if let Some(record) = parse_file_entry_set(primary, secondaries) {
            if !record.deleted && !record.directory {
                visible_files.push(build_visible_candidate(
                    reader,
                    boot_sector,
                    current_path,
                    &record,
                )?);
            } else if !record.deleted
                && record.directory
                && record.allocation_possible
                && record.first_cluster >= 2
                && record.data_length > 0
            {
                let next_path = if current_path == "/" {
                    format!("/{}", record.display_name)
                } else {
                    format!("{current_path}/{}", record.display_name)
                };
                scan_visible_directory(
                    reader,
                    boot_sector,
                    _allocation_bitmap,
                    DirectoryLocation::Child {
                        start_cluster: record.first_cluster,
                        data_length: record.data_length,
                        no_fat_chain: record.no_fat_chain,
                    },
                    &next_path,
                    visited_directories,
                    visible_files,
                )?;
            }
        }

        entry_index = next_index;
    }

    Ok(())
}

fn read_directory_bytes(
    reader: &mut File,
    boot_sector: &ExfatBootSector,
    location: &DirectoryLocation,
) -> Result<Vec<u8>, String> {
    match location {
        DirectoryLocation::Root { start_cluster } => {
            let clusters = follow_cluster_chain(reader, boot_sector, *start_cluster, None)?;
            read_clusters(reader, boot_sector, &clusters, None)
        }
        DirectoryLocation::Child {
            start_cluster,
            data_length,
            no_fat_chain,
        } => {
            if *data_length == 0 {
                return Ok(Vec::new());
            }

            let required_clusters =
                required_clusters(*data_length, boot_sector.cluster_size_bytes());
            let clusters = if *no_fat_chain {
                build_contiguous_cluster_range(*start_cluster, required_clusters, boot_sector)?
            } else {
                follow_cluster_chain(reader, boot_sector, *start_cluster, Some(required_clusters))?
            };
            read_clusters(reader, boot_sector, &clusters, Some(*data_length))
        }
    }
}

fn parse_file_entry_set(primary: &[u8], secondaries: &[u8]) -> Option<FileEntryRecord> {
    let deleted = primary[0] & EXFAT_ENTRY_IN_USE == 0;
    let directory = le_u16(&primary[4..6]) & EXFAT_ATTR_DIRECTORY != 0;
    let created_at = decode_exfat_timestamp(le_u32(&primary[8..12]));
    let modified_at = decode_exfat_timestamp(le_u32(&primary[12..16]));

    let mut stream = None;
    let mut name_code_units = Vec::new();

    for secondary in secondaries.chunks_exact(32) {
        match secondary[0] & EXFAT_ENTRY_TYPE_MASK {
            EXFAT_ENTRY_TYPE_STREAM_EXTENSION if stream.is_none() => {
                stream = Some(StreamEntry {
                    allocation_possible: secondary[1] & 0x01 != 0,
                    no_fat_chain: secondary[1] & 0x02 != 0,
                    name_length: secondary[3] as usize,
                    first_cluster: le_u32(&secondary[20..24]),
                    data_length: le_u64(&secondary[24..32]),
                });
            }
            EXFAT_ENTRY_TYPE_FILE_NAME => {
                for pair in secondary[2..32].chunks_exact(2) {
                    let code_unit = le_u16(pair);
                    if code_unit == 0x0000 || code_unit == 0xffff {
                        continue;
                    }
                    name_code_units.push(code_unit);
                }
            }
            _ => {}
        }
    }

    let stream = stream?;
    if name_code_units.len() > stream.name_length {
        name_code_units.truncate(stream.name_length);
    }

    let display_name = String::from_utf16_lossy(&name_code_units)
        .trim()
        .to_string();
    if display_name.is_empty() {
        return None;
    }

    Some(FileEntryRecord {
        deleted,
        directory,
        extension: extension_from_name(&display_name),
        display_name,
        created_at,
        modified_at,
        allocation_possible: stream.allocation_possible,
        no_fat_chain: stream.no_fat_chain,
        first_cluster: stream.first_cluster,
        data_length: stream.data_length,
    })
}

fn build_deleted_candidate(
    reader: &mut File,
    boot_sector: &ExfatBootSector,
    allocation_bitmap: &AllocationBitmap,
    current_path: &str,
    record: &FileEntryRecord,
) -> Result<ExfatDeletedFileCandidate, String> {
    let cluster_size = boot_sector.cluster_size_bytes();
    let required_clusters = required_clusters(record.data_length, cluster_size);

    let (clusters, reconstructable_size_bytes, integrity, recovery_score) =
        if required_clusters == 0 {
            (Vec::new(), 0, "intact".to_string(), 100)
        } else if !record.allocation_possible || record.first_cluster < 2 {
            return Err(format!(
                "The deleted exFAT entry `{}` does not expose a reconstructible allocation.",
                record.display_name
            ));
        } else if record.no_fat_chain {
            let contiguous = build_conservative_free_clusters(
                allocation_bitmap,
                boot_sector,
                record.first_cluster,
                required_clusters,
            )?;
            if contiguous.is_empty() {
                return Err(format!(
                    "No reliable free cluster range remains for the deleted exFAT entry `{}`.",
                    record.display_name
                ));
            }

            let reconstructable_size_bytes =
                (contiguous.len() as u64 * cluster_size).min(record.data_length);
            let integrity = if contiguous.len() < required_clusters {
                "partial"
            } else {
                "intact"
            };
            let score = if contiguous.len() < required_clusters {
                38
            } else {
                90
            };
            (
                contiguous,
                reconstructable_size_bytes,
                integrity.to_string(),
                score,
            )
        } else {
            match follow_deleted_file_chain(
                reader,
                boot_sector,
                allocation_bitmap,
                record.first_cluster,
                required_clusters,
            ) {
                Ok(clusters) => (clusters, record.data_length, "intact".to_string(), 84),
                Err(_) => {
                    let contiguous = build_conservative_free_clusters(
                        allocation_bitmap,
                        boot_sector,
                        record.first_cluster,
                        required_clusters,
                    )?;
                    if contiguous.is_empty() {
                        return Err(format!(
                        "No reliable free cluster range remains for the deleted exFAT entry `{}`.",
                        record.display_name
                    ));
                    }

                    let reconstructable_size_bytes =
                        (contiguous.len() as u64 * cluster_size).min(record.data_length);
                    let integrity = if contiguous.len() < required_clusters {
                        "partial"
                    } else if contiguous.len() == 1 {
                        "intact"
                    } else {
                        "fragmented"
                    };
                    let score = if contiguous.len() < required_clusters {
                        32
                    } else if contiguous.len() == 1 {
                        74
                    } else {
                        56
                    };
                    (
                        contiguous,
                        reconstructable_size_bytes,
                        integrity.to_string(),
                        score,
                    )
                }
            }
        };

    let byte_runs = byte_runs_from_clusters(&clusters, boot_sector)?;
    let (integrity, recovery_score) = if reconstructable_size_bytes == record.data_length
        && byte_runs.len() > 1
        && integrity == "intact"
    {
        ("fragmented".to_string(), recovery_score.min(78))
    } else {
        (integrity, recovery_score)
    };
    let start_offset = byte_runs.first().map(|run| run.offset).unwrap_or(0);

    Ok(ExfatDeletedFileCandidate {
        name: record.display_name.clone(),
        extension: record.extension.clone(),
        path: current_path.to_string(),
        size_bytes: reconstructable_size_bytes,
        expected_size_bytes: record.data_length,
        created_at: record.created_at.clone(),
        modified_at: record.modified_at.clone(),
        integrity,
        recovery_score,
        start_offset,
        clusters,
        byte_runs,
    })
}

fn build_visible_candidate(
    reader: &mut File,
    boot_sector: &ExfatBootSector,
    current_path: &str,
    record: &FileEntryRecord,
) -> Result<ExfatVisibleFileCandidate, String> {
    let cluster_size = boot_sector.cluster_size_bytes();
    let required_clusters = required_clusters(record.data_length, cluster_size);

    let clusters = if required_clusters == 0 {
        Vec::new()
    } else if !record.allocation_possible || record.first_cluster < 2 {
        return Err(format!(
            "The visible exFAT entry `{}` does not expose a usable allocation.",
            record.display_name
        ));
    } else if record.no_fat_chain {
        build_contiguous_cluster_range(record.first_cluster, required_clusters, boot_sector)?
    } else {
        let clusters = follow_cluster_chain(
            reader,
            boot_sector,
            record.first_cluster,
            Some(required_clusters),
        )?;
        if clusters.len() != required_clusters {
            return Err(format!(
                "The visible exFAT entry `{}` does not expose enough clusters for its advertised size.",
                record.display_name
            ));
        }
        clusters
    };

    let byte_runs = byte_runs_from_clusters(&clusters, boot_sector)?;
    let integrity = if byte_runs.len() > 1 {
        "fragmented"
    } else {
        "intact"
    };
    let recovery_score = if byte_runs.len() > 1 { 90 } else { 98 };

    Ok(ExfatVisibleFileCandidate {
        name: record.display_name.clone(),
        extension: record.extension.clone(),
        path: current_path.to_string(),
        size_bytes: record.data_length,
        created_at: record.created_at.clone(),
        modified_at: record.modified_at.clone(),
        integrity: integrity.into(),
        recovery_score,
        start_offset: byte_runs.first().map(|run| run.offset),
        clusters,
        byte_runs,
    })
}

fn follow_cluster_chain(
    reader: &mut File,
    boot_sector: &ExfatBootSector,
    start_cluster: u32,
    max_clusters: Option<usize>,
) -> Result<Vec<u32>, String> {
    validate_cluster(start_cluster, boot_sector.cluster_count)?;
    let mut seen = HashSet::new();
    let mut clusters = Vec::new();
    let mut current = start_cluster;

    loop {
        if !seen.insert(current) {
            return Err("An exFAT cluster loop was detected.".into());
        }

        clusters.push(current);
        if max_clusters == Some(clusters.len()) {
            break;
        }

        let next = read_fat_entry(reader, boot_sector, current)?;
        if is_end_of_chain(next) {
            break;
        }

        validate_cluster(next, boot_sector.cluster_count)?;
        current = next;
    }

    Ok(clusters)
}

fn follow_deleted_file_chain(
    reader: &mut File,
    boot_sector: &ExfatBootSector,
    allocation_bitmap: &AllocationBitmap,
    start_cluster: u32,
    required_clusters: usize,
) -> Result<Vec<u32>, String> {
    let clusters =
        follow_cluster_chain(reader, boot_sector, start_cluster, Some(required_clusters))?;
    if clusters.len() != required_clusters {
        return Err(
            "The exFAT FAT chain does not provide enough clusters for this deleted file.".into(),
        );
    }

    for cluster in &clusters {
        if !allocation_bitmap.cluster_is_free(*cluster)? {
            return Err(format!(
                "The exFAT cluster {cluster} is already allocated again."
            ));
        }
    }

    Ok(clusters)
}

fn build_contiguous_cluster_range(
    start_cluster: u32,
    required_clusters: usize,
    boot_sector: &ExfatBootSector,
) -> Result<Vec<u32>, String> {
    let mut clusters = Vec::with_capacity(required_clusters);
    for index in 0..required_clusters {
        let cluster = start_cluster.checked_add(index as u32).ok_or_else(|| {
            "Cluster range overflow while loading an exFAT directory.".to_string()
        })?;
        validate_cluster(cluster, boot_sector.cluster_count)?;
        clusters.push(cluster);
    }
    Ok(clusters)
}

fn build_conservative_free_clusters(
    allocation_bitmap: &AllocationBitmap,
    boot_sector: &ExfatBootSector,
    start_cluster: u32,
    required_clusters: usize,
) -> Result<Vec<u32>, String> {
    if required_clusters == 0 {
        return Ok(Vec::new());
    }

    let mut clusters = Vec::with_capacity(required_clusters);
    for index in 0..required_clusters {
        let cluster = start_cluster.checked_add(index as u32).ok_or_else(|| {
            "Cluster range overflow while reconstructing the deleted exFAT file.".to_string()
        })?;
        validate_cluster(cluster, boot_sector.cluster_count)?;
        if !allocation_bitmap.cluster_is_free(cluster)? {
            break;
        }
        clusters.push(cluster);
    }

    Ok(clusters)
}

fn read_clusters(
    reader: &mut File,
    boot_sector: &ExfatBootSector,
    clusters: &[u32],
    exact_length: Option<u64>,
) -> Result<Vec<u8>, String> {
    let mut buffer = Vec::with_capacity(clusters.len() * boot_sector.cluster_size_bytes() as usize);
    for cluster in clusters {
        let offset = boot_sector.cluster_offset(*cluster)?;
        let mut cluster_bytes = vec![0_u8; boot_sector.cluster_size_bytes() as usize];
        reader
            .seek(SeekFrom::Start(offset))
            .map_err(|error| format!("Unable to seek exFAT cluster {cluster}: {error}"))?;
        reader
            .read_exact(&mut cluster_bytes)
            .map_err(|error| format!("Unable to read exFAT cluster {cluster}: {error}"))?;
        buffer.extend_from_slice(&cluster_bytes);
    }

    if let Some(length) = exact_length {
        buffer.truncate(length as usize);
    }

    Ok(buffer)
}

fn read_fat_entry(
    reader: &mut File,
    boot_sector: &ExfatBootSector,
    cluster: u32,
) -> Result<u32, String> {
    let offset = boot_sector.fat_offset_bytes() + cluster as u64 * 4;
    let mut buffer = [0_u8; 4];
    reader.seek(SeekFrom::Start(offset)).map_err(|error| {
        format!("Unable to seek exFAT FAT entry for cluster {cluster}: {error}")
    })?;
    reader.read_exact(&mut buffer).map_err(|error| {
        format!("Unable to read exFAT FAT entry for cluster {cluster}: {error}")
    })?;
    Ok(le_u32(&buffer))
}

fn byte_runs_from_clusters(
    clusters: &[u32],
    boot_sector: &ExfatBootSector,
) -> Result<Vec<ByteRun>, String> {
    if clusters.is_empty() {
        return Ok(Vec::new());
    }

    let cluster_size = boot_sector.cluster_size_bytes();
    let mut runs = Vec::new();
    let mut run_start_cluster = clusters[0];
    let mut run_length_clusters = 1_u64;

    for window in clusters.windows(2) {
        if window[1] == window[0] + 1 {
            run_length_clusters = run_length_clusters.saturating_add(1);
            continue;
        }

        runs.push(ByteRun {
            offset: boot_sector.cluster_offset(run_start_cluster)?,
            length: run_length_clusters * cluster_size,
            zero_fill: false,
            ..Default::default()
        });
        run_start_cluster = window[1];
        run_length_clusters = 1;
    }

    runs.push(ByteRun {
        offset: boot_sector.cluster_offset(run_start_cluster)?,
        length: run_length_clusters * cluster_size,
        zero_fill: false,
        ..Default::default()
    });

    Ok(runs)
}

fn extension_from_name(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, extension)| extension.trim().to_lowercase())
        .filter(|extension| !extension.is_empty())
        .unwrap_or_default()
}

fn required_clusters(data_length: u64, cluster_size: u64) -> usize {
    if data_length == 0 {
        0
    } else {
        (((data_length - 1) / cluster_size) + 1) as usize
    }
}

fn validate_cluster(cluster: u32, cluster_count: u32) -> Result<(), String> {
    if cluster < 2 {
        return Err(format!("Invalid exFAT cluster {cluster}."));
    }

    let max_cluster = cluster_count.saturating_add(1);
    if cluster > max_cluster {
        return Err(format!(
            "exFAT cluster {cluster} is outside the available data region."
        ));
    }

    Ok(())
}

fn is_end_of_chain(value: u32) -> bool {
    value >= EXFAT_EOC_MIN
}

fn decode_exfat_timestamp(timestamp: u32) -> Option<String> {
    if timestamp == 0 {
        return None;
    }

    let date = (timestamp >> 16) as u16;
    let time = timestamp as u16;
    let day = (date & 0x1f) as u8;
    let month = ((date >> 5) & 0x0f) as u8;
    let year = 1980 + ((date >> 9) & 0x7f);
    let second = ((time & 0x1f) * 2) as u8;
    let minute = ((time >> 5) & 0x3f) as u8;
    let hour = ((time >> 11) & 0x1f) as u8;

    if !(1..=12).contains(&month)
        || !(1..=days_in_month(year, month)).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }

    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}"
    ))
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u16) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
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
    use std::{env, fs};

    fn write_test_image(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let root = env::temp_dir().join(format!("recupere-exfat-test-{}", std::process::id()));
        fs::create_dir_all(&root).expect("exfat test workspace should exist");
        let path = root.join(name);
        fs::write(&path, bytes).expect("exfat test image should be written");
        path
    }

    fn minimal_deleted_exfat_image() -> Vec<u8> {
        let mut image = vec![0_u8; 512 * 8];

        image[3..11].copy_from_slice(b"EXFAT   ");
        image[72..80].copy_from_slice(&8_u64.to_le_bytes());
        image[80..84].copy_from_slice(&1_u32.to_le_bytes());
        image[84..88].copy_from_slice(&1_u32.to_le_bytes());
        image[88..92].copy_from_slice(&2_u32.to_le_bytes());
        image[92..96].copy_from_slice(&6_u32.to_le_bytes());
        image[96..100].copy_from_slice(&2_u32.to_le_bytes());
        image[100..104].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
        image[104..106].copy_from_slice(&0x0100_u16.to_le_bytes());
        image[108] = 9;
        image[109] = 0;
        image[110] = 1;
        image[111] = 0x80;
        image[510] = 0x55;
        image[511] = 0xaa;

        let fat_offset = 512;
        image[fat_offset..fat_offset + 4].copy_from_slice(&0xffff_fff8_u32.to_le_bytes());
        image[fat_offset + 4..fat_offset + 8].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());
        image[fat_offset + 8..fat_offset + 12].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());
        image[fat_offset + 12..fat_offset + 16].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());

        let root_dir_offset = 1024;
        image[root_dir_offset] = 0x81;
        image[root_dir_offset + 20..root_dir_offset + 24].copy_from_slice(&3_u32.to_le_bytes());
        image[root_dir_offset + 24..root_dir_offset + 32].copy_from_slice(&1_u64.to_le_bytes());

        let deleted_file_offset = root_dir_offset + 32;
        image[deleted_file_offset] = 0x05;
        image[deleted_file_offset + 1] = 2;
        image[deleted_file_offset + 4..deleted_file_offset + 6]
            .copy_from_slice(&0x0020_u16.to_le_bytes());
        image[deleted_file_offset + 8..deleted_file_offset + 12]
            .copy_from_slice(&encode_timestamp(2024, 3, 18, 10, 22, 30).to_le_bytes());
        image[deleted_file_offset + 12..deleted_file_offset + 16]
            .copy_from_slice(&encode_timestamp(2024, 3, 19, 16, 45, 4).to_le_bytes());

        let stream_offset = deleted_file_offset + 32;
        image[stream_offset] = 0x40;
        image[stream_offset + 1] = 0x03;
        image[stream_offset + 3] = 10;
        image[stream_offset + 20..stream_offset + 24].copy_from_slice(&5_u32.to_le_bytes());
        image[stream_offset + 24..stream_offset + 32].copy_from_slice(&11_u64.to_le_bytes());

        let name_offset = stream_offset + 32;
        image[name_offset] = 0x41;
        write_name_entry(&mut image[name_offset..name_offset + 32], "Report.txt");
        let visible_file_offset = root_dir_offset + 128;
        image[visible_file_offset] = 0x85;
        image[visible_file_offset + 1] = 2;
        image[visible_file_offset + 4..visible_file_offset + 6]
            .copy_from_slice(&0x0020_u16.to_le_bytes());
        image[visible_file_offset + 8..visible_file_offset + 12]
            .copy_from_slice(&encode_timestamp(2024, 3, 17, 8, 0, 0).to_le_bytes());
        image[visible_file_offset + 12..visible_file_offset + 16]
            .copy_from_slice(&encode_timestamp(2024, 3, 17, 8, 5, 0).to_le_bytes());

        let visible_stream_offset = visible_file_offset + 32;
        image[visible_stream_offset] = 0xc0;
        image[visible_stream_offset + 1] = 0x03;
        image[visible_stream_offset + 3] = 8;
        image[visible_stream_offset + 20..visible_stream_offset + 24]
            .copy_from_slice(&4_u32.to_le_bytes());
        image[visible_stream_offset + 24..visible_stream_offset + 32]
            .copy_from_slice(&8_u64.to_le_bytes());

        let visible_name_offset = visible_stream_offset + 32;
        image[visible_name_offset] = 0xc1;
        write_name_entry(
            &mut image[visible_name_offset..visible_name_offset + 32],
            "Live.txt",
        );

        let bitmap_offset = 1536;
        image[bitmap_offset] = 0b0000_0111;

        let visible_data_offset = 2048;
        image[visible_data_offset..visible_data_offset + 8].copy_from_slice(b"exfat ok");
        let data_offset = 2560;
        image[data_offset..data_offset + 11].copy_from_slice(b"hello exfat");

        image
    }

    fn partially_overwritten_deleted_exfat_image() -> Vec<u8> {
        let mut image = minimal_deleted_exfat_image();
        let root_dir_offset = 1024;
        let deleted_file_offset = root_dir_offset + 32;
        let stream_offset = deleted_file_offset + 32;
        image[stream_offset + 3] = 10;
        image[stream_offset + 20..stream_offset + 24].copy_from_slice(&5_u32.to_le_bytes());
        image[stream_offset + 24..stream_offset + 32].copy_from_slice(&700_u64.to_le_bytes());
        image[1536] = 0b0001_0011;
        image
    }

    fn fragmented_deleted_exfat_image() -> Vec<u8> {
        let mut image = minimal_deleted_exfat_image();
        let fat_offset = 512;
        image[fat_offset + 20..fat_offset + 24].copy_from_slice(&7_u32.to_le_bytes());
        image[fat_offset + 28..fat_offset + 32].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());

        let root_dir_offset = 1024;
        let deleted_file_offset = root_dir_offset + 32;
        let stream_offset = deleted_file_offset + 32;
        image[stream_offset + 1] = 0x01;
        image[stream_offset + 3] = 10;
        image[stream_offset + 20..stream_offset + 24].copy_from_slice(&5_u32.to_le_bytes());
        image[stream_offset + 24..stream_offset + 32].copy_from_slice(&700_u64.to_le_bytes());

        image[1536] = 0b0001_0111;
        image[2560..2560 + 512].fill(b'A');
        image[3584..3584 + 188].fill(b'B');

        image
    }

    fn write_name_entry(entry: &mut [u8], name: &str) {
        for (index, code_unit) in name.encode_utf16().take(15).enumerate() {
            let offset = 2 + index * 2;
            entry[offset..offset + 2].copy_from_slice(&code_unit.to_le_bytes());
        }
    }

    fn encode_timestamp(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> u32 {
        let date = ((year - 1980) << 9) | ((month as u16) << 5) | day as u16;
        let time = ((hour as u16) << 11) | ((minute as u16) << 5) | ((second / 2) as u16);
        ((date as u32) << 16) | time as u32
    }

    #[test]
    fn recover_deleted_files_reads_a_deleted_exfat_entry() {
        let image_path = write_test_image("deleted-exfat.img", &minimal_deleted_exfat_image());
        let deleted =
            recover_deleted_files(&image_path).expect("deleted exfat files should be parsed");

        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].name, "Report.txt");
        assert_eq!(deleted[0].extension, "txt");
        assert_eq!(deleted[0].path, "/");
        assert_eq!(deleted[0].size_bytes, 11);
        assert_eq!(deleted[0].expected_size_bytes, 11);
        assert_eq!(
            deleted[0].created_at.as_deref(),
            Some("2024-03-18T10:22:30")
        );
        assert_eq!(
            deleted[0].modified_at.as_deref(),
            Some("2024-03-19T16:45:04")
        );
        assert_eq!(deleted[0].integrity, "intact");
        assert_eq!(deleted[0].clusters, vec![5]);
        assert_eq!(deleted[0].byte_runs.len(), 1);
        assert_eq!(deleted[0].start_offset, 2560);
    }

    #[test]
    fn recover_deleted_files_marks_partially_reconstructible_exfat_ranges() {
        let image_path = write_test_image(
            "deleted-exfat-partial.img",
            &partially_overwritten_deleted_exfat_image(),
        );
        let deleted =
            recover_deleted_files(&image_path).expect("partial exfat deletions should be parsed");

        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].name, "Report.txt");
        assert_eq!(deleted[0].expected_size_bytes, 700);
        assert_eq!(deleted[0].size_bytes, 512);
        assert_eq!(deleted[0].integrity, "partial");
        assert_eq!(deleted[0].clusters, vec![5]);
        assert_eq!(deleted[0].byte_runs.len(), 1);
    }

    #[test]
    fn recover_deleted_files_marks_fragmented_fat_chain_results() {
        let image_path = write_test_image(
            "deleted-exfat-fragmented.img",
            &fragmented_deleted_exfat_image(),
        );
        let deleted =
            recover_deleted_files(&image_path).expect("fragmented exfat deletions should parse");

        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].name, "Report.txt");
        assert_eq!(deleted[0].size_bytes, 700);
        assert_eq!(deleted[0].expected_size_bytes, 700);
        assert_eq!(deleted[0].integrity, "fragmented");
        assert_eq!(deleted[0].clusters, vec![5, 7]);
        assert_eq!(deleted[0].byte_runs.len(), 2);
    }

    #[test]
    fn list_visible_files_reads_a_visible_exfat_entry() {
        let image_path = write_test_image("visible-exfat.img", &minimal_deleted_exfat_image());
        let visible =
            list_visible_files(&image_path).expect("visible exFAT files should be parsed");

        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "Live.txt");
        assert_eq!(visible[0].extension, "txt");
        assert_eq!(visible[0].path, "/");
        assert_eq!(visible[0].size_bytes, 8);
        assert_eq!(
            visible[0].created_at.as_deref(),
            Some("2024-03-17T08:00:00")
        );
        assert_eq!(
            visible[0].modified_at.as_deref(),
            Some("2024-03-17T08:05:00")
        );
        assert_eq!(visible[0].integrity, "intact");
        assert_eq!(visible[0].clusters, vec![4]);
        assert_eq!(visible[0].start_offset, Some(2048));
        assert_eq!(visible[0].byte_runs.len(), 1);
    }
}
