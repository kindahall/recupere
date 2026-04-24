// ============================================================================
// Récupère — macOS privileged imaging helpers
// ============================================================================
// Moved out of `commands/mod.rs` in Sprint 2.1 Pass F2. Only `imaging_cmd/mod.rs`
// and the Tauri command layer reach these helpers, so they are `pub(crate)`
// and the private utilities (shell_escape, escape_applescript_literal, the two
// cfg-gated privileged_imager_executable_path variants) stay private to this
// file. Everything below is macOS-specific; nothing here compiles on Linux /
// Windows beyond the empty fallbacks.
// ============================================================================

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::imaging;
use crate::privileged_imager;

use super::super::unix_timestamp_ms;

#[cfg(target_os = "macos")]
pub(crate) fn privileged_imager_executable_path() -> Option<PathBuf> {
    None
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn privileged_imager_executable_path() -> Option<PathBuf> {
    None
}
pub(crate) fn create_privileged_helper_temp_dir(session_id: &str) -> Result<PathBuf, String> {
    let temp_dir = env::temp_dir().join(format!(
        "recupere-privileged-imager-{}-{}",
        session_id,
        unix_timestamp_ms()
    ));
    fs::create_dir_all(&temp_dir).map_err(|error| {
        format!(
            "Unable to prepare the privileged imaging helper workspace {}: {}",
            temp_dir.to_string_lossy(),
            error
        )
    })?;
    Ok(temp_dir)
}
#[cfg(target_os = "macos")]
fn shell_escape(argument: &str) -> String {
    format!("'{}'", argument.replace('\'', "'\"'\"'"))
}
#[cfg(target_os = "macos")]
fn escape_applescript_literal(content: &str) -> String {
    content.replace('\\', "\\\\").replace('"', "\\\"")
}
#[cfg(target_os = "macos")]
pub(crate) fn build_macos_privileged_imager_script(
    executable_path: &Path,
    source_path: &Path,
    destination_path: &Path,
    progress_file: &Path,
    error_file: &Path,
    summary_file: &Path,
    profile: imaging::ImagingProfile,
) -> String {
    let shell_command = [
        shell_escape(executable_path.to_string_lossy().as_ref()),
        shell_escape(privileged_imager::PRIVILEGED_IMAGER_FLAG),
        shell_escape("--source"),
        shell_escape(source_path.to_string_lossy().as_ref()),
        shell_escape("--destination"),
        shell_escape(destination_path.to_string_lossy().as_ref()),
        shell_escape("--progress-file"),
        shell_escape(progress_file.to_string_lossy().as_ref()),
        shell_escape("--error-file"),
        shell_escape(error_file.to_string_lossy().as_ref()),
        shell_escape("--summary-file"),
        shell_escape(summary_file.to_string_lossy().as_ref()),
        shell_escape("--profile"),
        shell_escape(profile.as_str()),
    ]
    .join(" ");

    format!(
        "do shell script \"{}\" with administrator privileges",
        escape_applescript_literal(&shell_command)
    )
}
#[cfg(target_os = "macos")]
pub(crate) fn build_privileged_imaging_failure(
    output: &std::process::Output,
    error_file: &Path,
) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let helper_error = fs::read_to_string(error_file)
        .ok()
        .map(|content| content.trim().to_string())
        .filter(|content| !content.is_empty());
    let combined_output = if !stderr.is_empty() { stderr } else { stdout };

    if combined_output.contains("User canceled")
        || combined_output.contains("User cancelled")
        || combined_output.contains("User canceled.")
    {
        return "Administrator approval was canceled before raw-device imaging could start.".into();
    }

    if let Some(error) = helper_error {
        return error;
    }

    if !combined_output.is_empty() {
        return format!("Administrator-approved read-only imaging failed: {combined_output}");
    }

    "Administrator-approved read-only imaging failed for an unknown reason.".into()
}
