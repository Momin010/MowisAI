use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use sha2::{Digest, Sha256};

/// Path to the machine-id file (created once, never changes on this host).
fn machine_id_path() -> std::path::PathBuf {
    crate::config::MowisConfig::config_dir().join("machine-id")
}

/// Read the machine-id, generating and persisting a random one on first call.
///
/// Fails with an error rather than falling back to a predictable constant —
/// the caller must be able to encrypt/decrypt reliably or not at all.
fn machine_id() -> Result<String> {
    let path = machine_id_path();

    if path.exists() {
        let id = std::fs::read_to_string(&path)
            .context("reading ~/.mowisai/machine-id")?;
        let id = id.trim().to_string();
        if id.is_empty() {
            bail!("machine-id file is empty — delete ~/.mowisai/machine-id and re-run setup");
        }
        return Ok(id);
    }

    // First run: generate a random 32-byte ID and persist it.
    let mut raw = [0u8; 32];
    use aes_gcm::aead::rand_core::RngCore;
    OsRng.fill_bytes(&mut raw);
    let id = BASE64.encode(raw);

    let dir = crate::config::MowisConfig::config_dir();
    std::fs::create_dir_all(&dir).context("creating ~/.mowisai/")?;
    std::fs::write(&path, &id).context("writing ~/.mowisai/machine-id")?;

    // Restrict to owner-read only — it's the root of key derivation.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400));
    }

    Ok(id)
}

/// Derive a 32-byte AES-256 key from the persistent machine-id.
fn machine_key() -> Result<Key<Aes256Gcm>> {
    let id = machine_id()?;
    let mut hasher = Sha256::new();
    hasher.update(id.as_bytes());
    hasher.update(b"|mowisai-provider-key-v1");
    let bytes: [u8; 32] = hasher.finalize().into();
    Ok(*Key::<Aes256Gcm>::from_slice(&bytes))
}

/// Encrypt `plaintext` with AES-256-GCM.  Returns `"<nonce_b64>:<ciphertext_b64>"`.
pub fn encrypt(plaintext: &str) -> Result<String> {
    let key = machine_key().context("deriving machine encryption key")?;
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

    let key = machine_key().context("deriving machine encryption key")?;
    let cipher = Aes256Gcm::new(&key);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| anyhow::anyhow!(
            "decryption failed — the config was saved on a different machine, \
             or ~/.mowisai/machine-id was deleted"
        ))?;

    String::from_utf8(plaintext).context("decrypted bytes are not valid UTF-8")
}
