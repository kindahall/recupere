import { invoke, isTauri } from '@tauri-apps/api/core';
import type {
  DetectedDevice,
  EncryptionInfo,
  FilesystemType,
  ImportedRecoverySourceStatus,
  RaidAnalysisBuildRequest,
  RaidAnalysisBuildResult,
  RaidAnalysisValidation,
  RaidCandidate,
  RaidMetadata,
  RiskLevel,
  SmartReport,
} from '../../types';
import {
  loadBrowserPreviewState,
  setBrowserPreviewImportedSourceStatus,
} from '../../utils/browserPreviewSeed';

export interface RustDevice {
  id: string;
  name: string;
  device_path: string;
  device_type: string;
  filesystem: string;
  capacity_bytes: number;
  used_bytes: number;
  status: string;
  risk_level: string;
  serial: string | null;
  model: string | null;
  is_trim_enabled: boolean | null;
  is_encrypted: boolean | null;
  smart_available: boolean | null;
  partitions: {
    id: string;
    label: string;
    filesystem: string;
    size_bytes: number;
    start_offset: number;
    mount_path: string | null;
    is_mounted: boolean;
    is_bootable: boolean;
  }[];
}

export function mapDevice(d: RustDevice): DetectedDevice {
  return {
    id: d.id,
    name: d.name,
    devicePath: d.device_path,
    type: d.device_type as DetectedDevice['type'],
    filesystem: d.filesystem as FilesystemType,
    capacityBytes: d.capacity_bytes,
    usedBytes: d.used_bytes,
    status: d.status as DetectedDevice['status'],
    riskLevel: d.risk_level as RiskLevel,
    serial: d.serial ?? undefined,
    model: d.model ?? undefined,
    isTrimEnabled: d.is_trim_enabled ?? false,
    isEncrypted: d.is_encrypted ?? false,
    smartAvailable: d.smart_available ?? false,
    partitions: d.partitions.map((p) => ({
      id: p.id,
      label: p.label,
      filesystem: p.filesystem as FilesystemType,
      sizeBytes: p.size_bytes,
      startOffset: p.start_offset,
      mountPath: p.mount_path ?? undefined,
      isMounted: p.is_mounted,
      isBootable: p.is_bootable,
    })),
  };
}

export async function fetchDevices(): Promise<DetectedDevice[]> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return loadBrowserPreviewState()?.devices ?? [];
  }
  const devices = await invoke<RustDevice[]>('get_devices');
  return devices.map(mapDevice);
}

export async function importRecoverySource(path: string): Promise<ImportedRecoverySourceStatus> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    throw new Error('Recovery-source import is only available in the desktop app.');
  }
  const status = await invoke<RustImportedRecoverySourceStatus>('import_recovery_source', { path });
  return mapImportedRecoverySourceStatus(status);
}

export async function removeImportedRecoverySource(path: string): Promise<void> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    throw new Error('Recovery-source removal is only available in the desktop app.');
  }
  await invoke('remove_imported_recovery_source', { path });
}

interface RustImportedRecoverySourceStatus {
  display_name: string;
  source_path: string;
  source_format: string;
  logical_size_bytes: number;
  support_tier: 'supported' | 'limited' | 'unsupported';
  support_note: string;
  safer_next_step: string;
  source_available: boolean;
  requires_preparation: boolean;
  prepared: boolean;
  analysis_path: string | null;
  cache_path: string | null;
  cache_size_bytes: number | null;
}

function mapImportedRecoverySourceStatus(
  status: RustImportedRecoverySourceStatus,
): ImportedRecoverySourceStatus {
  return {
    displayName: status.display_name,
    sourcePath: status.source_path,
    sourceFormat: status.source_format,
    logicalSizeBytes: status.logical_size_bytes,
    supportTier: status.support_tier,
    supportNote: status.support_note,
    saferNextStep: status.safer_next_step,
    sourceAvailable: status.source_available,
    requiresPreparation: status.requires_preparation,
    prepared: status.prepared,
    analysisPath: status.analysis_path ?? undefined,
    cachePath: status.cache_path ?? undefined,
    cacheSizeBytes: status.cache_size_bytes ?? undefined,
  };
}

export async function fetchImportedRecoverySourceStatus(
  deviceId: string,
): Promise<ImportedRecoverySourceStatus | null> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return loadBrowserPreviewState()?.importedSourceStatusesByDeviceId?.[deviceId] ?? null;
  }

  const status = await invoke<RustImportedRecoverySourceStatus | null>(
    'get_imported_recovery_source_status',
    { deviceId },
  );
  return status ? mapImportedRecoverySourceStatus(status) : null;
}

export async function prepareImportedRecoverySource(
  deviceId: string,
): Promise<ImportedRecoverySourceStatus> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    const seeded = loadBrowserPreviewState()?.importedSourceStatusesByDeviceId?.[deviceId];
    if (seeded) {
      return setBrowserPreviewImportedSourceStatus(deviceId, {
        ...seeded,
        prepared: true,
        analysisPath: seeded.analysisPath ?? seeded.cachePath ?? `/preview-cache/${deviceId}.img`,
        cacheSizeBytes: seeded.cacheSizeBytes ?? 1024 * 1024 * 512,
      });
    }
    throw new Error('Imported-source preparation is only available in the desktop app.');
  }

  const status = await invoke<RustImportedRecoverySourceStatus>(
    'prepare_imported_recovery_source',
    { deviceId },
  );
  return mapImportedRecoverySourceStatus(status);
}

interface RustSmartAttribute {
  id: number;
  name: string;
  value: number;
  worst: number;
  threshold: number;
  raw_value: string;
  status: string;
}

interface RustSmartReport {
  device_id: string;
  overall_health: string;
  temperature_celsius: number | null;
  power_on_hours: number | null;
  reallocated_sectors: number | null;
  pending_sectors: number | null;
  attributes: RustSmartAttribute[] | null;
  error_log_count: number | null;
}

export async function fetchSmartReport(deviceId: string): Promise<SmartReport> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    const previewSmartReport = loadBrowserPreviewState()?.smartReportsByDeviceId?.[deviceId] as
      | SmartReport
      | undefined;
    if (previewSmartReport) {
      return previewSmartReport;
    }
    return {
      deviceId,
      overallHealth: 'unavailable',
      attributes: [],
    };
  }
  const r = await invoke<RustSmartReport>('get_smart_report', { deviceId });
  return {
    deviceId: r.device_id,
    overallHealth: r.overall_health,
    temperatureCelsius: r.temperature_celsius ?? undefined,
    powerOnHours: r.power_on_hours ?? undefined,
    reallocatedSectors: r.reallocated_sectors ?? undefined,
    pendingSectors: r.pending_sectors ?? undefined,
    attributes: (r.attributes || []).map((a) => ({
      id: a.id,
      name: a.name,
      value: a.value,
      worst: a.worst,
      threshold: a.threshold,
      rawValue: a.raw_value,
      status: a.status,
    })),
    errorLogCount: r.error_log_count ?? undefined,
  };
}

interface RustRaidMetadata {
  level: string;
  member_count: number;
  stripe_size_bytes: number | null;
  superblock_version: string | null;
  data_offset_bytes: number | null;
}

interface RustRaidCandidateMember {
  device_id: string;
  device_name: string;
  device_path: string;
}

interface RustRaidCandidate {
  level: string;
  expected_member_count: number;
  detected_member_count: number;
  stripe_size_bytes: number;
  superblock_version: string;
  members: RustRaidCandidateMember[];
}

interface RustRaidAnalysisBuildResult {
  device_id: string;
  path: string;
  display_name: string;
  level: string;
  member_count: number;
  expected_member_count: number;
}

interface RustRaidAnalysisValidation {
  supported: boolean;
  variant: 'supported' | 'degraded' | 'unsupported';
  confidence: 'high' | 'medium' | 'low';
  missing_member_count: number;
  message: string;
  warnings: string[];
}

interface RustEncryptionInfo {
  detected: boolean;
  encryption_type: string;
  can_unlock: boolean;
  workflow_state: 'clear' | 'pre_unlock_blocked' | 'unsupported';
  safer_next_step: string;
  message: string;
}

export async function fetchRaidMetadata(deviceId: string): Promise<RaidMetadata | null> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return loadBrowserPreviewState()?.raidMetadataByDeviceId?.[deviceId] ?? null;
  }
  const metadata = await invoke<RustRaidMetadata | null>('detect_raid_metadata', { deviceId });
  if (!metadata) {
    return null;
  }
  return {
    level: metadata.level,
    memberCount: metadata.member_count,
    stripeSizeBytes: metadata.stripe_size_bytes ?? undefined,
    superblockVersion: metadata.superblock_version ?? undefined,
    dataOffsetBytes: metadata.data_offset_bytes ?? undefined,
  };
}

export async function fetchRaidCandidates(deviceId: string): Promise<RaidCandidate[]> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return [];
  }
  const candidates = await invoke<RustRaidCandidate[]>('scan_raid_candidates', { deviceId });
  return candidates.map((candidate) => ({
    level: candidate.level,
    expectedMemberCount: candidate.expected_member_count,
    detectedMemberCount: candidate.detected_member_count,
    stripeSizeBytes: candidate.stripe_size_bytes,
    superblockVersion: candidate.superblock_version,
    members: candidate.members.map((member) => ({
      deviceId: member.device_id,
      deviceName: member.device_name,
      devicePath: member.device_path,
    })),
  }));
}

export async function buildRaidAnalysisImage(
  deviceId: string,
  request?: RaidAnalysisBuildRequest,
): Promise<RaidAnalysisBuildResult> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    throw new Error('RAID analysis image build is only available in the desktop app.');
  }
  const result = await invoke<RustRaidAnalysisBuildResult>('build_raid_analysis_image', {
    deviceId,
    request: request
      ? {
          level: request.level,
          orderedMemberDeviceIds: request.orderedMemberDeviceIds,
          stripeSizeBytes: request.stripeSizeBytes,
          dataOffsetBytes: request.dataOffsetBytes,
        }
      : null,
  });
  return {
    deviceId: result.device_id,
    path: result.path,
    displayName: result.display_name,
    level: result.level,
    memberCount: result.member_count,
    expectedMemberCount: result.expected_member_count,
  };
}

export async function validateRaidAnalysisBuild(
  deviceId: string,
  request: RaidAnalysisBuildRequest,
): Promise<RaidAnalysisValidation> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    throw new Error('RAID validation is only available in the desktop app.');
  }
  const result = await invoke<RustRaidAnalysisValidation>('validate_raid_analysis_build', {
    deviceId,
    request: {
      level: request.level,
      orderedMemberDeviceIds: request.orderedMemberDeviceIds,
      stripeSizeBytes: request.stripeSizeBytes,
      dataOffsetBytes: request.dataOffsetBytes,
    },
  });
  return {
    supported: result.supported,
    variant: result.variant,
    confidence: result.confidence,
    missingMemberCount: result.missing_member_count,
    message: result.message,
    warnings: result.warnings,
  };
}

export async function fetchEncryptionInfo(deviceId: string): Promise<EncryptionInfo> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return (
      loadBrowserPreviewState()?.encryptionInfoByDeviceId?.[deviceId] ?? {
        detected: false,
        encryptionType: 'unknown',
        canUnlock: false,
        workflowState: 'clear',
        saferNextStep: 'Proceed with the normal read-only diagnostic and scan workflow.',
        message: 'No encryption detected in browser preview.',
      }
    );
  }
  const info = await invoke<RustEncryptionInfo>('get_encryption_info', { deviceId });
  return {
    detected: info.detected,
    encryptionType: info.encryption_type as EncryptionInfo['encryptionType'],
    canUnlock: info.can_unlock,
    workflowState: info.workflow_state,
    saferNextStep: info.safer_next_step,
    message: info.message,
  };
}

export async function unlockEncryptedDevice(deviceId: string, password: string): Promise<string> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    throw new Error('Encrypted-volume unlock is only available in the desktop app.');
  }
  return invoke<string>('unlock_encrypted_device', { deviceId, password });
}
