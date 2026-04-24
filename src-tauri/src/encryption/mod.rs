// ============================================================================
// Recupere — Encryption Detection & Unlock
// ============================================================================
// Magic-byte based detection for LUKS1/2, BitLocker, FileVault2, VeraCrypt.
// Cross-platform unlock via native tools (diskutil / manage-bde / cryptsetup).
// ============================================================================

use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub mod bruteforce;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncryptionType {
    FileVault2,
    BitLocker,
    Luks1,
    Luks2,
    VeraCrypt,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionInfo {
    pub detected: bool,
    pub encryption_type: EncryptionType,
    pub can_unlock: bool,
    pub workflow_state: String,
    pub safer_next_step: String,
    pub message: String,
}

/// Read the first 1 KiB of a device and identify the encryption format
/// from its magic bytes. Returns `Unknown` if nothing matches or read fails.
pub fn detect_encryption_real(device_path: &Path) -> EncryptionType {
    let mut buf = [0u8; 1024];
    let Ok(mut f) = File::open(device_path) else {
        return EncryptionType::Unknown;
    };
    if f.read(&mut buf).unwrap_or(0) < 16 {
        return EncryptionType::Unknown;
    }

    // LUKS1 / LUKS2: "LUKS\xba\xbe" at offset 0, version u16 BE at offset 6
    if &buf[0..6] == b"LUKS\xba\xbe" {
        let version = u16::from_be_bytes([buf[6], buf[7]]);
        return if version == 2 {
            EncryptionType::Luks2
        } else {
            EncryptionType::Luks1
        };
    }

    // BitLocker: "-FVE-FS-" at offset 3 of the NTFS-style boot sector
    if buf.len() >= 11 && &buf[3..11] == b"-FVE-FS-" {
        return EncryptionType::BitLocker;
    }
    // BitLocker-To-Go: "MSWIN4.1" / "MSDOS5.0" with FVE GUID later — best-effort
    if buf.len() >= 11 && (&buf[3..11] == b"MSWIN4.1" || &buf[3..11] == b"MSDOS5.0") {
        // Check for FVE signature deeper in the volume header
        if buf.windows(8).any(|w| w == b"-FVE-FS-") {
            return EncryptionType::BitLocker;
        }
    }

    // APFS / CoreStorage FileVault2: APFS container superblock magic "NXSB" at +32
    if buf.len() >= 36 && &buf[32..36] == b"NXSB" {
        // APFS encryption is per-volume; flag as potential FileVault candidate
        return EncryptionType::FileVault2;
    }

    EncryptionType::Unknown
}

/// Public-facing detection used by Tauri commands. Falls back to real
/// magic-byte detection when `is_encrypted` is set, otherwise returns "none".
pub fn detect_encryption_type(device_path: &str, is_encrypted: bool) -> EncryptionInfo {
    let path = Path::new(device_path);
    let detected_type = if path.exists() {
        detect_encryption_real(path)
    } else {
        EncryptionType::Unknown
    };

    let detected = is_encrypted || !matches!(detected_type, EncryptionType::Unknown);
    let can_unlock = matches!(
        detected_type,
        EncryptionType::FileVault2
            | EncryptionType::BitLocker
            | EncryptionType::Luks1
            | EncryptionType::Luks2
    );

    let (workflow_state, safer_next_step, message) = if !detected {
        (
            "clear",
            "Proceed with the normal read-only diagnostic and scan workflow.",
            "No encryption detected.".into(),
        )
    } else if can_unlock {
        (
            "pre_unlock_blocked",
            "Unlock the volume from Expert mode or through the host OS, then refresh devices and continue from the unlocked view only.",
            format!(
                "{:?} metadata detected. The locked view is not a trustworthy recovery surface until the volume is unlocked.",
                detected_type
            ),
        )
    } else {
        (
            "unsupported",
            "Do not keep scanning the locked view. Escalate to a supported host unlock workflow or a lab path before deeper analysis.",
            "Encrypted volume detected but unlock is not supported for this format.".into(),
        )
    };

    EncryptionInfo {
        detected,
        encryption_type: detected_type,
        can_unlock,
        workflow_state: workflow_state.into(),
        safer_next_step: safer_next_step.into(),
        message,
    }
}

/// Attempt to unlock an encrypted volume with a password.
/// Cross-platform: dispatches to the right native tool.
pub fn unlock_volume(device_path: &str, password: &str) -> Result<String, String> {
    let enc = detect_encryption_real(Path::new(device_path));
    match enc {
        EncryptionType::FileVault2 => unlock_filevault(device_path, password),
        EncryptionType::BitLocker => unlock_bitlocker(device_path, password),
        EncryptionType::Luks1 | EncryptionType::Luks2 => unlock_luks(device_path, password),
        _ => Err(format!("Unlock not supported for {:?}", enc)),
    }
}

#[cfg(target_os = "macos")]
fn unlock_filevault(_device_path: &str, _password: &str) -> Result<String, String> {
    Err(
        "FileVault unlock is disabled because passing passphrases to native tooling from the app \
         is not safe enough for production recovery workflows."
            .into(),
    )
}

#[cfg(not(target_os = "macos"))]
fn unlock_filevault(_device_path: &str, _password: &str) -> Result<String, String> {
    Err("FileVault unlock requires macOS.".into())
}

#[cfg(target_os = "windows")]
fn unlock_bitlocker(_device_path: &str, _password: &str) -> Result<String, String> {
    Err(
        "BitLocker unlock is disabled because passing passphrases to native tooling from the app \
         is not safe enough for production recovery workflows."
            .into(),
    )
}

#[cfg(not(target_os = "windows"))]
fn unlock_bitlocker(_device_path: &str, _password: &str) -> Result<String, String> {
    Err("BitLocker unlock requires Windows.".into())
}

#[cfg(target_os = "linux")]
fn unlock_luks(device_path: &str, password: &str) -> Result<String, String> {
    use std::io::Write;
    let mut child = std::process::Command::new("cryptsetup")
        .args(["luksOpen", device_path, "recupere-unlocked", "--key-file=-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("cryptsetup failed to spawn: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(password.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok("LUKS volume unlocked at /dev/mapper/recupere-unlocked".into())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(not(target_os = "linux"))]
fn unlock_luks(_device_path: &str, _password: &str) -> Result<String, String> {
    Err("LUKS unlock requires Linux with cryptsetup installed.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p =
            std::env::temp_dir().join(format!("recupere-enc-test-{}-{}", std::process::id(), name));
        let mut f = File::create(&p).unwrap();
        f.write_all(bytes).unwrap();
        p
    }

    #[test]
    fn detects_luks1() {
        let mut hdr = vec![0u8; 1024];
        hdr[..6].copy_from_slice(b"LUKS\xba\xbe");
        hdr[6] = 0;
        hdr[7] = 1; // version 1
        let p = write_temp("luks1", &hdr);
        assert_eq!(detect_encryption_real(&p), EncryptionType::Luks1);
    }

    #[test]
    fn detects_luks2() {
        let mut hdr = vec![0u8; 1024];
        hdr[..6].copy_from_slice(b"LUKS\xba\xbe");
        hdr[6] = 0;
        hdr[7] = 2;
        let p = write_temp("luks2", &hdr);
        assert_eq!(detect_encryption_real(&p), EncryptionType::Luks2);
    }

    #[test]
    fn detects_bitlocker() {
        let mut hdr = vec![0u8; 1024];
        hdr[3..11].copy_from_slice(b"-FVE-FS-");
        let p = write_temp("bitlocker", &hdr);
        assert_eq!(detect_encryption_real(&p), EncryptionType::BitLocker);
    }

    #[test]
    fn detects_unknown_for_random_bytes() {
        let p = write_temp("random", &vec![0xAAu8; 1024]);
        assert_eq!(detect_encryption_real(&p), EncryptionType::Unknown);
    }
}
