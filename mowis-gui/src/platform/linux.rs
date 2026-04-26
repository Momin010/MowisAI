use super::{ConnectionTarget, DaemonPlatform};
use crate::types::SetupProgress;
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

const SOCKET_PATH: &str = "/tmp/agentd.sock";

pub struct LinuxPlatform {
    child: Option<tokio::process::Child>,
}

impl LinuxPlatform {
    pub fn new() -> Self {
        Self { child: None }
    }
}

#[async_trait]
impl DaemonPlatform for LinuxPlatform {
    async fn ensure_running(&mut self, tx: mpsc::Sender<SetupProgress>) -> Result<()> {
        let _ = tx.send(SetupProgress::Checking).await;

        if self.is_reachable().await {
            let _ = tx.send(SetupProgress::Ready).await;
            return Ok(());
        }

        let _ = tx.send(SetupProgress::Starting).await;

        let bin = which::which("agentd")
            .unwrap_or_else(|_| std::path::PathBuf::from("./agentd"));

        log::info!("Launching agentd from {:?}", bin);

        let child = tokio::process::Command::new(&bin)
            .args(["socket", "--path", SOCKET_PATH])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to launch agentd ({bin:?}): {e}"))?;

        self.child = Some(child);

        // Wait up to 5 s for the socket to become connectable.
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if self.is_reachable().await {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow::anyhow!(
                    "agentd did not create socket at {SOCKET_PATH} within 5 seconds"
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        let _ = tx.send(SetupProgress::Ready).await;
        Ok(())
    }

    fn connection_target(&self) -> ConnectionTarget {
        ConnectionTarget::UnixSocket(SOCKET_PATH.to_owned())
    }

    async fn is_reachable(&self) -> bool {
        tokio::net::UnixStream::connect(SOCKET_PATH).await.is_ok()
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            child.kill().await.ok();
            child.wait().await.ok();
        }
        Ok(())
    }
}
