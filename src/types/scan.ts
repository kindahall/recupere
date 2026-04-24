// ============================================================================
// Shared TypeScript Types — Scan
// ============================================================================

export type ScanType =
  | 'quick'
  | 'deep'
  | 'image'
  | 'carving'
  | 'signature-carving'
  | 'lost-volume'
  | 'reconstruction';
export type ImagingProfile = 'standard' | 'cautious';

export type ScanStatus =
  | 'idle'
  | 'preparing'
  | 'scanning'
  | 'paused'
  | 'completed'
  | 'cancelled'
  | 'error';

export type ScanStage =
  | 'initializing'
  | 'creating-image'
  | 'reading-partition-table'
  | 'analyzing-filesystem'
  | 'scanning-deleted-entries'
  | 'carving-signatures'
  | 'scoring-results'
  | 'finalizing';

export interface ScanConfig {
  deviceId: string;
  scanType: ScanType;
  targetFilesystems: string[];
  enableCarving: boolean;
  imagingRequiresElevation?: boolean;
  imagingProfile?: ImagingProfile;
  imagingProfileReasonKey?: string;
  externalRescueMapPath?: string;
  potentialVolumeId?: string;
  potentialVolumeLabel?: string;
  potentialVolumeFilesystem?: string;
  potentialVolumeStartOffset?: number;
  potentialVolumeSizeBytes?: number;
  customSignatures?: string[];
  maxDepth?: number;
}

export interface ScanProgress {
  status: ScanStatus;
  stage: ScanStage;
  percentComplete: number;
  bytesScanned: number;
  totalBytes: number;
  filesFound: number;
  errorsCount: number;
  elapsedSeconds: number;
  resumeFromBytes?: number;
  unreadableRangesCount?: number;
  unreadableBytes?: number;
  rescuedAfterRetryBytes?: number;
  retryPassesCompleted?: number;
  unreadableRanges?: ImagingMapRange[];
  estimatedRemainingSeconds?: number;
  currentSector?: number;
}

export interface ScanLogEntry {
  timestampMs: number;
  level: 'info' | 'warning' | 'error' | 'debug';
  message: string;
  details?: string;
}

export interface ImagingMapRange {
  startOffset: number;
  length: number;
}

export interface ScanSession {
  id: string;
  deviceId: string;
  deviceName: string;
  sourceDisplayName?: string;
  sourceKind?: 'raid-analysis' | 'forensic-image' | 'virtual-disk' | 'raw-image' | 'generic-image';
  sourceFormat?: string;
  sourceAnalysisPath?: string;
  sourceAvailable?: boolean;
  sourceRequiresPreparation?: boolean;
  sourcePrepared?: boolean;
  reconstructedRaidSource?: boolean;
  scanType: ScanType;
  startedAtMs: number;
  completedAtMs?: number;
  status: ScanStatus;
  filesFound: number;
  filesRecovered: number;
  durationSeconds: number;
  errors: number;
  bytesCopied?: number;
  totalBytes?: number;
  resumeFromBytes?: number;
  unreadableRangesCount?: number;
  unreadableBytes?: number;
  rescuedAfterRetryBytes?: number;
  retryPassesCompleted?: number;
  unreadableRanges?: ImagingMapRange[];
}
