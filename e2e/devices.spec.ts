// ============================================================================
// Récupère — Devices page E2E
// ============================================================================
// Smoke checks for the Supports / Devices page:
//   - the page loads without throwing
//   - the refresh button is wired
//   - the filter tabs (All / Removable / Internal / Images) are reachable
// We don't depend on real hardware being attached; the page must render an
// empty state when no devices are detected (browser preview = sysinfo
// returns nothing) and still expose its controls.
// ============================================================================

import { expect, test } from '@playwright/test';
import { seedBrowserPreviewState, seedOnboarding } from './helpers';

test('Devices page renders header, filters, and refresh control', async ({ page, context }) => {
  await seedOnboarding(context);
  await seedBrowserPreviewState(context, {
    devices: [
      {
        id: 'preview-device-1',
        name: 'Encrypted RAID Preview',
        devicePath: '/images/encrypted-raid.img',
        type: 'image',
        filesystem: 'unknown',
        capacityBytes: 4 * 1024 * 1024 * 1024,
        usedBytes: 2 * 1024 * 1024 * 1024,
        status: 'degraded',
        riskLevel: 'high',
        isTrimEnabled: false,
        isEncrypted: true,
        smartAvailable: false,
        partitions: [],
      },
    ],
    raidMetadataByDeviceId: {
      'preview-device-1': {
        level: 'Raid5',
        memberCount: 4,
        stripeSizeBytes: 65536,
        superblockVersion: '1.2',
        dataOffsetBytes: 1048576,
      },
    },
    encryptionInfoByDeviceId: {
      'preview-device-1': {
        detected: true,
        encryptionType: 'bitlocker',
        canUnlock: true,
        message: 'BitLocker volume detected. Password unlock available.',
      },
    },
    importedSourceStatusesByDeviceId: {
      'preview-device-1': {
        sourcePath: '/images/encrypted-raid.img',
        sourceFormat: 'VMDK',
        sourceAvailable: true,
        requiresPreparation: true,
        prepared: false,
        cachePath: '/preview-cache/preview-device-1.img',
      },
    },
  });
  await page.goto('/');

  // Navigate via the sidebar.
  await page.getByTestId('nav-devices').click();

  // The page header must be visible (heading text is i18n-driven; we match
  // permissively to stay locale-agnostic).
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
  await expect(
    page.getByText(
      /remote-agent management requires the desktop app|gestion des agents distants nécessite l'application desktop/i,
    ),
  ).toBeVisible();

  // The refresh button must be present and clickable without throwing.
  const refresh = page.getByRole('button', { name: /refresh|actualiser/i }).first();
  await expect(refresh).toBeVisible();
  await refresh.click();

  const importImage = page.getByRole('button', { name: /open image|ouvrir une image/i }).first();
  await expect(importImage).toBeVisible();
  await importImage.click();
  await expect(
    page.getByText(
      /recovery-source import is only available in the desktop app|l'import d'une source de recuperation n'est disponible que dans l'application desktop/i,
    ),
  ).toBeVisible();

  await expect(
    page.getByText(
      /requires a local read-only preparation step|necessite une preparation locale read-only/i,
    ),
  ).toBeVisible();

  const diagnose = page.getByRole('button', { name: /diagnose|diagnostiquer/i }).first();
  await expect(diagnose).toBeDisabled();

  const prepare = page
    .getByRole('button', { name: /prepare for analysis|preparer pour l'analyse/i })
    .first();
  await expect(prepare).toBeVisible();
  await prepare.click();

  await expect(
    page.getByText(/ready for deeper inspection|pret pour une inspection plus poussee/i),
  ).toBeVisible();
  await expect(diagnose).toBeEnabled();

  const advancedSignals = page
    .getByRole('button', { name: /advanced signals|signaux avances/i })
    .first();
  await expect(advancedSignals).toBeVisible();
  await advancedSignals.click();

  await expect(page.getByText(/raid metadata|metadonnees raid/i)).toBeVisible();
  await expect(page.getByText(/^BITLOCKER metadata detected on this source\.$/i)).toBeVisible();
});
