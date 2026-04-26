use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

// Re-export so platform implementations import from one place.
pub use crate::types::SetupProgress;

// ── Where the GUI connects to the daemon ──────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ConnectionTarget {
    /// Unix domain socket — Linux / macOS native (not available on Windows)
    #[cfg(unix)]
    UnixSocket(String),
    /// TCP loopback port — VM bridge on macOS / Windows
    Tcp { port: u16 },
}

// ── Per-platform daemon management trait ─────────────────────────────────────

#[async_trait]
pub trait DaemonPlatform: Send + Sync {
    /// Idempotent: ensure daemon is running, send progress events to `tx`.
    async fn ensure_running(&mut self, tx: mpsc::Sender<SetupProgress>) -> Result<()>;
    /// Where the GUI should connect to reach the daemon.
    fn connection_target(&self) -> ConnectionTarget;
    /// Non-blocking reachability check.
    async fn is_reachable(&self) -> bool;
    /// Graceful stop (called on app exit when we own the process/VM).
    async fn stop(&mut self) -> Result<()>;
}

// ── Platform modules — only compiled on the matching OS ──────────────────────

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

// ── Factory ───────────────────────────────────────────────────────────────────

pub fn create_platform() -> Box<dyn DaemonPlatform> {
    #[cfg(target_os = "linux")]
    return Box::new(linux::LinuxPlatform::new());

    #[cfg(target_os = "macos")]
    return Box::new(macos::MacOsPlatform::new());

    #[cfg(target_os = "windows")]
    return Box::new(windows::WindowsPlatform::new());

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    panic!("MowisAI is not supported on this platform. Supported: Linux, macOS, Windows.");
}
