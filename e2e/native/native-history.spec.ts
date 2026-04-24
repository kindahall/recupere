// ============================================================================
// native-history.spec.ts — Chantier 83, tranche 7
// ============================================================================
//
// Verifies that a freshly completed scan session is persisted in the local
// history and that its technical logs are retrievable. Exercises the
// `get_scan_history` and `get_scan_logs` IPC commands wired to the on-disk
// session store.
//
// Flow:
//   1. Import a synthetic source.
//   2. Run a quick signature-carving scan to completion.
//   3. Call `get_scan_history` and assert the new session is present
//      with the right scanId, deviceId and a terminal status.
//   4. Call `get_scan_logs` and assert at least one log entry exists.
//   5. Cleanup.
// ============================================================================

import { existsSync, unlinkSync } from 'node:fs';

import { browser } from '@wdio/globals';

import { attachToWebview, invokeTauriCommand } from './helpers/driver.js';
import { type SyntheticFixture, generateFixture } from './helpers/fixtures.js';
import { ensureDevLicenseActive } from './helpers/license.js';

interface RustDevice {
  id: string;
  device_path: string;
}

interface RustScanProgress {
  status: string;
}

interface RustScanSessionSummary {
  id: string;
  device_id: string;
  scan_type: string;
  status: string;
  files_found: number;
}

interface RustTechnicalLogEntry {
  timestamp_ms: number;
  level: string;
  message: string;
}

const POLL_INTERVAL_MS = 500;
const SCAN_TIMEOUT_MS = 90_000;

describe('Native scan history flow', function () {
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
        // Best-effort cleanup.
      }
      if (existsSync(fixture.path)) {
        unlinkSync(fixture.path);
      }
    }
  });

  it('persists a completed scan session in history with retrievable logs', async () => {
    await invokeTauriCommand(browser, 'import_recovery_source', { path: fixture.path });
    const devices = await invokeTauriCommand<RustDevice[]>(browser, 'get_devices');
    const device = devices.find((d) => d.device_path === fixture.path);
    if (!device) {
      throw new Error(`import_recovery_source did not surface ${fixture.path} in get_devices.`);
    }

    const scanId = await invokeTauriCommand<string>(browser, 'start_scan', {
      deviceId: device.id,
      scanType: 'signature-carving',
    });

    await pollScanUntilDone(scanId);

    const history = await invokeTauriCommand<RustScanSessionSummary[]>(browser, 'get_scan_history');
    const entry = history.find((session) => session.id === scanId);
    if (!entry) {
      throw new Error(
        `Scan ${scanId} not found in get_scan_history (saw ${history.length} sessions). History persistence may be broken.`,
      );
    }
    if (entry.device_id !== device.id) {
      throw new Error(
        `History entry deviceId mismatch: expected ${device.id}, got ${entry.device_id}.`,
      );
    }
    if (!['completed', 'failed', 'cancelled'].includes(entry.status)) {
      throw new Error(
        `History entry has non-terminal status '${entry.status}' for a finished scan.`,
      );
    }

    const logs = await invokeTauriCommand<RustTechnicalLogEntry[]>(browser, 'get_scan_logs', {
      scanId,
    });
    if (logs.length === 0) {
      throw new Error(
        `get_scan_logs returned 0 entries for scan ${scanId}. Every scan should produce at least one technical log line.`,
      );
    }
    const malformed = logs.find(
      (log) => typeof log.message !== 'string' || typeof log.level !== 'string',
    );
    if (malformed) {
      throw new Error(`Scan log entry has missing fields: ${JSON.stringify(malformed)}`);
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
    await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS));
  }
  throw new Error(`Timed out polling scan ${scanId}. Last: ${JSON.stringify(last)}`);
}
