// Global install:
//   cargo build --release
//   sudo cp target/release/mowisai /usr/local/bin/
//   mowisai

use anyhow::Result;
use clap::{Parser, Subcommand};
use libagent::config::MowisConfig;
use libagent::setup::SetupWizard;
use std::fs;
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::path::Path;

#[derive(Parser)]
#[command(name = "agentd")]
#[command(about = "MowisAI — AI agent orchestration engine")]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the socket server
    Socket {
        /// Path to the unix socket
        #[arg(long)]
        path: String,
    },
    /// Run full orchestration simulation with mock agents (no LLM calls)
    Simulate(libagent::orchestration::simulate::SimulateCommand),
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Some(Commands::Socket { path }) => {
            // Run socket server
            libagent::socket_server::run(&path)?;
        }
        Some(Commands::Simulate(cmd)) => {
            let log_path = MowisConfig::config_dir().join("mowisai.log");
            let _ = libagent::logging::init(&log_path);
            tokio::runtime::Runtime::new()?.block_on(cmd.run())?;
        }
        None => {
            // Run TUI
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
        }
    }

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
