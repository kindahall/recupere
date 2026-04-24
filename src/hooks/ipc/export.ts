import { invoke, isTauri } from '@tauri-apps/api/core';
import type {
  ExportConfig,
  ExportHistoryEntry,
  ExportProgress,
  LocalHistoryPurgeResult,
  ScanLogEntry,
} from '../../types';
import { type RustTechnicalLogEntry, mapTechnicalLog } from './_shared';

let browserPreviewExportCounter = 0;

const browserPreviewExports = new Map<
  string,
  {
    progress: ExportProgress;
    logs: ScanLogEntry[];
  }
>();

export interface ExportValidationData {
  isSafe: boolean;
  availableBytes: number;
  message: string;
}

interface RustExportProgress {
  total_files: number;
  exported_files: number;
  total_bytes: number;
  exported_bytes: number;
  current_file: string;
  errors: { file_id: string; file_name: string; reason: string }[];
  status: string;
}

interface RustExportSessionSummary {
  id: string;
  scan_id: string;
  source_device_name?: string | null;
  source_display_name?: string | null;
  source_kind?: string | null;
  source_format?: string | null;
  source_analysis_path?: string | null;
  source_available?: boolean | null;
  source_requires_preparation?: boolean | null;
  source_prepared?: boolean | null;
  reconstructed_raid_source?: boolean;
  destination_path: string;
  started_at_ms: number;
  completed_at_ms: number | null;
  status: string;
  total_files: number;
  exported_files: number;
  total_bytes: number;
  exported_bytes: number;
  explicit_selection?: boolean;
  implicit_preview_first_excluded_count?: number;
  errors: { file_id: string; file_name: string; reason: string }[];
}

interface RustLocalHistoryPurgeResult {
  scope: string;
  removed_scan_records: number;
  removed_export_records: number;
  scan_archive_deleted: boolean;
  export_archive_deleted: boolean;
  live_scan_sessions: number;
  live_export_sessions: number;
}

export async function validateExportDestination(
  destination: string,
  sourceDevicePath: string,
): Promise<ExportValidationData> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    const normalizedDestination = destination.trim().replace(/\\/g, '/').toLowerCase();
    const normalizedSource = sourceDevicePath.trim().replace(/\\/g, '/').toLowerCase();
    if (!normalizedDestination) {
      return {
        isSafe: false,
        availableBytes: 0,
        message: 'Choose an export destination first.',
      };
    }
    const isUnsafe =
      normalizedDestination === normalizedSource ||
      normalizedDestination.startsWith(`${normalizedSource}/`);
    return {
      isSafe: !isUnsafe,
      availableBytes: 64 * 1024 * 1024 * 1024,
      message: isUnsafe
        ? 'The export destination cannot be the source device.'
        : 'Destination validated in browser preview. Export execution is simulated outside the desktop backend.',
    };
  }
  const v = await invoke<{ is_safe: boolean; available_bytes: number; message: string }>(
    'validate_export_destination',
    { destination, sourceDevicePath },
  );
  return { isSafe: v.is_safe, availableBytes: v.available_bytes, message: v.message };
}

export async function startExport(scanId: string, config: ExportConfig): Promise<string> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    browserPreviewExportCounter += 1;
    const exportId = `preview-export-${browserPreviewExportCounter}`;
    const totalFiles = Math.max(config.selectedFileIds.length, 1);
    const totalBytes = totalFiles * 512 * 1024;
    browserPreviewExports.set(exportId, {
      progress: {
        totalFiles,
        exportedFiles: totalFiles,
        totalBytes,
        exportedBytes: totalBytes,
        currentFile: 'Preview export complete',
        errors: [],
        status: 'completed',
      },
      logs: [
        {
          timestampMs: Date.now() - 750,
          level: 'info',
          message: `Preparing preview export for scan ${scanId}.`,
        },
        {
          timestampMs: Date.now(),
          level: 'info',
          message: `Preview export completed to ${config.destinationPath}.`,
        },
      ],
    });
    return exportId;
  }
  return invoke<string>('start_export', {
    scanId,
    destinationPath: config.destinationPath,
    selectedFileIds: config.selectedFileIds,
    conflictStrategy: config.conflictStrategy,
    preserveStructure: config.preserveStructure,
    verifyIntegrity: config.verifyIntegrity,
    explicitSelection: config.explicitSelection,
    implicitPreviewFirstExcludedCount: config.implicitPreviewFirstExcludedCount,
  });
}

export async function fetchExportProgress(exportId: string): Promise<ExportProgress> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return (
      browserPreviewExports.get(exportId)?.progress ?? {
        totalFiles: 0,
        exportedFiles: 0,
        totalBytes: 0,
        exportedBytes: 0,
        currentFile: '',
        errors: [],
        status: 'completed',
      }
    );
  }
  const p = await invoke<RustExportProgress>('get_export_progress', { exportId });
  return {
    totalFiles: p.total_files,
    exportedFiles: p.exported_files,
    totalBytes: p.total_bytes,
    exportedBytes: p.exported_bytes,
    currentFile: p.current_file,
    errors: p.errors.map((error) => ({
      fileId: error.file_id,
      fileName: error.file_name,
      reason: error.reason,
    })),
    status: p.status as ExportProgress['status'],
  };
}

export async function fetchExportLogs(exportId: string): Promise<ScanLogEntry[]> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return browserPreviewExports.get(exportId)?.logs ?? [];
  }
  const logs = await invoke<RustTechnicalLogEntry[]>('get_export_logs', { exportId });
  return logs.map(mapTechnicalLog);
}

export async function fetchExportHistory(): Promise<ExportHistoryEntry[]> {
  const exports = await invoke<RustExportSessionSummary[]>('get_export_history');
  return exports.map((entry) => ({
    id: entry.id,
    scanId: entry.scan_id,
    sourceDeviceName: entry.source_device_name ?? undefined,
    sourceDisplayName: entry.source_display_name ?? undefined,
    sourceKind: (entry.source_kind as ExportHistoryEntry['sourceKind']) ?? undefined,
    sourceFormat: entry.source_format ?? undefined,
    sourceAnalysisPath: entry.source_analysis_path ?? undefined,
    sourceAvailable: entry.source_available ?? undefined,
    sourceRequiresPreparation: entry.source_requires_preparation ?? undefined,
    sourcePrepared: entry.source_prepared ?? undefined,
    reconstructedRaidSource: entry.reconstructed_raid_source ?? false,
    destinationPath: entry.destination_path,
    startedAtMs: entry.started_at_ms,
    completedAtMs: entry.completed_at_ms ?? undefined,
    status: entry.status as ExportHistoryEntry['status'],
    totalFiles: entry.total_files,
    exportedFiles: entry.exported_files,
    totalBytes: entry.total_bytes,
    exportedBytes: entry.exported_bytes,
    explicitSelection: entry.explicit_selection ?? false,
    implicitPreviewFirstExcludedCount: entry.implicit_preview_first_excluded_count ?? 0,
    errors: entry.errors.map((error) => ({
      fileId: error.file_id,
      fileName: error.file_name,
      reason: error.reason,
    })),
  }));
}

export async function clearLocalHistory(
  scope: LocalHistoryPurgeResult['scope'],
): Promise<LocalHistoryPurgeResult> {
  const result = await invoke<RustLocalHistoryPurgeResult>('clear_local_history', { scope });
  return {
    scope: result.scope as LocalHistoryPurgeResult['scope'],
    removedScanRecords: result.removed_scan_records,
    removedExportRecords: result.removed_export_records,
    scanArchiveDeleted: result.scan_archive_deleted,
    exportArchiveDeleted: result.export_archive_deleted,
    liveScanSessions: result.live_scan_sessions,
    liveExportSessions: result.live_export_sessions,
  };
}

export async function saveTechnicalTimelineReport(
  destinationPath: string,
  content: string,
  sourceDevicePath?: string,
): Promise<string> {
  return invoke<string>('save_technical_timeline_report', {
    destinationPath,
    content,
    sourceDevicePath,
  });
}

export async function saveSupportBundle(
  destinationPath: string,
  sourceDevicePath?: string,
): Promise<string> {
  return invoke<string>('save_support_bundle', {
    destinationPath,
    sourceDevicePath,
  });
}

export async function generateRecoveryReport(
  scanId: string,
  language: string,
  includeFileInventory: boolean,
): Promise<string> {
  return invoke<string>('generate_recovery_report', { scanId, language, includeFileInventory });
}

export async function exportResultsCsv(scanId: string): Promise<string> {
  return invoke<string>('export_results_csv', { scanId });
}

export async function generateLabBundle(
  deviceId: string,
  destinationPath: string,
  activeScanId?: string | null,
  activeExportId?: string | null,
): Promise<string> {
  return invoke<string>('generate_lab_bundle', {
    deviceId,
    destinationPath,
    activeScanId: activeScanId ?? null,
    activeExportId: activeExportId ?? null,
  });
}
