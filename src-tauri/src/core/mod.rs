// ============================================================================
// Hardware Detector — Real OS-level disk detection
// ============================================================================
// SAFETY: This module is strictly READ-ONLY.
// It uses sysinfo for cross-platform mounted volume detection,
// and platform-specific tools for detailed device metadata:
//   - macOS:   `diskutil` + `system_profiler SPNVMeDataType SPStorageDataType`
//   - Linux:   `lsblk -Jbo NAME,MODEL,SERIAL,ROTA,TRAN,FSTYPE` + `findmnt`
//              (SMART/udev enrichment stays a future iteration).
//   - Windows: `sysinfo` for the base catalog + PowerShell
//              `Get-Partition | Get-Disk` to resolve a drive letter to the
//              `\\.\PhysicalDriveN` path used by the imaging pipeline. WMI
//              is intentionally not pulled in as a dependency yet — the
//              PowerShell shell-out is enough for the current command set.
// ============================================================================

pub mod raw_disks;

use crate::types::*;
use std::{
    fs::File,
    io::ErrorKind,
    path::{Path, PathBuf},
};

#[cfg(target_os = "macos")]
use std::process::Command;

// ============================================================================
// Platform-specific metadata enrichment
// ============================================================================

/// Extra device info obtained from platform-specific tools.
#[derive(Default)]
struct PlatformDeviceInfo {
    serial: Option<String>,
    model: Option<String>,
    trim_enabled: Option<bool>,
    encrypted: Option<bool>,
    smart_available: Option<bool>,
}

// ---------------------------------------------------------------------------
// macOS implementation
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn get_device_path_for_mount(mount_point: &str) -> Option<String> {
    let output = Command::new("df").arg(mount_point).output().ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    // df output: first line is header, second line starts with /dev/diskXsY
    let line = stdout.lines().nth(1)?;
    let path = line.split_whitespace().next()?;
    if path.starts_with("/dev/") {
        Some(path.to_string())
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn get_platform_device_info(device_path: &str) -> PlatformDeviceInfo {
    let mut info = PlatformDeviceInfo::default();

    // Extract the base disk (e.g., disk1 from /dev/disk1s1)
    let base_disk = extract_base_disk(device_path);
    let disk_to_query = base_disk.as_deref().unwrap_or(device_path);

    let output = match Command::new("diskutil")
        .arg("info")
        .arg(disk_to_query)
        .output()
    {
        Ok(o) => o,
        Err(_) => return info,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        let line = line.trim();

        if line.starts_with("Device / Media Name:") {
            info.model = Some(line.split(':').nth(1).unwrap_or("").trim().to_string());
        } else if line.starts_with("Disk Size:") || line.starts_with("Total Size:") {
            // Already have size from sysinfo
        } else if line.starts_with("TRIM Support:") {
            let val = line.split(':').nth(1).unwrap_or("").trim().to_lowercase();
            info.trim_enabled = Some(val.contains("yes"));
        } else if line.starts_with("Encryption:") || line.starts_with("FileVault:") {
            let val = line.split(':').nth(1).unwrap_or("").trim().to_lowercase();
            info.encrypted =
                Some(val.contains("yes") || val.contains("unlocked") || val.contains("locked"));
        } else if line.starts_with("SMART Status:") {
            let val = line.split(':').nth(1).unwrap_or("").trim().to_lowercase();
            info.smart_available = Some(!val.contains("not supported"));
        } else if line.starts_with("Device Block Size:") {
            // Could use for expert mode
        }
    }

    // Try to get serial from system_profiler if not found
    if info.serial.is_none() {
        info.serial = get_serial_for_disk(disk_to_query);
    }

    info
}

/// Try to get disk serial number from system_profiler (macOS).
#[cfg(target_os = "macos")]
fn get_serial_for_disk(_disk: &str) -> Option<String> {
    // system_profiler SPSerialATADataType or SPNVMeDataType
    let output = Command::new("system_profiler")
        .arg("SPNVMeDataType")
        .arg("-json")
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Simple JSON parsing to find serial number
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&stdout) {
        if let Some(items) = value.get("SPNVMeDataType").and_then(|v| v.as_array()) {
            for item in items {
                if let Some(serial) = item
                    .get("device_serial")
                    .or_else(|| item.get("serial_number"))
                    .and_then(|v| v.as_str())
                {
                    return Some(serial.trim().to_string());
                }
                // Check nested items
                if let Some(sub_items) = item.get("_items").and_then(|v| v.as_array()) {
                    for sub in sub_items {
                        if let Some(serial) = sub
                            .get("device_serial")
                            .or_else(|| sub.get("serial_number"))
                            .and_then(|v| v.as_str())
                        {
                            return Some(serial.trim().to_string());
                        }
                    }
                }
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Linux implementation (lsblk + udevadm + smartctl)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn get_device_path_for_mount(mount_point: &str) -> Option<String> {
    let out = std::process::Command::new("findmnt")
        .args(["-n", "-o", "SOURCE", mount_point])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(target_os = "linux")]
fn get_platform_device_info(device_path: &str) -> PlatformDeviceInfo {
    let mut info = PlatformDeviceInfo::default();

    // lsblk -Jbo NAME,MODEL,SERIAL,ROTA,TRAN,FSTYPE <device>
    if let Ok(out) = std::process::Command::new("lsblk")
        .args(["-Jbo", "NAME,MODEL,SERIAL,ROTA,TRAN,FSTYPE", device_path])
        .output()
    {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
                if let Some(arr) = v.get("blockdevices").and_then(|x| x.as_array()) {
                    if let Some(d) = arr.first() {
                        info.model = d
                            .get("model")
                            .and_then(|x| x.as_str())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty());
                        info.serial = d
                            .get("serial")
                            .and_then(|x| x.as_str())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty());
                        // ROTA=0 → SSD/NVMe → TRIM commonly available
                        if let Some(rota) = d.get("rota").and_then(|x| x.as_u64()) {
                            info.trim_enabled = Some(rota == 0);
                        }
                        // LUKS detection via fstype
                        if let Some(fst) = d.get("fstype").and_then(|x| x.as_str()) {
                            if fst.starts_with("crypto_LUKS") {
                                info.encrypted = Some(true);
                            }
                        }
                    }
                }
            }
        }
    }

    // smartctl -H -j <device> (best-effort; may need root)
    if let Ok(out) = std::process::Command::new("smartctl")
        .args(["-H", "-j", device_path])
        .output()
    {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
            if let Some(passed) = v
                .get("smart_status")
                .and_then(|s| s.get("passed"))
                .and_then(|p| p.as_bool())
            {
                info.smart_available = Some(passed || true);
                let _ = passed;
            }
        }
    }

    info
}

// ---------------------------------------------------------------------------
// Windows implementation (PowerShell Get-PhysicalDisk + Get-Partition)
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn get_device_path_for_mount(mount_point: &str) -> Option<String> {
    // mount_point is typically a drive letter like "C:\". Map drive letter →
    // physical disk number via PowerShell: Get-Partition -DriveLetter C | Select DiskNumber
    let letter = mount_point.chars().next()?;
    if !letter.is_ascii_alphabetic() {
        return None;
    }
    let script = format!(
        "(Get-Partition -DriveLetter {} -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty DiskNumber)",
        letter
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let n = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if n.is_empty() {
        return None;
    }
    Some(format!("\\\\.\\PhysicalDrive{}", n))
}

#[cfg(target_os = "windows")]
fn get_platform_device_info(device_path: &str) -> PlatformDeviceInfo {
    let mut info = PlatformDeviceInfo::default();
    // Extract physical disk number from \\.\PhysicalDriveN
    let num: Option<u32> = device_path
        .strip_prefix("\\\\.\\PhysicalDrive")
        .and_then(|s| s.parse().ok());
    let Some(n) = num else {
        return info;
    };

    let script = format!(
        "Get-PhysicalDisk -DeviceNumber {} | Select-Object Model,SerialNumber,MediaType,HealthStatus,BusType | ConvertTo-Json -Compress",
        n
    );
    let Ok(out) = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
    else {
        return info;
    };
    if !out.status.success() {
        return info;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
        info.model = v
            .get("Model")
            .and_then(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        info.serial = v
            .get("SerialNumber")
            .and_then(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(media) = v.get("MediaType").and_then(|x| x.as_str()) {
            // SSD = 4, HDD = 3 in MSFT_PhysicalDisk; here it serializes as string
            info.trim_enabled = Some(media.eq_ignore_ascii_case("SSD"));
        }
        if let Some(health) = v.get("HealthStatus").and_then(|x| x.as_str()) {
            info.smart_available = Some(!health.is_empty());
        }
    }
    info
}

// ============================================================================
// Cross-platform public API (sysinfo-based)
// ============================================================================

/// Detect all storage devices on the system.
/// Uses sysinfo for mounted volumes + platform-specific tools for metadata.
pub fn detect_devices() -> Vec<DetectedDevice> {
    let mut devices: Vec<DetectedDevice> = Vec::new();

    // Use sysinfo to get mounted disks
    let disks = sysinfo::Disks::new_with_refreshed_list();

    for (idx, disk) in disks.list().iter().enumerate() {
        let mount_point = disk.mount_point().to_string_lossy().to_string();
        let name = disk.name().to_string_lossy().to_string();
        let fs = disk.file_system().to_string_lossy().to_lowercase();
        let total = disk.total_space();
        let available = disk.available_space();
        let used = total.saturating_sub(available);
        let is_removable = disk.is_removable();

        // Determine device type from sysinfo disk kind
        let device_type = match disk.kind() {
            sysinfo::DiskKind::SSD => DeviceType::Ssd,
            sysinfo::DiskKind::HDD => DeviceType::Hdd,
            _ => {
                if is_removable {
                    DeviceType::Usb
                } else {
                    DeviceType::Ssd // default to SSD on modern machines
                }
            }
        };

        // Map filesystem string
        let filesystem = match fs.as_str() {
            "apfs" => FilesystemType::Apfs,
            "ntfs" => FilesystemType::Ntfs,
            "msdos" | "fat32" | "vfat" => FilesystemType::Fat32,
            "exfat" => FilesystemType::Exfat,
            "hfs" | "hfs+" => FilesystemType::HfsPlus,
            "ext4" => FilesystemType::Ext4,
            _ => FilesystemType::Unknown,
        };

        // Determine risk level based on usage and type
        let risk_level = if is_removable {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        };

        // Determine status
        let status = DeviceStatus::Healthy;

        // Try to get device path from mount point (platform-specific)
        let device_path =
            get_device_path_for_mount(&mount_point).unwrap_or_else(|| mount_point.clone());

        // Get extra info via platform-specific tools
        let platform_info = get_platform_device_info(&device_path);

        let display_name = if name.is_empty() {
            mount_point.clone()
        } else {
            name.clone()
        };

        let dev = DetectedDevice {
            id: format!("dev-{}", idx),
            name: display_name,
            device_path: device_path.clone(),
            device_type: device_type.clone(),
            filesystem: filesystem.clone(),
            capacity_bytes: total,
            used_bytes: used,
            status,
            risk_level,
            serial: platform_info.serial.clone(),
            model: platform_info.model.clone(),
            is_trim_enabled: platform_info.trim_enabled,
            is_encrypted: platform_info.encrypted,
            smart_available: platform_info.smart_available,
            partitions: vec![Partition {
                id: format!("p-{}", idx),
                label: name,
                filesystem,
                size_bytes: total,
                start_offset: 0,
                mount_path: Some(mount_point.clone()),
                is_mounted: true,
                is_bootable: mount_point == "/" || mount_point == "C:\\",
            }],
        };

        devices.push(dev);
    }

    // Deduplicate by underlying physical disk. On macOS APFS in particular a
    // single physical SSD exposes several synthesized volumes (Macintosh HD,
    // Macintosh HD - Data, Preboot, VM, Update...) and `sysinfo` reports each
    // one as a separate "disk". Without grouping them the user sees the same
    // SSD listed multiple times. We collapse them on:
    //   1. (model + serial) when both are known — strongest signal, also
    //      catches APFS synthesized → physical disk indirection.
    //   2. Otherwise the platform "base disk" identifier (e.g. `disk3` for
    //      `/dev/disk3s5`, `sda` for `/dev/sda1`).
    //
    // Among duplicates we keep the entry with the largest used space — that
    // is almost always the user's main data volume rather than `Preboot` or
    // `VM` which are nearly empty.
    fn dedup_key(device: &DetectedDevice) -> String {
        match (&device.model, &device.serial) {
            (Some(model), Some(serial)) if !model.is_empty() && !serial.is_empty() => {
                format!("hw::{model}::{serial}")
            }
            _ => extract_base_disk(&device.device_path)
                .map(|base| format!("base::{base}"))
                .unwrap_or_else(|| format!("path::{}", device.device_path)),
        }
    }

    devices.sort_by(|a, b| {
        let ka = dedup_key(a);
        let kb = dedup_key(b);
        ka.cmp(&kb).then_with(|| b.used_bytes.cmp(&a.used_bytes))
    });
    devices.dedup_by(|a, b| dedup_key(a) == dedup_key(b));

    // Surface raw block devices that `sysinfo` missed because they're not
    // mounted. This covers RAID members, wiped partition tables, LUKS /
    // FileVault volumes pre-unlock, and freshly-plugged external drives
    // that never mounted. Each entry is flagged `DeviceStatus::Unresponsive`
    // and `RiskLevel::High` so the UI pushes the user to image first.
    let sysinfo_paths: std::collections::HashSet<String> = devices
        .iter()
        .map(|device| device.device_path.clone())
        .collect();
    for raw in raw_disks::enumerate_unmounted_disks() {
        let device_path = raw.device_path.to_string_lossy().to_string();
        if sysinfo_paths.contains(&device_path) {
            continue;
        }
        devices.push(DetectedDevice {
            id: String::new(), // filled in by the dev-N rewrite below
            name: raw.label.clone().unwrap_or_else(|| raw.kernel_name.clone()),
            device_path,
            device_type: match raw.removable {
                Some(true) => DeviceType::Usb,
                _ => DeviceType::Hdd,
            },
            filesystem: FilesystemType::Unknown,
            capacity_bytes: raw.size_bytes.unwrap_or(0),
            used_bytes: 0,
            status: DeviceStatus::Unresponsive,
            risk_level: RiskLevel::High,
            serial: None,
            model: raw.label,
            is_trim_enabled: None,
            is_encrypted: None,
            smart_available: None,
            partitions: Vec::new(),
        });
    }

    // Reassign stable, sequential IDs after deduplication + raw-disk append
    // so the front-end doesn't end up with gaps like "dev-0", "dev-3" that
    // would confuse selection state.
    for (idx, device) in devices.iter_mut().enumerate() {
        device.id = format!("dev-{idx}");
        for (pidx, partition) in device.partitions.iter_mut().enumerate() {
            partition.id = format!("p-{idx}-{pidx}");
        }
    }

    devices.extend(crate::imported_sources::list_imported_devices());

    devices
}

pub fn primary_mount_path(device: &DetectedDevice) -> Option<PathBuf> {
    device
        .partitions
        .iter()
        .find_map(|partition| partition.mount_path.as_ref())
        .map(PathBuf::from)
}

/// Resolve the base device identifier for a path or device string.
///
/// Examples (macOS):
/// - "/dev/disk3s1" -> "disk3"
/// - "/Volumes/Backup" -> device backing that mount point, when available
///
/// Examples (Linux):
/// - "/dev/sda1" -> "sda"
///
/// Examples (Windows):
/// - "\\.\PhysicalDrive0" -> "PhysicalDrive0"
pub fn resolve_base_device_identifier(path_or_device: &str) -> Option<String> {
    // macOS / Linux: device paths start with /dev/
    if path_or_device.starts_with("/dev/") {
        return extract_base_disk(path_or_device);
    }

    // Windows: device paths start with \\.\
    #[cfg(target_os = "windows")]
    if path_or_device.starts_with("\\\\.\\") {
        return extract_base_disk(path_or_device);
    }

    let candidate = normalize_existing_path(path_or_device)?;
    let device_path = get_device_path_for_mount(candidate.to_string_lossy().as_ref())?;
    extract_base_disk(&device_path)
}

/// Return the available bytes for the disk containing the provided path.
pub fn available_space_for_path(path: &str) -> Option<u64> {
    let candidate = normalize_existing_path(path)?;
    let disks = sysinfo::Disks::new_with_refreshed_list();

    disks
        .list()
        .iter()
        .filter_map(|disk| {
            let mount_point = disk.mount_point();
            if candidate.starts_with(mount_point) {
                Some((mount_point.as_os_str().len(), disk.available_space()))
            } else {
                None
            }
        })
        .max_by_key(|(mount_len, _)| *mount_len)
        .map(|(_, available)| available)
}

// ============================================================================
// preferred_imaging_source_path — platform-specific
// ============================================================================

/// Return the preferred physical source path for raw read-only imaging.
///
/// - **macOS**: mounted partitions like `/dev/disk4s2` are promoted to the
///   whole raw device (`/dev/rdisk4`) so imaging targets the underlying media
///   rather than a single slice.
/// - **Linux**: partitions like `/dev/sda1` are promoted to the whole device
///   (`/dev/sda`).
/// - **Windows**: volume paths are mapped to `\\.\PhysicalDriveN`.
#[cfg(target_os = "macos")]
pub fn preferred_imaging_source_path(device_path: &str) -> PathBuf {
    if device_path.starts_with("/dev/disk") {
        if let Some(base_disk) = extract_base_disk(device_path) {
            let raw_device = PathBuf::from(format!("/dev/r{base_disk}"));
            if raw_device.exists() {
                return raw_device;
            }

            return PathBuf::from(format!("/dev/{base_disk}"));
        }
    }

    PathBuf::from(device_path)
}

#[cfg(target_os = "linux")]
pub fn preferred_imaging_source_path(device_path: &str) -> PathBuf {
    // Promote partition device (e.g. /dev/sda1) to whole disk (/dev/sda)
    if device_path.starts_with("/dev/sd") || device_path.starts_with("/dev/nvme") {
        if let Some(base_disk) = extract_base_disk(device_path) {
            return PathBuf::from(format!("/dev/{base_disk}"));
        }
    }

    PathBuf::from(device_path)
}

#[cfg(target_os = "windows")]
pub fn preferred_imaging_source_path(device_path: &str) -> PathBuf {
    // On Windows, physical drives are addressed as \\.\PhysicalDriveN. If
    // the caller already passed that form, we're done.
    if device_path.starts_with("\\\\.\\PhysicalDrive") {
        return PathBuf::from(device_path);
    }

    // Resolve "C:" / "C:\\" / "C:\\some\\path" to the underlying physical
    // drive via PowerShell. `Get-Partition -DriveLetter C | Get-Disk` is the
    // officially supported pipeline; we parse the `Number` field it returns.
    // PowerShell is part of every supported Windows build so we avoid pulling
    // in the WMI crate for something this narrow.
    if let Some(letter) = windows_drive_letter(device_path) {
        if let Some(physical) = physical_drive_for_letter(letter) {
            return PathBuf::from(physical);
        }
    }

    PathBuf::from(device_path)
}

#[cfg(target_os = "windows")]
fn windows_drive_letter(device_path: &str) -> Option<char> {
    // Match "C", "C:", "C:\\", or "C:\\Users\\foo" — the first character
    // must be ASCII alphabetic and the second (if present) must be ':'.
    let mut chars = device_path.chars();
    let first = chars.next()?.to_ascii_uppercase();
    if !first.is_ascii_alphabetic() {
        return None;
    }
    match chars.next() {
        None => Some(first),
        Some(':') => Some(first),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
fn physical_drive_for_letter(letter: char) -> Option<String> {
    let script = format!(
        "(Get-Partition -DriveLetter {} -ErrorAction Stop | Get-Disk -ErrorAction Stop).Number",
        letter
    );
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let number: u32 = stdout.trim().parse().ok()?;
    Some(format!("\\\\.\\PhysicalDrive{number}"))
}

// ============================================================================
// validate_imaging_source_readable — cross-platform
// ============================================================================

/// Check whether the current process can open the selected imaging source in read-only mode.
pub fn validate_imaging_source_readable(source_path: &Path) -> Result<(), String> {
    match File::open(source_path) {
        Ok(file) => {
            drop(file);
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {
            let source_display = source_path.to_string_lossy();

            #[cfg(target_os = "macos")]
            let hint = if source_display.starts_with("/dev/") {
                "On macOS this usually requires a privileged helper or a session with raw-device access. Mounted-volume scans remain available."
            } else {
                ""
            };

            #[cfg(target_os = "linux")]
            let hint = if source_display.starts_with("/dev/") {
                "On Linux this usually requires root privileges or membership in the 'disk' group."
            } else {
                ""
            };

            #[cfg(target_os = "windows")]
            let hint = if source_display.starts_with("\\\\.\\") {
                "On Windows this usually requires running the application as Administrator."
            } else {
                ""
            };

            let target_kind =
                if source_display.starts_with("/dev/") || source_display.starts_with("\\\\.\\") {
                    "raw device"
                } else {
                    "selected imaging source"
                };

            Err(format!(
                "Read-only imaging cannot start from {source_display} because the current app process does not have permission to read the {target_kind}. {hint}",
            ))
        }
        Err(error) => Err(format!(
            "Unable to open the imaging source {} for read-only acquisition: {}",
            source_path.to_string_lossy(),
            error
        )),
    }
}

// ============================================================================
// extract_base_disk — platform-specific
// ============================================================================

/// Extract the base disk identifier from a device path.
///
/// - macOS: "/dev/disk1s1" -> "disk1", "/dev/rdisk4s2" -> "disk4"
/// - Linux: "/dev/sda1" -> "sda", "/dev/nvme0n1p2" -> "nvme0n1"
/// - Windows: "\\.\PhysicalDrive0" -> "PhysicalDrive0"
pub fn extract_base_disk(device_path: &str) -> Option<String> {
    // macOS: /dev/disk* or /dev/rdisk*
    let path = device_path
        .trim_start_matches("/dev/")
        .trim_start_matches("\\\\.\\");

    // macOS disk paths: strip leading 'r' for raw device prefix
    let path = if path.starts_with("rdisk") {
        &path[1..]
    } else {
        path
    };

    // macOS: disk0, disk1s1, etc.
    if let Some(rest) = path.strip_prefix("disk") {
        let digits_len = rest
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .count();
        if digits_len > 0 {
            return Some(path[.."disk".len() + digits_len].to_string());
        }
    }

    // Linux: sda, sdb1, etc. -> strip trailing partition number
    if path.starts_with("sd") && path.len() >= 3 {
        let letter_part: String = path
            .chars()
            .skip(2)
            .take_while(|c| c.is_ascii_lowercase())
            .collect();
        if !letter_part.is_empty() {
            return Some(format!("sd{letter_part}"));
        }
    }

    // Linux: nvme0n1p2 -> nvme0n1
    if path.starts_with("nvme") {
        if let Some(p_pos) = path.find('p') {
            // Ensure it's a partition separator: character before 'p' should be a digit
            // and character after 'p' should be a digit
            let before = path.as_bytes().get(p_pos.wrapping_sub(1));
            let after = path.as_bytes().get(p_pos + 1);
            if before.is_some_and(|b| b.is_ascii_digit())
                && after.is_some_and(|b| b.is_ascii_digit())
            {
                return Some(path[..p_pos].to_string());
            }
        }
        return Some(path.to_string());
    }

    // Windows: PhysicalDrive0, PhysicalDrive1, etc.
    if path.starts_with("PhysicalDrive") {
        return Some(path.to_string());
    }

    Some(path.to_string())
}

// ============================================================================
// Cross-platform helpers
// ============================================================================

fn normalize_existing_path(path: &str) -> Option<PathBuf> {
    let direct = PathBuf::from(path);
    if direct.exists() {
        return direct.canonicalize().ok().or(Some(direct));
    }

    let parent = direct.parent()?;
    if parent.exists() {
        let parent_buf = Path::new(parent).to_path_buf();
        return parent_buf.canonicalize().ok().or(Some(parent_buf));
    }

    None
}

// ============================================================================
// SMART Report
// ============================================================================

#[cfg(target_os = "macos")]
pub fn get_smart_report(device_path: &str, device_id: &str) -> SmartReport {
    let default_report = SmartReport {
        device_id: device_id.to_string(),
        overall_health: "unavailable".to_string(),
        temperature_celsius: None,
        power_on_hours: None,
        reallocated_sectors: None,
        pending_sectors: None,
        attributes: vec![],
        error_log_count: None,
    };

    // Resolve to base disk for smartctl
    let base_disk = extract_base_disk(device_path);
    let disk_to_query = match &base_disk {
        Some(d) => format!("/dev/{d}"),
        None => device_path.to_string(),
    };

    let output = match Command::new("smartctl")
        .args(["-A", "-H", "-j", &disk_to_query])
        .output()
    {
        Ok(o) => o,
        Err(_) => return default_report,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => return default_report,
    };

    let overall_health = json
        .pointer("/smart_status/passed")
        .and_then(|v| v.as_bool())
        .map(|passed| if passed { "passed" } else { "failed" })
        .unwrap_or("unknown")
        .to_string();

    let mut attributes: Vec<SmartAttribute> = vec![];
    let mut temperature_celsius: Option<u8> = None;
    let mut power_on_hours: Option<u64> = None;
    let mut reallocated_sectors: Option<u64> = None;
    let mut pending_sectors: Option<u64> = None;

    if let Some(table) = json
        .pointer("/ata_smart_attributes/table")
        .and_then(|v| v.as_array())
    {
        for attr in table {
            let id = attr.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let name = attr
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let value = attr.get("value").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let worst = attr.get("worst").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let threshold = attr.get("thresh").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let raw_value = attr
                .pointer("/raw/string")
                .or_else(|| attr.pointer("/raw/value"))
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default();

            let status = if threshold > 0 && value <= threshold {
                "failed".to_string()
            } else if threshold > 0 && value <= threshold + 10 {
                "warning".to_string()
            } else {
                "ok".to_string()
            };

            // Extract well-known attributes
            match id {
                194 => {
                    temperature_celsius = attr
                        .pointer("/raw/value")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u8);
                }
                9 => {
                    power_on_hours = attr.pointer("/raw/value").and_then(|v| v.as_u64());
                }
                5 => {
                    reallocated_sectors = attr.pointer("/raw/value").and_then(|v| v.as_u64());
                }
                197 => {
                    pending_sectors = attr.pointer("/raw/value").and_then(|v| v.as_u64());
                }
                _ => {}
            }

            attributes.push(SmartAttribute {
                id,
                name,
                value,
                worst,
                threshold,
                raw_value,
                status,
            });
        }
    }

    let error_log_count = json
        .pointer("/ata_smart_error_log/summary/count")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    SmartReport {
        device_id: device_id.to_string(),
        overall_health,
        temperature_celsius,
        power_on_hours,
        reallocated_sectors,
        pending_sectors,
        attributes,
        error_log_count,
    }
}

#[cfg(target_os = "linux")]
pub fn get_smart_report(_device_path: &str, device_id: &str) -> SmartReport {
    SmartReport {
        device_id: device_id.to_string(),
        overall_health: "unavailable".to_string(),
        temperature_celsius: None,
        power_on_hours: None,
        reallocated_sectors: None,
        pending_sectors: None,
        attributes: vec![],
        error_log_count: None,
    }
}

#[cfg(target_os = "windows")]
pub fn get_smart_report(_device_path: &str, device_id: &str) -> SmartReport {
    SmartReport {
        device_id: device_id.to_string(),
        overall_health: "unavailable".to_string(),
        temperature_celsius: None,
        power_on_hours: None,
        reallocated_sectors: None,
        pending_sectors: None,
        attributes: vec![],
        error_log_count: None,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(target_os = "macos")]
    #[test]
    fn preferred_imaging_source_path_promotes_partition_to_raw_whole_disk() {
        let expected = if PathBuf::from("/dev/rdisk4").exists() {
            PathBuf::from("/dev/rdisk4")
        } else {
            PathBuf::from("/dev/disk4")
        };
        assert_eq!(preferred_imaging_source_path("/dev/disk4s2"), expected);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn preferred_imaging_source_path_leaves_non_device_paths_unchanged() {
        assert_eq!(
            preferred_imaging_source_path("/Volumes/Recovery"),
            PathBuf::from("/Volumes/Recovery")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn preferred_imaging_source_path_promotes_linux_partition() {
        assert_eq!(
            preferred_imaging_source_path("/dev/sda1"),
            PathBuf::from("/dev/sda")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn preferred_imaging_source_path_promotes_linux_nvme_partition() {
        assert_eq!(
            preferred_imaging_source_path("/dev/nvme0n1p2"),
            PathBuf::from("/dev/nvme0n1")
        );
    }

    #[test]
    fn extract_base_disk_normalizes_raw_device_prefix() {
        assert_eq!(extract_base_disk("/dev/rdisk4s2"), Some("disk4".into()));
        assert_eq!(extract_base_disk("/dev/rdisk4"), Some("disk4".into()));
    }

    #[test]
    fn extract_base_disk_linux_sd_devices() {
        assert_eq!(extract_base_disk("/dev/sda1"), Some("sda".into()));
        assert_eq!(extract_base_disk("/dev/sdb"), Some("sdb".into()));
    }

    #[test]
    fn extract_base_disk_linux_nvme_devices() {
        assert_eq!(extract_base_disk("/dev/nvme0n1p2"), Some("nvme0n1".into()));
        assert_eq!(extract_base_disk("/dev/nvme0n1"), Some("nvme0n1".into()));
    }

    #[test]
    fn extract_base_disk_windows_physical_drive() {
        assert_eq!(
            extract_base_disk("\\\\.\\PhysicalDrive0"),
            Some("PhysicalDrive0".into())
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_drive_letter_parses_supported_forms() {
        assert_eq!(super::windows_drive_letter("C"), Some('C'));
        assert_eq!(super::windows_drive_letter("c"), Some('C'));
        assert_eq!(super::windows_drive_letter("C:"), Some('C'));
        assert_eq!(super::windows_drive_letter("C:\\"), Some('C'));
        assert_eq!(super::windows_drive_letter("D:\\Users\\foo"), Some('D'));
        assert_eq!(super::windows_drive_letter("1:"), None);
        assert_eq!(super::windows_drive_letter(""), None);
        assert_eq!(super::windows_drive_letter("/dev/sda1"), None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn preferred_imaging_source_path_keeps_physical_drive_paths_as_is() {
        assert_eq!(
            preferred_imaging_source_path("\\\\.\\PhysicalDrive0"),
            PathBuf::from("\\\\.\\PhysicalDrive0")
        );
    }

    #[test]
    fn validate_imaging_source_readable_accepts_regular_files() {
        let root = env::temp_dir().join(format!("recupere-core-readable-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp root should exist");
        let source = root.join("readable.img");
        fs::write(&source, b"ok").expect("readable fixture should be written");

        validate_imaging_source_readable(&source).expect("regular file should be readable");

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn validate_imaging_source_readable_reports_permission_denied_clearly() {
        let root = env::temp_dir().join(format!("recupere-core-blocked-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp root should exist");
        let source = root.join("blocked.img");
        fs::write(&source, b"blocked").expect("blocked fixture should be written");

        let mut permissions = fs::metadata(&source)
            .expect("blocked fixture metadata should exist")
            .permissions();
        permissions.set_mode(0o000);
        fs::set_permissions(&source, permissions).expect("fixture permissions should be updated");

        let error = validate_imaging_source_readable(&source)
            .expect_err("permission denied fixture should be rejected");
        assert!(error.contains("does not have permission"));

        let mut cleanup_permissions = fs::metadata(&source)
            .expect("blocked fixture metadata should still exist")
            .permissions();
        cleanup_permissions.set_mode(0o644);
        let _ = fs::set_permissions(&source, cleanup_permissions);
        let _ = fs::remove_dir_all(root);
    }
}
