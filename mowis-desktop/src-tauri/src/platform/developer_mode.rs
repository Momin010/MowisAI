// platform/developer_mode.rs — Developer Mode QEMU Bootstrap Launcher
//
// Replaces the old configuration wizard with a fully automated bootstrap:
//   1. Spawns QEMU with the user's ISO + qcow2 disk
//   2. Waits for Alpine to boot (auto-login as root)
//   3. Sends initialization commands via the QEMU monitor serial console
//   4. Mounts persistent storage, installs socat, starts agentd, bridges to TCP
//   5. Returns a TCP ConnectionInfo so the desktop app connects automatically
//
// Uses QEMU monitor `sendkey` to automate shell commands — no manual typing.

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
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};

const BOOT_WAIT_SECS: u64 = 25;
const SHELL_READY_MARKER: &str = "MOWIS_SHELL_OK";
const PORT_READY_TIMEOUT_SECS: u64 = 120;

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
        Self {
            qemu_path: PathBuf::from("qemu-system-x86_64"),
            iso_path: PathBuf::from("alpine-virt-3.19.1-x86_64.iso"),
            disk_path: PathBuf::from("momin_disk.qcow2"),
            ram_mb: 512,
            agent_port: 8080,
            monitor_port: 4445,
            serial_port: 4444,
            mount_point: "/mnt/mowisai".into(),
            disk_device: "/dev/sda".into(),
            agentd_path: "/mnt/mowisai/agentd".into(),
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
            warnings.push(format!("ISO not found: {}", self.iso_path.display()));
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

// ── Launcher ─────────────────────────────────────────────────────────────────

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

        vec![
            "-m".into(), cfg.ram_mb.to_string(),
            "-drive".into(), format!("file={},format=qcow2", cfg.disk_path.display()),
            "-cdrom".into(), cfg.iso_path.display().to_string(),
            "-netdev".into(), format!(
                "user,id=net0,hostfwd=tcp::{}-:8080",
                cfg.agent_port
            ),
            "-device".into(), "virtio-net-pci,netdev=net0".into(),
            "-serial".into(), serial_tcp,
            "-monitor".into(), monitor_tcp,
            "-display".into(), "none".into(),
            "-daemonize".into(),
        ]
    }

    async fn send_via_monitor(&self, keydesc: &str) -> Result<()> {
        let addr = format!("127.0.0.1:{}", self.config.monitor_port);
        let mut stream = timeout(Duration::from_secs(5), TcpStream::connect(&addr))
            .await
            .context("connect to QEMU monitor")?
            .with_context(|| format!("connect to monitor at {addr}"))?;

        let cmd = format!("sendkey {}\n", keydesc);
        stream.write_all(cmd.as_bytes()).await?;
        Ok(())
    }

    fn keydesc_for_char(ch: char) -> String {
        match ch {
            'a'..='z' => ch.to_string(),
            'A'..='Z' => format!("shift-{}", ch.to_ascii_lowercase()),
            '0'..='9' => ch.to_string(),
            ' '  => "spc".into(),
            '/'  => "slash".into(),
            '-'  => "minus".into(),
            '.'  => "dot".into(),
            '_'  => "shift-minus".into(),
            '='  => "equal".into(),
            '+'  => "shift-equal".into(),
            '\'' => "apostrophe".into(),
            '"'  => "shift-apostrophe".into(),
            '\\' => "backslash".into(),
            '|'  => "shift-backslash".into(),
            '\n' => "ret".into(),
            '\t' => "tab".into(),
            ','  => "comma".into(),
            ';'  => "semicolon".into(),
            ':'  => "shift-semicolon".into(),
            '>'  => "shift-dot".into(),
            '<'  => "shift-comma".into(),
            '~'  => "shift-grave_accent".into(),
            '`'  => "grave_accent".into(),
            '!'  => "shift-1".into(),
            '@'  => "shift-2".into(),
            '#'  => "shift-3".into(),
            '$'  => "shift-4".into(),
            '%'  => "shift-5".into(),
            '^'  => "shift-6".into(),
            '&'  => "shift-7".into(),
            '*'  => "shift-8".into(),
            '('  => "shift-9".into(),
            ')'  => "shift-0".into(),
            '{'  => "shift-bracket_left".into(),
            '}'  => "shift-bracket_right".into(),
            '['  => "bracket_left".into(),
            ']'  => "bracket_right".into(),
            '?'  => "shift-slash".into(),
            _    => ch.to_string(),
        }
    }

    async fn type_command(&self, cmd: &str) -> Result<()> {
        for ch in cmd.chars() {
            self.send_via_monitor(&Self::keydesc_for_char(ch)).await?;
            sleep(Duration::from_millis(40)).await;
        }
        self.send_via_monitor("ret").await?;
        sleep(Duration::from_millis(200)).await;
        Ok(())
    }

    async fn type_command_slow(&self, cmd: &str, delay_ms: u64) -> Result<()> {
        for ch in cmd.chars() {
            self.send_via_monitor(&Self::keydesc_for_char(ch)).await?;
            sleep(Duration::from_millis(delay_ms)).await;
        }
        self.send_via_monitor("ret").await?;
        sleep(Duration::from_millis(200)).await;
        Ok(())
    }

    async fn check_shell_ready(&self) -> Result<bool> {
        let addr = format!("127.0.0.1:{}", self.config.serial_port);
        let stream = timeout(Duration::from_secs(5), TcpStream::connect(&addr))
            .await
            .context("connect to serial for shell check")?
            .with_context(|| format!("serial connect at {addr}"))?;

        let mut reader = BufReader::new(stream);

        // Drain any buffered boot output (2 seconds)
        let _ = timeout(Duration::from_secs(2), async {
            let mut buf = vec![0u8; 4096];
            loop {
                match tokio::io::AsyncReadExt::read(&mut reader, &mut buf).await {
                    Ok(0) => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        })
        .await;

        // Send a unique marker command and wait for it on the serial output
        self.type_command(&format!("echo {}", SHELL_READY_MARKER)).await?;

        let found = timeout(Duration::from_secs(15), async {
            let mut accumulated = Vec::new();
            let mut buf = vec![0u8; 1024];
            loop {
                match tokio::io::AsyncReadExt::read(&mut reader, &mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        accumulated.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&accumulated);
                        if text.contains(SHELL_READY_MARKER) {
                            return true;
                        }
                        if accumulated.len() > 50_000 {
                            return false;
                        }
                    }
                    Err(_) => break,
                }
            }
            false
        })
        .await
        .unwrap_or(false);

        Ok(found)
    }

    async fn wait_for_port(&self, label: &str, max_secs: u64) -> Result<()> {
        let addr = format!("127.0.0.1:{}", self.config.agent_port);
        let deadline = std::time::Instant::now() + Duration::from_secs(max_secs);
        while std::time::Instant::now() < deadline {
            if is_tcp_reachable(&addr).await {
                log::info!("{} reachable at {}", label, addr);
                return Ok(());
            }
            sleep(Duration::from_millis(500)).await;
        }
        anyhow::bail!("Timed out waiting for {} on {}", label, addr)
    }

    async fn bootstrap(&self, child: Child, pw: &Option<ProgressSender>) -> Result<ConnectionInfo> {
        *self.child.lock().unwrap() = Some(child);

        emit(pw, "booting", &format!("Waiting {}s for VM to boot…", BOOT_WAIT_SECS), 15, "info", None).await;
        log::info!("Waiting {}s for VM to boot...", BOOT_WAIT_SECS);
        sleep(Duration::from_secs(BOOT_WAIT_SECS)).await;

        // Wait for monitor socket to become available (QEMU may need a moment)
        let monitor_addr = format!("127.0.0.1:{}", self.config.monitor_port);
        emit(pw, "booting", &format!("Waiting for QEMU monitor on {}…", monitor_addr), 20, "info", None).await;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if is_tcp_reachable(&monitor_addr).await {
                break;
            }
            if std::time::Instant::now() > deadline {
                let msg = format!("QEMU monitor not reachable on {}", monitor_addr);
                emit(pw, "error", &msg, 0, "error", None).await;
                anyhow::bail!(
                    "QEMU monitor not reachable on {}. QEMU may have failed to start. \
                     Check that the QEMU binary, ISO, and disk paths are correct.",
                    monitor_addr
                );
            }
            sleep(Duration::from_millis(500)).await;
        }
        emit(pw, "booting", "QEMU monitor ready", 25, "success", Some(format!("Monitor: {}", monitor_addr))).await;
        log::info!("QEMU monitor ready");

        // Wait for shell to be ready — send "root" at login prompt
        emit(pw, "booting", "Waiting for guest login prompt…", 28, "info", None).await;
        log::info!("Waiting for guest login prompt...");
        let mut shell_ok = false;
        for attempt in 1..=15 {
            emit(pw, "booting", &format!("Login attempt {} — sending 'root'…", attempt), 28, "command", Some("root".into())).await;
            log::debug!("Login attempt {} — sending 'root'...", attempt);
            self.type_command("root").await?;
            sleep(Duration::from_secs(3)).await;

            match self.check_shell_ready().await {
                Ok(true) => {
                    emit(pw, "booting", &format!("Guest shell ready (attempt {})", attempt), 35, "success", Some("MOWIS_SHELL_OK marker received".into())).await;
                    log::info!("Guest shell is ready (attempt {})", attempt);
                    shell_ok = true;
                    break;
                }
                Ok(false) => {
                    emit(pw, "booting", &format!("Shell not ready yet (attempt {}), retrying…", attempt), 30, "output", None).await;
                    log::debug!("Shell not ready yet (attempt {}), retrying login...", attempt);
                    sleep(Duration::from_secs(3)).await;
                }
                Err(e) => {
                    emit(pw, "booting", &format!("Shell check error (attempt {}): {}", attempt, e), 30, "warning", None).await;
                    log::warn!("Shell check error (attempt {}): {}", attempt, e);
                    sleep(Duration::from_secs(3)).await;
                }
            }
        }

        if !shell_ok {
            let msg = "Guest shell did not become ready";
            emit(pw, "error", msg, 0, "error", None).await;
            anyhow::bail!(
                "Guest shell did not become ready. The VM may need a different ISO \
                 or the boot process may be stuck."
            );
        }

        // Phase 2: Network activation
        emit(pw, "booting", "Activating network…", 40, "command", Some("ifconfig eth0 up".into())).await;
        log::info!("Activating network...");
        self.type_command_slow("ifconfig eth0 up", 30).await?;
        sleep(Duration::from_secs(1)).await;
        emit(pw, "booting", "Requesting DHCP lease…", 42, "command", Some("udhcpc -i eth0".into())).await;
        self.type_command_slow("udhcpc -i eth0", 30).await?;
        sleep(Duration::from_secs(3)).await;
        emit(pw, "booting", "Network activated", 45, "success", None).await;

        // Phase 2b: Mount persistent storage
        emit(pw, "booting", "Mounting persistent storage…", 48, "command",
            Some(format!("mkdir -p {} && mount {} {}", self.config.mount_point, self.config.disk_device, self.config.mount_point))).await;
        log::info!("Mounting persistent storage...");
        let mount_cmd = format!("mkdir -p {}", self.config.mount_point);
        self.type_command_slow(&mount_cmd, 30).await?;
        sleep(Duration::from_secs(1)).await;
        let disk_cmd = format!(
            "mount {} {}",
            self.config.disk_device, self.config.mount_point
        );
        self.type_command_slow(&disk_cmd, 30).await?;
        sleep(Duration::from_secs(2)).await;
        emit(pw, "booting", "Persistent storage mounted", 52, "success", Some(format!("{} → {}", self.config.disk_device, self.config.mount_point))).await;

        // Phase 3: Repository setup
        emit(pw, "installing", "Setting up APK repositories…", 55, "command",
            Some("echo 'https://dl-cdn.alpinelinux.org/alpine/v3.19/main' > /etc/apk/repositories".into())).await;
        log::info!("Setting up repositories...");
        self.type_command_slow(
            "echo 'https://dl-cdn.alpinelinux.org/alpine/v3.19/main' > /etc/apk/repositories",
            25,
        )
        .await?;
        sleep(Duration::from_secs(1)).await;
        self.type_command_slow(
            "echo 'https://dl-cdn.alpinelinux.org/alpine/v3.19/community' >> /etc/apk/repositories",
            25,
        )
        .await?;
        sleep(Duration::from_secs(1)).await;
        emit(pw, "installing", "Running apk update…", 60, "command", Some("apk update".into())).await;
        self.type_command("apk update").await?;
        sleep(Duration::from_secs(10)).await;
        emit(pw, "installing", "Installing socat…", 65, "command", Some("apk add socat".into())).await;
        self.type_command("apk add socat").await?;
        sleep(Duration::from_secs(10)).await;
        emit(pw, "installing", "Packages installed", 70, "success", None).await;

        // Phase 4: Start agentd
        let agentd_cmd = format!(
            "{0} socket --path /tmp/mowisai.sock &",
            self.config.agentd_path
        );
        emit(pw, "booting", "Starting agentd daemon…", 75, "command", Some(agentd_cmd.clone())).await;
        log::info!("Starting agentd daemon...");
        self.type_command_slow(&agentd_cmd, 30).await?;
        sleep(Duration::from_secs(3)).await;
        emit(pw, "booting", "agentd started", 78, "success", None).await;

        // Phase 4b: Bridge with socat
        let socat_cmd = format!(
            "socat TCP-LISTEN:8080,fork,reuseaddr UNIX-CONNECT:/tmp/mowisai.sock &"
        );
        emit(pw, "booting", "Setting up TCP bridge (socat)…", 80, "command", Some(socat_cmd.clone())).await;
        log::info!("Setting up TCP bridge (socat)...");
        self.type_command_slow(&socat_cmd, 25).await?;
        sleep(Duration::from_secs(2)).await;
        emit(pw, "booting", "socat bridge configured", 82, "success", None).await;

        // Phase 5: Wait for the TCP port to become reachable from Windows
        emit(pw, "booting", &format!("Waiting for agent on port {} (up to {}s)…", self.config.agent_port, PORT_READY_TIMEOUT_SECS), 85, "info", None).await;
        log::info!(
            "Waiting for agent on port {} (up to {}s)...",
            self.config.agent_port,
            PORT_READY_TIMEOUT_SECS
        );
        self.wait_for_port("agentd", PORT_READY_TIMEOUT_SECS)
            .await?;

        self.running.store(true, Ordering::SeqCst);
        emit(pw, "ready", "Developer Mode bootstrap complete!", 100, "success",
            Some(format!("Bridge: 127.0.0.1:{}", self.config.agent_port))).await;
        log::info!("Developer Mode bootstrap complete!");

        Ok(ConnectionInfo {
            kind: ConnectionKind::TcpWithToken,
            socket_path: None,
            tcp_addr: Some(format!("127.0.0.1:{}", self.config.agent_port)),
            pipe_name: None,
            auth_token: None,
        })
    }
}

// ── VmLauncher implementation ────────────────────────────────────────────────

#[async_trait]
impl VmLauncher for DeveloperLauncher {
    async fn start(&self, progress: Option<ProgressSender>) -> Result<ConnectionInfo> {
        let pw = &progress;
        let args = self.build_qemu_args();

        emit(pw, "booting", &format!("Starting QEMU: {} {}", self.config.qemu_path.display(), args.join(" ")), 5, "command",
            Some(format!("{} {}", self.config.qemu_path.display(), args.join(" ")))).await;
        log::info!(
            "Starting QEMU (developer mode): {} {}",
            self.config.qemu_path.display(),
            args.join(" ")
        );

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
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let child = cmd
            .spawn()
            .context("Failed to start QEMU. Verify the binary path is correct.")?;

        emit(pw, "booting", &format!("QEMU process spawned (PID: {:?})", child.id()), 10, "success", None).await;
        log::info!("QEMU process spawned (pid: {:?})", child.id());
        self.bootstrap(child, pw).await
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
