# Market Benchmark and Differentiation Plan

## Purpose

This document keeps product positioning honest. It separates:

- what Récupère already implements in this repository
- what is partially exposed or not yet benchmarked
- where leading recovery tools still have an advantage
- which improvements can create real differentiation instead of vague marketing

Status date: `2026-04-23`

## Current Récupère Position

Récupère is already credible on these axes:

- read-only recovery posture
- guided novice UX with explicit safety warnings
- low-level Rust recovery engine with analyzers, carving, export, preview, reporting, and audit support
- strong automated validation inside the repository

Récupère is not yet proven on these axes:

- public benchmark superiority against accessible and reproducible comparators first, then commercial leaders as bonus evidence; the first accessible slice exists, but a full public-campaign Récupère run and the blocked APFS `P0` path still remain open
- bootable rescue workflows beyond the new Linux live-USB + `AppImage` MVP
- degraded-disk imaging comparable to specialist tools
- fully exposed RAID / NAS / VM / lab workflows in the desktop UX
- large-scale validation on real-world failure corpora

## Capability Snapshot

| Capability | Récupère now | R-Studio | DMDE | Stellar Toolkit | Notes |
|---|---|---|---|---|---|
| Source safety / read-only messaging | Strong | Mature | Mature | Mature | This is already one of Récupère's best product traits |
| Guided novice workflow | Strong | Moderate | Low | Moderate | Récupère can differentiate here if it keeps safety-first UX discipline |
| Core filesystem analyzers | Present | Mature | Mature | Mature | Récupère has real engine work, but not yet the same market proof level |
| Signature carving | Present | Mature | Mature | Mature | Needs benchmark data, not just feature presence |
| Reporting / audit trail | Present | Mature | Moderate | Moderate | Récupère should push signed, traceable reports further |
| RAID / advanced storage workflows | Partial | Mature | Mature | Mature | Backend pieces exist, but product exposure is incomplete |
| Disk imaging for unstable media | Basic to moderate | Mature | Mature | Moderate | This remains a major product gap |
| Bootable rescue environment | Partial | Present | Not core differentiator | Present | First MVP is now documented around Linux live USB + `AppImage`, but custom media and broader validation remain open |
| Public benchmark proof | Partial | Established reputation | Established reputation | Established reputation | First accessible comparator slice exists, but APFS `P0` and a full public-campaign Récupère run still block stronger claims |

## Official Competitor References Reviewed

The comparison above is anchored to official vendor material reviewed on `2026-04-09`, plus accessible-tool references rechecked on `2026-04-23`:

- R-Studio: `https://www.r-studio.com/data-recovery-software/`
- R-Studio extended recovery reference: `https://www.r-studio.com/Unformat_Help/extended_information_recovery.html`
- DMDE manual: `https://dmde.com/docs/dmde-3.2-manual.pdf`
- DMDE product site: `https://dmde.com/`
- TestDisk / PhotoRec download and documentation: `https://www.cgsecurity.org/wiki/TestDisk_Download`
- TestDisk script execution reference: `https://www.cgsecurity.org/wiki/Running_TestDisk_Commands_via_shell_script`
- DMDE free-recovery reference: `https://dmde.com/manual/datarecovery.html`
- Stellar Data Recovery Toolkit family: `https://www.stellarinfo.com/`

The repo-level top-tier status tracking now also lives in
[`benchmarks/scorecard-v1.md`](../benchmarks/scorecard-v1.md) and
[`benchmarks/scorecard-v1.json`](../benchmarks/scorecard-v1.json).

## Benchmark Protocol Récupère Needs

Récupère should not benchmark itself with vague success stories. The protocol should be repeatable.

The local evidence pipeline is:

```bash
npm run benchmark:check
npm run benchmark:report
```

`benchmark:check` validates the manifest and result JSON, then writes
`dist/benchmark-results-summary.json` and `dist/benchmark-results-summary.md`.
`benchmark:report` writes the browsable HTML report. These artefacts are
evidence summaries only; unsupported, blocked, and not-run scenarios must remain
visible.

### 1. Corpus

- healthy filesystem images with known ground truth
- deleted-file scenarios for FAT32, exFAT, NTFS, APFS, HFS+, EXT4
- fragmented-file scenarios
- partially corrupted metadata scenarios
- carved-only scenarios where metadata is unavailable
- compressed / ADS / resource fork / snapshot-derived edge cases where supported

### 2. Failure Classes

- accidental delete
- quick format / volume metadata loss
- damaged partition table
- raw carving only
- partial corruption
- unstable media read constraints

### 3. Metrics

- files found
- files correctly exportable
- intact export rate
- partial but usable export rate
- false positives
- analyst time to first safe action
- analyst time to first successful export
- whether the workflow preserved read-only guarantees throughout

### 4. Evidence

- image fixture identifier
- exact command / build version used
- exported file hashes where possible
- report artifact
- screenshots only as supporting material, never as primary proof

## Priority Gaps to Close

### P0: Trust and proof

- publish benchmark fixtures and protocol
- compare outcomes against at least `PhotoRec` and `TestDisk` for the mandatory public baseline; add `DMDE` free mode, `R-Studio`, `Stellar`, `Disk Drill`, or similar suites only as optional bonus evidence
- document unsupported or partially supported scenarios explicitly

### P1: Recovery operations

- stronger degraded-disk imaging with retries, resumability, and bad-sector visibility
- clearer RAID, encrypted-volume, and lab-only workflow boundaries
- fixture-backed end-to-end regression coverage for more recovery paths

### P2: Differentiation

- signed audit-ready reports
- best-in-class novice crisis UX
- strong “image first” safety discipline for risky media
- clearer expert instrumentation without making novice mode unsafe

## What Would Make Récupère Stand Out

The most realistic path to differentiation is not “more magic AI”. It is:

- safer first decisions than competitors
- clearer explanations under stress
- better traceability for every critical action
- honest confidence language instead of fake certainty
- measurable recovery outcomes on a public corpus

If Récupère becomes the tool that users trust first in a risky situation, that is a stronger moat than inflated marketing claims.
