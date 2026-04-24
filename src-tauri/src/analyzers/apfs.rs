use crate::types::ByteRun;
use ::apfs::{btree, catalog, object, omap, superblock};
use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const APFS_ROOT_DIR_RECORD: u64 = 2;

#[derive(Debug, Clone)]
pub struct ApfsVisibleFileCandidate {
    pub name: String,
    pub extension: String,
    pub path: String,
    pub size_bytes: u64,
    pub expected_size_bytes: Option<u64>,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub integrity: String,
    pub recovery_score: u8,
    pub start_offset: Option<u64>,
    pub byte_runs: Vec<ByteRun>,
}

#[derive(Debug, Clone)]
pub struct ApfsDeletedFileCandidate {
    pub name: String,
    pub extension: String,
    pub path: String,
    pub size_bytes: u64,
    pub expected_size_bytes: Option<u64>,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub integrity: String,
    pub recovery_score: u8,
    pub start_offset: Option<u64>,
    pub byte_runs: Vec<ByteRun>,
}

#[derive(Debug, Clone)]
struct ApfsVolumeContext {
    block_size: u32,
    catalog_root_block: u64,
    volume_omap_root_block: u64,
}

#[derive(Debug, Clone)]
struct ApfsCatalogState {
    active_file_ids: HashSet<u64>,
    file_inodes: Vec<ApfsCatalogInodeRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApfsCatalogInodeRecord {
    oid: u64,
    private_id: u64,
    size_bytes: u64,
    nlink: u32,
    created_at: Option<String>,
    modified_at: Option<String>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApfsDeletedCatalogDebugSummary {
    pub total_file_inodes: usize,
    pub active_file_ids: usize,
    pub deleted_inode_candidates: usize,
    pub deleted_zero_nlink_candidates: usize,
    pub deleted_nonzero_nlink_candidates: usize,
    pub deleted_candidates_with_extents: usize,
    pub deleted_candidates_without_extents: usize,
}

pub fn list_visible_files(image_path: &Path) -> Result<Vec<ApfsVisibleFileCandidate>, String> {
    let mut reader = File::open(image_path).map_err(|error| {
        format!(
            "Unable to open the APFS image {}: {}",
            image_path.to_string_lossy(),
            error
        )
    })?;
    let context = open_primary_volume_context(&mut reader)?;
    let mut files = Vec::new();
    walk_directory(&mut reader, &context, APFS_ROOT_DIR_RECORD, "", &mut files)?;
    Ok(files)
}

pub fn recover_deleted_files(image_path: &Path) -> Result<Vec<ApfsDeletedFileCandidate>, String> {
    let mut reader = File::open(image_path).map_err(|error| {
        format!(
            "Unable to open the APFS image {}: {}",
            image_path.to_string_lossy(),
            error
        )
    })?;
    let context = open_primary_volume_context(&mut reader)?;
    let catalog_state = scan_catalog_state(&mut reader, &context)?;
    let deleted_inodes = select_deleted_catalog_inodes(&catalog_state);
    let mut files = Vec::new();

    for inode in deleted_inodes {
        let extents = catalog::lookup_extents(
            &mut reader,
            context.catalog_root_block,
            context.volume_omap_root_block,
            context.block_size,
            inode.private_id,
        )
        .map_err(|error| {
            format!(
                "Unable to read APFS extents for deleted inode {}: {error}",
                inode.oid
            )
        })?;

        let (byte_runs, reconstructable_size, expected_size_bytes, integrity, recovery_score) =
            byte_runs_from_extents(context.block_size, inode.size_bytes, &extents);
        if reconstructable_size == 0 || byte_runs.is_empty() {
            continue;
        }

        let preview_bytes = read_preview_bytes(&mut reader, &byte_runs, 256)?;
        let extension = infer_extension(&preview_bytes);
        let (integrity, recovery_score) =
            deleted_apfs_recovery_profile(integrity, recovery_score, inode.nlink);

        files.push(ApfsDeletedFileCandidate {
            name: deleted_apfs_name(inode.oid, &extension),
            extension,
            path: "/orphaned-apfs-catalog".into(),
            size_bytes: reconstructable_size,
            expected_size_bytes,
            created_at: inode.created_at.clone(),
            modified_at: inode.modified_at.clone(),
            integrity,
            recovery_score,
            start_offset: byte_runs.first().map(|run| run.offset),
            byte_runs,
        });
    }

    Ok(files)
}

#[cfg(test)]
pub(crate) fn debug_deleted_catalog_candidates(
    image_path: &Path,
) -> Result<ApfsDeletedCatalogDebugSummary, String> {
    let mut reader = File::open(image_path).map_err(|error| {
        format!(
            "Unable to open the APFS image {}: {}",
            image_path.to_string_lossy(),
            error
        )
    })?;
    let context = open_primary_volume_context(&mut reader)?;
    summarize_deleted_catalog_candidates(&mut reader, &context)
}

fn open_primary_volume_context(reader: &mut File) -> Result<ApfsVolumeContext, String> {
    let nxsb = superblock::read_nxsb(reader)
        .and_then(|checkpoint| superblock::find_latest_nxsb(reader, &checkpoint))
        .map_err(|error| format!("Unable to read the APFS container superblock: {error}"))?;
    let block_size = nxsb.block_size;

    let container_omap_root = omap::read_omap_tree_root(reader, nxsb.omap_oid, block_size)
        .map_err(|error| format!("Unable to read the APFS container object map: {error}"))?;
    let volume_oid = nxsb
        .fs_oids
        .iter()
        .find(|oid| **oid != 0)
        .copied()
        .ok_or_else(|| {
            "The APFS container does not expose a usable volume superblock OID.".to_string()
        })?;
    let volume_block = omap::omap_lookup(reader, container_omap_root, block_size, volume_oid)
        .map_err(|error| format!("Unable to resolve the APFS volume superblock: {error}"))?;
    let volume_data = object::read_block(reader, volume_block, block_size)
        .map_err(|error| format!("Unable to read the APFS volume superblock block: {error}"))?;
    let volume_superblock = superblock::ApfsSuperblock::parse(&volume_data)
        .map_err(|error| format!("Unable to parse the APFS volume superblock: {error}"))?;

    let volume_omap_root_block =
        omap::read_omap_tree_root(reader, volume_superblock.omap_oid, block_size)
            .map_err(|error| format!("Unable to read the APFS volume object map: {error}"))?;
    let catalog_root_block = omap::omap_lookup(
        reader,
        volume_omap_root_block,
        block_size,
        volume_superblock.root_tree_oid,
    )
    .map_err(|error| format!("Unable to resolve the APFS catalog root: {error}"))?;

    Ok(ApfsVolumeContext {
        block_size,
        catalog_root_block,
        volume_omap_root_block,
    })
}

fn scan_catalog_state(
    reader: &mut File,
    context: &ApfsVolumeContext,
) -> Result<ApfsCatalogState, String> {
    let entries = btree::btree_scan(
        reader,
        context.catalog_root_block,
        context.block_size,
        0,
        0,
        &|_| Some(true),
        Some(context.volume_omap_root_block),
    )
    .map_err(|error| format!("Unable to scan the APFS catalog B-tree: {error}"))?;

    scan_catalog_state_from_entries(&entries)
}

fn scan_catalog_state_from_entries(
    entries: &[(Vec<u8>, Vec<u8>)],
) -> Result<ApfsCatalogState, String> {
    let mut active_file_ids = HashSet::new();
    let mut file_inodes = Vec::new();

    for (key, value) in entries {
        let (oid, record_type) = decode_catalog_key(key)?;
        match record_type {
            catalog::J_TYPE_DIR_REC => {
                let drec = catalog::DrecVal::parse(value).map_err(|error| {
                    format!("Unable to parse an APFS directory record: {error}")
                })?;
                if drec.file_type() == catalog::DT_REG {
                    active_file_ids.insert(drec.file_id);
                }
            }
            catalog::J_TYPE_INODE => {
                let inode = catalog::InodeVal::parse(value)
                    .map_err(|error| format!("Unable to parse an APFS inode record: {error}"))?;
                if inode.kind() == catalog::INODE_FILE_TYPE && inode.size() > 0 {
                    file_inodes.push(ApfsCatalogInodeRecord {
                        oid,
                        private_id: inode.private_id,
                        size_bytes: inode.size(),
                        nlink: inode.nlink(),
                        created_at: apfs_nanoseconds_to_iso(inode.create_time),
                        modified_at: apfs_nanoseconds_to_iso(inode.modify_time),
                    });
                }
            }
            _ => {}
        }
    }

    Ok(ApfsCatalogState {
        active_file_ids,
        file_inodes,
    })
}

fn select_deleted_catalog_inodes(state: &ApfsCatalogState) -> Vec<ApfsCatalogInodeRecord> {
    state
        .file_inodes
        .iter()
        .filter(|record| record.private_id != 0 && !state.active_file_ids.contains(&record.oid))
        .cloned()
        .collect()
}

#[cfg(test)]
fn summarize_deleted_catalog_candidates(
    reader: &mut File,
    context: &ApfsVolumeContext,
) -> Result<ApfsDeletedCatalogDebugSummary, String> {
    let catalog_state = scan_catalog_state(reader, context)?;
    let deleted_inodes = select_deleted_catalog_inodes(&catalog_state);
    let mut with_extents = 0_usize;
    let mut without_extents = 0_usize;

    for inode in &deleted_inodes {
        let extents = catalog::lookup_extents(
            reader,
            context.catalog_root_block,
            context.volume_omap_root_block,
            context.block_size,
            inode.private_id,
        )
        .map_err(|error| {
            format!(
                "Unable to inspect APFS extents for deleted inode {}: {error}",
                inode.oid
            )
        })?;

        if extents.iter().any(|extent| extent.length() > 0) {
            with_extents += 1;
        } else {
            without_extents += 1;
        }
    }

    Ok(ApfsDeletedCatalogDebugSummary {
        total_file_inodes: catalog_state.file_inodes.len(),
        active_file_ids: catalog_state.active_file_ids.len(),
        deleted_inode_candidates: deleted_inodes.len(),
        deleted_zero_nlink_candidates: deleted_inodes
            .iter()
            .filter(|inode| inode.nlink == 0)
            .count(),
        deleted_nonzero_nlink_candidates: deleted_inodes
            .iter()
            .filter(|inode| inode.nlink != 0)
            .count(),
        deleted_candidates_with_extents: with_extents,
        deleted_candidates_without_extents: without_extents,
    })
}

fn walk_directory(
    reader: &mut File,
    context: &ApfsVolumeContext,
    parent_oid: u64,
    parent_path: &str,
    files: &mut Vec<ApfsVisibleFileCandidate>,
) -> Result<(), String> {
    let entries = catalog::list_directory(
        reader,
        context.catalog_root_block,
        context.volume_omap_root_block,
        context.block_size,
        parent_oid,
    )
    .map_err(|error| format!("Unable to list the APFS catalog directory: {error}"))?;

    for entry in entries {
        let full_path = if parent_path.is_empty() {
            format!("/{}", entry.name)
        } else {
            format!("{parent_path}/{}", entry.name)
        };

        match entry.kind {
            ::apfs::EntryKind::Directory => {
                walk_directory(reader, context, entry.oid, &full_path, files)?;
            }
            ::apfs::EntryKind::File => {
                if let Some(candidate) =
                    build_visible_candidate(reader, context, &entry.name, &full_path, entry.oid)?
                {
                    files.push(candidate);
                }
            }
            ::apfs::EntryKind::Symlink => {}
        }
    }

    Ok(())
}

fn build_visible_candidate(
    reader: &mut File,
    context: &ApfsVolumeContext,
    name: &str,
    full_path: &str,
    oid: u64,
) -> Result<Option<ApfsVisibleFileCandidate>, String> {
    let inode = catalog::lookup_inode(
        reader,
        context.catalog_root_block,
        context.volume_omap_root_block,
        context.block_size,
        oid,
    )
    .map_err(|error| format!("Unable to read APFS inode metadata for {full_path}: {error}"))?;
    let extents = catalog::lookup_extents(
        reader,
        context.catalog_root_block,
        context.volume_omap_root_block,
        context.block_size,
        inode.private_id,
    )
    .map_err(|error| format!("Unable to read APFS extents for {full_path}: {error}"))?;

    let (byte_runs, reconstructable_size, expected_size_bytes, integrity, recovery_score) =
        byte_runs_from_extents(context.block_size, inode.size(), &extents);
    if reconstructable_size == 0 || byte_runs.is_empty() {
        return Ok(None);
    }

    Ok(Some(ApfsVisibleFileCandidate {
        name: name.into(),
        extension: file_extension(name),
        path: parent_display_path(full_path),
        size_bytes: reconstructable_size,
        expected_size_bytes,
        created_at: apfs_nanoseconds_to_iso(inode.create_time),
        modified_at: apfs_nanoseconds_to_iso(inode.modify_time),
        integrity,
        recovery_score,
        start_offset: byte_runs.first().map(|run| run.offset),
        byte_runs,
    }))
}

fn decode_catalog_key(key_bytes: &[u8]) -> Result<(u64, u8), String> {
    if key_bytes.len() < 8 {
        return Err("The APFS catalog key is truncated.".into());
    }

    let obj_id_and_type = u64::from_le_bytes(
        key_bytes[0..8]
            .try_into()
            .map_err(|_| "The APFS catalog key header is truncated.".to_string())?,
    );
    let obj_id = obj_id_and_type & 0x0FFF_FFFF_FFFF_FFFF;
    let record_type = ((obj_id_and_type >> 60) & 0x0F) as u8;
    Ok((obj_id, record_type))
}

fn byte_runs_from_extents(
    block_size: u32,
    logical_size: u64,
    extents: &[catalog::FileExtentVal],
) -> (Vec<ByteRun>, u64, Option<u64>, String, u8) {
    if logical_size == 0 {
        return (Vec::new(), 0, None, "intact".into(), 97);
    }

    let mut remaining = logical_size;
    let mut byte_runs = Vec::new();
    let mut covered_bytes = 0_u64;

    for extent in extents {
        if remaining == 0 {
            break;
        }

        let extent_length = extent.length();
        if extent_length == 0 {
            continue;
        }

        let bytes_in_run = remaining.min(extent_length);
        append_byte_run(
            &mut byte_runs,
            extent.phys_block_num.saturating_mul(block_size as u64),
            bytes_in_run,
        );
        covered_bytes = covered_bytes.saturating_add(bytes_in_run);
        remaining = remaining.saturating_sub(bytes_in_run);
    }

    if covered_bytes == 0 || byte_runs.is_empty() {
        return (Vec::new(), 0, None, "corrupt".into(), 12);
    }

    let expected_size_bytes = Some(logical_size);
    if covered_bytes < logical_size {
        return (
            byte_runs,
            covered_bytes,
            expected_size_bytes,
            "partial".into(),
            42,
        );
    }

    if byte_runs.len() > 1 {
        return (
            byte_runs,
            covered_bytes,
            expected_size_bytes,
            "fragmented".into(),
            86,
        );
    }

    (
        byte_runs,
        covered_bytes,
        expected_size_bytes,
        "intact".into(),
        96,
    )
}

fn append_byte_run(byte_runs: &mut Vec<ByteRun>, offset: u64, length: u64) {
    if let Some(last) = byte_runs.last_mut() {
        if last.offset.saturating_add(last.length) == offset {
            last.length = last.length.saturating_add(length);
            return;
        }
    }

    byte_runs.push(ByteRun {
        offset,
        length,
        zero_fill: false,
        ..Default::default()
    });
}

fn deleted_apfs_recovery_profile(
    integrity: String,
    recovery_score: u8,
    nlink: u32,
) -> (String, u8) {
    let adjusted = if nlink == 0 {
        recovery_score.saturating_sub(24).max(34)
    } else {
        recovery_score.saturating_sub(42).max(24)
    };
    (integrity, adjusted)
}

fn deleted_apfs_name(oid: u64, extension: &str) -> String {
    if extension.is_empty() {
        format!("orphan-{oid:016x}")
    } else {
        format!("orphan-{oid:016x}.{extension}")
    }
}

fn read_preview_bytes(
    reader: &mut File,
    byte_runs: &[ByteRun],
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    for run in byte_runs {
        if bytes.len() >= max_bytes {
            break;
        }
        let bytes_to_read = run.length.min((max_bytes - bytes.len()) as u64) as usize;
        let mut chunk = vec![0_u8; bytes_to_read];
        read_exact_at(reader, run.offset, &mut chunk, "APFS preview bytes")?;
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn infer_extension(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return "jpg".into();
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "png".into();
    }
    if bytes.starts_with(b"%PDF-") {
        return "pdf".into();
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return "zip".into();
    }

    if std::str::from_utf8(bytes)
        .ok()
        .filter(|text| !text.is_empty())
        .map(|text| {
            text.chars()
                .filter(|character| {
                    character.is_ascii_graphic()
                        || character.is_ascii_whitespace()
                        || *character == '\u{fffd}'
                })
                .count()
                * 100
                / text.chars().count().max(1)
                >= 85
        })
        .unwrap_or(false)
    {
        return "txt".into();
    }

    "bin".into()
}

fn read_exact_at(
    reader: &mut File,
    offset: u64,
    bytes: &mut [u8],
    label: &str,
) -> Result<(), String> {
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|error| format!("Unable to seek {label} at offset {offset}: {error}"))?;
    reader
        .read_exact(bytes)
        .map_err(|error| format!("Unable to read {label} at offset {offset}: {error}"))
}

fn parent_display_path(full_path: &str) -> String {
    match full_path.rsplit_once('/') {
        Some(("", _)) | None => "/".into(),
        Some((parent, _)) => {
            if parent.is_empty() {
                "/".into()
            } else {
                parent.into()
            }
        }
    }
}

fn file_extension(name: &str) -> String {
    name.rsplit('.')
        .next()
        .filter(|extension| *extension != name && !extension.is_empty())
        .map(|extension| extension.to_ascii_lowercase())
        .unwrap_or_default()
}

fn apfs_nanoseconds_to_iso(nanoseconds: i64) -> Option<String> {
    if nanoseconds <= 0 {
        return None;
    }

    let unix_seconds = nanoseconds.div_euclid(1_000_000_000);
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
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };

    (year as i32, month as u32, day as u32)
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::{imaging, partitioning, types::FilesystemType};
    use std::{
        fs::{self, File},
        io::Write,
        path::{Path, PathBuf},
        process::Command,
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    pub(crate) struct ApfsTestFixture {
        pub root_dir: PathBuf,
        pub image_path: PathBuf,
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn create_raw_apfs_image_for_tests(
        files: &[(&str, &[u8])],
    ) -> Result<ApfsTestFixture, String> {
        create_raw_apfs_image_with_deleted_files_for_tests(files, &[])
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn create_raw_apfs_image_with_deleted_files_for_tests(
        files: &[(&str, &[u8])],
        deleted_files: &[(&str, &[u8])],
    ) -> Result<ApfsTestFixture, String> {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("Unable to build APFS fixture timestamp: {error}"))?
            .as_millis();
        let root_dir = std::env::temp_dir().join(format!("recupere-apfs-fixture-{suffix}"));
        fs::create_dir_all(&root_dir)
            .map_err(|error| format!("Unable to create APFS fixture directory: {error}"))?;
        let image_path = root_dir.join("fixture.apfs.raw");
        File::create(&image_path)
            .and_then(|file| file.set_len(64 * 1024 * 1024))
            .map_err(|error| format!("Unable to allocate APFS raw fixture file: {error}"))?;

        let disk = attach_raw_disk(&image_path)?;
        let volume_name = format!("RecupereApfs{suffix}");
        let mount_point = PathBuf::from("/Volumes").join(&volume_name);
        let sliced_image_path = root_dir.join("fixture.apfs.slice.raw");

        let result = (|| -> Result<(), String> {
            run_command(
                Command::new("diskutil")
                    .arg("eraseDisk")
                    .arg("APFS")
                    .arg(&volume_name)
                    .arg(&disk),
                "Unable to format the APFS raw fixture",
            )?;

            wait_for_mount_point(&mount_point)?;

            for (relative_path, bytes) in files {
                let target = mount_point.join(relative_path.trim_start_matches('/'));
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|error| {
                        format!(
                            "Unable to create APFS fixture directory {}: {}",
                            parent.to_string_lossy(),
                            error
                        )
                    })?;
                }
                fs::write(&target, bytes).map_err(|error| {
                    format!(
                        "Unable to write APFS fixture file {}: {}",
                        target.to_string_lossy(),
                        error
                    )
                })?;
            }

            let mut deleted_handles = Vec::new();
            for (relative_path, bytes) in deleted_files {
                let target = mount_point.join(relative_path.trim_start_matches('/'));
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|error| {
                        format!(
                            "Unable to create APFS fixture directory {}: {}",
                            parent.to_string_lossy(),
                            error
                        )
                    })?;
                }
                let mut handle = File::create(&target).map_err(|error| {
                    format!(
                        "Unable to create APFS fixture file {}: {}",
                        target.to_string_lossy(),
                        error
                    )
                })?;
                handle.write_all(bytes).map_err(|error| {
                    format!(
                        "Unable to write APFS fixture file {}: {}",
                        target.to_string_lossy(),
                        error
                    )
                })?;
                handle.sync_all().map_err(|error| {
                    format!(
                        "Unable to flush APFS fixture file {}: {}",
                        target.to_string_lossy(),
                        error
                    )
                })?;
                fs::remove_file(&target).map_err(|error| {
                    format!(
                        "Unable to delete APFS fixture file {}: {}",
                        target.to_string_lossy(),
                        error
                    )
                })?;
                if let Some(parent) = target.parent() {
                    File::open(parent)
                        .and_then(|dir| dir.sync_all())
                        .map_err(|error| {
                            format!(
                                "Unable to flush APFS fixture directory {}: {}",
                                parent.to_string_lossy(),
                                error
                            )
                        })?;
                }
                deleted_handles.push(handle);
            }

            run_command(
                &mut Command::new("sync"),
                "Unable to flush APFS fixture writes before slicing",
            )?;

            let candidate = partitioning::inspect_potential_volumes(&image_path)
                .map_err(|error| format!("Unable to detect the APFS container fixture: {error}"))?
                .into_iter()
                .find(|volume| matches!(volume.filesystem, FilesystemType::Apfs))
                .ok_or_else(|| {
                    "The generated raw disk fixture does not expose a detectable APFS container."
                        .to_string()
                })?;
            imaging::create_read_only_image_slice_at_controlled(
                &sliced_image_path,
                &image_path,
                candidate.start_offset,
                candidate.size_bytes,
                &mut |_| Ok(()),
            )
            .map_err(|error| format!("Unable to extract the APFS test slice: {error}"))?;

            drop(deleted_handles);
            Ok(())
        })();

        let detach_result = run_command(
            Command::new("hdiutil").arg("detach").arg(&disk),
            "Unable to detach the APFS raw fixture disk",
        );

        if let Err(error) = result {
            let _ = detach_result;
            let _ = fs::remove_dir_all(&root_dir);
            return Err(error);
        }
        if let Err(error) = detach_result {
            let _ = fs::remove_dir_all(&root_dir);
            return Err(error);
        }

        Ok(ApfsTestFixture {
            root_dir,
            image_path: sliced_image_path,
        })
    }

    #[cfg(target_os = "macos")]
    fn attach_raw_disk(image_path: &Path) -> Result<String, String> {
        let output = Command::new("hdiutil")
            .arg("attach")
            .arg("-nomount")
            .arg("-imagekey")
            .arg("diskimage-class=CRawDiskImage")
            .arg(image_path)
            .output()
            .map_err(|error| format!("Unable to launch hdiutil for APFS fixture: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "Unable to attach the APFS raw fixture: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find(|line| line.starts_with("/dev/"))
            .and_then(|line| line.split_whitespace().next())
            .map(|line| line.to_string())
            .ok_or_else(|| {
                format!(
                    "Unable to parse the attached APFS raw device path from hdiutil output: {}",
                    String::from_utf8_lossy(&output.stdout).trim()
                )
            })
    }

    #[cfg(target_os = "macos")]
    fn wait_for_mount_point(mount_point: &Path) -> Result<(), String> {
        for _ in 0..50 {
            if mount_point.exists() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }

        Err(format!(
            "The APFS test fixture mount point {} did not appear in time.",
            mount_point.to_string_lossy()
        ))
    }

    #[cfg(target_os = "macos")]
    fn run_command(command: &mut Command, error_prefix: &str) -> Result<(), String> {
        let output = command
            .output()
            .map_err(|error| format!("{error_prefix}: {error}"))?;
        if output.status.success() {
            return Ok(());
        }

        Err(format!(
            "{error_prefix}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apfs_catalog_key(oid: u64, record_type: u8) -> Vec<u8> {
        ((((record_type as u64) & 0x0F) << 60) | (oid & 0x0FFF_FFFF_FFFF_FFFF))
            .to_le_bytes()
            .to_vec()
    }

    fn apfs_inode_key(oid: u64) -> Vec<u8> {
        apfs_catalog_key(oid, catalog::J_TYPE_INODE)
    }

    fn apfs_drec_key(parent_oid: u64, name: &str) -> Vec<u8> {
        let mut key = apfs_catalog_key(parent_oid, catalog::J_TYPE_DIR_REC);
        let mut name_bytes = name.as_bytes().to_vec();
        name_bytes.push(0);
        let name_len_and_hash = (name_bytes.len() as u32).to_le_bytes();
        key.extend_from_slice(&name_len_and_hash);
        key.extend_from_slice(&name_bytes);
        key
    }

    fn apfs_inode_value(
        parent_id: u64,
        private_id: u64,
        create_time: i64,
        modify_time: i64,
        nlink: i32,
        mode: u16,
        uncompressed_size: u64,
    ) -> Vec<u8> {
        let mut bytes = vec![0_u8; 92];
        bytes[0..8].copy_from_slice(&parent_id.to_le_bytes());
        bytes[8..16].copy_from_slice(&private_id.to_le_bytes());
        bytes[16..24].copy_from_slice(&create_time.to_le_bytes());
        bytes[24..32].copy_from_slice(&modify_time.to_le_bytes());
        bytes[32..40].copy_from_slice(&modify_time.to_le_bytes());
        bytes[40..48].copy_from_slice(&modify_time.to_le_bytes());
        bytes[56..60].copy_from_slice(&nlink.to_le_bytes());
        bytes[80..82].copy_from_slice(&mode.to_le_bytes());
        bytes[82..84].copy_from_slice(&0_u16.to_le_bytes());
        bytes[84..92].copy_from_slice(&uncompressed_size.to_le_bytes());
        bytes
    }

    fn apfs_drec_value(file_id: u64, file_type: u16) -> Vec<u8> {
        let mut bytes = vec![0_u8; 18];
        bytes[0..8].copy_from_slice(&file_id.to_le_bytes());
        bytes[8..16].copy_from_slice(&0_i64.to_le_bytes());
        bytes[16..18].copy_from_slice(&file_type.to_le_bytes());
        bytes
    }

    #[test]
    fn byte_runs_from_extents_marks_partial_and_fragmented_files() {
        let extents = vec![
            catalog::FileExtentVal {
                flags_and_length: 4_096,
                phys_block_num: 8,
                crypto_id: 0,
            },
            catalog::FileExtentVal {
                flags_and_length: 2_048,
                phys_block_num: 16,
                crypto_id: 0,
            },
        ];

        let (byte_runs, size_bytes, expected_size_bytes, integrity, recovery_score) =
            byte_runs_from_extents(4_096, 8_192, &extents);

        assert_eq!(size_bytes, 6_144);
        assert_eq!(expected_size_bytes, Some(8_192));
        assert_eq!(integrity, "partial");
        assert_eq!(recovery_score, 42);
        assert_eq!(byte_runs.len(), 2);
        assert_eq!(byte_runs[0].offset, 32_768);
    }

    #[test]
    fn apfs_nanoseconds_to_iso_formats_unix_nanoseconds() {
        assert_eq!(
            apfs_nanoseconds_to_iso(1_710_374_400_000_000_000),
            Some("2024-03-14T00:00:00".into())
        );
        assert_eq!(apfs_nanoseconds_to_iso(0), None);
    }

    #[test]
    fn scan_catalog_state_tracks_active_and_orphan_file_inodes() {
        let entries = vec![
            (
                apfs_drec_key(APFS_ROOT_DIR_RECORD, "active.txt"),
                apfs_drec_value(10, catalog::DT_REG),
            ),
            (
                apfs_inode_key(10),
                apfs_inode_value(
                    APFS_ROOT_DIR_RECORD,
                    100,
                    1_710_374_400_000_000_000,
                    1_710_374_400_000_000_000,
                    1,
                    catalog::INODE_FILE_TYPE,
                    11,
                ),
            ),
            (
                apfs_inode_key(20),
                apfs_inode_value(
                    APFS_ROOT_DIR_RECORD,
                    200,
                    1_710_374_400_000_000_000,
                    1_710_374_400_000_000_000,
                    0,
                    catalog::INODE_FILE_TYPE,
                    12,
                ),
            ),
        ];

        let state = scan_catalog_state_from_entries(&entries)
            .expect("APFS catalog state should parse from synthetic entries");
        assert!(state.active_file_ids.contains(&10));
        assert!(!state.active_file_ids.contains(&20));
        assert_eq!(state.file_inodes.len(), 2);

        let deleted = select_deleted_catalog_inodes(&state);
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].oid, 20);
        assert_eq!(deleted[0].private_id, 200);
        assert_eq!(deleted[0].size_bytes, 12);
        assert_eq!(deleted[0].nlink, 0);
    }

    #[test]
    fn deleted_apfs_name_and_profile_stay_conservative() {
        assert_eq!(
            deleted_apfs_name(0x2A, "txt"),
            "orphan-000000000000002a.txt"
        );
        assert_eq!(deleted_apfs_name(0x2A, ""), "orphan-000000000000002a");

        let (integrity, score) = deleted_apfs_recovery_profile("intact".into(), 96, 0);
        assert_eq!(integrity, "intact");
        assert_eq!(score, 72);

        let (_, linked_score) = deleted_apfs_recovery_profile("fragmented".into(), 86, 1);
        assert_eq!(linked_score, 44);
    }

    #[test]
    fn deleted_catalog_debug_summary_counts_candidates_from_synthetic_entries() {
        let entries = vec![
            (
                apfs_drec_key(APFS_ROOT_DIR_RECORD, "active.txt"),
                apfs_drec_value(10, catalog::DT_REG),
            ),
            (
                apfs_inode_key(10),
                apfs_inode_value(
                    APFS_ROOT_DIR_RECORD,
                    100,
                    1_710_374_400_000_000_000,
                    1_710_374_400_000_000_000,
                    1,
                    catalog::INODE_FILE_TYPE,
                    11,
                ),
            ),
            (
                apfs_inode_key(20),
                apfs_inode_value(
                    APFS_ROOT_DIR_RECORD,
                    200,
                    1_710_374_400_000_000_000,
                    1_710_374_400_000_000_000,
                    0,
                    catalog::INODE_FILE_TYPE,
                    12,
                ),
            ),
        ];

        let state = scan_catalog_state_from_entries(&entries)
            .expect("APFS catalog state should parse from synthetic entries");
        let deleted = select_deleted_catalog_inodes(&state);
        let summary = ApfsDeletedCatalogDebugSummary {
            total_file_inodes: state.file_inodes.len(),
            active_file_ids: state.active_file_ids.len(),
            deleted_inode_candidates: deleted.len(),
            deleted_zero_nlink_candidates: deleted.iter().filter(|inode| inode.nlink == 0).count(),
            deleted_nonzero_nlink_candidates: deleted
                .iter()
                .filter(|inode| inode.nlink != 0)
                .count(),
            deleted_candidates_with_extents: 0,
            deleted_candidates_without_extents: deleted.len(),
        };

        assert_eq!(
            summary,
            ApfsDeletedCatalogDebugSummary {
                total_file_inodes: 2,
                active_file_ids: 1,
                deleted_inode_candidates: 1,
                deleted_zero_nlink_candidates: 1,
                deleted_nonzero_nlink_candidates: 0,
                deleted_candidates_with_extents: 0,
                deleted_candidates_without_extents: 1,
            }
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn list_visible_files_reads_a_real_apfs_fixture() {
        let fixture = test_support::create_raw_apfs_image_for_tests(&[
            ("hello.txt", b"hello apfs"),
            ("docs/note.md", b"nested file"),
        ])
        .expect("APFS fixture should be created");

        let files = list_visible_files(&fixture.image_path)
            .expect("APFS visible files should be listed from the raw fixture");

        let hello = files
            .iter()
            .find(|file| file.name == "hello.txt")
            .expect("hello.txt should be present");
        assert_eq!(hello.path, "/");
        assert_eq!(hello.size_bytes, 10);
        assert!(!hello.byte_runs.is_empty());

        let note = files
            .iter()
            .find(|file| file.name == "note.md")
            .expect("note.md should be present");
        assert_eq!(note.path, "/docs");
        assert_eq!(note.extension, "md");
        assert!(matches!(note.integrity.as_str(), "intact" | "fragmented"));

        let _ = std::fs::remove_dir_all(&fixture.root_dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "APFS orphan-catalog fixture is not deterministic on macOS 15.7.4; tracked under TT-05/TT-01"]
    fn recover_deleted_files_reads_a_real_apfs_deleted_fixture() {
        let fixture = test_support::create_raw_apfs_image_with_deleted_files_for_tests(
            &[("hello.txt", b"hello apfs")],
            &[("deleted.txt", b"deleted apfs payload")],
        )
        .expect("APFS deleted fixture should be created");

        let files = recover_deleted_files(&fixture.image_path)
            .expect("APFS deleted candidates should be listed from the raw fixture");

        let deleted = files
            .iter()
            .find(|file| file.path == "/orphaned-apfs-catalog")
            .expect("deleted APFS candidate should be present");
        assert_eq!(deleted.size_bytes, b"deleted apfs payload".len() as u64);
        assert_eq!(
            deleted.expected_size_bytes,
            Some(b"deleted apfs payload".len() as u64)
        );
        assert!(!deleted.byte_runs.is_empty());

        let _ = std::fs::remove_dir_all(&fixture.root_dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "APFS orphan-catalog fixture is not deterministic on macOS 15.7.4; debug helper for TT-05/TT-01"]
    fn debug_deleted_catalog_candidates_reports_real_deleted_fixture_summary() {
        let fixture = test_support::create_raw_apfs_image_with_deleted_files_for_tests(
            &[("hello.txt", b"hello apfs")],
            &[("deleted.txt", b"deleted apfs payload")],
        )
        .expect("APFS deleted fixture should be created");

        let summary = debug_deleted_catalog_candidates(&fixture.image_path)
            .expect("debug summary should inspect the APFS deleted fixture");

        println!("APFS deleted fixture summary: {summary:?}");

        let _ = std::fs::remove_dir_all(&fixture.root_dir);
    }
}
