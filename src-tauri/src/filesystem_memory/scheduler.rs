// ============================================================================
// Récupère — Filesystem memory layer: passive scheduler (Chantier 82 B6)
// ============================================================================
// Optional, opt-in background tick that re-runs a snapshot on every tracked
// target path at a fixed interval. Deliberately NOT a realtime watcher — the
// UI must surface this as "scheduled passive snapshots" so the user knows
// exactly what's happening (AGENTS.md: honest, never magic).
//
// Design:
//   - One std::thread spawned from `start_with_policy`. It listens on a
//     `mpsc::Receiver<Stop>` with `recv_timeout` so `stop()` is reactive even
//     during a long sleep.
//   - Tick interval is read from `MonitoringPolicy::Scheduled { interval_minutes }`
//     and clamped upward to `MIN_INTERVAL_MINUTES` via `.normalize()` before
//     the loop is started. This is the same guarantee the UI gives the user.
//   - On each tick, we call `capture_snapshot` + `persist_snapshot` +
//     `rotate_snapshots` for every distinct `target_path` we already have a
//     snapshot for. If the store is empty, the tick is a no-op and we log it.
//   - `MonitoringPolicy::Manual` or `RealtimeDeferred` → the scheduler stays
//     stopped; `RealtimeDeferred` additionally logs an honest warning that the
//     product does not provide realtime monitoring today.
// ============================================================================

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::indexer::{capture_snapshot, CaptureOptions};
use super::store::{load_all_snapshots, persist_snapshot, rotate_snapshots};
use super::types::{MonitoringPolicy, SnapshotStatus};

/// Poll interval used by the scheduler thread when it sleeps between ticks.
/// We wake up roughly every 30 seconds to check the stop signal so `stop()`
/// does not have to wait for a full tick to take effect.
const STOP_POLL_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug)]
enum StopSignal {
    Stop,
}

struct SchedulerHandle {
    stop_tx: mpsc::Sender<StopSignal>,
    thread: Option<JoinHandle<()>>,
    policy: MonitoringPolicy,
}

fn handle_slot() -> &'static Mutex<Option<SchedulerHandle>> {
    static HANDLE: OnceLock<Mutex<Option<SchedulerHandle>>> = OnceLock::new();
    HANDLE.get_or_init(|| Mutex::new(None))
}

fn lock_handle() -> std::sync::MutexGuard<'static, Option<SchedulerHandle>> {
    handle_slot()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

/// Return the policy the scheduler thread is currently running under, or `None`
/// if no thread is active. Used by tests and by the UI to reflect live state.
pub fn active_policy() -> Option<MonitoringPolicy> {
    lock_handle().as_ref().map(|handle| handle.policy.clone())
}

/// Start (or restart) the scheduler with `policy`. If a thread is already
/// running it is stopped and replaced. `Manual` / `RealtimeDeferred` keep the
/// scheduler stopped.
pub fn start_with_policy(policy: MonitoringPolicy) {
    stop();
    start_with_policy_using(policy, default_tick);
}

/// Stop the scheduler thread if any. Safe to call multiple times.
pub fn stop() {
    let maybe_handle = {
        let mut guard = lock_handle();
        guard.take()
    };
    if let Some(mut handle) = maybe_handle {
        let _ = handle.stop_tx.send(StopSignal::Stop);
        if let Some(thread) = handle.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Testable variant: lets tests inject a short-circuit tick function instead
/// of running a full snapshot capture. `tick_fn` is called once per tick while
/// the scheduler is active; it receives the tick number (1-based).
fn start_with_policy_using<F>(policy: MonitoringPolicy, tick_fn: F)
where
    F: Fn(u64) + Send + 'static,
{
    let normalized = policy.normalize();

    match &normalized {
        MonitoringPolicy::Scheduled { interval_minutes } => {
            let interval = Duration::from_secs(u64::from(*interval_minutes) * 60);
            let (stop_tx, stop_rx) = mpsc::channel::<StopSignal>();
            let thread = thread::Builder::new()
                .name("recupere-fsmem-scheduler".into())
                .spawn(move || run_scheduler_loop(interval, stop_rx, tick_fn))
                .expect("filesystem-memory scheduler thread should spawn");
            tracing::info!(
                target: "recupere::filesystem_memory::scheduler",
                "scheduler started with interval {} minute(s)",
                interval_minutes
            );
            *lock_handle() = Some(SchedulerHandle {
                stop_tx,
                thread: Some(thread),
                policy: normalized,
            });
        }
        MonitoringPolicy::Manual => {
            tracing::info!(
                target: "recupere::filesystem_memory::scheduler",
                "scheduler stays idle: monitoring policy is Manual"
            );
        }
        MonitoringPolicy::RealtimeDeferred => {
            tracing::warn!(
                target: "recupere::filesystem_memory::scheduler",
                "realtime monitoring is not implemented yet; the scheduler will stay idle. \
                 Fall back to Scheduled or Manual to get passive snapshots."
            );
        }
    }
}

fn run_scheduler_loop(
    interval: Duration,
    stop_rx: mpsc::Receiver<StopSignal>,
    tick_fn: impl Fn(u64),
) {
    let mut tick_index: u64 = 0;
    loop {
        let mut waited = Duration::ZERO;
        while waited < interval {
            let remaining = interval - waited;
            let sleep_chunk = STOP_POLL_INTERVAL.min(remaining);
            match stop_rx.recv_timeout(sleep_chunk) {
                Ok(StopSignal::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    waited += sleep_chunk;
                }
            }
        }
        tick_index += 1;
        tick_fn(tick_index);
    }
}

fn default_tick(tick_index: u64) {
    match run_scheduled_tick() {
        Ok(summary) => {
            tracing::info!(
                target: "recupere::filesystem_memory::scheduler",
                "tick #{} completed: {} target(s), {} snapshot(s) captured, {} error(s)",
                tick_index,
                summary.targets,
                summary.snapshots_captured,
                summary.errors.len()
            );
            for error in summary.errors {
                tracing::warn!(
                    target: "recupere::filesystem_memory::scheduler",
                    "tick #{}: {}",
                    tick_index,
                    error
                );
            }
        }
        Err(error) => {
            tracing::warn!(
                target: "recupere::filesystem_memory::scheduler",
                "tick #{} aborted: {}",
                tick_index,
                error
            );
        }
    }
}

struct TickSummary {
    targets: usize,
    snapshots_captured: usize,
    errors: Vec<String>,
}

fn run_scheduled_tick() -> Result<TickSummary, String> {
    let snapshots = load_all_snapshots()?;
    let targets = collect_distinct_targets(&snapshots);

    if targets.is_empty() {
        return Ok(TickSummary {
            targets: 0,
            snapshots_captured: 0,
            errors: Vec::new(),
        });
    }

    let mut errors = Vec::new();
    let mut captured = 0;
    for target in &targets {
        match snapshot_target(target) {
            Ok(()) => captured += 1,
            Err(error) => errors.push(format!("{}: {error}", target.to_string_lossy())),
        }
    }

    Ok(TickSummary {
        targets: targets.len(),
        snapshots_captured: captured,
        errors,
    })
}

fn collect_distinct_targets(snapshots: &[super::types::FilesystemSnapshot]) -> Vec<PathBuf> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for snapshot in snapshots {
        if snapshot.status == SnapshotStatus::Running {
            continue;
        }
        if seen.insert(snapshot.target_path.clone()) {
            out.push(PathBuf::from(snapshot.target_path.clone()));
        }
    }
    out
}

fn snapshot_target(target: &Path) -> Result<(), String> {
    let id = format!(
        "fsm-sched-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default()
    );
    let options = CaptureOptions::default();
    let snapshot = capture_snapshot(&id, target, &options)?;
    persist_snapshot(&snapshot)?;
    rotate_snapshots(&snapshot.target_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    fn reset_handle_for_test() {
        // Tests share the process-wide OnceLock<Mutex<Option<SchedulerHandle>>>.
        // We stop any leftover thread to keep tests isolated.
        stop();
    }

    #[test]
    fn manual_policy_does_not_spawn_a_thread() {
        reset_handle_for_test();
        start_with_policy(MonitoringPolicy::Manual);
        assert!(
            active_policy().is_none(),
            "Manual policy must leave the scheduler idle"
        );
    }

    #[test]
    fn realtime_deferred_policy_does_not_spawn_a_thread() {
        reset_handle_for_test();
        start_with_policy(MonitoringPolicy::RealtimeDeferred);
        assert!(
            active_policy().is_none(),
            "RealtimeDeferred policy must not start a scheduler thread"
        );
    }

    #[test]
    fn scheduled_policy_normalizes_interval_before_storing_it() {
        reset_handle_for_test();
        start_with_policy(MonitoringPolicy::Scheduled {
            interval_minutes: 1,
        });
        let active = active_policy().expect("scheduler should report Scheduled policy");
        assert_eq!(
            active,
            MonitoringPolicy::Scheduled {
                interval_minutes: MonitoringPolicy::MIN_INTERVAL_MINUTES
            }
        );
        stop();
    }

    #[test]
    fn stop_is_idempotent() {
        reset_handle_for_test();
        stop();
        stop();
        assert!(active_policy().is_none());
    }

    #[test]
    fn tick_loop_fires_the_injected_closure_until_stopped() {
        reset_handle_for_test();
        // Use `start_with_policy_using` directly with a sub-second interval and
        // a counting closure so we don't have to wait 15 minutes.
        let counter = Arc::new(AtomicU64::new(0));
        let counter_clone = counter.clone();
        // Bypass `start_with_policy` so we can use a 0-minute interval purely
        // for this test. `start_with_policy_using` still clamps through
        // `normalize()` which is fine — we want to verify the spawn + stop
        // lifecycle, not timing precision.
        // Spin a lightweight thread directly so the interval is override-able
        // in this test without storing a policy in the scheduler handle.
        let (stop_tx, stop_rx) = mpsc::channel::<StopSignal>();
        let thread = thread::Builder::new()
            .name("recupere-fsmem-scheduler-test".into())
            .spawn(move || {
                run_scheduler_loop(Duration::from_millis(50), stop_rx, move |tick| {
                    counter_clone.fetch_add(tick, Ordering::SeqCst);
                })
            })
            .expect("test scheduler thread should spawn");

        // Wait up to ~1s for at least 3 ticks to accumulate.
        let mut attempts = 0;
        while counter.load(Ordering::SeqCst) < 3 && attempts < 40 {
            thread::sleep(Duration::from_millis(50));
            attempts += 1;
        }
        let _ = stop_tx.send(StopSignal::Stop);
        let _ = thread.join();
        assert!(
            counter.load(Ordering::SeqCst) >= 3,
            "scheduler should tick multiple times before stop (observed {})",
            counter.load(Ordering::SeqCst)
        );
        // The policy is not stored in the handle slot here since we bypassed
        // start_with_policy, so active_policy() should remain None.
        assert!(active_policy().is_none());
    }

    #[test]
    fn collect_distinct_targets_dedupes_and_ignores_running_snapshots() {
        use super::super::types::{FilesystemSnapshot, SnapshotStatus};
        fn make(id: &str, target: &str, status: SnapshotStatus) -> FilesystemSnapshot {
            FilesystemSnapshot {
                id: id.into(),
                target_path: target.into(),
                captured_at_ms: 0,
                status,
                files_indexed: 0,
                total_size_bytes: 0,
                errors: vec![],
                volume_fingerprint: None,
                records: vec![],
            }
        }

        let snapshots = vec![
            make("a", "/one", SnapshotStatus::Completed),
            make("b", "/two", SnapshotStatus::Partial),
            make("c", "/one", SnapshotStatus::Completed),
            make("d", "/three", SnapshotStatus::Running),
        ];
        let targets = collect_distinct_targets(&snapshots);
        let targets_str: Vec<String> = targets
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        assert_eq!(targets_str, vec!["/one".to_string(), "/two".to_string()]);
    }
}
