// ============================================================================
// Récupère — Runtime introspection commands
// ============================================================================
// Surface host runtime capabilities (which engines are available) and build
// identity (product name, bundle id, target triple) to the front-end. The
// computation lives here; `commands/mod.rs` re-exports `runtime_capabilities`
// and `app_build_info` via `pub(super) use` so that callers in neighbouring
// modules (scan, ai, support-bundle builder, tests) keep the bare name.
// ============================================================================

use std::env;

use crate::types::{AppBuildInfo, RuntimeCapabilities};

pub(crate) const APP_PRODUCT_NAME: &str = "Récupère";
pub(crate) const APP_BUNDLE_IDENTIFIER: &str = "com.recupere.desktop";

pub(crate) fn runtime_capabilities() -> RuntimeCapabilities {
    // `optional_cloud_ai` now reflects whether the bundled Gemma backend is
    // ready (Ollama reachable + model installed). The field name is kept for
    // serialization compatibility with the front-end mappers.
    let gemma_ready = crate::cloud_ai::check_status(&crate::cloud_ai::load_settings()).ready;
    RuntimeCapabilities {
        device_detection: true,
        heuristic_diagnostic: true,
        ai_advisory: true,
        optional_cloud_ai: gemma_ready,
        scan_engine: true,
        imaging_engine: true,
        results_browser: true,
        export_validation: true,
        export_engine: true,
        technical_logs: true,
        limited_capabilities: vec![
            "aiAdvisory".into(),
            "scanEngine".into(),
            "imagingEngine".into(),
            "technicalLogs".into(),
        ],
    }
}

pub(crate) fn app_build_info() -> AppBuildInfo {
    AppBuildInfo {
        product_name: APP_PRODUCT_NAME.into(),
        bundle_identifier: APP_BUNDLE_IDENTIFIER.into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        package_name: env!("CARGO_PKG_NAME").into(),
        build_profile: if cfg!(debug_assertions) {
            "debug".into()
        } else {
            "release".into()
        },
        operating_system: env::consts::OS.into(),
        architecture: env::consts::ARCH.into(),
        target_triple: option_env!("TARGET").unwrap_or("unknown").into(),
        tauri_runtime: "desktop".into(),
    }
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_runtime_capabilities() -> RuntimeCapabilities {
    tracing::info!("get_runtime_capabilities: returning available desktop capabilities");
    runtime_capabilities()
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_app_build_info() -> AppBuildInfo {
    tracing::info!("get_app_build_info: returning build/runtime identity");
    app_build_info()
}
