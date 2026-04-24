// ============================================================================
// Native dev license helper — Chantier 83
// ============================================================================
//
// Mints a fresh dev license bound to the current machine (via the
// `gen_license` Cargo bin), then activates it inside the running Récupère
// session through the Tauri IPC `activate_license` command.
//
// Why generate fresh: the dev license is bound to `compute_machine_fingerprint()`
// (see `src-tauri/src/license/mod.rs`) — a key generated on machine A is
// rejected on machine B. Generating per-run keeps every CI runner happy.
//
// Why not an env var: the existing Playwright suite does NOT consume a
// pre-baked key (`e2e/license-paywall.spec.ts` only exercises the paywall
// reject path with browser-preview state). Adding a hard-coded key in env
// would be a security regression. Re-minting per run is the cheapest right
// answer.
// ============================================================================

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { invokeTauriCommand } from './driver.js';

const HELPER_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HELPER_DIR, '..', '..', '..');
const CARGO_MANIFEST = path.resolve(REPO_ROOT, 'src-tauri', 'Cargo.toml');

const LICENSE_KEY_PREFIX = 'RECUP-';

export interface ActivatedLicense {
  key: string;
  email: string;
}

export function mintDevLicense(email = 'native-e2e@recupere.local'): string {
  const result = spawnSync(
    'cargo',
    ['run', '--quiet', '--manifest-path', CARGO_MANIFEST, '--bin', 'gen_license', '--', email],
    { encoding: 'utf8', timeout: 180_000 },
  );

  if (result.error) {
    throw new Error(`gen_license could not be launched: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(
      `gen_license failed (exit=${result.status ?? 'null'}, signal=${result.signal ?? 'null'}). ` +
        `stderr: ${result.stderr.trim()}`,
    );
  }

  const key = result.stdout
    .split('\n')
    .map((line) => line.trim())
    .find((line) => line.startsWith(LICENSE_KEY_PREFIX));

  if (!key) {
    throw new Error(
      `gen_license stdout did not contain a key starting with '${LICENSE_KEY_PREFIX}'. ` +
        `stdout: ${result.stdout.trim()}`,
    );
  }

  return key;
}

export async function ensureDevLicenseActive(
  browser: WebdriverIO.Browser,
  email = 'native-e2e@recupere.local',
): Promise<ActivatedLicense> {
  const key = mintDevLicense(email);
  const status = await invokeTauriCommand<{ valid: boolean; status: string; message: string }>(
    browser,
    'activate_license',
    { key },
  );
  if (!status.valid) {
    throw new Error(
      `activate_license rejected the freshly minted dev key. status=${status.status}, message=${status.message}`,
    );
  }
  return { key, email };
}
