// ============================================================================
// native-licensing.spec.ts — Chantier 83, tranche 9
// ============================================================================
//
// Exercises the licensing IPC end-to-end against the real backend:
//   1. Reset to a known baseline (deactivate any leftover license).
//   2. Confirm the status is 'free' once deactivated.
//   3. Reject a malformed key with a structured failure (no crash).
//   4. Mint a fresh dev license bound to the current machine fingerprint
//      (via `cargo run --bin gen_license`) and activate it. Assert the
//      backend reports tier=pro / status=pro.
//   5. Re-fetch status after activation — must remain pro.
//   6. Deactivate again. Status must drop back to 'free'.
//
// This spec writes nothing to disk and does not need a fixture. It verifies
// that the licensing layer behaves identically through real Tauri IPC as the
// Rust unit tests already prove (`license::tests::*`).
// ============================================================================

import { browser } from '@wdio/globals';

import { attachToWebview, invokeTauriCommand } from './helpers/driver.js';
import { mintDevLicense } from './helpers/license.js';

interface RustLicenseInfo {
  valid: boolean;
  status: string;
  tier: string | null;
  email: string | null;
  message: string;
}

const MALFORMED_KEY = 'RECUP-not-a-real-key';

describe('Native licensing flow', function () {
  this.timeout(120_000);

  before(async () => {
    await attachToWebview(browser);
    // Reset to a known baseline. Best-effort: deactivate may be a no-op if
    // the backend store has no current license.
    try {
      await invokeTauriCommand(browser, 'deactivate_license', {});
    } catch {
      // No license to deactivate — fine.
    }
  });

  after(async () => {
    // Leave the backend in a clean state so other specs do not inherit a
    // surprise pro license that wasn't theirs.
    try {
      await invokeTauriCommand(browser, 'deactivate_license', {});
    } catch {
      // Ignore.
    }
  });

  it('rejects a malformed key without crashing', async () => {
    const status = await invokeTauriCommand<RustLicenseInfo>(browser, 'activate_license', {
      key: MALFORMED_KEY,
    });
    if (status.valid) {
      throw new Error(
        `Backend accepted a malformed license key as valid. status=${JSON.stringify(status)}`,
      );
    }
    if (!['malformed', 'invalid_signature'].includes(status.status)) {
      throw new Error(
        `Expected malformed/invalid_signature status for a junk key, got '${status.status}'. ` +
          `message='${status.message}'`,
      );
    }
  });

  it('activates a freshly minted dev license, surfaces it via get_license_status, then deactivates cleanly', async () => {
    const baseline = await invokeTauriCommand<RustLicenseInfo>(browser, 'get_license_status', {});
    if (baseline.valid) {
      // Best-effort reset from a leftover state — re-deactivate.
      await invokeTauriCommand(browser, 'deactivate_license', {});
    }

    const key = mintDevLicense('native-licensing-spec@recupere.local');
    const activated = await invokeTauriCommand<RustLicenseInfo>(browser, 'activate_license', {
      key,
    });
    if (!activated.valid) {
      throw new Error(
        `activate_license rejected a freshly minted dev key. status=${JSON.stringify(activated)}`,
      );
    }
    if (activated.status !== 'pro' || activated.tier !== 'pro') {
      throw new Error(
        `Activated license is not pro: status='${activated.status}', tier='${activated.tier}'.`,
      );
    }

    const refetched = await invokeTauriCommand<RustLicenseInfo>(browser, 'get_license_status', {});
    if (!refetched.valid || refetched.status !== 'pro') {
      throw new Error(
        `get_license_status did not surface the active pro license: ${JSON.stringify(refetched)}`,
      );
    }

    await invokeTauriCommand(browser, 'deactivate_license', {});
    const afterDeactivate = await invokeTauriCommand<RustLicenseInfo>(
      browser,
      'get_license_status',
      {},
    );
    if (afterDeactivate.valid || afterDeactivate.status === 'pro') {
      throw new Error(
        `deactivate_license left the backend in a pro state: ${JSON.stringify(afterDeactivate)}`,
      );
    }
  });
});
