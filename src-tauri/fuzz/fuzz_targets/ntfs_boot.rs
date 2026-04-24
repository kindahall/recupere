#![no_main]
// ============================================================================
// NTFS analyzer fuzz target
// ============================================================================
// Fuzz the whole NTFS deleted-file recovery pipeline against arbitrary input
// bytes. The parser path reads a boot sector, walks the MFT, and decodes
// file record attributes; any panic on invalid input is a bug because the
// engine must always surface parser errors via `Result` rather than crash
// the recovery session.
//
// Run:
//   cd src-tauri
//   cargo +nightly fuzz run ntfs_boot -- -max_total_time=600
//
// Seed corpus lives under `src-tauri/fuzz/corpus/ntfs_boot/` — add small
// fragments of real NTFS images there to give the fuzzer a head start.
// ============================================================================

use libfuzzer_sys::fuzz_target;
use std::io::Write;
use tempfile::NamedTempFile;

fuzz_target!(|data: &[u8]| {
    // A valid NTFS boot sector needs at least the fixed 512-byte layout the
    // analyzer reads up-front. Smaller inputs would never reach the parser
    // logic anyway and just slow the fuzzer down.
    if data.len() < 512 {
        return;
    }

    let mut tmp = match NamedTempFile::new() {
        Ok(tmp) => tmp,
        Err(_) => return,
    };
    if tmp.write_all(data).is_err() {
        return;
    }
    if tmp.flush().is_err() {
        return;
    }

    // Return value is explicitly ignored — we care about "does this panic?",
    // not about the recovery outcome.
    let _ = recupere_lib::analyzers::ntfs::recover_deleted_files(tmp.path());
    let _ = recupere_lib::analyzers::ntfs::list_visible_files(tmp.path());
});
