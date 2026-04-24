import { expect, test } from '@playwright/test';
import { seedBrowserPreviewState, seedOnboarding } from './helpers';

function cautiousImportedPreviewState() {
  return {
    onboardingComplete: true,
    appMode: 'manual',
    runtimeCapabilities: {
      deviceDetection: false,
      heuristicDiagnostic: true,
      aiAdvisory: false,
      optionalCloudAi: false,
      scanEngine: true,
      imagingEngine: true,
      resultsBrowser: true,
      exportValidation: true,
      exportEngine: true,
      technicalLogs: true,
    },
    devices: [
      {
        id: 'preview-cautious-imported-e01',
        name: 'Failing Evidence Image',
        devicePath: '/images/failing-case.e01',
        type: 'image',
        filesystem: 'unknown',
        capacityBytes: 8 * 1024 * 1024 * 1024,
        usedBytes: 0,
        status: 'failing',
        riskLevel: 'high',
        isTrimEnabled: false,
        isEncrypted: false,
        smartAvailable: false,
        partitions: [],
      },
    ],
    selectedDeviceId: 'preview-cautious-imported-e01',
    importedSourceStatusesByDeviceId: {
      'preview-cautious-imported-e01': {
        sourcePath: '/images/failing-case.e01',
        sourceFormat: 'E01',
        sourceAvailable: true,
        requiresPreparation: true,
        prepared: false,
        cachePath: '/preview-cache/preview-cautious-imported-e01.img',
      },
    },
  };
}

test('browser preview surfaces the cautious imaging profile from diagnostic to scan', async ({
  page,
  context,
}) => {
  await seedOnboarding(context);
  await seedBrowserPreviewState(context, cautiousImportedPreviewState());

  await page.goto('/diagnostic');

  await page
    .getByRole('button', {
      name: /prepare for analysis|preparer pour l'analyse/i,
    })
    .click();

  await expect(
    page.getByText(/cautious read-only profile|profil read-only prudent/i),
  ).toBeVisible();

  await page
    .getByRole('button', {
      name: /run signature carving from a local image|lancer le carving de signatures depuis une image locale/i,
    })
    .click();

  await expect(
    page.getByText(
      /cautious read-only imaging is active|profil d'imagerie read-only prudent est actif/i,
    ),
  ).toBeVisible();
});
