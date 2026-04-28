/// Example demonstrating the platform detection module
///
/// This example shows how to use the Platform enum to detect the current
/// operating system and check for platform-specific virtualization support.

// Since mowis-gui is a binary crate, we need to include the module directly
#[path = "../src/platform.rs"]
mod platform;

use platform::Platform;

fn main() {
    println!("=== MowisAI Platform Detection Demo ===\n");

    // Detect current platform
    let current_platform = Platform::current();
    println!("Current platform: {:?}", current_platform);

    // Check for Virtualization.framework support (macOS)
    let virt_framework = current_platform.supports_virtualization_framework();
    println!(
        "Virtualization.framework support: {}",
        if virt_framework { "Yes" } else { "No" }
    );

    // Check for WSL2 support (Windows)
    let wsl2 = current_platform.supports_wsl2();
    println!("WSL2 support: {}", if wsl2 { "Yes" } else { "No" });

    // Provide platform-specific recommendations
    println!("\n=== Recommended VM Launcher ===");
    match current_platform {
        Platform::Linux => {
            println!("Linux detected: Use direct agentd execution (no VM needed)");
        }
        Platform::MacOS => {
            if virt_framework {
                println!("macOS with Virtualization.framework: Use macOS launcher");
            } else {
                println!("macOS without Virtualization.framework: Use QEMU fallback");
            }
        }
        Platform::Windows => {
            if wsl2 {
                println!("Windows with WSL2: Use WSL2 launcher");
            } else {
                println!("Windows without WSL2: Use QEMU fallback");
            }
        }
    }
}
