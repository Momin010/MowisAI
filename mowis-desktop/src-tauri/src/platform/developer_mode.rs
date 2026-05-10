// platform/developer_mode.rs — Developer Mode QEMU Bootstrap Launcher
//
// Fully automated bootstrap for non-admin Windows users:
//   1. Spawns QEMU with Alpine ISO + persistent qcow2 disk
//   2. Connects to the VM's serial console (TCP) for reliable command execution
//   3. Runs initialization: network, mount, install socat, start agentd, bridge to TCP
//   4. Returns a TCP ConnectionInfo so the desktop app connects automatically
//
// Uses the serial console as a bidirectional TTY — far more reliable than
// monitor `sendkey` which is fragile and loses characters.

use crate::platform::auth;
use crate::platform::connection::is_tcp_reachable;
use crate::platform::{ConnectionInfo, ConnectionKind, ProgressSender, VmLauncher};
use crate::backend::SetupProgress;
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};

const PORT_READY_TIMEOUT_SECS: u64 = 120;
const ALPINE_ISO: &str = "alpine-virt-3.19.1-x86_64.iso";

// ── Path helpers ──────────────────────────────────────────────────────────────

/// Strip the \\?\ extended-length path prefix that some Windows APIs add.
fn strip_unc(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with(r"\\?\") {
        PathBuf::from(s[4..].to_string())
    } else {
        path
    }
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

// ── Config ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeveloperConfig {
    pub qemu_path: PathBuf,
    pub iso_path: PathBuf,
    pub disk_path: PathBuf,
    pub ram_mb: u32,
    pub agent_port: u16,
    pub monitor_port: u16,
    pub serial_port: u16,
    pub mount_point: String,
    pub disk_device: String,
    pub agentd_path: String,
}

impl Default for DeveloperConfig {
    fn default() -> Self {
        let iso_path = find_bundled_file(ALPINE_ISO)
            .unwrap_or_else(|| PathBuf::from(ALPINE_ISO));
        Self {
            qemu_path: PathBuf::from("qemu-system-x86_64"),
            iso_path,
            disk_path: PathBuf::from("momin_disk.qcow2"),
            ram_mb: 512,
            agent_port: 8080,
            monitor_port: 4445,
            serial_port: 4444,
            mount_point: "/mnt/mowisai".into(),
            disk_device: "/dev/sda".into(),
            agentd_path: "/mnt/mowisai/agentd-linux-x86_64".into(),
        }
    }
}

impl DeveloperConfig {
    pub fn load_or_default() -> Self {
        let path = Self::config_file_path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn config_file_path() -> PathBuf {
        dirs::config_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("MowisAI")
            .join("developer_config.json")
    }

    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if !self.qemu_path.exists() {
            warnings.push(format!("QEMU binary not found: {}", self.qemu_path.display()));
        }
        if !self.iso_path.exists() {
            // Try to find the ISO in bundled locations
            if let Some(found) = find_bundled_file(ALPINE_ISO) {
                warnings.push(format!(
                    "ISO not found at configured path: {}\n  \
                     But found at: {} — update your config to use this path.",
                    self.iso_path.display(), found.display()
                ));
            } else {
                warnings.push(format!(
                    "ISO not found: {}\n  \
                     Download from https://dl-cdn.alpinelinux.org/alpine/v3.19/releases/x86_64/{}",
                    self.iso_path.display(), ALPINE_ISO
                ));
            }
        }
        if !self.disk_path.exists() {
            warnings.push(format!("Disk image not found: {}", self.disk_path.display()));
        }
        if self.agent_port < 1024 || self.agent_port > 65535 {
            warnings.push(format!("Invalid agent port: {}", self.agent_port));
        }
        warnings
    }
}

// ── Progress helper ──────────────────────────────────────────────────────────

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

// ── Serial Console Helper ────────────────────────────────────────────────────
//
// This is the core reliability improvement: instead of typing commands through
// the QEMU monitor with `sendkey` (which drops characters), we write directly
// to the serial console TCP port. This is like having a real terminal session.

struct SerialConsole {
    reader: BufReader<tokio::io::ReadHalf<TcpStream>>,
    writer: tokio::io::WriteHalf<TcpStream>,
    boot_log: String,
}

impl SerialConsole {
    /// Connect to the serial console TCP port. Retries for up to `timeout_secs`.
    async fn connect(port: u16, timeout_secs: u64) -> Result<Self> {
        let addr = format!("127.0.0.1:{}", port);
        let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);

        let stream = loop {
            if std::time::Instant::now() > deadline {
                anyhow::bail!(
                    "Could not connect to QEMU serial console on {}. \
                     QEMU may have failed to start or the port is blocked.",
                    addr
                );
            }
            match timeout(Duration::from_secs(2), TcpStream::connect(&addr)).await {
                Ok(Ok(s)) => break s,
                _ => sleep(Duration::from_millis(500)).await,
            }
        };

        let (read_half, write_half) = tokio::io::split(stream);
        Ok(Self {
            reader: BufReader::new(read_half),
            writer: write_half,
            boot_log: String::new(),
        })
    }

    /// Drain all buffered output from the serial console (boot messages etc).
    /// Stores them in boot_log for diagnostics.
    async fn drain_boot_output(&mut self, wait_secs: u64) {
        let _ = timeout(Duration::from_secs(wait_secs), async {
            let mut buf = vec![0u8; 4096];
            loop {
                match tokio::io::AsyncReadExt::read(&mut self.reader, &mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]);
                        self.boot_log.push_str(&text);
                        // Check if we see a login prompt
                        if self.boot_log.contains("login:") {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }).await;
    }

    /// Send a command and wait for a response containing expected text.
    /// Returns the full output captured after sending the command.
    async fn exec_command(&mut self, cmd: &str, expect: Option<&str>, timeout_secs: u64) -> Result<String> {
        // Write the command + newline
        self.writer.write_all(format!("{}\n", cmd).as_bytes()).await
            .context("write to serial console")?;
        self.writer.flush().await.context("flush serial console")?;

        // Read response until we see the expected marker or timeout
        let mut output = String::new();
        let result = timeout(Duration::from_secs(timeout_secs), async {
            let mut buf = vec![0u8; 4096];
            loop {
                match tokio::io::AsyncReadExt::read(&mut self.reader, &mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]);
                        output.push_str(&text);

                        // If we have an expected string, check for it
                        if let Some(marker) = expect {
                            if output.contains(marker) {
                                return true;
                            }
                        }

                        // If no expected string, look for a shell prompt (# or $)
                        if expect.is_none() {
                            let trimmed = output.trim_end();
                            if trimmed.ends_with('#') || trimmed.ends_with('$')
                                || trimmed.ends_with("~ #") || trimmed.ends_with(":~#")
                            {
                                return true;
                            }
                        }

                        // Safety: don't buffer forever
                        if output.len() > 100_000 {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            false
        }).await;

        match result {
            Ok(true) => Ok(output),
            Ok(false) => Ok(output), // Got output but didn't find marker
            Err(_) => {
                // Timeout — return what we have
                if output.is_empty() {
                    anyhow::bail!("Command '{}' timed out with no output after {}s", cmd, timeout_secs);
                }
                Ok(output)
            }
        }
    }

    /// Send a command without waiting for a response (fire-and-forget for background commands).
    async fn exec_background(&mut self, cmd: &str) -> Result<()> {
        self.writer.write_all(format!("{}\n", cmd).as_bytes()).await
            .context("write to serial console")?;
        self.writer.flush().await.context("flush serial console")?;
        // Small delay to let the command start
        sleep(Duration::from_millis(500)).await;
        Ok(())
    }

    /// Get the captured boot log for diagnostics.
    fn boot_log(&self) -> &str {
        &self.boot_log
    }
}

// ── Launcher ─────────────────────────────────────────────────────────────────

pub struct DeveloperLauncher {
    config: DeveloperConfig,
    child: Mutex<Option<Child>>,
    running: AtomicBool,
}

impl DeveloperLauncher {
    pub fn new(config: DeveloperConfig) -> Self {
        Self {
            config,
            child: Mutex::new(None),
            running: AtomicBool::new(false),
        }
    }

    pub fn is_configured() -> bool {
        let path = DeveloperConfig::config_file_path();
        path.exists()
            && std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str::<DeveloperConfig>(&raw).ok())
                .map(|cfg| cfg.qemu_path.exists())
                .unwrap_or(false)
    }

    fn build_qemu_args(&self) -> Vec<String> {
        let cfg = &self.config;
        let serial_tcp = format!("tcp:127.0.0.1:{},server=on,wait=off", cfg.serial_port);
        let monitor_tcp = format!("tcp:127.0.0.1:{},server=on,wait=off", cfg.monitor_port);

        // Detect if we have vmlinuz-virt and initramfs-virt alongside the ISO
        // (direct kernel boot is faster and guarantees serial console works)
        let iso_dir = cfg.iso_path.parent().unwrap_or(std::path::Path::new("."));
        let vmlinuz = iso_dir.join("vmlinuz-virt");
        let initramfs = iso_dir.join("initramfs-virt");
        let use_direct_kernel = vmlinuz.exists() && initramfs.exists();

        let mut args = vec![
            "-m".into(), cfg.ram_mb.to_string(),
            "-drive".into(), format!("file={},format=qcow2", cfg.disk_path.display()),
            "-cdrom".into(), cfg.iso_path.to_string_lossy().to_string(),
            "-boot".into(), "d".into(),  // Boot from CD-ROM (the Alpine ISO)
            "-netdev".into(), format!(
                "user,id=net0,hostfwd=tcp::{}-:8080",
                cfg.agent_port
            ),
            "-device".into(), "virtio-net-pci,netdev=net0".into(),
            // Serial console on TCP — this is our main control channel into the VM
            "-serial".into(), serial_tcp,
            // QEMU monitor for VM management (savevm, quit, etc)
            "-monitor".into(), monitor_tcp,
            // No GUI window, no VGA output — headless operation
            "-display".into(), "none".into(),
            "-vga".into(), "none".into(),
        ];

        // Direct kernel boot: bypasses BIOS, boots in ~5s, serial console guaranteed
        if use_direct_kernel {
            log::info!("Using direct kernel boot: kernel={}, initrd={}",
                vmlinuz.display(), initramfs.display());
            args.push("-kernel".into());
            args.push(vmlinuz.to_string_lossy().to_string());
            args.push("-initrd".into());
            args.push(initramfs.to_string_lossy().to_string());
            args.push("-append".into());
            // IMPORTANT: This must be a SINGLE argument string — spaces are part of the value
            args.push("console=ttyS0 root=/dev/sr0 modules=loop,squashfs,sd-mod,usb-storage quiet".into());
        } else {
            log::info!("Using ISO boot (no vmlinuz-virt found, BIOS boot — slower)");
            // -nographic redirects serial to stdio AND sets console=ttyS0 in the kernel
            args.push("-nographic".into());
        }

        args
    }

    async fn wait_for_port(&self, label: &str, max_secs: u64, pw: &Option<ProgressSender>) -> Result<()> {
        let addr = format!("127.0.0.1:{}", self.config.agent_port);
        let deadline = std::time::Instant::now() + Duration::from_secs(max_secs);
        let mut elapsed = 0u64;
        while std::time::Instant::now() < deadline {
            if is_tcp_reachable(&addr).await {
                log::info!("{} reachable at {}", label, addr);
                return Ok(());
            }
            sleep(Duration::from_secs(1)).await;
            elapsed += 1;
            if elapsed % 5 == 0 {
                emit(pw, "booting", &format!("Waiting for {} on {} ({}s)…", label, addr, elapsed), 88, "info", None).await;
            }
        }
        anyhow::bail!(
            "Timed out waiting for {} on {} after {}s.\n\
             The agentd process may have failed to start or socat bridge isn't working.\n\
             Check that the agentd binary exists at: {}",
            label, addr, max_secs, self.config.agentd_path
        )
    }

    async fn bootstrap(&self, mut child: Child, token: &str, args_log: &[String], pw: &Option<ProgressSender>) -> Result<ConnectionInfo> {
        // ── Step 0: Verify QEMU didn't crash immediately ─────────────────────
        // Give QEMU 2 seconds to start, then check if it's still alive
        sleep(Duration::from_secs(2)).await;

        // Check if the process already exited (common: bad args, missing accel, path issues)
        match child.try_wait() {
            Ok(Some(exit_status)) => {
                // QEMU already died! Capture stderr for diagnostics
                let stderr_output = if let Some(mut stderr) = child.stderr.take() {
                    let mut buf = String::new();
                    let _ = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut buf).await;
                    buf
                } else {
                    "(stderr not captured)".into()
                };

                // Also capture stdout
                let stdout_output = if let Some(mut stdout) = child.stdout.take() {
                    let mut buf = String::new();
                    let _ = tokio::io::AsyncReadExt::read_to_string(&mut stdout, &mut buf).await;
                    buf
                } else {
                    String::new()
                };

                // Write crash log to a file so the user can ALWAYS find it
                let log_path = dirs::data_local_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("MowisAI")
                    .join("qemu-crash.log");
                let _ = std::fs::create_dir_all(log_path.parent().unwrap());
                let crash_report = format!(
                    "=== QEMU CRASH REPORT ===\n\
                     Timestamp: {:?}\n\
                     Exit status: {}\n\
                     Command: {} {}\n\n\
                     === STDERR ===\n{}\n\n\
                     === STDOUT ===\n{}\n",
                    std::time::SystemTime::now(),
                    exit_status,
                    self.config.qemu_path.display(),
                    args_log.join(" "),
                    if stderr_output.is_empty() { "(empty)" } else { &stderr_output },
                    if stdout_output.is_empty() { "(empty)" } else { &stdout_output },
                );
                let _ = std::fs::write(&log_path, &crash_report);
                log::error!("Crash report written to: {}", log_path.display());

                let msg = format!(
                    "QEMU exited immediately with status: {}\n\n\
                     FULL STDERR:\n{}\n\n\
                     Log saved to: {}\n\n\
                     Common causes:\n\
                     - Paths with spaces (check QEMU args above)\n\
                     - WHPX/acceleration not available\n\
                     - Kernel or initrd file not found\n\
                     - Disk image corrupted or locked",
                    exit_status,
                    if stderr_output.is_empty() { "(empty)" } else { &stderr_output },
                    log_path.display()
                );
                emit(pw, "error", "QEMU crashed on startup!", 0, "error", Some(msg.clone())).await;
                log::error!("QEMU died immediately: {}", msg);
                anyhow::bail!("{}", msg);
            }
            Ok(None) => {
                // Still running — good!
                emit(pw, "booting", "QEMU process is alive", 12, "success", None).await;
                log::info!("QEMU process confirmed alive after 2s");
            }
            Err(e) => {
                log::warn!("Could not check QEMU process status: {}", e);
            }
        }

        {
            let mut guard = self.child.lock().unwrap();
            *guard = Some(child);
        }

        // ── Step 1: Wait for VM to boot ──────────────────────────────────────
        // Instead of waiting blindly, try to connect to serial port in a loop
        emit(pw, "booting", "Waiting for Alpine to boot (checking serial port)…", 15, "info", None).await;
        log::info!("Waiting for serial console to become available...");

        // Try connecting to serial port — this tells us the VM is alive and QEMU is listening
        let serial_deadline = std::time::Instant::now() + Duration::from_secs(60);
        let mut attempt_count = 0u32;

        let mut serial = loop {
            attempt_count += 1;
            let addr = format!("127.0.0.1:{}", self.config.serial_port);

            match timeout(Duration::from_secs(2), TcpStream::connect(&addr)).await {
                Ok(Ok(stream)) => {
                    emit(pw, "booting", &format!("Serial console connected (took ~{}s)", attempt_count * 2), 25, "success",
                        Some(format!("Port {}", self.config.serial_port))).await;
                    log::info!("Serial console connected after ~{}s", attempt_count * 2);

                    let (read_half, write_half) = tokio::io::split(stream);
                    break SerialConsole {
                        reader: BufReader::new(read_half),
                        writer: write_half,
                        boot_log: String::new(),
                    };
                }
                _ => {}
            }

            if std::time::Instant::now() > serial_deadline {
                emit(pw, "error", "Could not connect to QEMU serial console after 60s", 0, "error",
                    Some("QEMU may be running but serial port is not responding. Check if port 4444 is blocked by firewall.".into())).await;
                anyhow::bail!(
                    "Could not connect to QEMU serial console on 127.0.0.1:{} after 60s.\n\n\
                     QEMU appears to be running but the serial port is not accepting connections.\n\
                     Possible causes:\n\
                     - Windows firewall blocking localhost port {}\n\
                     - Another process using port {}\n\
                     - QEMU failed to set up the serial device",
                    self.config.serial_port, self.config.serial_port, self.config.serial_port
                );
            }

            if attempt_count % 5 == 0 {
                emit(pw, "booting", &format!("Still waiting for serial console ({}s)…", attempt_count * 2), 18, "info", None).await;

                // Check if QEMU is still alive (drop guard before any await)
                let qemu_died = {
                    let mut guard = self.child.lock().unwrap();
                    if let Some(ref mut c) = *guard {
                        c.try_wait().ok().flatten()
                    } else {
                        None
                    }
                };

                if let Some(status) = qemu_died {
                    emit(pw, "error", &format!("QEMU process died! Exit: {}", status), 0, "error", None).await;
                    anyhow::bail!("QEMU process died during boot with exit status: {}. \
                        The VM failed to start. Check QEMU binary compatibility and paths.", status);
                }
            }

            sleep(Duration::from_secs(2)).await;
        };

        emit(pw, "booting", "Serial console connected", 25, "success",
            Some(format!("Port {}", self.config.serial_port))).await;

        // Drain boot messages (wait up to 5s for login prompt)
        emit(pw, "booting", "Reading boot output…", 28, "info", None).await;
        serial.drain_boot_output(5).await;

        let boot_log_preview: String = serial.boot_log().chars().rev().take(500).collect::<String>().chars().rev().collect();
        log::info!("Boot log (last 500 chars):\n{}", boot_log_preview);
        emit(pw, "booting", "Boot output captured", 30, "output",
            Some(boot_log_preview.clone())).await;

        // ── Step 3: Login as root ────────────────────────────────────────────
        emit(pw, "booting", "Logging in as root…", 32, "command", Some("root".into())).await;
        log::info!("Sending 'root' login...");

        let mut logged_in = false;
        for attempt in 1..=10 {
            let login_output = serial.exec_command("root", Some("#"), 8).await
                .unwrap_or_default();

            if login_output.contains('#') || login_output.contains("~") {
                emit(pw, "booting", &format!("Logged in (attempt {})", attempt), 35, "success", None).await;
                log::info!("Login successful on attempt {}", attempt);
                logged_in = true;
                break;
            }

            emit(pw, "booting", &format!("Login attempt {} — waiting…", attempt), 32, "output",
                Some(login_output.chars().take(200).collect())).await;
            sleep(Duration::from_secs(3)).await;
        }

        if !logged_in {
            let full_log = serial.boot_log().to_string();
            emit(pw, "error", "Could not login to Alpine VM", 0, "error",
                Some(format!("Boot log:\n{}", full_log.chars().rev().take(2000).collect::<String>().chars().rev().collect::<String>()))).await;
            anyhow::bail!(
                "Could not login to Alpine VM after 10 attempts.\n\n\
                 Boot log (last 2000 chars):\n{}\n\n\
                 Possible causes:\n\
                 - Alpine ISO boot failed\n\
                 - Serial console not attached to the correct TTY\n\
                 - The VM is stuck at a different prompt",
                full_log.chars().rev().take(2000).collect::<String>().chars().rev().collect::<String>()
            );
        }

        // ── Step 4: Network activation ───────────────────────────────────────
        emit(pw, "booting", "Activating network (eth0 up + DHCP)…", 40, "command",
            Some("ifconfig eth0 up && udhcpc -i eth0".into())).await;
        log::info!("Activating network...");

        let net_output = serial.exec_command("ifconfig eth0 up && udhcpc -i eth0", None, 15).await
            .context("Network activation failed")?;
        log::info!("Network output: {}", net_output.chars().take(300).collect::<String>());

        if net_output.contains("lease") || net_output.contains("obtained") || net_output.contains("#") {
            emit(pw, "booting", "Network activated (DHCP lease obtained)", 45, "success", None).await;
        } else {
            emit(pw, "booting", "Network activation — DHCP response unclear, continuing…", 45, "warning",
                Some(net_output.chars().take(200).collect())).await;
            log::warn!("DHCP output unclear, continuing anyway: {}", net_output.chars().take(200).collect::<String>());
        }

        // ── Step 5: Mount persistent storage ─────────────────────────────────
        emit(pw, "booting", "Mounting persistent storage…", 48, "command",
            Some(format!("mount {} {}", self.config.disk_device, self.config.mount_point))).await;
        log::info!("Mounting {} -> {}", self.config.disk_device, self.config.mount_point);

        let mount_output = serial.exec_command(
            &format!("mkdir -p {} && mount {} {} 2>&1 && echo MOUNT_OK",
                self.config.mount_point, self.config.disk_device, self.config.mount_point),
            Some("MOUNT_OK"),
            10,
        ).await.context("Mount command failed")?;

        if mount_output.contains("MOUNT_OK") {
            emit(pw, "booting", "Persistent storage mounted", 52, "success",
                Some(format!("{} → {}", self.config.disk_device, self.config.mount_point))).await;
        } else {
            emit(pw, "booting", "Mount may have failed — checking…", 50, "warning",
                Some(mount_output.chars().take(200).collect())).await;
            log::warn!("Mount output: {}", mount_output);
            // Try to verify mount worked
            let check = serial.exec_command(
                &format!("ls {} 2>&1", self.config.mount_point), None, 5
            ).await.unwrap_or_default();
            log::info!("Mount check (ls): {}", check.chars().take(200).collect::<String>());
        }

        // ── Step 6: Verify agentd binary exists ──────────────────────────────
        emit(pw, "booting", "Verifying agentd binary…", 55, "info",
            Some(format!("ls -la {}", self.config.agentd_path))).await;

        let verify_output = serial.exec_command(
            &format!("ls -la {} 2>&1 && echo AGENT_EXISTS", self.config.agentd_path),
            Some("AGENT_EXISTS"),
            5,
        ).await.unwrap_or_default();

        if verify_output.contains("AGENT_EXISTS") && !verify_output.contains("No such file") {
            emit(pw, "booting", "agentd binary found", 57, "success",
                Some(verify_output.lines().find(|l| l.contains("agentd")).unwrap_or("").to_string())).await;
        } else {
            emit(pw, "error", "agentd binary NOT found on persistent disk!", 0, "error",
                Some(format!(
                    "Expected at: {}\nMount point contents: check logs\n\n\
                     You need to copy the agentd-linux-x86_64 binary to the qcow2 disk.",
                    self.config.agentd_path
                ))).await;
            anyhow::bail!(
                "agentd binary not found at {}.\n\
                 The persistent disk ({}) may not contain the binary.\n\
                 Copy agentd-linux-x86_64 to the disk and try again.",
                self.config.agentd_path,
                self.config.disk_path.display()
            );
        }

        // Make it executable just in case
        let _ = serial.exec_command(
            &format!("chmod +x {}", self.config.agentd_path), None, 3
        ).await;

        // ── Step 7: Install socat (if not already present) ───────────────────
        emit(pw, "installing", "Checking if socat is installed…", 60, "info", None).await;

        let socat_check = serial.exec_command("which socat 2>&1 && echo SOCAT_OK", Some("SOCAT_OK"), 5)
            .await.unwrap_or_default();

        if socat_check.contains("SOCAT_OK") && !socat_check.contains("not found") {
            emit(pw, "installing", "socat already installed", 70, "success", None).await;
            log::info!("socat already installed");
        } else {
            emit(pw, "installing", "Installing socat (requires internet)…", 62, "command",
                Some("apk add --no-cache socat".into())).await;
            log::info!("Installing socat...");

            // Set up repositories
            let _ = serial.exec_command(
                "echo 'https://dl-cdn.alpinelinux.org/alpine/v3.19/main' > /etc/apk/repositories",
                None, 5
            ).await;
            let _ = serial.exec_command(
                "echo 'https://dl-cdn.alpinelinux.org/alpine/v3.19/community' >> /etc/apk/repositories",
                None, 5
            ).await;

            // apk update + install socat
            emit(pw, "installing", "Running apk update…", 64, "command", Some("apk update".into())).await;
            let update_output = serial.exec_command("apk update 2>&1", None, 30)
                .await.unwrap_or_default();
            log::info!("apk update: {}", update_output.chars().take(300).collect::<String>());

            emit(pw, "installing", "Installing socat…", 67, "command", Some("apk add --no-cache socat".into())).await;
            let install_output = serial.exec_command(
                "apk add --no-cache socat 2>&1 && echo INSTALL_OK",
                Some("INSTALL_OK"),
                60,
            ).await.context("socat installation failed")?;

            if install_output.contains("INSTALL_OK") {
                emit(pw, "installing", "socat installed successfully", 70, "success", None).await;
            } else {
                emit(pw, "installing", "socat install may have failed — continuing…", 70, "warning",
                    Some(install_output.chars().take(300).collect())).await;
                log::warn!("socat install output: {}", install_output.chars().take(500).collect::<String>());
            }
        }

        // ── Step 8: Start agentd ─────────────────────────────────────────────
        emit(pw, "booting", "Starting agentd daemon…", 75, "command",
            Some(format!("{} socket --path /tmp/mowisai.sock", self.config.agentd_path))).await;
        log::info!("Starting agentd daemon...");

        // Kill any existing instance first
        let _ = serial.exec_command("pkill -f agentd 2>/dev/null; rm -f /tmp/mowisai.sock", None, 3).await;
        sleep(Duration::from_millis(500)).await;

        // Start agentd in background — use nohup + setsid so it survives when the
        // serial console session ends (otherwise SIGHUP kills backgrounded processes).
        serial.exec_background(
            &format!("nohup setsid {} socket --path /tmp/mowisai.sock </dev/null >/var/log/agentd.log 2>&1 &", self.config.agentd_path)
        ).await.context("Failed to send agentd start command")?;
        sleep(Duration::from_secs(2)).await;

        // Verify it's running
        let ps_output = serial.exec_command("ps aux | grep agentd | grep -v grep", None, 5)
            .await.unwrap_or_default();
        if ps_output.contains("agentd") {
            emit(pw, "booting", "agentd process running", 78, "success",
                Some(ps_output.lines().find(|l| l.contains("agentd")).unwrap_or("").to_string())).await;
        } else {
            emit(pw, "booting", "agentd may not have started — checking socket…", 78, "warning",
                Some(ps_output.chars().take(200).collect())).await;
            log::warn!("agentd process not visible in ps: {}", ps_output);
        }

        // ── Step 9: Start socat TCP bridge ───────────────────────────────────
        emit(pw, "booting", "Starting TCP bridge (socat)…", 82, "command",
            Some("socat TCP-LISTEN:8080,fork,reuseaddr UNIX-CONNECT:/tmp/mowisai.sock &".into())).await;
        log::info!("Starting socat TCP bridge...");

        // Kill any existing socat
        let _ = serial.exec_command("pkill -f 'socat TCP-LISTEN' 2>/dev/null", None, 3).await;
        sleep(Duration::from_millis(500)).await;

        // Same as agentd — nohup + setsid to survive serial disconnect.
        serial.exec_background(
            "nohup setsid socat TCP-LISTEN:8080,fork,reuseaddr UNIX-CONNECT:/tmp/mowisai.sock </dev/null >/var/log/socat.log 2>&1 &"
        ).await.context("Failed to send socat start command")?;
        sleep(Duration::from_secs(1)).await;

        // Verify socat is running
        let socat_ps = serial.exec_command("ps aux | grep socat | grep -v grep", None, 5)
            .await.unwrap_or_default();
        if socat_ps.contains("socat") {
            emit(pw, "booting", "socat bridge running", 85, "success",
                Some(socat_ps.lines().find(|l| l.contains("socat")).unwrap_or("").to_string())).await;
        } else {
            emit(pw, "booting", "socat may not have started — will check port…", 85, "warning", None).await;
            log::warn!("socat process not visible in ps: {}", socat_ps);
        }

        // ── Step 10: Wait for Windows-side port to be reachable ──────────────
        emit(pw, "booting", &format!("Waiting for port {} to become reachable from Windows…", self.config.agent_port), 88, "info", None).await;
        log::info!("Waiting for port {} to become reachable...", self.config.agent_port);

        self.wait_for_port("agentd+socat bridge", PORT_READY_TIMEOUT_SECS, pw).await?;

        // ── Step 11: Write auth token into VM ──────────────────────────────
        emit(pw, "booting", "Writing auth token into VM…", 92, "command",
            Some("mkdir -p /root/.mowisai && write token".into())).await;
        let token_cmd = format!(
            "mkdir -p /root/.mowisai && printf '%s' '{}' > /root/.mowisai/token && chmod 600 /root/.mowisai/token",
            token.replace('\'', "\\'")
        );
        let _ = serial.exec_command(&token_cmd, None, 5).await;

        self.running.store(true, Ordering::SeqCst);
        emit(pw, "ready", "Developer Mode bootstrap complete!", 100, "success",
            Some(format!("Bridge active on 127.0.0.1:{}", self.config.agent_port))).await;
        log::info!("Developer Mode bootstrap complete! Agent reachable on 127.0.0.1:{}", self.config.agent_port);

        Ok(ConnectionInfo {
            kind: ConnectionKind::TcpWithToken,
            socket_path: None,
            tcp_addr: Some(format!("127.0.0.1:{}", self.config.agent_port)),
            pipe_name: None,
            auth_token: Some(token.to_owned()),
        })
    }
}

// ── VmLauncher implementation ────────────────────────────────────────────────

#[async_trait]
impl VmLauncher for DeveloperLauncher {
    async fn start(&self, progress: Option<ProgressSender>) -> Result<ConnectionInfo> {
        let pw = &progress;
        let token = auth::load_or_create().context("load/create auth token")?;

        // ── Kill any zombie QEMU processes from previous attempts ───────────
        emit(pw, "detecting", "Cleaning up any previous QEMU instances…", 2, "info", None).await;
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/IM", "qemu-system-x86_64.exe", "/F"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            // Wait for ports to be released
            sleep(Duration::from_millis(500)).await;
        }
        #[cfg(not(windows))]
        {
            let _ = std::process::Command::new("pkill")
                .args(["-f", "qemu-system"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            sleep(Duration::from_millis(500)).await;
        }

        let args = self.build_qemu_args();
        let full_cmd = format!("{} {}", self.config.qemu_path.display(), args.join(" "));

        emit(pw, "booting", "Starting QEMU…", 5, "command", Some(full_cmd.clone())).await;
        log::info!("Starting QEMU (developer mode): {}", full_cmd);

        // Verify critical ports are free
        for port in [self.config.serial_port, self.config.monitor_port, self.config.agent_port] {
            let addr = format!("127.0.0.1:{}", port);
            if is_tcp_reachable(&addr).await {
                emit(pw, "error", &format!("Port {} is already in use!", port), 0, "error",
                    Some(format!("Another process is listening on {}.\nKill it or change the port in Developer config.", addr))).await;
                anyhow::bail!("Port {} is already in use. Kill the process or change the port.", port);
            }
        }

        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new(&self.config.qemu_path);
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
            c
        };
        #[cfg(not(windows))]
        let mut cmd = Command::new(&self.config.qemu_path);

        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .context("Failed to start QEMU. Verify the binary path is correct.")?;

        // Log stdout in background (non-critical). Keep stderr on the child
        // so bootstrap() can read it if QEMU crashes immediately.
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    log::info!("[QEMU stdout] {}", line);
                }
            });
        }

        emit(pw, "booting", &format!("QEMU process spawned (PID: {:?})", child.id()), 10, "success", None).await;
        log::info!("QEMU process spawned (pid: {:?})", child.id());
        self.bootstrap(child, &token, &args, pw).await
    }

    async fn stop(&self) -> Result<()> {
        let mut child_opt = {
            let mut guard = self.child.lock().unwrap();
            guard.take()
        };

        if let Some(ref mut child) = child_opt {
            log::info!("Stopping QEMU developer mode process");
            let _ = child.kill().await;
        }

        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        {
            let guard = self.child.lock().unwrap();
            if guard.is_none() {
                return Ok(false);
            }
        }

        let addr = format!("127.0.0.1:{}", self.config.agent_port);
        Ok(is_tcp_reachable(&addr).await)
    }

    fn name(&self) -> &str {
        "QEMU/Developer"
    }
}
