#![allow(dead_code)]
// ============================================================================
// Récupère — Structured Audit Trail (hash-chained)
// ============================================================================
// Immutable, append-only audit log for all critical user and system actions.
// Persisted to ~/.recupere/storage/audit_trail.json.
//
// Each record carries the SHA-256 hash of the canonical JSON of its
// predecessor. That turns the file into a tamper-evident chain: rewriting
// any past record invalidates every hash downstream, which `verify_trail()`
// detects in O(n). The genesis record commits to a fixed zero hash so an
// empty trail is distinguishable from a truncated one.
//
// Legacy records written before the hash chain existed have an empty
// `prev_hash` on deserialization (via `#[serde(default)]`). The verifier
// reports them as `Unverified` rather than as broken, so existing users
// don't see a false alarm after upgrade.
// ============================================================================

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Zero-hash used as the predecessor of the very first record in any chain.
/// Stored as lowercase hex so the full audit JSON stays human-readable.
const GENESIS_PREV_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventKind {
    DeviceSelected,
    ScanStarted,
    ScanCompleted,
    ScanCanceled,
    ScanFailed,
    ExportStarted,
    ExportCompleted,
    ExportFailed,
    SettingsChanged,
    HistoryPurged,
    ImagingStarted,
    ImagingCompleted,
    ImagingFailed,
    AuditExported,
    FilesystemSnapshotCaptured,
    FilesystemSnapshotFailed,
    FilesystemDiffComputed,
    FilesystemDiffFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub id: u64,
    pub timestamp_ms: u64,
    pub event: AuditEventKind,
    pub details: Value,
    /// SHA-256 hex of the canonical JSON of the previous record. Empty on
    /// records written before the chain was introduced.
    #[serde(default)]
    pub prev_hash: String,
}

/// Per-record status returned by [`verify_trail`]. `Ok` means the record's
/// `prev_hash` matched the hash computed from the previous record,
/// `Unverified` means the record was written before hash chaining existed
/// (legacy), and `Broken` means the stored hash doesn't match reality — a
/// sign that the file has been edited or truncated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditChainStatus {
    Ok,
    Unverified,
    Broken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditChainReport {
    pub total: usize,
    pub ok: usize,
    pub unverified: usize,
    pub broken: usize,
    pub first_broken_id: Option<u64>,
    pub chain_tip_hash: String,
    pub genesis_prev_hash: String,
    pub statuses: Vec<(u64, AuditChainStatus)>,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct AuditTrail {
    records: Vec<AuditRecord>,
    next_id: u64,
}

static TRAIL: OnceLock<Mutex<AuditTrail>> = OnceLock::new();

fn trail() -> &'static Mutex<AuditTrail> {
    TRAIL.get_or_init(|| {
        let (records, next_id) = load_persisted_trail();
        Mutex::new(AuditTrail { records, next_id })
    })
}

// ---------------------------------------------------------------------------
// Hash helpers
// ---------------------------------------------------------------------------

/// Canonical bytes that get hashed: serde_json::to_vec over the full record
/// INCLUDING its `prev_hash`. We deliberately use the default (field-order
/// preserving) serializer because every record is written and re-read with
/// the same code; no need for a sorted-key canonicalization that would break
/// roundtrips across Rust minor versions.
fn record_hash(record: &AuditRecord) -> String {
    let bytes = serde_json::to_vec(record).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    format!("{:x}", digest)
}

fn expected_prev_hash(records: &[AuditRecord]) -> String {
    match records.last() {
        None => GENESIS_PREV_HASH.to_string(),
        Some(previous) => record_hash(previous),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn record(event: AuditEventKind, details: Value) {
    let mut guard = match trail().lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let prev_hash = expected_prev_hash(&guard.records);

    let record = AuditRecord {
        id: guard.next_id,
        timestamp_ms: now,
        event,
        details,
        prev_hash,
    };

    guard.next_id += 1;
    guard.records.push(record);

    persist_trail(&guard.records);
}

pub fn get_trail() -> Vec<AuditRecord> {
    match trail().lock() {
        Ok(guard) => guard.records.clone(),
        Err(_) => Vec::new(),
    }
}

pub fn clear_trail() {
    if let Ok(mut guard) = trail().lock() {
        guard.records.clear();
        persist_trail(&guard.records);
    }
}

/// Walk the persisted trail and return one status per record. Used by the
/// `verify_audit_trail` Tauri command and by the support bundle so operators
/// can detect tampering after the fact.
pub fn verify_trail() -> AuditChainReport {
    let records = get_trail();
    verify_records(&records)
}

/// Public for tests so we can pass synthetic chains without touching disk.
pub(crate) fn verify_records(records: &[AuditRecord]) -> AuditChainReport {
    let mut statuses = Vec::with_capacity(records.len());
    let mut ok = 0usize;
    let mut unverified = 0usize;
    let mut broken = 0usize;
    let mut first_broken_id: Option<u64> = None;
    let mut expected = GENESIS_PREV_HASH.to_string();

    for record in records {
        let status = if record.prev_hash.is_empty() {
            unverified += 1;
            AuditChainStatus::Unverified
        } else if record.prev_hash == expected {
            ok += 1;
            AuditChainStatus::Ok
        } else {
            broken += 1;
            if first_broken_id.is_none() {
                first_broken_id = Some(record.id);
            }
            AuditChainStatus::Broken
        };
        statuses.push((record.id, status));
        expected = record_hash(record);
    }

    AuditChainReport {
        total: records.len(),
        ok,
        unverified,
        broken,
        first_broken_id,
        chain_tip_hash: records
            .last()
            .map(record_hash)
            .unwrap_or_else(|| GENESIS_PREV_HASH.to_string()),
        genesis_prev_hash: GENESIS_PREV_HASH.to_string(),
        statuses,
    }
}

// ---------------------------------------------------------------------------
// Persistence helpers
// ---------------------------------------------------------------------------

fn storage_dir() -> PathBuf {
    let home = dirs_next().unwrap_or_else(|| PathBuf::from("."));
    home.join(".recupere").join("storage")
}

fn trail_path() -> PathBuf {
    storage_dir().join("audit_trail.json")
}

fn dirs_next() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE").ok().map(PathBuf::from)
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}

fn load_persisted_trail() -> (Vec<AuditRecord>, u64) {
    let path = trail_path();
    if !path.exists() {
        return (Vec::new(), 1);
    }
    match fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<Vec<AuditRecord>>(&content) {
            Ok(records) => {
                let next_id = records.iter().map(|r| r.id).max().unwrap_or(0) + 1;
                (records, next_id)
            }
            Err(_) => (Vec::new(), 1),
        },
        Err(_) => (Vec::new(), 1),
    }
}

fn persist_trail(records: &[AuditRecord]) {
    let path = trail_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(records) {
        let tmp = path.with_extension("json.tmp");
        if let Ok(mut file) = fs::File::create(&tmp) {
            if file.write_all(json.as_bytes()).is_ok() {
                let _ = fs::rename(&tmp, &path);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record(id: u64, prev_hash: &str) -> AuditRecord {
        AuditRecord {
            id,
            timestamp_ms: id * 1000,
            event: AuditEventKind::ScanStarted,
            details: serde_json::json!({"seq": id}),
            prev_hash: prev_hash.to_string(),
        }
    }

    fn chain(events: usize) -> Vec<AuditRecord> {
        let mut records = Vec::with_capacity(events);
        for id in 1..=events as u64 {
            let prev = expected_prev_hash(&records);
            records.push(sample_record(id, &prev));
        }
        records
    }

    #[test]
    fn audit_record_serializes() {
        let record = AuditRecord {
            id: 1,
            timestamp_ms: 1700000000000,
            event: AuditEventKind::ScanStarted,
            details: serde_json::json!({"device_id": "disk1", "scan_type": "quick"}),
            prev_hash: GENESIS_PREV_HASH.to_string(),
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("scan_started"));
        assert!(json.contains("disk1"));
        assert!(json.contains(GENESIS_PREV_HASH));
    }

    #[test]
    fn event_kind_variants_serialize() {
        let kinds = vec![
            AuditEventKind::DeviceSelected,
            AuditEventKind::ScanStarted,
            AuditEventKind::ScanCompleted,
            AuditEventKind::ScanCanceled,
            AuditEventKind::ScanFailed,
            AuditEventKind::ExportStarted,
            AuditEventKind::ExportCompleted,
            AuditEventKind::ExportFailed,
            AuditEventKind::SettingsChanged,
            AuditEventKind::HistoryPurged,
            AuditEventKind::ImagingStarted,
            AuditEventKind::ImagingCompleted,
            AuditEventKind::ImagingFailed,
            AuditEventKind::AuditExported,
            AuditEventKind::FilesystemSnapshotCaptured,
            AuditEventKind::FilesystemSnapshotFailed,
            AuditEventKind::FilesystemDiffComputed,
            AuditEventKind::FilesystemDiffFailed,
        ];
        for kind in kinds {
            let record = AuditRecord {
                id: 0,
                timestamp_ms: 0,
                event: kind,
                details: Value::Null,
                prev_hash: GENESIS_PREV_HASH.to_string(),
            };
            assert!(serde_json::to_string(&record).is_ok());
        }
    }

    #[test]
    fn legacy_record_without_prev_hash_deserializes() {
        // Records written before the chain existed have no prev_hash; serde
        // must default it to an empty string so we can upgrade the file in
        // place and still surface the existing events.
        let legacy = r#"{
            "id": 1,
            "timestamp_ms": 1700000000000,
            "event": "scan_started",
            "details": {"device_id": "disk1"}
        }"#;
        let record: AuditRecord = serde_json::from_str(legacy).expect("legacy JSON accepted");
        assert_eq!(record.prev_hash, "");
    }

    #[test]
    fn chain_of_three_records_verifies_cleanly() {
        let records = chain(3);
        let report = verify_records(&records);
        assert_eq!(report.total, 3);
        assert_eq!(report.ok, 3);
        assert_eq!(report.unverified, 0);
        assert_eq!(report.broken, 0);
        assert_eq!(report.first_broken_id, None);
        assert_eq!(report.genesis_prev_hash, GENESIS_PREV_HASH);
        assert_eq!(report.chain_tip_hash, record_hash(records.last().unwrap()));
        assert!(report
            .statuses
            .iter()
            .all(|(_, status)| *status == AuditChainStatus::Ok));
    }

    #[test]
    fn tampered_record_breaks_chain_at_mutation_point() {
        let mut records = chain(5);
        // Mutate the details of record #3. Its own `prev_hash` is still
        // correct (because it was computed before the mutation), but record
        // #4's prev_hash no longer matches the new hash of #3, so the
        // verifier must flag #4 as the first broken record.
        records[2].details = serde_json::json!({"tampered": true});
        let report = verify_records(&records);
        assert_eq!(report.broken, 1);
        assert_eq!(report.first_broken_id, Some(4));
    }

    #[test]
    fn inserting_a_forged_record_breaks_successor() {
        let mut records = chain(4);
        // Insert a record between #2 and #3 with a plausible-looking
        // prev_hash. The successor's hash chain will reject it.
        let forged = sample_record(99, &records[1].prev_hash);
        records.insert(2, forged);
        let report = verify_records(&records);
        // Something after the insertion must break; we don't pin the exact
        // id because the successor retains its old prev_hash which no
        // longer matches the forged record.
        assert!(report.broken > 0);
        assert!(report.first_broken_id.is_some());
    }

    #[test]
    fn legacy_mixed_with_chained_records_reports_unverified_only_for_legacy() {
        // Simulate an upgrade: two legacy records with empty prev_hash,
        // then three freshly-written hashed records. The verifier must
        // report 2 Unverified + 3 Ok, no Broken.
        let mut records = vec![sample_record(1, ""), sample_record(2, "")];
        for id in 3..=5 {
            let prev = expected_prev_hash(&records);
            records.push(sample_record(id, &prev));
        }
        let report = verify_records(&records);
        assert_eq!(report.unverified, 2);
        assert_eq!(report.ok, 3);
        assert_eq!(report.broken, 0);
        assert_eq!(report.first_broken_id, None);
    }
}
