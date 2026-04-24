import { expect, test } from '@playwright/test';
import { seedBrowserPreviewState, seedOnboarding } from './helpers';

function importedVirtualSourceState() {
  return {
    devices: [
      {
        id: 'preview-imported-vmdk',
        name: 'Imported Virtual Disk',
        devicePath: '/images/case.vmdk',
        type: 'image',
        filesystem: 'unknown',
        capacityBytes: 8 * 1024 * 1024 * 1024,
        usedBytes: 0,
        status: 'healthy',
        riskLevel: 'medium',
        isTrimEnabled: false,
        isEncrypted: false,
        smartAvailable: false,
        partitions: [],
      },
    ],
    selectedDeviceId: 'preview-imported-vmdk',
    importedSourceStatusesByDeviceId: {
      'preview-imported-vmdk': {
        sourcePath: '/images/case.vmdk',
        sourceFormat: 'VMDK',
        sourceAvailable: true,
        requiresPreparation: true,
        prepared: false,
        cachePath: '/preview-cache/preview-imported-vmdk.img',
      },
    },
  };
}

test('Diagnostic page blocks imported virtual sources until preparation is explicit', async ({
  page,
  context,
}) => {
  await seedOnboarding(context);
  await seedBrowserPreviewState(context, importedVirtualSourceState());

  await page.goto('/diagnostic');

  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
  await expect(
    page.getByText(/diagnostic stays explicit|diagnostic reste explicite/i),
  ).toBeVisible();
  await expect(
    page.getByText(/imported source readiness|etat de preparation de la source importee/i),
  ).toBeVisible();
  await expect(
    page.getByRole('button', { name: /prepare for analysis|preparer pour l'analyse/i }),
  ).toBeVisible();
  await expect(
    page.getByText(/diagnostic data is unavailable|les donnees de diagnostic sont indisponibles/i),
  ).toHaveCount(0);
});

test('Scan page blocks auto-start until imported virtual sources are prepared', async ({
  page,
  context,
}) => {
  await seedOnboarding(context);
  await seedBrowserPreviewState(context, {
    ...importedVirtualSourceState(),
    scanConfig: {
      deviceId: 'preview-imported-vmdk',
      scanType: 'signature-carving',
      targetFilesystems: ['unknown'],
      enableCarving: true,
    },
  });

  await page.goto('/scan');

  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
  await expect(
    page.getByText(
      /scan workflows reuse an explicit local read-only analysis path|workflows d'analyse reutilisent un chemin local read-only explicite/i,
    ),
  ).toBeVisible();
  await expect(
    page.getByRole('button', { name: /prepare for analysis|preparer pour l'analyse/i }),
  ).toBeVisible();
  await expect(
    page.getByText(/waiting for backend log entries|en attente des entrees de journal du backend/i),
  ).toHaveCount(0);
});
