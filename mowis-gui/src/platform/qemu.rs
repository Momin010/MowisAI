//! QEMU-based VM launcher used as fallback on macOS and Windows.
//!
//! Boot strategy:
//!   • First boot  (~15-20 s): normal boot, then `savevm mowisai-snap`
//!     is sent to the QEMU monitor and a metadata marker is written.
//!   • Subsequent boots (~5 s): QEMU is launched with `-loadvm mowisai-snap`.
//!
//! Auth: a fresh 256-bit token is generated each launch and passed into
//! the VM via QEMU's `-fw_cfg` mechanism.  The daemon reads it from
//! `/sys/firmware/qemu_fw_cfg/by_name/opt/mowisai.token/raw` and validates
//! every incoming TCP connection.

use super::{auth, ConnectionInfo, VmLauncher};
use crate::types::SetupProgress;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;

// ── Constants ─────────────────────────────────────────────────────────────────

const VM_PORT: u16 = 9722;
const BOOT_TIMEOUT_SECS: u64 = 60;
const SNAPSHOT_NAME: &str = "mowisai-snap";

// ── QemuLauncher ──────────────────────────────────────────────────────────────

pub struct QemuLauncher {
    pub image_path: PathBuf,
    pub accel: &'static str,
    pub qemu_bin: PathBuf,
    qemu_child: Option<tokio::process::Child>,
    monitor_port: u16,
    snapshot_meta: PathBuf,
    token: String,
    conn_info: Option<ConnectionInfo>,
}

impl QemuLauncher {
    pub fn new(image_path: PathBuf, accel: &'static str) -> Self {
        let snapshot_meta = image_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("agentd-snap.meta");

        Self {
            image_path,
            accel,
            qemu_bin: PathBuf::new(),
            qemu_child: None,
            monitor_port: 0,
            snapshot_meta,
            token: String::new(),
            conn_info: None,
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    pub fn find_qemu() -> Result<PathBuf> {
        let candidates = [
            "qemu-system-x86_64",
            "/usr/local/bin/qemu-system-x86_64",
            "/opt/homebrew/bin/qemu-system-x86_64",
            r"C:\Program Files\qemu\qemu-system-x86_64.exe",
        ];
        if let Ok(p) = which::which("qemu-system-x86_64") {
            return Ok(p);
        }
        for c in &candidates[1..] {
            let p = PathBuf::from(c);
            if p.exists() {
                return Ok(p);
            }
        }
        Err(anyhow!(
            "qemu-system-x86_64 not found. \
             On macOS install with `brew install qemu`; \
             on Windows download from https://qemu.weilnetz.de/"
        ))
    }

    fn snapshot_exists(&self) -> bool {
        self.snapshot_meta.exists()
    }

    async fn tcp_reachable(&self) -> bool {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), VM_PORT);
        tokio::net::TcpStream::connect(addr).await.is_ok()
    }

    /// Pick a random free TCP port for the QEMU monitor.
    fn free_port() -> u16 {
        let sock =
            std::net::TcpListener::bind("127.0.0.1:0").expect("Cannot bind to find a free port");
        sock.local_addr().unwrap().port()
    }

    /// Send a command to the QEMU HMP monitor via TCP.
    async fn monitor_cmd(&self, cmd: &str) -> Result<()> {
        use tokio::io::AsyncWriteExt;
        let mut sock =
            tokio::net::TcpStream::connect(("127.0.0.1", self.monitor_port)).await?;
        // QEMU sends a welcome banner — give it a moment before sending commands.
        tokio::time::sleep(Duration::from_millis(300)).await;
        sock.write_all(format!("{cmd}\n").as_bytes()).await?;
        Ok(())
    }

    async fn ensure_image_dir(&self) -> Result<()> {
        if let Some(parent) = self.image_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| anyhow!("Cannot create image directory {:?}: {e}", parent))?;
        }
        Ok(())
    }

    async fn spawn_qemu(&mut self) -> Result<()> {
        let image_str = self
            .image_path
            .to_str()
            .ok_or_else(|| anyhow!("Image path is not valid UTF-8"))?;

        self.monitor_port = Self::free_port();

        let mut args = vec![
            "-nographic".to_string(),
            "-m".to_string(),
            "1024".to_string(),
            "-smp".to_string(),
            "2".to_string(),
            "-accel".to_string(),
            self.accel.to_string(),
            "-drive".to_string(),
            format!("file={image_str},format=qcow2,if=virtio"),
            "-netdev".to_string(),
            format!("user,id=net0,hostfwd=tcp::{VM_PORT}-:{VM_PORT}"),
            "-device".to_string(),
            "virtio-net-pci,netdev=net0".to_string(),
            "-monitor".to_string(),
            format!("tcp:127.0.0.1:{},server,nowait", self.monitor_port),
            // Pass auth token into the VM via QEMU fw_cfg.
            "-fw_cfg".to_string(),
            format!("name=opt/mowisai.token,string={}", self.token),
        ];

        if self.snapshot_exists() {
            args.push("-loadvm".to_string());
            args.push(SNAPSHOT_NAME.to_string());
        }

        let child = tokio::process::Command::new(&self.qemu_bin)
            .args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow!("Failed to spawn QEMU ({:?}): {e}", self.qemu_bin))?;

        self.qemu_child = Some(child);
        Ok(())
    }

    async fn wait_for_boot(&self, tx: &mpsc::Sender<SetupProgress>) -> Result<()> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(BOOT_TIMEOUT_SECS);
        let mut delay = Duration::from_millis(500);

        loop {
            if self.tcp_reachable().await {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow!(
                    "VM did not respond on port {VM_PORT} within {BOOT_TIMEOUT_SECS} s"
                ));
            }
            let elapsed_pct = {
                let elapsed = tokio::time::Instant::now()
                    .duration_since(tokio::time::Instant::now() - Duration::from_secs(BOOT_TIMEOUT_SECS));
                ((elapsed.as_secs_f32() / BOOT_TIMEOUT_SECS as f32) * 100.0) as u8
            };
            let _ = tx
                .send(SetupProgress::Downloading {
                    label: "Booting VM…".into(),
                    pct: elapsed_pct.min(99),
                })
                .await;
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(2));
        }
    }

    async fn save_snapshot(&self) -> Result<()> {
        // Allow QEMU to finish booting fully before snapshotting.
        tokio::time::sleep(Duration::from_secs(3)).await;

        self.monitor_cmd(&format!("savevm {SNAPSHOT_NAME}")).await?;

        // Wait for snapshot to be written (can take a few seconds).
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Write marker so subsequent launches use -loadvm.
        tokio::fs::write(&self.snapshot_meta, SNAPSHOT_NAME)
            .await
            .map_err(|e| anyhow!("Cannot write snapshot metadata: {e}"))?;

        log::info!("VM snapshot saved as '{SNAPSHOT_NAME}'");
        Ok(())
    }
}

#[async_trait]
impl VmLauncher for QemuLauncher {
    async fn start(&mut self, tx: mpsc::Sender<SetupProgress>) -> Result<ConnectionInfo> {
        let _ = tx.send(SetupProgress::Checking).await;

        // Already running?
        if self.tcp_reachable().await {
            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), VM_PORT);
            let info = ConnectionInfo::TcpWithToken {
                addr,
                token: self.token.clone(),
            };
            self.conn_info = Some(info.clone());
            let _ = tx.send(SetupProgress::Ready).await;
            return Ok(info);
        }

        if self.qemu_bin.as_os_str().is_empty() {
            self.qemu_bin = Self::find_qemu()?;
        }

        self.ensure_image_dir().await?;

        // Generate a fresh 256-bit token for this session.
        self.token = auth::generate_token();

        let first_boot = !self.snapshot_exists();

        let _ = tx.send(SetupProgress::Starting).await;
        self.spawn_qemu().await?;

        self.wait_for_boot(&tx).await?;

        // On the very first boot, save a snapshot for future fast starts.
        if first_boot {
            let _ = tx
                .send(SetupProgress::Installing {
                    step: "Saving VM snapshot for fast future boots…".into(),
                })
                .await;
            if let Err(e) = self.save_snapshot().await {
                log::warn!("Snapshot save failed (non-fatal): {e}");
            }
        }

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), VM_PORT);
        let info = ConnectionInfo::TcpWithToken {
            addr,
            token: self.token.clone(),
        };
        self.conn_info = Some(info.clone());
        let _ = tx.send(SetupProgress::Ready).await;
        Ok(info)
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.qemu_child.take() {
            // Graceful shutdown via monitor first.
            let _ = self.monitor_cmd("quit").await;
            tokio::time::sleep(Duration::from_secs(2)).await;
            child.kill().await.ok();
            child.wait().await.ok();
        }
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(self.tcp_reachable().await)
    }

    fn connection_info(&self) -> Option<&ConnectionInfo> {
        self.conn_info.as_ref()
    }
}
