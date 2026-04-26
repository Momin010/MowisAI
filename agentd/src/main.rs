// Global install:
//   cargo build --release
//   sudo cp target/release/mowisai /usr/local/bin/
//   mowisai

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::thread;
use libagent::config::MowisConfig;
use libagent::setup::SetupWizard;

#[derive(Parser)]
#[command(name = "agentd")]
#[command(about = "MowisAI — AI agent orchestration engine")]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(clap::Args, Debug)]
pub struct OrchCommand {
    /// The prompt / task description
    #[arg(long)]
    pub prompt: String,

    /// GCP project ID for Vertex AI
    #[arg(long)]
    pub project: String,

    /// Path to agentd Unix socket
    #[arg(long, default_value = "/tmp/agentd.sock")]
    pub socket: String,

    /// Project root directory (where code lives)
    #[arg(long, default_value = ".")]
    pub project_root: String,

    /// Orchestration mode: simple, standard, full, auto
    #[arg(long, default_value = "auto")]
    pub mode: String,

    /// Maximum concurrent agents
    #[arg(long, default_value = "50")]
    pub max_agents: usize,

    /// Enable verbose/development logging (shows every tool call, diff, socket payload)
    #[arg(long, short = 'v', default_value = "false")]
    pub verbose: bool,

    /// Directory to save output files (optional)
    #[arg(long)]
    pub save: Option<String>,
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
    /// Run real Gemini orchestration against a live agentd socket
    Orchestrate(OrchCommand),
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
            // Initialize env_logger so log::info! macros work
            env_logger::Builder::from_env(
                env_logger::Env::default().default_filter_or("info"),
            )
            .format_timestamp_secs()
            .init();
            tokio::runtime::Runtime::new()?.block_on(cmd.run())?;
        }
        Some(Commands::Orchestrate(cmd)) => {
            // Enable verbose logging if requested
            if cmd.verbose {
                libagent::orchestration::agent_execution::set_verbose(true);
                unsafe { std::env::set_var("RUST_LOG", "debug"); }
            } else {
                unsafe { std::env::set_var("RUST_LOG", "info"); }
            }

            // Init logging to stderr so user sees everything
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(
                if cmd.verbose { "debug" } else { "info" }
            )).init();

            let mode_override = match cmd.mode.as_str() {
                "simple" => Some(libagent::orchestration::ComplexityMode::Simple),
                "standard" => Some(libagent::orchestration::ComplexityMode::Standard),
                "full" => Some(libagent::orchestration::ComplexityMode::Full),
                _ => None, // auto
            };

            let project_root = std::path::PathBuf::from(&cmd.project_root);
            // Auto-create project root if it doesn't exist
            std::fs::create_dir_all(&project_root)?;
            let overlay_root = std::env::temp_dir().join("mowisai-overlays");
            let checkpoint_root = std::env::temp_dir().join("mowisai-checkpoints");
            let merge_work_dir = std::env::temp_dir().join("mowisai-merge");

            // Build LlmConfig: prefer saved MowisConfig; fall back to Vertex AI with --project flag
            let llm_config = MowisConfig::load()
                .ok()
                .and_then(|cfg| libagent::orchestration::provider_client::LlmConfig::from_config(&cfg).ok())
                .unwrap_or_else(|| libagent::orchestration::provider_client::LlmConfig::vertex(&cmd.project));

            let config = libagent::orchestration::new_orchestrator::OrchestratorConfig {
                llm_config,
                socket_path: cmd.socket.clone(),
                project_root: project_root.clone(),
                overlay_root,
                checkpoint_root,
                merge_work_dir,
                max_agents: cmd.max_agents,
                max_verification_rounds: 3,
                staging_dir: cmd.save.as_ref().map(std::path::PathBuf::from),
                event_tx: None,
                mode_override,
            };

            let orchestrator = libagent::orchestration::new_orchestrator::NewOrchestrator::new(config);

            let rt = tokio::runtime::Runtime::new()?;
            match rt.block_on(orchestrator.run(&cmd.prompt)) {
                Ok(output) => {
                    println!("\nOrchestration complete!");
                    println!("Summary: {}", output.summary);
                    println!("Tasks: {} total, {} completed, {} failed",
                        output.scheduler_stats.total_tasks,
                        output.scheduler_stats.completed,
                        output.scheduler_stats.failed,
                    );
                    if output.merged_diff.is_empty() {
                        println!("\nNo diff captured.");
                    } else {
                        println!("\nDiff ({} bytes):", output.merged_diff.len());
                        for line in output.merged_diff.lines().take(50) {
                            println!("{}", line);
                        }
                        if output.merged_diff.lines().count() > 50 {
                            println!("... ({} more lines)", output.merged_diff.lines().count() - 50);
                        }
                        // Save if requested
                        if let Some(ref save_dir) = cmd.save {
                            let save_path = std::path::PathBuf::from(save_dir);
                            std::fs::create_dir_all(&save_path)?;
                            // Write diff
                            std::fs::write(save_path.join("output.patch"), &output.merged_diff)?;
                            println!("\nSaved to {}", save_path.display());
                        }
                    }
                }
                Err(e) => {
                    eprintln!("\nOrchestration failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        None => {
            // Verify skopeo is installed (required for container image pulls)
            if !std::process::Command::new("skopeo")
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                eprintln!("Error: skopeo is not installed. MowisAI uses skopeo to pull container images.\n");
                eprintln!("Install it with:");
                eprintln!("  Alpine/Codespaces:  sudo apk add skopeo");
                eprintln!("  Ubuntu/Debian:      sudo apt-get install -y skopeo");
                eprintln!("  Fedora/RHEL:        sudo dnf install -y skopeo");
                eprintln!("  macOS (Homebrew):   brew install skopeo");
                eprintln!("\nThen re-run MowisAI.");
                std::process::exit(1);
            }

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
