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
use mowis_host::{image, transport, vmm};

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
    /// Boot a VM with mowis-executor inside. Stays in foreground until killed.
    Boot {
        #[arg(long)]
        kernel: PathBuf,
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
        /// Command and args. Use `--` to separate flags from the command.
        #[arg(last = true)]
        argv: Vec<String>,
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
        Cmd::Boot {
            kernel,
            initrd,
            rootfs,
            memory_mb,
            vcpus,
            cid,
            port,
        } => {
            let backend = vmm::default_backend()?;
            let handle = backend
                .boot(vmm::VmConfig {
                    kernel,
                    initrd,
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
            argv,
        } => {
            if argv.is_empty() {
                anyhow::bail!("provide a command after `--`, e.g. `mowisd exec --cid 42 -- /bin/ls /`");
            }
            let conn = transport::connect(cid, port).await?;

            let sandbox_id = match conn
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
            tracing::info!(%sandbox_id, "created sandbox");

            let (cmd, args) = argv.split_first().context("empty argv")?;
            let mut rx = conn
                .call_streaming(Payload::Exec(ExecRequest {
                    sandbox_id: Some(sandbox_id.clone()),
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
            let _ = conn.call(Payload::DestroySandbox { sandbox_id }).await;
            std::process::exit(exit);
        }
    }
    Ok(())
}
