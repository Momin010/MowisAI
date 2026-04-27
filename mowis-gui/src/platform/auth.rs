use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::path::PathBuf;

/// Generate a 256-bit cryptographically random token, base64-encoded.
pub fn generate_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    STANDARD.encode(bytes)
}

/// Write the token to a temp file with restrictive permissions (0600 on Unix).
/// Returns the path to the file.
pub fn write_token_file(token: &str) -> Result<PathBuf> {
    let path = std::env::temp_dir().join("mowisai-auth.token");

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(token.as_bytes())?;
    }

    #[cfg(not(unix))]
    {
        std::fs::write(&path, token)?;
    }

    Ok(path)
}

/// Constant-time token comparison to prevent timing attacks.
pub fn validate_token(provided: &str, expected: &str) -> bool {
    if provided.len() != expected.len() {
        return false;
    }
    provided
        .as_bytes()
        .iter()
        .zip(expected.as_bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}
