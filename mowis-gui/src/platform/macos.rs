use super::{ConnectionTarget, DaemonPlatform};
use crate::types::SetupProgress;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

const VM_PORT: u16 = 9722;
const IMAGE_URL: &str = "https://releases.mowisai.com/agentd-alpine-v1.0.qcow2";
const BOOT_TIMEOUT_SECS: u64 = 30;
const BOOT_POLL_MS: u64 = 500;

pub struct MacOsPlatform {
    qemu_child: Option<tokio::process::Child>,
    image_path: PathBuf,
    qemu_bin: PathBuf,
}

impl MacOsPlatform {
    pub fn new() -> Self {
        let image_path = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("mowisai/agentd-alpine.qcow2");

        Self {
            qemu_child: None,
            image_path,
            qemu_bin: PathBuf::new(),
        }
    }

    async fn tcp_reachable() -> bool {
        TcpStream::connect(("127.0.0.1", VM_PORT)).await.is_ok()
    }

    fn find_qemu() -> Result<PathBuf> {
        if let Ok(p) = which::which("qemu-system-x86_64") {
            return Ok(p);
        }
        for candidate in [
            "/usr/local/bin/qemu-system-x86_64",
            "/opt/homebrew/bin/qemu-system-x86_64",
        ] {
            let p = PathBuf::from(candidate);
            if p.exists() {
                return Ok(p);
            }
        }
        Err(anyhow!("QEMU not found. Install with: brew install qemu"))
    }

    async fn ensure_image(&self, tx: &mpsc::Sender<SetupProgress>) -> Result<()> {
        if self.image_path.exists() {
            return Ok(());
        }

        if let Some(parent) = self.image_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                anyhow!("Failed to create image directory {:?}: {e}", parent)
            })?;
        }

        let _ = tx
            .send(SetupProgress::Downloading {
                label: "MowisAI Alpine image".into(),
                pct: 0,
            })
            .await;

        let status = tokio::process::Command::new("curl")
            .args([
                "-L",
                "--progress-bar",
                "-o",
                self.image_path
                    .to_str()
                    .ok_or_else(|| anyhow!("Image path is not valid UTF-8"))?,
                IMAGE_URL,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map_err(|e| anyhow!("Failed to run curl: {e}"))?;

        if !status.success() {
            return Err(anyhow!(
                "curl exited with {} while downloading Alpine image",
                status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".into())
            ));
        }

        let _ = tx
            .send(SetupProgress::Downloading {
                label: "MowisAI Alpine image".into(),
                pct: 100,
            })
            .await;

        Ok(())
    }

    async fn spawn_qemu(&mut self) -> Result<()> {
        let image_str = self
            .image_path
            .to_str()
            .ok_or_else(|| anyhow!("Image path is not valid UTF-8"))?;

        let child = tokio::process::Command::new(&self.qemu_bin)
            .args([
                "-nographic",
                "-m",
                "512",
                "-smp",
                "2",
                "-accel",
                "hvf",
                "-drive",
                &format!("file={image_str},format=qcow2,if=virtio"),
                "-netdev",
                &format!("user,id=net0,hostfwd=tcp::{VM_PORT}-:{VM_PORT}"),
                "-device",
                "virtio-net-pci,netdev=net0",
                "-serial",
                "none",
                "-monitor",
                "none",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow!("Failed to spawn QEMU ({:?}): {e}", self.qemu_bin))?;

        self.qemu_child = Some(child);
        Ok(())
    }

    async fn wait_for_boot() -> Result<()> {
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_secs(BOOT_TIMEOUT_SECS);

        loop {
            if Self::tcp_reachable().await {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow!(
                    "agentd VM did not become reachable on port {VM_PORT} within {BOOT_TIMEOUT_SECS}s"
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(BOOT_POLL_MS)).await;
        }
    }
}

#[async_trait]
impl DaemonPlatform for MacOsPlatform {
    async fn ensure_running(&mut self, tx: mpsc::Sender<SetupProgress>) -> Result<()> {
        let _ = tx.send(SetupProgress::Checking).await;

        if Self::tcp_reachable().await {
            let _ = tx.send(SetupProgress::Ready).await;
            return Ok(());
        }

        self.qemu_bin = Self::find_qemu()?;

        self.ensure_image(&tx).await?;

        let _ = tx.send(SetupProgress::Starting).await;

        self.spawn_qemu().await?;

        Self::wait_for_boot().await?;

        let _ = tx.send(SetupProgress::Ready).await;
        Ok(())
    }

    fn connection_target(&self) -> ConnectionTarget {
        ConnectionTarget::Tcp { port: VM_PORT }
    }

    async fn is_reachable(&self) -> bool {
        Self::tcp_reachable().await
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.qemu_child.take() {
            child.kill().await.ok();
            child.wait().await.ok();
        }
        Ok(())
    }
}
