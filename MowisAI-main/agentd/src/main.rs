// Global install:
//   cargo build --release
//   sudo cp target/release/mowisai /usr/local/bin/
//   mowisai

use anyhow::Result;
use libagent::config::MowisConfig;
use libagent::setup::SetupWizard;
use std::path::Path;
use std::thread;
use std::time::Duration;

fn main() -> Result<()> {
    let log_path = MowisConfig::config_dir().join("mowisai.log");
    let _ = libagent::logging::init(&log_path);

    let config = if SetupWizard::needs_setup() {
        SetupWizard::run()?
    } else {
        MowisConfig::load()?.unwrap_or_default()
    };

    let socket_path = config.socket_path.clone();

    // Ensure socket server is running (with auto-start)
    ensure_socket_server(&socket_path);

    libagent::tui::run_interactive(config)?;

    Ok(())
}

/// Ensure socket server is running — auto-start with sudo if needed
fn ensure_socket_server(socket_path: &str) {
    use std::fs;
    use std::process::{Command, Stdio};

    // Check if socket already exists and is responding
    if socket_is_responsive(socket_path) {
        log::info!("Socket server already running at {}", socket_path);
        return;
    }

    // Try to clean up stale socket
    let _ = fs::remove_file(socket_path);

    log::info!("Attempting to start socket server at {} with sudo...", socket_path);

    // Try to start socket server as background process with sudo
    let result = Command::new("sudo")
        .args([
            "-n", // non-interactive (use cached credentials)
            "target/debug/agentd",
            "socket",
            "--path",
            socket_path,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    match result {
        Ok(mut child) => {
            // Detach the process so it keeps running
            let _ = child.id(); // Just spawn it

            // Give it a moment to start
            thread::sleep(Duration::from_millis(500));

            // Verify it started
            if socket_is_responsive(socket_path) {
                log::info!("✓ Socket server started successfully");
                return;
            } else {
                log::warn!("Socket server started but not responding yet, retrying...");
                thread::sleep(Duration::from_millis(1000));

                if socket_is_responsive(socket_path) {
                    log::info!("✓ Socket server up after retry");
                    return;
                }
            }
        }
        Err(e) => {
            log::warn!("Failed to start socket server with sudo -n: {}", e);
            log::warn!("Trying interactive sudo prompt...");

            // Fall back to interactive sudo (will prompt user)
            let result = Command::new("sudo")
                .args([
                    "target/debug/agentd",
                    "socket",
                    "--path",
                    socket_path,
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();

            match result {
                Ok(mut child) => {
                    let _ = child.id();
                    thread::sleep(Duration::from_millis(500));

                    if socket_is_responsive(socket_path) {
                        log::info!("✓ Socket server started successfully");
                        return;
                    }
                }
                Err(e) => {
                    log::warn!("Failed to start socket server: {}", e);
                }
            }
        }
    }

    // If we get here, socket server didn't start
    log::warn!("⚠️  Socket server is not available");
    log::warn!("Chat mode will work, but orchestration requires the socket server.");
    log::warn!("You can start it manually with:");
    log::warn!("  sudo target/debug/agentd socket --path {}", socket_path);
}

/// Check if socket server is responsive
fn socket_is_responsive(socket_path: &str) -> bool {
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    if !Path::new(socket_path).exists() {
        return false;
    }

    match UnixStream::connect(socket_path) {
        Ok(stream) => {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
            true
        }
        Err(_) => false,
    }
}
