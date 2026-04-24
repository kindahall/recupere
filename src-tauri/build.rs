fn main() {
    // Make the license public key env var visible to the compiler so that
    // changes trigger a rebuild of license/mod.rs.
    println!("cargo:rerun-if-env-changed=RECUPERE_LICENSE_PUBKEY_HEX");

    // Refuse to produce any non-debug build that still ships the development
    // placeholder license key. Set RECUPERE_LICENSE_PUBKEY_HEX to the real
    // 64-hex-char Ed25519 public key on the release pipeline.
    let profile = std::env::var("PROFILE").unwrap_or_default();
    let debug_profile = std::env::var("DEBUG")
        .map(|value| value == "true")
        .unwrap_or(profile == "debug");
    if !debug_profile {
        let pubkey = std::env::var("RECUPERE_LICENSE_PUBKEY_HEX").unwrap_or_default();
        validate_release_pubkey(&profile, &pubkey);
    }

    // tauri_build only needs to run when the desktop feature is enabled.
    // The headless build (used by recupere-agent) skips it so it can compile
    // without GUI deps.
    if std::env::var("CARGO_FEATURE_DESKTOP").is_ok() {
        tauri_build::build()
    }
}

fn validate_release_pubkey(profile: &str, pubkey: &str) {
    const DEV_PLACEHOLDER_PUBLIC_KEY_HEX: &str =
        "3c53dd0a122c2b684148c1754f9462e54acb1c52cc1e1265ff3e3780d474b83c";

    if pubkey.len() != 64
        || !pubkey
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        panic!(
            "Non-debug build profile `{profile}` refused: RECUPERE_LICENSE_PUBKEY_HEX \
             must be exactly 64 lowercase hex characters."
        );
    }

    if pubkey.bytes().all(|byte| byte == b'0') {
        panic!(
            "Non-debug build profile `{profile}` refused: RECUPERE_LICENSE_PUBKEY_HEX \
             cannot be the all-zero placeholder."
        );
    }

    if pubkey == DEV_PLACEHOLDER_PUBLIC_KEY_HEX {
        panic!(
            "Non-debug build profile `{profile}` refused: RECUPERE_LICENSE_PUBKEY_HEX \
             is the development placeholder public key."
        );
    }
}
