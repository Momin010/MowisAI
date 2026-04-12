// Global install:
//   cargo build --release
//   sudo cp target/release/mowisai /usr/local/bin/
//   mowisai

use anyhow::Result;
use libagent::config::MowisConfig;
use libagent::setup::SetupWizard;
use std::fs;
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::path::Path;

fn main() -> Result<()> {
    let log_path = MowisConfig::config_dir().join("mowisai.log");
    let _ = libagent::logging::init(&log_path);

    let config = if SetupWizard::needs_setup() {
        SetupWizard::run()?
    } else {
        MowisConfig::load()?.unwrap_or_default()
    };

    let socket_path = config.socket_path.clone();

    // Set up signal handlers for graceful shutdown
    setup_signal_handlers();

    // Ensure socket server is running (with auto-start)
    ensure_socket_server(&socket_path)?;

    // Store socket PID for cleanup on quit
    let socket_pid = libagent::read_socket_pid().ok();

    // Run TUI
    libagent::tui::run_interactive(config, socket_pid)?;

    Ok(())
}

/// Set up signal handlers for SIGINT and SIGTERM
fn setup_signal_handlers() {
    use signal_hook::consts::signal::*;
    use signal_hook::iterator::Signals;
    use std::thread;

    match Signals::new(&[SIGINT, SIGTERM]) {
        Ok(mut signals) => {
            thread::spawn(move || {
                for sig in &mut signals {
                    if sig == SIGINT {
                        log::info!("Received Ctrl+C — shutting down TUI (socket server stays alive)");
                        libagent::set_shutdown();
                    } else if sig == SIGTERM {
                        log::info!("Received SIGTERM — shutting down TUI (socket server stays alive)");
                        libagent::set_shutdown();
                    }
                }
            });
        }
        Err(e) => {
            log::warn!("Failed to set up signal handlers: {}", e);
        }
    }
}

/// Check if shutdown was requested via signal
pub fn is_shutdown_requested() -> bool {
    libagent::is_shutdown_requested()
}

/// Request shutdown programmatically (for /quit command)
pub fn request_quit_with_socket_cleanup(socket_pid: Option<u32>) {
    // Kill socket server if we have its PID
    if let Some(pid) = socket_pid {
        log::info!("Killing socket server (PID: {})", pid);
        let _ = Command::new("kill").arg(pid.to_string()).output();
        // Delete PID file
        if let Ok(config_dir) = std::env::home_dir() {
            let pid_file = config_dir.join(".mowisai").join(".socket-server.pid");
            let _ = fs::remove_file(pid_file);
        }
    }
    SHUTDOWN_FLAG.store(true, Ordering::Release);
}


/// Ensure socket server is running — auto-start if needed
fn ensure_socket_server(socket_path: &str) -> Result<()> {
    // Check if socket already exists and is responding
    if libagent::socket_is_responsive(socket_path) {
        log::info!("Socket server already running at {}", socket_path);
        return Ok(());
    }

    match libagent::start_socket_server_daemon(socket_path) {
        Ok(_pid) => Ok(()),
        Err(e) => {
            // If we get here, socket server didn't start
            log::warn!("⚠️  Socket server is not available: {}", e);
            log::warn!("Chat mode will work, but orchestration requires the socket server.");
            log::warn!("You can start it manually with:");
            log::warn!("  sudo target/debug/agentd socket --path {}", socket_path);
            Ok(()) // Don't fail completely, chat mode still works
        }
    }
}
