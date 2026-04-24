import { expect, test } from '@playwright/test';
import { seedBrowserPreviewState, seedOnboarding } from './helpers';

function importedPreviewFlowState() {
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
        id: 'preview-imported-e01',
        name: 'Imported Evidence Image',
        devicePath: '/images/case.e01',
        type: 'image',
        filesystem: 'unknown',
        capacityBytes: 16 * 1024 * 1024 * 1024,
        usedBytes: 0,
        status: 'healthy',
        riskLevel: 'medium',
        isTrimEnabled: false,
        isEncrypted: false,
        smartAvailable: false,
        partitions: [],
      },
    ],
    selectedDeviceId: 'preview-imported-e01',
    importedSourceStatusesByDeviceId: {
      'preview-imported-e01': {
        sourcePath: '/images/case.e01',
        sourceFormat: 'E01',
        sourceAvailable: true,
        requiresPreparation: true,
        prepared: false,
        cachePath: '/preview-cache/preview-imported-e01.img',
      },
    },
  };
}

test('browser preview can prepare an imported source, load diagnostic, and complete a scan flow', async ({
  page,
  context,
}) => {
  await seedOnboarding(context);
  await seedBrowserPreviewState(context, importedPreviewFlowState());

  await page.goto('/diagnostic');

  await expect(
    page.getByText(/fixture-backed diagnostic|preview navigateur affiche ici un diagnostic/i),
  ).toBeVisible();
  await expect(
    page.getByRole('button', { name: /prepare for analysis|preparer pour l'analyse/i }),
  ).toBeVisible();

  await page.getByRole('button', { name: /prepare for analysis|preparer pour l'analyse/i }).click();

  await expect(
    page.getByRole('button', {
      name: /run signature carving from a local image|lancer le carving de signatures depuis une image locale/i,
    }),
  ).toBeVisible();

  await page
    .getByRole('button', {
      name: /run signature carving from a local image|lancer le carving de signatures depuis une image locale/i,
    })
    .click();

  await expect(page.getByText(/fixture-backed|reposent sur des fixtures/i)).toBeVisible();
  await expect(
    page.getByRole('button', { name: /view results|voir les resultats/i }),
  ).toBeVisible();

  await page.getByRole('button', { name: /view results|voir les resultats/i }).click();

  await expect(page.getByRole('heading', { name: /results|resultats/i })).toBeVisible();
  await expect(
    page.getByRole('table').getByText('recovered-report.pdf', { exact: true }),
  ).toBeVisible();
});
