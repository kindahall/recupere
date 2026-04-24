#![allow(dead_code)]
// ============================================================================
// Recupere — Virtual Disk Support
// ============================================================================
// Provides a trait abstraction for reading from virtual disk formats
// (E01, VMDK, VHD) so that carving and analyzers can operate on any source.
// ============================================================================

pub mod e01;
pub mod vhd;
pub mod vmdk;

use std::{
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::Path,
};

/// A recoverable source that can be read and seeked.
pub trait RecoverySource: Read + Seek + Send {
    fn total_size(&self) -> u64;
    fn format_name(&self) -> &str;
}

/// Raw file source (DD/IMG format) — wraps a standard file.
pub struct RawFileSource {
    reader: BufReader<File>,
    size: u64,
}

impl RawFileSource {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path)
            .map_err(|e| format!("Cannot open raw image {}: {e}", path.to_string_lossy()))?;
        let size = file
            .metadata()
            .map_err(|e| format!("Cannot read metadata for {}: {e}", path.to_string_lossy()))?
            .len();
        Ok(Self {
            reader: BufReader::new(file),
            size,
        })
    }
}

impl Read for RawFileSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }
}

impl Seek for RawFileSource {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.reader.seek(pos)
    }
}

impl RecoverySource for RawFileSource {
    fn total_size(&self) -> u64 {
        self.size
    }
    fn format_name(&self) -> &str {
        "raw"
    }
}

/// Auto-detect and open the appropriate source format based on file extension.
pub fn open_recovery_source(path: &Path) -> Result<Box<dyn RecoverySource>, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "e01" => Ok(Box::new(e01::E01Source::open(path)?)),
        "vmdk" => Ok(Box::new(vmdk::VmdkSource::open(path)?)),
        "vhd" | "vhdx" => Ok(Box::new(vhd::VhdSource::open(path)?)),
        _ => Ok(Box::new(RawFileSource::open(path)?)),
    }
}

/// Check if a file extension indicates a virtual disk format.
pub fn is_virtual_disk_extension(ext: &str) -> bool {
    matches!(ext.to_lowercase().as_str(), "e01" | "vmdk" | "vhd" | "vhdx")
}
