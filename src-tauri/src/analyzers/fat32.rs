use crate::types::ByteRun;
use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_LONG_NAME: u8 = 0x0f;
const ATTR_VOLUME_ID: u8 = 0x08;
const FAT32_EOC_MIN: u32 = 0x0fff_fff8;

#[derive(Debug, Clone)]
pub struct Fat32DeletedFileCandidate {
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
pub struct Fat32VisibleFileCandidate {
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
struct Fat32BootSector {
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sector_count: u16,
    fat_count: u8,
    total_sectors: u32,
    fat_size_sectors: u32,
    root_cluster: u32,
}

impl Fat32BootSector {
    fn read_from(reader: &mut File) -> Result<Self, String> {
        let mut sector = [0_u8; 512];
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|error| format!("Unable to seek the FAT32 boot sector: {error}"))?;
        reader
            .read_exact(&mut sector)
            .map_err(|error| format!("Unable to read the FAT32 boot sector: {error}"))?;

        if sector[510] != 0x55 || sector[511] != 0xaa {
            return Err("The image does not expose a valid FAT boot signature.".into());
        }

        let bytes_per_sector = le_u16(&sector[11..13]);
        let sectors_per_cluster = sector[13];
        let reserved_sector_count = le_u16(&sector[14..16]);
        let fat_count = sector[16];
        let total_sectors_16 = le_u16(&sector[19..21]) as u32;
        let total_sectors_32 = le_u32(&sector[32..36]);
        let fat_size_16 = le_u16(&sector[22..24]) as u32;
        let fat_size_sectors = if fat_size_16 > 0 {
            fat_size_16
        } else {
            le_u32(&sector[36..40])
        };
        let root_cluster = le_u32(&sector[44..48]);

        let total_sectors = if total_sectors_16 > 0 {
            total_sectors_16
        } else {
            total_sectors_32
        };

        if bytes_per_sector == 0
            || sectors_per_cluster == 0
            || fat_size_sectors == 0
            || root_cluster < 2
        {
            return Err("The image does not contain a usable FAT32 layout.".into());
        }

        Ok(Self {
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sector_count,
            fat_count,
            total_sectors,
            fat_size_sectors,
            root_cluster,
        })
    }

    fn cluster_size_bytes(&self) -> u64 {
        self.bytes_per_sector as u64 * self.sectors_per_cluster as u64
    }

    fn fat_offset_bytes(&self) -> u64 {
        self.reserved_sector_count as u64 * self.bytes_per_sector as u64
    }

    fn data_region_offset_bytes(&self) -> u64 {
        (self.reserved_sector_count as u64 + self.fat_count as u64 * self.fat_size_sectors as u64)
            * self.bytes_per_sector as u64
    }

    fn total_clusters(&self) -> u32 {
        let first_data_sector =
            self.reserved_sector_count as u32 + self.fat_count as u32 * self.fat_size_sectors;
        let data_sectors = self.total_sectors.saturating_sub(first_data_sector);
        let per_cluster = self.sectors_per_cluster.max(1) as u32;
        data_sectors / per_cluster
    }

    fn cluster_offset(&self, cluster: u32) -> Result<u64, String> {
        validate_cluster(cluster, self.total_clusters())?;
        Ok(self.data_region_offset_bytes() + (cluster as u64 - 2) * self.cluster_size_bytes())
    }
}

#[derive(Debug, Clone)]
struct DirectoryEntry {
    deleted: bool,
    directory: bool,
    display_name: String,
    extension: String,
    start_cluster: u32,
    size_bytes: u64,
    created_at: Option<String>,
    modified_at: Option<String>,
}

pub fn recover_deleted_files(image_path: &Path) -> Result<Vec<Fat32DeletedFileCandidate>, String> {
    let mut reader = File::open(image_path).map_err(|error| {
        format!(
            "Unable to open the FAT32 image {}: {}",
            image_path.to_string_lossy(),
            error
        )
    })?;
    let boot_sector = Fat32BootSector::read_from(&mut reader)?;
    let mut deleted_files = Vec::new();
    let mut visited_directories = HashSet::new();

    scan_directory(
        &mut reader,
        &boot_sector,
        boot_sector.root_cluster,
        "/",
        &mut visited_directories,
        &mut deleted_files,
    )?;

    Ok(deleted_files)
}

pub fn list_visible_files(image_path: &Path) -> Result<Vec<Fat32VisibleFileCandidate>, String> {
    let mut reader = File::open(image_path).map_err(|error| {
        format!(
            "Unable to open the FAT32 image {}: {}",
            image_path.to_string_lossy(),
            error
        )
    })?;
    let boot_sector = Fat32BootSector::read_from(&mut reader)?;
    let mut visible_files = Vec::new();
    let mut visited_directories = HashSet::new();

    scan_visible_directory(
        &mut reader,
        &boot_sector,
        boot_sector.root_cluster,
        "/",
        &mut visited_directories,
        &mut visible_files,
    )?;

    Ok(visible_files)
}

fn scan_directory(
    reader: &mut File,
    boot_sector: &Fat32BootSector,
    start_cluster: u32,
    current_path: &str,
    visited_directories: &mut HashSet<u32>,
    deleted_files: &mut Vec<Fat32DeletedFileCandidate>,
) -> Result<(), String> {
    if !visited_directories.insert(start_cluster) {
        return Ok(());
    }

    let directory_clusters = follow_cluster_chain(reader, boot_sector, start_cluster, None)?;
    let mut directory_bytes = Vec::new();
    for cluster in &directory_clusters {
        directory_bytes.extend_from_slice(&read_cluster(reader, boot_sector, *cluster)?);
    }

    let mut pending_long_name_entries: Vec<LongNameEntry> = Vec::new();

    for chunk in directory_bytes.chunks_exact(32) {
        let first = chunk.first().copied().unwrap_or(0);
        if first == 0x00 {
            break;
        }

        let attributes = chunk[11];
        if attributes == ATTR_LONG_NAME {
            if let Some(long_name_entry) = parse_long_name_entry(chunk) {
                pending_long_name_entries.push(long_name_entry);
            }
            continue;
        }

        let long_name = build_long_name(&pending_long_name_entries);
        pending_long_name_entries.clear();

        let entry = match parse_directory_entry(chunk, long_name) {
            Some(entry) => entry,
            None => continue,
        };

        if entry.deleted && !entry.directory && (entry.size_bytes == 0 || entry.start_cluster >= 2)
        {
            deleted_files.push(build_deleted_candidate(
                reader,
                boot_sector,
                current_path,
                &entry,
            )?);
            continue;
        }

        if !entry.deleted && entry.directory && entry.start_cluster >= 2 {
            let next_path = if current_path == "/" {
                format!("/{}", entry.display_name)
            } else {
                format!("{current_path}/{}", entry.display_name)
            };
            scan_directory(
                reader,
                boot_sector,
                entry.start_cluster,
                &next_path,
                visited_directories,
                deleted_files,
            )?;
        }
    }

    Ok(())
}

fn scan_visible_directory(
    reader: &mut File,
    boot_sector: &Fat32BootSector,
    start_cluster: u32,
    current_path: &str,
    visited_directories: &mut HashSet<u32>,
    visible_files: &mut Vec<Fat32VisibleFileCandidate>,
) -> Result<(), String> {
    if !visited_directories.insert(start_cluster) {
        return Ok(());
    }

    let directory_clusters = follow_cluster_chain(reader, boot_sector, start_cluster, None)?;
    let mut directory_bytes = Vec::new();
    for cluster in &directory_clusters {
        directory_bytes.extend_from_slice(&read_cluster(reader, boot_sector, *cluster)?);
    }

    let mut pending_long_name_entries: Vec<LongNameEntry> = Vec::new();

    for chunk in directory_bytes.chunks_exact(32) {
        let first = chunk.first().copied().unwrap_or(0);
        if first == 0x00 {
            break;
        }

        let attributes = chunk[11];
        if attributes == ATTR_LONG_NAME {
            if let Some(long_name_entry) = parse_long_name_entry(chunk) {
                pending_long_name_entries.push(long_name_entry);
            }
            continue;
        }

        let long_name = build_long_name(&pending_long_name_entries);
        pending_long_name_entries.clear();

        let entry = match parse_directory_entry(chunk, long_name) {
            Some(entry) => entry,
            None => continue,
        };

        if !entry.deleted && !entry.directory {
            visible_files.push(build_visible_candidate(
                reader,
                boot_sector,
                current_path,
                &entry,
            )?);
            continue;
        }

        if !entry.deleted && entry.directory && entry.start_cluster >= 2 {
            let next_path = if current_path == "/" {
                format!("/{}", entry.display_name)
            } else {
                format!("{current_path}/{}", entry.display_name)
            };
            scan_visible_directory(
                reader,
                boot_sector,
                entry.start_cluster,
                &next_path,
                visited_directories,
                visible_files,
            )?;
        }
    }

    Ok(())
}

fn build_deleted_candidate(
    reader: &mut File,
    boot_sector: &Fat32BootSector,
    current_path: &str,
    entry: &DirectoryEntry,
) -> Result<Fat32DeletedFileCandidate, String> {
    let required_clusters = if entry.size_bytes == 0 {
        0
    } else {
        ((entry.size_bytes - 1) / boot_sector.cluster_size_bytes()) + 1
    };
    let cluster_size = boot_sector.cluster_size_bytes();

    let (clusters, reconstructable_size_bytes, integrity, recovery_score) =
        if required_clusters == 0 {
            (Vec::new(), 0, "intact".to_string(), 100)
        } else {
            match follow_deleted_file_chain(
                reader,
                boot_sector,
                entry.start_cluster,
                required_clusters as usize,
            ) {
                Ok(clusters) => (clusters, entry.size_bytes, "intact".to_string(), 88),
                Err(_) => {
                    let contiguous = build_conservative_free_clusters(
                        reader,
                        boot_sector,
                        entry.start_cluster,
                        required_clusters as usize,
                    )?;
                    if contiguous.is_empty() {
                        return Err(
                            "No reliable free cluster range remains for this deleted FAT32 entry."
                                .into(),
                        );
                    }

                    let reconstructable_size_bytes =
                        (contiguous.len() as u64 * cluster_size).min(entry.size_bytes);
                    let integrity = if contiguous.len() < required_clusters as usize {
                        "partial"
                    } else if contiguous.len() == 1 {
                        "intact"
                    } else {
                        "fragmented"
                    };
                    let score = if contiguous.len() < required_clusters as usize {
                        34
                    } else if contiguous.len() == 1 {
                        84
                    } else {
                        61
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
    let start_offset = byte_runs.first().map(|run| run.offset).unwrap_or(0);

    Ok(Fat32DeletedFileCandidate {
        name: entry.display_name.clone(),
        extension: entry.extension.to_lowercase(),
        path: current_path.to_string(),
        size_bytes: reconstructable_size_bytes,
        expected_size_bytes: entry.size_bytes,
        created_at: entry.created_at.clone(),
        modified_at: entry.modified_at.clone(),
        integrity,
        recovery_score,
        start_offset,
        clusters,
        byte_runs,
    })
}

fn build_visible_candidate(
    reader: &mut File,
    boot_sector: &Fat32BootSector,
    current_path: &str,
    entry: &DirectoryEntry,
) -> Result<Fat32VisibleFileCandidate, String> {
    let required_clusters = if entry.size_bytes == 0 {
        0
    } else {
        ((entry.size_bytes - 1) / boot_sector.cluster_size_bytes()) + 1
    };

    let clusters = if required_clusters == 0 {
        Vec::new()
    } else {
        if entry.start_cluster < 2 {
            return Err(format!(
                "The visible FAT32 entry `{}` does not expose a usable start cluster.",
                entry.display_name
            ));
        }

        let clusters = follow_cluster_chain(
            reader,
            boot_sector,
            entry.start_cluster,
            Some(required_clusters as usize),
        )?;
        if clusters.len() != required_clusters as usize {
            return Err(format!(
                "The visible FAT32 entry `{}` does not expose enough clusters for its advertised size.",
                entry.display_name
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

    Ok(Fat32VisibleFileCandidate {
        name: entry.display_name.clone(),
        extension: entry.extension.to_lowercase(),
        path: current_path.to_string(),
        size_bytes: entry.size_bytes,
        created_at: entry.created_at.clone(),
        modified_at: entry.modified_at.clone(),
        integrity: integrity.into(),
        recovery_score,
        start_offset: byte_runs.first().map(|run| run.offset),
        clusters,
        byte_runs,
    })
}

#[derive(Debug, Clone)]
struct LongNameEntry {
    fragment: String,
}

fn parse_directory_entry(chunk: &[u8], long_name: Option<String>) -> Option<DirectoryEntry> {
    let first = *chunk.first()?;
    if first == 0x00 {
        return None;
    }

    let attributes = chunk[11];
    if attributes & ATTR_VOLUME_ID != 0 {
        return None;
    }

    let deleted = first == 0xe5;
    let directory = attributes & ATTR_DIRECTORY != 0;
    let (short_name, short_extension) = decode_short_name(chunk, deleted);
    let display_name = display_name_from_entries(long_name, &short_name, &short_extension);
    let extension = extension_from_display_name(&display_name, &short_extension);

    if short_name.is_empty() || short_name == "." || short_name == ".." {
        return None;
    }

    let start_cluster = ((le_u16(&chunk[20..22]) as u32) << 16) | le_u16(&chunk[26..28]) as u32;
    let size_bytes = le_u32(&chunk[28..32]) as u64;
    let created_at = decode_fat_datetime(le_u16(&chunk[16..18]), le_u16(&chunk[14..16]));
    let modified_at = decode_fat_datetime(le_u16(&chunk[24..26]), le_u16(&chunk[22..24]));

    Some(DirectoryEntry {
        deleted,
        directory,
        display_name,
        extension,
        start_cluster,
        size_bytes,
        created_at,
        modified_at,
    })
}

fn parse_long_name_entry(chunk: &[u8]) -> Option<LongNameEntry> {
    let fragment = decode_long_name_fragment(chunk);
    if fragment.is_empty() {
        return None;
    }

    Some(LongNameEntry { fragment })
}

fn build_long_name(entries: &[LongNameEntry]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }

    let long_name = entries
        .iter()
        .rev()
        .map(|entry| entry.fragment.as_str())
        .collect::<String>()
        .trim()
        .to_string();

    if long_name.is_empty() {
        None
    } else {
        Some(long_name)
    }
}

fn decode_long_name_fragment(chunk: &[u8]) -> String {
    let mut code_units = Vec::with_capacity(13);
    decode_long_name_code_units(&chunk[1..11], &mut code_units);
    decode_long_name_code_units(&chunk[14..26], &mut code_units);
    decode_long_name_code_units(&chunk[28..32], &mut code_units);
    String::from_utf16_lossy(&code_units)
}

fn decode_long_name_code_units(bytes: &[u8], code_units: &mut Vec<u16>) {
    for pair in bytes.chunks_exact(2) {
        let code_unit = le_u16(pair);
        if code_unit == 0x0000 || code_unit == 0xffff {
            continue;
        }
        code_units.push(code_unit);
    }
}

fn display_name_from_entries(
    long_name: Option<String>,
    short_name: &str,
    extension: &str,
) -> String {
    if let Some(long_name) = long_name {
        let trimmed = long_name.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    if extension.is_empty() {
        short_name.to_string()
    } else {
        format!("{short_name}.{extension}")
    }
}

fn extension_from_display_name(display_name: &str, fallback_extension: &str) -> String {
    display_name
        .rsplit_once('.')
        .map(|(_, extension)| extension.trim().to_string())
        .filter(|extension| !extension.is_empty())
        .unwrap_or_else(|| fallback_extension.to_string())
}

fn decode_short_name(chunk: &[u8], deleted: bool) -> (String, String) {
    let mut raw_name = chunk[0..8].to_vec();
    if deleted && !raw_name.is_empty() {
        raw_name[0] = b'_';
    }

    let name = decode_name_component(&raw_name);
    let extension = decode_name_component(&chunk[8..11]);
    (name, extension)
}

fn decode_name_component(bytes: &[u8]) -> String {
    let decoded = bytes
        .iter()
        .map(|value| match value {
            b' '..=b'~' => *value as char,
            _ => '_',
        })
        .collect::<String>();

    decoded.trim().to_string()
}

fn follow_cluster_chain(
    reader: &mut File,
    boot_sector: &Fat32BootSector,
    start_cluster: u32,
    max_clusters: Option<usize>,
) -> Result<Vec<u32>, String> {
    validate_cluster(start_cluster, boot_sector.total_clusters())?;
    let mut seen = HashSet::new();
    let mut clusters = Vec::new();
    let mut current = start_cluster;

    loop {
        if !seen.insert(current) {
            return Err("A FAT32 cluster loop was detected.".into());
        }

        clusters.push(current);
        if max_clusters == Some(clusters.len()) {
            break;
        }

        let next = read_fat_entry(reader, boot_sector, current)?;
        if is_end_of_chain(next) {
            break;
        }

        validate_cluster(next, boot_sector.total_clusters())?;
        current = next;
    }

    Ok(clusters)
}

fn follow_deleted_file_chain(
    reader: &mut File,
    boot_sector: &Fat32BootSector,
    start_cluster: u32,
    required_clusters: usize,
) -> Result<Vec<u32>, String> {
    let clusters =
        follow_cluster_chain(reader, boot_sector, start_cluster, Some(required_clusters))?;
    if clusters.len() == required_clusters {
        Ok(clusters)
    } else {
        Err("The FAT chain does not provide enough clusters for this deleted file.".into())
    }
}

fn build_conservative_free_clusters(
    reader: &mut File,
    boot_sector: &Fat32BootSector,
    start_cluster: u32,
    required_clusters: usize,
) -> Result<Vec<u32>, String> {
    if required_clusters == 0 {
        return Ok(Vec::new());
    }

    let mut clusters = Vec::with_capacity(required_clusters);
    for index in 0..required_clusters {
        let cluster = start_cluster.checked_add(index as u32).ok_or_else(|| {
            "Cluster range overflow while reconstructing the deleted file.".to_string()
        })?;
        validate_cluster(cluster, boot_sector.total_clusters())?;

        let fat_value = read_fat_entry(reader, boot_sector, cluster)?;
        if fat_value != 0 {
            break;
        }

        clusters.push(cluster);
    }

    Ok(clusters)
}

fn byte_runs_from_clusters(
    clusters: &[u32],
    boot_sector: &Fat32BootSector,
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

fn read_cluster(
    reader: &mut File,
    boot_sector: &Fat32BootSector,
    cluster: u32,
) -> Result<Vec<u8>, String> {
    let offset = boot_sector.cluster_offset(cluster)?;
    let mut buffer = vec![0_u8; boot_sector.cluster_size_bytes() as usize];
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|error| format!("Unable to seek cluster {cluster}: {error}"))?;
    reader
        .read_exact(&mut buffer)
        .map_err(|error| format!("Unable to read cluster {cluster}: {error}"))?;
    Ok(buffer)
}

fn read_fat_entry(
    reader: &mut File,
    boot_sector: &Fat32BootSector,
    cluster: u32,
) -> Result<u32, String> {
    let offset = boot_sector.fat_offset_bytes() + cluster as u64 * 4;
    let mut buffer = [0_u8; 4];
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|error| format!("Unable to seek FAT entry for cluster {cluster}: {error}"))?;
    reader
        .read_exact(&mut buffer)
        .map_err(|error| format!("Unable to read FAT entry for cluster {cluster}: {error}"))?;
    Ok(le_u32(&buffer) & 0x0fff_ffff)
}

fn validate_cluster(cluster: u32, total_clusters: u32) -> Result<(), String> {
    if cluster < 2 {
        return Err(format!("Invalid FAT32 cluster {cluster}."));
    }

    let max_cluster = total_clusters.saturating_add(1);
    if cluster > max_cluster {
        return Err(format!(
            "FAT32 cluster {cluster} is outside the available data region."
        ));
    }

    Ok(())
}

fn is_end_of_chain(value: u32) -> bool {
    value >= FAT32_EOC_MIN
}

fn le_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn decode_fat_datetime(date: u16, time: u16) -> Option<String> {
    if date == 0 {
        return None;
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs};

    fn write_test_image(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let root = env::temp_dir().join(format!("recupere-fat32-test-{}", std::process::id()));
        fs::create_dir_all(&root).expect("fat32 test workspace should exist");
        let path = root.join(name);
        fs::write(&path, bytes).expect("fat32 test image should be written");
        path
    }

    fn minimal_deleted_fat32_image() -> Vec<u8> {
        let mut image = vec![0_u8; 512 * 6];

        image[11..13].copy_from_slice(&512_u16.to_le_bytes());
        image[13] = 1;
        image[14..16].copy_from_slice(&1_u16.to_le_bytes());
        image[16] = 1;
        image[32..36].copy_from_slice(&6_u32.to_le_bytes());
        image[36..40].copy_from_slice(&1_u32.to_le_bytes());
        image[44..48].copy_from_slice(&2_u32.to_le_bytes());
        image[82..90].copy_from_slice(b"FAT32   ");
        image[510] = 0x55;
        image[511] = 0xaa;

        let fat_offset = 512;
        image[fat_offset..fat_offset + 4].copy_from_slice(&0x0fff_fff8_u32.to_le_bytes());
        image[fat_offset + 4..fat_offset + 8].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());
        image[fat_offset + 8..fat_offset + 12].copy_from_slice(&0x0fff_ffff_u32.to_le_bytes());
        image[fat_offset + 12..fat_offset + 16].copy_from_slice(&0x0fff_ffff_u32.to_le_bytes());
        image[fat_offset + 16..fat_offset + 20].copy_from_slice(&0x0fff_ffff_u32.to_le_bytes());

        let root_dir_offset = 1024;
        image[root_dir_offset] = 0xe5;
        image[root_dir_offset + 1..root_dir_offset + 8].copy_from_slice(b"EPORT  ");
        image[root_dir_offset + 8..root_dir_offset + 11].copy_from_slice(b"TXT");
        image[root_dir_offset + 11] = 0x20;
        image[root_dir_offset + 14..root_dir_offset + 16]
            .copy_from_slice(&encode_fat_time(9, 26, 12).to_le_bytes());
        image[root_dir_offset + 16..root_dir_offset + 18]
            .copy_from_slice(&encode_fat_date(2024, 3, 14).to_le_bytes());
        image[root_dir_offset + 22..root_dir_offset + 24]
            .copy_from_slice(&encode_fat_time(16, 8, 0).to_le_bytes());
        image[root_dir_offset + 24..root_dir_offset + 26]
            .copy_from_slice(&encode_fat_date(2024, 3, 15).to_le_bytes());
        image[root_dir_offset + 26..root_dir_offset + 28].copy_from_slice(&3_u16.to_le_bytes());
        image[root_dir_offset + 28..root_dir_offset + 32].copy_from_slice(&11_u32.to_le_bytes());
        let visible_offset = root_dir_offset + 32;
        image[visible_offset..visible_offset + 8].copy_from_slice(b"LIVELOG ");
        image[visible_offset + 8..visible_offset + 11].copy_from_slice(b"TXT");
        image[visible_offset + 11] = 0x20;
        image[visible_offset + 14..visible_offset + 16]
            .copy_from_slice(&encode_fat_time(8, 10, 0).to_le_bytes());
        image[visible_offset + 16..visible_offset + 18]
            .copy_from_slice(&encode_fat_date(2024, 3, 13).to_le_bytes());
        image[visible_offset + 22..visible_offset + 24]
            .copy_from_slice(&encode_fat_time(8, 12, 0).to_le_bytes());
        image[visible_offset + 24..visible_offset + 26]
            .copy_from_slice(&encode_fat_date(2024, 3, 13).to_le_bytes());
        image[visible_offset + 26..visible_offset + 28].copy_from_slice(&4_u16.to_le_bytes());
        image[visible_offset + 28..visible_offset + 32].copy_from_slice(&9_u32.to_le_bytes());

        let data_offset = 1536;
        image[data_offset..data_offset + 11].copy_from_slice(b"hello world");
        let visible_data_offset = 2048;
        image[visible_data_offset..visible_data_offset + 9].copy_from_slice(b"live log!");

        image
    }

    fn deleted_fat32_image_with_long_name(long_name: &str) -> Vec<u8> {
        let mut image = vec![0_u8; 512 * 6];

        image[11..13].copy_from_slice(&512_u16.to_le_bytes());
        image[13] = 1;
        image[14..16].copy_from_slice(&1_u16.to_le_bytes());
        image[16] = 1;
        image[32..36].copy_from_slice(&6_u32.to_le_bytes());
        image[36..40].copy_from_slice(&1_u32.to_le_bytes());
        image[44..48].copy_from_slice(&2_u32.to_le_bytes());
        image[82..90].copy_from_slice(b"FAT32   ");
        image[510] = 0x55;
        image[511] = 0xaa;

        let fat_offset = 512;
        image[fat_offset..fat_offset + 4].copy_from_slice(&0x0fff_fff8_u32.to_le_bytes());
        image[fat_offset + 4..fat_offset + 8].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());
        image[fat_offset + 8..fat_offset + 12].copy_from_slice(&0x0fff_ffff_u32.to_le_bytes());
        image[fat_offset + 12..fat_offset + 16].copy_from_slice(&0_u32.to_le_bytes());

        let root_dir_offset = 1024;
        let lfn_entries = build_deleted_long_name_entries(long_name);
        for (index, entry) in lfn_entries.iter().enumerate() {
            let offset = root_dir_offset + index * 32;
            image[offset..offset + 32].copy_from_slice(entry);
        }

        let short_offset = root_dir_offset + lfn_entries.len() * 32;
        image[short_offset] = 0xe5;
        image[short_offset + 1..short_offset + 8].copy_from_slice(b"UARTER~");
        image[short_offset + 8..short_offset + 11].copy_from_slice(b"TXT");
        image[short_offset + 11] = 0x20;
        image[short_offset + 26..short_offset + 28].copy_from_slice(&3_u16.to_le_bytes());
        image[short_offset + 28..short_offset + 32].copy_from_slice(&11_u32.to_le_bytes());
        image[short_offset + 32] = 0x00;

        let data_offset = 1536;
        image[data_offset..data_offset + 11].copy_from_slice(b"hello world");

        image
    }

    fn partially_overwritten_deleted_fat32_image() -> Vec<u8> {
        let mut image = vec![0_u8; 512 * 8];

        image[11..13].copy_from_slice(&512_u16.to_le_bytes());
        image[13] = 1;
        image[14..16].copy_from_slice(&1_u16.to_le_bytes());
        image[16] = 1;
        image[32..36].copy_from_slice(&8_u32.to_le_bytes());
        image[36..40].copy_from_slice(&1_u32.to_le_bytes());
        image[44..48].copy_from_slice(&2_u32.to_le_bytes());
        image[82..90].copy_from_slice(b"FAT32   ");
        image[510] = 0x55;
        image[511] = 0xaa;

        let fat_offset = 512;
        image[fat_offset..fat_offset + 4].copy_from_slice(&0x0fff_fff8_u32.to_le_bytes());
        image[fat_offset + 4..fat_offset + 8].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());
        image[fat_offset + 8..fat_offset + 12].copy_from_slice(&0x0fff_ffff_u32.to_le_bytes());
        image[fat_offset + 12..fat_offset + 16].copy_from_slice(&0_u32.to_le_bytes());
        image[fat_offset + 16..fat_offset + 20].copy_from_slice(&7_u32.to_le_bytes());
        image[fat_offset + 20..fat_offset + 24].copy_from_slice(&0_u32.to_le_bytes());

        let root_dir_offset = 1024;
        image[root_dir_offset] = 0xe5;
        image[root_dir_offset + 1..root_dir_offset + 8].copy_from_slice(b"IDEOPA ");
        image[root_dir_offset + 8..root_dir_offset + 11].copy_from_slice(b"BIN");
        image[root_dir_offset + 11] = 0x20;
        image[root_dir_offset + 14..root_dir_offset + 16]
            .copy_from_slice(&encode_fat_time(7, 4, 0).to_le_bytes());
        image[root_dir_offset + 16..root_dir_offset + 18]
            .copy_from_slice(&encode_fat_date(2021, 12, 1).to_le_bytes());
        image[root_dir_offset + 22..root_dir_offset + 24]
            .copy_from_slice(&encode_fat_time(8, 2, 0).to_le_bytes());
        image[root_dir_offset + 24..root_dir_offset + 26]
            .copy_from_slice(&encode_fat_date(2022, 1, 10).to_le_bytes());
        image[root_dir_offset + 26..root_dir_offset + 28].copy_from_slice(&3_u16.to_le_bytes());
        image[root_dir_offset + 28..root_dir_offset + 32].copy_from_slice(&1200_u32.to_le_bytes());

        let data_offset = 1536;
        image[data_offset..data_offset + 512].fill(0x41);

        image
    }

    fn build_deleted_long_name_entries(long_name: &str) -> Vec<[u8; 32]> {
        let mut code_units: Vec<u16> = long_name.encode_utf16().collect();
        code_units.push(0x0000);
        while !code_units.len().is_multiple_of(13) {
            code_units.push(0xffff);
        }

        let chunk_count = code_units.len() / 13;
        let mut entries = Vec::with_capacity(chunk_count);

        for physical_index in 0..chunk_count {
            let logical_index = chunk_count - physical_index - 1;
            let chunk = &code_units[logical_index * 13..(logical_index + 1) * 13];
            let mut entry = [0_u8; 32];
            entry[0] = 0xe5;
            entry[11] = ATTR_LONG_NAME;
            entry[12] = 0;
            entry[13] = 0;
            entry[26..28].copy_from_slice(&0_u16.to_le_bytes());

            write_utf16_slice(&mut entry[1..11], &chunk[0..5]);
            write_utf16_slice(&mut entry[14..26], &chunk[5..11]);
            write_utf16_slice(&mut entry[28..32], &chunk[11..13]);
            entries.push(entry);
        }

        entries
    }

    fn write_utf16_slice(target: &mut [u8], code_units: &[u16]) {
        for (chunk, code_unit) in target.chunks_exact_mut(2).zip(code_units.iter()) {
            chunk.copy_from_slice(&code_unit.to_le_bytes());
        }
    }

    fn encode_fat_date(year: u16, month: u8, day: u8) -> u16 {
        ((year.saturating_sub(1980)) << 9) | ((month as u16) << 5) | day as u16
    }

    fn encode_fat_time(hour: u8, minute: u8, second: u8) -> u16 {
        ((hour as u16) << 11) | ((minute as u16) << 5) | ((second / 2) as u16)
    }

    #[test]
    fn recover_deleted_files_reads_a_deleted_short_name_entry() {
        let image_path = write_test_image("deleted-fat32.img", &minimal_deleted_fat32_image());
        let deleted =
            recover_deleted_files(&image_path).expect("deleted FAT32 files should be parsed");

        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].name, "_EPORT.TXT");
        assert_eq!(deleted[0].extension, "txt");
        assert_eq!(deleted[0].path, "/");
        assert_eq!(deleted[0].size_bytes, 11);
        assert_eq!(
            deleted[0].created_at.as_deref(),
            Some("2024-03-14T09:26:12")
        );
        assert_eq!(
            deleted[0].modified_at.as_deref(),
            Some("2024-03-15T16:08:00")
        );
        assert_eq!(deleted[0].integrity, "intact");
        assert_eq!(deleted[0].start_offset, 1536);
        assert_eq!(deleted[0].clusters, vec![3]);
        assert_eq!(deleted[0].byte_runs.len(), 1);
    }

    #[test]
    fn recover_deleted_files_rebuilds_deleted_long_file_names() {
        let image_path = write_test_image(
            "deleted-fat32-long-name.img",
            &deleted_fat32_image_with_long_name("Quarterly Report.txt"),
        );
        let deleted =
            recover_deleted_files(&image_path).expect("deleted FAT32 long names should be parsed");

        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].name, "Quarterly Report.txt");
        assert_eq!(deleted[0].extension, "txt");
        assert_eq!(deleted[0].path, "/");
        assert_eq!(deleted[0].size_bytes, 11);
        assert_eq!(deleted[0].clusters, vec![3]);
    }

    #[test]
    fn recover_deleted_files_marks_partially_reconstructible_cluster_runs() {
        let image_path = write_test_image(
            "deleted-fat32-partial.img",
            &partially_overwritten_deleted_fat32_image(),
        );
        let deleted =
            recover_deleted_files(&image_path).expect("partial FAT32 deletions should be parsed");

        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].name, "_IDEOPA.BIN");
        assert_eq!(deleted[0].expected_size_bytes, 1200);
        assert_eq!(deleted[0].size_bytes, 512);
        assert_eq!(
            deleted[0].created_at.as_deref(),
            Some("2021-12-01T07:04:00")
        );
        assert_eq!(
            deleted[0].modified_at.as_deref(),
            Some("2022-01-10T08:02:00")
        );
        assert_eq!(deleted[0].integrity, "partial");
        assert_eq!(deleted[0].clusters, vec![3]);
        assert_eq!(deleted[0].byte_runs.len(), 1);
    }

    #[test]
    fn list_visible_files_reads_a_visible_fat32_entry() {
        let image_path = write_test_image("visible-fat32.img", &minimal_deleted_fat32_image());
        let visible =
            list_visible_files(&image_path).expect("visible FAT32 files should be parsed");

        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "LIVELOG.TXT");
        assert_eq!(visible[0].extension, "txt");
        assert_eq!(visible[0].path, "/");
        assert_eq!(visible[0].size_bytes, 9);
        assert_eq!(
            visible[0].created_at.as_deref(),
            Some("2024-03-13T08:10:00")
        );
        assert_eq!(
            visible[0].modified_at.as_deref(),
            Some("2024-03-13T08:12:00")
        );
        assert_eq!(visible[0].integrity, "intact");
        assert_eq!(visible[0].start_offset, Some(2048));
        assert_eq!(visible[0].clusters, vec![4]);
        assert_eq!(visible[0].byte_runs.len(), 1);
    }
}
