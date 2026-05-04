// macos.rs — macOS QEMU HVF launcher

use crate::auth;
use crate::connection::is_tcp_reachable;
use crate::qemu::*;
use crate::types::*;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;

pub struct MacOSLauncher {
    qemu: QemuLauncher,
}

impl MacOSLauncher {
    pub fn new() -> Self {
        let image_path = std::env::var("MOWIS_QEMU_IMAGE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
                base.join("MowisAI").join("alpine.qcow2")
            });
        log::info!("[macos] QEMU image: {}", image_path.display());
        Self {
            qemu: QemuLauncher::new(QemuConfig::macos_hvf(image_path)),
        }
    }
}

#[async_trait]
impl VmLauncher for MacOSLauncher {
    async fn start(&self, progress: Option<ProgressSender>) -> Result<ConnectionInfo> {
        let pw = &progress;
        let token = auth::load_or_create().context("load auth token")?;

        emit(pw, "detecting", "macOS platform detected", 5, "info",
            Some(format!("Accelerator: HVF, Image: {}", self.qemu.agent_tcp()))).await;

        // Check if QEMU is already running
        let load_snap = !is_tcp_reachable(self.qemu.agent_tcp()).await;

        if load_snap {
            emit(pw, "booting", "Starting QEMU with HVF acceleration…", 15, "command",
                Some(format!("qemu-system-x86_64 -accel hvf -m 1024 …"))).await;

            let _child = self.qemu.spawn_process(&token, true).await
                .context("spawn QEMU")?;

            emit(pw, "booting", "QEMU spawned, waiting for serial bridge…", 40, "info", None).await;
            self.qemu.wait_for_agent().await.context("wait for agent")?;

            // Save snapshot for fast subsequent boots
            emit(pw, "booting", "Saving VM snapshot for fast restart…", 80, "info", None).await;
            let _ = self.qemu.save_snapshot().await;
        } else {
            emit(pw, "booting", "QEMU already running, reconnecting…", 80, "success", None).await;
        }

        emit(pw, "ready", "macOS QEMU ready", 100, "success",
            Some(format!("Agent TCP: {}", self.qemu.agent_tcp()))).await;

        Ok(ConnectionInfo {
            kind: ConnectionKind::TcpWithToken,
            socket_path: None,
            tcp_addr: Some(self.qemu.agent_tcp().to_owned()),
            pipe_name: None,
            auth_token: Some(token),
        })
    }

    async fn stop(&self) -> Result<()> {
        self.qemu.quit().await
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(is_tcp_reachable(self.qemu.agent_tcp()).await)
    }

    fn name(&self) -> &str { "macOS-QEMU-HVF" }
}
