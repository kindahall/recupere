import { describe, expect, it } from 'vitest';
import {
  buildPotentialVolumeScanConfig,
  isPotentialVolumeRecoverySupported,
  sortPotentialVolumeCandidates,
} from './potentialVolumeRecovery';

describe('potentialVolumeRecovery', () => {
  it('recognizes supported recovered-volume filesystems', () => {
    expect(isPotentialVolumeRecoverySupported('fat32')).toBe(true);
    expect(isPotentialVolumeRecoverySupported('exfat')).toBe(true);
    expect(isPotentialVolumeRecoverySupported('ntfs')).toBe(true);
    expect(isPotentialVolumeRecoverySupported('hfs+')).toBe(true);
    expect(isPotentialVolumeRecoverySupported('apfs')).toBe(true);
  });

  it('builds a lost-volume scan config from a candidate', () => {
    expect(
      buildPotentialVolumeScanConfig(
        'disk-test',
        {
          id: 'pv-1',
          label: 'Recovered FAT32',
          filesystem: 'fat32',
          startOffset: 1048576,
          sizeBytes: 8388608,
        },
        {
          imagingRequiresElevation: true,
          imagingProfile: 'cautious',
          imagingProfileReasonKey: 'imaging.profile_reason_risk',
        },
      ),
    ).toEqual({
      deviceId: 'disk-test',
      scanType: 'lost-volume',
      targetFilesystems: ['fat32'],
      enableCarving: true,
      imagingRequiresElevation: true,
      imagingProfile: 'cautious',
      imagingProfileReasonKey: 'imaging.profile_reason_risk',
      potentialVolumeId: 'pv-1',
      potentialVolumeLabel: 'Recovered FAT32',
      potentialVolumeFilesystem: 'fat32',
      potentialVolumeStartOffset: 1048576,
      potentialVolumeSizeBytes: 8388608,
    });
  });

  it('sorts recovered-volume candidates by recommendation, support, and confidence', () => {
    expect(
      sortPotentialVolumeCandidates(
        [
          {
            id: 'pv-unsupported',
            label: 'APFS candidate',
            filesystem: 'apfs',
            startOffset: 2097152,
            sizeBytes: 4194304,
            confidenceScore: 99,
            detectionMethod: 'gpt',
            notes: [],
          },
          {
            id: 'pv-weaker',
            label: 'Weaker FAT32',
            filesystem: 'fat32',
            startOffset: 3145728,
            sizeBytes: 4194304,
            confidenceScore: 74,
            detectionMethod: 'boot-signature',
            notes: [],
          },
          {
            id: 'pv-best',
            label: 'Best NTFS',
            filesystem: 'ntfs',
            startOffset: 1048576,
            sizeBytes: 8388608,
            confidenceScore: 90,
            detectionMethod: 'mbr',
            notes: [],
          },
        ],
        'pv-weaker',
      ).map((volume) => volume.id),
    ).toEqual(['pv-weaker', 'pv-unsupported', 'pv-best']);
  });
});
