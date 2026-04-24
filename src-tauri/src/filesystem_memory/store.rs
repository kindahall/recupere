// ============================================================================
// Récupère — Filesystem memory layer: local store
// ============================================================================
// Append-only JSON-lines store of `FilesystemSnapshot`s. Kept deliberately
// simple — no SQLite dependency, no migration layer, no external daemon. The
// file lives under the OS-specific user data directory (never inside a scanned
// source volume) and is rewritten atomically when snapshots are rotated out.
//
// SAFETY invariants:
//   - The store directory is resolved via `dirs::data_local_dir()` or the
//     `RECUPERE_FILESYSTEM_MEMORY_DIR` override (tests only). It is validated
//     to never be a descendant of any "source path" passed in by the caller.
//   - Writes are atomic: we write to `*.tmp` then rename, so a crash mid-write
//     cannot leave the store half-populated.
// ============================================================================

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use super::types::{FilesystemSnapshot, MonitoringPolicy, SnapshotStatus};

const STORE_FILE_NAME: &str = "filesystem_memory.jsonl";
const POLICY_FILE_NAME: &str = "monitoring_policy.json";
const STORE_ENV_OVERRIDE: &str = "RECUPERE_FILESYSTEM_MEMORY_DIR";

/// Default retention policy (enforced by `rotate_snapshots`): we keep at most
/// the 10 most recent snapshots per target path. Beyond that, older snapshots
/// are dropped so the store never grows unbounded across long-running sessions.
pub const MAX_SNAPSHOTS_PER_TARGET: usize = 10;

/// Resolve the directory where the snapshot store should live. The `env`
/// override is intended for integration tests / developer overrides;
/// production always lands under the user's local data directory.
pub fn local_store_dir() -> Result<PathBuf, String> {
    if let Some(override_path) = std::env::var_os(STORE_ENV_OVERRIDE) {
        let path = PathBuf::from(override_path);
        ensure_dir(&path)?;
        return Ok(path);
    }

    let base = dirs::data_local_dir().ok_or_else(|| {
        "Unable to resolve the local data directory for the filesystem-memory store.".to_string()
    })?;
    let path = base.join("recupere").join("filesystem_memory");
    ensure_dir(&path)?;
    Ok(path)
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| {
        format!(
            "Unable to create the filesystem-memory store directory {}: {}",
            path.to_string_lossy(),
            error
        )
    })
}

/// Returns the absolute path to the JSON-lines store file. Guaranteed to live
/// inside `local_store_dir()`.
pub fn store_file_path() -> Result<PathBuf, String> {
    Ok(local_store_dir()?.join(STORE_FILE_NAME))
}

/// Returns the absolute path to the persisted monitoring policy file. Guaranteed
/// to live inside `local_store_dir()` (never on a scanned source volume).
pub fn policy_file_path() -> Result<PathBuf, String> {
    Ok(local_store_dir()?.join(POLICY_FILE_NAME))
}

/// Load the persisted monitoring policy. Returns `Manual` when the file does
/// not exist yet, so a fresh install defaults to opt-in manual snapshots.
pub fn load_monitoring_policy() -> Result<MonitoringPolicy, String> {
    load_monitoring_policy_from(&local_store_dir()?)
}

/// Testable variant of `load_monitoring_policy`.
pub fn load_monitoring_policy_from(dir: &Path) -> Result<MonitoringPolicy, String> {
    ensure_dir(dir)?;
    let path = dir.join(POLICY_FILE_NAME);
    if !path.exists() {
        return Ok(MonitoringPolicy::Manual);
    }
    let bytes = fs::read(&path).map_err(|error| {
        format!(
            "Unable to read the filesystem-memory monitoring policy at {}: {error}",
            path.to_string_lossy()
        )
    })?;
    let policy: MonitoringPolicy = serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "Unable to deserialize the filesystem-memory monitoring policy at {}: {error}",
            path.to_string_lossy()
        )
    })?;
    Ok(policy.normalize())
}

/// Persist the monitoring policy. Writes are atomic (`*.tmp` + rename) and the
/// policy is always run through `.normalize()` first so the minimum-interval
/// guard is applied on disk and not only in memory.
pub fn save_monitoring_policy(policy: &MonitoringPolicy) -> Result<(), String> {
    save_monitoring_policy_in(&local_store_dir()?, policy)
}

/// Testable variant of `save_monitoring_policy`.
pub fn save_monitoring_policy_in(dir: &Path, policy: &MonitoringPolicy) -> Result<(), String> {
    ensure_dir(dir)?;
    let normalized = policy.clone().normalize();
    let serialized = serde_json::to_vec_pretty(&normalized).map_err(|error| {
        format!("Unable to serialize the filesystem-memory monitoring policy: {error}")
    })?;
    let final_path = dir.join(POLICY_FILE_NAME);
    let tmp_path = final_path.with_extension("json.tmp");
    fs::write(&tmp_path, &serialized).map_err(|error| {
        format!(
            "Unable to write the filesystem-memory monitoring policy temp file {}: {error}",
            tmp_path.to_string_lossy()
        )
    })?;
    fs::rename(&tmp_path, &final_path).map_err(|error| {
        format!(
            "Unable to finalize the filesystem-memory monitoring policy at {}: {error}",
            final_path.to_string_lossy()
        )
    })?;
    Ok(())
}

/// Guard used by `persist_snapshot` and callers of `local_store_dir`: refuses
/// to persist if the store directory would be inside the user-selected source
/// path. Enforces AGENTS.md rule "never write on the source disk".
pub fn assert_store_not_inside_source(source_path: &Path) -> Result<(), String> {
    let store_dir = local_store_dir()?;
    assert_store_dir_not_inside_source(&store_dir, source_path)
}

fn assert_store_dir_not_inside_source(store_dir: &Path, source_path: &Path) -> Result<(), String> {
    let canonical_store = fs::canonicalize(store_dir).unwrap_or_else(|_| store_dir.to_path_buf());
    let canonical_source =
        fs::canonicalize(source_path).unwrap_or_else(|_| source_path.to_path_buf());

    if canonical_store.starts_with(&canonical_source) {
        return Err(format!(
            "Refusing to persist the filesystem-memory store under the scanned source path {}. \
             Move the Récupère data directory outside the source volume before retrying.",
            canonical_source.to_string_lossy()
        ));
    }
    Ok(())
}

/// Load every snapshot currently persisted in the store. Returns an empty list
/// if the store file does not exist yet.
pub fn load_all_snapshots() -> Result<Vec<FilesystemSnapshot>, String> {
    load_all_snapshots_from(&local_store_dir()?)
}

/// Testable variant that reads from an explicit directory. Tests use this so
/// they never touch the shared `RECUPERE_FILESYSTEM_MEMORY_DIR` env var.
pub fn load_all_snapshots_from(dir: &Path) -> Result<Vec<FilesystemSnapshot>, String> {
    ensure_dir(dir)?;
    let path = dir.join(STORE_FILE_NAME);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = fs::File::open(&path).map_err(|error| {
        format!(
            "Unable to open the filesystem-memory store at {}: {error}",
            path.to_string_lossy()
        )
    })?;
    let reader = BufReader::new(file);

    let mut snapshots = Vec::new();
    for (line_number, line_result) in reader.lines().enumerate() {
        let line = line_result.map_err(|error| {
            format!(
                "Unable to read the filesystem-memory store at {} (line {}): {error}",
                path.to_string_lossy(),
                line_number + 1
            )
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let snapshot: FilesystemSnapshot = serde_json::from_str(trimmed).map_err(|error| {
            format!(
                "Unable to deserialize snapshot from {} (line {}): {error}",
                path.to_string_lossy(),
                line_number + 1
            )
        })?;
        snapshots.push(snapshot);
    }
    Ok(snapshots)
}

/// Persist a snapshot. Appends to the JSON-lines file unless the snapshot was
/// a "running" placeholder already persisted — in which case we rewrite the
/// store to replace that entry with the final one so we never leave dangling
/// "running" records.
pub fn persist_snapshot(snapshot: &FilesystemSnapshot) -> Result<(), String> {
    persist_snapshot_in(&local_store_dir()?, snapshot)
}

/// Testable variant that writes to an explicit directory.
pub fn persist_snapshot_in(dir: &Path, snapshot: &FilesystemSnapshot) -> Result<(), String> {
    ensure_dir(dir)?;
    let mut snapshots = load_all_snapshots_from(dir)?;
    if let Some(existing) = snapshots.iter_mut().find(|s| s.id == snapshot.id) {
        *existing = snapshot.clone();
    } else {
        snapshots.push(snapshot.clone());
    }
    rewrite_store_in(dir, &snapshots)
}

/// Drop snapshots older than the retention budget. Executed at the tail of
/// every successful `persist_snapshot` from B2.
pub fn rotate_snapshots(target_path: &str) -> Result<(), String> {
    rotate_snapshots_in(&local_store_dir()?, target_path)
}

/// Testable variant of `rotate_snapshots`.
pub fn rotate_snapshots_in(dir: &Path, target_path: &str) -> Result<(), String> {
    ensure_dir(dir)?;
    let mut snapshots = load_all_snapshots_from(dir)?;
    snapshots.sort_by_key(|s| s.captured_at_ms);

    let mut to_keep_ids: Vec<String> = snapshots
        .iter()
        .filter(|s| s.target_path == target_path && s.status != SnapshotStatus::Running)
        .rev()
        .take(MAX_SNAPSHOTS_PER_TARGET)
        .map(|s| s.id.clone())
        .collect();
    to_keep_ids.extend(
        snapshots
            .iter()
            .filter(|s| s.target_path == target_path && s.status == SnapshotStatus::Running)
            .map(|s| s.id.clone()),
    );

    let filtered: Vec<FilesystemSnapshot> = snapshots
        .into_iter()
        .filter(|s| s.target_path != target_path || to_keep_ids.contains(&s.id))
        .collect();

    rewrite_store_in(dir, &filtered)
}

fn rewrite_store_in(dir: &Path, snapshots: &[FilesystemSnapshot]) -> Result<(), String> {
    let store_path = dir.join(STORE_FILE_NAME);
    let tmp_path = store_path.with_extension("jsonl.tmp");

    let mut serialized = String::new();
    for snapshot in snapshots {
        let line = serde_json::to_string(snapshot).map_err(|error| {
            format!("Unable to serialize a snapshot before writing the store: {error}")
        })?;
        serialized.push_str(&line);
        serialized.push('\n');
    }

    fs::write(&tmp_path, serialized.as_bytes()).map_err(|error| {
        format!(
            "Unable to write the filesystem-memory store temp file {}: {error}",
            tmp_path.to_string_lossy()
        )
    })?;
    fs::rename(&tmp_path, &store_path).map_err(|error| {
        format!(
            "Unable to finalize the filesystem-memory store at {}: {error}",
            store_path.to_string_lossy()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::types::{
        FilesystemSnapshot, IndexedFileRecord, MonitoringPolicy, SnapshotStatus,
    };
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_store_dir(prefix: &str) -> PathBuf {
        let counter = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "recupere-fsmem-{prefix}-{}-{counter}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&dir).expect("temp store dir should be created");
        dir
    }

    fn fake_snapshot(id: &str, target: &str, captured_at_ms: u64) -> FilesystemSnapshot {
        FilesystemSnapshot {
            id: id.into(),
            target_path: target.into(),
            captured_at_ms,
            status: SnapshotStatus::Completed,
            files_indexed: 1,
            total_size_bytes: 10,
            errors: vec![],
            volume_fingerprint: Some("vol".into()),
            records: vec![IndexedFileRecord {
                absolute_path: format!("{target}/a.txt"),
                relative_path: "a.txt".into(),
                name: "a.txt".into(),
                extension: "txt".into(),
                size_bytes: 10,
                modified_at_ms: Some(captured_at_ms as i64),
                hash_prefix: None,
                volume_fingerprint: Some("vol".into()),
            }],
        }
    }

    #[test]
    fn persist_and_load_round_trip() {
        let dir = temp_store_dir("persist-load");

        let snap = fake_snapshot("s1", "/src", 100);
        persist_snapshot_in(&dir, &snap).expect("snapshot should persist");
        let loaded = load_all_snapshots_from(&dir).expect("store should load");

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], snap);
    }

    #[test]
    fn persist_replaces_snapshot_with_same_id() {
        let dir = temp_store_dir("replace");

        let mut snap = fake_snapshot("s1", "/src", 100);
        persist_snapshot_in(&dir, &snap).expect("first persist");
        snap.files_indexed = 99;
        persist_snapshot_in(&dir, &snap).expect("second persist");
        let loaded = load_all_snapshots_from(&dir).expect("store should load");

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].files_indexed, 99);
    }

    #[test]
    fn rotation_trims_history_per_target() {
        let dir = temp_store_dir("rotation");

        for i in 0..(MAX_SNAPSHOTS_PER_TARGET + 3) as u64 {
            persist_snapshot_in(&dir, &fake_snapshot(&format!("s{i}"), "/src", i * 1000))
                .expect("persist should succeed");
        }
        rotate_snapshots_in(&dir, "/src").expect("rotation should succeed");
        let loaded = load_all_snapshots_from(&dir).expect("store should load");

        assert_eq!(loaded.len(), MAX_SNAPSHOTS_PER_TARGET);
        let ids: Vec<String> = loaded.iter().map(|s| s.id.clone()).collect();
        assert!(!ids.contains(&"s0".to_string()));
        assert!(ids.contains(&format!("s{}", MAX_SNAPSHOTS_PER_TARGET + 2)));
    }

    #[test]
    fn guard_flags_a_source_that_contains_the_store() {
        let dir = temp_store_dir("guard");
        let parent = dir
            .parent()
            .expect("store dir should have a parent")
            .to_path_buf();

        let result = assert_store_dir_not_inside_source(&dir, &parent);
        assert!(
            result.is_err(),
            "a source path that contains the store must be rejected"
        );
    }

    #[test]
    fn guard_accepts_a_source_unrelated_to_the_store() {
        let store_dir = temp_store_dir("guard-ok-store");
        let source_dir = temp_store_dir("guard-ok-source");

        let result = assert_store_dir_not_inside_source(&store_dir, &source_dir);
        if let Err(message) = result {
            assert!(
                message.contains("Refusing to persist"),
                "unexpected guard failure: {message}"
            );
        }
    }

    #[test]
    fn load_policy_defaults_to_manual_on_a_fresh_store() {
        let dir = temp_store_dir("policy-default");

        let policy = load_monitoring_policy_from(&dir).expect("loading should succeed");
        assert_eq!(policy, MonitoringPolicy::Manual);
    }

    #[test]
    fn policy_round_trip_preserves_scheduled_interval() {
        let dir = temp_store_dir("policy-round-trip");

        let original = MonitoringPolicy::Scheduled {
            interval_minutes: 60,
        };
        save_monitoring_policy_in(&dir, &original).expect("save should succeed");
        let loaded = load_monitoring_policy_from(&dir).expect("load should succeed");
        assert_eq!(loaded, original);
    }

    #[test]
    fn save_policy_floors_short_intervals_to_minimum() {
        let dir = temp_store_dir("policy-floor");

        let short = MonitoringPolicy::Scheduled {
            interval_minutes: 1,
        };
        save_monitoring_policy_in(&dir, &short).expect("save should succeed");
        let loaded = load_monitoring_policy_from(&dir).expect("load should succeed");
        assert_eq!(
            loaded,
            MonitoringPolicy::Scheduled {
                interval_minutes: MonitoringPolicy::MIN_INTERVAL_MINUTES,
            }
        );
    }
}
