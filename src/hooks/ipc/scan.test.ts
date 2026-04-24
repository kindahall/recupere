import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));

import { invoke } from '@tauri-apps/api/core';
import { cancelScan, pauseScan, resumeScan } from './scan';

const invokeMock = invoke as unknown as ReturnType<typeof vi.fn>;

describe('pauseScan / resumeScan / cancelScan (native runtime)', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it('pauseScan forwards the scanId to the Tauri command', async () => {
    await pauseScan('scan-42');
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith('pause_scan', { scanId: 'scan-42' });
  });

  it('resumeScan forwards the scanId to the Tauri command', async () => {
    await resumeScan('scan-42');
    expect(invokeMock).toHaveBeenCalledWith('resume_scan', { scanId: 'scan-42' });
  });

  it('cancelScan forwards the scanId to the Tauri command', async () => {
    await cancelScan('scan-42');
    expect(invokeMock).toHaveBeenCalledWith('cancel_scan', { scanId: 'scan-42' });
  });

  it('propagates Tauri invoke failures to the caller', async () => {
    invokeMock.mockRejectedValueOnce(new Error('permission denied'));
    await expect(cancelScan('scan-42')).rejects.toThrow('permission denied');
  });
});
