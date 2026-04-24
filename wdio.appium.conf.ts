// ============================================================================
// WebdriverIO — native Tauri harness via Appium Mac2 (macOS only)
// ============================================================================
//
// Chantier 83. Drives the real Récupère window on macOS by attaching Appium
// Mac2 driver via XCTest, then switching context into the WKWebView so the
// specs can interact with the React DOM.
//
// Why not tauri-driver? Apple blocks external WebDriver from attaching to a
// WKWebView for security reasons. Mac2 driver works around this by using the
// Accessibility API + the WebKit Web Inspector bridge that Appium ships.
//
// Cohabits with `wdio.tauri-driver.conf.ts` (Linux + Windows) and Playwright
// browser-preview (`playwright.config.ts`). Each config picks one stack;
// none replaces the others.
//
// Prerequisites (see README "Running E2E tests"):
//   - Récupère debug binary built: `cargo build --manifest-path src-tauri/Cargo.toml`
//   - Appium 3.x running externally on 127.0.0.1:4723 with the Mac2 driver
//     installed. Appium is intentionally not a repo devDependency because its
//     current Mac2 dependency tree carries audit advisories.
//   - First run: macOS will prompt for Accessibility permission. Grant it
//     to the binary running the spec (typically `node` or `tsx`) under
//     System Settings → Privacy & Security → Accessibility.
//   - WKWebView debug introspection: Tauri 2 sets `inspectable = true` on
//     debug builds; nothing else to do.
//
// ============================================================================
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import '@wdio/types';

const here = path.dirname(fileURLToPath(import.meta.url));

if (process.platform !== 'darwin') {
  throw new Error(
    'wdio.appium.conf.ts is macOS-only. Use wdio.tauri-driver.conf.ts on ' +
      'Linux and Windows.',
  );
}

const appBinary = path.resolve(here, 'src-tauri', 'target', 'debug', 'recupere');
const RECUPERE_BUNDLE_ID = 'com.recupere.desktop';

export const config: WebdriverIO.Config = {
  runner: 'local',
  specs: ['./e2e/native/**/*.spec.ts'],
  exclude: [],
  maxInstances: 1,
  capabilities: [
    {
      platformName: 'mac',
      'appium:automationName': 'mac2',
      'appium:bundleId': RECUPERE_BUNDLE_ID,
      'appium:arguments': [],
      // Fall back to launching the raw binary if the .app bundle is not
      // installed in /Applications. Mac2 driver accepts either bundleId
      // (already installed) or app (path to launch).
      'appium:app': appBinary,
      'appium:noReset': true,
      'appium:showServerLogs': false,
    },
  ],
  logLevel: 'warn',
  bail: 0,
  waitforTimeout: 30_000,
  connectionRetryTimeout: 90_000,
  connectionRetryCount: 0,
  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: {
    ui: 'bdd',
    timeout: 90_000,
  },
  hostname: '127.0.0.1',
  port: 4723,
  path: '/',

  onPrepare() {
    if (!existsSync(appBinary)) {
      throw new Error(
        `Récupère debug binary not found at ${appBinary}. ` +
          'Run `cargo build --manifest-path src-tauri/Cargo.toml` first.',
      );
    }
  },
};
