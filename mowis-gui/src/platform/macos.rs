//! macOS launcher — uses QEMU with the Apple Hypervisor Framework (HVF)
//! accelerator.  Falls back to TCG software emulation if HVF is unavailable
//! (e.g. inside a CI VM).
//!
//! This file is only compiled on macOS.

use super::{qemu::QemuLauncher, ConnectionInfo, VmLauncher};
use crate::types::SetupProgress;
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::sync::mpsc;

const IMAGE_URL: &str = "https://releases.mowisai.com/agentd-alpine-v1.0.qcow2";

pub struct MacOSLauncher {
    inner: QemuLauncher,
}

impl MacOSLauncher {
    pub fn new() -> Self {
        let image_path = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("mowisai/agentd-alpine.qcow2");

        Self {
            inner: QemuLauncher::new(image_path, "hvf"),
        }
    }

    async fn ensure_image(&self, tx: &mpsc::Sender<SetupProgress>) -> Result<()> {
        if self.inner.image_path.exists() {
            return Ok(());
        }

        let _ = tx
            .send(SetupProgress::Downloading {
                label: "MowisAI Alpine image".into(),
                pct: 0,
            })
            .await;

        let image_str = self
            .inner
            .image_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Image path is not valid UTF-8"))?;

        let status = tokio::process::Command::new("curl")
            .args(["-fL", "--progress-bar", "-o", image_str, IMAGE_URL])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to run curl: {e}"))?;

        if !status.success() {
            return Err(anyhow::anyhow!(
                "curl failed (exit {:?}) while downloading {}",
                status.code(),
                IMAGE_URL
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
}

#[async_trait]
impl VmLauncher for MacOSLauncher {
    async fn start(&mut self, tx: mpsc::Sender<SetupProgress>) -> Result<ConnectionInfo> {
        // Resolve QEMU binary once.
        if self.inner.qemu_bin.as_os_str().is_empty() {
            self.inner.qemu_bin = QemuLauncher::find_qemu()?;
        }

        self.ensure_image(&tx).await?;

        self.inner.start(tx).await
    }

    async fn stop(&mut self) -> Result<()> {
        self.inner.stop().await
    }

    async fn health_check(&self) -> Result<bool> {
        self.inner.health_check().await
    }

    fn connection_info(&self) -> Option<&ConnectionInfo> {
        self.inner.connection_info()
    }
}
