import { invoke, isTauri } from '@tauri-apps/api/core';
import type { AppBuildInfo, RuntimeCapabilities, RuntimeCapabilityKey } from '../../types';
import { loadBrowserPreviewState } from '../../utils/browserPreviewSeed';

interface RustRuntimeCapabilities {
  device_detection: boolean;
  heuristic_diagnostic: boolean;
  ai_advisory: boolean;
  optional_cloud_ai: boolean;
  scan_engine: boolean;
  imaging_engine: boolean;
  results_browser: boolean;
  export_validation: boolean;
  export_engine: boolean;
  technical_logs: boolean;
  limited_capabilities?: string[];
}

interface RustAppBuildInfo {
  product_name: string;
  bundle_identifier: string;
  app_version: string;
  package_name: string;
  build_profile: string;
  operating_system: string;
  architecture: string;
  target_triple: string;
  tauri_runtime: string;
}

export async function fetchRuntimeCapabilities(): Promise<RuntimeCapabilities> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return (
      loadBrowserPreviewState()?.runtimeCapabilities ?? {
        deviceDetection: false,
        heuristicDiagnostic: false,
        aiAdvisory: false,
        optionalCloudAi: false,
        scanEngine: false,
        imagingEngine: false,
        resultsBrowser: true,
        exportValidation: false,
        exportEngine: false,
        technicalLogs: false,
        limitedCapabilities: [],
      }
    );
  }
  const c = await invoke<RustRuntimeCapabilities>('get_runtime_capabilities');
  return {
    deviceDetection: c.device_detection,
    heuristicDiagnostic: c.heuristic_diagnostic,
    aiAdvisory: c.ai_advisory,
    optionalCloudAi: c.optional_cloud_ai,
    scanEngine: c.scan_engine,
    imagingEngine: c.imaging_engine,
    resultsBrowser: c.results_browser,
    exportValidation: c.export_validation,
    exportEngine: c.export_engine,
    technicalLogs: c.technical_logs,
    limitedCapabilities: (c.limited_capabilities ?? []) as RuntimeCapabilityKey[],
  };
}

export async function fetchAppBuildInfo(): Promise<AppBuildInfo> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    const buildInfo = loadBrowserPreviewState()?.buildInfo;
    return {
      productName: buildInfo?.productName ?? 'Recupere',
      bundleIdentifier: buildInfo?.bundleIdentifier ?? 'com.recupere.browser-preview',
      appVersion: buildInfo?.appVersion ?? __APP_VERSION__,
      packageName: buildInfo?.packageName ?? 'recupere',
      buildProfile: buildInfo?.buildProfile ?? 'browser-preview',
      operatingSystem: buildInfo?.operatingSystem ?? 'browser',
      architecture: buildInfo?.architecture ?? 'web',
      targetTriple: buildInfo?.targetTriple ?? 'browser-preview',
      tauriRuntime: buildInfo?.tauriRuntime ?? 'browser-preview',
    };
  }
  const info = await invoke<RustAppBuildInfo>('get_app_build_info');
  return {
    productName: info.product_name,
    bundleIdentifier: info.bundle_identifier,
    appVersion: info.app_version,
    packageName: info.package_name,
    buildProfile: info.build_profile,
    operatingSystem: info.operating_system,
    architecture: info.architecture,
    targetTriple: info.target_triple,
    tauriRuntime: info.tauri_runtime,
  };
}
