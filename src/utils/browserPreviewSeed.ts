import type {
  AppBuildInfo,
  DetectedDevice,
  EncryptionInfo,
  ImportedRecoverySourceStatus,
  Language,
  RaidMetadata,
  RecoveryResult,
  RuntimeCapabilities,
  ScanConfig,
  ScanLogEntry,
  ScanProgress,
  ScanSession,
  Theme,
  UserMode,
} from '../types';

export const BROWSER_PREVIEW_STATE_KEY = 'recupere.browser-preview.v1';
export const BROWSER_PREVIEW_STATE_UPDATED_EVENT = 'recupere:browser-preview-updated';

export type BrowserPreviewAppMode = 'not-chosen' | 'ai-guided' | 'manual';
export type BrowserPreviewLicenseStatus = 'free' | 'pro' | 'trial';

export interface BrowserPreviewState {
  appMode?: BrowserPreviewAppMode;
  onboardingComplete?: boolean;
  licenseStatus?: BrowserPreviewLicenseStatus;
  theme?: Theme;
  language?: Language;
  userMode?: UserMode;
  runtimeCapabilities?: RuntimeCapabilities | null;
  devices?: DetectedDevice[];
  selectedDeviceId?: string | null;
  activeScanId?: string | null;
  activeRemoteAgentId?: string | null;
  scanConfig?: ScanConfig | null;
  scanHistory?: ScanSession[];
  scanProgressById?: Record<string, ScanProgress>;
  scanLogsById?: Record<string, ScanLogEntry[]>;
  recoveryResult?: RecoveryResult | null;
  selectedFileIds?: string[];
  buildInfo?: Partial<AppBuildInfo>;
  smartReportsByDeviceId?: Record<string, unknown>;
  raidMetadataByDeviceId?: Record<string, RaidMetadata | null>;
  encryptionInfoByDeviceId?: Record<string, EncryptionInfo>;
  importedSourceStatusesByDeviceId?: Record<string, ImportedRecoverySourceStatus>;
}

export function loadBrowserPreviewState(): BrowserPreviewState | null {
  if (!__ALLOW_BROWSER_PREVIEW__ || typeof window === 'undefined') {
    return null;
  }
  try {
    const raw = window.localStorage.getItem(BROWSER_PREVIEW_STATE_KEY);
    if (!raw) {
      return null;
    }
    return JSON.parse(raw) as BrowserPreviewState;
  } catch {
    return null;
  }
}

export function saveBrowserPreviewState(state: BrowserPreviewState): BrowserPreviewState {
  if (!__ALLOW_BROWSER_PREVIEW__ || typeof window === 'undefined') {
    return state;
  }

  try {
    window.localStorage.setItem(BROWSER_PREVIEW_STATE_KEY, JSON.stringify(state));
    window.dispatchEvent(
      new CustomEvent(BROWSER_PREVIEW_STATE_UPDATED_EVENT, {
        detail: state,
      }),
    );
  } catch {
    // localStorage may be unavailable in browser preview contexts.
  }

  return state;
}

export function updateBrowserPreviewState(
  updater: (current: BrowserPreviewState) => BrowserPreviewState,
): BrowserPreviewState | null {
  if (!__ALLOW_BROWSER_PREVIEW__ || typeof window === 'undefined') {
    return null;
  }

  const nextState = updater(loadBrowserPreviewState() ?? {});
  return saveBrowserPreviewState(nextState);
}

export function getBrowserPreviewDevice(deviceId: string): DetectedDevice | null {
  return loadBrowserPreviewState()?.devices?.find((device) => device.id === deviceId) ?? null;
}

export function getBrowserPreviewImportedSourceStatus(
  deviceId: string,
): ImportedRecoverySourceStatus | null {
  return loadBrowserPreviewState()?.importedSourceStatusesByDeviceId?.[deviceId] ?? null;
}

export function setBrowserPreviewImportedSourceStatus(
  deviceId: string,
  status: ImportedRecoverySourceStatus,
): ImportedRecoverySourceStatus {
  updateBrowserPreviewState((current) => ({
    ...current,
    importedSourceStatusesByDeviceId: {
      ...(current.importedSourceStatusesByDeviceId ?? {}),
      [deviceId]: status,
    },
  }));
  return status;
}

export function getBrowserPreviewRecoveryResult(scanId?: string): RecoveryResult | null {
  const result = loadBrowserPreviewState()?.recoveryResult ?? null;
  if (!result) {
    return null;
  }
  if (scanId && result.scanId !== scanId) {
    return null;
  }
  return result;
}

export function getBrowserPreviewScanProgress(scanId: string): ScanProgress | null {
  return loadBrowserPreviewState()?.scanProgressById?.[scanId] ?? null;
}

export function getBrowserPreviewScanLogs(scanId: string): ScanLogEntry[] {
  return loadBrowserPreviewState()?.scanLogsById?.[scanId] ?? [];
}

export function getBrowserPreviewScanHistory(): ScanSession[] {
  return loadBrowserPreviewState()?.scanHistory ?? [];
}
