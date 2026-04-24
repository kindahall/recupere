import { expect, test } from '@playwright/test';
import { seedBrowserPreviewState, seedOnboarding } from './helpers';

function imagingResumeHistoryState() {
  return {
    onboardingComplete: true,
    appMode: 'manual',
    language: 'en',
    scanHistory: [
      {
        id: 'preview-image-resume-1',
        deviceId: 'disk-preview-1',
        deviceName: 'Field SSD Clone',
        scanType: 'image',
        startedAtMs: Date.parse('2026-04-09T09:00:00.000Z'),
        completedAtMs: Date.parse('2026-04-09T09:01:35.000Z'),
        status: 'completed',
        filesFound: 0,
        filesRecovered: 0,
        durationSeconds: 95,
        errors: 0,
        bytesCopied: 64 * 1024 * 1024,
        resumeFromBytes: 16 * 1024 * 1024,
      },
    ],
    scanLogsById: {
      'preview-image-resume-1': [
        {
          timestampMs: Date.parse('2026-04-09T09:01:30.000Z'),
          level: 'info',
          message:
            'Read-only imaging resumed from an existing partial local image (16777216 bytes already captured).',
        },
      ],
    },
  };
}

function degradedImagingHistoryState() {
  return {
    onboardingComplete: true,
    appMode: 'manual',
    language: 'en',
    scanHistory: [
      {
        id: 'preview-image-degraded-1',
        deviceId: 'disk-preview-2',
        deviceName: 'Failing SATA Disk',
        scanType: 'image',
        startedAtMs: Date.parse('2026-04-09T10:00:00.000Z'),
        completedAtMs: Date.parse('2026-04-09T10:04:20.000Z'),
        status: 'completed',
        filesFound: 0,
        filesRecovered: 0,
        durationSeconds: 260,
        errors: 2,
        bytesCopied: 96 * 1024 * 1024,
        totalBytes: 128 * 1024 * 1024,
        unreadableRangesCount: 2,
        unreadableBytes: 8 * 1024,
        rescuedAfterRetryBytes: 12 * 1024,
        retryPassesCompleted: 2,
        unreadableRanges: [
          {
            startOffset: 1_048_576,
            length: 4_096,
          },
          {
            startOffset: 7_340_032,
            length: 4_096,
          },
        ],
      },
    ],
    scanLogsById: {
      'preview-image-degraded-1': [
        {
          timestampMs: Date.parse('2026-04-09T10:03:30.000Z'),
          level: 'warning',
          message:
            'Read-only imaging completed with 2 unreadable source segment(s) neutralized as zero-filled gaps (8192 bytes total).',
        },
        {
          timestampMs: Date.parse('2026-04-09T10:03:35.000Z'),
          level: 'warning',
          message:
            'Unreadable source sample offsets: 1048576 (+4096 bytes), 7340032 (+4096 bytes).',
        },
      ],
    },
  };
}

test('history surfaces resumed read-only imaging sessions', async ({ page, context }) => {
  await seedOnboarding(context);
  await seedBrowserPreviewState(context, imagingResumeHistoryState());

  await page.goto('/history');

  await expect(page.getByRole('heading', { name: /scan history/i })).toBeVisible();
  await expect(page.getByText(/resumed imaging/i)).toBeVisible();

  await page.getByText('Field SSD Clone', { exact: true }).click();

  await expect(page.getByText(/session details/i)).toBeVisible();
  await expect(page.getByText(/imaging bytes copied/i)).toBeVisible();
  await expect(page.getByText(/64 MB/i)).toBeVisible();
  await expect(page.getByText(/^Resumed from:$/i)).toBeVisible();
  await expect(page.getByText(/16 MB/i)).toBeVisible();
  await expect(
    page.getByText(/read-only imaging resumed from an existing partial local image/i),
  ).toBeVisible();
});

test('history surfaces degraded read-only imaging sessions with unreadable gaps', async ({
  page,
  context,
}) => {
  await seedOnboarding(context);
  await seedBrowserPreviewState(context, degradedImagingHistoryState());

  await page.goto('/history');

  await expect(page.getByRole('heading', { name: /scan history/i })).toBeVisible();
  await expect(page.getByText(/unreadable source gaps/i)).toBeVisible();

  await page.getByText('Failing SATA Disk', { exact: true }).click();

  await expect(page.getByText(/session details/i)).toBeVisible();
  await expect(page.getByText(/Unreadable source segments:\s*2/i)).toBeVisible();
  await expect(page.getByText(/Zero-filled unreadable bytes:\s*8 KB/i)).toBeVisible();
  await expect(
    page.getByText(/read-only imaging completed with 2 unreadable source segment/i),
  ).toBeVisible();
  await expect(page.getByText(/unreadable source sample offsets: 1048576/i)).toBeVisible();
});

test('history warns that imaging report and rescue-map export require the native runtime in browser preview', async ({
  page,
  context,
}) => {
  await seedOnboarding(context);
  await seedBrowserPreviewState(context, degradedImagingHistoryState());

  await page.goto('/history');
  await page.getByText('Failing SATA Disk', { exact: true }).click();

  await expect(page.getByRole('button', { name: /export imaging report/i })).toBeVisible();
  await expect(page.getByRole('button', { name: /export rescue map/i })).toBeVisible();

  await page.getByRole('button', { name: /export imaging report/i }).click();

  await expect(
    page.getByText(
      /native save dialog is required before an imaging session report can be exported/i,
    ),
  ).toBeVisible();

  await page.getByRole('button', { name: /export rescue map/i }).click();

  await expect(
    page.getByText(/native save dialog is required before an imaging rescue map can be exported/i),
  ).toBeVisible();
});
