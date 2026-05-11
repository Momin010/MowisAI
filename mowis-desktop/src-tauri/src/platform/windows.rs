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
use crate::platform::{ConnectionInfo, ConnectionKind, ProgressSender, VmLauncher};
use crate::backend::SetupProgress;
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

async fn emit(pw: &Option<ProgressSender>, stage: &str, message: &str, pct: u8, kind: &str, detail: Option<String>) {
    if let Some(tx) = pw {
        let _ = tx.send(SetupProgress {
            stage: stage.into(),
            message: message.into(),
            pct,
            detail,
            kind: kind.into(),
            timestamp: SetupProgress::now_millis(),
        }).await;
    }
}

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
        
        // Try to run wsl.exe with a timeout so it can't hang indefinitely.
        let cmd_future = win_cmd("wsl.exe")
            .args(["--list"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status();

        let ok = match tokio::time::timeout(Duration::from_secs(8), cmd_future).await {
            Err(_elapsed) => {
                log::warn!("wsl.exe --list timed out after 8s — treating WSL2 as unavailable");
                *self.wsl2_available.lock().unwrap() = Some(false);
                return false;
            }
            Ok(result) => match result {
                Ok(status) => status.success(),
                Err(e) => {
                    let err_str = e.to_string().to_lowercase();
                    if err_str.contains("not found") || err_str.contains("cannot find") {
                        log::warn!("WSL not found: {}", e);
                    }
                    false
                }
            },
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
        // Configure APK repositories — the minirootfs doesn't include them.
        // Without this, `apk add socat` will always fail.
        let repo_cmd = "\
            if [ ! -f /etc/apk/repositories ] || ! grep -q 'dl-cdn.alpinelinux.org' /etc/apk/repositories 2>/dev/null; then \
                mkdir -p /etc/apk && \
                echo 'https://dl-cdn.alpinelinux.org/alpine/v3.19/main' > /etc/apk/repositories && \
                echo 'https://dl-cdn.alpinelinux.org/alpine/v3.19/community' >> /etc/apk/repositories; \
            fi";
        let repo_out = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "sh", "-c", repo_cmd])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;
        if let Ok(ref out) = repo_out {
            if !out.status.success() {
                let stderr_raw = String::from_utf8_lossy(&out.stderr);
                let stderr = stderr_raw.trim();
                log::warn!("Configuring APK repositories failed: {}", stderr);
            } else {
                log::info!("APK repositories configured");
            }
        }

        // Install socat — with repos configured this should work.
        let apk_out = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "sh", "-c",
                "apk update 2>&1 && apk add --no-cache socat 2>&1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        match apk_out {
            Ok(ref out) if out.status.success() => {
                log::info!("socat installed successfully");
            }
            Ok(ref out) => {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let detail = if stderr.is_empty() { stdout } else { stderr };
                // Non-fatal: socat might already be installed
                log::warn!("apk add socat output: {}", detail);
            }
            Err(e) => {
                log::warn!("Failed to run apk add socat: {}", e);
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

        // Warm-launch fast path: if the TCP bridge is already reachable, skip
        // the kill/restart cycle entirely — this makes subsequent launches nearly instant.
        if is_tcp_reachable(AGENT_TCP_ADDR).await {
            log::info!("WSL2 bridge already reachable at {} — skipping restart", AGENT_TCP_ADDR);
            return Ok(());
        }

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

        // Wait for agentd to create the Unix socket (up to 10 seconds).
        log::info!("Waiting for agentd socket /tmp/agentd.sock...");
        let mut socket_ready = false;
        for _ in 0..20 {
            sleep(Duration::from_millis(500)).await;
            let check = win_cmd("wsl.exe")
                .args(["-d", WSL_DISTRO, "--", "sh", "-c",
                    "test -S /tmp/agentd.sock && echo READY"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .await;
            if let Ok(out) = check {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if stdout.contains("READY") {
                    socket_ready = true;
                    log::info!("agentd socket is ready");
                    break;
                }
            }
        }

        if !socket_ready {
            // Read the agentd log to get a useful error message
            let log_out = win_cmd("wsl.exe")
                .args(["-d", WSL_DISTRO, "--", "cat", "/var/log/agentd.log"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .await;
            let log_content = log_out
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();

            // Also check if agentd process is running
            let pgrep = win_cmd("wsl.exe")
                .args(["-d", WSL_DISTRO, "--", "pgrep", "-x", "agentd"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
            let agentd_running = pgrep.map(|s| s.success()).unwrap_or(false);

            anyhow::bail!(
                "agentd socket (/tmp/agentd.sock) was not created within 10 seconds.\n\
                 agentd process running: {}\n\
                 Log: {}",
                agentd_running,
                if log_content.is_empty() { "(empty — binary may be missing or crashed silently)" } else { &log_content }
            );
        }

        // Verify socat is available before trying to use it
        let socat_check = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "sh", "-c", "command -v socat"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        let socat_available = socat_check.map(|s| s.success()).unwrap_or(false);
        if !socat_available {
            anyhow::bail!(
                "socat is not installed in the MowisAI WSL distro.\n\
                 The distro may not have internet access to download packages.\n\
                 Try running manually:  wsl -d MowisAI -- apk add socat"
            );
        }

        // Bridge Unix socket → TCP so Windows can reach it.
        let socat_cmd = format!(
            "nohup socat TCP4-LISTEN:{port},reuseaddr,fork \
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
        self.wait_for_bridge_inner(&None).await
    }

    async fn wait_for_bridge_with_progress(&self, pw: &Option<ProgressSender>) -> Result<()> {
        self.wait_for_bridge_inner(pw).await
    }

    async fn wait_for_bridge_inner(&self, pw: &Option<ProgressSender>) -> Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(25);
        let mut checks = 0u32;
        loop {
            if std::time::Instant::now() > deadline {
                let logs = self.read_alpine_logs().await;
                anyhow::bail!(
                    "Timed out waiting for agentd TCP bridge on {}.\n\n{}",
                    AGENT_TCP_ADDR, logs
                );
            }
            if is_tcp_reachable(AGENT_TCP_ADDR).await {
                emit(pw, "booting", "TCP bridge reachable", 92, "success", Some(format!("Port {} open", AGENT_TCP_PORT))).await;
                return Ok(());
            }
            checks += 1;
            if checks % 4 == 0 {
                emit(pw, "booting", &format!("Waiting for TCP bridge… ({}s)", checks / 2), 88, "output", None).await;
            }
            sleep(Duration::from_millis(500)).await;
        }
    }

    // ── Progress-aware WSL2 methods ───────────────────────────────────────────

    async fn detect_wsl2_with_progress(&self, pw: &Option<ProgressSender>) -> bool {
        let cached_value = {
            let cached = self.wsl2_available.lock().unwrap();
            *cached
        };

        if let Some(v) = cached_value {
            emit(pw, "detecting", if v { "WSL2 cached as available" } else { "WSL2 cached as unavailable" }, 12, "info", None).await;
            return v;
        }

        emit(pw, "detecting", "Running: wsl.exe --list", 10, "command", Some("wsl.exe --list".into())).await;

        let cmd_future = win_cmd("wsl.exe")
            .args(["--list"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status();

        let ok = match tokio::time::timeout(Duration::from_secs(8), cmd_future).await {
            Err(_elapsed) => {
                emit(pw, "detecting", "wsl.exe --list timed out (8s)", 12, "warning", None).await;
                log::warn!("wsl.exe --list timed out after 8s — treating WSL2 as unavailable");
                *self.wsl2_available.lock().unwrap() = Some(false);
                return false;
            }
            Ok(result) => match result {
                Ok(status) => {
                    let ok = status.success();
                    emit(pw, "detecting", &format!("wsl.exe exited: {}", if ok { "success" } else { "failed" }), 12,
                        if ok { "success" } else { "warning" },
                        Some(format!("exit code: {:?}", status.code()))).await;
                    ok
                }
                Err(e) => {
                    let err_str = e.to_string().to_lowercase();
                    if err_str.contains("not found") || err_str.contains("cannot find") {
                        emit(pw, "detecting", "WSL not found on this system", 12, "warning", Some(format!("{}", e))).await;
                        log::warn!("WSL not found: {}", e);
                    } else {
                        emit(pw, "detecting", &format!("WSL detection error: {}", e), 12, "error", None).await;
                    }
                    false
                }
            },
        };

        *self.wsl2_available.lock().unwrap() = Some(ok);
        ok
    }

    async fn ensure_distro_with_progress(&self, pw: &Option<ProgressSender>) -> Result<()> {
        emit(pw, "installing", "Checking if MowisAI distro is registered…", 20, "command", Some("wsl --list --quiet".into())).await;

        if self.distro_is_registered().await {
            emit(pw, "installing", "MowisAI WSL distro already registered", 25, "success", None).await;
            log::info!("MowisAI WSL distro already registered");
            return Ok(());
        }

        emit(pw, "installing", "MowisAI distro not found — importing…", 22, "info", None).await;

        let rootfs = find_bundled_file(ALPINE_ROOTFS).ok_or_else(|| {
            anyhow::anyhow!(
                "Bundled Alpine rootfs not found ({ALPINE_ROOTFS}).\n\
                 Please reinstall MowisAI."
            )
        })?;

        let data_dir = wsl_data_dir();
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("creating WSL data dir: {}", data_dir.display()))?;

        emit(pw, "installing", &format!("Importing Alpine Linux from {}…", rootfs.display()), 23, "command",
            Some(format!("wsl --import {} {} {} --version 2", WSL_DISTRO, data_dir.display(), rootfs.display()))).await;

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
            emit(pw, "error", "wsl --import failed", 0, "error", Some(detail.clone())).await;
            anyhow::bail!(
                "wsl --import failed (exit {}).\n\
                 Output: {}\n\n\
                 WSL2 may not be fully enabled. Open PowerShell as Administrator and run:\n\
                   wsl --install --no-distribution\n\
                 then reboot and restart MowisAI.",
                import_out.status, detail
            );
        }

        emit(pw, "installing", "Alpine Linux imported successfully", 28, "success", Some(format!("Distro: {}", WSL_DISTRO))).await;
        log::info!("Alpine Linux imported successfully as '{WSL_DISTRO}'");
        sleep(Duration::from_secs(2)).await;
        Ok(())
    }

    async fn start_wsl2_bridge_with_progress(&self, token: &str, pw: &Option<ProgressSender>) -> Result<()> {
        self.ensure_agentd_in_distro_with_progress(token, pw).await?;

        // Warm-launch fast path
        if is_tcp_reachable(AGENT_TCP_ADDR).await {
            emit(pw, "booting", &format!("WSL2 bridge already reachable at {} — skipping restart", AGENT_TCP_ADDR), 85, "success", None).await;
            return Ok(());
        }

        // Kill stale processes
        emit(pw, "booting", "Cleaning up stale processes…", 60, "command", Some("pkill -f agentd; pkill -f socat".into())).await;
        for proc in &["agentd", "socat"] {
            let _ = win_cmd("wsl.exe")
                .args(["-d", WSL_DISTRO, "--", "pkill", "-f", proc])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
        sleep(Duration::from_millis(600)).await;

        // Start agentd
        emit(pw, "booting", "Starting agentd daemon…", 65, "command",
            Some("nohup /usr/local/bin/agentd socket --path /tmp/agentd.sock".into())).await;

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
            emit(pw, "error", "Failed to start agentd", 0, "error", Some(detail.clone())).await;
            anyhow::bail!(
                "Failed to start agentd (exit {}).\nOutput: {}",
                agentd_out.status, detail
            );
        }
        emit(pw, "booting", "agentd process started", 70, "success", None).await;

        // Wait for socket
        emit(pw, "booting", "Waiting for agentd socket /tmp/agentd.sock…", 72, "info", None).await;
        let mut socket_ready = false;
        for i in 0..20 {
            sleep(Duration::from_millis(500)).await;
            let check = win_cmd("wsl.exe")
                .args(["-d", WSL_DISTRO, "--", "sh", "-c",
                    "test -S /tmp/agentd.sock && echo READY"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .await;
            if let Ok(out) = check {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if stdout.contains("READY") {
                    socket_ready = true;
                    emit(pw, "booting", "agentd socket ready", 80, "success", Some("/tmp/agentd.sock".into())).await;
                    break;
                }
            }
            if i % 4 == 3 {
                emit(pw, "booting", &format!("Waiting for socket… ({}s)", (i + 1) / 2), 74, "output", None).await;
            }
        }

        if !socket_ready {
            let log_out = win_cmd("wsl.exe")
                .args(["-d", WSL_DISTRO, "--", "cat", "/var/log/agentd.log"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .await;
            let log_content = log_out
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();

            let pgrep = win_cmd("wsl.exe")
                .args(["-d", WSL_DISTRO, "--", "pgrep", "-x", "agentd"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
            let agentd_running = pgrep.map(|s| s.success()).unwrap_or(false);

            let err_detail = format!("agentd running: {}, log: {}", agentd_running,
                if log_content.is_empty() { "(empty)" } else { &log_content });
            emit(pw, "error", "agentd socket not created within 10s", 0, "error", Some(err_detail)).await;

            anyhow::bail!(
                "agentd socket (/tmp/agentd.sock) was not created within 10 seconds.\n\
                 agentd process running: {}\n\
                 Log: {}",
                agentd_running,
                if log_content.is_empty() { "(empty — binary may be missing or crashed silently)" } else { &log_content }
            );
        }

        // Verify socat
        emit(pw, "booting", "Checking socat availability…", 82, "command", Some("command -v socat".into())).await;
        let socat_check = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "sh", "-c", "command -v socat"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        let socat_available = socat_check.map(|s| s.success()).unwrap_or(false);
        if !socat_available {
            emit(pw, "error", "socat not installed", 0, "error", Some("Run: wsl -d MowisAI -- apk add socat".into())).await;
            anyhow::bail!(
                "socat is not installed in the MowisAI WSL distro.\n\
                 The distro may not have internet access to download packages.\n\
                 Try running manually:  wsl -d MowisAI -- apk add socat"
            );
        }
        emit(pw, "booting", "socat available", 83, "success", None).await;

        // Bridge with socat
        let socat_cmd = format!(
            "nohup socat TCP4-LISTEN:{port},reuseaddr,fork \
             UNIX-CONNECT:/tmp/agentd.sock \
             </dev/null >>/var/log/socat.log 2>&1 &",
            port = AGENT_TCP_PORT
        );
        emit(pw, "booting", "Starting socat TCP relay…", 84, "command",
            Some(format!("socat TCP4-LISTEN:{},reuseaddr,fork UNIX-CONNECT:/tmp/agentd.sock", AGENT_TCP_PORT))).await;

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
            emit(pw, "error", "Failed to start socat relay", 0, "error", Some(detail.clone())).await;
            anyhow::bail!(
                "Failed to start socat relay (exit {}).\nOutput: {}",
                socat_out.status, detail
            );
        }

        emit(pw, "booting", "socat TCP relay started", 85, "success", Some(format!("Port {}", AGENT_TCP_PORT))).await;
        Ok(())
    }

    async fn ensure_agentd_in_distro_with_progress(&self, token: &str, pw: &Option<ProgressSender>) -> Result<()> {
        // Configure APK repositories
        emit(pw, "installing", "Configuring APK repositories…", 32, "command",
            Some("echo 'https://dl-cdn.alpinelinux.org/alpine/v3.19/main' > /etc/apk/repositories".into())).await;

        let repo_cmd = "\
            if [ ! -f /etc/apk/repositories ] || ! grep -q 'dl-cdn.alpinelinux.org' /etc/apk/repositories 2>/dev/null; then \
                mkdir -p /etc/apk && \
                echo 'https://dl-cdn.alpinelinux.org/alpine/v3.19/main' > /etc/apk/repositories && \
                echo 'https://dl-cdn.alpinelinux.org/alpine/v3.19/community' >> /etc/apk/repositories; \
            fi";
        let repo_out = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "sh", "-c", repo_cmd])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;
        if let Ok(ref out) = repo_out {
            if !out.status.success() {
                let stderr_raw = String::from_utf8_lossy(&out.stderr);
                let stderr = stderr_raw.trim();
                emit(pw, "installing", "APK repository config failed", 33, "warning", Some(stderr.to_string())).await;
                log::warn!("Configuring APK repositories failed: {}", stderr);
            } else {
                emit(pw, "installing", "APK repositories configured", 34, "success", None).await;
            }
        }

        // Install socat
        emit(pw, "installing", "Installing socat…", 36, "command", Some("apk update && apk add --no-cache socat".into())).await;
        let apk_out = win_cmd("wsl.exe")
            .args(["-d", WSL_DISTRO, "--", "sh", "-c",
                "apk update 2>&1 && apk add --no-cache socat 2>&1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        match apk_out {
            Ok(ref out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                emit(pw, "installing", "socat installed", 40, "success", Some(stdout)).await;
            }
            Ok(ref out) => {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let detail = if stderr.is_empty() { stdout } else { stderr };
                emit(pw, "installing", "socat install output (may already be installed)", 40, "warning", Some(detail)).await;
                log::warn!("apk add socat output: {}", String::from_utf8_lossy(&out.stderr));
            }
            Err(e) => {
                emit(pw, "installing", &format!("Failed to run apk add socat: {}", e), 40, "error", None).await;
                log::warn!("Failed to run apk add socat: {}", e);
            }
        }

        // Check if agentd is already installed
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

            let wsl_src = windows_to_wsl_path(&agentd_win);
            emit(pw, "installing", "Copying agentd binary into distro…", 45, "command",
                Some(format!("cp '{}' /usr/local/bin/agentd", wsl_src))).await;

            log::info!("Copying {} → {WSL_DISTRO}:/usr/local/bin/agentd", agentd_win.display());

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
                emit(pw, "error", "Failed to copy agentd", 0, "error", Some(detail.clone())).await;
                anyhow::bail!(
                    "Failed to copy agentd into {WSL_DISTRO} (exit {}).\n\
                     Source: {} → WSL: {}\n\
                     Error: {}",
                    copy_out.status, agentd_win.display(), wsl_src, detail
                );
            }
            emit(pw, "installing", "agentd installed successfully", 50, "success", Some("/usr/local/bin/agentd".into())).await;
            log::info!("agentd installed successfully");

            // Verify binary
            let verify_out = win_cmd("wsl.exe")
                .args(["-d", WSL_DISTRO, "--", "sh", "-c",
                    "file /usr/local/bin/agentd && ldd /usr/local/bin/agentd 2>&1 | head -20"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await;

            if let Ok(out) = verify_out {
                let info = String::from_utf8_lossy(&out.stdout).trim().to_string();
                emit(pw, "installing", "Binary verification", 52, "info", Some(info.clone())).await;
                log::info!("agentd binary info:\n{}", info);
            }
        } else {
            emit(pw, "installing", "agentd already installed", 50, "success", None).await;
        }

        // Write auth token
        emit(pw, "installing", "Writing auth token…", 55, "command", Some("mkdir -p /root/.mowisai && write token".into())).await;
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
            emit(pw, "installing", "Auth token write failed", 56, "warning", Some(stderr.clone())).await;
            log::warn!("Writing auth token failed (exit {}): {}", tok_out.status, stderr);
        } else {
            emit(pw, "installing", "Auth token written", 58, "success", None).await;
        }

        Ok(())
    }
}

// ── VmLauncher impl ───────────────────────────────────────────────────────────

#[async_trait]
impl VmLauncher for WindowsLauncher {
    fn name(&self) -> &str { "Windows/WSL2+Alpine" }

    async fn start(&self, progress: Option<ProgressSender>) -> Result<ConnectionInfo> {
        let token = auth::load_or_create().context("load/create auth token")?;
        let pw = &progress;

        emit(pw, "detecting", "Detecting WSL2 availability…", 10, "info", None).await;

        if self.detect_wsl2_with_progress(pw).await {
            emit(pw, "detecting", "WSL2 available — using bundled Alpine distro", 15, "success", None).await;

            emit(pw, "installing", "Ensuring MowisAI WSL distro is registered…", 20, "command", Some("wsl --list --quiet".into())).await;
            self.ensure_distro_with_progress(pw)
                .await
                .context("registering MowisAI WSL distro")?;
            emit(pw, "installing", "WSL distro registered", 30, "success", None).await;

            emit(pw, "booting", "Starting WSL2 bridge…", 35, "command", Some("start_wsl2_bridge".into())).await;
            self.start_wsl2_bridge_with_progress(&token, pw)
                .await
                .context("starting WSL2 bridge")?;

            emit(pw, "booting", "Waiting for agentd TCP bridge…", 90, "info", None).await;
            self.wait_for_bridge_with_progress(pw)
                .await
                .context("waiting for agentd TCP bridge")?;

            emit(pw, "ready", "WSL2 bridge ready", 95, "success", Some(format!("TCP: {}", AGENT_TCP_ADDR))).await;

            return Ok(ConnectionInfo {
                kind: ConnectionKind::TcpWithToken,
                socket_path: None,
                tcp_addr: Some(AGENT_TCP_ADDR.into()),
                pipe_name: None,
                auth_token: Some(token),
            });
        }

        // ── QEMU/WHPX fallback ─────────────────────────────────────────────
        emit(pw, "detecting", "WSL2 not available — falling back to QEMU/WHPX", 15, "warning", None).await;
        log::warn!("WSL2 not available — falling back to QEMU/WHPX");
        let dev_cfg = crate::platform::developer_mode::DeveloperConfig::load_or_default();
        let mut qemu_cfg = QemuConfig::windows_whpx(qemu_image_path());
        if dev_cfg.qemu_path.exists() {
            qemu_cfg.qemu_bin = dev_cfg.qemu_path.clone();
        }
        if dev_cfg.disk_path.exists() {
            qemu_cfg.image_path = dev_cfg.disk_path.clone();
        }
        if dev_cfg.ram_mb >= 256 {
            qemu_cfg.ram_mb = dev_cfg.ram_mb;
        }
        let qemu_launcher = QemuLauncher::new(qemu_cfg.clone());
        
        emit(pw, "booting", "Checking QEMU availability…", 20, "command", Some(format!("{} --version", qemu_cfg.qemu_bin.display()))).await;

        // Check if QEMU is available before trying to spawn
        let qemu_check = Command::new(&qemu_cfg.qemu_bin)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        
        if let Err(e) = qemu_check {
            let err_str = e.to_string().to_lowercase();
            if err_str.contains("not found") || err_str.contains("cannot find") {
                let msg = "Neither WSL nor QEMU are available on this system.";
                emit(pw, "error", msg, 0, "error", Some(format!("{}", e))).await;
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
        
        emit(pw, "booting", "Spawning QEMU/WHPX…", 30, "command", Some(format!("{} {}", qemu_cfg.qemu_bin.display(), qemu_cfg.ram_mb))).await;
        let child = qemu_launcher
            .spawn_process(&token, true)
            .await
            .context("spawning QEMU/WHPX")?;
        let pid = child.id();
        emit(pw, "booting", &format!("QEMU process spawned (PID: {:?})", pid), 40, "success", None).await;
        drop(child);

        emit(pw, "booting", "Waiting for QEMU agentd bridge…", 50, "info", None).await;
        qemu_launcher
            .wait_for_agent()
            .await
            .context("waiting for QEMU agentd bridge")?;
        emit(pw, "ready", "QEMU bridge ready", 95, "success", None).await;

        Ok(ConnectionInfo {
            kind: ConnectionKind::TcpWithToken,
            socket_path: None,
            tcp_addr: Some(qemu_launcher.agent_tcp().to_owned()),
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
