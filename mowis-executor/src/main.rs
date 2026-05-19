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

#[derive(Debug, Parser)]
#[command(name = "mowis-executor", version)]
struct Cli {
    /// vsock port to listen on.
    #[arg(long, default_value_t = mowis_protocol::DEFAULT_VSOCK_PORT)]
    port: u32,
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
    tracing::info!(
        port = cli.port,
        version = env!("CARGO_PKG_VERSION"),
        protocol = mowis_protocol::PROTOCOL_VERSION,
        "starting mowis-executor"
    );
    server::serve(cli.port).await
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!(
        "mowis-executor only runs on Linux (it lives inside the guest VM). \
         Build it with --target x86_64-unknown-linux-musl for deployment."
    );
    std::process::exit(1);
}
