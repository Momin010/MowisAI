use crate::launcher::{ConnectionInfo, VmLauncher};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::sleep;

/// Linux direct launcher - runs agentd natively without VM
///
/// On Linux, agentd runs directly on the host OS without any virtualization.
/// This launcher manages the agentd process lifecycle and provides Unix socket
/// connection information.
pub struct LinuxDirectLauncher {
    socket_path: PathBuf,
    agentd_binary: PathBuf,
}

impl LinuxDirectLauncher {
    /// Create a new Linux direct launcher
    pub fn new() -> Result<Self> {
        let socket_path = Self::resolve_socket_path()?;
        let agentd_binary = which::which("agentd")
            .context("agentd binary not found in PATH")?;

        Ok(Self {
            socket_path,
            agentd_binary,
        })
    }

    /// Resolve the Unix socket path using XDG_RUNTIME_DIR with fallback
    fn resolve_socket_path() -> Result<PathBuf> {
        // Try XDG_RUNTIME_DIR first (standard on modern Linux)
        if let Ok(xdg_runtime) = std::env::var("XDG_RUNTIME_DIR") {
            return Ok(PathBuf::from(xdg_runtime).join("agentd.sock"));
        }

        // Fallback to /tmp/agentd-$UID.sock
        let uid = unsafe { libc::getuid() };
        Ok(PathBuf::from(format!("/tmp/agentd-{}.sock", uid)))
    }

    /// Wait for socket to become available with timeout
    async fn wait_for_socket(&self, timeout: Duration) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if self.socket_path.exists() {
                // Try to connect to verify it's actually responsive
                if tokio::net::UnixStream::connect(&self.socket_path).await.is_ok() {
                    return Ok(());
                }
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow::anyhow!(
                    "Socket did not become available within {:?}",
                    timeout
                ));
            }

            sleep(Duration::from_millis(100)).await;
        }
    }

    /// Set socket permissions to 0600 (owner read/write only)
    #[cfg(unix)]
    fn set_socket_permissions(&self) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&self.socket_path, perms)
            .context("Failed to set socket permissions to 0600")?;
        
        Ok(())
    }

    #[cfg(not(unix))]
    fn set_socket_permissions(&self) -> Result<()> {
        // No-op on non-Unix platforms
        Ok(())
    }
}

impl Default for LinuxDirectLauncher {
    fn default() -> Self {
        Self::new().expect("Failed to create LinuxDirectLauncher")
    }
}

#[async_trait::async_trait]
impl VmLauncher for LinuxDirectLauncher {
    async fn start(&self) -> Result<ConnectionInfo> {
        // Check if agentd is already running
        if self.socket_path.exists() {
            if tokio::net::UnixStream::connect(&self.socket_path).await.is_ok() {
                log::info!("agentd already running at {:?}", self.socket_path);
                return Ok(ConnectionInfo::UnixSocket {
                    path: self.socket_path.clone(),
                });
            } else {
                // Stale socket file, remove it
                let _ = std::fs::remove_file(&self.socket_path);
            }
        }

        // Start agentd process
        log::info!("Starting agentd at {:?}", self.socket_path);
        
        tokio::process::Command::new(&self.agentd_binary)
            .args(["socket", "--path", &self.socket_path.to_string_lossy()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("Failed to spawn agentd process")?;

        // Wait for socket to become available (up to 5 seconds)
        self.wait_for_socket(Duration::from_secs(5)).await?;

        // Set secure permissions on the socket
        self.set_socket_permissions()?;

        log::info!("agentd started successfully");

        Ok(ConnectionInfo::UnixSocket {
            path: self.socket_path.clone(),
        })
    }

    async fn stop(&self) -> Result<()> {
        // On Linux, we don't manage the agentd process lifecycle directly
        // The socket server is designed to run as a daemon
        // Just verify the socket exists
        if self.socket_path.exists() {
            log::info!("agentd socket exists at {:?}", self.socket_path);
        }
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        // Try to connect to the socket
        match tokio::net::UnixStream::connect(&self.socket_path).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn connection_info(&self) -> Result<ConnectionInfo> {
        Ok(ConnectionInfo::UnixSocket {
            path: self.socket_path.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_path_resolution() {
        // Test with XDG_RUNTIME_DIR set
        std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1000");
        let path = LinuxDirectLauncher::resolve_socket_path().unwrap();
        assert_eq!(path, PathBuf::from("/run/user/1000/agentd.sock"));

        // Test fallback to /tmp
        std::env::remove_var("XDG_RUNTIME_DIR");
        let path = LinuxDirectLauncher::resolve_socket_path().unwrap();
        assert!(path.to_string_lossy().starts_with("/tmp/agentd-"));
        assert!(path.to_string_lossy().ends_with(".sock"));
    }
}
