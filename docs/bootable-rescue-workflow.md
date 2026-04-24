# Bootable rescue workflow MVP

Status: `partial`  
Status date: `2026-04-23`

## Purpose

Define the first credible bootable rescue workflow for Récupère without
pretending that the repo already ships a universal recovery ISO.

This MVP is intentionally narrow:

- boot from a trusted Linux live USB;
- run the packaged Linux `AppImage`;
- keep the source strictly read-only;
- export only to a separate writable destination;
- preserve support-bundle and audit handoff.

## Supported MVP posture

The repo now supports documenting and packaging a rescue workflow around the
Linux desktop artifact already produced by the release pipeline.

### Supported in this MVP

- start from a non-installed live Linux environment;
- launch the Récupère `AppImage` from the live session or a second removable
  medium;
- inspect devices, run read-only analysis, run read-only imaging, export to a
  separate destination, and generate support or lab bundles;
- keep the product posture explicit:
  - never write to the source disk,
  - never restore back onto the source disk,
  - never imply that deleted bytes can be recreated when they are physically
    gone.

### Explicitly out of scope

- custom Récupère-branded boot ISO;
- broad hardware-driver claims across all chipsets and controllers;
- persistent rescue environment state between boots;
- direct rescue boot support for macOS or Windows hosts;
- network/NAS rescue parity;
- encryption unlock parity for all pre-boot cases.

## Operator workflow

1. Prepare a trusted live Linux USB and a separate writable destination disk.
2. Boot the target machine from the live USB.
3. Confirm the source disk stays read-only at the OS / operator level.
4. Launch the packaged Récupère `AppImage`.
5. Perform only:
   - read-only scan,
   - read-only imaging,
   - export to the separate destination,
   - support or lab bundle generation.
6. Shut down without mounting or writing back to the source volume.

## Safety constraints

- The source disk must never be used as the export destination.
- Any destination ambiguity must be treated as a stop condition.
- If the live environment auto-mounts a source volume read-write, the operator
  must stop and reconfigure before continuing.
- The workflow does not authorize in-place repair, restore, or mutation of the
  source media.

## Evidence currently present in the repo

- Linux packaging already emits an `AppImage` artifact through the Tauri bundle
  pipeline when built on Linux.
- release metadata can now describe rescue-workflow readiness explicitly.
- preflight can now verify that the rescue-workflow documentation is present.

## Honest maturity statement

This is not yet a full top-tier rescue environment.

It is the first serious rescue slice because it gives the repo:

- an official rescue posture,
- a concrete launch surface,
- auditable release metadata,
- explicit operator limits that match the read-only mission.
