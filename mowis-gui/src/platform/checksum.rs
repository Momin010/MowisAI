use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// Verify that `path` has the expected SHA-256 digest.
/// `expected_sha256` is a lowercase hex string.
pub fn verify_file(path: &Path, expected_sha256: &str) -> Result<()> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| anyhow!("Cannot open {:?} for checksum verification: {e}", path))?;

    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];

    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| anyhow!("Read error while hashing {:?}: {e}", path))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    let actual = hex::encode(hasher.finalize());
    let expected = expected_sha256.to_lowercase();

    if actual != expected {
        return Err(anyhow!(
            "SHA-256 mismatch for {:?}:\n  expected: {expected}\n  got:      {actual}",
            path
        ));
    }

    Ok(())
}
