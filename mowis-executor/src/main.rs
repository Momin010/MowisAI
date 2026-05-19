//! mowis-executor — guest binary that runs *inside* the Linux VM.
//!
//! Listens on AF_VSOCK and serves the `mowis-protocol` RPC. Owns the sandbox
//! primitives (overlayfs / chroot / namespaces) and the tool registry.

use clap::Parser;

#[cfg(target_os = "linux")]
mod sandbox;
#[cfg(target_os = "linux")]
mod server;
#[cfg(target_os = "linux")]
mod tools;
#[cfg(target_os = "linux")]
mod init;

#[derive(Debug, Parser)]
#[command(name = "mowis-executor", version)]
struct Cli {
    /// vsock port to listen on.
    #[arg(long, default_value_t = mowis_protocol::DEFAULT_VSOCK_PORT)]
    port: u32,

    /// Run as PID 1 / init: mount /proc, /sys, /dev, /tmp before serving.
    /// Auto-detected when actually running as PID 1.
    #[arg(long)]
    init: bool,
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let is_init = cli.init || std::process::id() == 1;
    if is_init {
        tracing::info!("running as init — mounting virtual filesystems");
        if let Err(e) = init::mount_essentials() {
            tracing::error!(error = %e, "essential mounts failed");
        }
    }

    tracing::info!(
        port = cli.port,
        version = env!("CARGO_PKG_VERSION"),
        protocol = mowis_protocol::PROTOCOL_VERSION,
        is_init,
        "starting mowis-executor"
    );

    // If we're PID 1 and the vsock server returns/crashes, the kernel will
    // panic with "attempted to kill init". Loop indefinitely on error so the
    // VM stays up for debugging instead of panic-rebooting.
    loop {
        match server::serve(cli.port).await {
            Ok(()) => {
                tracing::warn!("server returned Ok; restarting in 1s");
            }
            Err(e) => {
                tracing::error!(error = %e, "server failed; restarting in 1s");
            }
        }
        if !is_init {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!(
        "mowis-executor only runs on Linux (it lives inside the guest VM). \
         Build it with --target x86_64-unknown-linux-musl for deployment."
    );
    std::process::exit(1);
}
