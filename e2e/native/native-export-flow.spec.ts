// ============================================================================
// native-export-flow.spec.ts — Chantier 83, tranche 5
// ============================================================================
//
// End-to-end recovery + export flow:
//   1. Import a synthetic blob as recovery source.
//   2. Run a signature-carving scan.
//   3. Fetch the scan results (RecoveredFile[]).
//   4. Trigger `start_export` with the recovered file ids + a temp dest dir.
//   5. Poll until export completes.
//   6. Assert each exported file exists on disk under the destination.
//   7. Verify the source blob was NOT mutated (sha256 + size + mtime).
//
// Cross-platform via the same helpers as the other native specs.
// ============================================================================

import { existsSync, mkdtempSync, readdirSync, rmSync, statSync, unlinkSync } from 'node:fs';
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

interface RustRecoveredFile {
  id: string;
  name: string;
  size_bytes: number;
}

interface RustScanProgress {
  status: string;
  files_found?: number;
}

interface RustExportProgress {
  status: string;
  total_files: number;
  exported_files: number;
  errors: { file_id: string; file_name: string; reason: string }[];
}

const POLL_INTERVAL_MS = 500;
const SCAN_TIMEOUT_MS = 90_000;
const EXPORT_TIMEOUT_MS = 120_000;

describe('Native export flow', function () {
  this.timeout(240_000);

  let fixture: SyntheticFixture;
  let exportDir: string;

  before(async () => {
    fixture = generateFixture('carver-signatures');
    exportDir = mkdtempSync(path.join(tmpdir(), 'recupere-native-export-'));
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
    if (exportDir && existsSync(exportDir)) {
      rmSync(exportDir, { recursive: true, force: true });
    }
  });

  it('exports recovered files to a temp destination, leaves the source untouched', async () => {
    const sha256Before = fixture.sha256;
    const sizeBefore = fixture.sizeBytes;
    const mtimeBefore = readMtimeMs(fixture.path);

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
    await pollUntilDone<RustScanProgress>('get_scan_progress', { scanId }, SCAN_TIMEOUT_MS);

    const results = await invokeTauriCommand<RustRecoveredFile[]>(browser, 'get_results', {
      scanId,
    });
    if (results.length === 0) {
      throw new Error(
        'Scan returned 0 recovered files — cannot exercise export flow. ' +
          'Expected JPEG/PDF/ZIP candidates from the carver-signatures fixture.',
      );
    }
    const selectedFileIds = results.map((file) => file.id);

    const exportId = await invokeTauriCommand<string>(browser, 'start_export', {
      scanId,
      destinationPath: exportDir,
      selectedFileIds,
      conflictStrategy: 'rename',
      preserveStructure: false,
      verifyIntegrity: true,
    });
    if (!exportId || typeof exportId !== 'string') {
      throw new Error(`start_export did not return an export id. Got: ${JSON.stringify(exportId)}`);
    }

    const exportFinal = await pollUntilDone<RustExportProgress>(
      'get_export_progress',
      { exportId },
      EXPORT_TIMEOUT_MS,
    );
    if (exportFinal.status !== 'completed') {
      throw new Error(
        `Export did not complete cleanly. Final status=${exportFinal.status}, ` +
          `progress=${JSON.stringify(exportFinal)}`,
      );
    }
    if (exportFinal.errors.length > 0) {
      throw new Error(`Export reported per-file errors: ${JSON.stringify(exportFinal.errors)}`);
    }
    if (exportFinal.exported_files !== selectedFileIds.length) {
      throw new Error(
        `Export count mismatch: requested ${selectedFileIds.length}, exported ${exportFinal.exported_files}.`,
      );
    }

    const filesOnDisk = walkFiles(exportDir);
    if (filesOnDisk.length < selectedFileIds.length) {
      throw new Error(
        `Export reported ${exportFinal.exported_files} files but only ${filesOnDisk.length} found on disk under ${exportDir}.`,
      );
    }
    for (const file of filesOnDisk) {
      const stat = statSync(file);
      if (stat.size === 0) {
        throw new Error(`Exported file ${file} is empty (0 bytes).`);
      }
    }

    const sha256After = hashFile(fixture.path);
    const sizeAfter = statSync(fixture.path).size;
    const mtimeAfter = readMtimeMs(fixture.path);
    if (sha256After !== sha256Before || sizeAfter !== sizeBefore || mtimeAfter !== mtimeBefore) {
      throw new Error(
        `READ-ONLY INVARIANT BROKEN. Source fixture mutated by export:\n  sha256: ${sha256Before} → ${sha256After}\n  size:   ${sizeBefore} → ${sizeAfter}\n  mtime:  ${mtimeBefore} → ${mtimeAfter}`,
      );
    }
  });
});

async function pollUntilDone<TProgress extends { status: string }>(
  command: string,
  args: Record<string, unknown>,
  timeoutMs: number,
): Promise<TProgress> {
  const deadline = Date.now() + timeoutMs;
  let last: TProgress = { status: 'unknown' } as TProgress;
  while (Date.now() < deadline) {
    last = await invokeTauriCommand<TProgress>(browser, command, args);
    if (last.status === 'completed' || last.status === 'failed' || last.status === 'cancelled') {
      return last;
    }
    await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS));
  }
  throw new Error(
    `Timed out after ${timeoutMs}ms polling ${command}. Last: ${JSON.stringify(last)}`,
  );
}

function walkFiles(dir: string): string[] {
  const entries = readdirSync(dir, { withFileTypes: true });
  const out: string[] = [];
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...walkFiles(full));
    } else if (entry.isFile()) {
      out.push(full);
    }
  }
  return out;
}
