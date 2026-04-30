// platform/qemu.rs — QEMU-based VM launcher with HVF/WHPX acceleration,
//                    auth token injection, and save/restore snapshots.
//
// Used by:
//   macOS  — QemuLauncher::new_hvf()   (Apple HVF accelerator)
//   Windows— QemuLauncher::new_whpx()  (Windows Hypervisor Platform)
//
// IPC: a virtio-serial chardev is exposed as a TCP socket on the host.
// The VM writes to /dev/virtio-ports/agentd0 which is bridged out.
// Auth token is injected via -fw_cfg as "opt/mowis/token,string=<hex>".
//
// Snapshots: after first boot we run `savevm mowis-snap` via the HMP monitor
// (TCP port). On subsequent launches we pass `-loadvm mowis-snap` so the VM
// resumes in <5 s instead of a full boot.

use crate::platform::{auth, ConnectionInfo, ConnectionKind};
use anyhow::{bail, Context, Result};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct QemuConfig {
    /// Path to the QEMU binary.
    pub qemu_bin: PathBuf,
    /// Path to the Alpine Linux disk image.
    pub image_path: PathBuf,
    /// Host TCP address for the agentd serial bridge (e.g. "127.0.0.1:9722").
    pub agent_tcp: String,
    /// Host TCP address for the QEMU HMP monitor (e.g. "127.0.0.1:9723").
    pub monitor_tcp: String,
    /// Accelerator flag: "hvf" on macOS, "whpx" on Windows, "tcg" as fallback.
    pub accel: &'static str,
    /// RAM in MB for the VM.
    pub ram_mb: u32,
    /// vCPU count.
    pub vcpus: u32,
}

impl QemuConfig {
    pub fn macos_hvf(image_path: PathBuf) -> Self {
        Self {
            qemu_bin: PathBuf::from("qemu-system-x86_64"),
            image_path,
            agent_tcp: "127.0.0.1:9722".into(),
            monitor_tcp: "127.0.0.1:9723".into(),
            accel: "hvf",
            ram_mb: 1024,
            vcpus: 2,
        }
    }

    pub fn windows_whpx(image_path: PathBuf) -> Self {
        Self {
            qemu_bin: PathBuf::from("qemu-system-x86_64.exe"),
            image_path,
            agent_tcp: "127.0.0.1:9722".into(),
            monitor_tcp: "127.0.0.1:9723".into(),
            accel: "whpx",
            ram_mb: 1024,
            vcpus: 2,
        }
    }
}

// ── Launcher ──────────────────────────────────────────────────────────────────

pub struct QemuLauncher {
    config: QemuConfig,
    snapshot_exists: std::sync::atomic::AtomicBool,
}

impl QemuLauncher {
    pub fn new(config: QemuConfig) -> Self {
        Self {
            config,
            snapshot_exists: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Build the QEMU argument list for this launch.
    fn build_args(&self, token: &str, load_snapshot: bool) -> Vec<String> {
        let cfg = &self.config;
        let mut args = vec![
            "-nographic".into(),
            "-machine".into(), "q35".into(),
            "-cpu".into(), "host".into(),
            "-accel".into(), cfg.accel.into(),
            "-m".into(), cfg.ram_mb.to_string(),
            "-smp".into(), cfg.vcpus.to_string(),
            // Disk image
            "-drive".into(), format!("if=virtio,file={},format=qcow2", cfg.image_path.display()),
            // virtio-serial: exposes /dev/virtio-ports/agentd0 in the VM
            // and bridges it to a TCP socket on the host.
            "-device".into(), "virtio-serial".into(),
            "-chardev".into(), format!("socket,id=agentd0,host=127.0.0.1,port={},server=on,wait=off",
                cfg.agent_tcp.split(':').nth(1).unwrap_or("9722")),
            "-device".into(), "virtconsole,chardev=agentd0,name=agentd0".into(),
            // HMP monitor for savevm/loadvm
            "-monitor".into(), format!("tcp:{},server=on,wait=off", cfg.monitor_tcp),
            // Inject auth token via firmware config
            "-fw_cfg".into(), format!("opt/mowis/token,string={}", token),
            // No display
            "-vga".into(), "none".into(),
        ];
        if load_snapshot {
            args.push("-loadvm".into());
            args.push("mowis-snap".into());
        }
        args
    }

    /// Start QEMU process.
    pub async fn spawn_process(&self, token: &str, load_snapshot: bool) -> Result<Child> {
        let use_snap = load_snapshot
            && self.snapshot_exists.load(std::sync::atomic::Ordering::Relaxed);

        let mut cmd = Command::new(&self.config.qemu_bin);
        cmd.args(self.build_args(token, use_snap))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);

        cmd.spawn().context("spawning QEMU process")
    }

    /// Wait until the TCP serial bridge is accepting connections (up to 30 s).
    pub async fn wait_for_agent(&self) -> Result<()> {
        let addr: SocketAddr = self.config.agent_tcp
            .parse()
            .context("parse agent_tcp address")?;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if std::time::Instant::now() > deadline {
                bail!("Timed out waiting for agentd serial bridge on {}", self.config.agent_tcp);
            }
            if timeout(Duration::from_secs(1), TcpStream::connect(addr))
                .await
                .map(|r| r.is_ok())
                .unwrap_or(false)
            {
                return Ok(());
            }
            sleep(Duration::from_millis(500)).await;
        }
    }

    /// Send a command to the QEMU HMP monitor and read one line of response.
    async fn hmp_command(&self, cmd: &str) -> Result<String> {
        let mut stream = TcpStream::connect(&self.config.monitor_tcp)
            .await
            .context("connect to QEMU HMP monitor")?;
        let line = format!("{}\n", cmd);
        stream.write_all(line.as_bytes()).await?;
        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response).await?;
        Ok(response.trim().to_owned())
    }

    /// Save the current VM state so the next boot can resume from it.
    pub async fn save_snapshot(&self) -> Result<()> {
        self.hmp_command("savevm mowis-snap").await?;
        self.snapshot_exists.store(true, std::sync::atomic::Ordering::Relaxed);
        log::info!("QEMU snapshot saved (mowis-snap)");
        Ok(())
    }

    /// Gracefully stop QEMU (save first, then quit).
    pub async fn quit(&self) -> Result<()> {
        let _ = self.save_snapshot().await; // best-effort
        let _ = self.hmp_command("quit").await;
        Ok(())
    }

    pub fn agent_tcp(&self) -> &str {
        &self.config.agent_tcp
    }
}
