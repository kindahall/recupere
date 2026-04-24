#![allow(dead_code)]
// ============================================================================
// VHD/VHDX (Microsoft Virtual Hard Disk) Source — Real Implementation
// ============================================================================
// Parses VHD fixed/dynamic disk footers and BAT (Block Allocation Table)
// to provide Read+Seek over the logical disk content.
// ============================================================================

use super::RecoverySource;
use std::{
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::Path,
};

const VHD_COOKIE: &[u8; 8] = b"conectix";
const VHD_DYNAMIC_COOKIE: &[u8; 8] = b"cxsparse";
const VHDX_SIGNATURE: &[u8; 8] = b"vhdxfile";
const SECTOR_SIZE: u64 = 512;

#[derive(Debug, Clone, Copy, PartialEq)]
enum VhdType {
    Fixed,
    Dynamic,
    Differencing,
}

pub struct VhdSource {
    reader: BufReader<File>,
    logical_size: u64,
    vhd_type: VhdType,
    block_size: u64,
    bat: Vec<u32>, // Block Allocation Table: sector offset per block (0xFFFFFFFF = sparse)
    data_offset: u64,
    position: u64,
}

impl VhdSource {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path)
            .map_err(|e| format!("Cannot open VHD file {}: {e}", path.to_string_lossy()))?;
        let file_size = file
            .metadata()
            .map_err(|e| format!("Cannot read VHD metadata: {e}"))?
            .len();
        let mut reader = BufReader::new(file);

        // Check for VHDX first (at start of file)
        let mut sig = [0u8; 8];
        reader
            .read_exact(&mut sig)
            .map_err(|e| format!("Cannot read VHD signature: {e}"))?;

        if &sig == VHDX_SIGNATURE {
            return Self::open_vhdx(reader, file_size);
        }

        // VHD: footer is at the last 512 bytes
        if file_size < 512 {
            return Err("File too small to be a valid VHD.".into());
        }

        let mut footer = [0u8; 512];
        reader
            .seek(SeekFrom::Start(file_size - 512))
            .map_err(|e| format!("Cannot seek VHD footer: {e}"))?;
        reader
            .read_exact(&mut footer)
            .map_err(|e| format!("Cannot read VHD footer: {e}"))?;

        if &footer[0..8] != VHD_COOKIE {
            // Try footer copy at start of file
            reader
                .seek(SeekFrom::Start(0))
                .map_err(|e| format!("seek: {e}"))?;
            reader
                .read_exact(&mut footer)
                .map_err(|e| format!("read: {e}"))?;
            if &footer[0..8] != VHD_COOKIE {
                return Err("Not a valid VHD file (no conectix footer).".into());
            }
        }

        // Parse footer
        let disk_type = u32::from_be_bytes([footer[60], footer[61], footer[62], footer[63]]);
        let logical_size = u64::from_be_bytes([
            footer[48], footer[49], footer[50], footer[51], footer[52], footer[53], footer[54],
            footer[55],
        ]);
        let data_offset = u64::from_be_bytes([
            footer[16], footer[17], footer[18], footer[19], footer[20], footer[21], footer[22],
            footer[23],
        ]);

        let vhd_type = match disk_type {
            2 => VhdType::Fixed,
            3 => VhdType::Dynamic,
            4 => VhdType::Differencing,
            _ => return Err(format!("Unsupported VHD type: {disk_type}")),
        };

        if vhd_type == VhdType::Fixed {
            // Fixed VHD: data starts at offset 0, footer at end
            return Ok(Self {
                reader,
                logical_size,
                vhd_type,
                block_size: 0,
                bat: Vec::new(),
                data_offset: 0,
                position: 0,
            });
        }

        // Dynamic/Differencing: parse sparse header
        if data_offset + 1024 > file_size {
            return Err("VHD dynamic header offset out of bounds.".into());
        }

        let mut dyn_header = [0u8; 1024];
        reader
            .seek(SeekFrom::Start(data_offset))
            .map_err(|e| format!("Cannot seek dynamic header: {e}"))?;
        reader
            .read_exact(&mut dyn_header)
            .map_err(|e| format!("Cannot read dynamic header: {e}"))?;

        if &dyn_header[0..8] != VHD_DYNAMIC_COOKIE {
            return Err("Invalid VHD dynamic header (no cxsparse cookie).".into());
        }

        let bat_offset = u64::from_be_bytes([
            dyn_header[16],
            dyn_header[17],
            dyn_header[18],
            dyn_header[19],
            dyn_header[20],
            dyn_header[21],
            dyn_header[22],
            dyn_header[23],
        ]);
        let max_table_entries = u32::from_be_bytes([
            dyn_header[24],
            dyn_header[25],
            dyn_header[26],
            dyn_header[27],
        ]) as usize;
        let block_size = u32::from_be_bytes([
            dyn_header[28],
            dyn_header[29],
            dyn_header[30],
            dyn_header[31],
        ]) as u64;

        if block_size == 0 || max_table_entries == 0 || max_table_entries > 10_000_000 {
            return Err("Invalid VHD block allocation table parameters.".into());
        }

        // Read BAT
        let mut bat_bytes = vec![0u8; max_table_entries * 4];
        reader
            .seek(SeekFrom::Start(bat_offset))
            .map_err(|e| format!("Cannot seek BAT: {e}"))?;
        reader
            .read_exact(&mut bat_bytes)
            .map_err(|e| format!("Cannot read BAT: {e}"))?;

        let bat: Vec<u32> = (0..max_table_entries)
            .map(|i| {
                u32::from_be_bytes([
                    bat_bytes[i * 4],
                    bat_bytes[i * 4 + 1],
                    bat_bytes[i * 4 + 2],
                    bat_bytes[i * 4 + 3],
                ])
            })
            .collect();

        Ok(Self {
            reader,
            logical_size,
            vhd_type,
            block_size,
            bat,
            data_offset: bat_offset,
            position: 0,
        })
    }

    fn open_vhdx(_reader: BufReader<File>, _file_size: u64) -> Result<Self, String> {
        // VHDX uses region tables + BAT metadata distinct from VHD. Parsing
        // isn't implemented yet, and returning the file as-is would feed the
        // VHDX header to analyzers as if it were user data — a "magic
        // recovery" violation of AGENTS.md. Reject explicitly instead.
        Err(
            "VHDX format detected but not yet supported. Use VHD, E01, VMDK or raw .img/.dd."
                .into(),
        )
    }
}

impl Read for VhdSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.position >= self.logical_size {
            return Ok(0);
        }

        let to_read = buf.len().min((self.logical_size - self.position) as usize);

        if self.vhd_type == VhdType::Fixed || self.block_size == 0 {
            // Fixed VHD: read directly
            self.reader.seek(SeekFrom::Start(self.position))?;
            let n = self.reader.read(&mut buf[..to_read])?;
            self.position += n as u64;
            return Ok(n);
        }

        // Dynamic VHD: translate through BAT
        let block_index = (self.position / self.block_size) as usize;
        let offset_in_block = (self.position % self.block_size) as usize;
        let remaining_in_block = (self.block_size as usize).saturating_sub(offset_in_block);
        let chunk = to_read.min(remaining_in_block);

        if block_index >= self.bat.len() || self.bat[block_index] == 0xFFFF_FFFF {
            // Sparse block — return zeros
            buf[..chunk].fill(0);
            self.position += chunk as u64;
            return Ok(chunk);
        }

        // BAT entry points to sector offset of the block
        // Each block has a bitmap sector followed by data
        let block_sector = self.bat[block_index] as u64;
        let bitmap_sectors = (self.block_size / SECTOR_SIZE / 8).max(1);
        let data_offset = (block_sector + bitmap_sectors) * SECTOR_SIZE + offset_in_block as u64;

        self.reader.seek(SeekFrom::Start(data_offset))?;
        let n = self.reader.read(&mut buf[..chunk])?;
        self.position += n as u64;
        Ok(n)
    }
}

impl Seek for VhdSource {
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

impl RecoverySource for VhdSource {
    fn total_size(&self) -> u64 {
        self.logical_size
    }
    fn format_name(&self) -> &str {
        "vhd"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn vhdx_signature_is_rejected_with_explicit_error() {
        let dir = std::env::temp_dir().join(format!("recupere-vhdx-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("test.vhdx");
        {
            let mut f = File::create(&path).expect("create");
            f.write_all(b"vhdxfile").expect("write signature");
            f.write_all(&[0u8; 4096]).expect("write padding");
        }

        let result = VhdSource::open(&path);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);

        let err = match result {
            Ok(_) => panic!("VHDX must be rejected, got Ok"),
            Err(e) => e,
        };
        assert!(err.contains("VHDX"), "error must mention VHDX, got: {err}");
        assert!(
            err.contains("not yet supported"),
            "error must state not supported, got: {err}"
        );
    }
}
