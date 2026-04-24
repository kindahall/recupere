// ============================================================================
// Récupère — License commands
// ============================================================================
// Tauri command handlers for license activation, status retrieval, and
// deactivation. All four entry points are thin wrappers around
// `crate::license::*`. They are separated from the rest of `commands/` so
// that license logic can evolve (Stripe webhook, hardware re-binding, …)
// without touching the scan/export pipeline.
// ============================================================================

use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const LICENSE_ACTIVATION_WINDOW_SECS: u64 = 10 * 60;
const LICENSE_ACTIVATION_MAX_ATTEMPTS: usize = 10;
static LICENSE_ACTIVATION_ATTEMPTS: OnceLock<Mutex<Vec<u64>>> = OnceLock::new();

#[derive(serde::Serialize)]
pub struct PiiPurgeResult {
    pub license_deleted: bool,
    pub history_purged: bool,
    pub audit_cleared: bool,
    pub recent_traces_cleared: bool,
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn activate_license(key: String) -> crate::license::LicenseInfo {
    if !record_activation_attempt() {
        let info = crate::license::LicenseInfo::invalid(
            "rate_limited",
            "Too many license activation attempts. Wait a few minutes and try again.",
        );
        crate::audit::record(
            crate::audit::AuditEventKind::SettingsChanged,
            serde_json::json!({
                "license_activation": info.status,
                "valid": info.valid,
                "rate_limited": true,
            }),
        );
        return info;
    }

    let info = crate::license::activate_license(&key);
    crate::audit::record(
        crate::audit::AuditEventKind::SettingsChanged,
        serde_json::json!({
            "license_activation": info.status,
            "valid": info.valid,
        }),
    );
    info
}

fn record_activation_attempt() -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let attempts = LICENSE_ACTIVATION_ATTEMPTS.get_or_init(|| Mutex::new(Vec::new()));
    let mut attempts =
        crate::commands::state::lock_or_recover(attempts, "license activation attempts");
    attempts.retain(|timestamp| now.saturating_sub(*timestamp) <= LICENSE_ACTIVATION_WINDOW_SECS);
    if attempts.len() >= LICENSE_ACTIVATION_MAX_ATTEMPTS {
        return false;
    }
    attempts.push(now);
    true
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_license_status() -> crate::license::LicenseInfo {
    crate::license::get_license_status()
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn deactivate_license() -> Result<(), String> {
    crate::license::delete_license()?;
    crate::audit::record(
        crate::audit::AuditEventKind::SettingsChanged,
        serde_json::json!({"license_deactivated": true}),
    );
    Ok(())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_machine_fingerprint() -> String {
    crate::license::compute_machine_fingerprint()
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn purge_all_pii() -> Result<PiiPurgeResult, String> {
    let license_deleted = crate::license::delete_license().is_ok();
    let history_purged = super::clear_local_history("all".into()).is_ok();
    crate::audit::clear_trail();
    crate::telemetry::clear_traces();

    crate::audit::record(
        crate::audit::AuditEventKind::SettingsChanged,
        serde_json::json!({
            "pii_purge": true,
            "license_deleted": license_deleted,
            "history_purged": history_purged,
            "audit_trail_cleared_before_marker": true,
            "recent_traces_cleared": true,
        }),
    );

    Ok(PiiPurgeResult {
        license_deleted,
        history_purged,
        audit_cleared: true,
        recent_traces_cleared: true,
    })
}
