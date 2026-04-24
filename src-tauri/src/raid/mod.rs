#![allow(dead_code)]
// ============================================================================
// Recupere — RAID Reconstruction Engine
// ============================================================================
// Supports RAID 0 (stripe), RAID 1 (mirror), RAID 5 (stripe+parity),
// and RAID 6 (dual parity). Includes degraded mode for missing members,
// auto-detection of member ordering, and metadata scanning.
// Produces a Read+Seek source compatible with carving and analyzers.
// ============================================================================

use std::{
    fs,
    fs::File,
    io::{self, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

pub mod gf;

/// RAID configuration describing the array layout.
#[derive(Debug, Clone)]
pub struct RaidConfig {
    pub level: RaidLevel,
    pub member_paths: Vec<PathBuf>,
    pub stripe_size_bytes: u64,
    pub data_offset_bytes: u64,
    /// Indices of members that are missing/failed (degraded mode).
    /// Empty = all members present.
    pub missing_members: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RaidLevel {
    Raid0,
    Raid1,
    Raid5,
    Raid6,
    Jbod,
}

/// A virtual RAID device that presents a unified Read+Seek interface.
pub struct RaidSource {
    config: RaidConfig,
    /// One slot per member; `None` = missing/failed disk (degraded mode).
    members: Vec<Option<File>>,
    member_sizes: Vec<u64>,
    logical_size: u64,
    position: u64,
}

impl RaidSource {
    pub fn open(config: RaidConfig) -> Result<Self, String> {
        if config.member_paths.is_empty() {
            return Err("RAID array requires at least one member disk.".into());
        }
        if config.stripe_size_bytes == 0 {
            return Err("RAID stripe size must be greater than zero.".into());
        }

        let mut members: Vec<Option<File>> = Vec::new();
        let mut member_sizes: Vec<u64> = Vec::new();
        let mut max_size: u64 = 0;

        for (idx, path) in config.member_paths.iter().enumerate() {
            if config.missing_members.contains(&idx) {
                members.push(None);
                member_sizes.push(0);
                continue;
            }
            let file = File::open(path)
                .map_err(|e| format!("Cannot open RAID member {}: {e}", path.to_string_lossy()))?;
            let size = file
                .metadata()
                .map_err(|e| format!("Cannot read RAID member metadata: {e}"))?
                .len()
                .saturating_sub(config.data_offset_bytes);
            if size > max_size {
                max_size = size;
            }
            members.push(Some(file));
            member_sizes.push(size);
        }
        // Missing members inherit the max present size so logical_size is correct.
        for (i, s) in member_sizes.iter_mut().enumerate() {
            if config.missing_members.contains(&i) {
                *s = max_size;
            }
        }
        // Degraded-mode safety: RAID5 tolerates 1 loss, RAID6 tolerates 2.
        let missing = config.missing_members.len();
        match config.level {
            RaidLevel::Raid5 if missing > 1 => {
                return Err("RAID 5 cannot tolerate more than 1 missing member.".into())
            }
            RaidLevel::Raid6 if missing > 2 => {
                return Err("RAID 6 cannot tolerate more than 2 missing members.".into())
            }
            RaidLevel::Raid0 | RaidLevel::Jbod if missing > 0 => {
                return Err("RAID 0 / JBOD cannot tolerate any missing member.".into())
            }
            _ => {}
        }

        let logical_size = match config.level {
            RaidLevel::Raid0 => {
                // RAID 0: total = smallest_member * num_members
                let min_size = member_sizes.iter().copied().min().unwrap_or(0);
                min_size * members.len() as u64
            }
            RaidLevel::Raid1 => {
                // RAID 1: total = smallest member (mirrored)
                member_sizes.iter().copied().min().unwrap_or(0)
            }
            RaidLevel::Raid5 => {
                // RAID 5: total = smallest_member * (num_members - 1)
                let min_size = member_sizes.iter().copied().min().unwrap_or(0);
                min_size * (members.len().saturating_sub(1)) as u64
            }
            RaidLevel::Raid6 => {
                // RAID 6: total = smallest_member * (num_members - 2)
                let min_size = member_sizes.iter().copied().min().unwrap_or(0);
                min_size * (members.len().saturating_sub(2)) as u64
            }
            RaidLevel::Jbod => {
                // JBOD: total = sum of all members
                member_sizes.iter().sum()
            }
        };

        Ok(Self {
            config,
            members,
            member_sizes,
            logical_size,
            position: 0,
        })
    }

    pub fn logical_size(&self) -> u64 {
        self.logical_size
    }

    /// Translate a logical offset to (member_index, member_offset) for the given RAID level.
    fn translate_offset(&self, logical_offset: u64) -> Option<(usize, u64)> {
        let n = self.members.len();
        if n == 0 {
            return None;
        }

        match self.config.level {
            RaidLevel::Raid0 => {
                let stripe = self.config.stripe_size_bytes;
                let stripe_index = logical_offset / stripe;
                let offset_in_stripe = logical_offset % stripe;
                let member = (stripe_index % n as u64) as usize;
                let member_stripe = stripe_index / n as u64;
                let member_offset =
                    member_stripe * stripe + offset_in_stripe + self.config.data_offset_bytes;
                Some((member, member_offset))
            }
            RaidLevel::Raid1 => {
                // Mirror: read from first available member
                Some((0, logical_offset + self.config.data_offset_bytes))
            }
            RaidLevel::Raid5 => {
                if n < 3 {
                    return None;
                }
                let stripe = self.config.stripe_size_bytes;
                let data_members = (n - 1) as u64;
                let row = logical_offset / (stripe * data_members);
                let row_offset = logical_offset % (stripe * data_members);
                let data_col = (row_offset / stripe) as usize;
                let offset_in_stripe = row_offset % stripe;

                // Left-symmetric parity rotation
                let parity_col = (n - 1 - (row as usize % n)) % n;
                let mut actual_col = data_col;
                if actual_col >= parity_col {
                    actual_col += 1;
                }
                actual_col %= n;

                let member_offset = row * stripe + offset_in_stripe + self.config.data_offset_bytes;
                Some((actual_col, member_offset))
            }
            RaidLevel::Raid6 => {
                if n < 4 {
                    return None;
                }
                let stripe = self.config.stripe_size_bytes;
                let data_members = (n - 2) as u64;
                let row = logical_offset / (stripe * data_members);
                let row_offset = logical_offset % (stripe * data_members);
                let data_col = (row_offset / stripe) as usize;
                let offset_in_stripe = row_offset % stripe;

                // RAID 6: two parity columns per row (P and Q)
                let p_col = (n - 1 - (row as usize % n)) % n;
                let q_col = (n - 2 - (row as usize % n)) % n;
                let mut actual_col = data_col;
                // Skip both parity columns
                for &parity in &[p_col.min(q_col), p_col.max(q_col)] {
                    if actual_col >= parity {
                        actual_col += 1;
                    }
                }
                actual_col %= n;

                let member_offset = row * stripe + offset_in_stripe + self.config.data_offset_bytes;
                Some((actual_col, member_offset))
            }
            RaidLevel::Jbod => {
                // JBOD: linear concatenation
                let mut cumulative = 0u64;
                for (i, &size) in self.member_sizes.iter().enumerate() {
                    if logical_offset < cumulative + size {
                        let member_offset =
                            logical_offset - cumulative + self.config.data_offset_bytes;
                        return Some((i, member_offset));
                    }
                    cumulative += size;
                }
                None
            }
        }
    }
}

impl RaidSource {
    fn read_member_chunk(
        &mut self,
        member_idx: usize,
        member_offset: u64,
        len: usize,
    ) -> io::Result<Option<Vec<u8>>> {
        match self.members[member_idx].as_mut() {
            Some(file) => {
                file.seek(SeekFrom::Start(member_offset))?;
                let mut buf = vec![0u8; len];
                let read = file.read(&mut buf)?;
                buf.truncate(read);
                if read < len {
                    buf.resize(len, 0);
                }
                Ok(Some(buf))
            }
            None => Ok(None),
        }
    }

    fn raid6_row_layout(&self, row: usize) -> (usize, usize, Vec<Option<usize>>) {
        let n = self.members.len();
        let p_col = (n - 1 - (row % n)) % n;
        let q_col = (n - 2 - (row % n)) % n;
        let mut data_index_by_col = vec![None; n];
        let mut data_index = 0usize;
        for (col, slot) in data_index_by_col.iter_mut().enumerate() {
            if col == p_col || col == q_col {
                continue;
            }
            *slot = Some(data_index);
            data_index += 1;
        }
        (p_col, q_col, data_index_by_col)
    }

    /// Reconstruct `len` bytes at `member_offset` for a missing member, using
    /// XOR parity (RAID 5 / RAID 6 single-failure case).
    /// All other members must be readable at the same offset.
    fn reconstruct_block(
        &mut self,
        missing_idx: usize,
        member_offset: u64,
        len: usize,
    ) -> io::Result<Vec<u8>> {
        let n = self.members.len();
        // Collect all surviving members' data at this offset.
        let mut survivors: Vec<Vec<u8>> = Vec::with_capacity(n - 1);
        let mut second_missing: Option<usize> = None;
        for i in 0..n {
            if i == missing_idx {
                continue;
            }
            match self.members[i].as_mut() {
                Some(f) => {
                    f.seek(SeekFrom::Start(member_offset))?;
                    let mut buf = vec![0u8; len];
                    let r = f.read(&mut buf)?;
                    buf.truncate(r);
                    if r < len {
                        // Pad with zeros if past EOF; degraded reads tolerate this.
                        buf.resize(len, 0);
                    }
                    survivors.push(buf);
                }
                None => {
                    if second_missing.is_some() {
                        return Err(io::Error::other("Too many missing members to reconstruct"));
                    }
                    second_missing = Some(i);
                }
            }
        }

        // Single failure: XOR all surviving members (data + parity) → missing block.
        // This works for RAID 5 (P parity) and RAID 6 (when only 1 disk lost).
        if second_missing.is_none() {
            let mut out = vec![0u8; len];
            for s in &survivors {
                gf::xor_into(&mut out, s);
            }
            return Ok(out);
        }

        if !matches!(self.config.level, RaidLevel::Raid6) {
            return Err(io::Error::other(
                "Dual-member reconstruction is only implemented for RAID 6",
            ));
        }

        let row = ((member_offset.saturating_sub(self.config.data_offset_bytes))
            / self.config.stripe_size_bytes) as usize;
        let (p_col, q_col, data_index_by_col) = self.raid6_row_layout(row);
        let second_missing_idx = second_missing.expect("second missing member should be set");

        let mut present_blocks = Vec::new();
        for col in 0..n {
            if col == missing_idx || col == second_missing_idx {
                continue;
            }
            if let Some(block) = self.read_member_chunk(col, member_offset, len)? {
                present_blocks.push((col, block));
            }
        }

        let mut p_known = vec![0u8; len];
        let mut q_known = vec![0u8; len];
        let mut p_block: Option<Vec<u8>> = None;
        let mut q_block: Option<Vec<u8>> = None;

        for (col, block) in &present_blocks {
            if *col == p_col {
                p_block = Some(block.clone());
                continue;
            }
            if *col == q_col {
                q_block = Some(block.clone());
                continue;
            }

            gf::xor_into(&mut p_known, block);
            if let Some(data_index) = data_index_by_col[*col] {
                let coef = gf::pow(data_index);
                for (dst, byte) in q_known.iter_mut().zip(block.iter()) {
                    *dst ^= gf::mul(coef, *byte);
                }
            }
        }

        let requested_is_data = missing_idx != p_col && missing_idx != q_col;
        let second_is_data = second_missing_idx != p_col && second_missing_idx != q_col;

        if !requested_is_data {
            return Err(io::Error::other(
                "Parity-block reads are not part of the logical RAID source",
            ));
        }

        let requested_data_index = data_index_by_col[missing_idx]
            .ok_or_else(|| io::Error::other("Unable to resolve RAID 6 data index"))?;

        if second_missing_idx == p_col {
            let q =
                q_block.ok_or_else(|| io::Error::other("RAID 6 Q parity block is unavailable"))?;
            let coef = gf::pow(requested_data_index);
            let mut out = vec![0u8; len];
            for i in 0..len {
                out[i] = gf::div(q[i] ^ q_known[i], coef);
            }
            return Ok(out);
        }

        if second_missing_idx == q_col {
            let p =
                p_block.ok_or_else(|| io::Error::other("RAID 6 P parity block is unavailable"))?;
            let mut out = p;
            gf::xor_into(&mut out, &p_known);
            return Ok(out);
        }

        if !second_is_data {
            return Err(io::Error::other("Unsupported RAID 6 dual-failure layout"));
        }

        let second_data_index = data_index_by_col[second_missing_idx]
            .ok_or_else(|| io::Error::other("Unable to resolve the second RAID 6 data index"))?;
        let p = p_block.ok_or_else(|| io::Error::other("RAID 6 P parity block is unavailable"))?;
        let q = q_block.ok_or_else(|| io::Error::other("RAID 6 Q parity block is unavailable"))?;

        let coef_x = gf::pow(requested_data_index);
        let coef_y = gf::pow(second_data_index);
        let denominator = coef_x ^ coef_y;
        if denominator == 0 {
            return Err(io::Error::other(
                "Unable to solve RAID 6 dual-failure system",
            ));
        }

        let mut p_prime = p;
        gf::xor_into(&mut p_prime, &p_known);
        let mut q_prime = q;
        gf::xor_into(&mut q_prime, &q_known);

        let mut out = vec![0u8; len];
        for i in 0..len {
            let numerator = q_prime[i] ^ gf::mul(coef_y, p_prime[i]);
            out[i] = gf::div(numerator, denominator);
        }
        Ok(out)
    }
}

impl Read for RaidSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.position >= self.logical_size {
            return Ok(0);
        }

        let remaining = (self.logical_size - self.position) as usize;
        let to_read = buf.len().min(remaining);
        let mut total_read = 0usize;

        while total_read < to_read {
            let (member_idx, member_offset) =
                self.translate_offset(self.position).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "RAID offset translation failed")
                })?;

            // Calculate how many bytes we can read from this stripe before crossing to next
            let stripe = self.config.stripe_size_bytes;
            let offset_in_stripe = self.position % stripe;
            let bytes_left_in_stripe = (stripe - offset_in_stripe) as usize;
            let chunk = (to_read - total_read).min(bytes_left_in_stripe);

            if member_idx < self.members.len() {
                if let Some(file) = self.members[member_idx].as_mut() {
                    file.seek(SeekFrom::Start(member_offset))?;
                    let n = file.read(&mut buf[total_read..total_read + chunk])?;
                    if n == 0 {
                        break;
                    }
                    total_read += n;
                    self.position += n as u64;
                } else {
                    // Missing member — try parity reconstruction
                    let recovered = self.reconstruct_block(member_idx, member_offset, chunk)?;
                    let n = recovered.len();
                    buf[total_read..total_read + n].copy_from_slice(&recovered);
                    total_read += n;
                    self.position += n as u64;
                }
            } else {
                buf[total_read..total_read + chunk].fill(0);
                total_read += chunk;
                self.position += chunk as u64;
            }
        }

        Ok(total_read)
    }
}

impl Seek for RaidSource {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.position = match pos {
            SeekFrom::Start(n) => n.min(self.logical_size),
            SeekFrom::End(n) => {
                ((self.logical_size as i64 + n).max(0) as u64).min(self.logical_size)
            }
            SeekFrom::Current(n) => {
                ((self.position as i64 + n).max(0) as u64).min(self.logical_size)
            }
        };
        Ok(self.position)
    }
}

/// Detect RAID metadata on a disk image.
/// Checks for Linux MD superblock at common locations.
pub fn detect_raid_metadata(image_path: &Path) -> Option<RaidMetadata> {
    let mut file = File::open(image_path).ok()?;
    let file_size = file.metadata().ok()?.len();

    // Linux MD superblock v1.2: at offset 4096
    let mut buf = [0u8; 256];
    file.seek(SeekFrom::Start(4096)).ok()?;
    file.read_exact(&mut buf).ok()?;

    // MD magic: 0xa92b4efc at offset 0
    let magic = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    if magic == 0xa92b_4efc {
        let major_version = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let level = i32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]);
        let chunk_kb = u32::from_le_bytes([buf[40], buf[41], buf[42], buf[43]]);
        let raid_disks = u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]);

        let raid_level = match level {
            0 => Some(RaidLevel::Raid0),
            1 => Some(RaidLevel::Raid1),
            5 => Some(RaidLevel::Raid5),
            6 => Some(RaidLevel::Raid6),
            _ => None,
        };

        if let Some(raid_level) = raid_level {
            return Some(RaidMetadata {
                level: raid_level,
                member_count: raid_disks,
                stripe_size_bytes: chunk_kb as u64 * 1024,
                superblock_version: format!("{major_version}"),
                data_offset_bytes: 4096 + 256,
            });
        }
    }

    // Try v0.90: at offset file_size - 64K (legacy)
    if file_size > 65536 {
        let offset = (file_size / 65536) * 65536 - 65536;
        file.seek(SeekFrom::Start(offset)).ok()?;
        file.read_exact(&mut buf).ok()?;

        let magic = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        if magic == 0xa92b_4efc {
            let level = i32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]);
            let chunk_kb = u32::from_le_bytes([buf[40], buf[41], buf[42], buf[43]]);
            let raid_disks = u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]);

            let raid_level = match level {
                0 => Some(RaidLevel::Raid0),
                1 => Some(RaidLevel::Raid1),
                5 => Some(RaidLevel::Raid5),
                6 => Some(RaidLevel::Raid6),
                _ => None,
            };

            if let Some(raid_level) = raid_level {
                return Some(RaidMetadata {
                    level: raid_level,
                    member_count: raid_disks,
                    stripe_size_bytes: chunk_kb as u64 * 1024,
                    superblock_version: "0.90".into(),
                    data_offset_bytes: 0,
                });
            }
        }
    }

    None
}

#[derive(Debug, Clone)]
pub struct RaidMetadata {
    pub level: RaidLevel,
    pub member_count: u32,
    pub stripe_size_bytes: u64,
    pub superblock_version: String,
    pub data_offset_bytes: u64,
}

/// A multi-disk RAID candidate discovered by scanning a batch of devices.
/// Only surfaces when ≥2 members share the same level / stripe / disk
/// count — that's a strong signal the operator plugged in members of the
/// same array. The UI can surface "you plugged in what looks like a
/// 3-disk RAID 5" and offer one-click reconstruction.
#[derive(Debug, Clone)]
pub struct RaidCandidate {
    pub level: RaidLevel,
    pub stripe_size_bytes: u64,
    pub expected_member_count: u32,
    pub superblock_version: String,
    pub members: Vec<PathBuf>,
}

/// Walk a batch of device paths, read each one's RAID metadata, and
/// cluster matching members into `RaidCandidate`s. Caller typically feeds
/// it paths returned by `core::raw_disks::enumerate_unmounted_disks()` —
/// the unmounted set is the usual home of RAID members.
pub fn scan_multi_disk_raid_candidates(paths: &[PathBuf]) -> Vec<RaidCandidate> {
    use std::collections::HashMap;

    type Key = (RaidLevel, u64, u32, String);
    let mut groups: HashMap<Key, Vec<PathBuf>> = HashMap::new();
    for path in paths {
        if let Some(meta) = detect_raid_metadata(path) {
            let key = (
                meta.level,
                meta.stripe_size_bytes,
                meta.member_count,
                meta.superblock_version.clone(),
            );
            groups.entry(key).or_default().push(path.clone());
        }
    }

    groups
        .into_iter()
        .filter(|(_, members)| members.len() >= 2)
        .map(|((level, stripe, count, version), members)| RaidCandidate {
            level,
            stripe_size_bytes: stripe,
            expected_member_count: count,
            superblock_version: version,
            members,
        })
        .collect()
}

pub fn materialize_raid_image(
    config: RaidConfig,
    destination_path: &Path,
    progress: &mut dyn FnMut(u64) -> Result<(), String>,
) -> Result<u64, String> {
    let mut source = RaidSource::open(config)?;
    let total_bytes = source.logical_size();

    if let Some(parent) = destination_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Unable to prepare the RAID analysis directory {}: {error}",
                parent.to_string_lossy()
            )
        })?;
    }

    let destination = File::create(destination_path).map_err(|error| {
        format!(
            "Unable to create the RAID analysis image {}: {error}",
            destination_path.to_string_lossy()
        )
    })?;
    let mut writer = BufWriter::new(destination);
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut copied = 0_u64;

    loop {
        let read = source.read(&mut buffer).map_err(|error| {
            format!(
                "Unable to read the virtual RAID source while building {}: {error}",
                destination_path.to_string_lossy()
            )
        })?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read]).map_err(|error| {
            format!(
                "Unable to write the RAID analysis image {}: {error}",
                destination_path.to_string_lossy()
            )
        })?;
        copied = copied.saturating_add(read as u64);
        progress(copied)?;
    }

    writer.flush().map_err(|error| {
        format!(
            "Unable to finalize the RAID analysis image {}: {error}",
            destination_path.to_string_lossy()
        )
    })?;

    if total_bytes > 0 && copied != total_bytes {
        return Err(format!(
            "The RAID analysis image is incomplete (copied {} bytes out of {} expected bytes).",
            copied, total_bytes
        ));
    }

    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Write;

    fn temp_member_path(size: usize, idx: usize) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("recupere-raid-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("member-{}-{}.img", size, idx));
        let mut f = File::create(&path).unwrap();
        f.write_all(&vec![0u8; size]).unwrap();
        path
    }

    #[test]
    fn raid0_read_distributes_across_members() {
        let p1 = temp_member_path(1024 * 1024, 0);
        let p2 = temp_member_path(1024 * 1024, 1);
        let config = RaidConfig {
            level: RaidLevel::Raid0,
            member_paths: vec![p1, p2],
            stripe_size_bytes: 64 * 1024,
            data_offset_bytes: 0,
            missing_members: vec![],
        };
        let mut source = RaidSource::open(config).unwrap();
        assert_eq!(source.logical_size(), 2 * 1024 * 1024);
        let mut buf = [0u8; 128];
        let n = source.read(&mut buf).unwrap();
        assert!(n > 0);
    }

    #[test]
    fn raid1_read_from_first_member() {
        let p1 = temp_member_path(1024 * 1024, 2);
        let p2 = temp_member_path(1024 * 1024, 3);
        let config = RaidConfig {
            level: RaidLevel::Raid1,
            member_paths: vec![p1, p2],
            stripe_size_bytes: 64 * 1024,
            data_offset_bytes: 0,
            missing_members: vec![],
        };
        let source = RaidSource::open(config).unwrap();
        assert_eq!(source.logical_size(), 1024 * 1024);
    }

    #[test]
    fn scan_multi_disk_raid_candidates_clusters_matching_members() {
        // Three synthetic images, two of which carry an MD superblock v1.2
        // at offset 4096 with identical level/stripe/count — the third has
        // random bytes. The helper must surface ONE candidate holding
        // exactly the two matching members.
        let root =
            std::env::temp_dir().join(format!("recupere-raid-multidisk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("raid multidisk dir");

        let mut member_a = vec![0u8; 8192];
        // MD magic 0xa92b4efc at offset 4096
        member_a[4096..4100].copy_from_slice(&0xa92b_4efcu32.to_le_bytes());
        // major_version = 1 at offset +4
        member_a[4100..4104].copy_from_slice(&1u32.to_le_bytes());
        // level = 5 at offset +28
        member_a[4096 + 28..4096 + 32].copy_from_slice(&5i32.to_le_bytes());
        // raid_disks = 3 at offset +36
        member_a[4096 + 36..4096 + 40].copy_from_slice(&3u32.to_le_bytes());
        // chunk_kb = 64 at offset +40
        member_a[4096 + 40..4096 + 44].copy_from_slice(&64u32.to_le_bytes());
        let member_b = member_a.clone();
        let noise = vec![0x42u8; 8192];

        let path_a = root.join("a.img");
        let path_b = root.join("b.img");
        let path_noise = root.join("c.img");
        std::fs::write(&path_a, &member_a).unwrap();
        std::fs::write(&path_b, &member_b).unwrap();
        std::fs::write(&path_noise, &noise).unwrap();

        let candidates =
            scan_multi_disk_raid_candidates(&[path_a.clone(), path_b.clone(), path_noise]);
        assert_eq!(candidates.len(), 1);
        let cand = &candidates[0];
        assert_eq!(cand.level, RaidLevel::Raid5);
        assert_eq!(cand.expected_member_count, 3);
        assert_eq!(cand.stripe_size_bytes, 64 * 1024);
        assert_eq!(cand.members.len(), 2);
        assert!(cand.members.contains(&path_a));
        assert!(cand.members.contains(&path_b));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn raid5_logical_size_excludes_parity() {
        let p1 = temp_member_path(1024 * 1024, 4);
        let p2 = temp_member_path(1024 * 1024, 5);
        let p3 = temp_member_path(1024 * 1024, 6);
        let config = RaidConfig {
            level: RaidLevel::Raid5,
            member_paths: vec![p1, p2, p3],
            stripe_size_bytes: 64 * 1024,
            data_offset_bytes: 0,
            missing_members: vec![],
        };
        let source = RaidSource::open(config).unwrap();
        // 3 members, RAID 5 = 2 data disks worth
        assert_eq!(source.logical_size(), 2 * 1024 * 1024);
    }

    #[test]
    fn materialize_raid_image_writes_logical_bytes_to_destination() {
        let p1 = temp_member_path(512 * 1024, 7);
        let p2 = temp_member_path(512 * 1024, 8);
        let config = RaidConfig {
            level: RaidLevel::Raid1,
            member_paths: vec![p1, p2],
            stripe_size_bytes: 64 * 1024,
            data_offset_bytes: 0,
            missing_members: vec![],
        };
        let destination = std::env::temp_dir().join(format!(
            "recupere-raid-materialized-{}.img",
            std::process::id()
        ));
        let copied = materialize_raid_image(config, &destination, &mut |_copied| Ok(())).unwrap();
        let metadata = std::fs::metadata(&destination).unwrap();
        assert_eq!(copied, 512 * 1024);
        assert_eq!(metadata.len(), 512 * 1024);
        let _ = std::fs::remove_file(destination);
    }

    #[test]
    fn raid6_dual_missing_data_members_are_reconstructed() {
        let root = std::env::temp_dir().join(format!("recupere-raid6-dual-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let d0 = vec![1u8, 2, 3, 4];
        let d1 = vec![5u8, 6, 7, 8];
        let p = gf::compute_p(&[&d0, &d1]);
        let q = gf::compute_q(&[&d0, &d1]);

        let path0 = root.join("m0.img");
        let path1 = root.join("m1.img");
        let path2 = root.join("m2.img");
        let path3 = root.join("m3.img");
        std::fs::write(&path0, &d0).unwrap();
        std::fs::write(&path1, &d1).unwrap();
        std::fs::write(&path2, &q).unwrap();
        std::fs::write(&path3, &p).unwrap();

        let config = RaidConfig {
            level: RaidLevel::Raid6,
            member_paths: vec![path0, path1, path2, path3],
            stripe_size_bytes: 4,
            data_offset_bytes: 0,
            missing_members: vec![0, 1],
        };
        let mut source = RaidSource::open(config).unwrap();
        let mut out = vec![0u8; 8];
        let n = source.read(&mut out).unwrap();
        assert_eq!(n, 8);
        assert_eq!(&out[..4], &d0);
        assert_eq!(&out[4..8], &d1);

        let _ = std::fs::remove_dir_all(root);
    }
}
