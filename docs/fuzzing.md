# Fuzzing Récupère's parser layer

Récupère ships filesystem analyzers (NTFS, exFAT, FAT32, APFS, HFS+, ext4) and
a signature-carving engine that each read arbitrary bytes from disk. A user's
first recovery attempt is typically on a drive that is *already* in trouble;
the parsers must never panic on corrupt or truncated input, they must surface
an error. Fuzzing is the honest way to prove that.

## Scope

| Target | Entry point | Goal |
| --- | --- | --- |
| `ntfs_boot` | `analyzers::ntfs::recover_deleted_files` + `list_visible_files` | 0 panic on any 512 B+ input |
| `carving_signatures` | `carving::carve_signatures` | 0 panic / 0 hang > 10 s on any 64 B+ input |

More targets (`exfat_directory`, `fat32_cluster_chain`, `apfs_superblock`) are
planned — the layout below is reusable: add a new file under
`src-tauri/fuzz/fuzz_targets/`, append a `[[bin]]` block to
`src-tauri/fuzz/Cargo.toml`, drop seed corpus into
`src-tauri/fuzz/corpus/<target>/`.

## Toolchain

`cargo-fuzz` requires a nightly toolchain because it uses `libFuzzer`'s
sanitizer coverage. Install once:

```bash
rustup install nightly
cargo install cargo-fuzz
```

## Running locally

```bash
cd src-tauri
# 10-minute sanity run per target
cargo +nightly fuzz run ntfs_boot -- -max_total_time=600
cargo +nightly fuzz run carving_signatures -- -max_total_time=600
```

Corpus and findings land in `src-tauri/fuzz/artifacts/<target>/` (crashes)
and `src-tauri/fuzz/corpus/<target>/` (inputs that trigger new coverage).
Commit interesting seeds; never commit crash artifacts to the main repo —
triage them and file a fix.

## CI policy

Running a 10 minute fuzz per target on every PR is too expensive. The
sustainable setup is a weekly GitHub Actions job:

- Run each target with `-max_total_time=600 -runs=-1`.
- Fail the job if libFuzzer reports any crash.
- Upload `fuzz/corpus/` as an artifact so coverage grows over time.

When a crash is found, reduce it with `cargo +nightly fuzz cmin` / `tmin`,
commit a regression test using the reduced input, ship the fix.

## What the fuzzer does NOT prove

Fuzzing confirms "no crash on arbitrary input" — not "recovery is correct".
Correctness is covered by the unit tests in each analyzer module plus the
proptest invariants in `scoring/mod.rs`. A green fuzz run is a necessary
but not sufficient condition for a release.
