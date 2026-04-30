// platform/windows.rs — Windows launcher
//
// Strategy (tried in order):
//
//   1. WSL2 primary — fastest, uses the Linux kernel already on the machine.
//      a. Detect WSL2 (wsl.exe --list — reliable even offline).
//      b. Ensure Alpine Linux distro is installed (wsl --install Alpine).
//      c. Copy the agentd-linux-x86_64 binary that was installed next to the
//         exe by the NSIS installer into Alpine at /usr/local/bin/agentd.
//      d. Install socat inside Alpine (apk add socat).
//      e. Start agentd inside Alpine: nohup agentd socket --path /tmp/agentd.sock
//      f. Bridge the Unix socket out via TCP on localhost:9722 using socat.
//         WSL2 automatically forwards Linux localhost ports to Windows.
//      g. Connect from the Windows side to 127.0.0.1:9722 with auth token.
//
//   2. QEMU/WHPX fallback — if WSL2 is unavailable or Alpine install fails.
//      Same image as macOS launcher but with -accel whpx.
//
// Binary bundling: The Tauri NSIS installer places agentd-linux-x86_64 next
// to MowisAI.exe (via tauri.conf.json "resources"). At runtime we locate it
// with find_bundled_agentd() and copy it into the WSL2 distro over the
// automatic /mnt/<drive>/ filesystem mapping.

use crate::platform::auth;
use crate::platform::connection::is_tcp_reachable;
use crate::platform::qemu::{QemuConfig, QemuLauncher};
use crate::platform::{ConnectionInfo, ConnectionKind, VmLauncher};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::sleep;

const WSL_DISTRO: &str = "Alpine";
const AGENT_TCP_PORT: u16 = 9722;
const AGENT_TCP_ADDR: &str = "127.0.0.1:9722";
const AGENTD_BIN: &str = "agentd-linux-x86_64";

fn qemu_image_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("MowisAI")
        .join("alpine-agentd.qcow2")
}

// ── Path helpers ──────────────────────────────────────────────────────────────

/// Convert a Windows absolute path to the equivalent WSL2 /mnt/<drive>/... path.
/// "C:\Program Files\MowisAI\agentd" → "/mnt/c/Program Files/MowisAI/agentd"
fn windows_to_wsl_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut chars = s.chars();
    if let (Some(drive), Some(':')) = (chars.next(), chars.next()) {
        let rest = s[2..].replace('\\', "/");
        let rest = rest.trim_start_matches('/');
        return format!("/mnt/{}/{}", drive.to_ascii_lowercase(), rest);
    }
    s.replace('\\', "/")
}

/// Locate the agentd binary bundled by the installer.
/// Searches: next to the exe, a "resources/" sub-dir, and the Cargo workspace
/// root (handy during `cargo tauri dev`).
fn find_bundled_agentd() -> Option<PathBuf> {
    // 1. Next to the running exe (production NSIS install)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(AGENTD_BIN);
            if p.exists() { return Some(p); }
            // Some Tauri versions copy resources into a sub-dir
            let p2 = dir.join("resources").join(AGENTD_BIN);
            if p2.exists() { return Some(p2); }
        }
    }
    // 2. Workspace root during cargo dev builds
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let p = PathBuf::from(&manifest).join("..").join("..").join(AGENTD_BIN);
        if let Ok(canon) = p.canonicalize() {
            if canon.exists() { return Some(canon); }
        }
    }
    None
}

/// Decode the stdout of `wsl --list --quiet`, which is UTF-16LE on Windows.
fn decode_wsl_list(raw: &[u8]) -> String {
    // UTF-16LE BOM or every-other-byte being 0x00 → it's UTF-16LE
    let is_utf16 = raw.starts_with(&[0xFF, 0xFE])
        || (raw.len() >= 4 && raw[1] == 0 && raw[3] == 0);
    if is_utf16 {
        let words: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&words)
    } else {
        String::from_utf8_lossy(raw).into_owned()
    }
}

// ── WindowsLauncher ───────────────────────────────────────────────────────────

pub struct WindowsLauncher {
    qemu_fallback: QemuLauncher,
    /// Cached WSL2 availability check so we only run wsl.exe once.
    wsl2_available: Mutex<Option<bool>>,
}

impl WindowsLauncher {
    pub fn new() -> Self {
        let cfg = QemuConfig::windows_whpx(qemu_image_path());
        Self {
            qemu_fallback: QemuLauncher::new(cfg),
            wsl2_available: Mutex::new(None),
        }
    }

    // ── WSL2 detection ────────────────────────────────────────────────────────

    /// Returns true if WSL2 is enabled and wsl.exe is available.
    /// Uses `--list` (not `--status`) because --status requires internet access.
    async fn detect_wsl2(&self) -> bool {
        {
            let cached = self.wsl2_available.lock().unwrap();
            if let Some(v) = *cached { return v; }
        }
        let ok = Command::new("wsl.exe")
            .args(["--list"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);
        *self.wsl2_available.lock().unwrap() = Some(ok);
        ok
    }

    // ── Alpine distro install ─────────────────────────────────────────────────

    async fn ensure_alpine_distro(&self) -> Result<()> {
        let list = Command::new("wsl.exe")
            .args(["--list", "--quiet"])
            .output()
            .await
            .context("running wsl --list")?;

        let distros = decode_wsl_list(&list.stdout);
        let already_installed = distros
            .lines()
            .any(|l| l.trim().eq_ignore_ascii_case(WSL_DISTRO));

        if already_installed {
            log::info!("Alpine WSL2 distro already installed");
            return Ok(());
        }

        log::info!("Installing Alpine Linux via WSL2 (may show a UAC prompt)…");
        let status = Command::new("wsl.exe")
            .args(["--install", "--distribution", WSL_DISTRO, "--no-launch"])
            .status()
            .await
            .context("wsl --install Alpine")?;

        if !status.success() {
            anyhow::bail!(
                "Failed to install Alpine Linux via WSL2 (exit {}). \
                 Open PowerShell as Administrator, run: wsl --install \
                 then restart MowisAI.",
                status
            );
        }

        // Give the newly-registered distro time to finish first-boot init.
        sleep(Duration::from_secs(4)).await;
        Ok(())
    }

    // ── Copy agentd + socat into Alpine ──────────────────────────────────────

    async fn ensure_agentd_in_alpine(&self, token: &str) -> Result<()> {
        // Install socat — always run, it's idempotent and fast.
        let _ = Command::new("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "apk", "add", "--no-cache", "socat"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;

        // Check if agentd is already in Alpine.
        let already = Command::new("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "test", "-x", "/usr/local/bin/agentd"])
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);

        if !already {
            // Find the binary bundled by the NSIS installer next to our exe.
            let agentd_win = find_bundled_agentd().ok_or_else(|| {
                anyhow::anyhow!(
                    "The agentd engine binary was not found next to the application. \
                     Please reinstall MowisAI to restore bundled components. \
                     Expected: {}", AGENTD_BIN
                )
            })?;

            log::info!("Copying {} → Alpine /usr/local/bin/agentd", agentd_win.display());

            // WSL2 mounts Windows drives at /mnt/<drive>/.
            let wsl_src = windows_to_wsl_path(&agentd_win);

            // Single shell command: cp + chmod.
            let copy_cmd = format!(
                "cp '{}' /usr/local/bin/agentd && chmod +x /usr/local/bin/agentd",
                wsl_src.replace('\'', "\\'")
            );
            let status = Command::new("wsl.exe")
                .args(["-d", WSL_DISTRO, "--", "sh", "-c", &copy_cmd])
                .status()
                .await
                .context("copying agentd binary into Alpine")?;

            if !status.success() {
                anyhow::bail!(
                    "Failed to copy agentd into Alpine WSL2. \
                     Source path: {} (WSL path: {})",
                    agentd_win.display(),
                    wsl_src
                );
            }
            log::info!("agentd installed in Alpine successfully");
        }

        // Write the auth token into the distro so agentd can verify clients.
        let token_cmd = format!(
            "mkdir -p /root/.mowisai && \
             printf '%s' '{}' > /root/.mowisai/token && \
             chmod 600 /root/.mowisai/token",
            token.replace('\'', "\\'")
        );
        Command::new("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "sh", "-c", &token_cmd])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .context("writing auth token into Alpine")?;

        Ok(())
    }

    // ── Start agentd + socat TCP relay ────────────────────────────────────────

    async fn start_wsl2_bridge(&self, token: &str) -> Result<()> {
        self.ensure_agentd_in_alpine(token).await?;

        // Kill any stale processes from a previous session (best-effort).
        for proc in &["agentd", "socat"] {
            let _ = Command::new("wsl.exe")
                .args(["-d", WSL_DISTRO, "--", "pkill", "-f", proc])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
        sleep(Duration::from_millis(600)).await;

        // Start agentd in the background.
        Command::new("wsl.exe")
            .args([
                "-d", WSL_DISTRO, "--", "sh", "-c",
                "nohup /usr/local/bin/agentd socket --path /tmp/agentd.sock \
                 </dev/null >>/var/log/agentd.log 2>&1 &",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .context("starting agentd inside Alpine")?;

        // Give agentd time to create and bind its socket.
        sleep(Duration::from_secs(2)).await;

        // Bridge the Unix socket out to a TCP port Windows can reach.
        // WSL2 automatically makes Linux localhost ports accessible on Windows.
        let socat_cmd = format!(
            "nohup socat TCP-LISTEN:{port},reuseaddr,fork \
             UNIX-CONNECT:/tmp/agentd.sock \
             </dev/null >>/var/log/socat.log 2>&1 &",
            port = AGENT_TCP_PORT
        );
        Command::new("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "sh", "-c", &socat_cmd])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .context("starting socat TCP relay inside Alpine")?;

        Ok(())
    }

    // ── Wait for bridge ───────────────────────────────────────────────────────

    async fn wait_for_bridge(&self) -> Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if std::time::Instant::now() > deadline {
                anyhow::bail!(
                    "Timed out waiting for agentd TCP bridge on {}. \
                     To inspect: open WSL2 Alpine and run: \
                     cat /var/log/agentd.log",
                    AGENT_TCP_ADDR
                );
            }
            if is_tcp_reachable(AGENT_TCP_ADDR).await {
                return Ok(());
            }
            sleep(Duration::from_millis(500)).await;
        }
    }
}

// ── VmLauncher impl ───────────────────────────────────────────────────────────

#[async_trait]
impl VmLauncher for WindowsLauncher {
    fn name(&self) -> &str { "Windows/WSL2+Alpine" }

    async fn start(&self) -> Result<ConnectionInfo> {
        let token = auth::load_or_create().context("load/create auth token")?;

        if self.detect_wsl2().await {
            log::info!("WSL2 available — using Alpine Linux distro");

            self.ensure_alpine_distro()
                .await
                .context("ensuring Alpine WSL2 distro")?;

            self.start_wsl2_bridge(&token)
                .await
                .context("starting WSL2 bridge")?;

            self.wait_for_bridge()
                .await
                .context("waiting for agentd TCP bridge")?;

            return Ok(ConnectionInfo {
                kind: ConnectionKind::TcpWithToken,
                socket_path: None,
                tcp_addr: Some(AGENT_TCP_ADDR.into()),
                pipe_name: None,
                auth_token: Some(token),
            });
        }

        // ── QEMU/WHPX fallback ─────────────────────────────────────────────
        log::warn!("WSL2 not available — falling back to QEMU/WHPX");
        let child = self.qemu_fallback
            .spawn_process(&token, true)
            .await
            .context("spawning QEMU/WHPX")?;
        drop(child); // detached; continues running

        self.qemu_fallback
            .wait_for_agent()
            .await
            .context("waiting for QEMU agentd bridge")?;

        Ok(ConnectionInfo {
            kind: ConnectionKind::TcpWithToken,
            socket_path: None,
            tcp_addr: Some(self.qemu_fallback.agent_tcp().to_owned()),
            pipe_name: None,
            auth_token: Some(token),
        })
    }

    async fn stop(&self) -> Result<()> {
        for proc in &["agentd", "socat"] {
            let _ = Command::new("wsl.exe")
                .args(["-d", WSL_DISTRO, "--", "pkill", "-f", proc])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
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
}
