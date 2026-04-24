#![no_main]
// ============================================================================
// Signature carving fuzz target
// ============================================================================
// Drives the full `carving::carve_signatures` pipeline (5 000+ LOC of
// header/footer scanning, assembly, CRC checks) against arbitrary bytes.
// Historical regressions in carvers are almost always out-of-bounds reads
// on malformed footer offsets, division-by-zero on fragment sizes, or
// runaway loops when the same magic header appears thousands of times. The
// fuzzer is the only realistic way to surface them.
//
// Run:
//   cd src-tauri
//   cargo +nightly fuzz run carving_signatures -- -max_total_time=600
// ============================================================================

use libfuzzer_sys::fuzz_target;
use std::io::Write;
use tempfile::NamedTempFile;

fuzz_target!(|data: &[u8]| {
    // Skip trivial buffers the scanner would never invoke assembly on.
    if data.len() < 64 {
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

    let _ = recupere_lib::carving::carve_signatures(tmp.path());
});
