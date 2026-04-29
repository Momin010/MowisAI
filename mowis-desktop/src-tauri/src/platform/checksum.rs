// platform/checksum.rs — SHA-256 file integrity verification
//
// Used before mounting any VM image to guard against corruption or
// supply-chain tampering of the downloaded Alpine Linux disk image.

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// Verify that `path` has the expected SHA-256 `hex_hash`.
/// Returns Ok(()) on success, Err on mismatch or I/O failure.
pub fn verify(path: &Path, expected_hex: &str) -> Result<()> {
    let actual = compute(path)?;
    if actual.eq_ignore_ascii_case(expected_hex) {
        Ok(())
    } else {
        bail!(
            "Checksum mismatch for {}\n  expected: {}\n  actual:   {}",
            path.display(),
            expected_hex,
            actual
        )
    }
}

/// Compute the SHA-256 of a file and return it as a lowercase hex string.
pub fn compute(path: &Path) -> Result<String> {
    let file = File::open(path)
        .with_context(|| format!("opening {} for checksum", path.display()))?;
    let mut reader = BufReader::with_capacity(1 << 20, file); // 1 MiB buffer
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1 << 16]; // 64 KiB chunk
    loop {
        let n = reader.read(&mut buf).context("reading file for checksum")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}
