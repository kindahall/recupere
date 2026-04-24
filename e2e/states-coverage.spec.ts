import { expect, test } from '@playwright/test';
import { seedOnboarding } from './helpers';

// Sprint 2.3 state coverage gate. Each public page must render its shell
// without throwing, and route guards that protect destructive features
// (expert workbench, results before a scan is active) must do their job.
// The checks are intentionally blunt — detailed content assertions live in
// the per-feature specs so this file stays fast and stable.

test.beforeEach(async ({ context }) => {
  // Every test starts from a consistent post-onboarding state. Without
  // this, the app bounces to the onboarding overlay and the page we care
  // about never mounts.
  await seedOnboarding(context);
});

test('home page renders without errors', async ({ page }) => {
  await page.goto('/');
  await page.waitForLoadState('networkidle');
  await expect(page.locator('body')).toBeVisible();
  // The app shell always mounts the sidebar; its presence is proof the
  // React tree committed successfully.
  await expect(page.locator('[data-testid="sidebar-nav"], nav, aside').first()).toBeVisible();
});

test('devices page renders', async ({ page }) => {
  await page.goto('/devices');
  await page.waitForLoadState('networkidle');
  await expect(page.locator('body')).toBeVisible();
});

test('settings page renders the mode toggle', async ({ page }) => {
  await page.goto('/settings');
  await page.waitForLoadState('networkidle');
  await expect(page.getByTestId('settings-mode-novice')).toBeVisible();
  await expect(page.getByTestId('settings-mode-expert')).toBeVisible();
});

test('expert route redirects novice users to settings with gate banner', async ({ page }) => {
  // Onboarding seed leaves userMode as novice. Visiting /expert must bounce
  // the user to /settings?from=expert where the gate banner is surfaced.
  await page.goto('/expert');
  await page.waitForURL(/\/settings(\?|$)/);
  await expect(page.getByTestId('settings-expert-gate-banner')).toBeVisible();
});

test('results route without an active scan redirects to /devices', async ({ page }) => {
  await page.goto('/results');
  await page.waitForURL(/\/devices$/);
});

test('export route without an active scan also redirects', async ({ page }) => {
  await page.goto('/export');
  // Both `/scan` and `/devices` are valid destinations depending on whether
  // a device was selected; the invariant is that the user is NOT left on
  // `/export` with an undefined scan id.
  await page.waitForURL(/\/(devices|scan)$/);
});

test('history page renders', async ({ page }) => {
  await page.goto('/history');
  await page.waitForLoadState('networkidle');
  await expect(page.locator('body')).toBeVisible();
});
