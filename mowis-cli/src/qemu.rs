// qemu.rs — QEMU VM launcher with full debug logging

use anyhow::{bail, Context, Result};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};

#[derive(Debug, Clone)]
pub struct QemuConfig {
    pub qemu_bin: PathBuf,
    pub image_path: PathBuf,
    pub agent_tcp: String,
    pub monitor_tcp: String,
    pub accel: &'static str,
    pub ram_mb: u32,
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

pub struct QemuLauncher {
    pub config: QemuConfig,
    snapshot_exists: std::sync::atomic::AtomicBool,
}

impl QemuLauncher {
    pub fn new(config: QemuConfig) -> Self {
        Self {
            config,
            snapshot_exists: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn build_args(&self, token: &str, load_snapshot: bool) -> Vec<String> {
        let cfg = &self.config;
        let agent_port = cfg.agent_tcp.split(':').nth(1).unwrap_or("9722");
        let mut args = vec![
            "-nographic".into(),
            "-machine".into(), "q35".into(),
            "-cpu".into(), "host".into(),
            "-accel".into(), cfg.accel.into(),
            "-m".into(), cfg.ram_mb.to_string(),
            "-smp".into(), cfg.vcpus.to_string(),
            "-drive".into(), format!("if=virtio,file={},format=qcow2", cfg.image_path.display()),
            "-device".into(), "virtio-serial".into(),
            "-chardev".into(), format!("socket,id=agentd0,host=127.0.0.1,port={},server=on,wait=off", agent_port),
            "-device".into(), "virtconsole,chardev=agentd0,name=agentd0".into(),
            "-monitor".into(), format!("tcp:{},server=on,wait=off", cfg.monitor_tcp),
            "-fw_cfg".into(), format!("opt/mowis/token,string={}", token),
            "-vga".into(), "none".into(),
        ];
        if load_snapshot {
            args.push("-loadvm".into());
            args.push("mowis-snap".into());
        }
        log::debug!("[qemu] Full args: {:?}", args);
        args
    }

    pub async fn spawn_process(&self, token: &str, load_snapshot: bool) -> Result<Child> {
        let use_snap = load_snapshot
            && self.snapshot_exists.load(std::sync::atomic::Ordering::Relaxed);

        log::info!("[qemu] Spawning: {} {:?}", self.config.qemu_bin.display(), self.build_args(token, use_snap));

        let mut cmd = Command::new(&self.config.qemu_bin);
        cmd.args(self.build_args(token, use_snap))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn().context("spawning QEMU process")?;

        log::info!("[qemu] Process spawned, PID: {:?}", child.id());

        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    log::info!("[QEMU stdout] {}", line);
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    log::warn!("[QEMU stderr] {}", line);
                }
            });
        }

        Ok(child)
    }

    pub async fn wait_for_agent(&self) -> Result<()> {
        let addr: SocketAddr = self.config.agent_tcp.parse().context("parse agent_tcp")?;
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        let mut checks = 0u32;
        loop {
            if std::time::Instant::now() > deadline {
                bail!(
                    "Timed out waiting for agentd serial bridge on {} after 90s.\n\
                     QEMU started but the VM did not respond.\n\
                     Check: disk image={}, accel={}",
                    self.config.agent_tcp, self.config.image_path.display(), self.config.accel
                );
            }
            checks += 1;
            if checks % 4 == 0 {
                log::info!("[qemu] Waiting for serial bridge… ({}s)", checks / 2);
            }
            if timeout(Duration::from_secs(1), TcpStream::connect(addr)).await
                .map(|r| r.is_ok()).unwrap_or(false)
            {
                log::info!("[qemu] Serial bridge connected on {}", self.config.agent_tcp);
                return Ok(());
            }
            sleep(Duration::from_millis(500)).await;
        }
    }

    pub async fn hmp_command(&self, cmd: &str) -> Result<String> {
        log::debug!("[qemu] HMP command: {}", cmd);
        let mut stream = TcpStream::connect(&self.config.monitor_tcp).await
            .context("connect to QEMU HMP monitor")?;
        stream.write_all(format!("{}\n", cmd).as_bytes()).await?;
        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response).await?;
        log::debug!("[qemu] HMP response: {}", response.trim());
        Ok(response.trim().to_owned())
    }

    pub async fn save_snapshot(&self) -> Result<()> {
        self.hmp_command("savevm mowis-snap").await?;
        self.snapshot_exists.store(true, std::sync::atomic::Ordering::Relaxed);
        log::info!("[qemu] Snapshot saved");
        Ok(())
    }

    pub async fn quit(&self) -> Result<()> {
        let _ = self.save_snapshot().await;
        let _ = self.hmp_command("quit").await;
        log::info!("[qemu] Quit sent");
        Ok(())
    }

    pub fn agent_tcp(&self) -> &str {
        &self.config.agent_tcp
    }
}
