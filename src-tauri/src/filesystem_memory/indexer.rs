// ============================================================================
// Récupère — Filesystem memory layer: indexer
// ============================================================================
// Synchronous, read-only walker that turns a selected path into a
// `FilesystemSnapshot`. Designed for mounted-volume roots and regular folders
// (e.g. `~/Pictures`, a project directory, an imported image's mount point).
//
// Safety invariants:
//   - Hard read-only: the walker only calls `fs::read_dir`, `fs::metadata` and
//     `fs::File::open` in read mode. It never mutates directory entries.
//   - Bounded recursion: `CaptureOptions.max_depth` caps how deep we walk so
//     a symlink loop or a very deep hierarchy cannot hang the scanner.
//   - Bounded hashing: when hashing is enabled we only read the first and last
//     64 KiB so we can still fingerprint very large files without loading them
//     fully. The hash is a partial SHA-256 — not a cryptographic proof of
//     identity, but enough to match movements and renames.
// ============================================================================

use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::types::{FilesystemSnapshot, IndexedFileRecord, SnapshotStatus};

const DEFAULT_MAX_DEPTH: u32 = 8;
const HASH_WINDOW_BYTES: usize = 64 * 1024;

/// Caller-tunable options for `capture_snapshot`. All fields have sensible
/// defaults — `CaptureOptions::default()` is the recommended entry point.
#[derive(Debug, Clone)]
pub struct CaptureOptions {
    pub max_depth: u32,
    pub compute_hash_prefix: bool,
    pub volume_fingerprint: Option<String>,
    pub ignored_dir_names: Vec<String>,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
            compute_hash_prefix: true,
            volume_fingerprint: None,
            ignored_dir_names: vec![
                ".git".into(),
                "node_modules".into(),
                "target".into(),
                ".DS_Store".into(),
            ],
        }
    }
}

/// Walk `target_path` read-only and build a snapshot. Errors encountered on
/// individual entries are collected in `snapshot.errors` so the caller can
/// still use partial results rather than failing wholesale.
pub fn capture_snapshot(
    snapshot_id: &str,
    target_path: &Path,
    options: &CaptureOptions,
) -> Result<FilesystemSnapshot, String> {
    if !target_path.exists() {
        return Err(format!(
            "Filesystem memory: target path {} does not exist.",
            target_path.to_string_lossy()
        ));
    }
    if !target_path.is_dir() {
        return Err(format!(
            "Filesystem memory: target path {} is not a directory.",
            target_path.to_string_lossy()
        ));
    }

    let captured_at_ms = unix_timestamp_ms();

    let mut records: Vec<IndexedFileRecord> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut total_size_bytes: u64 = 0;
    let snapshot_volume_fingerprint = resolve_volume_fingerprint(target_path, options, &mut errors);

    walk(
        target_path,
        target_path,
        0,
        options,
        &snapshot_volume_fingerprint,
        &mut records,
        &mut errors,
        &mut total_size_bytes,
    );

    let status = if errors.is_empty() {
        SnapshotStatus::Completed
    } else {
        SnapshotStatus::Partial
    };

    Ok(FilesystemSnapshot {
        id: snapshot_id.to_string(),
        target_path: target_path.to_string_lossy().to_string(),
        captured_at_ms,
        status,
        files_indexed: records.len() as u64,
        total_size_bytes,
        errors,
        volume_fingerprint: snapshot_volume_fingerprint,
        records,
    })
}

#[allow(clippy::too_many_arguments)]
fn walk(
    root: &Path,
    current: &Path,
    depth: u32,
    options: &CaptureOptions,
    snapshot_volume_fingerprint: &Option<String>,
    records: &mut Vec<IndexedFileRecord>,
    errors: &mut Vec<String>,
    total_size_bytes: &mut u64,
) {
    if depth > options.max_depth {
        return;
    }

    let entries = match fs::read_dir(current) {
        Ok(entries) => entries,
        Err(error) => {
            errors.push(format!(
                "Unable to read directory {}: {error}",
                current.to_string_lossy()
            ));
            return;
        }
    };

    for entry_result in entries {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(error) => {
                errors.push(format!(
                    "Unable to read a directory entry under {}: {error}",
                    current.to_string_lossy()
                ));
                continue;
            }
        };

        let path = entry.path();
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) => {
                errors.push(format!(
                    "Unable to stat {}: {error}",
                    path.to_string_lossy()
                ));
                continue;
            }
        };

        if metadata.file_type().is_symlink() {
            // Record a symlink as its own entry but never follow it — the
            // indexer would otherwise be susceptible to unbounded loops on
            // pathologically configured volumes.
            continue;
        }

        if metadata.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if options
                .ignored_dir_names
                .iter()
                .any(|ignored| ignored == name)
            {
                continue;
            }
            walk(
                root,
                &path,
                depth + 1,
                options,
                snapshot_volume_fingerprint,
                records,
                errors,
                total_size_bytes,
            );
            continue;
        }

        if !metadata.is_file() {
            continue;
        }

        let size_bytes = metadata.len();
        let modified_at_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64);

        let relative_path = path
            .strip_prefix(root)
            .ok()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());

        let hash_prefix = if options.compute_hash_prefix {
            match compute_partial_hash(&path, size_bytes) {
                Ok(hash) => Some(hash),
                Err(error) => {
                    errors.push(format!(
                        "Unable to hash {}: {error}",
                        path.to_string_lossy()
                    ));
                    None
                }
            }
        } else {
            None
        };

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        records.push(IndexedFileRecord {
            absolute_path: path.to_string_lossy().to_string(),
            relative_path,
            name,
            extension,
            size_bytes,
            modified_at_ms,
            hash_prefix,
            volume_fingerprint: snapshot_volume_fingerprint.clone(),
        });
        *total_size_bytes = total_size_bytes.saturating_add(size_bytes);
    }
}

fn resolve_volume_fingerprint(
    target_path: &Path,
    options: &CaptureOptions,
    errors: &mut Vec<String>,
) -> Option<String> {
    if let Some(explicit) = options.volume_fingerprint.as_ref() {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    match detect_volume_fingerprint(target_path) {
        Ok(fingerprint) => Some(fingerprint),
        Err(error) => {
            errors.push(format!(
                "Unable to determine a stable volume identity for {}: {error}",
                target_path.to_string_lossy()
            ));
            None
        }
    }
}

fn detect_volume_fingerprint(target_path: &Path) -> Result<String, String> {
    detect_platform_volume_fingerprint(target_path)
}

#[cfg(unix)]
fn detect_platform_volume_fingerprint(target_path: &Path) -> Result<String, String> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::metadata(target_path).map_err(|error| {
        format!("stat failed while resolving the source volume identity: {error}")
    })?;
    Ok(format!("unix-dev:{:x}", metadata.dev()))
}

#[cfg(windows)]
fn detect_platform_volume_fingerprint(target_path: &Path) -> Result<String, String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    type Bool = i32;
    type Dword = u32;

    extern "system" {
        fn GetVolumeInformationW(
            lpRootPathName: *const u16,
            lpVolumeNameBuffer: *mut u16,
            nVolumeNameSize: Dword,
            lpVolumeSerialNumber: *mut Dword,
            lpMaximumComponentLength: *mut Dword,
            lpFileSystemFlags: *mut Dword,
            lpFileSystemNameBuffer: *mut u16,
            nFileSystemNameSize: Dword,
        ) -> Bool;
    }

    let root = target_path
        .ancestors()
        .last()
        .ok_or_else(|| "unable to resolve a Windows volume root".to_string())?;
    let mut root_wide: Vec<u16> = OsStr::new(&root.to_string_lossy().to_string())
        .encode_wide()
        .collect();
    if !matches!(root_wide.last(), Some(ch) if *ch == b'\\' as u16) {
        root_wide.push(b'\\' as u16);
    }
    root_wide.push(0);

    let mut serial_number: Dword = 0;
    // SAFETY: `root_wide` is a valid, NUL-terminated UTF-16 buffer that lives
    // for the duration of the call, and every output pointer is either null or
    // points to a properly aligned local variable.
    let ok = unsafe {
        GetVolumeInformationW(
            root_wide.as_ptr(),
            std::ptr::null_mut(),
            0,
            &mut serial_number,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };

    if ok == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }

    Ok(format!("win-vol:{serial_number:08x}"))
}

#[cfg(not(any(unix, windows)))]
fn detect_platform_volume_fingerprint(target_path: &Path) -> Result<String, String> {
    let canonical = fs::canonicalize(target_path).map_err(|error| {
        format!("canonicalize failed while resolving the source volume identity: {error}")
    })?;
    Ok(format!("path-root:{}", canonical.to_string_lossy()))
}

fn compute_partial_hash(path: &Path, size: u64) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("open failed: {error}"))?;
    let mut hasher = Sha256::new();

    let window = HASH_WINDOW_BYTES as u64;
    if size <= window * 2 {
        // Small file — hash the whole content.
        let mut buffer = vec![0u8; size as usize];
        file.read_exact(&mut buffer)
            .map_err(|error| format!("read failed: {error}"))?;
        hasher.update(&buffer);
    } else {
        let mut head = vec![0u8; HASH_WINDOW_BYTES];
        file.read_exact(&mut head)
            .map_err(|error| format!("head read failed: {error}"))?;
        hasher.update(&head);

        file.seek(SeekFrom::End(-(HASH_WINDOW_BYTES as i64)))
            .map_err(|error| format!("seek failed: {error}"))?;
        let mut tail = vec![0u8; HASH_WINDOW_BYTES];
        file.read_exact(&mut tail)
            .map_err(|error| format!("tail read failed: {error}"))?;
        hasher.update(&tail);

        hasher.update(size.to_le_bytes());
    }

    let digest = hasher.finalize();
    Ok(hex_encode(&digest))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch_dir(prefix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "recupere-fsmem-idx-{prefix}-{}-{}",
            std::process::id(),
            unix_timestamp_ms()
        ));
        fs::create_dir_all(&dir).expect("scratch dir should be created");
        dir
    }

    fn write_file(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent dir should be created");
        }
        let mut file = File::create(path).expect("file should be created");
        file.write_all(bytes).expect("file write should succeed");
    }

    #[test]
    fn capture_snapshot_indexes_files_with_hashes() {
        let dir = scratch_dir("capture-basic");
        write_file(&dir.join("a.txt"), b"hello");
        write_file(&dir.join("sub/b.txt"), b"world");

        let snapshot = capture_snapshot("s1", &dir, &CaptureOptions::default())
            .expect("capture should succeed");

        assert_eq!(snapshot.status, SnapshotStatus::Completed);
        assert_eq!(snapshot.files_indexed, 2);
        assert!(snapshot.total_size_bytes >= 10);
        assert!(snapshot.volume_fingerprint.is_some());
        assert!(snapshot.records.iter().all(|r| r.hash_prefix.is_some()));
        assert!(snapshot
            .records
            .iter()
            .all(|record| record.volume_fingerprint == snapshot.volume_fingerprint));
        let paths: Vec<String> = snapshot
            .records
            .iter()
            .map(|r| r.relative_path.clone())
            .collect();
        assert!(paths.iter().any(|p| p == "a.txt"));
        assert!(paths.iter().any(|p| p.ends_with("b.txt")));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn capture_snapshot_respects_ignored_dirs() {
        let dir = scratch_dir("capture-ignored");
        write_file(&dir.join("keep.txt"), b"keep");
        write_file(&dir.join("node_modules/dep.txt"), b"dep");
        write_file(&dir.join(".git/head"), b"head");

        let snapshot = capture_snapshot("s1", &dir, &CaptureOptions::default())
            .expect("capture should succeed");

        let paths: Vec<String> = snapshot
            .records
            .iter()
            .map(|r| r.relative_path.clone())
            .collect();
        assert!(paths.iter().any(|p| p == "keep.txt"));
        assert!(!paths.iter().any(|p| p.contains("node_modules")));
        assert!(!paths.iter().any(|p| p.contains(".git")));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn capture_snapshot_skips_large_file_when_hashing_disabled_but_still_records_metadata() {
        let dir = scratch_dir("capture-nohash");
        let path = dir.join("big.bin");
        // ~200 KiB — larger than two hash windows so the "full content" branch
        // would be avoided in the default implementation.
        write_file(&path, &vec![7u8; 200_000]);

        let options = CaptureOptions {
            compute_hash_prefix: false,
            ..CaptureOptions::default()
        };
        let snapshot = capture_snapshot("s1", &dir, &options).expect("capture should succeed");

        assert_eq!(snapshot.files_indexed, 1);
        assert!(snapshot.records[0].hash_prefix.is_none());
        assert_eq!(snapshot.records[0].size_bytes, 200_000);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn capture_snapshot_returns_error_on_missing_path() {
        let missing = std::env::temp_dir().join("recupere-fsmem-does-not-exist-xyz");
        let result = capture_snapshot("s1", &missing, &CaptureOptions::default());
        assert!(result.is_err());
    }
}
