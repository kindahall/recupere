use crate::types::ByteRun;
use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const EXT4_SUPERBLOCK_OFFSET: u64 = 1024;
const EXT4_SUPERBLOCK_MAGIC: u16 = 0xef53;
const EXT4_GOOD_OLD_INODE_SIZE: u16 = 128;
const EXT4_INODE_FLAG_EXTENTS: u32 = 0x0008_0000;
const EXT4_EXTENT_HEADER_MAGIC: u16 = 0xf30a;
const EXT4_S_IFREG: u16 = 0x8000;
const EXT4_MAX_EXTENT_TREE_DEPTH: u16 = 5;

#[derive(Debug, Clone)]
pub struct Ext4DeletedFileCandidate {
    pub name: String,
    pub extension: String,
    pub path: String,
    pub size_bytes: u64,
    pub expected_size_bytes: u64,
    pub modified_at: Option<String>,
    pub deleted_at: Option<String>,
    pub integrity: String,
    pub recovery_score: u8,
    pub start_offset: u64,
    pub clusters: Vec<u32>,
    pub byte_runs: Vec<ByteRun>,
}

#[derive(Debug, Clone)]
struct Ext4Superblock {
    blocks_count: u64,
    first_data_block: u64,
    block_size: u64,
    blocks_per_group: u64,
    inodes_per_group: u32,
    inode_size: u16,
    first_nonreserved_inode: u32,
    group_descriptor_size: u16,
}

impl Ext4Superblock {
    fn read_from(reader: &mut File) -> Result<Self, String> {
        let mut bytes = [0_u8; 1024];
        reader
            .seek(SeekFrom::Start(EXT4_SUPERBLOCK_OFFSET))
            .map_err(|error| format!("Unable to seek the ext4 superblock: {error}"))?;
        reader
            .read_exact(&mut bytes)
            .map_err(|error| format!("Unable to read the ext4 superblock: {error}"))?;

        if le_u16(&bytes[56..58]) != EXT4_SUPERBLOCK_MAGIC {
            return Err("The image does not expose a valid ext4 superblock.".into());
        }

        let blocks_count = le_u32(&bytes[4..8]) as u64;
        let first_data_block = le_u32(&bytes[20..24]) as u64;
        let block_size = 1024_u64 << le_u32(&bytes[24..28]);
        let blocks_per_group = le_u32(&bytes[32..36]) as u64;
        let inodes_per_group = le_u32(&bytes[40..44]);
        let inode_size = match le_u16(&bytes[88..90]) {
            0 => EXT4_GOOD_OLD_INODE_SIZE,
            value => value,
        };
        let first_nonreserved_inode = le_u32(&bytes[84..88]).max(1);
        let group_descriptor_size = match le_u16(&bytes[254..256]) {
            0 => 32,
            value if value < 32 => 32,
            value => value,
        };

        if blocks_count == 0 || blocks_per_group == 0 || inodes_per_group == 0 {
            return Err("The ext4 image does not contain a usable block-group layout.".into());
        }

        Ok(Self {
            blocks_count,
            first_data_block,
            block_size,
            blocks_per_group,
            inodes_per_group,
            inode_size,
            first_nonreserved_inode,
            group_descriptor_size,
        })
    }

    fn group_count(&self) -> u32 {
        let data_blocks = self.blocks_count.saturating_sub(self.first_data_block);
        div_ceil_u64(data_blocks.max(1), self.blocks_per_group.max(1)) as u32
    }

    fn group_descriptor_table_offset(&self) -> u64 {
        if self.block_size == 1024 {
            2048
        } else {
            self.block_size
        }
    }

    fn block_offset(&self, block: u64) -> Result<u64, String> {
        if block < self.first_data_block || block >= self.blocks_count {
            return Err(format!("ext4 block {block} is outside the image bounds."));
        }
        Ok(block.saturating_mul(self.block_size))
    }
}

#[derive(Debug, Clone)]
struct BlockGroupDescriptor {
    block_bitmap_block: u64,
    inode_bitmap_block: u64,
    inode_table_block: u64,
}

#[derive(Debug, Clone)]
struct Ext4Layout {
    superblock: Ext4Superblock,
    groups: Vec<BlockGroupDescriptor>,
    block_bitmaps: Vec<Vec<u8>>,
    inode_bitmaps: Vec<Vec<u8>>,
}

#[derive(Debug, Clone)]
struct Ext4Inode {
    inode_number: u32,
    mode: u16,
    size_bytes: u64,
    modified_at: Option<String>,
    deleted_at: Option<String>,
    flags: u32,
    block_bytes: [u8; 60],
}

impl Ext4Inode {
    fn is_regular_file(&self) -> bool {
        (self.mode & 0xf000) == EXT4_S_IFREG
    }
}

#[derive(Debug, Clone)]
struct LogicalBlockPointer {
    logical_index: u32,
    physical_block: u64,
}

#[derive(Debug, Clone, Copy)]
struct ExtentHeader {
    entries: usize,
    depth: u16,
}

pub fn recover_deleted_files(image_path: &Path) -> Result<Vec<Ext4DeletedFileCandidate>, String> {
    let mut reader = File::open(image_path).map_err(|error| {
        format!(
            "Unable to open the ext4 image {}: {}",
            image_path.to_string_lossy(),
            error
        )
    })?;

    let layout = load_layout(&mut reader)?;
    let mut deleted_files = Vec::new();

    for group_index in 0..layout.groups.len() {
        let group_descriptor = &layout.groups[group_index];
        for inode_slot in 0..layout.superblock.inodes_per_group {
            let inode_number =
                group_index as u32 * layout.superblock.inodes_per_group + inode_slot + 1;
            if inode_number < layout.superblock.first_nonreserved_inode {
                continue;
            }

            if inode_is_allocated(&layout, inode_number)? {
                continue;
            }

            let inode = read_inode(
                &mut reader,
                &layout.superblock,
                group_descriptor,
                inode_number,
                inode_slot,
            )?;

            if !inode.is_regular_file() || inode.size_bytes == 0 || inode.deleted_at.is_none() {
                continue;
            }

            if let Some(candidate) = build_deleted_candidate(&mut reader, &layout, &inode)? {
                deleted_files.push(candidate);
            }
        }
    }

    Ok(deleted_files)
}

fn load_layout(reader: &mut File) -> Result<Ext4Layout, String> {
    let superblock = Ext4Superblock::read_from(reader)?;
    let group_count = superblock.group_count();
    let descriptor_offset = superblock.group_descriptor_table_offset();
    let descriptor_size = superblock.group_descriptor_size as usize;
    let block_bitmap_bytes = superblock.blocks_per_group.div_ceil(8) as usize;
    let inode_bitmap_bytes = superblock.inodes_per_group.div_ceil(8) as usize;

    let mut groups = Vec::with_capacity(group_count as usize);
    let mut block_bitmaps = Vec::with_capacity(group_count as usize);
    let mut inode_bitmaps = Vec::with_capacity(group_count as usize);

    for group_index in 0..group_count {
        let entry_offset = descriptor_offset + group_index as u64 * descriptor_size as u64;
        let mut descriptor = vec![0_u8; descriptor_size];
        reader
            .seek(SeekFrom::Start(entry_offset))
            .map_err(|error| {
                format!("Unable to seek ext4 group descriptor {group_index}: {error}")
            })?;
        reader.read_exact(&mut descriptor).map_err(|error| {
            format!("Unable to read ext4 group descriptor {group_index}: {error}")
        })?;

        let group = BlockGroupDescriptor {
            block_bitmap_block: le_u32(&descriptor[0..4]) as u64,
            inode_bitmap_block: le_u32(&descriptor[4..8]) as u64,
            inode_table_block: le_u32(&descriptor[8..12]) as u64,
        };

        let mut block_bitmap = vec![0_u8; block_bitmap_bytes];
        read_exact_at(
            reader,
            superblock.block_offset(group.block_bitmap_block)?,
            &mut block_bitmap,
            "ext4 block bitmap",
        )?;

        let mut inode_bitmap = vec![0_u8; inode_bitmap_bytes];
        read_exact_at(
            reader,
            superblock.block_offset(group.inode_bitmap_block)?,
            &mut inode_bitmap,
            "ext4 inode bitmap",
        )?;

        groups.push(group);
        block_bitmaps.push(block_bitmap);
        inode_bitmaps.push(inode_bitmap);
    }

    Ok(Ext4Layout {
        superblock,
        groups,
        block_bitmaps,
        inode_bitmaps,
    })
}

fn read_inode(
    reader: &mut File,
    superblock: &Ext4Superblock,
    group: &BlockGroupDescriptor,
    inode_number: u32,
    inode_slot: u32,
) -> Result<Ext4Inode, String> {
    let inode_size = superblock.inode_size.max(EXT4_GOOD_OLD_INODE_SIZE) as usize;
    let inode_table_offset = superblock.block_offset(group.inode_table_block)?;
    let inode_offset = inode_table_offset + inode_slot as u64 * inode_size as u64;
    let mut bytes = vec![0_u8; inode_size];
    read_exact_at(reader, inode_offset, &mut bytes, "ext4 inode")?;

    let mut block_bytes = [0_u8; 60];
    block_bytes.copy_from_slice(&bytes[40..100]);

    let size_high = if inode_size >= 112 {
        le_u32(&bytes[108..112]) as u64
    } else {
        0
    };

    Ok(Ext4Inode {
        inode_number,
        mode: le_u16(&bytes[0..2]),
        size_bytes: (size_high << 32) | le_u32(&bytes[4..8]) as u64,
        modified_at: unix_seconds_to_iso(le_u32(&bytes[16..20])),
        deleted_at: unix_seconds_to_iso(le_u32(&bytes[20..24])),
        flags: le_u32(&bytes[32..36]),
        block_bytes,
    })
}

fn inode_is_allocated(layout: &Ext4Layout, inode_number: u32) -> Result<bool, String> {
    let zero_based = inode_number.saturating_sub(1);
    let group_index = zero_based / layout.superblock.inodes_per_group;
    let local_index = zero_based % layout.superblock.inodes_per_group;
    let bitmap = layout
        .inode_bitmaps
        .get(group_index as usize)
        .ok_or_else(|| format!("Missing ext4 inode bitmap for group {group_index}."))?;
    Ok(bitmap_bit_is_set(bitmap, local_index as usize))
}

fn block_is_free(layout: &Ext4Layout, block: u64) -> Result<bool, String> {
    if block < layout.superblock.first_data_block || block >= layout.superblock.blocks_count {
        return Err(format!(
            "ext4 block {block} is outside the allocation bitmap."
        ));
    }

    let relative_block = block - layout.superblock.first_data_block;
    let group_index = relative_block / layout.superblock.blocks_per_group;
    let local_index = (relative_block % layout.superblock.blocks_per_group) as usize;
    let bitmap = layout
        .block_bitmaps
        .get(group_index as usize)
        .ok_or_else(|| format!("Missing ext4 block bitmap for group {group_index}."))?;
    Ok(!bitmap_bit_is_set(bitmap, local_index))
}

fn build_deleted_candidate(
    reader: &mut File,
    layout: &Ext4Layout,
    inode: &Ext4Inode,
) -> Result<Option<Ext4DeletedFileCandidate>, String> {
    let logical_blocks = parse_logical_blocks(reader, &layout.superblock, inode)?;
    if logical_blocks.is_empty() {
        return Ok(None);
    }

    let mut remaining_bytes = inode.size_bytes;
    let mut recoverable_bytes = 0_u64;
    let mut byte_runs = Vec::new();
    let mut clusters = Vec::new();
    let mut logical_blocks = logical_blocks;
    logical_blocks.sort_by_key(|pointer| pointer.logical_index);

    for pointer in logical_blocks {
        if remaining_bytes == 0 {
            break;
        }

        if !block_is_free(layout, pointer.physical_block)? {
            break;
        }

        let length = remaining_bytes.min(layout.superblock.block_size);
        byte_runs.push(ByteRun {
            offset: layout.superblock.block_offset(pointer.physical_block)?,
            length,
            zero_fill: false,
            ..Default::default()
        });
        recoverable_bytes += length;
        remaining_bytes = remaining_bytes.saturating_sub(length);
        clusters.push(pointer.physical_block as u32);
    }

    if byte_runs.is_empty() {
        return Ok(None);
    }

    let preview_bytes = read_byte_runs_prefix(reader, &byte_runs, 1024)?;
    let extension = infer_extension(&preview_bytes);
    let name = format!("inode-{:06}.{}", inode.inode_number, extension);

    Ok(Some(Ext4DeletedFileCandidate {
        name,
        extension,
        path: "/orphaned-inodes".into(),
        size_bytes: recoverable_bytes,
        expected_size_bytes: inode.size_bytes,
        modified_at: inode.modified_at.clone(),
        deleted_at: inode.deleted_at.clone(),
        integrity: if recoverable_bytes == inode.size_bytes {
            "intact".into()
        } else {
            "partial".into()
        },
        recovery_score: if recoverable_bytes == inode.size_bytes {
            72
        } else {
            51
        },
        start_offset: byte_runs[0].offset,
        clusters,
        byte_runs,
    }))
}

fn parse_logical_blocks(
    reader: &mut File,
    superblock: &Ext4Superblock,
    inode: &Ext4Inode,
) -> Result<Vec<LogicalBlockPointer>, String> {
    if inode.flags & EXT4_INODE_FLAG_EXTENTS != 0 {
        return parse_extent_tree(reader, superblock, &inode.block_bytes);
    }

    let mut blocks = Vec::new();
    for (index, chunk) in inode.block_bytes[..48].chunks_exact(4).enumerate() {
        let block = le_u32(chunk) as u64;
        if block == 0 {
            break;
        }
        blocks.push(LogicalBlockPointer {
            logical_index: index as u32,
            physical_block: block,
        });
    }
    Ok(blocks)
}

fn parse_extent_tree(
    reader: &mut File,
    superblock: &Ext4Superblock,
    bytes: &[u8; 60],
) -> Result<Vec<LogicalBlockPointer>, String> {
    let mut blocks = Vec::new();
    let mut visited_leaf_blocks = HashSet::new();
    collect_extent_tree(
        reader,
        superblock,
        bytes,
        "inode extent root",
        &mut visited_leaf_blocks,
        &mut blocks,
    )?;
    Ok(blocks)
}

fn collect_extent_tree(
    reader: &mut File,
    superblock: &Ext4Superblock,
    bytes: &[u8],
    label: &str,
    visited_leaf_blocks: &mut HashSet<u64>,
    blocks: &mut Vec<LogicalBlockPointer>,
) -> Result<(), String> {
    let header = parse_extent_header(bytes, label)?;
    if header.depth > EXT4_MAX_EXTENT_TREE_DEPTH {
        return Err(format!(
            "The ext4 {label} exceeds the supported extent-tree depth {}.",
            EXT4_MAX_EXTENT_TREE_DEPTH
        ));
    }

    if header.depth == 0 {
        append_leaf_extent_blocks(bytes, header, label, blocks)?;
        return Ok(());
    }

    for index in 0..header.entries {
        let start = 12 + index * 12;
        let entry = &bytes[start..start + 12];
        let child_block = ((le_u16(&entry[8..10]) as u64) << 32) | le_u32(&entry[4..8]) as u64;
        if child_block == 0 || !visited_leaf_blocks.insert(child_block) {
            continue;
        }

        let child_bytes = read_extent_tree_block(reader, superblock, child_block)?;
        collect_extent_tree(
            reader,
            superblock,
            &child_bytes,
            "extent index leaf",
            visited_leaf_blocks,
            blocks,
        )?;
    }

    Ok(())
}

fn parse_extent_header(bytes: &[u8], label: &str) -> Result<ExtentHeader, String> {
    if bytes.len() < 12 {
        return Err(format!(
            "The ext4 {label} is too small to contain an extent header."
        ));
    }

    let magic = le_u16(&bytes[0..2]);
    if magic != EXT4_EXTENT_HEADER_MAGIC {
        return Err(format!("The ext4 {label} extent header is invalid."));
    }

    let entries = le_u16(&bytes[2..4]) as usize;
    let max_entries = le_u16(&bytes[4..6]) as usize;
    let depth = le_u16(&bytes[6..8]);
    if entries > max_entries {
        return Err(format!(
            "The ext4 {label} advertises more extent entries than its header allows."
        ));
    }

    let required_bytes = 12 + entries * 12;
    if required_bytes > bytes.len() {
        return Err(format!(
            "The ext4 {label} extent entries exceed the available node size."
        ));
    }

    Ok(ExtentHeader { entries, depth })
}

fn append_leaf_extent_blocks(
    bytes: &[u8],
    header: ExtentHeader,
    label: &str,
    blocks: &mut Vec<LogicalBlockPointer>,
) -> Result<(), String> {
    for index in 0..header.entries {
        let start = 12 + index * 12;
        let entry = &bytes[start..start + 12];
        let logical_block = le_u32(&entry[0..4]);
        let raw_length = le_u16(&entry[4..6]);
        let initialized = (raw_length & 0x8000) == 0;
        let block_count = (raw_length & 0x7fff) as u64;
        let start_block = ((le_u16(&entry[6..8]) as u64) << 32) | le_u32(&entry[8..12]) as u64;

        if !initialized || block_count == 0 {
            continue;
        }

        if start_block == 0 {
            return Err(format!("The ext4 {label} references a null extent block."));
        }

        for offset in 0..block_count {
            blocks.push(LogicalBlockPointer {
                logical_index: logical_block + offset as u32,
                physical_block: start_block + offset,
            });
        }
    }

    Ok(())
}

fn read_extent_tree_block(
    reader: &mut File,
    superblock: &Ext4Superblock,
    block: u64,
) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0_u8; superblock.block_size as usize];
    read_exact_at(
        reader,
        superblock.block_offset(block)?,
        &mut bytes,
        "ext4 extent tree block",
    )?;
    Ok(bytes)
}

fn read_byte_runs_prefix(
    reader: &mut File,
    byte_runs: &[ByteRun],
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    for run in byte_runs {
        if bytes.len() >= max_bytes {
            break;
        }
        let mut chunk = vec![0_u8; run.length.min((max_bytes - bytes.len()) as u64) as usize];
        read_exact_at(reader, run.offset, &mut chunk, "ext4 preview bytes")?;
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

fn bitmap_bit_is_set(bitmap: &[u8], index: usize) -> bool {
    let byte = bitmap.get(index / 8).copied().unwrap_or(0);
    ((byte >> (index % 8)) & 0x01) == 1
}

fn unix_seconds_to_iso(seconds: u32) -> Option<String> {
    if seconds == 0 {
        return None;
    }

    let total_seconds = seconds as i64;
    let days = total_seconds / 86_400;
    let seconds_of_day = total_seconds % 86_400;
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

fn div_ceil_u64(value: u64, divisor: u64) -> u64 {
    if divisor == 0 {
        0
    } else {
        value.div_ceil(divisor)
    }
}

fn le_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// Parse ext4 jbd2 journal for additional deleted file candidates.
/// Scans committed transactions for historical inode table snapshots that
/// reveal files not visible in the current allocation state.
pub fn recover_journal_files(image_path: &Path) -> Result<Vec<Ext4DeletedFileCandidate>, String> {
    let mut reader = File::open(image_path)
        .map_err(|e| format!("Unable to open ext4 image for journal: {e}"))?;

    let layout = match load_layout(&mut reader) {
        Ok(l) => l,
        Err(_) => return Ok(Vec::new()),
    };

    // ext4 journal is typically at inode 8
    const JOURNAL_INODE: u32 = 8;
    let journal_group =
        (JOURNAL_INODE.saturating_sub(1) / layout.superblock.inodes_per_group) as usize;
    let journal_slot = (JOURNAL_INODE - 1) % layout.superblock.inodes_per_group;

    if journal_group >= layout.groups.len() {
        return Ok(Vec::new());
    }

    let journal_inode = read_inode(
        &mut reader,
        &layout.superblock,
        &layout.groups[journal_group],
        JOURNAL_INODE,
        journal_slot,
    )?;

    if journal_inode.size_bytes == 0 {
        return Ok(Vec::new());
    }

    // Get journal data location from extent tree
    let journal_blocks = parse_logical_blocks(&mut reader, &layout.superblock, &journal_inode)?;
    if journal_blocks.is_empty() {
        return Ok(Vec::new());
    }

    let journal_start_block = journal_blocks[0].physical_block;
    let journal_offset = layout.superblock.block_offset(journal_start_block)?;

    // Read jbd2 superblock (first block of journal)
    let mut jbd2_header = [0u8; 1024];
    read_exact_at(
        &mut reader,
        journal_offset,
        &mut jbd2_header,
        "jbd2 superblock",
    )?;

    // jbd2 magic: 0xC03B3998 at offset 0 (big-endian)
    let jbd2_magic = u32::from_be_bytes([
        jbd2_header[0],
        jbd2_header[1],
        jbd2_header[2],
        jbd2_header[3],
    ]);
    if jbd2_magic != 0xC03B_3998 {
        return Ok(Vec::new());
    }

    let jbd2_block_size = u32::from_be_bytes([
        jbd2_header[12],
        jbd2_header[13],
        jbd2_header[14],
        jbd2_header[15],
    ]) as u64;
    let jbd2_maxlen = u32::from_be_bytes([
        jbd2_header[16],
        jbd2_header[17],
        jbd2_header[18],
        jbd2_header[19],
    ]) as u64;

    if jbd2_block_size == 0 || jbd2_block_size > 65536 || jbd2_maxlen == 0 {
        return Ok(Vec::new());
    }

    // Scan journal blocks for transaction descriptor blocks containing inode table copies
    let mut journal_inodes: Vec<(u32, Ext4Inode)> = Vec::new();
    let mut seen_inodes = std::collections::HashSet::new();
    let max_scan_blocks = jbd2_maxlen.min(8192); // Limit scan to 8K blocks

    for block_idx in 1..max_scan_blocks {
        let block_offset = journal_offset + block_idx * jbd2_block_size;
        let mut header = [0u8; 12];
        if read_exact_at(&mut reader, block_offset, &mut header, "jbd2 block header").is_err() {
            break;
        }

        let magic = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let block_type = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);

        if magic != 0xC03B_3998 {
            // Not a journal metadata block — could be a data block containing inodes
            // Try to parse as an inode table block (contains multiple inodes)
            let inode_size = layout.superblock.inode_size.max(EXT4_GOOD_OLD_INODE_SIZE) as usize;
            let inodes_per_block = (jbd2_block_size as usize) / inode_size;
            if inodes_per_block > 0 && inodes_per_block <= 64 {
                let mut block_data = vec![0u8; jbd2_block_size as usize];
                if read_exact_at(
                    &mut reader,
                    block_offset,
                    &mut block_data,
                    "journal data block",
                )
                .is_ok()
                {
                    for slot in 0..inodes_per_block {
                        let start = slot * inode_size;
                        let end = start + inode_size;
                        if end > block_data.len() {
                            break;
                        }

                        let inode_bytes = &block_data[start..end];
                        if inode_bytes.len() < 100 {
                            continue;
                        }

                        let mode = le_u16(&inode_bytes[0..2]);
                        let size = le_u32(&inode_bytes[4..8]) as u64;
                        let dtime = le_u32(&inode_bytes[20..24]);
                        let flags = le_u32(&inode_bytes[32..36]);

                        // Look for deleted regular files (dtime != 0, mode is regular file, has size)
                        if mode & EXT4_S_IFREG == 0 || size == 0 || dtime == 0 {
                            continue;
                        }

                        let mut block_bytes = [0u8; 60];
                        block_bytes.copy_from_slice(&inode_bytes[40..100]);

                        let size_high = if inode_size >= 112 {
                            le_u32(&inode_bytes[108..112]) as u64
                        } else {
                            0
                        };

                        // Generate a synthetic inode number for dedup
                        let synth_inode = (block_idx * 1000 + slot as u64) as u32;
                        if seen_inodes.contains(&(size, dtime as u64)) {
                            continue;
                        }
                        seen_inodes.insert((size, dtime as u64));

                        let inode = Ext4Inode {
                            inode_number: synth_inode,
                            mode,
                            size_bytes: (size_high << 32) | size,
                            modified_at: unix_seconds_to_iso(le_u32(&inode_bytes[16..20])),
                            deleted_at: unix_seconds_to_iso(dtime),
                            flags,
                            block_bytes,
                        };

                        journal_inodes.push((synth_inode, inode));
                    }
                }
            }
            continue;
        }

        // Skip commit blocks (type 2) and revoke blocks (type 5)
        // Descriptor blocks (type 1) list which FS blocks follow
        if block_type == 2 || block_type == 5 {
            continue;
        }
    }

    // Build candidates from journal-discovered inodes
    let mut candidates = Vec::new();
    for (_synth_num, inode) in &journal_inodes {
        if !inode.is_regular_file() || inode.size_bytes == 0 {
            continue;
        }
        if let Ok(Some(mut candidate)) = build_deleted_candidate(&mut reader, &layout, inode) {
            candidate.path = "/journal-recovered".into();
            candidate.name = format!("journal-{}", candidate.name);
            // Lower score for journal-derived files (less reliable)
            candidate.recovery_score = candidate.recovery_score.saturating_sub(8);
            candidates.push(candidate);
        }
    }

    Ok(candidates)
}

#[cfg(test)]
pub(crate) fn synthetic_deleted_ext4_image_for_tests(
    content: &[u8],
    mark_second_block_used: bool,
) -> Vec<u8> {
    let block_size = 1024usize;
    let total_blocks = 32usize;
    let mut image = vec![0_u8; block_size * total_blocks];

    let superblock = &mut image[1024..2048];
    superblock[4..8].copy_from_slice(&(total_blocks as u32).to_le_bytes());
    superblock[20..24].copy_from_slice(&1_u32.to_le_bytes());
    superblock[24..28].copy_from_slice(&0_u32.to_le_bytes());
    superblock[32..36].copy_from_slice(&(total_blocks as u32).to_le_bytes());
    superblock[40..44].copy_from_slice(&8_u32.to_le_bytes());
    superblock[56..58].copy_from_slice(&EXT4_SUPERBLOCK_MAGIC.to_le_bytes());
    superblock[84..88].copy_from_slice(&3_u32.to_le_bytes());
    superblock[88..90].copy_from_slice(&128_u16.to_le_bytes());
    superblock[96..100].copy_from_slice(&0x40_u32.to_le_bytes());
    superblock[254..256].copy_from_slice(&32_u16.to_le_bytes());

    let group_descriptor = &mut image[2048..2080];
    group_descriptor[0..4].copy_from_slice(&3_u32.to_le_bytes());
    group_descriptor[4..8].copy_from_slice(&4_u32.to_le_bytes());
    group_descriptor[8..12].copy_from_slice(&5_u32.to_le_bytes());

    let block_bitmap = &mut image[3 * block_size..4 * block_size];
    set_bitmap_bit(block_bitmap, 0);
    set_bitmap_bit(block_bitmap, 1);
    set_bitmap_bit(block_bitmap, 2);
    set_bitmap_bit(block_bitmap, 3);
    set_bitmap_bit(block_bitmap, 4);
    if mark_second_block_used {
        set_bitmap_bit(block_bitmap, 6);
    }

    let inode_bitmap = &mut image[4 * block_size..5 * block_size];
    set_bitmap_bit(inode_bitmap, 0);
    set_bitmap_bit(inode_bitmap, 1);

    let inode_offset = 5 * block_size + 2 * 128;
    let inode = &mut image[inode_offset..inode_offset + 128];
    inode[0..2].copy_from_slice(&EXT4_S_IFREG.to_le_bytes());
    inode[4..8].copy_from_slice(&(content.len() as u32).to_le_bytes());
    inode[16..20].copy_from_slice(&1_711_972_800_u32.to_le_bytes());
    inode[20..24].copy_from_slice(&1_712_059_200_u32.to_le_bytes());
    inode[26..28].copy_from_slice(&0_u16.to_le_bytes());
    inode[32..36].copy_from_slice(&EXT4_INODE_FLAG_EXTENTS.to_le_bytes());

    let extent_area = &mut inode[40..100];
    extent_area[0..2].copy_from_slice(&EXT4_EXTENT_HEADER_MAGIC.to_le_bytes());
    extent_area[2..4].copy_from_slice(&1_u16.to_le_bytes());
    extent_area[4..6].copy_from_slice(&4_u16.to_le_bytes());
    extent_area[6..8].copy_from_slice(&0_u16.to_le_bytes());
    extent_area[12..16].copy_from_slice(&0_u32.to_le_bytes());
    extent_area[16..18]
        .copy_from_slice(&(if mark_second_block_used { 2_u16 } else { 1_u16 }).to_le_bytes());
    extent_area[18..20].copy_from_slice(&0_u16.to_le_bytes());
    extent_area[20..24].copy_from_slice(&6_u32.to_le_bytes());

    let first_block_offset = 6 * block_size;
    image[first_block_offset..first_block_offset + content.len().min(block_size)]
        .copy_from_slice(&content[..content.len().min(block_size)]);
    if content.len() > block_size {
        let second_block_offset = 7 * block_size;
        image[second_block_offset..second_block_offset + (content.len() - block_size)]
            .copy_from_slice(&content[block_size..]);
    }

    image
}

#[cfg(test)]
pub(crate) fn synthetic_deleted_ext4_indexed_extent_image_for_tests(
    content: &[u8],
    mark_second_block_used: bool,
) -> Vec<u8> {
    let block_size = 1024usize;
    let total_blocks = 40usize;
    let mut image = vec![0_u8; block_size * total_blocks];

    let superblock = &mut image[1024..2048];
    superblock[4..8].copy_from_slice(&(total_blocks as u32).to_le_bytes());
    superblock[20..24].copy_from_slice(&1_u32.to_le_bytes());
    superblock[24..28].copy_from_slice(&0_u32.to_le_bytes());
    superblock[32..36].copy_from_slice(&(total_blocks as u32).to_le_bytes());
    superblock[40..44].copy_from_slice(&8_u32.to_le_bytes());
    superblock[56..58].copy_from_slice(&EXT4_SUPERBLOCK_MAGIC.to_le_bytes());
    superblock[84..88].copy_from_slice(&3_u32.to_le_bytes());
    superblock[88..90].copy_from_slice(&128_u16.to_le_bytes());
    superblock[96..100].copy_from_slice(&0x40_u32.to_le_bytes());
    superblock[254..256].copy_from_slice(&32_u16.to_le_bytes());

    let group_descriptor = &mut image[2048..2080];
    group_descriptor[0..4].copy_from_slice(&3_u32.to_le_bytes());
    group_descriptor[4..8].copy_from_slice(&4_u32.to_le_bytes());
    group_descriptor[8..12].copy_from_slice(&5_u32.to_le_bytes());

    let block_bitmap = &mut image[3 * block_size..4 * block_size];
    set_bitmap_bit(block_bitmap, 0);
    set_bitmap_bit(block_bitmap, 1);
    set_bitmap_bit(block_bitmap, 2);
    set_bitmap_bit(block_bitmap, 3);
    set_bitmap_bit(block_bitmap, 4);
    set_bitmap_bit(block_bitmap, 7);
    if mark_second_block_used {
        set_bitmap_bit(block_bitmap, 6);
    }

    let inode_bitmap = &mut image[4 * block_size..5 * block_size];
    set_bitmap_bit(inode_bitmap, 0);
    set_bitmap_bit(inode_bitmap, 1);

    let inode_offset = 5 * block_size + 2 * 128;
    let inode = &mut image[inode_offset..inode_offset + 128];
    inode[0..2].copy_from_slice(&EXT4_S_IFREG.to_le_bytes());
    inode[4..8].copy_from_slice(&(content.len() as u32).to_le_bytes());
    inode[16..20].copy_from_slice(&1_711_972_800_u32.to_le_bytes());
    inode[20..24].copy_from_slice(&1_712_059_200_u32.to_le_bytes());
    inode[26..28].copy_from_slice(&0_u16.to_le_bytes());
    inode[32..36].copy_from_slice(&EXT4_INODE_FLAG_EXTENTS.to_le_bytes());

    let root_extent_area = &mut inode[40..100];
    root_extent_area[0..2].copy_from_slice(&EXT4_EXTENT_HEADER_MAGIC.to_le_bytes());
    root_extent_area[2..4].copy_from_slice(&1_u16.to_le_bytes());
    root_extent_area[4..6].copy_from_slice(&4_u16.to_le_bytes());
    root_extent_area[6..8].copy_from_slice(&1_u16.to_le_bytes());
    root_extent_area[12..16].copy_from_slice(&0_u32.to_le_bytes());
    root_extent_area[16..20].copy_from_slice(&8_u32.to_le_bytes());
    root_extent_area[20..22].copy_from_slice(&0_u16.to_le_bytes());

    let leaf_block = &mut image[8 * block_size..9 * block_size];
    leaf_block[0..2].copy_from_slice(&EXT4_EXTENT_HEADER_MAGIC.to_le_bytes());
    leaf_block[2..4].copy_from_slice(&1_u16.to_le_bytes());
    let max_leaf_entries = ((block_size - 12) / 12) as u16;
    leaf_block[4..6].copy_from_slice(&max_leaf_entries.to_le_bytes());
    leaf_block[6..8].copy_from_slice(&0_u16.to_le_bytes());
    leaf_block[12..16].copy_from_slice(&0_u32.to_le_bytes());
    leaf_block[16..18]
        .copy_from_slice(&(if mark_second_block_used { 2_u16 } else { 1_u16 }).to_le_bytes());
    leaf_block[18..20].copy_from_slice(&0_u16.to_le_bytes());
    leaf_block[20..24].copy_from_slice(&6_u32.to_le_bytes());

    let first_block_offset = 6 * block_size;
    image[first_block_offset..first_block_offset + content.len().min(block_size)]
        .copy_from_slice(&content[..content.len().min(block_size)]);
    if content.len() > block_size {
        let second_block_offset = 7 * block_size;
        image[second_block_offset..second_block_offset + (content.len() - block_size)]
            .copy_from_slice(&content[block_size..]);
    }

    image
}

#[cfg(test)]
fn set_bitmap_bit(bitmap: &mut [u8], index: usize) {
    let byte_index = index / 8;
    let bit_offset = index % 8;
    bitmap[byte_index] |= 1 << bit_offset;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs};

    #[test]
    fn recover_deleted_files_reads_deleted_extent_backed_inode() {
        let root = env::temp_dir().join(format!("recupere-ext4-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("ext4 test root should exist");
        let image_path = root.join("deleted-ext4.img");
        fs::write(
            &image_path,
            synthetic_deleted_ext4_image_for_tests(b"hello ext4!", false),
        )
        .expect("synthetic ext4 image should be written");

        let candidates =
            recover_deleted_files(&image_path).expect("ext4 deleted recovery should parse");
        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert_eq!(candidate.name, "inode-000003.txt");
        assert_eq!(candidate.path, "/orphaned-inodes");
        assert_eq!(candidate.size_bytes, 11);
        assert_eq!(candidate.expected_size_bytes, 11);
        assert_eq!(candidate.integrity, "intact");
        assert_eq!(
            candidate.modified_at.as_deref(),
            Some("2024-04-01T12:00:00")
        );
        assert_eq!(candidate.deleted_at.as_deref(), Some("2024-04-02T12:00:00"));
        assert_eq!(candidate.byte_runs.len(), 1);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn recover_deleted_files_marks_partially_reconstructible_extent_ranges() {
        let root =
            env::temp_dir().join(format!("recupere-ext4-partial-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("ext4 test root should exist");
        let image_path = root.join("deleted-ext4-partial.img");
        fs::write(
            &image_path,
            synthetic_deleted_ext4_image_for_tests(&[b'A'; 2048], true),
        )
        .expect("synthetic partial ext4 image should be written");

        let candidates =
            recover_deleted_files(&image_path).expect("ext4 deleted recovery should parse");
        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert_eq!(candidate.size_bytes, 1024);
        assert_eq!(candidate.expected_size_bytes, 2048);
        assert_eq!(candidate.integrity, "partial");
        assert_eq!(candidate.byte_runs.len(), 1);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn recover_deleted_files_reads_indexed_extent_tree_nodes() {
        let root =
            env::temp_dir().join(format!("recupere-ext4-indexed-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("ext4 indexed test root should exist");
        let image_path = root.join("deleted-ext4-indexed.img");
        fs::write(
            &image_path,
            synthetic_deleted_ext4_indexed_extent_image_for_tests(b"hello indexed ext4", false),
        )
        .expect("synthetic indexed ext4 image should be written");

        let candidates =
            recover_deleted_files(&image_path).expect("indexed ext4 deleted recovery should parse");
        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert_eq!(candidate.name, "inode-000003.txt");
        assert_eq!(candidate.path, "/orphaned-inodes");
        assert_eq!(candidate.size_bytes, 18);
        assert_eq!(candidate.expected_size_bytes, 18);
        assert_eq!(candidate.integrity, "intact");
        assert_eq!(candidate.byte_runs.len(), 1);
        assert_eq!(candidate.byte_runs[0].offset, 6 * 1024);

        let _ = fs::remove_dir_all(&root);
    }
}
