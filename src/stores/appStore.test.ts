import { beforeEach, describe, expect, it } from 'vitest';
import type {
  DetectedDevice,
  ExportProgress,
  RecoveryResult,
  RuntimeCapabilities,
  ScanLogEntry,
  ScanProgress,
} from '../types';
import { useAppStore } from './appStore';

const sampleDevice: DetectedDevice = {
  id: 'device-1',
  name: 'Case Disk',
  devicePath: '/dev/disk9',
  type: 'hdd',
  filesystem: 'exfat',
  capacityBytes: 1024,
  usedBytes: 512,
  status: 'healthy',
  riskLevel: 'low',
  partitions: [],
};

const sampleProgress: ScanProgress = {
  status: 'scanning',
  stage: 'analyzing-filesystem',
  percentComplete: 25,
  bytesScanned: 256,
  totalBytes: 1024,
  filesFound: 4,
  errorsCount: 0,
  elapsedSeconds: 3,
};

const sampleRecoveryResult: RecoveryResult = {
  scanId: 'scan-1',
  totalFiles: 1,
  intactFiles: 1,
  partialFiles: 0,
  fragmentedFiles: 0,
  corruptFiles: 0,
  totalSizeBytes: 128,
  recoverableSizeBytes: 128,
  files: [
    {
      id: 'file-1',
      name: 'report.txt',
      path: '/',
      extension: 'txt',
      sizeBytes: 128,
      integrity: 'intact',
      recoveryScore: 100,
      recoveryMethod: 'filesystem',
      previewAvailable: true,
      mimeType: 'text/plain',
    },
  ],
  treeRoot: {
    id: 'root',
    name: '/',
    isDirectory: true,
    children: [],
  },
};

const sampleExportProgress: ExportProgress = {
  totalFiles: 1,
  exportedFiles: 0,
  totalBytes: 128,
  exportedBytes: 0,
  currentFile: 'report.txt',
  errors: [],
  status: 'exporting',
};

const sampleCapabilities: RuntimeCapabilities = {
  deviceDetection: true,
  heuristicDiagnostic: true,
  aiAdvisory: true,
  optionalCloudAi: false,
  scanEngine: true,
  imagingEngine: false,
  resultsBrowser: true,
  exportValidation: true,
  exportEngine: true,
  technicalLogs: true,
};

function resetStore() {
  useAppStore.setState({
    theme: 'light',
    language: 'en',
    userMode: 'novice',
    sidebarCollapsed: false,
    runtimeCapabilities: null,
    runtimeCapabilitiesLoading: false,
    devices: [],
    selectedDeviceId: null,
    devicesLoading: false,
    diagnosticResult: null,
    diagnosticLoading: false,
    activeScanId: null,
    scanConfig: null,
    scanProgress: null,
    scanLogs: [],
    recoveryResult: null,
    selectedFileIds: new Set(),
    activeExportId: null,
    exportProgress: null,
    exportLogs: [],
    scanHistory: [],
  });
}

describe('useAppStore safety resets', () => {
  beforeEach(() => {
    resetStore();
  });

  it('selectDevice clears active scan, results, logs and export state', () => {
    const backendLogs: ScanLogEntry[] = [
      { timestampMs: 1, level: 'info', message: 'scan started' },
    ];
    const exportLogs: ScanLogEntry[] = [
      { timestampMs: 2, level: 'info', message: 'export started' },
    ];

    useAppStore.setState({
      devices: [sampleDevice],
      runtimeCapabilities: sampleCapabilities,
      activeScanId: 'scan-1',
      scanProgress: sampleProgress,
      scanLogs: backendLogs,
      recoveryResult: sampleRecoveryResult,
      selectedFileIds: new Set(['file-1']),
      activeExportId: 'export-1',
      exportProgress: sampleExportProgress,
      exportLogs,
    });

    useAppStore.getState().selectDevice('device-1');
    const state = useAppStore.getState();

    expect(state.selectedDeviceId).toBe('device-1');
    expect(state.activeScanId).toBeNull();
    expect(state.scanProgress).toBeNull();
    expect(state.scanLogs).toEqual([]);
    expect(state.recoveryResult).toBeNull();
    expect(state.selectedFileIds.size).toBe(0);
    expect(state.activeExportId).toBeNull();
    expect(state.exportProgress).toBeNull();
    expect(state.exportLogs).toEqual([]);
  });

  it('setRecoveryResult clears existing selection and export progress', () => {
    useAppStore.setState({
      selectedFileIds: new Set(['file-1']),
      activeExportId: 'export-1',
      exportProgress: sampleExportProgress,
    });

    useAppStore.getState().setRecoveryResult(sampleRecoveryResult);
    const state = useAppStore.getState();

    expect(state.recoveryResult?.scanId).toBe('scan-1');
    expect(state.selectedFileIds.size).toBe(0);
    expect(state.activeExportId).toBeNull();
    expect(state.exportProgress).toBeNull();
    expect(state.exportLogs).toEqual([]);
  });

  it('setScanLogs replaces the current backend log snapshot', () => {
    useAppStore.getState().setScanLogs([{ timestampMs: 1, level: 'info', message: 'old-log' }]);

    useAppStore.getState().setScanLogs([{ timestampMs: 2, level: 'warning', message: 'new-log' }]);

    expect(useAppStore.getState().scanLogs).toEqual([
      { timestampMs: 2, level: 'warning', message: 'new-log' },
    ]);
  });

  it('setExportLogs replaces the current export backend log snapshot', () => {
    useAppStore
      .getState()
      .setExportLogs([{ timestampMs: 3, level: 'info', message: 'old-export-log' }]);

    useAppStore
      .getState()
      .setExportLogs([{ timestampMs: 4, level: 'error', message: 'new-export-log' }]);

    expect(useAppStore.getState().exportLogs).toEqual([
      { timestampMs: 4, level: 'error', message: 'new-export-log' },
    ]);
  });
});
