use super::{ConnectionInfo, VmLauncher};
use crate::types::SetupProgress;
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;

const SOCKET_PATH: &str = "/tmp/agentd.sock";

pub struct LinuxDirectLauncher {
    child: Option<tokio::process::Child>,
    conn_info: Option<ConnectionInfo>,
}

impl LinuxDirectLauncher {
    pub fn new() -> Self {
        Self { child: None, conn_info: None }
    }
}

#[async_trait]
impl VmLauncher for LinuxDirectLauncher {
    async fn start(&mut self, tx: mpsc::Sender<SetupProgress>) -> Result<ConnectionInfo> {
        let _ = tx.send(SetupProgress::Checking).await;

        if socket_reachable().await {
            let info = ConnectionInfo::UnixSocket { path: PathBuf::from(SOCKET_PATH) };
            self.conn_info = Some(info.clone());
            let _ = tx.send(SetupProgress::Ready).await;
            return Ok(info);
        }

        let _ = tx.send(SetupProgress::Starting).await;

        let bin = which::which("agentd").unwrap_or_else(|_| PathBuf::from("./agentd"));
        log::info!("Launching agentd from {:?}", bin);

        let child = tokio::process::Command::new(&bin)
            .args(["socket", "--path", SOCKET_PATH])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to launch agentd ({bin:?}): {e}"))?;

        self.child = Some(child);

        // Poll with exponential backoff until the socket appears (max 10 s).
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let mut delay = Duration::from_millis(100);
        loop {
            if socket_reachable().await {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow::anyhow!(
                    "agentd did not create socket at {SOCKET_PATH} within 10 s"
                ));
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(1));
        }

        let info = ConnectionInfo::UnixSocket { path: PathBuf::from(SOCKET_PATH) };
        self.conn_info = Some(info.clone());
        let _ = tx.send(SetupProgress::Ready).await;
        Ok(info)
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            child.kill().await.ok();
            child.wait().await.ok();
        }
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(socket_reachable().await)
    }

    fn connection_info(&self) -> Option<&ConnectionInfo> {
        self.conn_info.as_ref()
    }
}

async fn socket_reachable() -> bool {
    tokio::net::UnixStream::connect(SOCKET_PATH).await.is_ok()
}
