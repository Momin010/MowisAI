use anyhow::{Context, Result};
use log::{info, warn, error};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::agent_client::AgentClient;

/// Default port for the mowis-agent HTTP server.
pub const DEFAULT_AGENT_PORT: u16 = 4096;
/// Maximum ports to try if the default is occupied.
const MAX_PORT_ATTEMPTS: u16 = 10;

/// A log entry emitted during agent startup.
/// `(text, level)` where level is "info" | "command" | "output" | "success" | "error" | "warning".
pub type LogEntry = (String, String);

/// Sender for live log entries during startup.
pub type LogSender = mpsc::UnboundedSender<LogEntry>;

fn emit(tx: &Option<LogSender>, text: &str, level: &str) {
    info!("[agent] {}", text);
    if let Some(sender) = tx {
        let _ = sender.send((text.to_string(), level.to_string()));
    }
}

/// Manages the mowis-agent subprocess lifecycle.
pub struct AgentManager {
    process: Option<Child>,
    client: AgentClient,
    port: u16,
}

impl AgentManager {
    pub fn new(port: u16) -> Self {
        Self {
            process: None,
            client: AgentClient::new(port),
            port,
        }
    }

    pub fn client(&self) -> &AgentClient {
        &self.client
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Find and spawn the mowis-agent binary, then wait for it to become healthy.
    /// If `log_tx` is provided, live progress events are sent through it.
    pub async fn start(&mut self, resource_dir: &Path, log_tx: Option<LogSender>) -> Result<()> {
        let tx = log_tx;

        emit(&tx, &format!(
            "Checking ports {}-{} for a running mowis-agent instance...",
            DEFAULT_AGENT_PORT, DEFAULT_AGENT_PORT + MAX_PORT_ATTEMPTS - 1
        ), "info");

        // ── 1. Scan for an already-running agent ──────────────────────────────
        for port_offset in 0..MAX_PORT_ATTEMPTS {
            let port = DEFAULT_AGENT_PORT + port_offset;
            let test_client = AgentClient::new(port);
            match test_client.health().await {
                Ok(resp) if resp.healthy => {
                    emit(&tx, &format!(
                        "Found running agent on port {} (v{})", port, resp.version
                    ), "success");
                    self.port = port;
                    self.client = test_client;
                    return Ok(());
                }
                Ok(resp) => {
                    emit(&tx, &format!(
                        "Port {} responded but not healthy: version={:?}, healthy={}",
                        port, resp.version, resp.healthy
                    ), "warning");
                }
                Err(_) => {
                    // Port not responding — normal, will try to spawn below.
                }
            }
        }

        emit(&tx, "No running agent found", "info");

        // ── 2. Find the binary ────────────────────────────────────────────────
        emit(&tx, &format!("Searching for mowis-agent binary in: {}", resource_dir.display()), "info");

        let agent_path = match self.find_agent_binary(resource_dir) {
            Ok(p) => {
                emit(&tx, &format!("Found binary: {}", p.display()), "success");
                p
            }
            Err(e) => {
                let msg = format!("Binary not found: {:#}", e);
                emit(&tx, &msg, "error");
                // Also list what IS in the directory for debugging
                if let Ok(entries) = std::fs::read_dir(resource_dir) {
                    let names: Vec<String> = entries
                        .filter_map(|e| e.ok())
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .collect();
                    emit(&tx, &format!("Directory contents: [{}]", names.join(", ")), "output");
                }
                return Err(e);
            }
        };

        // ── 3. Try to spawn on each port ──────────────────────────────────────
        for port_offset in 0..MAX_PORT_ATTEMPTS {
            let port = DEFAULT_AGENT_PORT + port_offset;

            // Check if the port is occupied by something other than our agent
            if self.port_is_occupied(port).await {
                emit(&tx, &format!("Port {} is occupied — attempting to free it...", port), "warning");
                if let Err(e) = self.kill_port_process(port).await {
                    emit(&tx, &format!("Could not free port {}: {:#}", port, e), "warning");
                    continue;
                }
                // Give the OS a moment to release the port
                tokio::time::sleep(Duration::from_millis(500)).await;
            }

            emit(&tx, &format!(
                "Spawning mowis-agent on port {}...", port
            ), "command");
            emit(&tx, &format!(
                "  Command: {} serve --port {} --hostname 127.0.0.1",
                agent_path.display(), port
            ), "output");

            let child = Command::new(&agent_path)
                .arg("serve")
                .arg("--port")
                .arg(port.to_string())
                .arg("--hostname")
                .arg("127.0.0.1")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn();

            match child {
                Ok(mut c) => {
                    // Drain stdout/stderr in the background, forwarding lines as output
                    if let Some(stdout) = c.stdout.take() {
                        let tx_clone = tx.clone();
                        tokio::spawn(async move {
                            let mut lines = BufReader::new(stdout).lines();
                            while let Ok(Some(line)) = lines.next_line().await {
                                emit(&tx_clone, &line, "output");
                            }
                        });
                    }
                    if let Some(stderr) = c.stderr.take() {
                        let tx_clone = tx.clone();
                        tokio::spawn(async move {
                            let mut lines = BufReader::new(stderr).lines();
                            while let Ok(Some(line)) = lines.next_line().await {
                                emit(&tx_clone, &line, "output");
                            }
                        });
                    }

                    self.process = Some(c);
                    self.port = port;
                    self.client = AgentClient::new(port);

                    emit(&tx, &format!("Process started on port {}, running health checks...", port), "info");

                    match self.wait_for_health(&tx).await {
                        Ok(()) => {
                            emit(&tx, &format!(
                                "mowis-agent ready on port {}", port
                            ), "success");
                            return Ok(());
                        }
                        Err(e) => {
                            emit(&tx, &format!(
                                "Health check failed on port {}: {:#}", port, e
                            ), "error");
                            self.stop().await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    emit(&tx, &format!(
                        "Failed to spawn on port {}: {:#}", port, e
                    ), "error");
                    continue;
                }
            }
        }

        let msg = format!(
            "Failed to start mowis-agent on any port ({}-{})",
            DEFAULT_AGENT_PORT, DEFAULT_AGENT_PORT + MAX_PORT_ATTEMPTS - 1
        );
        emit(&tx, &msg, "error");
        anyhow::bail!("{}", msg)
    }

    /// Stop the agent subprocess gracefully.
    pub async fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            info!("[agent] Stopping agent on port {}...", self.port);
            if let Err(e) = child.kill().await {
                warn!("[agent] Failed to kill: {}", e);
            }
            match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => info!("[agent] Exited with {}", status),
                Ok(Err(e)) => warn!("[agent] Wait error: {}", e),
                Err(_) => warn!("[agent] Did not exit within 5s"),
            }
        }
    }

    /// Check if the agent is healthy.
    pub async fn is_healthy(&self) -> bool {
        match self.client.health().await {
            Ok(resp) => {
                info!("[agent] Health check OK: v{}", resp.version);
                true
            }
            Err(e) => {
                warn!("[agent] Health check failed: {}", e);
                false
            }
        }
    }

    fn find_agent_binary(&self, resource_dir: &Path) -> Result<PathBuf> {
        let names = if cfg!(target_os = "windows") {
            vec!["mowis-agent.exe", "mowis-agent"]
        } else {
            vec!["mowis-agent"]
        };

        for name in &names {
            let path = resource_dir.join(name);
            if path.exists() {
                return Ok(path);
            }
        }

        if let Ok(exe_dir) = std::env::current_exe() {
            if let Some(dir) = exe_dir.parent() {
                for name in &names {
                    let path = dir.join(name);
                    if path.exists() {
                        return Ok(path);
                    }
                }
            }
        }

        anyhow::bail!(
            "mowis-agent binary not found in {} or executable directory",
            resource_dir.display()
        )
    }

    async fn wait_for_health(&self, tx: &Option<LogSender>) -> Result<()> {
        let max_attempts = 30;
        let mut delay_ms = 250u64;

        for attempt in 1..=max_attempts {
            match self.client.health().await {
                Ok(resp) if resp.healthy => {
                    emit(tx, &format!(
                        "Health check passed on attempt {}/{} (v{})",
                        attempt, max_attempts, resp.version
                    ), "success");
                    return Ok(());
                }
                Ok(resp) => {
                    emit(tx, &format!(
                        "Attempt {}/{}: responded but not healthy (version={:?})",
                        attempt, max_attempts, resp.version
                    ), "warning");
                }
                Err(e) => {
                    // Only log every few attempts to avoid spam
                    if attempt == 1 || attempt == 3 || attempt == 5 || attempt % 10 == 0 {
                        emit(tx, &format!(
                            "Attempt {}/{}: {}", attempt, max_attempts, e
                        ), "info");
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            delay_ms = (delay_ms * 2).min(2000);
        }

        anyhow::bail!("Not healthy after {} attempts ({}ms total)", max_attempts, max_attempts * 1000)
    }

    /// Check if a port is occupied (something is listening on it but it's not our healthy agent).
    async fn port_is_occupied(&self, port: u16) -> bool {
        match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            Ok(_listener) => {
                // Port is free — bind succeeded, drop the listener to release it
                false
            }
            Err(_) => {
                // Port is occupied
                true
            }
        }
    }

    /// Kill the process occupying the given port.
    async fn kill_port_process(&self, port: u16) -> Result<()> {
        if cfg!(target_os = "windows") {
            // On Windows: use netstat to find the PID, then taskkill
            let output = Command::new("netstat")
                .arg("-ano")
                .output()
                .await
                .context("Failed to run netstat")?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let port_str = format!(":{}", port);

            for line in stdout.lines() {
                if line.contains(&port_str) && (line.contains("LISTENING") || line.contains("ESTABLISHED")) {
                    // Extract PID (last column)
                    if let Some(pid_str) = line.split_whitespace().last() {
                        if let Ok(pid) = pid_str.parse::<u32>() {
                            // Don't kill our own process
                            if pid == std::process::id() {
                                continue;
                            }
                            info!("[agent] Killing PID {} on port {}", pid, port);
                            let kill_result = Command::new("taskkill")
                                .args(["/F", "/PID", &pid.to_string()])
                                .output()
                                .await;
                            match kill_result {
                                Ok(o) if o.status.success() => {
                                    return Ok(());
                                }
                                Ok(o) => {
                                    let stderr = String::from_utf8_lossy(&o.stderr);
                                    anyhow::bail!("taskkill failed: {}", stderr);
                                }
                                Err(e) => {
                                    anyhow::bail!("taskkill error: {}", e);
                                }
                            }
                        }
                    }
                }
            }
            anyhow::bail!("Could not find PID for port {}", port)
        } else {
            // On Unix: use lsof or fuser
            let output = Command::new("fuser")
                .args([format!("{}/tcp", port)])
                .output()
                .await
                .context("Failed to run fuser")?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(pid_str) = stdout.split_whitespace().next() {
                if let Ok(pid) = pid_str.parse::<u32>() {
                    info!("[agent] Killing PID {} on port {}", pid, port);
                    Command::new("kill")
                        .args(["-9", &pid.to_string()])
                        .output()
                        .await
                        .context("Failed to kill process")?;
                    return Ok(());
                }
            }
            anyhow::bail!("Could not find PID for port {}", port)
        }
    }
}

impl Drop for AgentManager {
    fn drop(&mut self) {
        if self.process.is_some() {
            warn!("[agent] AgentManager dropped without calling stop() — process may be orphaned");
        }
    }
}
