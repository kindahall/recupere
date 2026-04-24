// ============================================================================
// Recupere — Multi-Source Correlation Engine
// ============================================================================
// Combines results from filesystem analyzers, journal replay, carving, and
// slack space scanning to maximize recovery. Deduplicates overlapping
// candidates and promotes the highest-confidence version of each file.
// ============================================================================

use crate::types::RecoveredFile;
use std::collections::HashMap;

/// Correlation result containing merged, deduplicated, and ranked files.
pub struct CorrelationResult {
    pub files: Vec<RecoveredFile>,
    pub filesystem_count: usize,
    pub journal_count: usize,
    pub carved_count: usize,
    pub promoted_count: usize,
    pub deduplicated_count: usize,
}

/// Merge results from multiple recovery sources into a single deduplicated set.
/// Files are correlated by:
/// 1. SHA-256 hash (exact content match)
/// 2. Byte offset overlap (same physical location)
/// 3. Name + size heuristic (same file from different sources)
///
/// When duplicates are found, the highest-scoring version is kept.
pub fn correlate_recovery_sources(
    filesystem_files: Vec<RecoveredFile>,
    journal_files: Vec<RecoveredFile>,
    carved_files: Vec<RecoveredFile>,
) -> CorrelationResult {
    let fs_count = filesystem_files.len();
    let journal_count = journal_files.len();
    let carved_count = carved_files.len();

    let mut all_files: Vec<RecoveredFile> =
        Vec::with_capacity(filesystem_files.len() + journal_files.len() + carved_files.len());
    all_files.extend(filesystem_files);
    all_files.extend(journal_files);
    all_files.extend(carved_files);

    let total_before = all_files.len();

    // Phase 1: Deduplicate by SHA-256 (exact content match)
    let mut sha_index: HashMap<String, usize> = HashMap::new();
    let mut keep = vec![true; all_files.len()];

    for (i, file) in all_files.iter().enumerate() {
        if let Some(hash) = &file.sha256 {
            if !hash.is_empty() {
                if let Some(&existing) = sha_index.get(hash) {
                    // Keep the one with higher score
                    if all_files[existing].recovery_score >= file.recovery_score {
                        keep[i] = false;
                    } else {
                        keep[existing] = false;
                        sha_index.insert(hash.clone(), i);
                    }
                } else {
                    sha_index.insert(hash.clone(), i);
                }
            }
        }
    }

    // Phase 2: Deduplicate by byte offset overlap
    // If two files share the same start_offset and similar size, they're likely the same
    let mut offset_index: HashMap<u64, usize> = HashMap::new();
    for (i, file) in all_files.iter().enumerate() {
        if !keep[i] {
            continue;
        }
        if let Some(offset) = file.start_offset {
            if let Some(&existing) = offset_index.get(&offset) {
                if !keep[existing] {
                    offset_index.insert(offset, i);
                    continue;
                }
                let size_ratio = if all_files[existing].size_bytes > 0 {
                    file.size_bytes as f64 / all_files[existing].size_bytes as f64
                } else {
                    0.0
                };
                // If sizes within 10%, consider same file
                if (0.9..=1.1).contains(&size_ratio) {
                    if all_files[existing].recovery_score >= file.recovery_score {
                        keep[i] = false;
                    } else {
                        keep[existing] = false;
                        offset_index.insert(offset, i);
                    }
                }
            } else {
                offset_index.insert(offset, i);
            }
        }
    }

    // Phase 3: Promote journal-derived files that confirm filesystem deletions
    let mut promoted = 0usize;
    let mut name_size_index: HashMap<(String, u64), usize> = HashMap::new();

    for (i, file) in all_files.iter().enumerate() {
        if !keep[i] || file.journal_derived {
            continue;
        }
        name_size_index.insert((file.name.clone(), file.size_bytes), i);
    }

    // Collect promotions to apply
    let mut promotions: Vec<(usize, usize)> = Vec::new(); // (journal_idx, fs_idx)
    for (i, file) in all_files.iter().enumerate() {
        if !keep[i] || !file.journal_derived {
            continue;
        }
        let key = (file.name.clone(), file.size_bytes);
        if let Some(&fs_idx) = name_size_index.get(&key) {
            if keep[fs_idx] {
                promotions.push((i, fs_idx));
            }
        }
    }

    for (journal_idx, fs_idx) in promotions {
        all_files[fs_idx].recovery_score =
            all_files[fs_idx].recovery_score.saturating_add(8).min(95);
        keep[journal_idx] = false;
        promoted += 1;
    }

    // Collect surviving files
    let mut result: Vec<RecoveredFile> = all_files
        .into_iter()
        .enumerate()
        .filter(|(i, _)| keep[*i])
        .map(|(_, f)| f)
        .collect();

    // Sort by recovery score descending, then by name
    result.sort_by(|a, b| {
        b.recovery_score
            .cmp(&a.recovery_score)
            .then_with(|| a.name.cmp(&b.name))
    });

    let deduplicated = total_before - result.len();

    CorrelationResult {
        files: result,
        filesystem_count: fs_count,
        journal_count,
        carved_count,
        promoted_count: promoted,
        deduplicated_count: deduplicated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RecoveredFile;

    fn make_file(
        name: &str,
        score: u8,
        method: &str,
        sha: Option<&str>,
        offset: Option<u64>,
    ) -> RecoveredFile {
        RecoveredFile {
            id: format!("test-{name}"),
            name: name.into(),
            path: "/test".into(),
            extension: "txt".into(),
            size_bytes: 1024,
            integrity: "intact".into(),
            recovery_score: score,
            recovery_method: method.into(),
            start_offset: offset,
            sha256: sha.map(|s| s.into()),
            journal_derived: method == "journal",
            ..Default::default()
        }
    }

    #[test]
    fn dedup_by_sha256_keeps_highest_score() {
        let fs = vec![make_file(
            "a.txt",
            80,
            "filesystem",
            Some("abc123"),
            Some(100),
        )];
        let carved = vec![make_file(
            "carved-0001.txt",
            60,
            "carving",
            Some("abc123"),
            Some(200),
        )];

        let result = correlate_recovery_sources(fs, vec![], carved);
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].recovery_score, 80);
        assert_eq!(result.deduplicated_count, 1);
    }

    #[test]
    fn dedup_by_offset_keeps_highest_score() {
        let fs = vec![make_file("a.txt", 72, "filesystem", None, Some(4096))];
        let carved = vec![make_file(
            "carved-0001.txt",
            76,
            "carving",
            None,
            Some(4096),
        )];

        let result = correlate_recovery_sources(fs, vec![], carved);
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].recovery_score, 76);
    }

    #[test]
    fn journal_confirms_filesystem_boosts_score() {
        let fs = vec![make_file("report.txt", 72, "filesystem", None, Some(8192))];
        let journal = vec![make_file("report.txt", 64, "journal", None, Some(8193))];

        let result = correlate_recovery_sources(fs, journal, vec![]);
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].recovery_score, 80); // 72 + 8 boost
        assert_eq!(result.promoted_count, 1);
    }

    #[test]
    fn different_files_from_different_sources_kept() {
        let fs = vec![make_file("a.txt", 80, "filesystem", Some("aaa"), Some(100))];
        let journal = vec![make_file("b.txt", 65, "journal", Some("bbb"), Some(5000))];
        let carved = vec![make_file("c.jpg", 70, "carving", Some("ccc"), Some(9000))];

        let result = correlate_recovery_sources(fs, journal, carved);
        assert_eq!(result.files.len(), 3);
    }
}
