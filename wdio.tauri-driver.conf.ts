// ============================================================================
// WebdriverIO — native Tauri harness via tauri-driver (Linux + Windows only)
// ============================================================================
//
// Chantier 83. Drives the real Récupère window via `tauri-driver` against:
//   - Linux:    webkit2gtk-driver (pulled by tauri-driver itself)
//   - Windows:  msedgedriver       (pulled by tauri-driver itself)
//
// macOS is intentionally NOT supported by this config — Apple does not allow
// external WebDriver to attach to a WKWebView. The macOS harness uses
// Appium + Mac2 driver instead; see `wdio.appium.conf.ts`.
//
// Cohabits with Playwright (`playwright.config.ts`) which keeps exercising
// the browser-preview bundle with mocked IPC. Neither replaces the other.
//
// Prerequisites (see README "Running E2E tests"):
//   - cargo install tauri-driver --version 2.0.5
//   - Récupère debug binary built: `cargo build --manifest-path src-tauri/Cargo.toml`
//
// ============================================================================
import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import '@wdio/types';

const here = path.dirname(fileURLToPath(import.meta.url));

if (process.platform === 'darwin') {
  throw new Error(
    'wdio.tauri-driver.conf.ts is not supported on macOS — Apple blocks ' +
      'external WebDriver attaching to WKWebView. Use wdio.appium.conf.ts ' +
      'on macOS instead (Appium Mac2 driver + WKWebView context switching).',
  );
}

const binaryName = process.platform === 'win32' ? 'recupere.exe' : 'recupere';
const appBinary = path.resolve(here, 'src-tauri', 'target', 'debug', binaryName);

let tauriDriverProcess: ChildProcess | null = null;

export const config: WebdriverIO.Config = {
  runner: 'local',
  specs: ['./e2e/native/**/*.spec.ts'],
  exclude: [],
  maxInstances: 1,
  capabilities: [
    {
      browserName: 'tauri',
      'tauri:options': { application: appBinary },
    },
  ],
  logLevel: 'warn',
  bail: 0,
  waitforTimeout: 30_000,
  connectionRetryTimeout: 60_000,
  connectionRetryCount: 0,
  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: {
    ui: 'bdd',
    timeout: 60_000,
  },
  hostname: '127.0.0.1',
  port: 4444,

  onPrepare() {
    if (!existsSync(appBinary)) {
      throw new Error(
        `Récupère debug binary not found at ${appBinary}. ` +
          'Run `cargo build --manifest-path src-tauri/Cargo.toml` first.',
      );
    }

    tauriDriverProcess = spawn('tauri-driver', [], {
      stdio: ['ignore', 'inherit', 'inherit'],
    });

    tauriDriverProcess.on('error', (err) => {
      throw new Error(
        `Failed to launch tauri-driver: ${err.message}. ` +
          'Install it with `cargo install tauri-driver --version 2.0.5`.',
      );
    });
  },

  onComplete() {
    if (tauriDriverProcess && !tauriDriverProcess.killed) {
      tauriDriverProcess.kill('SIGTERM');
      tauriDriverProcess = null;
    }
  },
};
