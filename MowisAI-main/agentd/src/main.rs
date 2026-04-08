use clap::{Parser, Subcommand};
use libagent::{socket_server, ResourceLimits, Sandbox};
use std::io::Write;
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
    /// Multi-sandbox orchestration (Gemini plan + parallel agents + synthesis) - OLD SYSTEM
    Orchestrate {
        #[arg(long)]
        prompt: String,
        #[arg(long)]
        project: String,
        #[arg(long, default_value = "/tmp/agentd.sock")]
        socket: String,
        #[arg(long, default_value_t = 10)]
        max_agents: usize,
        /// Verbose logging (HTTP/socket payloads, round timings, etc.)
        #[arg(long, default_value_t = false)]
        debug: bool,
    },
    /// NEW 7-layer orchestration system (fast planner + event-driven scheduler + checkpoints)
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
    },
    /// Same as orchestrate, but stay in-process: enter follow-ups without exiting (reuses sandboxes by team name)
    OrchestrateInteractive {
        #[arg(long)]
        project: String,
        #[arg(long, default_value = "/tmp/agentd.sock")]
        socket: String,
        #[arg(long, default_value_t = 10)]
        max_agents: usize,
        /// Persist transcript, context, sandbox map, and warm container ids (JSON). Also used with `--resume`.
        #[arg(long, value_name = "PATH")]
        session_file: Option<PathBuf>,
        /// Load `--session-file` and continue the REPL (skips the initial task prompt).
        #[arg(long, default_value_t = false)]
        resume: bool,
        /// Verbose logging (HTTP/socket payloads, round timings, etc.)
        #[arg(long, default_value_t = false)]
        debug: bool,
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
        Commands::Orchestrate {
            prompt: _,
            project: _,
            socket: _,
            max_agents: _,
            debug: _,
        } => {
            eprintln!("❌ ERROR: The old Orchestrate command is deprecated.");
            eprintln!("   Please use 'orchestrate-new' instead:");
            eprintln!("   agentd orchestrate-new --prompt \"...\" --project ... --socket ...");
            std::process::exit(1);
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
        } => {
            #[cfg(not(unix))]
            {
                return Err(anyhow::anyhow!(
                    "New orchestration requires Unix (overlayfs, chroot)"
                ));
            }
            #[cfg(unix)]
            {
                use libagent::orchestration::{NewOrchestrator, OrchestratorConfig};

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

                    // Create empty project directory
                    if output_path.exists() {
                        println!("⚠️  Output directory already exists: {:?}", output_path);
                        println!("   Contents will be overwritten if --save-all is used.");
                    } else {
                        std::fs::create_dir_all(output_path)?;
                        println!("📁 Created new project directory: {:?}", output_path);
                    }

                    // Initialize with basic structure
                    std::fs::write(output_path.join("README.md"), "# Generated by MowisAI\n\nThis project was generated from scratch.\n")?;

                    // Use the empty output dir as project_root
                    project_root = output_path.clone();
                }

                println!("🚀 MowisAI — New 7-Layer Orchestration System");
                println!("═══════════════════════════════════════════════");
                if new {
                    println!("🆕 NEW PROJECT MODE: Creating fresh codebase");
                    println!("📂 Output directory: {:?}", output_dir.as_ref().unwrap());
                    if save_all {
                        println!("💾 SAVE-ALL: Will copy all agent changes to host");
                    }
                }
                if verbose {
                    println!("🔍 VERBOSE MODE: Enabled (showing detailed agent output)");
                } else {
                    println!("💡 TIP: Add --verbose flag to see detailed agent output");
                }
                println!("═══════════════════════════════════════════════\n");

                // Set up staging directory for save-all functionality
                let staging_dir = if save_all {
                    let staging = std::env::temp_dir().join("mowis-staging");
                    std::fs::create_dir_all(&staging)?;
                    Some(staging)
                } else {
                    None
                };

                let config = OrchestratorConfig {
                    project_id: project,
                    socket_path: socket,
                    project_root,
                    overlay_root,
                    checkpoint_root,
                    merge_work_dir,
                    max_agents,
                    max_verification_rounds,
                    staging_dir,
                };

                let orchestrator = NewOrchestrator::new(config);

                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?;

                let output = runtime.block_on(orchestrator.run(&prompt))?;

                println!("\n═══════════════════════════════════════════════");
                println!("📊 FINAL RESULTS");
                println!("═══════════════════════════════════════════════\n");

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
                    println!("\n⚠️  Failed Tasks:");
                    for failed in &output.failed_tasks {
                        println!("  {} - {}", failed.task_id, failed.error);
                    }
                }

                if !output.known_issues.is_empty() {
                    println!("\n⚠️  Known Issues:");
                    for issue in &output.known_issues {
                        println!("  - {}", issue);
                    }
                }

                if !output.merged_diff.is_empty() {
                    println!("\n📝 Final merged diff ({} bytes)", output.merged_diff.len());
                    println!("\n{}", output.merged_diff);
                }

                // Handle --save-all: export staged workspaces to host
                if save_all {
                    if let Some(output_path) = output_dir.as_ref() {
                        println!("\n═══════════════════════════════════════════════");
                        println!("💾 Saving agent changes to host filesystem...");
                        println!("═══════════════════════════════════════════════\n");
                        println!("  🔍 Exporting staged workspaces...");

                        let staging_dir = std::env::temp_dir().join("mowis-staging");
                        let summary = libagent::orchestration::sandbox_topology::export_staged_workspaces_from_dir(
                            &staging_dir,
                            output_path,
                        )?;

                        println!("  📊 Total agents staged: {}", summary.containers_found);

                        if summary.workspaces_copied > 0 {
                            println!("  ✅ Exported {} workspaces ({} files)", summary.workspaces_copied, summary.files_copied);
                            println!("\n  📂 Generated project saved to: {:?}", output_path);

                            println!("\n  📋 Files created:");
                            let ls_result = std::process::Command::new("ls")
                                .arg("-lh")
                                .arg(output_path)
                                .output();

                            if let Ok(ls_output) = ls_result {
                                println!("{}", String::from_utf8_lossy(&ls_output.stdout));
                            }
                        } else {
                            println!("  ℹ️  No files found to save (staged workspaces may be empty)");
                        }
                    } else {
                        eprintln!("⚠️  --save-all requires --output-dir");
                    }
                }

                println!("\n✅ Orchestration complete!");
            }
        }
        Commands::OrchestrateInteractive {
            project: _,
            socket: _,
            max_agents: _,
            session_file: _,
            resume: _,
            debug: _,
        } => {
            eprintln!("❌ ERROR: The old OrchestrateInteractive command is deprecated.");
            eprintln!("   The new 7-layer orchestration system doesn't support interactive mode yet.");
            eprintln!("   Please use 'orchestrate-new' instead:");
            eprintln!("   agentd orchestrate-new --prompt \"...\" --project ... --socket ...");
            std::process::exit(1);
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
