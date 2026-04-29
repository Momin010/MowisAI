// platform/auth.rs — 256-bit auth token (Jupyter-style)
//
// The token is generated once per install, stored in ~/.mowisai/token (0600),
// and injected into every VM launch (via -fw_cfg or env var in WSL2).
// The desktop sends it as the first line on every new connection so the daemon
// can reject unauthenticated clients.

use anyhow::{Context, Result};
use rand::RngCore;
use std::fs;
use std::path::PathBuf;

/// Return the path where the token file lives.
pub fn token_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mowisai")
        .join("token")
}

/// Load an existing token, or generate + persist a fresh one.
pub fn load_or_create() -> Result<String> {
    let path = token_path();
    if path.exists() {
        let token = fs::read_to_string(&path)
            .context("reading token file")?
            .trim()
            .to_owned();
        if token.len() == 64 {
            return Ok(token);
        }
        // Corrupt / short token — regenerate
    }
    let token = generate();
    persist(&token)?;
    Ok(token)
}

/// Generate a new 256-bit (32-byte → 64 hex chars) random token.
pub fn generate() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Write token to disk with 0600 permissions.
pub fn persist(token: &str) -> Result<()> {
    let path = token_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("creating ~/.mowisai dir")?;
    }
    fs::write(&path, token).context("writing token file")?;

    // Restrict permissions on Unix so other users can't read it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(&path, perms).context("setting token file permissions")?;
    }

    Ok(())
}

/// Constant-time comparison to validate a received token.
pub fn validate(received: &str, expected: &str) -> bool {
    if received.len() != expected.len() {
        return false;
    }
    // XOR all bytes; if any differ the result is non-zero.
    let diff: u8 = received
        .bytes()
        .zip(expected.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b));
    diff == 0
}
