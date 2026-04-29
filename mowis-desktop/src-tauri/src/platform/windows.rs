// platform/windows.rs — Windows launcher
//
// Strategy (tried in order):
//
//   1. WSL2 primary — fastest, uses the Linux kernel already on the machine.
//      a. Detect WSL2 availability (`wsl.exe --status`).
//      b. Ensure Alpine Linux distro is installed (`wsl.exe --install Alpine`).
//      c. Install agentd binary inside Alpine if not present.
//      d. Start agentd inside Alpine on Unix socket /tmp/agentd.sock.
//      e. Bridge the socket out via a TCP relay on localhost:9722 using socat
//         (WSL2 automatically forwards WSL localhost ports to Windows).
//      f. Connect Windows side to 127.0.0.1:9722 with auth token.
//
//   2. QEMU/WHPX fallback — if WSL2 is unavailable or install fails.
//      Same as macOS launcher but with -accel whpx.
//
// Auth: 256-bit token written to %APPDATA%\MowisAI\token; injected into
//       WSL2 via env var, into QEMU via -fw_cfg.
//
// On first run the user will see a UAC prompt; wsl --install needs it.
// We surface a clear "Installing Linux environment…" progress screen.

use crate::platform::auth;
use crate::platform::connection::is_tcp_reachable;
use crate::platform::qemu::{QemuConfig, QemuLauncher};
use crate::platform::{ConnectionInfo, ConnectionKind, VmLauncher};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::sleep;

const WSL_DISTRO: &str = "Alpine";
const AGENT_TCP_PORT: u16 = 9722;
const AGENT_TCP_ADDR: &str = "127.0.0.1:9722";

fn qemu_image_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("MowisAI")
        .join("alpine-agentd.qcow2")
}

pub struct WindowsLauncher {
    qemu_fallback: QemuLauncher,
    wsl2_available: Mutex<Option<bool>>, // cached detection result
}

impl WindowsLauncher {
    pub fn new() -> Self {
        let cfg = QemuConfig::windows_whpx(qemu_image_path());
        Self {
            qemu_fallback: QemuLauncher::new(cfg),
            wsl2_available: Mutex::new(None),
        }
    }

    // ── WSL2 detection ───────────────────────────────────────────────────────

    async fn detect_wsl2(&self) -> bool {
        {
            let cached = self.wsl2_available.lock().unwrap();
            if let Some(v) = *cached {
                return v;
            }
        }
        let ok = Command::new("wsl.exe")
            .args(["--status"])
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);
        *self.wsl2_available.lock().unwrap() = Some(ok);
        ok
    }

    // ── Alpine distro install ─────────────────────────────────────────────────

    async fn ensure_alpine_distro(&self) -> Result<()> {
        // Check if Alpine is already registered.
        let list = Command::new("wsl.exe")
            .args(["--list", "--quiet"])
            .output()
            .await
            .context("wsl --list")?;
        let installed = String::from_utf8_lossy(&list.stdout)
            .lines()
            .any(|l| l.trim().eq_ignore_ascii_case(WSL_DISTRO));

        if installed {
            return Ok(());
        }

        log::info!("Installing Alpine Linux in WSL2…");
        let status = Command::new("wsl.exe")
            .args(["--install", "--distribution", WSL_DISTRO, "--no-launch"])
            .status()
            .await
            .context("wsl --install Alpine")?;
        if !status.success() {
            anyhow::bail!("wsl --install Alpine failed with {status}");
        }
        Ok(())
    }

    // ── agentd setup inside Alpine ────────────────────────────────────────────

    async fn ensure_agentd_in_alpine(&self, token: &str) -> Result<()> {
        // Check if agentd binary exists in the distro.
        let check = Command::new("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "test", "-x", "/usr/local/bin/agentd"])
            .status()
            .await
            .context("checking agentd in WSL2")?;

        if !check.success() {
            // Download / install agentd inside Alpine.
            // In production this would pull from a release URL; for now we
            // copy the binary from the Tauri resources directory.
            log::info!("Installing agentd inside Alpine WSL2…");
            let install = Command::new("wsl.exe")
                .args([
                    "-d", WSL_DISTRO, "--",
                    "sh", "-c",
                    // Minimal Alpine setup: apk add socat, then place agentd.
                    "apk add --no-cache socat curl && \
                     curl -Lo /usr/local/bin/agentd \
                       https://github.com/Momin010/MowisAI/releases/latest/download/agentd-linux-x86_64 && \
                     chmod +x /usr/local/bin/agentd",
                ])
                .status()
                .await
                .context("installing agentd in Alpine")?;
            if !install.success() {
                anyhow::bail!("Failed to install agentd inside Alpine WSL2");
            }
        }

        // Write the token into a file the daemon can read.
        let token_cmd = format!(
            "mkdir -p /root/.mowisai && echo -n '{}' > /root/.mowisai/token && chmod 600 /root/.mowisai/token",
            token
        );
        Command::new("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "sh", "-c", &token_cmd])
            .status()
            .await
            .context("writing token into Alpine")?;

        Ok(())
    }

    // ── Start agentd + socat TCP relay ────────────────────────────────────────

    async fn start_wsl2_bridge(&self, token: &str) -> Result<()> {
        self.ensure_agentd_in_alpine(token).await?;

        // Kill any stale relay from a previous run.
        let _ = Command::new("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "pkill", "-f", "agentd"])
            .status()
            .await;
        let _ = Command::new("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "pkill", "-f", "socat"])
            .status()
            .await;

        // Start agentd inside Alpine.
        Command::new("wsl.exe")
            .args([
                "-d", WSL_DISTRO, "--",
                "sh", "-c",
                "nohup /usr/local/bin/agentd socket --path /tmp/agentd.sock \
                 </dev/null >>/var/log/agentd.log 2>&1 &",
            ])
            .status()
            .await
            .context("starting agentd in WSL2")?;

        // Give agentd a moment to create its socket.
        sleep(Duration::from_secs(2)).await;

        // Bridge the Unix socket to a TCP port that Windows can reach.
        // WSL2 automatically forwards localhost ports from the Linux VM to Windows.
        Command::new("wsl.exe")
            .args([
                "-d", WSL_DISTRO, "--",
                "sh", "-c",
                &format!(
                    "nohup socat TCP-LISTEN:{port},reuseaddr,fork \
                     UNIX-CONNECT:/tmp/agentd.sock \
                     </dev/null >>/var/log/socat.log 2>&1 &",
                    port = AGENT_TCP_PORT
                ),
            ])
            .status()
            .await
            .context("starting socat relay in WSL2")?;

        Ok(())
    }

    // ── Wait for TCP relay to be reachable ────────────────────────────────────

    async fn wait_for_bridge(&self) -> Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if std::time::Instant::now() > deadline {
                anyhow::bail!("Timed out waiting for WSL2 TCP bridge on {AGENT_TCP_ADDR}");
            }
            if is_tcp_reachable(AGENT_TCP_ADDR).await {
                return Ok(());
            }
            sleep(Duration::from_millis(500)).await;
        }
    }
}

#[async_trait]
impl VmLauncher for WindowsLauncher {
    fn name(&self) -> &str { "Windows/WSL2+Alpine" }

    async fn start(&self) -> Result<ConnectionInfo> {
        let token = auth::load_or_create().context("load/create auth token")?;

        if self.detect_wsl2().await {
            log::info!("WSL2 detected — using Alpine Linux distro");
            self.ensure_alpine_distro()
                .await
                .context("ensure Alpine distro")?;
            self.start_wsl2_bridge(&token)
                .await
                .context("start WSL2 bridge")?;
            self.wait_for_bridge().await.context("wait for bridge")?;

            return Ok(ConnectionInfo {
                kind: ConnectionKind::TcpWithToken,
                socket_path: None,
                tcp_addr: Some(AGENT_TCP_ADDR.into()),
                pipe_name: None,
                auth_token: Some(token),
            });
        }

        // ── QEMU/WHPX fallback ───────────────────────────────────────────────
        log::warn!("WSL2 not available — falling back to QEMU/WHPX");
        let child = self.qemu_fallback
            .spawn_process(&token, true)
            .await
            .context("spawn QEMU/WHPX")?;
        drop(child); // process is detached; keep running

        self.qemu_fallback
            .wait_for_agent()
            .await
            .context("wait for QEMU agent bridge")?;

        Ok(ConnectionInfo {
            kind: ConnectionKind::TcpWithToken,
            socket_path: None,
            tcp_addr: Some(self.qemu_fallback.agent_tcp().to_owned()),
            pipe_name: None,
            auth_token: Some(token),
        })
    }

    async fn stop(&self) -> Result<()> {
        // Stop agentd and socat inside WSL2.
        let _ = Command::new("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "pkill", "-f", "agentd"])
            .status()
            .await;
        let _ = Command::new("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "pkill", "-f", "socat"])
            .status()
            .await;
        // Also stop QEMU if it was started as fallback.
        let _ = self.qemu_fallback.quit().await;
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        if self.detect_wsl2().await {
            return Ok(is_tcp_reachable(AGENT_TCP_ADDR).await);
        }
        Ok(is_tcp_reachable(self.qemu_fallback.agent_tcp()).await)
    }
}
