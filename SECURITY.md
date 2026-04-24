# Security Policy

## Reporting a vulnerability

If you discover a security issue in Récupère, **do not** open a public GitHub
issue. Instead, email the maintainers directly so the report can be
triaged privately and a fix shipped before disclosure.

## Threat model summary

Récupère is a desktop data-recovery tool that operates **strictly read-only**
on the source disk. The threat model centres on three properties:

1. **Source integrity** — the application must never write to the source
   media. This is enforced by `core::preferred_imaging_source_path()` which
   promotes mounted partitions to raw `/dev/r…` whole-device handles, by the
   destination canonicalisation in
   [`commands::validate_export_destination`](src-tauri/src/commands/mod.rs),
   and by the read-only file open mode used everywhere in [imaging/](src-tauri/src/imaging/) and the analyzers.

2. **No implicit data exfiltration** — the AI features run a local Gemma
   model via Ollama (see [`cloud_ai/mod.rs`](src-tauri/src/cloud_ai/mod.rs)).
   No third-party cloud AI providers, no API keys baked in, no telemetry.
   The `recupere-agent` remote mode (see
   [Remote agents threat model](#remote-agents-threat-model)) is **opt-in**:
   no network traffic leaves the desktop until the user explicitly registers
   an agent URL + bearer token via the Devices page.

3. **License authenticity** — license keys are Ed25519-signed and verified
   offline against a public key embedded at compile time. The development
   placeholder key is rejected by `build.rs` for `--release` builds — the
   real key must be supplied via `RECUPERE_LICENSE_PUBKEY_HEX`.

## Remote agents threat model

Récupère can optionally pilot a `recupere-agent` binary running on a separate
machine (see [src-tauri/src/remote/](src-tauri/src/remote/)). This is a
**power-user feature** — it is not used by any default workflow and no traffic
leaves the desktop unless the user actively registers an agent.

### Assets
- Scan metadata (device list, filesystem layout, recovered file paths) travels
  between desktop and agent over HTTP.
- The bearer token is the **only** authentication boundary — treat it as a
  password.
- File bytes transit the same channel when the user asks the desktop to pull a
  recovered file back (`remote_pull_recovered_file`).

### Enforced mitigations
- **TLS mandatory for non-loopback.** `validate_remote_base_url`
  (src-tauri/src/remote/commands.rs) refuses any plain-`http://` base URL
  whose host is not `127.0.0.1`, `::1`, or `localhost`. Public or LAN
  deployments must use `https://`. Tests:
  [`commands::tests::rejects_plain_http_for_non_loopback`](src-tauri/src/remote/commands.rs).
- **Tokens never touch disk in cleartext.** Bearer tokens go to the OS keyring
  (`keyring::Entry::new("recupere-agent", agent_id)`); only id/label/base_url
  are persisted to `<config_dir>/recupere/remote_agents.json`.
- **CSP keeps the webview out of the loop.** `connect-src` in
  [src-tauri/tauri.conf.json](src-tauri/tauri.conf.json) only allows Tauri IPC
  + asset protocol. All remote traffic flows through Rust (`reqwest::blocking`)
  invoked via IPC, so a hostile script injected into the webview cannot talk
  to a remote agent on its own.
- **`form-action 'none'`** in CSP prevents any HTML form from submitting to an
  external URL — defense-in-depth against injection attacks that try to
  exfiltrate state via a `<form>` POST.

### Residual risks
- The desktop trusts the agent's TLS certificate through the system trust
  store. Users registering `https://` URLs with self-signed certs should pin
  the cert at the OS level (not exposed in the UI today — tracked for a later
  release).
- A compromised agent host can feed malicious file bytes back to the desktop.
  The preview pipeline enforces byte limits and safe decoders (see
  `preview/mod.rs` size/dimension caps), but final file bytes written by the
  user via `remote_pull_recovered_file` are the user's responsibility to
  verify. The recovered file is written to the path provided by a native
  save-dialog — there is no filesystem escape vector from the agent itself.
- The loopback exception is deliberate: it supports the recommended deployment
  pattern (`ssh -L 7878:localhost:7878 user@server`) where SSH already
  provides encryption. Users who run the agent on the **same** machine without
  a tunnel are exposed to local-process snooping — document that the agent is
  intended for remote deployment, not same-host use.

### User-facing warning
The Devices > Remote agents UI shows a red banner whenever the `Base URL`
field is not `https://` and not a loopback address. The user must tick an
"I understand the risk" checkbox before the form submits. See
[src/components/device/RemoteAgentsSection.tsx](src/components/device/RemoteAgentsSection.tsx).

## Build hardening

| Mitigation | Where |
|---|---|
| Strict CSP | [src-tauri/tauri.conf.json](src-tauri/tauri.conf.json) `app.security.csp` |
| Tauri capabilities (no shell, no fs) | [src-tauri/capabilities/default.json](src-tauri/capabilities/default.json) |
| Hardened runtime + entitlements (macOS) | [src-tauri/tauri.conf.json](src-tauri/tauri.conf.json) `bundle.macOS` |
| `perMachine` install (Windows) | [src-tauri/tauri.conf.json](src-tauri/tauri.conf.json) `bundle.windows.nsis` |
| Atomic write + rename for audit trail | [src-tauri/src/audit/mod.rs](src-tauri/src/audit/mod.rs) |
| Symlink-resistant export validation | [src-tauri/src/commands/mod.rs](src-tauri/src/commands/mod.rs) `validate_export_destination` |
| Image preview DoS guards (50 MB + 16k×16k) | [src-tauri/src/preview/mod.rs](src-tauri/src/preview/mod.rs) `decode_image_with_limits` |
| Structured logging via `tracing` | [src-tauri/src/lib.rs](src-tauri/src/lib.rs) `init_tracing` |
| `cargo audit` + `cargo deny` in CI | [.github/workflows/ci.yml](.github/workflows/ci.yml) `security-audit` job |
| `cargo deny` license/source policy | [src-tauri/deny.toml](src-tauri/deny.toml) |

## Known supply-chain risks

Two Rust dependencies are central to the recovery engine but have been
**unmaintained** since 2021:

| Crate | Version | Last update | Why we keep it |
|---|---|---|---|
| `apfs` | 0.2.3 | 2021-09 | The only pure-Rust APFS parser available; reimplementing it from scratch is several weeks of work. |
| `lznt1` | 0.1.3 | 2020-11 | Required for NTFS LZNT1 decompression. No maintained alternative on crates.io. |

### Mitigation strategy

1. **Continuous monitoring** — `cargo audit` runs on every CI build (see
   [.github/workflows/ci.yml](.github/workflows/ci.yml) `security-audit`
   job). Any new RustSec advisory affecting these crates will fail the
   build immediately.
2. **Sandboxing** — both crates only ever process bytes coming from the
   read-only source disk. They never touch user input, network data, or
   the destination volume. A worst-case parser bug yields a panic, not RCE
   to an attacker.
3. **No CVE today** — as of the latest audit run, neither crate has any
   open RustSec advisory.

### Escalation plan

If a CVE is published against either crate:

1. **Pin the safe version** in [src-tauri/Cargo.toml](src-tauri/Cargo.toml)
   if a workaround exists, even if it requires forking.
2. **Fork into `vendor/`** under our maintenance, applying the upstream
   patch + any backported security fix.
3. **Document the fork** in this file with the CVE ID, the affected
   versions, and the date of the fix.

### Why we chose Option C ("accept + monitor")

The audit explicitly weighed three options:

| Option | Pros | Cons |
|---|---|---|
| A. Fork now | Full control | ~2-3 days to set up + ongoing maintenance burden with no concrete benefit |
| B. Rewrite parsers | Clean | Several weeks of expert work; no demand from any CVE today |
| **C. Accept + monitor** ← chosen | Zero immediate cost | Reactive: a CVE forces a sprint |

Option C is the right call **as long as `cargo audit` stays green**. Once a
CVE appears, this document and the CI job both surface the issue
immediately and the team can switch to Option A in a single day.

## Code-signing release builds

Both `signingIdentity` (macOS) and `certificateThumbprint` (Windows) in
[src-tauri/tauri.conf.json](src-tauri/tauri.conf.json) are intentionally
left at `null` in source control. Public release builds **must** populate
them locally before running `tauri build` — unsigned bundles will be
rejected by Gatekeeper / SmartScreen.

### macOS

Find your Developer ID Application identity:

```bash
security find-identity -v -p codesigning
```

Look for a line like
`1) ABCDEF1234567890ABCDEF1234567890ABCDEF12 "Developer ID Application: Your Name (TEAMID)"`.
Set the full quoted string (not the SHA1) as `bundle.macOS.signingIdentity`.

You also need `providerShortName` set to your Apple Team ID for notarization.
Notarize after building with `xcrun notarytool submit … --wait`.

### Windows

List installed code-signing certificates:

```powershell
certutil -store My
```

Copy the **Cert Hash(sha1)** value (40 hex chars, no spaces) into
`bundle.windows.certificateThumbprint`. The cert must be EV or OV from a
trusted CA — self-signed certs trigger SmartScreen warnings even when
correctly configured.

### CI

Never commit real signing values. Inject them at build time via the
`TAURI_SIGNING_IDENTITY` / `TAURI_PRIVATE_KEY` env vars and a GitHub Actions
secret (see `.github/workflows/release.yml` if/when one is added). The
`security-audit` job already runs on every push, so a credential leak in
this file would be flagged immediately.

## Crates marked unmaintained but still safe

`mac_address 1.x` is also pinned at an older release. It is a small,
pure-Rust crate used only to read the primary network interface MAC for the
license fingerprint computation. It has no network or filesystem reach and
is not a security concern.
