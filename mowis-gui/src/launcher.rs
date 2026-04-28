use anyhow::Result;
use std::path::PathBuf;
use std::time::Instant;

/// Connection information for communicating with agentd
#[derive(Debug, Clone)]
pub enum ConnectionInfo {
    /// Unix domain socket (Linux, macOS direct)
    UnixSocket { path: PathBuf },

    /// virtio-vsock (macOS Virtualization.framework)
    /// Exposed as Unix socket on host side
    Vsock { path: PathBuf },

    /// Windows named pipe (WSL2)
    NamedPipe {
        name: String, // e.g. "\\.\pipe\MowisAI\agentd"
    },

    /// TCP with auth token (QEMU fallback)
    TcpWithToken {
        addr: std::net::SocketAddr,
        token: String,
    },
}

/// Configuration for VM launcher
#[derive(Debug, Clone)]
pub struct LauncherConfig {
    /// Path to Alpine image
    pub image_path: PathBuf,

    /// Path to agentd binary (for Linux direct launch)
    pub agentd_binary: Option<PathBuf>,

    /// VM memory in MB (default: 512)
    pub memory_mb: u64,

    /// VM CPU count (default: 1)
    pub cpu_count: u32,

    /// Enable snapshot-based fast boot
    pub enable_snapshots: bool,

    /// Snapshot directory
    pub snapshot_dir: PathBuf,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            image_path: PathBuf::from("alpine.img"),
            agentd_binary: which::which("agentd").ok(),
            memory_mb: 512,
            cpu_count: 1,
            enable_snapshots: true,
            snapshot_dir: dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("mowisai")
                .join("snapshots"),
        }
    }
}

/// Handle to a running VM instance
#[derive(Debug, Clone)]
pub struct VmHandle {
    /// Unique identifier for this VM instance
    pub id: String,

    /// Platform-specific process ID or handle
    pub pid: Option<u32>,

    /// Connection information
    pub connection: ConnectionInfo,

    /// VM state snapshot path (for fast restart)
    pub snapshot_path: Option<PathBuf>,

    /// Timestamp of last health check
    pub last_health_check: Instant,
}

/// Platform-specific VM launcher trait
///
/// All platform-specific launchers (Linux direct, macOS Virtualization.framework,
/// Windows WSL2, QEMU fallback) implement this trait to provide a unified interface
/// for VM lifecycle management.
#[async_trait::async_trait]
pub trait VmLauncher: Send + Sync {
    /// Start the VM and agentd daemon
    ///
    /// Returns connection info (socket path or TCP address + token)
    async fn start(&self) -> Result<ConnectionInfo>;

    /// Stop the VM and clean up resources
    async fn stop(&self) -> Result<()>;

    /// Check if the VM is running and healthy
    async fn health_check(&self) -> Result<bool>;

    /// Get the connection info for an already-running VM
    async fn connection_info(&self) -> Result<ConnectionInfo>;
}
