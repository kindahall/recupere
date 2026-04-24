import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

function buildManualChunkName(id: string): string | undefined {
  const normalizedId = id.replace(/\\/g, "/");
  if (!normalizedId.includes("/node_modules/")) {
    return undefined;
  }

  if (normalizedId.includes("/@tauri-apps/")) {
    return "tauri";
  }

  if (
    normalizedId.includes("/react-dom/")
    || normalizedId.includes("/react-router/")
    || normalizedId.includes("/react-router-dom/")
    || normalizedId.includes("/react/")
  ) {
    return "react-vendor";
  }

  if (
    normalizedId.includes("/react-i18next/")
    || normalizedId.includes("/i18next/")
    || normalizedId.includes("/html-parse-stringify/")
  ) {
    return "i18n";
  }

  if (normalizedId.includes("/lucide-react/")) {
    return "icons";
  }

  if (normalizedId.includes("/zustand/")) {
    return "state";
  }

  return undefined;
}

// https://vite.dev/config/
export default defineConfig(async ({ mode }) => {
  // Browser-preview fixtures exist so Playwright can exercise the UI without
  // a live Tauri backend. They MUST be tree-shaken out of a shipped bundle —
  // a user staring at a broken Tauri bridge should get a real error, never
  // synthesized "customer-database.xlsx" recovery results.
  //
  //   - dev (`npm run dev`, `npm run tauri dev`): always allowed.
  //   - prod (`npm run build`, `npm run tauri build`): stripped.
  //   - prod + `RECUPERE_ENABLE_BROWSER_PREVIEW=1`: kept (Playwright uses this
  //     so its `npm run build && npm run preview` webServer can still drive
  //     the fixture UI).
  //
  // The code in src/hooks/ipc/*.ts guards every fixture branch behind
  // `if (__ALLOW_BROWSER_PREVIEW__ && !isTauri())`, which Rollup turns into
  // a statically-false `if (false && ...)` in prod builds and then deletes.
  const allowBrowserPreview =
    mode !== "production" ||
    process.env.RECUPERE_ENABLE_BROWSER_PREVIEW === "1" ||
    process.env.RECUPERE_ENABLE_BROWSER_PREVIEW?.toLowerCase() === "true";

  return ({
  define: {
    __APP_VERSION__: JSON.stringify(process.env.npm_package_version ?? "0.1.0"),
    __ALLOW_BROWSER_PREVIEW__: JSON.stringify(Boolean(allowBrowserPreview)),
  },
  plugins: [react()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  build: {
    rollupOptions: {
      output: {
        // Keep the desktop shell leaner on first load by splitting the
        // large shared vendor bundle into stable, cacheable chunks.
        manualChunks: buildManualChunkName,
      },
    },
  },
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
  });
});
