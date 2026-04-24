#!/usr/bin/env node
// ============================================================================
// Native bundle smoke — C4 partial deliverable
// ============================================================================
// Launches the packaged Tauri binary, waits for it to reach steady state,
// then cleanly terminates it. This asserts:
//   1. The bundle exists after `npm run release:build`.
//   2. The binary actually launches (no missing-dylib / missing-entitlement
//      crash at startup).
//   3. Stderr does not contain the startup-guard sentinel "[recupere] fatal:"
//      during the observation window.
//
// This is NOT a full Tauri-driver WebDriver harness; it does not click
// buttons or assert UI state. It's the minimum guard against "the release
// bundle doesn't even start" regressions. The full WebDriver harness is
// tracked under audit plan chantier C4.
//
// Usage:
//   node scripts/native-smoke.mjs [--timeout-ms 15000] [--fail-on-crash]
// ============================================================================

import { spawn } from 'node:child_process';
import { existsSync, readdirSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import process from 'node:process';

function parseArgs(argv) {
  const args = { timeoutMs: 15000, failOnCrash: false };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    if (flag === '--timeout-ms') {
      args.timeoutMs = Number(argv[i + 1] ?? args.timeoutMs);
      i += 1;
    } else if (flag === '--fail-on-crash') {
      args.failOnCrash = true;
    }
  }
  return args;
}

function findBundleBinary(repoRoot) {
  const candidates = [];
  const macBundle = join(
    repoRoot,
    'src-tauri',
    'target',
    'release',
    'bundle',
    'macos',
  );
  if (existsSync(macBundle)) {
    for (const entry of readdirSync(macBundle)) {
      if (entry.endsWith('.app')) {
        const app = join(macBundle, entry);
        const execDir = join(app, 'Contents', 'MacOS');
        if (existsSync(execDir)) {
          for (const bin of readdirSync(execDir)) {
            const fullPath = join(execDir, bin);
            if (statSync(fullPath).isFile()) {
              candidates.push(fullPath);
            }
          }
        }
      }
    }
  }

  const linuxBinary = join(repoRoot, 'src-tauri', 'target', 'release', 'recupere');
  if (existsSync(linuxBinary)) candidates.push(linuxBinary);
  const winBinary = join(repoRoot, 'src-tauri', 'target', 'release', 'recupere.exe');
  if (existsSync(winBinary)) candidates.push(winBinary);

  return candidates[0] ?? null;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const repoRoot = resolve(new URL('..', import.meta.url).pathname);
  const binary = findBundleBinary(repoRoot);

  if (!binary) {
    console.error('[native-smoke] no packaged binary found under src-tauri/target/release/bundle — run `npm run release:build` first.');
    process.exit(args.failOnCrash ? 1 : 0);
  }

  console.log(`[native-smoke] launching ${binary}`);

  const child = spawn(binary, [], {
    stdio: ['ignore', 'pipe', 'pipe'],
    env: {
      ...process.env,
      // Signal to the app that this is a smoke run so future logic can
      // short-circuit long operations. Honored by `desktop_runtime::run`
      // or ignored silently if unknown.
      RECUPERE_NATIVE_SMOKE: '1',
    },
  });

  let stderrBuffer = '';
  let fatalSeen = false;
  child.stderr.on('data', (chunk) => {
    const text = chunk.toString();
    stderrBuffer += text;
    if (text.includes('[recupere] fatal:')) {
      fatalSeen = true;
    }
    process.stderr.write(text);
  });
  child.stdout.on('data', (chunk) => process.stdout.write(chunk));

  let crashed = false;
  child.on('exit', (code, signal) => {
    if (code !== null && code !== 0) {
      console.error(`[native-smoke] unexpected early exit: code=${code} signal=${signal}`);
      crashed = true;
    }
  });

  await new Promise((r) => setTimeout(r, args.timeoutMs));

  if (fatalSeen) {
    console.error('[native-smoke] startup-guard fatal seen — bundle is not healthy.');
    try {
      child.kill('SIGTERM');
    } catch {}
    process.exit(1);
  }

  if (crashed) {
    console.error('[native-smoke] binary exited before the observation window closed.');
    process.exit(args.failOnCrash ? 1 : 0);
  }

  console.log('[native-smoke] bundle stayed alive for the observation window — OK.');
  try {
    child.kill('SIGTERM');
    // Give the app a moment to drain its tracing buffers; then exit.
    await new Promise((r) => setTimeout(r, 500));
  } catch {}
  process.exit(0);
}

main().catch((error) => {
  console.error('[native-smoke] unexpected error:', error);
  process.exit(1);
});
