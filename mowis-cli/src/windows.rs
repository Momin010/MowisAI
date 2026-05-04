// windows.rs — Windows WSL2 + QEMU/WHPX fallback launcher with full debug logging

use crate::auth;
use crate::connection::is_tcp_reachable;
use crate::developer_mode::{DeveloperConfig, DeveloperLauncher};
use crate::qemu::*;
use crate::types::*;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{sleep, Duration};

const WSL_DISTRO: &str = "MowisAI";
const AGENT_TCP_ADDR: &str = "127.0.0.1:9722";
const AGENT_SOCKET: &str = "/tmp/agentd.sock";
const WSL_BRIDGE_TIMEOUT: u64 = 60;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn strip_unc(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with(r"\\?\") { PathBuf::from(&s[4..]) } else { path }
}

fn find_bundled_file(name: &str) -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        let exe = strip_unc(exe);
        if let Some(dir) = exe.parent() {
            let p = dir.join(name);
            if p.exists() { log::debug!("[win] Found bundled: {}", p.display()); return Some(p); }
            let p2 = dir.join("resources").join(name);
            if p2.exists() { log::debug!("[win] Found bundled (resources): {}", p2.display()); return Some(p2); }
        }
    }
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let p = PathBuf::from(&manifest).join("..").join(name);
        if p.exists() { log::debug!("[win] Found bundled (workspace): {}", p.display()); return Some(p); }
    }
    None
}

async fn win_cmd(exe: &str) -> Command {
    let mut cmd = Command::new(exe);
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

fn agentd_path() -> PathBuf {
    find_bundled_file("agentd-linux-x86_64")
        .unwrap_or_else(|| {
            let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
            base.join("MowisAI").join("agentd-linux-x86_64")
        })
}

fn wsl_distro_exists() -> bool {
    log::debug!("[win] Checking if WSL distro '{}' exists…", WSL_DISTRO);
    std::process::Command::new("wsl.exe")
        .args(["-l"])
        .creation_flags(0x0800_0000)
        .output()
        .map(|o| {
            let text = String::from_utf8_lossy(&o.stdout);
            let exists = text.contains(WSL_DISTRO);
            log::debug!("[win] WSL distros: {:?}, exists={}", text, exists);
            exists
        })
        .unwrap_or(false)
}

// ── Launcher ──────────────────────────────────────────────────────────────────

pub struct WindowsLauncher {
    qemu_fallback: QemuLauncher,
}

impl WindowsLauncher {
    pub fn new() -> Self {
        let qemu_image = find_bundled_file("alpine-mowisai.qcow2")
            .unwrap_or_else(|| {
                let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
                base.join("MowisAI").join("alpine-mowisai.qcow2")
            });
        log::info!("[win] QEMU fallback image: {}", qemu_image.display());
        Self {
            qemu_fallback: QemuLauncher::new(QemuConfig::windows_whpx(qemu_image)),
        }
    }

    async fn detect_wsl2(&self) -> bool {
        log::debug!("[win] Detecting WSL2…");
        if !wsl_distro_exists() {
            log::info!("[win] WSL distro '{}' not found", WSL_DISTRO);
            return false;
        }
        let result = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "uname", "-r"])
            .output().await;
        match result {
            Ok(o) if o.status.success() => {
                let kernel = String::from_utf8_lossy(&o.stdout).trim().to_string();
                let wsl2 = kernel.contains("microsoft") || kernel.contains("WSL");
                log::info!("[win] WSL kernel: '{}', is_wsl2={}", kernel, wsl2);
                wsl2
            }
            _ => {
                log::info!("[win] WSL uname failed");
                false
            }
        }
    }

    async fn read_alpine_logs(&self) -> String {
        let Ok(output) = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "cat", "/var/log/agentd.log"])
            .output().await else { return "Could not read logs".into() };
        String::from_utf8_lossy(&output.stdout).to_string()
    }
}

#[async_trait]
impl VmLauncher for WindowsLauncher {
    async fn start(&self, progress: Option<ProgressSender>) -> Result<ConnectionInfo> {
        let pw = &progress;
        let token = auth::load_or_create().context("load auth token")?;

        emit(pw, "detecting", "Windows platform — checking WSL2…", 5, "info",
            Some(format!("Distro: {}, TCP: {}", WSL_DISTRO, AGENT_TCP_ADDR))).await;

        // ── Try WSL2 first ────────────────────────────────────────────────────
        if self.detect_wsl2().await {
            emit(pw, "detecting", "WSL2 detected", 10, "success",
                Some(format!("Distro: {}", WSL_DISTRO))).await;
            return self.start_wsl2(&token, pw).await;
        }

        // ── Try Developer Mode ────────────────────────────────────────────────
        if DeveloperLauncher::is_configured() {
            emit(pw, "detecting", "WSL2 not found — Developer Mode config detected", 12, "info", None).await;
            let config = DeveloperConfig::load_or_default();
            log::info!("[win] Using Developer Mode: qemu={}, iso={}", config.qemu_path.display(), config.iso_path.display());
            let dev = DeveloperLauncher::new(config);
            return dev.start(progress).await;
        }

        // ── Fall back to QEMU/WHPX ────────────────────────────────────────────
        emit(pw, "detecting", "WSL2 not found — trying QEMU/WHPX fallback…", 12, "warning", None).await;
        self.start_qemu_fallback(&token, pw).await
    }

    async fn stop(&self) -> Result<()> {
        for proc in &["agentd", "socat"] {
            let _ = win_cmd("wsl.exe")
                .args(["-d", WSL_DISTRO, "--", "pkill", "-f", proc])
                .stdout(Stdio::null()).stderr(Stdio::null())
                .status().await;
        }
        let _ = self.qemu_fallback.quit().await;
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        if self.detect_wsl2().await {
            return Ok(is_tcp_reachable(AGENT_TCP_ADDR).await);
        }
        Ok(is_tcp_reachable(self.qemu_fallback.agent_tcp()).await)
    }

    fn name(&self) -> &str { "Windows" }

    async fn read_logs(&self) -> String {
        self.read_alpine_logs().await
    }
}

// ── WSL2 boot path ───────────────────────────────────────────────────────────

impl WindowsLauncher {
    async fn start_wsl2(&self, token: &str, pw: &Option<ProgressSender>) -> Result<ConnectionInfo> {
        // Ensure agentd is running
        emit(pw, "booting", "Starting agentd in WSL2…", 15, "command",
            Some(format!("wsl -d {} -- {}", WSL_DISTRO, agentd_path().display()))).await;

        let agent = agentd_path();
        log::info!("[win/wsl] Agent binary: {}", agent.display());

        let _ = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "mkdir", "-p", "/var/log"])
            .status().await;

        // Start agentd
        let mut start_cmd = win_cmd("wsl.exe");
        start_cmd.args(["-d", WSL_DISTRO, "--"])
            .arg(agent.to_string_lossy().as_ref())
            .args(["socket", "--path", AGENT_SOCKET]);
        log::debug!("[win/wsl] Starting agentd: {:?}", start_cmd);

        let _child = start_cmd
            .stdout(Stdio::null()).stderr(Stdio::null())
            .spawn().context("spawn agentd in WSL2")?;

        // Write auth token
        emit(pw, "booting", "Writing auth token to WSL2…", 20, "command",
            Some(format!("printf '{}' > /root/.mowisai/token", token))).await;

        let _ = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "bash", "-c", &format!(
                "mkdir -p /root/.mowisai && printf '{}' > /root/.mowisai/token && chmod 600 /root/.mowisai/token",
                token.replace('\'', "\\'")
            )]).status().await;

        // Start socat bridge
        emit(pw, "booting", "Starting socat bridge (TCP:9722 → Unix socket)…", 30, "command",
            Some(format!("socat TCP-LISTEN:9722,fork,reuseaddr UNIX-CONNECT:{}", AGENT_SOCKET))).await;

        let _ = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "pkill", "-f", "socat TCP-LISTEN"])
            .stdout(Stdio::null()).stderr(Stdio::null())
            .status().await;

        let _ = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "bash", "-c", &format!(
                "nohup socat TCP-LISTEN:9722,fork,reuseaddr UNIX-CONNECT:{} &",
                AGENT_SOCKET
            )]).stdout(Stdio::null()).stderr(Stdio::null())
            .status().await;

        emit(pw, "booting", "Waiting for TCP bridge…", 50, "info",
            Some(format!("Probing {}…", AGENT_TCP_ADDR))).await;

        // Wait for bridge
        let deadline = std::time::Instant::now() + Duration::from_secs(WSL_BRIDGE_TIMEOUT);
        let mut waited = 0u64;
        while std::time::Instant::now() < deadline {
            if is_tcp_reachable(AGENT_TCP_ADDR).await {
                emit(pw, "ready", "WSL2 bridge active", 100, "success",
                    Some(format!("TCP: {}", AGENT_TCP_ADDR))).await;
                return Ok(ConnectionInfo {
                    kind: ConnectionKind::TcpWithToken,
                    socket_path: None,
                    tcp_addr: Some(AGENT_TCP_ADDR.to_owned()),
                    pipe_name: None,
                    auth_token: Some(token.to_owned()),
                });
            }
            sleep(Duration::from_secs(1)).await;
            waited += 1;
            if waited % 5 == 0 {
                emit(pw, "booting", &format!("Still waiting for bridge ({}s)…", waited), 50, "info", None).await;
            }
        }

        anyhow::bail!("WSL2 bridge did not become reachable on {} after {}s", AGENT_TCP_ADDR, WSL_BRIDGE_TIMEOUT)
    }

    async fn start_qemu_fallback(&self, token: &str, pw: &Option<ProgressSender>) -> Result<ConnectionInfo> {
        let image = self.qemu_fallback.config.image_path.clone();
        if !image.exists() {
            emit(pw, "error", "QEMU disk image not found", 0, "error",
                Some(format!("{}\nUse Developer Mode to bootstrap automatically.", image.display()))).await;
            anyhow::bail!("QEMU image not found: {}", image.display());
        }

        emit(pw, "booting", "Starting QEMU/WHPX…", 20, "command",
            Some(format!("qemu-system-x86_64.exe -accel whpx -m 1024"))).await;

        let _child = self.qemu_fallback.spawn_process(token, true).await
            .context("spawn QEMU/WHPX")?;

        emit(pw, "booting", "QEMU spawned, waiting for serial bridge…", 40, "info", None).await;
        self.qemu_fallback.wait_for_agent().await.context("wait for agent")?;

        emit(pw, "ready", "QEMU/WHPX ready", 100, "success",
            Some(format!("TCP: {}", self.qemu_fallback.agent_tcp()))).await;

        Ok(ConnectionInfo {
            kind: ConnectionKind::TcpWithToken,
            socket_path: None,
            tcp_addr: Some(self.qemu_fallback.agent_tcp().to_owned()),
            pipe_name: None,
            auth_token: Some(token.to_owned()),
        })
    }
}
