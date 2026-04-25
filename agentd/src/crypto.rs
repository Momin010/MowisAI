use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{Context, Result, bail};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use sha2::{Digest, Sha256};

/// Derive a 32-byte AES-256 key from stable machine identifiers.
/// The key is derived from hostname + OS username + a fixed salt so it stays
/// consistent across process restarts on the same machine but is different on
/// every other machine — no key ever leaves the host.
fn machine_key() -> Key<Aes256Gcm> {
    let hostname = hostname();
    let username = username();

    let mut hasher = Sha256::new();
    hasher.update(hostname.as_bytes());
    hasher.update(b"|");
    hasher.update(username.as_bytes());
    hasher.update(b"|mowisai-provider-key-v1");
    let bytes: [u8; 32] = hasher.finalize().into();
    *Key::<Aes256Gcm>::from_slice(&bytes)
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "unknown-host".into())
        })
}

fn username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown-user".into())
}

/// Encrypt `plaintext` with AES-256-GCM.  Returns `"<nonce_b64>:<ciphertext_b64>"`.
pub fn encrypt(plaintext: &str) -> Result<String> {
    let key = machine_key();
    let cipher = Aes256Gcm::new(&key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("AES-GCM encrypt error: {}", e))
        .context("encrypting API key")?;

    Ok(format!(
        "{}:{}",
        BASE64.encode(nonce.as_slice()),
        BASE64.encode(&ciphertext)
    ))
}

/// Decrypt a value produced by [`encrypt`].
pub fn decrypt(encoded: &str) -> Result<String> {
    let (nonce_b64, ct_b64) = encoded
        .split_once(':')
        .context("invalid encrypted format (expected nonce:ciphertext)")?;

    let nonce_bytes = BASE64.decode(nonce_b64).context("base64 decode nonce")?;
    let ciphertext = BASE64.decode(ct_b64).context("base64 decode ciphertext")?;

    if nonce_bytes.len() != 12 {
        bail!("invalid nonce length: expected 12 bytes, got {}", nonce_bytes.len());
    }

    let key = machine_key();
    let cipher = Aes256Gcm::new(&key);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| anyhow::anyhow!(
            "decryption failed — the config was saved on a different machine or is corrupted"
        ))?;

    String::from_utf8(plaintext).context("decrypted bytes are not valid UTF-8")
}
