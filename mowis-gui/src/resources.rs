use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Embedded checksums for Alpine images
/// Generated during build process
pub struct ImageChecksums {
    pub alpine_x86_64_qcow2: &'static str,
    pub alpine_aarch64_qcow2: &'static str,
    pub alpine_x86_64_wsl2: &'static str,
    pub alpine_aarch64_wsl2: &'static str,
}

impl ImageChecksums {
    /// Get the embedded checksums
    pub fn embedded() -> Self {
        Self {
            // These will be populated during the build process
            // For now, use placeholder values
            alpine_x86_64_qcow2: option_env!("ALPINE_X86_64_QCOW2_SHA256").unwrap_or(""),
            alpine_aarch64_qcow2: option_env!("ALPINE_AARCH64_QCOW2_SHA256").unwrap_or(""),
            alpine_x86_64_wsl2: option_env!("ALPINE_X86_64_WSL2_SHA256").unwrap_or(""),
            alpine_aarch64_wsl2: option_env!("ALPINE_AARCH64_WSL2_SHA256").unwrap_or(""),
        }
    }
}

/// Verify the integrity of an image file
pub fn verify_image_integrity(path: &Path, expected_checksum: &str) -> Result<bool> {
    if expected_checksum.is_empty() {
        log::warn!("No checksum available for {:?}, skipping verification", path);
        return Ok(true);
    }

    let contents = std::fs::read(path)
        .context(format!("Failed to read image file: {:?}", path))?;

    let mut hasher = Sha256::new();
    hasher.update(&contents);
    let result = hasher.finalize();
    let actual_checksum = format!("{:x}", result);

    if actual_checksum == expected_checksum {
        log::info!("Image integrity verified: {:?}", path);
        Ok(true)
    } else {
        log::error!(
            "Image integrity check failed for {:?}\n  Expected: {}\n  Actual: {}",
            path,
            expected_checksum,
            actual_checksum
        );
        Ok(false)
    }
}

/// Verify bundled image based on platform and architecture
pub fn verify_bundled_image(
    image_type: ImageType,
    path: &Path,
) -> Result<bool> {
    let checksums = ImageChecksums::embedded();

    let expected = match image_type {
        ImageType::AlpineX86_64Qcow2 => checksums.alpine_x86_64_qcow2,
        ImageType::AlpineAarch64Qcow2 => checksums.alpine_aarch64_qcow2,
        ImageType::AlpineX86_64Wsl2 => checksums.alpine_x86_64_wsl2,
        ImageType::AlpineAarch64Wsl2 => checksums.alpine_aarch64_wsl2,
    };

    verify_image_integrity(path, expected)
}

/// Image types
#[derive(Debug, Clone, Copy)]
pub enum ImageType {
    AlpineX86_64Qcow2,
    AlpineAarch64Qcow2,
    AlpineX86_64Wsl2,
    AlpineAarch64Wsl2,
}

impl ImageType {
    /// Get the image type for the current platform and architecture
    pub fn for_current_platform() -> Self {
        let arch = std::env::consts::ARCH;

        match (crate::platform::Platform::current(), arch) {
            (crate::platform::Platform::Linux, "x86_64") => Self::AlpineX86_64Qcow2,
            (crate::platform::Platform::Linux, "aarch64") => Self::AlpineAarch64Qcow2,
            (crate::platform::Platform::MacOS, "x86_64") => Self::AlpineX86_64Qcow2,
            (crate::platform::Platform::MacOS, "aarch64") => Self::AlpineAarch64Qcow2,
            (crate::platform::Platform::Windows, "x86_64") => Self::AlpineX86_64Wsl2,
            (crate::platform::Platform::Windows, "aarch64") => Self::AlpineAarch64Wsl2,
            _ => {
                log::warn!("Unknown platform/arch combination, defaulting to x86_64");
                Self::AlpineX86_64Qcow2
            }
        }
    }

    /// Get the filename for this image type
    pub fn filename(&self) -> &'static str {
        match self {
            Self::AlpineX86_64Qcow2 => "alpine-x86_64.qcow2",
            Self::AlpineAarch64Qcow2 => "alpine-aarch64.qcow2",
            Self::AlpineX86_64Wsl2 => "alpine-wsl2-x86_64.tar.gz",
            Self::AlpineAarch64Wsl2 => "alpine-wsl2-aarch64.tar.gz",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_type_filename() {
        assert_eq!(
            ImageType::AlpineX86_64Qcow2.filename(),
            "alpine-x86_64.qcow2"
        );
        assert_eq!(
            ImageType::AlpineX86_64Wsl2.filename(),
            "alpine-wsl2-x86_64.tar.gz"
        );
    }
}
