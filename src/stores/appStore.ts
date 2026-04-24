import { create } from 'zustand';
import type {
  DetectedDevice,
  DiagnosticResult,
  ExportProgress,
  FilesystemMemoryComparison,
  Language,
  RecoveryResult,
  RuntimeCapabilities,
  ScanConfig,
  ScanLogEntry,
  ScanProgress,
  ScanSession,
  Theme,
  UserMode,
} from '../types';

const TERMINAL_SCAN_STATUSES = new Set(['completed', 'cancelled', 'error']);
export function isTerminalScanStatus(status: string | undefined): boolean {
  return status ? TERMINAL_SCAN_STATUSES.has(status) : false;
}
import { loadBrowserPreviewState } from '../utils/browserPreviewSeed';

// ============================================================================
// App Store — Global application state
// ============================================================================

type AppMode = 'not-chosen' | 'ai-guided' | 'manual';
type LicenseStatus = 'free' | 'pro' | 'trial';

export interface TrackedScan {
  id: string;
  /// `null` for local scans, agent id for remote scans.
  agentId: string | null;
  /// Human-friendly label shown in the picker.
  label: string;
  /// Scan type as it was started (`quick`, `deep`, `carving`, ...).
  scanType: string;
  /// Wall-clock time the scan was registered.
  startedAtMs: number;
}

interface AppState {
  // App Mode & License
  appMode: AppMode;
  licenseStatus: LicenseStatus;
  onboardingComplete: boolean;
  setAppMode: (mode: AppMode) => void;
  setLicenseStatus: (status: LicenseStatus) => void;
  setOnboardingComplete: (complete: boolean) => void;

  // UI State
  theme: Theme;
  language: Language;
  userMode: UserMode;
  sidebarCollapsed: boolean;
  runtimeCapabilities: RuntimeCapabilities | null;
  runtimeCapabilitiesLoading: boolean;

  // Device State
  devices: DetectedDevice[];
  selectedDeviceId: string | null;
  devicesLoading: boolean;

  // Diagnostic State
  diagnosticResult: DiagnosticResult | null;
  diagnosticLoading: boolean;

  // Scan State
  activeScanId: string | null;
  // When set, the active scan lives on a remote `recupere-agent`. Local
  // scans leave this `null`. The Scan/Results pages branch their IPC calls
  // to the remote dispatcher when this is non-null.
  activeRemoteAgentId: string | null;
  // Tracked scans — both local and remote — kept around so the user can
  // hop between them without losing context. The `id` matches the scan's
  // backend id. `agentId === null` means "local engine". The list is
  // intentionally a flat array (small N, easy serialization) and lives
  // only in memory; persistence is out of scope for V1 multi-scan.
  trackedScans: TrackedScan[];
  // Per-scan progress maps — the background poller writes here for ALL
  // tracked scans, not just the focused one. The legacy single-field
  // `scanProgress` / `scanLogs` mirror the focused scan's entry.
  progressByScanId: Record<string, ScanProgress>;
  logsByScanId: Record<string, ScanLogEntry[]>;
  resultsByScanId: Record<string, RecoveryResult>;
  scanConfig: ScanConfig | null;
  scanProgress: ScanProgress | null;
  scanLogs: ScanLogEntry[];

  // Results State
  recoveryResult: RecoveryResult | null;
  selectedFileIds: Set<string>;
  activeExportId: string | null;
  exportProgress: ExportProgress | null;
  exportLogs: ScanLogEntry[];
  filesystemMemoryComparison: FilesystemMemoryComparison | null;

  // Toast State
  toasts: { id: string; variant: 'success' | 'info' | 'warning' | 'error'; message: string }[];
  addToast: (variant: 'success' | 'info' | 'warning' | 'error', message: string) => void;
  removeToast: (id: string) => void;

  // History
  scanHistory: ScanSession[];

  // Actions — UI
  setTheme: (theme: Theme) => void;
  setLanguage: (lang: Language) => void;
  setUserMode: (mode: UserMode) => void;
  toggleSidebar: () => void;
  setRuntimeCapabilities: (capabilities: RuntimeCapabilities | null) => void;
  setRuntimeCapabilitiesLoading: (loading: boolean) => void;

  // Actions — Devices
  setDevices: (devices: DetectedDevice[]) => void;
  selectDevice: (deviceId: string | null) => void;
  setDevicesLoading: (loading: boolean) => void;

  // Actions — Diagnostic
  setDiagnosticResult: (result: DiagnosticResult | null) => void;
  setDiagnosticLoading: (loading: boolean) => void;

  // Actions — Scan
  setActiveScanId: (scanId: string | null) => void;
  setActiveRemoteAgentId: (agentId: string | null) => void;
  trackScan: (scan: TrackedScan) => void;
  untrackScan: (scanId: string) => void;
  focusTrackedScan: (scanId: string) => void;
  updateTrackedScanProgress: (scanId: string, progress: ScanProgress) => void;
  updateTrackedScanLogs: (scanId: string, logs: ScanLogEntry[]) => void;
  updateTrackedScanResults: (scanId: string, result: RecoveryResult) => void;
  setScanConfig: (config: ScanConfig | null) => void;
  setScanProgress: (progress: ScanProgress | null) => void;
  setScanLogs: (logs: ScanLogEntry[]) => void;
  clearScanLogs: () => void;

  // Actions — Results
  setRecoveryResult: (result: RecoveryResult | null) => void;
  toggleFileSelection: (fileId: string) => void;
  selectAllFiles: () => void;
  deselectAllFiles: () => void;
  setActiveExportId: (exportId: string | null) => void;
  setExportProgress: (progress: ExportProgress | null) => void;
  setExportLogs: (logs: ScanLogEntry[]) => void;
  clearExportLogs: () => void;
  setFilesystemMemoryComparison: (comparison: FilesystemMemoryComparison | null) => void;

  // Actions — History
  setScanHistory: (history: ScanSession[]) => void;
  addScanSession: (session: ScanSession) => void;
}

// ---------------------------------------------------------------------------
// Lightweight localStorage persistence for onboarding-related fields only.
// We don't pull in zustand/persist to keep the bundle small and avoid
// rehydrating stateful runtime fields (devices, scans, etc.).
// ---------------------------------------------------------------------------
const ONBOARDING_STORAGE_KEY = 'recupere.onboarding.v1';
type OnboardingPersisted = { appMode: AppMode; onboardingComplete: boolean };

function loadPersistedOnboarding(): OnboardingPersisted {
  if (typeof window === 'undefined') {
    return { appMode: 'not-chosen', onboardingComplete: false };
  }
  try {
    const raw = window.localStorage.getItem(ONBOARDING_STORAGE_KEY);
    if (!raw) return { appMode: 'not-chosen', onboardingComplete: false };
    const parsed = JSON.parse(raw) as Partial<OnboardingPersisted>;
    return {
      appMode: parsed.appMode ?? 'not-chosen',
      onboardingComplete: parsed.onboardingComplete ?? false,
    };
  } catch {
    return { appMode: 'not-chosen', onboardingComplete: false };
  }
}

function savePersistedOnboarding(state: OnboardingPersisted) {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(ONBOARDING_STORAGE_KEY, JSON.stringify(state));
  } catch {
    // localStorage may be unavailable (private mode); ignore.
  }
}

const persistedOnboarding = loadPersistedOnboarding();
const browserPreviewState = loadBrowserPreviewState();

export const useAppStore = create<AppState>((set, get) => ({
  // App Mode & License
  appMode: browserPreviewState?.appMode ?? persistedOnboarding.appMode,
  licenseStatus: browserPreviewState?.licenseStatus ?? 'free',
  onboardingComplete:
    browserPreviewState?.onboardingComplete ?? persistedOnboarding.onboardingComplete,
  setAppMode: (appMode) => {
    set({ appMode });
    savePersistedOnboarding({
      appMode,
      onboardingComplete: get().onboardingComplete,
    });
  },
  setLicenseStatus: (licenseStatus) => set({ licenseStatus }),
  setOnboardingComplete: (onboardingComplete) => {
    set({ onboardingComplete });
    savePersistedOnboarding({
      appMode: get().appMode,
      onboardingComplete,
    });
  },

  // Defaults
  theme: browserPreviewState?.theme ?? 'light',
  language: browserPreviewState?.language ?? 'en',
  userMode: browserPreviewState?.userMode ?? 'novice',
  sidebarCollapsed: false,
  runtimeCapabilities: browserPreviewState?.runtimeCapabilities ?? null,
  runtimeCapabilitiesLoading: false,

  devices: browserPreviewState?.devices ?? [],
  selectedDeviceId: browserPreviewState?.selectedDeviceId ?? null,
  devicesLoading: false,

  diagnosticResult: null,
  diagnosticLoading: false,

  activeScanId: browserPreviewState?.activeScanId ?? null,
  activeRemoteAgentId: browserPreviewState?.activeRemoteAgentId ?? null,
  trackedScans: [],
  progressByScanId: {},
  logsByScanId: {},
  resultsByScanId: {},
  scanConfig: browserPreviewState?.scanConfig ?? null,
  scanProgress: null,
  scanLogs: [],

  recoveryResult: browserPreviewState?.recoveryResult ?? null,
  selectedFileIds: new Set(browserPreviewState?.selectedFileIds ?? []),
  activeExportId: null,
  exportProgress: null,
  exportLogs: [],
  filesystemMemoryComparison: null,

  toasts: [],

  scanHistory: [],

  // UI actions
  setTheme: (theme) => {
    document.documentElement.setAttribute('data-theme', theme);
    set({ theme });
  },
  setLanguage: (language) => set({ language }),
  setUserMode: (userMode) => set({ userMode }),
  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
  setRuntimeCapabilities: (runtimeCapabilities) => set({ runtimeCapabilities }),
  setRuntimeCapabilitiesLoading: (runtimeCapabilitiesLoading) =>
    set({ runtimeCapabilitiesLoading }),

  // Device actions
  setDevices: (devices) => set({ devices }),
  selectDevice: (deviceId) =>
    set({
      selectedDeviceId: deviceId,
      diagnosticResult: null,
      activeScanId: null,
      activeRemoteAgentId: null,
      scanProgress: null,
      scanLogs: [],
      recoveryResult: null,
      selectedFileIds: new Set(),
      activeExportId: null,
      exportProgress: null,
      exportLogs: [],
    }),
  setDevicesLoading: (loading) => set({ devicesLoading: loading }),

  // Diagnostic actions
  setDiagnosticResult: (result) => set({ diagnosticResult: result }),
  setDiagnosticLoading: (loading) => set({ diagnosticLoading: loading }),

  // Scan actions
  setActiveScanId: (activeScanId) => set({ activeScanId }),
  setActiveRemoteAgentId: (activeRemoteAgentId) => set({ activeRemoteAgentId }),
  trackScan: (scan) =>
    set((s) => ({
      trackedScans: [scan, ...s.trackedScans.filter((existing) => existing.id !== scan.id)],
    })),
  untrackScan: (scanId) =>
    set((s) => ({
      trackedScans: s.trackedScans.filter((scan) => scan.id !== scanId),
    })),
  focusTrackedScan: (scanId) =>
    set((s) => {
      const target = s.trackedScans.find((scan) => scan.id === scanId);
      if (!target) return {};
      return {
        activeScanId: target.id,
        activeRemoteAgentId: target.agentId,
        // Restore cached progress / results from the per-scan maps so the
        // page renders instantly without waiting for the next poll cycle.
        scanProgress: s.progressByScanId[scanId] ?? null,
        scanLogs: s.logsByScanId[scanId] ?? [],
        recoveryResult: s.resultsByScanId[scanId] ?? null,
        selectedFileIds: new Set(),
      };
    }),
  updateTrackedScanProgress: (scanId, progress) =>
    set((s) => {
      const next = { ...s.progressByScanId, [scanId]: progress };
      // If this is the focused scan, also mirror into the legacy field.
      if (s.activeScanId === scanId) {
        return { progressByScanId: next, scanProgress: progress };
      }
      return { progressByScanId: next };
    }),
  updateTrackedScanLogs: (scanId, logs) =>
    set((s) => {
      const next = { ...s.logsByScanId, [scanId]: logs };
      if (s.activeScanId === scanId) {
        return { logsByScanId: next, scanLogs: logs };
      }
      return { logsByScanId: next };
    }),
  updateTrackedScanResults: (scanId, result) =>
    set((s) => {
      const next = { ...s.resultsByScanId, [scanId]: result };
      if (s.activeScanId === scanId) {
        return { resultsByScanId: next, recoveryResult: result };
      }
      return { resultsByScanId: next };
    }),
  setScanConfig: (config) => set({ scanConfig: config }),
  setScanProgress: (progress) => set({ scanProgress: progress }),
  setScanLogs: (scanLogs) => set({ scanLogs }),
  clearScanLogs: () => set({ scanLogs: [] }),

  // Results actions
  setRecoveryResult: (result) =>
    set({
      recoveryResult: result,
      selectedFileIds: new Set(),
      activeExportId: null,
      exportProgress: null,
      exportLogs: [],
    }),
  toggleFileSelection: (fileId) =>
    set((s) => {
      const next = new Set(s.selectedFileIds);
      if (next.has(fileId)) next.delete(fileId);
      else next.add(fileId);
      return { selectedFileIds: next };
    }),
  selectAllFiles: () => {
    const result = get().recoveryResult;
    if (!result) return;
    set({ selectedFileIds: new Set(result.files.map((f) => f.id)) });
  },
  deselectAllFiles: () => set({ selectedFileIds: new Set() }),
  setActiveExportId: (activeExportId) => set({ activeExportId }),
  setExportProgress: (exportProgress) => set({ exportProgress }),
  setExportLogs: (exportLogs) => set({ exportLogs }),
  clearExportLogs: () => set({ exportLogs: [] }),
  setFilesystemMemoryComparison: (filesystemMemoryComparison) =>
    set({ filesystemMemoryComparison }),

  // Toast actions
  addToast: (variant, message) =>
    set((s) => ({
      toasts: [...s.toasts, { id: crypto.randomUUID(), variant, message }],
    })),
  removeToast: (id) =>
    set((s) => ({
      toasts: s.toasts.filter((t) => t.id !== id),
    })),

  // History actions
  setScanHistory: (history) => set({ scanHistory: history }),
  addScanSession: (session) =>
    set((s) => ({
      scanHistory: [session, ...s.scanHistory],
    })),
}));
