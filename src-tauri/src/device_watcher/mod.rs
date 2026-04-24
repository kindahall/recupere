#![allow(dead_code)]
// ============================================================================
// Récupère — Native device hotplug watcher
// ============================================================================
// Emits a `device:changed` Tauri event whenever a removable drive (USB stick,
// external SSD, SD card, mounted disk image…) is plugged in or unplugged.
// The front-end listens to this event and refreshes the device list instantly,
// without polling.
//
// Strategy:
//   - macOS: native FSEvents on `/Volumes` (notify crate uses FSEvents under
//     the hood). Mounting a USB stick creates a new entry in /Volumes;
//     ejecting it removes one. FSEvents fires a callback within milliseconds.
//   - Linux: native inotify on `/media/$USER`, `/run/media/$USER` and `/mnt`
//     (whichever exist). udisks2 / GNOME / KDE auto-mount removable drives
//     under one of these paths.
//   - Windows: notify is unable to watch for new drive letters appearing,
//     so we fall back to a fast 1 s diff loop on the disk count using
//     `sysinfo`. The latency is ~1 s but no extra dependency is required.
//
// All variants emit the same event payload (none — the front re-fetches the
// device list itself), so the front-end code is identical across platforms.
// ============================================================================

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};

const DEVICE_CHANGED_EVENT: &str = "device:changed";

/// Spawn a background thread that watches for device hotplug events and
/// emits `device:changed` to the front-end. Never blocks the caller.
pub fn start(app_handle: AppHandle) {
    thread::spawn(move || {
        if let Err(err) = run_watcher(&app_handle) {
            tracing::warn!("device_watcher exited with error: {err}");
        }
    });
}

#[cfg(target_os = "macos")]
fn run_watcher(app_handle: &AppHandle) -> Result<(), String> {
    watch_directories(app_handle, &[PathBuf::from("/Volumes")])
}

#[cfg(target_os = "linux")]
fn run_watcher(app_handle: &AppHandle) -> Result<(), String> {
    let user = std::env::var("USER").unwrap_or_default();
    let candidates = [
        PathBuf::from("/media").join(&user),
        PathBuf::from("/run/media").join(&user),
        PathBuf::from("/media"),
        PathBuf::from("/mnt"),
    ];
    let existing: Vec<PathBuf> = candidates.into_iter().filter(|p| p.exists()).collect();
    if existing.is_empty() {
        // Nothing to watch — fall back to slow polling instead of failing.
        return run_poll_fallback(app_handle);
    }
    watch_directories(app_handle, &existing)
}

#[cfg(target_os = "windows")]
fn run_watcher(app_handle: &AppHandle) -> Result<(), String> {
    // notify on Windows watches file changes inside an existing directory; it
    // does not catch new drive letters appearing. Use a fast diff loop.
    run_poll_fallback(app_handle)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn run_watcher(app_handle: &AppHandle) -> Result<(), String> {
    run_poll_fallback(app_handle)
}

// ---------------------------------------------------------------------------
// notify-based watcher (macOS / Linux)
// ---------------------------------------------------------------------------

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn watch_directories(app_handle: &AppHandle, paths: &[PathBuf]) -> Result<(), String> {
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |event| {
        // Best-effort: dropped events are ignored.
        let _ = tx.send(event);
    })
    .map_err(|e| format!("Cannot create watcher: {e}"))?;

    for path in paths {
        if let Err(e) = watcher.watch(path, RecursiveMode::NonRecursive) {
            // A missing directory should not abort the whole watcher; just log.
            tracing::warn!("device_watcher: cannot watch {}: {e}", path.display());
        }
    }

    // Debounce burst events: macOS FSEvents and Linux inotify both fire
    // multiple events for a single mount (directory created, attributes
    // changed, …). Coalesce them into a single emit per ~250 ms window.
    loop {
        match rx.recv() {
            Ok(_) => {
                // Drain any additional pending events to debounce.
                let drain_until = std::time::Instant::now() + Duration::from_millis(250);
                while std::time::Instant::now() < drain_until {
                    if rx.recv_timeout(Duration::from_millis(50)).is_err() {
                        break;
                    }
                }
                let _ = app_handle.emit(DEVICE_CHANGED_EVENT, ());
            }
            Err(_) => {
                // Channel closed — watcher dropped; exit.
                return Ok(());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Polling fallback (Windows / unsupported platforms)
// ---------------------------------------------------------------------------

fn run_poll_fallback(app_handle: &AppHandle) -> Result<(), String> {
    let mut last_signature = current_disk_signature();
    loop {
        thread::sleep(Duration::from_millis(1000));
        let next = current_disk_signature();
        if next != last_signature {
            last_signature = next;
            let _ = app_handle.emit(DEVICE_CHANGED_EVENT, ());
        }
    }
}

/// Cheap, allocation-light fingerprint of the currently mounted disks. Two
/// snapshots compare equal as long as the same set of (name, total_space)
/// pairs is mounted, regardless of order.
fn current_disk_signature() -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut entries: Vec<(String, u64)> = disks
        .list()
        .iter()
        .map(|d| (d.name().to_string_lossy().to_string(), d.total_space()))
        .collect();
    entries.sort();

    let mut hasher = DefaultHasher::new();
    entries.hash(&mut hasher);
    hasher.finish()
}
