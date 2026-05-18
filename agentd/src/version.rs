//! Version information for MowisAI
//!
//! IMPORTANT: Bump BUILD_NUMBER before every push to main.
//! This is how we verify which binary is running inside the VM.

/// Current version from Cargo.toml
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Build number — MUST be bumped before every push.
/// Format: YYYYMMDD.N where N is the Nth push of the day.
/// This is the single source of truth for "which code is running".
pub const BUILD_NUMBER: &str = "20260518.1";

/// Get full version string with build info
pub fn get_version() -> String {
    format!(
        "MowisAI v{} build {} ({})",
        VERSION,
        BUILD_NUMBER,
        std::env::consts::ARCH
    )
}

/// Check if this is a debug or release build
pub fn is_debug() -> bool {
    cfg!(debug_assertions)
}

/// Get build type string
pub fn build_type() -> &'static str {
    if is_debug() {
        "debug"
    } else {
        "release"
    }
}

/// Get full version info including build type
pub fn full_version() -> String {
    format!(
        "MowisAI v{} build {} ({}-{})",
        VERSION,
        BUILD_NUMBER,
        std::env::consts::ARCH,
        build_type()
    )
}
