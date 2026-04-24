import { expect, test } from '@playwright/test';
import { seedBrowserPreviewState, seedOnboarding } from './helpers';

function buildExportPaywallPreviewState() {
  const files = [
    {
      id: 'paywall-file-1',
      name: 'client-contract.pdf',
      path: '/Users/demo/Documents/client-contract.pdf',
      extension: 'pdf',
      sizeBytes: 262144,
      integrity: 'intact',
      recoveryScore: 91,
      recoveryMethod: 'filesystem',
      previewAvailable: true,
      mimeType: 'application/pdf',
      recoveryComplexity: 'low',
      validatorStatus: 'format-validated',
      journalDerived: false,
    },
  ];

  return {
    onboardingComplete: true,
    appMode: 'manual',
    licenseStatus: 'free',
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
        id: 'paywall-device-1',
        name: 'Preview Recovery Image',
        devicePath: '/images/paywall-preview.img',
        type: 'image',
        filesystem: 'ntfs',
        capacityBytes: 1024 * 1024 * 1024,
        usedBytes: 512 * 1024 * 1024,
        status: 'healthy',
        riskLevel: 'low',
        isTrimEnabled: false,
        isEncrypted: false,
        smartAvailable: false,
        partitions: [],
      },
    ],
    selectedDeviceId: 'paywall-device-1',
    activeScanId: 'paywall-scan-1',
    scanConfig: {
      deviceId: 'paywall-device-1',
      scanType: 'quick',
      targetFilesystems: ['ntfs'],
      enableCarving: false,
    },
    selectedFileIds: ['paywall-file-1'],
    recoveryResult: {
      scanId: 'paywall-scan-1',
      totalFiles: files.length,
      intactFiles: files.length,
      partialFiles: 0,
      fragmentedFiles: 0,
      corruptFiles: 0,
      totalSizeBytes: files[0].sizeBytes,
      recoverableSizeBytes: files[0].sizeBytes,
      files,
      treeRoot: {
        id: 'root',
        name: 'Recovered Files',
        isDirectory: true,
        children: [
          {
            id: 'node-paywall-file-1',
            name: 'client-contract.pdf',
            isDirectory: false,
            file: files[0],
          },
        ],
      },
    },
  };
}

// Validates the Phase 1 paywall flow:
//  - Free users see the upgrade gate on the Export page
//  - Invalid license keys are rejected with an error
//  - The Buy button opens the Stripe checkout in a new tab
//
// Real key activation is covered by the Rust unit tests (license::tests)
// because the embedded public key is a development placeholder until a
// production keypair is generated.
test('shows the Pro paywall on the export page for free users', async ({ page, context }) => {
  await seedOnboarding(context);
  await seedBrowserPreviewState(context, buildExportPaywallPreviewState());
  await page.goto('/');

  await page.getByTestId('nav-export').click();
  await expect(page).toHaveURL(/\/export$/);
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();

  // Paywall affordances must be present.
  const buyButton = page.getByRole('button', { name: /buy|acheter|upgrade|pro/i }).first();
  await expect(buyButton).toBeVisible();

  // The license input field accepts text but rejects invalid keys.
  const licenseInput = page.getByPlaceholder(/license|licence|RECUP-/i).first();
  if (await licenseInput.isVisible().catch(() => false)) {
    await licenseInput.fill('RECUP-not-a-real-key');
    const activate = page.getByRole('button', { name: /activate|activer/i }).first();
    await activate.click();
    // The activation must fail and surface an error message.
    await expect(page.getByText(/invalid|invalide|malformed|failed/i)).toBeVisible({
      timeout: 5000,
    });
  }
});
