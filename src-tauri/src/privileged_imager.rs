#[cfg(test)]
use crate::imaging;
use std::ffi::{OsStr, OsString};
#[cfg(test)]
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const PRIVILEGED_IMAGER_FLAG: &str = "--recupere-imager";

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct PrivilegedImagerRequest {
    source_path: PathBuf,
    destination_path: PathBuf,
    progress_file: PathBuf,
    error_file: PathBuf,
    summary_file: PathBuf,
    profile: imaging::ImagingProfile,
}

pub fn is_privileged_imager_mode(args: &[OsString]) -> bool {
    args.iter()
        .any(|argument| argument == OsStr::new(PRIVILEGED_IMAGER_FLAG))
}

pub fn run_privileged_imager_from_args(args: Vec<OsString>) -> Result<(), String> {
    let _ = args;
    Err(
        "The privileged imaging helper is disabled in this build. Use a read-only source image or \
         run imaging from an environment that already has the required read permissions."
            .into(),
    )
}

#[cfg(test)]
fn parse_privileged_imager_request(args: &[OsString]) -> Result<PrivilegedImagerRequest, String> {
    let helper_flag_index = args
        .iter()
        .position(|argument| argument == OsStr::new(PRIVILEGED_IMAGER_FLAG))
        .ok_or_else(|| {
            format!(
                "The privileged imager flag `{}` is missing from the current process arguments.",
                PRIVILEGED_IMAGER_FLAG
            )
        })?;

    let mut source_path = None;
    let mut destination_path = None;
    let mut progress_file = None;
    let mut error_file = None;
    let mut summary_file = None;
    let mut profile = imaging::ImagingProfile::Standard;
    let mut index = helper_flag_index + 1;

    while index < args.len() {
        let flag = args[index].to_string_lossy().to_string();
        let value = args.get(index + 1).ok_or_else(|| {
            format!("The privileged imager argument `{flag}` is missing its value.")
        })?;

        match flag.as_str() {
            "--source" => source_path = Some(PathBuf::from(value)),
            "--destination" => destination_path = Some(PathBuf::from(value)),
            "--progress-file" => progress_file = Some(PathBuf::from(value)),
            "--error-file" => error_file = Some(PathBuf::from(value)),
            "--summary-file" => summary_file = Some(PathBuf::from(value)),
            "--profile" => {
                profile = match value.to_string_lossy().as_ref() {
                    "standard" => imaging::ImagingProfile::Standard,
                    "cautious" => imaging::ImagingProfile::Cautious,
                    other => {
                        return Err(format!("Unsupported privileged imager profile `{other}`."));
                    }
                };
            }
            other => {
                return Err(format!("Unsupported privileged imager argument `{other}`."));
            }
        }

        index += 2;
    }

    Ok(PrivilegedImagerRequest {
        source_path: source_path
            .ok_or_else(|| "The privileged imager requires a `--source` path.".to_string())?,
        destination_path: destination_path
            .ok_or_else(|| "The privileged imager requires a `--destination` path.".to_string())?,
        progress_file: progress_file.ok_or_else(|| {
            "The privileged imager requires a `--progress-file` path.".to_string()
        })?,
        error_file: error_file
            .ok_or_else(|| "The privileged imager requires an `--error-file` path.".to_string())?,
        summary_file: summary_file
            .ok_or_else(|| "The privileged imager requires a `--summary-file` path.".to_string())?,
        profile,
    })
}

#[cfg(test)]
fn run_privileged_imager_request(request: &PrivilegedImagerRequest) -> Result<(), String> {
    let _ = fs::remove_file(&request.error_file);
    let _ = fs::remove_file(&request.progress_file);
    let _ = fs::remove_file(&request.summary_file);

    let result = imaging::create_read_only_image_at_with_profile(
        &request.destination_path,
        &request.source_path,
        request.profile,
        &mut |copied_bytes| {
            write_optional_report(&request.progress_file, copied_bytes.to_string());
        },
    );

    match result {
        Ok(artifact) => {
            write_optional_report(&request.progress_file, artifact.bytes_copied.to_string());
            write_optional_report(
                &request.summary_file,
                serde_json::to_string(&artifact).unwrap_or_else(|_| "{}".to_string()),
            );
            Ok(())
        }
        Err(error) => {
            write_optional_report(&request.error_file, error.clone());
            Err(error)
        }
    }
}

#[cfg(test)]
fn write_optional_report(path: &Path, content: String) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, content);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!(
            "recupere-privileged-imager-{prefix}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temporary directory should exist");
        dir
    }

    #[test]
    fn parse_privileged_imager_request_reads_required_paths() {
        let args = vec![
            OsString::from("/Applications/Recupere.app/Contents/MacOS/recupere"),
            OsString::from(PRIVILEGED_IMAGER_FLAG),
            OsString::from("--source"),
            OsString::from("/dev/rdisk4"),
            OsString::from("--destination"),
            OsString::from("/tmp/capture.img"),
            OsString::from("--progress-file"),
            OsString::from("/tmp/progress.txt"),
            OsString::from("--error-file"),
            OsString::from("/tmp/error.txt"),
            OsString::from("--summary-file"),
            OsString::from("/tmp/summary.json"),
        ];

        let request = parse_privileged_imager_request(&args)
            .expect("helper arguments should parse successfully");

        assert_eq!(request.source_path, PathBuf::from("/dev/rdisk4"));
        assert_eq!(request.destination_path, PathBuf::from("/tmp/capture.img"));
        assert_eq!(request.progress_file, PathBuf::from("/tmp/progress.txt"));
        assert_eq!(request.error_file, PathBuf::from("/tmp/error.txt"));
        assert_eq!(request.summary_file, PathBuf::from("/tmp/summary.json"));
        assert_eq!(request.profile, imaging::ImagingProfile::Standard);
    }

    #[test]
    fn parse_privileged_imager_request_reads_optional_profile() {
        let args = vec![
            OsString::from("/Applications/Recupere.app/Contents/MacOS/recupere"),
            OsString::from(PRIVILEGED_IMAGER_FLAG),
            OsString::from("--source"),
            OsString::from("/dev/rdisk4"),
            OsString::from("--destination"),
            OsString::from("/tmp/capture.img"),
            OsString::from("--progress-file"),
            OsString::from("/tmp/progress.txt"),
            OsString::from("--error-file"),
            OsString::from("/tmp/error.txt"),
            OsString::from("--summary-file"),
            OsString::from("/tmp/summary.json"),
            OsString::from("--profile"),
            OsString::from("cautious"),
        ];

        let request = parse_privileged_imager_request(&args)
            .expect("helper arguments with a cautious profile should parse successfully");

        assert_eq!(request.profile, imaging::ImagingProfile::Cautious);
    }

    #[test]
    fn parse_privileged_imager_request_rejects_missing_values() {
        let args = vec![
            OsString::from("recupere"),
            OsString::from(PRIVILEGED_IMAGER_FLAG),
            OsString::from("--source"),
        ];

        let error = parse_privileged_imager_request(&args)
            .expect_err("incomplete helper arguments should fail");

        assert!(error.contains("missing its value"));
    }

    #[test]
    fn run_privileged_imager_request_copies_source_and_updates_progress() {
        let root = unique_temp_dir("success");
        let source = root.join("source.bin");
        let destination = root.join("capture.img");
        let progress_file = root.join("progress.txt");
        let error_file = root.join("error.txt");
        let summary_file = root.join("summary.json");
        fs::write(&source, b"hello privileged imaging").expect("source fixture should be written");

        let request = PrivilegedImagerRequest {
            source_path: source.clone(),
            destination_path: destination.clone(),
            progress_file: progress_file.clone(),
            error_file: error_file.clone(),
            summary_file: summary_file.clone(),
            profile: imaging::ImagingProfile::Standard,
        };

        run_privileged_imager_request(&request)
            .expect("privileged helper should copy a regular source file");

        assert_eq!(
            fs::read(&destination).expect("destination image should exist"),
            b"hello privileged imaging"
        );
        assert_eq!(
            fs::read_to_string(&progress_file)
                .expect("progress file should exist")
                .trim(),
            fs::metadata(&destination)
                .expect("destination metadata should exist")
                .len()
                .to_string()
        );
        assert!(fs::read_to_string(&summary_file)
            .expect("summary file should exist")
            .contains("\"bytes_copied\":24"));
        assert!(!error_file.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn run_privileged_imager_request_writes_error_file_when_imaging_fails() {
        let root = unique_temp_dir("failure");
        let destination = root.join("capture.img");
        let progress_file = root.join("progress.txt");
        let error_file = root.join("error.txt");
        let summary_file = root.join("summary.json");

        let request = PrivilegedImagerRequest {
            source_path: root.join("missing.bin"),
            destination_path: destination,
            progress_file,
            error_file: error_file.clone(),
            summary_file,
            profile: imaging::ImagingProfile::Standard,
        };

        let error =
            run_privileged_imager_request(&request).expect_err("missing source should fail");

        assert!(error.contains("Unable to open the source"));
        assert!(fs::read_to_string(&error_file)
            .expect("error report should be written")
            .contains("Unable to open the source"));

        let _ = fs::remove_dir_all(root);
    }
}
