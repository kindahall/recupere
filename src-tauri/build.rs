fn main() {
    // Make the license public key env var visible to the compiler so that
    // changes trigger a rebuild of license/mod.rs.
    println!("cargo:rerun-if-env-changed=RECUPERE_LICENSE_PUBKEY_HEX");

    // Refuse to produce a release build that still ships the development
    // placeholder license key. Set RECUPERE_LICENSE_PUBKEY_HEX to the real
    // 64-hex-char Ed25519 public key on the release pipeline.
    let profile = std::env::var("PROFILE").unwrap_or_default();
    if profile == "release" && std::env::var("RECUPERE_LICENSE_PUBKEY_HEX").is_err() {
        panic!(
            "Release build refused: RECUPERE_LICENSE_PUBKEY_HEX is not set. \
             Set it to the production Ed25519 public key (64 hex chars) before \
             building a release artifact."
        );
    }

    // tauri_build only needs to run when the desktop feature is enabled.
    // The headless build (used by recupere-agent) skips it so it can compile
    // without GUI deps.
    if std::env::var("CARGO_FEATURE_DESKTOP").is_ok() {
        tauri_build::build()
    }
}
