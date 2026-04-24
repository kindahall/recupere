import { invoke, isTauri } from '@tauri-apps/api/core';
import { loadBrowserPreviewState } from '../../utils/browserPreviewSeed';

export interface LicenseInfo {
  valid: boolean;
  status:
    | 'free'
    | 'pro'
    | 'expired'
    | 'wrong_machine'
    | 'invalid_signature'
    | 'malformed'
    | 'storage_error'
    | 'clock_error'
    | 'rate_limited';
  email?: string;
  tier?: string;
  expiresAt?: number;
  message: string;
}

interface RustLicenseInfo {
  valid: boolean;
  status: LicenseInfo['status'];
  email: string | null;
  tier: string | null;
  expires_at: number | null;
  message: string;
}

function mapLicenseInfo(raw: RustLicenseInfo): LicenseInfo {
  return {
    valid: raw.valid,
    status: raw.status,
    email: raw.email ?? undefined,
    tier: raw.tier ?? undefined,
    expiresAt: raw.expires_at ?? undefined,
    message: raw.message,
  };
}

function browserPreviewLicenseInfo(overrides: Partial<LicenseInfo> = {}): LicenseInfo {
  const previewLicenseStatus = loadBrowserPreviewState()?.licenseStatus;
  const previewPro = previewLicenseStatus === 'pro';
  return {
    valid: previewPro,
    status: previewPro ? 'pro' : 'free',
    tier: previewPro ? 'pro' : undefined,
    message: previewPro
      ? 'Preview license: Pro features are enabled.'
      : 'License features require the desktop backend.',
    ...overrides,
  };
}

export async function activateLicense(key: string): Promise<LicenseInfo> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return browserPreviewLicenseInfo({
      valid: false,
      status: 'malformed',
      message:
        key.trim().length === 0
          ? 'License activation failed: empty key.'
          : 'Invalid or malformed license key in browser preview.',
    });
  }
  const raw = await invoke<RustLicenseInfo>('activate_license', { key });
  return mapLicenseInfo(raw);
}

export async function getLicenseStatus(): Promise<LicenseInfo> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return browserPreviewLicenseInfo();
  }
  const raw = await invoke<RustLicenseInfo>('get_license_status');
  return mapLicenseInfo(raw);
}

export async function deactivateLicense(): Promise<void> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return;
  }
  await invoke('deactivate_license');
}

export interface PiiPurgeResult {
  license_deleted: boolean;
  history_purged: boolean;
  audit_cleared: boolean;
  recent_traces_cleared: boolean;
}

export async function purgeAllPii(): Promise<PiiPurgeResult> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return {
      license_deleted: true,
      history_purged: true,
      audit_cleared: true,
      recent_traces_cleared: true,
    };
  }
  return invoke<PiiPurgeResult>('purge_all_pii');
}

export async function getMachineFingerprint(): Promise<string> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return 'browser-preview';
  }
  return invoke<string>('get_machine_fingerprint');
}
