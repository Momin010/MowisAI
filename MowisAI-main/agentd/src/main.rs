use clap::{Parser, Subcommand};
use libagent::{socket_server, ResourceLimits, Sandbox};
use std::path::PathBuf;

/// Command-line interface for the agent runtime.
#[derive(Parser)]
#[command(name = "agentd")]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new sandbox and print its id
    CreateSandbox {
        #[arg(long)]
        ram: Option<u64>,
        #[arg(long)]
        cpu: Option<u64>,
    },
    /// Run a prompt using an agent in a sandbox
    Run {
        #[arg(long)]
        sandbox: u64,
        prompt: String,
    },
    /// Register a tool with the sandbox
    RegisterTool {
        #[arg(long)]
        sandbox: u64,
        #[arg(long)]
        name: String,
    },
    /// Invoke a tool with JSON input
    InvokeTool {
        #[arg(long)]
        sandbox: u64,
        #[arg(long)]
        name: String,
        #[arg(long)]
        input: String,
    },
    /// List all active sandboxes
    List,
    /// Get status of an agent
    Status {
        #[arg(long)]
        agent: u64,
    },
    /// Start Unix socket API server
    Socket {
        #[arg(long, default_value = "/tmp/agentd.sock")]
        path: String,
    },
    /// Vertex AI Gemini loop: tools executed via agentd socket
    Agent {
        #[arg(long)]
        prompt: String,
        #[arg(long)]
        project: String,
        #[arg(long, default_value = "/tmp/agentd.sock")]
        socket: String,
    },
    /// Interactive orchestration with TUI dashboard (7-layer orchestration system)
    OrchestrateNew {
        #[arg(long)]
        prompt: String,
        #[arg(long)]
        project: String,
        #[arg(long, default_value = "/tmp/agentd.sock")]
        socket: String,
        #[arg(long, default_value = ".")]
        project_root: PathBuf,
        #[arg(long, default_value = "/tmp/mowis-overlay")]
        overlay_root: PathBuf,
        #[arg(long, default_value = "/tmp/mowis-checkpoints")]
        checkpoint_root: PathBuf,
        #[arg(long, default_value = "/tmp/mowis-merge")]
        merge_work_dir: PathBuf,
        #[arg(long, default_value_t = 1000)]
        max_agents: usize,
        #[arg(long, default_value_t = 3)]
        max_verification_rounds: usize,
        /// Show detailed agent output (tool calls, diffs, etc.)
        #[arg(long, default_value_t = false)]
        verbose: bool,
        /// Create a new project from scratch instead of editing existing code
        #[arg(long, default_value_t = false)]
        new: bool,
        /// Output directory for --new projects (where to save generated code)
        #[arg(long)]
        output_dir: Option<PathBuf>,
        /// Save all agent changes to host filesystem (use with --output-dir)
        #[arg(long, default_value_t = false)]
        save_all: bool,
        /// Run in non-interactive mode (no TUI, just print output)
        #[arg(long, default_value_t = false)]
        no_tui: bool,
        /// Session file path for saving/restoring orchestration state
        #[arg(long)]
        session_file: Option<PathBuf>,
        /// Resume from a previously saved session file
        #[arg(long, default_value_t = false)]
        resume: bool,
    },
    /// Run full orchestration simulation with mock agents (no LLM calls, $0 cost testing)
    Simulate {
        #[arg(long, default_value = "/tmp/agentd.sock")]
        socket: String,
        #[arg(long, default_value_t = 10)]
        tasks: usize,
        #[arg(long, default_value_t = 1)]
        sandboxes: usize,
        #[arg(long, default_value_t = 20)]
        max_agents: usize,
        #[arg(long, default_value_t = 0.0)]
        failure_rate: f64,
        #[arg(long, default_value_t = 100)]
        tool_delay: u64,
        #[arg(long, default_value = "/tmp/mock-project")]
        project_root: PathBuf,
        #[arg(long, short, default_value_t = false)]
        verbose: bool,
    },
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    match cli.cmd {
        Commands::CreateSandbox { ram, cpu } => {
            let limits = ResourceLimits {
                ram_bytes: ram,
                cpu_millis: cpu,
            };
            match Sandbox::new(limits) {
                Ok(sb) => println!("created sandbox {}", sb.id()),
                Err(e) => eprintln!("error: {}", e),
            }
        }
        Commands::Run {
            sandbox: _,
            prompt: _,
        } => {
            println!("run: use library API directly for now");
        }
        Commands::RegisterTool { sandbox: _, name } => {
            println!("registered tool {} - use library API", name);
        }
        Commands::InvokeTool {
            sandbox: _,
            name,
            input: _,
        } => {
            println!("invoked {} - use library API", name);
        }
        Commands::List => {
            println!("list: use persistence layer or library API");
        }
        Commands::Status { agent: _ } => {
            println!("status: placeholder");
        }
        Commands::Socket { path } => {
            if let Err(e) = socket_server::run_server(&path) {
                eprintln!("socket server error: {}", e);
            }
        }
        Commands::Agent {
            prompt,
            project,
            socket,
        } => {
            libagent::vertex_agent::run(&prompt, &project, &socket)?;
        }
        Commands::OrchestrateNew {
            prompt,
            project,
            socket,
            mut project_root,
            overlay_root,
            checkpoint_root,
            merge_work_dir,
            max_agents,
            max_verification_rounds,
            verbose,
            new,
            output_dir,
            save_all,
            no_tui,
            session_file,
            resume,
        } => {
            #[cfg(not(unix))]
            {
                return Err(anyhow::anyhow!(
                    "New orchestration requires Unix (overlayfs, chroot)"
                ));
            }
            #[cfg(unix)]
            {
                use libagent::orchestration::{NewOrchestrator, OrchestratorConfig, OrchestratorEvent};
                use libagent::orchestration::session_store::{
                    write_snapshot, read_snapshot, InteractiveSessionSnapshot,
                };
                use libagent::orchestration::types::ProjectContext;

                // Optionally initialize file logger
                let log_path = dirs::home_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                    .join(".mowis")
                    .join("agentd.log");
                let _ = libagent::logging::init(&log_path);

                // Handle --resume: load previous session
                if resume {
                    if let Some(ref sf) = session_file {
                        match read_snapshot(sf) {
                            Ok(snap) => {
                                println!("Resuming session: project={}, {} transcript entries",
                                    snap.project_id, snap.transcript.len());
                            }
                            Err(e) => {
                                eprintln!("Failed to load session from {:?}: {}", sf, e);
                            }
                        }
                    } else {
                        eprintln!("--resume requires --session-file");
                    }
                }

                // Set verbose mode if requested
                if verbose {
                    libagent::orchestration::agent_execution::set_verbose(true);
                }

                // Handle --new flag: create fresh project directory
                if new {
                    if output_dir.is_none() {
                        return Err(anyhow::anyhow!(
                            "--new requires --output-dir to specify where to save the generated project"
                        ));
                    }

                    let output_path = output_dir.as_ref().unwrap();

                    if output_path.exists() {
                        println!("Output directory already exists: {:?}", output_path);
                    } else {
                        std::fs::create_dir_all(output_path)?;
                        println!("Created new project directory: {:?}", output_path);
                    }

                    std::fs::write(
                        output_path.join("README.md"),
                        "# Generated by MowisAI\n\nThis project was generated from scratch.\n",
                    )?;

                    project_root = output_path.clone();
                }

                // Set up staging directory
                let staging_dir = if save_all {
                    let staging = std::env::temp_dir().join("mowis-staging");
                    std::fs::create_dir_all(&staging)?;
                    Some(staging)
                } else {
                    None
                };

                // Create event channel for TUI
                let (event_tx, event_rx) =
                    std::sync::mpsc::channel::<OrchestratorEvent>();

                let use_tui = !no_tui;

                let config = OrchestratorConfig {
                    project_id: project.clone(),
                    socket_path: socket,
                    project_root,
                    overlay_root,
                    checkpoint_root,
                    merge_work_dir,
                    max_agents,
                    max_verification_rounds,
                    staging_dir,
                    event_tx: if use_tui { Some(event_tx) } else { None },
                };

                if use_tui {
                    // TUI mode: run orchestrator in background thread, TUI in main thread
                    if verbose {
                        libagent::logging::set_tui_active(false);
                    } else {
                        libagent::logging::set_tui_active(true);
                    }

                    let prompt_clone = prompt.clone();
                    let orchestrator = NewOrchestrator::new(config);

                    // Spawn orchestrator in a background thread
                    let orch_thread = std::thread::Builder::new()
                        .name("orchestrator".into())
                        .spawn(move || -> anyhow::Result<libagent::orchestration::FinalOutput> {
                            let runtime = tokio::runtime::Builder::new_multi_thread()
                                .enable_all()
                                .build()?;
                            runtime.block_on(orchestrator.run(&prompt_clone))
                        })?;

                    // Run TUI on main thread
                    if let Err(e) = libagent::tui::run(event_rx) {
                        eprintln!("TUI error: {}", e);
                    }

                    libagent::logging::set_tui_active(false);

                    // Wait for orchestrator to finish and collect output
                    let output = match orch_thread.join() {
                        Ok(result) => result?,
                        Err(e) => {
                            return Err(anyhow::anyhow!("Orchestrator thread panicked: {:?}", e));
                        }
                    };

                    // Save session if requested
                    if let Some(ref sf) = session_file {
                        let snap = InteractiveSessionSnapshot::new_v1(
                            project.clone(),
                            "/tmp/agentd.sock".to_string(),
                            max_agents,
                            ProjectContext {
                                file_tree: String::new(),
                                relevant_files: Vec::new(),
                                metadata: std::collections::HashMap::new(),
                            },
                            vec![prompt.clone()],
                            std::collections::HashMap::new(),
                            std::collections::HashMap::new(),
                            vec![output.summary.clone()],
                        );
                        if let Err(e) = write_snapshot(sf, &snap) {
                            eprintln!("Failed to save session: {}", e);
                        } else {
                            println!("Session saved to {:?}", sf);
                        }
                    }

                    print_final_output(&output);

                    if save_all {
                        if let Some(output_path) = output_dir.as_ref() {
                            export_staged_workspaces(output_path)?;
                        }
                    }

                    println!("\nOrchestration complete!");
                } else {
                    // No-TUI mode: classic print-based output
                    println!("MowisAI — New 7-Layer Orchestration System");
                    println!("═══════════════════════════════════════════════");
                    if new {
                        println!("NEW PROJECT MODE: Creating fresh codebase");
                        if let Some(ref od) = output_dir {
                            println!("Output directory: {:?}", od);
                        }
                        if save_all {
                            println!("SAVE-ALL: Will copy all agent changes to host");
                        }
                    }
                    if verbose {
                        println!("VERBOSE MODE: Enabled");
                    } else {
                        println!("TIP: Add --verbose flag to see detailed agent output");
                    }
                    println!("TIP: Omit --no-tui to use the interactive TUI dashboard");
                    println!("═══════════════════════════════════════════════\n");

                    let orchestrator = NewOrchestrator::new(config);
                    let runtime = tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()?;

                    let output = runtime.block_on(orchestrator.run(&prompt))?;

                    // Save session if requested
                    if let Some(ref sf) = session_file {
                        let snap = InteractiveSessionSnapshot::new_v1(
                            project.clone(),
                            "/tmp/agentd.sock".to_string(),
                            max_agents,
                            ProjectContext {
                                file_tree: String::new(),
                                relevant_files: Vec::new(),
                                metadata: std::collections::HashMap::new(),
                            },
                            vec![prompt.clone()],
                            std::collections::HashMap::new(),
                            std::collections::HashMap::new(),
                            vec![output.summary.clone()],
                        );
                        if let Err(e) = write_snapshot(sf, &snap) {
                            eprintln!("Failed to save session: {}", e);
                        } else {
                            println!("Session saved to {:?}", sf);
                        }
                    }

                    println!("\n═══════════════════════════════════════════════");
                    println!("FINAL RESULTS");
                    println!("═══════════════════════════════════════════════\n");

                    print_final_output(&output);

                    if save_all {
                        if let Some(output_path) = output_dir.as_ref() {
                            export_staged_workspaces(output_path)?;
                        }
                    }

                    println!("\nOrchestration complete!");
                }
            }
        }
        Commands::Simulate {
            socket,
            tasks,
            sandboxes,
            max_agents,
            failure_rate,
            tool_delay,
            project_root,
            verbose,
        } => {
            #[cfg(not(unix))]
            {
                return Err(anyhow::anyhow!(
                    "Simulation requires Unix (overlayfs, chroot)"
                ));
            }
            #[cfg(unix)]
            {
                use libagent::orchestration::simulate::SimulateCommand;

                if verbose {
                    libagent::orchestration::agent_execution::set_verbose(true);
                }

                let cmd = SimulateCommand {
                    socket,
                    tasks,
                    sandboxes,
                    max_agents,
                    failure_rate,
                    tool_delay,
                    project_root,
                    verbose,
                };

                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?;

                runtime.block_on(cmd.run())?;
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn print_final_output(output: &libagent::orchestration::FinalOutput) {

    println!("Summary: {}", output.summary);
    println!("\nScheduler Stats:");
    println!("  Total tasks: {}", output.scheduler_stats.total_tasks);
    println!("  Completed: {}", output.scheduler_stats.completed);
    println!("  Failed: {}", output.scheduler_stats.failed);
    println!("  Running: {}", output.scheduler_stats.running);
    println!("  Pending: {}", output.scheduler_stats.pending);

    println!("\nSandbox Results:");
    for (name, result) in &output.sandbox_results {
        println!("  {} - {:?}", name, result.verification_status);
    }

    if !output.failed_tasks.is_empty() {
        println!("\nFailed Tasks:");
        for failed in &output.failed_tasks {
            println!("  {} - {}", failed.task_id, failed.error);
        }
    }

    if !output.known_issues.is_empty() {
        println!("\nKnown Issues:");
        for issue in &output.known_issues {
            println!("  - {}", issue);
        }
    }

    if !output.execution_errors.is_empty() {
        println!("\nExecution Errors ({}):", output.execution_errors.len());
        for error in &output.execution_errors {
            println!("  {}", error);
        }
    }

    if !output.merged_diff.is_empty() {
        println!("\nFinal merged diff ({} bytes)", output.merged_diff.len());
        println!("\n{}", output.merged_diff);
    }
}

#[cfg(unix)]
fn export_staged_workspaces(output_path: &std::path::Path) -> anyhow::Result<()> {
    println!("\nSaving agent changes to host filesystem...");
    let staging_dir = std::env::temp_dir().join("mowis-staging");
    let summary = libagent::orchestration::sandbox_topology::export_staged_workspaces_from_dir(
        &staging_dir,
        output_path,
    )?;

    println!("Total agents staged: {}", summary.containers_found);

    if summary.workspaces_copied > 0 {
        println!(
            "Exported {} workspaces ({} files)",
            summary.workspaces_copied, summary.files_copied
        );
        println!("Generated project saved to: {:?}", output_path);

        let ls_result = std::process::Command::new("ls")
            .arg("-lh")
            .arg(output_path)
            .output();
        if let Ok(ls_output) = ls_result {
            println!("{}", String::from_utf8_lossy(&ls_output.stdout));
        }
    } else {
        println!("No files found to save");
    }
    Ok(())
}
