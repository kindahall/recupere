// ============================================================================
// Criterion — scoring engine perf budget
// ============================================================================
// Tracks the steady-state cost of `scoring::recoverability_score` under a
// realistic mix of inputs. The budget is intentionally loose (µs range) —
// any regression beyond ±10 % between PRs is a signal that scoring became
// unexpectedly allocation-heavy or branch-dense.
//
// Run locally:
//   cargo bench --manifest-path src-tauri/Cargo.toml --bench scoring_budget
// ============================================================================

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use recupere_lib::scoring::recoverability_score;
use recupere_lib::types::{DeviceStatus, DeviceType, FilesystemType, RiskLevel};

fn bench_recoverability_score(c: &mut Criterion) {
    let scenarios = [
        (
            "healthy-ntfs-ssd-trim-on",
            RiskLevel::Low,
            DeviceStatus::Healthy,
            DeviceType::Ssd,
            Some(true),
            Some(false),
            FilesystemType::Ntfs,
            Some(true),
            false,
        ),
        (
            "critical-ext4-hdd-encrypted",
            RiskLevel::Critical,
            DeviceStatus::Failing,
            DeviceType::Hdd,
            Some(false),
            Some(true),
            FilesystemType::Ext4,
            Some(true),
            true,
        ),
        (
            "unknown-fs-unresponsive",
            RiskLevel::High,
            DeviceStatus::Unresponsive,
            DeviceType::Usb,
            None,
            None,
            FilesystemType::Unknown,
            None,
            false,
        ),
    ];

    let mut group = c.benchmark_group("recoverability_score");
    for scenario in &scenarios {
        let name = scenario.0;
        group.bench_function(name, |b| {
            b.iter(|| {
                recoverability_score(
                    black_box(&scenario.1),
                    black_box(&scenario.2),
                    black_box(&scenario.3),
                    black_box(scenario.4),
                    black_box(scenario.5),
                    black_box(&scenario.6),
                    black_box(scenario.7),
                    black_box(scenario.8),
                )
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_recoverability_score);
criterion_main!(benches);
