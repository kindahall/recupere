// ============================================================================
// Synthetic fixtures helper — Chantier 83, tranche 2 (Voie 2)
// ============================================================================
//
// WebdriverIO specs under `e2e/native/` build fixtures on demand by invoking
// the standalone `gen_synth_fixture` Cargo example (see
// `src-tauri/examples/gen_synth_fixture.rs`). Fixtures are written to a
// session-scoped temp directory — never into the repository tree.
//
// Fidelity disclaimer: these blobs do NOT pretend to be real filesystems.
// Parser correctness is covered by the 334 Rust unit tests. The fixtures
// here exist solely so the native harness has something deterministic to
// drive the UI/IPC/engine pipeline end-to-end.
// ============================================================================

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export type FixtureKind = 'carver-signatures' | 'mbr-gpt' | 'expert-stub';

export interface SyntheticFixture {
  kind: FixtureKind;
  path: string;
  sha256: string;
  sizeBytes: number;
  mtimeMs: number;
}

const HELPER_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HELPER_DIR, '..', '..', '..');
const CARGO_MANIFEST = path.resolve(REPO_ROOT, 'src-tauri', 'Cargo.toml');

let sessionDir: string | null = null;

function getSessionDir(): string {
  if (!sessionDir) {
    sessionDir = mkdtempSync(path.join(tmpdir(), 'recupere-native-e2e-'));
  }
  return sessionDir;
}

export function generateFixture(kind: FixtureKind): SyntheticFixture {
  const outPath = path.join(getSessionDir(), `${kind}.bin`);
  const result = spawnSync(
    'cargo',
    [
      'run',
      '--quiet',
      '--manifest-path',
      CARGO_MANIFEST,
      '--example',
      'gen_synth_fixture',
      '--',
      kind,
      outPath,
    ],
    { stdio: ['ignore', 'inherit', 'inherit'], timeout: 180_000 },
  );

  if (result.error) {
    throw new Error(
      `gen_synth_fixture could not be launched (kind=${kind}): ${result.error.message}`,
    );
  }
  if (result.status !== 0) {
    throw new Error(
      `gen_synth_fixture failed (kind=${kind}, exit=${result.status ?? 'null'}, signal=${result.signal ?? 'null'})`,
    );
  }

  return probe(kind, outPath);
}

export function hashFile(filePath: string): string {
  return createHash('sha256').update(readFileSync(filePath)).digest('hex');
}

export function readMtimeMs(filePath: string): number {
  return statSync(filePath).mtimeMs;
}

function probe(kind: FixtureKind, filePath: string): SyntheticFixture {
  const stat = statSync(filePath);
  return {
    kind,
    path: filePath,
    sha256: hashFile(filePath),
    sizeBytes: stat.size,
    mtimeMs: stat.mtimeMs,
  };
}
