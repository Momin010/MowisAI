use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use sha2::{Digest, Sha256};

fn machine_id_path() -> std::path::PathBuf {
    crate::config::OrchConfig::config_dir().join("machine-id")
}

fn machine_id() -> Result<String> {
    let path = machine_id_path();
    if path.exists() {
        let id = std::fs::read_to_string(&path).context("reading ~/.mowisai/machine-id")?;
        let id = id.trim().to_string();
        if id.is_empty() {
            bail!("machine-id file is empty — delete ~/.mowisai/machine-id and re-run setup");
        }
        return Ok(id);
    }
    let mut raw = [0u8; 32];
    use aes_gcm::aead::rand_core::RngCore;
    OsRng.fill_bytes(&mut raw);
    let id = BASE64.encode(raw);
    let dir = crate::config::OrchConfig::config_dir();
    std::fs::create_dir_all(&dir).context("creating ~/.mowisai/")?;
    std::fs::write(&path, &id).context("writing ~/.mowisai/machine-id")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400));
    }
    Ok(id)
}

fn machine_key() -> Result<Key<Aes256Gcm>> {
    let id = machine_id()?;
    let mut hasher = Sha256::new();
    hasher.update(id.as_bytes());
    hasher.update(b"|mowisai-orch-key-v1");
    let bytes: [u8; 32] = hasher.finalize().into();
    Ok(*Key::<Aes256Gcm>::from_slice(&bytes))
}

pub fn encrypt(plaintext: &str) -> Result<String> {
    let key = machine_key().context("deriving machine encryption key")?;
    let cipher = Aes256Gcm::new(&key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("AES-GCM encrypt error: {}", e))
        .context("encrypting value")?;
    Ok(format!(
        "{}:{}",
        BASE64.encode(nonce.as_slice()),
        BASE64.encode(&ciphertext)
    ))
}

pub fn decrypt(encoded: &str) -> Result<String> {
    let (nonce_b64, ct_b64) = encoded
        .split_once(':')
        .context("invalid encrypted format (expected nonce:ciphertext)")?;
    let nonce_bytes = BASE64.decode(nonce_b64).context("base64 decode nonce")?;
    let ciphertext = BASE64.decode(ct_b64).context("base64 decode ciphertext")?;
    if nonce_bytes.len() != 12 {
        bail!(
            "invalid nonce length: expected 12 bytes, got {}",
            nonce_bytes.len()
        );
    }
    let key = machine_key().context("deriving machine encryption key")?;
    let cipher = Aes256Gcm::new(&key);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher.decrypt(nonce, ciphertext.as_ref()).map_err(|_| {
        anyhow::anyhow!(
            "decryption failed — the config was saved on a different machine, \
             or ~/.mowisai/machine-id was deleted"
        )
    })?;
    String::from_utf8(plaintext).context("decrypted bytes are not valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let plaintext = "my-secret-api-key-12345";
        let encrypted = encrypt(plaintext).expect("encryption should succeed");
        let decrypted = decrypt(&encrypted).expect("decryption should succeed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_different_nonces() {
        let plaintext = "same-plaintext";
        let enc1 = encrypt(plaintext).unwrap();
        let enc2 = encrypt(plaintext).unwrap();
        assert_ne!(enc1, enc2);
        assert_eq!(decrypt(&enc1).unwrap(), plaintext);
        assert_eq!(decrypt(&enc2).unwrap(), plaintext);
    }

    #[test]
    fn test_decrypt_rejects_tampered_ciphertext() {
        let plaintext = "sensitive-data";
        let encrypted = encrypt(plaintext).unwrap();
        let mut tampered = encrypted.clone();
        let last_char = tampered.pop().unwrap();
        tampered.push(if last_char == 'A' { 'B' } else { 'A' });
        assert!(decrypt(&tampered).is_err());
    }

    #[test]
    fn test_encrypt_empty_string() {
        let encrypted = encrypt("").expect("should encrypt empty string");
        let decrypted = decrypt(&encrypted).expect("should decrypt empty string");
        assert_eq!(decrypted, "");
    }

    #[test]
    fn test_encrypt_unicode() {
        let plaintext = "Hello 世界";
        let encrypted = encrypt(plaintext).unwrap();
        let decrypted = decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }
}
