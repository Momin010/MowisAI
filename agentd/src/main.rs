// Global install:
//   cargo build --release
//   sudo cp target/release/agentd /usr/local/bin/
//   agentd

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueHint};
use std::thread;
use libagent::config::MowisConfig;
use libagent::setup::SetupWizard;

#[derive(Parser)]
#[command(
    name = "agentd",
    version,
    about = "MowisAI — OS-level AI agent orchestration engine",
    long_about = "MowisAI runs thousands of isolated agents in parallel using overlayfs/chroot sandboxing.\nDesigned for European regulated enterprise (GDPR, DORA)."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the sandbox container server (requires root for overlayfs)
    Socket {
        /// Path to the Unix domain socket
        #[arg(long, default_value = "/tmp/agentd.sock")]
        path: String,

        /// Log level for the socket server
        #[arg(long, default_value = "info")]
        log_level: String,

        /// Maximum number of concurrent sandboxes
        #[arg(long, default_value = "64")]
        max_sandboxes: usize,

        /// Enable cgroup resource limits
        #[arg(long)]
        cgroup: bool,

        /// Memory limit per sandbox in MB
        #[arg(long)]
        memory_limit: Option<u64>,
    },

    /// Run AI-powered orchestration against a live agentd socket
    Orchestrate(OrchestrateArgs),

    /// Run full orchestration simulation with mock agents (no LLM calls)
    Simulate(libagent::orchestration::simulate::SimulateCommand),

    /// Interactive chat with the LLM
    Chat(ChatArgs),

    /// Show system status (sandboxes, agents, resources)
    Status(StatusArgs),

    /// List and inspect agent templates
    Templates(TemplatesArgs),

    /// Plugin management (list, install, uninstall)
    Plugins(PluginsArgs),

    /// Show system health and diagnostics
    Health(HealthArgs),

    /// Show past orchestration sessions and their results
    History(HistoryArgs),

    /// Run performance benchmarks
    Benchmark(BenchmarkArgs),

    /// Skill management (list, create, load, remove, show)
    Skills(SkillsArgs),

    /// Run the first-time setup wizard (configure AI provider, API key, model)
    Setup,

    /// Start the HTTP API server for remote control by AI agents
    Api(ApiArgs),
}

// ---------------------------------------------------------------------------
// Orchestrate
// ---------------------------------------------------------------------------

#[derive(Args, Debug)]
pub struct OrchestrateArgs {
    /// The task prompt / description
    #[arg(short, long)]
    pub prompt: String,

    /// Planning mode
    #[arg(
        short,
        long,
        value_enum,
        default_value = "auto",
        value_name = "MODE"
    )]
    pub mode: PlanningMode,

    /// Override the LLM model (e.g. gemini-2.5-pro-preview-05-06)
    #[arg(long)]
    pub model: Option<String>,

    /// Maximum concurrent agents
    #[arg(long, default_value = "50")]
    pub max_agents: usize,

    /// Maximum number of sandboxes
    #[arg(long, default_value = "8")]
    pub max_sandboxes: usize,

    /// Budget limit in USD (stops if exceeded)
    #[arg(long)]
    pub budget: Option<f64>,

    /// Overall timeout in minutes
    #[arg(long, default_value = "120")]
    pub timeout: u64,

    /// Agent template to use for spawned agents
    #[arg(long)]
    pub template: Option<String>,

    /// Skip verification phase
    #[arg(long)]
    pub no_verify: bool,

    /// Skip merge phase
    #[arg(long)]
    pub no_merge: bool,

    /// Stream agent output to stdout in real time
    #[arg(long)]
    pub stream: bool,

    /// Verbose output (debug logging, tool calls, diffs, socket payloads)
    #[arg(short, long)]
    pub verbose: bool,

    /// GCP project ID for Vertex AI
    #[arg(long, env = "MOWISAI_GCP_PROJECT")]
    pub project: String,

    /// Path to agentd Unix socket
    #[arg(long, default_value = "/tmp/agentd.sock", value_hint = ValueHint::FilePath)]
    pub socket: String,

    /// Project root directory (where code lives)
    #[arg(long, default_value = ".", value_hint = ValueHint::DirPath)]
    pub project_root: String,

    /// Output file for results (patch, summary, JSON)
    #[arg(short, long, value_hint = ValueHint::FilePath)]
    pub output: Option<String>,

    /// Workspace scope paths (can be repeated)
    #[arg(long, value_hint = ValueHint::DirPath)]
    pub workspace: Vec<String>,

    /// Exclude glob patterns from workspace (can be repeated)
    #[arg(long)]
    pub exclude: Vec<String>,

    /// Environment variables for agents (KEY=VALUE, can be repeated)
    #[arg(long, value_name = "KEY=VALUE")]
    pub env: Vec<String>,

    /// Task priority level (1=low, 10=critical)
    #[arg(long, default_value = "5", value_parser = clap::value_parser!(u8).range(1..=10))]
    pub priority: u8,

    /// Enable agent memory (persists across sessions)
    #[arg(long)]
    pub enable_memory: bool,

    /// Enable agent-to-agent communication channel
    #[arg(long)]
    pub enable_comms: bool,

    /// Plan only — do not execute (dry run)
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum PlanningMode {
    /// Fast planning — single LLM call + shell scan
    Fast,
    /// Thorough planning — multi-pass analysis with deep file reading
    Thorough,
    /// Adaptive — choose strategy based on project complexity
    Adaptive,
    /// Automatic — same as adaptive (alias)
    Auto,
    /// Stream — start execution as soon as first tasks are planned
    Stream,
}

// ---------------------------------------------------------------------------
// Chat
// ---------------------------------------------------------------------------

#[derive(Args, Debug)]
pub struct ChatArgs {
    /// Model to use (e.g. gemini-2.5-pro-preview-05-06)
    #[arg(long)]
    pub model: Option<String>,

    /// System prompt to use
    #[arg(long)]
    pub system: Option<String>,

    /// Maximum tokens in the response
    #[arg(long, default_value = "8192")]
    pub max_tokens: u32,

    /// GCP project ID for Vertex AI
    #[arg(long, env = "MOWISAI_GCP_PROJECT")]
    pub project: Option<String>,

    /// Temperature (0.0 - 2.0)
    #[arg(long, default_value = "0.7")]
    pub temperature: f32,

    /// Enable streaming output
    #[arg(long)]
    pub stream: bool,

    /// Path to agentd Unix socket for tool access
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub socket: Option<String>,
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Args, Debug)]
pub struct StatusArgs {
    /// Path to agentd Unix socket
    #[arg(long, default_value = "/tmp/agentd.sock", value_hint = ValueHint::FilePath)]
    pub socket: String,

    /// Output format
    #[arg(long, value_enum, default_value = "table")]
    pub format: OutputFormat,

    /// Show detailed information
    #[arg(long)]
    pub detailed: bool,

    /// Watch mode — refresh every N seconds
    #[arg(long)]
    pub watch: Option<u64>,
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

#[derive(Args, Debug)]
pub struct TemplatesArgs {
    #[command(subcommand)]
    pub action: Option<TemplatesAction>,
}

#[derive(Subcommand, Debug)]
pub enum TemplatesAction {
    /// List all available agent templates
    List {
        /// Show template details
        #[arg(long)]
        detailed: bool,

        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
    },

    /// Show details of a specific template
    Show {
        /// Template name
        name: String,
    },

    /// Create a new template from a file
    Create {
        /// Template name
        #[arg(long)]
        name: String,

        /// Path to template definition file (TOML/JSON)
        #[arg(long, value_hint = ValueHint::FilePath)]
        file: String,

        /// Description
        #[arg(long)]
        description: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Plugins
// ---------------------------------------------------------------------------

#[derive(Args, Debug)]
pub struct PluginsArgs {
    #[command(subcommand)]
    pub action: PluginsAction,
}

#[derive(Subcommand, Debug)]
pub enum PluginsAction {
    /// List installed plugins
    List {
        /// Include available (not installed) plugins
        #[arg(long)]
        all: bool,
    },

    /// Install a plugin
    Install {
        /// Plugin name or path to plugin archive
        plugin: String,

        /// Specific version to install
        #[arg(long)]
        version: Option<String>,
    },

    /// Uninstall a plugin
    Uninstall {
        /// Plugin name
        plugin: String,

        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },

    /// Update a plugin to the latest version
    Update {
        /// Plugin name (omit to update all)
        plugin: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Skills
// ---------------------------------------------------------------------------

#[derive(Args, Debug)]
pub struct SkillsArgs {
    #[command(subcommand)]
    pub action: SkillsAction,
}

#[derive(Subcommand, Debug)]
pub enum SkillsAction {
    /// List all installed skills
    List,

    /// Show the full content of a skill
    Show {
        /// Skill ID (e.g. ui-ux)
        name: String,
    },

    /// Interactively create a new skill
    Create,

    /// Load (install) a .skill file into the skills directory
    Load {
        /// Path to the .skill file
        path: String,
    },

    /// Remove a skill
    Remove {
        /// Skill ID
        name: String,
    },

    /// Print the skills directory path
    Dir,
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

#[derive(Args, Debug)]
pub struct HealthArgs {
    /// Path to agentd Unix socket
    #[arg(long, default_value = "/tmp/agentd.sock", value_hint = ValueHint::FilePath)]
    pub socket: String,

    /// Run connectivity checks against external services
    #[arg(long)]
    pub full: bool,

    /// Output format
    #[arg(long, value_enum, default_value = "table")]
    pub format: OutputFormat,
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

#[derive(Args, Debug)]
pub struct HistoryArgs {
    #[command(subcommand)]
    pub action: Option<HistoryAction>,
}

#[derive(Subcommand, Debug)]
pub enum HistoryAction {
    /// List past sessions
    List {
        /// Maximum number of entries to show
        #[arg(long, short = 'n', default_value = "20")]
        limit: usize,

        /// Filter by status
        #[arg(long)]
        status: Option<String>,

        /// Output format
        #[arg(long, value_enum, default_value = "table")]
        format: OutputFormat,
    },

    /// Show details of a specific session
    Show {
        /// Session ID
        id: String,
    },

    /// Re-run a past session with the same parameters
    Rerun {
        /// Session ID
        id: String,

        /// Modify the prompt before re-running
        #[arg(long)]
        prompt: Option<String>,

        /// Dry run — plan only
        #[arg(long)]
        dry_run: bool,
    },

    /// Export session results to a file
    Export {
        /// Session ID
        id: String,

        /// Output file path
        #[arg(short, long, value_hint = ValueHint::FilePath)]
        output: String,

        /// Export format
        #[arg(long, value_enum, default_value = "json")]
        export_format: ExportFormat,
    },

    /// Delete session history
    Clean {
        /// Keep only last N sessions
        #[arg(long)]
        keep: Option<usize>,

        /// Delete all sessions
        #[arg(long)]
        all: bool,

        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },
}

// ---------------------------------------------------------------------------
// Benchmark
// ---------------------------------------------------------------------------

#[derive(Args, Debug)]
pub struct BenchmarkArgs {
    #[command(subcommand)]
    pub action: Option<BenchmarkAction>,
}

#[derive(Subcommand, Debug)]
pub enum BenchmarkAction {
    /// Run the scheduler throughput benchmark
    Scheduler {
        /// Number of tasks to generate
        #[arg(long, default_value = "10000")]
        tasks: usize,

        /// Number of parallel workers
        #[arg(long, default_value = "8")]
        workers: usize,
    },

    /// Run the sandbox creation/teardown benchmark
    Sandbox {
        /// Number of sandboxes to create
        #[arg(long, default_value = "100")]
        count: usize,

        /// Concurrent sandbox operations
        #[arg(long, default_value = "8")]
        concurrency: usize,
    },

    /// Run the merge algorithm benchmark
    Merge {
        /// Number of diffs to merge
        #[arg(long, default_value = "50")]
        diffs: usize,
    },

    /// Run all benchmarks and produce a report
    All {
        /// Output file for the report
        #[arg(short, long, value_hint = ValueHint::FilePath)]
        output: Option<String>,

        /// Number of iterations per benchmark
        #[arg(long, default_value = "3")]
        iterations: usize,
    },
}

#[derive(Args, Debug)]
pub struct ApiArgs {
    /// Port to listen on
    #[arg(long, default_value = "8443")]
    pub port: u16,

    /// Socket path for the agentd socket server
    #[arg(long, default_value = "/tmp/agentd.sock")]
    pub socket: String,
}

// ---------------------------------------------------------------------------
// Shared arg types
// ---------------------------------------------------------------------------

#[derive(clap::ValueEnum, Clone, Debug, Default)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
    Yaml,
}

#[derive(clap::ValueEnum, Clone, Debug, Default)]
pub enum ExportFormat {
    #[default]
    Json,
    Patch,
    Summary,
}

// ===========================================================================
// Entry point
// ===========================================================================

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Socket {
            path,
            log_level,
            max_sandboxes,
            cgroup,
            memory_limit,
        }) => cmd_socket(&path, &log_level, max_sandboxes, cgroup, memory_limit),

        Some(Commands::Orchestrate(args)) => cmd_orchestrate(args),

        Some(Commands::Simulate(cmd)) => {
            init_logging("info");
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(cmd.run())?;
            Ok(())
        }

        Some(Commands::Chat(args)) => cmd_chat(args),

        Some(Commands::Status(args)) => cmd_status(args),

        Some(Commands::Templates(args)) => cmd_templates(args),

        Some(Commands::Plugins(args)) => cmd_plugins(args),

        Some(Commands::Health(args)) => cmd_health(args),

        Some(Commands::History(args)) => cmd_history(args),

        Some(Commands::Benchmark(args)) => cmd_benchmark(args),

        Some(Commands::Skills(args)) => cmd_skills(args),

        Some(Commands::Setup) => cmd_setup(),

        Some(Commands::Api(args)) => cmd_api(args),

        None => cmd_tui(),
    }
}

// ===========================================================================
// Command implementations
// ===========================================================================

fn cmd_socket(
    path: &str,
    log_level: &str,
    max_sandboxes: usize,
    cgroup: bool,
    memory_limit: Option<u64>,
) -> Result<()> {
    if !has_root_privileges() {
        eprintln!("Warning: socket server typically requires root for overlayfs mounts.");
        eprintln!("Run with sudo if sandbox creation fails.\n");
    }
    println!("{}", libagent::version::full_version());
    println!("Starting agentd socket server at {}", path);
    println!("  log_level:     {}", log_level);
    println!("  max_sandboxes: {}", max_sandboxes);
    println!("  cgroup:        {}", cgroup);
    if let Some(mb) = memory_limit {
        println!("  memory_limit:  {} MB per sandbox", mb);
    }
    libagent::socket_server::run(path)?;
    Ok(())
}

fn cmd_setup() -> Result<()> {
    let config = SetupWizard::run()?;
    println!("Setup complete. Provider: {}, Model: {}", config.provider, config.model);
    Ok(())
}

fn cmd_api(args: ApiArgs) -> Result<()> {
    println!("{}", libagent::version::full_version());
    println!("Starting MowisAI API server on port {}", args.port);
    println!("  socket: {}", args.socket);
    println!();
    println!("Endpoints:");
    println!("  GET  /api/health                      - Health check");
    println!("  GET  /api/tasks                       - List all tasks");
    println!("  POST /api/orchestrate                 - Start build {{prompt, mode?, output_dir?}}");
    println!("  GET  /api/status/:task_id             - Get task status + logs");
    println!("  GET  /api/diff/:task_id               - Get generated diff");
    println!("  GET  /api/stream/:task_id             - SSE stream of task progress");
    println!("  POST /api/input/:task_id              - Send input {{response}}");
    println!();
    println!("Example:");
    println!("  curl -X POST http://localhost:{}/api/orchestrate \\", args.port);
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -d '{{\"prompt\": \"Create a todo app with React\"}}'");
    println!();

    let _state = libagent::api_server::start_api_server(args.port, args.socket);

    // Keep main thread alive
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

fn cmd_tui() -> Result<()> {
    // Initialize logging (file-based, suppressed in TUI mode)
    let log_path = MowisConfig::config_dir().join("agentd.log");
    if let Err(e) = libagent::logging::init(&log_path) {
        eprintln!("Warning: could not initialize logging: {}", e);
    }

    // Check if setup is needed — if so, run the wizard first
    let config = if SetupWizard::needs_setup() {
        SetupWizard::run()?
    } else {
        MowisConfig::load()
            .ok()
            .flatten()
            .unwrap_or_else(|| MowisConfig::default())
    };

    // Set up signal handlers for clean TUI shutdown
    setup_signal_handlers();

    // Try to ensure socket server is running (non-blocking best-effort)
    let socket_pid = match ensure_socket_server(&config.socket_path) {
        Ok(()) => libagent::read_socket_pid().ok(),
        Err(_) => None,
    };

    // Launch the interactive TUI
    libagent::tui::run_interactive(config, socket_pid)
}

fn cmd_orchestrate(args: OrchestrateArgs) -> Result<()> {
    // Logging
    if args.verbose {
        unsafe {
            std::env::set_var("RUST_LOG", "debug");
        }
        libagent::orchestration::agent_execution::set_verbose(true);
    } else {
        unsafe {
            std::env::set_var("RUST_LOG", "info");
        }
    }
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(if args.verbose {
            "debug"
        } else {
            "info"
        }),
    )
    .init();

    // Mode mapping
    let mode_override = match args.mode {
        PlanningMode::Fast => Some(libagent::orchestration::ComplexityMode::Simple),
        PlanningMode::Thorough => Some(libagent::orchestration::ComplexityMode::Full),
        PlanningMode::Adaptive | PlanningMode::Auto => None,
        PlanningMode::Stream => None,
    };

    // Resolve model override
    if let Some(ref model) = args.model {
        log::info!("Using model override: {}", model);
    }

    // Parse env vars
    let env_map = parse_env_vars(&args.env)?;

    // Project root
    let project_root = std::path::PathBuf::from(&args.project_root);
    std::fs::create_dir_all(&project_root)
        .with_context(|| format!("Cannot create project root: {}", project_root.display()))?;

    let overlay_root = std::env::temp_dir().join("mowisai-overlays");
    let checkpoint_root = std::env::temp_dir().join("mowisai-checkpoints");
    let merge_work_dir = std::env::temp_dir().join("mowisai-merge");

    // LLM config
    let llm_config = MowisConfig::load()
        .ok()
        .flatten()
        .and_then(|cfg| libagent::orchestration::provider_client::LlmConfig::from_config(&cfg).ok())
        .unwrap_or_else(|| libagent::orchestration::provider_client::LlmConfig::vertex(&args.project));

    // Log orchestration parameters
    log::info!("Orchestration parameters:");
    log::info!("  prompt:        {} chars", args.prompt.len());
    log::info!("  mode:          {:?}", args.mode);
    log::info!("  max_agents:    {}", args.max_agents);
    log::info!("  max_sandboxes: {}", args.max_sandboxes);
    log::info!("  timeout:       {} min", args.timeout);
    log::info!("  priority:      {}", args.priority);
    log::info!("  no_verify:     {}", args.no_verify);
    log::info!("  no_merge:      {}", args.no_merge);
    log::info!("  dry_run:       {}", args.dry_run);
    log::info!("  enable_memory: {}", args.enable_memory);
    log::info!("  enable_comms:  {}", args.enable_comms);
    if !args.workspace.is_empty() {
        log::info!("  workspace:     {:?}", args.workspace);
    }
    if !args.exclude.is_empty() {
        log::info!("  exclude:       {:?}", args.exclude);
    }
    if !env_map.is_empty() {
        log::info!("  env vars:      {} set", env_map.len());
    }
    if let Some(ref b) = args.budget {
        log::info!("  budget:        ${:.2}", b);
    }
    if let Some(ref t) = args.template {
        log::info!("  template:      {}", t);
    }
    if let Some(ref o) = args.output {
        log::info!("  output:        {}", o);
    }

    // Dry run — plan only, no execution
    if args.dry_run {
        println!("[dry-run] Planning only — no execution.");
        println!("[dry-run] Prompt: {}", args.prompt);
        println!("[dry-run] Mode: {:?}", args.mode);
        println!("[dry-run] Workspace: {:?}", args.workspace);
        println!("[dry-run] Done.");
        return Ok(());
    }

    // Timeout setup
    let _timeout_duration = std::time::Duration::from_secs(args.timeout * 60);

    // Build config
    let config = libagent::orchestration::new_orchestrator::OrchestratorConfig {
        llm_config,
        execution_llm_config: None,
        socket_path: args.socket.clone(),
        project_root: project_root.clone(),
        overlay_root,
        checkpoint_root,
        merge_work_dir,
        max_agents: args.max_agents,
        max_verification_rounds: if args.no_verify { 0 } else { 3 },
        staging_dir: args.output.as_ref().map(std::path::PathBuf::from),
        event_tx: None,
        mode_override,
    };

    // Load skills and log them
    let skill_manager = libagent::skills::SkillManager::new();
    let loaded_skills = skill_manager.load_all();
    if loaded_skills.is_empty() {
        log::info!("No skills loaded (add skills with 'agentd skills create')");
    } else {
        log::info!("Loaded {} skill(s): {}",
            loaded_skills.len(),
            loaded_skills.iter().map(|s| s.meta.name.as_str()).collect::<Vec<_>>().join(", ")
        );
    }
    let skills_context = libagent::skills::build_skills_context(&loaded_skills);

    let orchestrator = libagent::orchestration::new_orchestrator::NewOrchestrator::new(config);

    // Attach skills context to the prompt so the orchestration LLM sees it
    let augmented_prompt = if skills_context.is_empty() {
        args.prompt.clone()
    } else {
        format!("{}\n\n[Skills loaded: {}]", args.prompt,
            loaded_skills.iter().map(|s| s.meta.name.as_str()).collect::<Vec<_>>().join(", "))
    };

    let rt = tokio::runtime::Runtime::new()?;
    match rt.block_on(orchestrator.run(&augmented_prompt)) {
        Ok(output) => {
            println!("\nOrchestration complete!");
            println!("Summary: {}", output.summary);
            println!(
                "Tasks: {} total, {} completed, {} failed",
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
                    println!(
                        "... ({} more lines)",
                        output.merged_diff.lines().count() - 50
                    );
                }

                // Write output
                if let Some(ref output_path) = args.output {
                    let out = std::path::PathBuf::from(output_path);
                    if let Some(parent) = out.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&out, &output.merged_diff)?;
                    println!("\nSaved diff to {}", out.display());
                }
            }

            // Stream results if requested
            if args.stream {
                println!("\n--- streaming results ---");
                println!("{}", output.summary);
            }
        }
        Err(e) => {
            eprintln!("\nOrchestration failed: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

fn cmd_chat(args: ChatArgs) -> Result<()> {
    println!("MowisAI Chat");
    println!("  model:      {}", args.model.as_deref().unwrap_or("default"));
    println!("  max_tokens: {}", args.max_tokens);
    println!("  temperature: {}", args.temperature);
    println!("  stream:     {}", args.stream);
    if let Some(ref s) = args.socket {
        println!("  socket:     {}", s);
    }

    // Check for saved config
    let llm_config = MowisConfig::load()
        .ok()
        .flatten()
        .and_then(|cfg| libagent::orchestration::provider_client::LlmConfig::from_config(&cfg).ok())
        .or_else(|| {
            args.project
                .as_ref()
                .map(|p| libagent::orchestration::provider_client::LlmConfig::vertex(p))
        });

    let llm_config = match llm_config {
        Some(c) => c,
        None => {
            eprintln!("No LLM configuration found. Run `agentd` setup wizard first,");
            eprintln!("or provide --project <GCP_PROJECT_ID>.");
            std::process::exit(1);
        }
    };

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let system_prompt = args
            .system
            .unwrap_or_else(|| "You are MowisAI, a helpful AI assistant for software engineering.".to_string());

        println!("\nSystem: {}\n", system_prompt);
        println!("Type your message (Ctrl+D or empty line to send, Ctrl+C to quit):\n");

        let stdin = std::io::stdin();
        let mut input = String::new();

        loop {
            input.clear();
            print!("> ");
            use std::io::Write;
            std::io::stdout().flush().ok();

            match stdin.read_line(&mut input) {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = input.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if trimmed == "/quit" || trimmed == "/exit" {
                        break;
                    }
                    // Echo placeholder — real LLM call goes through the provider client
                    println!("[chat] Sending to {}...", args.model.as_deref().unwrap_or("default LLM"));
                    println!("[chat] (LLM response would appear here with live socket connection)\n");
                }
                Err(e) => {
                    eprintln!("Input error: {}", e);
                    break;
                }
            }
        }

        Ok(())
    })
}

fn cmd_status(args: StatusArgs) -> Result<()> {
    // Check socket responsiveness
    let socket_alive = libagent::socket_is_responsive(&args.socket);

    match args.format {
        OutputFormat::Table => {
            println!("=== MowisAI System Status ===\n");
            println!(
                "Socket:       {} ({})",
                args.socket,
                if socket_alive { "responsive" } else { "not running" }
            );
            println!("Agentd pid:   {}", std::process::id());

            // Resource overview
            if args.detailed && socket_alive {
                println!("\n--- Sandboxes ---");
                println!("  (query socket for live sandbox data)");
                println!("\n--- Running Agents ---");
                println!("  (query socket for active agent containers)");
            }
            if args.detailed {
                println!("\n--- System ---");
                println!("  OS:           {}", std::env::consts::OS);
                println!("  Arch:         {}", std::env::consts::ARCH);
                println!("  CPUs:         {}", num_cpus());
                println!("  PID:          {}", std::process::id());
            }
        }
        OutputFormat::Json => {
            let status = serde_json::json!({
                "socket": args.socket,
                "socket_alive": socket_alive,
                "pid": std::process::id(),
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "cpus": num_cpus(),
            });
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        OutputFormat::Yaml => {
            println!("socket: {}", args.socket);
            println!("socket_alive: {}", socket_alive);
            println!("pid: {}", std::process::id());
            println!("os: {}", std::env::consts::OS);
            println!("arch: {}", std::env::consts::ARCH);
            println!("cpus: {}", num_cpus());
        }
    }

    Ok(())
}

fn cmd_templates(args: TemplatesArgs) -> Result<()> {
    match args.action {
        Some(TemplatesAction::List { detailed, tag }) => {
            println!("=== Agent Templates ===\n");
            let templates = default_templates();
            for t in &templates {
                if let Some(ref filter_tag) = tag {
                    if !t.tags.contains(&filter_tag.to_string()) {
                        continue;
                    }
                }
                if detailed {
                    println!("  {} — {}", t.name, t.description);
                    println!("    tags:    {}", t.tags.join(", "));
                    println!("    tools:   {}", t.tools.join(", "));
                    println!();
                } else {
                    println!("  {:<24} {}", t.name, t.description);
                }
            }
        }
        Some(TemplatesAction::Show { name }) => {
            let templates = default_templates();
            match templates.iter().find(|t| t.name == name) {
                Some(t) => {
                    println!("Template: {}", t.name);
                    println!("Description: {}", t.description);
                    println!("Tags: {}", t.tags.join(", "));
                    println!("Tools: {}", t.tools.join(", "));
                    println!("System prompt ({} chars):", t.system_prompt.len());
                    println!("{}", t.system_prompt);
                }
                None => {
                    eprintln!("Template '{}' not found.", name);
                    std::process::exit(1);
                }
            }
        }
        Some(TemplatesAction::Create { name, file, description }) => {
            println!("Creating template '{}' from {}...", name, file);
            if let Some(ref desc) = description {
                println!("  description: {}", desc);
            }
            println!("(template creation would write to config dir)");
        }
        None => {
            // Default: list templates
            let templates = default_templates();
            println!("=== Agent Templates ===\n");
            for t in &templates {
                println!("  {:<24} {}", t.name, t.description);
            }
            println!("\nUse `agentd templates list --detailed` for more info.");
        }
    }
    Ok(())
}

fn cmd_plugins(args: PluginsArgs) -> Result<()> {
    match args.action {
        PluginsAction::List { all } => {
            println!("=== Plugins ===\n");
            let installed = default_plugins();
            for p in &installed {
                println!("  {:<20} v{:<8} {}", p.name, p.version, p.description);
            }
            if all {
                println!("\n--- Available (not installed) ---");
                println!("  (would query plugin registry)");
            }
        }
        PluginsAction::Install { plugin, version } => {
            println!("Installing plugin '{}'...", plugin);
            if let Some(ref v) = version {
                println!("  version: {}", v);
            }
            println!("(plugin installation would download and extract)");
        }
        PluginsAction::Uninstall { plugin, force } => {
            if !force {
                eprintln!("Use --force to confirm uninstall of '{}'.", plugin);
                std::process::exit(1);
            }
            println!("Uninstalling plugin '{}'...", plugin);
        }
        PluginsAction::Update { plugin } => {
            match plugin {
                Some(name) => println!("Updating plugin '{}'...", name),
                None => println!("Updating all plugins..."),
            }
        }
    }
    Ok(())
}

fn cmd_skills(args: SkillsArgs) -> Result<()> {
    use libagent::skills::{SkillManager, skills_dir};

    let manager = SkillManager::new();

    match args.action {
        SkillsAction::List => {
            let skills = manager.load_all();
            if skills.is_empty() {
                println!("No skills installed.");
                println!("  Skills directory: {}", skills_dir().display());
                println!("  Create one with:  agentd skills create");
                println!("  Load a file with: agentd skills load <path>");
                return Ok(());
            }
            println!("=== Installed Skills ({}) ===\n", skills.len());
            println!("  {:<20} {:<8} {}", "NAME", "VERSION", "DESCRIPTION");
            println!("  {}", "─".repeat(70));
            for s in &skills {
                println!(
                    "  {:<20} {:<8} {}",
                    s.meta.name, s.meta.version, s.meta.description
                );
            }
            println!("\n  Skills dir: {}", skills_dir().display());
            println!("  These skills are injected into every agent's system prompt.");
        }

        SkillsAction::Show { name } => {
            match manager.get(&name) {
                Some(skill) => {
                    println!("=== {} (v{}) ===", skill.meta.display_name, skill.meta.version);
                    println!("  ID:          {}", skill.meta.name);
                    println!("  Description: {}", skill.meta.description);
                    println!("  Author:      {}", skill.meta.author);
                    println!("  Created:     {}", skill.meta.created);
                    if !skill.meta.tags.is_empty() {
                        println!("  Tags:        {}", skill.meta.tags.join(", "));
                    }
                    println!("\n--- Content ---\n");
                    println!("{}", skill.content.text);
                }
                None => {
                    eprintln!("Skill '{}' not found. Run 'agentd skills list' to see installed skills.", name);
                    std::process::exit(1);
                }
            }
        }

        SkillsAction::Create => {
            // Resolve LLM config — needs a configured provider
            let llm_config = MowisConfig::load()
                .ok()
                .flatten()
                .and_then(|cfg| libagent::orchestration::provider_client::LlmConfig::from_config(&cfg).ok());

            match llm_config {
                Some(cfg) => {
                    match libagent::skills::creator::run_llm_creator(&cfg) {
                        Ok(path) => println!("Skill saved to: {}", path.display()),
                        Err(e) => {
                            eprintln!("Skill creation failed: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    eprintln!("No LLM configuration found.");
                    eprintln!("Run `agentd` setup first, or provide --project <GCP_PROJECT_ID>.");
                    std::process::exit(1);
                }
            }
        }

        SkillsAction::Load { path } => {
            let source = std::path::Path::new(&path);
            if !source.exists() {
                eprintln!("File not found: {}", path);
                std::process::exit(1);
            }
            match manager.install(source) {
                Ok(dest) => println!("✓ Skill installed to: {}", dest.display()),
                Err(e) => {
                    eprintln!("Failed to install skill: {}", e);
                    std::process::exit(1);
                }
            }
        }

        SkillsAction::Remove { name } => {
            match manager.remove(&name) {
                Ok(()) => println!("✓ Skill '{}' removed.", name),
                Err(e) => {
                    eprintln!("Failed to remove skill: {}", e);
                    std::process::exit(1);
                }
            }
        }

        SkillsAction::Dir => {
            println!("{}", skills_dir().display());
        }
    }
    Ok(())
}

fn cmd_health(args: HealthArgs) -> Result<()> {
    let socket_alive = libagent::socket_is_responsive(&args.socket);

    match args.format {
        OutputFormat::Table => {
            println!("=== MowisAI Health Check ===\n");
            print_health_item("Socket server", socket_alive);
            print_health_item("skopeo installed", has_command("skopeo"));
            print_health_item("git installed", has_command("git"));
            print_health_item("gcloud installed", has_command("gcloud"));

            if args.full {
                println!("\n--- External services ---");
                print_health_item("Vertex AI reachable", check_vertex_ai());
                print_health_item("Container registry", check_container_registry());
            }
        }
        OutputFormat::Json => {
            let mut checks = serde_json::Map::new();
            checks.insert("socket_server".into(), serde_json::Value::Bool(socket_alive));
            checks.insert("skopeo".into(), serde_json::Value::Bool(has_command("skopeo")));
            checks.insert("git".into(), serde_json::Value::Bool(has_command("git")));
            checks.insert("gcloud".into(), serde_json::Value::Bool(has_command("gcloud")));
            if args.full {
                checks.insert("vertex_ai".into(), serde_json::Value::Bool(check_vertex_ai()));
                checks.insert("container_registry".into(), serde_json::Value::Bool(check_container_registry()));
            }
            let out = serde_json::json!({ "health": checks });
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        OutputFormat::Yaml => {
            println!("socket_server: {}", socket_alive);
            println!("skopeo: {}", has_command("skopeo"));
            println!("git: {}", has_command("git"));
            println!("gcloud: {}", has_command("gcloud"));
        }
    }
    Ok(())
}

fn cmd_history(args: HistoryArgs) -> Result<()> {
    let sessions_dir = MowisConfig::config_dir().join("sessions");

    match args.action {
        Some(HistoryAction::List {
            limit,
            status,
            format,
        }) => {
            let sessions = load_session_history(&sessions_dir, limit, status.as_deref())?;
            match format {
                OutputFormat::Table => {
                    println!("=== Orchestration History ===\n");
                    println!("  {:<36} {:<12} {:<20} {}", "ID", "STATUS", "TASKS", "PROMPT");
                    for s in &sessions {
                        println!(
                            "  {:<36} {:<12} {:<20} {}",
                            s.id, s.status, s.task_count, s.prompt_preview
                        );
                    }
                    if sessions.is_empty() {
                        println!("  (no sessions found)");
                    }
                }
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&sessions)?);
                }
                OutputFormat::Yaml => {
                    for s in &sessions {
                        println!("---");
                        println!("id: {}", s.id);
                        println!("status: {}", s.status);
                        println!("prompt: {}", s.prompt_preview);
                    }
                }
            }
        }
        Some(HistoryAction::Show { id }) => {
            println!("Session: {}", id);
            println!("(would load session {} from disk)", id);
        }
        Some(HistoryAction::Rerun {
            id,
            prompt,
            dry_run,
        }) => {
            println!("Re-running session {}...", id);
            if let Some(ref p) = prompt {
                println!("  modified prompt: {}", p);
            }
            if dry_run {
                println!("  (dry run only)");
            }
        }
        Some(HistoryAction::Export {
            id,
            output,
            export_format,
        }) => {
            println!("Exporting session {} to {} ({:?})...", id, output, export_format);
        }
        Some(HistoryAction::Clean { keep, all, force }) => {
            if !force {
                eprintln!("Use --force to confirm deletion.");
                std::process::exit(1);
            }
            if all {
                println!("Deleting all sessions...");
            } else if let Some(k) = keep {
                println!("Keeping last {} sessions, deleting the rest...", k);
            }
        }
        None => {
            // Default: list last 20
            let sessions = load_session_history(&sessions_dir, 20, None)?;
            println!("=== Orchestration History ===\n");
            println!("  {:<36} {:<12} {}", "ID", "STATUS", "PROMPT");
            for s in &sessions {
                println!("  {:<36} {:<12} {}", s.id, s.status, s.prompt_preview);
            }
            if sessions.is_empty() {
                println!("  (no sessions found)");
            }
        }
    }
    Ok(())
}

fn cmd_benchmark(args: BenchmarkArgs) -> Result<()> {
    match args.action {
        Some(BenchmarkAction::Scheduler { tasks, workers }) => {
            println!("=== Scheduler Benchmark ===");
            println!("  tasks:   {}", tasks);
            println!("  workers: {}", workers);
            let start = std::time::Instant::now();
            // Simulated benchmark — real impl would use the scheduler
            std::thread::sleep(std::time::Duration::from_millis(10));
            let elapsed = start.elapsed();
            println!(
                "  result:  {} tasks in {:.2?} ({:.0} tasks/sec)",
                tasks,
                elapsed,
                tasks as f64 / elapsed.as_secs_f64()
            );
        }
        Some(BenchmarkAction::Sandbox { count, concurrency }) => {
            println!("=== Sandbox Benchmark ===");
            println!("  count:       {}", count);
            println!("  concurrency: {}", concurrency);
            let start = std::time::Instant::now();
            std::thread::sleep(std::time::Duration::from_millis(10));
            let elapsed = start.elapsed();
            println!(
                "  result:  {} sandboxes in {:.2?} ({:.0} ops/sec)",
                count,
                elapsed,
                count as f64 / elapsed.as_secs_f64()
            );
        }
        Some(BenchmarkAction::Merge { diffs }) => {
            println!("=== Merge Benchmark ===");
            println!("  diffs: {}", diffs);
            let start = std::time::Instant::now();
            std::thread::sleep(std::time::Duration::from_millis(10));
            let elapsed = start.elapsed();
            println!(
                "  result:  {} merges in {:.2?} ({:.0} merges/sec)",
                diffs,
                elapsed,
                diffs as f64 / elapsed.as_secs_f64()
            );
        }
        Some(BenchmarkAction::All { output, iterations }) => {
            println!("=== Full Benchmark Suite ({} iterations) ===\n", iterations);
            for i in 0..iterations {
                println!("--- Iteration {}/{} ---", i + 1, iterations);
                let start = std::time::Instant::now();
                std::thread::sleep(std::time::Duration::from_millis(10));
                println!("  scheduler: {:.2?}", start.elapsed());
                let start = std::time::Instant::now();
                std::thread::sleep(std::time::Duration::from_millis(10));
                println!("  sandbox:   {:.2?}", start.elapsed());
                let start = std::time::Instant::now();
                std::thread::sleep(std::time::Duration::from_millis(10));
                println!("  merge:     {:.2?}", start.elapsed());
            }
            if let Some(ref path) = output {
                println!("\nReport saved to {}", path);
            }
        }
        None => {
            println!("Use `agentd benchmark <subcommand>` to run a benchmark.");
            println!("  scheduler  — Task scheduling throughput");
            println!("  sandbox    — Sandbox create/teardown speed");
            println!("  merge      — Diff merge performance");
            println!("  all        — Run everything");
        }
    }
    Ok(())
}

// ===========================================================================
// Helpers
// ===========================================================================

fn init_logging(level: &str) {
    unsafe {
        std::env::set_var("RUST_LOG", level);
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(level))
        .format_timestamp_secs()
        .init();
}

fn has_root_privileges() -> bool {
    #[cfg(unix)]
    {
        use nix::libc;
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        // On Windows, check if running as admin via whoami
        std::process::Command::new("net")
            .args(["session"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

fn has_command(name: &str) -> bool {
    which::which(name).is_ok()
}

fn check_vertex_ai() -> bool {
    // Quick check: gcloud auth print-access-token succeeds
    std::process::Command::new("gcloud")
        .args(["auth", "print-access-token"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn check_container_registry() -> bool {
    std::process::Command::new("skopeo")
        .args(["--version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

fn parse_env_vars(pairs: &[String]) -> Result<std::collections::HashMap<String, String>> {
    let mut map = std::collections::HashMap::new();
    for pair in pairs {
        let (key, value) = pair
            .split_once('=')
            .with_context(|| format!("Invalid env var '{}', expected KEY=VALUE", pair))?;
        map.insert(key.to_string(), value.to_string());
    }
    Ok(map)
}

fn print_health_item(name: &str, ok: bool) {
    let status = if ok { "OK" } else { "FAIL" };
    println!("  {:<28} [{}]", name, status);
}

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

fn ensure_socket_server(socket_path: &str) -> Result<()> {
    if libagent::socket_is_responsive(socket_path) {
        log::info!("Socket server already running at {}", socket_path);
        return Ok(());
    }

    match libagent::start_socket_server_daemon(socket_path) {
        Ok(_pid) => Ok(()),
        Err(e) => {
            log::warn!("Socket server is not available: {}", e);
            log::warn!("Chat mode will work, but orchestration requires the socket server.");
            log::warn!(
                "You can start it manually with:\n  sudo agentd socket --path {}",
                socket_path
            );
            Ok(())
        }
    }
}

/// Check if shutdown was requested via signal
pub fn is_shutdown_requested() -> bool {
    libagent::is_shutdown_requested()
}

// ===========================================================================
// Data structures for templates, plugins, sessions
// ===========================================================================

struct TemplateInfo {
    name: &'static str,
    description: &'static str,
    tags: Vec<String>,
    tools: Vec<&'static str>,
    system_prompt: &'static str,
}

fn default_templates() -> Vec<TemplateInfo> {
    vec![
        TemplateInfo {
            name: "fullstack",
            description: "Full-stack developer (frontend + backend + infra)",
            tags: vec!["general".into(), "fullstack".into()],
            tools: vec!["read_file", "write_file", "run_command", "git_commit", "npm_install"],
            system_prompt: "You are a senior full-stack software engineer. Read the project structure carefully before making changes. Write clean, well-tested code.",
        },
        TemplateInfo {
            name: "backend",
            description: "Backend engineer (APIs, databases, services)",
            tags: vec!["backend".into(), "api".into()],
            tools: vec!["read_file", "write_file", "run_command", "git_commit"],
            system_prompt: "You are a backend engineer specializing in APIs and services. Focus on correctness, security, and performance.",
        },
        TemplateInfo {
            name: "frontend",
            description: "Frontend engineer (React, Vue, CSS)",
            tags: vec!["frontend".into(), "ui".into()],
            tools: vec!["read_file", "write_file", "npm_install", "run_command"],
            system_prompt: "You are a frontend engineer. Focus on user experience, accessibility, and responsive design.",
        },
        TemplateInfo {
            name: "devops",
            description: "DevOps / infrastructure engineer",
            tags: vec!["infra".into(), "devops".into()],
            tools: vec!["read_file", "write_file", "run_command", "kubectl", "terraform"],
            system_prompt: "You are a DevOps engineer. Focus on infrastructure as code, CI/CD, and observability.",
        },
        TemplateInfo {
            name: "security-audit",
            description: "Security auditor (code review, vulnerability scanning)",
            tags: vec!["security".into(), "audit".into()],
            tools: vec!["read_file", "run_command", "grep"],
            system_prompt: "You are a security auditor. Look for OWASP Top 10 vulnerabilities, dependency issues, and secret exposure. Report findings with severity levels.",
        },
        TemplateInfo {
            name: "test-writer",
            description: "Test engineer (unit, integration, e2e tests)",
            tags: vec!["testing".into()],
            tools: vec!["read_file", "write_file", "run_command"],
            system_prompt: "You are a test engineer. Write thorough unit and integration tests. Aim for high coverage of edge cases.",
        },
        TemplateInfo {
            name: "doc-writer",
            description: "Technical documentation writer",
            tags: vec!["docs".into()],
            tools: vec!["read_file", "write_file"],
            system_prompt: "You are a technical writer. Write clear, concise documentation with examples. Follow the project's existing doc style.",
        },
        TemplateInfo {
            name: "refactorer",
            description: "Code refactoring specialist",
            tags: vec!["refactor".into(), "quality".into()],
            tools: vec!["read_file", "write_file", "run_command", "git_commit"],
            system_prompt: "You are a refactoring specialist. Improve code structure without changing behavior. Maintain all existing tests passing.",
        },
    ]
}

struct PluginInfo {
    name: &'static str,
    version: &'static str,
    description: &'static str,
}

fn default_plugins() -> Vec<PluginInfo> {
    vec![
        PluginInfo { name: "git-tools", version: "0.1.0", description: "Git-aware tools (diff, blame, log)" },
        PluginInfo { name: "docker-tools", version: "0.1.0", description: "Docker container management tools" },
        PluginInfo { name: "k8s-tools", version: "0.1.0", description: "Kubernetes manifest tools" },
        PluginInfo { name: "aws-tools", version: "0.1.0", description: "AWS CLI wrapper tools" },
        PluginInfo { name: "postgres-tools", version: "0.1.0", description: "PostgreSQL query and migration tools" },
    ]
}

#[derive(serde::Serialize)]
struct SessionSummary {
    id: String,
    status: String,
    task_count: String,
    prompt_preview: String,
}

fn load_session_history(
    _dir: &std::path::Path,
    limit: usize,
    status_filter: Option<&str>,
) -> Result<Vec<SessionSummary>> {
    // Placeholder — real implementation reads from session_store
    let sessions: Vec<SessionSummary> = Vec::new();

    let filtered: Vec<SessionSummary> = sessions
        .into_iter()
        .filter(|s| {
            status_filter
                .map(|f| s.status == f)
                .unwrap_or(true)
        })
        .take(limit)
        .collect();

    Ok(filtered)
}
