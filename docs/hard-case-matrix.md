# Hard-case recovery matrix

Catalogue the hard-case scenarios Récupère is expected to handle, their
current coverage level, and the regression tests that lock each property
in. A "hard case" is anything where naive filesystem readers fail silently
— we owe users a crisp answer: *"yes we recover this", "we recover it
partially with these caveats", or "we do not recover this today"*.

Status legend:

- ✅ **Covered** — real test exercises the path end-to-end.
- 🟠 **Partial** — code path exists; tests only cover the happy corner.
- ❌ **Gap** — not implemented. Tracked under the chantier id in the
  right-hand column.

## Carving engine — fragmentation & damaged containers

| Scenario | Status | Lock-in / Owner |
|---|---|---|
| JPEG with intact SOI + EOI | ✅ | `carve_signatures_finds_a_jpeg_candidate` |
| PNG with intact IHDR + IEND | ✅ | `carve_signatures_marks_corrupt_png_candidates` (inverse) |
| PNG reassembled across 1 gap | ✅ | `carve_signatures_rebuilds_a_fragmented_png_across_a_single_gap` |
| PNG reassembled across 2 gaps | ✅ | `carve_signatures_rebuilds_a_fragmented_png_across_two_gaps` |
| PNG reassembled across 3 gaps | ✅ | `carve_signatures_rebuilds_a_fragmented_png_across_three_gaps` |
| PDF lacking `startxref` trailer | ✅ | `carve_signatures_marks_corrupt_pdf_candidates_without_startxref` |
| ZIP with broken central-directory | ✅ | `carve_signatures_marks_corrupt_zip_candidates` |
| JPEG reassembled across a single gap | 🟠 | TODO — parallel to PNG coverage; tracked I7. |
| MP4 with truncated `mdat` atom | 🟠 | Detected as corrupt; not reassembled. I7 follow-up. |
| MKV with partial EBML | ❌ | I7. |
| Office `.docx` with broken ZIP central-directory | 🟠 | Partial text preview only. I7. |

## NTFS — MFT & compressed / sparse / ADS

| Scenario | Status | Lock-in / Owner |
|---|---|---|
| Visible file, resident attribute | ✅ | `run_inventory_scan` + carving layer tests |
| Deleted file with LZNT1-compressed data runs | ✅ | `run_export_session_reconstructs_deleted_compressed_ntfs_file_from_image` |
| Deleted file with sparse (ZERO run) | ✅ | `run_export_session_reconstructs_deleted_sparse_ntfs_file_from_image` |
| Deleted file with named stream (Alternate Data Stream) | ✅ | `run_export_session_writes_ntfs_ads_sidecars` |
| MFT record with entirely-zeroed attribute list | ❌ | I7. |
| MFT mirror promoted after MFT corruption | ❌ | I7. Tracked as multi-session. |
| Deleted file whose first data run points outside volume bounds | 🟠 | Current impl rejects at bounds check. I7 could salvage partial. |

## FAT32 / exFAT

| Scenario | Status | Lock-in / Owner |
|---|---|---|
| Deleted short-name entry with preserved cluster chain | ✅ | `run_export_session_reconstructs_deleted_file_from_image` |
| Deleted file where one cluster was re-allocated | ✅ | `run_export_session_materializes_partial_deleted_file_size` |
| Deleted file with LFN chain + truncated last slot | 🟠 | Current impl falls back to 8.3 name. I7 could reconstruct LFN. |
| FAT chain with circular reference | ❌ | I7 — blocked by planned parser hardening. |

## APFS / HFS+

| Scenario | Status | Lock-in / Owner |
|---|---|---|
| APFS synthetic raw image with deleted catalog entry | 🟠 | Logic exists, but the real macOS fixture path is still not deterministic on macOS 15.7.4; fresh blocker proof shows `3` file inodes, `3` active ids, and `0` deleted candidates in the current catalog, so the present blocker is fixture/catalog generation rather than extent reconstruction. Archived in `benchmarks/results/2026-04-23-recupere-apfs-p0-blocker-note.md`, tracked under `TT-05` / `TT-01`. |
| HFS+ with resource-fork sidecar | ✅ | `run_export_session_writes_hfsplus_resource_fork_sidecar` |
| APFS volume snapshot / clone | ❌ | I8 (multi-disk / snapshot story). |
| APFS encrypted volume pre-unlock | ❌ | I8. |

## Partition tables

| Scenario | Status | Lock-in / Owner |
|---|---|---|
| MBR with recognisable entry types | ✅ | `partitioning::mbr_parser` tests |
| GPT primary + backup header cross-check | ✅ | `partitioning::gpt_parser` tests |
| GPT with corrupted primary, valid backup | 🟠 | Detected; surface as "lost volume" candidate. |
| Nested APFS container (EFI / GPT / APFS) | ✅ | APFS fixtures |
| Completely wiped partition table | ❌ | I8 — the "raw disk" module (I4) surfaces the device; carving takes over without FS context. |

## Hardware-level scenarios

| Scenario | Status | Lock-in / Owner |
|---|---|---|
| Unresponsive / offline disk (no partition visible) | ✅ (surface only) | `core::raw_disks` Linux impl (I4). macOS / Windows stubs. |
| SMART reallocated-sector count > 0 | ✅ | `core::smart` enrichment + scoring penalty. |
| TRIM-enabled SSD (deleted data likely zeroed) | ✅ (warning only) | Scoring deducts 12 points; UI shows a warning. |
| Cautious imaging across unreadable ranges | ✅ | `imaging::cautious_imaging_zero_fills_unreadable_ranges_and_continues` |
| Multi-disk RAID-1 member recovery | ❌ | I8. |
| BitLocker / LUKS / FileVault volume pre-unlock | 🟠 | `core::encryption` detects; unlock UX not integrated. I8. |

## How to add a new row

1. Pick the shortest test that demonstrates the hard case using
   synthetic fixtures (see `src-tauri/src/carving/mod.rs:5194+`).
2. Add the row to this matrix with the test function name.
3. If the scenario isn't covered yet, open the row as `❌` and tag
   the appropriate chantier (`I7` for filesystem hardening, `I8` for
   RAID / encryption / multi-disk).

Rationale: the matrix is deliberately cataloging *invariants* we promise
users, not signature coverage (see the 400+ file-format signatures in
[`src-tauri/src/carving/mod.rs`](../src-tauri/src/carving/mod.rs)). A
feature ticking "we support format X" doesn't prove anything if a
subtly-damaged file in format X isn't tested end-to-end.
