// ============================================================================
// Récupère — Dev license generator
// ============================================================================
// Mints a Pro license signed with a local development Ed25519 seed. Usage:
//
//   cargo run --bin gen_license -- you@example.com
//
// The seed is read from `RECUPERE_DEV_LICENSE_SEED_HEX` or a gitignored
// `.dev-license-seed` file containing 64 lowercase hex characters.
//
// The generated key is bound to the *current* machine's fingerprint, so it
// will only validate when the same machine runs Récupère. Run it on the
// machine you want to license.
// ============================================================================

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use recupere_lib::license::compute_machine_fingerprint;
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let email = env::args()
        .nth(1)
        .unwrap_or_else(|| "dev@recupere.local".into());
    let tier = env::args().nth(2).unwrap_or_else(|| "pro".into());

    let machine = compute_machine_fingerprint();

    let payload = serde_json::json!({
        "sub": email,
        "machine": machine,
        "tier": tier,
        "iat": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        "exp": null,
    });
    let payload_bytes = serde_json::to_vec(&payload).expect("serialize payload");

    let signing_seed = load_dev_signing_seed();
    let signing_key = SigningKey::from_bytes(&signing_seed);
    let signature = signing_key.sign(&payload_bytes);

    let payload_b64 = URL_SAFE_NO_PAD.encode(&payload_bytes);
    let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

    let key = format!("RECUP-{payload_b64}.{signature_b64}");

    println!();
    println!("=== Récupère dev license ===");
    println!("email   : {email}");
    println!("tier    : {tier}");
    println!("machine : {machine}");
    println!();
    println!("{key}");
    println!();
}

fn load_dev_signing_seed() -> [u8; 32] {
    if let Ok(seed) = env::var("RECUPERE_DEV_LICENSE_SEED_HEX") {
        return parse_seed_hex(seed.trim()).unwrap_or_else(|error| panic!("{error}"));
    }

    for path in dev_seed_candidates() {
        if let Ok(seed) = fs::read_to_string(&path) {
            return parse_seed_hex(seed.trim()).unwrap_or_else(|error| {
                panic!(
                    "Invalid dev license seed at {}: {error}",
                    path.to_string_lossy()
                )
            });
        }
    }

    panic!(
        "Missing dev license seed. Create a gitignored `.dev-license-seed` file \
         with 64 lowercase hex chars, or set RECUPERE_DEV_LICENSE_SEED_HEX."
    );
}

fn dev_seed_candidates() -> Vec<PathBuf> {
    let current = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    vec![
        current.join(".dev-license-seed"),
        current.join("src-tauri").join(".dev-license-seed"),
    ]
}

fn parse_seed_hex(hex: &str) -> Result<[u8; 32], String> {
    let bytes = hex.as_bytes();
    if bytes.len() != 64 {
        return Err("expected exactly 64 lowercase hex characters".into());
    }

    let mut out = [0_u8; 32];
    for index in 0..32 {
        let hi = hex_nibble(bytes[index * 2])?;
        let lo = hex_nibble(bytes[index * 2 + 1])?;
        out[index] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err("seed must use lowercase hex characters only".into()),
    }
}
