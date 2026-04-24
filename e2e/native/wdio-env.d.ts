// Type augmentation: vendor capabilities used by the two native harnesses
// (tauri-driver on Linux/Windows + Appium Mac2 on macOS) are not known by
// `@wdio/types`. Declaring them here keeps wdio.*.conf.ts files and all
// native specs strictly typed without resorting to casts.

declare global {
  namespace WebdriverIO {
    interface Capabilities {
      // tauri-driver capability — Linux + Windows runs.
      'tauri:options'?: {
        application: string;
      };
      // Appium Mac2 driver capabilities — macOS runs.
      'appium:automationName'?: string;
      'appium:bundleId'?: string;
      'appium:app'?: string;
      'appium:arguments'?: string[];
      'appium:noReset'?: boolean;
      'appium:showServerLogs'?: boolean;
    }
    interface Browser {
      // Appium-only commands surfaced when `automationName: mac2`.
      // WebdriverIO v9 doesn't ship these in the base typings because they
      // are not part of W3C WebDriver. Declared here so specs and helpers
      // can call them without casting.
      getContexts(): Promise<string[]>;
      switchContext(name: string): Promise<void>;
    }
  }
}

export {};
