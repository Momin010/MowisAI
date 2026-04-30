// platform/mod.rs — VmLauncher trait + platform-specific module routing
//
// Architecture:
//   Linux  → LinuxDirectLauncher  (agentd runs natively, Unix socket)
//   macOS  → MacOSLauncher        (QEMU + HVF acceleration, TCP+token)
//   Windows→ WindowsLauncher      (WSL2 Alpine primary, QEMU/WHPX fallback)

pub mod auth;
pub mod checksum;
pub mod connection;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;
#[cfg(any(target_os = "macos", windows))]
pub mod qemu;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ── Connection descriptor returned by VmLauncher::start() ────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub kind: ConnectionKind,
    /// Unix socket path (Linux / macOS QEMU chardev)
    pub socket_path: Option<PathBuf>,
    /// TCP address (QEMU TCP, WSL2 relay)
    pub tcp_addr: Option<String>,
    /// Windows named pipe name (e.g. `\\.\pipe\MowisAI\agentd`)
    pub pipe_name: Option<String>,
    /// 256-bit auth token (hex); required for TCP and named-pipe connections
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConnectionKind {
    /// Direct Unix socket — Linux native
    UnixSocket,
    /// Windows named pipe served by the WSL2 socat bridge
    NamedPipe,
    /// TCP + Jupyter-style 256-bit token — QEMU / WSL2 TCP relay
    TcpWithToken,
}

// ── Platform launcher trait ───────────────────────────────────────────────────

#[async_trait]
pub trait VmLauncher: Send + Sync {
    /// Boot (or locate) the Linux environment and return connection details.
    async fn start(&self) -> Result<ConnectionInfo>;
    /// Gracefully stop the environment (save snapshot if applicable).
    async fn stop(&self) -> Result<()>;
    /// Return true if the environment is reachable right now.
    async fn health_check(&self) -> Result<bool>;
    /// Human-readable name for logging.
    fn name(&self) -> &str;
    /// Return diagnostic logs from the Linux environment (best-effort).
    async fn read_logs(&self) -> String {
        format!("Log collection is not available on this platform ({}).", std::env::consts::OS)
    }
}

// ── Factory — pick the right launcher for the current OS ─────────────────────

pub fn create_launcher() -> Box<dyn VmLauncher> {
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxDirectLauncher::new())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacOSLauncher::new())
    }
    #[cfg(windows)]
    {
        Box::new(windows::WindowsLauncher::new())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        compile_error!("Unsupported platform")
    }
}
