// ============================================================================
// Récupère — Dev license generator
// ============================================================================
// Mints a Pro license signed with the development Ed25519 seed that matches
// the placeholder pubkey embedded in `license/mod.rs`. Usage:
//
//   cargo run --bin gen_license -- you@example.com
//
// The generated key is bound to the *current* machine's fingerprint, so it
// will only validate when the same machine runs Récupère. Run it on the
// machine you want to license.
// ============================================================================

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use recupere_lib::license::{compute_machine_fingerprint, DEV_PLACEHOLDER_SIGNING_SEED};
use std::env;

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

    let signing_key = SigningKey::from_bytes(&DEV_PLACEHOLDER_SIGNING_SEED);
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
