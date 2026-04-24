// ============================================================================
// Récupère — Filesystem memory layer (Chantier 82)
// ============================================================================
// Persistent, user-initiated snapshots of selected paths / volumes. Used to
// surface "missing files" insights without ever promising that erased data
// can be magically reconstructed.
//
// Safety invariants (see AGENTS.md):
//   - Read-only on the source: `indexer::capture_snapshot` never writes under
//     the selected path, only reads metadata + small prefix/suffix bytes for
//     hashing.
//   - Local-only: snapshots are persisted via `store::persist_snapshot` which
//     refuses to write if the store directory resolves inside the source.
//   - Honest confidence: every diff classification ships a `Confidence` so
//     the UI can always show "certain" vs "estimated".
// ============================================================================

pub mod diff;
pub mod indexer;
pub mod scheduler;
pub mod store;
pub mod types;

pub use diff::{compute_diff, missing_file_insights, recovery_context_hints_for};
pub use indexer::{capture_snapshot, CaptureOptions};
pub use store::{
    assert_store_not_inside_source, load_all_snapshots, load_monitoring_policy, local_store_dir,
    persist_snapshot, policy_file_path, rotate_snapshots, save_monitoring_policy, store_file_path,
};
pub use types::{
    Confidence, DiffChange, DiffChangeKind, FilesystemSnapshot, IndexedFileRecord,
    MissingFileInsight, MonitoringPolicy, RecoveryContextHint, RecoveryContextLookupInput,
    RecoveryContextMatchStrategy, SnapshotDiff, SnapshotStatus,
};
