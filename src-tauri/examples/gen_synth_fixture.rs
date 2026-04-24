// ============================================================================
// gen_synth_fixture — synthetic fixture generator for the native E2E harness
// ============================================================================
//
// Chantier 83, tranche 2. Writes deterministic binary blobs to disk so that
// `e2e/native/*.spec.ts` can exercise Récupère's UI/IPC/engine pipeline
// without ever touching a real device.
//
// Filesystem fidelity is NOT this example's job. Real parser correctness is
// already covered by the Rust unit tests (334 at time of writing) that
// consume `#[cfg(test)]` fixtures inside `analyzers::*`. The blobs written
// here exist only to give the WebdriverIO specs something deterministic the
// signature carver, lost-volume detector, and expert-mode toggles can see.
//
// Usage:
//   cargo run --example gen_synth_fixture -- <kind> <out_path>
//
// Kinds:
//   carver-signatures  8 MiB blob with JPEG, PDF and ZIP magic at known
//                      offsets — surfaced by the signature carver.
//   mbr-gpt            16 MiB blob with a minimal protective MBR + GPT
//                      primary header — surfaced by lost-volume detection.
//   expert-stub        1 MiB blob with readable text — enough to exercise
//                      the hex-preview / ADS / resource-fork UI toggles.
//
// This binary is a standalone Cargo example; it does not link against any
// `#[cfg(test)]` code from the library and does not modify the release
// bundle in any way.
// ============================================================================

use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: gen_synth_fixture <kind> <out_path>\n\n\
kinds:\n\
  carver-signatures  8 MiB blob with JPEG/PDF/ZIP signatures\n\
  mbr-gpt            16 MiB blob with protective MBR + GPT header\n\
  expert-stub        1 MiB blob with readable text\n";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }

    let kind = args[1].as_str();
    let out_path = PathBuf::from(&args[2]);

    let blob = match kind {
        "carver-signatures" => build_carver_signatures(),
        "mbr-gpt" => build_mbr_gpt(),
        "expert-stub" => build_expert_stub(),
        other => {
            eprintln!("unknown kind '{other}'\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(err) = fs::create_dir_all(parent) {
                eprintln!("failed to create {}: {err}", parent.display());
                return ExitCode::from(1);
            }
        }
    }

    match fs::File::create(&out_path).and_then(|mut f| f.write_all(&blob)) {
        Ok(()) => {
            println!("wrote {} bytes to {}", blob.len(), out_path.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("failed to write {}: {err}", out_path.display());
            ExitCode::from(1)
        }
    }
}

// ----------------------------------------------------------------------------
// carver-signatures: zero-filled 8 MiB blob with JPEG / PDF / ZIP signatures
// at fixed offsets. Exercises the signature carver without relying on any
// particular filesystem layout.
// ----------------------------------------------------------------------------
fn build_carver_signatures() -> Vec<u8> {
    let mut blob = vec![0_u8; 8 * 1024 * 1024];

    // JPEG: SOI (FFD8) + minimal APP0 (JFIF) at 0x1000, EOI (FFD9) ~64 KiB later.
    let jpeg_off = 0x1000;
    blob[jpeg_off] = 0xFF;
    blob[jpeg_off + 1] = 0xD8;
    blob[jpeg_off + 2] = 0xFF;
    blob[jpeg_off + 3] = 0xE0;
    blob[jpeg_off + 4] = 0x00;
    blob[jpeg_off + 5] = 0x10;
    blob[jpeg_off + 6..jpeg_off + 11].copy_from_slice(b"JFIF\x00");
    let jpeg_end = jpeg_off + 0x10000 - 2;
    blob[jpeg_end] = 0xFF;
    blob[jpeg_end + 1] = 0xD9;

    // PDF: "%PDF-1.4" header at 0x20000 + "%%EOF" trailer.
    let pdf_off = 0x20000;
    blob[pdf_off..pdf_off + 8].copy_from_slice(b"%PDF-1.4");
    let pdf_end = pdf_off + 0x10000 - 6;
    blob[pdf_end..pdf_end + 5].copy_from_slice(b"%%EOF");

    // ZIP: local file header (PK\x03\x04) at 0x40000 + EOCD (PK\x05\x06) later.
    let zip_off = 0x40000;
    blob[zip_off..zip_off + 4].copy_from_slice(b"PK\x03\x04");
    let zip_eocd = zip_off + 0x10000 - 22;
    blob[zip_eocd..zip_eocd + 4].copy_from_slice(b"PK\x05\x06");

    blob
}

// ----------------------------------------------------------------------------
// mbr-gpt: 16 MiB blob with a minimal protective MBR in sector 0 and a GPT
// primary header in sector 1. CRC fields are left zero — lost-volume
// detection is permissive enough on synthetic inputs for the UI flow test.
// ----------------------------------------------------------------------------
fn build_mbr_gpt() -> Vec<u8> {
    const SECTOR: usize = 512;
    const TOTAL_MIB: usize = 16;
    let total_sectors = (TOTAL_MIB * 1024 * 1024) / SECTOR;
    let mut blob = vec![0_u8; total_sectors * SECTOR];

    // Protective MBR — sector 0.
    blob[440..444].copy_from_slice(&0xDEAD_BEEF_u32.to_le_bytes());
    {
        let part = &mut blob[446..462];
        part[0] = 0x00;
        part[1] = 0x00;
        part[2] = 0x02;
        part[3] = 0x00;
        part[4] = 0xEE; // GPT protective partition type
        part[5] = 0xFF;
        part[6] = 0xFF;
        part[7] = 0xFF;
        part[8..12].copy_from_slice(&1_u32.to_le_bytes());
        let size_sectors: u32 = (total_sectors - 1).try_into().unwrap_or(u32::MAX);
        part[12..16].copy_from_slice(&size_sectors.to_le_bytes());
    }
    blob[510] = 0x55;
    blob[511] = 0xAA;

    // GPT primary header — sector 1.
    {
        let gpt = &mut blob[SECTOR..SECTOR * 2];
        gpt[0..8].copy_from_slice(b"EFI PART");
        gpt[8..12].copy_from_slice(&0x0001_0000_u32.to_le_bytes()); // revision 1.0
        gpt[12..16].copy_from_slice(&92_u32.to_le_bytes()); // header size
        gpt[24..32].copy_from_slice(&1_u64.to_le_bytes()); // my LBA
        gpt[32..40].copy_from_slice(&((total_sectors as u64) - 1).to_le_bytes()); // alternate LBA
        gpt[40..48].copy_from_slice(&34_u64.to_le_bytes()); // first usable
        gpt[48..56].copy_from_slice(&((total_sectors as u64) - 34).to_le_bytes());
    }

    blob
}

// ----------------------------------------------------------------------------
// expert-stub: 1 MiB blob whose first bytes are readable text. Used by the
// expert-mode spec to exercise hex-preview / ADS / resource-fork toggles
// without depending on a real filesystem.
// ----------------------------------------------------------------------------
fn build_expert_stub() -> Vec<u8> {
    let mut blob = vec![0_u8; 1024 * 1024];
    let text: &[u8] = b"RECUPERE EXPERT STUB FIXTURE\n\
This blob is NOT a real filesystem. It exists only to give the native\n\
E2E harness something deterministic to scroll through when exercising\n\
the hex-preview, ADS and resource-fork UI toggles. Filesystem fidelity\n\
is covered by the Rust unit tests.\n";
    blob[0..text.len()].copy_from_slice(text);
    blob
}
