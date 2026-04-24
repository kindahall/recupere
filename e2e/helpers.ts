// ============================================================================
// Récupère — Playwright shared helpers
// ============================================================================
// Most pages assume the onboarding wizard has already been completed. We
// inject the persisted state into localStorage *before* the app boots so the
// router renders normally instead of showing the onboarding overlay.
//
// Storage key and shape are mirrored from
// `src/stores/appStore.ts::ONBOARDING_STORAGE_KEY`.
// ============================================================================

import type { BrowserContext, Page } from '@playwright/test';

const ONBOARDING_STORAGE_KEY = 'recupere.onboarding.v1';
const BROWSER_PREVIEW_STATE_KEY = 'recupere.browser-preview.v1';

export interface OnboardingState {
  appMode: 'ai-guided' | 'manual' | 'not-chosen';
  onboardingComplete: boolean;
}

/**
 * Pre-seed the onboarding state for a Playwright context. Must be called
 * BEFORE the first navigation, otherwise the app boots once with the
 * onboarding visible.
 *
 * Default: manual mode + onboarding complete (no Gemma dependency).
 */
export async function seedOnboarding(
  context: BrowserContext,
  state: Partial<OnboardingState> = {},
): Promise<void> {
  const merged: OnboardingState = {
    appMode: 'manual',
    onboardingComplete: true,
    ...state,
  };
  // Inject the localStorage entry on every page load before any script runs.
  await context.addInitScript(
    ({ key, value }) => {
      try {
        window.localStorage.setItem(key, value);
      } catch {
        /* private mode — ignore */
      }
    },
    { key: ONBOARDING_STORAGE_KEY, value: JSON.stringify(merged) },
  );
}

/**
 * Clear the onboarding state so the wizard appears on next navigation. Use
 * this in tests that explicitly cover the onboarding flow.
 */
export async function clearOnboarding(context: BrowserContext): Promise<void> {
  await context.addInitScript((key) => {
    try {
      window.localStorage.removeItem(key);
    } catch {
      /* ignore */
    }
  }, ONBOARDING_STORAGE_KEY);
}

export async function seedBrowserPreviewState(
  context: BrowserContext,
  state: Record<string, unknown>,
): Promise<void> {
  await context.addInitScript(
    ({ key, value }) => {
      try {
        window.localStorage.setItem(key, value);
      } catch {
        /* ignore */
      }
    },
    { key: BROWSER_PREVIEW_STATE_KEY, value: JSON.stringify(state) },
  );
}

export async function clearBrowserPreviewState(context: BrowserContext): Promise<void> {
  await context.addInitScript((key) => {
    try {
      window.localStorage.removeItem(key);
    } catch {
      /* ignore */
    }
  }, BROWSER_PREVIEW_STATE_KEY);
}

/**
 * Convenience: navigate to the app root with onboarding pre-seeded.
 */
export async function gotoApp(page: Page, path = '/'): Promise<void> {
  await page.goto(path);
}
