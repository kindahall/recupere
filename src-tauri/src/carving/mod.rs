#![allow(dead_code)]
use crate::types::ByteRun;
use crc32fast::Hasher;
use rayon::prelude::*;
use std::{
    collections::HashSet,
    fs::File,
    io::{Cursor, Read, Seek, SeekFrom},
    path::Path,
};
use zip::ZipArchive;

const CARVING_CHUNK_SIZE: usize = 64 * 1024;
const CARVING_OVERLAP_SIZE: usize = 64;
const FRAGMENT_GAP_MIN_SIZE: usize = 32;
const MAX_REMOVABLE_GAPS: usize = 8;
const MAX_ASSEMBLY_SEGMENTS: usize = 10;
const MAX_FRAGMENT_GAP_CANDIDATES: usize = 16;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone)]
pub struct CarvedFileCandidate {
    pub name: String,
    pub extension: String,
    pub size_bytes: u64,
    pub integrity: String,
    pub recovery_score: u8,
    pub start_offset: u64,
    pub byte_runs: Vec<ByteRun>,
    pub validator_status: String,
    pub assembly_segment_count: u8,
    pub gap_count: u8,
    pub recovery_complexity: String,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum FooterStrategy {
    Fixed(&'static [u8]),
    ZipEocd,
    RiffChunk,
    ContainerAtom,
    EbmlElement,
    OleCompound,
    MaxScan,
    SizeFieldLe {
        offset_from_header: usize,
        field_size: u8,
    },
    SizeFieldBe {
        offset_from_header: usize,
        field_size: u8,
    },
    Id3Mp3,
}

#[derive(Debug, Clone, Copy)]
struct SignatureDefinition {
    label: &'static str,
    extension: &'static str,
    header: &'static [u8],
    sub_header: Option<(&'static [u8], usize)>,
    footer: FooterStrategy,
    min_size: u64,
    max_size: u64,
}

const MB: u64 = 1024 * 1024;
const GB: u64 = 1024 * MB;

const SIGNATURES: &[SignatureDefinition] = &[
    // =========================================================================
    // Images
    // =========================================================================
    SignatureDefinition {
        label: "jpeg",
        extension: "jpg",
        header: &[0xff, 0xd8, 0xff],
        sub_header: None,
        footer: FooterStrategy::Fixed(&[0xff, 0xd9]),
        min_size: 16,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "png",
        extension: "png",
        header: &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
        sub_header: None,
        footer: FooterStrategy::Fixed(&[b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]),
        min_size: 48,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "gif87a",
        extension: "gif",
        header: b"GIF87a",
        sub_header: None,
        footer: FooterStrategy::Fixed(&[0x00, 0x3B]),
        min_size: 13,
        max_size: 32 * MB,
    },
    SignatureDefinition {
        label: "gif89a",
        extension: "gif",
        header: b"GIF89a",
        sub_header: None,
        footer: FooterStrategy::Fixed(&[0x00, 0x3B]),
        min_size: 13,
        max_size: 32 * MB,
    },
    SignatureDefinition {
        label: "bmp",
        extension: "bmp",
        header: &[0x42, 0x4D],
        sub_header: None,
        footer: FooterStrategy::SizeFieldLe {
            offset_from_header: 2,
            field_size: 4,
        },
        min_size: 26,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "tiff-le",
        extension: "tiff",
        header: &[0x49, 0x49, 0x2A, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 128 * MB,
    },
    SignatureDefinition {
        label: "tiff-be",
        extension: "tiff",
        header: &[0x4D, 0x4D, 0x00, 0x2A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 128 * MB,
    },
    SignatureDefinition {
        label: "webp",
        extension: "webp",
        header: b"RIFF",
        sub_header: Some((b"WEBP", 8)),
        footer: FooterStrategy::RiffChunk,
        min_size: 12,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "psd",
        extension: "psd",
        header: b"8BPS",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 30,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "ico",
        extension: "ico",
        header: &[0x00, 0x00, 0x01, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 22,
        max_size: 4 * MB,
    },
    SignatureDefinition {
        label: "heic",
        extension: "heic",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftyp", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "cr2",
        extension: "cr2",
        header: &[0x49, 0x49, 0x2A, 0x00],
        sub_header: Some((&[0x43, 0x52], 8)),
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 128 * MB,
    },
    // =========================================================================
    // Video
    // =========================================================================
    SignatureDefinition {
        label: "mp4",
        extension: "mp4",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftyp", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 8,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "avi",
        extension: "avi",
        header: b"RIFF",
        sub_header: Some((b"AVI ", 8)),
        footer: FooterStrategy::RiffChunk,
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "mkv",
        extension: "mkv",
        header: &[0x1A, 0x45, 0xDF, 0xA3],
        sub_header: None,
        footer: FooterStrategy::EbmlElement,
        min_size: 64,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "flv",
        extension: "flv",
        header: b"FLV\x01",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 9,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "wmv",
        extension: "wmv",
        header: &[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 24,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "mpg",
        extension: "mpg",
        header: &[0x00, 0x00, 0x01, 0xBA],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 4 * GB,
    },
    // =========================================================================
    // Audio
    // =========================================================================
    SignatureDefinition {
        label: "mp3-id3",
        extension: "mp3",
        header: b"ID3",
        sub_header: None,
        footer: FooterStrategy::Id3Mp3,
        min_size: 128,
        max_size: 128 * MB,
    },
    SignatureDefinition {
        label: "mp3-sync",
        extension: "mp3",
        header: &[0xFF, 0xFB],
        sub_header: None,
        footer: FooterStrategy::Id3Mp3,
        min_size: 128,
        max_size: 128 * MB,
    },
    SignatureDefinition {
        label: "mp3-sync2",
        extension: "mp3",
        header: &[0xFF, 0xFA],
        sub_header: None,
        footer: FooterStrategy::Id3Mp3,
        min_size: 128,
        max_size: 128 * MB,
    },
    SignatureDefinition {
        label: "wav",
        extension: "wav",
        header: b"RIFF",
        sub_header: Some((b"WAVE", 8)),
        footer: FooterStrategy::RiffChunk,
        min_size: 12,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "flac",
        extension: "flac",
        header: b"fLaC",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 42,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "ogg",
        extension: "ogg",
        header: b"OggS",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 28,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "aac",
        extension: "aac",
        header: &[0xFF, 0xF1],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 7,
        max_size: 128 * MB,
    },
    SignatureDefinition {
        label: "midi",
        extension: "mid",
        header: b"MThd",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 14,
        max_size: 16 * MB,
    },
    SignatureDefinition {
        label: "aiff",
        extension: "aiff",
        header: b"FORM",
        sub_header: Some((b"AIFF", 8)),
        footer: FooterStrategy::RiffChunk,
        min_size: 12,
        max_size: 2 * GB,
    },
    // =========================================================================
    // Documents
    // =========================================================================
    SignatureDefinition {
        label: "pdf",
        extension: "pdf",
        header: b"%PDF-",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"%%EOF"),
        min_size: 32,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "ole2",
        extension: "doc",
        header: &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
        sub_header: None,
        footer: FooterStrategy::OleCompound,
        min_size: 512,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "rtf",
        extension: "rtf",
        header: b"{\\rtf",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"}"),
        min_size: 8,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "xml",
        extension: "xml",
        header: b"<?xml",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 10,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "html",
        extension: "html",
        header: b"<!DOCTYPE html",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"</html>"),
        min_size: 20,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "html2",
        extension: "html",
        header: b"<html",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"</html>"),
        min_size: 12,
        max_size: 64 * MB,
    },
    // =========================================================================
    // Archives
    // =========================================================================
    SignatureDefinition {
        label: "zip",
        extension: "zip",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "rar5",
        extension: "rar",
        header: &[0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x01, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 20,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "rar4",
        extension: "rar",
        header: &[0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 20,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "7z",
        extension: "7z",
        header: &[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "gz",
        extension: "gz",
        header: &[0x1F, 0x8B],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 18,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "xz",
        extension: "xz",
        header: &[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00],
        sub_header: None,
        footer: FooterStrategy::Fixed(&[0x59, 0x5A]),
        min_size: 12,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "bz2",
        extension: "bz2",
        header: &[0x42, 0x5A, 0x68],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 10,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "tar",
        extension: "tar",
        header: &[0x75, 0x73, 0x74, 0x61, 0x72],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 512,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "zstd",
        extension: "zst",
        header: &[0x28, 0xB5, 0x2F, 0xFD],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 2 * GB,
    },
    // =========================================================================
    // Database
    // =========================================================================
    SignatureDefinition {
        label: "sqlite",
        extension: "sqlite",
        header: b"SQLite format 3\x00",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 100,
        max_size: 2 * GB,
    },
    // =========================================================================
    // Email
    // =========================================================================
    SignatureDefinition {
        label: "pst",
        extension: "pst",
        header: b"!BDN",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 512,
        max_size: 50 * GB,
    },
    SignatureDefinition {
        label: "eml",
        extension: "eml",
        header: b"From:",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 64 * MB,
    },
    // =========================================================================
    // Executables
    // =========================================================================
    SignatureDefinition {
        label: "elf",
        extension: "elf",
        header: &[0x7F, b'E', b'L', b'F'],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 52,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "pe",
        extension: "exe",
        header: &[0x4D, 0x5A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "macho32",
        extension: "macho",
        header: &[0xFE, 0xED, 0xFA, 0xCE],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 28,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "macho64",
        extension: "macho",
        header: &[0xFE, 0xED, 0xFA, 0xCF],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 512 * MB,
    },
    // =========================================================================
    // Misc
    // =========================================================================
    SignatureDefinition {
        label: "wasm",
        extension: "wasm",
        header: &[0x00, 0x61, 0x73, 0x6D],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 128 * MB,
    },
    SignatureDefinition {
        label: "json",
        extension: "json",
        header: b"{\"",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 2,
        max_size: 64 * MB,
    },
    // =========================================================================
    // Camera RAW formats
    // =========================================================================
    SignatureDefinition {
        label: "cr3",
        extension: "cr3",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypcrx ", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "nef",
        extension: "nef",
        header: &[0x4D, 0x4D, 0x00, 0x2A],
        sub_header: Some((&[0x1C], 8)),
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "arw",
        extension: "arw",
        header: &[0x49, 0x49, 0x2A, 0x00],
        sub_header: Some((&[0x08, 0x00], 4)),
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "orf",
        extension: "orf",
        header: &[0x49, 0x49, 0x52, 0x4F],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "rw2",
        extension: "rw2",
        header: &[0x49, 0x49, 0x55, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "raf",
        extension: "raf",
        header: b"FUJIFILMCCD-RAW",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "dng",
        extension: "dng",
        header: &[0x49, 0x49, 0x2A, 0x00],
        sub_header: Some((&[0x08], 4)),
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "srw",
        extension: "srw",
        header: &[0x49, 0x49, 0x2A, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "pef",
        extension: "pef",
        header: &[0x4D, 0x4D, 0x00, 0x2A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "x3f",
        extension: "x3f",
        header: b"FOVb",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "mrw",
        extension: "mrw",
        header: &[0x00, 0x4D, 0x52, 0x4D],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "erf",
        extension: "erf",
        header: &[0x49, 0x49, 0x2A, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 128 * MB,
    },
    SignatureDefinition {
        label: "nrw",
        extension: "nrw",
        header: &[0x49, 0x49, 0x2A, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "3fr",
        extension: "3fr",
        header: &[0x49, 0x49, 0x2A, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "dcr",
        extension: "dcr",
        header: &[0x49, 0x49, 0x2A, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "kdc",
        extension: "kdc",
        header: &[0x49, 0x49, 0x2A, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "mef",
        extension: "mef",
        header: &[0x49, 0x49, 0x2A, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "mos",
        extension: "mos",
        header: &[0x49, 0x49, 0x2A, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "iiq",
        extension: "iiq",
        header: &[0x49, 0x49, 0x2A, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: GB,
    },
    SignatureDefinition {
        label: "rwl",
        extension: "rwl",
        header: &[0x49, 0x49, 0x2A, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    // =========================================================================
    // Vector/Design formats
    // =========================================================================
    SignatureDefinition {
        label: "svg",
        extension: "svg",
        header: b"<svg",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"</svg>"),
        min_size: 16,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "svg-xml",
        extension: "svg",
        header: b"<?xml",
        sub_header: Some((b"<svg", 20)),
        footer: FooterStrategy::Fixed(b"</svg>"),
        min_size: 32,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "eps",
        extension: "eps",
        header: b"%!PS-Adobe",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"%%EOF"),
        min_size: 32,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "ai",
        extension: "ai",
        header: b"%PDF-",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"%%EOF"),
        min_size: 32,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "cdr",
        extension: "cdr",
        header: b"RIFF",
        sub_header: Some((b"CDR", 8)),
        footer: FooterStrategy::RiffChunk,
        min_size: 12,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "dwg",
        extension: "dwg",
        header: b"AC10",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "dxf",
        extension: "dxf",
        header: b"0\r\nSECTION",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"EOF\r\n"),
        min_size: 32,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "step",
        extension: "step",
        header: b"ISO-10303-21;",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"END-ISO-10303-21;"),
        min_size: 32,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "stl-bin",
        extension: "stl",
        header: &[0x73, 0x6F, 0x6C, 0x69, 0x64],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 84,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "ply",
        extension: "ply",
        header: b"ply\n",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 512 * MB,
    },
    // =========================================================================
    // Video formats (additional)
    // =========================================================================
    SignatureDefinition {
        label: "3gp",
        extension: "3gp",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftyp3gp", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "m4v",
        extension: "m4v",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypM4V", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "m2ts",
        extension: "m2ts",
        header: &[0x47],
        sub_header: Some((&[0x47], 192)),
        footer: FooterStrategy::MaxScan,
        min_size: 192,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "vob",
        extension: "vob",
        header: &[0x00, 0x00, 0x01, 0xBA],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "ts",
        extension: "ts",
        header: &[0x47],
        sub_header: Some((&[0x47], 188)),
        footer: FooterStrategy::MaxScan,
        min_size: 188,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "webm",
        extension: "webm",
        header: &[0x1A, 0x45, 0xDF, 0xA3],
        sub_header: None,
        footer: FooterStrategy::EbmlElement,
        min_size: 64,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "asf",
        extension: "asf",
        header: &[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 24,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "swf-compressed",
        extension: "swf",
        header: b"CWS",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "swf-uncompressed",
        extension: "swf",
        header: b"FWS",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "swf-lzma",
        extension: "swf",
        header: b"ZWS",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "ogv",
        extension: "ogv",
        header: b"OggS",
        sub_header: Some((b"\x80theora", 28)),
        footer: FooterStrategy::MaxScan,
        min_size: 28,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "rmvb",
        extension: "rmvb",
        header: &[0x2E, 0x52, 0x4D, 0x46],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "dv",
        extension: "dv",
        header: &[0x1F, 0x07, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 120000,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "mxf",
        extension: "mxf",
        header: &[
            0x06, 0x0E, 0x2B, 0x34, 0x02, 0x05, 0x01, 0x01, 0x0D, 0x01, 0x02, 0x01, 0x01, 0x02,
        ],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "r3d",
        extension: "r3d",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"RED1", 4)),
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "braw",
        extension: "braw",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypBRAW", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 4 * GB,
    },
    // =========================================================================
    // Audio formats (additional)
    // =========================================================================
    SignatureDefinition {
        label: "m4a",
        extension: "m4a",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypM4A", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "wma",
        extension: "wma",
        header: &[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 24,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "ape",
        extension: "ape",
        header: b"MAC ",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "wv",
        extension: "wv",
        header: b"wvpk",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "alac",
        extension: "m4a",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypalac", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "ac3",
        extension: "ac3",
        header: &[0x0B, 0x77],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 7,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "dts",
        extension: "dts",
        header: &[0x7F, 0xFE, 0x80, 0x01],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "amr",
        extension: "amr",
        header: b"#!AMR\n",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "opus",
        extension: "opus",
        header: b"OggS",
        sub_header: Some((b"OpusHead", 28)),
        footer: FooterStrategy::MaxScan,
        min_size: 28,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "spx",
        extension: "spx",
        header: b"OggS",
        sub_header: Some((b"Speex   ", 28)),
        footer: FooterStrategy::MaxScan,
        min_size: 28,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "mpc",
        extension: "mpc",
        header: b"MPCK",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "mpc-sv7",
        extension: "mpc",
        header: b"MP+\x07",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "tta",
        extension: "tta",
        header: b"TTA1",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "dsf",
        extension: "dsf",
        header: b"DSD ",
        sub_header: None,
        footer: FooterStrategy::SizeFieldLe {
            offset_from_header: 12,
            field_size: 8,
        },
        min_size: 28,
        max_size: GB,
    },
    SignatureDefinition {
        label: "dff",
        extension: "dff",
        header: b"FRM8",
        sub_header: Some((b"DSD ", 12)),
        footer: FooterStrategy::SizeFieldBe {
            offset_from_header: 4,
            field_size: 8,
        },
        min_size: 16,
        max_size: GB,
    },
    SignatureDefinition {
        label: "au",
        extension: "au",
        header: &[0x2E, 0x73, 0x6E, 0x64],
        sub_header: None,
        footer: FooterStrategy::SizeFieldBe {
            offset_from_header: 8,
            field_size: 4,
        },
        min_size: 24,
        max_size: 512 * MB,
    },
    // =========================================================================
    // Document/Office formats (additional)
    // =========================================================================
    SignatureDefinition {
        label: "pages",
        extension: "pages",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "numbers",
        extension: "numbers",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "keynote",
        extension: "keynote",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "indd",
        extension: "indd",
        header: &[
            0x06, 0x06, 0xED, 0xF5, 0xD8, 0x1D, 0x46, 0xE5, 0xBD, 0x31, 0xEF, 0xE7, 0xFE, 0x74,
            0xB7, 0x1D,
        ],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 4096,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "pub",
        extension: "pub",
        header: &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
        sub_header: None,
        footer: FooterStrategy::OleCompound,
        min_size: 512,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "vsd",
        extension: "vsd",
        header: &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
        sub_header: None,
        footer: FooterStrategy::OleCompound,
        min_size: 512,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "xps",
        extension: "xps",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "epub",
        extension: "epub",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "mobi",
        extension: "mobi",
        header: b"BOOKMOBI",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "azw3",
        extension: "azw3",
        header: b"BOOKMOBI",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "fb2-zip",
        extension: "fb2.zip",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "djvu",
        extension: "djvu",
        header: b"AT&TFORM",
        sub_header: Some((b"DJVU", 12)),
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "djvm",
        extension: "djvu",
        header: b"AT&TFORM",
        sub_header: Some((b"DJVM", 12)),
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "chm",
        extension: "chm",
        header: b"ITSF\x03\x00\x00\x00",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 56,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "lit",
        extension: "lit",
        header: b"ITOLITLS",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "hlp",
        extension: "hlp",
        header: &[0x3F, 0x5F, 0x03, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "tex",
        extension: "tex",
        header: b"\\documentclass",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"\\end{document}"),
        min_size: 32,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "lyx",
        extension: "lyx",
        header: b"#LyX ",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "mbox",
        extension: "mbox",
        header: b"From ",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 2 * GB,
    },
    // =========================================================================
    // Spreadsheet/Data formats
    // =========================================================================
    SignatureDefinition {
        label: "parquet",
        extension: "parquet",
        header: b"PAR1",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"PAR1"),
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "avro",
        extension: "avro",
        header: b"Obj\x01",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "orc",
        extension: "orc",
        header: b"ORC",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "arrow",
        extension: "arrow",
        header: b"ARROW1",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"ARROW1"),
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "hdf5",
        extension: "hdf5",
        header: &[0x89, 0x48, 0x44, 0x46, 0x0D, 0x0A, 0x1A, 0x0A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 96,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "fits",
        extension: "fits",
        header: b"SIMPLE  =",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 2880,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "netcdf",
        extension: "nc",
        header: &[0x89, 0x48, 0x44, 0x46, 0x0D, 0x0A, 0x1A, 0x0A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 96,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "netcdf-classic",
        extension: "nc",
        header: b"CDF\x01",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "netcdf-64bit",
        extension: "nc",
        header: b"CDF\x02",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "mat5",
        extension: "mat",
        header: b"MATLAB 5.0 MAT-file",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 128,
        max_size: 4 * GB,
    },
    // =========================================================================
    // Database formats
    // =========================================================================
    SignatureDefinition {
        label: "mdb",
        extension: "mdb",
        header: &[
            0x00, 0x01, 0x00, 0x00, 0x53, 0x74, 0x61, 0x6E, 0x64, 0x61, 0x72, 0x64, 0x20, 0x4A,
            0x65, 0x74,
        ],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 512,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "accdb",
        extension: "accdb",
        header: &[
            0x00, 0x01, 0x00, 0x00, 0x53, 0x74, 0x61, 0x6E, 0x64, 0x61, 0x72, 0x64, 0x20, 0x41,
            0x43, 0x45,
        ],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 512,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "dbf",
        extension: "dbf",
        header: &[0x03],
        sub_header: None,
        footer: FooterStrategy::Fixed(&[0x1A]),
        min_size: 32,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "fdb",
        extension: "fdb",
        header: &[0x01, 0x00, 0x39, 0x30],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 1024,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "ibd",
        extension: "ibd",
        header: &[0x97, 0xE4, 0xDB, 0x31],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16384,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "lmdb",
        extension: "lmdb",
        header: &[0xDE, 0xC0, 0xEF, 0xBE],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 4096,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "rdb",
        extension: "rdb",
        header: b"REDIS",
        sub_header: None,
        footer: FooterStrategy::Fixed(&[0xFF]),
        min_size: 16,
        max_size: 4 * GB,
    },
    // cdb removed: header [0x00,0x00,0x00,0x00] causes false positives
    SignatureDefinition {
        label: "bdb",
        extension: "bdb",
        header: &[0x00, 0x06, 0x15, 0x61],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 512,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "bdb-hash",
        extension: "bdb",
        header: &[0x61, 0x15, 0x06, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 512,
        max_size: 4 * GB,
    },
    // =========================================================================
    // Compressed/Archive (additional)
    // =========================================================================
    SignatureDefinition {
        label: "lz4-frame",
        extension: "lz4",
        header: &[0x04, 0x22, 0x4D, 0x18],
        sub_header: None,
        footer: FooterStrategy::Fixed(&[0x00, 0x00, 0x00, 0x00]),
        min_size: 7,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "lzo",
        extension: "lzo",
        header: &[0x89, 0x4C, 0x5A, 0x4F, 0x00, 0x0D, 0x0A, 0x1A, 0x0A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 25,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "cab",
        extension: "cab",
        header: b"MSCF\x00\x00\x00\x00",
        sub_header: None,
        footer: FooterStrategy::SizeFieldLe {
            offset_from_header: 8,
            field_size: 4,
        },
        min_size: 36,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "msi",
        extension: "msi",
        header: &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
        sub_header: None,
        footer: FooterStrategy::OleCompound,
        min_size: 512,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "dmg",
        extension: "dmg",
        header: b"koly",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 512,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "iso9660",
        extension: "iso",
        header: b"CD001",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32769,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "cpio-bin",
        extension: "cpio",
        header: &[0xC7, 0x71],
        sub_header: None,
        footer: FooterStrategy::Fixed(b"TRAILER!!!"),
        min_size: 26,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "cpio-newc",
        extension: "cpio",
        header: b"070701",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"TRAILER!!!"),
        min_size: 110,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "cpio-crc",
        extension: "cpio",
        header: b"070702",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"TRAILER!!!"),
        min_size: 110,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "ar",
        extension: "a",
        header: b"!<arch>\n",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "deb",
        extension: "deb",
        header: b"!<arch>\ndebian-binary",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "rpm",
        extension: "rpm",
        header: &[0xED, 0xAB, 0xEE, 0xDB],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 96,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "apk",
        extension: "apk",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "ipa",
        extension: "ipa",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "nsis",
        extension: "exe",
        header: &[
            0xEF, 0xBE, 0xAD, 0xDE, 0x4E, 0x75, 0x6C, 0x6C, 0x73, 0x6F, 0x66, 0x74, 0x49, 0x6E,
            0x73, 0x74,
        ],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "innosetup",
        extension: "exe",
        header: b"Inno Setup Setup Data",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 512 * MB,
    },
    // =========================================================================
    // Font formats
    // =========================================================================
    SignatureDefinition {
        label: "ttf",
        extension: "ttf",
        header: &[0x00, 0x01, 0x00, 0x00, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 32 * MB,
    },
    SignatureDefinition {
        label: "otf",
        extension: "otf",
        header: b"OTTO",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 32 * MB,
    },
    SignatureDefinition {
        label: "woff",
        extension: "woff",
        header: b"wOFF",
        sub_header: None,
        footer: FooterStrategy::SizeFieldBe {
            offset_from_header: 4,
            field_size: 4,
        },
        min_size: 44,
        max_size: 32 * MB,
    },
    SignatureDefinition {
        label: "woff2",
        extension: "woff2",
        header: b"wOF2",
        sub_header: None,
        footer: FooterStrategy::SizeFieldBe {
            offset_from_header: 4,
            field_size: 4,
        },
        min_size: 48,
        max_size: 32 * MB,
    },
    SignatureDefinition {
        label: "eot",
        extension: "eot",
        header: &[0x00, 0x00],
        sub_header: Some((b"\x4C\x50", 8)),
        footer: FooterStrategy::SizeFieldLe {
            offset_from_header: 0,
            field_size: 4,
        },
        min_size: 64,
        max_size: 32 * MB,
    },
    SignatureDefinition {
        label: "ttc",
        extension: "ttc",
        header: b"ttcf",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "pfb",
        extension: "pfb",
        header: &[0x80, 0x01],
        sub_header: None,
        footer: FooterStrategy::Fixed(&[0x80, 0x03]),
        min_size: 6,
        max_size: 16 * MB,
    },
    SignatureDefinition {
        label: "afm",
        extension: "afm",
        header: b"StartFontMetrics",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"EndFontMetrics"),
        min_size: 32,
        max_size: 4 * MB,
    },
    // =========================================================================
    // Executable/Binary (additional)
    // =========================================================================
    SignatureDefinition {
        label: "dex",
        extension: "dex",
        header: b"dex\n",
        sub_header: None,
        footer: FooterStrategy::SizeFieldLe {
            offset_from_header: 32,
            field_size: 4,
        },
        min_size: 112,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "class",
        extension: "class",
        header: &[0xCA, 0xFE, 0xBA, 0xBE],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 26,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "pyc-311",
        extension: "pyc",
        header: &[0xA7, 0x0D, 0x0D, 0x0A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "pyc-310",
        extension: "pyc",
        header: &[0x6F, 0x0D, 0x0D, 0x0A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "pyc-39",
        extension: "pyc",
        header: &[0x61, 0x0D, 0x0D, 0x0A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "pyc-38",
        extension: "pyc",
        header: &[0x55, 0x0D, 0x0D, 0x0A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "pyc-312",
        extension: "pyc",
        header: &[0xCB, 0x0D, 0x0D, 0x0A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "llvm-bc",
        extension: "bc",
        header: b"BC\xC0\xDE",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "macho-fat",
        extension: "macho",
        header: &[0xCA, 0xFE, 0xBA, 0xBE],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 28,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "macho-fat-64",
        extension: "macho",
        header: &[0xCA, 0xFE, 0xBA, 0xBF],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 28,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "coff",
        extension: "obj",
        header: &[0x4C, 0x01],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 20,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "coff-amd64",
        extension: "obj",
        header: &[0x64, 0x86],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 20,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "ne",
        extension: "exe",
        header: &[0x4D, 0x5A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "xbe",
        extension: "xbe",
        header: b"XBEH",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 376,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "pbp",
        extension: "pbp",
        header: &[0x00, 0x50, 0x42, 0x50],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 40,
        max_size: 512 * MB,
    },
    // =========================================================================
    // Disk/Forensic images
    // =========================================================================
    SignatureDefinition {
        label: "e01",
        extension: "e01",
        header: b"EVF\x09\x0D\x0A\xFF\x00",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "e01-v2",
        extension: "ex01",
        header: b"EVF2\x0D\x0A\x81",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "aff",
        extension: "aff",
        header: b"AFF10\x00",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "aff4",
        extension: "aff4",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "vdi",
        extension: "vdi",
        header: &[0x7F, 0x10, 0xDA, 0xBE],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 512,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "qcow2",
        extension: "qcow2",
        header: b"QFI\xFB",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 72,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "vmdk-sparse",
        extension: "vmdk",
        header: b"KDMV",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 512,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "vmdk-descriptor",
        extension: "vmdk",
        header: b"# Disk Descriptor",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "vhd",
        extension: "vhd",
        header: b"conectix",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 512,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "vhdx",
        extension: "vhdx",
        header: b"vhdxfile",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 1024,
        max_size: 4 * GB,
    },
    // =========================================================================
    // Encryption/Security
    // =========================================================================
    SignatureDefinition {
        label: "pgp-pubkey",
        extension: "pgp",
        header: &[0x99],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "pgp-encrypted",
        extension: "pgp",
        header: &[0x85],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "pgp-signed",
        extension: "pgp",
        header: &[0xA3],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "gpg-symmetric",
        extension: "gpg",
        header: &[0x8C],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "kdbx",
        extension: "kdbx",
        header: &[0x03, 0xD9, 0xA2, 0x9A, 0x67, 0xFB, 0x4B, 0xB5],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "kdbx4",
        extension: "kdbx",
        header: &[
            0x03, 0xD9, 0xA2, 0x9A, 0x67, 0xFB, 0x4B, 0xB5, 0x01, 0x00, 0x04, 0x00,
        ],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "ssh-privkey",
        extension: "pem",
        header: b"-----BEGIN OPENSSH PRIVATE KEY-----",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"-----END OPENSSH PRIVATE KEY-----"),
        min_size: 64,
        max_size: 16 * MB,
    },
    SignatureDefinition {
        label: "x509-der",
        extension: "der",
        header: &[0x30, 0x82],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 16 * MB,
    },
    // =========================================================================
    // Scientific/Engineering
    // =========================================================================
    SignatureDefinition {
        label: "dicom",
        extension: "dcm",
        header: &[0x44, 0x49, 0x43, 0x4D],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 132,
        max_size: GB,
    },
    SignatureDefinition {
        label: "dicom-preamble",
        extension: "dcm",
        header: &[0x00, 0x00, 0x00, 0x00],
        sub_header: Some((&[0x44, 0x49, 0x43, 0x4D], 128)),
        footer: FooterStrategy::MaxScan,
        min_size: 132,
        max_size: GB,
    },
    SignatureDefinition {
        label: "nifti1",
        extension: "nii",
        header: &[0x5C, 0x01, 0x00, 0x00],
        sub_header: Some((b"n+1\x00", 344)),
        footer: FooterStrategy::MaxScan,
        min_size: 352,
        max_size: GB,
    },
    SignatureDefinition {
        label: "nifti2",
        extension: "nii",
        header: &[0xC0, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 540,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "minc2",
        extension: "mnc",
        header: &[0x89, 0x48, 0x44, 0x46, 0x0D, 0x0A, 0x1A, 0x0A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 96,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "shapefile",
        extension: "shp",
        header: &[0x00, 0x00, 0x27, 0x0A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 100,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "geotiff-le",
        extension: "tiff",
        header: &[0x49, 0x49, 0x2A, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "las-1.2",
        extension: "las",
        header: b"LASF",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 227,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "e57",
        extension: "e57",
        header: b"ASTM-E57",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 48,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "iges",
        extension: "iges",
        header: b"                                                                        S",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 80,
        max_size: 512 * MB,
    },
    // =========================================================================
    // Multimedia/Game/Image (additional)
    // =========================================================================
    SignatureDefinition {
        label: "xcf",
        extension: "xcf",
        header: b"gimp xcf ",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "bpg",
        extension: "bpg",
        header: &[0x42, 0x50, 0x47, 0xFB],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "avif",
        extension: "avif",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypavif", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "jxl-codestream",
        extension: "jxl",
        header: &[0xFF, 0x0A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "jxl-container",
        extension: "jxl",
        header: &[
            0x00, 0x00, 0x00, 0x0C, 0x4A, 0x58, 0x4C, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "flif",
        extension: "flif",
        header: b"FLIF",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "psb",
        extension: "psb",
        header: b"8BPS\x00\x02",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 30,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "skp",
        extension: "skp",
        header: &[
            0xFF, 0xFE, 0xFF, 0x0E, 0x53, 0x00, 0x6B, 0x00, 0x65, 0x00, 0x74, 0x00, 0x63, 0x00,
            0x68, 0x00, 0x55, 0x00, 0x70, 0x00,
        ],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "blend",
        extension: "blend",
        header: b"BLENDER",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"ENDB"),
        min_size: 12,
        max_size: 4 * GB,
    },
    // =========================================================================
    // System/Config formats
    // =========================================================================
    SignatureDefinition {
        label: "regf",
        extension: "reg",
        header: b"regf",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 4096,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "evtx",
        extension: "evtx",
        header: b"ElfFile\x00",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 4096,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "pcap",
        extension: "pcap",
        header: &[0xD4, 0xC3, 0xB2, 0xA1],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 24,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "pcap-be",
        extension: "pcap",
        header: &[0xA1, 0xB2, 0xC3, 0xD4],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 24,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "pcapng",
        extension: "pcapng",
        header: &[0x0A, 0x0D, 0x0D, 0x0A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 28,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "sqlite-wal",
        extension: "sqlite-wal",
        header: &[0x37, 0x7F, 0x06, 0x82],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "sqlite-wal-be",
        extension: "sqlite-wal",
        header: &[0x37, 0x7F, 0x06, 0x83],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "bson",
        extension: "bson",
        header: &[0x05, 0x00, 0x00, 0x00, 0x00],
        sub_header: None,
        footer: FooterStrategy::SizeFieldLe {
            offset_from_header: 0,
            field_size: 4,
        },
        min_size: 5,
        max_size: 16 * MB,
    },
    // =========================================================================
    // Additional Images/Formats
    // =========================================================================
    SignatureDefinition {
        label: "jp2",
        extension: "jp2",
        header: &[
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "jpx",
        extension: "jpx",
        header: &[
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "cur",
        extension: "cur",
        header: &[0x00, 0x00, 0x02, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 22,
        max_size: 4 * MB,
    },
    SignatureDefinition {
        label: "tga",
        extension: "tga",
        header: &[0x00, 0x00, 0x02],
        sub_header: None,
        footer: FooterStrategy::Fixed(b"TRUEVISION-XFILE.\x00"),
        min_size: 18,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "pcx",
        extension: "pcx",
        header: &[0x0A, 0x05, 0x01, 0x08],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 128,
        max_size: 32 * MB,
    },
    SignatureDefinition {
        label: "dpx",
        extension: "dpx",
        header: b"SDPX",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 2048,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "exr",
        extension: "exr",
        header: &[0x76, 0x2F, 0x31, 0x01],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: GB,
    },
    SignatureDefinition {
        label: "hdr",
        extension: "hdr",
        header: b"#?RADIANCE\n",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 256 * MB,
    },
    // =========================================================================
    // Additional Archives / Package formats
    // =========================================================================
    SignatureDefinition {
        label: "lzma",
        extension: "lzma",
        header: &[0x5D, 0x00, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 13,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "snappy-framing",
        extension: "sz",
        header: &[0xFF, 0x06, 0x00, 0x00, 0x73, 0x4E, 0x61, 0x50, 0x70, 0x59],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 10,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "brotli",
        extension: "br",
        header: &[0xCE, 0xB2, 0xCF, 0x81],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 4,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "sit",
        extension: "sit",
        header: b"StuffIt",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "sitx",
        extension: "sitx",
        header: b"StuffIt!",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "zoo",
        extension: "zoo",
        header: &[0xFD, 0xC4, 0xA7, 0xDC],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "arj",
        extension: "arj",
        header: &[0x60, 0xEA],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 30,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "ace",
        extension: "ace",
        header: b"**ACE**",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 50,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "lha",
        extension: "lha",
        header: &[0x2D, 0x6C, 0x68],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 21,
        max_size: 2 * GB,
    },
    // =========================================================================
    // Additional Document/Office
    // =========================================================================
    SignatureDefinition {
        label: "docx",
        extension: "docx",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "xlsx",
        extension: "xlsx",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "pptx",
        extension: "pptx",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "odt",
        extension: "odt",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "ods",
        extension: "ods",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "odp",
        extension: "odp",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "xls",
        extension: "xls",
        header: &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
        sub_header: None,
        footer: FooterStrategy::OleCompound,
        min_size: 512,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "ppt",
        extension: "ppt",
        header: &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
        sub_header: None,
        footer: FooterStrategy::OleCompound,
        min_size: 512,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "msg",
        extension: "msg",
        header: &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
        sub_header: None,
        footer: FooterStrategy::OleCompound,
        min_size: 512,
        max_size: 256 * MB,
    },
    // =========================================================================
    // Additional Audio
    // =========================================================================
    SignatureDefinition {
        label: "wv-correction",
        extension: "wvc",
        header: b"wvpk",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "aac-adts2",
        extension: "aac",
        header: &[0xFF, 0xF9],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 7,
        max_size: 128 * MB,
    },
    SignatureDefinition {
        label: "mp3-sync3",
        extension: "mp3",
        header: &[0xFF, 0xF3],
        sub_header: None,
        footer: FooterStrategy::Id3Mp3,
        min_size: 128,
        max_size: 128 * MB,
    },
    SignatureDefinition {
        label: "mp3-sync4",
        extension: "mp3",
        header: &[0xFF, 0xF2],
        sub_header: None,
        footer: FooterStrategy::Id3Mp3,
        min_size: 128,
        max_size: 128 * MB,
    },
    SignatureDefinition {
        label: "amr-wb",
        extension: "awb",
        header: b"#!AMR-WB\n",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 10,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "caf",
        extension: "caf",
        header: b"caff\x00\x01",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "aiff-c",
        extension: "aifc",
        header: b"FORM",
        sub_header: Some((b"AIFC", 8)),
        footer: FooterStrategy::RiffChunk,
        min_size: 12,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "ra",
        extension: "ra",
        header: &[0x2E, 0x72, 0x61, 0xFD],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "s3m",
        extension: "s3m",
        header: b"SCRM",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 16 * MB,
    },
    SignatureDefinition {
        label: "it",
        extension: "it",
        header: b"IMPM",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "xm",
        extension: "xm",
        header: b"Extended Module:",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 60,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "mod",
        extension: "mod",
        header: b"M.K.",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 1084,
        max_size: 16 * MB,
    },
    // =========================================================================
    // Additional Video/Multimedia
    // =========================================================================
    SignatureDefinition {
        label: "mov",
        extension: "mov",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypqt", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "f4v",
        extension: "f4v",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypf4v", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "mp4-isom",
        extension: "mp4",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypisom", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "mp4-mp41",
        extension: "mp4",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypmp41", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "mp4-mp42",
        extension: "mp4",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypmp42", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "mp4-dash",
        extension: "mp4",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypdash", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "mp4-m4a",
        extension: "mp4",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypM4A ", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "3gp-3gp4",
        extension: "3gp",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftyp3gp4", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "3gp-3gp5",
        extension: "3gp",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftyp3gp5", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "3gp-3gp6",
        extension: "3gp",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftyp3gp6", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "3g2",
        extension: "3g2",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftyp3g2a", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "heif",
        extension: "heif",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypheic", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "heif-mif1",
        extension: "heif",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypmif1", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "heif-msf1",
        extension: "heif",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypmsf1", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "heif-heix",
        extension: "heif",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"ftypheix", 4)),
        footer: FooterStrategy::ContainerAtom,
        min_size: 12,
        max_size: 256 * MB,
    },
    // =========================================================================
    // Additional Executables/Bytecode
    // =========================================================================
    SignatureDefinition {
        label: "dex-035",
        extension: "dex",
        header: b"dex\n035\x00",
        sub_header: None,
        footer: FooterStrategy::SizeFieldLe {
            offset_from_header: 32,
            field_size: 4,
        },
        min_size: 112,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "dex-037",
        extension: "dex",
        header: b"dex\n037\x00",
        sub_header: None,
        footer: FooterStrategy::SizeFieldLe {
            offset_from_header: 32,
            field_size: 4,
        },
        min_size: 112,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "dex-038",
        extension: "dex",
        header: b"dex\n038\x00",
        sub_header: None,
        footer: FooterStrategy::SizeFieldLe {
            offset_from_header: 32,
            field_size: 4,
        },
        min_size: 112,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "dex-039",
        extension: "dex",
        header: b"dex\n039\x00",
        sub_header: None,
        footer: FooterStrategy::SizeFieldLe {
            offset_from_header: 32,
            field_size: 4,
        },
        min_size: 112,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "luac",
        extension: "luac",
        header: &[0x1B, 0x4C, 0x75, 0x61],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "macho32-le",
        extension: "macho",
        header: &[0xCE, 0xFA, 0xED, 0xFE],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 28,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "macho64-le",
        extension: "macho",
        header: &[0xCF, 0xFA, 0xED, 0xFE],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 512 * MB,
    },
    // =========================================================================
    // Additional Disk/VM images
    // =========================================================================
    SignatureDefinition {
        label: "cramfs",
        extension: "cramfs",
        header: &[0x45, 0x3D, 0xCD, 0x28],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 76,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "squashfs",
        extension: "squashfs",
        header: b"hsqs",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 96,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "squashfs-be",
        extension: "squashfs",
        header: b"sqsh",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 96,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "ext-superblock",
        extension: "ext",
        header: &[0x53, 0xEF],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 1024,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "jffs2-le",
        extension: "jffs2",
        header: &[0x85, 0x19],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "jffs2-be",
        extension: "jffs2",
        header: &[0x19, 0x85],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "ubifs",
        extension: "ubifs",
        header: &[0x31, 0x18, 0x10, 0x06],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    // =========================================================================
    // Additional Forensic/Security
    // =========================================================================
    SignatureDefinition {
        label: "pem-cert",
        extension: "pem",
        header: b"-----BEGIN CERTIFICATE-----",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"-----END CERTIFICATE-----"),
        min_size: 64,
        max_size: 16 * MB,
    },
    SignatureDefinition {
        label: "pem-rsa",
        extension: "pem",
        header: b"-----BEGIN RSA PRIVATE KEY-----",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"-----END RSA PRIVATE KEY-----"),
        min_size: 64,
        max_size: 16 * MB,
    },
    SignatureDefinition {
        label: "pem-ec",
        extension: "pem",
        header: b"-----BEGIN EC PRIVATE KEY-----",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"-----END EC PRIVATE KEY-----"),
        min_size: 64,
        max_size: 16 * MB,
    },
    SignatureDefinition {
        label: "pem-pkcs8",
        extension: "pem",
        header: b"-----BEGIN PRIVATE KEY-----",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"-----END PRIVATE KEY-----"),
        min_size: 64,
        max_size: 16 * MB,
    },
    SignatureDefinition {
        label: "pem-encrypted",
        extension: "pem",
        header: b"-----BEGIN ENCRYPTED PRIVATE KEY-----",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"-----END ENCRYPTED PRIVATE KEY-----"),
        min_size: 64,
        max_size: 16 * MB,
    },
    SignatureDefinition {
        label: "pgp-armored",
        extension: "asc",
        header: b"-----BEGIN PGP",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"-----END PGP"),
        min_size: 64,
        max_size: 64 * MB,
    },
    // =========================================================================
    // Additional Data / Config
    // =========================================================================
    SignatureDefinition {
        label: "yaml",
        extension: "yaml",
        header: b"---\n",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 4,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "toml",
        extension: "toml",
        header: b"[",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 3,
        max_size: 16 * MB,
    },
    SignatureDefinition {
        label: "msgpack-fixmap",
        extension: "msgpack",
        header: &[0x80],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 1,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "protobuf-varint",
        extension: "pb",
        header: &[0x08],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 2,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "asn1-sequence",
        extension: "asn1",
        header: &[0x30, 0x80],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 4,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "cbor-map",
        extension: "cbor",
        header: &[0xBF],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 2,
        max_size: 64 * MB,
    },
    // =========================================================================
    // Additional CAD/3D/Modeling
    // =========================================================================
    SignatureDefinition {
        label: "obj-wavefront",
        extension: "obj",
        header: b"# ",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "3ds",
        extension: "3ds",
        header: &[0x4D, 0x4D, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "fbx-binary",
        extension: "fbx",
        header: b"Kaydara FBX Binary  \x00",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 27,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "gltf-binary",
        extension: "glb",
        header: b"glTF",
        sub_header: None,
        footer: FooterStrategy::SizeFieldLe {
            offset_from_header: 8,
            field_size: 4,
        },
        min_size: 12,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "usdz",
        extension: "usdz",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "abc",
        extension: "abc",
        header: &[0x00, 0x00, 0x00, 0x01, 0x4F, 0x67, 0x61, 0x77, 0x61],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "ifc",
        extension: "ifc",
        header: b"ISO-10303-21;",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"END-ISO-10303-21;"),
        min_size: 32,
        max_size: GB,
    },
    SignatureDefinition {
        label: "dwg-2018",
        extension: "dwg",
        header: b"AC1032",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "dwg-2013",
        extension: "dwg",
        header: b"AC1027",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "dwg-2010",
        extension: "dwg",
        header: b"AC1024",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "dwg-2007",
        extension: "dwg",
        header: b"AC1021",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "dwg-2004",
        extension: "dwg",
        header: b"AC1018",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "dwg-2000",
        extension: "dwg",
        header: b"AC1015",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    // =========================================================================
    // Additional Compressed/Encrypted
    // =========================================================================
    SignatureDefinition {
        label: "zpaq",
        extension: "zpaq",
        header: b"zPQ",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "zlib",
        extension: "zlib",
        header: &[0x78, 0x9C],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "zlib-best",
        extension: "zlib",
        header: &[0x78, 0xDA],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "zlib-low",
        extension: "zlib",
        header: &[0x78, 0x01],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "lz4-legacy",
        extension: "lz4",
        header: &[0x02, 0x21, 0x4C, 0x18],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 7,
        max_size: 4 * GB,
    },
    // =========================================================================
    // Additional Multimedia
    // =========================================================================
    SignatureDefinition {
        label: "swf-zlib",
        extension: "swf",
        header: b"CWS",
        sub_header: None,
        footer: FooterStrategy::SizeFieldLe {
            offset_from_header: 4,
            field_size: 4,
        },
        min_size: 8,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "ani",
        extension: "ani",
        header: b"RIFF",
        sub_header: Some((b"ACON", 8)),
        footer: FooterStrategy::RiffChunk,
        min_size: 12,
        max_size: 4 * MB,
    },
    SignatureDefinition {
        label: "rmf",
        extension: "rm",
        header: &[0x2E, 0x52, 0x4D, 0x46, 0x00, 0x00, 0x00, 0x12],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 18,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "ivf",
        extension: "ivf",
        header: b"DKIF",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 4 * GB,
    },
    // =========================================================================
    // Additional Database / Index
    // =========================================================================
    SignatureDefinition {
        label: "sqlite-journal",
        extension: "sqlite-journal",
        header: &[0xD9, 0xD5, 0x05, 0xF9, 0x20, 0xA1, 0x63, 0xD7],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 512,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "tdb",
        extension: "tdb",
        header: b"TDB file\n",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "gdbm",
        extension: "gdbm",
        header: &[0x13, 0x57, 0x9A, 0xCE],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "gdbm-be",
        extension: "gdbm",
        header: &[0xCE, 0x9A, 0x57, 0x13],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    // =========================================================================
    // Additional Fonts / Typography
    // =========================================================================
    SignatureDefinition {
        label: "type1-ascii",
        extension: "pfa",
        header: b"%!PS-AdobeFont",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 4 * MB,
    },
    SignatureDefinition {
        label: "bdf",
        extension: "bdf",
        header: b"STARTFONT",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"ENDFONT"),
        min_size: 32,
        max_size: 16 * MB,
    },
    SignatureDefinition {
        label: "pcf",
        extension: "pcf",
        header: &[0x01, 0x66, 0x63, 0x70],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 16 * MB,
    },
    // =========================================================================
    // Additional Scientific / Geospatial
    // =========================================================================
    SignatureDefinition {
        label: "grib2",
        extension: "grib2",
        header: b"GRIB",
        sub_header: Some((&[0x02], 7)),
        footer: FooterStrategy::Fixed(b"7777"),
        min_size: 16,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "grib1",
        extension: "grib",
        header: b"GRIB",
        sub_header: Some((&[0x01], 7)),
        footer: FooterStrategy::Fixed(b"7777"),
        min_size: 16,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "bufr",
        extension: "bufr",
        header: b"BUFR",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"7777"),
        min_size: 8,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "dbf-foxpro",
        extension: "dbf",
        header: &[0x30],
        sub_header: None,
        footer: FooterStrategy::Fixed(&[0x1A]),
        min_size: 32,
        max_size: 2 * GB,
    },
    SignatureDefinition {
        label: "shapefile-index",
        extension: "shx",
        header: &[0x00, 0x00, 0x27, 0x0A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 100,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "geojson",
        extension: "geojson",
        header: b"{\"type\":",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "kml",
        extension: "kml",
        header: b"<?xml",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"</kml>"),
        min_size: 32,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "kmz",
        extension: "kmz",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "gpx",
        extension: "gpx",
        header: b"<?xml",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"</gpx>"),
        min_size: 32,
        max_size: 256 * MB,
    },
    // =========================================================================
    // Additional System / Firmware / Embedded
    // =========================================================================
    SignatureDefinition {
        label: "uefi-capsule",
        extension: "cap",
        header: &[0xB9, 0x82, 0x2F, 0x51, 0x36, 0x23, 0x4E, 0xD2, 0xA2, 0x03],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 28,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "dtb",
        extension: "dtb",
        header: &[0xD0, 0x0D, 0xFE, 0xED],
        sub_header: None,
        footer: FooterStrategy::SizeFieldBe {
            offset_from_header: 4,
            field_size: 4,
        },
        min_size: 48,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "uimage",
        extension: "uimage",
        header: &[0x27, 0x05, 0x19, 0x56],
        sub_header: None,
        footer: FooterStrategy::SizeFieldBe {
            offset_from_header: 12,
            field_size: 4,
        },
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "android-boot",
        extension: "img",
        header: b"ANDROID!",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "android-sparse",
        extension: "simg",
        header: &[0x3A, 0xFF, 0x26, 0xED],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 28,
        max_size: 4 * GB,
    },
    // =========================================================================
    // Additional Misc formats
    // =========================================================================
    SignatureDefinition {
        label: "torrent",
        extension: "torrent",
        header: b"d8:announce",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 16 * MB,
    },
    SignatureDefinition {
        label: "ics-calendar",
        extension: "ics",
        header: b"BEGIN:VCALENDAR",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"END:VCALENDAR"),
        min_size: 32,
        max_size: 16 * MB,
    },
    SignatureDefinition {
        label: "vcf-vcard",
        extension: "vcf",
        header: b"BEGIN:VCARD",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"END:VCARD"),
        min_size: 32,
        max_size: 16 * MB,
    },
    SignatureDefinition {
        label: "mht",
        extension: "mht",
        header: b"MIME-Version:",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "warc",
        extension: "warc",
        header: b"WARC/1.0\r\n",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "fits-standard",
        extension: "fits",
        header: b"SIMPLE  = ",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 2880,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "wad",
        extension: "wad",
        header: b"IWAD",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "wad-pwad",
        extension: "wad",
        header: b"PWAD",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "pak-quake",
        extension: "pak",
        header: b"PACK",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "unity-assets",
        extension: "assets",
        header: &[0x00, 0x00, 0x00],
        sub_header: Some((b"Unity", 20)),
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "unreal-pak",
        extension: "pak",
        header: &[0xE1, 0x12, 0x6F, 0x5A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "jar",
        extension: "jar",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: GB,
    },
    SignatureDefinition {
        label: "war",
        extension: "war",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: GB,
    },
    SignatureDefinition {
        label: "ear",
        extension: "ear",
        header: &[b'P', b'K', 0x03, 0x04],
        sub_header: None,
        footer: FooterStrategy::ZipEocd,
        min_size: 30,
        max_size: GB,
    },
    SignatureDefinition {
        label: "crx",
        extension: "crx",
        header: b"Cr24",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "xar",
        extension: "xar",
        header: b"xar!",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 28,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "macho-dylib",
        extension: "dylib",
        header: &[0xCF, 0xFA, 0xED, 0xFE],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "pe-dll",
        extension: "dll",
        header: &[0x4D, 0x5A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 512 * MB,
    },
    SignatureDefinition {
        label: "pe-sys",
        extension: "sys",
        header: &[0x4D, 0x5A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "plist-binary",
        extension: "plist",
        header: b"bplist00",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "plist-xml",
        extension: "plist",
        header: b"<?xml",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"</plist>"),
        min_size: 16,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "leveldb-log",
        extension: "log",
        header: &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "leveldb-table",
        extension: "ldb",
        header: &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        sub_header: Some((&[0x57, 0xFB, 0x80, 0x8B, 0x24, 0x75, 0x47, 0xDB], 0)),
        footer: FooterStrategy::MaxScan,
        min_size: 48,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "parcel",
        extension: "parcel",
        header: b"PARCEL",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 64 * MB,
    },
    // Additional image formats
    SignatureDefinition {
        label: "jxr",
        extension: "jxr",
        header: &[0x49, 0x49, 0xBC, 0x01],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 16,
        max_size: 128 * MB,
    },
    // Additional video
    SignatureDefinition {
        label: "mxf2",
        extension: "mxf",
        header: &[0x06, 0x0E, 0x2B, 0x34, 0x02, 0x05, 0x01, 0x01],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    // Additional audio
    SignatureDefinition {
        label: "tak",
        extension: "tak",
        header: b"tBaK",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 512 * MB,
    },
    // Additional documents
    SignatureDefinition {
        label: "ps",
        extension: "ps",
        header: b"%!PS",
        sub_header: None,
        footer: FooterStrategy::Fixed(b"%%EOF"),
        min_size: 16,
        max_size: 128 * MB,
    },
    SignatureDefinition {
        label: "xps2",
        extension: "xps",
        header: b"PK",
        sub_header: Some((b"[Content_Types].xml", 30)),
        footer: FooterStrategy::ZipEocd,
        min_size: 128,
        max_size: 256 * MB,
    },
    // Additional system
    SignatureDefinition {
        label: "lnk",
        extension: "lnk",
        header: &[0x4C, 0x00, 0x00, 0x00, 0x01, 0x14, 0x02, 0x00],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 4 * MB,
    },
    SignatureDefinition {
        label: "pf",
        extension: "pf",
        header: &[0x11, 0x00, 0x00, 0x00, 0x53, 0x43, 0x43, 0x41],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 16 * MB,
    },
    SignatureDefinition {
        label: "thumbsdb",
        extension: "db",
        header: &[0xD0, 0xCF, 0x11, 0xE0],
        sub_header: Some((b"Thumbs", 0)),
        footer: FooterStrategy::OleCompound,
        min_size: 512,
        max_size: 64 * MB,
    },
    // Additional texture/image formats
    SignatureDefinition {
        label: "dds",
        extension: "dds",
        header: b"DDS ",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 128,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "ktx",
        extension: "ktx",
        header: &[
            0xAB, 0x4B, 0x54, 0x58, 0x20, 0x31, 0x31, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
        ],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "jxl",
        extension: "jxl",
        header: &[0xFF, 0x0A],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "jxl-container",
        extension: "jxl",
        header: &[
            0x00, 0x00, 0x00, 0x0C, 0x4A, 0x58, 0x4C, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ],
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 12,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "qoi",
        extension: "qoi",
        header: b"qoif",
        sub_header: None,
        footer: FooterStrategy::Fixed(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01]),
        min_size: 14,
        max_size: 256 * MB,
    },
    // Additional system/archive formats
    SignatureDefinition {
        label: "cpio",
        extension: "cpio",
        header: b"070707",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 76,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "cpio-new",
        extension: "cpio",
        header: b"070701",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 110,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "appimage",
        extension: "AppImage",
        header: &[0x7F, 0x45, 0x4C, 0x46],
        sub_header: Some((b"AI\x02", 8)),
        footer: FooterStrategy::MaxScan,
        min_size: 64,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "snap",
        extension: "snap",
        header: b"hsqs",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 96,
        max_size: 4 * GB,
    },
    SignatureDefinition {
        label: "romfs",
        extension: "romfs",
        header: b"-rom1fs-",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 32,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "pbm",
        extension: "pbm",
        header: b"P4\n",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 64 * MB,
    },
    SignatureDefinition {
        label: "pgm",
        extension: "pgm",
        header: b"P5\n",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 256 * MB,
    },
    SignatureDefinition {
        label: "ppm",
        extension: "ppm",
        header: b"P6\n",
        sub_header: None,
        footer: FooterStrategy::MaxScan,
        min_size: 8,
        max_size: 256 * MB,
    },
];

pub fn carve_signatures(image_path: &Path) -> Result<Vec<CarvedFileCandidate>, String> {
    let file_size = std::fs::metadata(image_path)
        .map_err(|e| {
            format!(
                "Unable to read image metadata for {}: {e}",
                image_path.to_string_lossy()
            )
        })?
        .len();

    let num_threads = num_cpus::get().max(1);
    let min_segment = CARVING_CHUNK_SIZE as u64 * 16;

    // For small images or single-core, use sequential scan
    if file_size < min_segment * 2 || num_threads <= 1 {
        return carve_segment(image_path, 0, file_size, file_size, 0);
    }

    let segment_size = (file_size / num_threads as u64).max(min_segment);
    let max_header_len = SIGNATURES
        .iter()
        .map(|s| s.header.len())
        .max()
        .unwrap_or(16) as u64;
    let overlap = max_header_len + CARVING_OVERLAP_SIZE as u64;

    let segments: Vec<(u64, u64, usize)> = (0..num_threads)
        .map(|i| {
            let raw_start = i as u64 * segment_size;
            let start = if i > 0 {
                raw_start.saturating_sub(overlap)
            } else {
                0
            };
            let end = ((i as u64 + 1) * segment_size).min(file_size);
            (start, end, i)
        })
        .filter(|(start, end, _)| end > start)
        .collect();

    let all_results: Result<Vec<Vec<CarvedFileCandidate>>, String> = segments
        .par_iter()
        .map(|(start, end, idx)| carve_segment(image_path, *start, *end, file_size, *idx * 10000))
        .collect();

    let mut all_candidates: Vec<CarvedFileCandidate> = all_results?.into_iter().flatten().collect();

    // Deduplicate candidates found in overlap zones (same start_offset = same file)
    let mut seen_offsets = HashSet::new();
    all_candidates.retain(|c| seen_offsets.insert(c.start_offset));
    all_candidates.sort_by_key(|c| c.start_offset);

    // Re-number ordinals sequentially
    for (i, candidate) in all_candidates.iter_mut().enumerate() {
        candidate.name = format!("carved-{:04}.{}", i + 1, candidate.extension);
    }

    Ok(all_candidates)
}

fn carve_segment(
    image_path: &Path,
    segment_start: u64,
    segment_end: u64,
    file_size: u64,
    ordinal_offset: usize,
) -> Result<Vec<CarvedFileCandidate>, String> {
    let mut reader = File::open(image_path).map_err(|e| {
        format!(
            "Unable to open image {} for carving segment: {e}",
            image_path.to_string_lossy()
        )
    })?;

    let mut carved = Vec::new();
    let mut next_scan_offset = segment_start;

    while next_scan_offset < segment_end {
        let search_window = read_window(&mut reader, next_scan_offset, CARVING_CHUNK_SIZE)?;
        if search_window.is_empty() {
            break;
        }

        // Skip TRIM-zeroed regions: large blocks of 0x00 aligned to 4096-byte boundaries
        // are almost certainly wiped by SSD TRIM and contain no recoverable signatures.
        if search_window.iter().all(|&b| b == 0) {
            next_scan_offset = next_scan_offset.saturating_add(search_window.len() as u64);
            continue;
        }

        // Detect TRIM-erased 4KB pages within the chunk
        // Count consecutive 4KB zero-aligned blocks
        let zero_pages = search_window
            .chunks(4096)
            .filter(|page| page.iter().all(|&b| b == 0))
            .count();
        let total_pages = search_window.len() / 4096;
        // If >80% of the chunk is TRIM-zeroed pages, skip it
        if total_pages > 0 && zero_pages * 100 / total_pages > 80 {
            next_scan_offset = next_scan_offset.saturating_add(search_window.len() as u64);
            continue;
        }

        if let Some((relative_offset, signature)) = find_next_signature(&search_window) {
            let absolute_offset = next_scan_offset + relative_offset as u64;
            let ordinal = ordinal_offset + carved.len() + 1;
            let candidate =
                carve_candidate(&mut reader, file_size, absolute_offset, signature, ordinal)?;

            let candidate_end = carving_candidate_end_offset(&candidate);
            next_scan_offset = candidate_end.max(absolute_offset.saturating_add(1));
            carved.push(candidate);
            continue;
        }

        let advance = search_window.len().saturating_sub(CARVING_OVERLAP_SIZE);
        if advance == 0 {
            break;
        }
        next_scan_offset = next_scan_offset.saturating_add(advance as u64);
    }

    Ok(carved)
}

fn find_next_signature(buffer: &[u8]) -> Option<(usize, SignatureDefinition)> {
    let mut earliest: Option<(usize, SignatureDefinition)> = None;

    for signature in SIGNATURES {
        if let Some(position) = find_bytes(buffer, signature.header) {
            // Check sub_header if present (e.g., RIFF must have AVI/WAVE/WEBP at offset 8)
            if let Some((sub_bytes, sub_offset)) = signature.sub_header {
                let abs = position + sub_offset;
                if abs + sub_bytes.len() > buffer.len() {
                    continue;
                }
                if &buffer[abs..abs + sub_bytes.len()] != sub_bytes {
                    continue;
                }
            }
            match earliest {
                Some((earliest_position, _)) if earliest_position <= position => {}
                _ => earliest = Some((position, *signature)),
            }
        }
    }

    earliest
}

fn carve_candidate(
    reader: &mut File,
    file_size: u64,
    start_offset: u64,
    signature: SignatureDefinition,
    ordinal: usize,
) -> Result<CarvedFileCandidate, String> {
    let mut byte_runs = Vec::new();
    let search_start = start_offset.saturating_add(signature.header.len() as u64);
    let search_limit = file_size.min(start_offset.saturating_add(signature.max_size));
    let search_length = search_limit.saturating_sub(search_start);
    let search_buffer = read_window(reader, search_start, search_length as usize)?;
    let mut validator_status = "partial-unvalidated".to_string();
    let mut gap_count = 0_u8;
    let mut assembly_segment_count = 1_u8;

    let (end_offset, mut integrity, mut recovery_score) = match signature.footer {
        FooterStrategy::Fixed(footer) => {
            if let Some(relative_footer) = find_bytes(&search_buffer, footer) {
                let score = crate::scoring::carving_recovery_score("intact", true, signature.label);
                (
                    search_start + relative_footer as u64 + footer.len() as u64,
                    "intact".to_string(),
                    score,
                )
            } else {
                let score =
                    crate::scoring::carving_recovery_score("partial", false, signature.label);
                (search_limit, "partial".to_string(), score)
            }
        }
        FooterStrategy::ZipEocd => {
            match find_zip_end_offset(search_start, &search_buffer, search_limit) {
                Some(end_offset) => {
                    let score = crate::scoring::carving_recovery_score("intact", true, "zip");
                    (end_offset, "intact".to_string(), score)
                }
                None => {
                    let score = crate::scoring::carving_recovery_score("partial", false, "zip");
                    (search_limit, "partial".to_string(), score)
                }
            }
        }
        FooterStrategy::RiffChunk => {
            // RIFF: total file size = LE u32 at bytes 4-7 of header + 8
            let header_buf = read_window(reader, start_offset, 12.min(search_length as usize))?;
            if header_buf.len() >= 8 {
                let chunk_size = u32::from_le_bytes([
                    header_buf[4],
                    header_buf[5],
                    header_buf[6],
                    header_buf[7],
                ]) as u64;
                let end = start_offset + chunk_size + 8;
                if end <= search_limit && chunk_size >= signature.min_size {
                    let score =
                        crate::scoring::carving_recovery_score("intact", true, signature.label);
                    (end, "intact".to_string(), score)
                } else {
                    let score =
                        crate::scoring::carving_recovery_score("partial", false, signature.label);
                    (search_limit, "partial".to_string(), score)
                }
            } else {
                let score =
                    crate::scoring::carving_recovery_score("partial", false, signature.label);
                (search_limit, "partial".to_string(), score)
            }
        }
        FooterStrategy::ContainerAtom => {
            // ISO BMFF: walk atom chain (4B size BE + 4B type), sum until no more valid atoms
            match resolve_container_atom_end(reader, start_offset, search_limit) {
                Some(end) => {
                    let score =
                        crate::scoring::carving_recovery_score("intact", true, signature.label);
                    (end, "intact".to_string(), score)
                }
                None => {
                    let score =
                        crate::scoring::carving_recovery_score("partial", false, signature.label);
                    (search_limit, "partial".to_string(), score)
                }
            }
        }
        FooterStrategy::EbmlElement => {
            // MKV/WebM: parse EBML header element size to estimate container length
            match resolve_ebml_end(reader, start_offset, search_limit) {
                Some(end) => {
                    let score =
                        crate::scoring::carving_recovery_score("intact", true, signature.label);
                    (end, "intact".to_string(), score)
                }
                None => {
                    let score =
                        crate::scoring::carving_recovery_score("partial", false, signature.label);
                    (search_limit, "partial".to_string(), score)
                }
            }
        }
        FooterStrategy::OleCompound => {
            // OLE2: read sector size from header (offset 30, LE u16 = power of 2), sector count from FAT
            let header_buf = read_window(reader, start_offset, 512.min(search_length as usize))?;
            if header_buf.len() >= 48 {
                let sector_power = u16::from_le_bytes([header_buf[30], header_buf[31]]);
                let sector_size = 1u64 << sector_power.min(16);
                let total_sectors = u32::from_le_bytes([
                    header_buf[44],
                    header_buf[45],
                    header_buf[46],
                    header_buf[47],
                ]) as u64;
                let end = start_offset + (total_sectors + 1) * sector_size;
                if end <= search_limit && end > start_offset + 512 {
                    let score =
                        crate::scoring::carving_recovery_score("intact", true, signature.label);
                    (end, "intact".to_string(), score)
                } else {
                    let score =
                        crate::scoring::carving_recovery_score("partial", false, signature.label);
                    (search_limit, "partial".to_string(), score)
                }
            } else {
                let score =
                    crate::scoring::carving_recovery_score("partial", false, signature.label);
                (search_limit, "partial".to_string(), score)
            }
        }
        FooterStrategy::MaxScan => {
            let score = crate::scoring::carving_recovery_score("partial", false, signature.label);
            (search_limit, "partial".to_string(), score)
        }
        FooterStrategy::SizeFieldLe {
            offset_from_header,
            field_size,
        } => {
            let header_buf = read_window(
                reader,
                start_offset,
                (offset_from_header + field_size as usize).min(search_length as usize),
            )?;
            let total_size = read_size_field(&header_buf, offset_from_header, field_size, false);
            if let Some(size) = total_size {
                let end = start_offset + size;
                if end <= search_limit && size >= signature.min_size {
                    let score =
                        crate::scoring::carving_recovery_score("intact", true, signature.label);
                    (end, "intact".to_string(), score)
                } else {
                    let score =
                        crate::scoring::carving_recovery_score("partial", false, signature.label);
                    (search_limit, "partial".to_string(), score)
                }
            } else {
                let score =
                    crate::scoring::carving_recovery_score("partial", false, signature.label);
                (search_limit, "partial".to_string(), score)
            }
        }
        FooterStrategy::SizeFieldBe {
            offset_from_header,
            field_size,
        } => {
            let header_buf = read_window(
                reader,
                start_offset,
                (offset_from_header + field_size as usize).min(search_length as usize),
            )?;
            let total_size = read_size_field(&header_buf, offset_from_header, field_size, true);
            if let Some(size) = total_size {
                let end = start_offset + size;
                if end <= search_limit && size >= signature.min_size {
                    let score =
                        crate::scoring::carving_recovery_score("intact", true, signature.label);
                    (end, "intact".to_string(), score)
                } else {
                    let score =
                        crate::scoring::carving_recovery_score("partial", false, signature.label);
                    (search_limit, "partial".to_string(), score)
                }
            } else {
                let score =
                    crate::scoring::carving_recovery_score("partial", false, signature.label);
                (search_limit, "partial".to_string(), score)
            }
        }
        FooterStrategy::Id3Mp3 => {
            // MP3: parse ID3 tag size if present, then scan for end of MP3 frames
            match resolve_mp3_end(reader, start_offset, search_limit) {
                Some(end) => {
                    let score =
                        crate::scoring::carving_recovery_score("intact", true, signature.label);
                    (end, "intact".to_string(), score)
                }
                None => {
                    let score =
                        crate::scoring::carving_recovery_score("partial", false, signature.label);
                    (search_limit, "partial".to_string(), score)
                }
            }
        }
    };

    let size_bytes = end_offset.saturating_sub(start_offset);
    if size_bytes < signature.min_size {
        return Err(format!(
            "Signature carving for {} found a candidate below the minimum credible size.",
            signature.label
        ));
    }

    if integrity == "intact" {
        let candidate_length = usize::try_from(size_bytes).map_err(|_| {
            format!(
                "Signature carving for {} produced a candidate larger than the supported validation window.",
                signature.label
            )
        })?;
        let candidate_bytes = read_window(reader, start_offset, candidate_length)?;
        if candidate_bytes.len() as u64 != size_bytes {
            integrity = "partial".into();
            recovery_score = recovery_score.min(35);
            validator_status = "partial-unvalidated".into();
        } else if validate_candidate_bytes(signature, &candidate_bytes) {
            byte_runs.push(ByteRun {
                offset: start_offset,
                length: size_bytes,
                zero_fill: false,
                ..Default::default()
            });
            validator_status = validated_status_for_signature(signature, &candidate_bytes, false);
        } else if let Some(fragmented_runs) =
            recover_fragmented_candidate_runs(signature, start_offset, &candidate_bytes)
        {
            integrity = "fragmented".into();
            recovery_score =
                crate::scoring::carving_recovery_score("fragmented", true, signature.label);
            gap_count = fragmented_runs.gap_count as u8;
            assembly_segment_count = fragmented_runs.byte_runs.len() as u8;
            validator_status =
                validated_status_for_signature(signature, &fragmented_runs.stitched_bytes, true);
            byte_runs = fragmented_runs.byte_runs;
        } else {
            integrity = "corrupt".into();
            recovery_score =
                crate::scoring::carving_recovery_score("corrupt", false, signature.label);
            validator_status = "failed".into();
        }
    }

    if byte_runs.is_empty() {
        byte_runs.push(ByteRun {
            offset: start_offset,
            length: size_bytes,
            zero_fill: false,
            ..Default::default()
        });
    }

    let reconstructed_size = byte_runs.iter().map(|run| run.length).sum::<u64>();
    let recovery_complexity = classify_recovery_complexity(
        &integrity,
        assembly_segment_count,
        gap_count,
        &validator_status,
    )
    .to_string();

    // Compute SHA-256 for validated candidates (read from byte runs)
    let sha256 = if integrity == "intact" || integrity == "fragmented" {
        compute_sha256_from_runs(reader, &byte_runs).ok()
    } else {
        None
    };

    Ok(CarvedFileCandidate {
        name: format!("carved-{:04}.{}", ordinal, signature.extension),
        extension: signature.extension.into(),
        size_bytes: reconstructed_size,
        integrity,
        recovery_score,
        start_offset,
        byte_runs,
        validator_status,
        assembly_segment_count,
        gap_count,
        recovery_complexity,
        sha256,
    })
}

fn carving_candidate_end_offset(candidate: &CarvedFileCandidate) -> u64 {
    candidate
        .byte_runs
        .last()
        .map(|run| run.offset.saturating_add(run.length))
        .unwrap_or_else(|| candidate.start_offset.saturating_add(candidate.size_bytes))
}

fn recover_fragmented_candidate_runs(
    signature: SignatureDefinition,
    start_offset: u64,
    candidate_bytes: &[u8],
) -> Option<FragmentedCandidateAssembly> {
    let mut gap_ranges = removable_gap_ranges(candidate_bytes);
    if gap_ranges.is_empty() {
        return None;
    }
    gap_ranges.truncate(MAX_FRAGMENT_GAP_CANDIDATES);

    for target_gap_count in 1..=MAX_REMOVABLE_GAPS
        .min(gap_ranges.len())
        .min(MAX_ASSEMBLY_SEGMENTS.saturating_sub(1))
    {
        let mut selected = Vec::with_capacity(target_gap_count);
        if let Some(runs) = recover_fragmented_candidate_runs_with_gap_count(
            signature,
            start_offset,
            candidate_bytes,
            &gap_ranges,
            target_gap_count,
            0,
            &mut selected,
        ) {
            return Some(runs);
        }
    }

    None
}

fn recover_fragmented_candidate_runs_with_gap_count(
    signature: SignatureDefinition,
    start_offset: u64,
    candidate_bytes: &[u8],
    gap_ranges: &[(usize, usize)],
    target_gap_count: usize,
    next_index: usize,
    selected: &mut Vec<(usize, usize)>,
) -> Option<FragmentedCandidateAssembly> {
    if selected.len() == target_gap_count {
        let normalized_gaps = normalized_gap_ranges(selected);
        let stitched = stitched_bytes_without_gaps(candidate_bytes, &normalized_gaps);
        if stitched.len() < signature.min_size as usize
            || !validate_candidate_bytes(signature, &stitched)
        {
            return None;
        }
        let byte_runs =
            byte_runs_without_gaps(start_offset, candidate_bytes.len(), &normalized_gaps);
        if byte_runs.len() > MAX_ASSEMBLY_SEGMENTS {
            return None;
        }
        return Some(FragmentedCandidateAssembly {
            byte_runs,
            stitched_bytes: stitched,
            gap_count: normalized_gaps.len(),
        });
    }

    for gap_index in next_index..gap_ranges.len() {
        let gap = gap_ranges[gap_index];
        if gap.0 == 0 || gap.1 >= candidate_bytes.len() {
            continue;
        }
        if selected
            .iter()
            .any(|selected_gap| gaps_overlap_or_touch(*selected_gap, gap))
        {
            continue;
        }

        selected.push(gap);
        if let Some(runs) = recover_fragmented_candidate_runs_with_gap_count(
            signature,
            start_offset,
            candidate_bytes,
            gap_ranges,
            target_gap_count,
            gap_index + 1,
            selected,
        ) {
            return Some(runs);
        }
        selected.pop();
    }

    None
}

#[derive(Debug, Clone)]
struct FragmentedCandidateAssembly {
    byte_runs: Vec<ByteRun>,
    stitched_bytes: Vec<u8>,
    gap_count: usize,
}

fn normalized_gap_ranges(gaps: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut normalized = gaps.to_vec();
    normalized.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    normalized
}

fn stitched_bytes_without_gaps(candidate_bytes: &[u8], gaps: &[(usize, usize)]) -> Vec<u8> {
    let total_gap_bytes = gaps.iter().map(|(start, end)| end - start).sum::<usize>();
    let mut stitched = Vec::with_capacity(candidate_bytes.len().saturating_sub(total_gap_bytes));
    let mut cursor = 0usize;

    for (gap_start, gap_end) in gaps {
        if *gap_start > cursor {
            stitched.extend_from_slice(&candidate_bytes[cursor..*gap_start]);
        }
        cursor = *gap_end;
    }

    if cursor < candidate_bytes.len() {
        stitched.extend_from_slice(&candidate_bytes[cursor..]);
    }

    stitched
}

fn byte_runs_without_gaps(
    start_offset: u64,
    candidate_length: usize,
    gaps: &[(usize, usize)],
) -> Vec<ByteRun> {
    let mut runs = Vec::with_capacity(gaps.len() + 1);
    let mut cursor = 0usize;

    for (gap_start, gap_end) in gaps {
        if *gap_start > cursor {
            runs.push(ByteRun {
                offset: start_offset + cursor as u64,
                length: (*gap_start - cursor) as u64,
                zero_fill: false,
                ..Default::default()
            });
        }
        cursor = *gap_end;
    }

    if cursor < candidate_length {
        runs.push(ByteRun {
            offset: start_offset + cursor as u64,
            length: (candidate_length - cursor) as u64,
            zero_fill: false,
            ..Default::default()
        });
    }

    runs
}

fn gaps_overlap_or_touch(left: (usize, usize), right: (usize, usize)) -> bool {
    let (left_start, left_end) = left;
    let (right_start, right_end) = right;
    left_start <= right_end && right_start <= left_end
}

fn removable_gap_ranges(candidate_bytes: &[u8]) -> Vec<(usize, usize)> {
    if candidate_bytes.len() < FRAGMENT_GAP_MIN_SIZE {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    while cursor < candidate_bytes.len() {
        let value = candidate_bytes[cursor];
        if value != 0x00 && value != 0xFF {
            cursor += 1;
            continue;
        }

        let start = cursor;
        while cursor < candidate_bytes.len() && candidate_bytes[cursor] == value {
            cursor += 1;
        }
        if cursor - start >= FRAGMENT_GAP_MIN_SIZE {
            ranges.push((start, cursor));
        }
    }

    ranges.sort_by(|left, right| {
        (right.1 - right.0)
            .cmp(&(left.1 - left.0))
            .then_with(|| left.0.cmp(&right.0))
    });
    ranges
}

fn validate_candidate_bytes(signature: SignatureDefinition, bytes: &[u8]) -> bool {
    match signature.extension {
        "jpg" => validate_jpeg_bytes(bytes),
        "png" => validate_png_bytes(bytes),
        "pdf" => validate_pdf_bytes(bytes),
        "zip" => validate_zip_bytes(bytes),
        "gif" => bytes.starts_with(b"GIF8") && bytes.len() >= 13,
        "bmp" => bytes.starts_with(&[0x42, 0x4D]) && bytes.len() >= 26,
        "sqlite" => bytes.starts_with(b"SQLite format 3\x00"),
        _ => true,
    }
}

fn validate_jpeg_bytes(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xff, 0xd8, 0xff]) && bytes.ends_with(&[0xff, 0xd9])
}

fn validate_png_bytes(bytes: &[u8]) -> bool {
    if !bytes.starts_with(PNG_SIGNATURE) {
        return false;
    }

    let mut offset = PNG_SIGNATURE.len();
    let mut saw_ihdr = false;

    while offset < bytes.len() {
        if offset + 12 > bytes.len() {
            return false;
        }

        let chunk_length = u32::from_be_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        let chunk_type = &bytes[offset + 4..offset + 8];
        let data_start = offset + 8;
        let data_end = data_start + chunk_length;
        let crc_end = data_end + 4;

        if crc_end > bytes.len() {
            return false;
        }

        let expected_crc = u32::from_be_bytes([
            bytes[data_end],
            bytes[data_end + 1],
            bytes[data_end + 2],
            bytes[data_end + 3],
        ]);
        let mut hasher = Hasher::new();
        hasher.update(chunk_type);
        hasher.update(&bytes[data_start..data_end]);
        if hasher.finalize() != expected_crc {
            return false;
        }

        if !saw_ihdr {
            if chunk_type != b"IHDR" {
                return false;
            }
            saw_ihdr = true;
        }

        offset = crc_end;
        if chunk_type == b"IEND" {
            return offset == bytes.len();
        }
    }

    false
}

fn validate_pdf_bytes(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"%PDF-") {
        return false;
    }

    if !bytes.ends_with(b"%%EOF") {
        return false;
    }

    let search_start = bytes.len().saturating_sub(2048);
    bytes[search_start..]
        .windows("startxref".len())
        .any(|window| window == b"startxref")
}

fn validate_zip_bytes(bytes: &[u8]) -> bool {
    let cursor = Cursor::new(bytes);
    let mut archive = match ZipArchive::new(cursor) {
        Ok(archive) => archive,
        Err(_) => return false,
    };

    for index in 0..archive.len().min(4) {
        if archive.by_index(index).is_err() {
            return false;
        }
    }

    true
}

fn validated_status_for_signature(
    signature: SignatureDefinition,
    bytes: &[u8],
    reassembled: bool,
) -> String {
    if signature.extension == "zip" {
        if is_office_zip(bytes) {
            return "office-validated".into();
        }
        return "zip-validated".into();
    }

    if reassembled {
        return "reassembled".into();
    }

    "validated".into()
}

fn is_office_zip(bytes: &[u8]) -> bool {
    let cursor = Cursor::new(bytes);
    let mut archive = match ZipArchive::new(cursor) {
        Ok(archive) => archive,
        Err(_) => return false,
    };

    let mut has_content_types = false;
    let mut has_office_part = false;

    for index in 0..archive.len().min(32) {
        let Ok(entry) = archive.by_index(index) else {
            return false;
        };
        let name = entry.name();
        if name == "[Content_Types].xml" {
            has_content_types = true;
        }
        if name.starts_with("word/") || name.starts_with("xl/") || name.starts_with("ppt/") {
            has_office_part = true;
        }
    }

    has_content_types && has_office_part
}

fn classify_recovery_complexity(
    integrity: &str,
    assembly_segment_count: u8,
    gap_count: u8,
    validator_status: &str,
) -> &'static str {
    crate::scoring::classify_recovery_complexity(
        integrity,
        assembly_segment_count,
        gap_count,
        validator_status,
    )
}

// ---------------------------------------------------------------------------
// SHA-256 hashing
// ---------------------------------------------------------------------------

fn compute_sha256_from_runs(reader: &mut File, byte_runs: &[ByteRun]) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for run in byte_runs {
        if run.zero_fill {
            let zeros = vec![0u8; run.length.min(65536) as usize];
            let mut remaining = run.length;
            while remaining > 0 {
                let chunk = remaining.min(zeros.len() as u64) as usize;
                hasher.update(&zeros[..chunk]);
                remaining -= chunk as u64;
            }
        } else {
            let data = read_window(reader, run.offset, run.length as usize)?;
            hasher.update(&data);
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

// ---------------------------------------------------------------------------
// Footer resolvers for new strategies
// ---------------------------------------------------------------------------

fn resolve_container_atom_end(reader: &mut File, start: u64, limit: u64) -> Option<u64> {
    let mut offset = start;
    let mut found_mdat = false;
    while offset < limit {
        let buf = read_window(reader, offset, 16).ok()?;
        if buf.len() < 8 {
            break;
        }
        let size = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as u64;
        let atom_type = &buf[4..8];
        if size == 0 {
            // Atom extends to end of file
            return Some(limit);
        }
        if size == 1 && buf.len() >= 16 {
            // 64-bit extended size
            let ext_size = u64::from_be_bytes([
                buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15],
            ]);
            offset += ext_size;
        } else if size < 8 {
            break;
        } else {
            offset += size;
        }
        if atom_type == b"mdat" || atom_type == b"moov" {
            found_mdat = true;
        }
    }
    if found_mdat || offset > start + 32 {
        Some(offset.min(limit))
    } else {
        None
    }
}

fn resolve_ebml_end(reader: &mut File, start: u64, limit: u64) -> Option<u64> {
    // Read EBML header: element ID (1A 45 DF A3) + variable-length size
    let buf = read_window(reader, start, 16).ok()?;
    if buf.len() < 8 || buf[0..4] != [0x1A, 0x45, 0xDF, 0xA3] {
        return None;
    }
    // Parse VINT size at offset 4
    let (header_size, header_len) = parse_ebml_vint(&buf[4..])?;
    let after_header = start + 4 + header_len as u64 + header_size;
    // After EBML header comes the Segment element (0x18 0x53 0x80 0x67)
    let seg_buf = read_window(reader, after_header, 16).ok()?;
    if seg_buf.len() >= 4 && seg_buf[0..4] == [0x18, 0x53, 0x80, 0x67] {
        let (seg_size, seg_len) = parse_ebml_vint(&seg_buf[4..])?;
        let end = after_header + 4 + seg_len as u64 + seg_size;
        return Some(end.min(limit));
    }
    None
}

fn parse_ebml_vint(data: &[u8]) -> Option<(u64, usize)> {
    if data.is_empty() {
        return None;
    }
    let first = data[0];
    let len = first.leading_zeros() as usize + 1;
    if len > 8 || data.len() < len {
        return None;
    }
    let mut value = (first & (0xFF >> len)) as u64;
    for byte in &data[1..len] {
        value = (value << 8) | *byte as u64;
    }
    // Check for "unknown size" marker
    let unknown_marker = (1u64 << (7 * len)) - 1;
    if value == unknown_marker {
        return None;
    }
    Some((value, len))
}

fn resolve_mp3_end(reader: &mut File, start: u64, limit: u64) -> Option<u64> {
    let buf = read_window(reader, start, 10.min((limit - start) as usize)).ok()?;
    let mut offset = start;
    // If ID3 tag present, skip it
    if buf.len() >= 10 && &buf[0..3] == b"ID3" {
        let size = ((buf[6] as u64 & 0x7F) << 21)
            | ((buf[7] as u64 & 0x7F) << 14)
            | ((buf[8] as u64 & 0x7F) << 7)
            | (buf[9] as u64 & 0x7F);
        offset = start + 10 + size;
    }
    // Walk MP3 frames: look for sync word 0xFFE0 mask
    let mut frame_count = 0u32;
    while offset < limit && frame_count < 10000 {
        let hdr = read_window(reader, offset, 4).ok()?;
        if hdr.len() < 4 {
            break;
        }
        if hdr[0] != 0xFF || (hdr[1] & 0xE0) != 0xE0 {
            break;
        }
        let bitrate_index = ((hdr[2] >> 4) & 0x0F) as usize;
        let sample_rate_index = ((hdr[2] >> 2) & 0x03) as usize;
        let padding = ((hdr[2] >> 1) & 0x01) as u64;
        // MPEG1 Layer3 bitrate table
        let bitrates = [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
        ];
        let sample_rates = [44100u64, 48000, 32000, 0];
        if bitrate_index == 0 || bitrate_index == 15 || sample_rate_index == 3 {
            break;
        }
        let frame_size = (144 * bitrates[bitrate_index] as u64 * 1000
            / sample_rates[sample_rate_index])
            + padding;
        if frame_size < 4 {
            break;
        }
        offset += frame_size;
        frame_count += 1;
    }
    if frame_count >= 3 {
        Some(offset.min(limit))
    } else {
        None
    }
}

fn read_size_field(buf: &[u8], offset: usize, field_size: u8, big_endian: bool) -> Option<u64> {
    let end = offset + field_size as usize;
    if buf.len() < end {
        return None;
    }
    let bytes = &buf[offset..end];
    match field_size {
        2 => {
            if big_endian {
                Some(u16::from_be_bytes([bytes[0], bytes[1]]) as u64)
            } else {
                Some(u16::from_le_bytes([bytes[0], bytes[1]]) as u64)
            }
        }
        4 => {
            if big_endian {
                Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64)
            } else {
                Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64)
            }
        }
        8 => {
            if big_endian {
                Some(u64::from_be_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ]))
            } else {
                Some(u64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ]))
            }
        }
        _ => None,
    }
}

fn find_zip_end_offset(search_start: u64, buffer: &[u8], search_limit: u64) -> Option<u64> {
    let marker = [b'P', b'K', 0x05, 0x06];
    let mut offset = 0usize;

    while offset < buffer.len() {
        let relative_marker = find_bytes(&buffer[offset..], &marker)?;
        let marker_index = offset + relative_marker;
        if marker_index + 22 > buffer.len() {
            return Some(search_limit);
        }

        let comment_length =
            u16::from_le_bytes([buffer[marker_index + 20], buffer[marker_index + 21]]) as usize;
        let end_index = marker_index + 22 + comment_length;
        if end_index <= buffer.len() {
            return Some(search_start + end_index as u64);
        }

        offset = marker_index + marker.len();
    }

    None
}

fn find_bytes(buffer: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || buffer.len() < needle.len() {
        return None;
    }

    buffer
        .windows(needle.len())
        .position(|window| window == needle)
}

fn read_window(reader: &mut File, offset: u64, length: usize) -> Result<Vec<u8>, String> {
    if length == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0_u8; length];
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|error| format!("Unable to seek the image at offset {offset}: {error}"))?;
    let bytes_read = reader
        .read(&mut buffer)
        .map_err(|error| format!("Unable to read the image at offset {offset}: {error}"))?;
    buffer.truncate(bytes_read);
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs, io::Write};
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    fn write_test_image(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let root = env::temp_dir().join(format!("recupere-carving-test-{}", std::process::id()));
        fs::create_dir_all(&root).expect("carving test workspace should exist");
        let path = root.join(name);
        fs::write(&path, bytes).expect("carving test image should be written");
        path
    }

    fn jpeg_signature_image() -> Vec<u8> {
        let mut bytes = vec![0_u8; 128];
        let payload = [
            0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x02, 0x03,
            0x04, 0x05, 0xff, 0xd9,
        ];
        bytes[24..24 + payload.len()].copy_from_slice(&payload);
        bytes
    }

    fn zip_signature_image() -> Vec<u8> {
        let mut bytes = vec![0_u8; 256];
        let payload = [
            b'P', b'K', 0x03, 0x04, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, b'H', b'I', b'P', b'K', 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ];
        bytes[40..40 + payload.len()].copy_from_slice(&payload);
        bytes
    }

    fn chunk_crc(chunk_type: &[u8; 4], data: &[u8]) -> u32 {
        let mut hasher = Hasher::new();
        hasher.update(chunk_type);
        hasher.update(data);
        hasher.finalize()
    }

    fn png_signature_image() -> Vec<u8> {
        let mut bytes = vec![0_u8; 256];
        let mut payload = Vec::new();
        payload.extend_from_slice(PNG_SIGNATURE);

        let ihdr_data = [0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0];
        payload.extend_from_slice(&(ihdr_data.len() as u32).to_be_bytes());
        payload.extend_from_slice(b"IHDR");
        payload.extend_from_slice(&ihdr_data);
        payload.extend_from_slice(&chunk_crc(b"IHDR", &ihdr_data).to_be_bytes());

        let idat_data = [0x78, 0x9c, 0x63, 0x00];
        payload.extend_from_slice(&(idat_data.len() as u32).to_be_bytes());
        payload.extend_from_slice(b"IDAT");
        payload.extend_from_slice(&idat_data);
        payload.extend_from_slice(&chunk_crc(b"IDAT", &idat_data).to_be_bytes());

        payload.extend_from_slice(&0_u32.to_be_bytes());
        payload.extend_from_slice(b"IEND");
        payload.extend_from_slice(&chunk_crc(b"IEND", &[]).to_be_bytes());

        bytes[64..64 + payload.len()].copy_from_slice(&payload);
        bytes
    }

    fn corrupt_png_signature_image() -> Vec<u8> {
        let mut bytes = png_signature_image();
        let payload_offset = 64;
        let ihdr_crc_offset = payload_offset + 8 + 4 + 13;
        bytes[ihdr_crc_offset] ^= 0xff;
        bytes
    }

    fn fragmented_png_signature_image() -> Vec<u8> {
        let mut bytes = vec![0_u8; 512];
        let mut payload = Vec::new();
        payload.extend_from_slice(PNG_SIGNATURE);

        let ihdr_data = [0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0];
        payload.extend_from_slice(&(ihdr_data.len() as u32).to_be_bytes());
        payload.extend_from_slice(b"IHDR");
        payload.extend_from_slice(&ihdr_data);
        payload.extend_from_slice(&chunk_crc(b"IHDR", &ihdr_data).to_be_bytes());

        let first_segment = payload.clone();

        let idat_data = [0x78, 0x9c, 0x63, 0x00];
        payload.extend_from_slice(&(idat_data.len() as u32).to_be_bytes());
        payload.extend_from_slice(b"IDAT");
        payload.extend_from_slice(&idat_data);
        payload.extend_from_slice(&chunk_crc(b"IDAT", &idat_data).to_be_bytes());
        payload.extend_from_slice(&0_u32.to_be_bytes());
        payload.extend_from_slice(b"IEND");
        payload.extend_from_slice(&chunk_crc(b"IEND", &[]).to_be_bytes());

        let second_segment = &payload[first_segment.len()..];
        let start_offset = 64usize;
        let gap_size = 96usize;
        bytes[start_offset + first_segment.len()..start_offset + first_segment.len() + gap_size]
            .fill(0xFF);
        bytes[start_offset..start_offset + first_segment.len()].copy_from_slice(&first_segment);
        let second_offset = start_offset + first_segment.len() + gap_size;
        bytes[second_offset..second_offset + second_segment.len()].copy_from_slice(second_segment);
        bytes
    }

    fn multi_fragmented_png_signature_image() -> Vec<u8> {
        let mut bytes = vec![0_u8; 768];
        let mut payload = Vec::new();
        payload.extend_from_slice(PNG_SIGNATURE);

        let ihdr_data = [0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0];
        payload.extend_from_slice(&(ihdr_data.len() as u32).to_be_bytes());
        payload.extend_from_slice(b"IHDR");
        payload.extend_from_slice(&ihdr_data);
        payload.extend_from_slice(&chunk_crc(b"IHDR", &ihdr_data).to_be_bytes());

        let idat_data = [0x78, 0x9c, 0x63, 0x00];
        payload.extend_from_slice(&(idat_data.len() as u32).to_be_bytes());
        payload.extend_from_slice(b"IDAT");
        payload.extend_from_slice(&idat_data);
        payload.extend_from_slice(&chunk_crc(b"IDAT", &idat_data).to_be_bytes());
        payload.extend_from_slice(&0_u32.to_be_bytes());
        payload.extend_from_slice(b"IEND");
        payload.extend_from_slice(&chunk_crc(b"IEND", &[]).to_be_bytes());

        let segment_one_end = PNG_SIGNATURE.len() + 12;
        let segment_two_end = segment_one_end + 20;
        let first_segment = &payload[..segment_one_end];
        let second_segment = &payload[segment_one_end..segment_two_end];
        let third_segment = &payload[segment_two_end..];

        let start_offset = 96usize;
        let first_gap_size = 64usize;
        let second_gap_size = 80usize;

        bytes[start_offset..start_offset + first_segment.len()].copy_from_slice(first_segment);
        bytes[start_offset + first_segment.len()
            ..start_offset + first_segment.len() + first_gap_size]
            .fill(0xFF);

        let second_offset = start_offset + first_segment.len() + first_gap_size;
        bytes[second_offset..second_offset + second_segment.len()].copy_from_slice(second_segment);
        bytes[second_offset + second_segment.len()
            ..second_offset + second_segment.len() + second_gap_size]
            .fill(0x00);

        let third_offset = second_offset + second_segment.len() + second_gap_size;
        bytes[third_offset..third_offset + third_segment.len()].copy_from_slice(third_segment);
        bytes
    }

    fn three_gap_fragmented_png_signature_image() -> Vec<u8> {
        let mut bytes = vec![0_u8; 1024];
        let mut payload = Vec::new();
        payload.extend_from_slice(PNG_SIGNATURE);

        let ihdr_data = [0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0];
        payload.extend_from_slice(&(ihdr_data.len() as u32).to_be_bytes());
        payload.extend_from_slice(b"IHDR");
        payload.extend_from_slice(&ihdr_data);
        payload.extend_from_slice(&chunk_crc(b"IHDR", &ihdr_data).to_be_bytes());

        let idat_data = [0x78, 0x9c, 0x63, 0x00];
        payload.extend_from_slice(&(idat_data.len() as u32).to_be_bytes());
        payload.extend_from_slice(b"IDAT");
        payload.extend_from_slice(&idat_data);
        payload.extend_from_slice(&chunk_crc(b"IDAT", &idat_data).to_be_bytes());
        payload.extend_from_slice(&0_u32.to_be_bytes());
        payload.extend_from_slice(b"IEND");
        payload.extend_from_slice(&chunk_crc(b"IEND", &[]).to_be_bytes());

        let segment_one_end = 14usize;
        let segment_two_end = 30usize;
        let segment_three_end = 46usize;
        let first_segment = &payload[..segment_one_end];
        let second_segment = &payload[segment_one_end..segment_two_end];
        let third_segment = &payload[segment_two_end..segment_three_end];
        let fourth_segment = &payload[segment_three_end..];

        let start_offset = 128usize;
        let first_gap_size = 48usize;
        let second_gap_size = 64usize;
        let third_gap_size = 72usize;

        bytes[start_offset..start_offset + first_segment.len()].copy_from_slice(first_segment);
        bytes[start_offset + first_segment.len()
            ..start_offset + first_segment.len() + first_gap_size]
            .fill(0xFF);

        let second_offset = start_offset + first_segment.len() + first_gap_size;
        bytes[second_offset..second_offset + second_segment.len()].copy_from_slice(second_segment);
        bytes[second_offset + second_segment.len()
            ..second_offset + second_segment.len() + second_gap_size]
            .fill(0x00);

        let third_offset = second_offset + second_segment.len() + second_gap_size;
        bytes[third_offset..third_offset + third_segment.len()].copy_from_slice(third_segment);
        bytes[third_offset + third_segment.len()
            ..third_offset + third_segment.len() + third_gap_size]
            .fill(0xFF);

        let fourth_offset = third_offset + third_segment.len() + third_gap_size;
        bytes[fourth_offset..fourth_offset + fourth_segment.len()].copy_from_slice(fourth_segment);
        bytes
    }

    fn valid_zip_payload() -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut archive = ZipWriter::new(&mut cursor);
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            archive
                .start_file("note.txt", options)
                .expect("zip entry should start");
            archive
                .write_all(b"zip payload")
                .expect("zip bytes should be written");
            archive.finish().expect("zip archive should finish");
        }
        cursor.into_inner()
    }

    fn corrupt_zip_signature_image() -> Vec<u8> {
        let mut payload = valid_zip_payload();
        let eocd_offset = payload
            .windows(4)
            .rposition(|window| window == [b'P', b'K', 0x05, 0x06])
            .expect("zip payload should contain EOCD");
        payload[eocd_offset + 16..eocd_offset + 20].copy_from_slice(&9_999_u32.to_le_bytes());

        let mut bytes = vec![0_u8; 512];
        bytes[96..96 + payload.len()].copy_from_slice(&payload);
        bytes
    }

    fn corrupt_pdf_signature_image() -> Vec<u8> {
        let mut bytes = vec![0_u8; 256];
        let payload = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\n%%EOF";
        bytes[48..48 + payload.len()].copy_from_slice(payload);
        bytes
    }

    #[test]
    fn carve_signatures_finds_a_jpeg_candidate() {
        let image_path = write_test_image("carving-jpeg.img", &jpeg_signature_image());
        let carved = carve_signatures(&image_path).expect("jpeg candidate should be carved");

        assert_eq!(carved.len(), 1);
        assert_eq!(carved[0].extension, "jpg");
        assert_eq!(carved[0].start_offset, 24);
        assert_eq!(carved[0].integrity, "intact");
        assert_eq!(carved[0].validator_status, "validated");
        assert_eq!(carved[0].assembly_segment_count, 1);
        assert_eq!(carved[0].gap_count, 0);
        assert_eq!(carved[0].recovery_complexity, "low");
        assert_eq!(carved[0].byte_runs[0].offset, 24);
        assert_eq!(carved[0].byte_runs[0].length, carved[0].size_bytes);
    }

    #[test]
    fn carve_signatures_truncated_jpeg_without_eoi_is_reported_corrupt() {
        // Camera write failure: JPEG SOI + APP0 JFIF header but the
        // End-of-Image marker (0xFF 0xD9) never made it to disk. The
        // carver should not silently produce a "clean" file — it must
        // mark the candidate corrupt so the operator knows to try a
        // different strategy (e.g. extract the thumbnail from APP1).
        let mut bytes = vec![0_u8; 256];
        let truncated = [
            0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x02, 0x03,
            0x04, 0x05, // note: no 0xff 0xd9 footer
        ];
        bytes[32..32 + truncated.len()].copy_from_slice(&truncated);
        let image_path = write_test_image("carving-jpeg-truncated.img", &bytes);
        let carved = carve_signatures(&image_path).expect("truncated jpeg should still be handled");
        // Either no candidate (strict) or a candidate marked corrupt —
        // both are acceptable outcomes; silently producing an "intact"
        // JPEG would be wrong.
        for candidate in &carved {
            if candidate.extension == "jpg" {
                assert_ne!(
                    candidate.integrity, "intact",
                    "a JPEG without its EOI marker must not be tagged intact"
                );
            }
        }
    }

    #[test]
    fn carve_signatures_jpeg_with_trailing_garbage_stops_at_eoi() {
        // Many cameras pad files with XMP / Exif metadata after EOI.
        // The carver must clamp the recovered length to the EOI position
        // so the exported file is a clean JPEG — trailing garbage would
        // cause downstream decoders to error on some viewers.
        let mut bytes = vec![0_u8; 256];
        let clean_jpeg = [
            0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x02, 0x03,
            0x04, 0x05, 0xff, 0xd9,
        ];
        bytes[32..32 + clean_jpeg.len()].copy_from_slice(&clean_jpeg);
        // Trailing garbage right after EOI — simulates Exif + XMP padding.
        let trailing_start = 32 + clean_jpeg.len();
        bytes[trailing_start..trailing_start + 8].copy_from_slice(b"TRAILING");
        let image_path = write_test_image("carving-jpeg-trailing.img", &bytes);
        let carved = carve_signatures(&image_path).expect("clean jpeg should be carved");
        let jpeg = carved
            .iter()
            .find(|c| c.extension == "jpg")
            .expect("jpeg candidate expected");
        assert_eq!(jpeg.integrity, "intact");
        assert_eq!(jpeg.size_bytes, clean_jpeg.len() as u64);
    }

    #[test]
    fn carve_signatures_finds_a_zip_candidate() {
        let image_path = write_test_image("carving-zip.img", &zip_signature_image());
        let carved = carve_signatures(&image_path).expect("zip candidate should be carved");

        assert_eq!(carved.len(), 1);
        assert_eq!(carved[0].extension, "zip");
        assert_eq!(carved[0].start_offset, 40);
        assert_eq!(carved[0].integrity, "intact");
        assert_eq!(carved[0].validator_status, "zip-validated");
    }

    #[test]
    fn carve_signatures_marks_corrupt_png_candidates() {
        let image_path =
            write_test_image("carving-png-corrupt.img", &corrupt_png_signature_image());
        let carved = carve_signatures(&image_path).expect("corrupt png candidate should be carved");

        assert_eq!(carved.len(), 1);
        assert_eq!(carved[0].extension, "png");
        assert_eq!(carved[0].integrity, "corrupt");
        assert_eq!(carved[0].recovery_score, 18);
        assert_eq!(carved[0].validator_status, "failed");
        assert_eq!(carved[0].recovery_complexity, "high");
    }

    #[test]
    fn carve_signatures_marks_corrupt_zip_candidates() {
        let image_path =
            write_test_image("carving-zip-corrupt.img", &corrupt_zip_signature_image());
        let carved = carve_signatures(&image_path).expect("corrupt zip candidate should be carved");

        assert_eq!(carved.len(), 1);
        assert_eq!(carved[0].extension, "zip");
        assert_eq!(carved[0].integrity, "corrupt");
        assert_eq!(carved[0].recovery_score, 18);
    }

    #[test]
    fn carve_signatures_marks_corrupt_pdf_candidates_without_startxref() {
        let image_path =
            write_test_image("carving-pdf-corrupt.img", &corrupt_pdf_signature_image());
        let carved = carve_signatures(&image_path).expect("corrupt pdf candidate should be carved");

        assert_eq!(carved.len(), 1);
        assert_eq!(carved[0].extension, "pdf");
        assert_eq!(carved[0].integrity, "corrupt");
        assert_eq!(carved[0].recovery_score, 18);
    }

    #[test]
    fn carve_signatures_rebuilds_a_fragmented_png_across_a_single_gap() {
        let image_path = write_test_image(
            "carving-png-fragmented.img",
            &fragmented_png_signature_image(),
        );
        let carved =
            carve_signatures(&image_path).expect("fragmented png candidate should be carved");

        assert_eq!(carved.len(), 1);
        assert_eq!(carved[0].extension, "png");
        assert_eq!(carved[0].integrity, "fragmented");
        assert_eq!(carved[0].byte_runs.len(), 2);
        assert_eq!(carved[0].assembly_segment_count, 2);
        assert_eq!(carved[0].gap_count, 1);
        assert_eq!(carved[0].validator_status, "reassembled");
        assert_eq!(carved[0].recovery_complexity, "medium");
        assert!(carved[0].byte_runs[1].offset > carved[0].byte_runs[0].offset);
        assert_eq!(
            carved[0].size_bytes,
            carved[0]
                .byte_runs
                .iter()
                .map(|run| run.length)
                .sum::<u64>()
        );
    }

    #[test]
    fn carve_signatures_rebuilds_a_fragmented_png_across_two_gaps() {
        let image_path = write_test_image(
            "carving-png-fragmented-two-gaps.img",
            &multi_fragmented_png_signature_image(),
        );
        let carved =
            carve_signatures(&image_path).expect("multi-gap png candidate should be carved");

        assert_eq!(carved.len(), 1);
        assert_eq!(carved[0].extension, "png");
        assert_eq!(carved[0].integrity, "fragmented");
        assert_eq!(carved[0].byte_runs.len(), 3);
        assert_eq!(carved[0].assembly_segment_count, 3);
        assert_eq!(carved[0].gap_count, 2);
        assert_eq!(carved[0].validator_status, "reassembled");
        assert_eq!(carved[0].recovery_complexity, "high");
        assert!(carved[0]
            .byte_runs
            .windows(2)
            .all(|pair| pair[1].offset > pair[0].offset));
        assert_eq!(
            carved[0].size_bytes,
            carved[0]
                .byte_runs
                .iter()
                .map(|run| run.length)
                .sum::<u64>()
        );
    }

    #[test]
    fn carve_signatures_rebuilds_a_fragmented_png_across_three_gaps() {
        let image_path = write_test_image(
            "carving-png-fragmented-three-gaps.img",
            &three_gap_fragmented_png_signature_image(),
        );
        let carved =
            carve_signatures(&image_path).expect("three-gap png candidate should be carved");

        assert_eq!(carved.len(), 1);
        assert_eq!(carved[0].extension, "png");
        assert_eq!(carved[0].integrity, "fragmented");
        assert_eq!(carved[0].byte_runs.len(), 4);
        assert_eq!(carved[0].assembly_segment_count, 4);
        assert_eq!(carved[0].gap_count, 3);
        assert_eq!(carved[0].validator_status, "reassembled");
        assert_eq!(carved[0].recovery_complexity, "high");
        assert!(carved[0]
            .byte_runs
            .windows(2)
            .all(|pair| pair[1].offset > pair[0].offset));
    }
}
