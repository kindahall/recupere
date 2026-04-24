# Récupère APFS P0 blocker note

Date: `2026-04-23`  
Status: `blocked`, not a completed benchmark run

## Purpose

Refresh the repo proof for the remaining `TT-01` / `TT-05` blocker:
`apfs_deleted_orphan_catalog_v1`.

This note records fresh execution evidence from the ignored APFS tests instead
of leaving the blocker as a stale comment.

## Commands executed

```bash
cargo test --manifest-path src-tauri/Cargo.toml \
  recover_deleted_files_reads_a_real_apfs_deleted_fixture \
  -- --ignored --nocapture
```

```bash
cargo test --manifest-path src-tauri/Cargo.toml \
  run_deleted_apfs_scan_marks_results_as_live_catalog_provenance \
  -- --ignored --nocapture
```

## Observed result

- `recover_deleted_files_reads_a_real_apfs_deleted_fixture` failed because the
  deleted APFS candidate was not present.
- `run_deleted_apfs_scan_marks_results_as_live_catalog_provenance` failed
  because no deleted APFS result was produced.
- the synthetic parser sanity check still passed:
  `scan_catalog_state_tracks_active_and_orphan_file_inodes`.
- the new debug helper passed and narrowed the blocker:
  `debug_deleted_catalog_candidates_reports_real_deleted_fixture_summary`
  reported:
  - `total_file_inodes: 3`
  - `active_file_ids: 3`
  - `deleted_inode_candidates: 0`
  - `deleted_candidates_with_extents: 0`

In other words, the current real fixture path is not producing a current-catalog
orphan at all on this host path. The failure is upstream of extent recovery.

## Why this matters

The blocker is no longer just historical context from `2026-04-22`. The repo
now carries a same-day proof that the real APFS deleted-file fixture path still
does not surface a benchmark-grade orphan candidate on the current host path.

## Honest conclusion

- `APFS P0` is still not closed.
- the next useful step is to stabilize fixture generation or catalog capture on
  macOS before claiming a publishable APFS benchmark slice.
- the most likely next debugging target is fixture generation, not the current
  extent-to-byte-run reconstruction path, because the catalog currently exposes
  no deleted inode candidate to reconstruct.
- until then, this scenario should stay explicitly visible as `blocked` or
  `public-artifact-pending`, never silently omitted.
