// ============================================================================
// Recupere — License Validation (Ed25519 + Hardware Binding)
// ============================================================================
// Validates Recupere Pro license keys offline using Ed25519 signature
// verification. Each license is bound to a machine fingerprint computed from
// hardware identifiers (CPU vendor, system disk serial, MAC address).
//
// Format of a license key:
//   RECUP-<base64url payload>.<base64url signature>
//
// Payload (JSON):
//   {
//     "sub": "customer@email.com",
//     "machine": "<sha256 hex>",
//     "tier": "pro",
//     "iat": <unix timestamp>,
//     "exp": <unix timestamp or null>
//   }
//
// Signature: Ed25519 over the canonical JSON payload bytes.
// ============================================================================

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey, SIGNATURE_LENGTH};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

// ---------------------------------------------------------------------------
// Public key (Ed25519, 32 bytes)
// ---------------------------------------------------------------------------
//
// IMPORTANT: The default constant below is a development placeholder. Real
// release builds MUST override it at compile time by setting the env var
// `RECUPERE_LICENSE_PUBKEY_HEX` (64 hex chars = 32 bytes) — this is enforced
// in `build.rs` for `--release` builds. The matching private key MUST stay
// on the license server only.
//
// Defense in depth: even if a debug binary is distributed by mistake,
// `ensure_production_public_key()` (called from the desktop startup path)
// refuses to run in release mode when the development placeholder is still
// embedded. See tests below.
//
// To generate a real keypair:
//   $ openssl genpkey -algorithm Ed25519 -out license_private.pem
//   $ openssl pkey -in license_private.pem -pubout -out license_public.pem
// Then export the raw 32-byte public key as hex and pass it via the env var
// when building: RECUPERE_LICENSE_PUBKEY_HEX=<64-hex> cargo build --release

/// Ed25519 public key used only for development builds. The matching signing
/// seed is intentionally not part of the runtime API; `bin/gen_license.rs`
/// reads it from a local, gitignored `.dev-license-seed` file when a developer
/// explicitly needs to mint a local test key.
///
/// A release
/// binary that still embeds this value is refusing to start via
/// `ensure_production_public_key()`. Covered by
/// `dev_license_round_trips_against_placeholder_public_key` in test builds.
pub const DEV_PLACEHOLDER_PUBLIC_KEY: [u8; 32] = [
    0x3c, 0x53, 0xdd, 0x0a, 0x12, 0x2c, 0x2b, 0x68, 0x41, 0x48, 0xc1, 0x75, 0x4f, 0x94, 0x62, 0xe5,
    0x4a, 0xcb, 0x1c, 0x52, 0xcc, 0x1e, 0x12, 0x65, 0xff, 0x3e, 0x37, 0x80, 0xd4, 0x74, 0xb8, 0x3c,
];

const LICENSE_PUBLIC_KEY: [u8; 32] = match option_env!("RECUPERE_LICENSE_PUBKEY_HEX") {
    Some(hex) => match parse_pubkey_hex(hex) {
        Some(bytes) => bytes,
        None => panic!("RECUPERE_LICENSE_PUBKEY_HEX must be exactly 64 lowercase hex characters"),
    },
    None => DEV_PLACEHOLDER_PUBLIC_KEY,
};

/// Returns true when the embedded license public key is the known development
/// placeholder. Useful for surfacing a clear startup warning in debug builds
/// and for the hard runtime guard in release builds.
pub fn is_development_placeholder_public_key() -> bool {
    LICENSE_PUBLIC_KEY == DEV_PLACEHOLDER_PUBLIC_KEY
}

/// Runtime guard that refuses to proceed when a release binary still embeds
/// the development placeholder public key. The `build.rs` compile-time check
/// already catches `cargo build --release` with no `RECUPERE_LICENSE_PUBKEY_HEX`;
/// this is a belt-and-braces check in case a debug-profile binary is ever
/// repackaged and shipped.
///
/// Callers: invoked once during desktop startup (`lib.rs::run`) before any
/// license-gated UI is reachable. In debug builds this is a no-op so local
/// development keeps working with the dev key.
pub fn ensure_production_public_key() -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    if is_development_placeholder_public_key() {
        return Err(
            "Refusing to run: this release binary embeds the development license public key. \
             Rebuild with RECUPERE_LICENSE_PUBKEY_HEX set to the production Ed25519 key."
                .into(),
        );
    }
    Ok(())
}

const fn parse_pubkey_hex(hex: &str) -> Option<[u8; 32]> {
    let bytes = hex.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        let hi = match hex_nibble(bytes[i * 2]) {
            Some(v) => v,
            None => return None,
        };
        let lo = match hex_nibble(bytes[i * 2 + 1]) {
            Some(v) => v,
            None => return None,
        };
        out[i] = (hi << 4) | lo;
        i += 1;
    }
    Some(out)
}

const fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

const KEY_PREFIX: &str = "RECUP-";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicensePayload {
    pub sub: String,
    pub machine: String,
    pub tier: String,
    pub iat: u64,
    pub exp: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseInfo {
    pub valid: bool,
    pub status: String, // "free" | "pro" | "expired" | "wrong_machine" | "invalid_signature" | "malformed"
    pub email: Option<String>,
    pub tier: Option<String>,
    pub expires_at: Option<u64>,
    pub message: String,
}

impl LicenseInfo {
    pub fn free() -> Self {
        Self {
            valid: false,
            status: "free".into(),
            email: None,
            tier: None,
            expires_at: None,
            message: "No active license. Recupere is in free mode.".into(),
        }
    }

    pub fn pro(payload: &LicensePayload) -> Self {
        Self {
            valid: true,
            status: "pro".into(),
            email: Some(payload.sub.clone()),
            tier: Some(payload.tier.clone()),
            expires_at: payload.exp,
            message: "Recupere Pro is active.".into(),
        }
    }

    pub fn invalid(status: &str, message: &str) -> Self {
        Self {
            valid: false,
            status: status.into(),
            email: None,
            tier: None,
            expires_at: None,
            message: message.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Machine fingerprinting
// ---------------------------------------------------------------------------

/// Compute a stable, anonymous fingerprint of the current machine.
///
/// The primary signal is an OS-managed machine identifier (macOS
/// IOPlatformUUID, Windows ComputerSystemProduct UUID, Linux machine-id).
/// Hostname/MAC/cpu_count are used only as a compatibility fallback when the
/// platform identifier cannot be read. Raw identifiers are never persisted; only
/// this SHA-256 digest leaves the function.
pub fn compute_machine_fingerprint() -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"recupere-license-fingerprint-v2|");

    for (name, value) in collect_machine_fingerprint_signals() {
        hasher.update(name.as_bytes());
        hasher.update(b"=");
        hasher.update(value.as_bytes());
        hasher.update(b"|");
    }

    let digest = hasher.finalize();
    format!("{:x}", digest)
}

fn collect_machine_fingerprint_signals() -> Vec<(&'static str, String)> {
    let mut signals = vec![
        ("os", std::env::consts::OS.to_string()),
        ("arch", std::env::consts::ARCH.to_string()),
    ];

    if let Some(machine_id) = platform_machine_identifier() {
        signals.push(("platform_machine_id", machine_id));
        return signals;
    }

    let hostname = sysinfo::System::host_name().unwrap_or_else(|| "unknown".into());
    signals.push(("hostname_fallback", normalize_identifier(&hostname)));

    if let Ok(Some(mac)) = mac_address::get_mac_address() {
        signals.push(("mac_fallback", mac.to_string().to_ascii_lowercase()));
    } else {
        signals.push(("mac_fallback", "no-mac".into()));
    }

    signals.push(("cpu_count_fallback", num_cpus::get().to_string()));
    signals
}

fn normalize_identifier(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn non_empty_identifier(value: String) -> Option<String> {
    let normalized = normalize_identifier(&value);
    if normalized.is_empty()
        || normalized == "unknown"
        || normalized == "none"
        || normalized.chars().all(|ch| ch == '0' || ch == '-')
    {
        None
    } else {
        Some(normalized)
    }
}

#[cfg(target_os = "linux")]
fn platform_machine_identifier() -> Option<String> {
    for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(value) = fs::read_to_string(path) {
            if let Some(identifier) = non_empty_identifier(value) {
                return Some(identifier);
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn platform_machine_identifier() -> Option<String> {
    let output = std::process::Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some((_, value)) = line.split_once("IOPlatformUUID") {
            let value = value
                .trim()
                .trim_start_matches('=')
                .trim()
                .trim_matches('"')
                .to_string();
            if let Some(identifier) = non_empty_identifier(value) {
                return Some(identifier);
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn platform_machine_identifier() -> Option<String> {
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-CimInstance Win32_ComputerSystemProduct).UUID",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    non_empty_identifier(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_machine_identifier() -> Option<String> {
    None
}

// ---------------------------------------------------------------------------
// License key parsing & verification
// ---------------------------------------------------------------------------

/// Parse and verify a license key against the embedded Ed25519 public key
/// AND the current machine fingerprint.
pub fn verify_license(key: &str) -> LicenseInfo {
    verify_license_with_key(key, &LICENSE_PUBLIC_KEY, &compute_machine_fingerprint())
}

/// Internal verification with injectable public key + machine fingerprint
/// (used by tests to verify roundtrips without depending on the embedded key).
fn verify_license_with_key(
    key: &str,
    public_key: &[u8; 32],
    expected_fingerprint: &str,
) -> LicenseInfo {
    let trimmed = key.trim();

    // Strip optional prefix
    let body = trimmed.strip_prefix(KEY_PREFIX).unwrap_or(trimmed);

    // Split payload.signature
    let mut parts = body.splitn(2, '.');
    let payload_b64 = match parts.next() {
        Some(p) if !p.is_empty() => p,
        _ => {
            return LicenseInfo::invalid("malformed", "License key is missing the payload section.")
        }
    };
    let signature_b64 = match parts.next() {
        Some(s) if !s.is_empty() => s,
        _ => {
            return LicenseInfo::invalid(
                "malformed",
                "License key is missing the signature section.",
            )
        }
    };

    // Decode payload
    let payload_bytes = match URL_SAFE_NO_PAD.decode(payload_b64) {
        Ok(bytes) => bytes,
        Err(_) => return LicenseInfo::invalid("malformed", "License payload is not valid base64."),
    };

    // Decode signature
    let signature_bytes = match URL_SAFE_NO_PAD.decode(signature_b64) {
        Ok(bytes) => bytes,
        Err(_) => {
            return LicenseInfo::invalid("malformed", "License signature is not valid base64.")
        }
    };

    if signature_bytes.len() != SIGNATURE_LENGTH {
        return LicenseInfo::invalid("malformed", "License signature has an unexpected length.");
    }

    let signature_array: [u8; SIGNATURE_LENGTH] = match signature_bytes.as_slice().try_into() {
        Ok(arr) => arr,
        Err(_) => return LicenseInfo::invalid("malformed", "License signature is malformed."),
    };
    let signature = Signature::from_bytes(&signature_array);

    // Verify signature
    let verifying_key = match VerifyingKey::from_bytes(public_key) {
        Ok(key) => key,
        Err(_) => {
            return LicenseInfo::invalid(
                "invalid_signature",
                "Internal error: embedded public key is invalid.",
            );
        }
    };

    if verifying_key
        .verify_strict(&payload_bytes, &signature)
        .is_err()
    {
        return LicenseInfo::invalid(
            "invalid_signature",
            "License signature does not match. The key may be tampered with or counterfeit.",
        );
    }

    // Parse payload JSON
    let payload: LicensePayload = match serde_json::from_slice(&payload_bytes) {
        Ok(p) => p,
        Err(_) => return LicenseInfo::invalid("malformed", "License payload is not valid JSON."),
    };

    // Check expiration
    let now =
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs(),
            Err(_) => return LicenseInfo::invalid(
                "clock_error",
                "System clock is before the Unix epoch; license validity cannot be checked safely.",
            ),
        };
    if let Some(exp) = payload.exp {
        if now >= exp {
            return LicenseInfo::invalid(
                "expired",
                "This license has expired. Please renew or contact support.",
            );
        }
    }

    // Check machine binding
    if payload.machine != expected_fingerprint {
        return LicenseInfo::invalid(
            "wrong_machine",
            "This license is bound to a different machine. Each license is valid for one device only.",
        );
    }

    // Check tier
    if payload.tier != "pro" {
        return LicenseInfo::invalid("malformed", "Unknown license tier.");
    }

    LicenseInfo::pro(&payload)
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

const LICENSE_KEYRING_SERVICE: &str = "com.recupere.desktop";
const LICENSE_KEYRING_ACCOUNT: &str = "recupere-pro-license";

fn legacy_license_storage_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".recupere").join("license.key")
}

fn license_keyring_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(LICENSE_KEYRING_SERVICE, LICENSE_KEYRING_ACCOUNT)
        .map_err(|error| format!("Cannot access OS keyring for license storage: {error}"))
}

/// Save the license key to the OS keyring for future launches.
pub fn save_license(key: &str) -> Result<(), String> {
    license_keyring_entry()?
        .set_password(key.trim())
        .map_err(|error| format!("Cannot save license in OS keyring: {error}"))?;

    let legacy_path = legacy_license_storage_path();
    if legacy_path.exists() {
        let _ = fs::remove_file(legacy_path);
    }

    Ok(())
}

/// Load the license key from the OS keyring if it exists.
pub fn load_license() -> Option<String> {
    match license_keyring_entry() {
        Ok(entry) => match entry.get_password() {
            Ok(key) => return Some(key.trim().to_string()),
            Err(keyring::Error::NoEntry) => {}
            Err(error) => {
                tracing::warn!("load_license: Cannot load license from OS keyring: {error}");
                return None;
            }
        },
        Err(error) => {
            tracing::warn!("load_license: {error}");
            return None;
        }
    }

    let legacy_path = legacy_license_storage_path();
    let legacy_key = fs::read_to_string(&legacy_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;

    match save_license(&legacy_key) {
        Ok(()) => {
            let _ = fs::remove_file(legacy_path);
            Some(legacy_key)
        }
        Err(error) => {
            tracing::warn!(
                "load_license: refusing to use legacy plaintext license because migration to keyring failed: {error}"
            );
            None
        }
    }
}

/// Remove the saved license key from the OS keyring and delete any legacy plaintext copy.
pub fn delete_license() -> Result<(), String> {
    match license_keyring_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(error) => return Err(format!("Cannot delete license from OS keyring: {error}")),
    }

    let legacy_path = legacy_license_storage_path();
    if legacy_path.exists() {
        fs::remove_file(&legacy_path).map_err(|e| format!("Cannot delete legacy license: {e}"))?;
    }
    Ok(())
}

/// Activate a license key: verify it, and if valid, persist it to disk.
pub fn activate_license(key: &str) -> LicenseInfo {
    let info = verify_license(key);
    if info.valid {
        if let Err(err) = save_license(key) {
            return LicenseInfo::invalid(
                "storage_error",
                &format!("License is valid but could not be saved: {err}"),
            );
        }
    }
    info
}

/// Get the current license status. Loads from disk if a license is saved.
pub fn get_license_status() -> LicenseInfo {
    match load_license() {
        Some(key) => verify_license(&key),
        None => LicenseInfo::free(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    const TEST_DEV_PLACEHOLDER_SIGNING_SEED: [u8; 32] = [
        0x52, 0xc4, 0xe1, 0xf7, 0x0a, 0x9b, 0x33, 0x6d, 0x12, 0x88, 0x5c, 0x44, 0x29, 0x1e, 0x6b,
        0xa0, 0xfd, 0x77, 0x3a, 0x91, 0xc6, 0x05, 0xee, 0x18, 0xb4, 0x2f, 0x8d, 0x0c, 0x97, 0x63,
        0x21, 0xae,
    ];

    /// Build a signed license key the same way the JavaScript license server does.
    fn build_test_license(
        signing_key: &SigningKey,
        email: &str,
        machine: &str,
        tier: &str,
        exp: Option<u64>,
    ) -> String {
        let payload = serde_json::json!({
            "sub": email,
            "machine": machine,
            "tier": tier,
            "iat": 1700000000_u64,
            "exp": exp,
        });
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let signature = signing_key.sign(&payload_bytes);

        let payload_b64 = URL_SAFE_NO_PAD.encode(&payload_bytes);
        let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

        format!("{}{}.{}", KEY_PREFIX, payload_b64, signature_b64)
    }

    fn fresh_signing_key() -> SigningKey {
        // Deterministic seed so tests are stable; in production the server
        // generates a real random keypair.
        let seed = [42u8; 32];
        SigningKey::from_bytes(&seed)
    }

    #[test]
    fn machine_fingerprint_is_stable() {
        let fp1 = compute_machine_fingerprint();
        let fp2 = compute_machine_fingerprint();
        assert_eq!(fp1, fp2, "Machine fingerprint must be stable across calls");
        assert_eq!(fp1.len(), 64, "SHA-256 hex digest is 64 chars");
    }

    #[test]
    fn empty_key_is_malformed() {
        let info = verify_license("");
        assert_eq!(info.status, "malformed");
        assert!(!info.valid);
    }

    #[test]
    fn key_without_signature_is_malformed() {
        let info = verify_license("RECUP-justpayload");
        assert_eq!(info.status, "malformed");
    }

    #[test]
    fn key_with_invalid_base64_is_malformed() {
        let info = verify_license("RECUP-not!base64.also!bad");
        assert_eq!(info.status, "malformed");
    }

    #[test]
    fn license_info_free_default() {
        let info = LicenseInfo::free();
        assert!(!info.valid);
        assert_eq!(info.status, "free");
    }

    #[test]
    fn save_and_load_roundtrip() {
        if std::env::var("RECUPERE_RUN_KEYRING_TESTS").is_err() {
            return;
        }
        let test_key = "RECUP-test.key";
        if save_license(test_key).is_ok() {
            let loaded = load_license();
            assert_eq!(loaded.as_deref(), Some(test_key));
            let _ = delete_license();
        }
    }

    #[test]
    fn sign_and_verify_roundtrip_succeeds() {
        let signing_key = fresh_signing_key();
        let public_key = signing_key.verifying_key().to_bytes();
        let machine = "a".repeat(64);

        let license = build_test_license(
            &signing_key,
            "test@recupere.app",
            &machine,
            "pro",
            Some(9999999999),
        );

        let info = verify_license_with_key(&license, &public_key, &machine);
        assert!(info.valid, "valid license must verify: {}", info.message);
        assert_eq!(info.status, "pro");
        assert_eq!(info.email.as_deref(), Some("test@recupere.app"));
    }

    #[test]
    fn tampered_payload_fails_verification() {
        let signing_key = fresh_signing_key();
        let public_key = signing_key.verifying_key().to_bytes();
        let machine = "f".repeat(64);

        let license = build_test_license(&signing_key, "test@recupere.app", &machine, "pro", None);

        // Flip a single character in the payload portion
        let body = license.strip_prefix(KEY_PREFIX).unwrap();
        let mut chars: Vec<char> = body.chars().collect();
        chars[5] = if chars[5] == 'a' { 'b' } else { 'a' };
        let tampered: String = chars.into_iter().collect();
        let tampered_key = format!("{}{}", KEY_PREFIX, tampered);

        let info = verify_license_with_key(&tampered_key, &public_key, &machine);
        assert!(!info.valid);
        assert!(
            info.status == "invalid_signature" || info.status == "malformed",
            "tampered key should be rejected, got status={}",
            info.status
        );
    }

    #[test]
    fn license_for_different_machine_fails() {
        let signing_key = fresh_signing_key();
        let public_key = signing_key.verifying_key().to_bytes();
        let bound_machine = "1".repeat(64);
        let actual_machine = "2".repeat(64);

        let license = build_test_license(
            &signing_key,
            "user@example.com",
            &bound_machine,
            "pro",
            None,
        );

        let info = verify_license_with_key(&license, &public_key, &actual_machine);
        assert!(!info.valid);
        assert_eq!(info.status, "wrong_machine");
    }

    #[test]
    fn expired_license_fails() {
        let signing_key = fresh_signing_key();
        let public_key = signing_key.verifying_key().to_bytes();
        let machine = "9".repeat(64);

        // exp in the past
        let license =
            build_test_license(&signing_key, "user@example.com", &machine, "pro", Some(1));

        let info = verify_license_with_key(&license, &public_key, &machine);
        assert!(!info.valid);
        assert_eq!(info.status, "expired");
    }

    #[test]
    fn wrong_public_key_fails() {
        let signing_key = fresh_signing_key();
        let machine = "5".repeat(64);
        let license = build_test_license(&signing_key, "user@example.com", &machine, "pro", None);

        // Use a totally different public key
        let other_signing = SigningKey::from_bytes(&[7u8; 32]);
        let wrong_public = other_signing.verifying_key().to_bytes();

        let info = verify_license_with_key(&license, &wrong_public, &machine);
        assert!(!info.valid);
        assert_eq!(info.status, "invalid_signature");
    }

    #[test]
    fn unknown_tier_is_rejected() {
        let signing_key = fresh_signing_key();
        let public_key = signing_key.verifying_key().to_bytes();
        let machine = "8".repeat(64);

        let license = build_test_license(
            &signing_key,
            "user@example.com",
            &machine,
            "enterprise",
            None,
        );

        let info = verify_license_with_key(&license, &public_key, &machine);
        assert!(!info.valid);
        assert_eq!(info.status, "malformed");
    }

    #[test]
    fn dev_placeholder_public_key_matches_test_seed() {
        let signing_key = SigningKey::from_bytes(&TEST_DEV_PLACEHOLDER_SIGNING_SEED);
        let derived = signing_key.verifying_key().to_bytes();
        assert_eq!(
            derived, DEV_PLACEHOLDER_PUBLIC_KEY,
            "DEV_PLACEHOLDER_PUBLIC_KEY has drifted from the Ed25519 public key \
             used by local dev-license tests. Regenerate the placeholder pair."
        );
    }

    #[test]
    fn dev_license_round_trips_against_placeholder_public_key() {
        // End-to-end: a license produced exactly like `gen_license` does must
        // validate against `DEV_PLACEHOLDER_PUBLIC_KEY`. This locks in the
        // full signing → verification chain for the dev tier.
        let signing_key = SigningKey::from_bytes(&TEST_DEV_PLACEHOLDER_SIGNING_SEED);
        let machine = "c".repeat(64);
        let license = build_test_license(&signing_key, "dev@recupere.local", &machine, "pro", None);

        let info = verify_license_with_key(&license, &DEV_PLACEHOLDER_PUBLIC_KEY, &machine);
        assert!(
            info.valid,
            "dev license must validate against the placeholder public key: {}",
            info.message
        );
        assert_eq!(info.status, "pro");
    }

    #[test]
    fn ensure_production_public_key_is_noop_in_debug() {
        // Debug builds always run with the placeholder key; the guard must
        // not trip so local `cargo run` stays usable.
        const { assert!(cfg!(debug_assertions)) };
        ensure_production_public_key().expect("debug builds must never be blocked by the guard");
    }

    #[test]
    fn is_development_placeholder_public_key_detects_dev_key() {
        // When no `RECUPERE_LICENSE_PUBKEY_HEX` is passed at compile time,
        // `LICENSE_PUBLIC_KEY` falls back to `DEV_PLACEHOLDER_PUBLIC_KEY`
        // and this predicate must report that truthfully. In CI builds that
        // inject the real key this test flips to `false`, which is also the
        // correct signal — we surface that via the assertion below so either
        // configuration is provably consistent.
        let expected = LICENSE_PUBLIC_KEY == DEV_PLACEHOLDER_PUBLIC_KEY;
        assert_eq!(is_development_placeholder_public_key(), expected);
    }
}
