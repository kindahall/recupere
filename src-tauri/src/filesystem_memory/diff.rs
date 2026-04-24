// ============================================================================
// Récupère — Filesystem memory layer: diff engine
// ============================================================================
// Compare two `FilesystemSnapshot`s of the same target and classify every
// change as new / missing / moved / renamed / modified / ambiguous. Every
// classification ships with a `Confidence` level so the UI can be honest about
// what we know vs what we estimate.
//
// Classification rules (deterministic):
//   - Identity key: `(hash_prefix, size_bytes)` when both snapshots have a
//     hash; otherwise fall back to `(relative_path, size_bytes)`.
//   - New: present in head, absent in baseline (no identity match in baseline).
//   - Missing: present in baseline, absent in head (no identity match in head).
//   - Moved: identity match but different directory in head.
//   - Renamed: identity match, same directory, different filename.
//   - Modified: same relative path but size_bytes or hash_prefix changed.
//   - Ambiguous: multiple baseline entries could explain a missing / moved
//     entry (e.g. two files with the same hash prefix).
// ============================================================================

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::types::{
    Confidence, DiffChange, DiffChangeKind, FilesystemSnapshot, IndexedFileRecord,
    MissingFileInsight, RecoveryContextHint, RecoveryContextLookupInput,
    RecoveryContextMatchStrategy, SnapshotDiff,
};

/// Compute a diff between `baseline` (older) and `head` (newer) snapshots. The
/// two snapshots must target the same `target_path`; otherwise the diff is
/// refused so the UI never conflates unrelated trees.
pub fn compute_diff(
    baseline: &FilesystemSnapshot,
    head: &FilesystemSnapshot,
) -> Result<SnapshotDiff, String> {
    if baseline.target_path != head.target_path {
        return Err(format!(
            "Filesystem memory diff: baseline target path `{}` does not match head target path `{}`.",
            baseline.target_path, head.target_path
        ));
    }
    ensure_volume_identity_matches(baseline, head)?;

    let baseline_by_path: HashMap<String, &IndexedFileRecord> = baseline
        .records
        .iter()
        .map(|r| (r.relative_path.clone(), r))
        .collect();
    let head_by_path: HashMap<String, &IndexedFileRecord> = head
        .records
        .iter()
        .map(|r| (r.relative_path.clone(), r))
        .collect();

    let mut baseline_by_hash: HashMap<String, Vec<&IndexedFileRecord>> = HashMap::new();
    for record in &baseline.records {
        if let Some(hash) = record.hash_prefix.as_ref() {
            baseline_by_hash
                .entry(identity_key(hash, record.size_bytes))
                .or_default()
                .push(record);
        }
    }
    let mut head_by_hash: HashMap<String, Vec<&IndexedFileRecord>> = HashMap::new();
    for record in &head.records {
        if let Some(hash) = record.hash_prefix.as_ref() {
            head_by_hash
                .entry(identity_key(hash, record.size_bytes))
                .or_default()
                .push(record);
        }
    }

    let mut changes: Vec<DiffChange> = Vec::new();

    for (path, head_record) in &head_by_path {
        if let Some(baseline_record) = baseline_by_path.get(path) {
            if records_unchanged(baseline_record, head_record) {
                continue;
            }
            changes.push(DiffChange {
                kind: DiffChangeKind::Modified,
                confidence: modified_confidence(baseline_record, head_record),
                reason: "Same path, different size or content fingerprint.".into(),
                from: Some((*baseline_record).clone()),
                to: Some((*head_record).clone()),
            });
            continue;
        }

        // Not at the same path. See if baseline had an identity match elsewhere.
        if let Some(hash) = head_record.hash_prefix.as_ref() {
            let key = identity_key(hash, head_record.size_bytes);
            if let Some(candidates) = baseline_by_hash.get(&key) {
                match candidates.len() {
                    1 => {
                        let candidate = candidates[0];
                        let kind = classify_move_or_rename(candidate, head_record);
                        let reason = match kind {
                            DiffChangeKind::Moved => {
                                "Identical fingerprint in a different directory."
                            }
                            DiffChangeKind::Renamed => {
                                "Identical fingerprint in the same directory with a different name."
                            }
                            _ => "Identical fingerprint elsewhere in the tree.",
                        };
                        changes.push(DiffChange {
                            kind,
                            confidence: Confidence::High,
                            reason: reason.into(),
                            from: Some(candidate.clone()),
                            to: Some((*head_record).clone()),
                        });
                    }
                    _ => {
                        changes.push(DiffChange {
                            kind: DiffChangeKind::Ambiguous,
                            confidence: Confidence::Low,
                            reason:
                                "Multiple baseline entries share the same fingerprint — cannot decide if this is a move or a rename."
                                    .into(),
                            from: None,
                            to: Some((*head_record).clone()),
                        });
                    }
                }
                continue;
            }
        }

        changes.push(DiffChange {
            kind: DiffChangeKind::New,
            confidence: if head_record.hash_prefix.is_some() {
                Confidence::High
            } else {
                Confidence::Medium
            },
            reason: "Present in the newer snapshot, absent in the baseline.".into(),
            from: None,
            to: Some((*head_record).clone()),
        });
    }

    for (path, baseline_record) in &baseline_by_path {
        if head_by_path.contains_key(path) {
            continue;
        }

        // Already accounted for as moved / renamed / ambiguous?
        let already_matched = changes.iter().any(|change| {
            matches!(change.kind, DiffChangeKind::Moved | DiffChangeKind::Renamed)
                && change
                    .from
                    .as_ref()
                    .map(|record| record.relative_path == *path)
                    .unwrap_or(false)
        });
        if already_matched {
            continue;
        }

        if let Some(hash) = baseline_record.hash_prefix.as_ref() {
            let key = identity_key(hash, baseline_record.size_bytes);
            if head_by_hash.contains_key(&key) {
                // The head has a match, but it was already classified as moved
                // / renamed above. Skip to avoid double-counting.
                continue;
            }
        }

        let confidence = if baseline_record.hash_prefix.is_some() {
            Confidence::High
        } else {
            Confidence::Medium
        };
        changes.push(DiffChange {
            kind: DiffChangeKind::Missing,
            confidence,
            reason: "Present in the baseline, absent in the newer snapshot.".into(),
            from: Some((*baseline_record).clone()),
            to: None,
        });
    }

    Ok(SnapshotDiff {
        baseline_id: baseline.id.clone(),
        head_id: head.id.clone(),
        target_path: head.target_path.clone(),
        computed_at_ms: unix_timestamp_ms(),
        changes,
    })
}

/// Extract the missing-file insights from a diff. Always returns a fresh
/// allocation (no shared refs) so the caller can ship the payload over IPC.
pub fn missing_file_insights(
    diff: &SnapshotDiff,
    baseline_captured_at_ms: u64,
    head_captured_at_ms: u64,
) -> Vec<MissingFileInsight> {
    diff.changes
        .iter()
        .filter(|change| matches!(change.kind, DiffChangeKind::Missing))
        .filter_map(|change| {
            let record = change.from.as_ref()?;
            Some(MissingFileInsight {
                name: record.name.clone(),
                last_known_path: record.absolute_path.clone(),
                last_observed_at_ms: baseline_captured_at_ms,
                first_missing_observed_at_ms: head_captured_at_ms,
                file_modified_at_ms: record.modified_at_ms,
                size_bytes: record.size_bytes,
                extension: record.extension.clone(),
                confidence: change.confidence,
                recovery_hint: recovery_hint_for(
                    record,
                    change.confidence,
                    baseline_captured_at_ms,
                    head_captured_at_ms,
                ),
            })
        })
        .collect()
}

/// Build `RecoveryContextHint`s for an iterator of recovered files. This
/// is how the `scoring` / results layer ties a current `RecoveredFile` back to
/// the last time the filesystem memory saw it on the source — used by B5 to
/// weight scoring, and by the results UI to explain "why" a file is flagged.
pub fn recovery_context_hints_for(
    diff: &SnapshotDiff,
    recovered_files: &[RecoveryContextLookupInput],
    baseline_captured_at_ms: u64,
    head_captured_at_ms: u64,
) -> Vec<RecoveryContextHint> {
    let missing_changes: Vec<&DiffChange> = diff
        .changes
        .iter()
        .filter(|change| matches!(change.kind, DiffChangeKind::Missing))
        .collect();

    let mut missing_by_path: HashMap<String, Vec<&DiffChange>> = HashMap::new();
    let mut missing_by_name_size: HashMap<String, Vec<&DiffChange>> = HashMap::new();

    for change in &missing_changes {
        if let Some(record) = change.from.as_ref() {
            missing_by_path
                .entry(normalize_comparable_path(&record.absolute_path))
                .or_default()
                .push(*change);
            missing_by_name_size
                .entry(name_size_key(&record.name, record.size_bytes))
                .or_default()
                .push(*change);
        }
    }

    recovered_files
        .iter()
        .filter_map(|file| {
            let path_key = normalize_comparable_path(&file.path);
            let path_match = missing_by_path
                .get(&path_key)
                .and_then(|matches| unique_change(matches))
                .map(|change| (change, RecoveryContextMatchStrategy::Path));
            let name_size_match = missing_by_name_size
                .get(&name_size_key(&file.name, file.size_bytes))
                .and_then(|matches| unique_change(matches))
                .map(|change| (change, RecoveryContextMatchStrategy::NameSize));
            let (change, matched_by) = path_match.or(name_size_match)?;
            let record = change.from.as_ref()?;
            Some(RecoveryContextHint {
                file_id: file.file_id.clone(),
                file_name: file.name.clone(),
                last_known_path: record.absolute_path.clone(),
                last_observed_at_ms: baseline_captured_at_ms,
                first_missing_observed_at_ms: Some(head_captured_at_ms),
                file_modified_at_ms: record.modified_at_ms,
                confidence: adjusted_recovery_context_confidence(change.confidence, matched_by),
                matched_by,
            })
        })
        .collect()
}

fn ensure_volume_identity_matches(
    baseline: &FilesystemSnapshot,
    head: &FilesystemSnapshot,
) -> Result<(), String> {
    match (
        baseline.volume_fingerprint.as_deref(),
        head.volume_fingerprint.as_deref(),
    ) {
        (Some(baseline_fp), Some(head_fp)) if baseline_fp == head_fp => Ok(()),
        (Some(baseline_fp), Some(head_fp)) => Err(format!(
            "Filesystem memory diff: snapshots `{}` and `{}` point to different volumes ({} vs {}). \
             Recapture them on the same source before comparing changes.",
            baseline.id, head.id, baseline_fp, head_fp
        )),
        _ => Err(format!(
            "Filesystem memory diff: snapshots `{}` and `{}` do not carry a stable volume identity. \
             Recapture them with the current build before comparing changes.",
            baseline.id, head.id
        )),
    }
}

fn identity_key(hash_prefix: &str, size_bytes: u64) -> String {
    format!("{hash_prefix}:{size_bytes}")
}

fn name_size_key(name: &str, size_bytes: u64) -> String {
    format!("{name}:{size_bytes}")
}

fn unique_change<'a>(changes: &'a [&'a DiffChange]) -> Option<&'a DiffChange> {
    if changes.len() == 1 {
        Some(changes[0])
    } else {
        None
    }
}

fn normalize_comparable_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if normalized.len() > 1 {
        normalized.trim_end_matches('/').to_string()
    } else {
        normalized
    }
}

fn records_unchanged(a: &IndexedFileRecord, b: &IndexedFileRecord) -> bool {
    a.size_bytes == b.size_bytes && a.hash_prefix == b.hash_prefix
}

fn modified_confidence(from: &IndexedFileRecord, to: &IndexedFileRecord) -> Confidence {
    match (from.hash_prefix.as_deref(), to.hash_prefix.as_deref()) {
        (Some(_), Some(_)) => Confidence::High,
        (Some(_), None) | (None, Some(_)) => Confidence::Medium,
        (None, None) => Confidence::Low,
    }
}

fn adjusted_recovery_context_confidence(
    confidence: Confidence,
    matched_by: RecoveryContextMatchStrategy,
) -> Confidence {
    match matched_by {
        RecoveryContextMatchStrategy::Path => confidence,
        RecoveryContextMatchStrategy::NameSize => match confidence {
            Confidence::High => Confidence::Medium,
            Confidence::Medium | Confidence::Low => Confidence::Low,
        },
    }
}

fn classify_move_or_rename(from: &IndexedFileRecord, to: &IndexedFileRecord) -> DiffChangeKind {
    let from_parent = parent_path(&from.relative_path);
    let to_parent = parent_path(&to.relative_path);

    if from_parent == to_parent {
        DiffChangeKind::Renamed
    } else {
        DiffChangeKind::Moved
    }
}

fn parent_path(relative: &str) -> &str {
    match relative.rfind(['/', '\\']) {
        Some(idx) => &relative[..idx],
        None => "",
    }
}

fn recovery_hint_for(
    record: &IndexedFileRecord,
    confidence: Confidence,
    baseline_captured_at_ms: u64,
    head_captured_at_ms: u64,
) -> String {
    match confidence {
        Confidence::High => format!(
            "Observed at {} in the snapshot taken at {} and missing in the snapshot taken at {}. Start recovery from the same volume; the identity match is stable.",
            record.absolute_path, baseline_captured_at_ms, head_captured_at_ms
        ),
        Confidence::Medium => format!(
            "Observed at {} in the older snapshot and absent in the newer one. Recovery should still target the same volume but confidence is moderate — consider a second snapshot.",
            record.absolute_path
        ),
        Confidence::Low => format!(
            "Last known at {} across the compared snapshots, but identity is ambiguous — treat any recovered candidate as an estimate until a second snapshot confirms it.",
            record.absolute_path
        ),
    }
}

fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::types::SnapshotStatus;
    use super::*;

    fn record(
        path: &str,
        name: &str,
        size: u64,
        hash: Option<&str>,
        modified: Option<i64>,
    ) -> IndexedFileRecord {
        IndexedFileRecord {
            absolute_path: format!("/src/{path}"),
            relative_path: path.into(),
            name: name.into(),
            extension: path
                .rsplit_once('.')
                .map(|(_, ext)| ext.to_ascii_lowercase())
                .unwrap_or_default(),
            size_bytes: size,
            modified_at_ms: modified,
            hash_prefix: hash.map(str::to_string),
            volume_fingerprint: Some("vol".into()),
        }
    }

    fn snapshot(
        id: &str,
        captured_at_ms: u64,
        records: Vec<IndexedFileRecord>,
    ) -> FilesystemSnapshot {
        FilesystemSnapshot {
            id: id.into(),
            target_path: "/src".into(),
            captured_at_ms,
            status: SnapshotStatus::Completed,
            files_indexed: records.len() as u64,
            total_size_bytes: records.iter().map(|r| r.size_bytes).sum(),
            errors: vec![],
            volume_fingerprint: Some("vol".into()),
            records,
        }
    }

    #[test]
    fn detects_new_and_missing_files() {
        let baseline = snapshot(
            "b",
            100,
            vec![
                record("a.txt", "a.txt", 10, Some("h1"), Some(50)),
                record("b.txt", "b.txt", 20, Some("h2"), Some(60)),
            ],
        );
        let head = snapshot(
            "h",
            200,
            vec![
                record("a.txt", "a.txt", 10, Some("h1"), Some(50)),
                record("c.txt", "c.txt", 30, Some("h3"), Some(150)),
            ],
        );

        let diff = compute_diff(&baseline, &head).expect("diff should succeed");
        let kinds: Vec<DiffChangeKind> = diff.changes.iter().map(|c| c.kind).collect();
        assert!(kinds.contains(&DiffChangeKind::New));
        assert!(kinds.contains(&DiffChangeKind::Missing));
        assert!(!kinds.contains(&DiffChangeKind::Modified));
    }

    #[test]
    fn detects_move_when_hash_matches_in_new_directory() {
        let baseline = snapshot(
            "b",
            100,
            vec![record("photos/a.jpg", "a.jpg", 100, Some("hX"), Some(50))],
        );
        let head = snapshot(
            "h",
            200,
            vec![record("backup/a.jpg", "a.jpg", 100, Some("hX"), Some(60))],
        );

        let diff = compute_diff(&baseline, &head).expect("diff should succeed");
        assert_eq!(diff.changes.len(), 1);
        assert_eq!(diff.changes[0].kind, DiffChangeKind::Moved);
        assert_eq!(diff.changes[0].confidence, Confidence::High);
    }

    #[test]
    fn detects_rename_when_hash_matches_same_directory() {
        let baseline = snapshot(
            "b",
            100,
            vec![record(
                "notes/draft.txt",
                "draft.txt",
                42,
                Some("hY"),
                Some(10),
            )],
        );
        let head = snapshot(
            "h",
            200,
            vec![record(
                "notes/final.txt",
                "final.txt",
                42,
                Some("hY"),
                Some(20),
            )],
        );

        let diff = compute_diff(&baseline, &head).expect("diff should succeed");
        assert_eq!(diff.changes.len(), 1);
        assert_eq!(diff.changes[0].kind, DiffChangeKind::Renamed);
    }

    #[test]
    fn flags_ambiguity_when_multiple_baseline_entries_share_hash() {
        let baseline = snapshot(
            "b",
            100,
            vec![
                record(
                    "a/duplicate.bin",
                    "duplicate.bin",
                    500,
                    Some("hZ"),
                    Some(10),
                ),
                record(
                    "b/duplicate.bin",
                    "duplicate.bin",
                    500,
                    Some("hZ"),
                    Some(15),
                ),
            ],
        );
        let head = snapshot(
            "h",
            200,
            vec![record(
                "archive/duplicate.bin",
                "duplicate.bin",
                500,
                Some("hZ"),
                Some(25),
            )],
        );

        let diff = compute_diff(&baseline, &head).expect("diff should succeed");
        let ambiguous_count = diff
            .changes
            .iter()
            .filter(|c| matches!(c.kind, DiffChangeKind::Ambiguous))
            .count();
        assert_eq!(ambiguous_count, 1);
    }

    #[test]
    fn detects_modified_when_same_path_changes_content_fingerprint() {
        let baseline = snapshot(
            "b",
            100,
            vec![record("doc.txt", "doc.txt", 200, Some("h1"), Some(10))],
        );
        let head = snapshot(
            "h",
            200,
            vec![record("doc.txt", "doc.txt", 250, Some("h2"), Some(20))],
        );

        let diff = compute_diff(&baseline, &head).expect("diff should succeed");
        assert_eq!(diff.changes.len(), 1);
        assert_eq!(diff.changes[0].kind, DiffChangeKind::Modified);
    }

    #[test]
    fn treats_identical_hash_and_size_as_unchanged() {
        let baseline = snapshot(
            "b",
            100,
            vec![record("keep.txt", "keep.txt", 10, Some("h1"), Some(10))],
        );
        let head = snapshot(
            "h",
            200,
            vec![record("keep.txt", "keep.txt", 10, Some("h1"), Some(20))],
        );

        let diff = compute_diff(&baseline, &head).expect("diff should succeed");
        assert_eq!(diff.changes.len(), 0);
    }

    #[test]
    fn missing_file_insights_project_the_diff_correctly() {
        let baseline = snapshot(
            "b",
            100,
            vec![record("gone.txt", "gone.txt", 10, Some("h1"), Some(50))],
        );
        let head = snapshot("h", 200, vec![]);

        let diff = compute_diff(&baseline, &head).expect("diff should succeed");
        let insights = missing_file_insights(&diff, baseline.captured_at_ms, 300);
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].name, "gone.txt");
        assert_eq!(insights[0].last_observed_at_ms, 100);
        assert_eq!(insights[0].first_missing_observed_at_ms, 300);
        assert_eq!(insights[0].file_modified_at_ms, Some(50));
        assert_eq!(insights[0].confidence, Confidence::High);
        assert!(insights[0].recovery_hint.contains("Observed at"));
    }

    #[test]
    fn recovery_context_hints_match_recovered_files_with_exact_paths() {
        let baseline = snapshot(
            "b",
            100,
            vec![
                record("a.txt", "a.txt", 10, Some("h1"), Some(50)),
                record("keep.txt", "keep.txt", 10, Some("h2"), Some(10)),
            ],
        );
        let head = snapshot(
            "h",
            200,
            vec![record("keep.txt", "keep.txt", 10, Some("h2"), Some(10))],
        );

        let diff = compute_diff(&baseline, &head).expect("diff should succeed");
        let hints = recovery_context_hints_for(
            &diff,
            &[
                RecoveryContextLookupInput {
                    file_id: "file-a".into(),
                    name: "a.txt".into(),
                    path: "/src/a.txt".into(),
                    size_bytes: 10,
                },
                RecoveryContextLookupInput {
                    file_id: "file-b".into(),
                    name: "unknown.bin".into(),
                    path: "/src/unknown.bin".into(),
                    size_bytes: 20,
                },
            ],
            baseline.captured_at_ms,
            300,
        );
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].file_id, "file-a");
        assert_eq!(hints[0].file_name, "a.txt");
        assert_eq!(hints[0].last_observed_at_ms, 100);
        assert_eq!(hints[0].first_missing_observed_at_ms, Some(300));
        assert_eq!(hints[0].file_modified_at_ms, Some(50));
        assert_eq!(hints[0].matched_by, RecoveryContextMatchStrategy::Path);
        assert_eq!(hints[0].confidence, Confidence::High);
    }

    #[test]
    fn recovery_context_hints_fall_back_to_unique_name_and_size_matching() {
        let baseline = snapshot(
            "b",
            100,
            vec![record(
                "folder/report.txt",
                "report.txt",
                128,
                Some("h1"),
                Some(50),
            )],
        );
        let head = snapshot("h", 200, vec![]);

        let diff = compute_diff(&baseline, &head).expect("diff should succeed");
        let hints = recovery_context_hints_for(
            &diff,
            &[RecoveryContextLookupInput {
                file_id: "file-report".into(),
                name: "report.txt".into(),
                path: "".into(),
                size_bytes: 128,
            }],
            baseline.captured_at_ms,
            300,
        );

        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].file_id, "file-report");
        assert_eq!(hints[0].matched_by, RecoveryContextMatchStrategy::NameSize);
        assert_eq!(hints[0].confidence, Confidence::Medium);
    }

    #[test]
    fn recovery_context_hints_skip_ambiguous_name_size_matches() {
        let baseline = snapshot(
            "b",
            100,
            vec![
                record("a/report.txt", "report.txt", 128, Some("h1"), Some(50)),
                record("b/report.txt", "report.txt", 128, Some("h2"), Some(60)),
            ],
        );
        let head = snapshot("h", 200, vec![]);

        let diff = compute_diff(&baseline, &head).expect("diff should succeed");
        let hints = recovery_context_hints_for(
            &diff,
            &[RecoveryContextLookupInput {
                file_id: "file-report".into(),
                name: "report.txt".into(),
                path: "".into(),
                size_bytes: 128,
            }],
            baseline.captured_at_ms,
            300,
        );

        assert!(hints.is_empty());
    }

    #[test]
    fn refuses_to_diff_snapshots_with_different_target_paths() {
        let mut baseline = snapshot("b", 100, vec![]);
        baseline.target_path = "/srcA".into();
        let head = snapshot("h", 200, vec![]);

        let diff = compute_diff(&baseline, &head);
        assert!(diff.is_err());
    }

    #[test]
    fn refuses_to_diff_snapshots_with_different_volume_fingerprints() {
        let baseline = snapshot("b", 100, vec![]);
        let mut head = snapshot("h", 200, vec![]);
        head.volume_fingerprint = Some("other-vol".into());

        let diff = compute_diff(&baseline, &head);
        assert!(diff.is_err());
    }

    #[test]
    fn refuses_to_diff_snapshots_without_stable_volume_identity() {
        let mut baseline = snapshot("b", 100, vec![]);
        baseline.volume_fingerprint = None;
        let head = snapshot("h", 200, vec![]);

        let diff = compute_diff(&baseline, &head);
        assert!(diff.is_err());
    }
}
