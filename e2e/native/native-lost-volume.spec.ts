// ============================================================================
// native-lost-volume.spec.ts — Chantier 83, tranche 6
// ============================================================================
//
// End-to-end lost-volume detection flow:
//   1. Import a synthetic mbr-gpt blob as recovery source.
//   2. Fetch the diagnostic — must report potential_volumes_inspected = true
//      and surface at least one candidate volume on the synthetic GPT header.
//   3. (Optional) trigger `start_potential_volume_scan` on the candidate to
//      confirm the IPC accepts it; we do NOT poll to completion because the
//      synthetic blob has no real filesystem behind it — Récupère may legitly
//      report 0 files without that being a bug. The scan id surfacing is
//      enough to validate the wiring.
//
// Read-only invariant verified at the end (sha256 + size + mtime).
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
}

interface RustDiagnostic {
  potential_volumes_inspected: boolean;
  potential_volumes_notice: string | null;
  potential_volumes: Array<{
    id: string;
    label: string;
    filesystem: string;
    start_offset: number;
    size_bytes: number | null;
    confidence_score: number;
    detection_method: string;
  }>;
}

describe('Native lost-volume flow', function () {
  this.timeout(180_000);

  let fixture: SyntheticFixture;

  before(async () => {
    fixture = generateFixture('mbr-gpt');
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

  it('detects a candidate partition on a synthetic mbr-gpt blob and accepts a potential-volume scan request', async () => {
    const sha256Before = fixture.sha256;
    const sizeBefore = fixture.sizeBytes;
    const mtimeBefore = readMtimeMs(fixture.path);

    await invokeTauriCommand(browser, 'import_recovery_source', { path: fixture.path });

    const devices = await invokeTauriCommand<RustDevice[]>(browser, 'get_devices');
    const device = devices.find((d) => d.device_path === fixture.path);
    if (!device) {
      throw new Error(`import_recovery_source did not surface ${fixture.path} in get_devices.`);
    }

    const diagnostic = await invokeTauriCommand<RustDiagnostic>(browser, 'get_diagnostic', {
      deviceId: device.id,
    });

    if (!diagnostic.potential_volumes_inspected) {
      throw new Error(
        `Diagnostic skipped potential-volume inspection. notice=${diagnostic.potential_volumes_notice}`,
      );
    }
    if (diagnostic.potential_volumes.length === 0) {
      throw new Error(
        'Diagnostic returned 0 potential volumes for the mbr-gpt fixture. ' +
          'Expected at least one candidate (the synthetic GPT primary header in sector 1).',
      );
    }
    const candidate = diagnostic.potential_volumes[0];
    if (typeof candidate.id !== 'string' || candidate.id.length === 0) {
      throw new Error(`First candidate volume has no id: ${JSON.stringify(candidate)}`);
    }
    if (candidate.start_offset < 0) {
      throw new Error(`Candidate volume start_offset is negative: ${candidate.start_offset}`);
    }

    // Validate that the IPC accepts a potential-volume scan request — we do
    // NOT poll to completion: the synthetic blob has no real filesystem, so
    // the scan may legitimately surface 0 files. The contract under test
    // here is "the IPC wires up", not "carving finds something".
    const scanId = await invokeTauriCommand<string>(browser, 'start_potential_volume_scan', {
      deviceId: device.id,
      volumeId: candidate.id,
    });
    if (!scanId || typeof scanId !== 'string') {
      throw new Error(
        `start_potential_volume_scan did not return a scan id. Got: ${JSON.stringify(scanId)}`,
      );
    }

    // Best-effort cancel — we don't need it to finish.
    try {
      await invokeTauriCommand(browser, 'cancel_scan', { scanId });
    } catch {
      // Some scans complete instantly on tiny fixtures; cancel may be no-op.
    }

    const sha256After = hashFile(fixture.path);
    const sizeAfter = statSync(fixture.path).size;
    const mtimeAfter = readMtimeMs(fixture.path);
    if (sha256After !== sha256Before || sizeAfter !== sizeBefore || mtimeAfter !== mtimeBefore) {
      throw new Error(
        `READ-ONLY INVARIANT BROKEN. Source fixture mutated by lost-volume inspection:\n  sha256: ${sha256Before} → ${sha256After}\n  size:   ${sizeBefore} → ${sizeAfter}\n  mtime:  ${mtimeBefore} → ${mtimeAfter}`,
      );
    }
  });
});
