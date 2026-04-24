// ============================================================================
// Récupère — Filesystem memory layer: shared types
// ============================================================================
// Snapshot index of selected paths / volumes. These types are serialised via
// serde (for the JSON-lines store and the Tauri IPC contract) and mirror the
// TypeScript contracts in `src/types/filesystemMemory.ts`.
//
// SAFETY: nothing here writes to the source. The store is local-only, and its
// destination is guarded by `store::local_store_dir()` which never points at a
// scanned source volume.
// ============================================================================

use serde::{Deserialize, Serialize};

/// One file entry captured in a snapshot. `hash_prefix` is a partial fingerprint
/// (first + last 64 KiB SHA-256) used for move/rename detection without having
/// to hash multi-gigabyte files. It is deliberately not a cryptographic proof.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IndexedFileRecord {
    pub absolute_path: String,
    pub relative_path: String,
    pub name: String,
    pub extension: String,
    pub size_bytes: u64,
    pub modified_at_ms: Option<i64>,
    pub hash_prefix: Option<String>,
    pub volume_fingerprint: Option<String>,
}

/// Execution state of a snapshot capture.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SnapshotStatus {
    Running,
    Completed,
    Failed,
    Partial,
}

/// One snapshot — the full set of `IndexedFileRecord`s captured under a
/// selected root path at a given point in time, plus a short status summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemSnapshot {
    pub id: String,
    pub target_path: String,
    pub captured_at_ms: u64,
    pub status: SnapshotStatus,
    pub files_indexed: u64,
    pub total_size_bytes: u64,
    pub errors: Vec<String>,
    pub volume_fingerprint: Option<String>,
    pub records: Vec<IndexedFileRecord>,
}

/// Confidence level surfaced next to a diff classification. Always shown to the
/// user — the product never claims certainty when the heuristic is ambiguous.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

/// Kind of change observed between two snapshots.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiffChangeKind {
    New,
    Missing,
    Moved,
    Renamed,
    Modified,
    Ambiguous,
}

/// One change in a diff between two snapshots. `from` and `to` describe the old
/// and new `IndexedFileRecord` when applicable; for "new" only `to` is set, for
/// "missing" only `from`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffChange {
    pub kind: DiffChangeKind,
    pub confidence: Confidence,
    pub reason: String,
    pub from: Option<IndexedFileRecord>,
    pub to: Option<IndexedFileRecord>,
}

/// Aggregate diff between two snapshots. `baseline_id` is the older snapshot
/// and `head_id` the newer one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDiff {
    pub baseline_id: String,
    pub head_id: String,
    pub target_path: String,
    pub computed_at_ms: u64,
    pub changes: Vec<DiffChange>,
}

/// Reader-friendly projection of the files that disappeared between two
/// snapshots. Drives the novice "Missing files" view.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MissingFileInsight {
    pub name: String,
    pub last_known_path: String,
    pub last_observed_at_ms: u64,
    pub first_missing_observed_at_ms: u64,
    pub file_modified_at_ms: Option<i64>,
    pub size_bytes: u64,
    pub extension: String,
    pub confidence: Confidence,
    pub recovery_hint: String,
}

/// Optional annotation attached to an existing `RecoveredFile` so downstream
/// scoring / UI layers can weight recovery attempts by how recently the file
/// was present.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryContextHint {
    pub file_id: String,
    pub file_name: String,
    pub last_known_path: String,
    pub last_observed_at_ms: u64,
    pub first_missing_observed_at_ms: Option<u64>,
    pub file_modified_at_ms: Option<i64>,
    pub confidence: Confidence,
    pub matched_by: RecoveryContextMatchStrategy,
}

/// Lookup payload used when the UI wants to enrich current recovery results
/// with filesystem-memory context. The `file_id` is echoed back so the frontend
/// can attach the hint without relying on unstable positional ordering.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryContextLookupInput {
    pub file_id: String,
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
}

/// Strategy that produced a `RecoveryContextHint`. Surfaced to expert users so
/// the product stays explicit about whether the match came from an exact path
/// or from a weaker unique `(name, size)` correspondence.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryContextMatchStrategy {
    Path,
    NameSize,
}

/// Monitoring policy the user opts into when they ask for repeated snapshots.
/// "Manual" is the default; "scheduled" takes a minimum interval to avoid CPU
/// thrash; "realtime" is explicitly deferred to a later tranche.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "mode")]
pub enum MonitoringPolicy {
    Manual,
    Scheduled {
        interval_minutes: u32,
    },
    #[serde(rename = "realtime")]
    RealtimeDeferred,
}

impl MonitoringPolicy {
    /// Minimum interval the scheduler is allowed to use. Values below this
    /// threshold are clamped upward so we never spin a tight polling loop that
    /// would hammer the source filesystem.
    pub const MIN_INTERVAL_MINUTES: u32 = 15;

    pub fn normalize(self) -> Self {
        match self {
            MonitoringPolicy::Scheduled { interval_minutes } => MonitoringPolicy::Scheduled {
                interval_minutes: interval_minutes.max(Self::MIN_INTERVAL_MINUTES),
            },
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_snapshot_preserves_records_and_status() {
        let snapshot = FilesystemSnapshot {
            id: "snap-1".into(),
            target_path: "/Users/demo/Pictures".into(),
            captured_at_ms: 1_700_000_000_000,
            status: SnapshotStatus::Completed,
            files_indexed: 2,
            total_size_bytes: 2048,
            errors: vec![],
            volume_fingerprint: Some("vol-abc".into()),
            records: vec![
                IndexedFileRecord {
                    absolute_path: "/Users/demo/Pictures/a.jpg".into(),
                    relative_path: "a.jpg".into(),
                    name: "a.jpg".into(),
                    extension: "jpg".into(),
                    size_bytes: 1024,
                    modified_at_ms: Some(1_700_000_000_000),
                    hash_prefix: Some("abc123".into()),
                    volume_fingerprint: Some("vol-abc".into()),
                },
                IndexedFileRecord {
                    absolute_path: "/Users/demo/Pictures/b.png".into(),
                    relative_path: "b.png".into(),
                    name: "b.png".into(),
                    extension: "png".into(),
                    size_bytes: 1024,
                    modified_at_ms: None,
                    hash_prefix: None,
                    volume_fingerprint: Some("vol-abc".into()),
                },
            ],
        };

        let json = serde_json::to_string(&snapshot).expect("snapshot should serialize");
        let decoded: FilesystemSnapshot =
            serde_json::from_str(&json).expect("snapshot should deserialize");
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn monitoring_policy_floors_scheduled_interval_to_safe_minimum() {
        let clamped = MonitoringPolicy::Scheduled {
            interval_minutes: 2,
        }
        .normalize();
        assert_eq!(
            clamped,
            MonitoringPolicy::Scheduled {
                interval_minutes: MonitoringPolicy::MIN_INTERVAL_MINUTES
            }
        );

        let preserved = MonitoringPolicy::Scheduled {
            interval_minutes: 60,
        }
        .normalize();
        assert_eq!(
            preserved,
            MonitoringPolicy::Scheduled {
                interval_minutes: 60
            }
        );

        let manual = MonitoringPolicy::Manual.normalize();
        assert_eq!(manual, MonitoringPolicy::Manual);
    }
}
