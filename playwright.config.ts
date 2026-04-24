// ============================================================================
// Playwright — browser-preview E2E harness
// ============================================================================
//
// IMPORTANT: these tests run against the `vite preview` bundle with the
// `__ALLOW_BROWSER_PREVIEW__` flag on. They exercise the React UI and its
// state wiring against deterministic fixtures — they do NOT launch the real
// Tauri binary. IPC is mocked via `seedBrowserPreviewState()` in
// `e2e/helpers.ts`.
//
// If you change IPC command shapes or native-only behaviour, these specs
// will not catch the regression. A separate native-smoke harness — tracked
// in the audit plan under chantier C4 — is required before we can claim
// "Tauri desktop E2E" coverage.
//
// ============================================================================
import { defineConfig, devices } from '@playwright/test';

const port = 4173;
const baseURL = `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: process.env.CI ? [['html'], ['list']] : 'list',
  use: {
    baseURL,
    trace: 'on-first-retry',
  },
  webServer: {
    // Playwright needs the browser-preview fixtures baked into the prod
    // bundle so it can exercise the UI without a live Tauri backend. The
    // opt-in env var is honored by `vite.config.ts`; any other prod build
    // strips the fixtures (see Sprint 1.4 in the audit plan).
    command: `npm run build && npm run preview -- --host 127.0.0.1 --port ${port} --strictPort`,
    env: {
      RECUPERE_ENABLE_BROWSER_PREVIEW: '1',
    },
    port,
    reuseExistingServer: !process.env.CI,
    timeout: 120000,
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
});
