# Release Secrets & Signing Runbook

This document is the canonical walk-through for turning a fresh checkout of
Récupère into a signed, notarized, auto-updatable distribution. Every step
below is reproducible. No secret is ever committed to the repo — the runbook
only prescribes _where_ each secret must live (CI secret store, Keychain,
disk path) and how to verify that it's wired correctly.

## At a glance

| Pipeline stage | Required secrets | Guard that enforces it |
| --- | --- | --- |
| License verification  | `RECUPERE_LICENSE_PUBKEY_HEX` (build-time) | `src-tauri/build.rs` panic + `license::ensure_production_public_key()` runtime check |
| macOS code signing    | `APPLE_SIGNING_IDENTITY`, `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD` | `scripts/release-preflight.mjs` strict release gate |
| macOS notarization    | `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH` _or_ `APPLE_API_PRIVATE_KEY` | same |
| Tauri updater signing | `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` + updater pubkey embedded in `tauri.conf.json` | same + `updater-pubkey` / `updater-active-for-release` checks |

## 1. License public key

The Ed25519 public key that validates Récupère Pro license keys is baked into
the binary at compile time. `src-tauri/build.rs` refuses to produce a
`cargo build --release` without `RECUPERE_LICENSE_PUBKEY_HEX`, and
`license::ensure_production_public_key()` refuses to start the app at runtime
if the dev placeholder is still embedded (defense in depth).

Generate the keypair once, then keep the private key on the license server
only:

```bash
# Offline machine, one time:
openssl genpkey -algorithm Ed25519 -out license_private.pem
openssl pkey -in license_private.pem -pubout -out license_public.pem
# Convert the raw 32-byte public key to lowercase hex (no 0x prefix, 64 chars).
```

In CI, prefer a repository variable because this is a public key and the
release-smoke job on pull requests should not depend on secret availability.
A secret fallback also works:

```yaml
env:
  RECUPERE_LICENSE_PUBKEY_HEX: ${{ vars.RECUPERE_LICENSE_PUBKEY_HEX || secrets.RECUPERE_LICENSE_PUBKEY_HEX }}
```

## 2. macOS code signing

Required for unsigned `.app` / `.dmg` to launch without SIP warnings.

### Certificates

```bash
# Export your Developer ID Application certificate from Keychain as a .p12,
# then base64-encode it for GitHub Actions:
base64 -i DeveloperID.p12 -o DeveloperID.p12.base64
```

| Secret | Source |
| --- | --- |
| `APPLE_SIGNING_IDENTITY` | Exact name of the identity, e.g. `"Developer ID Application: Récupère (TEAMID)"` |
| `APPLE_CERTIFICATE` | Contents of `DeveloperID.p12.base64` |
| `APPLE_CERTIFICATE_PASSWORD` | The password you set when exporting the .p12 |

### Notarization (App Store Connect API key)

1. In App Store Connect → Users & Access → Keys → App Store Connect API,
   generate a new API key with the `Developer` role.
2. Download the `.p8` private key.

| Secret | Source |
| --- | --- |
| `APPLE_API_ISSUER` | Issuer ID shown in App Store Connect |
| `APPLE_API_KEY` | Key ID (e.g. `ABCDE12345`) |
| `APPLE_API_PRIVATE_KEY` | **Contents** of the `.p8` file (PEM) |
| `APPLE_API_KEY_PATH` | _(alternative)_ path on runner when the key is pre-materialized |

`.github/workflows/release-macos.yml` materializes `APPLE_API_PRIVATE_KEY`
into `.release-secrets/AuthKey.p8` at runtime, which Tauri picks up.

## 3. Tauri updater keypair

The in-app updater verifies update manifests signed with an Ed25519 key.
The **public** key ships inside the binary (paste it into
`src-tauri/tauri.conf.json` → `plugins.updater.pubkey`); the **private**
key stays in CI secrets.

Generate it once with the helper we ship:

```bash
npm run release:updater-keygen -- --password 'a-long-unguessable-passphrase'
```

Output:

- `./updater.key`      → private key, **do NOT commit** (gitignored)
- `./updater.key.pub`  → public key, paste into `tauri.conf.json`

Then:

1. Open `src-tauri/tauri.conf.json` and set:

   ```json
   "plugins": {
     "updater": {
       "active": true,
       "endpoints": [
         "https://recupere.app/updates/{{target}}/{{arch}}/{{current_version}}"
       ],
       "dialog": true,
       "pubkey": "<contents of updater.key.pub>"
     }
   }
   ```

2. Register the CI secrets:

| Secret | Source |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | **Contents** of `updater.key` (the base64 blob) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The passphrase you used at keygen |

3. (Optional) Host the update manifest endpoint. The Tauri updater expects
   the endpoint to return a JSON manifest per target. See
   <https://v2.tauri.app/plugin/updater/> for the schema.

## 4. Strict preflight

Once the secrets above are in place, verify locally with the strict gate:

```bash
RECUPERE_RELEASE=1 npm run release:preflight
```

The `strictRelease` path in `scripts/release-preflight.mjs` elevates every
signing / notarization / updater check from `warning` to `error`. Green here
means a tag build on CI will also go green on the same checks.

## 5. Publishing

Push a `v*` tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

`release-macos.yml` (and `release-cross-platform.yml`) pick it up, build,
sign, notarize, staple, and publish the GitHub Release with the bundle,
signed updater manifest, and checksum file.

## What is already automated

- local `npm run release:preflight` (with a strict `RECUPERE_RELEASE=1` mode)
- local `npm run release:build`
- local `npm run release:manifest`
- local `npm run release:updater-keygen`
- GitHub Actions CI verification on macOS
- GitHub Actions lightweight verification on Windows and Linux
- GitHub Actions macOS bundle build with uploaded `.app`, `.dmg`, manifest, and checksums
- GitHub Release publication on `v*` tags with attached release assets
- macOS signing / notarization inputs forwarded through the release workflow when secrets are configured

## What is still external

- Apple Developer Program membership & certificate issuance.
- Notarization account setup (App Store Connect admin access).
- Hosting the updater manifest endpoint on your own infrastructure.
- Changelog policy beyond `scripts/generate-release-manifest.mjs` output.

## Red flags

- Never log `TAURI_SIGNING_PRIVATE_KEY` or `APPLE_API_PRIVATE_KEY`. CI steps
  that consume them MUST use `::add-mask::` or write to a file under
  `$RUNNER_TEMP`.
- Never commit `updater.key`, `updater.key.pub`, or the `.release-secrets/`
  folder — `.gitignore` blocks the obvious names but double-check before
  every push.
- If an updater key is ever leaked, rotate it. A new pubkey in
  `tauri.conf.json` invalidates all existing manifests and forces a fresh
  release; there is no recall of already-signed updates.
