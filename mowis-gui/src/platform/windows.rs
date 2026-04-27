//! Windows launcher for MowisAI.
//!
//! Strategy (tried in order):
//!   1. WSL2 — import a custom Alpine distro ("MowisAI"), run agentd inside it
//!              and forward the socket via socat to TCP 127.0.0.1:9722.
//!              Connection: TcpWithToken.
//!   2. QEMU — fallback when WSL2 is absent; uses WHPX accelerator when
//!              available, otherwise TCG software emulation.
//!              Connection: TcpWithToken.
//!
//! This file is only compiled on Windows.

#![cfg(target_os = "windows")]

use super::{auth, qemu::QemuLauncher, ConnectionInfo, VmLauncher};
use crate::types::SetupProgress;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;

// ── Constants ─────────────────────────────────────────────────────────────────

const VM_PORT: u16 = 9722;
const WSL_DISTRO_NAME: &str = "MowisAI";
const ALPINE_TAR_URL: &str = "https://releases.mowisai.com/agentd-alpine-v1.0.tar";
const ALPINE_QCOW2_URL: &str = "https://releases.mowisai.com/agentd-alpine-v1.0.qcow2";
const BOOT_TIMEOUT_SECS: u64 = 60;

// ── Mode ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum WindowsMode {
    Wsl2,
    Qemu,
}

// ── WindowsLauncher ───────────────────────────────────────────────────────────

pub struct WindowsLauncher {
    mode: Option<WindowsMode>,
    wsl_process: Option<tokio::process::Child>,
    qemu: Option<QemuLauncher>,
    token: String,
    conn_info: Option<ConnectionInfo>,
}

impl WindowsLauncher {
    pub fn new() -> Self {
        Self {
            mode: None,
            wsl_process: None,
            qemu: None,
            token: String::new(),
            conn_info: None,
        }
    }

    // ── Path helpers ──────────────────────────────────────────────────────────

    fn local_app_data() -> Result<PathBuf> {
        dirs::data_local_dir()
            .or_else(|| std::env::var("LOCALAPPDATA").ok().map(PathBuf::from))
            .ok_or_else(|| anyhow!("Cannot determine %LOCALAPPDATA%"))
    }

    fn alpine_tar_path() -> Result<PathBuf> {
        Ok(Self::local_app_data()?.join("mowisai").join("agentd-alpine.tar"))
    }

    fn wsl_install_dir() -> Result<PathBuf> {
        Ok(Self::local_app_data()?.join("mowisai").join("wsl"))
    }

    fn alpine_qcow2_path() -> Result<PathBuf> {
        Ok(Self::local_app_data()?.join("mowisai").join("agentd-alpine.qcow2"))
    }

    // ── WSL2 helpers ──────────────────────────────────────────────────────────

    fn wsl_available() -> bool {
        which::which("wsl").is_ok()
            || PathBuf::from(r"C:\Windows\System32\wsl.exe").exists()
    }

    async fn wsl_distro_installed() -> bool {
        tokio::process::Command::new("wsl")
            .args(["--list", "--quiet"])
            .output()
            .await
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .any(|l| l.trim().eq_ignore_ascii_case(WSL_DISTRO_NAME))
            })
            .unwrap_or(false)
    }

    async fn download_file(
        url: &str,
        dest: &PathBuf,
        label: &str,
        tx: &mpsc::Sender<SetupProgress>,
    ) -> Result<()> {
        if dest.exists() {
            return Ok(());
        }
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let _ = tx
            .send(SetupProgress::Downloading { label: label.into(), pct: 0 })
            .await;

        let dest_str = dest.to_str().ok_or_else(|| anyhow!("Non-UTF-8 path"))?;
        let status = tokio::process::Command::new("curl")
            .args(["-fL", "--progress-bar", "-o", dest_str, url])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map_err(|e| anyhow!("curl failed: {e}"))?;

        if !status.success() {
            return Err(anyhow!("curl exit {:?} downloading {url}", status.code()));
        }
        let _ = tx
            .send(SetupProgress::Downloading { label: label.into(), pct: 100 })
            .await;
        Ok(())
    }

    async fn install_wsl_distro(tx: &mpsc::Sender<SetupProgress>) -> Result<()> {
        let tar_path = Self::alpine_tar_path()?;
        let install_dir = Self::wsl_install_dir()?;

        Self::download_file(ALPINE_TAR_URL, &tar_path, "MowisAI Alpine rootfs", tx).await?;

        let _ = tx
            .send(SetupProgress::Installing {
                step: "Importing MowisAI WSL2 distro…".into(),
            })
            .await;

        tokio::fs::create_dir_all(&install_dir).await?;
        let status = tokio::process::Command::new("wsl")
            .args([
                "--import",
                WSL_DISTRO_NAME,
                install_dir.to_str().unwrap_or("."),
                tar_path.to_str().unwrap_or("agentd-alpine.tar"),
            ])
            .status()
            .await
            .map_err(|e| anyhow!("wsl --import failed: {e}"))?;

        if !status.success() {
            return Err(anyhow!("wsl --import exited with {:?}", status.code()));
        }
        Ok(())
    }

    async fn start_wsl_bridge(&mut self, token: &str) -> Result<()> {
        // Run agentd inside WSL2, then bridge its Unix socket to TCP.
        let bridge_cmd = format!(
            "MOWISAI_TOKEN={token} /usr/local/bin/agentd socket --path /tmp/agentd.sock & \
             sleep 1 && socat TCP-LISTEN:{VM_PORT},fork,reuseaddr UNIX-CONNECT:/tmp/agentd.sock"
        );

        let child = tokio::process::Command::new("wsl")
            .args(["-d", WSL_DISTRO_NAME, "--", "sh", "-c", &bridge_cmd])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow!("Failed to start WSL2 bridge: {e}"))?;

        self.wsl_process = Some(child);
        Ok(())
    }

    async fn tcp_reachable() -> bool {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), VM_PORT);
        tokio::net::TcpStream::connect(addr).await.is_ok()
    }

    async fn wait_for_boot(tx: &mpsc::Sender<SetupProgress>) -> Result<()> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(BOOT_TIMEOUT_SECS);
        let mut delay = Duration::from_millis(500);
        while !Self::tcp_reachable().await {
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow!(
                    "agentd did not respond on port {VM_PORT} within {BOOT_TIMEOUT_SECS} s"
                ));
            }
            let _ = tx
                .send(SetupProgress::Downloading {
                    label: "Starting daemon…".into(),
                    pct: 50,
                })
                .await;
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(2));
        }
        Ok(())
    }
}

#[async_trait]
impl VmLauncher for WindowsLauncher {
    async fn start(&mut self, tx: mpsc::Sender<SetupProgress>) -> Result<ConnectionInfo> {
        let _ = tx.send(SetupProgress::Checking).await;

        // Reuse existing connection if still alive.
        if Self::tcp_reachable().await {
            if let Some(info) = &self.conn_info {
                let _ = tx.send(SetupProgress::Ready).await;
                return Ok(info.clone());
            }
        }

        // ── Try WSL2 ──────────────────────────────────────────────────────────
        if Self::wsl_available() {
            self.token = auth::generate_token();

            if !Self::wsl_distro_installed().await {
                Self::install_wsl_distro(&tx).await?;
            }

            let _ = tx.send(SetupProgress::Starting).await;
            let token = self.token.clone();
            self.start_wsl_bridge(&token).await?;
            Self::wait_for_boot(&tx).await?;

            self.mode = Some(WindowsMode::Wsl2);
            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), VM_PORT);
            let info = ConnectionInfo::TcpWithToken { addr, token: self.token.clone() };
            self.conn_info = Some(info.clone());
            let _ = tx.send(SetupProgress::Ready).await;
            return Ok(info);
        }

        // ── QEMU fallback ─────────────────────────────────────────────────────
        log::info!("WSL2 not available — falling back to QEMU");
        let _ = tx
            .send(SetupProgress::Warning(
                "WSL2 not found — using QEMU (slower first boot)".into(),
            ))
            .await;

        let qcow2_path = Self::alpine_qcow2_path()?;
        Self::download_file(ALPINE_QCOW2_URL, &qcow2_path, "MowisAI Alpine image", &tx).await?;

        let mut launcher = QemuLauncher::new(qcow2_path, "whpx,kernel-irqchip=off");
        launcher.qemu_bin = QemuLauncher::find_qemu()?;

        let info = launcher.start(tx).await?;
        self.qemu = Some(launcher);
        self.mode = Some(WindowsMode::Qemu);
        self.conn_info = Some(info.clone());
        Ok(info)
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.wsl_process.take() {
            child.kill().await.ok();
            child.wait().await.ok();
            let _ = tokio::process::Command::new("wsl")
                .args(["--terminate", WSL_DISTRO_NAME])
                .status()
                .await;
        }
        if let Some(ref mut q) = self.qemu {
            q.stop().await?;
        }
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(Self::tcp_reachable().await)
    }

    fn connection_info(&self) -> Option<&ConnectionInfo> {
        self.conn_info.as_ref()
    }
}
