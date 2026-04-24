// ============================================================================
// native-expert.spec.ts — Chantier 83, tranche 8
// ============================================================================
//
// Smokes the expert-mode IPC layer: hex preview + auxiliary forks (resource
// fork / ADS). The synthetic carver-signatures fixture surfaces signature
// candidates without producing real HFS+ resource forks or NTFS named
// streams, so the auxiliary requests are expected to return either an empty
// payload or a structured "not available" response — what we assert is that
// the IPC accepts the request shape and never crashes.
//
// The hex preview is asserted to return at least one byte for the first
// recovered file (the carver's first JPEG candidate is a 64 KiB block — any
// hex window inside that is fine).
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

interface RustRecoveredFile {
  id: string;
  name: string;
  size_bytes: number;
}

interface RustHexPreview {
  start_offset: number;
  bytes: number[];
  total_size_bytes: number;
}

interface RustAuxiliaryPreview {
  kind: string;
  payload?: unknown;
  error?: string | null;
}

const POLL_INTERVAL_MS = 500;
const SCAN_TIMEOUT_MS = 90_000;
const HEX_BYTES_TO_READ = 256;

describe('Native expert-mode flow', function () {
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

  it('hex preview returns bytes for a recovered file, auxiliary previews accept ADS / resource-fork without crashing', async () => {
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

    const results = await invokeTauriCommand<RustRecoveredFile[]>(browser, 'get_results', {
      scanId,
    });
    if (results.length === 0) {
      throw new Error('Scan returned 0 recovered files — cannot exercise expert-mode preview.');
    }
    const target = results[0];

    const hex = await invokeTauriCommand<RustHexPreview>(browser, 'get_file_hex_preview', {
      scanId,
      fileId: target.id,
      startOffset: 0,
      bytesToRead: HEX_BYTES_TO_READ,
    });
    if (!Array.isArray(hex.bytes)) {
      throw new Error(`get_file_hex_preview did not return a bytes array: ${JSON.stringify(hex)}`);
    }
    if (hex.bytes.length === 0) {
      throw new Error(
        `get_file_hex_preview returned 0 bytes for file ${target.id} (${target.name}). Expected at least 1 byte from the carver-signatures fixture.`,
      );
    }
    if (hex.bytes.length > HEX_BYTES_TO_READ) {
      throw new Error(
        `Hex preview returned more bytes than requested: got ${hex.bytes.length}, asked for ${HEX_BYTES_TO_READ}.`,
      );
    }

    // Auxiliary previews — synthetic fixture has no real ADS/resource-fork so
    // the contract under test is "IPC accepts request and returns a
    // structured response", not "payload is non-empty".
    for (const kind of ['ads', 'resource-fork'] as const) {
      try {
        const aux = await invokeTauriCommand<RustAuxiliaryPreview>(
          browser,
          'get_file_auxiliary_preview',
          { scanId, fileId: target.id, auxiliaryKind: kind, auxiliaryName: null },
        );
        if (typeof aux !== 'object' || aux === null) {
          throw new Error(
            `get_file_auxiliary_preview(kind=${kind}) returned non-object: ${JSON.stringify(aux)}`,
          );
        }
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        if (!/not available|no auxiliary|not present/i.test(message)) {
          throw new Error(
            `Auxiliary preview kind=${kind} crashed unexpectedly: ${message}. Expected either a structured response or a "not available" rejection.`,
          );
        }
      }
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
