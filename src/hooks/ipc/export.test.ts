import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ExportConfig } from '../../types';
import {
  fetchExportLogs,
  fetchExportProgress,
  startExport,
  validateExportDestination,
} from './export';

describe('validateExportDestination (browser preview)', () => {
  it('rejects an empty destination with a clear message', async () => {
    const result = await validateExportDestination('', '/Volumes/source');
    expect(result.isSafe).toBe(false);
    expect(result.message).toMatch(/Choose an export destination/i);
  });

  it('flags the source device as an unsafe destination', async () => {
    const result = await validateExportDestination('/Volumes/source', '/Volumes/source');
    expect(result.isSafe).toBe(false);
    expect(result.message).toMatch(/cannot be the source device/i);
  });

  it('flags any sub-path of the source device as unsafe', async () => {
    const result = await validateExportDestination('/Volumes/source/recovered', '/Volumes/source');
    expect(result.isSafe).toBe(false);
  });

  it('accepts a distinct safe destination', async () => {
    const result = await validateExportDestination('/Volumes/backup', '/Volumes/source');
    expect(result.isSafe).toBe(true);
    expect(result.availableBytes).toBeGreaterThan(0);
  });

  it('normalises Windows backslashes before comparing paths', async () => {
    const result = await validateExportDestination(
      'C:\\Volumes\\source\\recovered',
      'C:/Volumes/source',
    );
    expect(result.isSafe).toBe(false);
  });
});

describe('startExport + fetchExportProgress (browser preview)', () => {
  const config: ExportConfig = {
    destinationPath: '/tmp/recovered',
    selectedFileIds: ['file-a', 'file-b'],
    conflictStrategy: 'rename',
    preserveStructure: true,
    verifyIntegrity: true,
    explicitSelection: true,
    implicitPreviewFirstExcludedCount: 0,
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('returns a unique preview export id each call', async () => {
    const first = await startExport('scan-1', config);
    const second = await startExport('scan-2', config);
    expect(first).toMatch(/^preview-export-\d+$/);
    expect(second).toMatch(/^preview-export-\d+$/);
    expect(first).not.toBe(second);
  });

  it('surfaces a completed progress snapshot for the returned id', async () => {
    const exportId = await startExport('scan-3', config);
    const progress = await fetchExportProgress(exportId);
    expect(progress.status).toBe('completed');
    expect(progress.totalFiles).toBeGreaterThan(0);
    expect(progress.exportedFiles).toBe(progress.totalFiles);
    expect(progress.errors).toEqual([]);
  });

  it('returns info-level preview logs for the returned id', async () => {
    const exportId = await startExport('scan-4', config);
    const logs = await fetchExportLogs(exportId);
    expect(logs.length).toBeGreaterThan(0);
    expect(logs.every((entry) => entry.level === 'info')).toBe(true);
    expect(logs[logs.length - 1].message).toContain('/tmp/recovered');
  });

  it('returns a zeroed progress snapshot for an unknown preview id', async () => {
    const unknownProgress = await fetchExportProgress('preview-export-does-not-exist');
    expect(unknownProgress.status).toBe('completed');
    expect(unknownProgress.totalFiles).toBe(0);
    expect(unknownProgress.exportedBytes).toBe(0);
  });
});
