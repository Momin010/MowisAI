use anyhow::Result;
use async_trait::async_trait;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::sync::mpsc;

pub use crate::types::SetupProgress;

// Sub-modules — always public so backend.rs can use them directly
pub mod auth;
pub mod connection;
pub mod checksum;

// Platform launchers — only compiled on their target OS
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

// QEMU launcher — used as fallback on macOS and Windows
#[cfg(not(target_os = "linux"))]
pub mod qemu;

// ── Connection info returned by a launcher ────────────────────────────────────

/// Describes how the GUI should connect to the running agentd daemon.
#[derive(Debug, Clone)]
pub enum ConnectionInfo {
    /// Unix domain socket — Linux direct, and macOS vsock proxy socket.
    #[cfg(unix)]
    UnixSocket { path: PathBuf },

    /// Windows named pipe — WSL2 bridge.
    #[cfg(windows)]
    NamedPipe { name: String },

    /// TCP loopback with a one-time auth token — QEMU fallback on all platforms.
    TcpWithToken { addr: SocketAddr, token: String },
}

// ── VmLauncher trait ──────────────────────────────────────────────────────────

/// Each platform provides one implementation.  The backend calls `start` once
/// at startup, uses `connection_info` to open connections, and polls
/// `health_check` every 10 s.
#[async_trait]
pub trait VmLauncher: Send + Sync {
    /// Start the VM / daemon.  Idempotent — safe to call when already running.
    /// Progress events are sent to `tx` for the setup screen.
    async fn start(&mut self, tx: mpsc::Sender<SetupProgress>) -> Result<ConnectionInfo>;

    /// Stop the VM and release all resources.
    async fn stop(&mut self) -> Result<()>;

    /// Non-blocking liveness probe — returns `Ok(true)` when responsive.
    async fn health_check(&self) -> Result<bool>;

    /// Return the last known `ConnectionInfo` if the VM has been started,
    /// or `None` if not yet started.
    fn connection_info(&self) -> Option<&ConnectionInfo>;
}

// ── Factory ───────────────────────────────────────────────────────────────────

pub fn create_launcher() -> Box<dyn VmLauncher> {
    #[cfg(target_os = "linux")]
    return Box::new(linux::LinuxDirectLauncher::new());

    #[cfg(target_os = "macos")]
    return Box::new(macos::MacOSLauncher::new());

    #[cfg(target_os = "windows")]
    return Box::new(windows::WindowsLauncher::new());

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    panic!("MowisAI: unsupported platform. Supported: Linux, macOS, Windows.");
}
