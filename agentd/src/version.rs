//! Version information for MowisAI

/// Current version of MowisAI
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Get full version string with build info
pub fn get_version() -> String {
    format!(
        "MowisAI v{} ({})",
        VERSION,
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
        "MowisAI v{} ({}-{})",
        VERSION,
        std::env::consts::ARCH,
        build_type()
    )
}
