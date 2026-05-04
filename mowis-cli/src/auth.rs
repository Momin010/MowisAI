// auth.rs — 256-bit auth token (Jupyter-style)
//
// Generated once per install, stored in ~/.mowisai/token.
// Injected into every VM launch.

use anyhow::{Context, Result};
use rand::RngCore;
use std::fs;
use std::path::PathBuf;

pub fn token_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mowisai")
        .join("token")
}

pub fn load_or_create() -> Result<String> {
    let path = token_path();
    log::debug!("[auth] Token path: {}", path.display());

    if path.exists() {
        let token = fs::read_to_string(&path)
            .context("reading token file")?
            .trim()
            .to_owned();
        if token.len() == 64 {
            log::info!("[auth] Loaded existing token ({}…)", &token[..8]);
            return Ok(token);
        }
        log::warn!(
            "[auth] Token file exists but is corrupt (len={}), regenerating",
            token.len()
        );
    }

    let token = generate();
    persist(&token)?;
    log::info!("[auth] Generated new token ({}…)", &token[..8]);
    Ok(token)
}

pub fn generate() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn persist(token: &str) -> Result<()> {
    let path = token_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("creating ~/.mowisai dir")?;
    }
    fs::write(&path, token).context("writing token file")?;
    log::debug!("[auth] Token written to {}", path.display());
    Ok(())
}

pub fn validate(received: &str, expected: &str) -> bool {
    if received.len() != expected.len() {
        return false;
    }
    let diff: u8 = received
        .bytes()
        .zip(expected.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b));
    diff == 0
}
