// Tauri commands exposing the remote-agent client to the frontend.
//
// All `remote_*` commands take an `agent_id` so the frontend can multiplex
// across several registered agents. The agent registry is process-local and
// in-memory (see `remote::registry`).
//
// Security invariants (see SECURITY.md — Remote agents threat model):
//   1. TLS is mandatory for any non-loopback base URL. Plain `http://` is only
//      accepted when the host resolves to `127.0.0.1`, `::1`, or `localhost`
//      (typical SSH-tunnel setup). `validate_remote_base_url` enforces this at
//      the registration boundary.
//   2. The bearer token never touches disk in cleartext — the `registry` stores
//      it in the OS keyring.
//   3. Each remote agent is an explicit user opt-in — no network traffic leaves
//      the desktop until `add_remote_agent` is called.

use crate::cloud_ai::{FileClassification, RecoveryPrediction};
use crate::remote::{client, registry};
use crate::types::{
    AiRecoveryBrief, DetectedDevice, ExportProgress, FileHexPreview, FilePreview, RecoveredFile,
    ScanProgress, TechnicalLogEntry,
};
use std::net::{Ipv4Addr, Ipv6Addr};
use url::{Host, Url};

fn fetch_agent(agent_id: &str) -> Result<registry::RemoteAgent, String> {
    registry::get(agent_id).ok_or_else(|| format!("Remote agent `{agent_id}` is not registered."))
}

/// Validate that a remote agent base URL is safe to register.
///
/// Rules:
///   - scheme must be `http://` or `https://`
///   - `http://` is only allowed for loopback hosts (`127.0.0.1`, `::1`,
///     `localhost`) — typical `ssh -L` tunnel setup
///   - any non-loopback `http://` URL is rejected with a clear error to surface
///     the TLS requirement
///   - empty host / malformed URL is rejected
///
/// Returns the trimmed, normalized URL on success.
pub(crate) fn validate_remote_base_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("base_url is required.".into());
    }

    let url = Url::parse(trimmed).map_err(|error| format!("base_url is malformed: {error}"))?;
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err("base_url must start with http:// or https://".into());
    }
    if let Some(raw_host) = raw_host_segment(trimmed) {
        if !raw_host.is_ascii() {
            return Err("base_url host must use an ASCII DNS name.".into());
        }
        if raw_host.ends_with('.') {
            return Err("base_url host must not use a trailing dot.".into());
        }
        if raw_host_uses_ambiguous_ipv4_notation(raw_host) {
            return Err("base_url host must not use ambiguous IPv4 notation.".into());
        }
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("base_url must not include embedded credentials.".into());
    }
    if url.host().is_none() {
        return Err("base_url host is empty.".into());
    }

    let host = url.host().expect("host checked above");
    let loopback = is_loopback_host(host.clone())?;
    if scheme == "http" && !loopback {
        return Err(
            "HTTPS is required for non-loopback remote agents. Use https:// or expose the agent over SSH tunnel to 127.0.0.1.".into(),
        );
    }
    if scheme == "https" && is_private_or_link_local_ip(host) {
        return Err(
            "HTTPS remote agents must not use private or link-local IP literals. Use a trusted DNS name with TLS, or an SSH tunnel to 127.0.0.1.".into(),
        );
    }

    Ok(url.to_string().trim_end_matches('/').to_string())
}

fn is_loopback_host(host: Host<&str>) -> Result<bool, String> {
    match host {
        Host::Domain(domain) => {
            let domain = domain.to_ascii_lowercase();
            if domain.ends_with('.') {
                return Err("base_url host must not use a trailing dot.".into());
            }
            if domain.chars().all(|ch| ch.is_ascii_digit()) {
                return Err("base_url host must not use integer IPv4 notation.".into());
            }
            if !domain.is_ascii() {
                return Err("base_url host must use an ASCII DNS name.".into());
            }
            Ok(domain == "localhost")
        }
        Host::Ipv4(addr) => Ok(addr.is_loopback()),
        Host::Ipv6(addr) => {
            if addr.to_ipv4_mapped().is_some() {
                return Err("base_url host must not use IPv6-mapped IPv4 literals.".into());
            }
            Ok(addr.is_loopback())
        }
    }
}

fn raw_host_segment(raw: &str) -> Option<&str> {
    let authority = raw.split_once("://")?.1;
    let authority_end = authority.find(['/', '?', '#']).unwrap_or(authority.len());
    let authority = &authority[..authority_end];
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);

    if host_port.starts_with('[') {
        let end = host_port.find(']')?;
        Some(&host_port[..=end])
    } else {
        let host_end = host_port.find(':').unwrap_or(host_port.len());
        Some(&host_port[..host_end])
    }
}

fn raw_host_uses_ambiguous_ipv4_notation(raw_host: &str) -> bool {
    if raw_host.starts_with('[') {
        return false;
    }
    let host = raw_host.to_ascii_lowercase();
    if host.chars().all(|ch| ch.is_ascii_digit()) {
        return true;
    }

    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() <= 1 || !parts.iter().all(|part| is_ipv4_numberish_part(part)) {
        return false;
    }
    parts.len() != 4
        || parts
            .iter()
            .any(|part| part.starts_with("0x") || (part.len() > 1 && part.starts_with('0')))
}

fn is_ipv4_numberish_part(part: &str) -> bool {
    if part.is_empty() {
        return false;
    }
    if let Some(hex) = part.strip_prefix("0x") {
        return !hex.is_empty() && hex.chars().all(|ch| ch.is_ascii_hexdigit());
    }
    part.chars().all(|ch| ch.is_ascii_digit())
}

fn is_private_or_link_local_ip(host: Host<&str>) -> bool {
    match host {
        Host::Ipv4(addr) => is_private_or_link_local_v4(addr),
        Host::Ipv6(addr) => is_private_or_link_local_v6(addr),
        Host::Domain(_) => false,
    }
}

fn is_private_or_link_local_v4(addr: Ipv4Addr) -> bool {
    addr.is_private() || addr.is_link_local() || addr.is_loopback() || addr.is_unspecified()
}

fn is_private_or_link_local_v6(addr: Ipv6Addr) -> bool {
    let segments = addr.segments();
    let unique_local = (segments[0] & 0xfe00) == 0xfc00;
    let link_local = (segments[0] & 0xffc0) == 0xfe80;
    addr.is_loopback() || addr.is_unspecified() || unique_local || link_local
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn add_remote_agent(
    label: String,
    base_url: String,
    token: String,
) -> Result<registry::RemoteAgentSummary, String> {
    let label = label.trim();
    let token = token.trim();
    if label.is_empty() || token.is_empty() {
        return Err("label and token are required.".into());
    }
    let base_url = validate_remote_base_url(&base_url)?;

    let agent = registry::RemoteAgent {
        id: format!("remote-{}", uuid_like()),
        label: label.to_string(),
        base_url: base_url.clone(),
        token: token.to_string(),
    };

    // Probe the agent before persisting it — fail fast on bad URL/token.
    client::health(&agent)
        .map_err(|error| format!("Unable to reach remote agent at {base_url}: {error}"))?;

    let summary = registry::RemoteAgentSummary::from(&agent);
    registry::add(agent)?;
    Ok(summary)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn list_remote_agents() -> Vec<registry::RemoteAgentSummary> {
    registry::list_summaries()
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remove_remote_agent(agent_id: String) -> Result<bool, String> {
    Ok(registry::remove(&agent_id))
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_get_devices(agent_id: String) -> Result<Vec<DetectedDevice>, String> {
    let agent = fetch_agent(&agent_id)?;
    client::list_devices(&agent)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_start_scan(
    agent_id: String,
    device_id: String,
    scan_type: String,
) -> Result<String, String> {
    let agent = fetch_agent(&agent_id)?;
    client::start_scan(&agent, device_id, scan_type)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_get_scan_progress(agent_id: String, scan_id: String) -> Result<ScanProgress, String> {
    let agent = fetch_agent(&agent_id)?;
    client::get_scan_progress(&agent, &scan_id)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_get_results(agent_id: String, scan_id: String) -> Result<Vec<RecoveredFile>, String> {
    let agent = fetch_agent(&agent_id)?;
    client::get_results(&agent, &scan_id)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_get_scan_logs(
    agent_id: String,
    scan_id: String,
) -> Result<Vec<TechnicalLogEntry>, String> {
    let agent = fetch_agent(&agent_id)?;
    client::get_logs(&agent, &scan_id)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_pause_scan(agent_id: String, scan_id: String) -> Result<(), String> {
    let agent = fetch_agent(&agent_id)?;
    client::pause_scan(&agent, &scan_id).map(|_| ())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_resume_scan(agent_id: String, scan_id: String) -> Result<(), String> {
    let agent = fetch_agent(&agent_id)?;
    client::resume_scan(&agent, &scan_id).map(|_| ())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_cancel_scan(agent_id: String, scan_id: String) -> Result<(), String> {
    let agent = fetch_agent(&agent_id)?;
    client::cancel_scan(&agent, &scan_id).map(|_| ())
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_restore(
    agent_id: String,
    scan_id: String,
    destination_path: String,
    selected_file_ids: Vec<String>,
    conflict_strategy: String,
    preserve_structure: bool,
    verify_integrity: bool,
) -> Result<String, String> {
    let agent = fetch_agent(&agent_id)?;
    client::restore(
        &agent,
        &scan_id,
        destination_path,
        selected_file_ids,
        conflict_strategy,
        preserve_structure,
        verify_integrity,
    )
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_get_export_progress(
    agent_id: String,
    export_id: String,
) -> Result<ExportProgress, String> {
    let agent = fetch_agent(&agent_id)?;
    client::get_export_progress(&agent, &export_id)
}

// ---------- Preview ----------

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_get_file_preview(
    agent_id: String,
    scan_id: String,
    file_id: String,
) -> Result<FilePreview, String> {
    let agent = fetch_agent(&agent_id)?;
    client::get_file_preview(&agent, &scan_id, &file_id)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_get_file_hex_preview(
    agent_id: String,
    scan_id: String,
    file_id: String,
    start_offset: u64,
    bytes_to_read: u64,
) -> Result<FileHexPreview, String> {
    crate::commands::validate_hex_preview_request(bytes_to_read)?;
    let agent = fetch_agent(&agent_id)?;
    client::get_file_hex_preview(&agent, &scan_id, &file_id, start_offset, bytes_to_read)
}

/// Materialise a remote media asset locally so the desktop `<video>`/`<audio>`
/// element can play it. Returns the local path on success.
#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_get_file_media_asset(
    agent_id: String,
    scan_id: String,
    file_id: String,
) -> Result<String, String> {
    let agent = fetch_agent(&agent_id)?;
    let remote_path = client::get_file_media_asset(&agent, &scan_id, &file_id)?;
    let local_path = remote_cache_path(&agent_id, &scan_id, &file_id, &remote_path);
    client::download_file(&agent, &remote_path, &local_path, |_, _| {})?;
    Ok(local_path.to_string_lossy().to_string())
}

// ---------- AI heuristics ----------

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_get_scan_ai_brief(
    agent_id: String,
    scan_id: String,
) -> Result<AiRecoveryBrief, String> {
    let agent = fetch_agent(&agent_id)?;
    client::get_scan_ai_brief(&agent, &scan_id)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_classify_scan_files(
    agent_id: String,
    scan_id: String,
) -> Result<Vec<FileClassification>, String> {
    let agent = fetch_agent(&agent_id)?;
    client::classify_scan_files(&agent, &scan_id)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_predict_scan_recovery(
    agent_id: String,
    scan_id: String,
) -> Result<Vec<RecoveryPrediction>, String> {
    let agent = fetch_agent(&agent_id)?;
    client::predict_scan_recovery(&agent, &scan_id)
}

// ---------- Reports ----------
//
// `remote_generate_recovery_report` and `remote_export_results_csv` ask the
// agent to generate the report on the server and then pull the bytes back to
// a local path the user can open. The local destination is provided by the
// caller (typically a save dialog) so the file ends up where the user wants.

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_generate_recovery_report(
    agent_id: String,
    scan_id: String,
    language: String,
    include_file_inventory: bool,
    local_destination_path: String,
) -> Result<String, String> {
    let agent = fetch_agent(&agent_id)?;
    let remote_path =
        client::generate_recovery_report(&agent, &scan_id, language, include_file_inventory)?;
    let local = std::path::PathBuf::from(&local_destination_path);
    client::download_file(&agent, &remote_path, &local, |_, _| {})?;
    Ok(local_destination_path)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_export_results_csv(
    agent_id: String,
    scan_id: String,
    local_destination_path: String,
) -> Result<String, String> {
    let agent = fetch_agent(&agent_id)?;
    let remote_path = client::export_results_csv(&agent, &scan_id)?;
    let local = std::path::PathBuf::from(&local_destination_path);
    client::download_file(&agent, &remote_path, &local, |_, _| {})?;
    Ok(local_destination_path)
}

// ---------- Generic file download ----------
//
// `remote_download_file` is the lower-level entry point used to pull
// agent-generated artifacts. The agent refuses paths outside its controlled
// artifact workspaces. Existing local files are replaced from byte zero so
// stale or attacker-created bytes cannot be appended to trusted output.

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_download_file(
    agent_id: String,
    remote_path: String,
    local_destination_path: String,
) -> Result<u64, String> {
    let agent = fetch_agent(&agent_id)?;
    let local = std::path::PathBuf::from(&local_destination_path);
    client::download_file(&agent, &remote_path, &local, |_, _| {})
}

/// One-shot "pull a recovered file from the remote server to a local path".
/// Server-side: runs a single-file export to a temp directory and returns the
/// resulting path. Desktop-side: streams the bytes back via the agent's
/// generic file endpoint, then asks the agent to delete the temp artefact.
#[cfg_attr(feature = "desktop", tauri::command)]
pub fn remote_pull_recovered_file(
    agent_id: String,
    scan_id: String,
    file_id: String,
    local_destination_path: String,
) -> Result<u64, String> {
    let agent = fetch_agent(&agent_id)?;
    let pulled = client::pull_recovered_file(&agent, &scan_id, &file_id)?;
    let local = std::path::PathBuf::from(&local_destination_path);
    let bytes_written = client::download_file(&agent, &pulled.path, &local, |_, _| {})?;
    client::verify_downloaded_file(&local, pulled.size_bytes, &pulled.sha256)?;
    // Best-effort cleanup of the server-side temp file. We don't surface
    // delete failures to the caller — the file ends up in the agent's temp
    // dir which is wiped on reboot anyway.
    let _ = client::delete_remote_file(&agent, &pulled.path);
    Ok(bytes_written)
}

/// Build a local cache path for a remote media asset, preserving the file
/// extension so the desktop `<video>` element can sniff the type.
fn remote_cache_path(
    agent_id: &str,
    scan_id: &str,
    file_id: &str,
    remote_path: &str,
) -> std::path::PathBuf {
    let extension = std::path::Path::new(remote_path)
        .extension()
        .and_then(|os| os.to_str())
        .unwrap_or("bin");
    let base = std::env::temp_dir()
        .join("recupere-remote-cache")
        .join(agent_id)
        .join(scan_id);
    let _ = std::fs::create_dir_all(&base);
    base.join(format!("{file_id}.{extension}"))
}

/// Cheap, dependency-free pseudo-uuid based on the system clock + thread id.
/// Uniqueness across a process lifetime is sufficient for the in-memory
/// registry; we don't need RFC-4122 randomness here.
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

#[cfg(test)]
mod tests {
    use super::validate_remote_base_url;

    #[test]
    fn accepts_https_urls() {
        assert!(validate_remote_base_url("https://agent.example.com").is_ok());
        assert!(validate_remote_base_url("https://agent.example.com:8443/v1").is_ok());
        assert_eq!(
            validate_remote_base_url("  https://agent.example.com  ").unwrap(),
            "https://agent.example.com"
        );
    }

    #[test]
    fn accepts_http_only_for_loopback() {
        assert_eq!(
            validate_remote_base_url("http://127.0.0.1:7878/").unwrap(),
            "http://127.0.0.1:7878"
        );
        assert!(validate_remote_base_url("http://127.0.0.1:7878").is_ok());
        assert!(validate_remote_base_url("http://localhost:7878").is_ok());
        assert!(validate_remote_base_url("http://[::1]:7878").is_ok());
        assert!(validate_remote_base_url("http://127.1.2.3:7878").is_ok());
    }

    #[test]
    fn rejects_plain_http_for_non_loopback() {
        let err = validate_remote_base_url("http://agent.example.com:7878")
            .expect_err("plain http non-loopback should be rejected");
        assert!(err.contains("HTTPS is required"), "got: {err}");
    }

    #[test]
    fn rejects_plain_http_for_public_ip() {
        let err = validate_remote_base_url("http://203.0.113.5:7878")
            .expect_err("plain http to public IP should be rejected");
        assert!(err.contains("HTTPS is required"), "got: {err}");
    }

    #[test]
    fn rejects_ambiguous_loopback_forms() {
        assert!(validate_remote_base_url("http://localhost.:7878").is_err());
        assert!(validate_remote_base_url("http://2130706433:7878").is_err());
        assert!(validate_remote_base_url("http://0177.0.0.1:7878").is_err());
        assert!(validate_remote_base_url("http://0x7f.0.0.1:7878").is_err());
        assert!(validate_remote_base_url("http://[::ffff:127.0.0.1]:7878").is_err());
    }

    #[test]
    fn rejects_https_private_ip_literals() {
        assert!(validate_remote_base_url("https://10.0.0.2:7878").is_err());
        assert!(validate_remote_base_url("https://[fe80::1]:7878").is_err());
        assert!(validate_remote_base_url("https://agent.example.com").is_ok());
    }

    #[test]
    fn rejects_unknown_scheme() {
        assert!(validate_remote_base_url("ftp://example.com").is_err());
        assert!(validate_remote_base_url("file:///etc/passwd").is_err());
        assert!(validate_remote_base_url("javascript:alert(1)").is_err());
        assert!(validate_remote_base_url("agent.example.com").is_err());
    }

    #[test]
    fn rejects_empty_and_host_less_urls() {
        assert!(validate_remote_base_url("").is_err());
        assert!(validate_remote_base_url("   ").is_err());
        assert!(validate_remote_base_url("https://").is_err());
        assert!(validate_remote_base_url("http:///path").is_err());
    }
}
