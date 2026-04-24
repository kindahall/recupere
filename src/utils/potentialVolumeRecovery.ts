import type { FilesystemType, PotentialVolume } from '../types/device';
import type { ImagingProfile, ScanConfig } from '../types/scan';

export type PotentialVolumeRecoveryTarget = Pick<
  PotentialVolume,
  'id' | 'label' | 'filesystem' | 'startOffset' | 'sizeBytes'
>;

export interface PotentialVolumeScanOptions {
  imagingRequiresElevation?: boolean;
  imagingProfile?: ImagingProfile;
  imagingProfileReasonKey?: string;
}

export function isPotentialVolumeRecoverySupported(filesystem: FilesystemType): boolean {
  return (
    filesystem === 'ntfs' ||
    filesystem === 'fat32' ||
    filesystem === 'exfat' ||
    filesystem === 'hfs+' ||
    filesystem === 'apfs'
  );
}

export function buildPotentialVolumeScanConfig(
  deviceId: string,
  volume: PotentialVolumeRecoveryTarget,
  options: PotentialVolumeScanOptions = {},
): ScanConfig {
  return {
    deviceId,
    scanType: 'lost-volume',
    targetFilesystems: [volume.filesystem],
    enableCarving: true,
    imagingRequiresElevation: options.imagingRequiresElevation ?? false,
    imagingProfile: options.imagingProfile,
    imagingProfileReasonKey: options.imagingProfileReasonKey,
    potentialVolumeId: volume.id,
    potentialVolumeLabel: volume.label,
    potentialVolumeFilesystem: volume.filesystem,
    potentialVolumeStartOffset: volume.startOffset,
    potentialVolumeSizeBytes: volume.sizeBytes,
  };
}

function detectionMethodRank(detectionMethod: string): number {
  switch (detectionMethod) {
    case 'gpt':
      return 3;
    case 'mbr':
      return 2;
    case 'gpt-backup':
      return 1;
    default:
      return 0;
  }
}

export function sortPotentialVolumeCandidates(
  volumes: PotentialVolume[],
  recommendedVolumeId?: string,
): PotentialVolume[] {
  return [...volumes].sort((left, right) => {
    const leftRecommended = left.id === recommendedVolumeId ? 1 : 0;
    const rightRecommended = right.id === recommendedVolumeId ? 1 : 0;
    if (leftRecommended !== rightRecommended) {
      return rightRecommended - leftRecommended;
    }

    const leftSupported = isPotentialVolumeRecoverySupported(left.filesystem) ? 1 : 0;
    const rightSupported = isPotentialVolumeRecoverySupported(right.filesystem) ? 1 : 0;
    if (leftSupported !== rightSupported) {
      return rightSupported - leftSupported;
    }

    if (left.confidenceScore !== right.confidenceScore) {
      return right.confidenceScore - left.confidenceScore;
    }

    const leftDetectionRank = detectionMethodRank(left.detectionMethod);
    const rightDetectionRank = detectionMethodRank(right.detectionMethod);
    if (leftDetectionRank !== rightDetectionRank) {
      return rightDetectionRank - leftDetectionRank;
    }

    return left.startOffset - right.startOffset;
  });
}
