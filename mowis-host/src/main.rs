//! mowisd — host-side CLI for the new architecture.
//!
//! Subcommands for the MVP focus on validating the host<->guest path:
//!   - `mowisd pull --image <ref>`        pull an OCI image, extract rootfs
//!   - `mowisd boot --kernel ... --initrd ...`  boot a VM with the executor
//!   - `mowisd ping --cid <n>`            sanity-check vsock to a running VM
//!   - `mowisd exec --cid <n> <cmd> ...`  run a command in a fresh sandbox
//!
//! Boot + exec are intentionally separate so you can re-run `exec` against an
//! already-running VM without paying boot cost each time. Once the path is
//! solid, the orchestrate / chat surface from `agentd` will move here.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use mowis_host::protocol::{ExecRequest, Payload, SandboxSpec};
use mowis_host::{image, initrd, transport, vmm};
use mowis_orchestration::conductor::Conductor;
use mowis_orchestration::captain::Captain;
use mowis_orchestration::critic::Critic;
use mowis_orchestration::config::OrchConfig;
use mowis_orchestration::events::{Event, EventBus};
use mowis_orchestration::plan::PlanId;

#[derive(Debug, Parser)]
#[command(name = "mowisd", version, about = "MowisAI host-side daemon (new architecture)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Pull an OCI image and extract its rootfs to a cache dir.
    Pull {
        #[arg(long)]
        image: String,
        #[arg(long, default_value = ".mowis-cache")]
        cache: PathBuf,
    },
    /// Build a bootable initramfs (cpio.gz) that runs mowis-executor as PID 1.
    BuildInitrd {
        /// Path to the mowis-executor binary to embed.
        #[arg(long)]
        executor: PathBuf,
        /// Where to write the initramfs.
        #[arg(long, default_value = "mowis-initrd.cpio.gz")]
        output: PathBuf,
    },
    /// Boot a VM with mowis-executor inside. Stays in foreground until killed.
    Boot {
        /// Linux kernel image. Defaults to the host's running kernel
        /// (/boot/vmlinuz-$(uname -r)) when not specified.
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
        /// Optional rootfs path (inside the guest) to use as overlay lower layer.
        #[arg(long)]
        guest_rootfs: Option<String>,
        /// Skip sandbox creation; run directly in the executor's own
        /// namespace. Useful for loopback tests where the sandbox would be an
        /// empty tmpfs with no binaries.
        #[arg(long)]
        no_sandbox: bool,
        /// Command and args. Use `--` to separate flags from the command.
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// Start an interactive chat session with the orchestration engine.
    Chat {
        /// Optional vsock CID to connect to a running VM for tool execution.
        #[arg(long)]
        cid: Option<u32>,
        #[arg(long, default_value_t = mowis_host::protocol::DEFAULT_VSOCK_PORT)]
        port: u32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Pull { image, cache } => {
            let path = image::pull_rootfs(&image, &cache).await?;
            println!("{}", path.display());
        }
        Cmd::BuildInitrd { executor, output } => {
            initrd::build(&executor, &output).await?;
            println!("{}", output.display());
        }
        Cmd::Boot {
            kernel,
            initrd: initrd_path,
            rootfs,
            memory_mb,
            vcpus,
            cid,
            port,
        } => {
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
            println!("VM booted; cid={} port={}", handle.guest_cid(), handle.executor_port());
            println!("Try: mowisd ping --cid {cid} --port {port}");
            // Hold the handle to keep the VM alive until ctrl-c.
            tokio::signal::ctrl_c().await.ok();
            backend.shutdown(handle).await?;
        }
        Cmd::Ping { cid, port } => {
            let conn = transport::connect(cid, port).await?;
            let (version, protocol) = conn.ping().await?;
            println!("guest version={version} protocol={protocol}");
        }
        Cmd::Exec {
            cid,
            port,
            guest_rootfs,
            no_sandbox,
            argv,
        } => {
            if argv.is_empty() {
                anyhow::bail!("provide a command after `--`, e.g. `mowisd exec --cid 42 -- /bin/ls /`");
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

            // Best-effort cleanup; ignore errors.
            if let Some(id) = sandbox_id {
                let _ = conn.call(Payload::DestroySandbox { sandbox_id: id }).await;
            }
            std::process::exit(exit);
        }
        Cmd::Chat { cid, port } => {
            let cfg = OrchConfig::load().unwrap_or_default();
            let bus = EventBus::new();

            let mut conductor = Conductor::new(&cfg, bus.clone())?;
            let mut critic = Critic::new(&cfg, bus.clone())?;

            // Spawn critic on background
            let critic_handle = tokio::spawn(async move {
                if let Err(e) = critic.run().await {
                    tracing::error!(error = %e, "critic exited with error");
                }
            });

            // Spawn event logger
            let bus_clone = bus.clone();
            let logger_handle = tokio::spawn(async move {
                let mut rx = bus_clone.subscribe();
                loop {
                    match rx.recv().await {
                        Ok(event) => {
                            tracing::info!(?event, "orchestration event");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            });

            println!("MowisAI chat session started. Type your message and press Enter.");
            println!("Type 'quit' or 'exit' to leave.\n");

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
                    Ok(mowis_orchestration::conductor::ConductorAction::Chat { reply }) => {
                        println!("\n{}\n", reply);
                    }
                    Ok(mowis_orchestration::conductor::ConductorAction::PlanDrafted { plan_id }) => {
                        println!("\nPlan drafted: {}", plan_id.0);
                        println!("The critic will review it now.");
                        println!("Type 'approve' to approve or 'cancel' to cancel.\n");

                        // Wait for user approval
                        print!("> ");
                        std::io::stdout().flush()?;
                        let mut approval = String::new();
                        stdin.read_line(&mut approval)?;
                        let approval = approval.trim();
                        if approval == "approve" || approval == "y" || approval == "yes" {
                            bus.emit(Event::UserApproved { plan_id: plan_id.clone() });
                            println!("Plan approved! Spawning Captain...\n");

                            let captain = Captain::new(&cfg, plan_id, bus.clone())?;
                            match captain.run().await {
                                Ok(mowis_orchestration::captain::CaptainOutcome::Completed { sandbox_id }) => {
                                    println!("Plan completed! Sandbox: {}", sandbox_id);
                                }
                                Ok(mowis_orchestration::captain::CaptainOutcome::Failed { reason, .. }) => {
                                    eprintln!("Plan failed: {}", reason);
                                }
                                Ok(mowis_orchestration::captain::CaptainOutcome::Aborted) => {
                                    println!("Plan aborted.");
                                }
                                Err(e) => {
                                    eprintln!("Captain error: {}", e);
                                }
                            }
                        } else {
                            bus.emit(Event::UserCancelled { plan_id });
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

            critic_handle.abort();
            logger_handle.abort();
        }
    }
    Ok(())
}
