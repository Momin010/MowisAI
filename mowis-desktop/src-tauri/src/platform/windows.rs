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

// Suppress the brief console window that appears when spawning wsl.exe from a GUI process.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const WSL_DISTRO: &str = "Alpine";
const AGENT_TCP_PORT: u16 = 9722;
const AGENT_TCP_ADDR: &str = "127.0.0.1:9722";
const AGENTD_BIN: &str = "agentd-linux-x86_64";

/// Create a `wsl.exe` Command with the console window suppressed on Windows.
fn wsl_cmd() -> Command {
    let mut cmd = Command::new("wsl.exe");
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

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
        let ok = wsl_cmd()
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
        let list = wsl_cmd()
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
        let out = wsl_cmd()
            .args(["--install", "--distribution", WSL_DISTRO, "--no-launch"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("wsl --install Alpine")?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let detail = if stderr.is_empty() { stdout } else { stderr };
            anyhow::bail!(
                "Failed to install Alpine Linux via WSL2 (exit {}).\n\
                 Output: {}\n\
                 Fix: open PowerShell as Administrator and run: wsl --install --distribution Alpine",
                out.status, detail
            );
        }

        // Give the newly-registered distro time to finish first-boot init.
        sleep(Duration::from_secs(4)).await;
        Ok(())
    }

    // ── Copy agentd + socat into Alpine ──────────────────────────────────────

    async fn ensure_agentd_in_alpine(&self, token: &str) -> Result<()> {
        // Install socat — always run, idempotent and fast. Capture output for diagnostics.
        let apk_out = wsl_cmd()
            .args(["-d", WSL_DISTRO, "--", "apk", "add", "--no-cache", "socat"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        if let Ok(ref out) = apk_out {
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let detail = if stderr.is_empty() { stdout } else { stderr };
                log::warn!("apk add socat failed (exit {}): {}", out.status, detail);
                // Non-fatal: agentd may already work without socat if it was previously installed.
            }
        }

        // Check if agentd is already in Alpine.
        let already = wsl_cmd()
            .args(["-d", WSL_DISTRO, "--", "test", "-x", "/usr/local/bin/agentd"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);

        if !already {
            // Find the binary bundled by the NSIS installer next to our exe.
            let agentd_win = find_bundled_agentd().ok_or_else(|| {
                anyhow::anyhow!(
                    "The agentd engine binary ({}) was not found next to the application.\n\
                     Please reinstall MowisAI to restore bundled components.",
                    AGENTD_BIN
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
            let copy_out = wsl_cmd()
                .args(["-d", WSL_DISTRO, "--", "sh", "-c", &copy_cmd])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
                .context("copying agentd binary into Alpine")?;

            if !copy_out.status.success() {
                let stderr = String::from_utf8_lossy(&copy_out.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&copy_out.stdout).trim().to_string();
                let detail = if stderr.is_empty() { stdout } else { stderr };
                anyhow::bail!(
                    "Failed to copy agentd into Alpine WSL2 (exit {}).\n\
                     Source: {} → WSL: {}\n\
                     Error: {}",
                    copy_out.status, agentd_win.display(), wsl_src, detail
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
        let tok_out = wsl_cmd()
            .args(["-d", WSL_DISTRO, "--", "sh", "-c", &token_cmd])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("writing auth token into Alpine")?;

        if !tok_out.status.success() {
            let stderr = String::from_utf8_lossy(&tok_out.stderr).trim().to_string();
            log::warn!("Writing auth token failed (exit {}): {}", tok_out.status, stderr);
        }

        Ok(())
    }

    // ── Start agentd + socat TCP relay ────────────────────────────────────────

    async fn start_wsl2_bridge(&self, token: &str) -> Result<()> {
        self.ensure_agentd_in_alpine(token).await?;

        // Kill any stale processes from a previous session (best-effort).
        for proc in &["agentd", "socat"] {
            let _ = wsl_cmd()
                .args(["-d", WSL_DISTRO, "--", "pkill", "-f", proc])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
        sleep(Duration::from_millis(600)).await;

        // Start agentd in the background. The nohup shell command itself exits
        // immediately after forking; errors will appear in /var/log/agentd.log.
        let agentd_out = wsl_cmd()
            .args([
                "-d", WSL_DISTRO, "--", "sh", "-c",
                "nohup /usr/local/bin/agentd socket --path /tmp/agentd.sock \
                 </dev/null >>/var/log/agentd.log 2>&1 &",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("starting agentd inside Alpine")?;

        if !agentd_out.status.success() {
            let stderr = String::from_utf8_lossy(&agentd_out.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&agentd_out.stdout).trim().to_string();
            let detail = if stderr.is_empty() { stdout } else { stderr };
            anyhow::bail!(
                "Failed to start agentd inside Alpine (exit {}).\nOutput: {}",
                agentd_out.status, detail
            );
        }

        // Give agentd time to create and bind its socket.
        sleep(Duration::from_secs(2)).await;

        // Bridge the Unix socket out to a TCP port Windows can reach.
        let socat_cmd = format!(
            "nohup socat TCP-LISTEN:{port},reuseaddr,fork \
             UNIX-CONNECT:/tmp/agentd.sock \
             </dev/null >>/var/log/socat.log 2>&1 &",
            port = AGENT_TCP_PORT
        );
        let socat_out = wsl_cmd()
            .args(["-d", WSL_DISTRO, "--", "sh", "-c", &socat_cmd])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("starting socat TCP relay inside Alpine")?;

        if !socat_out.status.success() {
            let stderr = String::from_utf8_lossy(&socat_out.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&socat_out.stdout).trim().to_string();
            let detail = if stderr.is_empty() { stdout } else { stderr };
            anyhow::bail!(
                "Failed to start socat TCP relay inside Alpine (exit {}).\nOutput: {}",
                socat_out.status, detail
            );
        }

        Ok(())
    }

    /// Read the agentd and socat log files from Alpine for diagnostics.
    pub async fn read_alpine_logs(&self) -> String {
        let agentd_log = wsl_cmd()
            .args(["-d", WSL_DISTRO, "--", "sh", "-c",
                "cat /var/log/agentd.log 2>/dev/null || echo '(agentd.log not found)'"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "(could not read agentd.log)".into());

        let socat_log = wsl_cmd()
            .args(["-d", WSL_DISTRO, "--", "sh", "-c",
                "cat /var/log/socat.log 2>/dev/null || echo '(socat.log not found)'"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "(could not read socat.log)".into());

        format!("=== /var/log/agentd.log ===\n{agentd_log}\n\n=== /var/log/socat.log ===\n{socat_log}")
    }

    // ── Wait for bridge ───────────────────────────────────────────────────────

    async fn wait_for_bridge(&self) -> Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if std::time::Instant::now() > deadline {
                let logs = self.read_alpine_logs().await;
                anyhow::bail!(
                    "Timed out waiting for agentd TCP bridge on {}.\n\n{}",
                    AGENT_TCP_ADDR, logs
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
            let _ = wsl_cmd()
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

    async fn read_logs(&self) -> String {
        self.read_alpine_logs().await
    }
}
