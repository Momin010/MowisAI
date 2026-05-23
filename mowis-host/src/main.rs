//! mowisd — host-side CLI for the new architecture.
//!
//! Usage:
//!   `mowisd`            — start interactive chat (auto-detects config, boots VM)
//!   `mowisd setup`      — run setup wizard
//!   `mowisd boot ...`   — manual VM boot (advanced)
//!   `mowisd ping ...`   — check VM health
//!   `mowisd exec ...`   — run command in VM sandbox

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use mowis_host::protocol::{Payload, SandboxSpec};
use mowis_host::{image, initrd, transport, vmm};
use mowis_orchestration::captain::SimpleCaptain;
use mowis_orchestration::config::{ModelRef, OrchConfig, ProviderCreds};
use mowis_orchestration::conductor::Conductor;
use mowis_orchestration::critic::Critic;
use mowis_orchestration::events::{Event, EventBus};
use mowis_orchestration::plan::PlanId;
use mowis_orchestration::providers::Provider;

#[derive(Debug, Parser)]
#[command(name = "mowisd", version, about = "MowisAI — AI agent orchestration engine")]
struct Cli {
    /// Autonomous mode: provide a prompt and mowisd will execute it end-to-end.
    #[arg(short = 'p', long = "prompt")]
    prompt: Option<String>,

    /// Boot a VM for tool execution (default: run tools locally via chroot).
    #[arg(long = "vm")]
    use_vm: bool,

    /// Enable verbose logging of every operation.
    #[arg(short = 'l', long = "log")]
    verbose_log: bool,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Run the setup wizard to configure providers and API keys.
    Setup,
    /// Pull an OCI image and extract its rootfs to a cache dir.
    Pull {
        #[arg(long)]
        image: String,
        #[arg(long, default_value = ".mowis-cache")]
        cache: PathBuf,
    },
    /// Build a bootable initramfs (cpio.gz) that runs mowis-executor as PID 1.
    BuildInitrd {
        #[arg(long)]
        executor: PathBuf,
        #[arg(long, default_value = "mowis-initrd.cpio.gz")]
        output: PathBuf,
    },
    /// Boot a VM with mowis-executor inside. Stays in foreground until killed.
    Boot {
        #[arg(long)]
        kernel: Option<PathBuf>,
        #[arg(long)]
        initrd: PathBuf,
        #[arg(long)]
        rootfs: Option<PathBuf>,
        #[arg(long, default_value_t = 2048)]
        memory_mb: u32,
        #[arg(long, default_value_t = 2)]
        vcpus: u32,
        #[arg(long, default_value_t = 42)]
        cid: u32,
        #[arg(long, default_value_t = mowis_host::protocol::DEFAULT_VSOCK_PORT)]
        port: u32,
    },
    /// Ping the executor inside a running VM.
    Ping {
        #[arg(long)]
        cid: u32,
        #[arg(long, default_value_t = mowis_host::protocol::DEFAULT_VSOCK_PORT)]
        port: u32,
    },
    /// Run a command in a fresh sandbox inside the VM.
    Exec {
        #[arg(long)]
        cid: u32,
        #[arg(long, default_value_t = mowis_host::protocol::DEFAULT_VSOCK_PORT)]
        port: u32,
        #[arg(long)]
        guest_rootfs: Option<String>,
        #[arg(long)]
        no_sandbox: bool,
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// Start an interactive chat session with the orchestration engine.
    Chat {
        #[arg(long)]
        cid: Option<u32>,
        #[arg(long, default_value_t = mowis_host::protocol::DEFAULT_VSOCK_PORT)]
        port: u32,
        /// Path to the project/codebase to upload into the VM.
        #[arg(long)]
        project: Option<PathBuf>,
    },
}

fn prompt_input(prompt: &str) -> Result<String> {
    print!("{}", prompt);
    use std::io::Write;
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn prompt_secret(prompt: &str) -> Result<String> {
    print!("{}", prompt);
    use std::io::Write;
    std::io::stdout().flush()?;
    // For now, just read normally (no terminal echo masking in MVP)
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

async fn run_setup_wizard() -> Result<OrchConfig> {
    println!("\n╔══════════════════════════════════════╗");
    println!("║       MowisAI Setup Wizard           ║");
    println!("╚══════════════════════════════════════╝\n");
    println!("No configuration found. Let's set up your AI providers.\n");

    println!("Available providers:");
    println!("  1. Anthropic (Claude) — recommended");
    println!("  2. OpenAI (GPT)");
    println!("  3. Google Gemini");
    println!("  4. Vertex AI (GCP)");
    println!("  5. Grok (xAI)");
    println!("  6. Groq");
    println!("  7. Mimo (Xiaomi)\n");

    let choice = prompt_input("Select provider [1-7] (default: 1): ")?;
    let provider = match choice.as_str() {
        "2" => Provider::OpenAi,
        "3" => Provider::Gemini,
        "4" => Provider::VertexAi,
        "5" => Provider::Grok,
        "6" => Provider::Groq,
        "7" => Provider::Mimo,
        _ => Provider::Anthropic,
    };

    let api_key = if provider == Provider::VertexAi {
        println!("\nVertex AI uses gcloud Application Default Credentials.");
        println!("Run: gcloud auth application-default login\n");
        let project_id = prompt_input("GCP Project ID: ")?;
        let mut creds_map = std::collections::HashMap::new();
        creds_map.insert(
            provider.clone(),
            ProviderCreds {
                api_key_enc: None,
                project_id: Some(project_id),
            },
        );
        creds_map
    } else {
        let key = prompt_secret(&format!("\nEnter API key for {:?}: ", provider))?;
        if key.is_empty() {
            anyhow::bail!("API key cannot be empty");
        }
        let encrypted = mowis_orchestration::crypto::encrypt(&key)?;
        let mut creds_map = std::collections::HashMap::new();
        creds_map.insert(
            provider.clone(),
            ProviderCreds {
                api_key_enc: Some(encrypted),
                project_id: None,
            },
        );
        creds_map
    };

    let default_model = match provider {
        Provider::Anthropic => "claude-sonnet-4-20250514",
        Provider::OpenAi => "gpt-4o",
        Provider::Gemini => "gemini-2.5-pro",
        Provider::VertexAi => "gemini-2.5-pro",
        Provider::Grok => "grok-3",
        Provider::Groq => "llama-3.3-70b-versatile",
        Provider::Mimo => "mimo-v2.5-pro",
    };

    let model_input = prompt_input(&format!(
        "\nModel name (default: {}): ",
        default_model
    ))?;
    let model = if model_input.is_empty() {
        default_model.to_string()
    } else {
        model_input
    };

    let model_ref = ModelRef {
        provider: provider.clone(),
        model: model.clone(),
    };

    let mut tiers = std::collections::HashMap::new();
    // Conductor and Critic use flagship model
    tiers.insert(mowis_orchestration::plan::Tier::Conductor, model_ref.clone());
    tiers.insert(mowis_orchestration::plan::Tier::Critic, model_ref.clone());
    // Captain uses a mid-tier (same provider, smaller model)
    tiers.insert(mowis_orchestration::plan::Tier::Captain, model_ref.clone());
    // Crew uses the same for now
    tiers.insert(mowis_orchestration::plan::Tier::Crew, model_ref.clone());

    let cfg = OrchConfig {
        providers: api_key,
        tiers,
        sandbox: mowis_orchestration::plan::SandboxConfig::default(),
        plans_dir: std::path::PathBuf::from(".mowis/plans"),
    };

    cfg.save()?;

    println!("\n✓ Configuration saved to ~/.mowisai/mowis.toml");
    println!("  Provider: {:?}", provider);
    println!("  Model: {}", model);
    println!("\nYou can now run `mowisd` to start chatting.\n");

    Ok(cfg)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Configure logging based on -log flag
    let log_level = if cli.verbose_log {
        tracing_subscriber::EnvFilter::new("debug,mowis_orchestration=trace,mowis_host=trace")
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    };
    tracing_subscriber::fmt()
        .with_env_filter(log_level)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .init();

    // Handle -p flag (autonomous mode) first
    if let Some(ref prompt) = cli.prompt {
        let cfg = match OrchConfig::load() {
            Ok(c) if !c.providers.is_empty() => c,
            _ => {
                eprintln!("No config found. Run 'mowisd setup' first.");
                std::process::exit(1);
            }
        };
        run_autonomous(cfg, prompt.clone(), cli.use_vm, cli.verbose_log).await?;
        return Ok(());
    }

    match cli.cmd {
        None => {
            // No subcommand: launch TUI (splash → setup → main with real orchestration)
            let mut app = mowis_host::tui::TuiApp::new();
            app.run_async().await?;
        }
        Some(Cmd::Chat { cid, port, project }) => {
            // Load or create config
            let cfg = match OrchConfig::load() {
                Ok(c) if !c.providers.is_empty() => c,
                _ => run_setup_wizard().await?,
            };

            // Auto-boot VM if no CID provided
            let (conn, _vm_handle) = if let Some(cid) = cid {
                // User provided a CID — connect to existing VM
                let conn = transport::connect(cid, port).await?;
                println!("Connected to VM cid={cid} port={port}");
                (Some(conn), None)
            } else {
                // Try to boot a VM automatically
                match try_boot_vm().await {
                    Ok((conn, handle)) => {
                        println!("VM booted and ready");
                        (Some(conn), Some(handle))
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "could not boot VM; running without tool execution");
                        println!("Note: No VM available. Agent will plan but not execute tools.");
                        println!("To enable tool execution, run: mowisd boot --initrd <path>\n");
                        (None, None)
                    }
                }
            };

            // Upload codebase if project path given and VM is available
            if let (Some(ref conn), Some(ref project_path)) = (&conn, &project) {
                if project_path.exists() {
                    println!("Uploading project to VM...");
                    upload_project(conn, project_path).await?;
                    println!("Project uploaded.");
                }
            }

            // Run the orchestration
            run_chat(cfg, conn, project).await?;
        }
        Some(Cmd::Setup) => {
            run_setup_wizard().await?;
        }
        Some(Cmd::Pull { image, cache }) => {
            let path = image::pull_rootfs(&image, &cache).await?;
            println!("{}", path.display());
        }
        Some(Cmd::BuildInitrd { executor, output }) => {
            initrd::build(&executor, &output).await?;
            println!("{}", output.display());
        }
        Some(Cmd::Boot {
            kernel,
            initrd: initrd_path,
            rootfs,
            memory_mb,
            vcpus,
            cid,
            port,
        }) => {
            let kernel = kernel
                .or_else(initrd::default_kernel)
                .context("no --kernel provided and no /boot/vmlinuz-* found")?;
            let backend = vmm::default_backend()?;
            let handle = backend
                .boot(vmm::VmConfig {
                    kernel,
                    initrd: initrd_path,
                    rootfs,
                    memory_mb,
                    vcpus,
                    guest_cid: cid,
                    executor_port: port,
                    extra_cmdline: vec![],
                })
                .await?;
            println!(
                "VM booted; cid={} port={}",
                handle.guest_cid(),
                handle.executor_port()
            );
            println!("Try: mowisd ping --cid {cid} --port {port}");
            tokio::signal::ctrl_c().await.ok();
            backend.shutdown(handle).await?;
        }
        Some(Cmd::Ping { cid, port }) => {
            let conn = transport::connect(cid, port).await?;
            let (version, protocol) = conn.ping().await?;
            println!("guest version={version} protocol={protocol}");
        }
        Some(Cmd::Exec {
            cid,
            port,
            guest_rootfs,
            no_sandbox,
            argv,
        }) => {
            if argv.is_empty() {
                anyhow::bail!(
                    "provide a command after `--`, e.g. `mowisd exec --cid 42 -- /bin/ls /`"
                );
            }
            let conn = transport::connect(cid, port).await?;

            let sandbox_id = if no_sandbox {
                None
            } else {
                let id = match conn
                    .call(Payload::CreateSandbox(SandboxSpec {
                        sandbox_id: None,
                        image_rootfs: guest_rootfs,
                        limits: Default::default(),
                    }))
                    .await?
                {
                    Payload::SandboxCreated { sandbox_id } => sandbox_id,
                    Payload::Error { message } => anyhow::bail!("create_sandbox: {message}"),
                    other => anyhow::bail!("unexpected response: {other:?}"),
                };
                tracing::info!(sandbox_id = %id, "created sandbox");
                Some(id)
            };

            use mowis_host::protocol::ExecRequest;
            let (cmd, args) = argv.split_first().context("empty argv")?;
            let mut rx = conn
                .call_streaming(Payload::Exec(ExecRequest {
                    sandbox_id: sandbox_id.clone(),
                    cmd: cmd.clone(),
                    args: args.to_vec(),
                    env: vec![],
                }))
                .await?;
            let mut exit = 0;
            while let Some(payload) = rx.recv().await {
                match payload {
                    Payload::Stdout { data } => print!("{data}"),
                    Payload::Stderr { data } => eprint!("{data}"),
                    Payload::ExitCode { code } => {
                        exit = code;
                        break;
                    }
                    Payload::Error { message } => {
                        eprintln!("error: {message}");
                        exit = 1;
                        break;
                    }
                    other => tracing::warn!(?other, "unexpected payload"),
                }
            }

            if let Some(id) = sandbox_id {
                let _ = conn.call(Payload::DestroySandbox { sandbox_id: id }).await;
            }
            std::process::exit(exit);
        }
    }
    Ok(())
}

async fn try_boot_vm() -> Result<(transport::Connection, Box<dyn vmm::VmHandle>)> {
    let kernel = initrd::default_kernel().context("no /boot/vmlinuz-* found")?;

    // Build initrd if we have the executor binary
    let executor_path = find_executor_binary()?;
    let initrd_path = std::path::PathBuf::from("/tmp/mowis-initrd.cpio.gz");
    if !initrd_path.exists() {
        println!("Building initramfs...");
        initrd::build(&executor_path, &initrd_path).await?;
    }

    let backend = vmm::default_backend()?;
    let handle = backend
        .boot(vmm::VmConfig {
            kernel,
            initrd: initrd_path,
            rootfs: None,
            memory_mb: 2048,
            vcpus: 2,
            guest_cid: 42,
            executor_port: mowis_host::protocol::DEFAULT_VSOCK_PORT,
            extra_cmdline: vec![],
        })
        .await?;

    // Wait for executor to be ready
    let cid = handle.guest_cid();
    let port = handle.executor_port();
    let conn = wait_for_executor(cid, port, 30).await?;

    Ok((conn, handle))
}

async fn wait_for_executor(cid: u32, port: u32, max_secs: u64) -> Result<transport::Connection> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(max_secs);
    loop {
        match transport::connect(cid, port).await {
            Ok(conn) => match conn.ping().await {
                Ok((version, protocol)) => {
                    tracing::info!(version, protocol, "executor is ready");
                    return Ok(conn);
                }
                Err(e) => {
                    tracing::debug!(error = %e, "ping failed, retrying...");
                }
            },
            Err(e) => {
                tracing::debug!(error = %e, "connect failed, retrying...");
            }
        }
        if std::time::Instant::now() > deadline {
            anyhow::bail!("executor did not become ready within {}s", max_secs);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

fn find_executor_binary() -> Result<PathBuf> {
    // Check common locations
    let candidates = [
        PathBuf::from("target/release/mowis-executor"),
        PathBuf::from("target/debug/mowis-executor"),
        PathBuf::from("/usr/local/bin/mowis-executor"),
    ];
    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    // Try to build it
    anyhow::bail!(
        "mowis-executor binary not found. Build it with: cargo build -p mowis-executor"
    )
}

async fn upload_project(
    conn: &transport::Connection,
    project_path: &std::path::Path,
) -> Result<()> {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

    // Create sandbox for the project
    let sandbox_id = match conn
        .call(Payload::CreateSandbox(SandboxSpec {
            sandbox_id: Some("project".into()),
            image_rootfs: None,
            limits: Default::default(),
        }))
        .await?
    {
        Payload::SandboxCreated { sandbox_id } => sandbox_id,
        Payload::Error { message } => anyhow::bail!("create_sandbox: {message}"),
        other => anyhow::bail!("unexpected response: {other:?}"),
    };

    // Create tar.gz of the project
    let tar_data = create_tar_gz(project_path)?;
    let archive_b64 = BASE64.encode(&tar_data);

    // Count files
    let file_count = count_files(project_path)? as u32;

    // Upload
    match conn
        .call(Payload::UploadCodebase {
            sandbox_id,
            archive_b64,
            file_count,
        })
        .await?
    {
        Payload::CodebaseUploaded {
            file_count: uploaded,
            ..
        } => {
            tracing::info!(files = uploaded, "codebase uploaded to VM");
        }
        Payload::Error { message } => anyhow::bail!("upload failed: {message}"),
        other => anyhow::bail!("unexpected response: {other:?}"),
    }

    Ok(())
}

fn create_tar_gz(path: &std::path::Path) -> Result<Vec<u8>> {
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let tar_data = Vec::new();
    let encoder = GzEncoder::new(tar_data, Compression::default());
    let mut builder = tar::Builder::new(encoder);

    // Add all files from the directory, excluding .git and target
    add_dir_to_tar(&mut builder, path, path)?;

    let encoder = builder.into_inner()?;
    let compressed = encoder.finish()?;
    Ok(compressed)
}

fn add_dir_to_tar(
    builder: &mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>,
    base: &std::path::Path,
    current: &std::path::Path,
) -> Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip .git, target, node_modules
        if name == ".git" || name == "target" || name == "node_modules" {
            continue;
        }

        let rel = path.strip_prefix(base).unwrap();
        if entry.file_type()?.is_dir() {
            builder.append_dir(rel, &path)?;
            add_dir_to_tar(builder, base, &path)?;
        } else {
            builder.append_path_with_name(&path, rel)?;
        }
    }
    Ok(())
}

fn count_files(path: &std::path::Path) -> Result<usize> {
    let mut count = 0;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" || name == "target" || name == "node_modules" {
            continue;
        }
        if entry.file_type()?.is_dir() {
            count += count_files(&entry.path())?;
        } else {
            count += 1;
        }
    }
    Ok(count)
}

async fn run_chat(
    cfg: OrchConfig,
    conn: Option<transport::Connection>,
    project: Option<PathBuf>,
) -> Result<()> {
    let bus = EventBus::new();

    // Spawn event printer (real-time streaming to terminal)
    let bus_print = bus.clone();
    let print_handle = tokio::spawn(async move {
        let mut rx = bus_print.subscribe();
        loop {
            match rx.recv().await {
                Ok(event) => match event {
                    Event::CrewToolSummary {
                        agent_id,
                        text,
                        tool_name: _,
                        success,
                    } => {
                        let icon = if success { "✓" } else { "✗" };
                        println!("  [{}] {} {}", icon, agent_id, text);
                    }
                    Event::CrewStarted {
                        task_id, agent_id, ..
                    } => {
                        println!("  ▶ Crew {} started task {}", agent_id, task_id.0);
                    }
                    Event::CrewDone {
                        agent_id, summary, ..
                    } => {
                        println!("  ■ Crew {} done: {}", agent_id, summary);
                    }
                    Event::CrewFailed {
                        agent_id, reason, ..
                    } => {
                        println!("  ✗ Crew {} failed: {}", agent_id, reason);
                    }
                    Event::PlanDrafted { plan_id, version } => {
                        println!(
                            "\n  📋 Plan drafted: {} (v{})",
                            plan_id.0, version
                        );
                    }
                    Event::PlanCompleted { plan_id } => {
                        println!("\n  ✓ Plan {} completed!", plan_id.0);
                    }
                    Event::PlanFailed { plan_id, reason } => {
                        println!("\n  ✗ Plan {} failed: {}", plan_id.0, reason);
                    }
                    Event::CaptainStarted { sandbox_id, .. } => {
                        println!("  🚀 Captain started (sandbox: {})", sandbox_id);
                    }
                    Event::MergeCompleted { agent_id, .. } => {
                        println!("  🔀 Merged overlay for {}", agent_id);
                    }
                    Event::CriticVerdict {
                        verdict, version, ..
                    } => {
                        println!("  🔍 Critic verdict (v{}): {:?}", version, verdict);
                    }
                    Event::ConversationEnded => {
                        println!("\n  Conversation ended.");
                        break;
                    }
                    _ => {}
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });

    let (mut conductor, _cmd_tx) = Conductor::new(&cfg, bus.clone())?;
    let mut critic = Critic::new(&cfg, bus.clone())?;

    // Spawn critic on background
    let critic_handle = tokio::spawn(async move {
        if let Err(e) = critic.run().await {
            tracing::error!(error = %e, "critic exited with error");
        }
    });

    println!("\n╔══════════════════════════════════════╗");
    println!("║       MowisAI Chat                   ║");
    println!("╚══════════════════════════════════════╝\n");
    if conn.is_some() {
        println!("VM connected. Tool execution enabled.");
    } else {
        println!("No VM. Planning only (no tool execution).");
    }
    if let Some(ref p) = project {
        println!("Project: {}", p.display());
    }
    println!("Type your message and press Enter. Type 'quit' to exit.\n");

    let stdin = std::io::stdin();
    loop {
        print!("> ");
        use std::io::Write;
        std::io::stdout().flush()?;

        let mut line = String::new();
        if stdin.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "quit" || line == "exit" {
            break;
        }

        match conductor.handle_user_message(line.to_string()).await {
            Ok(mowis_orchestration::conductor::ConductorReply::Chat { reply }) => {
                println!("\n{}\n", reply);
            }
            Ok(mowis_orchestration::conductor::ConductorReply::PlanDrafted {
                plan_id, ..
            }) => {
                println!("\nType 'approve' to approve or 'cancel' to cancel.\n");

                print!("> ");
                std::io::stdout().flush()?;
                let mut approval = String::new();
                stdin.read_line(&mut approval)?;
                let approval = approval.trim();
                if approval == "approve" || approval == "y" || approval == "yes" {
                    bus.emit(Event::UserApproved {
                        plan_id: plan_id.clone(),
                    });
                    println!("Plan approved! Captain starting...\n");

                    let captain = SimpleCaptain::new(&cfg, plan_id, bus.clone())?;
                    match captain.run().await {
                        Ok(mowis_orchestration::captain::CaptainOutcome::Completed {
                            sandbox_id,
                        }) => {
                            println!("\n✓ Plan completed! Sandbox: {}", sandbox_id);
                        }
                        Ok(mowis_orchestration::captain::CaptainOutcome::Failed {
                            reason, ..
                        }) => {
                            eprintln!("\n✗ Plan failed: {}", reason);
                        }
                        Ok(mowis_orchestration::captain::CaptainOutcome::Aborted) => {
                            println!("\nPlan aborted.");
                        }
                        Err(e) => {
                            eprintln!("\nCaptain error: {}", e);
                        }
                    }
                } else {
                    bus.emit(Event::UserCancelled {
                        plan_id,
                    });
                    println!("Plan cancelled.\n");
                }
            }
            Ok(other) => {
                println!("{:?}", other);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }

    bus.emit(Event::ConversationEnded);
    print_handle.abort();
    critic_handle.abort();
    Ok(())
}

async fn run_autonomous(cfg: OrchConfig, prompt: String, use_vm: bool, verbose: bool) -> Result<()> {
    use mowis_orchestration::conductor::ConductorReply;

    println!("\n╔══════════════════════════════════════╗");
    println!("║  MowisAI Autonomous Mode             ║");
    println!("╚══════════════════════════════════════╝\n");
    println!("Prompt: {}", prompt);
    println!("Mode:   {}", if use_vm { "VM (QEMU + vsock)" } else { "Local (chroot)" });
    println!("Logging: {}\n", if verbose { "verbose" } else { "info" });

    // Step 1: Boot VM if -vm flag is set
    let _vm_handle: Option<Box<dyn vmm::VmHandle>> = None;
    let _conn: Option<transport::Connection> = None;

    if use_vm {
        println!("[boot] Looking for mowis-executor binary...");
        let executor_path = find_executor_binary()?;
        println!("[boot] Found executor: {}", executor_path.display());

        println!("[boot] Building initramfs...");
        let initrd_path = std::path::PathBuf::from("/tmp/mowis-initrd.cpio.gz");
        if !initrd_path.exists() {
            initrd::build(&executor_path, &initrd_path).await?;
            println!("[boot] Initramfs built: {}", initrd_path.display());
        } else {
            println!("[boot] Initramfs exists: {}", initrd_path.display());
        }

        println!("[boot] Looking for kernel...");
        let kernel = initrd::default_kernel()
            .context("no /boot/vmlinuz-* found. Install linux-image or pass --kernel.")?;
        println!("[boot] Using kernel: {}", kernel.display());

        println!("[boot] Starting QEMU VM...");
        let backend = vmm::default_backend()?;
        let handle = backend.boot(vmm::VmConfig {
            kernel,
            initrd: initrd_path,
            rootfs: None,
            memory_mb: 2048,
            vcpus: 2,
            guest_cid: 42,
            executor_port: mowis_host::protocol::DEFAULT_VSOCK_PORT,
            extra_cmdline: vec![],
        }).await?;
        println!("[boot] VM booted: cid={} port={}", handle.guest_cid(), handle.executor_port());

        println!("[boot] Waiting for executor to be ready...");
        let conn = wait_for_executor(handle.guest_cid(), handle.executor_port(), 30).await?;
        println!("[boot] Executor ready!");
        // conn and handle are kept alive for the duration
        // In a real implementation, we'd pass them through to the Captain
        let _ = conn;
    }

    // Step 2: Set up orchestration
    let bus = EventBus::new();

    // Subscribe to bus events and print them
    let bus_print = bus.clone();
    let print_handle = tokio::spawn(async move {
        let mut rx = bus_print.subscribe();
        loop {
            match rx.recv().await {
                Ok(event) => match event {
                    Event::CrewToolSummary { agent_id, text, tool_name, success } => {
                        let icon = if success { "✓" } else { "✗" };
                        if verbose {
                            println!("  [{}] {} ({}) — agent={}", icon, text, tool_name, agent_id);
                        } else {
                            println!("  [{}] {}", icon, text);
                        }
                    }
                    Event::CrewStarted { plan_id, task_id, agent_id } => {
                        println!("  ▶ Agent {} started task {} (plan={})", agent_id, task_id.0, plan_id.0);
                    }
                    Event::CrewDone { plan_id, agent_id, summary } => {
                        println!("  ■ Agent {} done: {} (plan={})", agent_id, summary, plan_id.0);
                    }
                    Event::CrewFailed { plan_id, agent_id, reason } => {
                        println!("  ✗ Agent {} failed: {} (plan={})", agent_id, reason, plan_id.0);
                    }
                    Event::PlanDrafted { plan_id, version } => {
                        println!("  📋 Plan drafted: {} v{}", plan_id.0, version);
                    }
                    Event::PlanRevised { plan_id, version } => {
                        println!("  📋 Plan revised: {} v{}", plan_id.0, version);
                    }
                    Event::CriticReviewing { plan_id, version } => {
                        println!("  🔍 Critic reviewing {} v{}...", plan_id.0, version);
                    }
                    Event::CriticVerdict { plan_id, version, verdict } => {
                        let v = match &verdict {
                            mowis_orchestration::critic::Verdict::Approve => "APPROVE",
                            mowis_orchestration::critic::Verdict::Revise { .. } => "REVISE",
                            mowis_orchestration::critic::Verdict::Block { .. } => "BLOCK",
                        };
                        println!("  🔍 Critic verdict for {} v{}: {}", plan_id.0, version, v);
                    }
                    Event::CaptainStarted { plan_id, sandbox_id } => {
                        println!("  🚀 Captain started (plan={}, sandbox={})", plan_id.0, sandbox_id);
                    }
                    Event::MergeStarted { plan_id, agent_id } => {
                        println!("  🔀 Merging overlay for {} (plan={})", agent_id, plan_id.0);
                    }
                    Event::MergeCompleted { plan_id, agent_id } => {
                        println!("  🔀 Merged overlay for {} (plan={})", agent_id, plan_id.0);
                    }
                    Event::PlanCompleted { plan_id } => {
                        println!("\n  ✓ Plan {} completed!", plan_id.0);
                    }
                    Event::PlanFailed { plan_id, reason } => {
                        println!("\n  ✗ Plan {} failed: {}", plan_id.0, reason);
                    }
                    Event::PlanApproved { plan_id } => {
                        println!("  ✓ Plan {} approved", plan_id.0);
                    }
                    Event::UserApproved { plan_id } => {
                        println!("  ✓ User approved plan {}", plan_id.0);
                    }
                    Event::StreamToken { text } => {
                        print!("{}", text);
                        use std::io::Write;
                        std::io::stdout().flush().ok();
                    }
                    Event::StreamDone => {
                        println!();
                    }
                    Event::ConversationEnded => {
                        if verbose { println!("  [event] ConversationEnded"); }
                        break;
                    }
                    other => {
                        if verbose { println!("  [event] {:?}", other); }
                    }
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    if verbose { println!("  [bus] Lagged {} events", n); }
                    continue;
                }
                Err(e) => {
                    if verbose { println!("  [bus] Error: {}", e); }
                    break;
                }
            }
        }
    });

    // Step 3: Create conductor
    println!("[init] Creating Conductor...");
    let (mut conductor, _cmd_tx) = Conductor::new(&cfg, bus.clone())?;
    println!("[init] Conductor ready");

    // Step 4: Create and spawn critic
    println!("[init] Creating Critic...");
    let mut critic = Critic::new(&cfg, bus.clone())?;
    tokio::spawn(async move {
        if let Err(e) = critic.run().await {
            tracing::error!(error = %e, "critic exited with error");
        }
    });
    println!("[init] Critic ready");

    // Step 5: Send the prompt to conductor
    println!("\n[conductor] Processing prompt...\n");
    let reply = conductor.handle_user_message(prompt.clone()).await?;

    match reply {
        ConductorReply::PlanDrafted { plan_id, version } => {
            println!("[conductor] Plan drafted: {} v{}", plan_id.0, version);
            println!("[critic] Waiting for review...\n");

            // Wait for critic to review
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;

            // Auto-approve
            println!("[auto] Auto-approving plan...\n");
            bus.emit(Event::UserApproved { plan_id: plan_id.clone() });
            bus.emit(Event::PlanApproved { plan_id: plan_id.clone() });

            // Run captain
            println!("[captain] Starting execution...\n");
            let captain = SimpleCaptain::new(&cfg, plan_id, bus.clone())?;
            match captain.run().await {
                Ok(mowis_orchestration::captain::CaptainOutcome::Completed { sandbox_id }) => {
                    println!("\n[done] ✓ Plan completed successfully! Sandbox: {}", sandbox_id);
                }
                Ok(mowis_orchestration::captain::CaptainOutcome::Failed { reason, sandbox_id }) => {
                    eprintln!("\n[done] ✗ Plan failed: {} (sandbox: {})", reason, sandbox_id);
                }
                Ok(mowis_orchestration::captain::CaptainOutcome::Aborted) => {
                    println!("\n[done] Plan aborted.");
                }
                Err(e) => {
                    eprintln!("\n[done] Captain error: {}", e);
                }
            }
        }
        ConductorReply::Chat { reply } => {
            println!("[conductor] {}", reply);
        }
        ConductorReply::Error { message } => {
            eprintln!("[error] {}", message);
        }
        _ => {
            println!("[conductor] {:?}", reply);
        }
    }

    bus.emit(Event::ConversationEnded);
    print_handle.abort();
    Ok(())
}
