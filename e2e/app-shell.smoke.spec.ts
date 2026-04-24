import { expect, test } from '@playwright/test';
import { seedOnboarding } from './helpers';

test('navigates the shell, handles support bundle fallback, and switches mode/language', async ({
  page,
  context,
}) => {
  await seedOnboarding(context);
  await page.goto('/');

  await expect(page.getByRole('heading', { name: 'Data Recovery' })).toBeVisible();
  await expect(
    page.getByText('All operations are strictly read-only. Your source data is safe.'),
  ).toBeVisible();

  await page.getByTestId('nav-settings').click();
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();

  await page.getByTestId('settings-support-bundle-export').click();
  await expect(
    page.getByText(
      'A native save dialog is required before a support bundle can be exported in this environment.',
    ),
  ).toBeVisible();

  await page.getByTestId('settings-mode-expert').click();
  await expect(page.getByTestId('nav-expert')).toBeVisible();

  await page.getByTestId('nav-expert').click();
  await expect(page.getByRole('heading', { name: 'Expert Mode' })).toBeVisible();

  await page.getByTestId('nav-settings').click();
  await page.getByTestId('settings-language-fr').click();

  await expect(page.getByRole('heading', { name: 'Paramètres' })).toBeVisible();
  await expect(page.getByTestId('nav-home')).toContainText('Accueil');
  await expect(page.getByTestId('nav-settings')).toContainText('Paramètres');

  await page.getByTestId('nav-home').click();
  await expect(page.getByRole('heading', { name: 'Récupération de données' })).toBeVisible();
  await expect(
    page.getByText(
      'Toutes les opérations sont strictement en lecture seule. Vos données source sont en sécurité.',
    ),
  ).toBeVisible();
});
