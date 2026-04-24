use crate::types::{ByteRun, FileFork};
use std::{
    array,
    collections::{HashMap, HashSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const HFSPLUS_VOLUME_HEADER_OFFSET: u64 = 1024;
const HFSPLUS_SIGNATURE: u16 = 0x482B;
const HFSX_SIGNATURE: u16 = 0x4858;
const HFS_WRAPPER_MDB_SIGNATURE: u16 = 0x4244;
const HFSPLUS_CATALOG_FILE_ID: u32 = 4;
const HFSPLUS_ROOT_FOLDER_ID: u32 = 2;
const HFSPLUS_DATA_FORK_TYPE: u8 = 0;
const HFSPLUS_RESOURCE_FORK_TYPE: u8 = 0xFF;
const HFSPLUS_CATALOG_RECORD_FOLDER: u16 = 0x0001;
const HFSPLUS_CATALOG_RECORD_FILE: u16 = 0x0002;
const HFSPLUS_LEAF_NODE_KIND: i8 = -1;
const HFSPLUS_UNIX_EPOCH_OFFSET_SECONDS: i64 = 2_082_844_800;

type HfsPlusForkRuns = (Vec<ByteRun>, u64, Option<u64>, String, u8);

#[derive(Debug, Clone)]
pub struct HfsPlusVisibleFileCandidate {
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
    pub resource_fork: Option<FileFork>,
}

#[derive(Debug, Clone)]
pub struct HfsPlusDeletedFileCandidate {
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
    pub resource_fork: Option<FileFork>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HfsPlusVolumeLayout {
    pub volume_offset: u64,
    pub block_size: u32,
    pub total_blocks: u32,
    pub extents_file: HfsPlusForkData,
    pub catalog_file: HfsPlusForkData,
    pub wrapped: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HfsPlusForkData {
    logical_size: u64,
    total_blocks: u32,
    extents: [HfsPlusExtentDescriptor; 8],
}

#[derive(Debug, Clone, Copy, Default)]
struct HfsPlusExtentDescriptor {
    start_block: u32,
    block_count: u32,
}

#[derive(Debug, Clone)]
struct FolderPathNode {
    name: String,
    parent_id: u32,
}

#[derive(Debug, Clone)]
struct PendingFileRecord {
    file_id: u32,
    name: String,
    parent_id: u32,
    size_bytes: u64,
    expected_size_bytes: Option<u64>,
    created_at: Option<String>,
    modified_at: Option<String>,
    integrity: String,
    recovery_score: u8,
    start_offset: Option<u64>,
    byte_runs: Vec<ByteRun>,
    resource_fork: Option<FileFork>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CatalogFileIdentity {
    file_id: u32,
    parent_id: u32,
    name: String,
}

#[derive(Debug, Clone)]
struct LeafRecordLayout {
    bounds: Vec<(usize, usize)>,
    free_space: usize,
}

#[derive(Debug, Clone, Copy)]
struct HfsPlusExtentKey {
    fork_type: u8,
    file_id: u32,
    start_block: u32,
}

pub fn list_visible_files(image_path: &Path) -> Result<Vec<HfsPlusVisibleFileCandidate>, String> {
    let mut reader = File::open(image_path).map_err(|error| {
        format!(
            "Unable to open the HFS+ image {}: {}",
            image_path.to_string_lossy(),
            error
        )
    })?;
    let image_size = reader
        .metadata()
        .map_err(|error| {
            format!(
                "Unable to inspect the HFS+ image metadata for {}: {}",
                image_path.to_string_lossy(),
                error
            )
        })?
        .len();

    let layout = inspect_volume_layout(&mut reader, 0, Some(image_size))?.ok_or_else(|| {
        "The image does not expose a usable HFS+ or HFSX volume header.".to_string()
    })?;
    let catalog_bytes = read_fork_bytes(
        &mut reader,
        &layout,
        HFSPLUS_CATALOG_FILE_ID,
        HFSPLUS_DATA_FORK_TYPE,
        &layout.catalog_file,
        "catalog file",
    )?;

    parse_catalog_visible_files(&mut reader, &layout, &catalog_bytes)
}

pub fn recover_deleted_files(
    image_path: &Path,
) -> Result<Vec<HfsPlusDeletedFileCandidate>, String> {
    let mut reader = File::open(image_path).map_err(|error| {
        format!(
            "Unable to open the HFS+ image {}: {}",
            image_path.to_string_lossy(),
            error
        )
    })?;
    let image_size = reader
        .metadata()
        .map_err(|error| {
            format!(
                "Unable to inspect the HFS+ image metadata for {}: {}",
                image_path.to_string_lossy(),
                error
            )
        })?
        .len();

    let layout = inspect_volume_layout(&mut reader, 0, Some(image_size))?.ok_or_else(|| {
        "The image does not expose a usable HFS+ or HFSX volume header.".to_string()
    })?;
    let catalog_bytes = read_fork_bytes(
        &mut reader,
        &layout,
        HFSPLUS_CATALOG_FILE_ID,
        HFSPLUS_DATA_FORK_TYPE,
        &layout.catalog_file,
        "catalog file",
    )?;

    parse_catalog_deleted_files(&mut reader, &layout, &catalog_bytes)
}

pub(crate) fn inspect_volume_layout(
    reader: &mut File,
    base_offset: u64,
    remaining_bytes: Option<u64>,
) -> Result<Option<HfsPlusVolumeLayout>, String> {
    if let Some(layout) =
        read_hfsplus_volume_header(reader, base_offset, 0, remaining_bytes, false)?
    {
        return Ok(Some(layout));
    }

    let embedded_relative_offset =
        match read_hfs_wrapper_embedded_offset(reader, base_offset, remaining_bytes)? {
            Some(offset) => offset,
            None => return Ok(None),
        };

    read_hfsplus_volume_header(
        reader,
        base_offset,
        embedded_relative_offset,
        remaining_bytes,
        true,
    )
}

fn read_hfs_wrapper_embedded_offset(
    reader: &mut File,
    base_offset: u64,
    remaining_bytes: Option<u64>,
) -> Result<Option<u64>, String> {
    if remaining_bytes.is_some_and(|remaining| remaining < HFSPLUS_VOLUME_HEADER_OFFSET + 512) {
        return Ok(None);
    }

    let mut mdb = [0_u8; 512];
    read_exact_at(
        reader,
        base_offset + HFSPLUS_VOLUME_HEADER_OFFSET,
        &mut mdb,
        "HFS wrapper MDB",
    )?;

    if be_u16(&mdb[0..2]) != HFS_WRAPPER_MDB_SIGNATURE {
        return Ok(None);
    }

    let embedded_signature = be_u16(&mdb[0x7C..0x7E]);
    if !matches!(embedded_signature, HFSPLUS_SIGNATURE | HFSX_SIGNATURE) {
        return Ok(None);
    }

    let allocation_block_size = be_u32(&mdb[0x14..0x18]) as u64;
    let allocation_block_start = be_u16(&mdb[0x1C..0x1E]) as u64;
    let embedded_start_block = be_u16(&mdb[0x7E..0x80]) as u64;

    if allocation_block_size == 0 {
        return Ok(None);
    }

    let embedded_relative_offset = allocation_block_start
        .saturating_mul(512)
        .saturating_add(embedded_start_block.saturating_mul(allocation_block_size));

    if remaining_bytes.is_some_and(|remaining| {
        embedded_relative_offset.saturating_add(HFSPLUS_VOLUME_HEADER_OFFSET + 512) > remaining
    }) {
        return Ok(None);
    }

    Ok(Some(embedded_relative_offset))
}

fn read_hfsplus_volume_header(
    reader: &mut File,
    base_offset: u64,
    relative_volume_offset: u64,
    remaining_bytes: Option<u64>,
    wrapped: bool,
) -> Result<Option<HfsPlusVolumeLayout>, String> {
    if remaining_bytes.is_some_and(|remaining| {
        relative_volume_offset.saturating_add(HFSPLUS_VOLUME_HEADER_OFFSET + 512) > remaining
    }) {
        return Ok(None);
    }

    let mut header = [0_u8; 512];
    read_exact_at(
        reader,
        base_offset + relative_volume_offset + HFSPLUS_VOLUME_HEADER_OFFSET,
        &mut header,
        "HFS+ volume header",
    )?;

    let signature = be_u16(&header[0..2]);
    if !matches!(signature, HFSPLUS_SIGNATURE | HFSX_SIGNATURE) {
        return Ok(None);
    }

    let block_size = be_u32(&header[40..44]);
    let total_blocks = be_u32(&header[44..48]);
    if block_size == 0 || total_blocks == 0 {
        return Ok(None);
    }

    let volume_size_bytes = block_size as u64 * total_blocks as u64;
    if remaining_bytes.is_some_and(|remaining| {
        relative_volume_offset.saturating_add(volume_size_bytes) > remaining
    }) {
        return Ok(None);
    }

    let extents_file = parse_fork_data(&header[192..272])?;
    let catalog_file = parse_fork_data(&header[272..352])?;
    if catalog_file.logical_size == 0 {
        return Ok(None);
    }

    Ok(Some(HfsPlusVolumeLayout {
        volume_offset: base_offset + relative_volume_offset,
        block_size,
        total_blocks,
        extents_file,
        catalog_file,
        wrapped,
    }))
}

fn parse_fork_data(bytes: &[u8]) -> Result<HfsPlusForkData, String> {
    if bytes.len() < 80 {
        return Err("The HFS+ fork data is truncated.".into());
    }

    let extents = array::from_fn(|index| {
        let offset = 16 + index * 8;
        HfsPlusExtentDescriptor {
            start_block: be_u32(&bytes[offset..offset + 4]),
            block_count: be_u32(&bytes[offset + 4..offset + 8]),
        }
    });

    Ok(HfsPlusForkData {
        logical_size: be_u64(&bytes[0..8]),
        total_blocks: be_u32(&bytes[12..16]),
        extents,
    })
}

fn read_inline_fork_bytes(
    reader: &mut File,
    layout: &HfsPlusVolumeLayout,
    fork: &HfsPlusForkData,
    label: &str,
) -> Result<Vec<u8>, String> {
    let mut remaining = fork.logical_size;
    let mut bytes = Vec::new();

    for extent in fork.extents.iter().copied() {
        if remaining == 0 {
            break;
        }
        if extent.block_count == 0 {
            continue;
        }

        let extent_bytes = extent.block_count as u64 * layout.block_size as u64;
        let bytes_to_read = remaining.min(extent_bytes);
        let mut chunk = vec![0_u8; bytes_to_read as usize];
        read_exact_at(
            reader,
            layout.volume_offset + extent.start_block as u64 * layout.block_size as u64,
            &mut chunk,
            label,
        )?;
        bytes.extend_from_slice(&chunk);
        remaining = remaining.saturating_sub(bytes_to_read);
    }

    if remaining > 0 {
        return Err(format!(
            "The HFS+ {label} exceeds the first inline extents. Extents-overflow follow-up is not implemented yet."
        ));
    }

    Ok(bytes)
}

fn read_fork_bytes(
    reader: &mut File,
    layout: &HfsPlusVolumeLayout,
    file_id: u32,
    fork_type: u8,
    fork: &HfsPlusForkData,
    label: &str,
) -> Result<Vec<u8>, String> {
    let (extents, overflow_complete) =
        resolve_fork_extents(reader, layout, file_id, fork_type, fork)?;
    let mut remaining = fork.logical_size;
    let mut bytes = Vec::with_capacity(fork.logical_size.min(usize::MAX as u64) as usize);

    for extent in extents {
        if remaining == 0 {
            break;
        }
        if extent.block_count == 0 {
            continue;
        }

        let extent_bytes = extent.block_count as u64 * layout.block_size as u64;
        let bytes_to_read = remaining.min(extent_bytes);
        let mut chunk = vec![0_u8; bytes_to_read as usize];
        read_exact_at(
            reader,
            layout.volume_offset + extent.start_block as u64 * layout.block_size as u64,
            &mut chunk,
            label,
        )?;
        bytes.extend_from_slice(&chunk);
        remaining = remaining.saturating_sub(bytes_to_read);
    }

    if remaining > 0 || !overflow_complete {
        return Err(format!(
            "The HFS+ {label} could not be reconstructed completely from inline and extents-overflow records."
        ));
    }

    Ok(bytes)
}

fn resolve_fork_extents(
    reader: &mut File,
    layout: &HfsPlusVolumeLayout,
    file_id: u32,
    fork_type: u8,
    fork: &HfsPlusForkData,
) -> Result<(Vec<HfsPlusExtentDescriptor>, bool), String> {
    let mut extents = fork
        .extents
        .iter()
        .copied()
        .filter(|extent| extent.block_count > 0)
        .collect::<Vec<_>>();
    let mut covered_blocks = total_extent_blocks_slice(&extents);

    if covered_blocks >= fork.total_blocks {
        return Ok((extents, true));
    }

    let extents_bytes = read_inline_fork_bytes(
        reader,
        layout,
        &layout.extents_file,
        "extents overflow file",
    )?;

    while covered_blocks < fork.total_blocks {
        let record = match lookup_extent_overflow_record(
            &extents_bytes,
            HfsPlusExtentKey {
                fork_type,
                file_id,
                start_block: covered_blocks,
            },
        )? {
            Some(record) => record,
            None => return Ok((extents, false)),
        };

        let before = covered_blocks;
        for extent in record.into_iter().filter(|extent| extent.block_count > 0) {
            extents.push(extent);
        }
        covered_blocks = total_extent_blocks_slice(&extents);
        if covered_blocks <= before {
            return Ok((extents, false));
        }
    }

    Ok((extents, true))
}

fn lookup_extent_overflow_record(
    extents_bytes: &[u8],
    expected_key: HfsPlusExtentKey,
) -> Result<Option<[HfsPlusExtentDescriptor; 8]>, String> {
    if extents_bytes.len() < 34 {
        return Err(
            "The HFS+ extents-overflow file is too small to contain a usable B-tree header.".into(),
        );
    }

    let node_size = be_u16(&extents_bytes[32..34]) as usize;
    let first_leaf_node = be_u32(&extents_bytes[24..28]) as usize;
    if node_size < 256 || extents_bytes.len() < node_size {
        return Err("The HFS+ extents-overflow B-tree node size is not usable.".into());
    }

    let mut visited_nodes = HashSet::<usize>::new();
    let mut current_node = first_leaf_node;

    while current_node != 0 {
        if !visited_nodes.insert(current_node) {
            break;
        }

        let node_offset = current_node.saturating_mul(node_size);
        if node_offset + node_size > extents_bytes.len() {
            return Err(format!(
                "The HFS+ extents-overflow leaf node {} is outside the fork bounds.",
                current_node
            ));
        }

        let node = &extents_bytes[node_offset..node_offset + node_size];
        let next_node = be_u32(&node[0..4]) as usize;
        let kind = node[8] as i8;
        let record_count = be_u16(&node[10..12]) as usize;
        if kind != HFSPLUS_LEAF_NODE_KIND {
            current_node = next_node;
            continue;
        }

        for (record_start, record_end) in leaf_node_layout(node, record_count)?.bounds {
            let record = &node[record_start..record_end];
            if let Some((key, extents)) = parse_extent_overflow_record(record)? {
                if key.file_id == expected_key.file_id
                    && key.fork_type == expected_key.fork_type
                    && key.start_block == expected_key.start_block
                {
                    return Ok(Some(extents));
                }
            }
        }

        current_node = next_node;
    }

    Ok(None)
}

fn parse_extent_overflow_record(
    record: &[u8],
) -> Result<Option<(HfsPlusExtentKey, [HfsPlusExtentDescriptor; 8])>, String> {
    if record.len() < 2 + 10 + 64 {
        return Ok(None);
    }

    let key_length = be_u16(&record[0..2]) as usize;
    let key_end = 2 + key_length;
    if key_length != 10 || key_end + 64 > record.len() {
        return Ok(None);
    }

    let key = HfsPlusExtentKey {
        fork_type: record[2],
        file_id: be_u32(&record[4..8]),
        start_block: be_u32(&record[8..12]),
    };

    let extents = array::from_fn(|index| {
        let offset = key_end + index * 8;
        HfsPlusExtentDescriptor {
            start_block: be_u32(&record[offset..offset + 4]),
            block_count: be_u32(&record[offset + 4..offset + 8]),
        }
    });

    Ok(Some((key, extents)))
}

fn parse_catalog_visible_files(
    reader: &mut File,
    layout: &HfsPlusVolumeLayout,
    catalog_bytes: &[u8],
) -> Result<Vec<HfsPlusVisibleFileCandidate>, String> {
    if catalog_bytes.len() < 34 {
        return Err("The HFS+ catalog file is too small to contain a usable B-tree header.".into());
    }

    let node_size = be_u16(&catalog_bytes[32..34]) as usize;
    let first_leaf_node = be_u32(&catalog_bytes[24..28]) as usize;
    if node_size < 256 || catalog_bytes.len() < node_size {
        return Err("The HFS+ catalog B-tree node size is not usable.".into());
    }

    let mut folders = HashMap::<u32, FolderPathNode>::new();
    let mut files = Vec::<PendingFileRecord>::new();
    let mut visited_nodes = HashSet::<usize>::new();
    let mut current_node = first_leaf_node;

    while current_node != 0 {
        if !visited_nodes.insert(current_node) {
            break;
        }

        let node_offset = current_node.saturating_mul(node_size);
        if node_offset + node_size > catalog_bytes.len() {
            return Err(format!(
                "The HFS+ catalog leaf node {} is outside the catalog fork bounds.",
                current_node
            ));
        }

        let node = &catalog_bytes[node_offset..node_offset + node_size];
        let next_node = be_u32(&node[0..4]) as usize;
        let kind = node[8] as i8;
        let record_count = be_u16(&node[10..12]) as usize;
        if kind != HFSPLUS_LEAF_NODE_KIND {
            current_node = next_node;
            continue;
        }

        for (record_start, record_end) in leaf_node_layout(node, record_count)?.bounds {
            let record = &node[record_start..record_end];
            if let Some(entry) = parse_catalog_record(reader, layout, record)? {
                match entry {
                    CatalogEntry::Folder {
                        folder_id,
                        parent_id,
                        name,
                    } => {
                        folders.insert(folder_id, FolderPathNode { name, parent_id });
                    }
                    CatalogEntry::File(file) => files.push(file),
                }
            }
        }

        current_node = next_node;
    }

    let mut visible_files = Vec::new();
    for file in files {
        let path = build_parent_path(&folders, file.parent_id);
        let extension = file_extension(&file.name);
        visible_files.push(HfsPlusVisibleFileCandidate {
            name: file.name,
            extension,
            path,
            size_bytes: file.size_bytes,
            expected_size_bytes: file.expected_size_bytes,
            created_at: file.created_at,
            modified_at: file.modified_at,
            integrity: file.integrity,
            recovery_score: file.recovery_score,
            start_offset: file.start_offset,
            byte_runs: file.byte_runs,
            resource_fork: file.resource_fork,
        });
    }

    Ok(visible_files)
}

fn leaf_node_layout(node: &[u8], record_count: usize) -> Result<LeafRecordLayout, String> {
    let node_size = node.len();
    if record_count == 0 {
        return Ok(LeafRecordLayout {
            bounds: Vec::new(),
            free_space: 14,
        });
    }
    if node_size < 14 + record_count * 2 {
        return Err("The HFS+ catalog node is too small for its advertised record count.".into());
    }

    let mut reversed_offsets = Vec::with_capacity(record_count + 1);
    for index in 0..=record_count {
        let offset_index = node_size
            .checked_sub(2 * (index + 1))
            .ok_or_else(|| "The HFS+ catalog node offset table underflowed.".to_string())?;
        reversed_offsets.push(be_u16(&node[offset_index..offset_index + 2]) as usize);
    }

    let mut bounds = Vec::with_capacity(record_count);
    for record_index in 0..record_count {
        let start = reversed_offsets[record_index];
        let end = reversed_offsets[record_index + 1];
        if start >= end || end > node_size {
            continue;
        }
        bounds.push((start, end));
    }

    Ok(LeafRecordLayout {
        bounds,
        free_space: *reversed_offsets.last().unwrap_or(&14),
    })
}

enum CatalogEntry {
    Folder {
        folder_id: u32,
        parent_id: u32,
        name: String,
    },
    File(PendingFileRecord),
}

fn parse_catalog_record(
    reader: &mut File,
    layout: &HfsPlusVolumeLayout,
    record: &[u8],
) -> Result<Option<CatalogEntry>, String> {
    if record.len() < 10 {
        return Ok(None);
    }

    let key_length = be_u16(&record[0..2]) as usize;
    let key_end = 2 + key_length;
    if key_end > record.len() || key_length < 6 {
        return Ok(None);
    }

    let parent_id = be_u32(&record[2..6]);
    let name_length = be_u16(&record[6..8]) as usize;
    let name_bytes_end = 8 + name_length * 2;
    if name_bytes_end > key_end {
        return Ok(None);
    }

    let name = decode_utf16be(&record[8..name_bytes_end])?;
    if name.is_empty() {
        return Ok(None);
    }

    let data = &record[key_end..];
    if data.len() < 2 {
        return Ok(None);
    }

    match be_u16(&data[0..2]) {
        HFSPLUS_CATALOG_RECORD_FOLDER if data.len() >= 88 => Ok(Some(CatalogEntry::Folder {
            folder_id: be_u32(&data[8..12]),
            parent_id,
            name,
        })),
        HFSPLUS_CATALOG_RECORD_FILE if data.len() >= 248 => {
            let data_fork = parse_fork_data(&data[88..168])?;
            let file_id = be_u32(&data[8..12]);
            let resource_fork = resource_fork_from_fork(
                reader,
                layout,
                file_id,
                &parse_fork_data(&data[168..248])?,
            )?;
            let (byte_runs, reconstructable_size, expected_size_bytes, integrity, recovery_score) =
                visible_byte_runs_from_fork(
                    reader,
                    layout,
                    file_id,
                    HFSPLUS_DATA_FORK_TYPE,
                    &data_fork,
                )?;
            let (size_bytes, expected_size_bytes, integrity, recovery_score, start_offset) =
                visible_hfsplus_primary_fork_profile(
                    reconstructable_size,
                    expected_size_bytes,
                    &byte_runs,
                    integrity,
                    recovery_score,
                    resource_fork.as_ref(),
                );
            if size_bytes == 0 && resource_fork.is_none() {
                return Ok(None);
            }

            Ok(Some(CatalogEntry::File(PendingFileRecord {
                file_id,
                name,
                parent_id,
                size_bytes,
                expected_size_bytes,
                created_at: hfsplus_seconds_to_iso(be_u32(&data[12..16])),
                modified_at: hfsplus_seconds_to_iso(be_u32(&data[16..20])),
                integrity,
                recovery_score,
                start_offset,
                byte_runs,
                resource_fork,
            })))
        }
        _ => Ok(None),
    }
}

fn parse_catalog_deleted_files(
    reader: &mut File,
    layout: &HfsPlusVolumeLayout,
    catalog_bytes: &[u8],
) -> Result<Vec<HfsPlusDeletedFileCandidate>, String> {
    if catalog_bytes.len() < 34 {
        return Err("The HFS+ catalog file is too small to contain a usable B-tree header.".into());
    }

    let node_size = be_u16(&catalog_bytes[32..34]) as usize;
    let first_leaf_node = be_u32(&catalog_bytes[24..28]) as usize;
    if node_size < 256 || catalog_bytes.len() < node_size {
        return Err("The HFS+ catalog B-tree node size is not usable.".into());
    }

    let mut folders = HashMap::<u32, FolderPathNode>::new();
    let mut active_files = HashSet::<CatalogFileIdentity>::new();
    let mut deleted_files = Vec::<PendingFileRecord>::new();
    let mut seen_deleted = HashSet::<CatalogFileIdentity>::new();
    let mut visited_nodes = HashSet::<usize>::new();
    let mut current_node = first_leaf_node;

    while current_node != 0 {
        if !visited_nodes.insert(current_node) {
            break;
        }

        let node_offset = current_node.saturating_mul(node_size);
        if node_offset + node_size > catalog_bytes.len() {
            return Err(format!(
                "The HFS+ catalog leaf node {} is outside the catalog fork bounds.",
                current_node
            ));
        }

        let node = &catalog_bytes[node_offset..node_offset + node_size];
        let next_node = be_u32(&node[0..4]) as usize;
        let kind = node[8] as i8;
        let record_count = be_u16(&node[10..12]) as usize;
        if kind != HFSPLUS_LEAF_NODE_KIND {
            current_node = next_node;
            continue;
        }

        let layout_info = leaf_node_layout(node, record_count)?;
        for (record_start, record_end) in &layout_info.bounds {
            let record = &node[*record_start..*record_end];
            if let Some(entry) = parse_catalog_record(reader, layout, record)? {
                match entry {
                    CatalogEntry::Folder {
                        folder_id,
                        parent_id,
                        name,
                    } => {
                        folders.insert(folder_id, FolderPathNode { name, parent_id });
                    }
                    CatalogEntry::File(file) => {
                        active_files.insert(CatalogFileIdentity {
                            file_id: file.file_id,
                            parent_id: file.parent_id,
                            name: file.name.clone(),
                        });
                    }
                }
            }
        }

        let slack_start = layout_info.free_space.min(node.len());
        let slack_end = node
            .len()
            .saturating_sub(2 * (record_count.saturating_add(1)));
        if slack_start < slack_end {
            scan_deleted_catalog_slack(
                reader,
                layout,
                &node[slack_start..slack_end],
                &active_files,
                &mut seen_deleted,
                &mut deleted_files,
            )?;
        }

        current_node = next_node;
    }

    Ok(deleted_files
        .into_iter()
        .map(|file| HfsPlusDeletedFileCandidate {
            name: file.name.clone(),
            extension: file_extension(&file.name),
            path: build_parent_path(&folders, file.parent_id),
            size_bytes: file.size_bytes,
            expected_size_bytes: file.expected_size_bytes,
            created_at: file.created_at,
            modified_at: file.modified_at,
            integrity: file.integrity,
            recovery_score: file.recovery_score,
            start_offset: file.start_offset,
            byte_runs: file.byte_runs,
            resource_fork: file.resource_fork,
        })
        .collect())
}

fn scan_deleted_catalog_slack(
    reader: &mut File,
    layout: &HfsPlusVolumeLayout,
    slack: &[u8],
    active_files: &HashSet<CatalogFileIdentity>,
    seen_deleted: &mut HashSet<CatalogFileIdentity>,
    deleted_files: &mut Vec<PendingFileRecord>,
) -> Result<(), String> {
    let minimum_record_size = 2 + 6 + 248;
    let mut cursor = 0usize;
    while cursor + minimum_record_size <= slack.len() {
        if let Some((candidate, consumed_len)) =
            parse_deleted_catalog_record(reader, layout, &slack[cursor..], active_files)?
        {
            let identity = CatalogFileIdentity {
                file_id: candidate.file_id,
                parent_id: candidate.parent_id,
                name: candidate.name.clone(),
            };
            if seen_deleted.insert(identity) {
                deleted_files.push(candidate);
            }
            cursor += consumed_len.max(1);
            continue;
        }

        cursor += 1;
    }

    Ok(())
}

fn parse_deleted_catalog_record(
    reader: &mut File,
    layout: &HfsPlusVolumeLayout,
    bytes: &[u8],
    active_files: &HashSet<CatalogFileIdentity>,
) -> Result<Option<(PendingFileRecord, usize)>, String> {
    if bytes.len() < 256 {
        return Ok(None);
    }

    let key_length = be_u16(&bytes[0..2]) as usize;
    let key_end = 2 + key_length;
    if key_length < 6 || key_end + 248 > bytes.len() {
        return Ok(None);
    }

    let parent_id = be_u32(&bytes[2..6]);
    let name_length = be_u16(&bytes[6..8]) as usize;
    let name_bytes_end = 8 + name_length * 2;
    if name_length == 0 || name_bytes_end > key_end {
        return Ok(None);
    }

    let name = decode_utf16be(&bytes[8..name_bytes_end])?;
    if !plausible_catalog_name(&name) {
        return Ok(None);
    }

    let data = &bytes[key_end..key_end + 248];
    if be_u16(&data[0..2]) != HFSPLUS_CATALOG_RECORD_FILE {
        return Ok(None);
    }

    let file_id = be_u32(&data[8..12]);
    if file_id <= HFSPLUS_ROOT_FOLDER_ID {
        return Ok(None);
    }

    let identity = CatalogFileIdentity {
        file_id,
        parent_id,
        name: name.clone(),
    };
    if active_files.contains(&identity) {
        return Ok(None);
    }

    let data_fork = parse_fork_data(&data[88..168])?;
    let resource_fork =
        resource_fork_from_fork(reader, layout, file_id, &parse_fork_data(&data[168..248])?)?;
    let (byte_runs, reconstructable_size, expected_size_bytes, integrity, recovery_score) =
        visible_byte_runs_from_fork(reader, layout, file_id, HFSPLUS_DATA_FORK_TYPE, &data_fork)?;
    let (size_bytes, expected_size_bytes, integrity, recovery_score, start_offset) =
        deleted_hfsplus_primary_fork_profile(
            reconstructable_size,
            expected_size_bytes,
            &byte_runs,
            integrity,
            recovery_score,
            resource_fork.as_ref(),
        );
    if size_bytes == 0 && resource_fork.is_none() {
        return Ok(None);
    }

    Ok(Some((
        PendingFileRecord {
            file_id,
            name,
            parent_id,
            size_bytes,
            expected_size_bytes,
            created_at: hfsplus_seconds_to_iso(be_u32(&data[12..16])),
            modified_at: hfsplus_seconds_to_iso(be_u32(&data[16..20])),
            integrity,
            recovery_score,
            start_offset,
            byte_runs,
            resource_fork,
        },
        key_end + 248,
    )))
}

fn deleted_hfsplus_recovery_profile(integrity: String, recovery_score: u8) -> (String, u8) {
    let capped_score = match integrity.as_str() {
        "intact" => recovery_score.min(68),
        "fragmented" => recovery_score.min(57),
        "partial" => recovery_score.min(46),
        _ => recovery_score.min(22),
    };
    (integrity, capped_score)
}

fn plausible_catalog_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && name
            .chars()
            .all(|character| !character.is_control() || character.is_ascii_whitespace())
}

fn visible_byte_runs_from_fork(
    reader: &mut File,
    layout: &HfsPlusVolumeLayout,
    file_id: u32,
    fork_type: u8,
    fork: &HfsPlusForkData,
) -> Result<HfsPlusForkRuns, String> {
    if fork.logical_size == 0 {
        return Ok((Vec::new(), 0, None, "intact".into(), 97));
    }

    let (extents, overflow_complete) =
        resolve_fork_extents(reader, layout, file_id, fork_type, fork)?;
    let mut remaining = fork.logical_size;
    let mut byte_runs = Vec::new();
    let mut covered_bytes = 0_u64;

    for extent in extents {
        if remaining == 0 {
            break;
        }
        if extent.block_count == 0 {
            continue;
        }

        let extent_bytes = extent.block_count as u64 * layout.block_size as u64;
        let bytes_in_run = remaining.min(extent_bytes);
        byte_runs.push(ByteRun {
            offset: layout.volume_offset + extent.start_block as u64 * layout.block_size as u64,
            length: bytes_in_run,
            zero_fill: false,
            ..Default::default()
        });
        covered_bytes = covered_bytes.saturating_add(bytes_in_run);
        remaining = remaining.saturating_sub(bytes_in_run);
    }

    if covered_bytes == 0 || byte_runs.is_empty() {
        return Ok((Vec::new(), 0, None, "corrupt".into(), 12));
    }

    let expected_size_bytes = Some(fork.logical_size);
    if covered_bytes < fork.logical_size || !overflow_complete {
        return Ok((
            byte_runs,
            covered_bytes,
            expected_size_bytes,
            "partial".into(),
            42,
        ));
    }

    if byte_runs.len() > 1 {
        return Ok((
            byte_runs,
            covered_bytes,
            expected_size_bytes,
            "fragmented".into(),
            86,
        ));
    }

    Ok((
        byte_runs,
        covered_bytes,
        expected_size_bytes,
        "intact".into(),
        96,
    ))
}

fn resource_fork_from_fork(
    reader: &mut File,
    layout: &HfsPlusVolumeLayout,
    file_id: u32,
    fork: &HfsPlusForkData,
) -> Result<Option<FileFork>, String> {
    if fork.logical_size == 0 {
        return Ok(None);
    }

    let (byte_runs, size_bytes, expected_size_bytes, integrity, _) =
        visible_byte_runs_from_fork(reader, layout, file_id, HFSPLUS_RESOURCE_FORK_TYPE, fork)?;
    if size_bytes == 0 || byte_runs.is_empty() || integrity == "corrupt" {
        return Ok(None);
    }

    Ok(Some(FileFork {
        size_bytes,
        expected_size_bytes,
        byte_runs,
    }))
}

fn visible_hfsplus_primary_fork_profile(
    size_bytes: u64,
    expected_size_bytes: Option<u64>,
    byte_runs: &[ByteRun],
    integrity: String,
    recovery_score: u8,
    resource_fork: Option<&FileFork>,
) -> (u64, Option<u64>, String, u8, Option<u64>) {
    if size_bytes > 0 || expected_size_bytes.unwrap_or(0) > 0 {
        return (
            size_bytes,
            expected_size_bytes,
            integrity,
            recovery_score,
            byte_runs.first().map(|run| run.offset),
        );
    }

    match resource_fork {
        Some(resource_fork) => {
            let (integrity, recovery_score) = standalone_resource_fork_profile(resource_fork);
            (
                0,
                Some(0),
                integrity,
                recovery_score,
                resource_fork.byte_runs.first().map(|run| run.offset),
            )
        }
        None => (0, None, integrity, recovery_score, None),
    }
}

fn deleted_hfsplus_primary_fork_profile(
    size_bytes: u64,
    expected_size_bytes: Option<u64>,
    byte_runs: &[ByteRun],
    integrity: String,
    recovery_score: u8,
    resource_fork: Option<&FileFork>,
) -> (u64, Option<u64>, String, u8, Option<u64>) {
    let (size_bytes, expected_size_bytes, integrity, recovery_score, start_offset) =
        visible_hfsplus_primary_fork_profile(
            size_bytes,
            expected_size_bytes,
            byte_runs,
            integrity,
            recovery_score,
            resource_fork,
        );
    let (integrity, recovery_score) = deleted_hfsplus_recovery_profile(integrity, recovery_score);
    (
        size_bytes,
        expected_size_bytes,
        integrity,
        recovery_score,
        start_offset,
    )
}

fn standalone_resource_fork_profile(resource_fork: &FileFork) -> (String, u8) {
    match resource_fork.expected_size_bytes {
        Some(expected) if expected > resource_fork.size_bytes => ("partial".into(), 48),
        _ if resource_fork.byte_runs.len() > 1 => ("fragmented".into(), 74),
        _ => ("intact".into(), 84),
    }
}

fn total_extent_blocks_slice(extents: &[HfsPlusExtentDescriptor]) -> u32 {
    extents.iter().fold(0_u32, |total, extent| {
        total.saturating_add(extent.block_count)
    })
}

fn build_parent_path(folders: &HashMap<u32, FolderPathNode>, parent_id: u32) -> String {
    if parent_id == HFSPLUS_ROOT_FOLDER_ID {
        return "/".into();
    }

    let mut current = parent_id;
    let mut seen = HashSet::new();
    let mut segments = Vec::new();

    while current != HFSPLUS_ROOT_FOLDER_ID {
        if !seen.insert(current) {
            break;
        }

        let node = match folders.get(&current) {
            Some(node) => node,
            None => break,
        };
        segments.push(node.name.clone());
        current = node.parent_id;
    }

    segments.reverse();
    if segments.is_empty() {
        "/".into()
    } else {
        format!("/{}", segments.join("/"))
    }
}

fn file_extension(name: &str) -> String {
    name.rsplit('.')
        .next()
        .filter(|extension| *extension != name && !extension.is_empty())
        .map(|extension| extension.to_ascii_lowercase())
        .unwrap_or_default()
}

fn decode_utf16be(bytes: &[u8]) -> Result<String, String> {
    if !bytes.len().is_multiple_of(2) {
        return Err("The HFS+ catalog key contains an odd UTF-16 byte count.".into());
    }

    let code_units = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    Ok(String::from_utf16_lossy(&code_units).trim().to_string())
}

fn hfsplus_seconds_to_iso(seconds: u32) -> Option<String> {
    if seconds == 0 {
        return None;
    }

    let unix_seconds = seconds as i64 - HFSPLUS_UNIX_EPOCH_OFFSET_SECONDS;
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

fn read_exact_at(
    reader: &mut File,
    offset: u64,
    bytes: &mut [u8],
    label: &str,
) -> Result<(), String> {
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|error| format!("Unable to seek the HFS+ {label} at offset {offset}: {error}"))?;
    reader
        .read_exact(bytes)
        .map_err(|error| format!("Unable to read the HFS+ {label} at offset {offset}: {error}"))
}

fn be_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn be_u64(bytes: &[u8]) -> u64 {
    u64::from_be_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

/// Parse HFS+ journal for additional deleted file candidates.
/// Reads the journal info block from the volume header, parses the journal
/// header to find the buffer range, then scans transaction blocks for
/// catalog B-tree leaf node snapshots containing deleted file records.
pub fn recover_journal_files(
    image_path: &std::path::Path,
) -> Result<Vec<HfsPlusDeletedFileCandidate>, String> {
    let mut reader = File::open(image_path)
        .map_err(|e| format!("Unable to open HFS+ image for journal: {e}"))?;

    // Read volume header to find journal info block
    let mut header = [0u8; 512];
    if read_exact_at(
        &mut reader,
        HFSPLUS_VOLUME_HEADER_OFFSET,
        &mut header,
        "HFS+ volume header for journal",
    )
    .is_err()
    {
        return Ok(Vec::new());
    }

    let signature = be_u16(&header[0..2]);
    if !matches!(signature, HFSPLUS_SIGNATURE | HFSX_SIGNATURE) {
        return Ok(Vec::new());
    }

    let block_size = be_u32(&header[40..44]) as u64;
    if block_size == 0 {
        return Ok(Vec::new());
    }

    // Journal info block number at offset 124 (BE u32)
    let journal_info_block = be_u32(&header[124..128]) as u64;
    if journal_info_block == 0 {
        return Ok(Vec::new()); // No journal
    }

    let journal_info_offset = journal_info_block * block_size;

    // Read journal info block (contains offset and size of journal)
    let mut ji_buf = [0u8; 180];
    if read_exact_at(
        &mut reader,
        journal_info_offset,
        &mut ji_buf,
        "HFS+ journal info",
    )
    .is_err()
    {
        return Ok(Vec::new());
    }

    // Journal info block: flags at 0 (BE u32), device_signature at 4..12
    // offset at 16 (BE u64), size at 24 (BE u64)
    let journal_offset = be_u64(&ji_buf[16..24]);
    let journal_size = be_u64(&ji_buf[24..32]);

    if journal_offset == 0 || journal_size == 0 || journal_size > 256 * 1024 * 1024 {
        return Ok(Vec::new());
    }

    // Read journal header (first 180 bytes of journal)
    let mut jh_buf = [0u8; 180];
    if read_exact_at(
        &mut reader,
        journal_offset,
        &mut jh_buf,
        "HFS+ journal header",
    )
    .is_err()
    {
        return Ok(Vec::new());
    }

    // Journal header: magic at 0 (BE u32 = 0x4a4e4c78 'JNLx')
    let journal_magic = be_u32(&jh_buf[0..4]);
    if journal_magic != 0x4a4e_4c78 {
        return Ok(Vec::new());
    }

    let _jh_start = be_u64(&jh_buf[16..24]);
    let _jh_end = be_u64(&jh_buf[24..32]);
    let jh_blhdr_size = be_u32(&jh_buf[32..36]) as u64;

    if jh_blhdr_size == 0 || jh_blhdr_size > 65536 {
        return Ok(Vec::new());
    }

    // Scan journal blocks for catalog B-tree leaf nodes
    let mut candidates = Vec::new();
    let mut seen_names = std::collections::HashSet::new();
    let scan_limit = journal_size.min(32 * 1024 * 1024); // Limit scan to 32MB

    let mut scan_offset = jh_blhdr_size; // Skip first block header
    while scan_offset < scan_limit && candidates.len() < 500 {
        let abs_offset = journal_offset + (scan_offset % journal_size);
        let read_size = block_size.min(4096) as usize;
        let mut block = vec![0u8; read_size];

        if read_exact_at(&mut reader, abs_offset, &mut block, "HFS+ journal block").is_err() {
            scan_offset += block_size;
            continue;
        }

        // Look for catalog B-tree leaf node records
        // HFS+ catalog leaf nodes have node type = 0xFF (-1 signed) at offset 8
        if block.len() >= 14 {
            let node_type = block[8] as i8;
            let num_records = be_u16(&block[10..12]);

            if node_type == -1 && num_records > 0 && num_records < 200 {
                // This looks like a catalog leaf node — parse records
                let node_size = read_size;
                for record_idx in 0..num_records as usize {
                    // Record offsets are stored at the end of the node, growing backwards
                    let offset_pos = node_size - 2 - (record_idx + 1) * 2;
                    if offset_pos + 2 > block.len() {
                        break;
                    }

                    let record_offset = be_u16(&block[offset_pos..offset_pos + 2]) as usize;
                    if record_offset + 10 > block.len() {
                        break;
                    }

                    // Catalog key: key_length (BE u16), parent_id (BE u32), name_length (BE u16)
                    let key_length = be_u16(&block[record_offset..record_offset + 2]) as usize;
                    if key_length < 6 || record_offset + 2 + key_length > block.len() {
                        continue;
                    }

                    let _parent_id = be_u32(&block[record_offset + 2..record_offset + 6]);
                    let name_length = be_u16(&block[record_offset + 6..record_offset + 8]) as usize;

                    if name_length == 0
                        || name_length > 255
                        || record_offset + 8 + name_length * 2 > block.len()
                    {
                        continue;
                    }

                    // Read UTF-16BE name
                    let name_start = record_offset + 8;
                    let name: String = (0..name_length)
                        .map(|i| {
                            let pos = name_start + i * 2;
                            if pos + 2 <= block.len() {
                                char::from_u32(be_u16(&block[pos..pos + 2]) as u32).unwrap_or('?')
                            } else {
                                '?'
                            }
                        })
                        .collect();

                    if name.starts_with('.') || name.starts_with('\0') || name.is_empty() {
                        continue;
                    }

                    // Check record type after key
                    let data_offset = record_offset + 2 + key_length;
                    if data_offset + 2 > block.len() {
                        continue;
                    }

                    let record_type = be_u16(&block[data_offset..data_offset + 2]) as i16;

                    // Record type 0x0200 = file record
                    if record_type == 0x0200 && data_offset + 88 <= block.len() {
                        // Parse file record: data fork at offset 88
                        let data_fork_offset = data_offset + 16;
                        if data_fork_offset + 80 > block.len() {
                            continue;
                        }

                        let logical_size = be_u64(&block[data_fork_offset..data_fork_offset + 8]);
                        if logical_size == 0 {
                            continue;
                        }

                        let dedup_key = format!("{}:{}", name, logical_size);
                        if seen_names.contains(&dedup_key) {
                            continue;
                        }
                        seen_names.insert(dedup_key);

                        // Extract first extent from fork
                        let ext_offset = data_fork_offset + 16;
                        if ext_offset + 8 > block.len() {
                            continue;
                        }

                        let start_block_val = be_u32(&block[ext_offset..ext_offset + 4]) as u64;
                        let block_count = be_u32(&block[ext_offset + 4..ext_offset + 8]) as u64;

                        if start_block_val > 0 && block_count > 0 {
                            let ext = name
                                .rsplit('.')
                                .next()
                                .filter(|e| e.len() <= 10)
                                .unwrap_or("bin")
                                .to_lowercase();

                            let byte_runs = vec![ByteRun::physical(
                                start_block_val * block_size,
                                (block_count * block_size).min(logical_size),
                            )];

                            let recoverable = byte_runs.iter().map(|r| r.length).sum::<u64>();

                            candidates.push(HfsPlusDeletedFileCandidate {
                                name: format!("journal-{}", name),
                                extension: ext,
                                path: "/journal-recovered".into(),
                                size_bytes: recoverable,
                                expected_size_bytes: Some(logical_size),
                                created_at: None,
                                modified_at: None,
                                integrity: if recoverable >= logical_size {
                                    "intact"
                                } else {
                                    "partial"
                                }
                                .into(),
                                recovery_score: if recoverable >= logical_size { 60 } else { 42 },
                                start_offset: Some(start_block_val * block_size),
                                byte_runs,
                                resource_fork: None,
                            });
                        }
                    }
                }
            }
        }

        scan_offset += block_size;
    }

    Ok(candidates)
}

#[cfg(test)]
pub(crate) fn synthetic_visible_hfsplus_image_for_tests(content: &[u8]) -> Vec<u8> {
    let block_size = 4096usize;
    let total_blocks = 8usize;
    let mut image = vec![0_u8; block_size * total_blocks];
    let catalog_fork = build_test_fork_data(4096, 1, 1);

    {
        let header_offset = HFSPLUS_VOLUME_HEADER_OFFSET as usize;
        let header = &mut image[header_offset..header_offset + 512];
        header[0..2].copy_from_slice(&HFSPLUS_SIGNATURE.to_be_bytes());
        header[2..4].copy_from_slice(&4_u16.to_be_bytes());
        header[32..36].copy_from_slice(&1_u32.to_be_bytes());
        header[36..40].copy_from_slice(&1_u32.to_be_bytes());
        header[40..44].copy_from_slice(&(block_size as u32).to_be_bytes());
        header[44..48].copy_from_slice(&(total_blocks as u32).to_be_bytes());
        header[272..352].copy_from_slice(&catalog_fork);
    }

    let folder_record = build_catalog_folder_record(
        HFSPLUS_ROOT_FOLDER_ID,
        "Docs",
        16,
        3_791_818_800,
        3_791_905_200,
    );
    let file_record = build_catalog_file_record(
        16,
        "Report.txt",
        17,
        3_791_818_900,
        3_791_905_260,
        HfsPlusForkData {
            logical_size: content.len() as u64,
            total_blocks: 1,
            extents: [
                HfsPlusExtentDescriptor {
                    start_block: 3,
                    block_count: 1,
                },
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
            ],
        },
        empty_fork_data(),
    );

    let catalog_block = &mut image[block_size..block_size * 2];
    write_catalog_header_node(catalog_block, 512, 1);
    write_catalog_leaf_node(&mut catalog_block[512..1024], &[folder_record, file_record]);

    let file_offset = block_size * 3;
    image[file_offset..file_offset + content.len()].copy_from_slice(content);

    image
}

#[cfg(test)]
fn build_test_fork_data(logical_size: u64, start_block: u32, block_count: u32) -> [u8; 80] {
    let mut bytes = [0_u8; 80];
    bytes[0..8].copy_from_slice(&logical_size.to_be_bytes());
    bytes[12..16].copy_from_slice(&block_count.to_be_bytes());
    bytes[16..20].copy_from_slice(&start_block.to_be_bytes());
    bytes[20..24].copy_from_slice(&block_count.to_be_bytes());
    bytes
}

#[cfg(test)]
fn build_extent_overflow_record(
    file_id: u32,
    start_block: u32,
    extents: &[HfsPlusExtentDescriptor],
) -> Vec<u8> {
    let mut record = Vec::with_capacity(2 + 10 + 64);
    record.extend_from_slice(&10_u16.to_be_bytes());
    record.push(HFSPLUS_DATA_FORK_TYPE);
    record.push(0);
    record.extend_from_slice(&file_id.to_be_bytes());
    record.extend_from_slice(&start_block.to_be_bytes());

    for index in 0..8 {
        let extent = extents.get(index).copied().unwrap_or_default();
        record.extend_from_slice(&extent.start_block.to_be_bytes());
        record.extend_from_slice(&extent.block_count.to_be_bytes());
    }

    record
}

#[cfg(test)]
fn build_catalog_key(parent_id: u32, name: &str) -> Vec<u8> {
    let code_units = name.encode_utf16().collect::<Vec<_>>();
    let key_length = (4 + 2 + code_units.len() * 2) as u16;
    let mut bytes = Vec::with_capacity(2 + key_length as usize);
    bytes.extend_from_slice(&key_length.to_be_bytes());
    bytes.extend_from_slice(&parent_id.to_be_bytes());
    bytes.extend_from_slice(&(code_units.len() as u16).to_be_bytes());
    for code_unit in code_units {
        bytes.extend_from_slice(&code_unit.to_be_bytes());
    }
    bytes
}

#[cfg(test)]
fn build_catalog_folder_record(
    parent_id: u32,
    name: &str,
    folder_id: u32,
    created_at: u32,
    modified_at: u32,
) -> Vec<u8> {
    let mut record = build_catalog_key(parent_id, name);
    let mut data = vec![0_u8; 88];
    data[0..2].copy_from_slice(&HFSPLUS_CATALOG_RECORD_FOLDER.to_be_bytes());
    data[8..12].copy_from_slice(&folder_id.to_be_bytes());
    data[12..16].copy_from_slice(&created_at.to_be_bytes());
    data[16..20].copy_from_slice(&modified_at.to_be_bytes());
    record.extend_from_slice(&data);
    record
}

#[cfg(test)]
fn build_catalog_file_record(
    parent_id: u32,
    name: &str,
    file_id: u32,
    created_at: u32,
    modified_at: u32,
    data_fork: HfsPlusForkData,
    resource_fork: HfsPlusForkData,
) -> Vec<u8> {
    let mut record = build_catalog_key(parent_id, name);
    let mut data = vec![0_u8; 248];
    data[0..2].copy_from_slice(&HFSPLUS_CATALOG_RECORD_FILE.to_be_bytes());
    data[8..12].copy_from_slice(&file_id.to_be_bytes());
    data[12..16].copy_from_slice(&created_at.to_be_bytes());
    data[16..20].copy_from_slice(&modified_at.to_be_bytes());
    write_fork_data_bytes(&mut data[88..168], &data_fork);
    write_fork_data_bytes(&mut data[168..248], &resource_fork);
    record.extend_from_slice(&data);
    record
}

#[cfg(test)]
fn write_fork_data_bytes(target: &mut [u8], fork: &HfsPlusForkData) {
    target[0..8].copy_from_slice(&fork.logical_size.to_be_bytes());
    target[12..16].copy_from_slice(&fork.total_blocks.to_be_bytes());
    for (index, extent) in fork.extents.iter().enumerate() {
        let offset = 16 + index * 8;
        target[offset..offset + 4].copy_from_slice(&extent.start_block.to_be_bytes());
        target[offset + 4..offset + 8].copy_from_slice(&extent.block_count.to_be_bytes());
    }
}

#[cfg(test)]
fn empty_fork_data() -> HfsPlusForkData {
    HfsPlusForkData {
        logical_size: 0,
        total_blocks: 0,
        extents: [HfsPlusExtentDescriptor::default(); 8],
    }
}

#[cfg(test)]
fn write_catalog_header_node(node: &mut [u8], node_size: u16, first_leaf_node: u32) {
    node[8] = 1_u8;
    node[10..12].copy_from_slice(&3_u16.to_be_bytes());
    node[16..20].copy_from_slice(&1_u32.to_be_bytes());
    node[24..28].copy_from_slice(&first_leaf_node.to_be_bytes());
    node[28..32].copy_from_slice(&first_leaf_node.to_be_bytes());
    node[32..34].copy_from_slice(&node_size.to_be_bytes());
    node[34..38].copy_from_slice(&2_u32.to_be_bytes());
    node[38..42].copy_from_slice(&0_u32.to_be_bytes());
}

#[cfg(test)]
fn write_catalog_leaf_node(node: &mut [u8], records: &[Vec<u8>]) {
    node[8] = HFSPLUS_LEAF_NODE_KIND as u8;
    node[9] = 1;
    node[10..12].copy_from_slice(&(records.len() as u16).to_be_bytes());

    let mut cursor = 14usize;
    let mut starts = Vec::with_capacity(records.len());
    for record in records {
        starts.push(cursor);
        node[cursor..cursor + record.len()].copy_from_slice(record);
        cursor += record.len();
    }

    let free_space = cursor as u16;
    let mut table_index = node.len() - 2;
    for start in &starts {
        node[table_index..table_index + 2].copy_from_slice(&(*start as u16).to_be_bytes());
        table_index -= 2;
    }
    node[table_index..table_index + 2].copy_from_slice(&free_space.to_be_bytes());
}

#[cfg(test)]
fn write_catalog_leaf_node_with_stale_records(
    node: &mut [u8],
    active_records: &[Vec<u8>],
    stale_records: &[Vec<u8>],
) {
    write_catalog_leaf_node(node, active_records);

    let record_count = active_records.len();
    let table_start = node.len() - 2 * (record_count + 1);
    let mut free_space = be_u16(&node[table_start..table_start + 2]) as usize;

    for record in stale_records {
        let end = free_space + record.len();
        node[free_space..end].copy_from_slice(record);
        free_space = end;
    }
}

#[cfg(test)]
pub(crate) fn synthetic_deleted_hfsplus_image_for_tests(
    visible_content: &[u8],
    deleted_content: &[u8],
) -> Vec<u8> {
    let block_size = 4096usize;
    let total_blocks = 8usize;
    let mut image = vec![0_u8; block_size * total_blocks];
    let catalog_fork = build_test_fork_data(4096, 1, 1);

    {
        let header_offset = HFSPLUS_VOLUME_HEADER_OFFSET as usize;
        let header = &mut image[header_offset..header_offset + 512];
        header[0..2].copy_from_slice(&HFSPLUS_SIGNATURE.to_be_bytes());
        header[2..4].copy_from_slice(&4_u16.to_be_bytes());
        header[32..36].copy_from_slice(&1_u32.to_be_bytes());
        header[36..40].copy_from_slice(&1_u32.to_be_bytes());
        header[40..44].copy_from_slice(&(block_size as u32).to_be_bytes());
        header[44..48].copy_from_slice(&(total_blocks as u32).to_be_bytes());
        header[272..352].copy_from_slice(&catalog_fork);
    }

    let folder_record = build_catalog_folder_record(
        HFSPLUS_ROOT_FOLDER_ID,
        "Docs",
        16,
        3_791_818_800,
        3_791_905_200,
    );
    let visible_file_record = build_catalog_file_record(
        16,
        "Report.txt",
        17,
        3_791_818_900,
        3_791_905_260,
        HfsPlusForkData {
            logical_size: visible_content.len() as u64,
            total_blocks: 1,
            extents: [
                HfsPlusExtentDescriptor {
                    start_block: 3,
                    block_count: 1,
                },
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
            ],
        },
        empty_fork_data(),
    );
    let deleted_file_record = build_catalog_file_record(
        16,
        "Deleted.txt",
        18,
        3_791_819_000,
        3_791_905_320,
        HfsPlusForkData {
            logical_size: deleted_content.len() as u64,
            total_blocks: 1,
            extents: [
                HfsPlusExtentDescriptor {
                    start_block: 4,
                    block_count: 1,
                },
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
            ],
        },
        empty_fork_data(),
    );

    let catalog_block = &mut image[block_size..block_size * 2];
    write_catalog_header_node(catalog_block, 1024, 1);
    write_catalog_leaf_node_with_stale_records(
        &mut catalog_block[1024..2048],
        &[folder_record, visible_file_record],
        &[deleted_file_record],
    );

    let visible_offset = block_size * 3;
    image[visible_offset..visible_offset + visible_content.len()].copy_from_slice(visible_content);
    let deleted_offset = block_size * 4;
    image[deleted_offset..deleted_offset + deleted_content.len()].copy_from_slice(deleted_content);

    image
}

#[cfg(test)]
pub(crate) fn synthetic_deleted_hfsplus_overflow_image_for_tests() -> Vec<u8> {
    let block_size = 4096usize;
    let total_blocks = 24usize;
    let mut image = vec![0_u8; block_size * total_blocks];
    let extents_fork = build_test_fork_data(2048, 1, 1);
    let catalog_fork = build_test_fork_data(4096, 2, 1);

    {
        let header_offset = HFSPLUS_VOLUME_HEADER_OFFSET as usize;
        let header = &mut image[header_offset..header_offset + 512];
        header[0..2].copy_from_slice(&HFSPLUS_SIGNATURE.to_be_bytes());
        header[2..4].copy_from_slice(&4_u16.to_be_bytes());
        header[32..36].copy_from_slice(&2_u32.to_be_bytes());
        header[36..40].copy_from_slice(&1_u32.to_be_bytes());
        header[40..44].copy_from_slice(&(block_size as u32).to_be_bytes());
        header[44..48].copy_from_slice(&(total_blocks as u32).to_be_bytes());
        header[192..272].copy_from_slice(&extents_fork);
        header[272..352].copy_from_slice(&catalog_fork);
    }

    let visible_inline_extents = [
        HfsPlusExtentDescriptor {
            start_block: 4,
            block_count: 1,
        },
        HfsPlusExtentDescriptor {
            start_block: 6,
            block_count: 1,
        },
        HfsPlusExtentDescriptor {
            start_block: 8,
            block_count: 1,
        },
        HfsPlusExtentDescriptor {
            start_block: 10,
            block_count: 1,
        },
        HfsPlusExtentDescriptor {
            start_block: 12,
            block_count: 1,
        },
        HfsPlusExtentDescriptor {
            start_block: 14,
            block_count: 1,
        },
        HfsPlusExtentDescriptor {
            start_block: 16,
            block_count: 1,
        },
        HfsPlusExtentDescriptor {
            start_block: 18,
            block_count: 1,
        },
    ];
    let deleted_inline_extents = [
        HfsPlusExtentDescriptor {
            start_block: 5,
            block_count: 1,
        },
        HfsPlusExtentDescriptor {
            start_block: 7,
            block_count: 1,
        },
        HfsPlusExtentDescriptor {
            start_block: 9,
            block_count: 1,
        },
        HfsPlusExtentDescriptor {
            start_block: 11,
            block_count: 1,
        },
        HfsPlusExtentDescriptor {
            start_block: 13,
            block_count: 1,
        },
        HfsPlusExtentDescriptor {
            start_block: 15,
            block_count: 1,
        },
        HfsPlusExtentDescriptor {
            start_block: 17,
            block_count: 1,
        },
        HfsPlusExtentDescriptor {
            start_block: 19,
            block_count: 1,
        },
    ];

    let folder_record = build_catalog_folder_record(
        HFSPLUS_ROOT_FOLDER_ID,
        "Docs",
        16,
        3_791_818_800,
        3_791_905_200,
    );
    let visible_file_record = build_catalog_file_record(
        16,
        "Overflow.txt",
        17,
        3_791_818_900,
        3_791_905_260,
        HfsPlusForkData {
            logical_size: (8 * block_size + 13) as u64,
            total_blocks: 9,
            extents: visible_inline_extents,
        },
        empty_fork_data(),
    );
    let deleted_file_record = build_catalog_file_record(
        16,
        "DeletedOverflow.txt",
        18,
        3_791_819_000,
        3_791_905_320,
        HfsPlusForkData {
            logical_size: (8 * block_size + 15) as u64,
            total_blocks: 9,
            extents: deleted_inline_extents,
        },
        empty_fork_data(),
    );

    let extents_block = &mut image[block_size..block_size * 2];
    write_catalog_header_node(extents_block, 1024, 1);
    write_catalog_leaf_node(
        &mut extents_block[1024..2048],
        &[
            build_extent_overflow_record(
                17,
                8,
                &[HfsPlusExtentDescriptor {
                    start_block: 20,
                    block_count: 1,
                }],
            ),
            build_extent_overflow_record(
                18,
                8,
                &[HfsPlusExtentDescriptor {
                    start_block: 21,
                    block_count: 1,
                }],
            ),
        ],
    );

    let catalog_block = &mut image[block_size * 2..block_size * 3];
    write_catalog_header_node(catalog_block, 1024, 1);
    write_catalog_leaf_node_with_stale_records(
        &mut catalog_block[1024..2048],
        &[folder_record, visible_file_record],
        &[deleted_file_record],
    );

    for block in [4usize, 6, 8, 10, 12, 14, 16, 18] {
        let start = block * block_size;
        image[start..start + block_size].fill(b'V');
    }
    image[20 * block_size..20 * block_size + 13].copy_from_slice(b"VISIBLE-TAIL!");

    for block in [5usize, 7, 9, 11, 13, 15, 17, 19] {
        let start = block * block_size;
        image[start..start + block_size].fill(b'D');
    }
    image[21 * block_size..21 * block_size + 15].copy_from_slice(b"DELETED-TAIL!!!");

    image
}

#[cfg(test)]
pub(crate) fn synthetic_deleted_hfsplus_resource_fork_image_for_tests(
    visible_content: &[u8],
    visible_resource_fork: &[u8],
    deleted_content: &[u8],
    deleted_resource_fork: &[u8],
) -> Vec<u8> {
    let block_size = 4096usize;
    let total_blocks = 10usize;
    let mut image = vec![0_u8; block_size * total_blocks];
    let catalog_fork = build_test_fork_data(4096, 1, 1);

    {
        let header_offset = HFSPLUS_VOLUME_HEADER_OFFSET as usize;
        let header = &mut image[header_offset..header_offset + 512];
        header[0..2].copy_from_slice(&HFSPLUS_SIGNATURE.to_be_bytes());
        header[2..4].copy_from_slice(&4_u16.to_be_bytes());
        header[32..36].copy_from_slice(&1_u32.to_be_bytes());
        header[36..40].copy_from_slice(&1_u32.to_be_bytes());
        header[40..44].copy_from_slice(&(block_size as u32).to_be_bytes());
        header[44..48].copy_from_slice(&(total_blocks as u32).to_be_bytes());
        header[272..352].copy_from_slice(&catalog_fork);
    }

    let folder_record = build_catalog_folder_record(
        HFSPLUS_ROOT_FOLDER_ID,
        "Docs",
        16,
        3_791_818_800,
        3_791_905_200,
    );
    let visible_file_record = build_catalog_file_record(
        16,
        "Report.txt",
        17,
        3_791_818_900,
        3_791_905_260,
        HfsPlusForkData {
            logical_size: visible_content.len() as u64,
            total_blocks: 1,
            extents: [
                HfsPlusExtentDescriptor {
                    start_block: 3,
                    block_count: 1,
                },
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
            ],
        },
        HfsPlusForkData {
            logical_size: visible_resource_fork.len() as u64,
            total_blocks: if visible_resource_fork.is_empty() {
                0
            } else {
                1
            },
            extents: [
                HfsPlusExtentDescriptor {
                    start_block: 5,
                    block_count: if visible_resource_fork.is_empty() {
                        0
                    } else {
                        1
                    },
                },
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
            ],
        },
    );
    let deleted_file_record = build_catalog_file_record(
        16,
        "Deleted.txt",
        18,
        3_791_819_000,
        3_791_905_320,
        HfsPlusForkData {
            logical_size: deleted_content.len() as u64,
            total_blocks: 1,
            extents: [
                HfsPlusExtentDescriptor {
                    start_block: 4,
                    block_count: 1,
                },
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
            ],
        },
        HfsPlusForkData {
            logical_size: deleted_resource_fork.len() as u64,
            total_blocks: if deleted_resource_fork.is_empty() {
                0
            } else {
                1
            },
            extents: [
                HfsPlusExtentDescriptor {
                    start_block: 6,
                    block_count: if deleted_resource_fork.is_empty() {
                        0
                    } else {
                        1
                    },
                },
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
                HfsPlusExtentDescriptor::default(),
            ],
        },
    );

    let catalog_block = &mut image[block_size..block_size * 2];
    write_catalog_header_node(catalog_block, 1024, 1);
    write_catalog_leaf_node_with_stale_records(
        &mut catalog_block[1024..2048],
        &[folder_record, visible_file_record],
        &[deleted_file_record],
    );

    let visible_offset = block_size * 3;
    image[visible_offset..visible_offset + visible_content.len()].copy_from_slice(visible_content);
    let deleted_offset = block_size * 4;
    image[deleted_offset..deleted_offset + deleted_content.len()].copy_from_slice(deleted_content);

    if !visible_resource_fork.is_empty() {
        let visible_resource_offset = block_size * 5;
        image[visible_resource_offset..visible_resource_offset + visible_resource_fork.len()]
            .copy_from_slice(visible_resource_fork);
    }
    if !deleted_resource_fork.is_empty() {
        let deleted_resource_offset = block_size * 6;
        image[deleted_resource_offset..deleted_resource_offset + deleted_resource_fork.len()]
            .copy_from_slice(deleted_resource_fork);
    }

    image
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs};

    #[test]
    fn list_visible_files_reads_hfsplus_catalog_records() {
        let image_path = temp_fixture_path("hfsplus-visible.img");
        fs::write(
            &image_path,
            synthetic_visible_hfsplus_image_for_tests(b"hello hfs"),
        )
        .expect("HFS+ fixture should be written");

        let files = list_visible_files(&image_path).expect("HFS+ visible files should be listed");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "Report.txt");
        assert_eq!(files[0].path, "/Docs");
        assert_eq!(files[0].extension, "txt");
        assert_eq!(files[0].size_bytes, 9);
        assert_eq!(files[0].expected_size_bytes, Some(9));
        assert_eq!(files[0].integrity, "intact");
        assert_eq!(files[0].start_offset, Some(12_288));
        assert!(files[0].created_at.is_some());
        assert!(files[0].modified_at.is_some());

        let _ = fs::remove_file(image_path);
    }

    #[test]
    fn recover_deleted_files_reads_catalog_slack_records() {
        let image_path = temp_fixture_path("hfsplus-deleted.img");
        fs::write(
            &image_path,
            synthetic_deleted_hfsplus_image_for_tests(b"hello hfs", b"deleted hfs"),
        )
        .expect("HFS+ deleted fixture should be written");

        let files =
            recover_deleted_files(&image_path).expect("HFS+ deleted candidates should be listed");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "Deleted.txt");
        assert_eq!(files[0].path, "/Docs");
        assert_eq!(files[0].extension, "txt");
        assert_eq!(files[0].size_bytes, 11);
        assert_eq!(files[0].expected_size_bytes, Some(11));
        assert_eq!(files[0].integrity, "intact");
        assert_eq!(files[0].start_offset, Some(16_384));
        assert!(files[0].created_at.is_some());
        assert!(files[0].modified_at.is_some());

        let _ = fs::remove_file(image_path);
    }

    #[test]
    fn list_visible_files_reads_hfsplus_overflow_extents() {
        let image_path = temp_fixture_path("hfsplus-visible-overflow.img");
        fs::write(
            &image_path,
            synthetic_deleted_hfsplus_overflow_image_for_tests(),
        )
        .expect("HFS+ overflow fixture should be written");

        let files = list_visible_files(&image_path).expect("HFS+ overflow files should be listed");
        let visible = files
            .iter()
            .find(|file| file.name == "Overflow.txt")
            .expect("visible HFS+ overflow file should be present");

        assert_eq!(visible.path, "/Docs");
        assert_eq!(visible.size_bytes, (8 * 4096 + 13) as u64);
        assert_eq!(visible.expected_size_bytes, Some((8 * 4096 + 13) as u64));
        assert_eq!(visible.integrity, "fragmented");
        assert_eq!(visible.start_offset, Some(4 * 4096));
        assert!(visible.byte_runs.len() >= 9);

        let _ = fs::remove_file(image_path);
    }

    #[test]
    fn recover_deleted_files_reads_hfsplus_overflow_extents() {
        let image_path = temp_fixture_path("hfsplus-deleted-overflow.img");
        fs::write(
            &image_path,
            synthetic_deleted_hfsplus_overflow_image_for_tests(),
        )
        .expect("HFS+ deleted overflow fixture should be written");

        let files = recover_deleted_files(&image_path)
            .expect("HFS+ deleted overflow candidates should be listed");
        let deleted = files
            .iter()
            .find(|file| file.name == "DeletedOverflow.txt")
            .expect("deleted HFS+ overflow file should be present");

        assert_eq!(deleted.path, "/Docs");
        assert_eq!(deleted.size_bytes, (8 * 4096 + 15) as u64);
        assert_eq!(deleted.expected_size_bytes, Some((8 * 4096 + 15) as u64));
        assert_eq!(deleted.integrity, "fragmented");
        assert_eq!(deleted.start_offset, Some(5 * 4096));
        assert!(deleted.byte_runs.len() >= 9);

        let _ = fs::remove_file(image_path);
    }

    #[test]
    fn list_visible_files_surfaces_hfsplus_resource_fork_metadata() {
        let image_path = temp_fixture_path("hfsplus-visible-resource-fork.img");
        fs::write(
            &image_path,
            synthetic_deleted_hfsplus_resource_fork_image_for_tests(
                b"hello hfs",
                b"visible-rsrc",
                b"deleted hfs",
                b"deleted-rsrc",
            ),
        )
        .expect("HFS+ resource fork fixture should be written");

        let files = list_visible_files(&image_path).expect("HFS+ visible files should be listed");
        let visible = files
            .iter()
            .find(|file| file.name == "Report.txt")
            .expect("visible HFS+ file should be present");

        let resource_fork = visible
            .resource_fork
            .as_ref()
            .expect("visible resource fork should be present");
        assert_eq!(resource_fork.size_bytes, 12);
        assert_eq!(resource_fork.expected_size_bytes, Some(12));
        assert_eq!(resource_fork.byte_runs.len(), 1);
        assert_eq!(resource_fork.byte_runs[0].offset, 5 * 4096);

        let _ = fs::remove_file(image_path);
    }

    #[test]
    fn recover_deleted_files_surfaces_hfsplus_resource_fork_metadata() {
        let image_path = temp_fixture_path("hfsplus-deleted-resource-fork.img");
        fs::write(
            &image_path,
            synthetic_deleted_hfsplus_resource_fork_image_for_tests(
                b"hello hfs",
                b"visible-rsrc",
                b"deleted hfs",
                b"deleted-rsrc",
            ),
        )
        .expect("HFS+ deleted resource fork fixture should be written");

        let files =
            recover_deleted_files(&image_path).expect("HFS+ deleted candidates should be listed");
        let deleted = files
            .iter()
            .find(|file| file.name == "Deleted.txt")
            .expect("deleted HFS+ file should be present");

        let resource_fork = deleted
            .resource_fork
            .as_ref()
            .expect("deleted resource fork should be present");
        assert_eq!(resource_fork.size_bytes, 12);
        assert_eq!(resource_fork.expected_size_bytes, Some(12));
        assert_eq!(resource_fork.byte_runs.len(), 1);
        assert_eq!(resource_fork.byte_runs[0].offset, 6 * 4096);

        let _ = fs::remove_file(image_path);
    }

    #[test]
    fn inspect_volume_layout_accepts_direct_hfsplus_headers() {
        let image_path = temp_fixture_path("hfsplus-layout.img");
        fs::write(
            &image_path,
            synthetic_visible_hfsplus_image_for_tests(b"layout"),
        )
        .expect("HFS+ fixture should be written");

        let mut reader = File::open(&image_path).expect("fixture should open");
        let size = reader.metadata().expect("metadata should exist").len();
        let layout = inspect_volume_layout(&mut reader, 0, Some(size))
            .expect("volume inspection should work")
            .expect("direct HFS+ layout should be found");
        assert_eq!(layout.volume_offset, 0);
        assert_eq!(layout.block_size, 4096);
        assert_eq!(layout.total_blocks, 8);
        assert!(!layout.wrapped);

        let _ = fs::remove_file(image_path);
    }

    fn temp_fixture_path(name: &str) -> std::path::PathBuf {
        env::temp_dir().join(format!("recupere-{}-{}", std::process::id(), name))
    }
}
