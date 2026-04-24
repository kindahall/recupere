import type { DetectedDevice, ImportedRecoverySourceStatus } from '../types';

function normalize(value: string | undefined): string {
  return (value ?? '').trim().toUpperCase();
}

export type ImportedSourceKind =
  | 'raid-analysis'
  | 'forensic-image'
  | 'virtual-disk'
  | 'raw-image'
  | 'generic-image';

export function getImportedSourceKind(
  status: ImportedRecoverySourceStatus | null | undefined,
  device: DetectedDevice | null | undefined,
): ImportedSourceKind {
  const sourceFormat = normalize(status?.sourceFormat);
  const model = normalize(device?.model);

  if (sourceFormat.startsWith('RAID') || model.includes('IMPORTED RAID')) {
    return 'raid-analysis';
  }

  if (sourceFormat === 'E01') {
    return 'forensic-image';
  }

  if (sourceFormat === 'VMDK' || sourceFormat === 'VHD' || sourceFormat === 'VHDX') {
    return 'virtual-disk';
  }

  if (
    sourceFormat === 'RAW' ||
    sourceFormat === 'IMG' ||
    sourceFormat === 'DD' ||
    sourceFormat === 'BIN'
  ) {
    return 'raw-image';
  }

  return 'generic-image';
}

export function isRaidImportedSource(
  status: ImportedRecoverySourceStatus | null | undefined,
  device: DetectedDevice | null | undefined,
): boolean {
  return getImportedSourceKind(status, device) === 'raid-analysis';
}
