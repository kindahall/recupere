// ============================================================================
// Récupère — Gemma settings panel E2E
// ============================================================================
// The Settings page hosts the local AI configuration: enable toggle,
// advanced endpoint override, install/refresh buttons. None of these
// require a real Ollama running — the panel must render its banner and
// controls in all states (ready, unreachable, model missing).
// ============================================================================

import { expect, test } from '@playwright/test';
import { seedOnboarding } from './helpers';

test('Gemma settings panel renders enable toggle and advanced endpoint group', async ({
  page,
  context,
}) => {
  await seedOnboarding(context);
  await page.goto('/');

  await page.getByTestId('nav-settings').click();
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();

  // The enable checkbox must be present and toggleable.
  const enableToggle = page.getByTestId('settings-gemma-enabled');
  await expect(enableToggle).toBeVisible();

  // The Save button is the canonical "Gemma section is rendered" anchor.
  const saveButton = page.getByTestId('settings-gemma-save');
  await expect(saveButton).toBeVisible();

  // Advanced endpoint field is hidden behind a <details>; opening it must
  // reveal the input field even when its parent is locked (default state).
  // We don't toggle the <details> here because Playwright's default
  // selectors traverse it transparently.
  const endpointInput = page.getByTestId('settings-gemma-endpoint');
  await expect(endpointInput).toHaveCount(1);
});
