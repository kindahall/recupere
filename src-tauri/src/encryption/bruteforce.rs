// ============================================================================
// Recupere — Dictionary brute-force for encrypted volumes
// ============================================================================
// Tries an embedded common-password list, plus an optional user wordlist,
// against the unlock_volume() entry point. Single-threaded by design: native
// unlock tools (cryptsetup, diskutil, manage-bde) are slow per-attempt and
// don't benefit from parallelism without GPU/hash extraction.
//
// LEGAL: callers MUST display a consent prompt confirming the user is the
// legitimate owner of the volume before invoking this module.
// ============================================================================

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Top common passwords (subset of SecLists rockyou-top). Embedded so the
/// app works offline. Users can supply additional wordlists at runtime.
pub const COMMON_PASSWORDS: &[&str] = &[
    "123456",
    "password",
    "12345678",
    "qwerty",
    "123456789",
    "12345",
    "1234",
    "111111",
    "1234567",
    "dragon",
    "123123",
    "baseball",
    "abc123",
    "football",
    "monkey",
    "letmein",
    "shadow",
    "master",
    "666666",
    "qwertyuiop",
    "123321",
    "mustang",
    "1234567890",
    "michael",
    "654321",
    "superman",
    "1qaz2wsx",
    "7777777",
    "121212",
    "000000",
    "qazwsx",
    "123qwe",
    "killer",
    "trustno1",
    "jordan",
    "jennifer",
    "zxcvbnm",
    "asdfgh",
    "hunter",
    "buster",
    "soccer",
    "harley",
    "batman",
    "andrew",
    "tigger",
    "sunshine",
    "iloveyou",
    "2000",
    "charlie",
    "robert",
    "thomas",
    "hockey",
    "ranger",
    "daniel",
    "starwars",
    "klaster",
    "112233",
    "george",
    "computer",
    "michelle",
    "jessica",
    "pepper",
    "1111",
    "zxcvbn",
    "555555",
    "11111111",
    "131313",
    "freedom",
    "777777",
    "pass",
    "maggie",
    "159753",
    "aaaaaa",
    "ginger",
    "princess",
    "joshua",
    "cheese",
    "amanda",
    "summer",
    "love",
    "ashley",
    "nicole",
    "chelsea",
    "biteme",
    "matthew",
    "access",
    "yankees",
    "987654321",
    "dallas",
    "austin",
    "thunder",
    "taylor",
    "matrix",
    "mobilemail",
    "mom",
    "monitor",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BruteForceProgress {
    pub attempted: usize,
    pub total: usize,
    pub current_password_preview: String,
    pub elapsed_ms: u128,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BruteForceResult {
    pub success: bool,
    pub password: Option<String>,
    pub attempts: usize,
    pub elapsed_ms: u128,
    pub message: String,
}

/// Run a dictionary attack against `device_path` using the embedded list plus
/// any extra passwords supplied by the caller. Calls `progress_cb` after each
/// attempt; `cancel` is checked between attempts and aborts cleanly when set.
pub fn run_dictionary_attack(
    device_path: &str,
    extra_passwords: &[String],
    cancel: Arc<AtomicBool>,
    mut progress_cb: impl FnMut(&BruteForceProgress),
) -> BruteForceResult {
    let start = std::time::Instant::now();
    let total = COMMON_PASSWORDS.len() + extra_passwords.len();
    let mut attempted = 0usize;

    let iter = COMMON_PASSWORDS
        .iter()
        .map(|s| s.to_string())
        .chain(extra_passwords.iter().cloned());

    for pw in iter {
        if cancel.load(Ordering::Relaxed) {
            return BruteForceResult {
                success: false,
                password: None,
                attempts: attempted,
                elapsed_ms: start.elapsed().as_millis(),
                message: "Cancelled by user.".into(),
            };
        }
        attempted += 1;
        let result = super::unlock_volume(device_path, &pw);
        let elapsed_ms = start.elapsed().as_millis();
        let preview = preview(&pw);
        progress_cb(&BruteForceProgress {
            attempted,
            total,
            current_password_preview: preview.clone(),
            elapsed_ms,
            success: result.is_ok(),
        });
        if result.is_ok() {
            return BruteForceResult {
                success: true,
                password: Some(pw),
                attempts: attempted,
                elapsed_ms,
                message: format!("Password found after {attempted} attempts."),
            };
        }
    }

    BruteForceResult {
        success: false,
        password: None,
        attempts: attempted,
        elapsed_ms: start.elapsed().as_millis(),
        message: "Dictionary exhausted, no match.".into(),
    }
}

fn preview(pw: &str) -> String {
    if pw.len() <= 2 {
        "**".into()
    } else {
        format!("{}{}", &pw[..1], "*".repeat(pw.len() - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_is_non_empty() {
        assert!(COMMON_PASSWORDS.len() >= 50);
    }

    #[test]
    fn cancellation_short_circuits() {
        let cancel = Arc::new(AtomicBool::new(true));
        let result = run_dictionary_attack("/nonexistent", &[], cancel, |_| {});
        assert!(!result.success);
        assert_eq!(result.attempts, 0);
    }

    #[test]
    fn preview_masks_password() {
        assert_eq!(preview("hunter2"), "h******");
        assert_eq!(preview("ab"), "**");
    }
}
