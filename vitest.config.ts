import { defineConfig } from 'vitest/config';

export default defineConfig({
  // Mirrors `vite.config.ts` so unit tests see the same `__ALLOW_BROWSER_PREVIEW__`
  // substitution the prod bundle sees. Vitest otherwise leaves the identifier
  // undefined and every guarded branch throws at runtime.
  define: {
    __APP_VERSION__: JSON.stringify('0.0.0-test'),
    __ALLOW_BROWSER_PREVIEW__: JSON.stringify(true),
  },
  test: {
    exclude: ['e2e/**', 'node_modules/**', 'dist/**', 'src-tauri/**'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html'],
      include: ['src/**/*.{ts,tsx}'],
      exclude: [
        'src/**/*.d.ts',
        'src/**/*.test.ts',
        'src/**/*.test.tsx',
        'src/browser-preview/**',
        'src/main.tsx',
        'src/types/**',
      ],
    },
  },
});
