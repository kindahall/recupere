#![allow(dead_code)]
// ============================================================================
// E01 (EnCase Evidence File / EWF) Source — Real Implementation
// ============================================================================
// Parses EWF segment headers, builds a chunk index, and decompresses
// zlib-compressed chunks on demand to provide a Read+Seek interface
// over the logical evidence stream.
// ============================================================================

use super::RecoverySource;
use flate2::read::ZlibDecoder;
use std::{
    fs::File,
    io::{self, BufReader, Cursor, Read, Seek, SeekFrom},
    path::Path,
};

const EWF_SIGNATURE: &[u8; 8] = b"EVF\x09\x0d\x0a\xff\x00";
const EWF_SECTION_HEADER_SIZE: usize = 76;

#[derive(Debug, Clone)]
struct EwfChunk {
    offset_in_file: u64,
    compressed_size: u32,
    is_compressed: bool,
}

pub struct E01Source {
    reader: BufReader<File>,
    chunks: Vec<EwfChunk>,
    chunk_size: u32,
    logical_size: u64,
    position: u64,
    // Cache for the last decompressed chunk
    cached_chunk_index: Option<usize>,
    cached_chunk_data: Vec<u8>,
}

impl E01Source {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path)
            .map_err(|e| format!("Cannot open E01 file {}: {e}", path.to_string_lossy()))?;
        let file_size = file
            .metadata()
            .map_err(|e| format!("Cannot read E01 metadata: {e}"))?
            .len();
        let mut reader = BufReader::new(file);

        // Validate EWF signature
        let mut sig = [0u8; 8];
        reader
            .read_exact(&mut sig)
            .map_err(|e| format!("Cannot read E01 signature: {e}"))?;
        if &sig != EWF_SIGNATURE {
            return Err("Not a valid E01 (EWF) evidence file.".into());
        }

        // Skip to first section after 13-byte header
        reader
            .seek(SeekFrom::Start(13))
            .map_err(|e| format!("Cannot seek E01: {e}"))?;

        // Parse sections to find chunk table and data
        let mut chunks = Vec::new();
        let mut chunk_size: u32 = 32768; // Default 32KB chunks
        let mut logical_size: u64 = 0;
        let mut offset = 13u64;

        while offset < file_size.saturating_sub(EWF_SECTION_HEADER_SIZE as u64) {
            let mut section_header = [0u8; EWF_SECTION_HEADER_SIZE];
            if reader.seek(SeekFrom::Start(offset)).is_err() {
                break;
            }
            if reader.read_exact(&mut section_header).is_err() {
                break;
            }

            // Section type is a 16-byte string at offset 0
            let section_type = String::from_utf8_lossy(&section_header[0..16])
                .trim_end_matches('\0')
                .to_lowercase();
            let section_size = u64::from_le_bytes([
                section_header[16],
                section_header[17],
                section_header[18],
                section_header[19],
                section_header[20],
                section_header[21],
                section_header[22],
                section_header[23],
            ]);
            let next_offset = u64::from_le_bytes([
                section_header[24],
                section_header[25],
                section_header[26],
                section_header[27],
                section_header[28],
                section_header[29],
                section_header[30],
                section_header[31],
            ]);

            match section_type.as_str() {
                "header" | "header2" => {
                    // Skip header sections
                }
                "volume" | "disk" => {
                    // Parse volume section for chunk size and media size
                    if section_size > EWF_SECTION_HEADER_SIZE as u64 + 16 {
                        let mut vol = [0u8; 96];
                        let vol_offset = offset + EWF_SECTION_HEADER_SIZE as u64;
                        if reader.seek(SeekFrom::Start(vol_offset)).is_ok()
                            && reader.read_exact(&mut vol).is_ok()
                        {
                            // Chunk size at offset 4 in volume data (LE u32)
                            let cs = u32::from_le_bytes([vol[4], vol[5], vol[6], vol[7]]);
                            if cs > 0 && cs <= 1024 * 1024 {
                                chunk_size = cs;
                            }
                            // Sectors count at offset 8 (LE u64)
                            let sectors = u64::from_le_bytes([
                                vol[8], vol[9], vol[10], vol[11], vol[12], vol[13], vol[14],
                                vol[15],
                            ]);
                            // Bytes per sector at offset 16 (LE u32)
                            let bps = u32::from_le_bytes([vol[16], vol[17], vol[18], vol[19]]);
                            if sectors > 0 && bps > 0 {
                                logical_size = sectors * bps as u64;
                            }
                        }
                    }
                }
                "sectors" => {
                    // Data section: contains compressed chunks sequentially
                    let data_start = offset + EWF_SECTION_HEADER_SIZE as u64;
                    let data_size = section_size.saturating_sub(EWF_SECTION_HEADER_SIZE as u64);
                    let mut data_offset = data_start;
                    let mut remaining = data_size;

                    while remaining >= 4 {
                        // Each chunk: potentially compressed data
                        // Without the table section, estimate chunk boundaries
                        let estimated_chunk_size = (chunk_size as u64).min(remaining);
                        chunks.push(EwfChunk {
                            offset_in_file: data_offset,
                            compressed_size: estimated_chunk_size as u32,
                            is_compressed: true,
                        });
                        data_offset += estimated_chunk_size;
                        remaining = remaining.saturating_sub(estimated_chunk_size);
                    }
                }
                "table" | "table2" => {
                    // Chunk offset table — rebuild chunk index from this
                    let table_start = offset + EWF_SECTION_HEADER_SIZE as u64;
                    let entry_count_size = 4; // LE u32 entry count
                    let mut count_buf = [0u8; 4];
                    if reader.seek(SeekFrom::Start(table_start)).is_ok()
                        && reader.read_exact(&mut count_buf).is_ok()
                    {
                        let entry_count = u32::from_le_bytes(count_buf) as usize;
                        if entry_count > 0 && entry_count < 1_000_000 {
                            // Rebuild chunks from table entries
                            // Each entry is a 4-byte LE offset (MSB = compressed flag)
                            chunks.clear();
                            let mut entries = vec![0u8; entry_count * 4];
                            let entries_offset = table_start + entry_count_size + 16; // skip padding
                            if reader.seek(SeekFrom::Start(entries_offset)).is_ok()
                                && reader.read_exact(&mut entries).is_ok()
                            {
                                for i in 0..entry_count {
                                    let raw = u32::from_le_bytes([
                                        entries[i * 4],
                                        entries[i * 4 + 1],
                                        entries[i * 4 + 2],
                                        entries[i * 4 + 3],
                                    ]);
                                    let is_compressed = (raw & 0x8000_0000) != 0;
                                    let chunk_offset = (raw & 0x7FFF_FFFF) as u64;
                                    let next_offset = if i + 1 < entry_count {
                                        let next_raw = u32::from_le_bytes([
                                            entries[(i + 1) * 4],
                                            entries[(i + 1) * 4 + 1],
                                            entries[(i + 1) * 4 + 2],
                                            entries[(i + 1) * 4 + 3],
                                        ]);
                                        (next_raw & 0x7FFF_FFFF) as u64
                                    } else {
                                        chunk_offset + chunk_size as u64
                                    };
                                    let comp_size = next_offset.saturating_sub(chunk_offset) as u32;
                                    chunks.push(EwfChunk {
                                        offset_in_file: chunk_offset,
                                        compressed_size: comp_size.min(chunk_size * 2),
                                        is_compressed,
                                    });
                                }
                            }
                        }
                    }
                }
                "done" | "next" => break,
                _ => {}
            }

            if next_offset > offset && next_offset < file_size {
                offset = next_offset;
            } else if section_size > 0 {
                offset += section_size;
            } else {
                break;
            }
        }

        if logical_size == 0 && !chunks.is_empty() {
            logical_size = chunks.len() as u64 * chunk_size as u64;
        }

        Ok(Self {
            reader,
            chunks,
            chunk_size,
            logical_size,
            position: 0,
            cached_chunk_index: None,
            cached_chunk_data: Vec::new(),
        })
    }

    fn read_chunk(&mut self, chunk_index: usize) -> io::Result<()> {
        if self.cached_chunk_index == Some(chunk_index) {
            return Ok(());
        }

        let chunk = self
            .chunks
            .get(chunk_index)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Chunk index out of range"))?
            .clone();

        let mut compressed = vec![0u8; chunk.compressed_size as usize];
        self.reader.seek(SeekFrom::Start(chunk.offset_in_file))?;
        self.reader.read_exact(&mut compressed)?;

        if chunk.is_compressed {
            let mut decoder = ZlibDecoder::new(Cursor::new(&compressed));
            self.cached_chunk_data.clear();
            self.cached_chunk_data.resize(self.chunk_size as usize, 0);
            let n = decoder.read(&mut self.cached_chunk_data).unwrap_or(0);
            self.cached_chunk_data.truncate(n);
        } else {
            self.cached_chunk_data = compressed;
        }

        self.cached_chunk_index = Some(chunk_index);
        Ok(())
    }
}

impl Read for E01Source {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.position >= self.logical_size || self.chunks.is_empty() {
            return Ok(0);
        }

        let chunk_index = (self.position / self.chunk_size as u64) as usize;
        let offset_in_chunk = (self.position % self.chunk_size as u64) as usize;

        if chunk_index >= self.chunks.len() {
            return Ok(0);
        }

        self.read_chunk(chunk_index)?;

        let available = self.cached_chunk_data.len().saturating_sub(offset_in_chunk);
        let to_copy = buf.len().min(available);
        if to_copy == 0 {
            return Ok(0);
        }

        buf[..to_copy]
            .copy_from_slice(&self.cached_chunk_data[offset_in_chunk..offset_in_chunk + to_copy]);
        self.position += to_copy as u64;
        Ok(to_copy)
    }
}

impl Seek for E01Source {
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

impl RecoverySource for E01Source {
    fn total_size(&self) -> u64 {
        self.logical_size
    }
    fn format_name(&self) -> &str {
        "e01"
    }
}
