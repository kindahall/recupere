// Frontend IPC bindings for the remote-agent commands.
//
// These wrap the Tauri `remote_*` commands declared in
// `src-tauri/src/remote/commands.rs`. The Rust types returned by the agent
// match the local engine's types exactly, so we run them through the same
// mappers as the local IPC layer (`mapDevice`, `mapScanProgress`,
// `mapRecoveredFile`, `mapTechnicalLog`). The result: callers can treat a
// remote payload identically to a local one.

import { invoke, isTauri } from '@tauri-apps/api/core';
import type {
  AiRecoveryBrief,
  DetectedDevice,
  FileHexPreview,
  FilePreview,
  ScanLogEntry,
  ScanProgress,
} from '../../types';
import { type RustTechnicalLogEntry, mapTechnicalLog } from './_shared';
import type { FileClassification, RecoveryPrediction } from './ai';
import { type RustDevice, mapDevice } from './device';
import { type RustAiRecoveryBrief, mapAiRecoveryBrief } from './diagnostic';
import { type RecoveredFileData, type RustRecoveredFile, mapRecoveredFile } from './results';
import { mapScanProgress } from './scan';

export interface RemoteAgentSummary {
  id: string;
  label: string;
  base_url: string;
}

function isBrowserPreviewRemoteUnsupported(): boolean {
  return __ALLOW_BROWSER_PREVIEW__ && !isTauri();
}

function remoteDesktopOnlyError(): Error {
  return new Error('Remote-agent workflows are only available in the desktop app.');
}

function ensureRemoteDesktop(): void {
  if (isBrowserPreviewRemoteUnsupported()) {
    throw remoteDesktopOnlyError();
  }
}

async function invokeRemote<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  ensureRemoteDesktop();
  return invoke<T>(command, args);
}

export async function addRemoteAgent(
  label: string,
  baseUrl: string,
  token: string,
): Promise<RemoteAgentSummary> {
  return invokeRemote<RemoteAgentSummary>('add_remote_agent', {
    label,
    baseUrl,
    token,
  });
}

export async function listRemoteAgents(): Promise<RemoteAgentSummary[]> {
  if (isBrowserPreviewRemoteUnsupported()) {
    return [];
  }
  return invoke<RemoteAgentSummary[]>('list_remote_agents');
}

export async function removeRemoteAgent(agentId: string): Promise<boolean> {
  return invokeRemote<boolean>('remove_remote_agent', { agentId });
}

export async function remoteGetDevices(agentId: string): Promise<DetectedDevice[]> {
  const devices = await invokeRemote<RustDevice[]>('remote_get_devices', { agentId });
  return devices.map(mapDevice);
}

export async function remoteStartScan(
  agentId: string,
  deviceId: string,
  scanType: string,
): Promise<string> {
  return invokeRemote<string>('remote_start_scan', {
    agentId,
    deviceId,
    scanType,
  });
}

export async function remoteGetScanProgress(
  agentId: string,
  scanId: string,
): Promise<ScanProgress> {
  const p = await invokeRemote<unknown>('remote_get_scan_progress', {
    agentId,
    scanId,
  });
  return mapScanProgress(p);
}

export async function remoteGetResults(
  agentId: string,
  scanId: string,
): Promise<RecoveredFileData[]> {
  const files = await invokeRemote<RustRecoveredFile[]>('remote_get_results', {
    agentId,
    scanId,
  });
  return files.map(mapRecoveredFile);
}

export async function remoteGetScanLogs(agentId: string, scanId: string): Promise<ScanLogEntry[]> {
  const logs = await invokeRemote<RustTechnicalLogEntry[]>('remote_get_scan_logs', {
    agentId,
    scanId,
  });
  return logs.map(mapTechnicalLog);
}

export async function remotePauseScan(agentId: string, scanId: string): Promise<void> {
  await invokeRemote('remote_pause_scan', { agentId, scanId });
}

export async function remoteResumeScan(agentId: string, scanId: string): Promise<void> {
  await invokeRemote('remote_resume_scan', { agentId, scanId });
}

export async function remoteCancelScan(agentId: string, scanId: string): Promise<void> {
  await invokeRemote('remote_cancel_scan', { agentId, scanId });
}

export async function remoteRestore(
  agentId: string,
  scanId: string,
  destinationPath: string,
  selectedFileIds: string[],
  conflictStrategy = 'rename',
  preserveStructure = true,
  verifyIntegrity = true,
): Promise<string> {
  return invokeRemote<string>('remote_restore', {
    agentId,
    scanId,
    destinationPath,
    selectedFileIds,
    conflictStrategy,
    preserveStructure,
    verifyIntegrity,
  });
}

export async function remoteGetExportProgress(agentId: string, exportId: string): Promise<unknown> {
  return invokeRemote<unknown>('remote_get_export_progress', { agentId, exportId });
}

// ---------- Preview ----------
//
// The Rust shapes returned by the agent are byte-for-byte identical to the
// local IPC payloads, so we run them through the same mappers as `preview.ts`
// (kept private there). To avoid duplicating the mapper, we re-shape inline.

interface RustFilePreview {
  file_id: string;
  kind: string;
  mime_type: string | null;
  text_content: string | null;
  asset_path: string | null;
  truncated: boolean;
  message: string | null;
}

interface RustHexPreviewLine {
  offset: number;
  hex: string;
  ascii: string;
}

interface RustFileHexPreview {
  file_id: string;
  start_offset: number;
  bytes_read: number;
  total_size_bytes: number;
  line_width: number;
  has_more_before: boolean;
  has_more_after: boolean;
  lines: RustHexPreviewLine[];
}

function mapRemoteFilePreview(p: RustFilePreview): FilePreview {
  return {
    fileId: p.file_id,
    kind: p.kind as FilePreview['kind'],
    mimeType: p.mime_type ?? undefined,
    textContent: p.text_content ?? undefined,
    assetPath: p.asset_path ?? undefined,
    truncated: p.truncated,
    message: p.message ?? undefined,
  };
}

function mapRemoteFileHexPreview(p: RustFileHexPreview): FileHexPreview {
  return {
    fileId: p.file_id,
    startOffset: p.start_offset,
    bytesRead: p.bytes_read,
    totalSizeBytes: p.total_size_bytes,
    lineWidth: p.line_width,
    hasMoreBefore: p.has_more_before,
    hasMoreAfter: p.has_more_after,
    lines: p.lines.map((line) => ({
      offset: line.offset,
      hex: line.hex,
      ascii: line.ascii,
    })),
  };
}

export async function remoteGetFilePreview(
  agentId: string,
  scanId: string,
  fileId: string,
): Promise<FilePreview> {
  const p = await invokeRemote<RustFilePreview>('remote_get_file_preview', {
    agentId,
    scanId,
    fileId,
  });
  return mapRemoteFilePreview(p);
}

export async function remoteGetFileHexPreview(
  agentId: string,
  scanId: string,
  fileId: string,
  startOffset: number,
  bytesToRead: number,
): Promise<FileHexPreview> {
  const p = await invokeRemote<RustFileHexPreview>('remote_get_file_hex_preview', {
    agentId,
    scanId,
    fileId,
    startOffset,
    bytesToRead,
  });
  return mapRemoteFileHexPreview(p);
}

/// Materialise a remote media asset locally and return the local file path,
/// suitable for `convertFileSrc` + `<video>`/`<audio>`.
export async function remoteGetFileMediaAsset(
  agentId: string,
  scanId: string,
  fileId: string,
): Promise<string> {
  return invokeRemote<string>('remote_get_file_media_asset', {
    agentId,
    scanId,
    fileId,
  });
}

// ---------- AI heuristics ----------

interface RustFileClassification {
  file_id: string;
  category: string;
  importance: string;
  description: string;
  confidence: number;
}

interface RustRecoveryPrediction {
  file_id: string;
  success_probability: number;
  estimated_quality: string;
  risk_factors: string[];
  recommendation: string;
}

export async function remoteGetScanAiBrief(
  agentId: string,
  scanId: string,
): Promise<AiRecoveryBrief> {
  const brief = await invokeRemote<RustAiRecoveryBrief>('remote_get_scan_ai_brief', {
    agentId,
    scanId,
  });
  return mapAiRecoveryBrief(brief);
}

export async function remoteClassifyScanFiles(
  agentId: string,
  scanId: string,
): Promise<FileClassification[]> {
  const results = await invokeRemote<RustFileClassification[]>('remote_classify_scan_files', {
    agentId,
    scanId,
  });
  return results.map((c) => ({
    fileId: c.file_id,
    category: c.category,
    importance: c.importance,
    description: c.description,
    confidence: c.confidence,
  }));
}

export async function remotePredictScanRecovery(
  agentId: string,
  scanId: string,
): Promise<RecoveryPrediction[]> {
  const results = await invokeRemote<RustRecoveryPrediction[]>('remote_predict_scan_recovery', {
    agentId,
    scanId,
  });
  return results.map((r) => ({
    fileId: r.file_id,
    successProbability: r.success_probability,
    estimatedQuality: r.estimated_quality,
    riskFactors: r.risk_factors,
    recommendation: r.recommendation,
  }));
}

// ---------- Reports ----------

export async function remoteGenerateRecoveryReport(
  agentId: string,
  scanId: string,
  language: string,
  includeFileInventory: boolean,
  localDestinationPath: string,
): Promise<string> {
  return invokeRemote<string>('remote_generate_recovery_report', {
    agentId,
    scanId,
    language,
    includeFileInventory,
    localDestinationPath,
  });
}

export async function remoteExportResultsCsv(
  agentId: string,
  scanId: string,
  localDestinationPath: string,
): Promise<string> {
  return invokeRemote<string>('remote_export_results_csv', {
    agentId,
    scanId,
    localDestinationPath,
  });
}

// ---------- Generic file download ----------

export async function remoteDownloadFile(
  agentId: string,
  remotePath: string,
  localDestinationPath: string,
): Promise<number> {
  return invokeRemote<number>('remote_download_file', {
    agentId,
    remotePath,
    localDestinationPath,
  });
}

export async function remotePullRecoveredFile(
  agentId: string,
  scanId: string,
  fileId: string,
  localDestinationPath: string,
): Promise<number> {
  return invokeRemote<number>('remote_pull_recovered_file', {
    agentId,
    scanId,
    fileId,
    localDestinationPath,
  });
}

// ---------------------------------------------------------------------------
// Dispatcher helpers — pick the local or remote implementation depending on
// whether an `agentId` is set. Lets call sites stay agnostic.
// ---------------------------------------------------------------------------

import { fetchResults } from './results';
import { cancelScan, fetchScanLogs, fetchScanProgress, pauseScan, resumeScan } from './scan';

export async function fetchScanProgressFor(
  scanId: string,
  agentId: string | null,
): Promise<ScanProgress> {
  return agentId ? remoteGetScanProgress(agentId, scanId) : fetchScanProgress(scanId);
}

export async function fetchScanLogsFor(
  scanId: string,
  agentId: string | null,
): Promise<ScanLogEntry[]> {
  return agentId ? remoteGetScanLogs(agentId, scanId) : fetchScanLogs(scanId);
}

export async function fetchResultsFor(
  scanId: string,
  agentId: string | null,
): Promise<RecoveredFileData[]> {
  return agentId ? remoteGetResults(agentId, scanId) : fetchResults(scanId);
}

export async function pauseScanFor(scanId: string, agentId: string | null): Promise<void> {
  return agentId ? remotePauseScan(agentId, scanId) : pauseScan(scanId);
}

export async function resumeScanFor(scanId: string, agentId: string | null): Promise<void> {
  return agentId ? remoteResumeScan(agentId, scanId) : resumeScan(scanId);
}

export async function cancelScanFor(scanId: string, agentId: string | null): Promise<void> {
  return agentId ? remoteCancelScan(agentId, scanId) : cancelScan(scanId);
}

import { classifyScanFiles, predictScanRecovery } from './ai';
import { fetchScanAiBrief } from './diagnostic';
import { exportResultsCsv, generateRecoveryReport } from './export';
import { fetchFileHexPreview, fetchFileMediaAsset, fetchFilePreview } from './preview';

export async function fetchFilePreviewFor(
  scanId: string,
  fileId: string,
  agentId: string | null,
): Promise<FilePreview> {
  return agentId ? remoteGetFilePreview(agentId, scanId, fileId) : fetchFilePreview(scanId, fileId);
}

export async function fetchFileHexPreviewFor(
  scanId: string,
  fileId: string,
  startOffset: number,
  bytesToRead: number,
  agentId: string | null,
): Promise<FileHexPreview> {
  return agentId
    ? remoteGetFileHexPreview(agentId, scanId, fileId, startOffset, bytesToRead)
    : fetchFileHexPreview(scanId, fileId, startOffset, bytesToRead);
}

export async function fetchFileMediaAssetFor(
  scanId: string,
  fileId: string,
  agentId: string | null,
): Promise<string> {
  return agentId
    ? remoteGetFileMediaAsset(agentId, scanId, fileId)
    : fetchFileMediaAsset(scanId, fileId);
}

export async function fetchScanAiBriefFor(
  scanId: string,
  agentId: string | null,
): Promise<AiRecoveryBrief> {
  return agentId ? remoteGetScanAiBrief(agentId, scanId) : fetchScanAiBrief(scanId);
}

export async function classifyScanFilesFor(
  scanId: string,
  agentId: string | null,
): Promise<FileClassification[]> {
  return agentId ? remoteClassifyScanFiles(agentId, scanId) : classifyScanFiles(scanId);
}

export async function predictScanRecoveryFor(
  scanId: string,
  agentId: string | null,
): Promise<RecoveryPrediction[]> {
  return agentId ? remotePredictScanRecovery(agentId, scanId) : predictScanRecovery(scanId);
}

/// `localDestinationPath` is required when running against a remote agent —
/// the agent generates the report on the server then we pull the bytes back.
/// For local scans the existing pipeline writes wherever the local engine
/// chooses, so the parameter is ignored.
export async function generateRecoveryReportFor(
  scanId: string,
  language: string,
  includeFileInventory: boolean,
  agentId: string | null,
  localDestinationPath: string,
): Promise<string> {
  return agentId
    ? remoteGenerateRecoveryReport(
        agentId,
        scanId,
        language,
        includeFileInventory,
        localDestinationPath,
      )
    : generateRecoveryReport(scanId, language, includeFileInventory);
}

export async function exportResultsCsvFor(
  scanId: string,
  agentId: string | null,
  localDestinationPath: string,
): Promise<string> {
  return agentId
    ? remoteExportResultsCsv(agentId, scanId, localDestinationPath)
    : exportResultsCsv(scanId);
}
