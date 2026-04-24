use super::imaging_cmd::helpers::imaging_requires_elevation_fallback;
#[cfg(target_os = "macos")]
use super::imaging_cmd::privileged_macos::build_macos_privileged_imager_script;
use super::runtime::{APP_BUNDLE_IDENTIFIER, APP_PRODUCT_NAME};
use super::state::{
    is_scan_cancelled_error, load_persisted_export_archive_from, load_persisted_scan_archive_from,
    scan_control_handle, snapshot_export_record, snapshot_scan_record,
    upsert_persisted_export_record_at, upsert_persisted_scan_record_at, ExportSession,
    InventoryScanSession, LegacyPersistedExportArchive, ScanControl, MAX_SESSION_LOGS,
};
use super::*;
use crate::analyzers::{apfs, ext4, hfsplus, ntfs};
use crate::imaging;
use crate::partitioning;
use crate::privileged_imager;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use zip::ZipArchive;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    env,
    io::{Cursor, Write},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!(
        "recupere-{prefix}-{}-{}",
        std::process::id(),
        unix_timestamp_ms()
    ));
    fs::create_dir_all(&dir).expect("temporary directory should be created");
    dir
}

fn sample_recovered_file() -> RecoveredFile {
    RecoveredFile {
        id: "file-1".into(),
        name: "report.txt".into(),
        path: "/cases/client-a".into(),
        extension: "txt".into(),
        size_bytes: 128,
        created_at: None,
        modified_at: None,
        integrity: "intact".into(),
        recovery_score: 100,
        recovery_method: "filesystem".into(),
        preview_available: true,
        mime_type: Some("text/plain".into()),
        expected_size_bytes: Some(128),
        deleted_at: None,
        start_offset: None,
        clusters: None,
        byte_runs: None,
        resource_fork: None,
        alternate_data_streams: None,
        source_image_path: None,
        is_deleted: false,
        ..Default::default()
    }
}

fn sample_detected_device_with_path(device_path: &Path) -> DetectedDevice {
    DetectedDevice {
        id: "dev-test".into(),
        name: "Test Device".into(),
        device_path: device_path.to_string_lossy().to_string(),
        device_type: DeviceType::Usb,
        filesystem: FilesystemType::Fat32,
        capacity_bytes: 1024,
        used_bytes: 512,
        status: DeviceStatus::Healthy,
        risk_level: RiskLevel::Low,
        serial: None,
        model: None,
        is_trim_enabled: Some(false),
        is_encrypted: Some(false),
        smart_available: Some(false),
        partitions: vec![Partition {
            id: "part-test".into(),
            label: "Test Volume".into(),
            filesystem: FilesystemType::Fat32,
            size_bytes: 1024,
            start_offset: 0,
            mount_path: Some(device_path.to_string_lossy().to_string()),
            is_mounted: true,
            is_bootable: false,
        }],
    }
}

fn build_zip_bytes(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    let mut archive = ZipWriter::new(&mut cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    for (path, content) in entries {
        archive
            .start_file(path, options)
            .expect("zip entry should start");
        archive
            .write_all(content.as_bytes())
            .expect("zip entry bytes should be written");
    }

    archive.finish().expect("zip archive should finish");
    cursor.into_inner()
}

#[test]
fn export_p0_benchmark_fixtures_to_env_dir() {
    let Some(output_dir) = env::var_os("RECUPERE_BENCHMARK_FIXTURES_DIR") else {
        return;
    };

    let output_dir = PathBuf::from(output_dir);
    fs::create_dir_all(&output_dir).expect("benchmark fixture output directory should exist");

    let fixtures = [
        (
            "ntfs_deleted_basic_v1.img",
            ntfs::synthetic_deleted_ntfs_image(),
        ),
        (
            "ntfs_compressed_deleted_v1.img",
            ntfs::synthetic_compressed_deleted_ntfs_image(),
        ),
        (
            "ext4_deleted_indexed_extent_v1.img",
            ext4::synthetic_deleted_ext4_indexed_extent_image_for_tests(
                b"hello indexed ext4",
                false,
            ),
        ),
        ("signature_carving_jpeg_v1.img", jpeg_signature_image()),
    ];

    for (file_name, bytes) in fixtures {
        fs::write(output_dir.join(file_name), bytes).expect("benchmark fixture should be written");
    }
}

fn sample_scan_session() -> Arc<Mutex<InventoryScanSession>> {
    Arc::new(Mutex::new(InventoryScanSession {
        id: "scan-123".into(),
        device_id: "disk-1".into(),
        device_name: "Case Disk".into(),
        scan_type: "deep".into(),
        root_path: "/Volumes/CaseDisk".into(),
        started_at_ms: 100,
        completed_at_ms: Some(200),
        imaging_profile: None,
        imaging_profile_reason_key: None,
        progress: ScanProgress {
            status: "completed".into(),
            stage: "finalizing".into(),
            percent_complete: 100.0,
            bytes_scanned: 4096,
            total_bytes: 4096,
            files_found: 12,
            errors_count: 1,
            elapsed_seconds: 42,
            resume_from_bytes: 0,
            unreadable_ranges_count: 0,
            unreadable_bytes: 0,
            rescued_after_retry_bytes: 0,
            retry_passes_completed: 0,
            unreadable_ranges: Vec::new(),
        },
        imaging_unreadable_ranges: Vec::new(),
        imaging_rescued_after_retry_bytes: 0,
        imaging_retry_passes_completed: 0,
        results: vec![sample_recovered_file()],
        logs: Vec::new(),
        control: Arc::new(ScanControl::default()),
    }))
}

fn scan_session_for_root(
    root_path: &Path,
    scan_type: &str,
    total_bytes: u64,
) -> Arc<Mutex<InventoryScanSession>> {
    Arc::new(Mutex::new(InventoryScanSession {
        id: format!("scan-test-{}", unix_timestamp_ms()),
        device_id: "disk-temp".into(),
        device_name: "Temp Scan Root".into(),
        scan_type: scan_type.into(),
        root_path: root_path.to_string_lossy().to_string(),
        started_at_ms: unix_timestamp_ms(),
        completed_at_ms: None,
        imaging_profile: None,
        imaging_profile_reason_key: None,
        progress: ScanProgress {
            status: "preparing".into(),
            stage: "initializing".into(),
            percent_complete: 0.0,
            bytes_scanned: 0,
            total_bytes,
            files_found: 0,
            errors_count: 0,
            elapsed_seconds: 0,
            resume_from_bytes: 0,
            unreadable_ranges_count: 0,
            unreadable_bytes: 0,
            rescued_after_retry_bytes: 0,
            retry_passes_completed: 0,
            unreadable_ranges: Vec::new(),
        },
        imaging_unreadable_ranges: Vec::new(),
        imaging_rescued_after_retry_bytes: 0,
        imaging_retry_passes_completed: 0,
        results: Vec::new(),
        logs: Vec::new(),
        control: Arc::new(ScanControl::default()),
    }))
}

fn register_scan_session_for_test(session: &Arc<Mutex<InventoryScanSession>>) -> String {
    let session_id = {
        let state = crate::commands::state::lock_or_recover(session, "scan session");
        state.id.clone()
    };
    scan_sessions()
        .lock()
        .expect("scan session registry lock poisoned")
        .insert(session_id.clone(), Arc::clone(session));
    session_id
}

fn register_export_session_for_test(session: &Arc<Mutex<ExportSession>>) -> String {
    let export_id = {
        let state = session.lock().expect("export session lock poisoned");
        state.id.clone()
    };
    export_sessions()
        .lock()
        .expect("export session registry lock poisoned")
        .insert(export_id.clone(), Arc::clone(session));
    export_id
}

fn export_session_for_destination(
    scan_id: &str,
    destination_path: &Path,
    total_files: usize,
    total_bytes: u64,
) -> Arc<Mutex<ExportSession>> {
    Arc::new(Mutex::new(ExportSession {
        id: format!("export-test-{}", unix_timestamp_ms()),
        scan_id: scan_id.into(),
        destination_path: destination_path.to_string_lossy().to_string(),
        started_at_ms: unix_timestamp_ms(),
        completed_at_ms: None,
        explicit_selection: false,
        implicit_preview_first_excluded_count: 0,
        progress: ExportProgress {
            total_files: total_files as u32,
            exported_files: 0,
            total_bytes,
            exported_bytes: 0,
            current_file: String::new(),
            errors: Vec::new(),
            status: "preparing".into(),
        },
        logs: Vec::new(),
    }))
}

fn sample_export_session() -> Arc<Mutex<ExportSession>> {
    Arc::new(Mutex::new(ExportSession {
        id: "export-123".into(),
        scan_id: "scan-123".into(),
        destination_path: "/Volumes/Recovery/export".into(),
        started_at_ms: 400,
        completed_at_ms: Some(500),
        explicit_selection: false,
        implicit_preview_first_excluded_count: 2,
        progress: ExportProgress {
            total_files: 3,
            exported_files: 2,
            total_bytes: 2048,
            exported_bytes: 1024,
            current_file: String::new(),
            errors: vec![ExportError {
                file_id: "file-9".into(),
                file_name: "missing.bin".into(),
                reason: "copy failed".into(),
            }],
            status: "completed".into(),
        },
        logs: vec![TechnicalLogEntry {
            timestamp_ms: 450,
            level: "info".into(),
            message: "Copied report.txt successfully.".into(),
        }],
    }))
}

#[test]
fn pause_resume_and_cancel_scan_update_session_status() {
    let session = sample_scan_session();
    {
        let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
        state.progress.status = "scanning".into();
        state.completed_at_ms = None;
        state.logs.clear();
    }
    let session_id = register_scan_session_for_test(&session);

    pause_scan(session_id.clone()).expect("active scan should pause");
    {
        let state = crate::commands::state::lock_or_recover(&session, "scan session");
        assert_eq!(state.progress.status, "paused");
        assert!(state
            .logs
            .iter()
            .any(|log| log.message.contains("Scan paused by user request")));
    }

    resume_scan(session_id.clone()).expect("paused scan should resume");
    {
        let state = crate::commands::state::lock_or_recover(&session, "scan session");
        assert_eq!(state.progress.status, "scanning");
        assert!(state
            .logs
            .iter()
            .any(|log| log.message.contains("Scan resumed by user request")));
    }

    cancel_scan(session_id.clone()).expect("active scan should cancel");
    {
        let state = crate::commands::state::lock_or_recover(&session, "scan session");
        assert_eq!(state.progress.status, "cancelled");
        assert!(state.completed_at_ms.is_some());
        assert!(state
            .logs
            .iter()
            .any(|log| log.message.contains("Scan canceled at user request")));
    }
}

#[test]
fn wait_for_scan_permission_respects_cancelled_control() {
    let session = sample_scan_session();
    {
        let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
        state.progress.status = "scanning".into();
    }
    let control = scan_control_handle(&session);
    {
        let mut control_state = control.state.lock().expect("scan control lock poisoned");
        control_state.cancelled = true;
    }

    let result = wait_for_scan_permission(&session);
    assert!(result.is_err());
    assert!(is_scan_cancelled_error(
        &result.expect_err("cancelled control should error")
    ));
}

fn minimal_deleted_fat32_image() -> Vec<u8> {
    let mut image = vec![0_u8; 512 * 6];

    image[11..13].copy_from_slice(&512_u16.to_le_bytes());
    image[13] = 1;
    image[14..16].copy_from_slice(&1_u16.to_le_bytes());
    image[16] = 1;
    image[32..36].copy_from_slice(&6_u32.to_le_bytes());
    image[36..40].copy_from_slice(&1_u32.to_le_bytes());
    image[44..48].copy_from_slice(&2_u32.to_le_bytes());
    image[82..90].copy_from_slice(b"FAT32   ");
    image[510] = 0x55;
    image[511] = 0xaa;

    let fat_offset = 512;
    image[fat_offset..fat_offset + 4].copy_from_slice(&0x0fff_fff8_u32.to_le_bytes());
    image[fat_offset + 4..fat_offset + 8].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());
    image[fat_offset + 8..fat_offset + 12].copy_from_slice(&0x0fff_ffff_u32.to_le_bytes());
    image[fat_offset + 12..fat_offset + 16].copy_from_slice(&0x0fff_ffff_u32.to_le_bytes());
    image[fat_offset + 16..fat_offset + 20].copy_from_slice(&0x0fff_ffff_u32.to_le_bytes());

    let root_dir_offset = 1024;
    image[root_dir_offset] = 0xe5;
    image[root_dir_offset + 1..root_dir_offset + 8].copy_from_slice(b"EPORT  ");
    image[root_dir_offset + 8..root_dir_offset + 11].copy_from_slice(b"TXT");
    image[root_dir_offset + 11] = 0x20;
    image[root_dir_offset + 14..root_dir_offset + 16]
        .copy_from_slice(&encode_fat_time(9, 26, 12).to_le_bytes());
    image[root_dir_offset + 16..root_dir_offset + 18]
        .copy_from_slice(&encode_fat_date(2024, 3, 14).to_le_bytes());
    image[root_dir_offset + 22..root_dir_offset + 24]
        .copy_from_slice(&encode_fat_time(16, 8, 0).to_le_bytes());
    image[root_dir_offset + 24..root_dir_offset + 26]
        .copy_from_slice(&encode_fat_date(2024, 3, 15).to_le_bytes());
    image[root_dir_offset + 26..root_dir_offset + 28].copy_from_slice(&3_u16.to_le_bytes());
    image[root_dir_offset + 28..root_dir_offset + 32].copy_from_slice(&11_u32.to_le_bytes());
    let visible_offset = root_dir_offset + 32;
    image[visible_offset..visible_offset + 8].copy_from_slice(b"LIVELOG ");
    image[visible_offset + 8..visible_offset + 11].copy_from_slice(b"TXT");
    image[visible_offset + 11] = 0x20;
    image[visible_offset + 14..visible_offset + 16]
        .copy_from_slice(&encode_fat_time(8, 10, 0).to_le_bytes());
    image[visible_offset + 16..visible_offset + 18]
        .copy_from_slice(&encode_fat_date(2024, 3, 13).to_le_bytes());
    image[visible_offset + 22..visible_offset + 24]
        .copy_from_slice(&encode_fat_time(8, 12, 0).to_le_bytes());
    image[visible_offset + 24..visible_offset + 26]
        .copy_from_slice(&encode_fat_date(2024, 3, 13).to_le_bytes());
    image[visible_offset + 26..visible_offset + 28].copy_from_slice(&4_u16.to_le_bytes());
    image[visible_offset + 28..visible_offset + 32].copy_from_slice(&9_u32.to_le_bytes());

    let data_offset = 1536;
    image[data_offset..data_offset + 11].copy_from_slice(b"hello world");
    let visible_data_offset = 2048;
    image[visible_data_offset..visible_data_offset + 9].copy_from_slice(b"live log!");

    image
}

fn minimal_apfs_container_image() -> Vec<u8> {
    let block_size = 4096_u32;
    let block_count = 1024_u64;
    let mut image = vec![0_u8; block_size as usize * block_count as usize];
    image[32..36].copy_from_slice(b"NXSB");
    image[36..40].copy_from_slice(&block_size.to_le_bytes());
    image[40..48].copy_from_slice(&block_count.to_le_bytes());
    image
}

fn deleted_fat32_image_with_long_name(long_name: &str) -> Vec<u8> {
    let mut image = vec![0_u8; 512 * 6];

    image[11..13].copy_from_slice(&512_u16.to_le_bytes());
    image[13] = 1;
    image[14..16].copy_from_slice(&1_u16.to_le_bytes());
    image[16] = 1;
    image[32..36].copy_from_slice(&6_u32.to_le_bytes());
    image[36..40].copy_from_slice(&1_u32.to_le_bytes());
    image[44..48].copy_from_slice(&2_u32.to_le_bytes());
    image[82..90].copy_from_slice(b"FAT32   ");
    image[510] = 0x55;
    image[511] = 0xaa;

    let fat_offset = 512;
    image[fat_offset..fat_offset + 4].copy_from_slice(&0x0fff_fff8_u32.to_le_bytes());
    image[fat_offset + 4..fat_offset + 8].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());
    image[fat_offset + 8..fat_offset + 12].copy_from_slice(&0x0fff_ffff_u32.to_le_bytes());
    image[fat_offset + 12..fat_offset + 16].copy_from_slice(&0_u32.to_le_bytes());

    let root_dir_offset = 1024;
    let lfn_entries = build_deleted_long_name_entries(long_name);
    for (index, entry) in lfn_entries.iter().enumerate() {
        let offset = root_dir_offset + index * 32;
        image[offset..offset + 32].copy_from_slice(entry);
    }

    let short_offset = root_dir_offset + lfn_entries.len() * 32;
    image[short_offset] = 0xe5;
    image[short_offset + 1..short_offset + 8].copy_from_slice(b"UARTER~");
    image[short_offset + 8..short_offset + 11].copy_from_slice(b"TXT");
    image[short_offset + 11] = 0x20;
    image[short_offset + 26..short_offset + 28].copy_from_slice(&3_u16.to_le_bytes());
    image[short_offset + 28..short_offset + 32].copy_from_slice(&11_u32.to_le_bytes());
    image[short_offset + 32] = 0x00;

    let data_offset = 1536;
    image[data_offset..data_offset + 11].copy_from_slice(b"hello world");

    image
}

fn partially_overwritten_deleted_fat32_image() -> Vec<u8> {
    let mut image = vec![0_u8; 512 * 8];

    image[11..13].copy_from_slice(&512_u16.to_le_bytes());
    image[13] = 1;
    image[14..16].copy_from_slice(&1_u16.to_le_bytes());
    image[16] = 1;
    image[32..36].copy_from_slice(&8_u32.to_le_bytes());
    image[36..40].copy_from_slice(&1_u32.to_le_bytes());
    image[44..48].copy_from_slice(&2_u32.to_le_bytes());
    image[82..90].copy_from_slice(b"FAT32   ");
    image[510] = 0x55;
    image[511] = 0xaa;

    let fat_offset = 512;
    image[fat_offset..fat_offset + 4].copy_from_slice(&0x0fff_fff8_u32.to_le_bytes());
    image[fat_offset + 4..fat_offset + 8].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());
    image[fat_offset + 8..fat_offset + 12].copy_from_slice(&0x0fff_ffff_u32.to_le_bytes());
    image[fat_offset + 12..fat_offset + 16].copy_from_slice(&0_u32.to_le_bytes());
    image[fat_offset + 16..fat_offset + 20].copy_from_slice(&7_u32.to_le_bytes());
    image[fat_offset + 20..fat_offset + 24].copy_from_slice(&0_u32.to_le_bytes());

    let root_dir_offset = 1024;
    image[root_dir_offset] = 0xe5;
    image[root_dir_offset + 1..root_dir_offset + 8].copy_from_slice(b"IDEOPA ");
    image[root_dir_offset + 8..root_dir_offset + 11].copy_from_slice(b"BIN");
    image[root_dir_offset + 11] = 0x20;
    image[root_dir_offset + 14..root_dir_offset + 16]
        .copy_from_slice(&encode_fat_time(7, 4, 0).to_le_bytes());
    image[root_dir_offset + 16..root_dir_offset + 18]
        .copy_from_slice(&encode_fat_date(2021, 12, 1).to_le_bytes());
    image[root_dir_offset + 22..root_dir_offset + 24]
        .copy_from_slice(&encode_fat_time(8, 2, 0).to_le_bytes());
    image[root_dir_offset + 24..root_dir_offset + 26]
        .copy_from_slice(&encode_fat_date(2022, 1, 10).to_le_bytes());
    image[root_dir_offset + 26..root_dir_offset + 28].copy_from_slice(&3_u16.to_le_bytes());
    image[root_dir_offset + 28..root_dir_offset + 32].copy_from_slice(&1200_u32.to_le_bytes());

    let data_offset = 1536;
    image[data_offset..data_offset + 512].fill(0x41);

    image
}

fn minimal_deleted_exfat_image() -> Vec<u8> {
    let mut image = vec![0_u8; 512 * 8];

    image[3..11].copy_from_slice(b"EXFAT   ");
    image[72..80].copy_from_slice(&8_u64.to_le_bytes());
    image[80..84].copy_from_slice(&1_u32.to_le_bytes());
    image[84..88].copy_from_slice(&1_u32.to_le_bytes());
    image[88..92].copy_from_slice(&2_u32.to_le_bytes());
    image[92..96].copy_from_slice(&6_u32.to_le_bytes());
    image[96..100].copy_from_slice(&2_u32.to_le_bytes());
    image[100..104].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
    image[104..106].copy_from_slice(&0x0100_u16.to_le_bytes());
    image[108] = 9;
    image[109] = 0;
    image[110] = 1;
    image[111] = 0x80;
    image[510] = 0x55;
    image[511] = 0xaa;

    let fat_offset = 512;
    image[fat_offset..fat_offset + 4].copy_from_slice(&0xffff_fff8_u32.to_le_bytes());
    image[fat_offset + 4..fat_offset + 8].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());
    image[fat_offset + 8..fat_offset + 12].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());
    image[fat_offset + 12..fat_offset + 16].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());

    let root_dir_offset = 1024;
    image[root_dir_offset] = 0x81;
    image[root_dir_offset + 20..root_dir_offset + 24].copy_from_slice(&3_u32.to_le_bytes());
    image[root_dir_offset + 24..root_dir_offset + 32].copy_from_slice(&1_u64.to_le_bytes());

    let deleted_file_offset = root_dir_offset + 32;
    image[deleted_file_offset] = 0x05;
    image[deleted_file_offset + 1] = 2;
    image[deleted_file_offset + 4..deleted_file_offset + 6]
        .copy_from_slice(&0x0020_u16.to_le_bytes());
    image[deleted_file_offset + 8..deleted_file_offset + 12]
        .copy_from_slice(&encode_exfat_timestamp(2024, 3, 18, 10, 22, 30).to_le_bytes());
    image[deleted_file_offset + 12..deleted_file_offset + 16]
        .copy_from_slice(&encode_exfat_timestamp(2024, 3, 19, 16, 45, 4).to_le_bytes());

    let stream_offset = deleted_file_offset + 32;
    image[stream_offset] = 0x40;
    image[stream_offset + 1] = 0x03;
    image[stream_offset + 3] = 10;
    image[stream_offset + 20..stream_offset + 24].copy_from_slice(&5_u32.to_le_bytes());
    image[stream_offset + 24..stream_offset + 32].copy_from_slice(&11_u64.to_le_bytes());

    let name_offset = stream_offset + 32;
    image[name_offset] = 0x41;
    write_exfat_name_entry(&mut image[name_offset..name_offset + 32], "Report.txt");

    let bitmap_offset = 1536;
    image[bitmap_offset] = 0b0000_0011;

    let data_offset = 2560;
    image[data_offset..data_offset + 11].copy_from_slice(b"hello exfat");

    image
}

fn partially_overwritten_deleted_exfat_image() -> Vec<u8> {
    let mut image = minimal_deleted_exfat_image();
    let stream_offset = 1024 + 32 + 32;
    image[stream_offset + 24..stream_offset + 32].copy_from_slice(&700_u64.to_le_bytes());
    image[1536] = 0b0001_0011;
    image
}

fn jpeg_signature_image() -> Vec<u8> {
    let mut bytes = vec![0_u8; 128];
    let payload = [
        0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x02, 0x03, 0x04,
        0x05, 0xff, 0xd9,
    ];
    bytes[24..24 + payload.len()].copy_from_slice(&payload);
    bytes
}

fn corrupt_pdf_signature_image() -> Vec<u8> {
    let mut bytes = vec![0_u8; 256];
    let payload = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\n%%EOF";
    bytes[48..48 + payload.len()].copy_from_slice(payload);
    bytes
}

fn build_deleted_long_name_entries(long_name: &str) -> Vec<[u8; 32]> {
    let mut code_units: Vec<u16> = long_name.encode_utf16().collect();
    code_units.push(0x0000);
    while !code_units.len().is_multiple_of(13) {
        code_units.push(0xffff);
    }

    let chunk_count = code_units.len() / 13;
    let mut entries = Vec::with_capacity(chunk_count);

    for physical_index in 0..chunk_count {
        let logical_index = chunk_count - physical_index - 1;
        let chunk = &code_units[logical_index * 13..(logical_index + 1) * 13];
        let mut entry = [0_u8; 32];
        entry[0] = 0xe5;
        entry[11] = 0x0f;
        entry[12] = 0;
        entry[13] = 0;
        entry[26..28].copy_from_slice(&0_u16.to_le_bytes());

        write_utf16_slice(&mut entry[1..11], &chunk[0..5]);
        write_utf16_slice(&mut entry[14..26], &chunk[5..11]);
        write_utf16_slice(&mut entry[28..32], &chunk[11..13]);
        entries.push(entry);
    }

    entries
}

fn write_utf16_slice(target: &mut [u8], code_units: &[u16]) {
    for (chunk, code_unit) in target.chunks_exact_mut(2).zip(code_units.iter()) {
        chunk.copy_from_slice(&code_unit.to_le_bytes());
    }
}

fn encode_fat_date(year: u16, month: u8, day: u8) -> u16 {
    ((year.saturating_sub(1980)) << 9) | ((month as u16) << 5) | day as u16
}

fn encode_fat_time(hour: u8, minute: u8, second: u8) -> u16 {
    ((hour as u16) << 11) | ((minute as u16) << 5) | ((second / 2) as u16)
}

fn encode_exfat_timestamp(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> u32 {
    let date = encode_fat_date(year, month, day);
    let time = encode_fat_time(hour, minute, second);
    ((date as u32) << 16) | time as u32
}

fn write_exfat_name_entry(entry: &mut [u8], name: &str) {
    for (index, code_unit) in name.encode_utf16().take(15).enumerate() {
        let offset = 2 + index * 2;
        entry[offset..offset + 2].copy_from_slice(&code_unit.to_le_bytes());
    }
}

#[test]
fn relative_dir_from_display_path_handles_root_and_nested_paths() {
    assert_eq!(relative_dir_from_display_path("/"), PathBuf::new());
    assert_eq!(
        relative_dir_from_display_path("/cases/client-a"),
        PathBuf::from("cases/client-a")
    );
    assert_eq!(
        relative_dir_from_display_path("/cases/../../evil"),
        PathBuf::from("cases/evil")
    );
    assert_eq!(
        relative_dir_from_display_path("../../outside"),
        PathBuf::from("outside")
    );
}

#[test]
fn safe_export_file_name_removes_path_control() {
    assert_eq!(safe_export_file_name("../../id_rsa"), "id_rsa");
    assert_eq!(safe_export_file_name("/etc/passwd"), "passwd");
    assert_eq!(safe_export_file_name("bad:name?.txt"), "bad_name_.txt");
    assert_eq!(safe_export_file_name("CON.txt"), "CON_.txt");
    assert_eq!(safe_export_file_name("LPT1"), "LPT1_");
    assert_eq!(safe_export_file_name(".."), "recovered-file");
}

#[test]
fn build_source_path_preserves_relative_structure() {
    let source = build_source_path(Path::new("/Volumes/CaseDisk"), &sample_recovered_file());
    assert_eq!(
        source,
        PathBuf::from("/Volumes/CaseDisk/cases/client-a/report.txt")
    );
}

// `normalize_conflict_strategy_rejects_unknown_values` moved alongside
// the helper to `commands/validation.rs` (plan I5 slice 1).

#[test]
fn resolve_target_path_supports_rename_skip_and_overwrite() {
    let temp_dir = unique_temp_dir("conflicts");
    let target = temp_dir.join("report.txt");
    fs::write(&target, "existing").expect("existing target should be written");

    let rename_target = resolve_target_path(&target, "rename")
        .expect("rename strategy should succeed")
        .expect("rename strategy should return a target");
    assert_eq!(rename_target, temp_dir.join("report (1).txt"));

    let skip_target = resolve_target_path(&target, "skip").expect("skip strategy should succeed");
    assert!(skip_target.is_none());

    let overwrite_target = resolve_target_path(&target, "overwrite")
        .expect("overwrite strategy should succeed")
        .expect("overwrite strategy should return the original target");
    assert_eq!(overwrite_target, target);
    assert!(overwrite_target.exists());

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn write_text_report_to_path_writes_and_replaces_existing_file() {
    let temp_dir = unique_temp_dir("technical-report");
    let target = temp_dir.join("expert.log");

    write_text_report_to_path(&target, "first pass")
        .expect("initial technical report should be written");
    assert_eq!(
        fs::read_to_string(&target).expect("initial report should be readable"),
        "first pass"
    );

    write_text_report_to_path(&target, "second pass")
        .expect("existing technical report should be replaced");
    assert_eq!(
        fs::read_to_string(&target).expect("replaced report should be readable"),
        "second pass"
    );
    assert!(!temp_dir.join("expert.tmp").exists());

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn save_technical_timeline_report_rejects_empty_content() {
    let temp_dir = unique_temp_dir("technical-report-empty");
    let target = temp_dir.join("expert.log");

    let error =
        save_technical_timeline_report(target.to_string_lossy().to_string(), "   ".into(), None)
            .expect_err("empty technical report content should be rejected");

    assert!(error.contains("empty"));
    assert!(!target.exists());

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn build_support_bundle_archive_bytes_contains_manifest_histories_and_logs() {
    let scan_session = sample_scan_session();
    {
        let mut state = crate::commands::state::lock_or_recover(&scan_session, "scan session");
        state.id = format!("scan-support-{}", unix_timestamp_ms());
        state.scan_type = "image".into();
        state.progress.resume_from_bytes = 8 * 1024 * 1024;
        state.progress.unreadable_ranges_count = 1;
        state.progress.unreadable_bytes = 4096;
        state.imaging_retry_passes_completed = 1;
        state.imaging_unreadable_ranges = vec![ImagingMapRange {
            start_offset: 524_288,
            length: 4_096,
        }];
        state.logs.push(TechnicalLogEntry {
            timestamp_ms: 180,
            level: "info".into(),
            message: "Scan summary ready.".into(),
        });
    }
    let scan_id = register_scan_session_for_test(&scan_session);

    let export_session = sample_export_session();
    {
        let mut state = export_session.lock().expect("export session lock poisoned");
        state.id = format!("export-support-{}", unix_timestamp_ms());
        state.scan_id = scan_id.clone();
    }
    let export_id = register_export_session_for_test(&export_session);

    let bundle =
        build_support_bundle_archive_bytes().expect("support bundle archive should be created");
    let mut archive =
        ZipArchive::new(Cursor::new(bundle)).expect("support bundle zip should be readable");

    let manifest = read_zip_entry(&mut archive, "manifest.json");
    let manifest_json: serde_json::Value =
        serde_json::from_str(&manifest).expect("manifest json should be readable");
    assert_eq!(manifest_json["bundle_format_version"], 5);
    assert_eq!(
        manifest_json["build_info"]["bundle_identifier"],
        APP_BUNDLE_IDENTIFIER
    );
    assert_eq!(manifest_json["build_info"]["tauri_runtime"], "desktop");

    let scan_history = read_zip_entry(&mut archive, "scan-history.json");
    assert!(scan_history.contains(&scan_id));
    assert!(!scan_history.contains("/Volumes/CaseDisk"));

    let scan_provenance_summary = read_zip_entry(&mut archive, "scan-provenance-summary.txt");
    assert!(scan_provenance_summary.contains(&scan_id));
    assert!(scan_provenance_summary.contains("<redacted>"));

    let export_history = read_zip_entry(&mut archive, "export-history.json");
    assert!(export_history.contains(&export_id));
    assert!(export_history.contains("implicit_preview_first_excluded_count"));
    assert!(export_history.contains("<redacted-path>"));

    let export_posture_summary = read_zip_entry(&mut archive, "export-posture-summary.txt");
    assert!(export_posture_summary.contains("implicit-prudent-batch"));
    assert!(export_posture_summary.contains("APFS preview-first candidates held out"));

    let live_scan_ai_briefs = read_zip_entry(&mut archive, "live-scan-ai-briefs.json");
    assert!(live_scan_ai_briefs.contains(&scan_id));
    assert!(live_scan_ai_briefs.contains("apfs_catalog_reassembled"));

    let live_scan_ai_summary = read_zip_entry(&mut archive, "live-scan-ai-summary.txt");
    assert!(live_scan_ai_summary.contains("APFS reassembled"));

    let imaging_handoff_summary = read_zip_entry(&mut archive, "imaging-handoff-summary.txt");
    assert!(imaging_handoff_summary.contains(&scan_id));
    assert!(imaging_handoff_summary.contains("Operator status: degraded"));
    assert!(imaging_handoff_summary.contains("Rescue map included: yes"));
    assert!(imaging_handoff_summary.contains("Precise unreadable ranges persisted: 1"));
    assert!(imaging_handoff_summary.contains(
        "Largest unreadable range: 0x0000000000080000 - 0x0000000000080FFF (4096 bytes)"
    ));

    let imaging_report = read_zip_entry(&mut archive, &format!("imaging-reports/{scan_id}.txt"));
    assert!(imaging_report.contains("RECUPERE IMAGING SESSION INCIDENT REPORT"));
    assert!(imaging_report.contains("Operator status: degraded"));

    let imaging_map = read_zip_entry(&mut archive, &format!("imaging-rescue-maps/{scan_id}.map"));
    assert!(imaging_map.contains("# Mapfile. Generated by"));
    assert!(imaging_map.contains("# current_pos  current_status  current_pass"));

    let scan_log = read_zip_entry(&mut archive, &format!("scan-logs/{scan_id}.log"));
    assert!(scan_log.contains("Scan summary ready."));

    let export_log = read_zip_entry(&mut archive, &format!("export-logs/{export_id}.log"));
    assert!(export_log.contains("Copied report.txt successfully."));

    scan_sessions()
        .lock()
        .expect("scan session registry lock poisoned")
        .remove(&scan_id);
    export_sessions()
        .lock()
        .expect("export session registry lock poisoned")
        .remove(&export_id);
}

#[test]
fn get_app_build_info_returns_expected_identity() {
    let info = super::runtime::get_app_build_info();

    assert_eq!(info.product_name, APP_PRODUCT_NAME);
    assert_eq!(info.bundle_identifier, APP_BUNDLE_IDENTIFIER);
    assert_eq!(info.app_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(info.package_name, env!("CARGO_PKG_NAME"));
    assert_eq!(info.operating_system, env::consts::OS);
    assert_eq!(info.architecture, env::consts::ARCH);
    assert_eq!(info.tauri_runtime, "desktop");
    assert!(!info.target_triple.is_empty());
    assert!(matches!(info.build_profile.as_str(), "debug" | "release"));
}

#[test]
fn generate_imaging_session_report_summarizes_resume_and_unreadable_context() {
    let archive_root = unique_temp_dir("imaging-report-provenance");
    let archive_path = archive_root.join("scan-history.json");
    let session = sample_scan_session();
    let scan_id = {
        let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
        state.id = format!("scan-imaging-report-{}", unix_timestamp_ms());
        state.scan_type = "image".into();
        state.progress.bytes_scanned = 96 * 1024 * 1024;
        state.progress.resume_from_bytes = 16 * 1024 * 1024;
        state.progress.unreadable_ranges_count = 2;
        state.progress.unreadable_bytes = 8192;
        state.progress.errors_count = 2;
        state.logs.push(TechnicalLogEntry {
            timestamp_ms: 250,
            level: "warning".into(),
            message: "Read-only imaging completed with 2 unreadable source segment(s).".into(),
        });
        state.imaging_unreadable_ranges = vec![
            ImagingMapRange {
                start_offset: 1_048_576,
                length: 4_096,
            },
            ImagingMapRange {
                start_offset: 7_340_032,
                length: 2_048,
            },
        ];
        state.id.clone()
    };
    let mut record = snapshot_scan_record(&session);
    record.summary.source_display_name = Some("Case Mirror".into());
    record.summary.source_kind = Some("raid-analysis".into());
    record.summary.source_format = Some("RAID5".into());
    record.summary.source_analysis_path = Some("/cases/raid-analysis/case-mirror.img".into());
    record.summary.source_available = Some(true);
    record.summary.source_requires_preparation = Some(true);
    record.summary.source_prepared = Some(true);
    record.summary.reconstructed_raid_source = true;
    upsert_persisted_scan_record_at(&archive_path, record)
        .expect("imaging report fixture should be persisted");
    // SAFETY: This test serially sets a process-local environment override and
    // removes it before returning.
    unsafe {
        env::set_var("RECUPERE_HISTORY_PATH", &archive_path);
    }

    let report = generate_imaging_session_report(scan_id.clone())
        .expect("imaging report should be generated");

    assert!(report.contains("RECUPERE IMAGING SESSION INCIDENT REPORT"));
    assert!(report.contains(&scan_id));
    assert!(report.contains("Resumed from existing partial image: 16777216"));
    assert!(report.contains("Unreadable source segments: 2"));
    assert!(report.contains("Zero-filled unreadable bytes: 8192"));
    assert!(report.contains("=== UNREADABLE RANGE SAMPLE ==="));
    assert!(report.contains(
        "Largest unreadable range: 0x0000000000100000 - 0x0000000000100FFF (4096 bytes)"
    ));
    assert!(report.contains("Precise unreadable ranges persisted: 2"));
    assert!(report.contains("=== SOURCE PROVENANCE ==="));
    assert!(report.contains("Registered source: Case Mirror"));
    assert!(report.contains("Source kind: raid-analysis"));
    assert!(report.contains("Source format: RAID5"));
    assert!(report.contains("Analysis path: /cases/raid-analysis/case-mirror.img"));
    assert!(report.contains("Preparation state: prepared-local-analysis"));
    assert!(report.contains("Reconstructed RAID analysis source: yes"));
    assert!(report.contains("=== OPERATOR HANDOFF ==="));
    assert!(report.contains("Operator status: degraded"));
    assert!(report.contains("Operator summary: Image completed with unrecovered source gaps."));
    assert!(
        report.contains("Safer next step: Keep the rescue map and incident report with the case")
    );
    assert!(report.contains("These zero-filled bytes were not reconstructed"));
    assert!(report.contains("Read-only imaging completed with 2 unreadable source segment"));

    // SAFETY: Paired cleanup for the test-only environment override above.
    unsafe {
        env::remove_var("RECUPERE_HISTORY_PATH");
    }
    let _ = fs::remove_dir_all(archive_root);
}

#[test]
fn generate_imaging_session_report_rejects_non_imaging_sessions() {
    let session = sample_scan_session();
    {
        let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
        state.id = format!("scan-no-imaging-report-{}", unix_timestamp_ms());
        state.scan_type = "quick".into();
        state.progress.resume_from_bytes = 0;
        state.progress.unreadable_ranges_count = 0;
        state.progress.unreadable_bytes = 0;
    }
    let scan_id = register_scan_session_for_test(&session);

    let error = generate_imaging_session_report(scan_id.clone())
        .expect_err("non-imaging sessions should not expose imaging reports");

    assert!(error.contains("does not expose an imaging session report"));

    scan_sessions()
        .lock()
        .expect("scan session registry lock poisoned")
        .remove(&scan_id);
}

#[test]
fn generate_imaging_rescue_map_builds_ddrescue_style_blocks() {
    let archive_root = unique_temp_dir("imaging-map-provenance");
    let archive_path = archive_root.join("scan-history.json");
    let session = sample_scan_session();
    let scan_id = {
        let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
        state.id = format!("scan-imaging-map-{}", unix_timestamp_ms());
        state.scan_type = "image".into();
        state.progress.status = "completed".into();
        state.progress.bytes_scanned = 96 * 1024 * 1024;
        state.progress.total_bytes = 128 * 1024 * 1024;
        state.progress.resume_from_bytes = 16 * 1024 * 1024;
        state.progress.unreadable_ranges_count = 2;
        state.progress.unreadable_bytes = 8192;
        state.imaging_retry_passes_completed = 2;
        state.imaging_rescued_after_retry_bytes = 12 * 1024;
        state.imaging_unreadable_ranges = vec![
            ImagingMapRange {
                start_offset: 1_048_576,
                length: 4_096,
            },
            ImagingMapRange {
                start_offset: 7_340_032,
                length: 4_096,
            },
        ];
        state.id.clone()
    };
    let mut record = snapshot_scan_record(&session);
    record.summary.source_display_name = Some("Lab E01 Intake".into());
    record.summary.source_kind = Some("forensic-image".into());
    record.summary.source_format = Some("E01".into());
    record.summary.source_analysis_path = Some("/cases/intake/lab-cache.dd".into());
    record.summary.source_available = Some(true);
    record.summary.source_requires_preparation = Some(true);
    record.summary.source_prepared = Some(false);
    upsert_persisted_scan_record_at(&archive_path, record)
        .expect("imaging map fixture should be persisted");
    // SAFETY: This test serially sets a process-local environment override and
    // removes it before returning.
    unsafe {
        env::set_var("RECUPERE_HISTORY_PATH", &archive_path);
    }

    let mapfile = generate_imaging_rescue_map(scan_id.clone())
        .expect("imaging rescue map should be generated");

    assert!(mapfile.contains("# Mapfile. Generated by"));
    assert!(mapfile.contains("# Registered source: Lab E01 Intake"));
    assert!(mapfile.contains("# Source kind: forensic-image"));
    assert!(mapfile.contains("# Source format: E01"));
    assert!(mapfile.contains("# Analysis path: /cases/intake/lab-cache.dd"));
    assert!(mapfile.contains("# Preparation state: preparation-pending"));
    assert!(mapfile.contains("# current_pos  current_status  current_pass"));
    assert!(mapfile.contains("#      pos              size  status"));
    assert!(mapfile.contains("0x0000000000100000  0x0000000000001000  -"));
    assert!(mapfile.contains("0x0000000000700000  0x0000000000001000  -"));
    assert!(mapfile.contains("0x0000000000000000  0x0000000000100000  +"));
    assert!(mapfile.contains("0x0000000006000000  0x0000000002000000  ?"));

    // SAFETY: Paired cleanup for the test-only environment override above.
    unsafe {
        env::remove_var("RECUPERE_HISTORY_PATH");
    }
    let _ = fs::remove_dir_all(archive_root);
}

#[test]
fn generate_imaging_rescue_map_rejects_non_imaging_sessions() {
    let session = sample_scan_session();
    {
        let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
        state.id = format!("scan-no-imaging-map-{}", unix_timestamp_ms());
        state.scan_type = "quick".into();
    }
    let scan_id = register_scan_session_for_test(&session);

    let error = generate_imaging_rescue_map(scan_id.clone())
        .expect_err("non-imaging sessions should not expose rescue maps");

    assert!(error.contains("does not expose an imaging rescue map"));

    scan_sessions()
        .lock()
        .expect("scan session registry lock poisoned")
        .remove(&scan_id);
}

#[test]
fn save_support_bundle_writes_zip_to_requested_destination() {
    let temp_dir = unique_temp_dir("support-bundle");
    let target = temp_dir.join("support-bundle.zip");

    save_support_bundle(target.to_string_lossy().to_string(), None)
        .expect("support bundle should be written");

    let bytes = fs::read(&target).expect("support bundle file should exist");
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).expect("support bundle zip should be readable");
    let readme = read_zip_entry(&mut archive, "README.txt");
    assert!(readme.contains("never includes recovered file contents"));

    let _ = fs::remove_dir_all(temp_dir);
}

fn read_zip_entry(archive: &mut ZipArchive<Cursor<Vec<u8>>>, path: &str) -> String {
    let mut file = archive
        .by_name(path)
        .unwrap_or_else(|_| panic!("zip entry `{path}` should exist"));
    let mut content = String::new();
    file.read_to_string(&mut content)
        .unwrap_or_else(|_| panic!("zip entry `{path}` should be readable as text"));
    content
}

#[test]
fn verify_exported_file_detects_size_mismatch() {
    let temp_dir = unique_temp_dir("verify");
    let source = temp_dir.join("source.bin");
    let destination = temp_dir.join("destination.bin");

    fs::write(&source, vec![0_u8; 8]).expect("source file should be written");
    fs::write(&destination, vec![0_u8; 4]).expect("destination file should be written");

    let result = verify_exported_file(&source, &destination);
    assert!(result.is_err());
    assert!(result
        .expect_err("mismatch should return an error")
        .contains("size mismatch"));

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn append_scan_log_caps_log_count() {
    let session = sample_scan_session();
    {
        let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
        state.logs = (0..MAX_SESSION_LOGS)
            .map(|index| TechnicalLogEntry {
                timestamp_ms: index as u64,
                level: "info".into(),
                message: format!("log-{index}"),
            })
            .collect();
    }

    append_scan_log(&session, "info", "latest-log".into());

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.logs.len(), MAX_SESSION_LOGS);
    assert_eq!(
        state.logs.first().expect("first log should exist").message,
        "log-1"
    );
    assert_eq!(
        state.logs.last().expect("last log should exist").message,
        "latest-log"
    );
}

#[test]
fn build_scan_summary_returns_expected_metadata() {
    let session = sample_scan_session();
    let summary = build_scan_summary(&session);

    assert_eq!(summary.id, "scan-123");
    assert_eq!(summary.device_id, "disk-1");
    assert_eq!(summary.device_name, "Case Disk");
    assert_eq!(summary.scan_type, "deep");
    assert_eq!(summary.started_at_ms, 100);
    assert_eq!(summary.completed_at_ms, Some(200));
    assert_eq!(summary.status, "completed");
    assert_eq!(summary.files_found, 12);
    assert_eq!(summary.files_recovered, 0);
    assert_eq!(summary.duration_seconds, 42);
    assert_eq!(summary.errors, 1);
}

#[test]
fn compute_progress_clamps_between_expected_bounds() {
    assert_eq!(compute_progress(0, 100, false), 4.0);
    assert_eq!(compute_progress(0, 0, false), 50.0);
    assert_eq!(compute_progress(10_000, 100, false), 99.0);
    assert_eq!(compute_progress(100, 100, true), 100.0);
}

#[test]
fn imaging_requires_elevation_fallback_only_for_permission_denied_raw_devices() {
    assert!(imaging_requires_elevation_fallback(
        Path::new("/dev/rdisk4"),
        "does not have permission",
        true,
    ));
    assert!(!imaging_requires_elevation_fallback(
        Path::new("/tmp/source.img"),
        "does not have permission",
        true,
    ));
    assert!(!imaging_requires_elevation_fallback(
        Path::new("/dev/rdisk4"),
        "No such file or directory",
        true,
    ));
    assert!(!imaging_requires_elevation_fallback(
        Path::new("/dev/rdisk4"),
        "does not have permission",
        false,
    ));
}

#[cfg(target_os = "macos")]
#[test]
fn build_macos_privileged_imager_script_quotes_paths_safely() {
    let script = build_macos_privileged_imager_script(
        Path::new("/Applications/Recupere.app/Contents/MacOS/recupere"),
        Path::new("/dev/rdisk4"),
        Path::new("/Users/test/O'Brien/capture image.img"),
        Path::new("/tmp/progress file.txt"),
        Path::new("/tmp/error file.txt"),
        Path::new("/tmp/summary file.json"),
        imaging::ImagingProfile::Cautious,
    );

    assert!(script.contains("do shell script"));
    assert!(script.contains("with administrator privileges"));
    assert!(script.contains(privileged_imager::PRIVILEGED_IMAGER_FLAG));
    assert!(script.contains("/Applications/Recupere.app/Contents/MacOS/recupere"));
    assert!(script.contains("capture image.img"));
    assert!(script.contains("summary file.json"));
    assert!(script.contains("--profile"));
    assert!(script.contains("cautious"));
    assert!(script.contains("\\\""));
}

#[test]
fn build_diagnostic_marks_imaging_ready_for_readable_source() {
    let root = unique_temp_dir("diagnostic-imaging-ready");
    let source = root.join("source.img");
    fs::write(&source, b"diagnostic source").expect("readable source should be written");

    let diagnostic = build_diagnostic(&sample_detected_device_with_path(&source));
    assert!(diagnostic.imaging_ready);
    assert_eq!(
        diagnostic.imaging_source_path,
        Some(source.to_string_lossy().to_string())
    );
    assert!(!diagnostic.imaging_requires_elevation);
    assert!(diagnostic.imaging_block_reason.is_none());

    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn build_diagnostic_reports_imaging_block_reason_for_unreadable_source() {
    let root = unique_temp_dir("diagnostic-imaging-blocked");
    let source = root.join("blocked.img");
    fs::write(&source, b"blocked source").expect("blocked source should be written");

    let mut permissions = fs::metadata(&source)
        .expect("blocked source metadata should exist")
        .permissions();
    permissions.set_mode(0o000);
    fs::set_permissions(&source, permissions).expect("blocked source permissions should update");

    let diagnostic = build_diagnostic(&sample_detected_device_with_path(&source));
    assert!(!diagnostic.imaging_ready);
    assert!(!diagnostic.imaging_requires_elevation);
    assert!(diagnostic
        .imaging_block_reason
        .as_ref()
        .is_some_and(|reason| reason.contains("does not have permission")));
    assert!(diagnostic
        .limitations
        .iter()
        .any(|limitation| limitation.contains("does not have permission")));

    let mut cleanup_permissions = fs::metadata(&source)
        .expect("blocked source metadata should still exist")
        .permissions();
    cleanup_permissions.set_mode(0o644);
    let _ = fs::set_permissions(&source, cleanup_permissions);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn build_export_summary_returns_expected_metadata() {
    let session = sample_export_session();
    let summary = build_export_summary(&session);

    assert_eq!(summary.id, "export-123");
    assert_eq!(summary.scan_id, "scan-123");
    assert_eq!(summary.destination_path, "/Volumes/Recovery/export");
    assert_eq!(summary.started_at_ms, 400);
    assert_eq!(summary.completed_at_ms, Some(500));
    assert_eq!(summary.status, "completed");
    assert_eq!(summary.total_files, 3);
    assert_eq!(summary.exported_files, 2);
    assert_eq!(summary.total_bytes, 2048);
    assert_eq!(summary.exported_bytes, 1024);
    assert!(!summary.explicit_selection);
    assert_eq!(summary.implicit_preview_first_excluded_count, 2);
    assert_eq!(summary.errors.len(), 1);
    assert_eq!(summary.errors[0].file_name, "missing.bin");
}

#[test]
fn persisted_scan_archive_round_trips_session_summary_and_logs() {
    let archive_path = unique_temp_dir("persisted-history").join("scan-history.json");
    let session = sample_scan_session();
    {
        let mut state = crate::commands::state::lock_or_recover(&session, "scan session");
        state.logs.push(TechnicalLogEntry {
            timestamp_ms: 300,
            level: "info".into(),
            message: "persisted-log".into(),
        });
    }

    upsert_persisted_scan_record_at(&archive_path, snapshot_scan_record(&session))
        .expect("persisted scan record should be written");

    let archive = load_persisted_scan_archive_from(&archive_path);
    assert_eq!(archive.scans.len(), 1);
    assert_eq!(archive.scans[0].summary.id, "scan-123");
    assert!(archive.scans[0]
        .logs
        .iter()
        .any(|log| log.message == "persisted-log"));

    if let Some(parent) = archive_path.parent() {
        let _ = fs::remove_dir_all(parent);
    }
}

#[test]
fn persisted_export_archive_round_trips_summary_logs_and_errors() {
    let archive_path = unique_temp_dir("persisted-export-history").join("export-history.json");
    let session = sample_export_session();

    upsert_persisted_export_record_at(&archive_path, snapshot_export_record(&session))
        .expect("persisted export record should be written");

    let archive = load_persisted_export_archive_from(&archive_path);
    assert_eq!(archive.exports.len(), 1);
    assert_eq!(archive.exports[0].summary.id, "export-123");
    assert_eq!(archive.exports[0].summary.errors.len(), 1);
    assert_eq!(
        archive.exports[0]
            .summary
            .implicit_preview_first_excluded_count,
        2
    );
    assert_eq!(archive.exports[0].summary.errors[0].reason, "copy failed");
    assert_eq!(archive.exports[0].logs.len(), 1);
    assert_eq!(
        archive.exports[0].logs[0].message,
        "Copied report.txt successfully."
    );

    if let Some(parent) = archive_path.parent() {
        let _ = fs::remove_dir_all(parent);
    }
}

#[test]
fn load_persisted_export_archive_from_supports_legacy_summary_only_format() {
    let archive_path = unique_temp_dir("persisted-export-legacy").join("export-history.json");
    let legacy_archive = LegacyPersistedExportArchive {
        exports: vec![build_export_summary(&sample_export_session())],
    };

    if let Some(parent) = archive_path.parent() {
        fs::create_dir_all(parent).expect("legacy archive parent should exist");
    }
    fs::write(
        &archive_path,
        serde_json::to_string_pretty(&legacy_archive).expect("legacy archive should serialize"),
    )
    .expect("legacy archive should be written");

    let archive = load_persisted_export_archive_from(&archive_path);
    assert_eq!(archive.exports.len(), 1);
    assert_eq!(archive.exports[0].summary.id, "export-123");
    assert!(archive.exports[0].logs.is_empty());

    if let Some(parent) = archive_path.parent() {
        let _ = fs::remove_dir_all(parent);
    }
}

#[test]
fn clear_local_history_at_removes_only_scan_archive_for_scan_scope() {
    let root = unique_temp_dir("history-purge-scan");
    let scan_archive_path = root.join("scan-history.json");
    let export_archive_path = root.join("export-history.json");

    upsert_persisted_scan_record_at(
        &scan_archive_path,
        snapshot_scan_record(&sample_scan_session()),
    )
    .expect("scan archive should be written");
    upsert_persisted_export_record_at(
        &export_archive_path,
        snapshot_export_record(&sample_export_session()),
    )
    .expect("export archive should be written");

    let result = clear_local_history_at("scan", &scan_archive_path, &export_archive_path, 1, 0)
        .expect("scan purge should succeed");

    assert_eq!(result.scope, "scan");
    assert_eq!(result.removed_scan_records, 1);
    assert_eq!(result.removed_export_records, 0);
    assert!(result.scan_archive_deleted);
    assert!(!result.export_archive_deleted);
    assert_eq!(result.live_scan_sessions, 1);
    assert_eq!(result.live_export_sessions, 0);
    assert!(!scan_archive_path.exists());
    assert!(export_archive_path.exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn clear_local_history_at_removes_both_archives_for_all_scope() {
    let root = unique_temp_dir("history-purge-all");
    let scan_archive_path = root.join("scan-history.json");
    let export_archive_path = root.join("export-history.json");

    upsert_persisted_scan_record_at(
        &scan_archive_path,
        snapshot_scan_record(&sample_scan_session()),
    )
    .expect("scan archive should be written");
    upsert_persisted_export_record_at(
        &export_archive_path,
        snapshot_export_record(&sample_export_session()),
    )
    .expect("export archive should be written");

    let result = clear_local_history_at("all", &scan_archive_path, &export_archive_path, 2, 3)
        .expect("history purge should succeed");

    assert_eq!(result.scope, "all");
    assert_eq!(result.removed_scan_records, 1);
    assert_eq!(result.removed_export_records, 1);
    assert!(result.scan_archive_deleted);
    assert!(result.export_archive_deleted);
    assert_eq!(result.live_scan_sessions, 2);
    assert_eq!(result.live_export_sessions, 3);
    assert!(!scan_archive_path.exists());
    assert!(!export_archive_path.exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn run_inventory_scan_catalogs_files_and_completes_session() {
    let root = unique_temp_dir("scan-integration");
    let nested_dir = root.join("cases");
    fs::create_dir_all(&nested_dir).expect("nested scan directory should be created");
    fs::write(root.join("readme.txt"), b"hello world").expect("root test file should be written");
    fs::write(nested_dir.join("photo.jpg"), vec![1_u8; 32])
        .expect("nested test file should be written");

    let total_bytes = fs::metadata(root.join("readme.txt")).unwrap().len()
        + fs::metadata(nested_dir.join("photo.jpg")).unwrap().len();
    let session = scan_session_for_root(&root, "deep", total_bytes);

    run_inventory_scan(
        "scan-integration".into(),
        Arc::clone(&session),
        root.clone(),
        "deep",
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.progress.files_found, 2);
    assert_eq!(state.results.len(), 2);
    assert!(state
        .results
        .iter()
        .any(|file| file.name == "readme.txt" && file.path == "/"));
    assert!(state
        .results
        .iter()
        .any(|file| file.name == "photo.jpg" && file.path == "/cases"));
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Scan completed")));

    drop(state);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn run_image_acquisition_completes_without_generating_results() {
    let source_root = unique_temp_dir("image-source");
    let source_image = source_root.join("device.bin");
    fs::write(&source_image, b"image me").expect("image source should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("image source metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "image", total_bytes);

    imaging_cmd::run_image_acquisition(
        "scan-image".into(),
        Arc::clone(&session),
        ImagingSourcePlan::Direct {
            source_path: source_image.clone(),
        },
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.progress.stage, "finalizing");
    assert_eq!(state.progress.bytes_scanned, total_bytes);
    assert_eq!(state.progress.files_found, 0);
    assert!(state.results.is_empty());
    assert!(state.logs.iter().any(|log| log
        .message
        .contains("Standalone read-only imaging completed: image saved at")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_image_acquisition_to_writes_the_requested_destination() {
    let source_root = unique_temp_dir("image-source-explicit");
    let source_image = source_root.join("device.bin");
    fs::write(&source_image, b"image me").expect("image source should be written");

    let destination_root = unique_temp_dir("image-destination-explicit");
    let destination_image = destination_root.join("captures").join("disk.dd");
    let total_bytes = fs::metadata(&source_image)
        .expect("image source metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "image", total_bytes);

    imaging_cmd::run_image_acquisition_to(
        "scan-image-explicit".into(),
        Arc::clone(&session),
        ImagingSourcePlan::Direct {
            source_path: source_image.clone(),
        },
        total_bytes,
        destination_image.clone(),
        None,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.progress.bytes_scanned, total_bytes);
    assert!(state.results.is_empty());
    assert!(state.logs.iter().any(|log| log
        .message
        .contains(destination_image.to_string_lossy().as_ref())));
    drop(state);

    assert_eq!(
        fs::read(&destination_image).expect("explicit image destination should exist"),
        b"image me"
    );

    let _ = fs::remove_dir_all(source_root);
    let _ = fs::remove_dir_all(destination_root);
}

#[test]
fn run_export_session_copies_cataloged_files_with_structure() {
    let source_root = unique_temp_dir("export-source");
    let source_nested = source_root.join("cases");
    fs::create_dir_all(&source_nested).expect("source nested directory should be created");
    fs::write(source_root.join("notes.txt"), b"case notes")
        .expect("source text file should be written");
    fs::write(source_nested.join("evidence.bin"), vec![9_u8; 16])
        .expect("source binary file should be written");

    let total_bytes = fs::metadata(source_root.join("notes.txt")).unwrap().len()
        + fs::metadata(source_nested.join("evidence.bin"))
            .unwrap()
            .len();
    let scan_session = scan_session_for_root(&source_root, "deep", total_bytes);
    run_inventory_scan(
        "scan-export-flow".into(),
        Arc::clone(&scan_session),
        source_root.clone(),
        "deep",
    );

    let scanned_files = {
        let state = crate::commands::state::lock_or_recover(&scan_session, "scan session");
        state.results.clone()
    };
    let destination_root = unique_temp_dir("export-destination");
    let export_session = export_session_for_destination(
        "scan-export-flow",
        &destination_root,
        scanned_files.len(),
        scanned_files.iter().map(|file| file.size_bytes).sum(),
    );

    run_export_session(
        "export-integration".into(),
        Arc::clone(&export_session),
        source_root.clone(),
        destination_root.clone(),
        scanned_files.clone(),
        "rename".into(),
        true,
        true,
    );

    let export_state = export_session.lock().expect("export session lock poisoned");
    assert_eq!(export_state.progress.status, "completed");
    assert_eq!(
        export_state.progress.exported_files,
        scanned_files.len() as u32
    );
    assert_eq!(export_state.progress.errors.len(), 0);
    assert!(export_state
        .logs
        .iter()
        .any(|log| log.message.contains("Starting export")));
    assert!(export_state
        .logs
        .iter()
        .any(|log| log.message.contains("Copied notes.txt")));
    assert!(export_state
        .logs
        .iter()
        .any(|log| log.message.contains("Copied evidence.bin")));
    assert!(export_state
        .logs
        .iter()
        .any(|log| log.message.contains("Export finished")));
    drop(export_state);

    for file in &scanned_files {
        let exported_path = destination_root
            .join(relative_dir_from_display_path(&file.path))
            .join(&file.name);
        assert!(
            exported_path.exists(),
            "expected {:?} to exist",
            exported_path
        );
        assert_eq!(
            fs::metadata(&exported_path)
                .expect("exported file metadata should be available")
                .len(),
            file.size_bytes
        );
    }

    let _ = fs::remove_dir_all(source_root);
    let _ = fs::remove_dir_all(destination_root);
}

#[test]
fn run_deleted_fat32_scan_recovers_deleted_entries_from_a_local_image() {
    let source_root = unique_temp_dir("deleted-fat32-source");
    let source_image = source_root.join("fat32-source.img");
    fs::write(&source_image, minimal_deleted_fat32_image())
        .expect("synthetic FAT32 source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_fat32_scan(
        "scan-deleted-fat32".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.progress.files_found, 1);
    assert_eq!(state.results.len(), 1);
    assert!(state.results[0].is_deleted);
    assert_eq!(state.results[0].name, "_EPORT.TXT");
    assert_eq!(state.results[0].size_bytes, 11);
    assert_eq!(
        state.results[0].created_at.as_deref(),
        Some("2024-03-14T09:26:12")
    );
    assert_eq!(
        state.results[0].modified_at.as_deref(),
        Some("2024-03-15T16:08:00")
    );
    assert_eq!(state.results[0].integrity, "intact");
    assert!(state.results[0].source_image_path.is_some());
    assert!(state.results[0].byte_runs.is_some());
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Local image created")));
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Deleted FAT32 recovery completed")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_deleted_fat32_scan_rebuilds_long_name_entries() {
    let source_root = unique_temp_dir("deleted-fat32-source-long-name");
    let source_image = source_root.join("fat32-source-long-name.img");
    fs::write(
        &source_image,
        deleted_fat32_image_with_long_name("Quarterly Report.txt"),
    )
    .expect("synthetic FAT32 long-name source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_fat32_scan(
        "scan-deleted-fat32-long-name".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.results.len(), 1);
    assert_eq!(state.results[0].name, "Quarterly Report.txt");
    assert_eq!(state.results[0].extension, "txt");
    assert!(state.results[0].is_deleted);
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_deleted_fat32_scan_marks_partially_reconstructible_results() {
    let source_root = unique_temp_dir("deleted-fat32-source-partial");
    let source_image = source_root.join("fat32-source-partial.img");
    fs::write(&source_image, partially_overwritten_deleted_fat32_image())
        .expect("synthetic partial FAT32 source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_fat32_scan(
        "scan-deleted-fat32-partial".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.results.len(), 1);
    assert!(state.results[0].is_deleted);
    assert_eq!(state.results[0].name, "_IDEOPA.BIN");
    assert_eq!(state.results[0].size_bytes, 512);
    assert_eq!(state.results[0].expected_size_bytes, Some(1200));
    assert_eq!(
        state.results[0].created_at.as_deref(),
        Some("2021-12-01T07:04:00")
    );
    assert_eq!(
        state.results[0].modified_at.as_deref(),
        Some("2022-01-10T08:02:00")
    );
    assert_eq!(state.results[0].integrity, "partial");
    assert_eq!(state.results[0].clusters.as_deref(), Some(&[3][..]));
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Deleted FAT32 recovery completed")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_deleted_exfat_scan_recovers_deleted_entries_from_a_local_image() {
    let source_root = unique_temp_dir("deleted-exfat-source");
    let source_image = source_root.join("exfat-source.img");
    fs::write(&source_image, minimal_deleted_exfat_image())
        .expect("synthetic exFAT source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_exfat_scan(
        "scan-deleted-exfat".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.progress.files_found, 1);
    assert_eq!(state.results.len(), 1);
    assert!(state.results[0].is_deleted);
    assert_eq!(state.results[0].name, "Report.txt");
    assert_eq!(state.results[0].size_bytes, 11);
    assert_eq!(
        state.results[0].created_at.as_deref(),
        Some("2024-03-18T10:22:30")
    );
    assert_eq!(
        state.results[0].modified_at.as_deref(),
        Some("2024-03-19T16:45:04")
    );
    assert_eq!(state.results[0].integrity, "intact");
    assert!(state.results[0].source_image_path.is_some());
    assert!(state.results[0].byte_runs.is_some());
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Deleted exFAT recovery completed")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_deleted_exfat_scan_marks_partially_reconstructible_results() {
    let source_root = unique_temp_dir("deleted-exfat-source-partial");
    let source_image = source_root.join("exfat-source-partial.img");
    fs::write(&source_image, partially_overwritten_deleted_exfat_image())
        .expect("synthetic partial exFAT source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_exfat_scan(
        "scan-deleted-exfat-partial".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.results.len(), 1);
    assert!(state.results[0].is_deleted);
    assert_eq!(state.results[0].name, "Report.txt");
    assert_eq!(state.results[0].size_bytes, 512);
    assert_eq!(state.results[0].expected_size_bytes, Some(700));
    assert_eq!(state.results[0].integrity, "partial");
    assert_eq!(state.results[0].clusters.as_deref(), Some(&[5][..]));
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Deleted exFAT recovery completed")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_deleted_ntfs_scan_recovers_deleted_entries_from_a_local_image() {
    let source_root = unique_temp_dir("deleted-ntfs-source");
    let source_image = source_root.join("ntfs-source.img");
    fs::write(&source_image, ntfs::synthetic_deleted_ntfs_image())
        .expect("synthetic NTFS source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_ntfs_scan(
        "scan-deleted-ntfs".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert!(
        state.progress.files_found >= 2,
        "should find at least 2 files (primary + journal)"
    );
    assert!(state.results.len() >= 2, "should have at least 2 results");

    let note = state
        .results
        .iter()
        .find(|file| file.name == "Note.txt")
        .expect("resident NTFS deleted file should exist");
    assert!(note.is_deleted);
    assert_eq!(note.path, "/Docs");
    assert_eq!(note.size_bytes, 10);
    assert_eq!(note.expected_size_bytes, Some(10));
    assert_eq!(note.integrity, "intact");
    assert_eq!(note.created_at.as_deref(), Some("2024-03-14T09:26:12"));
    assert_eq!(note.modified_at.as_deref(), Some("2024-03-15T16:08:00"));

    let archive = state
        .results
        .iter()
        .find(|file| file.name == "Archive.bin")
        .expect("non-resident NTFS deleted file should exist");
    assert!(archive.is_deleted);
    assert_eq!(archive.path, "/Docs");
    assert_eq!(archive.size_bytes, 512);
    assert_eq!(archive.expected_size_bytes, Some(700));
    assert_eq!(archive.integrity, "partial");
    assert_eq!(archive.clusters.as_deref(), Some(&[40][..]));
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Deleted NTFS recovery completed")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_deleted_ntfs_scan_recovers_sparse_runlist_entries() {
    let source_root = unique_temp_dir("deleted-ntfs-sparse-source");
    let source_image = source_root.join("ntfs-sparse-source.img");
    fs::write(&source_image, ntfs::synthetic_sparse_deleted_ntfs_image())
        .expect("synthetic sparse NTFS source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_ntfs_scan(
        "scan-deleted-ntfs-sparse".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.results.len(), 3);

    let sparse = state
        .results
        .iter()
        .find(|file| file.name == "Sparse.bin")
        .expect("sparse NTFS deleted file should exist");
    assert!(sparse.is_deleted);
    assert_eq!(sparse.path, "/Docs");
    assert_eq!(sparse.size_bytes, 1536);
    assert_eq!(sparse.expected_size_bytes, Some(1536));
    assert_eq!(sparse.integrity, "fragmented");
    assert_eq!(sparse.clusters.as_deref(), Some(&[50, 52][..]));
    assert!(sparse.source_image_path.is_some());
    assert!(sparse.byte_runs.is_some());
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Deleted NTFS recovery completed")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_deleted_ntfs_scan_surfaces_named_data_stream_metadata() {
    let source_root = unique_temp_dir("deleted-ntfs-ads-source");
    let source_image = source_root.join("ntfs-ads-source.img");
    fs::write(&source_image, ntfs::synthetic_ntfs_named_streams_image())
        .expect("synthetic NTFS ADS source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_ntfs_scan(
        "scan-deleted-ntfs-ads".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    let deleted = state
        .results
        .iter()
        .find(|file| file.name == "Note.txt")
        .expect("deleted NTFS ADS result should exist");
    let streams = deleted
        .alternate_data_streams
        .as_ref()
        .expect("deleted NTFS ADS result should expose named streams");
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].name, "Summary");
    assert_eq!(streams[0].size_bytes, 13);
    assert_eq!(streams[0].expected_size_bytes, Some(13));
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Deleted NTFS recovery completed")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_deleted_ntfs_scan_surfaces_lznt1_compression_metadata() {
    let source_root = unique_temp_dir("deleted-ntfs-compressed-source");
    let source_image = source_root.join("ntfs-compressed-source.img");
    fs::write(
        &source_image,
        ntfs::synthetic_compressed_deleted_ntfs_image(),
    )
    .expect("synthetic compressed NTFS source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_ntfs_scan(
        "scan-deleted-ntfs-compressed".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    let compressed = state
        .results
        .iter()
        .find(|file| file.name == "Compressed.txt")
        .expect("compressed NTFS result should exist");
    assert_eq!(compressed.compression_kind.as_deref(), Some("lznt1"));
    assert_eq!(compressed.recovery_complexity.as_deref(), Some("medium"));
    assert_eq!(compressed.validator_status.as_deref(), Some("validated"));
    assert_eq!(compressed.integrity, "intact");
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_deleted_ext4_scan_recovers_deleted_entries_from_a_local_image() {
    let source_root = unique_temp_dir("deleted-ext4-source");
    let source_image = source_root.join("ext4-source.img");
    fs::write(
        &source_image,
        ext4::synthetic_deleted_ext4_image_for_tests(b"hello ext4!", false),
    )
    .expect("synthetic ext4 source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_ext4_scan(
        "scan-deleted-ext4".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.progress.files_found, 1);
    assert_eq!(state.results.len(), 1);
    assert_eq!(state.results[0].name, "inode-000003.txt");
    assert_eq!(state.results[0].path, "/orphaned-inodes");
    assert_eq!(state.results[0].size_bytes, 11);
    assert_eq!(state.results[0].expected_size_bytes, Some(11));
    assert_eq!(
        state.results[0].deleted_at.as_deref(),
        Some("2024-04-02T12:00:00")
    );
    assert_eq!(state.results[0].integrity, "intact");
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Deleted ext4 recovery completed")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_deleted_ext4_scan_recovers_indexed_extent_tree_entries() {
    let source_root = unique_temp_dir("deleted-ext4-indexed-source");
    let source_image = source_root.join("ext4-indexed-source.img");
    fs::write(
        &source_image,
        ext4::synthetic_deleted_ext4_indexed_extent_image_for_tests(b"hello indexed ext4", false),
    )
    .expect("synthetic indexed ext4 source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("indexed ext4 source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_ext4_scan(
        "scan-deleted-ext4-indexed".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.progress.files_found, 1);
    assert_eq!(state.results.len(), 1);
    assert_eq!(state.results[0].name, "inode-000003.txt");
    assert_eq!(state.results[0].size_bytes, 18);
    assert_eq!(state.results[0].expected_size_bytes, Some(18));
    assert_eq!(state.results[0].integrity, "intact");
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Deleted ext4 recovery completed")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_deleted_hfsplus_scan_recovers_catalog_slack_entries_from_a_local_image() {
    let source_root = unique_temp_dir("deleted-hfsplus-source");
    let source_image = source_root.join("hfsplus-source.img");
    fs::write(
        &source_image,
        hfsplus::synthetic_deleted_hfsplus_image_for_tests(b"hello hfs", b"deleted hfs"),
    )
    .expect("synthetic HFS+ source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("HFS+ source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_hfsplus_scan(
        "scan-deleted-hfsplus".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.progress.files_found, 1);
    assert_eq!(state.results.len(), 1);
    assert_eq!(state.results[0].name, "Deleted.txt");
    assert_eq!(state.results[0].path, "/Docs");
    assert_eq!(state.results[0].size_bytes, 11);
    assert_eq!(state.results[0].expected_size_bytes, Some(11));
    assert_eq!(state.results[0].integrity, "intact");
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Deleted HFS+ recovery completed")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_deleted_hfsplus_scan_surfaces_resource_fork_metadata() {
    let source_root = unique_temp_dir("deleted-hfsplus-resource-source");
    let source_image = source_root.join("hfsplus-resource-source.img");
    fs::write(
        &source_image,
        hfsplus::synthetic_deleted_hfsplus_resource_fork_image_for_tests(
            b"hello hfs",
            b"visible-rsrc",
            b"deleted hfs",
            b"deleted-rsrc",
        ),
    )
    .expect("synthetic HFS+ resource source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("HFS+ resource source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_hfsplus_scan(
        "scan-deleted-hfsplus-resource".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.results.len(), 1);
    let deleted = &state.results[0];
    let resource_fork = deleted
        .resource_fork
        .as_ref()
        .expect("deleted HFS+ result should expose a resource fork");
    assert_eq!(resource_fork.size_bytes, 12);
    assert_eq!(resource_fork.expected_size_bytes, Some(12));
    assert_eq!(resource_fork.byte_runs.len(), 1);
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_export_session_writes_hfsplus_resource_fork_sidecar() {
    let source_root = unique_temp_dir("deleted-hfsplus-resource-export-source");
    let source_image = source_root.join("hfsplus-resource-export-source.img");
    fs::write(
        &source_image,
        hfsplus::synthetic_deleted_hfsplus_resource_fork_image_for_tests(
            b"hello hfs",
            b"visible-rsrc",
            b"deleted hfs",
            b"deleted-rsrc",
        ),
    )
    .expect("synthetic HFS+ resource export source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let scan_session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_hfsplus_scan(
        "scan-deleted-hfsplus-resource-export".into(),
        Arc::clone(&scan_session),
        source_image.clone(),
        total_bytes,
    );

    let deleted_file = {
        let state = crate::commands::state::lock_or_recover(&scan_session, "scan session");
        state.results[0].clone()
    };
    let resource_fork_size = deleted_file
        .resource_fork
        .as_ref()
        .expect("deleted HFS+ export result should expose a resource fork")
        .size_bytes;

    let destination_root = unique_temp_dir("deleted-hfsplus-resource-export-destination");
    let export_session = export_session_for_destination(
        "scan-deleted-hfsplus-resource-export",
        &destination_root,
        1,
        deleted_file.size_bytes.saturating_add(resource_fork_size),
    );

    run_export_session(
        "export-deleted-hfsplus-resource".into(),
        Arc::clone(&export_session),
        PathBuf::from("/dev/disk-test"),
        destination_root.clone(),
        vec![deleted_file.clone()],
        "rename".into(),
        true,
        true,
    );

    let export_state = export_session.lock().expect("export session lock poisoned");
    assert_eq!(export_state.progress.status, "completed");
    assert_eq!(export_state.progress.exported_files, 1);
    assert_eq!(
        export_state.progress.exported_bytes,
        deleted_file.size_bytes.saturating_add(resource_fork_size)
    );
    assert!(export_state.progress.errors.is_empty());
    assert!(export_state.logs.iter().any(|log| {
        log.message.contains("resource-fork sidecar") && log.message.contains("Deleted.txt")
    }));
    drop(export_state);

    let exported_path = destination_root.join("Docs").join("Deleted.txt");
    let exported_sidecar = destination_root
        .join("Docs")
        .join("Deleted.txt.resource-fork.bin");
    assert_eq!(
        fs::read(&exported_path).expect("exported HFS+ file should be readable"),
        b"deleted hfs"
    );
    assert_eq!(
        fs::read(&exported_sidecar).expect("resource-fork sidecar should be readable"),
        b"deleted-rsrc"
    );

    let _ = fs::remove_dir_all(source_root);
    let _ = fs::remove_dir_all(destination_root);
}

#[test]
fn run_export_session_reconstructs_deleted_ntfs_file_from_image() {
    let source_root = unique_temp_dir("deleted-ntfs-export-source");
    let source_image = source_root.join("ntfs-source.img");
    fs::write(&source_image, ntfs::synthetic_deleted_ntfs_image())
        .expect("synthetic NTFS source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let scan_session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_ntfs_scan(
        "scan-deleted-ntfs-export".into(),
        Arc::clone(&scan_session),
        source_image.clone(),
        total_bytes,
    );

    let deleted_file = {
        let state = crate::commands::state::lock_or_recover(&scan_session, "scan session");
        state
            .results
            .iter()
            .find(|file| file.name == "Note.txt")
            .expect("resident NTFS result should exist")
            .clone()
    };

    let destination_root = unique_temp_dir("deleted-ntfs-export-destination");
    let export_session = export_session_for_destination(
        "scan-deleted-ntfs-export",
        &destination_root,
        1,
        deleted_file.size_bytes,
    );

    run_export_session(
        "export-deleted-ntfs".into(),
        Arc::clone(&export_session),
        PathBuf::from("/dev/disk-test"),
        destination_root.clone(),
        vec![deleted_file.clone()],
        "rename".into(),
        true,
        true,
    );

    let export_state = export_session.lock().expect("export session lock poisoned");
    assert_eq!(export_state.progress.status, "completed");
    assert_eq!(export_state.progress.exported_files, 1);
    assert!(export_state.progress.errors.is_empty());
    assert!(export_state
        .logs
        .iter()
        .any(|log| log.message.contains("Copied Note.txt")));
    drop(export_state);

    let exported_path = destination_root.join("Docs").join("Note.txt");
    assert!(exported_path.exists());
    assert_eq!(
        fs::read(&exported_path).expect("exported NTFS file should be readable"),
        b"HELLO NTFS"
    );

    let _ = fs::remove_dir_all(source_root);
    let _ = fs::remove_dir_all(destination_root);
}

#[test]
fn run_export_session_writes_ntfs_ads_sidecars() {
    let source_root = unique_temp_dir("deleted-ntfs-ads-export-source");
    let source_image = source_root.join("ntfs-ads-source.img");
    fs::write(&source_image, ntfs::synthetic_ntfs_named_streams_image())
        .expect("synthetic NTFS ADS source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let scan_session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_ntfs_scan(
        "scan-deleted-ntfs-ads-export".into(),
        Arc::clone(&scan_session),
        source_image.clone(),
        total_bytes,
    );

    let deleted_file = {
        let state = crate::commands::state::lock_or_recover(&scan_session, "scan session");
        state
            .results
            .iter()
            .find(|file| file.name == "Note.txt")
            .expect("deleted NTFS ADS result should exist")
            .clone()
    };
    let ads_size = deleted_file
        .alternate_data_streams
        .as_ref()
        .expect("deleted NTFS ADS export result should expose named streams")[0]
        .size_bytes;

    let destination_root = unique_temp_dir("deleted-ntfs-ads-export-destination");
    let export_session = export_session_for_destination(
        "scan-deleted-ntfs-ads-export",
        &destination_root,
        1,
        deleted_file.size_bytes.saturating_add(ads_size),
    );

    run_export_session(
        "export-deleted-ntfs-ads".into(),
        Arc::clone(&export_session),
        PathBuf::from("/dev/disk-test"),
        destination_root.clone(),
        vec![deleted_file.clone()],
        "rename".into(),
        true,
        true,
    );

    let export_state = export_session.lock().expect("export session lock poisoned");
    assert_eq!(export_state.progress.status, "completed");
    assert_eq!(export_state.progress.exported_files, 1);
    assert_eq!(
        export_state.progress.exported_bytes,
        deleted_file.size_bytes.saturating_add(ads_size)
    );
    assert!(export_state.progress.errors.is_empty());
    assert!(export_state.logs.iter().any(|log| {
        log.message.contains("alternate-data-stream sidecar")
            && log.message.contains("Summary")
            && log.message.contains("Note.txt")
    }));
    drop(export_state);

    let exported_path = destination_root.join("Docs").join("Note.txt");
    let exported_sidecar = destination_root
        .join("Docs")
        .join("Note.txt.ads.Summary.bin");
    assert!(exported_path.exists());
    assert!(exported_sidecar.exists());
    assert_eq!(
        fs::read(&exported_path).expect("exported NTFS file should be readable"),
        b"HELLO NTFS"
    );
    assert_eq!(
        fs::read(&exported_sidecar).expect("exported NTFS ADS sidecar should be readable"),
        b"NTFS ADS NOTE"
    );

    let _ = fs::remove_dir_all(source_root);
    let _ = fs::remove_dir_all(destination_root);
}

#[test]
fn run_export_session_reconstructs_deleted_sparse_ntfs_file_from_image() {
    let source_root = unique_temp_dir("deleted-ntfs-sparse-export-source");
    let source_image = source_root.join("ntfs-sparse-source.img");
    fs::write(&source_image, ntfs::synthetic_sparse_deleted_ntfs_image())
        .expect("synthetic sparse NTFS source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let scan_session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_ntfs_scan(
        "scan-deleted-ntfs-sparse-export".into(),
        Arc::clone(&scan_session),
        source_image.clone(),
        total_bytes,
    );

    let deleted_file = {
        let state = crate::commands::state::lock_or_recover(&scan_session, "scan session");
        state
            .results
            .iter()
            .find(|file| file.name == "Sparse.bin")
            .expect("sparse NTFS result should exist")
            .clone()
    };

    let destination_root = unique_temp_dir("deleted-ntfs-sparse-export-destination");
    let export_session = export_session_for_destination(
        "scan-deleted-ntfs-sparse-export",
        &destination_root,
        1,
        deleted_file.size_bytes,
    );

    run_export_session(
        "export-deleted-ntfs-sparse".into(),
        Arc::clone(&export_session),
        PathBuf::from("/dev/disk-test"),
        destination_root.clone(),
        vec![deleted_file.clone()],
        "rename".into(),
        true,
        true,
    );

    let export_state = export_session.lock().expect("export session lock poisoned");
    assert_eq!(export_state.progress.status, "completed");
    assert_eq!(export_state.progress.exported_files, 1);
    assert!(export_state.progress.errors.is_empty());
    drop(export_state);

    let exported_path = destination_root.join("Docs").join("Sparse.bin");
    let exported_bytes =
        fs::read(&exported_path).expect("sparse reconstructed export should be readable");
    assert_eq!(exported_bytes.len(), 1536);
    assert!(exported_bytes[..512].iter().all(|byte| *byte == 0x53));
    assert!(exported_bytes[512..1024].iter().all(|byte| *byte == 0));
    assert!(exported_bytes[1024..].iter().all(|byte| *byte == 0x54));

    let _ = fs::remove_dir_all(source_root);
    let _ = fs::remove_dir_all(destination_root);
}

#[test]
fn run_export_session_reconstructs_deleted_compressed_ntfs_file_from_image() {
    let source_root = unique_temp_dir("deleted-ntfs-compressed-export-source");
    let source_image = source_root.join("ntfs-compressed-source.img");
    fs::write(
        &source_image,
        ntfs::synthetic_compressed_deleted_ntfs_image(),
    )
    .expect("synthetic compressed NTFS source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let scan_session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_ntfs_scan(
        "scan-deleted-ntfs-compressed-export".into(),
        Arc::clone(&scan_session),
        source_image.clone(),
        total_bytes,
    );

    let deleted_file = {
        let state = crate::commands::state::lock_or_recover(&scan_session, "scan session");
        state
            .results
            .iter()
            .find(|file| file.name == "Compressed.txt")
            .expect("compressed NTFS result should exist")
            .clone()
    };

    let destination_root = unique_temp_dir("deleted-ntfs-compressed-export-destination");
    let export_session = export_session_for_destination(
        "scan-deleted-ntfs-compressed-export",
        &destination_root,
        1,
        deleted_file.size_bytes,
    );

    run_export_session(
        "export-deleted-ntfs-compressed".into(),
        Arc::clone(&export_session),
        PathBuf::from("/dev/disk-test"),
        destination_root.clone(),
        vec![deleted_file.clone()],
        "rename".into(),
        true,
        true,
    );

    let export_state = export_session.lock().expect("export session lock poisoned");
    assert_eq!(export_state.progress.status, "completed");
    assert_eq!(export_state.progress.exported_files, 1);
    assert!(export_state.progress.errors.is_empty());
    drop(export_state);

    let exported_path = destination_root.join("Docs").join("Compressed.txt");
    let exported_bytes =
        fs::read(&exported_path).expect("compressed reconstructed export should be readable");
    let expected = b"NTFS LZNT1 TEST DATA NTFS LZNT1 TEST DATA NTFS LZNT1 TEST DATA NTFS LZNT1 TEST DATA NTFS LZNT1 TEST DATA NTFS LZNT1 TEST DATA ".repeat(8);
    assert_eq!(exported_bytes, expected);

    let _ = fs::remove_dir_all(source_root);
    let _ = fs::remove_dir_all(destination_root);
}

#[test]
fn build_file_preview_reads_cataloged_text_content() {
    let source_root = unique_temp_dir("preview-catalog-source");
    let source_file = source_root
        .join("cases")
        .join("client-a")
        .join("report.txt");
    fs::create_dir_all(
        source_file
            .parent()
            .expect("preview source file should have a parent"),
    )
    .expect("preview source parent should exist");
    fs::write(&source_file, b"catalog preview").expect("catalog preview source should be written");

    let mut file = sample_recovered_file();
    file.size_bytes = 15;
    let preview = build_file_preview("scan-preview-catalog", &source_root, &file)
        .expect("preview should build");

    assert_eq!(preview.kind, "text");
    assert_eq!(preview.text_content.as_deref(), Some("catalog preview"));
    assert!(!preview.truncated);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_preview_reads_cataloged_docx_content() {
    let source_root = unique_temp_dir("preview-catalog-docx-source");
    let source_file = source_root
        .join("cases")
        .join("client-a")
        .join("report.docx");
    fs::create_dir_all(
        source_file
            .parent()
            .expect("preview source file should have a parent"),
    )
    .expect("preview source parent should exist");
    fs::write(
        &source_file,
        build_zip_bytes(&[(
            "word/document.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
                <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                  <w:body><w:p><w:r><w:t>Client note</w:t></w:r></w:p></w:body>
                </w:document>"#,
        )]),
    )
    .expect("catalog docx preview source should be written");

    let mut file = sample_recovered_file();
    file.name = "report.docx".into();
    file.extension = "docx".into();
    file.size_bytes = fs::metadata(&source_file)
        .expect("catalog docx source metadata should exist")
        .len();
    file.mime_type =
        Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document".into());

    let preview = build_file_preview("scan-preview-catalog-docx", &source_root, &file)
        .expect("docx preview should build");

    assert_eq!(preview.kind, "text");
    assert_eq!(preview.text_content.as_deref(), Some("Client note"));
    assert!(!preview.truncated);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_preview_reads_cataloged_pptx_content() {
    let source_root = unique_temp_dir("preview-catalog-pptx-source");
    let source_file = source_root.join("cases").join("client-a").join("deck.pptx");
    fs::create_dir_all(
        source_file
            .parent()
            .expect("preview source file should have a parent"),
    )
    .expect("preview source parent should exist");
    fs::write(
            &source_file,
            build_zip_bytes(&[
                (
                    "ppt/presentation.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
                    <p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"></p:presentation>"#,
                ),
                (
                    "ppt/slides/slide1.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
                    <p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                           xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                      <p:cSld><p:spTree>
                        <p:sp><p:txBody><a:p><a:r><a:t>Quarterly Review</a:t></a:r></a:p></p:txBody></p:sp>
                        <p:sp><p:txBody><a:p><a:r><a:t>Recovered devices stable</a:t></a:r></a:p></p:txBody></p:sp>
                      </p:spTree></p:cSld>
                    </p:sld>"#,
                ),
            ]),
        )
        .expect("catalog pptx preview source should be written");

    let mut file = sample_recovered_file();
    file.name = "deck.pptx".into();
    file.extension = "pptx".into();
    file.size_bytes = fs::metadata(&source_file)
        .expect("catalog pptx source metadata should exist")
        .len();
    file.mime_type =
        Some("application/vnd.openxmlformats-officedocument.presentationml.presentation".into());

    let preview = build_file_preview("scan-preview-catalog-pptx", &source_root, &file)
        .expect("pptx preview should build");

    assert_eq!(preview.kind, "text");
    assert_eq!(
        preview.text_content.as_deref(),
        Some("Quarterly Review\nRecovered devices stable")
    );
    assert!(!preview.truncated);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_preview_reads_deleted_ntfs_text_from_recovery_image() {
    let source_root = unique_temp_dir("preview-deleted-ntfs-source");
    let source_image = source_root.join("ntfs-source.img");
    fs::write(&source_image, ntfs::synthetic_deleted_ntfs_image())
        .expect("synthetic NTFS source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_ntfs_scan(
        "scan-preview-deleted-ntfs".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let note = {
        let state = crate::commands::state::lock_or_recover(&session, "scan session");
        state
            .results
            .iter()
            .find(|file| file.name == "Note.txt")
            .expect("deleted NTFS text result should exist")
            .clone()
    };

    let preview = build_file_preview(
        "scan-preview-deleted-ntfs",
        Path::new("/dev/disk-test"),
        &note,
    )
    .expect("deleted NTFS preview should build");

    assert_eq!(preview.kind, "text");
    assert_eq!(preview.text_content.as_deref(), Some("HELLO NTFS"));
    assert!(!preview.truncated);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_preview_reads_visible_text_from_recovery_image() {
    let source_root = unique_temp_dir("preview-visible-fat32-source");
    let source_image = source_root.join("fat32-slice.img");
    fs::write(&source_image, minimal_deleted_fat32_image())
        .expect("synthetic FAT32 slice should be written");

    let visible_file = RecoveredFile {
        id: "visible-slice-file".into(),
        name: "LIVELOG.TXT".into(),
        path: "/".into(),
        extension: "txt".into(),
        size_bytes: 9,
        created_at: Some("2024-03-13T08:10:00".into()),
        modified_at: Some("2024-03-13T08:12:00".into()),
        integrity: "intact".into(),
        recovery_score: 98,
        recovery_method: "filesystem".into(),
        preview_available: true,
        mime_type: Some("text/plain".into()),
        expected_size_bytes: Some(9),
        deleted_at: None,
        start_offset: Some(2048),
        clusters: Some(vec![4]),
        byte_runs: Some(vec![ByteRun {
            offset: 2048,
            length: 512,
            zero_fill: false,
            ..Default::default()
        }]),
        resource_fork: None,
        alternate_data_streams: None,
        source_image_path: Some(source_image.to_string_lossy().to_string()),
        is_deleted: false,
        source_view: Some("recovery-image".into()),
        ..Default::default()
    };

    let preview = build_file_preview(
        "scan-preview-visible-fat32",
        Path::new("/dev/disk-test"),
        &visible_file,
    )
    .expect("recovery-image-backed visible preview should build");

    assert_eq!(preview.kind, "text");
    assert_eq!(preview.text_content.as_deref(), Some("live log!"));
    assert!(!preview.truncated);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_preview_reads_recovery_backed_xlsx_content() {
    let source_root = unique_temp_dir("preview-recovery-xlsx-source");
    let source_image = source_root.join("office-source.img");
    let xlsx_bytes = build_zip_bytes(&[
        (
            "xl/sharedStrings.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
                <sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                  <si><t>Nom</t></si>
                  <si><t>Valeur</t></si>
                </sst>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
                <worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                  <sheetData>
                    <row r="1">
                      <c r="A1" t="s"><v>0</v></c>
                      <c r="B1" t="s"><v>1</v></c>
                    </row>
                    <row r="2">
                      <c r="A2" t="inlineStr"><is><t>Alpha</t></is></c>
                      <c r="B2"><v>7</v></c>
                    </row>
                  </sheetData>
                </worksheet>"#,
        ),
    ]);
    fs::write(&source_image, &xlsx_bytes).expect("synthetic XLSX image should be written");

    let recovery_file = RecoveredFile {
        id: "recovery-xlsx-file".into(),
        name: "table.xlsx".into(),
        path: "/".into(),
        extension: "xlsx".into(),
        size_bytes: xlsx_bytes.len() as u64,
        created_at: None,
        modified_at: None,
        integrity: "intact".into(),
        recovery_score: 91,
        recovery_method: "reconstruction".into(),
        preview_available: true,
        mime_type: Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into()),
        expected_size_bytes: Some(xlsx_bytes.len() as u64),
        deleted_at: None,
        start_offset: Some(0),
        clusters: Some(vec![2]),
        byte_runs: Some(vec![ByteRun {
            offset: 0,
            length: xlsx_bytes.len() as u64,
            zero_fill: false,
            ..Default::default()
        }]),
        resource_fork: None,
        alternate_data_streams: None,
        source_image_path: Some(source_image.to_string_lossy().to_string()),
        is_deleted: true,
        source_view: Some("recovery-image".into()),
        ..Default::default()
    };

    let preview = build_file_preview(
        "scan-preview-recovery-xlsx",
        Path::new("/dev/disk-test"),
        &recovery_file,
    )
    .expect("recovery-backed xlsx preview should build");

    assert_eq!(preview.kind, "text");
    assert_eq!(
        preview.text_content.as_deref(),
        Some("Nom\tValeur\nAlpha\t7")
    );
    assert!(!preview.truncated);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_preview_materializes_carved_image_asset() {
    let source_root = unique_temp_dir("preview-carved-source");
    let source_image = source_root.join("signature-source.img");
    fs::write(&source_image, jpeg_signature_image())
        .expect("synthetic carving source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "signature-carving", total_bytes);

    run_signature_carving_scan(
        "scan-preview-carved".into(),
        Arc::clone(&session),
        ImagingSourcePlan::Direct {
            source_path: source_image.clone(),
        },
        total_bytes,
    );

    let carved_file = {
        let state = crate::commands::state::lock_or_recover(&session, "scan session");
        state.results[0].clone()
    };

    let preview = build_file_preview("scan-preview-carved", &source_image, &carved_file)
        .expect("carved image preview should build");

    assert_eq!(preview.kind, "image");
    let asset_path = preview
        .asset_path
        .as_deref()
        .expect("carved preview should materialize an asset path");
    assert!(Path::new(asset_path).exists());

    let _ = fs::remove_dir_all(source_root);
    let _ = fs::remove_file(asset_path);
}

#[test]
fn build_file_hex_preview_reads_cataloged_bytes_with_offset() {
    let source_root = unique_temp_dir("hex-catalog-source");
    let source_file = source_root
        .join("cases")
        .join("client-a")
        .join("report.txt");
    fs::create_dir_all(
        source_file
            .parent()
            .expect("hex source file should have a parent"),
    )
    .expect("hex source parent should exist");
    fs::write(&source_file, b"HELLO HEX VIEWER").expect("hex source fixture should be written");

    let mut file = sample_recovered_file();
    file.size_bytes = 16;

    let preview = build_file_hex_preview(&source_root, &file, 6, 8)
        .expect("hex preview should build for cataloged file");

    assert_eq!(preview.file_id, file.id);
    assert_eq!(preview.start_offset, 6);
    assert_eq!(preview.bytes_read, 8);
    assert_eq!(preview.total_size_bytes, 16);
    assert!(preview.has_more_before);
    assert!(preview.has_more_after);
    assert_eq!(preview.lines.len(), 1);
    assert_eq!(preview.lines[0].offset, 6);
    assert_eq!(preview.lines[0].ascii, "HEX VIEW");

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_hex_preview_reads_recovery_image_backed_bytes() {
    let source_root = unique_temp_dir("hex-visible-fat32-source");
    let source_image = source_root.join("fat32-slice.img");
    fs::write(&source_image, minimal_deleted_fat32_image())
        .expect("synthetic FAT32 slice should be written");

    let visible_file = RecoveredFile {
        id: "visible-slice-file".into(),
        name: "LIVELOG.TXT".into(),
        path: "/".into(),
        extension: "txt".into(),
        size_bytes: 9,
        created_at: Some("2024-03-13T08:10:00".into()),
        modified_at: Some("2024-03-13T08:12:00".into()),
        integrity: "intact".into(),
        recovery_score: 98,
        recovery_method: "filesystem".into(),
        preview_available: true,
        mime_type: Some("text/plain".into()),
        expected_size_bytes: Some(9),
        deleted_at: None,
        start_offset: Some(2048),
        clusters: Some(vec![4]),
        byte_runs: Some(vec![ByteRun {
            offset: 2048,
            length: 512,
            zero_fill: false,
            ..Default::default()
        }]),
        resource_fork: None,
        alternate_data_streams: None,
        source_image_path: Some(source_image.to_string_lossy().to_string()),
        is_deleted: false,
        source_view: Some("recovery-image".into()),
        ..Default::default()
    };

    let preview = build_file_hex_preview(Path::new("/dev/disk-test"), &visible_file, 5, 4)
        .expect("hex preview should build for recovery image-backed file");

    assert_eq!(preview.start_offset, 5);
    assert_eq!(preview.bytes_read, 4);
    assert_eq!(preview.lines.len(), 1);
    assert_eq!(preview.lines[0].offset, 5);
    assert_eq!(preview.lines[0].ascii, "log!");

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_auxiliary_hex_preview_reads_hfsplus_resource_fork_bytes() {
    let source_root = unique_temp_dir("hex-hfsplus-resource-fork-source");
    let source_image = source_root.join("hfsplus-resource-source.img");
    fs::write(
        &source_image,
        hfsplus::synthetic_deleted_hfsplus_resource_fork_image_for_tests(
            b"hello hfs",
            b"visible-rsrc",
            b"deleted hfs",
            b"deleted-rsrc",
        ),
    )
    .expect("synthetic HFS+ resource source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_hfsplus_scan(
        "scan-hex-hfsplus-resource".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let deleted_file = {
        let state = crate::commands::state::lock_or_recover(&session, "scan session");
        state.results[0].clone()
    };

    let preview = build_file_auxiliary_hex_preview(&deleted_file, "resource-fork", None, 0, 12)
        .expect("resource-fork hex preview should build");

    assert_eq!(preview.bytes_read, 12);
    assert_eq!(preview.total_size_bytes, 12);
    assert_eq!(preview.lines.len(), 1);
    assert_eq!(preview.lines[0].offset, 0);
    assert_eq!(preview.lines[0].ascii, "deleted-rsrc");

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_auxiliary_hex_preview_reads_ntfs_ads_bytes() {
    let source_root = unique_temp_dir("hex-ntfs-ads-source");
    let source_image = source_root.join("ntfs-ads-source.img");
    fs::write(&source_image, ntfs::synthetic_ntfs_named_streams_image())
        .expect("synthetic NTFS ADS source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_ntfs_scan(
        "scan-hex-ntfs-ads".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let deleted_file = {
        let state = crate::commands::state::lock_or_recover(&session, "scan session");
        state
            .results
            .iter()
            .find(|file| file.name == "Note.txt")
            .expect("deleted NTFS ADS result should exist")
            .clone()
    };

    let preview = build_file_auxiliary_hex_preview(&deleted_file, "ads", Some("Summary"), 0, 13)
        .expect("NTFS ADS hex preview should build");

    assert_eq!(preview.bytes_read, 13);
    assert_eq!(preview.total_size_bytes, 13);
    assert_eq!(preview.lines.len(), 1);
    assert_eq!(preview.lines[0].offset, 0);
    assert_eq!(preview.lines[0].ascii, "NTFS ADS NOTE");

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_auxiliary_preview_reads_hfsplus_resource_fork_text_content() {
    let source_root = unique_temp_dir("preview-hfsplus-resource-fork-source");
    let source_image = source_root.join("hfsplus-resource-source.img");
    fs::write(
        &source_image,
        hfsplus::synthetic_deleted_hfsplus_resource_fork_image_for_tests(
            b"hello hfs",
            b"visible-rsrc",
            b"deleted hfs",
            b"deleted-rsrc",
        ),
    )
    .expect("synthetic HFS+ resource source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_hfsplus_scan(
        "scan-preview-hfsplus-resource".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let deleted_file = {
        let state = crate::commands::state::lock_or_recover(&session, "scan session");
        state.results[0].clone()
    };

    let preview = build_file_auxiliary_preview(
        "scan-preview-hfsplus-resource",
        &deleted_file,
        "resource-fork",
        None,
    )
    .expect("resource-fork preview should build");

    assert_eq!(preview.kind, "text");
    assert_eq!(preview.text_content.as_deref(), Some("deleted-rsrc"));
    assert!(!preview.truncated);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_auxiliary_preview_reads_ntfs_ads_text_content() {
    let source_root = unique_temp_dir("preview-ntfs-ads-source");
    let source_image = source_root.join("ntfs-ads-source.img");
    fs::write(&source_image, ntfs::synthetic_ntfs_named_streams_image())
        .expect("synthetic NTFS ADS source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_ntfs_scan(
        "scan-preview-ntfs-ads".into(),
        Arc::clone(&session),
        source_image.clone(),
        total_bytes,
    );

    let deleted_file = {
        let state = crate::commands::state::lock_or_recover(&session, "scan session");
        state
            .results
            .iter()
            .find(|file| file.name == "Note.txt")
            .expect("deleted NTFS ADS result should exist")
            .clone()
    };

    let preview = build_file_auxiliary_preview(
        "scan-preview-ntfs-ads",
        &deleted_file,
        "ads",
        Some("Summary"),
    )
    .expect("NTFS ADS preview should build");

    assert_eq!(preview.kind, "text");
    assert_eq!(preview.text_content.as_deref(), Some("NTFS ADS NOTE"));
    assert!(!preview.truncated);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_auxiliary_preview_materializes_ntfs_ads_image_asset() {
    let source_root = unique_temp_dir("preview-ntfs-ads-image-source");
    let source_image = source_root.join("ntfs-ads-image-source.img");
    let png_bytes = b"\x89PNG\r\n\x1A\nPNG AUX PREVIEW";
    fs::write(&source_image, png_bytes).expect("synthetic NTFS ADS image source should be written");

    let mut file = sample_recovered_file();
    file.name = "Readme.txt".into();
    file.source_image_path = Some(source_image.to_string_lossy().to_string());
    file.alternate_data_streams = Some(vec![NamedFileFork {
        name: "Preview".into(),
        size_bytes: png_bytes.len() as u64,
        expected_size_bytes: Some(png_bytes.len() as u64),
        byte_runs: vec![ByteRun::physical(0, png_bytes.len() as u64)],
    }]);

    let preview =
        build_file_auxiliary_preview("scan-preview-ntfs-ads-image", &file, "ads", Some("Preview"))
            .expect("NTFS ADS image preview should build");

    assert_eq!(preview.kind, "image");
    assert_eq!(preview.mime_type.as_deref(), Some("image/png"));
    let asset_path = preview
        .asset_path
        .as_ref()
        .expect("NTFS ADS image preview should materialize an asset");
    assert_eq!(
        fs::read(asset_path).expect("materialized auxiliary asset should exist"),
        png_bytes
    );

    let _ = fs::remove_file(asset_path);
    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_auxiliary_preview_materializes_hfsplus_resource_pdf_asset() {
    let source_root = unique_temp_dir("preview-hfsplus-resource-pdf-source");
    let source_image = source_root.join("hfsplus-resource-pdf-source.img");
    let pdf_bytes = b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\n";
    fs::write(&source_image, pdf_bytes)
        .expect("synthetic HFS+ resource PDF source should be written");

    let mut file = sample_recovered_file();
    file.name = "Deleted.txt".into();
    file.source_image_path = Some(source_image.to_string_lossy().to_string());
    file.resource_fork = Some(FileFork {
        size_bytes: pdf_bytes.len() as u64,
        expected_size_bytes: Some(pdf_bytes.len() as u64),
        byte_runs: vec![ByteRun::physical(0, pdf_bytes.len() as u64)],
    });

    let preview = build_file_auxiliary_preview(
        "scan-preview-hfsplus-resource-pdf",
        &file,
        "resource-fork",
        None,
    )
    .expect("HFS+ resource PDF preview should build");

    assert_eq!(preview.kind, "pdf");
    assert_eq!(preview.mime_type.as_deref(), Some("application/pdf"));
    let asset_path = preview
        .asset_path
        .as_ref()
        .expect("HFS+ resource PDF preview should materialize an asset");
    assert_eq!(
        fs::read(asset_path).expect("materialized auxiliary PDF should exist"),
        pdf_bytes
    );

    let _ = fs::remove_file(asset_path);
    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_auxiliary_preview_reads_ntfs_ads_docx_content() {
    let source_root = unique_temp_dir("preview-ntfs-ads-docx-source");
    let source_image = source_root.join("ntfs-ads-docx-source.img");
    let docx_bytes = build_zip_bytes(&[(
        "word/document.xml",
        r#"<?xml version="1.0" encoding="UTF-8"?>
            <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
              <w:body><w:p><w:r><w:t>ADS DOCX NOTE</w:t></w:r></w:p></w:body>
            </w:document>"#,
    )]);
    fs::write(&source_image, &docx_bytes)
        .expect("synthetic NTFS ADS DOCX source should be written");

    let mut file = sample_recovered_file();
    file.name = "Readme.txt".into();
    file.source_image_path = Some(source_image.to_string_lossy().to_string());
    file.alternate_data_streams = Some(vec![NamedFileFork {
        name: "DocPreview".into(),
        size_bytes: docx_bytes.len() as u64,
        expected_size_bytes: Some(docx_bytes.len() as u64),
        byte_runs: vec![ByteRun::physical(0, docx_bytes.len() as u64)],
    }]);

    let preview = build_file_auxiliary_preview(
        "scan-preview-ntfs-ads-docx",
        &file,
        "ads",
        Some("DocPreview"),
    )
    .expect("NTFS ADS DOCX preview should build");

    assert_eq!(preview.kind, "text");
    assert_eq!(
        preview.mime_type.as_deref(),
        Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
    );
    assert_eq!(preview.text_content.as_deref(), Some("ADS DOCX NOTE"));
    assert!(preview.asset_path.is_none());

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_auxiliary_preview_reads_ntfs_ads_pptx_content() {
    let source_root = unique_temp_dir("preview-ntfs-ads-pptx-source");
    let source_image = source_root.join("ntfs-ads-pptx-source.img");
    let pptx_bytes = build_zip_bytes(&[
        (
            "ppt/presentation.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
                <p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"></p:presentation>"#,
        ),
        (
            "ppt/slides/slide1.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
                <p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                       xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                  <p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>ADS PPTX NOTE</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld>
                </p:sld>"#,
        ),
    ]);
    fs::write(&source_image, &pptx_bytes)
        .expect("synthetic NTFS ADS PPTX source should be written");

    let mut file = sample_recovered_file();
    file.name = "Readme.txt".into();
    file.source_image_path = Some(source_image.to_string_lossy().to_string());
    file.alternate_data_streams = Some(vec![NamedFileFork {
        name: "DeckPreview".into(),
        size_bytes: pptx_bytes.len() as u64,
        expected_size_bytes: Some(pptx_bytes.len() as u64),
        byte_runs: vec![ByteRun::physical(0, pptx_bytes.len() as u64)],
    }]);

    let preview = build_file_auxiliary_preview(
        "scan-preview-ntfs-ads-pptx",
        &file,
        "ads",
        Some("DeckPreview"),
    )
    .expect("NTFS ADS PPTX preview should build");

    assert_eq!(preview.kind, "text");
    assert_eq!(
        preview.mime_type.as_deref(),
        Some("application/vnd.openxmlformats-officedocument.presentationml.presentation")
    );
    assert_eq!(preview.text_content.as_deref(), Some("ADS PPTX NOTE"));
    assert!(preview.asset_path.is_none());

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_auxiliary_preview_reads_ntfs_ads_pptx_notes_content() {
    let source_root = unique_temp_dir("preview-ntfs-ads-pptx-notes-source");
    let source_image = source_root.join("ntfs-ads-pptx-notes-source.img");
    let pptx_bytes = build_zip_bytes(&[
        (
            "ppt/presentation.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
                <p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"></p:presentation>"#,
        ),
        (
            "ppt/slides/slide1.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
                <p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                       xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                  <p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>ADS PPTX NOTE</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld>
                </p:sld>"#,
        ),
        (
            "ppt/notesSlides/notesSlide1.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
                <p:notes xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                         xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                  <p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Keep the attached speaker notes.</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld>
                </p:notes>"#,
        ),
    ]);
    fs::write(&source_image, &pptx_bytes)
        .expect("synthetic NTFS ADS PPTX notes source should be written");

    let mut file = sample_recovered_file();
    file.name = "Readme.txt".into();
    file.source_image_path = Some(source_image.to_string_lossy().to_string());
    file.alternate_data_streams = Some(vec![NamedFileFork {
        name: "DeckPreview".into(),
        size_bytes: pptx_bytes.len() as u64,
        expected_size_bytes: Some(pptx_bytes.len() as u64),
        byte_runs: vec![ByteRun::physical(0, pptx_bytes.len() as u64)],
    }]);

    let preview = build_file_auxiliary_preview(
        "scan-preview-ntfs-ads-pptx-notes",
        &file,
        "ads",
        Some("DeckPreview"),
    )
    .expect("NTFS ADS PPTX notes preview should build");

    assert_eq!(preview.kind, "text");
    assert_eq!(
        preview.text_content.as_deref(),
        Some("ADS PPTX NOTE\n\n[Speaker Notes]\nKeep the attached speaker notes.")
    );

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_file_auxiliary_preview_reads_hfsplus_resource_xlsx_content() {
    let source_root = unique_temp_dir("preview-hfsplus-resource-xlsx-source");
    let source_image = source_root.join("hfsplus-resource-xlsx-source.img");
    let xlsx_bytes = build_zip_bytes(&[
        (
            "xl/sharedStrings.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
                <sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                  <si><t>Case</t></si>
                  <si><t>Status</t></si>
                </sst>"#,
        ),
        (
            "xl/workbook.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
                <workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"></workbook>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
                <worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                  <sheetData>
                    <row r="1">
                      <c r="A1" t="s"><v>0</v></c>
                      <c r="B1" t="s"><v>1</v></c>
                    </row>
                    <row r="2">
                      <c r="A2" t="inlineStr"><is><t>R-204</t></is></c>
                      <c r="B2" t="inlineStr"><is><t>Ready</t></is></c>
                    </row>
                  </sheetData>
                </worksheet>"#,
        ),
    ]);
    fs::write(&source_image, &xlsx_bytes)
        .expect("synthetic HFS+ resource XLSX source should be written");

    let mut file = sample_recovered_file();
    file.name = "Deleted.txt".into();
    file.source_image_path = Some(source_image.to_string_lossy().to_string());
    file.resource_fork = Some(FileFork {
        size_bytes: xlsx_bytes.len() as u64,
        expected_size_bytes: Some(xlsx_bytes.len() as u64),
        byte_runs: vec![ByteRun::physical(0, xlsx_bytes.len() as u64)],
    });

    let preview = build_file_auxiliary_preview(
        "scan-preview-hfsplus-resource-xlsx",
        &file,
        "resource-fork",
        None,
    )
    .expect("HFS+ resource XLSX preview should build");

    assert_eq!(preview.kind, "text");
    assert_eq!(
        preview.mime_type.as_deref(),
        Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")
    );
    assert_eq!(
        preview.text_content.as_deref(),
        Some("Case\tStatus\nR-204\tReady")
    );
    assert!(preview.asset_path.is_none());

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn save_file_auxiliary_payload_to_path_writes_hfsplus_resource_fork_bytes() {
    let source_root = unique_temp_dir("save-hfsplus-resource-payload-source");
    let destination_root = unique_temp_dir("save-hfsplus-resource-payload-destination");
    let source_image = source_root.join("hfsplus-resource-payload-source.img");
    let resource_bytes = b"HFSPLUS RESOURCE";
    fs::write(&source_image, resource_bytes)
        .expect("synthetic HFS+ resource source should be written");

    let mut file = sample_recovered_file();
    file.name = "Deleted.txt".into();
    file.source_image_path = Some(source_image.to_string_lossy().to_string());
    file.resource_fork = Some(FileFork {
        size_bytes: resource_bytes.len() as u64,
        expected_size_bytes: Some(resource_bytes.len() as u64),
        byte_runs: vec![ByteRun::physical(0, resource_bytes.len() as u64)],
    });

    let destination_path = destination_root.join("Deleted.txt.resource-fork.bin");
    let written =
        save_file_auxiliary_payload_to_path(&file, "resource-fork", None, &destination_path)
            .expect("HFS+ resource fork payload should be saved");

    assert_eq!(written, resource_bytes.len() as u64);
    assert_eq!(
        fs::read(&destination_path).expect("saved resource-fork payload should exist"),
        resource_bytes
    );

    let _ = fs::remove_dir_all(source_root);
    let _ = fs::remove_dir_all(destination_root);
}

#[test]
fn save_file_auxiliary_payload_to_path_writes_ntfs_ads_bytes() {
    let source_root = unique_temp_dir("save-ntfs-ads-payload-source");
    let destination_root = unique_temp_dir("save-ntfs-ads-payload-destination");
    let source_image = source_root.join("ntfs-ads-payload-source.img");
    let ads_bytes = b"NTFS ADS PAYLOAD";
    fs::write(&source_image, ads_bytes).expect("synthetic NTFS ADS source should be written");

    let mut file = sample_recovered_file();
    file.name = "Note.txt".into();
    file.source_image_path = Some(source_image.to_string_lossy().to_string());
    file.alternate_data_streams = Some(vec![NamedFileFork {
        name: "Summary".into(),
        size_bytes: ads_bytes.len() as u64,
        expected_size_bytes: Some(ads_bytes.len() as u64),
        byte_runs: vec![ByteRun::physical(0, ads_bytes.len() as u64)],
    }]);

    let destination_path = destination_root.join("Note.txt.ads.Summary.bin");
    let written =
        save_file_auxiliary_payload_to_path(&file, "ads", Some("Summary"), &destination_path)
            .expect("NTFS ADS payload should be saved");

    assert_eq!(written, ads_bytes.len() as u64);
    assert_eq!(
        fs::read(&destination_path).expect("saved ADS payload should exist"),
        ads_bytes
    );

    let _ = fs::remove_dir_all(source_root);
    let _ = fs::remove_dir_all(destination_root);
}

#[test]
fn run_signature_carving_scan_detects_known_file_signatures() {
    let source_root = unique_temp_dir("signature-carving-source");
    let source_image = source_root.join("signature-source.img");
    fs::write(&source_image, jpeg_signature_image())
        .expect("synthetic carving source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "signature-carving", total_bytes);

    run_signature_carving_scan(
        "scan-signature-carving".into(),
        Arc::clone(&session),
        ImagingSourcePlan::Direct {
            source_path: source_image.clone(),
        },
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.progress.files_found, 1);
    assert_eq!(state.results.len(), 1);
    assert!(state.results[0].is_deleted);
    assert_eq!(state.results[0].recovery_method, "carving");
    assert_eq!(state.results[0].extension, "jpg");
    assert_eq!(state.results[0].start_offset, Some(24));
    assert!(state.results[0].source_image_path.is_some());
    assert!(state.results[0].byte_runs.is_some());
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Signature carving completed")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_signature_carving_scan_marks_corrupt_pdf_candidates() {
    let source_root = unique_temp_dir("signature-carving-source-corrupt-pdf");
    let source_image = source_root.join("signature-source-corrupt-pdf.img");
    fs::write(&source_image, corrupt_pdf_signature_image())
        .expect("synthetic corrupt pdf carving source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let session = scan_session_for_root(&source_image, "signature-carving", total_bytes);

    run_signature_carving_scan(
        "scan-signature-carving-corrupt-pdf".into(),
        Arc::clone(&session),
        ImagingSourcePlan::Direct {
            source_path: source_image.clone(),
        },
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.results.len(), 1);
    assert_eq!(state.results[0].extension, "pdf");
    assert_eq!(state.results[0].integrity, "corrupt");
    assert_eq!(state.results[0].recovery_score, 18);
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_export_session_reconstructs_carved_file_from_image() {
    let source_root = unique_temp_dir("signature-carving-export-source");
    let source_image = source_root.join("signature-source.img");
    fs::write(&source_image, jpeg_signature_image())
        .expect("synthetic carving source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let scan_session = scan_session_for_root(&source_image, "signature-carving", total_bytes);

    run_signature_carving_scan(
        "scan-signature-carving-export".into(),
        Arc::clone(&scan_session),
        ImagingSourcePlan::Direct {
            source_path: source_image.clone(),
        },
        total_bytes,
    );

    let carved_file = {
        let state = crate::commands::state::lock_or_recover(&scan_session, "scan session");
        state.results[0].clone()
    };

    let destination_root = unique_temp_dir("signature-carving-export-destination");
    let export_session = export_session_for_destination(
        "scan-signature-carving-export",
        &destination_root,
        1,
        carved_file.size_bytes,
    );

    run_export_session(
        "export-signature-carving".into(),
        Arc::clone(&export_session),
        source_root.clone(),
        destination_root.clone(),
        vec![carved_file.clone()],
        "rename".into(),
        true,
        true,
    );

    let export_state = export_session.lock().expect("export session lock poisoned");
    assert_eq!(export_state.progress.status, "completed");
    assert_eq!(export_state.progress.exported_files, 1);
    assert!(export_state
        .logs
        .iter()
        .any(|log| log.message.contains("Copied carved-0001.jpg")));
    drop(export_state);

    let exported_path = destination_root.join("carved").join("carved-0001.jpg");
    assert!(exported_path.exists());
    assert_eq!(
        fs::metadata(&exported_path)
            .expect("exported carved file metadata should exist")
            .len(),
        carved_file.size_bytes
    );

    let _ = fs::remove_dir_all(source_root);
    let _ = fs::remove_dir_all(destination_root);
}

#[test]
fn run_export_session_materializes_partial_deleted_file_size() {
    let source_root = unique_temp_dir("deleted-fat32-export-partial-source");
    let source_image = source_root.join("fat32-source-partial.img");
    fs::write(&source_image, partially_overwritten_deleted_fat32_image())
        .expect("synthetic partial FAT32 source image should be written");

    let total_bytes = fs::metadata(&source_image)
        .expect("source image metadata should exist")
        .len();
    let scan_session = scan_session_for_root(&source_image, "carving", total_bytes);

    run_deleted_fat32_scan(
        "scan-deleted-fat32-export-partial".into(),
        Arc::clone(&scan_session),
        source_image.clone(),
        total_bytes,
    );

    let scanned_files = {
        let state = crate::commands::state::lock_or_recover(&scan_session, "scan session");
        state.results.clone()
    };
    assert_eq!(scanned_files.len(), 1);
    assert_eq!(scanned_files[0].size_bytes, 512);
    assert_eq!(scanned_files[0].expected_size_bytes, Some(1200));

    let destination_root = unique_temp_dir("deleted-fat32-export-partial-destination");
    let export_session = export_session_for_destination(
        "scan-deleted-fat32-export-partial",
        &destination_root,
        scanned_files.len(),
        scanned_files.iter().map(|file| file.size_bytes).sum(),
    );

    run_export_session(
        "export-deleted-fat32-partial".into(),
        Arc::clone(&export_session),
        PathBuf::from("/dev/disk-test"),
        destination_root.clone(),
        scanned_files.clone(),
        "rename".into(),
        true,
        true,
    );

    let export_state = export_session.lock().expect("export session lock poisoned");
    assert_eq!(export_state.progress.status, "completed");
    assert_eq!(export_state.progress.exported_files, 1);
    assert!(export_state.progress.errors.is_empty());
    drop(export_state);

    let exported_path = destination_root.join("_IDEOPA.BIN");
    let exported_bytes =
        fs::read(&exported_path).expect("partial reconstructed export should be readable");
    assert_eq!(exported_bytes.len(), 512);
    assert!(exported_bytes.iter().all(|byte| *byte == 0x41));

    let _ = fs::remove_dir_all(source_root);
    let _ = fs::remove_dir_all(destination_root);
}

#[test]
fn run_export_session_reconstructs_deleted_file_from_image() {
    let source_root = unique_temp_dir("deleted-fat32-export-source");
    let source_image = source_root.join("fat32-source.img");
    fs::write(&source_image, minimal_deleted_fat32_image())
        .expect("synthetic FAT32 source image should be written");

    let destination_root = unique_temp_dir("deleted-fat32-export-destination");
    let export_session =
        export_session_for_destination("scan-deleted-fat32", &destination_root, 1, 11);
    let deleted_file = RecoveredFile {
        id: "deleted-1".into(),
        name: "_EPORT.TXT".into(),
        path: "/".into(),
        extension: "txt".into(),
        size_bytes: 11,
        created_at: Some("2024-03-14T09:26:12".into()),
        modified_at: Some("2024-03-15T16:08:00".into()),
        integrity: "intact".into(),
        recovery_score: 84,
        recovery_method: "reconstruction".into(),
        preview_available: true,
        mime_type: Some("text/plain".into()),
        expected_size_bytes: Some(11),
        deleted_at: None,
        start_offset: Some(1536),
        clusters: Some(vec![3]),
        byte_runs: Some(vec![ByteRun {
            offset: 1536,
            length: 512,
            zero_fill: false,
            ..Default::default()
        }]),
        resource_fork: None,
        alternate_data_streams: None,
        source_image_path: Some(source_image.to_string_lossy().to_string()),
        is_deleted: true,
        source_view: Some("recovery-image".into()),
        ..Default::default()
    };

    run_export_session(
        "export-deleted-fat32".into(),
        Arc::clone(&export_session),
        PathBuf::from("/dev/disk-test"),
        destination_root.clone(),
        vec![deleted_file],
        "rename".into(),
        true,
        true,
    );

    let export_state = export_session.lock().expect("export session lock poisoned");
    assert_eq!(export_state.progress.status, "completed");
    assert_eq!(export_state.progress.exported_files, 1);
    assert!(export_state.progress.errors.is_empty());
    assert!(export_state
        .logs
        .iter()
        .any(|log| log.message.contains("Copying _EPORT.TXT")));
    assert!(export_state
        .logs
        .iter()
        .any(|log| log.message.contains("Copied _EPORT.TXT")));
    drop(export_state);

    let exported_path = destination_root.join("_EPORT.TXT");
    assert!(exported_path.exists());
    assert_eq!(
        fs::read(&exported_path).expect("reconstructed export should be readable"),
        b"hello world"
    );

    let _ = fs::remove_dir_all(source_root);
    let _ = fs::remove_dir_all(destination_root);
}

#[test]
fn run_export_session_reconstructs_visible_file_from_recovery_image() {
    let source_root = unique_temp_dir("visible-fat32-export-source");
    let source_image = source_root.join("fat32-slice.img");
    fs::write(&source_image, minimal_deleted_fat32_image())
        .expect("synthetic FAT32 slice should be written");

    let destination_root = unique_temp_dir("visible-fat32-export-destination");
    let export_session =
        export_session_for_destination("scan-lost-volume-visible", &destination_root, 1, 9);
    let visible_file = RecoveredFile {
        id: "visible-1".into(),
        name: "LIVELOG.TXT".into(),
        path: "/".into(),
        extension: "txt".into(),
        size_bytes: 9,
        created_at: Some("2024-03-13T08:10:00".into()),
        modified_at: Some("2024-03-13T08:12:00".into()),
        integrity: "intact".into(),
        recovery_score: 98,
        recovery_method: "filesystem".into(),
        preview_available: true,
        mime_type: Some("text/plain".into()),
        expected_size_bytes: Some(9),
        deleted_at: None,
        start_offset: Some(2048),
        clusters: Some(vec![4]),
        byte_runs: Some(vec![ByteRun {
            offset: 2048,
            length: 512,
            zero_fill: false,
            ..Default::default()
        }]),
        resource_fork: None,
        alternate_data_streams: None,
        source_image_path: Some(source_image.to_string_lossy().to_string()),
        is_deleted: false,
        source_view: Some("recovery-image".into()),
        ..Default::default()
    };

    run_export_session(
        "export-visible-fat32".into(),
        Arc::clone(&export_session),
        PathBuf::from("/dev/disk-test"),
        destination_root.clone(),
        vec![visible_file],
        "rename".into(),
        true,
        true,
    );

    let export_state = export_session.lock().expect("export session lock poisoned");
    assert_eq!(export_state.progress.status, "completed");
    assert_eq!(export_state.progress.exported_files, 1);
    assert!(export_state.progress.errors.is_empty());
    assert!(export_state
        .logs
        .iter()
        .any(|log| log.message.contains("Copying LIVELOG.TXT")));
    assert!(export_state
        .logs
        .iter()
        .any(|log| log.message.contains("Copied LIVELOG.TXT")));
    drop(export_state);

    let exported_path = destination_root.join("LIVELOG.TXT");
    assert!(exported_path.exists());
    assert_eq!(
        fs::read(&exported_path).expect("reconstructed visible export should be readable"),
        b"live log!"
    );

    let _ = fs::remove_dir_all(source_root);
    let _ = fs::remove_dir_all(destination_root);
}

#[test]
fn build_diagnostic_marks_partition_lost_when_potential_volume_is_detected() {
    let source_root = unique_temp_dir("partition-lost-diagnostic");
    let source_image = source_root.join("partition-lost.img");
    let start_lba = 2048_u32;
    let sector_count = 8192_u32;
    let start_offset = start_lba as usize * 512;
    let mut bytes = vec![0_u8; (start_offset as u64 + sector_count as u64 * 512) as usize];

    bytes[446 + 4] = 0x0C;
    bytes[446 + 8..446 + 12].copy_from_slice(&start_lba.to_le_bytes());
    bytes[446 + 12..446 + 16].copy_from_slice(&sector_count.to_le_bytes());
    bytes[510] = 0x55;
    bytes[511] = 0xAA;

    let sector = &mut bytes[start_offset..start_offset + 512];
    sector[11..13].copy_from_slice(&512_u16.to_le_bytes());
    sector[13] = 8;
    sector[14..16].copy_from_slice(&32_u16.to_le_bytes());
    sector[16] = 2;
    sector[32..36].copy_from_slice(&sector_count.to_le_bytes());
    sector[36..40].copy_from_slice(&128_u32.to_le_bytes());
    sector[44..48].copy_from_slice(&2_u32.to_le_bytes());
    sector[82..90].copy_from_slice(b"FAT32   ");
    sector[510] = 0x55;
    sector[511] = 0xAA;

    fs::write(&source_image, bytes).expect("diagnostic fixture should be written");

    let diagnostic = build_diagnostic(&DetectedDevice {
        id: "device-test".into(),
        name: "Partition Lost Fixture".into(),
        device_path: source_image.to_string_lossy().to_string(),
        device_type: DeviceType::Usb,
        filesystem: FilesystemType::Unknown,
        capacity_bytes: fs::metadata(&source_image)
            .expect("fixture metadata should exist")
            .len(),
        used_bytes: 0,
        status: DeviceStatus::Healthy,
        risk_level: RiskLevel::Medium,
        serial: None,
        model: None,
        is_trim_enabled: Some(false),
        is_encrypted: Some(false),
        smart_available: Some(false),
        partitions: Vec::new(),
    });

    assert_eq!(diagnostic.loss_type, "partition-lost");
    assert!(diagnostic.potential_volumes_inspected);
    assert_eq!(diagnostic.potential_volumes.len(), 1);
    assert!(diagnostic
        .recommendations
        .iter()
        .any(|recommendation| recommendation.rec_type == "review-potential-volumes"));
    let direct_recommendation = diagnostic
        .recommendations
        .iter()
        .find(|recommendation| recommendation.rec_type == "scan-lost-volume")
        .expect("direct lost-volume recommendation should exist");
    assert_eq!(
        direct_recommendation.target_potential_volume_id.as_deref(),
        Some("pv-mbr-0")
    );
    assert_eq!(
        direct_recommendation
            .target_potential_volume_filesystem
            .as_deref(),
        Some("fat32")
    );

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn build_diagnostic_surfaces_apfs_potential_volume_as_limited_deleted_capability() {
    let source_root = unique_temp_dir("partition-lost-diagnostic-apfs");
    let source_image = source_root.join("partition-lost-apfs.img");
    let start_offset = 1024 * 1024;
    let apfs_image = minimal_apfs_container_image();
    let mut bytes = vec![0_u8; start_offset + apfs_image.len()];
    bytes[start_offset..start_offset + apfs_image.len()].copy_from_slice(&apfs_image);
    fs::write(&source_image, bytes).expect("diagnostic APFS fixture should be written");

    let diagnostic = build_diagnostic(&DetectedDevice {
        id: "device-apfs-test".into(),
        name: "APFS Candidate Fixture".into(),
        device_path: source_image.to_string_lossy().to_string(),
        device_type: DeviceType::External,
        filesystem: FilesystemType::Unknown,
        capacity_bytes: fs::metadata(&source_image)
            .expect("fixture metadata should exist")
            .len(),
        used_bytes: 0,
        status: DeviceStatus::Healthy,
        risk_level: RiskLevel::Medium,
        serial: None,
        model: None,
        is_trim_enabled: Some(false),
        is_encrypted: Some(false),
        smart_available: Some(false),
        partitions: Vec::new(),
    });

    assert_eq!(diagnostic.loss_type, "partition-lost");
    assert!(diagnostic
        .potential_volumes
        .iter()
        .any(|volume| matches!(volume.filesystem, FilesystemType::Apfs)));
    assert!(diagnostic
        .recommendations
        .iter()
        .any(|recommendation| recommendation.rec_type == "scan-lost-volume"));
    assert!(diagnostic
        .recommendations
        .iter()
        .any(|recommendation| recommendation.rec_type == "review-potential-volumes"));
    assert!(diagnostic
        .limitations
        .iter()
        .any(|limitation| limitation.contains("apfs")));
    assert!(diagnostic
        .probable_causes
        .iter()
        .any(|cause| cause.contains("apfs") || cause.contains("diagnostic.observed")));

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn guided_supported_potential_volume_candidate_prefers_a_clear_winner() {
    let volumes = vec![
        PotentialVolume {
            id: "pv-boot".into(),
            label: "Potential exFAT volume @ 0x200000".into(),
            filesystem: FilesystemType::Exfat,
            start_offset: 0x200000,
            size_bytes: Some(16 * 1024 * 1024),
            confidence_score: 74,
            detection_method: "boot-signature".into(),
            notes: Vec::new(),
        },
        PotentialVolume {
            id: "pv-mbr-0".into(),
            label: "MBR partition 1 @ 0x100000".into(),
            filesystem: FilesystemType::Fat32,
            start_offset: 0x100000,
            size_bytes: Some(32 * 1024 * 1024),
            confidence_score: 90,
            detection_method: "mbr".into(),
            notes: Vec::new(),
        },
    ];

    let candidate = guided_supported_potential_volume_candidate(&volumes)
        .expect("a clear supported winner should be selected");
    assert_eq!(candidate.id, "pv-mbr-0");
}

#[test]
fn guided_supported_potential_volume_candidate_rejects_ambiguous_candidates() {
    let volumes = vec![
        PotentialVolume {
            id: "pv-mbr-0".into(),
            label: "MBR partition 1 @ 0x100000".into(),
            filesystem: FilesystemType::Fat32,
            start_offset: 0x100000,
            size_bytes: Some(32 * 1024 * 1024),
            confidence_score: 90,
            detection_method: "mbr".into(),
            notes: Vec::new(),
        },
        PotentialVolume {
            id: "pv-mbr-1".into(),
            label: "MBR partition 2 @ 0x200000".into(),
            filesystem: FilesystemType::Ntfs,
            start_offset: 0x200000,
            size_bytes: Some(64 * 1024 * 1024),
            confidence_score: 88,
            detection_method: "mbr".into(),
            notes: Vec::new(),
        },
    ];

    assert!(
        guided_supported_potential_volume_candidate(&volumes).is_none(),
        "close supported candidates should remain an explicit expert decision"
    );
}

#[test]
fn run_potential_volume_scan_recovers_deleted_entries_from_detected_fat32_candidate() {
    let source_root = unique_temp_dir("lost-volume-fat32-source");
    let source_image = source_root.join("lost-volume-fat32.img");
    let embedded_volume = minimal_deleted_fat32_image();
    let start_lba = 2048_u32;
    let sector_count = (embedded_volume.len() / 512) as u32;
    let start_offset = start_lba as usize * 512;
    let mut raw_image = vec![0_u8; start_offset + embedded_volume.len()];

    raw_image[446 + 4] = 0x0C;
    raw_image[446 + 8..446 + 12].copy_from_slice(&start_lba.to_le_bytes());
    raw_image[446 + 12..446 + 16].copy_from_slice(&sector_count.to_le_bytes());
    raw_image[510] = 0x55;
    raw_image[511] = 0xAA;
    raw_image[start_offset..start_offset + embedded_volume.len()].copy_from_slice(&embedded_volume);
    fs::write(&source_image, raw_image).expect("lost-volume source image should be written");

    let candidate = partitioning::inspect_potential_volumes(&source_image)
        .expect("potential volumes should be detected")
        .into_iter()
        .find(|volume| volume.id == "pv-mbr-0")
        .expect("MBR FAT32 candidate should exist");
    let total_bytes = fs::metadata(&source_image)
        .expect("lost-volume metadata should exist")
        .len()
        .saturating_add(candidate.size_bytes.unwrap_or(0));
    let session = scan_session_for_root(&source_image, "lost-volume", total_bytes);

    run_potential_volume_scan(
        "scan-lost-volume-fat32".into(),
        Arc::clone(&session),
        ImagingSourcePlan::Direct {
            source_path: source_image.clone(),
        },
        candidate,
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.progress.files_found, 2);
    assert_eq!(state.scan_type, "lost-volume");
    assert_eq!(state.results.len(), 2);
    let visible = state
        .results
        .iter()
        .find(|file| file.name == "LIVELOG.TXT")
        .expect("visible FAT32 slice file should be present");
    assert!(!visible.is_deleted);
    assert_eq!(visible.recovery_method, "filesystem");
    assert!(visible.source_image_path.is_some());
    assert_eq!(visible.start_offset, Some((start_offset + 2048) as u64));
    let deleted = state
        .results
        .iter()
        .find(|file| file.name == "_EPORT.TXT")
        .expect("deleted FAT32 slice file should be present");
    assert!(deleted.is_deleted);
    assert_eq!(deleted.start_offset, Some((start_offset + 1536) as u64));
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Recovered-volume analysis completed")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_potential_volume_scan_catalogs_visible_and_deleted_hfsplus_files_from_detected_candidate() {
    let source_root = unique_temp_dir("lost-volume-hfsplus-source");
    let source_image = source_root.join("lost-volume-hfsplus.img");
    let embedded_volume =
        hfsplus::synthetic_deleted_hfsplus_image_for_tests(b"hello hfs", b"deleted hfs");
    let start_lba = 2048_u32;
    let sector_count = (embedded_volume.len() / 512) as u32;
    let start_offset = start_lba as usize * 512;
    let mut raw_image = vec![0_u8; start_offset + embedded_volume.len()];

    raw_image[446 + 4] = 0xAF;
    raw_image[446 + 8..446 + 12].copy_from_slice(&start_lba.to_le_bytes());
    raw_image[446 + 12..446 + 16].copy_from_slice(&sector_count.to_le_bytes());
    raw_image[510] = 0x55;
    raw_image[511] = 0xAA;
    raw_image[start_offset..start_offset + embedded_volume.len()].copy_from_slice(&embedded_volume);
    fs::write(&source_image, raw_image).expect("lost-volume HFS+ source image should be written");

    let candidate = partitioning::inspect_potential_volumes(&source_image)
        .expect("potential volumes should be detected")
        .into_iter()
        .find(|volume| volume.id == "pv-mbr-0")
        .expect("MBR HFS+ candidate should exist");
    assert!(matches!(candidate.filesystem, FilesystemType::HfsPlus));

    let total_bytes = fs::metadata(&source_image)
        .expect("lost-volume metadata should exist")
        .len()
        .saturating_add(candidate.size_bytes.unwrap_or(0));
    let session = scan_session_for_root(&source_image, "lost-volume", total_bytes);

    run_potential_volume_scan(
        "scan-lost-volume-hfsplus".into(),
        Arc::clone(&session),
        ImagingSourcePlan::Direct {
            source_path: source_image.clone(),
        },
        candidate,
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.progress.files_found, 2);
    assert_eq!(state.scan_type, "lost-volume");
    let visible = state
        .results
        .iter()
        .find(|file| file.name == "Report.txt")
        .expect("visible HFS+ slice file should be present");
    assert!(!visible.is_deleted);
    assert_eq!(visible.path, "/Docs");
    assert_eq!(visible.recovery_method, "filesystem");
    assert_eq!(visible.start_offset, Some((start_offset + 12_288) as u64));
    let deleted = state
        .results
        .iter()
        .find(|file| file.name == "Deleted.txt")
        .expect("deleted HFS+ slice file should be present");
    assert!(deleted.is_deleted);
    assert_eq!(deleted.path, "/Docs");
    assert_eq!(deleted.recovery_method, "reconstruction");
    assert_eq!(deleted.start_offset, Some((start_offset + 16_384) as u64));
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Recovered-volume analysis completed")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn run_potential_volume_scan_catalogs_hfsplus_overflow_backed_files_from_detected_candidate() {
    let source_root = unique_temp_dir("lost-volume-hfsplus-overflow-source");
    let source_image = source_root.join("lost-volume-hfsplus-overflow.img");
    let embedded_volume = hfsplus::synthetic_deleted_hfsplus_overflow_image_for_tests();
    let start_lba = 2048_u32;
    let sector_count = (embedded_volume.len() / 512) as u32;
    let start_offset = start_lba as usize * 512;
    let mut raw_image = vec![0_u8; start_offset + embedded_volume.len()];

    raw_image[446 + 4] = 0xAF;
    raw_image[446 + 8..446 + 12].copy_from_slice(&start_lba.to_le_bytes());
    raw_image[446 + 12..446 + 16].copy_from_slice(&sector_count.to_le_bytes());
    raw_image[510] = 0x55;
    raw_image[511] = 0xAA;
    raw_image[start_offset..start_offset + embedded_volume.len()].copy_from_slice(&embedded_volume);
    fs::write(&source_image, raw_image)
        .expect("lost-volume HFS+ overflow source image should be written");

    let candidate = partitioning::inspect_potential_volumes(&source_image)
        .expect("potential volumes should be detected")
        .into_iter()
        .find(|volume| volume.id == "pv-mbr-0")
        .expect("MBR HFS+ overflow candidate should exist");
    assert!(matches!(candidate.filesystem, FilesystemType::HfsPlus));

    let total_bytes = fs::metadata(&source_image)
        .expect("lost-volume overflow metadata should exist")
        .len()
        .saturating_add(candidate.size_bytes.unwrap_or(0));
    let session = scan_session_for_root(&source_image, "lost-volume", total_bytes);

    run_potential_volume_scan(
        "scan-lost-volume-hfsplus-overflow".into(),
        Arc::clone(&session),
        ImagingSourcePlan::Direct {
            source_path: source_image.clone(),
        },
        candidate,
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert_eq!(state.scan_type, "lost-volume");
    let visible = state
        .results
        .iter()
        .find(|file| file.name == "Overflow.txt")
        .expect("visible HFS+ overflow slice file should be present");
    assert!(!visible.is_deleted);
    assert_eq!(visible.path, "/Docs");
    assert_eq!(visible.recovery_method, "filesystem");
    assert_eq!(visible.start_offset, Some((start_offset + 16_384) as u64));
    assert_eq!(visible.expected_size_bytes, Some((8 * 4096 + 13) as u64));

    let deleted = state
        .results
        .iter()
        .find(|file| file.name == "DeletedOverflow.txt")
        .expect("deleted HFS+ overflow slice file should be present");
    assert!(deleted.is_deleted);
    assert_eq!(deleted.path, "/Docs");
    assert_eq!(deleted.recovery_method, "reconstruction");
    assert_eq!(deleted.start_offset, Some((start_offset + 20_480) as u64));
    assert_eq!(deleted.expected_size_bytes, Some((8 * 4096 + 15) as u64));
    assert!(state
        .logs
        .iter()
        .any(|log| log.message.contains("Recovered-volume analysis completed")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
}

#[cfg(target_os = "macos")]
#[test]
fn run_potential_volume_scan_catalogs_visible_apfs_files_from_detected_candidate() {
    let fixture = apfs::test_support::create_raw_apfs_image_for_tests(&[
        ("hello.txt", b"hello apfs"),
        ("docs/note.md", b"nested file"),
    ])
    .expect("APFS fixture should be created");
    let source_root = unique_temp_dir("lost-volume-apfs-source");
    let source_image = source_root.join("lost-volume-apfs.img");
    let embedded_volume =
        fs::read(&fixture.image_path).expect("APFS raw fixture bytes should be readable");
    let start_offset = 1024 * 1024;
    let mut raw_image = vec![0_u8; start_offset + embedded_volume.len()];
    raw_image[start_offset..start_offset + embedded_volume.len()].copy_from_slice(&embedded_volume);
    fs::write(&source_image, raw_image).expect("lost-volume APFS source image should be written");

    let candidate = partitioning::inspect_potential_volumes(&source_image)
        .expect("potential volumes should be detected")
        .into_iter()
        .find(|volume| matches!(volume.filesystem, FilesystemType::Apfs))
        .expect("APFS candidate should exist");

    let total_bytes = fs::metadata(&source_image)
        .expect("lost-volume metadata should exist")
        .len()
        .saturating_add(candidate.size_bytes.unwrap_or(0));
    let session = scan_session_for_root(&source_image, "lost-volume", total_bytes);

    run_potential_volume_scan(
        "scan-lost-volume-apfs".into(),
        Arc::clone(&session),
        ImagingSourcePlan::Direct {
            source_path: source_image.clone(),
        },
        candidate,
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    assert_eq!(state.progress.status, "completed");
    assert!(state.progress.files_found >= 2);
    let hello = state
        .results
        .iter()
        .find(|file| file.name == "hello.txt")
        .expect("visible APFS hello.txt file should be present");
    assert!(!hello.is_deleted);
    assert_eq!(hello.path, "/");
    assert_eq!(hello.recovery_method, "filesystem");
    assert!(hello
        .start_offset
        .is_some_and(|offset| offset >= start_offset as u64));
    assert!(hello
        .byte_runs
        .as_ref()
        .is_some_and(|runs| !runs.is_empty()));

    let note = state
        .results
        .iter()
        .find(|file| file.name == "note.md")
        .expect("visible APFS nested file should be present");
    assert_eq!(note.path, "/docs");
    assert!(state.logs.iter().any(|log| log
        .message
        .contains("Scanning deleted APFS orphaned catalog inodes")));
    drop(state);

    let _ = fs::remove_dir_all(source_root);
    let _ = fs::remove_dir_all(fixture.root_dir);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "APFS orphan-catalog fixture is not deterministic on macOS 15.7.4; tracked under TT-05/TT-01"]
fn run_deleted_apfs_scan_marks_results_as_live_catalog_provenance() {
    let fixture = apfs::test_support::create_raw_apfs_image_with_deleted_files_for_tests(
        &[("hello.txt", b"hello apfs")],
        &[("deleted.txt", b"deleted apfs payload")],
    )
    .expect("APFS fixture with deleted file should be created");

    let total_bytes = fs::metadata(&fixture.image_path)
        .expect("APFS fixture metadata should exist")
        .len();
    let session = scan_session_for_root(&fixture.image_path, "carving", total_bytes);

    run_deleted_apfs_scan(
        "scan-deleted-apfs-source-view".into(),
        Arc::clone(&session),
        fixture.image_path.clone(),
        total_bytes,
    );

    let state = crate::commands::state::lock_or_recover(&session, "scan session");
    let deleted = state
        .results
        .iter()
        .find(|file| file.is_deleted)
        .expect("deleted APFS result should exist");
    assert_eq!(deleted.source_view.as_deref(), Some("live-catalog"));
    assert_eq!(deleted.recovery_method, "reconstruction");
    assert_eq!(deleted.path, "/orphaned-apfs-catalog");
    assert_eq!(deleted.validator_status.as_deref(), Some("reassembled"));
    assert_eq!(deleted.recovery_complexity.as_deref(), Some("medium"));
    assert_eq!(deleted.assembly_segment_count, Some(1));
    assert_eq!(deleted.gap_count, Some(0));
    drop(state);

    let _ = fs::remove_dir_all(fixture.root_dir);
}
