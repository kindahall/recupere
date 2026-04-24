import AxeBuilder from '@axe-core/playwright';
import { type Page, expect, test } from '@playwright/test';

// WCAG 2.1 AA smoke. We run axe-core against every top-level page the app
// exposes to the browser-preview webServer (configured in
// `playwright.config.ts` with RECUPERE_ENABLE_BROWSER_PREVIEW=1). Any
// `critical` or `serious` axe violation fails the run. The UI has many
// `moderate`/`minor` findings today; locking them in as errors would be
// noise, so the first gate focuses on the highest impact tier.
//
// When a page surfaces a new serious violation, the right fix is almost
// always in the source (missing label, low contrast, invalid ARIA), not a
// test exclusion. Exclusions are intentionally disabled here.

async function auditPage(page: Page): Promise<void> {
  const results = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze();

  const severeViolations = results.violations.filter(
    (v) => v.impact === 'critical' || v.impact === 'serious',
  );

  expect(
    severeViolations,
    severeViolations
      .map(
        (v) =>
          `${v.id} (${v.impact}): ${v.help}\n  ${v.nodes
            .map((n) => n.target.join(', '))
            .slice(0, 5)
            .join('\n  ')}`,
      )
      .join('\n'),
  ).toEqual([]);
}

test.describe('WCAG 2.1 AA smoke', () => {
  const routes = [
    { name: 'home', url: '/' },
    { name: 'devices', url: '/devices' },
    { name: 'history', url: '/history' },
    { name: 'settings', url: '/settings' },
  ];

  for (const route of routes) {
    test(`${route.name} page has no critical or serious axe violations`, async ({ page }) => {
      await page.goto(route.url);
      // Give lazy-loaded pages a moment to settle before scanning.
      await page.waitForLoadState('networkidle');
      await auditPage(page);
    });
  }
});
