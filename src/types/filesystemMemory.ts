// ============================================================================
// Shared TypeScript Types — Filesystem memory layer (Chantier 82)
// ============================================================================
// These types mirror the Rust contracts in
// `src-tauri/src/filesystem_memory/types.rs` and are serialised with serde's
// `rename_all = "camelCase"`, so the field names here match what the Tauri
// bridge emits without an intermediate transformer.
// ============================================================================

export type SnapshotStatus = 'running' | 'completed' | 'failed' | 'partial';

export type Confidence = 'high' | 'medium' | 'low';

export type DiffChangeKind = 'new' | 'missing' | 'moved' | 'renamed' | 'modified' | 'ambiguous';

export interface IndexedFileRecord {
  absolutePath: string;
  relativePath: string;
  name: string;
  extension: string;
  sizeBytes: number;
  modifiedAtMs: number | null;
  hashPrefix: string | null;
  volumeFingerprint: string | null;
}

export interface FilesystemSnapshot {
  id: string;
  targetPath: string;
  capturedAtMs: number;
  status: SnapshotStatus;
  filesIndexed: number;
  totalSizeBytes: number;
  errors: string[];
  volumeFingerprint: string | null;
  records: IndexedFileRecord[];
}

export interface DiffChange {
  kind: DiffChangeKind;
  confidence: Confidence;
  reason: string;
  from: IndexedFileRecord | null;
  to: IndexedFileRecord | null;
}

export interface SnapshotDiff {
  baselineId: string;
  headId: string;
  targetPath: string;
  computedAtMs: number;
  changes: DiffChange[];
}

export interface MissingFileInsight {
  name: string;
  lastKnownPath: string;
  lastObservedAtMs: number;
  firstMissingObservedAtMs: number;
  fileModifiedAtMs: number | null;
  sizeBytes: number;
  extension: string;
  confidence: Confidence;
  recoveryHint: string;
}

export interface RecoveryContextHint {
  fileId: string;
  fileName: string;
  lastKnownPath: string;
  lastObservedAtMs: number;
  firstMissingObservedAtMs: number | null;
  fileModifiedAtMs: number | null;
  confidence: Confidence;
  matchedBy: 'path' | 'name_size';
}

export interface RecoveryContextLookupInput {
  fileId: string;
  name: string;
  path: string;
  sizeBytes: number;
}

export interface FilesystemMemoryComparison {
  baselineId: string;
  headId: string;
  targetPath: string;
  comparedAtMs: number;
}

// MonitoringPolicy is a serde internally-tagged enum (`tag = "mode"`). The
// Rust side applies `rename_all = "camelCase"` so the `interval_minutes` field
// is serialised as `intervalMinutes`; the `RealtimeDeferred` variant is
// explicitly renamed to `"realtime"` to reflect that realtime monitoring is
// NOT shipped today — the scheduler stays idle under that policy.
export type MonitoringPolicy =
  | { mode: 'manual' }
  | { mode: 'scheduled'; intervalMinutes: number }
  | { mode: 'realtime' };

/// Floor applied by the backend on every `save_filesystem_memory_policy` call.
/// Re-exported here so the UI can block a slider below this value instead of
/// letting the backend silently clamp the request.
export const MIN_MONITORING_INTERVAL_MINUTES = 15;
