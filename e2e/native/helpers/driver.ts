// ============================================================================
// Native driver helper — stack-aware browser bootstrap
// ============================================================================
//
// Chantier 83. Abstracts the difference between the two native stacks so that
// the seven `e2e/native/*.spec.ts` files do not have to know which one is
// running:
//
//   - tauri-driver (Linux + Windows): `browser` is already attached to the
//     webview, no context switch needed.
//   - Appium Mac2 (macOS): `browser` starts in NATIVE context. Specs must
//     switch into the WKWebView context (named `WEBVIEW_<bundleId>`) to
//     interact with the React DOM.
//
// All native specs MUST call `attachToWebview(browser)` in a `before` hook
// before issuing any DOM-level command. The helper is idempotent.
//
// ============================================================================

const RECUPERE_BUNDLE_ID = 'com.recupere.desktop';
const WEBVIEW_CONTEXT_TIMEOUT_MS = 30_000;
const WEBVIEW_CONTEXT_POLL_MS = 500;

export async function attachToWebview(browser: WebdriverIO.Browser): Promise<void> {
  if (process.platform !== 'darwin') {
    return;
  }

  const deadline = Date.now() + WEBVIEW_CONTEXT_TIMEOUT_MS;
  let lastContexts: string[] = [];

  while (Date.now() < deadline) {
    const contexts = (await browser.getContexts()) as string[];
    lastContexts = contexts;
    const webview = contexts.find(
      (ctx) =>
        ctx.startsWith('WEBVIEW_') &&
        (ctx.includes(RECUPERE_BUNDLE_ID) || ctx === 'WEBVIEW_recupere'),
    );
    if (webview) {
      await browser.switchContext(webview);
      return;
    }
    await sleep(WEBVIEW_CONTEXT_POLL_MS);
  }

  throw new Error(
    `Timed out after ${WEBVIEW_CONTEXT_TIMEOUT_MS}ms waiting for the Récupère WKWebView context. Last seen contexts: ${JSON.stringify(lastContexts)}. Confirm the debug binary is built with inspectable=true (default in Tauri 2 debug).`,
  );
}

interface NativeError {
  __wdio_native_error: string;
}

function isNativeError(value: unknown): value is NativeError {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as NativeError).__wdio_native_error === 'string'
  );
}

export async function invokeTauriCommand<TResult>(
  browser: WebdriverIO.Browser,
  command: string,
  args: Record<string, unknown> = {},
): Promise<TResult> {
  const result = await browser.execute(
    async (cmd: string, payload: Record<string, unknown>) => {
      const tauri = (window as unknown as { __TAURI__?: { core?: { invoke?: unknown } } })
        .__TAURI__;
      const invoke = tauri?.core?.invoke;
      if (typeof invoke !== 'function') {
        return {
          __wdio_native_error: 'window.__TAURI__.core.invoke is not available in this context',
        };
      }
      try {
        return (await (invoke as (c: string, p: unknown) => Promise<unknown>)(
          cmd,
          payload,
        )) as unknown;
      } catch (err) {
        return { __wdio_native_error: String(err) };
      }
    },
    command,
    args,
  );

  if (isNativeError(result)) {
    throw new Error(`Tauri invoke('${command}') failed: ${result.__wdio_native_error}`);
  }
  return result as TResult;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
