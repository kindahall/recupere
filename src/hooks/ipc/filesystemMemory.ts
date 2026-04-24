import { invoke } from '@tauri-apps/api/core';
import type {
  FilesystemSnapshot,
  MissingFileInsight,
  MonitoringPolicy,
  RecoveryContextHint,
  RecoveryContextLookupInput,
  SnapshotDiff,
} from '../../types';

export async function createFilesystemMemorySnapshot(
  targetPath: string,
  volumeFingerprint?: string,
): Promise<FilesystemSnapshot> {
  return invoke<FilesystemSnapshot>('create_filesystem_memory_snapshot', {
    targetPath,
    volumeFingerprint: volumeFingerprint ?? null,
  });
}

export async function listFilesystemMemorySnapshots(): Promise<FilesystemSnapshot[]> {
  return invoke<FilesystemSnapshot[]>('list_filesystem_memory_snapshots');
}

export async function computeFilesystemMemoryDiff(
  baselineId: string,
  headId: string,
): Promise<SnapshotDiff> {
  return invoke<SnapshotDiff>('compute_filesystem_memory_diff', {
    baselineId,
    headId,
  });
}

export async function fetchMissingFiles(
  baselineId: string,
  headId: string,
): Promise<MissingFileInsight[]> {
  return invoke<MissingFileInsight[]>('get_filesystem_memory_missing_files', {
    baselineId,
    headId,
  });
}

export async function fetchRecoveryContextHints(
  baselineId: string,
  headId: string,
  recoveredFiles: RecoveryContextLookupInput[],
): Promise<RecoveryContextHint[]> {
  return invoke<RecoveryContextHint[]>('get_recovery_context_hints', {
    baselineId,
    headId,
    recoveredFiles,
  });
}

export async function fetchFilesystemMemoryPolicy(): Promise<MonitoringPolicy> {
  return invoke<MonitoringPolicy>('get_filesystem_memory_policy');
}

export async function saveFilesystemMemoryPolicy(
  policy: MonitoringPolicy,
): Promise<MonitoringPolicy> {
  return invoke<MonitoringPolicy>('save_filesystem_memory_policy', { policy });
}
