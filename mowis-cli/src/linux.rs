// linux.rs — Linux direct Unix socket launcher

use crate::connection::is_tcp_reachable;
use crate::types::*;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;

pub struct LinuxDirectLauncher {
    socket_path: PathBuf,
}

impl LinuxDirectLauncher {
    pub fn new() -> Self {
        let socket_path = std::env::var("MOWIS_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp/agentd.sock"));
        log::info!("[linux] Socket path: {}", socket_path.display());
        Self { socket_path }
    }
}

#[async_trait]
impl VmLauncher for LinuxDirectLauncher {
    async fn start(&self, progress: Option<ProgressSender>) -> Result<ConnectionInfo> {
        let pw = &progress;
        emit(pw, "detecting", "Checking agentd socket…", 10, "info",
            Some(format!("Path: {}", self.socket_path.display()))).await;

        if !self.socket_path.exists() {
            emit(pw, "error", "Socket not found", 0, "error",
                Some(format!("{}\nStart agentd: sudo agentd socket --path {}", self.socket_path.display(), self.socket_path.display()))).await;
            anyhow::bail!("Socket not found: {}", self.socket_path.display());
        }

        emit(pw, "detecting", "Socket file exists, testing connection…", 30, "info", None).await;

        // Test that agentd is responsive
        let socket_str = self.socket_path.to_string_lossy().to_string();
        let responsive = tokio::task::spawn_blocking(move || {
            use std::os::unix::net::UnixStream;
            use std::time::Duration;
            match UnixStream::connect(&socket_str) {
                Ok(stream) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                    true
                }
                Err(_) => false,
            }
        }).await.unwrap_or(false);

        if !responsive {
            emit(pw, "error", "Socket exists but agentd is not responding", 0, "error", None).await;
            anyhow::bail!("agentd not responding on {}", self.socket_path.display());
        }

        emit(pw, "ready", "agentd is responsive", 100, "success",
            Some(format!("Socket: {}", self.socket_path.display()))).await;

        Ok(ConnectionInfo {
            kind: ConnectionKind::UnixSocket,
            socket_path: Some(self.socket_path.clone()),
            tcp_addr: None,
            pipe_name: None,
            auth_token: None,
        })
    }

    async fn stop(&self) -> Result<()> { Ok(()) }

    async fn health_check(&self) -> Result<bool> {
        Ok(self.socket_path.exists())
    }

    fn name(&self) -> &str { "LinuxDirect" }
}
