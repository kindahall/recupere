use crate::types::*;
use serde::Serialize;
use std::io::{Cursor, Write};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use super::export::{get_export_history, get_export_logs};
use super::runtime::{app_build_info, runtime_capabilities};
use super::scan::{
    generate_imaging_rescue_map, generate_imaging_session_report, get_scan_history, get_scan_logs,
};
use super::state::{export_sessions, lock_or_recover, scan_sessions, unix_timestamp_ms};

#[derive(Debug, Serialize)]
struct SupportBundleManifest {
    bundle_format_version: u8,
    generated_at_ms: u64,
    build_info: AppBuildInfo,
    runtime_capabilities: RuntimeCapabilities,
    live_scan_sessions: usize,
    live_export_sessions: usize,
    scan_history_entries: usize,
    export_history_entries: usize,
}

#[derive(Debug, Serialize)]
struct ExportPostureSummaryEntry {
    export_id: String,
    scan_id: String,
    started_at_ms: u64,
    destination_path: String,
    selection_mode: &'static str,
    implicit_preview_first_excluded_count: u32,
    status: String,
}

#[derive(Debug, Serialize)]
struct ScanProvenanceSummaryEntry {
    scan_id: String,
    device_name: String,
    source_kind: Option<String>,
    source_format: Option<String>,
    source_display_name: Option<String>,
    source_analysis_path: Option<String>,
    reconstructed_raid_source: bool,
    status: String,
}

#[derive(Debug, Serialize)]
struct LiveScanAiBriefSummaryEntry {
    scan_id: String,
    strategy_title: String,
    safe_export_strategy: String,
    apfs_catalog_preview_first: u32,
    apfs_catalog_reassembled: u32,
    complex_recovery_review: u32,
    unstable: u32,
}

#[derive(Debug)]
struct ImagingHandoffBundleEntry {
    scan_id: String,
    operator_status: &'static str,
    rescue_map_included: bool,
    precise_unreadable_ranges: usize,
    largest_unreadable_range: Option<String>,
    report: String,
    rescue_map: Option<String>,
}

fn format_unreadable_range_summary(range: &ImagingMapRange) -> String {
    let end_offset = range
        .start_offset
        .saturating_add(range.length.saturating_sub(1));
    format!(
        "0x{:016X} - 0x{:016X} ({} bytes)",
        range.start_offset, end_offset, range.length
    )
}

pub(crate) fn build_support_bundle_archive_bytes() -> Result<Vec<u8>, String> {
    let scan_history = get_scan_history();
    let export_history = get_export_history();
    let live_scan_ai_briefs = build_live_scan_ai_brief_summaries();
    let imaging_handoffs = build_imaging_handoff_entries(&scan_history);
    let manifest = SupportBundleManifest {
        bundle_format_version: 5,
        generated_at_ms: unix_timestamp_ms(),
        build_info: app_build_info(),
        runtime_capabilities: runtime_capabilities(),
        live_scan_sessions: lock_or_recover(scan_sessions(), "scan session registry").len(),
        live_export_sessions: lock_or_recover(export_sessions(), "export session registry").len(),
        scan_history_entries: scan_history.len(),
        export_history_entries: export_history.len(),
    };

    let mut cursor = Cursor::new(Vec::new());
    let mut archive = ZipWriter::new(&mut cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    add_json_bundle_entry(&mut archive, "manifest.json", &manifest, options)?;
    add_text_bundle_entry(
        &mut archive,
        "README.txt",
        "This support bundle contains redacted runtime metadata, history summaries, and technical logs.\nIt never includes recovered file contents, source disk bytes, or raw local paths by default.\n",
        options,
    )?;
    add_redacted_json_bundle_entry(&mut archive, "scan-history.json", &scan_history, options)?;
    add_text_bundle_entry(
        &mut archive,
        "scan-provenance-summary.txt",
        &format_scan_provenance_summary(&scan_history),
        options,
    )?;
    add_redacted_json_bundle_entry(
        &mut archive,
        "export-history.json",
        &export_history,
        options,
    )?;
    add_json_bundle_entry(
        &mut archive,
        "live-scan-ai-briefs.json",
        &live_scan_ai_briefs,
        options,
    )?;
    add_text_bundle_entry(
        &mut archive,
        "export-posture-summary.txt",
        &format_export_posture_summary(&export_history),
        options,
    )?;
    add_text_bundle_entry(
        &mut archive,
        "live-scan-ai-summary.txt",
        &format_live_scan_ai_summary(&live_scan_ai_briefs),
        options,
    )?;
    add_text_bundle_entry(
        &mut archive,
        "imaging-handoff-summary.txt",
        &format_imaging_handoff_summary(&imaging_handoffs),
        options,
    )?;

    for summary in &scan_history {
        if let Ok(logs) = get_scan_logs(summary.id.clone()) {
            if !logs.is_empty() {
                add_text_bundle_entry(
                    &mut archive,
                    &format!("scan-logs/{}.log", sanitize_bundle_segment(&summary.id)),
                    &redact_sensitive_text(&format_technical_logs(&logs)),
                    options,
                )?;
            }
        }
    }

    for summary in &export_history {
        if let Ok(logs) = get_export_logs(summary.id.clone()) {
            if !logs.is_empty() {
                add_text_bundle_entry(
                    &mut archive,
                    &format!("export-logs/{}.log", sanitize_bundle_segment(&summary.id)),
                    &redact_sensitive_text(&format_technical_logs(&logs)),
                    options,
                )?;
            }
        }
    }

    for entry in &imaging_handoffs {
        add_text_bundle_entry(
            &mut archive,
            &format!(
                "imaging-reports/{}.txt",
                sanitize_bundle_segment(&entry.scan_id)
            ),
            &redact_sensitive_text(&entry.report),
            options,
        )?;
        if let Some(rescue_map) = entry.rescue_map.as_ref() {
            add_text_bundle_entry(
                &mut archive,
                &format!(
                    "imaging-rescue-maps/{}.map",
                    sanitize_bundle_segment(&entry.scan_id)
                ),
                &redact_sensitive_text(rescue_map),
                options,
            )?;
        }
    }

    archive
        .finish()
        .map_err(|error| format!("Unable to finalize support bundle archive: {error}"))?;
    Ok(cursor.into_inner())
}

fn add_json_bundle_entry<T: Serialize>(
    archive: &mut ZipWriter<&mut Cursor<Vec<u8>>>,
    path: &str,
    value: &T,
    options: SimpleFileOptions,
) -> Result<(), String> {
    let payload = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Unable to serialize support bundle entry {path}: {error}"))?;
    add_binary_bundle_entry(archive, path, &payload, options)
}

fn add_redacted_json_bundle_entry<T: Serialize>(
    archive: &mut ZipWriter<&mut Cursor<Vec<u8>>>,
    path: &str,
    value: &T,
    options: SimpleFileOptions,
) -> Result<(), String> {
    let mut value = serde_json::to_value(value)
        .map_err(|error| format!("Unable to serialize support bundle entry {path}: {error}"))?;
    redact_json_value(&mut value, None);
    let payload = serde_json::to_vec_pretty(&value)
        .map_err(|error| format!("Unable to serialize support bundle entry {path}: {error}"))?;
    add_binary_bundle_entry(archive, path, &payload, options)
}

fn add_text_bundle_entry(
    archive: &mut ZipWriter<&mut Cursor<Vec<u8>>>,
    path: &str,
    content: &str,
    options: SimpleFileOptions,
) -> Result<(), String> {
    add_binary_bundle_entry(archive, path, content.as_bytes(), options)
}

fn add_binary_bundle_entry(
    archive: &mut ZipWriter<&mut Cursor<Vec<u8>>>,
    path: &str,
    bytes: &[u8],
    options: SimpleFileOptions,
) -> Result<(), String> {
    archive
        .start_file(path, options)
        .map_err(|error| format!("Unable to open support bundle entry {path}: {error}"))?;
    archive
        .write_all(bytes)
        .map_err(|error| format!("Unable to write support bundle entry {path}: {error}"))
}

fn format_technical_logs(logs: &[TechnicalLogEntry]) -> String {
    logs.iter()
        .map(|entry| {
            format!(
                "[{}] [{}] {}",
                entry.timestamp_ms, entry.level, entry.message
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_json_value(value: &mut serde_json::Value, key: Option<&str>) {
    match value {
        serde_json::Value::String(content) => {
            if key.is_some_and(is_sensitive_json_key) {
                *content = redacted_label(key.unwrap_or("value")).to_string();
            } else {
                *content = redact_sensitive_text(content);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_json_value(item, key);
            }
        }
        serde_json::Value::Object(map) => {
            for (child_key, child_value) in map.iter_mut() {
                redact_json_value(child_value, Some(child_key));
            }
        }
        _ => {}
    }
}

fn is_sensitive_json_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("path")
        || key == "device_name"
        || key == "destination_path"
        || key == "source_display_name"
        || key == "source_name"
        || key == "display_name"
}

fn redacted_label(key: &str) -> &'static str {
    if key.to_ascii_lowercase().contains("path") {
        "<redacted-path>"
    } else {
        "<redacted>"
    }
}

fn redact_sensitive_text(content: &str) -> String {
    let mut redacted = String::with_capacity(content.len());
    let mut token = String::new();
    for character in content.chars() {
        if character.is_whitespace() {
            if !token.is_empty() {
                redacted.push_str(&redact_sensitive_token(&token));
                token.clear();
            }
            redacted.push(character);
        } else {
            token.push(character);
        }
    }
    if !token.is_empty() {
        redacted.push_str(&redact_sensitive_token(&token));
    }
    redacted
}

fn redact_sensitive_token(token: &str) -> String {
    let trimmed = token.trim_matches(|character: char| {
        matches!(character, ',' | ';' | ')' | '(' | '[' | ']' | '"' | '\'')
    });
    let looks_like_unix_path = trimmed.starts_with('/') && trimmed.len() > 1;
    let looks_like_windows_path = trimmed.len() > 3
        && trimmed.as_bytes().get(1) == Some(&b':')
        && matches!(trimmed.as_bytes().get(2), Some(b'\\' | b'/'));
    let looks_like_unc_path = trimmed.starts_with("\\\\");
    if looks_like_unix_path || looks_like_windows_path || looks_like_unc_path {
        token.replace(trimmed, "<redacted-path>")
    } else {
        token.to_string()
    }
}

fn sanitize_bundle_segment(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn format_export_posture_summary(exports: &[ExportSessionSummary]) -> String {
    if exports.is_empty() {
        return "No export history is available yet.\n".into();
    }

    let entries: Vec<ExportPostureSummaryEntry> = exports
        .iter()
        .map(|summary| ExportPostureSummaryEntry {
            export_id: summary.id.clone(),
            scan_id: summary.scan_id.clone(),
            started_at_ms: summary.started_at_ms,
            destination_path: redact_sensitive_text(&summary.destination_path),
            selection_mode: if summary.explicit_selection {
                "explicit-selection"
            } else {
                "implicit-prudent-batch"
            },
            implicit_preview_first_excluded_count: summary.implicit_preview_first_excluded_count,
            status: summary.status.clone(),
        })
        .collect();

    let mut lines = vec![
        "Export posture summary".to_string(),
        "Tracks whether each export used an explicit file selection or the safer implicit batch."
            .to_string(),
        String::new(),
    ];

    for entry in entries {
        let held_out_line = if entry.implicit_preview_first_excluded_count > 0 {
            format!(
                "  APFS preview-first candidates held out of implicit batch: {}",
                entry.implicit_preview_first_excluded_count
            )
        } else {
            "  No APFS preview-first holdout was recorded for this export.".to_string()
        };
        lines.push(format!(
            "- {} | scan {} | {} | {}",
            entry.export_id, entry.scan_id, entry.selection_mode, entry.destination_path
        ));
        lines.push(format!("  Started: {}", entry.started_at_ms));
        lines.push(format!("  Status: {}", entry.status));
        if let Some(source_kind) = entry_source_kind(exports, &entry.export_id) {
            lines.push(format!("  Source kind: {}", source_kind));
        }
        lines.push(held_out_line);
        lines.push(String::new());
    }

    lines.join("\n")
}

fn entry_source_kind(exports: &[ExportSessionSummary], export_id: &str) -> Option<String> {
    exports
        .iter()
        .find(|summary| summary.id == export_id)
        .and_then(|summary| summary.source_kind.clone())
}

fn format_scan_provenance_summary(scans: &[ScanSessionSummary]) -> String {
    if scans.is_empty() {
        return "No scan history is available yet.\n".into();
    }

    let entries: Vec<ScanProvenanceSummaryEntry> = scans
        .iter()
        .map(|summary| ScanProvenanceSummaryEntry {
            scan_id: summary.id.clone(),
            device_name: "<redacted>".to_string(),
            source_kind: summary.source_kind.clone(),
            source_format: summary.source_format.clone(),
            source_display_name: summary
                .source_display_name
                .as_ref()
                .map(|_| "<redacted>".to_string()),
            source_analysis_path: summary
                .source_analysis_path
                .as_ref()
                .map(|_| "<redacted-path>".to_string()),
            reconstructed_raid_source: summary.reconstructed_raid_source,
            status: summary.status.clone(),
        })
        .collect();

    let mut lines = vec![
        "Scan provenance summary".to_string(),
        "Summarizes which source context each scan history entry used.".to_string(),
        String::new(),
    ];

    for entry in entries {
        lines.push(format!(
            "- {} | {} | {}",
            entry.scan_id, entry.device_name, entry.status
        ));
        if let Some(display_name) = entry.source_display_name {
            lines.push(format!("  Registered source: {}", display_name));
        }
        if let Some(source_kind) = entry.source_kind {
            lines.push(format!("  Source kind: {}", source_kind));
        }
        if let Some(source_format) = entry.source_format {
            lines.push(format!("  Source format: {}", source_format));
        }
        if let Some(source_analysis_path) = entry.source_analysis_path {
            lines.push(format!("  Analysis path: {}", source_analysis_path));
        }
        if entry.reconstructed_raid_source {
            lines.push("  Uses reconstructed RAID analysis source".to_string());
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

fn build_live_scan_ai_brief_summaries() -> Vec<LiveScanAiBriefSummaryEntry> {
    let sessions = lock_or_recover(scan_sessions(), "scan session registry");
    let mut summaries = Vec::new();

    for (scan_id, session) in sessions.iter() {
        let state = lock_or_recover(session, "scan session");
        let brief = crate::ai::build_scan_recovery_brief(scan_id, &state.results, &state.logs);
        summaries.push(LiveScanAiBriefSummaryEntry {
            scan_id: scan_id.clone(),
            strategy_title: brief.strategy_title,
            safe_export_strategy: brief.safe_export_strategy,
            apfs_catalog_preview_first: brief.counts.apfs_catalog_preview_first,
            apfs_catalog_reassembled: brief.counts.apfs_catalog_reassembled,
            complex_recovery_review: brief.counts.complex_recovery_review,
            unstable: brief.counts.unstable,
        });
    }

    summaries.sort_by(|left, right| left.scan_id.cmp(&right.scan_id));
    summaries
}

fn format_live_scan_ai_summary(briefs: &[LiveScanAiBriefSummaryEntry]) -> String {
    if briefs.is_empty() {
        return "No live scan AI brief is available in this support bundle.\n".into();
    }

    let mut lines = vec![
        "Live scan AI summary".to_string(),
        "Summarizes the current local triage posture for live scan sessions only.".to_string(),
        String::new(),
    ];

    for brief in briefs {
        lines.push(format!("- {}", brief.scan_id));
        lines.push(format!("  Strategy title: {}", brief.strategy_title));
        lines.push(format!(
            "  Safe export strategy: {}",
            brief.safe_export_strategy
        ));
        lines.push(format!(
            "  APFS preview-first: {}",
            brief.apfs_catalog_preview_first
        ));
        lines.push(format!(
            "  APFS reassembled: {}",
            brief.apfs_catalog_reassembled
        ));
        lines.push(format!(
            "  Complex review: {} | Unstable: {}",
            brief.complex_recovery_review, brief.unstable
        ));
        lines.push(String::new());
    }

    lines.join("\n")
}

fn imaging_operator_status(summary: &ScanSessionSummary) -> &'static str {
    if summary.unreadable_bytes > 0 || summary.unreadable_ranges_count > 0 {
        return "degraded";
    }

    if summary.rescued_after_retry_bytes > 0 || summary.retry_passes_completed > 0 {
        return "rescued";
    }

    if summary.resume_from_bytes > 0 {
        return "resumed";
    }

    "stable"
}

fn summary_uses_imaging(summary: &ScanSessionSummary) -> bool {
    summary.scan_type == "image" || summary.resume_from_bytes > 0 || summary.unreadable_bytes > 0
}

fn build_imaging_handoff_entries(scans: &[ScanSessionSummary]) -> Vec<ImagingHandoffBundleEntry> {
    let mut entries = Vec::new();

    for summary in scans {
        if !summary_uses_imaging(summary) {
            continue;
        }

        let Ok(report) = generate_imaging_session_report(summary.id.clone()) else {
            continue;
        };
        let rescue_map = generate_imaging_rescue_map(summary.id.clone()).ok();
        let mut ranges = summary.unreadable_ranges.clone();
        ranges.sort_by(|left, right| {
            right
                .length
                .cmp(&left.length)
                .then_with(|| left.start_offset.cmp(&right.start_offset))
        });
        entries.push(ImagingHandoffBundleEntry {
            scan_id: summary.id.clone(),
            operator_status: imaging_operator_status(summary),
            rescue_map_included: rescue_map.is_some(),
            precise_unreadable_ranges: ranges.len(),
            largest_unreadable_range: ranges.first().map(format_unreadable_range_summary),
            report,
            rescue_map,
        });
    }

    entries.sort_by(|left, right| left.scan_id.cmp(&right.scan_id));
    entries
}

fn format_imaging_handoff_summary(entries: &[ImagingHandoffBundleEntry]) -> String {
    if entries.is_empty() {
        return "No imaging handoff artifact is available in this support bundle.\n".into();
    }

    let mut lines = vec![
        "Imaging handoff summary".to_string(),
        "Lists incident reports and rescue maps bundled for imaging-oriented sessions.".to_string(),
        String::new(),
    ];

    for entry in entries {
        lines.push(format!("- {}", entry.scan_id));
        lines.push(format!("  Operator status: {}", entry.operator_status));
        lines.push(format!(
            "  Rescue map included: {}",
            if entry.rescue_map_included {
                "yes"
            } else {
                "no"
            }
        ));
        if entry.precise_unreadable_ranges > 0 {
            lines.push(format!(
                "  Precise unreadable ranges persisted: {}",
                entry.precise_unreadable_ranges
            ));
        }
        if let Some(largest_unreadable_range) = entry.largest_unreadable_range.as_ref() {
            lines.push(format!(
                "  Largest unreadable range: {}",
                largest_unreadable_range
            ));
        }
        lines.push(format!(
            "  Incident report path: imaging-reports/{}.txt",
            sanitize_bundle_segment(&entry.scan_id)
        ));
        if entry.rescue_map_included {
            lines.push(format!(
                "  Rescue map path: imaging-rescue-maps/{}.map",
                sanitize_bundle_segment(&entry.scan_id)
            ));
        }
        lines.push(String::new());
    }

    lines.join("\n")
}
