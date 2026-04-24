#!/usr/bin/env node
// ============================================================================
// run-native-e2e.mjs — platform dispatcher for the native E2E harness
// ============================================================================
//
// Chantier 83. Picks the right WebdriverIO config for the current OS:
//   - macOS   → wdio.appium.conf.ts        (Appium Mac2 + WKWebView)
//   - Linux   → wdio.tauri-driver.conf.ts  (tauri-driver + webkit2gtk)
//   - Windows → wdio.tauri-driver.conf.ts  (tauri-driver + msedgedriver)
//
// Forwards any extra CLI args to `wdio run`. Exits with the runner's code.
// ============================================================================

import { spawn } from 'node:child_process';

const platform = process.platform;
const config =
  platform === 'darwin' ? 'wdio.appium.conf.ts' : 'wdio.tauri-driver.conf.ts';

if (!['darwin', 'linux', 'win32'].includes(platform)) {
  console.error(`Unsupported platform "${platform}" for native E2E harness.`);
  process.exit(2);
}

const child = spawn('npx', ['wdio', 'run', config, ...process.argv.slice(2)], {
  stdio: 'inherit',
  shell: process.platform === 'win32',
});

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
