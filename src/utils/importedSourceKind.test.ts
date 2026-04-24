import { describe, expect, it } from 'vitest';
import { getImportedSourceKind, isRaidImportedSource } from './importedSourceKind';

describe('importedSourceKind', () => {
  it('classifies forensic evidence images', () => {
    expect(
      getImportedSourceKind(
        {
          displayName: 'case.E01',
          sourcePath: '/cases/case.E01',
          sourceFormat: 'E01',
          logicalSizeBytes: 1024,
          sourceAvailable: true,
          requiresPreparation: true,
          prepared: false,
        },
        null,
      ),
    ).toBe('forensic-image');
  });

  it('classifies reconstructed RAID sources', () => {
    const status = {
      displayName: 'RAID 5 analysis',
      sourcePath: '/cases/raid5.img',
      sourceFormat: 'RAID5',
      logicalSizeBytes: 1024,
      sourceAvailable: true,
      requiresPreparation: false,
      prepared: true,
    };

    expect(
      getImportedSourceKind(status, {
        id: 'dev-1',
        name: 'Imported RAID 5 source',
        devicePath: '/cases/raid5.img',
        type: 'image',
        filesystem: 'unknown',
        capacityBytes: 1024,
        usedBytes: 0,
        status: 'healthy',
        riskLevel: 'low',
        model: 'Imported RAID 5 Analysis Image',
        partitions: [],
      }),
    ).toBe('raid-analysis');
    expect(isRaidImportedSource(status, null)).toBe(true);
  });
});
