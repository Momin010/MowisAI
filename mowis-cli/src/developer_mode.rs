// developer_mode.rs — Standalone Developer Mode QEMU Bootstrap Launcher (CLI)
//
// Fully automated bootstrap for non-admin Windows users:
//   1. Spawns QEMU with Alpine ISO + persistent qcow2 disk
//   2. Connects to the VM's serial console (TCP) for reliable command execution
//   3. Runs initialization: network, mount, install socat, start agentd, bridge to TCP
//   4. Returns a TCP ConnectionInfo so the CLI connects automatically
//
// Uses the serial console as a bidirectional TTY — far more reliable than
// monitor `sendkey` which is fragile and loses characters.

use crate::auth;
use crate::connection::is_tcp_reachable;
use crate::types::*;
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};

const PORT_READY_TIMEOUT_SECS: u64 = 120;
const ALPINE_ISO: &str = "alpine-virt-3.19.1-x86_64.iso";

// ── Path helpers ──────────────────────────────────────────────────────────────

/// Strip the \\?\ extended-length path prefix that some Windows APIs add.
fn strip_unc(path: PathBuf) -> PathBuf {
    log::debug!("[developer_mode] strip_unc: input={}", path.display());
    let s = path.to_string_lossy();
    if s.starts_with(r"\\?\") {
        let stripped = PathBuf::from(s[4..].to_string());
        log::debug!("[developer_mode] strip_unc: output={}", stripped.display());
        stripped
    } else {
        log::debug!("[developer_mode] strip_unc: no prefix, returning as-is");
        path
    }
}

/// Locate a bundled file by filename.
/// Searches: next to the exe, a "resources/" sub-dir, and the Cargo workspace root.
fn find_bundled_file(name: &str) -> Option<PathBuf> {
    log::debug!("[developer_mode] find_bundled_file: searching for '{}'", name);

    if let Ok(exe) = std::env::current_exe() {
        let exe = strip_unc(exe);
        if let Some(dir) = exe.parent() {
            let p = dir.join(name);
            log::debug!("[developer_mode] find_bundled_file: checking {}", p.display());
            if p.exists() {
                log::debug!("[developer_mode] find_bundled_file: FOUND at {}", p.display());
                return Some(p);
            }
            let p2 = dir.join("resources").join(name);
            log::debug!("[developer_mode] find_bundled_file: checking {}", p2.display());
            if p2.exists() {
                log::debug!("[developer_mode] find_bundled_file: FOUND at {}", p2.display());
                return Some(p2);
            }
        }
    }

    // Workspace root during `cargo run`
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let p = PathBuf::from(&manifest).join("..").join(name);
        log::debug!("[developer_mode] find_bundled_file: checking workspace root {}", p.display());
        if p.exists() {
            log::debug!("[developer_mode] find_bundled_file: FOUND at {}", p.display());
            return Some(p);
        }
    }

    log::debug!("[developer_mode] find_bundled_file: '{}' not found anywhere", name);
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
        log::debug!("[developer_mode] DeveloperConfig::default()");
        let iso_path = find_bundled_file(ALPINE_ISO)
            .unwrap_or_else(|| PathBuf::from(ALPINE_ISO));
        log::debug!("[developer_mode] Default iso_path: {}", iso_path.display());
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
        log::debug!("[developer_mode] DeveloperConfig::load_or_default()");
        let path = Self::config_file_path();
        log::debug!("[developer_mode] Config file path: {}", path.display());
        let config = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| {
                log::debug!("[developer_mode] Read config file ({} bytes)", raw.len());
                serde_json::from_str(&raw).ok()
            })
            .unwrap_or_default();
        log::debug!("[developer_mode] Loaded config: {:?}", config);
        config
    }

    pub fn save(&self) -> Result<()> {
        log::debug!("[developer_mode] DeveloperConfig::save()");
        let path = Self::config_file_path();
        log::debug!("[developer_mode] Saving config to {}", path.display());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        log::debug!("[developer_mode] Config JSON ({} bytes): {}", json.len(), json);
        std::fs::write(&path, json)?;
        log::debug!("[developer_mode] Config saved successfully");
        Ok(())
    }

    pub fn config_file_path() -> PathBuf {
        let path = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("MowisAI")
            .join("developer_config.json");
        log::debug!("[developer_mode] config_file_path: {}", path.display());
        path
    }

    pub fn validate(&self) -> Vec<String> {
        log::debug!("[developer_mode] DeveloperConfig::validate()");
        let mut warnings = Vec::new();

        log::debug!("[developer_mode] Validating qemu_path: {}", self.qemu_path.display());
        if !self.qemu_path.exists() {
            let msg = format!("QEMU binary not found: {}", self.qemu_path.display());
            log::debug!("[developer_mode] Validation warning: {}", msg);
            warnings.push(msg);
        }

        log::debug!("[developer_mode] Validating iso_path: {}", self.iso_path.display());
        if !self.iso_path.exists() {
            if let Some(found) = find_bundled_file(ALPINE_ISO) {
                let msg = format!(
                    "ISO not found at configured path: {}\n  \
                     But found at: {} — update your config to use this path.",
                    self.iso_path.display(), found.display()
                );
                log::debug!("[developer_mode] Validation warning: {}", msg);
                warnings.push(msg);
            } else {
                let msg = format!(
                    "ISO not found: {}\n  \
                     Download from https://dl-cdn.alpinelinux.org/alpine/v3.19/releases/x86_64/{}",
                    self.iso_path.display(), ALPINE_ISO
                );
                log::debug!("[developer_mode] Validation warning: {}", msg);
                warnings.push(msg);
            }
        }

        log::debug!("[developer_mode] Validating disk_path: {}", self.disk_path.display());
        if !self.disk_path.exists() {
            let msg = format!("Disk image not found: {}", self.disk_path.display());
            log::debug!("[developer_mode] Validation warning: {}", msg);
            warnings.push(msg);
        }

        log::debug!("[developer_mode] Validating agent_port: {}", self.agent_port);
        if self.agent_port < 1024 || self.agent_port > 65535 {
            let msg = format!("Invalid agent port: {}", self.agent_port);
            log::debug!("[developer_mode] Validation warning: {}", msg);
            warnings.push(msg);
        }

        log::debug!("[developer_mode] Validation complete: {} warnings", warnings.len());
        warnings
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
        log::debug!("[developer_mode] SerialConsole::connect: port={}, timeout={}s", port, timeout_secs);
        let addr = format!("127.0.0.1:{}", port);
        let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
        let mut attempt = 0u32;

        let stream = loop {
            attempt += 1;
            if std::time::Instant::now() > deadline {
                log::error!("[developer_mode] SerialConsole::connect: timed out after {}s on {}", timeout_secs, addr);
                anyhow::bail!(
                    "Could not connect to QEMU serial console on {}. \
                     QEMU may have failed to start or the port is blocked.",
                    addr
                );
            }
            log::debug!("[developer_mode] SerialConsole::connect: attempt {} to {}", attempt, addr);
            match timeout(Duration::from_secs(2), TcpStream::connect(&addr)).await {
                Ok(Ok(s)) => {
                    log::debug!("[developer_mode] SerialConsole::connect: connected on attempt {}", attempt);
                    break s;
                }
                Ok(Err(e)) => {
                    log::debug!("[developer_mode] SerialConsole::connect: attempt {} failed: {}", attempt, e);
                }
                Err(_) => {
                    log::debug!("[developer_mode] SerialConsole::connect: attempt {} timed out", attempt);
                }
            }
            sleep(Duration::from_millis(500)).await;
        };

        log::debug!("[developer_mode] SerialConsole::connect: splitting stream into read/write halves");
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
        log::debug!("[developer_mode] drain_boot_output: waiting up to {}s for login prompt", wait_secs);
        let _ = timeout(Duration::from_secs(wait_secs), async {
            let mut buf = vec![0u8; 4096];
            loop {
                match self.reader.read(&mut buf).await {
                    Ok(0) => {
                        log::debug!("[developer_mode] drain_boot_output: EOF (0 bytes)");
                        break;
                    }
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]);
                        log::debug!("[developer_mode] drain_boot_output: read {} bytes: {}", n,
                            text.chars().take(120).collect::<String>());
                        self.boot_log.push_str(&text);
                        if self.boot_log.contains("login:") {
                            log::debug!("[developer_mode] drain_boot_output: found 'login:' prompt");
                            break;
                        }
                    }
                    Err(e) => {
                        log::debug!("[developer_mode] drain_boot_output: read error: {}", e);
                        break;
                    }
                }
            }
        }).await;
        log::debug!("[developer_mode] drain_boot_output: boot_log length={} chars", self.boot_log.len());
    }

    /// Send a command and wait for a response containing expected text.
    /// Returns the full output captured after sending the command.
    async fn exec_command(&mut self, cmd: &str, expect: Option<&str>, timeout_secs: u64) -> Result<String> {
        log::debug!("[developer_mode] exec_command: cmd='{}', expect={:?}, timeout={}s",
            cmd, expect, timeout_secs);

        // Write the command + newline
        self.writer.write_all(format!("{}\n", cmd).as_bytes()).await
            .context("write to serial console")?;
        log::debug!("[developer_mode] exec_command: command written to serial console");
        self.writer.flush().await.context("flush serial console")?;
        log::debug!("[developer_mode] exec_command: serial console flushed");

        // Read response until we see the expected marker or timeout
        let mut output = String::new();
        let result = timeout(Duration::from_secs(timeout_secs), async {
            let mut buf = vec![0u8; 4096];
            loop {
                match self.reader.read(&mut buf).await {
                    Ok(0) => {
                        log::debug!("[developer_mode] exec_command: read EOF");
                        break;
                    }
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]);
                        log::trace!("[developer_mode] exec_command: read {} bytes: {}",
                            n, text.chars().take(100).collect::<String>());
                        output.push_str(&text);

                        // If we have an expected string, check for it
                        if let Some(marker) = expect {
                            if output.contains(marker) {
                                log::debug!("[developer_mode] exec_command: found expected marker '{}'", marker);
                                return true;
                            }
                        }

                        // If no expected string, look for a shell prompt (# or $)
                        if expect.is_none() {
                            let trimmed = output.trim_end();
                            if trimmed.ends_with('#') || trimmed.ends_with('$')
                                || trimmed.ends_with("~ #") || trimmed.ends_with(":~#")
                            {
                                log::debug!("[developer_mode] exec_command: found shell prompt");
                                return true;
                            }
                        }

                        // Safety: don't buffer forever
                        if output.len() > 100_000 {
                            log::warn!("[developer_mode] exec_command: output exceeded 100KB, breaking");
                            break;
                        }
                    }
                    Err(e) => {
                        log::debug!("[developer_mode] exec_command: read error: {}", e);
                        break;
                    }
                }
            }
            false
        }).await;

        match result {
            Ok(true) => {
                log::debug!("[developer_mode] exec_command: success ({} bytes output)", output.len());
                Ok(output)
            }
            Ok(false) => {
                log::debug!("[developer_mode] exec_command: completed without marker match ({} bytes output)", output.len());
                Ok(output)
            }
            Err(_) => {
                log::debug!("[developer_mode] exec_command: timed out ({} bytes output)", output.len());
                if output.is_empty() {
                    anyhow::bail!("Command '{}' timed out with no output after {}s", cmd, timeout_secs);
                }
                Ok(output)
            }
        }
    }

    /// Send a command without waiting for a response (fire-and-forget for background commands).
    async fn exec_background(&mut self, cmd: &str) -> Result<()> {
        log::debug!("[developer_mode] exec_background: cmd='{}'", cmd);
        self.writer.write_all(format!("{}\n", cmd).as_bytes()).await
            .context("write to serial console")?;
        self.writer.flush().await.context("flush serial console")?;
        log::debug!("[developer_mode] exec_background: command sent, waiting 500ms");
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
        log::debug!("[developer_mode] DeveloperLauncher::new()");
        log::debug!("[developer_mode] Config: qemu_path={}, iso_path={}, disk_path={}",
            config.qemu_path.display(), config.iso_path.display(), config.disk_path.display());
        log::debug!("[developer_mode] Config: ram_mb={}, agent_port={}, monitor_port={}, serial_port={}",
            config.ram_mb, config.agent_port, config.monitor_port, config.serial_port);
        log::debug!("[developer_mode] Config: mount_point={}, disk_device={}, agentd_path={}",
            config.mount_point, config.disk_device, config.agentd_path);
        Self {
            config,
            child: Mutex::new(None),
            running: AtomicBool::new(false),
        }
    }

    pub fn is_configured() -> bool {
        log::debug!("[developer_mode] DeveloperLauncher::is_configured()");
        let path = DeveloperConfig::config_file_path();
        log::debug!("[developer_mode] Checking config file at {}", path.display());
        let result = path.exists()
            && std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str::<DeveloperConfig>(&raw).ok())
                .map(|cfg| {
                    let exists = cfg.qemu_path.exists();
                    log::debug!("[developer_mode] QEMU binary exists: {}", exists);
                    exists
                })
                .unwrap_or(false);
        log::debug!("[developer_mode] is_configured = {}", result);
        result
    }

    fn build_qemu_args(&self) -> Vec<String> {
        log::debug!("[developer_mode] build_qemu_args()");
        let cfg = &self.config;
        let serial_tcp = format!("tcp:127.0.0.1:{},server=on,wait=off", cfg.serial_port);
        let monitor_tcp = format!("tcp:127.0.0.1:{},server=on,wait=off", cfg.monitor_port);

        log::debug!("[developer_mode] serial_tcp='{}'", serial_tcp);
        log::debug!("[developer_mode] monitor_tcp='{}'", monitor_tcp);

        // Detect if we have vmlinuz-virt and initramfs-virt alongside the ISO
        // (direct kernel boot is faster and guarantees serial console works)
        let iso_dir = cfg.iso_path.parent().unwrap_or(std::path::Path::new("."));
        let vmlinuz = iso_dir.join("vmlinuz-virt");
        let initramfs = iso_dir.join("initramfs-virt");
        let use_direct_kernel = vmlinuz.exists() && initramfs.exists();

        log::debug!("[developer_mode] iso_dir={}", iso_dir.display());
        log::debug!("[developer_mode] vmlinuz={} (exists={})", vmlinuz.display(), vmlinuz.exists());
        log::debug!("[developer_mode] initramfs={} (exists={})", initramfs.display(), initramfs.exists());
        log::debug!("[developer_mode] use_direct_kernel={}", use_direct_kernel);

        let mut args = vec![
            "-m".into(), cfg.ram_mb.to_string(),
            "-drive".into(), format!("file={},format=qcow2", cfg.disk_path.display()),
            "-cdrom".into(), cfg.iso_path.display().to_string(),
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
            log::info!("[developer_mode] Using direct kernel boot: kernel={}, initrd={}",
                vmlinuz.display(), initramfs.display());
            args.push("-kernel".into());
            args.push(vmlinuz.display().to_string());
            args.push("-initrd".into());
            args.push(initramfs.display().to_string());
            args.push("-append".into());
            // IMPORTANT: This must be a SINGLE argument string — spaces are part of the value
            args.push("console=ttyS0 root=/dev/sr0 modules=loop,squashfs,sd-mod,usb-storage quiet".into());
        } else {
            log::info!("[developer_mode] Using ISO boot (no vmlinuz-virt found, BIOS boot — slower)");
            // -nographic redirects serial to stdio AND sets console=ttyS0 in the kernel
            args.push("-nographic".into());
        }

        log::debug!("[developer_mode] Final QEMU args: {:?}", args);
        args
    }

    async fn wait_for_port(&self, label: &str, max_secs: u64, pw: &Option<ProgressSender>) -> Result<()> {
        let addr = format!("127.0.0.1:{}", self.config.agent_port);
        log::debug!("[developer_mode] wait_for_port: label='{}', addr={}, max_secs={}", label, addr, max_secs);
        let deadline = std::time::Instant::now() + Duration::from_secs(max_secs);
        let mut elapsed = 0u64;
        while std::time::Instant::now() < deadline {
            if is_tcp_reachable(&addr).await {
                log::info!("[developer_mode] {} reachable at {}", label, addr);
                return Ok(());
            }
            sleep(Duration::from_secs(1)).await;
            elapsed += 1;
            if elapsed % 5 == 0 {
                log::debug!("[developer_mode] wait_for_port: still waiting ({}s elapsed)", elapsed);
                emit(pw, "booting", &format!("Waiting for {} on {} ({}s)…", label, addr, elapsed), 88, "info", None).await;
            }
        }
        log::error!("[developer_mode] wait_for_port: timed out after {}s for {}", max_secs, label);
        anyhow::bail!(
            "Timed out waiting for {} on {} after {}s.\n\
             The agentd process may have failed to start or socat bridge isn't working.\n\
             Check that the agentd binary exists at: {}",
            label, addr, max_secs, self.config.agentd_path
        )
    }

    async fn bootstrap(&self, mut child: Child, token: &str, pw: &Option<ProgressSender>) -> Result<ConnectionInfo> {
        log::info!("[developer_mode] bootstrap() starting");

        // ── Step 0: Verify QEMU didn't crash immediately ─────────────────────
        // Give QEMU 2 seconds to start, then check if it's still alive
        log::debug!("[developer_mode] Step 0: Waiting 2s before checking QEMU process");
        sleep(Duration::from_secs(2)).await;

        // Check if the process already exited (common: bad args, missing accel, path issues)
        log::debug!("[developer_mode] Step 0: Checking if QEMU process is still alive");
        match child.try_wait() {
            Ok(Some(exit_status)) => {
                log::error!("[developer_mode] Step 0: QEMU already exited with status: {}", exit_status);
                // QEMU already died! Capture stderr for diagnostics
                let stderr_output = if let Some(mut stderr) = child.stderr.take() {
                    let mut buf = String::new();
                    let _ = stderr.read_to_string(&mut buf).await;
                    buf
                } else {
                    "(stderr not captured)".into()
                };

                let msg = format!(
                    "QEMU exited immediately with status: {}\n\nQEMU stderr:\n{}\n\n\
                     Common causes:\n\
                     - Paths with spaces not handled correctly\n\
                     - WHPX/acceleration not available (try without -accel)\n\
                     - Kernel or initrd file not found\n\
                     - Disk image corrupted or locked by another process",
                    exit_status,
                    if stderr_output.is_empty() { "(empty)" } else { &stderr_output }
                );
                emit(pw, "error", "QEMU crashed on startup!", 0, "error", Some(msg.clone())).await;
                log::error!("[developer_mode] QEMU died immediately: {}", msg);
                anyhow::bail!("{}", msg);
            }
            Ok(None) => {
                // Still running — good!
                emit(pw, "booting", "QEMU process is alive", 12, "success", None).await;
                log::info!("[developer_mode] Step 0: QEMU process confirmed alive after 2s");
            }
            Err(e) => {
                log::warn!("[developer_mode] Step 0: Could not check QEMU process status: {}", e);
            }
        }

        // Store child handle for later cleanup
        {
            log::debug!("[developer_mode] Storing child process handle");
            let mut guard = self.child.lock().unwrap();
            *guard = Some(child);
        }

        // ── Step 1: Wait for VM to boot ──────────────────────────────────────
        // Instead of waiting blindly, try to connect to serial port in a loop
        emit(pw, "booting", "Waiting for Alpine to boot (checking serial port)…", 15, "info", None).await;
        log::info!("[developer_mode] Step 1: Waiting for serial console to become available...");

        // Try connecting to serial port — this tells us the VM is alive and QEMU is listening
        let serial_deadline = std::time::Instant::now() + Duration::from_secs(60);
        let mut attempt_count = 0u32;

        let mut serial = loop {
            attempt_count += 1;
            let addr = format!("127.0.0.1:{}", self.config.serial_port);
            log::debug!("[developer_mode] Step 1: Serial connect attempt {} to {}", attempt_count, addr);

            match timeout(Duration::from_secs(2), TcpStream::connect(&addr)).await {
                Ok(Ok(stream)) => {
                    log::info!("[developer_mode] Step 1: Serial console connected after ~{} attempts", attempt_count);
                    emit(pw, "booting", &format!("Serial console connected (took ~{}s)", attempt_count * 2), 25, "success",
                        Some(format!("Port {}", self.config.serial_port))).await;

                    let (read_half, write_half) = tokio::io::split(stream);
                    break SerialConsole {
                        reader: BufReader::new(read_half),
                        writer: write_half,
                        boot_log: String::new(),
                    };
                }
                Ok(Err(e)) => {
                    log::trace!("[developer_mode] Step 1: Serial connect attempt {} error: {}", attempt_count, e);
                }
                Err(_) => {
                    log::trace!("[developer_mode] Step 1: Serial connect attempt {} timed out", attempt_count);
                }
            }

            if std::time::Instant::now() > serial_deadline {
                log::error!("[developer_mode] Step 1: Serial console not available after 60s");
                emit(pw, "error", "Could not connect to QEMU serial console after 60s", 0, "error",
                    Some("QEMU may be running but serial port is not responding. Check if port is blocked by firewall.".into())).await;
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
                log::debug!("[developer_mode] Step 1: Still waiting for serial console (attempt {})", attempt_count);
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
                    log::error!("[developer_mode] Step 1: QEMU process died during boot: {}", status);
                    emit(pw, "error", &format!("QEMU process died! Exit: {}", status), 0, "error", None).await;
                    anyhow::bail!("QEMU process died during boot with exit status: {}. \
                        The VM failed to start. Check QEMU binary compatibility and paths.", status);
                }
            }

            sleep(Duration::from_secs(2)).await;
        };

        emit(pw, "booting", "Serial console connected", 25, "success",
            Some(format!("Port {}", self.config.serial_port))).await;

        // ── Step 2: Drain boot output ────────────────────────────────────────
        log::debug!("[developer_mode] Step 2: Draining boot output");
        emit(pw, "booting", "Reading boot output…", 28, "info", None).await;
        serial.drain_boot_output(5).await;

        let boot_log_preview: String = serial.boot_log().chars().rev().take(500).collect::<String>().chars().rev().collect();
        log::info!("[developer_mode] Step 2: Boot log (last 500 chars):\n{}", boot_log_preview);
        emit(pw, "booting", "Boot output captured", 30, "output",
            Some(boot_log_preview.clone())).await;

        // ── Step 3: Login as root ────────────────────────────────────────────
        log::info!("[developer_mode] Step 3: Attempting root login");
        emit(pw, "booting", "Logging in as root…", 32, "command", Some("root".into())).await;

        let mut logged_in = false;
        for attempt in 1..=10 {
            log::debug!("[developer_mode] Step 3: Login attempt {}", attempt);
            let login_output = serial.exec_command("root", Some("#"), 8).await
                .unwrap_or_default();

            log::debug!("[developer_mode] Step 3: Login attempt {} output ({} chars): {}",
                attempt, login_output.len(),
                login_output.chars().take(200).collect::<String>());

            if login_output.contains('#') || login_output.contains("~") {
                emit(pw, "booting", &format!("Logged in (attempt {})", attempt), 35, "success", None).await;
                log::info!("[developer_mode] Step 3: Login successful on attempt {}", attempt);
                logged_in = true;
                break;
            }

            emit(pw, "booting", &format!("Login attempt {} — waiting…", attempt), 32, "output",
                Some(login_output.chars().take(200).collect())).await;
            sleep(Duration::from_secs(3)).await;
        }

        if !logged_in {
            let full_log = serial.boot_log().to_string();
            let log_tail: String = full_log.chars().rev().take(2000).collect::<String>().chars().rev().collect();
            log::error!("[developer_mode] Step 3: Could not login after 10 attempts. Boot log tail:\n{}", log_tail);
            emit(pw, "error", "Could not login to Alpine VM", 0, "error",
                Some(format!("Boot log:\n{}", log_tail))).await;
            anyhow::bail!(
                "Could not login to Alpine VM after 10 attempts.\n\n\
                 Boot log (last 2000 chars):\n{}\n\n\
                 Possible causes:\n\
                 - Alpine ISO boot failed\n\
                 - Serial console not attached to the correct TTY\n\
                 - The VM is stuck at a different prompt",
                log_tail
            );
        }

        // ── Step 4: Network activation ───────────────────────────────────────
        log::info!("[developer_mode] Step 4: Activating network");
        emit(pw, "booting", "Activating network (eth0 up + DHCP)…", 40, "command",
            Some("ifconfig eth0 up && udhcpc -i eth0".into())).await;

        let net_output = serial.exec_command("ifconfig eth0 up && udhcpc -i eth0", None, 15).await
            .context("Network activation failed")?;
        let net_preview: String = net_output.chars().take(300).collect();
        log::info!("[developer_mode] Step 4: Network output: {}", net_preview);

        if net_output.contains("lease") || net_output.contains("obtained") || net_output.contains("#") {
            emit(pw, "booting", "Network activated (DHCP lease obtained)", 45, "success", None).await;
            log::info!("[developer_mode] Step 4: Network activation successful");
        } else {
            emit(pw, "booting", "Network activation — DHCP response unclear, continuing…", 45, "warning",
                Some(net_output.chars().take(200).collect())).await;
            log::warn!("[developer_mode] Step 4: DHCP output unclear, continuing anyway: {}",
                net_preview);
        }

        // ── Step 5: Mount persistent storage ─────────────────────────────────
        log::info!("[developer_mode] Step 5: Mounting persistent storage");
        emit(pw, "booting", "Mounting persistent storage…", 48, "command",
            Some(format!("mount {} {}", self.config.disk_device, self.config.mount_point))).await;

        let mount_cmd = format!("mkdir -p {} && mount {} {} 2>&1 && echo MOUNT_OK",
            self.config.mount_point, self.config.disk_device, self.config.mount_point);
        log::debug!("[developer_mode] Step 5: Mount command: {}", mount_cmd);

        let mount_output = serial.exec_command(
            &mount_cmd,
            Some("MOUNT_OK"),
            10,
        ).await.context("Mount command failed")?;

        log::debug!("[developer_mode] Step 5: Mount output: {}", mount_output.chars().take(300).collect::<String>());

        if mount_output.contains("MOUNT_OK") {
            emit(pw, "booting", "Persistent storage mounted", 52, "success",
                Some(format!("{} → {}", self.config.disk_device, self.config.mount_point))).await;
            log::info!("[developer_mode] Step 5: Mount successful");
        } else {
            emit(pw, "booting", "Mount may have failed — checking…", 50, "warning",
                Some(mount_output.chars().take(200).collect())).await;
            log::warn!("[developer_mode] Step 5: Mount output unclear: {}", mount_output.chars().take(200).collect::<String>());
            // Try to verify mount worked
            log::debug!("[developer_mode] Step 5: Verifying mount with ls");
            let check = serial.exec_command(
                &format!("ls {} 2>&1", self.config.mount_point), None, 5
            ).await.unwrap_or_default();
            log::info!("[developer_mode] Step 5: Mount check (ls): {}", check.chars().take(200).collect::<String>());
        }

        // ── Step 6: Verify agentd binary exists ──────────────────────────────
        log::info!("[developer_mode] Step 6: Verifying agentd binary at {}", self.config.agentd_path);
        emit(pw, "booting", "Verifying agentd binary…", 55, "info",
            Some(format!("ls -la {}", self.config.agentd_path))).await;

        let verify_cmd = format!("ls -la {} 2>&1 && echo AGENT_EXISTS", self.config.agentd_path);
        log::debug!("[developer_mode] Step 6: Verify command: {}", verify_cmd);

        let verify_output = serial.exec_command(
            &verify_cmd,
            Some("AGENT_EXISTS"),
            5,
        ).await.unwrap_or_default();

        log::debug!("[developer_mode] Step 6: Verify output: {}", verify_output.chars().take(300).collect::<String>());

        if verify_output.contains("AGENT_EXISTS") && !verify_output.contains("No such file") {
            let agentd_line = verify_output.lines().find(|l| l.contains("agentd")).unwrap_or("").to_string();
            emit(pw, "booting", "agentd binary found", 57, "success", Some(agentd_line)).await;
            log::info!("[developer_mode] Step 6: agentd binary verified");
        } else {
            let err_msg = format!(
                "Expected at: {}\nMount point contents: check logs\n\n\
                 You need to copy the agentd-linux-x86_64 binary to the qcow2 disk.",
                self.config.agentd_path
            );
            emit(pw, "error", "agentd binary NOT found on persistent disk!", 0, "error",
                Some(err_msg)).await;
            log::error!("[developer_mode] Step 6: agentd binary NOT found at {}", self.config.agentd_path);
            anyhow::bail!(
                "agentd binary not found at {}.\n\
                 The persistent disk ({}) may not contain the binary.\n\
                 Copy agentd-linux-x86_64 to the disk and try again.",
                self.config.agentd_path,
                self.config.disk_path.display()
            );
        }

        // Make it executable just in case
        log::debug!("[developer_mode] Step 6: Making agentd executable");
        let _ = serial.exec_command(
            &format!("chmod +x {}", self.config.agentd_path), None, 3
        ).await;

        // ── Step 7: Install socat (if not already present) ───────────────────
        log::info!("[developer_mode] Step 7: Checking if socat is installed");
        emit(pw, "installing", "Checking if socat is installed…", 60, "info", None).await;

        let socat_check = serial.exec_command("which socat 2>&1 && echo SOCAT_OK", Some("SOCAT_OK"), 5)
            .await.unwrap_or_default();

        log::debug!("[developer_mode] Step 7: socat check output: {}", socat_check.chars().take(200).collect::<String>());

        if socat_check.contains("SOCAT_OK") && !socat_check.contains("not found") {
            emit(pw, "installing", "socat already installed", 70, "success", None).await;
            log::info!("[developer_mode] Step 7: socat already installed");
        } else {
            emit(pw, "installing", "Installing socat (requires internet)…", 62, "command",
                Some("apk add --no-cache socat".into())).await;
            log::info!("[developer_mode] Step 7: Installing socat...");

            // Set up repositories
            log::debug!("[developer_mode] Step 7: Setting up APK repositories (main)");
            let _ = serial.exec_command(
                "echo 'https://dl-cdn.alpinelinux.org/alpine/v3.19/main' > /etc/apk/repositories",
                None, 5
            ).await;
            log::debug!("[developer_mode] Step 7: Setting up APK repositories (community)");
            let _ = serial.exec_command(
                "echo 'https://dl-cdn.alpinelinux.org/alpine/v3.19/community' >> /etc/apk/repositories",
                None, 5
            ).await;

            // apk update + install socat
            emit(pw, "installing", "Running apk update…", 64, "command", Some("apk update".into())).await;
            log::debug!("[developer_mode] Step 7: Running apk update");
            let update_output = serial.exec_command("apk update 2>&1", None, 30)
                .await.unwrap_or_default();
            log::info!("[developer_mode] Step 7: apk update: {}", update_output.chars().take(300).collect::<String>());

            emit(pw, "installing", "Installing socat…", 67, "command", Some("apk add --no-cache socat".into())).await;
            log::debug!("[developer_mode] Step 7: Running apk add socat");
            let install_output = serial.exec_command(
                "apk add --no-cache socat 2>&1 && echo INSTALL_OK",
                Some("INSTALL_OK"),
                60,
            ).await.context("socat installation failed")?;

            log::debug!("[developer_mode] Step 7: Install output: {}", install_output.chars().take(500).collect::<String>());

            if install_output.contains("INSTALL_OK") {
                emit(pw, "installing", "socat installed successfully", 70, "success", None).await;
                log::info!("[developer_mode] Step 7: socat installed successfully");
            } else {
                emit(pw, "installing", "socat install may have failed — continuing…", 70, "warning",
                    Some(install_output.chars().take(300).collect())).await;
                log::warn!("[developer_mode] Step 7: socat install output unclear: {}",
                    install_output.chars().take(500).collect::<String>());
            }
        }

        // ── Step 8: Start agentd ─────────────────────────────────────────────
        log::info!("[developer_mode] Step 8: Starting agentd daemon");
        emit(pw, "booting", "Starting agentd daemon…", 75, "command",
            Some(format!("{} socket --path /tmp/mowisai.sock", self.config.agentd_path))).await;

        // Kill any existing instance first
        log::debug!("[developer_mode] Step 8: Killing any existing agentd processes");
        let _ = serial.exec_command("pkill -f agentd 2>/dev/null; rm -f /tmp/mowisai.sock", None, 3).await;
        sleep(Duration::from_millis(500)).await;

        // Start agentd in background
        let agentd_start_cmd = format!("{} socket --path /tmp/mowisai.sock &", self.config.agentd_path);
        log::debug!("[developer_mode] Step 8: Starting agentd: {}", agentd_start_cmd);
        serial.exec_background(&agentd_start_cmd).await
            .context("Failed to send agentd start command")?;
        log::debug!("[developer_mode] Step 8: agentd start command sent, waiting 2s");
        sleep(Duration::from_secs(2)).await;

        // Verify it's running
        log::debug!("[developer_mode] Step 8: Verifying agentd process is running");
        let ps_output = serial.exec_command("ps aux | grep agentd | grep -v grep", None, 5)
            .await.unwrap_or_default();

        log::debug!("[developer_mode] Step 8: ps output: {}", ps_output.chars().take(200).collect::<String>());

        if ps_output.contains("agentd") {
            let agentd_line = ps_output.lines().find(|l| l.contains("agentd")).unwrap_or("").to_string();
            emit(pw, "booting", "agentd process running", 78, "success", Some(agentd_line)).await;
            log::info!("[developer_mode] Step 8: agentd process confirmed running");
        } else {
            emit(pw, "booting", "agentd may not have started — checking socket…", 78, "warning",
                Some(ps_output.chars().take(200).collect())).await;
            log::warn!("[developer_mode] Step 8: agentd process not visible in ps: {}",
                ps_output.chars().take(200).collect::<String>());
        }

        // ── Step 9: Start socat TCP bridge ───────────────────────────────────
        log::info!("[developer_mode] Step 9: Starting socat TCP bridge");
        emit(pw, "booting", "Starting TCP bridge (socat)…", 82, "command",
            Some("socat TCP4-LISTEN:8080,fork,reuseaddr UNIX-CONNECT:/tmp/mowisai.sock &".into())).await;

        // Kill any existing socat
        log::debug!("[developer_mode] Step 9: Killing any existing socat processes");
        let _ = serial.exec_command("pkill -f 'socat TCP' 2>/dev/null", None, 3).await;
        sleep(Duration::from_millis(500)).await;

        let socat_cmd = "socat TCP4-LISTEN:8080,fork,reuseaddr UNIX-CONNECT:/tmp/mowisai.sock &";
        log::debug!("[developer_mode] Step 9: Starting socat: {}", socat_cmd);
        serial.exec_background(socat_cmd).await
            .context("Failed to send socat start command")?;
        log::debug!("[developer_mode] Step 9: socat start command sent, waiting 1s");
        sleep(Duration::from_secs(1)).await;

        // Verify socat is running
        log::debug!("[developer_mode] Step 9: Verifying socat process is running");
        let socat_ps = serial.exec_command("ps aux | grep socat | grep -v grep", None, 5)
            .await.unwrap_or_default();

        log::debug!("[developer_mode] Step 9: socat ps output: {}", socat_ps.chars().take(200).collect::<String>());

        if socat_ps.contains("socat") {
            let socat_line = socat_ps.lines().find(|l| l.contains("socat")).unwrap_or("").to_string();
            emit(pw, "booting", "socat bridge running", 85, "success", Some(socat_line)).await;
            log::info!("[developer_mode] Step 9: socat bridge confirmed running");
        } else {
            emit(pw, "booting", "socat may not have started — will check port…", 85, "warning", None).await;
            log::warn!("[developer_mode] Step 9: socat process not visible in ps: {}",
                socat_ps.chars().take(200).collect::<String>());
        }

        // ── Step 10: Wait for Windows-side port to be reachable ──────────────
        log::info!("[developer_mode] Step 10: Waiting for TCP port {} to become reachable", self.config.agent_port);
        emit(pw, "booting", &format!("Waiting for port {} to become reachable from Windows…", self.config.agent_port), 88, "info", None).await;

        self.wait_for_port("agentd+socat bridge", PORT_READY_TIMEOUT_SECS, pw).await?;
        log::info!("[developer_mode] Step 10: Port {} is reachable", self.config.agent_port);

        // ── Step 11: Write auth token into VM ──────────────────────────────
        log::info!("[developer_mode] Step 11: Writing auth token into VM");
        emit(pw, "booting", "Writing auth token into VM…", 92, "command",
            Some("mkdir -p /root/.mowisai && write token".into())).await;

        let token_cmd = format!(
            "mkdir -p /root/.mowisai && printf '%s' '{}' > /root/.mowisai/token && chmod 600 /root/.mowisai/token",
            token.replace('\'', "\\'")
        );
        log::debug!("[developer_mode] Step 11: Token command (redacted)");
        let _ = serial.exec_command(&token_cmd, None, 5).await;
        log::debug!("[developer_mode] Step 11: Auth token written to VM");

        self.running.store(true, Ordering::SeqCst);
        emit(pw, "ready", "Developer Mode bootstrap complete!", 100, "success",
            Some(format!("Bridge active on 127.0.0.1:{}", self.config.agent_port))).await;
        log::info!("[developer_mode] Bootstrap complete! Agent reachable on 127.0.0.1:{}", self.config.agent_port);

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
        log::info!("[developer_mode] VmLauncher::start()");
        let pw = &progress;

        log::debug!("[developer_mode] Loading auth token");
        let token = auth::load_or_create().context("load/create auth token")?;
        log::debug!("[developer_mode] Auth token loaded ({} chars)", token.len());

        let args = self.build_qemu_args();
        let full_cmd = format!("{} {}", self.config.qemu_path.display(), args.join(" "));

        emit(pw, "booting", "Starting QEMU…", 5, "command", Some(full_cmd.clone())).await;
        log::info!("[developer_mode] Starting QEMU (developer mode): {}", full_cmd);

        #[cfg(windows)]
        let mut cmd = {
            log::debug!("[developer_mode] Using CREATE_NO_WINDOW flag on Windows");
            let mut c = Command::new(&self.config.qemu_path);
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
            c
        };
        #[cfg(not(windows))]
        let mut cmd = {
            log::debug!("[developer_mode] Using standard Command (non-Windows)");
            Command::new(&self.config.qemu_path)
        };

        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        log::debug!("[developer_mode] Spawning QEMU process");
        let mut child = cmd
            .spawn()
            .context("Failed to start QEMU. Verify the binary path is correct.")?;

        log::info!("[developer_mode] QEMU process spawned (PID: {:?})", child.id());

        // Log stdout in background (non-critical). Keep stderr on the child
        // so bootstrap() can read it if QEMU crashes immediately.
        if let Some(stdout) = child.stdout.take() {
            log::debug!("[developer_mode] Spawning stdout logger task");
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    log::info!("[QEMU stdout] {}", line);
                }
                log::debug!("[QEMU stdout] Stream closed");
            });
        }

        emit(pw, "booting", &format!("QEMU process spawned (PID: {:?})", child.id()), 10, "success", None).await;
        self.bootstrap(child, &token, pw).await
    }

    async fn stop(&self) -> Result<()> {
        log::info!("[developer_mode] VmLauncher::stop()");
        let mut child_opt = {
            log::debug!("[developer_mode] Taking child process handle from mutex");
            let mut guard = self.child.lock().unwrap();
            guard.take()
        };

        if let Some(ref mut child) = child_opt {
            log::info!("[developer_mode] Killing QEMU process");
            match child.kill().await {
                Ok(()) => log::info!("[developer_mode] QEMU process killed successfully"),
                Err(e) => log::warn!("[developer_mode] Failed to kill QEMU process: {}", e),
            }
        } else {
            log::debug!("[developer_mode] No child process to stop");
        }

        self.running.store(false, Ordering::SeqCst);
        log::debug!("[developer_mode] running flag set to false");
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        log::trace!("[developer_mode] VmLauncher::health_check()");
        {
            let guard = self.child.lock().unwrap();
            if guard.is_none() {
                log::debug!("[developer_mode] health_check: no child process, returning false");
                return Ok(false);
            }
        }

        let addr = format!("127.0.0.1:{}", self.config.agent_port);
        log::debug!("[developer_mode] health_check: probing {}", addr);
        let reachable = is_tcp_reachable(&addr).await;
        log::debug!("[developer_mode] health_check: {} reachable={}", addr, reachable);
        Ok(reachable)
    }

    fn name(&self) -> &str {
        "QEMU/Developer"
    }
}
