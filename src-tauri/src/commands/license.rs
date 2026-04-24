// ============================================================================
// Récupère — License commands
// ============================================================================
// Tauri command handlers for license activation, status retrieval, and
// deactivation. All four entry points are thin wrappers around
// `crate::license::*`. They are separated from the rest of `commands/` so
// that license logic can evolve (Stripe webhook, hardware re-binding, …)
// without touching the scan/export pipeline.
// ============================================================================

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn activate_license(key: String) -> crate::license::LicenseInfo {
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
