use crate::{
    imaging,
    types::{
        DetectedDevice, DeviceStatus, DeviceType, FilesystemType, ImportedRecoverySourceStatus,
        RiskLevel,
    },
    virtual_disk,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

const IMPORTED_SOURCE_EXTENSIONS: &[&str] =
    &["img", "dd", "raw", "bin", "e01", "vmdk", "vhd", "vhdx"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedRecoverySourceRecord {
    pub path: String,
    pub display_name: String,
    pub source_format: String,
    pub logical_size_bytes: u64,
}

pub fn list_imported_devices() -> Vec<DetectedDevice> {
    load_records().into_iter().map(record_to_device).collect()
}

pub fn import_recovery_source(source_path: &Path) -> Result<ImportedRecoverySourceRecord, String> {
    let normalized_path = normalize_source_path(source_path)?;
    let extension = normalized_path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !IMPORTED_SOURCE_EXTENSIONS.contains(&extension.as_str()) {
        return Err(format!(
            "Unsupported recovery source `{}`. Supported extensions are: {}.",
            normalized_path.to_string_lossy(),
            IMPORTED_SOURCE_EXTENSIONS.join(", ")
        ));
    }

    if !normalized_path.is_file() {
        return Err(format!(
            "The selected recovery source {} is not a readable file.",
            normalized_path.to_string_lossy()
        ));
    }

    let source = virtual_disk::open_recovery_source(&normalized_path)?;
    let logical_size_bytes = source.total_size().max(
        fs::metadata(&normalized_path)
            .map_err(|error| {
                format!(
                    "Unable to inspect the imported recovery source {}: {error}",
                    normalized_path.to_string_lossy()
                )
            })?
            .len(),
    );
    let source_format = source.format_name().to_ascii_uppercase();
    drop(source);

    let record = ImportedRecoverySourceRecord {
        path: normalized_path.to_string_lossy().to_string(),
        display_name: normalized_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Imported recovery source")
            .to_string(),
        source_format,
        logical_size_bytes,
    };

    let mut records = load_records();
    if let Some(existing) = records
        .iter_mut()
        .find(|existing| existing.path == record.path)
    {
        *existing = record.clone();
    } else {
        records.push(record.clone());
    }
    persist_records(&records)?;

    Ok(record)
}

pub fn remove_imported_recovery_source(source_path: &Path) -> Result<(), String> {
    let normalized_path = normalize_source_path_without_requirements(source_path);
    let normalized_path = normalized_path.to_string_lossy().to_string();
    let mut records = load_records();
    let cache_paths_to_remove: Vec<PathBuf> = records
        .iter()
        .filter(|record| record.path == normalized_path)
        .map(analysis_cache_path_for_record)
        .collect();
    let original_len = records.len();
    records.retain(|record| record.path != normalized_path);
    if records.len() == original_len {
        return Err(format!(
            "The imported recovery source {} is not registered.",
            normalized_path
        ));
    }
    persist_records(&records)?;
    for cache_path in cache_paths_to_remove {
        if cache_path.exists() {
            let _ = fs::remove_file(cache_path);
        }
    }
    Ok(())
}

pub fn resolve_analysis_source_path_if_imported(
    source_path: &Path,
) -> Result<Option<PathBuf>, String> {
    let normalized_path = normalize_source_path_without_requirements(source_path);
    let Some(record) = find_record_for_path(&normalized_path) else {
        return Ok(None);
    };

    ensure_source_available(&record)?;

    if !requires_analysis_normalization(&record) {
        return Ok(Some(PathBuf::from(record.path)));
    }

    Ok(Some(prepare_analysis_cache_for_record(&record)?))
}

pub fn get_imported_source_status(
    source_path: &Path,
) -> Result<Option<ImportedRecoverySourceStatus>, String> {
    let normalized_path = normalize_source_path_without_requirements(source_path);
    let Some(record) = find_record_for_path(&normalized_path) else {
        return Ok(None);
    };

    Ok(Some(status_for_record(&record)))
}

pub fn prepare_imported_source(source_path: &Path) -> Result<ImportedRecoverySourceStatus, String> {
    let normalized_path = normalize_source_path_without_requirements(source_path);
    let record = find_record_for_path(&normalized_path).ok_or_else(|| {
        format!(
            "The imported recovery source {} is not registered.",
            normalized_path.to_string_lossy()
        )
    })?;

    ensure_source_available(&record)?;
    if requires_analysis_normalization(&record) {
        let _ = prepare_analysis_cache_for_record(&record)?;
    }

    Ok(status_for_record(&record))
}

pub fn register_generated_recovery_source(
    source_path: &Path,
    display_name: &str,
    source_format: &str,
    logical_size_bytes: u64,
) -> Result<ImportedRecoverySourceRecord, String> {
    let normalized_path = normalize_source_path(source_path)?;
    if !normalized_path.is_file() {
        return Err(format!(
            "The generated recovery source {} is not a readable file.",
            normalized_path.to_string_lossy()
        ));
    }

    let record = ImportedRecoverySourceRecord {
        path: normalized_path.to_string_lossy().to_string(),
        display_name: display_name.to_string(),
        source_format: source_format.to_string(),
        logical_size_bytes,
    };

    let mut records = load_records();
    if let Some(existing) = records
        .iter_mut()
        .find(|existing| existing.path == record.path)
    {
        *existing = record.clone();
    } else {
        records.push(record.clone());
    }
    persist_records(&records)?;

    Ok(record)
}

pub fn generated_analysis_artifact_dir(category: &str) -> PathBuf {
    app_data_dir().join(category)
}

fn record_to_device(record: ImportedRecoverySourceRecord) -> DetectedDevice {
    let source_exists = Path::new(&record.path).is_file();
    let status = if source_exists {
        DeviceStatus::Healthy
    } else {
        DeviceStatus::Unresponsive
    };
    let risk_level = if source_exists {
        RiskLevel::Low
    } else {
        RiskLevel::High
    };
    let model = if source_exists {
        format!("Imported {} source", record.source_format)
    } else {
        format!("Imported {} source (missing)", record.source_format)
    };

    DetectedDevice {
        id: build_device_id(&record.path),
        name: record.display_name,
        device_path: record.path,
        device_type: DeviceType::Image,
        filesystem: FilesystemType::Unknown,
        capacity_bytes: record.logical_size_bytes,
        used_bytes: 0,
        status,
        risk_level,
        serial: None,
        model: Some(model),
        is_trim_enabled: Some(false),
        is_encrypted: Some(false),
        smart_available: Some(false),
        partitions: Vec::new(),
    }
}

fn build_device_id(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    let digest = hasher.finalize();
    let hex_prefix: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("img-{hex_prefix}")
}

fn source_cache_dir() -> PathBuf {
    if let Some(path) = env::var_os("RECUPERE_IMPORTED_SOURCES_CACHE_DIR") {
        return PathBuf::from(path);
    }

    app_data_dir().join("imported-sources-cache")
}

fn registry_path() -> PathBuf {
    if let Some(path) = env::var_os("RECUPERE_IMPORTED_SOURCES_PATH") {
        return PathBuf::from(path);
    }

    if cfg!(test) {
        return env::temp_dir()
            .join(format!("recupere-test-{}", std::process::id()))
            .join("imported-sources.json");
    }

    app_data_dir().join("imported-sources.json")
}

fn app_data_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        if let Some(home) = user_home_dir() {
            return home.join("Library/Application Support/recupere");
        }
    }

    if cfg!(target_os = "windows") {
        if let Some(app_data) = env::var_os("LOCALAPPDATA").or_else(|| env::var_os("APPDATA")) {
            return PathBuf::from(app_data).join("recupere");
        }
    }

    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(data_home).join("recupere");
    }

    if let Some(home) = user_home_dir() {
        return home.join(".local/share/recupere");
    }

    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".recupere")
}

fn user_home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
}

fn normalize_source_path(source_path: &Path) -> Result<PathBuf, String> {
    let normalized = normalize_source_path_without_requirements(source_path);
    if !normalized.exists() {
        return Err(format!(
            "The selected recovery source {} does not exist.",
            normalized.to_string_lossy()
        ));
    }
    Ok(normalized)
}

fn normalize_source_path_without_requirements(source_path: &Path) -> PathBuf {
    let absolute = if source_path.is_absolute() {
        source_path.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(source_path)
    };
    absolute.canonicalize().unwrap_or(absolute)
}

fn find_record_for_path(source_path: &Path) -> Option<ImportedRecoverySourceRecord> {
    let normalized = normalize_source_path_without_requirements(source_path)
        .to_string_lossy()
        .to_string();
    load_records()
        .into_iter()
        .find(|record| record.path == normalized)
}

fn requires_analysis_normalization(record: &ImportedRecoverySourceRecord) -> bool {
    matches!(
        record.source_format.as_str(),
        "E01" | "VMDK" | "VHD" | "VHDX"
    )
}

fn ensure_source_available(record: &ImportedRecoverySourceRecord) -> Result<(), String> {
    if Path::new(&record.path).is_file() {
        return Ok(());
    }

    Err(format!(
        "The imported recovery source {} is no longer available on disk.",
        record.path
    ))
}

fn analysis_cache_path_for_record(record: &ImportedRecoverySourceRecord) -> PathBuf {
    source_cache_dir().join(format!("{}.img", build_device_id(&record.path)))
}

fn analysis_cache_is_current(record: &ImportedRecoverySourceRecord, cache_path: &Path) -> bool {
    let source_meta = match fs::metadata(&record.path) {
        Ok(meta) => meta,
        Err(_) => return false,
    };
    let cache_meta = match fs::metadata(cache_path) {
        Ok(meta) => meta,
        Err(_) => return false,
    };
    if cache_meta.len() == 0 {
        return false;
    }

    match (source_meta.modified(), cache_meta.modified()) {
        (Ok(source_modified), Ok(cache_modified)) => cache_modified >= source_modified,
        _ => true,
    }
}

fn prepare_analysis_cache_for_record(
    record: &ImportedRecoverySourceRecord,
) -> Result<PathBuf, String> {
    let cache_path = analysis_cache_path_for_record(record);
    if analysis_cache_is_current(record, &cache_path) {
        return Ok(cache_path);
    }

    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Unable to prepare the imported-source cache directory {}: {error}",
                parent.to_string_lossy()
            )
        })?;
    }

    imaging::create_read_only_image_at(&cache_path, Path::new(&record.path), &mut |_| {})
        .map_err(|error| {
            format!(
                "Unable to normalize the imported {} source {} into a local read-only analysis image: {}",
                record.source_format,
                record.path,
                error
            )
        })?;

    Ok(cache_path)
}

fn status_for_record(record: &ImportedRecoverySourceRecord) -> ImportedRecoverySourceStatus {
    let source_available = Path::new(&record.path).is_file();
    let requires_preparation = requires_analysis_normalization(record);
    let cache_path = analysis_cache_path_for_record(record);
    let prepared = source_available
        && (!requires_preparation || analysis_cache_is_current(record, &cache_path));
    let source_format = record.source_format.trim().to_ascii_uppercase();
    let (support_tier, support_note, safer_next_step) = match source_format.as_str() {
        "RAW" | "IMG" | "DD" | "BIN" => (
            "supported",
            "Raw read-only disk image. This format is directly analyzable when the file is available.",
            "Use the imported file directly as the analysis source and keep exports away from the original evidence path.",
        ),
        "E01" => (
            "supported",
            "Forensic evidence container. Recupere normalizes it into a local read-only analysis cache before deeper recovery work.",
            "Prepare the source locally, then continue from the prepared analysis path instead of reopening the evidence container each time.",
        ),
        "VMDK" | "VHD" => (
            "limited",
            "Virtual disk container supported through local read-only normalization. Keep the original container as evidence and treat the cache as the working copy only.",
            "Prepare the source locally before scan/export, then continue from the derived analysis path.",
        ),
        "VHDX" => (
            "unsupported",
            "VHDX is detected but not yet supported by the current virtual-disk reader. Leaving it imported without preparation does not make it analyzable.",
            "Convert the source to VHD, E01, or a raw .img/.dd with a trusted read-only workflow before importing it here.",
        ),
        _ => (
            "limited",
            "Imported analysis source recognized, but this format does not yet have a stronger support guarantee in the current desktop workflow.",
            "Confirm the format in Expert mode and prefer a raw or forensic evidence image when you need the most predictable analysis path.",
        ),
    };
    let analysis_path = if !source_available {
        None
    } else if requires_preparation {
        prepared.then(|| cache_path.to_string_lossy().to_string())
    } else {
        Some(record.path.clone())
    };

    ImportedRecoverySourceStatus {
        display_name: record.display_name.clone(),
        source_path: record.path.clone(),
        source_format: record.source_format.clone(),
        logical_size_bytes: record.logical_size_bytes,
        support_tier: support_tier.into(),
        support_note: support_note.into(),
        safer_next_step: safer_next_step.into(),
        source_available,
        requires_preparation,
        prepared,
        analysis_path,
        cache_path: requires_preparation.then(|| cache_path.to_string_lossy().to_string()),
        cache_size_bytes: fs::metadata(&cache_path).ok().map(|meta| meta.len()),
    }
}

fn load_records() -> Vec<ImportedRecoverySourceRecord> {
    let path = registry_path();
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(_) => return Vec::new(),
    };

    serde_json::from_str::<Vec<ImportedRecoverySourceRecord>>(&contents).unwrap_or_else(|error| {
        tracing::info!(
            "imported_sources: unable to parse {}: {}",
            path.to_string_lossy(),
            error
        );
        Vec::new()
    })
}

fn persist_records(records: &[ImportedRecoverySourceRecord]) -> Result<(), String> {
    let path = registry_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Unable to prepare the imported-source registry directory {}: {error}",
                parent.to_string_lossy()
            )
        })?;
    }

    let contents = serde_json::to_string_pretty(records)
        .map_err(|error| format!("Unable to serialize imported recovery sources: {error}"))?;
    fs::write(&path, contents).map_err(|error| {
        format!(
            "Unable to persist the imported recovery-source registry {}: {error}",
            path.to_string_lossy()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn test_env_guard() -> std::sync::MutexGuard<'static, ()> {
        static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
        GUARD
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("test env guard should not be poisoned")
    }

    #[test]
    fn import_and_remove_recovery_source_round_trip() {
        let _guard = test_env_guard();
        let root = env::temp_dir().join(format!(
            "recupere-imported-source-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("temp root should exist");
        let registry_path = root.join("imported-sources.json");
        let cache_dir = root.join("cache");
        let source_path = root.join("disk.raw");
        fs::write(&source_path, b"0123456789").expect("source should be written");

        env::set_var("RECUPERE_IMPORTED_SOURCES_PATH", &registry_path);
        env::set_var("RECUPERE_IMPORTED_SOURCES_CACHE_DIR", &cache_dir);

        let imported = import_recovery_source(&source_path).expect("import should succeed");
        assert_eq!(imported.source_format, "RAW");
        assert_eq!(imported.logical_size_bytes, 10);

        let devices = list_imported_devices();
        assert_eq!(devices.len(), 1);
        assert!(matches!(devices[0].device_type, DeviceType::Image));
        assert_eq!(devices[0].device_path, imported.path);

        let status = get_imported_source_status(&source_path)
            .expect("status lookup should succeed")
            .expect("status should exist");
        assert!(status.source_available);
        assert!(!status.requires_preparation);
        assert!(status.prepared);
        assert_eq!(status.support_tier, "supported");
        assert_eq!(
            status.analysis_path.as_deref(),
            Some(imported.path.as_str())
        );

        remove_imported_recovery_source(&source_path).expect("remove should succeed");
        assert!(list_imported_devices().is_empty());

        env::remove_var("RECUPERE_IMPORTED_SOURCES_PATH");
        env::remove_var("RECUPERE_IMPORTED_SOURCES_CACHE_DIR");
        let _ = fs::remove_file(source_path);
        let _ = fs::remove_file(registry_path);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_analysis_source_path_materializes_virtual_sources_into_local_cache() {
        let _guard = test_env_guard();
        let root = env::temp_dir().join(format!(
            "recupere-imported-source-cache-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("temp root should exist");
        let registry_path = root.join("imported-sources.json");
        let cache_dir = root.join("cache");
        let source_path = root.join("disk.vhd");
        let payload = b"VHD-CACHE-PAYLOAD";

        let mut bytes = Vec::with_capacity(payload.len() + 512);
        bytes.extend_from_slice(payload);
        let mut footer = [0_u8; 512];
        footer[0..8].copy_from_slice(b"conectix");
        footer[16..24].copy_from_slice(&u64::MAX.to_be_bytes());
        footer[48..56].copy_from_slice(&(payload.len() as u64).to_be_bytes());
        footer[60..64].copy_from_slice(&2_u32.to_be_bytes());
        bytes.extend_from_slice(&footer);
        fs::write(&source_path, bytes).expect("synthetic VHD should be written");

        env::set_var("RECUPERE_IMPORTED_SOURCES_PATH", &registry_path);
        env::set_var("RECUPERE_IMPORTED_SOURCES_CACHE_DIR", &cache_dir);

        let imported = import_recovery_source(&source_path).expect("import should succeed");
        let status_before = get_imported_source_status(&source_path)
            .expect("status lookup should succeed")
            .expect("status should exist");
        assert!(status_before.requires_preparation);
        assert!(!status_before.prepared);
        assert_eq!(status_before.support_tier, "limited");

        let analysis_path = resolve_analysis_source_path_if_imported(Path::new(&imported.path))
            .expect("analysis path should resolve")
            .expect("imported source should be recognized");

        assert!(analysis_path.starts_with(&cache_dir));
        assert_eq!(
            fs::read(&analysis_path).expect("cache should exist"),
            payload
        );

        let status_after = prepare_imported_source(&source_path).expect("prepare should succeed");
        assert!(status_after.prepared);
        assert_eq!(status_after.support_tier, "limited");
        assert_eq!(
            status_after.analysis_path.as_deref(),
            Some(analysis_path.to_string_lossy().as_ref())
        );

        env::remove_var("RECUPERE_IMPORTED_SOURCES_PATH");
        env::remove_var("RECUPERE_IMPORTED_SOURCES_CACHE_DIR");
        let _ = fs::remove_dir_all(root);
    }
}
