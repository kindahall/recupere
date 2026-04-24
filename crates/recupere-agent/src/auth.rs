// Bearer-token auth for the agent.
//
// `init_token` generates 32 random bytes, hex-encodes them, hashes the result
// with SHA-256 and writes the hash to `<state_dir>/token.hash`. The plaintext
// token is returned to the caller exactly once.
//
// `verify_bearer` reads the stored hash and compares it (constant-time) to the
// SHA-256 of a presented token. We never load the plaintext from disk.

use rand::RngCore;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const TOKEN_FILE: &str = "token.hash";

fn token_path(state_dir: &Path) -> PathBuf {
    state_dir.join(TOKEN_FILE)
}

pub fn init_token(state_dir: &Path) -> Result<String, String> {
    fs::create_dir_all(state_dir).map_err(|error| {
        format!(
            "unable to create state directory {}: {error}",
            state_dir.display()
        )
    })?;
    #[cfg(unix)]
    fs::set_permissions(state_dir, fs::Permissions::from_mode(0o700)).map_err(|error| {
        format!(
            "unable to lock down state directory {}: {error}",
            state_dir.display()
        )
    })?;

    let mut bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token = hex::encode(bytes);

    let hash = Sha256::digest(token.as_bytes());
    let path = token_path(state_dir);
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&path)
        .map_err(|error| format!("unable to open token hash at {}: {error}", path.display()))?;
    file.write_all(hex::encode(hash).as_bytes())
        .map_err(|error| {
            format!(
                "unable to persist token hash to {}: {error}",
                path.display()
            )
        })?;
    #[cfg(unix)]
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        format!(
            "unable to lock down token hash at {}: {error}",
            path.display()
        )
    })?;

    Ok(token)
}

#[derive(Clone)]
pub struct TokenStore {
    expected_hash_hex: String,
}

impl TokenStore {
    pub fn load(state_dir: &Path) -> Result<Self, String> {
        let path = token_path(state_dir);
        let expected_hash_hex = fs::read_to_string(&path)
            .map_err(|error| {
                format!(
                    "unable to read token hash from {} (run `recupere-agent init` first): {error}",
                    path.display()
                )
            })?
            .trim()
            .to_string();
        if expected_hash_hex.len() != 64 {
            return Err(format!(
                "token hash at {} is malformed (expected 64 hex chars)",
                path.display()
            ));
        }
        Ok(Self { expected_hash_hex })
    }

    /// Constant-time comparison of the SHA-256 of `presented` against the
    /// stored hash.
    pub fn verify(&self, presented: &str) -> bool {
        let actual = hex::encode(Sha256::digest(presented.as_bytes()));
        constant_time_eq(actual.as_bytes(), self.expected_hash_hex.as_bytes())
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
