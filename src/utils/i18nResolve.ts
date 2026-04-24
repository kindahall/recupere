import type { TFunction } from 'i18next';

/**
 * Resolves a backend i18n key to translated text.
 *
 * Backend sends keys like:
 *   - Simple: "ai_recovery.caution.fragmented_incomplete"
 *   - Composite with pipe: "ai_advisory.summary.high|recommendation.scan_deleted.title"
 *   - Expert note with value: "ai_recovery.expert.results_analyzed|42"
 *
 * If the key resolves to itself (not found), returns the raw key as fallback.
 */
export function resolveKey(t: TFunction, key: string): string {
  if (!key) return '';

  // Handle pipe-separated composite keys
  if (key.includes('|')) {
    const parts = key.split('|');
    // Try to resolve the first part as a key
    const resolved = t(parts[0], { defaultValue: parts[0] });
    if (parts.length === 2) {
      // Second part could be a key or a value
      const secondResolved = t(parts[1], { defaultValue: parts[1] });
      return `${resolved}: ${secondResolved}`;
    }
    return parts.map((p) => t(p, { defaultValue: p })).join(' ');
  }

  // Simple key resolution
  const resolved = t(key, { defaultValue: key });
  return resolved;
}

/**
 * Resolves an array of backend i18n keys to translated text.
 */
export function resolveKeys(t: TFunction, keys: string[]): string[] {
  return keys.map((key) => resolveKey(t, key));
}
