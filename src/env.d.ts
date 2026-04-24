declare const __APP_VERSION__: string;
/**
 * Build-time flag exposed by `vite.config.ts`. `true` in `npm run dev` and in
 * prod builds that opt in via `RECUPERE_ENABLE_BROWSER_PREVIEW=1` (Playwright
 * uses this). `false` in a regular `npm run build`, which lets Rollup
 * tree-shake every fixture branch out of the shipped bundle.
 */
declare const __ALLOW_BROWSER_PREVIEW__: boolean;
