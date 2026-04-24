import { invoke, isTauri } from '@tauri-apps/api/core';
import type {
  DetectedDevice,
  FileTreeNode,
  RecoveredFile,
  RecoveryResult,
  ScanLogEntry,
  ScanProgress,
  ScanSession,
  ScanType,
} from '../../types';
import { mapRustScanProgress } from '../../types/ipc';
import {
  getBrowserPreviewDevice,
  getBrowserPreviewRecoveryResult,
  getBrowserPreviewScanHistory,
  getBrowserPreviewScanLogs,
  getBrowserPreviewScanProgress,
  updateBrowserPreviewState,
} from '../../utils/browserPreviewSeed';
import { type RustTechnicalLogEntry, mapTechnicalLog } from './_shared';

export type ScanProgressData = ScanProgress;

interface RustScanSessionSummary {
  id: string;
  device_id: string;
  device_name: string;
  source_display_name?: string | null;
  source_kind?: string | null;
  source_format?: string | null;
  source_analysis_path?: string | null;
  source_available?: boolean | null;
  source_requires_preparation?: boolean | null;
  source_prepared?: boolean | null;
  reconstructed_raid_source?: boolean;
  scan_type: string;
  started_at_ms: number;
  completed_at_ms: number | null;
  status: string;
  files_found: number;
  files_recovered: number;
  duration_seconds: number;
  errors: number;
  bytes_copied?: number;
  total_bytes?: number;
  resume_from_bytes?: number;
  unreadable_ranges_count?: number;
  unreadable_bytes?: number;
  rescued_after_retry_bytes?: number;
  retry_passes_completed?: number;
  unreadable_ranges?: Array<{
    start_offset: number;
    length: number;
  }>;
}

function buildPreviewScanId(scanType: string): string {
  return `preview-scan-${scanType}-${Date.now()}`;
}

function buildPreviewPath(device: DetectedDevice, scanType: ScanType, name: string): string {
  const baseFolder =
    scanType === 'quick' || scanType === 'deep'
      ? '/Mounted Volume'
      : scanType === 'lost-volume'
        ? '/Recovered Volume'
        : '/Recovered Image';
  return `${baseFolder}/${device.name}/${name}`;
}

function buildPreviewFiles(
  scanId: string,
  device: DetectedDevice,
  scanType: ScanType,
): RecoveredFile[] {
  const baseTimestamp = new Date('2026-04-09T10:00:00.000Z').toISOString();
  if (scanType === 'quick' || scanType === 'deep') {
    return [
      {
        id: `${scanId}-file-1`,
        name: 'customer-database.xlsx',
        path: buildPreviewPath(device, scanType, 'customer-database.xlsx'),
        extension: 'xlsx',
        sizeBytes: 734_003,
        modifiedAt: baseTimestamp,
        integrity: 'intact',
        recoveryScore: scanType === 'deep' ? 95 : 88,
        recoveryMethod: 'filesystem',
        previewAvailable: true,
        mimeType: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
        recoveryComplexity: 'low',
        validatorStatus: 'office-validated',
        sourceView: 'mounted-volume',
        journalDerived: false,
      },
      {
        id: `${scanId}-file-2`,
        name: 'team-photo.jpg',
        path: buildPreviewPath(device, scanType, 'team-photo.jpg'),
        extension: 'jpg',
        sizeBytes: 1_482_752,
        modifiedAt: baseTimestamp,
        integrity: 'intact',
        recoveryScore: scanType === 'deep' ? 91 : 84,
        recoveryMethod: 'filesystem',
        previewAvailable: true,
        mimeType: 'image/jpeg',
        recoveryComplexity: 'low',
        validatorStatus: 'validated',
        sourceView: 'mounted-volume',
        journalDerived: false,
      },
    ];
  }

  if (scanType === 'carving') {
    return [
      {
        id: `${scanId}-file-1`,
        name: 'deleted-contract.docx',
        path: buildPreviewPath(device, scanType, 'deleted-contract.docx'),
        extension: 'docx',
        sizeBytes: 428_192,
        modifiedAt: baseTimestamp,
        deletedAt: baseTimestamp,
        isDeleted: true,
        integrity: 'intact',
        recoveryScore: 89,
        recoveryMethod: 'filesystem',
        previewAvailable: true,
        mimeType: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
        recoveryComplexity: 'medium',
        validatorStatus: 'office-validated',
        sourceView: 'recovery-image',
        journalDerived: false,
      },
      {
        id: `${scanId}-file-2`,
        name: 'deleted-ledger.csv',
        path: buildPreviewPath(device, scanType, 'deleted-ledger.csv'),
        extension: 'csv',
        sizeBytes: 91_204,
        modifiedAt: baseTimestamp,
        deletedAt: baseTimestamp,
        isDeleted: true,
        integrity: 'partial',
        recoveryScore: 71,
        recoveryMethod: 'filesystem',
        previewAvailable: true,
        mimeType: 'text/csv',
        recoveryComplexity: 'medium',
        validatorStatus: 'partial-unvalidated',
        sourceView: 'recovery-image',
        journalDerived: false,
      },
    ];
  }

  return [
    {
      id: `${scanId}-file-1`,
      name: 'recovered-report.pdf',
      path: buildPreviewPath(device, scanType, 'recovered-report.pdf'),
      extension: 'pdf',
      sizeBytes: 516_224,
      modifiedAt: baseTimestamp,
      integrity: 'intact',
      recoveryScore: 92,
      recoveryMethod: 'carving',
      previewAvailable: true,
      mimeType: 'application/pdf',
      recoveryComplexity: 'medium',
      validatorStatus: 'validated',
      sourceView: 'recovery-image',
      assemblySegmentCount: 1,
      gapCount: 0,
      journalDerived: false,
    },
    {
      id: `${scanId}-file-2`,
      name: 'family-archive.zip',
      path: buildPreviewPath(device, scanType, 'family-archive.zip'),
      extension: 'zip',
      sizeBytes: 3_145_728,
      modifiedAt: baseTimestamp,
      integrity: 'partial',
      recoveryScore: 74,
      recoveryMethod: 'carving',
      previewAvailable: true,
      mimeType: 'application/zip',
      recoveryComplexity: 'high',
      validatorStatus: 'zip-validated',
      sourceView: 'recovery-image',
      assemblySegmentCount: 2,
      gapCount: 1,
      journalDerived: false,
    },
    {
      id: `${scanId}-file-3`,
      name: 'raw-photo-001.jpg',
      path: buildPreviewPath(device, scanType, 'raw-photo-001.jpg'),
      extension: 'jpg',
      sizeBytes: 1_204_992,
      modifiedAt: baseTimestamp,
      integrity: 'intact',
      recoveryScore: 86,
      recoveryMethod: 'carving',
      previewAvailable: true,
      mimeType: 'image/jpeg',
      recoveryComplexity: 'medium',
      validatorStatus: 'validated',
      sourceView: 'recovery-image',
      assemblySegmentCount: 1,
      gapCount: 0,
      journalDerived: false,
    },
  ];
}

function insertFileNode(root: FileTreeNode, segments: string[], file: RecoveredFile): void {
  if (segments.length === 0) {
    root.children = root.children ?? [];
    root.children.push({
      id: `node-${file.id}`,
      name: file.name,
      isDirectory: false,
      file,
    });
    return;
  }

  const [head, ...tail] = segments;
  root.children = root.children ?? [];
  let child = root.children.find((candidate) => candidate.isDirectory && candidate.name === head);
  if (!child) {
    child = {
      id: `dir-${root.id}-${head.toLowerCase().replace(/[^a-z0-9]+/g, '-')}`,
      name: head,
      isDirectory: true,
      children: [],
    };
    root.children.push(child);
  }

  insertFileNode(child, tail, file);
}

function buildPreviewTree(files: RecoveredFile[]): FileTreeNode {
  const root: FileTreeNode = {
    id: 'root',
    name: 'Recovered Files',
    isDirectory: true,
    children: [],
  };

  for (const file of files) {
    const parts = file.path.split('/').filter(Boolean);
    const directorySegments = parts.slice(0, -1);
    insertFileNode(root, directorySegments, file);
  }

  return root;
}

function buildPreviewRecoveryResult(
  scanId: string,
  device: DetectedDevice,
  scanType: ScanType,
): RecoveryResult {
  const files = buildPreviewFiles(scanId, device, scanType);
  const partialFiles = files.filter((file) => file.integrity === 'partial').length;
  const fragmentedFiles = files.filter((file) => file.integrity === 'fragmented').length;
  const corruptFiles = files.filter((file) => file.integrity === 'corrupt').length;
  const totalSizeBytes = files.reduce((sum, file) => sum + file.sizeBytes, 0);
  const recoverableSizeBytes = files
    .filter((file) => file.integrity !== 'corrupt')
    .reduce((sum, file) => sum + file.sizeBytes, 0);

  return {
    scanId,
    totalFiles: files.length,
    intactFiles: files.filter((file) => file.integrity === 'intact').length,
    partialFiles,
    fragmentedFiles,
    corruptFiles,
    totalSizeBytes,
    recoverableSizeBytes,
    files,
    treeRoot: buildPreviewTree(files),
  };
}

function buildPreviewProgress(scanType: ScanType, recoveryResult: RecoveryResult): ScanProgress {
  const imageBackedScan =
    scanType === 'image' ||
    scanType === 'carving' ||
    scanType === 'signature-carving' ||
    scanType === 'lost-volume';
  return {
    status: 'completed',
    stage:
      scanType === 'signature-carving' || scanType === 'carving' || scanType === 'lost-volume'
        ? 'carving-signatures'
        : 'scoring-results',
    percentComplete: 100,
    bytesScanned: recoveryResult.totalSizeBytes,
    totalBytes: recoveryResult.totalSizeBytes,
    filesFound: recoveryResult.totalFiles,
    errorsCount: 0,
    elapsedSeconds: 4,
    resumeFromBytes: 0,
    unreadableRangesCount: 0,
    unreadableBytes: 0,
    estimatedRemainingSeconds: 0,
    currentSector: imageBackedScan ? Math.floor(recoveryResult.totalSizeBytes / 512) : undefined,
  };
}

function buildPreviewLogs(
  scanId: string,
  device: DetectedDevice,
  scanType: ScanType,
  recoveryResult: RecoveryResult,
): ScanLogEntry[] {
  const now = Date.now();
  return [
    {
      timestampMs: now - 3000,
      level: 'info',
      message: `Browser preview initialized ${scanType} for ${device.name}.`,
    },
    {
      timestampMs: now - 2000,
      level: 'info',
      message:
        device.type === 'image'
          ? 'Preview reused the explicit read-only imported-source analysis path.'
          : 'Preview reused seeded device metadata to validate the scan workflow.',
    },
    {
      timestampMs: now - 1000,
      level: 'info',
      message: `Preview scan ${scanId} completed with ${recoveryResult.totalFiles} file(s).`,
    },
  ];
}

function buildPreviewSession(
  scanId: string,
  device: DetectedDevice,
  scanType: ScanType,
  recoveryResult: RecoveryResult,
): ScanSession {
  const completedAtMs = Date.now();
  const imageBackedScan =
    scanType === 'image' ||
    scanType === 'carving' ||
    scanType === 'signature-carving' ||
    scanType === 'lost-volume';
  return {
    id: scanId,
    deviceId: device.id,
    deviceName: device.name,
    sourceDisplayName: device.name,
    sourceKind: device.type === 'image' ? 'raw-image' : undefined,
    sourceFormat: device.type === 'image' ? 'RAW' : undefined,
    sourceAnalysisPath: device.devicePath,
    sourceAvailable: true,
    sourceRequiresPreparation: false,
    sourcePrepared: true,
    reconstructedRaidSource: false,
    scanType,
    startedAtMs: completedAtMs - 4000,
    completedAtMs,
    status: 'completed',
    filesFound: recoveryResult.totalFiles,
    filesRecovered: recoveryResult.intactFiles + recoveryResult.partialFiles,
    durationSeconds: 4,
    errors: 0,
    bytesCopied: imageBackedScan ? recoveryResult.totalSizeBytes : 0,
    resumeFromBytes: 0,
  };
}

function startBrowserPreviewScan(deviceId: string, scanType: ScanType): string {
  const device = getBrowserPreviewDevice(deviceId);
  if (!device) {
    throw new Error('No preview device is available for this scan.');
  }

  const scanId = buildPreviewScanId(scanType);
  const recoveryResult = buildPreviewRecoveryResult(scanId, device, scanType);
  const progress = buildPreviewProgress(scanType, recoveryResult);
  const logs = buildPreviewLogs(scanId, device, scanType, recoveryResult);
  const session = buildPreviewSession(scanId, device, scanType, recoveryResult);

  updateBrowserPreviewState((current) => ({
    ...current,
    selectedDeviceId: deviceId,
    activeScanId: scanId,
    activeRemoteAgentId: null,
    recoveryResult,
    scanHistory: [
      session,
      ...(current.scanHistory ?? []).filter((existing) => existing.id !== scanId),
    ],
    scanProgressById: {
      ...(current.scanProgressById ?? {}),
      [scanId]: progress,
    },
    scanLogsById: {
      ...(current.scanLogsById ?? {}),
      [scanId]: logs,
    },
  }));

  return scanId;
}

export async function startScan(deviceId: string, scanType: string): Promise<string> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return startBrowserPreviewScan(deviceId, scanType as ScanType);
  }
  return invoke<string>('start_scan', { deviceId, scanType });
}

export async function startPotentialVolumeScan(
  deviceId: string,
  volumeId: string,
): Promise<string> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return startBrowserPreviewScan(deviceId, 'lost-volume');
  }
  return invoke<string>('start_potential_volume_scan', { deviceId, volumeId });
}

export async function pauseScan(scanId: string): Promise<void> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    updateBrowserPreviewState((current) => ({
      ...current,
      scanProgressById: current.scanProgressById?.[scanId]
        ? {
            ...(current.scanProgressById ?? {}),
            [scanId]: {
              ...current.scanProgressById[scanId],
              status: 'paused',
            },
          }
        : current.scanProgressById,
    }));
    return;
  }
  await invoke('pause_scan', { scanId });
}

export async function resumeScan(scanId: string): Promise<void> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    updateBrowserPreviewState((current) => ({
      ...current,
      scanProgressById: current.scanProgressById?.[scanId]
        ? {
            ...(current.scanProgressById ?? {}),
            [scanId]: {
              ...current.scanProgressById[scanId],
              status: 'completed',
            },
          }
        : current.scanProgressById,
    }));
    return;
  }
  await invoke('resume_scan', { scanId });
}

export async function cancelScan(scanId: string): Promise<void> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    updateBrowserPreviewState((current) => ({
      ...current,
      scanProgressById: current.scanProgressById?.[scanId]
        ? {
            ...(current.scanProgressById ?? {}),
            [scanId]: {
              ...current.scanProgressById[scanId],
              status: 'cancelled',
            },
          }
        : current.scanProgressById,
    }));
    return;
  }
  await invoke('cancel_scan', { scanId });
}

export function mapScanProgress(p: unknown): ScanProgressData {
  return mapRustScanProgress(p);
}

export async function fetchScanProgress(scanId: string): Promise<ScanProgressData> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    const progress = getBrowserPreviewScanProgress(scanId);
    if (progress) {
      return progress;
    }

    const recoveryResult = getBrowserPreviewRecoveryResult(scanId);
    if (recoveryResult) {
      return buildPreviewProgress('deep', recoveryResult);
    }

    throw new Error('Preview scan progress is unavailable.');
  }
  const p = await invoke<unknown>('get_scan_progress', { scanId });
  return mapScanProgress(p);
}

export async function fetchScanHistory(): Promise<ScanSession[]> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return getBrowserPreviewScanHistory();
  }
  const sessions = await invoke<RustScanSessionSummary[]>('get_scan_history');
  return sessions.map((session) => ({
    id: session.id,
    deviceId: session.device_id,
    deviceName: session.device_name,
    sourceDisplayName: session.source_display_name ?? undefined,
    sourceKind: (session.source_kind as ScanSession['sourceKind']) ?? undefined,
    sourceFormat: session.source_format ?? undefined,
    sourceAnalysisPath: session.source_analysis_path ?? undefined,
    sourceAvailable: session.source_available ?? undefined,
    sourceRequiresPreparation: session.source_requires_preparation ?? undefined,
    sourcePrepared: session.source_prepared ?? undefined,
    reconstructedRaidSource: session.reconstructed_raid_source ?? false,
    scanType: session.scan_type as ScanSession['scanType'],
    startedAtMs: session.started_at_ms,
    completedAtMs: session.completed_at_ms ?? undefined,
    status: session.status as ScanSession['status'],
    filesFound: session.files_found,
    filesRecovered: session.files_recovered,
    durationSeconds: session.duration_seconds,
    errors: session.errors,
    bytesCopied: session.bytes_copied ?? 0,
    totalBytes: session.total_bytes ?? 0,
    resumeFromBytes: session.resume_from_bytes ?? 0,
    unreadableRangesCount: session.unreadable_ranges_count ?? 0,
    unreadableBytes: session.unreadable_bytes ?? 0,
    rescuedAfterRetryBytes: session.rescued_after_retry_bytes ?? 0,
    retryPassesCompleted: session.retry_passes_completed ?? 0,
    unreadableRanges: (session.unreadable_ranges ?? []).map((range) => ({
      startOffset: range.start_offset,
      length: range.length,
    })),
  }));
}

export async function fetchScanLogs(scanId: string): Promise<ScanLogEntry[]> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return getBrowserPreviewScanLogs(scanId);
  }
  const logs = await invoke<RustTechnicalLogEntry[]>('get_scan_logs', { scanId });
  return logs.map(mapTechnicalLog);
}

export async function generateImagingSessionReport(scanId: string): Promise<string> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    throw new Error(
      'A native runtime is required before an imaging session report can be exported.',
    );
  }

  return invoke<string>('generate_imaging_session_report', { scanId });
}

export async function generateImagingRescueMap(scanId: string): Promise<string> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    throw new Error('A native runtime is required before an imaging rescue map can be exported.');
  }

  return invoke<string>('generate_imaging_rescue_map', { scanId });
}
