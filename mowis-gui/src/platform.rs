/// Platform detection module for cross-platform support
///
/// This module provides platform detection and capability checking for
/// MowisAI's cross-platform architecture. It determines the current OS
/// and checks for platform-specific virtualization support.

use std::process::Command;

/// Supported platforms for MowisAI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Linux - runs agentd directly
    Linux,
    /// macOS - uses Virtualization.framework or QEMU fallback
    MacOS,
    /// Windows - uses WSL2 or QEMU fallback
    Windows,
}

impl Platform {
    /// Detect the current platform based on the OS
    ///
    /// # Returns
    /// The current platform enum variant
    ///
    /// # Panics
    /// Panics if running on an unsupported platform
    pub fn current() -> Self {
        match std::env::consts::OS {
            "linux" => Platform::Linux,
            "macos" => Platform::MacOS,
            "windows" => Platform::Windows,
            os => panic!("Unsupported platform: {}", os),
        }
    }

    /// Check if the platform supports Apple's Virtualization.framework
    ///
    /// This checks if the current macOS version is 10.15 (Catalina) or later,
    /// which is required for Virtualization.framework support.
    ///
    /// # Returns
    /// `true` if Virtualization.framework is available, `false` otherwise
    ///
    /// # Platform Support
    /// - macOS: Checks version >= 10.15
    /// - Other platforms: Always returns `false`
    pub fn supports_virtualization_framework(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            // Check macOS version >= 10.15 (Catalina)
            check_macos_version() >= (10, 15)
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    /// Check if the platform supports WSL2
    ///
    /// This executes `wsl --status` to determine if WSL2 is available
    /// on the current Windows system.
    ///
    /// # Returns
    /// `true` if WSL2 is available and functional, `false` otherwise
    ///
    /// # Platform Support
    /// - Windows: Executes `wsl --status` and checks exit code
    /// - Other platforms: Always returns `false`
    pub fn supports_wsl2(&self) -> bool {
        #[cfg(target_os = "windows")]
        {
            // Execute `wsl --status` and check if it succeeds
            Command::new("wsl")
                .arg("--status")
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        }
        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }
}

/// Check the macOS version
///
/// Parses the output of `sw_vers -productVersion` to determine the macOS version.
///
/// # Returns
/// A tuple of (major, minor) version numbers
///
/// # Platform Support
/// This function is only compiled on macOS targets.
#[cfg(target_os = "macos")]
fn check_macos_version() -> (u32, u32) {
    // Execute `sw_vers -productVersion` to get macOS version
    let output = Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .expect("Failed to execute sw_vers");

    if !output.status.success() {
        // Default to a version that doesn't support Virtualization.framework
        return (10, 14);
    }

    let version_string = String::from_utf8_lossy(&output.stdout);
    let version_string = version_string.trim();

    // Parse version string (e.g., "10.15.7" or "11.6.1" or "14.2")
    let parts: Vec<&str> = version_string.split('.').collect();

    if parts.is_empty() {
        return (10, 14);
    }

    let major = parts[0].parse::<u32>().unwrap_or(10);
    let minor = if parts.len() > 1 {
        parts[1].parse::<u32>().unwrap_or(14)
    } else {
        0
    };

    (major, minor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_current() {
        let platform = Platform::current();

        // Verify we get a valid platform
        match std::env::consts::OS {
            "linux" => assert_eq!(platform, Platform::Linux),
            "macos" => assert_eq!(platform, Platform::MacOS),
            "windows" => assert_eq!(platform, Platform::Windows),
            _ => panic!("Unexpected OS"),
        }
    }

    #[test]
    fn test_supports_virtualization_framework() {
        let platform = Platform::current();
        let supports = platform.supports_virtualization_framework();

        // On macOS, this should return a boolean based on version
        // On other platforms, it should always return false
        #[cfg(target_os = "macos")]
        {
            // We can't assert a specific value since it depends on the macOS version
            // Just verify it returns a boolean without panicking
            let _ = supports;
        }

        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(supports, false);
        }
    }

    #[test]
    fn test_supports_wsl2() {
        let platform = Platform::current();
        let supports = platform.supports_wsl2();

        // On Windows, this should return a boolean based on WSL2 availability
        // On other platforms, it should always return false
        #[cfg(target_os = "windows")]
        {
            // We can't assert a specific value since it depends on WSL2 installation
            // Just verify it returns a boolean without panicking
            let _ = supports;
        }

        #[cfg(not(target_os = "windows"))]
        {
            assert_eq!(supports, false);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_check_macos_version() {
        let (major, minor) = check_macos_version();

        // Verify we get reasonable version numbers
        // macOS 10.x or 11+ (Big Sur changed versioning)
        assert!(major >= 10);
        assert!(minor < 100); // Sanity check
    }
}
