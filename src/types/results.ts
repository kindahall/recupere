import type { RecoveryContextHint } from './filesystemMemory';

// ============================================================================
// Shared TypeScript Types — Results
// ============================================================================

export type FileIntegrity = 'intact' | 'partial' | 'fragmented' | 'uncertain' | 'corrupt';

export type RecoveryMethod = 'filesystem' | 'carving' | 'reconstruction' | 'mixed';
export type CompressionKind = 'lznt1';
export type RecoverySourceView =
  | 'live-catalog'
  | 'snapshot'
  | 'journal'
  | 'recovery-image'
  | 'mounted-volume'
  | 'mixed';
export type RecoveryComplexity = 'low' | 'medium' | 'high';
export type ValidatorStatus =
  | 'validated'
  | 'reassembled'
  | 'partial-unvalidated'
  | 'failed'
  | 'unsupported'
  | 'office-validated'
  | 'zip-validated';

export interface FileFork {
  sizeBytes: number;
  expectedSizeBytes?: number;
}

export interface NamedFileFork extends FileFork {
  name: string;
}

export interface RecoveredFile {
  id: string;
  name: string;
  path: string; // Original path if known
  extension: string;
  sizeBytes: number;
  expectedSizeBytes?: number;
  createdAt?: string;
  modifiedAt?: string;
  deletedAt?: string;
  isDeleted?: boolean;
  integrity: FileIntegrity;
  recoveryScore: number; // 0-100 (always presented as estimate)
  baseRecoveryScore?: number; // Raw engine estimate before any UI-only triage adjustments
  recoveryScoreAdjustment?: number; // Bounded filesystem-memory triage delta, if any
  recoveryMethod: RecoveryMethod;
  previewAvailable: boolean;
  mimeType?: string;
  startOffset?: number; // For expert mode
  clusters?: number[]; // For expert mode
  resourceFork?: FileFork;
  alternateDataStreams?: NamedFileFork[];
  compressionKind?: CompressionKind;
  sourceView?: RecoverySourceView;
  nativeAuxiliaryKind?: 'resource-fork' | 'ads' | 'mixed';
  snapshotXid?: number;
  recoveryComplexity?: RecoveryComplexity;
  validatorStatus?: ValidatorStatus;
  assemblySegmentCount?: number;
  gapCount?: number;
  journalDerived?: boolean;
  sha256?: string;
  filesystemMemoryContext?: RecoveryContextHint;
}

export interface FilePreview {
  fileId: string;
  kind: 'text' | 'image' | 'pdf' | 'unavailable';
  mimeType?: string;
  textContent?: string;
  assetPath?: string;
  truncated: boolean;
  message?: string;
}

export interface HexPreviewLine {
  offset: number;
  hex: string;
  ascii: string;
}

export interface FileHexPreview {
  fileId: string;
  startOffset: number;
  bytesRead: number;
  totalSizeBytes: number;
  lineWidth: number;
  hasMoreBefore: boolean;
  hasMoreAfter: boolean;
  lines: HexPreviewLine[];
}

export interface RecoveryResult {
  scanId: string;
  totalFiles: number;
  intactFiles: number;
  partialFiles: number;
  fragmentedFiles: number;
  uncertainFiles?: number;
  corruptFiles: number;
  totalSizeBytes: number;
  recoverableSizeBytes: number;
  files: RecoveredFile[];
  treeRoot: FileTreeNode;
}

export interface FileTreeNode {
  id: string;
  name: string;
  isDirectory: boolean;
  children?: FileTreeNode[];
  file?: RecoveredFile;
}

export interface ExportConfig {
  destinationPath: string;
  selectedFileIds: string[];
  conflictStrategy: 'rename' | 'skip' | 'overwrite';
  preserveStructure: boolean;
  verifyIntegrity: boolean;
  explicitSelection: boolean;
  implicitPreviewFirstExcludedCount: number;
}

export interface ExportProgress {
  totalFiles: number;
  exportedFiles: number;
  totalBytes: number;
  exportedBytes: number;
  currentFile: string;
  errors: ExportError[];
  status: 'preparing' | 'exporting' | 'verifying' | 'completed' | 'error';
}

export interface ExportError {
  fileId: string;
  fileName: string;
  reason: string;
}

export interface ExportHistoryEntry {
  id: string;
  scanId: string;
  sourceDeviceName?: string;
  sourceDisplayName?: string;
  sourceKind?: 'raid-analysis' | 'forensic-image' | 'virtual-disk' | 'raw-image' | 'generic-image';
  sourceFormat?: string;
  sourceAnalysisPath?: string;
  sourceAvailable?: boolean;
  sourceRequiresPreparation?: boolean;
  sourcePrepared?: boolean;
  reconstructedRaidSource?: boolean;
  destinationPath: string;
  startedAtMs: number;
  completedAtMs?: number;
  status: ExportProgress['status'];
  totalFiles: number;
  exportedFiles: number;
  totalBytes: number;
  exportedBytes: number;
  explicitSelection: boolean;
  implicitPreviewFirstExcludedCount: number;
  errors: ExportError[];
}

export interface LocalHistoryPurgeResult {
  scope: 'scan' | 'export' | 'all';
  removedScanRecords: number;
  removedExportRecords: number;
  scanArchiveDeleted: boolean;
  exportArchiveDeleted: boolean;
  liveScanSessions: number;
  liveExportSessions: number;
}
