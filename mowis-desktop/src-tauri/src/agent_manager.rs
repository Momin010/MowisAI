use anyhow::{Context, Result};
use log::{info, warn, error};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Child;
use tokio::process::Command;

use crate::agent_client::AgentClient;

/// Default port for the mowis-agent HTTP server.
pub const DEFAULT_AGENT_PORT: u16 = 4096;
/// Maximum ports to try if the default is occupied.
const MAX_PORT_ATTEMPTS: u16 = 10;

/// Manages the mowis-agent subprocess lifecycle.
pub struct AgentManager {
    process: Option<Child>,
    client: AgentClient,
    port: u16,
}

impl AgentManager {
    pub fn new(port: u16) -> Self {
        info!("[agent] AgentManager created for port {}", port);
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
    /// Tries multiple ports if the default is occupied.
    pub async fn start(&mut self, resource_dir: &Path) -> Result<()> {
        info!("[agent] Starting agent discovery — checking ports {}-{}", DEFAULT_AGENT_PORT, DEFAULT_AGENT_PORT + MAX_PORT_ATTEMPTS - 1);
        info!("[agent] Resource dir: {}", resource_dir.display());

        // First: check if an agent is already running on any port starting from default
        for port_offset in 0..MAX_PORT_ATTEMPTS {
            let port = DEFAULT_AGENT_PORT + port_offset;
            let test_client = AgentClient::new(port);
            match test_client.health().await {
                Ok(resp) => {
                    if resp.healthy {
                        info!("[agent] ✓ Found running agent on port {} (v{})", port, resp.version);
                        self.port = port;
                        self.client = test_client;
                        return Ok(());
                    } else {
                        info!("[agent] Port {} responded but not healthy: {:?}", port, resp);
                    }
                }
                Err(e) => {
                    if port_offset == 0 {
                        info!("[agent] Port {} not available: {}", port, e);
                    }
                }
            }
        }

        info!("[agent] No running agent found — will spawn new process");

        // Find the binary
        let agent_path = match self.find_agent_binary(resource_dir) {
            Ok(p) => {
                info!("[agent] ✓ Found binary at: {}", p.display());
                p
            }
            Err(e) => {
                error!("[agent] ✗ Binary not found: {}", e);
                return Err(e);
            }
        };

        // Try ports starting from default
        for port_offset in 0..MAX_PORT_ATTEMPTS {
            let port = DEFAULT_AGENT_PORT + port_offset;
            info!("[agent] Attempting to spawn on port {}...", port);

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
                Ok(c) => {
                    info!("[agent] Process spawned on port {}, waiting for health...", port);
                    self.process = Some(c);
                    self.port = port;
                    self.client = AgentClient::new(port);

                    match self.wait_for_health().await {
                        Ok(()) => {
                            info!("[agent] ✓ Agent healthy on port {}", port);
                            return Ok(());
                        }
                        Err(e) => {
                            warn!("[agent] ✗ Health check failed on port {}: {}", port, e);
                            self.stop().await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    warn!("[agent] ✗ Failed to spawn on port {}: {}", port, e);
                    continue;
                }
            }
        }

        error!("[agent] ✗ Failed to start on any port ({}-{})", DEFAULT_AGENT_PORT, DEFAULT_AGENT_PORT + MAX_PORT_ATTEMPTS - 1);
        anyhow::bail!("Failed to start mowis-agent on any port ({}-{})", DEFAULT_AGENT_PORT, DEFAULT_AGENT_PORT + MAX_PORT_ATTEMPTS - 1)
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

        info!("[agent] Looking for binary: {:?}", names);
        info!("[agent]   Resource dir: {}", resource_dir.display());

        for name in &names {
            let path = resource_dir.join(name);
            info!("[agent]   Checking: {}", path.display());
            if path.exists() {
                return Ok(path);
            }
        }

        if let Ok(exe_dir) = std::env::current_exe() {
            if let Some(dir) = exe_dir.parent() {
                info!("[agent]   Exe dir: {}", dir.display());
                for name in &names {
                    let path = dir.join(name);
                    info!("[agent]   Checking: {}", path.display());
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

    async fn wait_for_health(&self) -> Result<()> {
        let max_attempts = 20;
        let mut delay_ms = 200u64;

        for attempt in 1..=max_attempts {
            match self.client.health().await {
                Ok(resp) if resp.healthy => {
                    info!("[agent] Health check passed on attempt {}/{} (v{})", attempt, max_attempts, resp.version);
                    return Ok(());
                }
                Ok(resp) => {
                    info!("[agent] Responded but not healthy (attempt {}/{}): {:?}", attempt, max_attempts, resp);
                }
                Err(e) => {
                    if attempt <= 3 || attempt % 5 == 0 {
                        info!("[agent] Health attempt {}/{}: {}", attempt, max_attempts, e);
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            delay_ms = (delay_ms * 2).min(1000);
        }

        anyhow::bail!("Not healthy after {} attempts", max_attempts)
    }
}

impl Drop for AgentManager {
    fn drop(&mut self) {
        if self.process.is_some() {
            warn!("[agent] AgentManager dropped without calling stop() — process may be orphaned");
        }
    }
}
