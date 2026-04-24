import { describe, expect, it } from 'vitest';
import enCatalogue from './locales/en.json';
import frCatalogue from './locales/fr.json';

type TranslationNode = string | { [key: string]: TranslationNode };

function flatten(node: TranslationNode, prefix = ''): Set<string> {
  const keys = new Set<string>();
  if (typeof node === 'string') {
    keys.add(prefix);
    return keys;
  }
  for (const [key, value] of Object.entries(node)) {
    const nextPrefix = prefix ? `${prefix}.${key}` : key;
    for (const subKey of flatten(value, nextPrefix)) {
      keys.add(subKey);
    }
  }
  return keys;
}

function diff(a: Set<string>, b: Set<string>): string[] {
  const out: string[] = [];
  for (const key of a) {
    if (!b.has(key)) {
      out.push(key);
    }
  }
  return out.sort();
}

describe('i18n parity', () => {
  // AGENTS.md mandates EN default + FR support. The two catalogues must
  // expose the exact same leaf keys so a missing translation surfaces as a
  // build-time test failure rather than a mystery literal in the UI.

  const enKeys = flatten(enCatalogue as TranslationNode);
  const frKeys = flatten(frCatalogue as TranslationNode);

  it('every English key has a French counterpart', () => {
    const missingInFr = diff(enKeys, frKeys);
    expect(missingInFr, `missing French translations for ${missingInFr.length} keys`).toEqual([]);
  });

  it('every French key has an English counterpart', () => {
    const missingInEn = diff(frKeys, enKeys);
    expect(missingInEn, `missing English translations for ${missingInEn.length} keys`).toEqual([]);
  });

  it('no translation is an empty string', () => {
    const emptyKeys: string[] = [];
    const recurse = (node: TranslationNode, prefix: string, locale: 'en' | 'fr') => {
      if (typeof node === 'string') {
        if (node.trim().length === 0) {
          emptyKeys.push(`${locale}:${prefix}`);
        }
        return;
      }
      for (const [key, value] of Object.entries(node)) {
        recurse(value, prefix ? `${prefix}.${key}` : key, locale);
      }
    };
    recurse(enCatalogue as TranslationNode, '', 'en');
    recurse(frCatalogue as TranslationNode, '', 'fr');
    expect(emptyKeys).toEqual([]);
  });
});
