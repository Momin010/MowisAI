use anyhow::{Context, Result};
use rand::rngs::OsRng;
use rand::RngCore;
use std::path::PathBuf;

/// Generate a cryptographically secure auth token
pub fn generate_auth_token() -> String {
    let mut token_bytes = [0u8; 32]; // 256 bits
    OsRng.fill_bytes(&mut token_bytes);
    hex::encode(token_bytes)
}

/// Get the path to the auth token file
pub fn auth_token_path() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    
    let mowisai_dir = home.join(".mowisai");
    std::fs::create_dir_all(&mowisai_dir)
        .context("Failed to create ~/.mowisai directory")?;
    
    Ok(mowisai_dir.join("auth-token"))
}

/// Write auth token to file with secure permissions
pub fn write_auth_token(token: &str) -> Result<()> {
    let path = auth_token_path()?;
    
    std::fs::write(&path, token)
        .context("Failed to write auth token file")?;
    
    // Set permissions to 0600 (owner read/write only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&path, perms)
            .context("Failed to set auth token file permissions")?;
    }
    
    // On Windows, use ACLs to restrict access to current user
    #[cfg(windows)]
    {
        // TODO: Implement Windows ACL restriction
        log::warn!("Windows ACL restriction not yet implemented for auth token");
    }
    
    log::info!("Auth token written to {:?}", path);
    Ok(())
}

/// Read auth token from file
pub fn read_auth_token() -> Result<String> {
    let path = auth_token_path()?;
    
    let token = std::fs::read_to_string(&path)
        .context("Failed to read auth token file")?;
    
    Ok(token.trim().to_string())
}

/// Validate an auth token
pub fn validate_token(provided: &str, expected: &str) -> bool {
    // Use constant-time comparison to prevent timing attacks
    if provided.len() != expected.len() {
        return false;
    }
    
    provided.as_bytes()
        .iter()
        .zip(expected.as_bytes().iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_token() {
        let token1 = generate_auth_token();
        let token2 = generate_auth_token();
        
        // Tokens should be 64 hex characters (32 bytes)
        assert_eq!(token1.len(), 64);
        assert_eq!(token2.len(), 64);
        
        // Tokens should be different
        assert_ne!(token1, token2);
        
        // Tokens should be valid hex
        assert!(hex::decode(&token1).is_ok());
        assert!(hex::decode(&token2).is_ok());
    }

    #[test]
    fn test_validate_token() {
        let token = "abcd1234";
        
        assert!(validate_token(token, token));
        assert!(!validate_token(token, "different"));
        assert!(!validate_token(token, "abcd123")); // Different length
    }
}
