// ============================================================================
// Récupère — Raw block-device enumeration (unmounted disks)
// ============================================================================
//
// `sysinfo::Disks` only surfaces disks with a live mount; the data-recovery
// workflow specifically needs to find the ones without one — a drive whose
// partition table was wiped, an external disk that plugged in but failed to
// mount, a RAID member, a LUKS/FileVault volume pre-unlock, etc.
//
// This module enumerates *raw* block devices directly from the kernel:
//
// - **Linux**: `/proc/partitions` + `/proc/mounts` cross-reference. Parses
//   the kernel's own block-device table and flags entries whose device path
//   is absent from `/proc/mounts`. `/sys/block/<name>/removable` tells us
//   whether to label it as external.
// - **macOS**: `diskutil list -plist` surfaces whole-disk entries. Disks
//   without any child partition that carries a `MountPoint` key — and
//   without a `MountPoint` on the whole-disk entry itself — are reported
//   as unmounted. Parsing is delegated to `parse_diskutil_plist`, which
//   is compiled on every platform so the canned-fixture tests can run
//   cross-OS.
// - **Windows**: `Get-Disk | Where-Object { $_.OperationalStatus -eq
//   'Offline' -or $_.IsOffline -eq $true } | ConvertTo-Json` via
//   PowerShell. PowerShell returns a single object when one disk matches
//   and an array otherwise; `parse_get_disk_json` handles both shapes and
//   is likewise compiled on every platform.
//
// The callers never block on these shell-outs — the implementations fall
// back to an empty list if the underlying command / file is unavailable
// (sandboxed environments, missing `diskutil`, Powershell denied, etc.).
// This keeps `detect_devices()` resilient on minimal CI runners.
// ============================================================================

#![allow(dead_code)]

use std::path::PathBuf;

/// One raw, unmounted block-device entry. Mounted disks are reported by
/// `sysinfo`; this struct deliberately carries only the fields we can
/// determine without a mount (size via `BLKGETSIZE64` / plist / PowerShell).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDisk {
    /// Absolute device path (e.g. `/dev/sda`, `/dev/rdisk5`,
    /// `\\.\PhysicalDrive2`).
    pub device_path: PathBuf,
    /// Kernel-level name (e.g. `sda`, `disk5`, `PhysicalDrive2`). Used for
    /// `/sys/block` lookups on Linux and PowerShell `DiskNumber` on Windows.
    pub kernel_name: String,
    /// Size in bytes. `None` if the platform couldn't report it — callers
    /// that need a concrete size should fall back to `BLKGETSIZE64` /
    /// `DeviceIoControl(IOCTL_DISK_GET_LENGTH_INFO)` themselves.
    pub size_bytes: Option<u64>,
    /// `Some(true)` if we're confident the device was removable (USB, SD,
    /// Thunderbolt). `None` means the platform didn't expose the flag.
    pub removable: Option<bool>,
    /// Best-effort label returned by the platform ("WD My Passport", etc.).
    pub label: Option<String>,
    /// True when the device path is NOT present in `/proc/mounts` (Linux)
    /// or when the platform reports the disk as offline (Windows /
    /// macOS). Mounted disks are excluded from this list entirely.
    pub is_unmounted: bool,
}

pub fn enumerate_unmounted_disks() -> Vec<RawDisk> {
    #[cfg(target_os = "linux")]
    {
        return linux::enumerate();
    }
    #[cfg(target_os = "macos")]
    {
        macos::enumerate()
    }
    #[cfg(target_os = "windows")]
    {
        return windows::enumerate();
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        return Vec::new();
    }
}

// ---------------------------------------------------------------------------
// Linux
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::fs;
    use std::path::Path;

    pub fn enumerate() -> Vec<RawDisk> {
        let partitions_text = match fs::read_to_string("/proc/partitions") {
            Ok(content) => content,
            Err(_) => return Vec::new(),
        };
        let mounts_text = fs::read_to_string("/proc/mounts").unwrap_or_default();
        enumerate_from_strings(&partitions_text, &mounts_text)
    }

    pub(super) fn enumerate_from_strings(partitions_text: &str, mounts_text: &str) -> Vec<RawDisk> {
        let mounted_devices = parse_mounted_devices(mounts_text);
        parse_proc_partitions(partitions_text)
            .into_iter()
            .filter_map(|entry| {
                let (kernel_name, blocks_1k) = entry;
                // Filter out partitions; we only want whole disks. The
                // kernel names a partition as `<disk><N>` (sda1) or
                // `<disk>p<N>` (nvme0n1p2, mmcblk0p1). If the stripped name
                // still exists as another row, this is a partition and we
                // skip it.
                if looks_like_partition(&kernel_name) {
                    return None;
                }
                let device_path = format!("/dev/{kernel_name}");
                let is_mounted = mounted_devices.iter().any(|mount| {
                    mount == &device_path
                        || mount.starts_with(&format!("{device_path}p"))
                        || mount.starts_with(&format!("{device_path}"))
                });
                if is_mounted {
                    return None;
                }
                let removable = fs::read_to_string(format!("/sys/block/{kernel_name}/removable"))
                    .ok()
                    .map(|raw| raw.trim() == "1");
                let label = fs::read_to_string(format!("/sys/block/{kernel_name}/device/model"))
                    .ok()
                    .map(|raw| raw.trim().to_string())
                    .filter(|label| !label.is_empty());
                let size_bytes = blocks_1k.map(|blocks| blocks.saturating_mul(1024));
                Some(RawDisk {
                    device_path: PathBuf::from(&device_path),
                    kernel_name,
                    size_bytes,
                    removable,
                    label,
                    is_unmounted: true,
                })
            })
            .collect()
    }

    pub(super) fn parse_proc_partitions(text: &str) -> Vec<(String, Option<u64>)> {
        let mut entries = Vec::new();
        for (idx, line) in text.lines().enumerate() {
            // The first two lines are a header + blank.
            if idx < 2 {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 4 {
                continue;
            }
            let blocks: Option<u64> = fields[2].parse().ok();
            entries.push((fields[3].to_string(), blocks));
        }
        entries
    }

    pub(super) fn parse_mounted_devices(text: &str) -> Vec<String> {
        text.lines()
            .filter_map(|line| {
                let device = line.split_whitespace().next()?;
                if device.starts_with("/dev/") {
                    Some(device.to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    fn looks_like_partition(kernel_name: &str) -> bool {
        // Strip a trailing numeric partition suffix (`sda1`, `nvme0n1p2`,
        // `mmcblk0p1`). If doing so yields a different name that is itself a
        // plausible whole-disk, treat this as a partition.
        let bytes = kernel_name.as_bytes();
        if bytes.is_empty() {
            return false;
        }
        // nvme / mmc: <base>p<n>
        if let Some(p_idx) = kernel_name.rfind('p') {
            let tail = &kernel_name[p_idx + 1..];
            if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
                let base = &kernel_name[..p_idx];
                if base.starts_with("nvme")
                    || base.starts_with("mmcblk")
                    || base.starts_with("loop")
                {
                    return true;
                }
            }
        }
        // sdX<n> / hdX<n> / vdX<n> — first alpha prefix, then digits.
        let alpha_len = bytes.iter().take_while(|b| b.is_ascii_alphabetic()).count();
        let numeric_tail = &bytes[alpha_len..];
        if !numeric_tail.is_empty() && numeric_tail.iter().all(|b| b.is_ascii_digit()) {
            // But `sda` alone has no numeric tail — handled by the empty check above.
            // `sda1` has tail "1" → partition.
            return true;
        }
        false
    }

    // Exported for integration tests in the super module.
    #[cfg(test)]
    pub(super) fn test_parse_proc_partitions(text: &str) -> Vec<(String, Option<u64>)> {
        parse_proc_partitions(text)
    }

    #[allow(unused)]
    pub(super) fn _unused_touch(_: &Path) {}
}

// ---------------------------------------------------------------------------
// macOS — `diskutil list -plist`
// ---------------------------------------------------------------------------

/// Parse the `stdout` of `diskutil list -plist` and return the whole-disks
/// whose partitions are all unmounted. Deliberately lenient: malformed XML,
/// missing keys, unexpected types → empty list rather than panic. The
/// function is compiled on every platform so it can be unit-tested without
/// a real `diskutil` binary.
pub(super) fn parse_diskutil_plist(bytes: &[u8]) -> Vec<RawDisk> {
    use plist::Value;

    let root = match Value::from_reader_xml(std::io::Cursor::new(bytes)) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    let root_dict = match root.as_dictionary() {
        Some(dict) => dict,
        None => return Vec::new(),
    };
    let entries = match root_dict
        .get("AllDisksAndPartitions")
        .and_then(|v| v.as_array())
    {
        Some(arr) => arr,
        None => return Vec::new(),
    };

    let mut disks = Vec::new();
    for entry in entries {
        let dict = match entry.as_dictionary() {
            Some(d) => d,
            None => continue,
        };
        let kernel_name = match dict.get("DeviceIdentifier").and_then(|v| v.as_string()) {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => continue,
        };

        if whole_disk_or_any_child_is_mounted(dict) {
            continue;
        }

        let size_bytes = dict
            .get("Size")
            .and_then(|v| v.as_unsigned_integer())
            .or_else(|| {
                dict.get("Size")
                    .and_then(|v| v.as_signed_integer())
                    .and_then(|n| u64::try_from(n).ok())
            });
        let removable = dict.get("RemovableMedia").and_then(|v| v.as_boolean());
        let label = dict
            .get("MediaName")
            .and_then(|v| v.as_string())
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .or_else(|| first_partition_volume_name(dict));

        disks.push(RawDisk {
            device_path: PathBuf::from(format!("/dev/{kernel_name}")),
            kernel_name,
            size_bytes,
            removable,
            label,
            is_unmounted: true,
        });
    }
    disks
}

fn whole_disk_or_any_child_is_mounted(dict: &plist::Dictionary) -> bool {
    if non_empty_mount_point(dict) {
        return true;
    }
    for child_key in ["Partitions", "APFSVolumes"] {
        if let Some(children) = dict.get(child_key).and_then(|v| v.as_array()) {
            for child in children {
                if let Some(child_dict) = child.as_dictionary() {
                    if non_empty_mount_point(child_dict) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn non_empty_mount_point(dict: &plist::Dictionary) -> bool {
    dict.get("MountPoint")
        .and_then(|v| v.as_string())
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

fn first_partition_volume_name(dict: &plist::Dictionary) -> Option<String> {
    for child_key in ["Partitions", "APFSVolumes"] {
        if let Some(children) = dict.get(child_key).and_then(|v| v.as_array()) {
            for child in children {
                if let Some(child_dict) = child.as_dictionary() {
                    if let Some(name) = child_dict
                        .get("VolumeName")
                        .and_then(|v| v.as_string())
                        .filter(|s| !s.is_empty())
                    {
                        return Some(name.to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::process::Command;

    pub fn enumerate() -> Vec<RawDisk> {
        let output = match Command::new("diskutil").args(["list", "-plist"]).output() {
            Ok(out) => out,
            Err(_) => return Vec::new(),
        };
        if !output.status.success() {
            return Vec::new();
        }
        parse_diskutil_plist(&output.stdout)
    }
}

// ---------------------------------------------------------------------------
// Windows — `Get-Disk | ConvertTo-Json`
// ---------------------------------------------------------------------------

/// Parse the JSON emitted by `Get-Disk | ConvertTo-Json`. PowerShell
/// returns either a single object (when exactly one disk matches the
/// filter) or an array of objects. Unknown or missing fields are tolerated
/// — we only surface what the JSON actually contains. The function is
/// compiled on every platform so it can be unit-tested without
/// `powershell.exe`.
pub(super) fn parse_get_disk_json(bytes: &[u8]) -> Vec<RawDisk> {
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t.trim(),
        Err(_) => return Vec::new(),
    };
    if text.is_empty() {
        return Vec::new();
    }
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let entries: Vec<&serde_json::Value> = match &value {
        serde_json::Value::Array(items) => items.iter().collect(),
        serde_json::Value::Object(_) => vec![&value],
        _ => return Vec::new(),
    };

    let mut disks = Vec::new();
    for entry in entries {
        let number = match entry.get("Number").and_then(|v| v.as_u64()) {
            Some(n) => n,
            None => continue,
        };
        let size_bytes = entry.get("Size").and_then(|v| v.as_u64());
        let friendly = entry
            .get("FriendlyName")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .filter(|s| !s.is_empty());
        // `IsRemovable` is the authoritative signal. When it's missing, we
        // only infer removability from a USB bus — not from SATA/NVMe/etc.,
        // since those aren't necessarily non-removable (Thunderbolt disks,
        // hot-swappable bays, …). In other words: we stay `None` on
        // ambiguous bus types rather than asserting `Some(false)` and
        // misleading callers.
        let removable = entry
            .get("IsRemovable")
            .and_then(|v| v.as_bool())
            .or_else(|| {
                entry
                    .get("BusType")
                    .and_then(|v| v.as_str())
                    .and_then(|bt| {
                        if bt.eq_ignore_ascii_case("USB") {
                            Some(true)
                        } else {
                            None
                        }
                    })
            });
        let kernel_name = format!("PhysicalDrive{number}");
        disks.push(RawDisk {
            device_path: PathBuf::from(format!(r"\\.\{kernel_name}")),
            kernel_name,
            size_bytes,
            removable,
            label: friendly,
            is_unmounted: true,
        });
    }
    disks
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::process::Command;

    pub fn enumerate() -> Vec<RawDisk> {
        let output = match Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-Disk | Where-Object { $_.OperationalStatus -eq 'Offline' -or $_.IsOffline -eq $true } | ConvertTo-Json -Depth 3",
            ])
            .output()
        {
            Ok(out) => out,
            Err(_) => return Vec::new(),
        };
        if !output.status.success() {
            return Vec::new();
        }
        parse_get_disk_json(&output.stdout)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_enumerate_never_panics_on_empty_platform_stub() {
        // Whatever the host OS, enumerate should never panic. On Linux a
        // real /proc read happens; on macOS/Windows stubs the list is
        // empty. We just want no crash.
        let _ = enumerate_unmounted_disks();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn proc_partitions_parser_handles_kernel_layout() {
        let canned = "\
major minor  #blocks  name

   8        0  500107608 sda
   8        1     524288 sda1
   8        2  499582464 sda2
   7        0     987654 loop0
 259        0  976762584 nvme0n1
 259        1    1048576 nvme0n1p1
 259        2  975713280 nvme0n1p2
";
        let parsed = linux::test_parse_proc_partitions(canned);
        let names: Vec<&str> = parsed.iter().map(|(name, _)| name.as_str()).collect();
        assert!(names.contains(&"sda"));
        assert!(names.contains(&"sda1"));
        assert!(names.contains(&"nvme0n1"));
        assert!(names.contains(&"nvme0n1p2"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn enumerate_from_strings_filters_mounted_and_partitions() {
        let partitions = "\
major minor  #blocks  name

   8        0  500107608 sda
   8        1     524288 sda1
   8        2  499582464 sda2
   8       16  250000000 sdb
 259        0  976762584 nvme0n1
 259        2  975713280 nvme0n1p2
";
        // sdb is unmounted; sda & nvme0n1 are mounted through their partitions.
        let mounts = "\
/dev/sda1 /boot vfat defaults 0 0
/dev/sda2 / ext4 defaults 0 0
/dev/nvme0n1p2 /home ext4 defaults 0 0
proc /proc proc defaults 0 0
";
        let disks = linux::enumerate_from_strings(partitions, mounts);
        let names: Vec<&str> = disks.iter().map(|d| d.kernel_name.as_str()).collect();
        assert_eq!(names, vec!["sdb"]);
        assert_eq!(disks[0].device_path, PathBuf::from("/dev/sdb"));
        assert_eq!(disks[0].size_bytes, Some(250_000_000 * 1024));
        assert!(disks[0].is_unmounted);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn enumerate_from_strings_empty_when_everything_mounted() {
        let partitions = "\
major minor  #blocks  name

   8        0  500107608 sda
   8        1     524288 sda1
";
        let mounts = "/dev/sda1 / ext4 defaults 0 0\n";
        let disks = linux::enumerate_from_strings(partitions, mounts);
        assert!(disks.is_empty());
    }

    // -----------------------------------------------------------------------
    // macOS — parse_diskutil_plist (tests run on every platform)
    // -----------------------------------------------------------------------

    const MACOS_INTERNAL_MOUNTED_PLIST: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>AllDisksAndPartitions</key>
  <array>
    <dict>
      <key>DeviceIdentifier</key><string>disk0</string>
      <key>Size</key><integer>500277790720</integer>
      <key>Content</key><string>GUID_partition_scheme</string>
      <key>Partitions</key>
      <array>
        <dict>
          <key>DeviceIdentifier</key><string>disk0s1</string>
          <key>Content</key><string>EFI</string>
          <key>Size</key><integer>314572800</integer>
          <key>VolumeName</key><string>EFI</string>
          <key>MountPoint</key><string>/Volumes/EFI</string>
        </dict>
        <dict>
          <key>DeviceIdentifier</key><string>disk0s2</string>
          <key>Content</key><string>Apple_APFS</string>
          <key>Size</key><integer>499962830848</integer>
          <key>VolumeName</key><string>Macintosh HD</string>
          <key>MountPoint</key><string>/</string>
        </dict>
      </array>
    </dict>
  </array>
</dict>
</plist>
"#;

    const MACOS_EXTERNAL_UNMOUNTED_PLIST: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>AllDisksAndPartitions</key>
  <array>
    <dict>
      <key>DeviceIdentifier</key><string>disk5</string>
      <key>Size</key><integer>2000398934016</integer>
      <key>Content</key><string>GUID_partition_scheme</string>
      <key>RemovableMedia</key><true/>
      <key>MediaName</key><string>WD My Passport</string>
      <key>Partitions</key>
      <array>
        <dict>
          <key>DeviceIdentifier</key><string>disk5s1</string>
          <key>Content</key><string>Microsoft Basic Data</string>
          <key>Size</key><integer>2000398934016</integer>
          <key>VolumeName</key><string>Backup</string>
          <key>MountPoint</key><string></string>
        </dict>
      </array>
    </dict>
  </array>
</dict>
</plist>
"#;

    const MACOS_MIXED_MOUNTED_AND_UNMOUNTED_PLIST: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>AllDisksAndPartitions</key>
  <array>
    <dict>
      <key>DeviceIdentifier</key><string>disk0</string>
      <key>Size</key><integer>500107862016</integer>
      <key>Partitions</key>
      <array>
        <dict>
          <key>DeviceIdentifier</key><string>disk0s1</string>
          <key>VolumeName</key><string>Macintosh HD</string>
          <key>MountPoint</key><string>/</string>
        </dict>
      </array>
    </dict>
    <dict>
      <key>DeviceIdentifier</key><string>disk4</string>
      <key>Size</key><integer>1000204886016</integer>
      <key>RemovableMedia</key><true/>
      <key>Partitions</key>
      <array>
        <dict>
          <key>DeviceIdentifier</key><string>disk4s1</string>
          <key>VolumeName</key><string>CrashPlan Backup</string>
          <key>MountPoint</key><string></string>
        </dict>
      </array>
    </dict>
  </array>
</dict>
</plist>
"#;

    #[test]
    fn parse_diskutil_plist_filters_fully_mounted_internal_disk() {
        let disks = parse_diskutil_plist(MACOS_INTERNAL_MOUNTED_PLIST);
        assert!(disks.is_empty(), "expected empty list, got {disks:?}");
    }

    #[test]
    fn parse_diskutil_plist_returns_external_unmounted_disk() {
        let disks = parse_diskutil_plist(MACOS_EXTERNAL_UNMOUNTED_PLIST);
        assert_eq!(disks.len(), 1, "expected exactly one entry: {disks:?}");
        let disk = &disks[0];
        assert_eq!(disk.kernel_name, "disk5");
        assert_eq!(disk.device_path, PathBuf::from("/dev/disk5"));
        assert_eq!(disk.size_bytes, Some(2_000_398_934_016));
        assert_eq!(disk.removable, Some(true));
        assert_eq!(disk.label.as_deref(), Some("WD My Passport"));
        assert!(disk.is_unmounted);
    }

    #[test]
    fn parse_diskutil_plist_returns_only_unmounted_entry_in_mixed_output() {
        let disks = parse_diskutil_plist(MACOS_MIXED_MOUNTED_AND_UNMOUNTED_PLIST);
        let names: Vec<&str> = disks.iter().map(|d| d.kernel_name.as_str()).collect();
        assert_eq!(names, vec!["disk4"]);
        assert_eq!(disks[0].label.as_deref(), Some("CrashPlan Backup"));
        assert_eq!(disks[0].removable, Some(true));
        assert!(disks[0].is_unmounted);
    }

    #[test]
    fn parse_diskutil_plist_returns_empty_on_malformed_input() {
        assert!(parse_diskutil_plist(b"").is_empty());
        assert!(parse_diskutil_plist(b"not actually a plist").is_empty());
        assert!(
            parse_diskutil_plist(b"<?xml version=\"1.0\"?><plist><dict></dict></plist>").is_empty()
        );
    }

    // -----------------------------------------------------------------------
    // Windows — parse_get_disk_json (tests run on every platform)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_get_disk_json_handles_single_object_output() {
        let json = br#"{
  "Number": 2,
  "FriendlyName": "Seagate Expansion Desk",
  "Size": 4000787030016,
  "OperationalStatus": "Offline",
  "IsOffline": true,
  "BusType": "USB",
  "IsRemovable": true
}"#;
        let disks = parse_get_disk_json(json);
        assert_eq!(disks.len(), 1);
        let disk = &disks[0];
        assert_eq!(disk.kernel_name, "PhysicalDrive2");
        assert_eq!(disk.device_path, PathBuf::from(r"\\.\PhysicalDrive2"));
        assert_eq!(disk.size_bytes, Some(4_000_787_030_016));
        assert_eq!(disk.label.as_deref(), Some("Seagate Expansion Desk"));
        assert_eq!(disk.removable, Some(true));
        assert!(disk.is_unmounted);
    }

    #[test]
    fn parse_get_disk_json_handles_array_with_multiple_disks() {
        let json = br#"[
  {"Number":1,"FriendlyName":"Offline NVMe","Size":512110190592,"IsRemovable":false,"BusType":"NVMe"},
  {"Number":3,"FriendlyName":"External Samsung T7","Size":1000204886016,"IsRemovable":true,"BusType":"USB"},
  {"Number":7,"FriendlyName":"","Size":500107862016,"BusType":"SATA"}
]"#;
        let disks = parse_get_disk_json(json);
        assert_eq!(disks.len(), 3);
        let names: Vec<&str> = disks.iter().map(|d| d.kernel_name.as_str()).collect();
        assert_eq!(
            names,
            vec!["PhysicalDrive1", "PhysicalDrive3", "PhysicalDrive7"]
        );
        assert_eq!(disks[0].size_bytes, Some(512_110_190_592));
        assert_eq!(disks[1].removable, Some(true));
        // Entry 2: no IsRemovable and BusType=SATA → None (not inferred as USB).
        assert_eq!(disks[2].removable, None);
        // Entry 2: FriendlyName is empty → label stripped to None.
        assert_eq!(disks[2].label, None);
    }

    #[test]
    fn parse_get_disk_json_returns_empty_on_blank_input() {
        assert!(parse_get_disk_json(b"").is_empty());
        assert!(parse_get_disk_json(b"   \r\n  ").is_empty());
    }

    #[test]
    fn parse_get_disk_json_returns_empty_on_malformed_json() {
        assert!(parse_get_disk_json(b"{nope").is_empty());
        assert!(parse_get_disk_json(b"[1, 2, 3]").is_empty());
        // Array element without Number → skipped silently, not a panic.
        let json = br#"[{"FriendlyName":"no number"}]"#;
        assert!(parse_get_disk_json(json).is_empty());
    }
}
