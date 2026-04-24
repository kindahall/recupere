// HTTP client for the recupere-agent API.
//
// All methods are blocking — they're invoked from Tauri command handlers
// which already run on a worker thread, so we use `reqwest::blocking` to keep
// the call sites simple and avoid pulling tokio into the desktop crate.

use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::Read;
use std::time::Duration;

use crate::cloud_ai::{FileClassification, RecoveryPrediction};
use crate::remote::registry::RemoteAgent;
use crate::types::{
    AiRecoveryBrief, DetectedDevice, ExportProgress, FileHexPreview, FilePreview, RecoveredFile,
    ScanProgress, TechnicalLogEntry,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

fn build_client(agent: &RemoteAgent) -> Result<Client, String> {
    let mut headers = HeaderMap::new();
    let token_header = HeaderValue::from_str(&format!("Bearer {}", agent.token))
        .map_err(|error| format!("invalid bearer token for agent `{}`: {error}", agent.id))?;
    headers.insert(AUTHORIZATION, token_header);
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    Client::builder()
        .default_headers(headers)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| format!("unable to build remote client: {error}"))
}

fn endpoint(agent: &RemoteAgent, path: &str) -> String {
    format!("{}{}", agent.base_url.trim_end_matches('/'), path)
}

fn get<T: DeserializeOwned>(agent: &RemoteAgent, path: &str) -> Result<T, String> {
    let client = build_client(agent)?;
    let response = client
        .get(endpoint(agent, path))
        .send()
        .map_err(|error| format!("remote agent unreachable: {error}"))?;
    parse_response(response)
}

fn post<B: Serialize, T: DeserializeOwned>(
    agent: &RemoteAgent,
    path: &str,
    body: &B,
) -> Result<T, String> {
    let client = build_client(agent)?;
    let response = client
        .post(endpoint(agent, path))
        .json(body)
        .send()
        .map_err(|error| format!("remote agent unreachable: {error}"))?;
    parse_response(response)
}

fn post_empty<T: DeserializeOwned>(agent: &RemoteAgent, path: &str) -> Result<T, String> {
    let client = build_client(agent)?;
    let response = client
        .post(endpoint(agent, path))
        .send()
        .map_err(|error| format!("remote agent unreachable: {error}"))?;
    parse_response(response)
}

fn parse_response<T: DeserializeOwned>(response: reqwest::blocking::Response) -> Result<T, String> {
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| format!("unable to read remote agent response: {error}"))?;
    if !status.is_success() {
        // The agent returns `{"error": "..."}` on failure — surface that
        // string directly so the desktop app can show it as-is.
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(message) = value.get("error").and_then(|v| v.as_str()) {
                return Err(format!("remote agent error ({status}): {message}"));
            }
        }
        return Err(format!("remote agent error ({status}): {body}"));
    }
    serde_json::from_str(&body)
        .map_err(|error| format!("unable to decode remote agent response: {error}: {body}"))
}

// ---------- Public API ----------

#[derive(serde::Serialize)]
struct StartScanBody {
    device_id: String,
    scan_type: String,
}

#[derive(serde::Deserialize)]
struct StartScanResponse {
    scan_id: String,
}

#[derive(serde::Serialize)]
struct RestoreBody {
    destination_path: String,
    selected_file_ids: Vec<String>,
    conflict_strategy: String,
    preserve_structure: bool,
    verify_integrity: bool,
}

#[derive(serde::Deserialize)]
struct RestoreResponse {
    export_id: String,
}

#[derive(serde::Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

pub fn health(agent: &RemoteAgent) -> Result<HealthResponse, String> {
    get(agent, "/v1/health")
}

pub fn list_devices(agent: &RemoteAgent) -> Result<Vec<DetectedDevice>, String> {
    get(agent, "/v1/devices")
}

pub fn start_scan(
    agent: &RemoteAgent,
    device_id: String,
    scan_type: String,
) -> Result<String, String> {
    let body = StartScanBody {
        device_id,
        scan_type,
    };
    let response: StartScanResponse = post(agent, "/v1/scans", &body)?;
    Ok(response.scan_id)
}

pub fn get_scan_progress(agent: &RemoteAgent, scan_id: &str) -> Result<ScanProgress, String> {
    get(agent, &format!("/v1/scans/{scan_id}/progress"))
}

pub fn get_results(agent: &RemoteAgent, scan_id: &str) -> Result<Vec<RecoveredFile>, String> {
    get(agent, &format!("/v1/scans/{scan_id}/results"))
}

pub fn get_logs(agent: &RemoteAgent, scan_id: &str) -> Result<Vec<TechnicalLogEntry>, String> {
    get(agent, &format!("/v1/scans/{scan_id}/logs"))
}

pub fn pause_scan(agent: &RemoteAgent, scan_id: &str) -> Result<serde_json::Value, String> {
    post_empty(agent, &format!("/v1/scans/{scan_id}/pause"))
}

pub fn resume_scan(agent: &RemoteAgent, scan_id: &str) -> Result<serde_json::Value, String> {
    post_empty(agent, &format!("/v1/scans/{scan_id}/resume"))
}

pub fn cancel_scan(agent: &RemoteAgent, scan_id: &str) -> Result<serde_json::Value, String> {
    post_empty(agent, &format!("/v1/scans/{scan_id}/cancel"))
}

pub fn restore(
    agent: &RemoteAgent,
    scan_id: &str,
    destination_path: String,
    selected_file_ids: Vec<String>,
    conflict_strategy: String,
    preserve_structure: bool,
    verify_integrity: bool,
) -> Result<String, String> {
    let body = RestoreBody {
        destination_path,
        selected_file_ids,
        conflict_strategy,
        preserve_structure,
        verify_integrity,
    };
    let response: RestoreResponse = post(agent, &format!("/v1/scans/{scan_id}/restore"), &body)?;
    Ok(response.export_id)
}

pub fn get_export_progress(agent: &RemoteAgent, export_id: &str) -> Result<ExportProgress, String> {
    get(agent, &format!("/v1/exports/{export_id}/progress"))
}

// ---------- Preview ----------

pub fn get_file_preview(
    agent: &RemoteAgent,
    scan_id: &str,
    file_id: &str,
) -> Result<FilePreview, String> {
    get(
        agent,
        &format!("/v1/scans/{scan_id}/files/{file_id}/preview"),
    )
}

pub fn get_file_hex_preview(
    agent: &RemoteAgent,
    scan_id: &str,
    file_id: &str,
    start_offset: u64,
    bytes_to_read: u64,
) -> Result<FileHexPreview, String> {
    get(
        agent,
        &format!(
            "/v1/scans/{scan_id}/files/{file_id}/hex_preview?start_offset={start_offset}&bytes_to_read={bytes_to_read}"
        ),
    )
}

#[derive(serde::Deserialize)]
struct MediaAssetResponse {
    path: String,
}

pub fn get_file_media_asset(
    agent: &RemoteAgent,
    scan_id: &str,
    file_id: &str,
) -> Result<String, String> {
    let response: MediaAssetResponse = get(
        agent,
        &format!("/v1/scans/{scan_id}/files/{file_id}/media_asset"),
    )?;
    Ok(response.path)
}

// ---------- AI heuristics ----------

pub fn get_scan_ai_brief(agent: &RemoteAgent, scan_id: &str) -> Result<AiRecoveryBrief, String> {
    get(agent, &format!("/v1/scans/{scan_id}/ai_brief"))
}

pub fn classify_scan_files(
    agent: &RemoteAgent,
    scan_id: &str,
) -> Result<Vec<FileClassification>, String> {
    get(agent, &format!("/v1/scans/{scan_id}/classify"))
}

pub fn predict_scan_recovery(
    agent: &RemoteAgent,
    scan_id: &str,
) -> Result<Vec<RecoveryPrediction>, String> {
    get(agent, &format!("/v1/scans/{scan_id}/predict"))
}

// ---------- Reports ----------

#[derive(serde::Serialize)]
struct ReportBody {
    language: String,
    include_file_inventory: bool,
}

#[derive(serde::Deserialize)]
struct PathResponse {
    path: String,
}

pub fn generate_recovery_report(
    agent: &RemoteAgent,
    scan_id: &str,
    language: String,
    include_file_inventory: bool,
) -> Result<String, String> {
    let body = ReportBody {
        language,
        include_file_inventory,
    };
    let response: PathResponse = post(agent, &format!("/v1/scans/{scan_id}/report"), &body)?;
    Ok(response.path)
}

pub fn export_results_csv(agent: &RemoteAgent, scan_id: &str) -> Result<String, String> {
    let response: PathResponse = post_empty(agent, &format!("/v1/scans/{scan_id}/csv"))?;
    Ok(response.path)
}

// ---------- File streaming ----------
//
// Pulls bytes from `GET /v1/files?path=<absolute>` and writes them to
// `local_path`. The agent accepts only paths for artifacts it generated under
// controlled workspaces. Supports resumable downloads through the HTTP
// `Range` header — if the local file already exists, we send
// `Range: bytes=<existing>-` and append.

pub fn download_file<F>(
    agent: &RemoteAgent,
    remote_path: &str,
    local_path: &std::path::Path,
    mut progress: F,
) -> Result<u64, String>
where
    F: FnMut(u64, Option<u64>),
{
    use std::fs::OpenOptions;
    use std::io::Write;

    let client = build_client(agent)?;
    let url = endpoint(agent, &format!("/v1/files?path={}", urlencode(remote_path)));

    let resume_offset = std::fs::metadata(local_path)
        .map(|meta| meta.len())
        .unwrap_or(0);

    let mut request = client.get(&url);
    if resume_offset > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={resume_offset}-"));
    }

    let mut response = request
        .send()
        .map_err(|error| format!("remote agent unreachable: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(format!("remote agent error ({status}): {body}"));
    }

    let total_size = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(|len| resume_offset + len);

    if let Some(parent) = local_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("unable to create download directory: {error}"))?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(resume_offset > 0)
        .write(true)
        .truncate(resume_offset == 0)
        .open(local_path)
        .map_err(|error| format!("unable to open local download path: {error}"))?;

    let mut buffer = [0_u8; 64 * 1024];
    let mut written = resume_offset;
    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|error| format!("remote download read error: {error}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|error| format!("local write error: {error}"))?;
        written = written.saturating_add(read as u64);
        progress(written, total_size);
    }

    file.flush()
        .map_err(|error| format!("local flush error: {error}"))?;
    Ok(written)
}

#[derive(serde::Deserialize)]
pub struct PullResponse {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
}

pub fn pull_recovered_file(
    agent: &RemoteAgent,
    scan_id: &str,
    file_id: &str,
) -> Result<PullResponse, String> {
    post_empty(agent, &format!("/v1/scans/{scan_id}/files/{file_id}/pull"))
}

pub fn delete_remote_file(agent: &RemoteAgent, remote_path: &str) -> Result<(), String> {
    let client = build_client(agent)?;
    let url = endpoint(agent, &format!("/v1/files?path={}", urlencode(remote_path)));
    let response = client
        .delete(&url)
        .send()
        .map_err(|error| format!("remote agent unreachable: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "remote agent error ({}): unable to delete temp file",
            response.status()
        ));
    }
    Ok(())
}

fn urlencode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}
