use crate::{types::ByteRun, virtual_disk};
use lznt1::decompress as decompress_lznt1;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    env,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

/// Open a potential imaging source with the narrowest possible access: read
/// only, no write intent, no exclusive lock. This is the single allowed
/// gateway to the source disk — Rust's `File::open` already sets `O_RDONLY`
/// on Unix, but we codify the intent in one helper so future refactors cannot
/// accidentally pass `write: true` or `create: true` while targeting the same
/// path. On Windows we also widen `share_mode` so a disk that's being written
/// to by the OS (e.g. the user's own system volume) can still be imaged.
///
/// Contract enforced by `open_source_read_only_refuses_write_intent` test.
pub(crate) fn open_source_read_only(path: &Path) -> std::io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true);
    // Belt-and-braces: explicitly opt OUT of every write-adjacent flag. These
    // are no-ops because we never set them, but the zero-assignments make a
    // future reviewer's intent-check trivial.
    opts.write(false)
        .append(false)
        .create(false)
        .truncate(false);

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE. Without this,
        // Windows refuses to open a disk that another process is writing to,
        // which is exactly the common case for imaging a live system drive.
        const FILE_SHARE_READ_WRITE_DELETE: u32 = 0x00000001 | 0x00000002 | 0x00000004;
        opts.share_mode(FILE_SHARE_READ_WRITE_DELETE);
    }

    opts.open(path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImagingProfile {
    Standard,
    Cautious,
}

impl ImagingProfile {
    fn buffer_size_bytes(self) -> usize {
        match self {
            Self::Standard => 1024 * 1024,
            Self::Cautious => 256 * 1024,
        }
    }

    fn read_attempts(self) -> u8 {
        match self {
            Self::Standard => 1,
            Self::Cautious => 3,
        }
    }

    fn retry_delay(self) -> Duration {
        match self {
            Self::Standard => Duration::from_millis(0),
            Self::Cautious => Duration::from_millis(80),
        }
    }

    fn unreadable_skip_span_bytes(self) -> u64 {
        match self {
            Self::Standard => 0,
            Self::Cautious => 4096,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Cautious => "cautious",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageArtifact {
    pub path: PathBuf,
    pub bytes_copied: u64,
    pub resume_from_bytes: u64,
    pub unreadable_ranges_count: u64,
    pub unreadable_bytes: u64,
    #[serde(default)]
    pub unreadable_ranges: Vec<UnreadableRange>,
    pub unreadable_range_samples: Vec<UnreadableRange>,
    #[serde(default)]
    pub rescued_after_retry_bytes: u64,
    #[serde(default)]
    pub retry_passes_completed: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnreadableRange {
    pub start_offset: u64,
    pub length: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExternalRescueMapStatus {
    NonTried,
    NonTrimmed,
    NonScraped,
    BadSector,
    Finished,
}

impl ExternalRescueMapStatus {
    fn from_char(value: char) -> Option<Self> {
        match value {
            '?' => Some(Self::NonTried),
            '*' => Some(Self::NonTrimmed),
            '/' => Some(Self::NonScraped),
            '-' => Some(Self::BadSector),
            '+' => Some(Self::Finished),
            _ => None,
        }
    }

    fn is_not_yet_copied(self) -> bool {
        matches!(self, Self::NonTried)
    }

    fn is_unresolved_in_copied_prefix(self) -> bool {
        matches!(self, Self::NonTrimmed | Self::NonScraped | Self::BadSector)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExternalRescueMapBlock {
    start_offset: u64,
    length: u64,
    status: ExternalRescueMapStatus,
}

#[derive(Debug, Clone)]
struct ParsedExternalRescueMap {
    blocks: Vec<ExternalRescueMapBlock>,
    domain_end: u64,
}

#[derive(Debug, Clone)]
struct ExternalRescueMapSeed {
    resume_from_bytes: u64,
    unresolved_ranges: Vec<UnreadableRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalRescueMapImportSummary {
    pub resume_from_bytes: u64,
    pub mapped_bytes: u64,
    pub unreadable_ranges_count: u64,
    pub unreadable_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
struct ImagingIssueSummary {
    unreadable_ranges_count: u64,
    unreadable_bytes: u64,
}

#[derive(Debug, Clone)]
struct PreparedPartialImageDestination {
    resume_from_bytes: u64,
    issue_summary: ImagingIssueSummary,
    carried_unmapped_issue_summary: ImagingIssueSummary,
    unresolved_ranges: Vec<UnreadableRange>,
    retry_refinement_summary: RetryRefinementSummary,
    checkpoint_path: PathBuf,
    checkpoint_seed: PartialImageCheckpoint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PartialImageCheckpoint {
    source_path: String,
    start_offset_bytes: u64,
    requested_length_bytes: Option<u64>,
    source_length_bytes: Option<u64>,
    #[serde(default)]
    unreadable_ranges_count: u64,
    #[serde(default)]
    unreadable_bytes: u64,
    #[serde(default)]
    unreadable_ranges: Vec<UnreadableRange>,
    #[serde(default)]
    rescued_after_retry_bytes: u64,
    #[serde(default)]
    retry_passes_completed: u8,
}

enum ChunkReadOutcome {
    Read(usize),
    Unreadable,
}

const MAX_UNREADABLE_RANGE_SAMPLES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetryTraversalDirection {
    Forward,
    Reverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetryPassMode {
    Sequential(RetryTraversalDirection),
    EdgeTrim,
    CenterOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetryRangeOrder {
    SourceAscending,
    SourceDescending,
    SmallestFirst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetryReadStrategy {
    FullCautious,
    Probe,
}

#[derive(Debug, Clone, Copy)]
struct RetryRefinementPass {
    block_size: usize,
    mode: RetryPassMode,
    range_order: RetryRangeOrder,
    read_strategy: RetryReadStrategy,
    neighbor_zoom_block_size: Option<usize>,
    partial_progress_split_block_size: Option<usize>,
}

const CAUTIOUS_RETRY_REFINEMENT_PASSES: [RetryRefinementPass; 9] = [
    RetryRefinementPass {
        block_size: 1024,
        mode: RetryPassMode::Sequential(RetryTraversalDirection::Forward),
        range_order: RetryRangeOrder::SourceAscending,
        read_strategy: RetryReadStrategy::FullCautious,
        neighbor_zoom_block_size: None,
        partial_progress_split_block_size: None,
    },
    RetryRefinementPass {
        block_size: 1024,
        mode: RetryPassMode::Sequential(RetryTraversalDirection::Reverse),
        range_order: RetryRangeOrder::SourceDescending,
        read_strategy: RetryReadStrategy::FullCautious,
        neighbor_zoom_block_size: None,
        partial_progress_split_block_size: None,
    },
    RetryRefinementPass {
        block_size: 512,
        mode: RetryPassMode::EdgeTrim,
        range_order: RetryRangeOrder::SourceAscending,
        read_strategy: RetryReadStrategy::Probe,
        neighbor_zoom_block_size: None,
        partial_progress_split_block_size: None,
    },
    RetryRefinementPass {
        block_size: 256,
        mode: RetryPassMode::Sequential(RetryTraversalDirection::Forward),
        range_order: RetryRangeOrder::SmallestFirst,
        read_strategy: RetryReadStrategy::Probe,
        neighbor_zoom_block_size: None,
        partial_progress_split_block_size: None,
    },
    RetryRefinementPass {
        block_size: 256,
        mode: RetryPassMode::Sequential(RetryTraversalDirection::Reverse),
        range_order: RetryRangeOrder::SmallestFirst,
        read_strategy: RetryReadStrategy::Probe,
        neighbor_zoom_block_size: None,
        partial_progress_split_block_size: None,
    },
    RetryRefinementPass {
        block_size: 128,
        mode: RetryPassMode::EdgeTrim,
        range_order: RetryRangeOrder::SmallestFirst,
        read_strategy: RetryReadStrategy::Probe,
        neighbor_zoom_block_size: None,
        partial_progress_split_block_size: None,
    },
    RetryRefinementPass {
        block_size: 64,
        mode: RetryPassMode::CenterOut,
        range_order: RetryRangeOrder::SmallestFirst,
        read_strategy: RetryReadStrategy::Probe,
        neighbor_zoom_block_size: Some(32),
        partial_progress_split_block_size: Some(32),
    },
    RetryRefinementPass {
        block_size: 32,
        mode: RetryPassMode::Sequential(RetryTraversalDirection::Forward),
        range_order: RetryRangeOrder::SmallestFirst,
        read_strategy: RetryReadStrategy::Probe,
        neighbor_zoom_block_size: Some(16),
        partial_progress_split_block_size: Some(16),
    },
    RetryRefinementPass {
        block_size: 16,
        mode: RetryPassMode::Sequential(RetryTraversalDirection::Reverse),
        range_order: RetryRangeOrder::SmallestFirst,
        read_strategy: RetryReadStrategy::Probe,
        neighbor_zoom_block_size: None,
        partial_progress_split_block_size: None,
    },
];

#[derive(Debug, Clone, Copy, Default)]
struct RetryRefinementSummary {
    rescued_after_retry_bytes: u64,
    retry_passes_completed: u8,
}

fn parse_mapfile_u64(token: &str) -> Result<u64, String> {
    let trimmed = token.trim();
    let hex = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"));
    if let Some(value) = hex {
        u64::from_str_radix(value, 16)
            .map_err(|error| format!("Unable to parse mapfile value `{trimmed}`: {error}"))
    } else {
        trimmed
            .parse::<u64>()
            .map_err(|error| format!("Unable to parse mapfile value `{trimmed}`: {error}"))
    }
}

fn parse_external_rescue_map(mapfile: &str) -> Result<ParsedExternalRescueMap, String> {
    let mut blocks = Vec::new();

    for (line_index, raw_line) in mapfile.lines().enumerate() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.len() != 3 {
            return Err(format!(
                "Unable to parse rescue map line {}: expected three whitespace-separated columns.",
                line_index + 1
            ));
        }

        if parse_mapfile_u64(tokens[0]).is_ok()
            && tokens[1].chars().count() == 1
            && tokens[1]
                .chars()
                .next()
                .and_then(ExternalRescueMapStatus::from_char)
                .is_some()
            && tokens[2].parse::<u32>().is_ok()
        {
            continue;
        }

        let start_offset = parse_mapfile_u64(tokens[0])?;
        let length = parse_mapfile_u64(tokens[1])?;
        if length == 0 {
            return Err(format!(
                "Unable to import a zero-length rescue-map block on line {}.",
                line_index + 1
            ));
        }

        let status_char = tokens[2].chars().next().ok_or_else(|| {
            format!(
                "Unable to parse rescue map line {}: missing block status.",
                line_index + 1
            )
        })?;
        if tokens[2].chars().count() != 1 {
            return Err(format!(
                "Unable to parse rescue map line {}: block status must be a single character.",
                line_index + 1
            ));
        }
        let status = ExternalRescueMapStatus::from_char(status_char).ok_or_else(|| {
            format!(
                "Rescue map line {} uses unsupported block status `{status_char}`.",
                line_index + 1
            )
        })?;

        blocks.push(ExternalRescueMapBlock {
            start_offset,
            length,
            status,
        });
    }

    if blocks.is_empty() {
        return Err("The selected rescue map does not contain any importable block entry.".into());
    }

    let mut expected_start = 0_u64;
    for block in &blocks {
        if block.start_offset != expected_start {
            return Err(format!(
                "The rescue map is not contiguous at offset {}. Recupere can only import contiguous mapfiles into its prefix-based partial imaging model.",
                block.start_offset
            ));
        }
        expected_start = block.start_offset.saturating_add(block.length);
    }

    Ok(ParsedExternalRescueMap {
        blocks,
        domain_end: expected_start,
    })
}

fn seed_external_rescue_map(
    parsed: &ParsedExternalRescueMap,
    partial_size: u64,
) -> Result<ExternalRescueMapSeed, String> {
    let mut resume_from_bytes = parsed.domain_end;
    let mut seen_not_yet_copied = false;
    let mut has_non_prefix_layout = false;
    let mut unresolved_prefix_ranges = Vec::new();

    for block in &parsed.blocks {
        if block.status.is_not_yet_copied() {
            if !seen_not_yet_copied {
                seen_not_yet_copied = true;
                resume_from_bytes = block.start_offset;
            }
            continue;
        }

        if seen_not_yet_copied {
            has_non_prefix_layout = true;
        }

        if block.status.is_unresolved_in_copied_prefix() {
            unresolved_prefix_ranges.push(UnreadableRange {
                start_offset: block.start_offset,
                length: block.length,
            });
        }
    }

    if has_non_prefix_layout {
        if partial_size != parsed.domain_end {
            return Err(format!(
                "The rescue map describes a sparse or out-of-order layout, but the matching partial image is only {} bytes long while the map covers {} bytes. Recupere can only reuse this kind of map when the local `.partial` image already has the full logical length of the map domain.",
                partial_size, parsed.domain_end
            ));
        }

        let unresolved_ranges = parsed
            .blocks
            .iter()
            .filter(|block| block.status != ExternalRescueMapStatus::Finished)
            .map(|block| UnreadableRange {
                start_offset: block.start_offset,
                length: block.length,
            })
            .collect::<Vec<_>>();

        return Ok(ExternalRescueMapSeed {
            resume_from_bytes: partial_size,
            unresolved_ranges: merge_unreadable_ranges(unresolved_ranges),
        });
    }

    if resume_from_bytes == 0 {
        return Err(
            "The rescue map does not describe any reusable copied prefix. Recupere cannot seed a local resume state from a map that starts with not-yet-copied blocks."
                .into(),
        );
    }

    Ok(ExternalRescueMapSeed {
        resume_from_bytes,
        unresolved_ranges: merge_unreadable_ranges(unresolved_prefix_ranges),
    })
}

pub fn create_read_only_image(
    scan_id: &str,
    source_path: &Path,
    progress: &mut dyn FnMut(u64),
) -> Result<ImageArtifact, String> {
    create_read_only_image_with_profile(scan_id, source_path, ImagingProfile::Standard, progress)
}

pub fn create_read_only_image_with_profile(
    scan_id: &str,
    source_path: &Path,
    profile: ImagingProfile,
    progress: &mut dyn FnMut(u64),
) -> Result<ImageArtifact, String> {
    let final_path = workspace_image_path_for_scan(scan_id);
    create_read_only_image_at_with_profile(&final_path, source_path, profile, progress)
}

pub fn workspace_image_path_for_scan(scan_id: &str) -> PathBuf {
    let file_stem = sanitize_scan_id(scan_id);
    imaging_workspace_dir().join(format!("{file_stem}.img"))
}

pub fn create_read_only_image_at(
    destination_path: &Path,
    source_path: &Path,
    progress: &mut dyn FnMut(u64),
) -> Result<ImageArtifact, String> {
    create_read_only_image_at_with_profile(
        destination_path,
        source_path,
        ImagingProfile::Standard,
        progress,
    )
}

pub fn create_read_only_image_at_with_profile(
    destination_path: &Path,
    source_path: &Path,
    profile: ImagingProfile,
    progress: &mut dyn FnMut(u64),
) -> Result<ImageArtifact, String> {
    create_read_only_image_at_controlled_with_profile(
        destination_path,
        source_path,
        profile,
        &mut |copied| {
            progress(copied);
            Ok(())
        },
    )
}

pub fn create_read_only_image_at_controlled(
    destination_path: &Path,
    source_path: &Path,
    progress: &mut dyn FnMut(u64) -> Result<(), String>,
) -> Result<ImageArtifact, String> {
    create_read_only_image_at_controlled_with_profile(
        destination_path,
        source_path,
        ImagingProfile::Standard,
        progress,
    )
}

pub fn create_read_only_image_at_controlled_with_profile(
    destination_path: &Path,
    source_path: &Path,
    profile: ImagingProfile,
    progress: &mut dyn FnMut(u64) -> Result<(), String>,
) -> Result<ImageArtifact, String> {
    copy_source_bytes_to_image(destination_path, source_path, 0, None, profile, progress)
}

pub fn create_read_only_image_slice_at_controlled(
    destination_path: &Path,
    source_path: &Path,
    start_offset_bytes: u64,
    max_length_bytes: Option<u64>,
    progress: &mut dyn FnMut(u64) -> Result<(), String>,
) -> Result<ImageArtifact, String> {
    create_read_only_image_slice_at_controlled_with_profile(
        destination_path,
        source_path,
        start_offset_bytes,
        max_length_bytes,
        ImagingProfile::Standard,
        progress,
    )
}

pub fn create_read_only_image_slice_at_controlled_with_profile(
    destination_path: &Path,
    source_path: &Path,
    start_offset_bytes: u64,
    max_length_bytes: Option<u64>,
    profile: ImagingProfile,
    progress: &mut dyn FnMut(u64) -> Result<(), String>,
) -> Result<ImageArtifact, String> {
    copy_source_bytes_to_image(
        destination_path,
        source_path,
        start_offset_bytes,
        max_length_bytes,
        profile,
        progress,
    )
}

pub fn import_external_rescue_map_for_image_destination(
    destination_path: &Path,
    source_path: &Path,
    source_length_bytes: u64,
    mapfile_path: &Path,
) -> Result<ExternalRescueMapImportSummary, String> {
    let partial_path = build_partial_image_path(destination_path)?;
    if !partial_path.exists() {
        return Err(format!(
            "The rescue map {} cannot be imported yet because the matching partial image {} does not exist. Recupere only reuses an external map when a coherent local `.partial` image is already present.",
            mapfile_path.to_string_lossy(),
            partial_path.to_string_lossy()
        ));
    }

    let mapfile = fs::read_to_string(mapfile_path).map_err(|error| {
        format!(
            "Unable to read the rescue map {}: {}",
            mapfile_path.to_string_lossy(),
            error
        )
    })?;
    let parsed_map = parse_external_rescue_map(&mapfile)?;
    if parsed_map.domain_end > source_length_bytes {
        return Err(format!(
            "The rescue map covers {} bytes, which exceeds the known source length of {} bytes.",
            parsed_map.domain_end, source_length_bytes
        ));
    }

    let partial_size = fs::metadata(&partial_path)
        .map_err(|error| {
            format!(
                "Unable to inspect the matching partial image {}: {}",
                partial_path.to_string_lossy(),
                error
            )
        })?
        .len();
    let seed = seed_external_rescue_map(&parsed_map, partial_size)?;
    if partial_size != seed.resume_from_bytes {
        let compatibility_hint = if partial_size > seed.resume_from_bytes {
            "This usually means the selected partial image diverges from the copied prefix described by the rescue map. Recupere only accepts this when the map itself is sparse or out of order and the local `.partial` image already matches the full logical length of the map domain."
        } else {
            "The local partial image is shorter than the copied prefix described by the rescue map."
        };
        return Err(format!(
            "The rescue map and the matching partial image disagree on the reusable prefix (map: {} bytes, partial image: {} bytes). {}",
            seed.resume_from_bytes, partial_size, compatibility_hint
        ));
    }

    let checkpoint_path = partial_image_checkpoint_path(&partial_path)?;
    let unresolved_summary = summarize_unreadable_ranges(&seed.unresolved_ranges);
    let checkpoint = PartialImageCheckpoint {
        source_path: source_path.to_string_lossy().to_string(),
        start_offset_bytes: 0,
        requested_length_bytes: None,
        source_length_bytes: Some(source_length_bytes),
        unreadable_ranges_count: unresolved_summary.unreadable_ranges_count,
        unreadable_bytes: unresolved_summary.unreadable_bytes,
        unreadable_ranges: seed.unresolved_ranges,
        rescued_after_retry_bytes: 0,
        retry_passes_completed: 0,
    };
    write_partial_image_checkpoint(&checkpoint_path, &checkpoint)?;

    Ok(ExternalRescueMapImportSummary {
        resume_from_bytes: seed.resume_from_bytes,
        mapped_bytes: parsed_map.domain_end,
        unreadable_ranges_count: unresolved_summary.unreadable_ranges_count,
        unreadable_bytes: unresolved_summary.unreadable_bytes,
    })
}

fn copy_source_bytes_to_image(
    destination_path: &Path,
    source_path: &Path,
    start_offset_bytes: u64,
    max_length_bytes: Option<u64>,
    profile: ImagingProfile,
    progress: &mut dyn FnMut(u64) -> Result<(), String>,
) -> Result<ImageArtifact, String> {
    if destination_path.is_dir() {
        return Err(format!(
            "The selected image destination {} is a directory.",
            destination_path.to_string_lossy()
        ));
    }

    let parent = destination_path.parent().ok_or_else(|| {
        format!(
            "The selected image destination {} has no writable parent directory.",
            destination_path.to_string_lossy()
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "Unable to prepare the image destination directory {}: {}",
            parent.to_string_lossy(),
            error
        )
    })?;

    let partial_path = build_partial_image_path(destination_path)?;

    if source_path.is_file() {
        return copy_recovery_source_file_to_image(
            destination_path,
            source_path,
            &partial_path,
            start_offset_bytes,
            max_length_bytes,
            profile,
            progress,
        );
    }

    let mut source_file = open_source_read_only(source_path).map_err(|error| {
        format!(
            "Unable to open the source for read-only imaging {}: {}",
            source_path.to_string_lossy(),
            error
        )
    })?;
    let source_length = source_file
        .metadata()
        .map_err(|error| {
            format!(
                "Unable to inspect the source length for read-only imaging {}: {}",
                source_path.to_string_lossy(),
                error
            )
        })?
        .len();

    // Block devices on Unix (e.g. /dev/disk4s2 on macOS, /dev/sda1 on Linux)
    // report a `metadata().len()` of 0 because their size is not exposed via
    // stat(). When the caller wants the full source and we observe a zero
    // length, fall back to a probe-based size discovery and, if that also
    // returns zero, stream until EOF without an upper bound.
    let mut effective_source_length = source_length;
    if effective_source_length == 0 && max_length_bytes.is_none() {
        if let Some(probed) = probe_block_device_length(&mut source_file) {
            effective_source_length = probed;
        }
    }

    if effective_source_length > 0 && start_offset_bytes > effective_source_length {
        return Err(format!(
            "The requested imaging slice starts beyond the end of the source {}.",
            source_path.to_string_lossy()
        ));
    }

    let bytes_to_copy = if effective_source_length == 0 && max_length_bytes.is_none() {
        // Unknown length (block device) — stream until EOF.
        u64::MAX
    } else {
        let remaining_length = effective_source_length.saturating_sub(start_offset_bytes);
        let requested_length = max_length_bytes.unwrap_or(remaining_length);
        requested_length.min(remaining_length)
    };
    let prepared_destination = prepare_partial_image_destination(
        &partial_path,
        source_path,
        start_offset_bytes,
        max_length_bytes,
        if effective_source_length > 0 {
            Some(effective_source_length)
        } else {
            None
        },
        if bytes_to_copy == u64::MAX {
            None
        } else {
            Some(bytes_to_copy)
        },
    )?;
    let resume_from_bytes = prepared_destination.resume_from_bytes;
    let mut issue_summary = prepared_destination.issue_summary;
    let carried_unmapped_issue_summary = prepared_destination.carried_unmapped_issue_summary;
    let mut unreadable_ranges = prepared_destination.unresolved_ranges;
    let mut retry_refinement_summary = prepared_destination.retry_refinement_summary;
    let checkpoint_path = prepared_destination.checkpoint_path;
    let checkpoint_seed = prepared_destination.checkpoint_seed;

    source_file
        .seek(SeekFrom::Start(
            start_offset_bytes.saturating_add(resume_from_bytes),
        ))
        .map_err(|error| {
            format!(
                "Unable to seek the source {} to the requested read-only offset: {}",
                source_path.to_string_lossy(),
                error
            )
        })?;

    let destination_file =
        open_partial_image_writer(&partial_path, resume_from_bytes).map_err(|error| {
            format!(
                "Unable to create the read-only image {}: {}",
                partial_path.to_string_lossy(),
                error
            )
        })?;

    let mut reader = BufReader::new(source_file);
    let mut writer = BufWriter::new(destination_file);
    let mut buffer = vec![0_u8; profile.buffer_size_bytes()];
    let mut copied = resume_from_bytes;
    let mut remaining = bytes_to_copy;
    let mut last_unreadable_end = unreadable_ranges
        .last()
        .map(|range| range.start_offset.saturating_add(range.length));
    if resume_from_bytes > 0 {
        remaining = remaining.saturating_sub(resume_from_bytes);
        progress(copied)?;
    }

    loop {
        if remaining == 0 {
            break;
        }

        let read_limit = buffer.len().min(remaining as usize);
        let current_offset = start_offset_bytes.saturating_add(copied);
        let read = read_source_chunk_with_profile(
            &mut reader,
            &mut buffer[..read_limit],
            source_path,
            profile,
        )?;

        match read {
            ChunkReadOutcome::Read(read) => {
                if read == 0 {
                    break;
                }

                writer.write_all(&buffer[..read]).map_err(|error| {
                    format!(
                        "Unable to write the read-only image {}: {}",
                        partial_path.to_string_lossy(),
                        error
                    )
                })?;

                copied = copied.saturating_add(read as u64);
                remaining = remaining.saturating_sub(read as u64);
                last_unreadable_end = None;
                progress(copied)?;
            }
            ChunkReadOutcome::Unreadable => {
                let unreadable_length = if profile == ImagingProfile::Cautious {
                    (read_limit as u64).min(profile.unreadable_skip_span_bytes())
                } else {
                    read_limit as u64
                };
                write_zero_fill(&mut writer, unreadable_length)?;
                track_unreadable_range(
                    &mut issue_summary,
                    &mut unreadable_ranges,
                    &mut last_unreadable_end,
                    current_offset,
                    unreadable_length,
                );
                persist_partial_image_rescue_state(
                    &checkpoint_path,
                    &checkpoint_seed,
                    issue_summary,
                    &unreadable_ranges,
                    retry_refinement_summary,
                )?;
                reader
                    .seek(SeekFrom::Start(
                        current_offset.saturating_add(unreadable_length),
                    ))
                    .map_err(|error| {
                        format!(
                            "Unable to skip the unreadable source region during cautious imaging {}: {}",
                            source_path.to_string_lossy(),
                            error
                        )
                    })?;
                copied = copied.saturating_add(unreadable_length);
                remaining = remaining.saturating_sub(unreadable_length);
                progress(copied)?;
            }
        }
    }

    writer.flush().map_err(|error| {
        format!(
            "Unable to flush the read-only image {}: {}",
            partial_path.to_string_lossy(),
            error
        )
    })?;
    drop(writer);

    if profile == ImagingProfile::Cautious && !unreadable_ranges.is_empty() {
        let (refined_unreadable_ranges, summary) = recover_unreadable_ranges_with_refinement(
            &mut reader,
            &partial_path,
            source_path,
            start_offset_bytes,
            unreadable_ranges,
            carried_unmapped_issue_summary,
            retry_refinement_summary,
            &checkpoint_path,
            &checkpoint_seed,
        )?;
        unreadable_ranges = refined_unreadable_ranges;
        retry_refinement_summary = summary;
    }

    let current_issue_summary = summarize_unreadable_ranges(&unreadable_ranges);
    issue_summary = add_issue_summaries(carried_unmapped_issue_summary, current_issue_summary);
    persist_partial_image_rescue_state(
        &checkpoint_path,
        &checkpoint_seed,
        issue_summary,
        &unreadable_ranges,
        retry_refinement_summary,
    )?;

    let unreadable_range_samples = collect_unreadable_range_samples(&unreadable_ranges);

    finalize_partial_image(destination_path, &partial_path)?;

    Ok(ImageArtifact {
        path: destination_path.to_path_buf(),
        bytes_copied: copied,
        resume_from_bytes,
        unreadable_ranges_count: issue_summary.unreadable_ranges_count,
        unreadable_bytes: issue_summary.unreadable_bytes,
        unreadable_ranges: unreadable_ranges.clone(),
        unreadable_range_samples,
        rescued_after_retry_bytes: retry_refinement_summary.rescued_after_retry_bytes,
        retry_passes_completed: retry_refinement_summary.retry_passes_completed,
    })
}

fn copy_recovery_source_file_to_image(
    destination_path: &Path,
    source_path: &Path,
    partial_path: &Path,
    start_offset_bytes: u64,
    max_length_bytes: Option<u64>,
    profile: ImagingProfile,
    progress: &mut dyn FnMut(u64) -> Result<(), String>,
) -> Result<ImageArtifact, String> {
    let mut source = virtual_disk::open_recovery_source(source_path)?;
    let source_length = source.total_size().max(
        fs::metadata(source_path)
            .map_err(|error| {
                format!(
                    "Unable to inspect the source length for read-only imaging {}: {}",
                    source_path.to_string_lossy(),
                    error
                )
            })?
            .len(),
    );

    if source_length > 0 && start_offset_bytes > source_length {
        return Err(format!(
            "The requested imaging slice starts beyond the end of the source {}.",
            source_path.to_string_lossy()
        ));
    }

    let remaining_length = source_length.saturating_sub(start_offset_bytes);
    let bytes_to_copy = max_length_bytes
        .unwrap_or(remaining_length)
        .min(remaining_length);
    let prepared_destination = prepare_partial_image_destination(
        partial_path,
        source_path,
        start_offset_bytes,
        max_length_bytes,
        Some(source_length),
        Some(bytes_to_copy),
    )?;
    let resume_from_bytes = prepared_destination.resume_from_bytes;
    let mut issue_summary = prepared_destination.issue_summary;
    let carried_unmapped_issue_summary = prepared_destination.carried_unmapped_issue_summary;
    let mut unreadable_ranges = prepared_destination.unresolved_ranges;
    let mut retry_refinement_summary = prepared_destination.retry_refinement_summary;
    let checkpoint_path = prepared_destination.checkpoint_path;
    let checkpoint_seed = prepared_destination.checkpoint_seed;

    source
        .seek(SeekFrom::Start(
            start_offset_bytes.saturating_add(resume_from_bytes),
        ))
        .map_err(|error| {
            format!(
                "Unable to seek the source {} to the requested read-only offset: {}",
                source_path.to_string_lossy(),
                error
            )
        })?;

    let destination_file =
        open_partial_image_writer(partial_path, resume_from_bytes).map_err(|error| {
            format!(
                "Unable to create the read-only image {}: {}",
                partial_path.to_string_lossy(),
                error
            )
        })?;
    let mut writer = BufWriter::new(destination_file);
    let mut buffer = vec![0_u8; profile.buffer_size_bytes()];
    let mut copied = resume_from_bytes;
    let mut remaining = bytes_to_copy.saturating_sub(resume_from_bytes);
    let mut last_unreadable_end = unreadable_ranges
        .last()
        .map(|range| range.start_offset.saturating_add(range.length));
    if resume_from_bytes > 0 {
        progress(copied)?;
    }

    loop {
        if remaining == 0 {
            break;
        }

        let read_limit = buffer.len().min(remaining as usize);
        let current_offset = start_offset_bytes.saturating_add(copied);
        let read = read_imported_source_chunk_with_profile(
            &mut source,
            &mut buffer[..read_limit],
            source_path,
            profile,
        )?;

        match read {
            ChunkReadOutcome::Read(read) => {
                if read == 0 {
                    break;
                }

                writer.write_all(&buffer[..read]).map_err(|error| {
                    format!(
                        "Unable to write the read-only image {}: {}",
                        partial_path.to_string_lossy(),
                        error
                    )
                })?;

                copied = copied.saturating_add(read as u64);
                remaining = remaining.saturating_sub(read as u64);
                last_unreadable_end = None;
                progress(copied)?;
            }
            ChunkReadOutcome::Unreadable => {
                let unreadable_length = if profile == ImagingProfile::Cautious {
                    (read_limit as u64).min(profile.unreadable_skip_span_bytes())
                } else {
                    read_limit as u64
                };
                write_zero_fill(&mut writer, unreadable_length)?;
                track_unreadable_range(
                    &mut issue_summary,
                    &mut unreadable_ranges,
                    &mut last_unreadable_end,
                    current_offset,
                    unreadable_length,
                );
                persist_partial_image_rescue_state(
                    &checkpoint_path,
                    &checkpoint_seed,
                    issue_summary,
                    &unreadable_ranges,
                    retry_refinement_summary,
                )?;
                source
                    .seek(SeekFrom::Start(
                        current_offset.saturating_add(unreadable_length),
                    ))
                    .map_err(|error| {
                        format!(
                            "Unable to skip the unreadable imported-source region during cautious imaging {}: {}",
                            source_path.to_string_lossy(),
                            error
                        )
                    })?;
                copied = copied.saturating_add(unreadable_length);
                remaining = remaining.saturating_sub(unreadable_length);
                progress(copied)?;
            }
        }
    }

    writer.flush().map_err(|error| {
        format!(
            "Unable to flush the read-only image {}: {}",
            partial_path.to_string_lossy(),
            error
        )
    })?;
    drop(writer);

    if profile == ImagingProfile::Cautious && !unreadable_ranges.is_empty() {
        let (refined_unreadable_ranges, summary) = recover_unreadable_ranges_with_refinement(
            &mut source,
            partial_path,
            source_path,
            start_offset_bytes,
            unreadable_ranges,
            carried_unmapped_issue_summary,
            retry_refinement_summary,
            &checkpoint_path,
            &checkpoint_seed,
        )?;
        unreadable_ranges = refined_unreadable_ranges;
        retry_refinement_summary = summary;
    }

    let current_issue_summary = summarize_unreadable_ranges(&unreadable_ranges);
    issue_summary = add_issue_summaries(carried_unmapped_issue_summary, current_issue_summary);
    persist_partial_image_rescue_state(
        &checkpoint_path,
        &checkpoint_seed,
        issue_summary,
        &unreadable_ranges,
        retry_refinement_summary,
    )?;

    let unreadable_range_samples = collect_unreadable_range_samples(&unreadable_ranges);

    finalize_partial_image(destination_path, partial_path)?;

    Ok(ImageArtifact {
        path: destination_path.to_path_buf(),
        bytes_copied: copied,
        resume_from_bytes,
        unreadable_ranges_count: issue_summary.unreadable_ranges_count,
        unreadable_bytes: issue_summary.unreadable_bytes,
        unreadable_ranges: unreadable_ranges.clone(),
        unreadable_range_samples,
        rescued_after_retry_bytes: retry_refinement_summary.rescued_after_retry_bytes,
        retry_passes_completed: retry_refinement_summary.retry_passes_completed,
    })
}

fn read_source_chunk_with_profile<R: Read>(
    reader: &mut R,
    buffer: &mut [u8],
    source_path: &Path,
    profile: ImagingProfile,
) -> Result<ChunkReadOutcome, String> {
    read_chunk_with_retry_settings(
        profile.read_attempts(),
        profile.retry_delay(),
        profile == ImagingProfile::Cautious,
        || reader.read(buffer),
    )
    .map_err(|error| {
        format!(
            "Unable to read from the source during {} imaging {}: {}",
            profile.as_str(),
            source_path.to_string_lossy(),
            error
        )
    })
}

fn read_imported_source_chunk_with_profile<R: Read>(
    reader: &mut R,
    buffer: &mut [u8],
    source_path: &Path,
    profile: ImagingProfile,
) -> Result<ChunkReadOutcome, String> {
    read_chunk_with_retry_settings(
        profile.read_attempts(),
        profile.retry_delay(),
        profile == ImagingProfile::Cautious,
        || reader.read(buffer),
    )
    .map_err(|error| {
        format!(
            "Unable to read from the imported recovery source during {} imaging {}: {}",
            profile.as_str(),
            source_path.to_string_lossy(),
            error
        )
    })
}

fn read_chunk_with_retry_settings(
    attempts: u8,
    retry_delay: Duration,
    unreadable_on_failure: bool,
    mut read_once: impl FnMut() -> std::io::Result<usize>,
) -> Result<ChunkReadOutcome, String> {
    let mut last_error: Option<std::io::Error> = None;

    for attempt in 0..attempts {
        match read_once() {
            Ok(read) => return Ok(ChunkReadOutcome::Read(read)),
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < attempts {
                    thread::sleep(retry_delay);
                }
            }
        }
    }

    if let Some(error) = last_error {
        if unreadable_on_failure {
            return Ok(ChunkReadOutcome::Unreadable);
        }
        if attempts > 1 {
            return Err(format!(
                "{} after {} cautious read attempt(s)",
                error, attempts
            ));
        }
        return Err(error.to_string());
    }

    Err("unknown read error".into())
}

fn push_merged_unreadable_range(
    unreadable_ranges: &mut Vec<UnreadableRange>,
    last_unreadable_end: &mut Option<u64>,
    start_offset: u64,
    length: u64,
) {
    if *last_unreadable_end == Some(start_offset) {
        if let Some(last_range) = unreadable_ranges.last_mut() {
            if last_range.start_offset.saturating_add(last_range.length) == start_offset {
                last_range.length = last_range.length.saturating_add(length);
            }
        }
    } else {
        unreadable_ranges.push(UnreadableRange {
            start_offset,
            length,
        });
    }

    *last_unreadable_end = Some(start_offset.saturating_add(length));
}

fn summarize_unreadable_ranges(unreadable_ranges: &[UnreadableRange]) -> ImagingIssueSummary {
    let unreadable_bytes = unreadable_ranges
        .iter()
        .fold(0_u64, |total, range| total.saturating_add(range.length));

    ImagingIssueSummary {
        unreadable_ranges_count: unreadable_ranges.len() as u64,
        unreadable_bytes,
    }
}

fn add_issue_summaries(
    left: ImagingIssueSummary,
    right: ImagingIssueSummary,
) -> ImagingIssueSummary {
    ImagingIssueSummary {
        unreadable_ranges_count: left
            .unreadable_ranges_count
            .saturating_add(right.unreadable_ranges_count),
        unreadable_bytes: left.unreadable_bytes.saturating_add(right.unreadable_bytes),
    }
}

fn subtract_issue_summaries(
    left: ImagingIssueSummary,
    right: ImagingIssueSummary,
) -> ImagingIssueSummary {
    ImagingIssueSummary {
        unreadable_ranges_count: left
            .unreadable_ranges_count
            .saturating_sub(right.unreadable_ranges_count),
        unreadable_bytes: left.unreadable_bytes.saturating_sub(right.unreadable_bytes),
    }
}

fn collect_unreadable_range_samples(unreadable_ranges: &[UnreadableRange]) -> Vec<UnreadableRange> {
    unreadable_ranges
        .iter()
        .take(MAX_UNREADABLE_RANGE_SAMPLES)
        .cloned()
        .collect()
}

fn merge_unreadable_ranges(mut unreadable_ranges: Vec<UnreadableRange>) -> Vec<UnreadableRange> {
    unreadable_ranges.sort_by_key(|range| range.start_offset);
    let mut merged_ranges = Vec::new();
    let mut last_unreadable_end = None;

    for range in unreadable_ranges {
        if range.length == 0 {
            continue;
        }
        push_merged_unreadable_range(
            &mut merged_ranges,
            &mut last_unreadable_end,
            range.start_offset,
            range.length,
        );
    }

    merged_ranges
}

fn track_unreadable_range(
    issue_summary: &mut ImagingIssueSummary,
    unreadable_ranges: &mut Vec<UnreadableRange>,
    last_unreadable_end: &mut Option<u64>,
    start_offset: u64,
    length: u64,
) {
    if *last_unreadable_end != Some(start_offset) {
        issue_summary.unreadable_ranges_count =
            issue_summary.unreadable_ranges_count.saturating_add(1);
    }
    issue_summary.unreadable_bytes = issue_summary.unreadable_bytes.saturating_add(length);
    push_merged_unreadable_range(unreadable_ranges, last_unreadable_end, start_offset, length);
}

fn open_partial_image_writer(partial_path: &Path, resume_from_bytes: u64) -> std::io::Result<File> {
    if resume_from_bytes > 0 {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(partial_path)
    } else {
        File::create(partial_path)
    }
}

fn open_partial_image_patch_writer(partial_path: &Path) -> std::io::Result<File> {
    OpenOptions::new().read(true).write(true).open(partial_path)
}

fn refinement_chunks_for_range(
    range: &UnreadableRange,
    block_size: usize,
    mode: RetryPassMode,
) -> Vec<(u64, usize)> {
    let range_end = range.start_offset.saturating_add(range.length);
    let mut chunks = Vec::new();

    match mode {
        RetryPassMode::Sequential(RetryTraversalDirection::Forward) => {
            let mut offset = range.start_offset;
            while offset < range_end {
                let chunk_length =
                    (range_end.saturating_sub(offset)).min(block_size as u64) as usize;
                chunks.push((offset, chunk_length));
                offset = offset.saturating_add(chunk_length as u64);
            }
        }
        RetryPassMode::Sequential(RetryTraversalDirection::Reverse) => {
            let mut cursor = range_end;
            while cursor > range.start_offset {
                let chunk_start = range
                    .start_offset
                    .max(cursor.saturating_sub(block_size as u64));
                let chunk_length = cursor.saturating_sub(chunk_start) as usize;
                chunks.push((chunk_start, chunk_length));
                cursor = chunk_start;
            }
        }
        RetryPassMode::EdgeTrim => {
            let mut front = range.start_offset;
            let mut back = range_end;

            while front < back {
                let leading_length = (back.saturating_sub(front)).min(block_size as u64) as usize;
                chunks.push((front, leading_length));
                front = front.saturating_add(leading_length as u64);

                if front >= back {
                    break;
                }

                let trailing_start = front.max(back.saturating_sub(block_size as u64));
                let trailing_length = back.saturating_sub(trailing_start) as usize;
                chunks.push((trailing_start, trailing_length));
                back = trailing_start;
            }
        }
        RetryPassMode::CenterOut => {
            let mut offset = range.start_offset;
            while offset < range_end {
                let chunk_length =
                    (range_end.saturating_sub(offset)).min(block_size as u64) as usize;
                chunks.push((offset, chunk_length));
                offset = offset.saturating_add(chunk_length as u64);
            }

            if chunks.len() > 1 {
                let base_chunks = chunks;
                let pivot = base_chunks.len() / 2;
                let mut ordered = Vec::with_capacity(base_chunks.len());
                ordered.push(base_chunks[pivot]);

                let mut step = 1_usize;
                while ordered.len() < base_chunks.len() {
                    if let Some(left_index) = pivot.checked_sub(step) {
                        ordered.push(base_chunks[left_index]);
                    }

                    let right_index = pivot.saturating_add(step);
                    if right_index < base_chunks.len() {
                        ordered.push(base_chunks[right_index]);
                    }

                    step = step.saturating_add(1);
                }

                return ordered;
            }
        }
    }

    chunks
}

#[allow(clippy::too_many_arguments)]
fn recover_unreadable_ranges_with_refinement<R: Read + Seek>(
    reader: &mut R,
    partial_path: &Path,
    source_path: &Path,
    start_offset_bytes: u64,
    mut unresolved_ranges: Vec<UnreadableRange>,
    carried_unmapped_issue_summary: ImagingIssueSummary,
    mut refinement_summary: RetryRefinementSummary,
    checkpoint_path: &Path,
    checkpoint_seed: &PartialImageCheckpoint,
) -> Result<(Vec<UnreadableRange>, RetryRefinementSummary), String> {
    fn ordered_ranges_for_pass(
        unresolved_ranges: &[UnreadableRange],
        range_order: RetryRangeOrder,
    ) -> Vec<UnreadableRange> {
        let mut ordered = unresolved_ranges.to_vec();

        match range_order {
            RetryRangeOrder::SourceAscending => {}
            RetryRangeOrder::SourceDescending => ordered.reverse(),
            RetryRangeOrder::SmallestFirst => {
                ordered.sort_by_key(|range| (range.length, range.start_offset));
            }
        }

        ordered
    }

    fn read_targeted_rescue_chunk<R: Read>(
        reader: &mut R,
        buffer: &mut [u8],
        source_path: &Path,
        read_strategy: RetryReadStrategy,
    ) -> Result<ChunkReadOutcome, String> {
        let (attempts, retry_delay) = match read_strategy {
            RetryReadStrategy::FullCautious => (
                ImagingProfile::Cautious.read_attempts(),
                ImagingProfile::Cautious.retry_delay(),
            ),
            RetryReadStrategy::Probe => (1, Duration::from_millis(0)),
        };

        read_chunk_with_retry_settings(attempts, retry_delay, true, || reader.read(buffer)).map_err(
            |error| {
                format!(
                    "Unable to read from the source {} during targeted cautious rescue: {}",
                    source_path.to_string_lossy(),
                    error
                )
            },
        )
    }

    fn subtract_resolved_span(
        unresolved_ranges: &mut Vec<UnreadableRange>,
        start_offset: u64,
        length: u64,
    ) {
        if length == 0 {
            return;
        }

        let end_offset = start_offset.saturating_add(length);
        let mut next_ranges = Vec::new();

        for range in unresolved_ranges.drain(..) {
            let range_end = range.start_offset.saturating_add(range.length);
            if end_offset <= range.start_offset || start_offset >= range_end {
                next_ranges.push(range);
                continue;
            }

            if start_offset > range.start_offset {
                next_ranges.push(UnreadableRange {
                    start_offset: range.start_offset,
                    length: start_offset.saturating_sub(range.start_offset),
                });
            }

            if end_offset < range_end {
                next_ranges.push(UnreadableRange {
                    start_offset: end_offset,
                    length: range_end.saturating_sub(end_offset),
                });
            }
        }

        *unresolved_ranges = next_ranges;
    }

    fn rightmost_left_zoom_candidate(
        unresolved_ranges: &[UnreadableRange],
        boundary_offset: u64,
        zoom_block_size: usize,
    ) -> Option<(u64, usize)> {
        for range in unresolved_ranges.iter().rev() {
            let range_end = range.start_offset.saturating_add(range.length);
            if range_end <= boundary_offset
                && range_end > boundary_offset.saturating_sub(zoom_block_size as u64)
            {
                let start_offset = range_end
                    .saturating_sub(zoom_block_size as u64)
                    .max(range.start_offset);
                let length = range_end.saturating_sub(start_offset) as usize;
                if length > 0 {
                    return Some((start_offset, length));
                }
            }
        }

        None
    }

    fn leftmost_right_zoom_candidate(
        unresolved_ranges: &[UnreadableRange],
        boundary_offset: u64,
        zoom_block_size: usize,
    ) -> Option<(u64, usize)> {
        for range in unresolved_ranges {
            let range_end = range.start_offset.saturating_add(range.length);
            if range.start_offset >= boundary_offset
                && range.start_offset < boundary_offset.saturating_add(zoom_block_size as u64)
            {
                let end_offset =
                    range_end.min(range.start_offset.saturating_add(zoom_block_size as u64));
                let length = end_offset.saturating_sub(range.start_offset) as usize;
                if length > 0 {
                    return Some((range.start_offset, length));
                }
            }
        }

        None
    }

    fn schedule_front_chunks(
        pending_chunks: &mut VecDeque<(u64, usize)>,
        scheduled_chunks: &mut HashSet<(u64, usize)>,
        chunks: Vec<(u64, usize)>,
    ) {
        for chunk in chunks.into_iter().rev() {
            if scheduled_chunks.insert(chunk) {
                pending_chunks.push_front(chunk);
            }
        }
    }

    for pass in CAUTIOUS_RETRY_REFINEMENT_PASSES
        .iter()
        .copied()
        .skip(refinement_summary.retry_passes_completed as usize)
    {
        if unresolved_ranges.is_empty() {
            break;
        }

        refinement_summary.retry_passes_completed =
            refinement_summary.retry_passes_completed.saturating_add(1);
        let mut writer = open_partial_image_patch_writer(partial_path).map_err(|error| {
            format!(
                "Unable to reopen the partial read-only image {} for targeted rescue passes: {}",
                partial_path.to_string_lossy(),
                error
            )
        })?;
        let mut next_unresolved_segments = Vec::new();
        let ranges_for_pass = ordered_ranges_for_pass(&unresolved_ranges, pass.range_order);

        for range in ranges_for_pass {
            let mut range_unresolved = vec![range.clone()];
            let mut pending_chunks = VecDeque::from(refinement_chunks_for_range(
                &range,
                pass.block_size,
                pass.mode,
            ));
            let mut scheduled_chunks: HashSet<(u64, usize)> =
                pending_chunks.iter().copied().collect();

            while let Some((offset, chunk_length)) = pending_chunks.pop_front() {
                if !range_unresolved.iter().any(|current| {
                    offset >= current.start_offset
                        && offset.saturating_add(chunk_length as u64)
                            <= current.start_offset.saturating_add(current.length)
                }) {
                    continue;
                }

                let mut buffer = vec![0_u8; chunk_length];
                reader.seek(SeekFrom::Start(offset)).map_err(|error| {
                    format!(
                        "Unable to seek the source {} during targeted cautious rescue: {}",
                        source_path.to_string_lossy(),
                        error
                    )
                })?;

                match read_targeted_rescue_chunk(
                    reader,
                    &mut buffer,
                    source_path,
                    pass.read_strategy,
                )? {
                    ChunkReadOutcome::Read(0) => {}
                    ChunkReadOutcome::Read(read) => {
                        writer
                            .seek(SeekFrom::Start(offset.saturating_sub(start_offset_bytes)))
                            .map_err(|error| {
                                format!(
                                    "Unable to seek the partial image {} during targeted cautious rescue: {}",
                                    partial_path.to_string_lossy(),
                                    error
                                )
                            })?;
                        writer.write_all(&buffer[..read]).map_err(|error| {
                            format!(
                                "Unable to patch the partial image {} during targeted cautious rescue: {}",
                                partial_path.to_string_lossy(),
                                error
                            )
                        })?;
                        refinement_summary.rescued_after_retry_bytes = refinement_summary
                            .rescued_after_retry_bytes
                            .saturating_add(read as u64);
                        subtract_resolved_span(&mut range_unresolved, offset, read as u64);

                        if read < chunk_length && chunk_length == pass.block_size {
                            if let Some(split_block_size) = pass.partial_progress_split_block_size {
                                let unresolved_tail = UnreadableRange {
                                    start_offset: offset.saturating_add(read as u64),
                                    length: (chunk_length - read) as u64,
                                };
                                schedule_front_chunks(
                                    &mut pending_chunks,
                                    &mut scheduled_chunks,
                                    refinement_chunks_for_range(
                                        &unresolved_tail,
                                        split_block_size,
                                        RetryPassMode::Sequential(RetryTraversalDirection::Forward),
                                    ),
                                );
                            }
                        }

                        if read == chunk_length && chunk_length == pass.block_size {
                            if let Some(zoom_block_size) = pass.neighbor_zoom_block_size {
                                let resolved_end = offset.saturating_add(read as u64);
                                if let Some(candidate) = rightmost_left_zoom_candidate(
                                    &range_unresolved,
                                    offset,
                                    zoom_block_size,
                                ) {
                                    schedule_front_chunks(
                                        &mut pending_chunks,
                                        &mut scheduled_chunks,
                                        vec![candidate],
                                    );
                                }
                                if let Some(candidate) = leftmost_right_zoom_candidate(
                                    &range_unresolved,
                                    resolved_end,
                                    zoom_block_size,
                                ) {
                                    schedule_front_chunks(
                                        &mut pending_chunks,
                                        &mut scheduled_chunks,
                                        vec![candidate],
                                    );
                                }
                            }
                        }
                    }
                    ChunkReadOutcome::Unreadable => {}
                }
            }

            next_unresolved_segments.extend(range_unresolved);
        }

        writer.flush().map_err(|error| {
            format!(
                "Unable to flush the targeted cautious rescue updates for {}: {}",
                partial_path.to_string_lossy(),
                error
            )
        })?;

        let next_unresolved_ranges = merge_unreadable_ranges(next_unresolved_segments);

        let next_summary = add_issue_summaries(
            carried_unmapped_issue_summary,
            summarize_unreadable_ranges(&next_unresolved_ranges),
        );
        persist_partial_image_rescue_state(
            checkpoint_path,
            checkpoint_seed,
            next_summary,
            &next_unresolved_ranges,
            refinement_summary,
        )?;
        unresolved_ranges = next_unresolved_ranges;
    }

    Ok((unresolved_ranges, refinement_summary))
}

fn partial_image_checkpoint_path(partial_path: &Path) -> Result<PathBuf, String> {
    let file_name = partial_path.file_name().ok_or_else(|| {
        format!(
            "The partial image path {} is missing a file name.",
            partial_path.to_string_lossy()
        )
    })?;
    Ok(partial_path.with_file_name(format!("{}.json", file_name.to_string_lossy())))
}

fn write_partial_image_checkpoint(
    checkpoint_path: &Path,
    checkpoint: &PartialImageCheckpoint,
) -> Result<(), String> {
    let payload = serde_json::to_vec_pretty(checkpoint)
        .map_err(|error| format!("Unable to serialize the partial imaging checkpoint: {error}"))?;
    fs::write(checkpoint_path, payload).map_err(|error| {
        format!(
            "Unable to write the partial imaging checkpoint {}: {}",
            checkpoint_path.to_string_lossy(),
            error
        )
    })
}

fn load_partial_image_checkpoint(checkpoint_path: &Path) -> Result<PartialImageCheckpoint, String> {
    let payload = fs::read(checkpoint_path).map_err(|error| {
        format!(
            "Unable to read the partial imaging checkpoint {}: {}",
            checkpoint_path.to_string_lossy(),
            error
        )
    })?;
    serde_json::from_slice(&payload)
        .map_err(|error| format!("Unable to parse the partial imaging checkpoint: {error}"))
}

fn remove_partial_image_checkpoint(checkpoint_path: &Path) {
    let _ = fs::remove_file(checkpoint_path);
}

fn discard_partial_image_resume_state(partial_path: &Path, checkpoint_path: &Path) {
    let _ = fs::remove_file(partial_path);
    remove_partial_image_checkpoint(checkpoint_path);
}

fn persist_partial_image_rescue_state(
    checkpoint_path: &Path,
    checkpoint_seed: &PartialImageCheckpoint,
    issue_summary: ImagingIssueSummary,
    unresolved_ranges: &[UnreadableRange],
    retry_refinement_summary: RetryRefinementSummary,
) -> Result<(), String> {
    let mut checkpoint = checkpoint_seed.clone();
    checkpoint.unreadable_ranges_count = issue_summary.unreadable_ranges_count;
    checkpoint.unreadable_bytes = issue_summary.unreadable_bytes;
    checkpoint.unreadable_ranges = unresolved_ranges.to_vec();
    checkpoint.rescued_after_retry_bytes = retry_refinement_summary.rescued_after_retry_bytes;
    checkpoint.retry_passes_completed = retry_refinement_summary.retry_passes_completed;
    write_partial_image_checkpoint(checkpoint_path, &checkpoint)
}

fn prepare_partial_image_destination(
    partial_path: &Path,
    source_path: &Path,
    start_offset_bytes: u64,
    requested_length_bytes: Option<u64>,
    source_length_bytes: Option<u64>,
    validated_copy_length_bytes: Option<u64>,
) -> Result<PreparedPartialImageDestination, String> {
    let checkpoint_path = partial_image_checkpoint_path(partial_path)?;
    let expected_checkpoint = PartialImageCheckpoint {
        source_path: source_path.to_string_lossy().to_string(),
        start_offset_bytes,
        requested_length_bytes,
        source_length_bytes,
        unreadable_ranges_count: 0,
        unreadable_bytes: 0,
        unreadable_ranges: Vec::new(),
        rescued_after_retry_bytes: 0,
        retry_passes_completed: 0,
    };

    let mut resume_from_bytes = 0_u64;
    let mut issue_summary = ImagingIssueSummary::default();
    let mut carried_unmapped_issue_summary = ImagingIssueSummary::default();
    let mut unresolved_ranges = Vec::new();
    let mut retry_refinement_summary = RetryRefinementSummary::default();
    let partial_exists = partial_path.exists();
    let checkpoint_exists = checkpoint_path.exists();

    if partial_exists && checkpoint_exists {
        let saved_checkpoint = match load_partial_image_checkpoint(&checkpoint_path) {
            Ok(checkpoint) => checkpoint,
            Err(_) => {
                discard_partial_image_resume_state(partial_path, &checkpoint_path);
                persist_partial_image_rescue_state(
                    &checkpoint_path,
                    &expected_checkpoint,
                    issue_summary,
                    &unresolved_ranges,
                    retry_refinement_summary,
                )?;
                return Ok(PreparedPartialImageDestination {
                    resume_from_bytes: 0,
                    issue_summary,
                    carried_unmapped_issue_summary,
                    unresolved_ranges,
                    retry_refinement_summary,
                    checkpoint_path,
                    checkpoint_seed: expected_checkpoint,
                });
            }
        };
        let partial_size = fs::metadata(partial_path)
            .map_err(|error| {
                format!(
                    "Unable to inspect the partial image {}: {}",
                    partial_path.to_string_lossy(),
                    error
                )
            })?
            .len();
        let checkpoint_matches = saved_checkpoint.source_path == expected_checkpoint.source_path
            && saved_checkpoint.start_offset_bytes == expected_checkpoint.start_offset_bytes
            && saved_checkpoint.requested_length_bytes
                == expected_checkpoint.requested_length_bytes
            && saved_checkpoint.source_length_bytes == expected_checkpoint.source_length_bytes;
        let partial_within_bounds = validated_copy_length_bytes
            .map(|limit| partial_size <= limit)
            .unwrap_or(true);

        if checkpoint_matches && partial_within_bounds {
            resume_from_bytes = partial_size;
            issue_summary = ImagingIssueSummary {
                unreadable_ranges_count: saved_checkpoint.unreadable_ranges_count,
                unreadable_bytes: saved_checkpoint.unreadable_bytes,
            };
            unresolved_ranges = saved_checkpoint.unreadable_ranges;
            let unresolved_summary = summarize_unreadable_ranges(&unresolved_ranges);
            carried_unmapped_issue_summary =
                subtract_issue_summaries(issue_summary, unresolved_summary);
            retry_refinement_summary = RetryRefinementSummary {
                rescued_after_retry_bytes: saved_checkpoint.rescued_after_retry_bytes,
                retry_passes_completed: saved_checkpoint.retry_passes_completed,
            };
        } else {
            discard_partial_image_resume_state(partial_path, &checkpoint_path);
        }
    } else if partial_exists || checkpoint_exists {
        discard_partial_image_resume_state(partial_path, &checkpoint_path);
    }

    persist_partial_image_rescue_state(
        &checkpoint_path,
        &expected_checkpoint,
        issue_summary,
        &unresolved_ranges,
        retry_refinement_summary,
    )?;
    Ok(PreparedPartialImageDestination {
        resume_from_bytes,
        issue_summary,
        carried_unmapped_issue_summary,
        unresolved_ranges,
        retry_refinement_summary,
        checkpoint_path,
        checkpoint_seed: expected_checkpoint,
    })
}

fn finalize_partial_image(destination_path: &Path, partial_path: &Path) -> Result<(), String> {
    if destination_path.exists() {
        fs::remove_file(destination_path).map_err(|error| {
            format!(
                "Unable to replace the previous read-only image {}: {}",
                destination_path.to_string_lossy(),
                error
            )
        })?;
    }

    fs::rename(partial_path, destination_path).map_err(|error| {
        format!(
            "Unable to finalize the read-only image {}: {}",
            destination_path.to_string_lossy(),
            error
        )
    })?;

    if let Ok(checkpoint_path) = partial_image_checkpoint_path(partial_path) {
        remove_partial_image_checkpoint(&checkpoint_path);
    }

    Ok(())
}

pub fn materialize_byte_runs(
    image_path: &Path,
    byte_runs: &[ByteRun],
    bytes_to_materialize: u64,
    target_path: &Path,
) -> Result<u64, String> {
    let compression_kind = compression_kind_for_runs(byte_runs)?;
    let parent = target_path.parent().ok_or_else(|| {
        format!(
            "The reconstructed target {} has no writable parent directory.",
            target_path.to_string_lossy()
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "Unable to create the reconstructed destination directory {}: {}",
            parent.to_string_lossy(),
            error
        )
    })?;

    let mut source = File::open(image_path).map_err(|error| {
        format!(
            "Unable to open the recovery image {}: {}",
            image_path.to_string_lossy(),
            error
        )
    })?;
    let mut target = BufWriter::new(File::create(target_path).map_err(|error| {
        format!(
            "Unable to create the reconstructed file {}: {}",
            target_path.to_string_lossy(),
            error
        )
    })?);

    if compression_kind.is_some() {
        let decompressed =
            read_decompressed_byte_runs(&mut source, byte_runs, bytes_to_materialize)?;
        target
            .write_all(&decompressed[..bytes_to_materialize as usize])
            .map_err(|error| {
                format!("Unable to materialize compressed reconstructed bytes: {error}")
            })?;
        target
            .flush()
            .map_err(|error| format!("Unable to flush the reconstructed file: {error}"))?;
        return Ok(bytes_to_materialize);
    }

    let mut remaining = bytes_to_materialize;
    let mut written = 0_u64;

    for run in byte_runs {
        if remaining == 0 {
            break;
        }

        if run.zero_fill {
            let run_length = run.length.min(remaining);
            write_zero_fill(&mut target, run_length)?;
            written = written.saturating_add(run_length);
            remaining = remaining.saturating_sub(run_length);
            continue;
        }

        source
            .seek(SeekFrom::Start(run.offset))
            .map_err(|error| format!("Unable to seek the recovery image: {error}"))?;

        let run_length = run.length.min(remaining);
        let mut limited = std::io::Read::take(&mut source, run_length);
        let copied = std::io::copy(&mut limited, &mut target)
            .map_err(|error| format!("Unable to materialize reconstructed file bytes: {error}"))?;
        written = written.saturating_add(copied);
        remaining = remaining.saturating_sub(copied);
    }

    target
        .flush()
        .map_err(|error| format!("Unable to flush the reconstructed file: {error}"))?;

    if written != bytes_to_materialize {
        return Err(format!(
            "Reconstructed file size mismatch: expected {} bytes, wrote {} bytes.",
            bytes_to_materialize, written
        ));
    }

    Ok(written)
}

fn write_zero_fill(writer: &mut impl Write, length: u64) -> Result<(), String> {
    let chunk = [0_u8; 8192];
    let mut remaining = length;
    while remaining > 0 {
        let to_write = remaining.min(chunk.len() as u64) as usize;
        writer
            .write_all(&chunk[..to_write])
            .map_err(|error| format!("Unable to materialize zero-filled sparse bytes: {error}"))?;
        remaining = remaining.saturating_sub(to_write as u64);
    }
    Ok(())
}

pub fn read_byte_runs(
    image_path: &Path,
    byte_runs: &[ByteRun],
    bytes_to_read: u64,
) -> Result<Vec<u8>, String> {
    read_byte_runs_range(image_path, byte_runs, 0, bytes_to_read)
}

pub fn read_byte_runs_range(
    image_path: &Path,
    byte_runs: &[ByteRun],
    start_offset: u64,
    bytes_to_read: u64,
) -> Result<Vec<u8>, String> {
    if bytes_to_read == 0 {
        return Ok(Vec::new());
    }

    let compression_kind = compression_kind_for_runs(byte_runs)?;
    let mut source = File::open(image_path).map_err(|error| {
        format!(
            "Unable to open the recovery image {}: {}",
            image_path.to_string_lossy(),
            error
        )
    })?;

    if compression_kind.is_some() {
        let required_logical_bytes = start_offset
            .checked_add(bytes_to_read)
            .ok_or_else(|| "Requested compressed preview range overflowed.".to_string())?;
        let decompressed =
            read_decompressed_byte_runs(&mut source, byte_runs, required_logical_bytes)?;
        return Ok(decompressed[start_offset as usize..required_logical_bytes as usize].to_vec());
    }

    let mut remaining = bytes_to_read;
    let mut bytes = Vec::with_capacity(bytes_to_read as usize);
    let mut skipped = 0_u64;

    for run in byte_runs {
        if remaining == 0 {
            break;
        }

        if skipped + run.length <= start_offset {
            skipped = skipped.saturating_add(run.length);
            continue;
        }

        let offset_within_run = start_offset.saturating_sub(skipped).min(run.length);
        let readable_length = run.length.saturating_sub(offset_within_run);
        if readable_length == 0 {
            skipped = skipped.saturating_add(run.length);
            continue;
        }

        let run_length = readable_length.min(remaining);
        if run.zero_fill {
            bytes.resize(bytes.len().saturating_add(run_length as usize), 0);
        } else {
            source
                .seek(SeekFrom::Start(
                    run.offset.saturating_add(offset_within_run),
                ))
                .map_err(|error| format!("Unable to seek the recovery image: {error}"))?;

            let mut limited = std::io::Read::take(&mut source, run_length);
            limited
                .read_to_end(&mut bytes)
                .map_err(|error| format!("Unable to read reconstructed preview bytes: {error}"))?;
        }
        remaining = bytes_to_read.saturating_sub(bytes.len() as u64);
        skipped = skipped.saturating_add(run.length);
    }

    if bytes.len() as u64 != bytes_to_read {
        return Err(format!(
            "Preview byte count mismatch: expected {} bytes, read {} bytes.",
            bytes_to_read,
            bytes.len()
        ));
    }

    Ok(bytes)
}

fn compression_kind_for_runs(byte_runs: &[ByteRun]) -> Result<Option<&str>, String> {
    let mut compression_kind = None;

    for run in byte_runs {
        if run.zero_fill && run.compression_kind.is_some() {
            return Err(
                "Compressed reconstructed byte runs cannot include synthetic zero-fill segments."
                    .into(),
            );
        }

        match (compression_kind, run.compression_kind.as_deref()) {
            (None, None) => {}
            (None, Some(kind)) => compression_kind = Some(kind),
            (Some(current), Some(kind)) if current == kind => {}
            (Some(_), Some(_)) => {
                return Err(
                    "Mixed compression kinds are not supported in reconstructed byte runs.".into(),
                )
            }
            (Some(_), None) => {
                return Err(
                    "Mixed compressed and uncompressed reconstructed byte runs are not supported."
                        .into(),
                )
            }
        }
    }

    if let Some(kind) = compression_kind {
        if kind != "lznt1" {
            return Err(format!(
                "Unsupported reconstructed compression kind {kind}."
            ));
        }
    }

    Ok(compression_kind)
}

fn read_raw_byte_runs_from_source(
    source: &mut File,
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
                "Compressed reconstructed byte runs cannot include synthetic zero-fill segments."
                    .into(),
            );
        }

        let to_read = run.length.min(remaining);
        let mut buffer = vec![0_u8; to_read as usize];
        source
            .seek(SeekFrom::Start(run.offset))
            .map_err(|error| format!("Unable to seek the recovery image: {error}"))?;
        source
            .read_exact(&mut buffer)
            .map_err(|error| format!("Unable to read compressed reconstructed bytes: {error}"))?;
        bytes.extend_from_slice(&buffer);
        remaining = remaining.saturating_sub(to_read);
    }

    if remaining > 0 {
        return Err(
            "Compressed reconstructed byte runs do not cover the advertised stored size.".into(),
        );
    }

    Ok(bytes)
}

fn read_decompressed_byte_runs(
    source: &mut File,
    byte_runs: &[ByteRun],
    required_logical_bytes: u64,
) -> Result<Vec<u8>, String> {
    let compressed_size = byte_runs
        .iter()
        .try_fold(0_u64, |total, run| total.checked_add(run.length))
        .ok_or_else(|| {
            "Compressed reconstructed byte runs overflowed their stored size.".to_string()
        })?;
    let compressed_bytes = read_raw_byte_runs_from_source(source, byte_runs, compressed_size)?;
    let mut decompressed = Vec::new();
    decompress_lznt1(&compressed_bytes, &mut decompressed)
        .map_err(|error| format!("Unable to decompress reconstructed LZNT1 payload: {error}"))?;

    if (decompressed.len() as u64) < required_logical_bytes {
        return Err(format!(
            "Compressed reconstructed payload is shorter than expected: required {} bytes, got {}.",
            required_logical_bytes,
            decompressed.len()
        ));
    }

    Ok(decompressed)
}

/// Probe the byte length of a source file when `metadata().len()` returns 0.
/// On Unix, block devices like `/dev/disk4s2` advertise a stat size of zero
/// even though they are perfectly seekable. We rediscover the length by seeking
/// to the end and reading the resulting position, then restoring the cursor.
fn probe_block_device_length(file: &mut File) -> Option<u64> {
    let original = file.stream_position().ok()?;
    let end = file.seek(SeekFrom::End(0)).ok()?;
    let _ = file.seek(SeekFrom::Start(original));
    if end > 0 {
        Some(end)
    } else {
        None
    }
}

fn imaging_workspace_dir() -> PathBuf {
    if let Some(path) = env::var_os("RECUPERE_WORKSPACE_PATH") {
        return PathBuf::from(path);
    }

    env::temp_dir().join("recupere-workspace").join("images")
}

fn sanitize_scan_id(scan_id: &str) -> String {
    scan_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn build_partial_image_path(destination_path: &Path) -> Result<PathBuf, String> {
    let file_name = destination_path.file_name().ok_or_else(|| {
        format!(
            "The selected image destination {} is missing a file name.",
            destination_path.to_string_lossy()
        )
    })?;
    let partial_name = format!("{}.partial", file_name.to_string_lossy());
    Ok(destination_path.with_file_name(partial_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lznt1::compress as compress_lznt1;
    use std::cmp::min;
    use std::io::{self, Read, Seek, SeekFrom};
    use std::time::Duration;

    struct FaultyReader {
        bytes: Vec<u8>,
        position: u64,
        unreadable_ranges: Vec<(u64, u64)>,
    }

    impl FaultyReader {
        fn new(bytes: Vec<u8>, unreadable_ranges: Vec<(u64, u64)>) -> Self {
            Self {
                bytes,
                position: 0,
                unreadable_ranges,
            }
        }

        fn current_range_is_unreadable(&self) -> bool {
            self.unreadable_ranges.iter().any(|(start, length)| {
                let end = start.saturating_add(*length);
                self.position >= *start && self.position < end
            })
        }

        fn next_unreadable_start(&self) -> u64 {
            self.unreadable_ranges
                .iter()
                .filter(|(start, _)| *start > self.position)
                .map(|(start, _)| *start)
                .min()
                .unwrap_or(self.bytes.len() as u64)
        }
    }

    impl Read for FaultyReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.position >= self.bytes.len() as u64 {
                return Ok(0);
            }

            if self.current_range_is_unreadable() {
                return Err(io::Error::other("synthetic unreadable sector"));
            }

            let readable_until = self.next_unreadable_start();
            let available = min(
                buffer.len(),
                min(
                    readable_until.saturating_sub(self.position) as usize,
                    self.bytes.len().saturating_sub(self.position as usize),
                ),
            );

            buffer[..available].copy_from_slice(
                &self.bytes[self.position as usize..self.position as usize + available],
            );
            self.position = self.position.saturating_add(available as u64);
            Ok(available)
        }
    }

    impl Seek for FaultyReader {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            let next_position = match pos {
                SeekFrom::Start(offset) => offset as i128,
                SeekFrom::Current(delta) => self.position as i128 + delta as i128,
                SeekFrom::End(delta) => self.bytes.len() as i128 + delta as i128,
            };

            if next_position < 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "negative seek is not allowed",
                ));
            }

            self.position = next_position as u64;
            Ok(self.position)
        }
    }

    struct RetrySensitiveReader {
        bytes: Vec<u8>,
        position: u64,
        always_unreadable_ranges: Vec<(u64, u64)>,
        deferred_chunk_start: u64,
        deferred_chunk_length: usize,
        attempt_counts: std::collections::HashMap<(u64, usize), u8>,
    }

    impl RetrySensitiveReader {
        fn new(
            bytes: Vec<u8>,
            always_unreadable_ranges: Vec<(u64, u64)>,
            deferred_chunk_start: u64,
            deferred_chunk_length: usize,
        ) -> Self {
            Self {
                bytes,
                position: 0,
                always_unreadable_ranges,
                deferred_chunk_start,
                deferred_chunk_length,
                attempt_counts: std::collections::HashMap::new(),
            }
        }

        fn next_problem_start(&self) -> u64 {
            let next_always_unreadable = self
                .always_unreadable_ranges
                .iter()
                .filter(|(start, _)| *start > self.position)
                .map(|(start, _)| *start)
                .min();
            let next_deferred =
                (self.deferred_chunk_start > self.position).then_some(self.deferred_chunk_start);

            match (next_always_unreadable, next_deferred) {
                (Some(left), Some(right)) => left.min(right),
                (Some(left), None) => left,
                (None, Some(right)) => right,
                (None, None) => self.bytes.len() as u64,
            }
        }
    }

    impl Read for RetrySensitiveReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.position >= self.bytes.len() as u64 {
                return Ok(0);
            }

            let key = (self.position, buffer.len());
            let attempts = self.attempt_counts.entry(key).or_insert(0);
            *attempts = attempts.saturating_add(1);

            if self.always_unreadable_ranges.iter().any(|(start, length)| {
                let end = start.saturating_add(*length);
                self.position >= *start && self.position < end
            }) {
                return Err(io::Error::other("synthetic permanently unreadable range"));
            }

            if self.position == self.deferred_chunk_start
                && (buffer.len() != self.deferred_chunk_length || *attempts < 2)
            {
                return Err(io::Error::other(
                    "synthetic deferred chunk remains unreadable",
                ));
            }

            if self.position > self.deferred_chunk_start
                && self.position
                    < self
                        .deferred_chunk_start
                        .saturating_add(self.deferred_chunk_length as u64)
            {
                return Err(io::Error::other(
                    "synthetic subchunk remains unreadable until full reverse retry",
                ));
            }

            let available = min(
                buffer.len(),
                min(
                    self.next_problem_start().saturating_sub(self.position) as usize,
                    self.bytes.len().saturating_sub(self.position as usize),
                ),
            );
            buffer[..available].copy_from_slice(
                &self.bytes[self.position as usize..self.position as usize + available],
            );
            self.position = self.position.saturating_add(available as u64);
            Ok(available)
        }
    }

    impl Seek for RetrySensitiveReader {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            let next_position = match pos {
                SeekFrom::Start(offset) => offset as i128,
                SeekFrom::Current(delta) => self.position as i128 + delta as i128,
                SeekFrom::End(delta) => self.bytes.len() as i128 + delta as i128,
            };

            if next_position < 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "negative seek is not allowed",
                ));
            }

            self.position = next_position as u64;
            Ok(self.position)
        }
    }

    struct EdgeTrimSensitiveReader {
        bytes: Vec<u8>,
        position: u64,
        protected_range_start: u64,
        protected_range_length: usize,
        readable_edge_chunk_size: usize,
    }

    impl EdgeTrimSensitiveReader {
        fn new(
            bytes: Vec<u8>,
            protected_range_start: u64,
            protected_range_length: usize,
            readable_edge_chunk_size: usize,
        ) -> Self {
            Self {
                bytes,
                position: 0,
                protected_range_start,
                protected_range_length,
                readable_edge_chunk_size,
            }
        }

        fn protected_range_end(&self) -> u64 {
            self.protected_range_start
                .saturating_add(self.protected_range_length as u64)
        }
    }

    impl Read for EdgeTrimSensitiveReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.position >= self.bytes.len() as u64 {
                return Ok(0);
            }

            let range_start = self.protected_range_start;
            let range_end = self.protected_range_end();
            if self.position >= range_start && self.position < range_end {
                let trailing_edge_start =
                    range_end.saturating_sub(self.readable_edge_chunk_size as u64);
                let allowed_edge = (self.position == range_start
                    || self.position == trailing_edge_start)
                    && buffer.len() == self.readable_edge_chunk_size;

                if !allowed_edge {
                    return Err(io::Error::other(
                        "synthetic edge-trim-only range remains unreadable",
                    ));
                }
            }

            let next_boundary = if self.position < range_start {
                range_start
            } else {
                self.bytes.len() as u64
            };
            let available = min(
                buffer.len(),
                min(
                    next_boundary.saturating_sub(self.position) as usize,
                    self.bytes.len().saturating_sub(self.position as usize),
                ),
            );
            buffer[..available].copy_from_slice(
                &self.bytes[self.position as usize..self.position as usize + available],
            );
            self.position = self.position.saturating_add(available as u64);
            Ok(available)
        }
    }

    impl Seek for EdgeTrimSensitiveReader {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            let next_position = match pos {
                SeekFrom::Start(offset) => offset as i128,
                SeekFrom::Current(delta) => self.position as i128 + delta as i128,
                SeekFrom::End(delta) => self.bytes.len() as i128 + delta as i128,
            };

            if next_position < 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "negative seek is not allowed",
                ));
            }

            self.position = next_position as u64;
            Ok(self.position)
        }
    }

    struct CenterIslandSensitiveReader {
        bytes: Vec<u8>,
        position: u64,
        protected_range_start: u64,
        protected_range_length: usize,
        readable_island_start: u64,
        readable_island_length: usize,
    }

    impl CenterIslandSensitiveReader {
        fn new(
            bytes: Vec<u8>,
            protected_range_start: u64,
            protected_range_length: usize,
            readable_island_start: u64,
            readable_island_length: usize,
        ) -> Self {
            Self {
                bytes,
                position: 0,
                protected_range_start,
                protected_range_length,
                readable_island_start,
                readable_island_length,
            }
        }

        fn protected_range_end(&self) -> u64 {
            self.protected_range_start
                .saturating_add(self.protected_range_length as u64)
        }
    }

    impl Read for CenterIslandSensitiveReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.position >= self.bytes.len() as u64 {
                return Ok(0);
            }

            let range_start = self.protected_range_start;
            let range_end = self.protected_range_end();
            if self.position >= range_start && self.position < range_end {
                let allowed_island = self.position == self.readable_island_start
                    && buffer.len() == self.readable_island_length;

                if !allowed_island {
                    return Err(io::Error::other(
                        "synthetic center-island range remains unreadable",
                    ));
                }
            }

            let next_boundary = if self.position < range_start {
                range_start
            } else {
                self.bytes.len() as u64
            };
            let available = min(
                buffer.len(),
                min(
                    next_boundary.saturating_sub(self.position) as usize,
                    self.bytes.len().saturating_sub(self.position as usize),
                ),
            );
            buffer[..available].copy_from_slice(
                &self.bytes[self.position as usize..self.position as usize + available],
            );
            self.position = self.position.saturating_add(available as u64);
            Ok(available)
        }
    }

    impl Seek for CenterIslandSensitiveReader {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            let next_position = match pos {
                SeekFrom::Start(offset) => offset as i128,
                SeekFrom::Current(delta) => self.position as i128 + delta as i128,
                SeekFrom::End(delta) => self.bytes.len() as i128 + delta as i128,
            };

            if next_position < 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "negative seek is not allowed",
                ));
            }

            self.position = next_position as u64;
            Ok(self.position)
        }
    }

    struct MicroScrapeSensitiveReader {
        bytes: Vec<u8>,
        position: u64,
        protected_range_start: u64,
        protected_range_length: usize,
        readable_micro_chunk_start: u64,
        readable_micro_chunk_length: usize,
    }

    impl MicroScrapeSensitiveReader {
        fn new(
            bytes: Vec<u8>,
            protected_range_start: u64,
            protected_range_length: usize,
            readable_micro_chunk_start: u64,
            readable_micro_chunk_length: usize,
        ) -> Self {
            Self {
                bytes,
                position: 0,
                protected_range_start,
                protected_range_length,
                readable_micro_chunk_start,
                readable_micro_chunk_length,
            }
        }

        fn protected_range_end(&self) -> u64 {
            self.protected_range_start
                .saturating_add(self.protected_range_length as u64)
        }
    }

    impl Read for MicroScrapeSensitiveReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.position >= self.bytes.len() as u64 {
                return Ok(0);
            }

            let range_start = self.protected_range_start;
            let range_end = self.protected_range_end();
            if self.position >= range_start && self.position < range_end {
                let allowed_chunk = self.position == self.readable_micro_chunk_start
                    && buffer.len() == self.readable_micro_chunk_length;

                if !allowed_chunk {
                    return Err(io::Error::other(
                        "synthetic micro-scrape range remains unreadable",
                    ));
                }
            }

            let next_boundary = if self.position < range_start {
                range_start
            } else {
                self.bytes.len() as u64
            };
            let available = min(
                buffer.len(),
                min(
                    next_boundary.saturating_sub(self.position) as usize,
                    self.bytes.len().saturating_sub(self.position as usize),
                ),
            );
            buffer[..available].copy_from_slice(
                &self.bytes[self.position as usize..self.position as usize + available],
            );
            self.position = self.position.saturating_add(available as u64);
            Ok(available)
        }
    }

    impl Seek for MicroScrapeSensitiveReader {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            let next_position = match pos {
                SeekFrom::Start(offset) => offset as i128,
                SeekFrom::Current(delta) => self.position as i128 + delta as i128,
                SeekFrom::End(delta) => self.bytes.len() as i128 + delta as i128,
            };

            if next_position < 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "negative seek is not allowed",
                ));
            }

            self.position = next_position as u64;
            Ok(self.position)
        }
    }

    struct NeighborZoomSensitiveReader {
        bytes: Vec<u8>,
        position: u64,
        protected_range_start: u64,
        protected_range_length: usize,
        center_chunk_start: u64,
        center_chunk_length: usize,
        left_neighbor_start: u64,
        right_neighbor_start: u64,
        neighbor_chunk_length: usize,
        follow_up_armed: bool,
        left_neighbor_recovered: bool,
        right_neighbor_recovered: bool,
    }

    impl NeighborZoomSensitiveReader {
        fn new(
            bytes: Vec<u8>,
            protected_range_start: u64,
            protected_range_length: usize,
            center_chunk_start: u64,
            center_chunk_length: usize,
            neighbor_chunk_length: usize,
        ) -> Self {
            Self {
                bytes,
                position: 0,
                protected_range_start,
                protected_range_length,
                center_chunk_start,
                center_chunk_length,
                left_neighbor_start: center_chunk_start
                    .saturating_sub(neighbor_chunk_length as u64),
                right_neighbor_start: center_chunk_start.saturating_add(center_chunk_length as u64),
                neighbor_chunk_length,
                follow_up_armed: false,
                left_neighbor_recovered: false,
                right_neighbor_recovered: false,
            }
        }

        fn protected_range_end(&self) -> u64 {
            self.protected_range_start
                .saturating_add(self.protected_range_length as u64)
        }

        fn fill_current_chunk_up_to(&mut self, buffer: &mut [u8], upper_bound: u64) -> usize {
            let available = min(
                buffer.len(),
                min(
                    upper_bound.saturating_sub(self.position) as usize,
                    self.bytes.len().saturating_sub(self.position as usize),
                ),
            );
            buffer[..available].copy_from_slice(
                &self.bytes[self.position as usize..self.position as usize + available],
            );
            self.position = self.position.saturating_add(available as u64);
            available
        }
    }

    impl Read for NeighborZoomSensitiveReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.position >= self.bytes.len() as u64 {
                return Ok(0);
            }

            let protected_start = self.protected_range_start;
            let protected_end = self.protected_range_end();
            if self.position >= protected_start && self.position < protected_end {
                let is_center_chunk = self.position == self.center_chunk_start
                    && buffer.len() == self.center_chunk_length;
                let is_left_neighbor = self.follow_up_armed
                    && !self.left_neighbor_recovered
                    && self.position == self.left_neighbor_start
                    && buffer.len() == self.neighbor_chunk_length;
                let is_right_neighbor = self.follow_up_armed
                    && !self.right_neighbor_recovered
                    && self.position == self.right_neighbor_start
                    && buffer.len() == self.neighbor_chunk_length;

                if is_center_chunk {
                    self.follow_up_armed = true;
                    return Ok(self.fill_current_chunk_up_to(buffer, protected_end));
                }

                if is_left_neighbor {
                    self.left_neighbor_recovered = true;
                    self.follow_up_armed = !self.right_neighbor_recovered;
                    return Ok(self.fill_current_chunk_up_to(buffer, protected_end));
                }

                if is_right_neighbor {
                    self.right_neighbor_recovered = true;
                    self.follow_up_armed = !self.left_neighbor_recovered;
                    return Ok(self.fill_current_chunk_up_to(buffer, protected_end));
                }

                self.follow_up_armed = false;
                return Err(io::Error::other(
                    "synthetic neighbor zoom window expired before adjacent probes",
                ));
            }

            let next_boundary = if self.position < protected_start {
                protected_start
            } else {
                self.bytes.len() as u64
            };

            Ok(self.fill_current_chunk_up_to(buffer, next_boundary))
        }
    }

    impl Seek for NeighborZoomSensitiveReader {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            let next_position = match pos {
                SeekFrom::Start(offset) => offset as i128,
                SeekFrom::Current(delta) => self.position as i128 + delta as i128,
                SeekFrom::End(delta) => self.bytes.len() as i128 + delta as i128,
            };

            if next_position < 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "negative seek is not allowed",
                ));
            }

            self.position = next_position as u64;
            Ok(self.position)
        }
    }

    struct PartialProgressSplitSensitiveReader {
        bytes: Vec<u8>,
        position: u64,
        protected_range_start: u64,
        protected_range_length: usize,
        pivot_chunk_start: u64,
        pivot_chunk_length: usize,
        partial_read_length: usize,
        follow_up_tail_start: u64,
        follow_up_tail_length: usize,
        tail_window_armed: bool,
        tail_recovered: bool,
    }

    impl PartialProgressSplitSensitiveReader {
        fn new(
            bytes: Vec<u8>,
            protected_range_start: u64,
            protected_range_length: usize,
            pivot_chunk_start: u64,
            pivot_chunk_length: usize,
            partial_read_length: usize,
        ) -> Self {
            Self {
                bytes,
                position: 0,
                protected_range_start,
                protected_range_length,
                pivot_chunk_start,
                pivot_chunk_length,
                partial_read_length,
                follow_up_tail_start: pivot_chunk_start.saturating_add(partial_read_length as u64),
                follow_up_tail_length: pivot_chunk_length.saturating_sub(partial_read_length),
                tail_window_armed: false,
                tail_recovered: false,
            }
        }

        fn protected_range_end(&self) -> u64 {
            self.protected_range_start
                .saturating_add(self.protected_range_length as u64)
        }

        fn fill_current_chunk_up_to(&mut self, buffer: &mut [u8], upper_bound: u64) -> usize {
            let available = min(
                buffer.len(),
                min(
                    upper_bound.saturating_sub(self.position) as usize,
                    self.bytes.len().saturating_sub(self.position as usize),
                ),
            );
            buffer[..available].copy_from_slice(
                &self.bytes[self.position as usize..self.position as usize + available],
            );
            self.position = self.position.saturating_add(available as u64);
            available
        }
    }

    impl Read for PartialProgressSplitSensitiveReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.position >= self.bytes.len() as u64 {
                return Ok(0);
            }

            let protected_start = self.protected_range_start;
            let protected_end = self.protected_range_end();
            if self.position >= protected_start && self.position < protected_end {
                let is_partial_pivot = !self.tail_recovered
                    && self.position == self.pivot_chunk_start
                    && buffer.len() == self.pivot_chunk_length;
                let is_immediate_tail = self.tail_window_armed
                    && !self.tail_recovered
                    && self.position == self.follow_up_tail_start
                    && buffer.len() == self.follow_up_tail_length;

                if is_partial_pivot {
                    self.tail_window_armed = true;
                    let upper_bound = self
                        .position
                        .saturating_add(self.partial_read_length as u64);
                    return Ok(self.fill_current_chunk_up_to(buffer, upper_bound));
                }

                if is_immediate_tail {
                    self.tail_window_armed = false;
                    self.tail_recovered = true;
                    return Ok(self.fill_current_chunk_up_to(buffer, protected_end));
                }

                self.tail_window_armed = false;
                return Err(io::Error::other(
                    "synthetic adaptive split tail expired before immediate follow-up",
                ));
            }

            let next_boundary = if self.position < protected_start {
                protected_start
            } else {
                self.bytes.len() as u64
            };

            Ok(self.fill_current_chunk_up_to(buffer, next_boundary))
        }
    }

    impl Seek for PartialProgressSplitSensitiveReader {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            let next_position = match pos {
                SeekFrom::Start(offset) => offset as i128,
                SeekFrom::Current(delta) => self.position as i128 + delta as i128,
                SeekFrom::End(delta) => self.bytes.len() as i128 + delta as i128,
            };

            if next_position < 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "negative seek is not allowed",
                ));
            }

            self.position = next_position as u64;
            Ok(self.position)
        }
    }

    fn image_reader_to_destination<R: Read + Seek>(
        reader: &mut R,
        destination_path: &Path,
        source_path: &Path,
        total_bytes: u64,
        profile: ImagingProfile,
    ) -> Result<ImageArtifact, String> {
        let parent = destination_path.parent().ok_or_else(|| {
            format!(
                "The selected image destination {} has no writable parent directory.",
                destination_path.to_string_lossy()
            )
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Unable to prepare the image destination directory {}: {}",
                parent.to_string_lossy(),
                error
            )
        })?;

        let partial_path = build_partial_image_path(destination_path)?;
        let prepared_destination = prepare_partial_image_destination(
            &partial_path,
            source_path,
            0,
            Some(total_bytes),
            Some(total_bytes),
            Some(total_bytes),
        )?;
        let destination_file =
            open_partial_image_writer(&partial_path, prepared_destination.resume_from_bytes)
                .map_err(|error| {
                    format!(
                        "Unable to create the read-only image {}: {}",
                        partial_path.to_string_lossy(),
                        error
                    )
                })?;

        let mut writer = BufWriter::new(destination_file);
        let mut buffer = vec![0_u8; profile.buffer_size_bytes()];
        let mut copied = prepared_destination.resume_from_bytes;
        let mut remaining = total_bytes.saturating_sub(prepared_destination.resume_from_bytes);
        let mut issue_summary = prepared_destination.issue_summary;
        let carried_unmapped_issue_summary = prepared_destination.carried_unmapped_issue_summary;
        let mut unreadable_ranges = prepared_destination.unresolved_ranges;
        let mut retry_refinement_summary = prepared_destination.retry_refinement_summary;
        let mut last_unreadable_end = unreadable_ranges
            .last()
            .map(|range| range.start_offset.saturating_add(range.length));

        while remaining > 0 {
            let read_limit = buffer.len().min(remaining as usize);
            let current_offset = copied;
            match read_source_chunk_with_profile(
                reader,
                &mut buffer[..read_limit],
                source_path,
                profile,
            )? {
                ChunkReadOutcome::Read(read) => {
                    if read == 0 {
                        break;
                    }
                    writer.write_all(&buffer[..read]).map_err(|error| {
                        format!(
                            "Unable to write the read-only image {}: {}",
                            partial_path.to_string_lossy(),
                            error
                        )
                    })?;
                    copied = copied.saturating_add(read as u64);
                    remaining = remaining.saturating_sub(read as u64);
                    last_unreadable_end = None;
                }
                ChunkReadOutcome::Unreadable => {
                    let unreadable_length =
                        (read_limit as u64).min(profile.unreadable_skip_span_bytes());
                    write_zero_fill(&mut writer, unreadable_length)?;
                    track_unreadable_range(
                        &mut issue_summary,
                        &mut unreadable_ranges,
                        &mut last_unreadable_end,
                        current_offset,
                        unreadable_length,
                    );
                    persist_partial_image_rescue_state(
                        &prepared_destination.checkpoint_path,
                        &prepared_destination.checkpoint_seed,
                        issue_summary,
                        &unreadable_ranges,
                        retry_refinement_summary,
                    )?;
                    reader
                        .seek(SeekFrom::Start(
                            current_offset.saturating_add(unreadable_length),
                        ))
                        .map_err(|error| {
                            format!(
                                "Unable to skip the unreadable source region during cautious imaging {}: {}",
                                source_path.to_string_lossy(),
                                error
                            )
                        })?;
                    copied = copied.saturating_add(unreadable_length);
                    remaining = remaining.saturating_sub(unreadable_length);
                }
            }
        }

        writer.flush().map_err(|error| {
            format!(
                "Unable to flush the read-only image {}: {}",
                partial_path.to_string_lossy(),
                error
            )
        })?;
        drop(writer);

        if profile == ImagingProfile::Cautious && !unreadable_ranges.is_empty() {
            let (refined_unreadable_ranges, summary) = recover_unreadable_ranges_with_refinement(
                reader,
                &partial_path,
                source_path,
                0,
                unreadable_ranges,
                carried_unmapped_issue_summary,
                retry_refinement_summary,
                &prepared_destination.checkpoint_path,
                &prepared_destination.checkpoint_seed,
            )?;
            unreadable_ranges = refined_unreadable_ranges;
            retry_refinement_summary = summary;
        }

        let current_issue_summary = summarize_unreadable_ranges(&unreadable_ranges);
        issue_summary = add_issue_summaries(carried_unmapped_issue_summary, current_issue_summary);
        let unreadable_range_samples = collect_unreadable_range_samples(&unreadable_ranges);
        finalize_partial_image(destination_path, &partial_path)?;

        Ok(ImageArtifact {
            path: destination_path.to_path_buf(),
            bytes_copied: copied,
            resume_from_bytes: prepared_destination.resume_from_bytes,
            unreadable_ranges_count: issue_summary.unreadable_ranges_count,
            unreadable_bytes: issue_summary.unreadable_bytes,
            unreadable_ranges: unreadable_ranges.clone(),
            unreadable_range_samples,
            rescued_after_retry_bytes: retry_refinement_summary.rescued_after_retry_bytes,
            retry_passes_completed: retry_refinement_summary.retry_passes_completed,
        })
    }

    fn image_faulty_reader_to_destination(
        reader: &mut FaultyReader,
        destination_path: &Path,
        source_path: &Path,
        total_bytes: u64,
        profile: ImagingProfile,
    ) -> Result<ImageArtifact, String> {
        image_reader_to_destination(reader, destination_path, source_path, total_bytes, profile)
    }

    #[test]
    fn cautious_imaging_profile_uses_smaller_reads_and_retries() {
        assert_eq!(ImagingProfile::Standard.buffer_size_bytes(), 1024 * 1024);
        assert_eq!(ImagingProfile::Cautious.buffer_size_bytes(), 256 * 1024);
        assert_eq!(ImagingProfile::Standard.read_attempts(), 1);
        assert_eq!(ImagingProfile::Cautious.read_attempts(), 3);
        assert_eq!(
            ImagingProfile::Standard.retry_delay(),
            Duration::from_millis(0)
        );
        assert_eq!(
            ImagingProfile::Cautious.retry_delay(),
            Duration::from_millis(80)
        );
        assert_eq!(ImagingProfile::Cautious.as_str(), "cautious");
    }

    #[test]
    fn cautious_imaging_zero_fills_unreadable_ranges_and_continues() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-unreadable-cautious-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test workspace should exist");
        let destination = root.join("capture.img");
        let source_path = root.join("synthetic-source.bin");
        let page = 4096_usize;
        let mut payload = vec![b'A'; page];
        payload.extend(vec![b'B'; page]);
        payload.extend(vec![b'C'; page]);
        let mut reader = FaultyReader::new(payload, vec![(page as u64, page as u64)]);

        let artifact = image_faulty_reader_to_destination(
            &mut reader,
            &destination,
            &source_path,
            (page * 3) as u64,
            ImagingProfile::Cautious,
        )
        .expect("cautious imaging should continue across unreadable ranges");

        let output = fs::read(&artifact.path).expect("image output should exist");
        assert_eq!(output.len(), page * 3);
        assert!(output[..page].iter().all(|byte| *byte == b'A'));
        assert!(output[page..page * 2].iter().all(|byte| *byte == 0));
        assert!(output[page * 2..].iter().all(|byte| *byte == b'C'));
        assert_eq!(artifact.unreadable_ranges_count, 1);
        assert_eq!(artifact.unreadable_bytes, page as u64);
        assert_eq!(artifact.unreadable_range_samples.len(), 1);
        assert_eq!(
            artifact.unreadable_range_samples[0].start_offset,
            page as u64
        );
        assert_eq!(artifact.unreadable_range_samples[0].length, page as u64);
        assert!(artifact.retry_passes_completed > 0);
        assert_eq!(artifact.rescued_after_retry_bytes, 0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cautious_imaging_refines_large_zero_fill_ranges_with_smaller_retry_passes() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-refinement-cautious-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test workspace should exist");
        let destination = root.join("capture.img");
        let source_path = root.join("synthetic-source.bin");
        let page = 4096_usize;
        let initial_bad_slice = 512_u64;
        let mut payload = vec![b'A'; page];
        payload.extend(vec![b'B'; page]);
        payload.extend(vec![b'C'; page]);
        let mut reader = FaultyReader::new(payload, vec![(page as u64, initial_bad_slice)]);

        let artifact = image_faulty_reader_to_destination(
            &mut reader,
            &destination,
            &source_path,
            (page * 3) as u64,
            ImagingProfile::Cautious,
        )
        .expect("cautious imaging should refine partially readable gaps");

        let output = fs::read(&artifact.path).expect("image output should exist");
        assert_eq!(output.len(), page * 3);
        assert!(output[..page].iter().all(|byte| *byte == b'A'));
        assert!(output[page..page + initial_bad_slice as usize]
            .iter()
            .all(|byte| *byte == 0));
        assert!(output[page + initial_bad_slice as usize..page * 2]
            .iter()
            .all(|byte| *byte == b'B'));
        assert!(output[page * 2..].iter().all(|byte| *byte == b'C'));
        assert_eq!(artifact.unreadable_ranges_count, 1);
        assert_eq!(artifact.unreadable_bytes, initial_bad_slice);
        assert_eq!(artifact.unreadable_range_samples.len(), 1);
        assert_eq!(
            artifact.unreadable_range_samples[0].start_offset,
            page as u64
        );
        assert_eq!(
            artifact.unreadable_range_samples[0].length,
            initial_bad_slice
        );
        assert!(artifact.retry_passes_completed > 0);
        assert_eq!(
            artifact.rescued_after_retry_bytes,
            page as u64 - initial_bad_slice
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cautious_imaging_reverse_retry_pass_rescues_a_deferred_trailing_chunk() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-reverse-refinement-cautious-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test workspace should exist");
        let destination = root.join("capture.img");
        let source_path = root.join("synthetic-source.bin");
        let page = 4096_usize;
        let deferred_chunk_start = page as u64 + 3072;
        let deferred_chunk_length = 1024_usize;
        let mut payload = vec![b'A'; page];
        payload.extend(vec![b'B'; page]);
        payload.extend(vec![b'C'; page]);
        let mut reader = RetrySensitiveReader::new(
            payload,
            vec![(page as u64, 3072)],
            deferred_chunk_start,
            deferred_chunk_length,
        );

        let artifact = image_reader_to_destination(
            &mut reader,
            &destination,
            &source_path,
            (page * 3) as u64,
            ImagingProfile::Cautious,
        )
        .expect("reverse targeted rescue pass should recover the deferred trailing chunk");

        let output = fs::read(&artifact.path).expect("image output should exist");
        assert_eq!(output.len(), page * 3);
        assert!(output[..page].iter().all(|byte| *byte == b'A'));
        assert!(output[page..page + 3072].iter().all(|byte| *byte == 0));
        assert!(output[page + 3072..page * 2]
            .iter()
            .all(|byte| *byte == b'B'));
        assert!(output[page * 2..].iter().all(|byte| *byte == b'C'));
        assert_eq!(artifact.unreadable_ranges_count, 1);
        assert_eq!(artifact.unreadable_bytes, 3072);
        assert_eq!(
            artifact.rescued_after_retry_bytes,
            deferred_chunk_length as u64
        );
        assert_eq!(artifact.retry_passes_completed, 9);
        assert_eq!(artifact.unreadable_range_samples.len(), 1);
        assert_eq!(
            artifact.unreadable_range_samples[0].start_offset,
            page as u64
        );
        assert_eq!(artifact.unreadable_range_samples[0].length, 3072);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cautious_imaging_edge_trim_pass_recovers_border_chunks_after_coarse_passes_fail() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-edge-trim-cautious-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test workspace should exist");
        let destination = root.join("capture.img");
        let source_path = root.join("synthetic-source.bin");
        let page = 4096_usize;
        let edge_chunk = 128_usize;
        let mut payload = vec![b'A'; page];
        payload.extend(vec![b'B'; page]);
        payload.extend(vec![b'C'; page]);
        let mut reader = EdgeTrimSensitiveReader::new(payload, page as u64, page, edge_chunk);

        let artifact = image_reader_to_destination(
            &mut reader,
            &destination,
            &source_path,
            (page * 3) as u64,
            ImagingProfile::Cautious,
        )
        .expect("edge trim pass should recover the leading and trailing border chunks");

        let output = fs::read(&artifact.path).expect("image output should exist");
        assert_eq!(output.len(), page * 3);
        assert!(output[..page].iter().all(|byte| *byte == b'A'));
        assert!(output[page..page + edge_chunk]
            .iter()
            .all(|byte| *byte == b'B'));
        assert!(output[page + edge_chunk..page * 2 - edge_chunk]
            .iter()
            .all(|byte| *byte == 0));
        assert!(output[page * 2 - edge_chunk..page * 2]
            .iter()
            .all(|byte| *byte == b'B'));
        assert!(output[page * 2..].iter().all(|byte| *byte == b'C'));
        assert_eq!(artifact.unreadable_ranges_count, 1);
        assert_eq!(artifact.unreadable_bytes, (page - edge_chunk * 2) as u64);
        assert_eq!(artifact.rescued_after_retry_bytes, (edge_chunk * 2) as u64);
        assert_eq!(artifact.retry_passes_completed, 9);
        assert_eq!(artifact.unreadable_range_samples.len(), 1);
        assert_eq!(
            artifact.unreadable_range_samples[0],
            UnreadableRange {
                start_offset: (page + edge_chunk) as u64,
                length: (page - edge_chunk * 2) as u64,
            }
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cautious_imaging_center_out_pass_recovers_a_readable_island_inside_the_last_gap() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-center-island-cautious-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test workspace should exist");
        let destination = root.join("capture.img");
        let source_path = root.join("synthetic-source.bin");
        let page = 4096_usize;
        let island_length = 64_usize;
        let island_start = page as u64 + (page / 2) as u64;
        let mut payload = vec![b'A'; page];
        payload.extend(vec![b'B'; page]);
        payload.extend(vec![b'C'; page]);
        let mut reader = CenterIslandSensitiveReader::new(
            payload,
            page as u64,
            page,
            island_start,
            island_length,
        );

        let artifact = image_reader_to_destination(
            &mut reader,
            &destination,
            &source_path,
            (page * 3) as u64,
            ImagingProfile::Cautious,
        )
        .expect("center-out scraping should recover the readable island inside the last gap");

        let output = fs::read(&artifact.path).expect("image output should exist");
        assert_eq!(output.len(), page * 3);
        assert!(output[..page].iter().all(|byte| *byte == b'A'));
        assert!(output[page..island_start as usize]
            .iter()
            .all(|byte| *byte == 0));
        assert!(
            output[island_start as usize..island_start as usize + island_length]
                .iter()
                .all(|byte| *byte == b'B')
        );
        assert!(output[island_start as usize + island_length..page * 2]
            .iter()
            .all(|byte| *byte == 0));
        assert!(output[page * 2..].iter().all(|byte| *byte == b'C'));
        assert_eq!(artifact.unreadable_ranges_count, 2);
        assert_eq!(artifact.unreadable_bytes, (page - island_length) as u64);
        assert_eq!(artifact.rescued_after_retry_bytes, island_length as u64);
        assert_eq!(artifact.retry_passes_completed, 9);
        assert_eq!(
            artifact.unreadable_range_samples,
            vec![
                UnreadableRange {
                    start_offset: page as u64,
                    length: (page / 2) as u64,
                },
                UnreadableRange {
                    start_offset: island_start + island_length as u64,
                    length: (page / 2 - island_length) as u64,
                },
            ]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cautious_imaging_micro_scrape_pass_recovers_a_chunk_readable_only_in_32_bytes() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-micro-scrape-cautious-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test workspace should exist");
        let destination = root.join("capture.img");
        let source_path = root.join("synthetic-source.bin");
        let page = 4096_usize;
        let micro_chunk_length = 32_usize;
        let micro_chunk_start = page as u64 + 1024;
        let mut payload = vec![b'A'; page];
        payload.extend(vec![b'B'; page]);
        payload.extend(vec![b'C'; page]);
        let mut reader = MicroScrapeSensitiveReader::new(
            payload,
            page as u64,
            page,
            micro_chunk_start,
            micro_chunk_length,
        );

        let artifact = image_reader_to_destination(
            &mut reader,
            &destination,
            &source_path,
            (page * 3) as u64,
            ImagingProfile::Cautious,
        )
        .expect("micro-scrape pass should recover a chunk readable only at 32-byte granularity");

        let output = fs::read(&artifact.path).expect("image output should exist");
        assert_eq!(output.len(), page * 3);
        assert!(output[..page].iter().all(|byte| *byte == b'A'));
        assert!(output[page..micro_chunk_start as usize]
            .iter()
            .all(|byte| *byte == 0));
        assert!(output
            [micro_chunk_start as usize..micro_chunk_start as usize + micro_chunk_length]
            .iter()
            .all(|byte| *byte == b'B'));
        assert!(
            output[micro_chunk_start as usize + micro_chunk_length..page * 2]
                .iter()
                .all(|byte| *byte == 0)
        );
        assert!(output[page * 2..].iter().all(|byte| *byte == b'C'));
        assert_eq!(artifact.unreadable_ranges_count, 2);
        assert_eq!(
            artifact.unreadable_bytes,
            (page - micro_chunk_length) as u64
        );
        assert_eq!(
            artifact.rescued_after_retry_bytes,
            micro_chunk_length as u64
        );
        assert_eq!(artifact.retry_passes_completed, 9);
        assert_eq!(
            artifact.unreadable_range_samples,
            vec![
                UnreadableRange {
                    start_offset: page as u64,
                    length: 1024,
                },
                UnreadableRange {
                    start_offset: micro_chunk_start + micro_chunk_length as u64,
                    length: (page - 1024 - micro_chunk_length) as u64,
                },
            ]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cautious_imaging_neighbor_zoom_recovers_adjacent_chunks_before_the_window_expires() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-neighbor-zoom-cautious-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test workspace should exist");
        let destination = root.join("capture.img");
        let source_path = root.join("synthetic-source.bin");
        let page = 4096_usize;
        let center_chunk_length = 64_usize;
        let neighbor_chunk_length = 32_usize;
        let center_chunk_start = page as u64 + (page / 2) as u64;
        let left_neighbor_start = center_chunk_start.saturating_sub(neighbor_chunk_length as u64);
        let recovered_end = center_chunk_start
            .saturating_add(center_chunk_length as u64)
            .saturating_add(neighbor_chunk_length as u64);
        let mut payload = vec![b'A'; page];
        payload.extend(vec![b'B'; page]);
        payload.extend(vec![b'C'; page]);
        let mut reader = NeighborZoomSensitiveReader::new(
            payload,
            page as u64,
            page,
            center_chunk_start,
            center_chunk_length,
            neighbor_chunk_length,
        );

        let artifact = image_reader_to_destination(
            &mut reader,
            &destination,
            &source_path,
            (page * 3) as u64,
            ImagingProfile::Cautious,
        )
        .expect(
            "neighbor zoom should recover adjacent chunks before the local rescue window closes",
        );

        let output = fs::read(&artifact.path).expect("image output should exist");
        assert_eq!(output.len(), page * 3);
        assert!(output[..page].iter().all(|byte| *byte == b'A'));
        assert!(output[page..left_neighbor_start as usize]
            .iter()
            .all(|byte| *byte == 0));
        assert!(output[left_neighbor_start as usize..recovered_end as usize]
            .iter()
            .all(|byte| *byte == b'B'));
        assert!(output[recovered_end as usize..page * 2]
            .iter()
            .all(|byte| *byte == 0));
        assert!(output[page * 2..].iter().all(|byte| *byte == b'C'));
        assert_eq!(artifact.unreadable_ranges_count, 2);
        assert_eq!(
            artifact.unreadable_bytes,
            (page - (center_chunk_length + neighbor_chunk_length * 2)) as u64
        );
        assert_eq!(
            artifact.rescued_after_retry_bytes,
            (center_chunk_length + neighbor_chunk_length * 2) as u64
        );
        assert_eq!(artifact.retry_passes_completed, 9);
        assert_eq!(
            artifact.unreadable_range_samples,
            vec![
                UnreadableRange {
                    start_offset: page as u64,
                    length: left_neighbor_start.saturating_sub(page as u64),
                },
                UnreadableRange {
                    start_offset: recovered_end,
                    length: (page as u64 * 2).saturating_sub(recovered_end),
                },
            ]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cautious_imaging_adaptive_split_recovers_the_tail_of_a_partial_32b_progress() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-adaptive-split-cautious-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test workspace should exist");
        let destination = root.join("capture.img");
        let source_path = root.join("synthetic-source.bin");
        let page = 4096_usize;
        let pivot_chunk_length = 32_usize;
        let partial_read_length = 16_usize;
        let pivot_chunk_start = page as u64 + 2048;
        let recovered_end = pivot_chunk_start.saturating_add(pivot_chunk_length as u64);
        let mut payload = vec![b'A'; page];
        payload.extend(vec![b'B'; page]);
        payload.extend(vec![b'C'; page]);
        let mut reader = PartialProgressSplitSensitiveReader::new(
            payload,
            page as u64,
            page,
            pivot_chunk_start,
            pivot_chunk_length,
            partial_read_length,
        );

        let artifact = image_reader_to_destination(
            &mut reader,
            &destination,
            &source_path,
            (page * 3) as u64,
            ImagingProfile::Cautious,
        )
        .expect("adaptive split should recover the remaining tail immediately after a partial 32-byte progress");

        let output = fs::read(&artifact.path).expect("image output should exist");
        assert_eq!(output.len(), page * 3);
        assert!(output[..page].iter().all(|byte| *byte == b'A'));
        assert!(output[page..pivot_chunk_start as usize]
            .iter()
            .all(|byte| *byte == 0));
        assert!(output[pivot_chunk_start as usize..recovered_end as usize]
            .iter()
            .all(|byte| *byte == b'B'));
        assert!(output[recovered_end as usize..page * 2]
            .iter()
            .all(|byte| *byte == 0));
        assert!(output[page * 2..].iter().all(|byte| *byte == b'C'));
        assert_eq!(artifact.unreadable_ranges_count, 2);
        assert_eq!(
            artifact.unreadable_bytes,
            (page - pivot_chunk_length) as u64
        );
        assert_eq!(
            artifact.rescued_after_retry_bytes,
            pivot_chunk_length as u64
        );
        assert_eq!(artifact.retry_passes_completed, 9);
        assert_eq!(
            artifact.unreadable_range_samples,
            vec![
                UnreadableRange {
                    start_offset: page as u64,
                    length: pivot_chunk_start.saturating_sub(page as u64),
                },
                UnreadableRange {
                    start_offset: recovered_end,
                    length: (page as u64 * 2).saturating_sub(recovered_end),
                },
            ]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parse_external_rescue_map_accepts_retry_statuses_and_seeds_prefix_resume() {
        let mapfile = "\
# Mapfile. Generated by GNU ddrescue 1.28\n\
# current_pos  current_status  current_pass\n\
0x0000000000004000     ?               5\n\
#      pos              size  status\n\
0x0000000000000000  0x0000000000001000  +\n\
0x0000000000001000  0x0000000000001000  *\n\
0x0000000000002000  0x0000000000001000  /\n\
0x0000000000003000  0x0000000000001000  -\n\
0x0000000000004000  0x0000000000001000  ?\n";

        let parsed = parse_external_rescue_map(mapfile).expect("mapfile should parse");
        assert_eq!(parsed.domain_end, 5 * 4096);

        let seed = seed_external_rescue_map(&parsed, 4 * 4096)
            .expect("retry statuses before the first gap should be importable");

        assert_eq!(seed.resume_from_bytes, 4 * 4096);
        assert_eq!(seed.unresolved_ranges.len(), 1);
        assert_eq!(
            seed.unresolved_ranges[0],
            UnreadableRange {
                start_offset: 4096,
                length: 3 * 4096,
            }
        );
    }

    #[test]
    fn import_external_rescue_map_for_image_destination_seeds_checkpoint_and_resumes_targeted_repair(
    ) {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-external-map-import-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("external map import workspace should exist");
        let source = root.join("source.bin");
        let destination = root.join("exports").join("capture.img");
        let partial_path =
            build_partial_image_path(&destination).expect("partial path should be available");
        let checkpoint_path = partial_image_checkpoint_path(&partial_path)
            .expect("checkpoint path should be available");
        let mapfile_path = root.join("capture.map");
        fs::create_dir_all(
            partial_path
                .parent()
                .expect("partial image parent should exist"),
        )
        .expect("partial image directory should exist");

        let page = 4096_usize;
        let remaining_gap = 1024_usize;
        let mut source_bytes = vec![b'A'; page];
        source_bytes.extend(vec![b'B'; page]);
        source_bytes.extend(vec![b'C'; page]);
        fs::write(&source, &source_bytes).expect("source fixture should be written");

        let mut partial_bytes = source_bytes.clone();
        partial_bytes[page..page + remaining_gap].fill(0);
        fs::write(&partial_path, &partial_bytes).expect("partial image should be written");

        let mapfile = "\
# Mapfile. Generated by GNU ddrescue 1.28\n\
# current_pos  current_status  current_pass\n\
0x0000000000003000     +               2\n\
#      pos              size  status\n\
0x0000000000000000  0x0000000000001000  +\n\
0x0000000000001000  0x0000000000000400  -\n\
0x0000000000001400  0x0000000000000C00  +\n\
0x0000000000002000  0x0000000000001000  +\n";
        fs::write(&mapfile_path, mapfile).expect("mapfile fixture should be written");

        let import_summary = import_external_rescue_map_for_image_destination(
            &destination,
            &source,
            source_bytes.len() as u64,
            &mapfile_path,
        )
        .expect("external rescue map should seed the local checkpoint");

        assert_eq!(import_summary.resume_from_bytes, source_bytes.len() as u64);
        assert_eq!(import_summary.mapped_bytes, source_bytes.len() as u64);
        assert_eq!(import_summary.unreadable_ranges_count, 1);
        assert_eq!(import_summary.unreadable_bytes, remaining_gap as u64);

        let imported_checkpoint = load_partial_image_checkpoint(&checkpoint_path)
            .expect("imported checkpoint should be readable");
        assert_eq!(imported_checkpoint.retry_passes_completed, 0);
        assert_eq!(imported_checkpoint.rescued_after_retry_bytes, 0);
        assert_eq!(
            imported_checkpoint.unreadable_ranges,
            vec![UnreadableRange {
                start_offset: page as u64,
                length: remaining_gap as u64,
            }]
        );

        let mut progress_updates = Vec::new();
        let artifact = create_read_only_image_at_controlled_with_profile(
            &destination,
            &source,
            ImagingProfile::Cautious,
            &mut |copied| {
                progress_updates.push(copied);
                Ok(())
            },
        )
        .expect("cautious imaging should reuse the imported rescue map and repair the gap");

        assert_eq!(artifact.resume_from_bytes, source_bytes.len() as u64);
        assert_eq!(artifact.bytes_copied, source_bytes.len() as u64);
        assert_eq!(
            progress_updates.first().copied(),
            Some(source_bytes.len() as u64)
        );
        assert_eq!(artifact.unreadable_ranges_count, 0);
        assert_eq!(artifact.unreadable_bytes, 0);
        assert_eq!(artifact.retry_passes_completed, 1);
        assert_eq!(artifact.rescued_after_retry_bytes, remaining_gap as u64);
        assert_eq!(
            fs::read(&destination).expect("repaired destination image should exist"),
            source_bytes
        );
        assert!(!partial_path.exists());
        assert!(!checkpoint_path.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn import_external_rescue_map_for_image_destination_rejects_out_of_order_layouts() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-external-map-reject-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("external map reject workspace should exist");
        let source = root.join("source.bin");
        let destination = root.join("exports").join("capture.img");
        let partial_path =
            build_partial_image_path(&destination).expect("partial path should be available");
        let mapfile_path = root.join("capture.map");
        fs::create_dir_all(
            partial_path
                .parent()
                .expect("partial image parent should exist"),
        )
        .expect("partial image directory should exist");

        let page = 4096_usize;
        let payload = vec![b'R'; page * 3];
        fs::write(&source, &payload).expect("source fixture should be written");
        fs::write(&partial_path, &payload[..page]).expect("partial image should be written");
        let mapfile = "\
# Mapfile. Generated by GNU ddrescue 1.28\n\
0x0000000000000000     ?               1\n\
0x0000000000000000  0x0000000000001000  +\n\
0x0000000000001000  0x0000000000001000  ?\n\
0x0000000000002000  0x0000000000001000  +\n";
        fs::write(&mapfile_path, mapfile).expect("mapfile fixture should be written");

        let error = import_external_rescue_map_for_image_destination(
            &destination,
            &source,
            payload.len() as u64,
            &mapfile_path,
        )
        .expect_err("out-of-order copied blocks should be rejected");

        assert!(error.contains("full logical length"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn import_external_rescue_map_for_image_destination_accepts_sparse_logical_length_layouts() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-external-map-sparse-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("external map sparse workspace should exist");
        let source = root.join("source.bin");
        let destination = root.join("exports").join("capture.img");
        let partial_path =
            build_partial_image_path(&destination).expect("partial path should be available");
        let checkpoint_path = partial_image_checkpoint_path(&partial_path)
            .expect("checkpoint path should be available");
        let mapfile_path = root.join("capture.map");
        fs::create_dir_all(
            partial_path
                .parent()
                .expect("partial image parent should exist"),
        )
        .expect("partial image directory should exist");

        let page = 4096_usize;
        let mut source_bytes = vec![b'A'; page];
        source_bytes.extend(vec![b'B'; page]);
        source_bytes.extend(vec![b'C'; page]);
        source_bytes.extend(vec![b'D'; page]);
        source_bytes.extend(vec![b'E'; page]);
        fs::write(&source, &source_bytes).expect("source fixture should be written");

        let mut partial_bytes = vec![0_u8; source_bytes.len()];
        partial_bytes[..page].copy_from_slice(&source_bytes[..page]);
        partial_bytes[page * 2..page * 3].copy_from_slice(&source_bytes[page * 2..page * 3]);
        fs::write(&partial_path, &partial_bytes)
            .expect("sparse logical-length partial should exist");

        let mapfile = "\
# Mapfile. Generated by GNU ddrescue 1.28\n\
# current_pos  current_status  current_pass\n\
0x0000000000005000     ?               6\n\
#      pos              size  status\n\
0x0000000000000000  0x0000000000001000  +\n\
0x0000000000001000  0x0000000000001000  ?\n\
0x0000000000002000  0x0000000000001000  +\n\
0x0000000000003000  0x0000000000001000  -\n\
0x0000000000004000  0x0000000000001000  ?\n";
        fs::write(&mapfile_path, mapfile).expect("mapfile fixture should be written");

        let import_summary = import_external_rescue_map_for_image_destination(
            &destination,
            &source,
            source_bytes.len() as u64,
            &mapfile_path,
        )
        .expect("logical-length sparse rescue map should seed a reusable checkpoint");

        assert_eq!(import_summary.resume_from_bytes, source_bytes.len() as u64);
        assert_eq!(import_summary.mapped_bytes, source_bytes.len() as u64);
        assert_eq!(import_summary.unreadable_ranges_count, 2);
        assert_eq!(import_summary.unreadable_bytes, (page * 3) as u64);

        let imported_checkpoint = load_partial_image_checkpoint(&checkpoint_path)
            .expect("imported checkpoint should be readable");
        assert_eq!(
            imported_checkpoint.unreadable_ranges,
            vec![
                UnreadableRange {
                    start_offset: page as u64,
                    length: page as u64,
                },
                UnreadableRange {
                    start_offset: (page * 3) as u64,
                    length: (page * 2) as u64,
                },
            ]
        );

        let mut progress_updates = Vec::new();
        let artifact = create_read_only_image_at_controlled_with_profile(
            &destination,
            &source,
            ImagingProfile::Cautious,
            &mut |copied| {
                progress_updates.push(copied);
                Ok(())
            },
        )
        .expect("cautious imaging should repair sparse logical-length gaps from the imported map");

        assert_eq!(artifact.resume_from_bytes, source_bytes.len() as u64);
        assert_eq!(artifact.bytes_copied, source_bytes.len() as u64);
        assert_eq!(
            progress_updates.first().copied(),
            Some(source_bytes.len() as u64)
        );
        assert_eq!(artifact.unreadable_ranges_count, 0);
        assert_eq!(artifact.unreadable_bytes, 0);
        assert_eq!(artifact.retry_passes_completed, 1);
        assert_eq!(artifact.rescued_after_retry_bytes, (page * 3) as u64);
        assert_eq!(
            fs::read(&destination).expect("fully repaired destination should exist"),
            source_bytes
        );
        assert!(!partial_path.exists());
        assert!(!checkpoint_path.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn standard_imaging_fails_on_unreadable_ranges() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-unreadable-standard-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test workspace should exist");
        let destination = root.join("capture.img");
        let source_path = root.join("synthetic-source.bin");
        let page = 4096_usize;
        let mut payload = vec![b'A'; page];
        payload.extend(vec![b'B'; page]);
        payload.extend(vec![b'C'; page]);
        let mut reader = FaultyReader::new(payload, vec![(page as u64, page as u64)]);

        let error = image_faulty_reader_to_destination(
            &mut reader,
            &destination,
            &source_path,
            (page * 3) as u64,
            ImagingProfile::Standard,
        )
        .expect_err("standard imaging should stop on unreadable ranges");

        assert!(error.contains("Unable to read from the source during standard imaging"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn open_source_read_only_refuses_write_intent_at_caller_level() {
        // Contract: the helper hands back a `File` whose OS handle is RO.
        // Writing through it must fail with a permission/invalid-argument
        // error before we even hit the kernel I/O path. We prove the intent
        // by trying to write and asserting the error bubbles up.
        let root = env::temp_dir().join(format!("recupere-ro-helper-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("workspace should be creatable");
        let source = root.join("ro-source.bin");
        fs::write(&source, b"read-only payload").expect("fixture should write");

        let mut handle = open_source_read_only(&source).expect("helper should open the file");
        let write_result = handle.write(b"tampered");
        assert!(
            write_result.is_err(),
            "a handle returned by open_source_read_only must not accept writes; \
             got Ok({:?}) which would mean we hold a writable handle on the source",
            write_result.as_ref().ok()
        );

        // Sanity: the source is still open for read and returns the original
        // bytes, i.e. we didn't accidentally truncate or touch it.
        drop(handle);
        let after = fs::read(&source).expect("source should still be readable");
        assert_eq!(after, b"read-only payload");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn open_source_read_only_returns_not_found_for_missing_path() {
        // If the source doesn't exist the caller must see a real IO error,
        // not a silent success. This regression-guards against a future
        // refactor that adds `.create(true)` by accident.
        let missing = env::temp_dir().join(format!(
            "recupere-ro-helper-missing-{}.bin",
            std::process::id()
        ));
        let _ = fs::remove_file(&missing);
        let err = open_source_read_only(&missing).expect_err("missing source must error out");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn create_read_only_image_copies_a_regular_file() {
        let root = env::temp_dir().join(format!("recupere-imaging-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test workspace should exist");
        let source = root.join("source.bin");
        fs::write(&source, b"hello imaging").expect("source image fixture should be written");

        let mut progress_updates = Vec::new();
        let artifact = create_read_only_image("scan:test", &source, &mut |copied| {
            progress_updates.push(copied);
        })
        .expect("regular file should be imaged");

        assert_eq!(
            fs::read(&artifact.path).expect("image output should exist"),
            b"hello imaging"
        );
        assert_eq!(artifact.bytes_copied, 13);
        assert!(!progress_updates.is_empty());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(&artifact.path);
        if let Some(parent) = artifact.path.parent() {
            let _ = fs::remove_dir(parent);
        }
    }

    #[test]
    fn create_read_only_image_at_writes_to_the_requested_destination() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-explicit-destination-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test workspace should exist");
        let source = root.join("source.bin");
        let destination = root.join("exports").join("capture.dd");
        fs::write(&source, b"explicit destination")
            .expect("source image fixture should be written");

        let mut progress_updates = Vec::new();
        let artifact = create_read_only_image_at(&destination, &source, &mut |copied| {
            progress_updates.push(copied);
        })
        .expect("regular file should be imaged to explicit destination");

        assert_eq!(artifact.path, destination);
        assert_eq!(
            fs::read(&artifact.path).expect("image output should exist"),
            b"explicit destination"
        );
        assert_eq!(artifact.bytes_copied, 20);
        assert!(!progress_updates.is_empty());
        assert!(!root.join("exports").join("capture.dd.partial").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn create_read_only_image_at_normalizes_fixed_vhd_sources() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-vhd-destination-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test workspace should exist");
        let source = root.join("source.vhd");
        let destination = root.join("exports").join("capture.img");
        let payload = b"VHD-PAYLOAD";
        let mut bytes = Vec::with_capacity(payload.len() + 512);
        bytes.extend_from_slice(payload);
        let mut footer = [0_u8; 512];
        footer[0..8].copy_from_slice(b"conectix");
        footer[16..24].copy_from_slice(&u64::MAX.to_be_bytes());
        footer[48..56].copy_from_slice(&(payload.len() as u64).to_be_bytes());
        footer[60..64].copy_from_slice(&2_u32.to_be_bytes());
        bytes.extend_from_slice(&footer);
        fs::write(&source, bytes).expect("synthetic VHD fixture should be written");

        let mut progress_updates = Vec::new();
        let artifact = create_read_only_image_at(&destination, &source, &mut |copied| {
            progress_updates.push(copied);
        })
        .expect("fixed VHD source should be normalized");

        assert_eq!(artifact.path, destination);
        assert_eq!(
            fs::read(&artifact.path).expect("normalized image output should exist"),
            payload
        );
        assert_eq!(artifact.bytes_copied, payload.len() as u64);
        assert!(!progress_updates.is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn create_read_only_image_slice_at_writes_only_the_requested_range() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-slice-destination-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test workspace should exist");
        let source = root.join("source.bin");
        let destination = root.join("exports").join("slice.dd");
        fs::write(&source, b"0123456789abcdef").expect("source image fixture should be written");

        let mut progress_updates = Vec::new();
        let artifact = create_read_only_image_slice_at_controlled(
            &destination,
            &source,
            4,
            Some(6),
            &mut |copied| {
                progress_updates.push(copied);
                Ok(())
            },
        )
        .expect("slice imaging should succeed");

        assert_eq!(artifact.path, destination);
        assert_eq!(
            fs::read(&artifact.path).expect("slice image output should exist"),
            b"456789"
        );
        assert_eq!(artifact.bytes_copied, 6);
        assert!(!progress_updates.is_empty());
        assert!(!root.join("exports").join("slice.dd.partial").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn create_read_only_image_at_resumes_a_coherent_partial_image() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-resume-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("resume test workspace should exist");
        let source = root.join("source.bin");
        let destination = root.join("exports").join("capture.img");
        let source_bytes = vec![0x5A_u8; 2_500_000];
        fs::write(&source, &source_bytes).expect("resume test source should be written");

        let interruption_error =
            create_read_only_image_at_controlled(&destination, &source, &mut |copied| {
                if copied >= 1_048_576 {
                    return Err("simulated interruption".into());
                }
                Ok(())
            })
            .expect_err("the first imaging pass should be interrupted");
        assert!(interruption_error.contains("simulated interruption"));

        let partial_path =
            build_partial_image_path(&destination).expect("partial path should be available");
        let checkpoint_path = partial_image_checkpoint_path(&partial_path)
            .expect("checkpoint path should be available");
        let partial_size = fs::metadata(&partial_path)
            .expect("partial image should remain on interruption")
            .len();
        assert_eq!(partial_size, 1_048_576);
        assert!(checkpoint_path.exists());

        let mut progress_updates = Vec::new();
        let artifact = create_read_only_image_at_controlled(&destination, &source, &mut |copied| {
            progress_updates.push(copied);
            Ok(())
        })
        .expect("the second imaging pass should resume successfully");

        assert_eq!(artifact.resume_from_bytes, partial_size);
        assert_eq!(artifact.bytes_copied, source_bytes.len() as u64);
        assert_eq!(progress_updates.first().copied(), Some(partial_size));
        assert_eq!(
            fs::read(&destination).expect("resumed image should exist"),
            source_bytes
        );
        assert!(!partial_path.exists());
        assert!(!checkpoint_path.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn create_read_only_image_at_resumes_targeted_rescue_passes_from_checkpoint() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-rescue-map-resume-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("rescue map resume workspace should exist");
        let source = root.join("source.bin");
        let destination = root.join("exports").join("capture.img");
        let partial_path =
            build_partial_image_path(&destination).expect("partial path should be available");
        let checkpoint_path = partial_image_checkpoint_path(&partial_path)
            .expect("checkpoint path should be available");
        fs::create_dir_all(
            partial_path
                .parent()
                .expect("partial image parent should exist"),
        )
        .expect("partial image directory should be created");

        let page = 4096_usize;
        let remaining_gap = 1024_usize;
        let mut source_bytes = vec![b'A'; page];
        source_bytes.extend(vec![b'B'; page]);
        source_bytes.extend(vec![b'C'; page]);
        fs::write(&source, &source_bytes).expect("rescue map source should be written");

        let mut partial_bytes = source_bytes.clone();
        partial_bytes[page..page + remaining_gap].fill(0);
        fs::write(&partial_path, &partial_bytes).expect("partial image should be written");
        write_partial_image_checkpoint(
            &checkpoint_path,
            &PartialImageCheckpoint {
                source_path: source.to_string_lossy().to_string(),
                start_offset_bytes: 0,
                requested_length_bytes: None,
                source_length_bytes: Some(source_bytes.len() as u64),
                unreadable_ranges_count: 1,
                unreadable_bytes: remaining_gap as u64,
                unreadable_ranges: vec![UnreadableRange {
                    start_offset: page as u64,
                    length: remaining_gap as u64,
                }],
                rescued_after_retry_bytes: (page - remaining_gap) as u64,
                retry_passes_completed: 1,
            },
        )
        .expect("rescue map checkpoint should be written");

        let mut progress_updates = Vec::new();
        let artifact = create_read_only_image_at_controlled_with_profile(
            &destination,
            &source,
            ImagingProfile::Cautious,
            &mut |copied| {
                progress_updates.push(copied);
                Ok(())
            },
        )
        .expect("resumed cautious imaging should continue the remaining rescue pass");

        assert_eq!(artifact.resume_from_bytes, source_bytes.len() as u64);
        assert_eq!(artifact.bytes_copied, source_bytes.len() as u64);
        assert_eq!(
            progress_updates.first().copied(),
            Some(source_bytes.len() as u64)
        );
        assert_eq!(artifact.unreadable_ranges_count, 0);
        assert_eq!(artifact.unreadable_bytes, 0);
        assert!(artifact.unreadable_range_samples.is_empty());
        assert_eq!(artifact.retry_passes_completed, 2);
        assert_eq!(artifact.rescued_after_retry_bytes, page as u64);
        assert_eq!(
            fs::read(&destination).expect("resumed image should exist"),
            source_bytes
        );
        assert!(!partial_path.exists());
        assert!(!checkpoint_path.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn create_read_only_image_at_discards_a_stale_partial_checkpoint() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-stale-checkpoint-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("stale checkpoint workspace should exist");
        let source = root.join("source.bin");
        let destination = root.join("exports").join("capture.img");
        let source_bytes = b"fresh read only payload".to_vec();
        fs::write(&source, &source_bytes).expect("stale checkpoint source should be written");

        let partial_path =
            build_partial_image_path(&destination).expect("partial path should be available");
        let checkpoint_path = partial_image_checkpoint_path(&partial_path)
            .expect("checkpoint path should be available");
        fs::create_dir_all(
            partial_path
                .parent()
                .expect("partial image parent should exist"),
        )
        .expect("partial image directory should be created");
        fs::write(&partial_path, b"stale-bytes").expect("stale partial image should be written");
        write_partial_image_checkpoint(
            &checkpoint_path,
            &PartialImageCheckpoint {
                source_path: source.to_string_lossy().to_string(),
                start_offset_bytes: 4,
                requested_length_bytes: None,
                source_length_bytes: Some(source_bytes.len() as u64),
                unreadable_ranges_count: 0,
                unreadable_bytes: 0,
                unreadable_ranges: Vec::new(),
                rescued_after_retry_bytes: 0,
                retry_passes_completed: 0,
            },
        )
        .expect("stale checkpoint should be written");

        let artifact =
            create_read_only_image_at_controlled(&destination, &source, &mut |_copied| Ok(()))
                .expect("stale checkpoint should be discarded and imaging restarted");

        assert_eq!(artifact.resume_from_bytes, 0);
        assert_eq!(artifact.bytes_copied, source_bytes.len() as u64);
        assert_eq!(
            fs::read(&destination).expect("fresh image should exist"),
            source_bytes
        );
        assert!(!partial_path.exists());
        assert!(!checkpoint_path.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn read_byte_runs_range_reads_across_multiple_runs_from_an_offset() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-range-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("range test workspace should exist");
        let image_path = root.join("source.img");
        fs::write(&image_path, b"0123456789abcdefghij")
            .expect("range test source image should be written");

        let bytes = read_byte_runs_range(
            &image_path,
            &[
                ByteRun {
                    offset: 2,
                    length: 4,
                    zero_fill: false,
                    ..Default::default()
                },
                ByteRun {
                    offset: 10,
                    length: 5,
                    zero_fill: false,
                    ..Default::default()
                },
            ],
            3,
            4,
        )
        .expect("range read should succeed");

        assert_eq!(bytes, b"5abc");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn read_byte_runs_range_reads_across_sparse_zero_fill_runs() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-sparse-range-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("sparse range workspace should exist");
        let image_path = root.join("source.img");
        fs::write(&image_path, b"ABCDEFGHIJ").expect("sparse range source image should be written");

        let bytes = read_byte_runs_range(
            &image_path,
            &[
                ByteRun::physical(0, 3),
                ByteRun::synthetic_zero_fill(2),
                ByteRun::physical(3, 4),
            ],
            1,
            7,
        )
        .expect("sparse range read should succeed");

        assert_eq!(bytes, vec![b'B', b'C', 0, 0, b'D', b'E', b'F']);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn materialize_byte_runs_preserves_sparse_zero_fill_segments() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-sparse-materialize-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("sparse materialize workspace should exist");
        let image_path = root.join("source.img");
        let target_path = root.join("export").join("reconstructed.bin");
        fs::write(&image_path, b"ABCDEFGH")
            .expect("sparse materialize source image should be written");

        let written = materialize_byte_runs(
            &image_path,
            &[
                ByteRun::physical(0, 2),
                ByteRun::synthetic_zero_fill(3),
                ByteRun::physical(2, 2),
            ],
            7,
            &target_path,
        )
        .expect("sparse materialize should succeed");

        assert_eq!(written, 7);
        assert_eq!(
            fs::read(&target_path).expect("sparse target should exist"),
            vec![b'A', b'B', 0, 0, 0, b'C', b'D']
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn read_byte_runs_range_decompresses_lznt1_byte_runs() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-lznt1-range-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("lznt1 range workspace should exist");
        let image_path = root.join("source.img");

        let original = b"HELLO HELLO HELLO HELLO";
        let mut compressed = Vec::new();
        compress_lznt1(original, &mut compressed);
        fs::write(&image_path, &compressed).expect("lznt1 source image should be written");

        let bytes = read_byte_runs_range(
            &image_path,
            &[ByteRun {
                offset: 0,
                length: compressed.len() as u64,
                compression_kind: Some("lznt1".into()),
                ..Default::default()
            }],
            6,
            11,
        )
        .expect("lznt1 range read should succeed");

        assert_eq!(bytes, b"HELLO HELLO");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn materialize_byte_runs_decompresses_lznt1_payloads() {
        let root = env::temp_dir().join(format!(
            "recupere-imaging-lznt1-materialize-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("lznt1 materialize workspace should exist");
        let image_path = root.join("source.img");
        let target_path = root.join("export").join("reconstructed.bin");

        let original = b"NTFS COMPRESSED PAYLOAD NTFS COMPRESSED PAYLOAD";
        let mut compressed = Vec::new();
        compress_lznt1(original, &mut compressed);
        fs::write(&image_path, &compressed).expect("lznt1 source image should be written");

        let written = materialize_byte_runs(
            &image_path,
            &[ByteRun {
                offset: 0,
                length: compressed.len() as u64,
                compression_kind: Some("lznt1".into()),
                ..Default::default()
            }],
            original.len() as u64,
            &target_path,
        )
        .expect("lznt1 materialize should succeed");

        assert_eq!(written, original.len() as u64);
        assert_eq!(
            fs::read(&target_path).expect("lznt1 target should exist"),
            original
        );

        let _ = fs::remove_dir_all(root);
    }
}
