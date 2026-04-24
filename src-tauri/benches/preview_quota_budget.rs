// ============================================================================
// Criterion — preview workspace quota sweep budget
// ============================================================================
// `enforce_preview_workspace_quota_at` runs on every preview materialisation,
// so it must stay cheap even when the workspace holds hundreds of entries.
// The benchmark seeds a workspace with N files and measures one sweep.
//
// Run locally:
//   cargo bench --manifest-path src-tauri/Cargo.toml --bench preview_quota_budget
// ============================================================================

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use recupere_lib::preview::enforce_preview_workspace_quota_at;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn seed_workspace(root: &Path, count: usize) -> PathBuf {
    let _ = fs::remove_dir_all(root);
    fs::create_dir_all(root).expect("workspace dir");
    let now = SystemTime::now();
    let mut latest = root.join("file-0.bin");
    for i in 0..count {
        let path = root.join(format!("file-{i}.bin"));
        fs::write(&path, vec![b'x'; 4096]).expect("seed write");
        let mtime = now - Duration::from_secs((count - i) as u64);
        let file = fs::File::options()
            .write(true)
            .open(&path)
            .expect("open for mtime");
        file.set_modified(mtime).expect("set mtime");
        latest = path;
    }
    latest
}

fn bench_quota_sweep(c: &mut Criterion) {
    let root = std::env::temp_dir().join(format!("recupere-bench-quota-{}", std::process::id()));
    let fresh = seed_workspace(&root, 256);

    let mut group = c.benchmark_group("preview_quota_sweep_256_entries");
    // Quota = half of total size → sweep must evict ~half the entries.
    group.bench_function("sweep_half", |b| {
        b.iter(|| {
            // Re-seed so each iteration exercises a directory in the state
            // we intended to measure (benches share process state otherwise).
            seed_workspace(&root, 256);
            let _ = enforce_preview_workspace_quota_at(
                black_box(&root),
                black_box(&fresh),
                black_box((256 * 4096 / 2) as u64),
            );
        });
    });
    group.finish();

    let _ = fs::remove_dir_all(&root);
}

criterion_group!(benches, bench_quota_sweep);
criterion_main!(benches);
