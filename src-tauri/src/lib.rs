// ============================================================================
// Récupère — Rust Backend Library
// ============================================================================
// Core architecture:
//   - commands/: IPC command handlers (bridge between UI and engine)
//   - core/:     Low-level I/O, hardware detection, disk imaging
//   - analyzers/: Filesystem parsers (FAT32, exFAT, NTFS)
//   - partitioning/: Conservative partition-table and lost-volume detection
//   - carving/:  Signature-based file recovery engine
//   - scoring/:  Recoverability scoring engine
//   - ai/:       AI diagnostic service layer
//   - preview/:  File preview generation
//   - export/:   Secure file export engine
//   - audit/:    Audit trail and logging
//   - types/:    Shared type definitions
// ============================================================================

pub mod ai;
pub mod analyzers;
pub mod audit;
pub mod carving;
pub mod cloud_ai;
// `commands` is no longer Tauri-only: the `#[tauri::command]` attribute is now
// `#[cfg_attr(feature = "desktop", tauri::command)]` on every handler, so the
// module compiles in headless builds and exposes plain `pub fn`s that the
// `recupere-agent` binary calls directly.
pub mod commands;
pub mod core;
pub mod correlation;
#[cfg(feature = "desktop")]
mod device_watcher;
pub mod encryption;
pub mod fallback;
pub mod filesystem_memory;
pub mod imaging;
pub mod imported_sources;
pub mod license;
pub mod partitioning;
pub mod preview;
pub mod privileged_imager;
pub mod raid;
#[cfg(feature = "desktop")]
pub mod remote;
pub mod repair;
pub mod scoring;
pub mod slack;
#[cfg(feature = "desktop")]
mod startup_guard;
pub mod telemetry;
pub mod types;
pub mod virtual_disk;

// The Tauri runtime entrypoints below are desktop-only. The headless build
// (used by `recupere-agent`) compiles the engine modules above and stops here.
#[cfg(feature = "desktop")]
mod desktop_runtime {
    use super::*;

    /// Initialise the global `tracing` subscriber. Idempotent — safe to call
    /// from `run()` and from tests. Honours the `RUST_LOG` env var (defaults to
    /// `recupere=info` so the app emits its own events but stays quiet about
    /// transitive crates).
    fn init_tracing() {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;
        use tracing_subscriber::EnvFilter;
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("recupere=info,recupere_lib=info"));
        // Compose the formatting layer with the local ring-buffer layer so
        // the same events that reach the terminal also land in the in-memory
        // ring exposed by `get_recent_traces`. The ring is local only — no
        // event leaves the process.
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_target(true)
                    .with_level(true),
            )
            .with(crate::telemetry::RingBufferLayer::new())
            .try_init();
    }

    #[cfg_attr(mobile, tauri::mobile_entry_point)]
    pub fn run() {
        init_tracing();

        // Refuse to start a release binary that still embeds the development
        // license public key. The compile-time guard in build.rs catches the
        // common case (no env var set at `cargo build --release`); this is
        // the defense-in-depth runtime check for anything that slips through.
        if let Err(error) = crate::license::ensure_production_public_key() {
            tracing::error!(
                target: "recupere::license",
                "fatal license-key check failed: {error}"
            );
            eprintln!("[recupere] fatal: {error}");
            std::process::exit(78); // EX_CONFIG
        }

        tauri::Builder::default()
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_opener::init())
            .setup(|app| {
                // Spawn the native hotplug watcher. It runs on its own thread and
                // emits `device:changed` to the front-end whenever a removable
                // drive is plugged in or unplugged.
                device_watcher::start(app.handle().clone());

                // Start the opt-in passive filesystem-memory scheduler with the
                // policy the user persisted in a previous session. `Manual` is
                // the default and keeps the thread idle; `Scheduled { ... }`
                // spawns a single ticking thread; `RealtimeDeferred` stays idle
                // and logs an honest warning (no realtime watcher is shipped
                // today). Failures here are non-fatal — the app stays usable
                // even if the policy file is unreadable.
                match crate::filesystem_memory::load_monitoring_policy() {
                    Ok(policy) => crate::filesystem_memory::scheduler::start_with_policy(policy),
                    Err(error) => tracing::warn!(
                        target: "recupere::filesystem_memory::scheduler",
                        "skipping scheduler startup: {error}"
                    ),
                }
                Ok(())
            })
            .invoke_handler(tauri::generate_handler![
                commands::get_runtime_capabilities,
                commands::get_app_build_info,
                commands::get_devices,
                commands::import_recovery_source,
                commands::get_imported_recovery_source_status,
                commands::prepare_imported_recovery_source,
                commands::remove_imported_recovery_source,
                commands::get_diagnostic,
                commands::get_ai_advisory,
                commands::get_scan_ai_brief,
                commands::start_scan,
                commands::start_potential_volume_scan,
                commands::start_imaging,
                commands::get_scan_progress,
                commands::pause_scan,
                commands::resume_scan,
                commands::cancel_scan,
                commands::get_scan_history,
                commands::get_scan_logs,
                commands::generate_imaging_session_report,
                commands::generate_imaging_rescue_map,
                commands::get_results,
                commands::get_file_preview,
                commands::get_file_media_asset,
                commands::repair_file,
                commands::save_repaired_file,
                commands::get_file_hex_preview,
                commands::get_file_auxiliary_hex_preview,
                commands::get_file_auxiliary_preview,
                commands::save_file_auxiliary_payload,
                commands::validate_export_destination,
                commands::start_export,
                commands::get_export_progress,
                commands::get_export_logs,
                commands::get_export_history,
                commands::clear_local_history,
                commands::save_technical_timeline_report,
                commands::save_support_bundle,
                commands::get_smart_report,
                commands::detect_raid_metadata,
                commands::scan_raid_candidates,
                commands::validate_raid_analysis_build,
                commands::build_raid_analysis_image,
                commands::get_encryption_info,
                commands::unlock_encrypted_device,
                commands::generate_recovery_report,
                commands::export_results_csv,
                commands::generate_lab_bundle,
                commands::ai_autopilot_scan,
                commands::get_gemma_settings,
                commands::save_gemma_settings,
                commands::probe_gemma_registry,
                commands::get_gemma_memory_advisory,
                commands::get_gemma_status,
                commands::start_gemma_pull,
                commands::get_gemma_pull_progress,
                commands::classify_scan_files,
                commands::predict_scan_recovery,
                commands::generate_narrative_report,
                commands::suggest_file_reconstruction,
                commands::build_cloud_ai_prompt,
                commands::run_gemma_analysis,
                commands::start_gemma_analysis,
                commands::get_gemma_analysis_progress,
                commands::chat_with_ai,
                commands::smart_select_by_category,
                commands::search_file_by_name,
                commands::activate_license,
                commands::get_license_status,
                commands::deactivate_license,
                commands::get_machine_fingerprint,
                commands::get_audit_trail,
                commands::verify_audit_trail,
                commands::get_recent_traces,
                commands::clear_recent_traces,
                commands::create_filesystem_memory_snapshot,
                commands::list_filesystem_memory_snapshots,
                commands::compute_filesystem_memory_diff,
                commands::get_filesystem_memory_missing_files,
                commands::get_recovery_context_hints,
                commands::get_filesystem_memory_policy,
                commands::save_filesystem_memory_policy,
                crate::remote::commands::add_remote_agent,
                crate::remote::commands::list_remote_agents,
                crate::remote::commands::remove_remote_agent,
                crate::remote::commands::remote_get_devices,
                crate::remote::commands::remote_start_scan,
                crate::remote::commands::remote_get_scan_progress,
                crate::remote::commands::remote_get_results,
                crate::remote::commands::remote_get_scan_logs,
                crate::remote::commands::remote_pause_scan,
                crate::remote::commands::remote_resume_scan,
                crate::remote::commands::remote_cancel_scan,
                crate::remote::commands::remote_restore,
                crate::remote::commands::remote_get_export_progress,
                crate::remote::commands::remote_get_file_preview,
                crate::remote::commands::remote_get_file_hex_preview,
                crate::remote::commands::remote_get_file_media_asset,
                crate::remote::commands::remote_get_scan_ai_brief,
                crate::remote::commands::remote_classify_scan_files,
                crate::remote::commands::remote_predict_scan_recovery,
                crate::remote::commands::remote_generate_recovery_report,
                crate::remote::commands::remote_export_results_csv,
                crate::remote::commands::remote_download_file,
                crate::remote::commands::remote_pull_recovered_file,
            ])
            .run(tauri::generate_context!())
            .expect("error while running tauri application");
    }

    pub fn run_entrypoint() -> i32 {
        let args: Vec<_> = std::env::args_os().collect();
        if privileged_imager::is_privileged_imager_mode(&args) {
            match privileged_imager::run_privileged_imager_from_args(args) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("[recupere] privileged_imager: {error}");
                    1
                }
            }
        } else {
            #[cfg(debug_assertions)]
            if let Err(error) = startup_guard::ensure_dev_frontend_available() {
                startup_guard::report_startup_error(&error);
                return 1;
            }

            run();
            0
        }
    }
} // mod desktop_runtime

#[cfg(feature = "desktop")]
pub use desktop_runtime::{run, run_entrypoint};
