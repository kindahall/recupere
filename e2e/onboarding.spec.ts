// ============================================================================
// Récupère — Onboarding wizard E2E
// ============================================================================
// Ensures the first-launch wizard renders, all steps are reachable, and the
// user can complete it without depending on Ollama or Gemma being installed.
// Also covers the regression detected during refactor: shipping a new
// onboarding overlay broke every other E2E test that assumed the app boots
// directly on the home page.
// ============================================================================

import { expect, test } from '@playwright/test';
import { clearOnboarding } from './helpers';

test.describe('Onboarding wizard', () => {
  test('walks through Welcome → Mode → Ready in manual mode', async ({ page, context }) => {
    await clearOnboarding(context);
    await page.goto('/');

    // Step 1: Welcome
    const getStarted = page.getByTestId('onboarding-get-started');
    await expect(getStarted).toBeVisible();
    await getStarted.click();

    // Step 2: Mode selection — pick Manual to avoid the AI setup branch.
    await expect(page.getByTestId('onboarding-mode-manual')).toBeVisible();
    await page.getByTestId('onboarding-mode-manual').click();

    // Step 3 (Ready) auto-advances after a short delay because manual mode
    // skips the AI setup step.
    const finish = page.getByTestId('onboarding-finish');
    await expect(finish).toBeVisible({ timeout: 5000 });
    await finish.click();

    // The router is now active — Settings nav must be reachable.
    await expect(page.getByTestId('nav-settings')).toBeVisible();
  });

  test('AI Guided mode shows the Gemma setup step', async ({ page, context }) => {
    await clearOnboarding(context);
    await page.goto('/');

    await page.getByTestId('onboarding-get-started').click();
    await page.getByTestId('onboarding-mode-ai').click();

    // Step 3: AI setup. We don't actually pull Gemma — we just verify the
    // checklist UI is rendered with the install/refresh buttons.
    await expect(page.getByTestId('onboarding-install-ollama')).toBeVisible({
      timeout: 5000,
    });

    // The "Skip for now" button must be reachable so the user is never
    // trapped in the wizard if Ollama is unreachable.
    const next = page.getByTestId('onboarding-next');
    await expect(next).toBeVisible();
    await next.click();

    // Should land on the Ready step.
    await expect(page.getByTestId('onboarding-finish')).toBeVisible();
  });
});
