// platform/windows.rs — Windows launcher
//
// Strategy (tried in order):
//
//   1. WSL2 primary — fastest, uses the Linux kernel already on the machine.
//      a. Detect WSL2 (wsl.exe --list — reliable even offline).
//      b. Ensure the private "MowisAI" distro is registered:
//           - Check wsl --list --quiet for "MowisAI".
//           - If absent: run  wsl --import MowisAI <data_dir> <bundled_rootfs> --version 2
//             The Alpine mini-rootfs tarball is bundled with the installer at
//             resources/alpine-minirootfs-x86_64.tar.gz (~3.5 MB).
//      c. Copy the agentd-linux-x86_64 binary into the distro at /usr/local/bin/agentd.
//      d. Install socat inside the distro (apk add socat).
//      e. Start agentd:  nohup agentd socket --path /tmp/agentd.sock
//      f. Bridge the Unix socket out via TCP on localhost:9722 with socat.
//         WSL2 automatically forwards Linux localhost ports to Windows.
//      g. Connect from Windows to 127.0.0.1:9722 with auth token.
//
//   2. QEMU/WHPX fallback — if WSL2 is unavailable.
//
// Binary bundling: the Tauri NSIS installer copies both
//   resources/agentd-linux-x86_64                    → next to MowisAI.exe
//   resources/alpine-minirootfs-x86_64.tar.gz        → next to MowisAI.exe
// At runtime we locate them with find_bundled_file().

use crate::platform::auth;
use crate::platform::connection::is_tcp_reachable;
use crate::platform::qemu::{QemuConfig, QemuLauncher};
use crate::platform::{ConnectionInfo, ConnectionKind, VmLauncher};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::sleep;

// Suppress the brief console window that appears when spawning wsl.exe from a GUI process.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Private distro name — unique to MowisAI so it never conflicts with user distros.
const WSL_DISTRO: &str = "MowisAI";
const AGENT_TCP_PORT: u16 = 9722;
const AGENT_TCP_ADDR: &str = "127.0.0.1:9722";
const AGENTD_BIN: &str = "agentd-linux-x86_64";
const ALPINE_ROOTFS: &str = "alpine-minirootfs-x86_64.tar.gz";

/// Create any Windows Command with the console window suppressed.
fn win_cmd(prog: &str) -> Command {
    let mut cmd = Command::new(prog);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Strip the \\?\ extended-length path prefix that some Windows APIs add.
fn strip_unc(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with(r"\\?\") {
        PathBuf::from(s[4..].to_string())
    } else {
        path
    }
}

/// Where the private WSL distro's VHDX is stored.
fn wsl_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("MowisAI")
        .join("wsl")
}

fn qemu_image_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("MowisAI")
        .join("alpine-agentd.qcow2")
}

// ── Path helpers ──────────────────────────────────────────────────────────────

/// Convert a Windows absolute path to the equivalent WSL2 /mnt/<drive>/... path.
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

/// Locate a bundled file by filename.
/// Searches: next to the exe, a "resources/" sub-dir, and the Cargo workspace root.
fn find_bundled_file(name: &str) -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        let exe = strip_unc(exe);
        if let Some(dir) = exe.parent() {
            let p = dir.join(name);
            if p.exists() { return Some(p); }
            let p2 = dir.join("resources").join(name);
            if p2.exists() { return Some(p2); }
        }
    }
    // Workspace root during `cargo tauri dev`
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let p = PathBuf::from(&manifest).join("..").join("..").join(name);
        if p.exists() { return Some(p); }
    }
    None
}

/// Decode the stdout of `wsl --list --quiet`, which is UTF-16LE on Windows.
fn decode_wsl_list(raw: &[u8]) -> String {
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

    /// Returns true if WSL2 is enabled and wsl.exe responds.
    /// Returns false if wsl.exe is not found (program not found error).
    async fn detect_wsl2(&self) -> bool {
        {
            let cached = self.wsl2_available.lock().unwrap();
            if let Some(v) = *cached { return v; }
        }
        
        // Try to run wsl.exe - if it's not found, we'll get a specific error
        let result = win_cmd("wsl.exe")
            .args(["--list"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .await;
        
        let ok = match result {
            Ok(status) => status.success(),
            Err(e) => {
                // Check if this is a "program not found" error
                let err_str = e.to_string().to_lowercase();
                if err_str.contains("not found") || err_str.contains("cannot find") {
                    log::warn!("WSL not found: {}", e);
                }
                false
            }
        };
        
        *self.wsl2_available.lock().unwrap() = Some(ok);
        ok
    }

    // ── Private distro management ─────────────────────────────────────────────

    async fn distro_is_registered(&self) -> bool {
        match win_cmd("wsl.exe")
            .args(["--list", "--quiet"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
        {
            Ok(out) => decode_wsl_list(&out.stdout)
                .lines()
                .any(|l| l.trim().eq_ignore_ascii_case(WSL_DISTRO)),
            Err(_) => false,
        }
    }

    /// Ensure the private MowisAI WSL distro is registered.
    ///
    /// Uses `wsl --import` with the Alpine mini-rootfs bundled with the installer.
    /// The VHDX is written to %LOCALAPPDATA%\MowisAI\wsl\ and persists across runs.
    /// On subsequent launches the distro check is instant (just a wsl --list call).
    async fn ensure_distro(&self) -> Result<()> {
        if self.distro_is_registered().await {
            log::info!("MowisAI WSL distro already registered");
            return Ok(());
        }

        // Find the bundled Alpine mini-rootfs tarball (~3.5 MB, shipped with the installer).
        let rootfs = find_bundled_file(ALPINE_ROOTFS).ok_or_else(|| {
            anyhow::anyhow!(
                "Bundled Alpine rootfs not found ({ALPINE_ROOTFS}).\n\
                 Please reinstall MowisAI."
            )
        })?;

        // Create the directory where WSL stores the VHDX.
        let data_dir = wsl_data_dir();
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("creating WSL data dir: {}", data_dir.display()))?;

        log::info!(
            "Importing Alpine Linux as '{WSL_DISTRO}' from {} into {}…",
            rootfs.display(),
            data_dir.display()
        );

        let import_out = win_cmd("wsl.exe")
            .args([
                "--import", WSL_DISTRO,
                data_dir.to_str().unwrap_or("."),
                rootfs.to_str().unwrap_or(ALPINE_ROOTFS),
                "--version", "2",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("running wsl --import")?;

        if !import_out.status.success() {
            let stderr = String::from_utf8_lossy(&import_out.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&import_out.stdout).trim().to_string();
            let detail = if stderr.is_empty() { stdout } else { stderr };
            anyhow::bail!(
                "wsl --import failed (exit {}).\n\
                 Output: {}\n\n\
                 WSL2 may not be fully enabled. Open PowerShell as Administrator and run:\n\
                   wsl --install --no-distribution\n\
                 then reboot and restart MowisAI.",
                import_out.status, detail
            );
        }

        log::info!("Alpine Linux imported successfully as '{WSL_DISTRO}'");
        // Brief pause for WSL to complete registration.
        sleep(Duration::from_secs(2)).await;
        Ok(())
    }

    // ── Copy agentd + socat into the distro ──────────────────────────────────

    async fn ensure_agentd_in_distro(&self, token: &str) -> Result<()> {
        // Install socat — idempotent and fast.
        let apk_out = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "apk", "add", "--no-cache", "socat"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        if let Ok(ref out) = apk_out {
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                log::warn!("apk add socat failed: {}", if stderr.is_empty() { stdout } else { stderr });
            }
        }

        // Check if agentd is already installed.
        let already = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "test", "-x", "/usr/local/bin/agentd"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);

        if !already {
            let agentd_win = find_bundled_file(AGENTD_BIN).ok_or_else(|| {
                anyhow::anyhow!(
                    "agentd binary ({AGENTD_BIN}) not found next to the application.\n\
                     Please reinstall MowisAI."
                )
            })?;

            log::info!("Copying {} → {WSL_DISTRO}:/usr/local/bin/agentd", agentd_win.display());

            let wsl_src = windows_to_wsl_path(&agentd_win);
            let copy_cmd = format!(
                "cp '{}' /usr/local/bin/agentd && chmod +x /usr/local/bin/agentd",
                wsl_src.replace('\'', "\\'")
            );

            let copy_out = win_cmd("wsl.exe")
                .args(["-d", WSL_DISTRO, "--", "sh", "-c", &copy_cmd])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
                .context("copying agentd into the distro")?;

            if !copy_out.status.success() {
                let stderr = String::from_utf8_lossy(&copy_out.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&copy_out.stdout).trim().to_string();
                let detail = if stderr.is_empty() { stdout } else { stderr };
                anyhow::bail!(
                    "Failed to copy agentd into {WSL_DISTRO} (exit {}).\n\
                     Source: {} → WSL: {}\n\
                     Error: {}",
                    copy_out.status, agentd_win.display(), wsl_src, detail
                );
            }
            log::info!("agentd installed successfully");

            // Verify the binary is executable and check its architecture
            let verify_out = win_cmd("wsl.exe")
                .args(["-d", WSL_DISTRO, "--", "sh", "-c", 
                    "file /usr/local/bin/agentd && ldd /usr/local/bin/agentd 2>&1 | head -20"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await;

            if let Ok(out) = verify_out {
                let info = String::from_utf8_lossy(&out.stdout).trim().to_string();
                log::info!("agentd binary info:\n{}", info);
                
                // Check if it's a static binary (should show "not a dynamic executable")
                if !info.contains("statically linked") && !info.contains("not a dynamic executable") {
                    log::warn!("agentd appears to be dynamically linked - may have missing dependencies");
                }
            }
        }

        // Write auth token.
        let token_cmd = format!(
            "mkdir -p /root/.mowisai && \
             printf '%s' '{}' > /root/.mowisai/token && \
             chmod 600 /root/.mowisai/token",
            token.replace('\'', "\\'")
        );
        let tok_out = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "sh", "-c", &token_cmd])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("writing auth token into distro")?;

        if !tok_out.status.success() {
            let stderr = String::from_utf8_lossy(&tok_out.stderr).trim().to_string();
            log::warn!("Writing auth token failed (exit {}): {}", tok_out.status, stderr);
        }

        Ok(())
    }

    // ── Start agentd + socat TCP relay ────────────────────────────────────────

    async fn start_wsl2_bridge(&self, token: &str) -> Result<()> {
        self.ensure_agentd_in_distro(token).await?;

        // Kill any stale processes from a previous session (best-effort).
        for proc in &["agentd", "socat"] {
            let _ = win_cmd("wsl.exe")
                .args(["-d", WSL_DISTRO, "--", "pkill", "-f", proc])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
        sleep(Duration::from_millis(600)).await;

        // Start agentd in the background.
        let agentd_out = win_cmd("wsl.exe")
            .args([
                "-d", WSL_DISTRO, "--", "sh", "-c",
                "nohup /usr/local/bin/agentd socket --path /tmp/agentd.sock \
                 </dev/null >>/var/log/agentd.log 2>&1 &",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("starting agentd inside distro")?;

        if !agentd_out.status.success() {
            let stderr = String::from_utf8_lossy(&agentd_out.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&agentd_out.stdout).trim().to_string();
            let detail = if stderr.is_empty() { stdout } else { stderr };
            anyhow::bail!(
                "Failed to start agentd (exit {}).\nOutput: {}",
                agentd_out.status, detail
            );
        }

        sleep(Duration::from_secs(2)).await;

        // Verify agentd is actually running
        let check_out = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "pgrep", "-f", "agentd"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        if let Ok(out) = check_out {
            if !out.status.success() {
                // agentd crashed or never started - read the log
                let log_out = win_cmd("wsl.exe")
                    .args(["-d", WSL_DISTRO, "--", "cat", "/var/log/agentd.log"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                    .await;
                
                let log_content = if let Ok(log) = log_out {
                    String::from_utf8_lossy(&log.stdout).trim().to_string()
                } else {
                    "(could not read log)".to_string()
                };

                anyhow::bail!(
                    "agentd failed to start or crashed immediately.\n\
                     Log output:\n{}",
                    if log_content.is_empty() { "(empty log - binary may be missing dependencies)" } else { &log_content }
                );
            }
        }

        // Bridge Unix socket → TCP so Windows can reach it.
        let socat_cmd = format!(
            "nohup socat TCP-LISTEN:{port},reuseaddr,fork \
             UNIX-CONNECT:/tmp/agentd.sock \
             </dev/null >>/var/log/socat.log 2>&1 &",
            port = AGENT_TCP_PORT
        );
        let socat_out = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "sh", "-c", &socat_cmd])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("starting socat TCP relay")?;

        if !socat_out.status.success() {
            let stderr = String::from_utf8_lossy(&socat_out.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&socat_out.stdout).trim().to_string();
            let detail = if stderr.is_empty() { stdout } else { stderr };
            anyhow::bail!(
                "Failed to start socat relay (exit {}).\nOutput: {}",
                socat_out.status, detail
            );
        }

        Ok(())
    }

    // ── Log reader ────────────────────────────────────────────────────────────

    pub async fn read_alpine_logs(&self) -> String {
        let agentd_log = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "sh", "-c",
                "cat /var/log/agentd.log 2>/dev/null || echo '(agentd.log not found)'"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "(could not read agentd.log)".into());

        let socat_log = win_cmd("wsl.exe")
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
        let deadline = std::time::Instant::now() + Duration::from_secs(90);  // Increased from 30 to 90 seconds
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
            log::info!("WSL2 available — using bundled Alpine distro");

            self.ensure_distro()
                .await
                .context("registering MowisAI WSL distro")?;

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
        
        // Check if QEMU is available before trying to spawn
        let qemu_check = Command::new("qemu-system-x86_64")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        
        if let Err(e) = qemu_check {
            let err_str = e.to_string().to_lowercase();
            if err_str.contains("not found") || err_str.contains("cannot find") {
                anyhow::bail!(
                    "Neither WSL nor QEMU are available on this system.\n\n\
                     To use MowisAI with the full engine:\n\
                     1. Install WSL2: Open PowerShell as Administrator and run 'wsl --install', then reboot\n\
                     2. OR install QEMU: Download from https://qemu.org/download/\n\n\
                     You can continue using MowisAI in Zero-Protection mode (no engine required) \
                     by selecting 'zero' mode in Settings."
                );
            }
        }
        
        let child = self.qemu_fallback
            .spawn_process(&token, true)
            .await
            .context("spawning QEMU/WHPX")?;
        drop(child);

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
            let _ = win_cmd("wsl.exe")
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
