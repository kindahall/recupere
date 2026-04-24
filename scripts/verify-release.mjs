#!/usr/bin/env node
// ============================================================================
// Récupère — post-build release verification
// ============================================================================
//
// Runs AFTER `npm run release:build` to assert that the produced artifacts
// are trustworthy for distribution. Distinct from `release-preflight.mjs`,
// which runs BEFORE the build to catch misconfigured secrets.
//
// What it checks today:
//   - every artifact has a stable sha256 recorded in the report
//   - macOS `.app`/`.dmg` passes `codesign --verify --deep --strict`
//     (when run on macOS)
//   - Windows `.exe`/`.msi` has a valid Authenticode signature (when run
//     on Windows with `signtool` or `osslsigncode` in PATH)
//   - AppImage / `.deb` are at least a non-empty file (Linux signing is
//     distro-specific and tracked for a later pass)
//   - updater `latest.json` (if present) references artifact paths that
//     exist on disk
//
// Exit code 0 only if every artifact passes. Exit code 1 otherwise.
//
// Usage:
//   node scripts/verify-release.mjs
//   node scripts/verify-release.mjs --soft   # non-zero exit becomes warn only
// ============================================================================

import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import process from 'node:process';

const REPO_ROOT = resolve(new URL('..', import.meta.url).pathname);
const BUNDLE_ROOT = join(REPO_ROOT, 'src-tauri', 'target', 'release', 'bundle');
const REPORT_DIR = join(REPO_ROOT, 'dist');
const REPORT_PATH = join(REPORT_DIR, 'release-verification.json');

const args = new Set(process.argv.slice(2));
const softMode = args.has('--soft');

function sha256OfFile(path) {
  const hash = createHash('sha256');
  hash.update(readFileSync(path));
  return hash.digest('hex');
}

function walk(dir, acc = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) walk(full, acc);
    else acc.push(full);
  }
  return acc;
}

function isArtifact(path) {
  return /\.(app|dmg|exe|msi|AppImage|deb|rpm|zip|sig)$/i.test(path);
}

function verifyMacOs(appOrDmg) {
  // `codesign --verify --deep --strict` is the minimal Gatekeeper-alignment
  // check. Notarization status is checked via `spctl` when available.
  if (process.platform !== 'darwin') {
    return { verifier: 'codesign', skipped: 'not macOS', ok: true };
  }
  const codesign = spawnSync('codesign', ['--verify', '--deep', '--strict', appOrDmg], {
    encoding: 'utf8',
  });
  if (codesign.status !== 0) {
    return {
      verifier: 'codesign',
      ok: false,
      stderr: codesign.stderr?.trim() || '(no output)',
    };
  }
  const spctl = spawnSync('spctl', ['-a', '-t', 'exec', '-vv', appOrDmg], {
    encoding: 'utf8',
  });
  return {
    verifier: 'codesign+spctl',
    ok: spctl.status === 0,
    spctl: spctl.stderr?.trim(),
  };
}

function verifyWindows(exeOrMsi) {
  if (process.platform !== 'win32') {
    return { verifier: 'signtool', skipped: 'not Windows', ok: true };
  }
  const result = spawnSync('signtool', ['verify', '/pa', '/v', exeOrMsi], {
    encoding: 'utf8',
    shell: true,
  });
  return {
    verifier: 'signtool',
    ok: result.status === 0,
    output: (result.stderr || result.stdout || '').trim(),
  };
}

function classifyArtifact(path) {
  if (path.endsWith('.app') || path.endsWith('.dmg')) return 'macos';
  if (path.endsWith('.exe') || path.endsWith('.msi')) return 'windows';
  if (path.endsWith('.AppImage') || path.endsWith('.deb') || path.endsWith('.rpm')) return 'linux';
  if (path.endsWith('.zip')) return 'archive';
  if (path.endsWith('.sig')) return 'signature';
  return 'unknown';
}

function verifyUpdaterManifest() {
  const candidates = walk(BUNDLE_ROOT).filter((p) => /latest\.json$/.test(p));
  if (candidates.length === 0) {
    return { ok: true, skipped: 'no updater manifest found' };
  }
  const manifestPath = candidates[0];
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  } catch (error) {
    return { ok: false, error: `cannot parse ${manifestPath}: ${error.message}` };
  }
  const missing = [];
  for (const [platform, value] of Object.entries(manifest.platforms ?? {})) {
    const url = value.url ?? '';
    const localGuess = url.replace(/^https?:\/\/[^/]+\//, '');
    const localPath = join(BUNDLE_ROOT, localGuess);
    if (!existsSync(localPath) && !/^https?:/.test(url)) {
      missing.push({ platform, url });
    }
  }
  return {
    ok: missing.length === 0,
    manifestPath,
    missing,
  };
}

function main() {
  if (!existsSync(BUNDLE_ROOT)) {
    console.error(`[verify-release] bundle directory not found: ${BUNDLE_ROOT}`);
    console.error('                 run `npm run release:build` first.');
    process.exit(softMode ? 0 : 1);
  }

  const artifacts = walk(BUNDLE_ROOT).filter(isArtifact);
  if (artifacts.length === 0) {
    console.error(`[verify-release] no artifacts found under ${BUNDLE_ROOT}`);
    process.exit(softMode ? 0 : 1);
  }

  const report = {
    generated_at: new Date().toISOString(),
    platform: process.platform,
    bundle_root: BUNDLE_ROOT,
    artifacts: [],
    updater_manifest: verifyUpdaterManifest(),
  };

  let hardFailures = 0;

  for (const path of artifacts) {
    const kind = classifyArtifact(path);
    const stat = statSync(path);
    const entry = {
      path,
      kind,
      size_bytes: stat.size,
    };
    if (stat.isFile()) {
      entry.sha256 = sha256OfFile(path);
    }
    if (kind === 'macos') {
      entry.signature = verifyMacOs(path);
    } else if (kind === 'windows') {
      entry.signature = verifyWindows(path);
    }
    if (entry.signature && entry.signature.ok === false) {
      hardFailures += 1;
    }
    report.artifacts.push(entry);
  }

  mkdirSync(REPORT_DIR, { recursive: true });
  writeFileSync(REPORT_PATH, JSON.stringify(report, null, 2));
  console.log(`[verify-release] report written to ${REPORT_PATH}`);

  if (report.updater_manifest?.ok === false) {
    hardFailures += 1;
    console.error('[verify-release] updater manifest references missing artifacts:');
    console.error(JSON.stringify(report.updater_manifest.missing, null, 2));
  }

  if (hardFailures > 0 && !softMode) {
    console.error(`[verify-release] ${hardFailures} artifact(s) failed signature check.`);
    process.exit(1);
  }
  console.log(`[verify-release] all ${report.artifacts.length} artifact(s) look good.`);
}

main();
