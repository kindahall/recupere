import { invoke, isTauri } from '@tauri-apps/api/core';
import { getBrowserPreviewRecoveryResult } from '../../utils/browserPreviewSeed';

export interface RustRecoveredFile {
  id: string;
  name: string;
  path: string;
  extension: string;
  size_bytes: number;
  created_at: string | null;
  modified_at: string | null;
  expected_size_bytes: number | null;
  deleted_at: string | null;
  is_deleted: boolean;
  integrity: string;
  recovery_score: number;
  recovery_method: string;
  preview_available: boolean;
  mime_type: string | null;
  start_offset: number | null;
  clusters: number[] | null;
  resource_fork: {
    size_bytes: number;
    expected_size_bytes: number | null;
  } | null;
  alternate_data_streams: Array<{
    name: string;
    size_bytes: number;
    expected_size_bytes: number | null;
  }> | null;
  compression_kind: string | null;
  source_view: string | null;
  native_auxiliary_kind: string | null;
  snapshot_xid: number | null;
  recovery_complexity: string | null;
  validator_status: string | null;
  assembly_segment_count: number | null;
  gap_count: number | null;
  journal_derived: boolean;
}

export interface RecoveredFileData {
  id: string;
  name: string;
  path: string;
  extension: string;
  sizeBytes: number;
  createdAt?: string;
  modifiedAt?: string;
  expectedSizeBytes?: number;
  deletedAt?: string;
  isDeleted?: boolean;
  integrity: string;
  recoveryScore: number;
  recoveryMethod: string;
  previewAvailable: boolean;
  mimeType?: string;
  startOffset?: number;
  clusters?: number[];
  resourceFork?: {
    sizeBytes: number;
    expectedSizeBytes?: number;
  };
  alternateDataStreams?: Array<{
    name: string;
    sizeBytes: number;
    expectedSizeBytes?: number;
  }>;
  compressionKind?: string;
  sourceView?: string;
  nativeAuxiliaryKind?: string;
  snapshotXid?: number;
  recoveryComplexity?: string;
  validatorStatus?: string;
  assemblySegmentCount?: number;
  gapCount?: number;
  journalDerived?: boolean;
}

export function mapRecoveredFile(f: RustRecoveredFile): RecoveredFileData {
  return {
    id: f.id,
    name: f.name,
    path: f.path,
    extension: f.extension,
    createdAt: f.created_at ?? undefined,
    modifiedAt: f.modified_at ?? undefined,
    expectedSizeBytes: f.expected_size_bytes ?? undefined,
    deletedAt: f.deleted_at ?? undefined,
    isDeleted: f.is_deleted,
    sizeBytes: f.size_bytes,
    integrity: f.integrity,
    recoveryScore: f.recovery_score,
    recoveryMethod: f.recovery_method,
    previewAvailable: f.preview_available,
    mimeType: f.mime_type ?? undefined,
    startOffset: f.start_offset ?? undefined,
    clusters: f.clusters ?? undefined,
    resourceFork: f.resource_fork
      ? {
          sizeBytes: f.resource_fork.size_bytes,
          expectedSizeBytes: f.resource_fork.expected_size_bytes ?? undefined,
        }
      : undefined,
    alternateDataStreams:
      f.alternate_data_streams?.map((stream) => ({
        name: stream.name,
        sizeBytes: stream.size_bytes,
        expectedSizeBytes: stream.expected_size_bytes ?? undefined,
      })) ?? undefined,
    compressionKind: f.compression_kind ?? undefined,
    sourceView: f.source_view ?? undefined,
    nativeAuxiliaryKind: f.native_auxiliary_kind ?? undefined,
    snapshotXid: f.snapshot_xid ?? undefined,
    recoveryComplexity: f.recovery_complexity ?? undefined,
    validatorStatus: f.validator_status ?? undefined,
    assemblySegmentCount: f.assembly_segment_count ?? undefined,
    gapCount: f.gap_count ?? undefined,
    journalDerived: f.journal_derived,
  };
}

export async function fetchResults(scanId: string): Promise<RecoveredFileData[]> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return (getBrowserPreviewRecoveryResult(scanId)?.files ?? []).map((file) => ({
      ...file,
    }));
  }
  const files = await invoke<RustRecoveredFile[]>('get_results', { scanId });
  return files.map((f) => ({
    id: f.id,
    name: f.name,
    path: f.path,
    extension: f.extension,
    createdAt: f.created_at ?? undefined,
    modifiedAt: f.modified_at ?? undefined,
    expectedSizeBytes: f.expected_size_bytes ?? undefined,
    deletedAt: f.deleted_at ?? undefined,
    isDeleted: f.is_deleted,
    sizeBytes: f.size_bytes,
    integrity: f.integrity,
    recoveryScore: f.recovery_score,
    recoveryMethod: f.recovery_method,
    previewAvailable: f.preview_available,
    mimeType: f.mime_type ?? undefined,
    startOffset: f.start_offset ?? undefined,
    clusters: f.clusters ?? undefined,
    resourceFork: f.resource_fork
      ? {
          sizeBytes: f.resource_fork.size_bytes,
          expectedSizeBytes: f.resource_fork.expected_size_bytes ?? undefined,
        }
      : undefined,
    alternateDataStreams:
      f.alternate_data_streams?.map((stream) => ({
        name: stream.name,
        sizeBytes: stream.size_bytes,
        expectedSizeBytes: stream.expected_size_bytes ?? undefined,
      })) ?? undefined,
    compressionKind: f.compression_kind ?? undefined,
    sourceView: f.source_view ?? undefined,
    nativeAuxiliaryKind: f.native_auxiliary_kind ?? undefined,
    snapshotXid: f.snapshot_xid ?? undefined,
    recoveryComplexity: f.recovery_complexity ?? undefined,
    validatorStatus: f.validator_status ?? undefined,
    assemblySegmentCount: f.assembly_segment_count ?? undefined,
    gapCount: f.gap_count ?? undefined,
    journalDerived: f.journal_derived,
  }));
}
