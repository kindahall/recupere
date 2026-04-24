// HTTP surface for the recupere-agent.
//
// V1 endpoints:
//   GET  /v1/health                       — public liveness probe
//   GET  /v1/devices                      — auth-gated; engine device list
//   POST /v1/scans                        — auth-gated; start a scan
//                                            body: {"device_id": "...", "scan_type": "..."}
//   GET  /v1/scans/:id/progress           — auth-gated; ScanProgress snapshot
//   GET  /v1/scans/:id/results            — auth-gated; recovered files
//   GET  /v1/scans/:id/logs               — auth-gated; technical log entries
//   POST /v1/scans/:id/pause              — auth-gated
//   POST /v1/scans/:id/resume             — auth-gated
//   POST /v1/scans/:id/cancel             — auth-gated
//
// All authenticated endpoints expect `Authorization: Bearer <token>`. The
// token is checked against the SHA-256 hash stored by `recupere-agent init`.
//
// All scan engine calls go through `tokio::task::spawn_blocking` because the
// underlying functions are synchronous and can be long-running.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, SeekFrom};
use std::net::SocketAddr;
use std::path::{Path as StdPath, PathBuf};
use std::sync::Arc;
use tokio_util::io::ReaderStream;
use tower_http::trace::TraceLayer;

use crate::auth::TokenStore;

#[derive(Clone)]
struct AppState {
    tokens: Arc<TokenStore>,
}

pub async fn serve(bind: SocketAddr, state_dir: PathBuf) -> Result<(), String> {
    let tokens = TokenStore::load(&state_dir)?;
    let state = AppState {
        tokens: Arc::new(tokens),
    };

    if !is_loopback(&bind) {
        tracing::warn!(
            %bind,
            "binding to a non-loopback address — exposing the recovery engine to the network. \
             Strongly prefer 127.0.0.1 + `ssh -L` for V1."
        );
    }

    let protected = Router::new()
        .route("/v1/devices", get(list_devices))
        .route("/v1/scans", post(start_scan))
        .route("/v1/scans/:id/progress", get(get_scan_progress))
        .route("/v1/scans/:id/results", get(get_scan_results))
        .route("/v1/scans/:id/logs", get(get_scan_logs))
        .route("/v1/scans/:id/pause", post(pause_scan))
        .route("/v1/scans/:id/resume", post(resume_scan))
        .route("/v1/scans/:id/cancel", post(cancel_scan))
        .route("/v1/scans/:id/restore", post(restore_files))
        .route("/v1/exports/:id/progress", get(get_export_progress))
        // Preview / hex / media — used by the desktop ResultsPage panels.
        .route(
            "/v1/scans/:scan_id/files/:file_id/preview",
            get(file_preview),
        )
        .route(
            "/v1/scans/:scan_id/files/:file_id/hex_preview",
            get(file_hex_preview),
        )
        .route(
            "/v1/scans/:scan_id/files/:file_id/media_asset",
            get(file_media_asset),
        )
        // AI heuristics that don't write to disk.
        .route("/v1/scans/:id/ai_brief", get(scan_ai_brief))
        .route("/v1/scans/:id/classify", get(scan_classify))
        .route("/v1/scans/:id/predict", get(scan_predict))
        // Reports — generated server-side, returned as a path the desktop can
        // then download via /v1/files (the generic streaming endpoint below).
        .route("/v1/scans/:id/report", post(scan_report))
        .route("/v1/scans/:id/csv", post(scan_csv))
        // Artifact streaming with optional Range — restricted to files that
        // the agent generated under controlled temp workspaces.
        .route("/v1/files", get(stream_file).delete(delete_file))
        // One-shot "pull a recovered file" — runs a single-file export to a
        // temp directory on the server and returns the resulting path so the
        // desktop can stream it back via /v1/files.
        .route(
            "/v1/scans/:scan_id/files/:file_id/pull",
            post(pull_recovered_file),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_bearer,
        ));

    let app = Router::new()
        .route("/v1/health", get(health))
        .merge(protected)
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    tracing::info!(%bind, "recupere-agent listening");

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|error| format!("unable to bind {bind}: {error}"))?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| format!("server crashed: {error}"))?;
    Ok(())
}

fn is_loopback(addr: &SocketAddr) -> bool {
    addr.ip().is_loopback()
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown requested, stopping recupere-agent");
}

async fn require_bearer(
    State(state): State<AppState>,
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let header_value = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let presented = header_value
        .strip_prefix("Bearer ")
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if !state.tokens.verify(presented) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
}

// ---------- Health ----------

#[derive(serde::Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

// ---------- Engine call helpers ----------
//
// Wraps a sync engine call in `spawn_blocking` and turns its `Result<T,
// String>` into an HTTP `Response`. Internal join failures map to 500;
// engine errors map to 400 with the engine message in the body.

async fn run_blocking<T, F>(operation: F) -> Result<T, (StatusCode, String)>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    match tokio::task::spawn_blocking(operation).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err((StatusCode::BAD_REQUEST, error)),
        Err(join_error) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("agent task crashed: {join_error}"),
        )),
    }
}

fn into_response<T: serde::Serialize>(result: Result<T, (StatusCode, String)>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err((code, message)) => {
            (code, Json(serde_json::json!({ "error": message }))).into_response()
        }
    }
}

// ---------- Devices ----------

async fn list_devices() -> Response {
    match tokio::task::spawn_blocking(recupere_lib::commands::get_devices).await {
        Ok(devices) => Json(devices).into_response(),
        Err(join_error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("agent task crashed: {join_error}")
            })),
        )
            .into_response(),
    }
}

// ---------- Scans ----------

#[derive(Deserialize)]
struct StartScanBody {
    device_id: String,
    scan_type: String,
}

#[derive(serde::Serialize)]
struct StartScanResponse {
    scan_id: String,
}

async fn start_scan(Json(body): Json<StartScanBody>) -> Response {
    let result = run_blocking(move || {
        recupere_lib::commands::start_scan(body.device_id, body.scan_type)
            .map(|scan_id| StartScanResponse { scan_id })
    })
    .await;
    into_response(result)
}

async fn get_scan_progress(Path(scan_id): Path<String>) -> Response {
    let result = run_blocking(move || recupere_lib::commands::get_scan_progress(scan_id)).await;
    into_response(result)
}

async fn get_scan_results(Path(scan_id): Path<String>) -> Response {
    let result = run_blocking(move || recupere_lib::commands::get_results(scan_id)).await;
    into_response(result)
}

async fn get_scan_logs(Path(scan_id): Path<String>) -> Response {
    let result = run_blocking(move || recupere_lib::commands::get_scan_logs(scan_id)).await;
    into_response(result)
}

async fn pause_scan(Path(scan_id): Path<String>) -> Response {
    let result = run_blocking(move || recupere_lib::commands::pause_scan(scan_id)).await;
    into_response(result.map(|()| serde_json::json!({"status": "paused"})))
}

async fn resume_scan(Path(scan_id): Path<String>) -> Response {
    let result = run_blocking(move || recupere_lib::commands::resume_scan(scan_id)).await;
    into_response(result.map(|()| serde_json::json!({"status": "resumed"})))
}

async fn cancel_scan(Path(scan_id): Path<String>) -> Response {
    let result = run_blocking(move || recupere_lib::commands::cancel_scan(scan_id)).await;
    into_response(result.map(|()| serde_json::json!({"status": "cancelled"})))
}

// ---------- Restore (server-side export) ----------
//
// V1 restore is a wrapper around the existing export pipeline. The agent only
// supports restoration to a path on the *server* — no streaming back to the
// desktop client. The destination path must be writable by the agent process.

#[derive(Deserialize)]
struct RestoreBody {
    /// Absolute path on the server where files will be written.
    destination_path: String,
    /// Selected recovered file ids. Empty means "all results in the scan".
    #[serde(default)]
    selected_file_ids: Vec<String>,
    /// "skip" | "overwrite" | "rename". Defaults to "rename".
    #[serde(default = "default_conflict_strategy")]
    conflict_strategy: String,
    #[serde(default = "default_true")]
    preserve_structure: bool,
    #[serde(default = "default_true")]
    verify_integrity: bool,
}

fn default_conflict_strategy() -> String {
    "rename".to_string()
}
fn default_true() -> bool {
    true
}

#[derive(serde::Serialize)]
struct RestoreResponse {
    export_id: String,
}

async fn restore_files(Path(scan_id): Path<String>, Json(body): Json<RestoreBody>) -> Response {
    let result = run_blocking(move || {
        recupere_lib::commands::start_export(
            scan_id,
            body.destination_path,
            body.selected_file_ids,
            body.conflict_strategy,
            body.preserve_structure,
            body.verify_integrity,
            true,
            None,
        )
        .map(|export_id| RestoreResponse { export_id })
    })
    .await;
    into_response(result)
}

async fn get_export_progress(Path(export_id): Path<String>) -> Response {
    let result = run_blocking(move || recupere_lib::commands::get_export_progress(export_id)).await;
    into_response(result)
}

// ---------- Preview ----------

async fn file_preview(Path((scan_id, file_id)): Path<(String, String)>) -> Response {
    let result =
        run_blocking(move || recupere_lib::commands::get_file_preview(scan_id, file_id)).await;
    into_response(result)
}

#[derive(Deserialize)]
struct HexPreviewQuery {
    #[serde(default)]
    start_offset: u64,
    #[serde(default = "default_hex_bytes")]
    bytes_to_read: u64,
}

fn default_hex_bytes() -> u64 {
    4096
}

async fn file_hex_preview(
    Path((scan_id, file_id)): Path<(String, String)>,
    Query(query): Query<HexPreviewQuery>,
) -> Response {
    let result = run_blocking(move || {
        recupere_lib::commands::get_file_hex_preview(
            scan_id,
            file_id,
            query.start_offset,
            query.bytes_to_read,
        )
    })
    .await;
    into_response(result)
}

#[derive(serde::Serialize)]
struct MediaAssetResponse {
    /// Absolute path on the server. The desktop fetches the bytes via
    /// `GET /v1/files?path=<this>`.
    path: String,
}

async fn file_media_asset(Path((scan_id, file_id)): Path<(String, String)>) -> Response {
    let result = run_blocking(move || {
        recupere_lib::commands::get_file_media_asset(scan_id, file_id)
            .map(|path| MediaAssetResponse { path })
    })
    .await;
    into_response(result)
}

// ---------- AI heuristics ----------

async fn scan_ai_brief(Path(scan_id): Path<String>) -> Response {
    let result = run_blocking(move || recupere_lib::commands::get_scan_ai_brief(scan_id)).await;
    into_response(result)
}

async fn scan_classify(Path(scan_id): Path<String>) -> Response {
    let result = run_blocking(move || recupere_lib::commands::classify_scan_files(scan_id)).await;
    into_response(result)
}

async fn scan_predict(Path(scan_id): Path<String>) -> Response {
    let result = run_blocking(move || recupere_lib::commands::predict_scan_recovery(scan_id)).await;
    into_response(result)
}

// ---------- Reports ----------
//
// Reports are generated *on the server*. The endpoint returns the absolute
// path of the artifact; the desktop then pulls it via `/v1/files`. This keeps
// the report generation pipeline identical to local mode (writes to the
// `RECUPERE_WORKSPACE_PATH` workspace) and avoids ballooning report bytes
// inside JSON responses.

#[derive(Deserialize)]
struct ReportBody {
    #[serde(default = "default_report_language")]
    language: String,
    #[serde(default = "default_true")]
    include_file_inventory: bool,
}

fn default_report_language() -> String {
    "en".to_string()
}

#[derive(serde::Serialize)]
struct PathResponse {
    path: String,
}

async fn scan_report(Path(scan_id): Path<String>, Json(body): Json<ReportBody>) -> Response {
    let result = run_blocking(move || {
        recupere_lib::commands::generate_recovery_report(
            scan_id,
            body.language,
            body.include_file_inventory,
        )
        .map(|path| PathResponse { path })
    })
    .await;
    into_response(result)
}

async fn scan_csv(Path(scan_id): Path<String>) -> Response {
    let result = run_blocking(move || {
        recupere_lib::commands::export_results_csv(scan_id).map(|path| PathResponse { path })
    })
    .await;
    into_response(result)
}

// ---------- Controlled artifact streaming ----------
//
// `GET /v1/files?path=<absolute>` only returns bytes for files generated by
// the agent under controlled temp workspaces. This keeps the V1 wire contract
// compatible while avoiding a bearer-token-to-arbitrary-file-read primitive.

#[derive(Deserialize)]
struct StreamQuery {
    path: String,
}

async fn stream_file(
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let path = validated_stream_path(&query.path)?;
    let metadata = match tokio::fs::metadata(&path).await {
        Ok(meta) => meta,
        Err(error) => return Err((StatusCode::NOT_FOUND, format!("file not found: {error}"))),
    };
    if !metadata.is_file() {
        return Err((
            StatusCode::BAD_REQUEST,
            "path does not point to a regular file".into(),
        ));
    }
    let total_size = metadata.len();

    // Parse Range header (single byte range only — V1 doesn't need multi-range).
    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| parse_byte_range(value, total_size));

    let (start, end) = range.unwrap_or((0, total_size.saturating_sub(1)));
    if start > end || end >= total_size {
        return Err((
            StatusCode::RANGE_NOT_SATISFIABLE,
            format!("invalid range {start}-{end} for file of size {total_size}"),
        ));
    }
    let length = end - start + 1;

    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(error) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("unable to open file: {error}"),
            ))
        }
    };

    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let mut file = file;
    if start > 0 {
        if let Err(error) = file.seek(SeekFrom::Start(start)).await {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("seek failed: {error}"),
            ));
        }
    }

    let limited = file.take(length);
    let stream = ReaderStream::new(limited);
    let body = Body::from_stream(stream);

    let mut response_headers = HashMap::new();
    response_headers.insert("content-length", length.to_string());
    response_headers.insert("accept-ranges", "bytes".to_string());
    if range.is_some() {
        response_headers.insert("content-range", format!("bytes {start}-{end}/{total_size}"));
    }

    let mut response = Response::builder()
        .status(if range.is_some() {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(header::CONTENT_TYPE, "application/octet-stream");
    for (key, value) in response_headers {
        if let Ok(header_value) = HeaderValue::from_str(&value) {
            response = response.header(key, header_value);
        }
    }
    Ok(response
        .body(body)
        .unwrap_or_else(|_| Response::new(Body::empty())))
}

async fn delete_file(Query(query): Query<StreamQuery>) -> Response {
    let path = match validated_delete_path(&query.path) {
        Ok(path) => path,
        Err((code, message)) => {
            return (code, Json(serde_json::json!({ "error": message }))).into_response();
        }
    };
    match tokio::task::spawn_blocking(move || std::fs::remove_file(&path)).await {
        Ok(Ok(())) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "deleted"})),
        )
            .into_response(),
        Ok(Err(error)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("agent task crashed: {error}") })),
        )
            .into_response(),
    }
}

fn validated_stream_path(raw_path: &str) -> Result<PathBuf, (StatusCode, String)> {
    let path = canonical_regular_file(raw_path)?;
    if controlled_artifact_roots()
        .iter()
        .any(|root| path.starts_with(root))
    {
        return Ok(path);
    }
    Err((
        StatusCode::FORBIDDEN,
        "file streaming is restricted to agent-generated artifacts".into(),
    ))
}

fn validated_delete_path(raw_path: &str) -> Result<PathBuf, (StatusCode, String)> {
    let path = canonical_regular_file(raw_path)?;
    let pull_root = canonical_existing_dir(&std::env::temp_dir().join("recupere-agent-pull"))?;
    if path.starts_with(pull_root) {
        return Ok(path);
    }
    Err((
        StatusCode::FORBIDDEN,
        "remote deletion is restricted to agent pull artifacts".into(),
    ))
}

fn canonical_regular_file(raw_path: &str) -> Result<PathBuf, (StatusCode, String)> {
    let canonical = std::fs::canonicalize(raw_path)
        .map_err(|error| (StatusCode::NOT_FOUND, format!("file not found: {error}")))?;
    let metadata = std::fs::metadata(&canonical)
        .map_err(|error| (StatusCode::NOT_FOUND, format!("file not found: {error}")))?;
    if metadata.is_file() {
        Ok(canonical)
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            "path does not point to a regular file".into(),
        ))
    }
}

fn controlled_artifact_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        std::env::temp_dir().join("recupere").join("reports"),
        std::env::temp_dir()
            .join("recupere-workspace")
            .join("previews"),
        std::env::temp_dir().join("recupere-agent-pull"),
    ];
    if let Some(data_dir) = dirs::data_dir() {
        roots.push(data_dir.join("recupere").join("reports"));
    }
    roots
        .iter()
        .filter_map(|path| canonical_existing_dir(path).ok())
        .collect()
}

fn canonical_existing_dir(path: &StdPath) -> Result<PathBuf, (StatusCode, String)> {
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        (
            StatusCode::FORBIDDEN,
            format!("artifact root unavailable: {error}"),
        )
    })?;
    if canonical.is_dir() {
        Ok(canonical)
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "artifact root is not a directory".into(),
        ))
    }
}

#[derive(serde::Serialize)]
struct PullResponse {
    path: String,
    name: String,
    size_bytes: u64,
    sha256: String,
}

async fn pull_recovered_file(Path((scan_id, file_id)): Path<(String, String)>) -> Response {
    let result = run_blocking(move || pull_recovered_file_blocking(scan_id, file_id)).await;
    into_response(result)
}

fn pull_recovered_file_blocking(scan_id: String, file_id: String) -> Result<PullResponse, String> {
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    // Carve out a fresh temp directory dedicated to this single-file pull so
    // the resulting file is trivially discoverable (it's the only one).
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let temp_dir = std::env::temp_dir()
        .join("recupere-agent-pull")
        .join(format!("{scan_id}-{file_id}-{nanos:x}"));
    std::fs::create_dir_all(&temp_dir)
        .map_err(|error| format!("unable to create pull temp dir: {error}"))?;

    let export_id = recupere_lib::commands::start_export(
        scan_id,
        temp_dir.to_string_lossy().to_string(),
        vec![file_id.clone()],
        "rename".to_string(),
        true,
        true,
        true,
        None,
    )?;

    // Poll until the export session reports a terminal status (or times out).
    let deadline = Instant::now() + Duration::from_secs(60 * 30);
    loop {
        let progress = recupere_lib::commands::get_export_progress(export_id.clone())?;
        match progress.status.as_str() {
            "completed" => break,
            "error" => {
                return Err("server-side export reported an error during pull".into());
            }
            "cancelled" => {
                return Err("server-side export was cancelled during pull".into());
            }
            _ => {}
        }
        if Instant::now() > deadline {
            return Err("server-side pull timed out after 30 minutes".into());
        }
        sleep(Duration::from_millis(250));
    }

    // The temp dir contains exactly one materialized file (single selection).
    // Walk it recursively and grab the first regular file.
    let pulled = first_regular_file(&temp_dir)
        .ok_or_else(|| "server-side export produced no file".to_string())?;
    let metadata = std::fs::metadata(&pulled)
        .map_err(|error| format!("unable to stat pulled file: {error}"))?;
    let sha256 = sha256_file(&pulled)?;
    let name = pulled
        .file_name()
        .and_then(|os| os.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "recovered".to_string());

    Ok(PullResponse {
        path: pulled.to_string_lossy().to_string(),
        name,
        size_bytes: metadata.len(),
        sha256,
    })
}

fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("unable to hash pulled file: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("unable to read pulled file for hashing: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn first_regular_file(root: &std::path::Path) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                return Some(path);
            } else if path.is_dir() {
                stack.push(path);
            }
        }
    }
    None
}

fn parse_byte_range(value: &str, total_size: u64) -> Option<(u64, u64)> {
    let value = value.trim();
    let value = value.strip_prefix("bytes=")?;
    let mut parts = value.splitn(2, '-');
    let start_str = parts.next()?;
    let end_str = parts.next()?;
    if start_str.is_empty() {
        // Suffix range: bytes=-N → last N bytes.
        let suffix: u64 = end_str.parse().ok()?;
        if suffix == 0 || suffix > total_size {
            return None;
        }
        return Some((total_size - suffix, total_size - 1));
    }
    let start: u64 = start_str.parse().ok()?;
    let end: u64 = if end_str.is_empty() {
        total_size.saturating_sub(1)
    } else {
        end_str.parse().ok()?
    };
    Some((start, end))
}
