import { invoke } from '@tauri-apps/api/core';
import type { FileHexPreview, FilePreview } from '../../types';

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

function mapPreview(preview: RustFilePreview): FilePreview {
  return {
    fileId: preview.file_id,
    kind: preview.kind as FilePreview['kind'],
    mimeType: preview.mime_type ?? undefined,
    textContent: preview.text_content ?? undefined,
    assetPath: preview.asset_path ?? undefined,
    truncated: preview.truncated,
    message: preview.message ?? undefined,
  };
}

function mapHexPreview(preview: RustFileHexPreview): FileHexPreview {
  return {
    fileId: preview.file_id,
    startOffset: preview.start_offset,
    bytesRead: preview.bytes_read,
    totalSizeBytes: preview.total_size_bytes,
    lineWidth: preview.line_width,
    hasMoreBefore: preview.has_more_before,
    hasMoreAfter: preview.has_more_after,
    lines: preview.lines.map((line) => ({
      offset: line.offset,
      hex: line.hex,
      ascii: line.ascii,
    })),
  };
}

export async function fetchFilePreview(scanId: string, fileId: string): Promise<FilePreview> {
  const preview = await invoke<RustFilePreview>('get_file_preview', { scanId, fileId });
  return mapPreview(preview);
}

export async function fetchFileMediaAsset(scanId: string, fileId: string): Promise<string> {
  return invoke<string>('get_file_media_asset', { scanId, fileId });
}

export type RepairConfidence = 'intact' | 'high' | 'medium' | 'none';

export interface RepairReport {
  format: string;
  confidence: RepairConfidence;
  original_size: number;
  repaired_size: number;
  actions: string[];
  notes: string[];
}

export interface RepairCommandResult {
  report: RepairReport;
  asset_path: string | null;
}

export async function repairFile(scanId: string, fileId: string): Promise<RepairCommandResult> {
  return invoke<RepairCommandResult>('repair_file', { scanId, fileId });
}

export async function saveRepairedFile(
  assetPath: string,
  destinationPath: string,
  sourceDevicePath?: string,
): Promise<string> {
  return invoke<string>('save_repaired_file', { assetPath, destinationPath, sourceDevicePath });
}

export async function fetchFileAuxiliaryPreview(
  scanId: string,
  fileId: string,
  auxiliaryKind: 'resource-fork' | 'ads',
  auxiliaryName?: string,
): Promise<FilePreview> {
  const preview = await invoke<RustFilePreview>('get_file_auxiliary_preview', {
    scanId,
    fileId,
    auxiliaryKind,
    auxiliaryName,
  });
  return mapPreview(preview);
}

export async function fetchFileHexPreview(
  scanId: string,
  fileId: string,
  startOffset: number,
  bytesToRead: number,
): Promise<FileHexPreview> {
  const preview = await invoke<RustFileHexPreview>('get_file_hex_preview', {
    scanId,
    fileId,
    startOffset,
    bytesToRead,
  });
  return mapHexPreview(preview);
}

export async function fetchFileAuxiliaryHexPreview(
  scanId: string,
  fileId: string,
  auxiliaryKind: 'resource-fork' | 'ads',
  startOffset: number,
  bytesToRead: number,
  auxiliaryName?: string,
): Promise<FileHexPreview> {
  const preview = await invoke<RustFileHexPreview>('get_file_auxiliary_hex_preview', {
    scanId,
    fileId,
    auxiliaryKind,
    auxiliaryName,
    startOffset,
    bytesToRead,
  });
  return mapHexPreview(preview);
}

export async function saveFileAuxiliaryPayload(
  scanId: string,
  fileId: string,
  auxiliaryKind: 'resource-fork' | 'ads',
  destinationPath: string,
  sourceDevicePath?: string,
  auxiliaryName?: string,
): Promise<string> {
  return invoke<string>('save_file_auxiliary_payload', {
    scanId,
    fileId,
    auxiliaryKind,
    auxiliaryName,
    destinationPath,
    sourceDevicePath,
  });
}
