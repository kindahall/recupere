# Récupère — Desktop Data Recovery, Read-Only by Design

Récupère is a desktop data recovery application built with Tauri 2, React, TypeScript, and Rust. It focuses on safe read-only analysis, guided recovery decisions, local preview/export workflows, and auditable reporting.

## Current Product Status

As of April 9, 2026, this repository validates:

- production web builds
- UI unit coverage
- Playwright browser-preview coverage for onboarding, devices, paywall, and a seeded `Results -> Export` flow
- Rust tests covering analyzers, carving, export, reporting, licensing, RAID, repair, and supporting recovery logic

Récupère is already a substantial recovery workbench. It is not yet presented here as a proven replacement for the most mature commercial recovery suites, because that requires reproducible public benchmarks and broader real-world validation.

The benchmark pipeline — corpus manifest, scenario protocol, evidence levels, and an HTML rollup — is defined under [`benchmarks/`](benchmarks/):

- Corpus manifest: [`benchmarks/corpus/v1/manifest.json`](benchmarks/corpus/v1/manifest.json)
- Protocol & evidence levels: [`benchmarks/protocol-v1.md`](benchmarks/protocol-v1.md)
- Public comparative campaign runbook: [`benchmarks/public-comparative-campaign-v1.md`](benchmarks/public-comparative-campaign-v1.md)
- Top-tier scorecard: [`benchmarks/scorecard-v1.md`](benchmarks/scorecard-v1.md)
- Hard-case coverage matrix: [`docs/hard-case-matrix.md`](docs/hard-case-matrix.md)
- HTML report generator: `npm run benchmark:report` → `dist/benchmarks-report.html` (CI publishes it as a build artefact).
- JSON/Markdown evidence summary: `npm run benchmark:results` → `dist/benchmark-results-summary.json` and `dist/benchmark-results-summary.md`.

The internal baseline at [`benchmarks/results/2026-04-12-recupere-internal-baseline.json`](benchmarks/results/2026-04-12-recupere-internal-baseline.json) is an *asserted-minimum* run backed by the Rust test suite, not a public competitive benchmark. A first non-paid-accessible comparator slice is now checked in with [`2026-04-23-photorec-7.2-accessible-p0.json`](benchmarks/results/2026-04-23-photorec-7.2-accessible-p0.json) and [`2026-04-23-testdisk-7.2-accessible-p0.json`](benchmarks/results/2026-04-23-testdisk-7.2-accessible-p0.json). The APFS `P0` blocker also has a fresh same-day note and targeted result in [`2026-04-23-recupere-apfs-p0-blocker-note.md`](benchmarks/results/2026-04-23-recupere-apfs-p0-blocker-note.md) and [`2026-04-23-recupere-apfs-regression.json`](benchmarks/results/2026-04-23-recupere-apfs-regression.json). Contributions adding more runs with the same scenario ids remain welcome — file names follow `YYYY-MM-DD-<tool>-<build-ref>.json`.

## What It Does Today

- **Read-Only First** — Source disks are analyzed without writing back to them
- **Guided Diagnostics** — Loss classification, recoverability estimates, risk surfacing, and recommended next actions
- **Disk Imaging** — Bit-for-bit imaging workflows for safer follow-up analysis
- **Filesystem Analysis** — Recovery-oriented analyzers for FAT32, exFAT, NTFS, APFS, HFS+, and EXT4
- **File Carving** — Signature-based recovery when metadata is missing or incomplete
- **Preview & Export** — Local preview flows, export validation, reporting, CSV export, and support bundles
- **Novice & Expert Modes** — Guided UX for safer first actions and expert views for deeper inspection
- **Internationalization** — English by default, French included
- **Bootable Rescue MVP** — A first documented rescue posture exists for Linux live USB + packaged `AppImage`; see [`docs/bootable-rescue-workflow.md`](docs/bootable-rescue-workflow.md)

## What It Does Not Claim

- It does **not** recreate physically destroyed data
- It does **not** write recovered files or images back to the source disk
- It does **not** claim to be better than the market without benchmark evidence
- It does **not** imply that every advanced workflow is already at parity with specialist tools for lab, RAID, NAS, or boot-rescue cases

## Filesystem Coverage

- Implemented engine paths: FAT32, exFAT, NTFS, APFS, HFS+, EXT4
- Raw signature carving is available when filesystem metadata is unavailable
- Workflow depth still varies by scenario; some engine capabilities are ahead of the current UI exposure

## Market Position

Récupère aims to differentiate on safety, clarity, traceability, and guided recovery UX. The current benchmark and gap analysis lives in [docs/benchmark-market.md](docs/benchmark-market.md).

## Architecture

- **Frontend**: React + TypeScript (Vite)
- **Backend**: Rust (Tauri 2)
- **Communication**: Tauri IPC (commands + events)
- **State**: Zustand
- **Routing**: React Router
- **i18n**: react-i18next

## Getting Started

```bash
# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Validate local release readiness
npm run release:preflight

# Build for production
npm run release:build

# Or build directly when preflight was already run
npm run tauri build
```

## Running E2E tests

Récupère ships with **two complementary E2E harnesses**. They cohabit; neither replaces the other.

### 1. Browser-preview harness (Playwright)

Runs the React UI against `vite preview` with the `__ALLOW_BROWSER_PREVIEW__` build flag on and all Tauri IPC mocked (`seedBrowserPreviewState()` in `e2e/helpers.ts`). Fast, deterministic, no native binary required — but it does **not** exercise the real Tauri runtime, so native-only regressions (IPC shape changes, privileged imaging, filesystem scanners) are invisible to it.

```bash
npm run test:e2e
```

Configuration: [playwright.config.ts](playwright.config.ts). Specs: [e2e/*.spec.ts](e2e/) (excluding `e2e/native/`).

### 2. Native harness (cross-platform: tauri-driver + Appium Mac2)

Launches the real Récupère debug binary, drives the actual webview window with WebdriverIO, and asserts against synthetic fixtures generated by the `gen_synth_fixture` Cargo example. Slower than browser-preview, runs serially, but exercises the real Tauri runtime — including IPC shape, privileged imaging paths, and filesystem scanners.

Récupère ships on Linux, Windows and macOS, so the native harness covers all three. Apple does not allow external WebDriver to attach to a WKWebView, so the harness uses **two complementary stacks**:

| Platform | WebView | Stack |
|---|---|---|
| Linux | webkit2gtk | `tauri-driver` 2.0.5 + webkit2gtk-driver |
| Windows | WebView2 | `tauri-driver` 2.0.5 + msedgedriver |
| macOS | WKWebView | External Appium 3.x + `appium-mac2-driver` 3.x + WKWebView context switching |

Prerequisites (one-time):

```bash
# Required everywhere — build the Récupère debug binary the harness attaches to.
cargo build --manifest-path src-tauri/Cargo.toml

# Linux + Windows only — install tauri-driver as a global Rust binary.
cargo install tauri-driver --version 2.0.5

# macOS only — install Appium/Mac2 outside this repo and run the server.
# Appium is intentionally not a repo devDependency so `npm audit` stays clean.
npm install -g appium@3
appium driver install mac2
appium --address 127.0.0.1 --port 4723
```

Run the suite (the npm script auto-detects your OS):

```bash
npm run test:e2e:native
```

Or pin a stack explicitly:

```bash
npm run test:e2e:native:linux     # tauri-driver + webkit2gtk
npm run test:e2e:native:windows   # tauri-driver + msedgedriver
npm run test:e2e:native:macos     # Appium Mac2 + WKWebView
```

Configurations: [wdio.tauri-driver.conf.ts](wdio.tauri-driver.conf.ts) (Linux + Windows), [wdio.appium.conf.ts](wdio.appium.conf.ts) (macOS), [tsconfig.wdio.json](tsconfig.wdio.json). Specs: [e2e/native/*.spec.ts](e2e/native/).

#### Native macOS — dev requirements

The macOS native harness uses Appium Mac2 driver, which itself depends on **XCTest**. To run or develop the macOS native suite locally, the following are **required prerequisites**, not optional extras:

1. **Full Xcode** installed from the App Store (~15 GB). Xcode Command Line Tools alone are insufficient — Mac2 needs the XCTest runtime that only ships with the full Xcode bundle.
2. **External Appium 3 + Mac2 driver** installed outside the repository and already listening on `127.0.0.1:4723`. This keeps the default dependency tree free of the current Appium Mac2 transitive audit advisories.
3. **A debug app bundle** (`.app`) for Récupère, produced with `npm run tauri -- build --debug`. The raw binary at `src-tauri/target/debug/recupere` is not a `.app` and Mac2 will not attach to it.
4. **First-run Accessibility grant**: the first time the harness attaches Récupère, macOS prompts for Accessibility permission for `node`. Grant it under *System Settings → Privacy & Security → Accessibility* — once per machine.

These mirror the standard requirements for any macOS UI automation against XCTest. Contributors who do not work on the native macOS harness do not need them; the rest of the project (Rust backend, React frontend, Linux/Windows native E2E) builds and tests fine without Xcode.

#### CI

The native E2E suite runs in a **dedicated workflow** ([.github/workflows/e2e-native.yml](.github/workflows/e2e-native.yml)), separate from the main `ci.yml`. It triggers on:

- a daily cron schedule (03:00 UTC) on `main`,
- manual `workflow_dispatch`,
- pushes to release tags (`v*`).

It does **not** run on every pull request — keeping the main `ci.yml` fast and signal-rich. Once the native suite is observed green for two consecutive scheduled runs per platform, the `pull_request:` trigger will be added so it becomes blocking on PRs (tracked in Chantier 83).

## Release Preflight

The repository includes a local release preflight to catch packaging issues before a production bundle is built.

```bash
npm run release:preflight
npm run release:preflight:bundle
RECUPERE_RELEASE=1 npm run release:preflight
```

What it checks:

- version alignment across `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml`
- Tauri bundle identifier and product name presence
- referenced bundle icons exist
- Rust package metadata no longer uses placeholder values
- release scripts are exposed through `package.json`
- production signing and secrets runbook exists
- `npm audit` reports no known dependency vulnerabilities

What it does not pretend to complete locally:

- macOS signing and notarization
- privileged helper signing
- update feed signing
- CI/CD publication workflow

The command writes a machine-readable report to `dist/release-preflight.json`. `release:preflight` is intentionally usable on developer machines and reports missing external signing inputs as warnings. `release:preflight:bundle` makes the license public key mandatory. `RECUPERE_RELEASE=1 npm run release:preflight` turns signing, notarization, updater, and license readiness into hard release blockers, matching the `v*` tag path in CI.

## Release Manifest

After a successful macOS bundle build, the repository can also generate a release manifest and SHA-256 checksum file:

```bash
npm run release:manifest
```

This writes:

- `dist/release/release-manifest-<version>.json`
- `dist/release/release-checksums.txt`
- `dist/release/release-notes.md`

## CI / Release Automation

The repository now includes GitHub Actions workflows for the desktop pipeline:

- `.github/workflows/ci.yml`
  Runs the full macOS verification pipeline with `release:preflight`, full tests, `cargo check`, `build`, and Playwright smoke coverage, plus lightweight Windows/Linux verification for `test:ui`, `cargo check`, and `build`.
- `.github/workflows/release-macos.yml`
  Builds the macOS `.app` and `.dmg` bundles on manual dispatch or `v*` tags, generates release metadata, uploads workflow artifacts, and publishes a GitHub Release on version tags. If Apple signing/notarization secrets are configured, they are forwarded to the build pipeline automatically.

The expected external signing/notarization inputs are documented in `.github/RELEASE_SECRETS.md`.

## Project Structure

```
recupere/
├── src/                  # React frontend
│   ├── components/       # Reusable UI components
│   ├── pages/            # Page components (9 screens)
│   ├── stores/           # Zustand state stores
│   ├── hooks/            # Custom React hooks
│   ├── types/            # Shared TypeScript types
│   ├── i18n/             # Internationalization
│   └── index.css         # Design system tokens
├── src-tauri/            # Rust backend
│   └── src/
│       ├── commands/     # IPC command handlers
│       ├── core/         # Low-level I/O, hardware detection
│       ├── analyzers/    # Filesystem parsers
│       ├── carving/      # Signature-based recovery
│       ├── scoring/      # Recoverability scoring
│       ├── ai/           # AI service layer
│       ├── preview/      # File preview engine
│       ├── export/       # Secure export engine
│       ├── audit/        # Audit trail logging
│       └── types/        # Shared Rust types
├── AGENTS.md             # AI agent governance rules
└── PLANS.md              # Execution plans
```

## Safety Guarantees

- All disk access is strictly read-only (`O_RDONLY`)
- Export destination is validated to never be on the source disk
- Reporting uses the real app identity and exact timestamps
- Critical operations are designed to stay traceable through logs and reports
- Risky situations are surfaced explicitly to the user

## License

Proprietary — All rights reserved.

## Support

If this project is useful to you, you can support its development with a free and entirely optional tip through the repository's **Sponsor** button. Thank you for your support.
