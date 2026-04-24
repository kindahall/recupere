# Testing layers

Récupère keeps three test tiers with distinct guarantees, running
surfaces, and CI expectations. Knowing which tier to reach for is part of
our review checklist.

## Tier 1 — Unit tests (hermetic)

- **Scope**: pure functions, parser helpers, small state transitions.
- **Surface**: in-process, no real filesystem beyond `env::temp_dir()`,
  no network, no Tauri runtime.
- **Where**: `#[cfg(test)] mod tests` inside each module (e.g.
  [`src-tauri/src/preview/mod.rs`](../src-tauri/src/preview/mod.rs),
  [`src-tauri/src/audit/mod.rs`](../src-tauri/src/audit/mod.rs)), plus
  Vitest specs under [`src/`](../src/).
- **Run**: `cargo test --manifest-path src-tauri/Cargo.toml --lib` /
  `npm run test:ui`.
- **Must-haves**: deterministic, no sleeps > 50 ms, no real disks, no
  Ollama / network calls.

## Tier 2 — Integration tests (still hermetic, but cross-module)

- **Scope**: end-to-end flows that touch several modules but still run
  under the same Rust or TypeScript process.
- **Surface**: synthetic filesystem images, scan-session registries,
  scoring pipelines, audit-trail hash chain verification.
- **Where**: the same `#[cfg(test)] mod tests` blocks when the flow is
  cheap; larger fixtures live in `tests/` directories alongside modules.
- **Run**: same as Tier 1 (included in `cargo test --lib`).

## Tier 3 — Host-dependent tests

Some recovery paths can only be exercised against a real kernel driver.
These tests are gated at compile time so they never run on the wrong OS
— see [`src-tauri/src/analyzers/apfs.rs:596+`](../src-tauri/src/analyzers/apfs.rs)
where `create_raw_apfs_image_with_deleted_files_for_tests` is guarded by
`#[cfg(target_os = "macos")]`.

- **APFS**: creates a real APFS disk image via `hdiutil` and
  `diskutil`. Only compiles on macOS. Tests tolerate `hdiutil` failures
  on machines without `com.apple.DiskManagement` entitlements by
  returning early.
- **startup_guard** ([`src-tauri/src/startup_guard.rs`](../src-tauri/src/startup_guard.rs)):
  hermetic despite looking infra-shaped — the "listening local port"
  test opens a `TcpListener` on `127.0.0.1:0` which is available on all
  three supported OSes.

CI runs Tier 3 via the macOS runner (see
[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) → `verify-macos`).
Linux and Windows runners compile the code but don't execute the APFS
fixtures because the `cfg(target_os)` guard hides them.

## Tier 4 — Browser-preview E2E (Playwright)

- **Scope**: React UI flows, router guards, accessibility smoke tests.
- **Surface**: `npm run preview` bundle (built with
  `RECUPERE_ENABLE_BROWSER_PREVIEW=1`) plus mocked IPC via
  `seedBrowserPreviewState()` in
  [`e2e/helpers.ts`](../e2e/helpers.ts).
- **Run**: `npm run test:e2e`.
- **Does NOT test**: the real Tauri IPC layer, native file dialogs,
  native packaging. See chantier C4 in
  [`docs/top-tier-roadmap.md`](./top-tier-roadmap.md) and
  [`playwright.config.ts`](../playwright.config.ts) for why.

## Tier 5 — Native bundle smoke

- **Scope**: launches the packaged binary once and verifies it reaches
  steady state without a startup-guard fatal.
- **Surface**: `src-tauri/target/release/bundle/<os>/…` (macOS runner
  today; Linux + Windows are tracked by chantier C4).
- **Run**: `node scripts/native-smoke.mjs --timeout-ms 15000`.
- **Does NOT test**: UI interactions. Full WebDriver coverage via
  `tauri-driver` is the next C4 deliverable.

---

## Rules of thumb

- A bug caught at Tier 1 or 2 is worth 10 caught at Tier 4.
- Avoid adding tests to Tier 3 unless the behaviour genuinely depends
  on the kernel (APFS mount, `diskutil`, `DeviceIoControl`). Look for
  an in-process fixture first.
- Do not mark a tier-3 test `#[ignore]` — prefer a `cfg(target_os)`
  guard so tests that can't run on the host simply disappear from the
  suite instead of lingering as silent debt.
- When fixing a bug, the added regression test lives at the lowest tier
  that can express the failure. If only a native bundle smoke can
  reproduce it, say so explicitly in the PR so we can invest in
  tier-2 coverage.
