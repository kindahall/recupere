// ============================================================================
// native-scan-flow.spec.ts — Chantier 83, tranche 3
// ============================================================================
//
// End-to-end scan flow against the real Récupère runtime: import a synthetic
// blob as a recovery source, kick off a signature-carving scan, poll until
// completion, assert at least one candidate file surfaced, and verify the
// source blob was NOT modified by the scan (read-only invariant).
//
// Runs unchanged on:
//   - macOS via Appium Mac2 + WKWebView context (`wdio.appium.conf.ts`)
//   - Linux + Windows via tauri-driver (`wdio.tauri-driver.conf.ts`)
//
// Stack divergence is hidden by `helpers/driver.ts`. Specs talk to the app
// strictly through the Tauri IPC layer — no DOM clicks here. UI navigation
// specs land in later tranches.
// ============================================================================

import { existsSync, statSync, unlinkSync } from 'node:fs';

import { browser } from '@wdio/globals';

import { attachToWebview, invokeTauriCommand } from './helpers/driver.js';
import {
  type SyntheticFixture,
  generateFixture,
  hashFile,
  readMtimeMs,
} from './helpers/fixtures.js';
import { ensureDevLicenseActive } from './helpers/license.js';

interface RustDevice {
  id: string;
  device_path: string;
  device_type: string;
}

interface RustScanProgress {
  status: string;
  percent_complete?: number;
  files_found?: number;
}

const SCAN_POLL_INTERVAL_MS = 500;
const SCAN_TIMEOUT_MS = 90_000;

describe('Native scan flow', function () {
  this.timeout(180_000);

  let fixture: SyntheticFixture;

  before(async () => {
    fixture = generateFixture('carver-signatures');
    await attachToWebview(browser);
    await ensureDevLicenseActive(browser);
  });

  after(async () => {
    if (fixture) {
      try {
        await invokeTauriCommand(browser, 'remove_imported_recovery_source', {
          path: fixture.path,
        });
      } catch {
        // Best-effort cleanup — the fixture lives in tmp anyway.
      }
      if (existsSync(fixture.path)) {
        unlinkSync(fixture.path);
      }
    }
  });

  it('imports a synthetic source, runs signature-carving scan, surfaces candidates without mutating the source', async () => {
    const mtimeBefore = readMtimeMs(fixture.path);
    const sha256Before = fixture.sha256;
    const sizeBefore = fixture.sizeBytes;

    await invokeTauriCommand(browser, 'import_recovery_source', { path: fixture.path });

    const devices = await invokeTauriCommand<RustDevice[]>(browser, 'get_devices');
    const importedDevice = devices.find((d) => d.device_path === fixture.path);
    if (!importedDevice) {
      throw new Error(
        `import_recovery_source did not surface the fixture in get_devices. Looked for device_path=${fixture.path}, got ${devices.length} devices.`,
      );
    }

    const scanId = await invokeTauriCommand<string>(browser, 'start_scan', {
      deviceId: importedDevice.id,
      scanType: 'signature-carving',
    });
    if (!scanId || typeof scanId !== 'string') {
      throw new Error(`start_scan did not return a scan id. Got: ${JSON.stringify(scanId)}`);
    }

    const finalProgress = await pollScanUntilDone(scanId);
    if (finalProgress.status !== 'completed') {
      throw new Error(
        `Scan did not complete cleanly. Final status=${finalProgress.status}, ` +
          `progress=${JSON.stringify(finalProgress)}`,
      );
    }

    const filesFound = finalProgress.files_found ?? 0;
    if (filesFound < 1) {
      throw new Error(
        `Signature-carving scan over the carver-signatures fixture surfaced 0 candidates. Expected at least one (JPEG / PDF / ZIP magic was planted at fixed offsets). Final progress: ${JSON.stringify(finalProgress)}`,
      );
    }

    const mtimeAfter = readMtimeMs(fixture.path);
    const sha256After = hashFile(fixture.path);
    const sizeAfter = statSync(fixture.path).size;
    if (sha256After !== sha256Before || sizeAfter !== sizeBefore || mtimeAfter !== mtimeBefore) {
      throw new Error(
        `Source fixture was mutated by the scan — read-only invariant broken. sha256 ${sha256Before} → ${sha256After}, size ${sizeBefore} → ${sizeAfter}, mtime ${mtimeBefore} → ${mtimeAfter}.`,
      );
    }
  });
});

async function pollScanUntilDone(scanId: string): Promise<RustScanProgress> {
  const deadline = Date.now() + SCAN_TIMEOUT_MS;
  let last: RustScanProgress = { status: 'unknown' };
  while (Date.now() < deadline) {
    last = await invokeTauriCommand<RustScanProgress>(browser, 'get_scan_progress', { scanId });
    if (last.status === 'completed' || last.status === 'failed' || last.status === 'cancelled') {
      return last;
    }
    await new Promise((resolve) => setTimeout(resolve, SCAN_POLL_INTERVAL_MS));
  }
  throw new Error(
    `Timed out after ${SCAN_TIMEOUT_MS}ms polling scan ${scanId}. Last progress: ${JSON.stringify(last)}`,
  );
}
