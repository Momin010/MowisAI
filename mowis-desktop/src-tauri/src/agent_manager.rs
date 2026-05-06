use anyhow::{Context, Result};
use log::{info, warn};
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
        // First: check if an agent is already running on any port starting from default
        for port_offset in 0..MAX_PORT_ATTEMPTS {
            let port = DEFAULT_AGENT_PORT + port_offset;
            let test_client = AgentClient::new(port);
            if let Ok(resp) = test_client.health().await {
                if resp.healthy {
                    info!("mowis-agent already running on port {}", port);
                    self.port = port;
                    self.client = test_client;
                    return Ok(());
                }
            }
        }

        // Find the binary
        let agent_path = self.find_agent_binary(resource_dir)?;
        info!("Starting mowis-agent from {}", agent_path.display());

        // Try ports starting from default
        for port_offset in 0..MAX_PORT_ATTEMPTS {
            let port = DEFAULT_AGENT_PORT + port_offset;
            info!("Trying to start mowis-agent on port {}", port);

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
                    self.process = Some(c);
                    self.port = port;
                    self.client = AgentClient::new(port);

                    // Wait for health
                    match self.wait_for_health().await {
                        Ok(()) => {
                            info!("mowis-agent started on port {}", port);
                            return Ok(());
                        }
                        Err(e) => {
                            warn!("Agent on port {} failed health check: {}", port, e);
                            self.stop().await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to spawn on port {}: {}", port, e);
                    continue;
                }
            }
        }

        anyhow::bail!("Failed to start mowis-agent on any port ({}-{})", DEFAULT_AGENT_PORT, DEFAULT_AGENT_PORT + MAX_PORT_ATTEMPTS - 1)
    }

    /// Stop the agent subprocess gracefully.
    pub async fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            info!("Stopping mowis-agent...");
            if let Err(e) = child.kill().await {
                warn!("Failed to kill mowis-agent: {}", e);
            }
            match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => info!("mowis-agent exited with {}", status),
                Ok(Err(e)) => warn!("mowis-agent wait error: {}", e),
                Err(_) => warn!("mowis-agent did not exit within 5s"),
            }
        }
    }

    /// Check if the agent is healthy.
    pub async fn is_healthy(&self) -> bool {
        self.client.health().await.is_ok()
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

    async fn wait_for_health(&self) -> Result<()> {
        let max_attempts = 20;
        let mut delay_ms = 200u64;

        for attempt in 1..=max_attempts {
            match self.client.health().await {
                Ok(resp) if resp.healthy => {
                    info!("mowis-agent healthy after {} attempts (v{})", attempt, resp.version);
                    return Ok(());
                }
                Ok(_) => {
                    info!("mowis-agent responded but not healthy, attempt {}", attempt);
                }
                Err(e) => {
                    if attempt % 5 == 0 {
                        info!("Waiting for mowis-agent... attempt {}/{}: {}", attempt, max_attempts, e);
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            delay_ms = (delay_ms * 2).min(1000);
        }

        anyhow::bail!("mowis-agent did not become healthy within {} attempts", max_attempts)
    }
}

impl Drop for AgentManager {
    fn drop(&mut self) {
        if self.process.is_some() {
            warn!("AgentManager dropped without calling stop() — agent process may be orphaned");
        }
    }
}
