import { expect, test } from '@playwright/test';
import { seedBrowserPreviewState, seedOnboarding } from './helpers';

function buildResultsExportPreviewState() {
  const files = [
    {
      id: 'file-1',
      name: 'resume.docx',
      path: '/Users/demo/Documents/resume.docx',
      extension: 'docx',
      sizeBytes: 524288,
      integrity: 'intact',
      recoveryScore: 93,
      recoveryMethod: 'filesystem',
      previewAvailable: true,
      mimeType: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
      recoveryComplexity: 'low',
      validatorStatus: 'office-validated',
      journalDerived: false,
    },
    {
      id: 'file-2',
      name: 'vacation.jpg',
      path: '/Users/demo/Pictures/vacation.jpg',
      extension: 'jpg',
      sizeBytes: 1048576,
      isDeleted: true,
      integrity: 'partial',
      recoveryScore: 71,
      recoveryMethod: 'carving',
      previewAvailable: true,
      mimeType: 'image/jpeg',
      recoveryComplexity: 'medium',
      assemblySegmentCount: 2,
      gapCount: 1,
      journalDerived: false,
    },
  ];

  return {
    onboardingComplete: true,
    appMode: 'manual',
    licenseStatus: 'pro',
    runtimeCapabilities: {
      deviceDetection: false,
      heuristicDiagnostic: false,
      aiAdvisory: false,
      optionalCloudAi: false,
      scanEngine: false,
      imagingEngine: false,
      resultsBrowser: true,
      exportValidation: true,
      exportEngine: true,
      technicalLogs: true,
    },
    devices: [
      {
        id: 'preview-device-1',
        name: 'Preview Recovery Image',
        devicePath: '/images/preview-recovery.img',
        type: 'image',
        filesystem: 'ntfs',
        capacityBytes: 2 * 1024 * 1024 * 1024,
        usedBytes: 1024 * 1024 * 1024,
        status: 'healthy',
        riskLevel: 'low',
        isTrimEnabled: false,
        isEncrypted: false,
        smartAvailable: false,
        partitions: [],
      },
    ],
    selectedDeviceId: 'preview-device-1',
    activeScanId: 'preview-scan-1',
    scanConfig: {
      deviceId: 'preview-device-1',
      scanType: 'quick',
      targetFilesystems: ['ntfs'],
      enableCarving: true,
    },
    selectedFileIds: ['file-1'],
    recoveryResult: {
      scanId: 'preview-scan-1',
      totalFiles: files.length,
      intactFiles: 1,
      partialFiles: 1,
      fragmentedFiles: 0,
      corruptFiles: 0,
      totalSizeBytes: files.reduce((sum, file) => sum + file.sizeBytes, 0),
      recoverableSizeBytes: files.reduce((sum, file) => sum + file.sizeBytes, 0),
      files,
      treeRoot: {
        id: 'root',
        name: 'Recovered Files',
        isDirectory: true,
        children: [
          {
            id: 'dir-documents',
            name: 'Documents',
            isDirectory: true,
            children: [
              {
                id: 'node-file-1',
                name: 'resume.docx',
                isDirectory: false,
                file: files[0],
              },
            ],
          },
          {
            id: 'dir-pictures',
            name: 'Pictures',
            isDirectory: true,
            children: [
              {
                id: 'node-file-2',
                name: 'vacation.jpg',
                isDirectory: false,
                file: files[1],
              },
            ],
          },
        ],
      },
    },
  };
}

test('results fixture flows into export wizard and completes a preview export', async ({
  page,
  context,
}) => {
  await seedOnboarding(context);
  await seedBrowserPreviewState(context, buildResultsExportPreviewState());
  await page.goto('/');

  await page.getByTestId('nav-results').click();
  await expect(page.getByRole('heading', { name: /results|resultats/i })).toBeVisible();
  await expect(page.getByRole('table').getByText('resume.docx', { exact: true })).toBeVisible();

  await page.getByRole('button', { name: /export selected|exporter la selection/i }).click();
  await expect(page.getByRole('heading', { level: 1, name: /export/i })).toBeVisible();

  const next = page.getByRole('button', { name: /next|suivant/i });
  await next.click();

  const destinationInput = page.getByPlaceholder('/Volumes/ExternalDrive/Recovery');
  await destinationInput.fill('/Users/demo/Exports/Recovery');
  await page.getByRole('button', { name: /validate|valider/i }).click();
  await expect(page.getByText(/browser preview|desktop backend|validated/i)).toBeVisible();

  await next.click();
  await page.getByRole('button', { name: /start export|demarrer|commencer/i }).click();

  await expect(
    page.getByText(/^Export completed successfully\.$|^L'export est termine avec succes\.$/i),
  ).toBeVisible();
  await expect(page.getByText(/preview export completed/i)).toBeVisible();
});
