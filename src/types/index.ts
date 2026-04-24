// ============================================================================
// Shared TypeScript Types — Barrel Export
// ============================================================================

export * from './device';
export * from './scan';
export * from './results';
export * from './diagnostic';
export * from './ai';
export * from './ipc';
export * from './filesystemMemory';
export * from './audit';

// ============================================================================
// App-level types
// ============================================================================

export type UserMode = 'novice' | 'expert';

export type Theme = 'light' | 'dark';

export type Language = 'en' | 'fr';

export type PageState =
  | 'empty'
  | 'loading'
  | 'ready'
  | 'scanning'
  | 'paused'
  | 'partial-success'
  | 'success'
  | 'warning'
  | 'critical'
  | 'error'
  | 'no-preview';

export type RuntimeCapabilityKey =
  | 'deviceDetection'
  | 'heuristicDiagnostic'
  | 'aiAdvisory'
  | 'optionalCloudAi'
  | 'scanEngine'
  | 'imagingEngine'
  | 'resultsBrowser'
  | 'exportValidation'
  | 'exportEngine'
  | 'technicalLogs';

export interface AppSettings {
  theme: Theme;
  language: Language;
  userMode: UserMode;
  cloudEnabled: boolean;
  cloudApiKey?: string;
  cachePath?: string;
  maxCacheSizeMb: number;
}

export interface RuntimeCapabilities {
  deviceDetection: boolean;
  heuristicDiagnostic: boolean;
  aiAdvisory: boolean;
  optionalCloudAi: boolean;
  scanEngine: boolean;
  imagingEngine: boolean;
  resultsBrowser: boolean;
  exportValidation: boolean;
  exportEngine: boolean;
  technicalLogs: boolean;
  limitedCapabilities?: RuntimeCapabilityKey[];
}

export interface AppBuildInfo {
  productName: string;
  bundleIdentifier: string;
  appVersion: string;
  packageName: string;
  buildProfile: string;
  operatingSystem: string;
  architecture: string;
  targetTriple: string;
  tauriRuntime: string;
}
