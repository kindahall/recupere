// ============================================================================
// native-imaging-flow.spec.ts — Chantier 83, tranche 4
// ============================================================================
//
// End-to-end imaging flow: import a synthetic blob as a recovery source, kick
// off a read-only image copy to a temp destination, poll until completion,
// and verify two invariants:
//
//   1. The destination image was actually written (file exists, size > 0).
//   2. The source blob was NOT mutated (sha256 + size + mtime preserved).
//
// Invariant (2) is the hard contract of AGENTS.md "never write to source".
// Imaging is the most dangerous flow in the app — if it ever broke, this is
// where it would surface.
//
// Cross-platform via the same helpers as native-scan-flow.spec.ts.
// ============================================================================

import { existsSync, mkdtempSync, rmSync, statSync, unlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

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
}

interface RustScanProgress {
  status: string;
  bytes_scanned?: number;
  total_bytes?: number;
}

const POLL_INTERVAL_MS = 500;
const IMAGING_TIMEOUT_MS = 120_000;

describe('Native imaging flow', function () {
  this.timeout(180_000);

  let fixture: SyntheticFixture;
  let destDir: string;
  let destImagePath: string;

  before(async () => {
    fixture = generateFixture('carver-signatures');
    destDir = mkdtempSync(path.join(tmpdir(), 'recupere-native-imaging-'));
    destImagePath = path.join(destDir, 'recupere-source.img');
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
    if (destDir && existsSync(destDir)) {
      rmSync(destDir, { recursive: true, force: true });
    }
  });

  it('images a synthetic source to a temp destination without mutating the source', async () => {
    const sha256Before = fixture.sha256;
    const sizeBefore = fixture.sizeBytes;
    const mtimeBefore = readMtimeMs(fixture.path);

    await invokeTauriCommand(browser, 'import_recovery_source', { path: fixture.path });

    const devices = await invokeTauriCommand<RustDevice[]>(browser, 'get_devices');
    const importedDevice = devices.find((d) => d.device_path === fixture.path);
    if (!importedDevice) {
      throw new Error(
        `import_recovery_source did not surface the fixture in get_devices. Looked for device_path=${fixture.path}, got ${devices.length} devices.`,
      );
    }

    const scanId = await invokeTauriCommand<string>(browser, 'start_imaging', {
      deviceId: importedDevice.id,
      destinationPath: destImagePath,
    });
    if (!scanId || typeof scanId !== 'string') {
      throw new Error(`start_imaging did not return a scan id. Got: ${JSON.stringify(scanId)}`);
    }

    const finalProgress = await pollImagingUntilDone(scanId);
    if (finalProgress.status !== 'completed') {
      throw new Error(
        `Imaging did not complete cleanly. Final status=${finalProgress.status}, ` +
          `progress=${JSON.stringify(finalProgress)}`,
      );
    }

    if (!existsSync(destImagePath)) {
      throw new Error(
        `Imaging reported completed but destination image was not created at ${destImagePath}.`,
      );
    }
    const destSize = statSync(destImagePath).size;
    if (destSize === 0) {
      throw new Error(`Destination image at ${destImagePath} is empty (0 bytes).`);
    }
    if (destSize < sizeBefore) {
      throw new Error(
        `Destination image is smaller than source: ${destSize} bytes vs source ${sizeBefore}. Imaging must produce a full read-only copy.`,
      );
    }

    const sha256After = hashFile(fixture.path);
    const sizeAfter = statSync(fixture.path).size;
    const mtimeAfter = readMtimeMs(fixture.path);
    if (sha256After !== sha256Before || sizeAfter !== sizeBefore || mtimeAfter !== mtimeBefore) {
      throw new Error(
        `READ-ONLY INVARIANT BROKEN. Source fixture mutated by imaging:\n  sha256: ${sha256Before} → ${sha256After}\n  size:   ${sizeBefore} → ${sizeAfter}\n  mtime:  ${mtimeBefore} → ${mtimeAfter}`,
      );
    }
  });
});

async function pollImagingUntilDone(scanId: string): Promise<RustScanProgress> {
  const deadline = Date.now() + IMAGING_TIMEOUT_MS;
  let last: RustScanProgress = { status: 'unknown' };
  while (Date.now() < deadline) {
    last = await invokeTauriCommand<RustScanProgress>(browser, 'get_scan_progress', { scanId });
    if (last.status === 'completed' || last.status === 'failed' || last.status === 'cancelled') {
      return last;
    }
    await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS));
  }
  throw new Error(
    `Timed out after ${IMAGING_TIMEOUT_MS}ms polling imaging ${scanId}. Last: ${JSON.stringify(last)}`,
  );
}
