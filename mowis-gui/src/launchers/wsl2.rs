use crate::launcher::{ConnectionInfo, LauncherConfig, VmLauncher};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{sleep, Duration};

/// WSL2 launcher for Windows
///
/// Uses Windows Subsystem for Linux 2 to run Alpine Linux with agentd.
/// Communicates via named pipe bridge.
#[cfg(target_os = "windows")]
pub struct WSL2Launcher {
    config: LauncherConfig,
    distro_name: String,
    install_dir: PathBuf,
}

#[cfg(target_os = "windows")]
impl WSL2Launcher {
    /// Create a new WSL2 launcher
    pub fn new(config: LauncherConfig) -> Result<Self> {
        let distro_name = "MowisAI".to_string();
        let install_dir = Self::get_install_dir()?;

        Ok(Self {
            config,
            distro_name,
            install_dir,
        })
    }

    /// Get the installation directory
    fn get_install_dir() -> Result<PathBuf> {
        let local_app_data = std::env::var("LOCALAPPDATA")
            .context("LOCALAPPDATA environment variable not set")?;
        
        let install_dir = PathBuf::from(local_app_data)
            .join("MowisAI")
            .join("wsl");
        
        std::fs::create_dir_all(&install_dir)
            .context("Failed to create installation directory")?;
        
        Ok(install_dir)
    }

    /// Check if WSL2 is available
    async fn check_wsl2_available() -> Result<bool> {
        let output = Command::new("wsl")
            .arg("--status")
            .output()
            .await
            .context("Failed to run 'wsl --status'")?;

        if !output.status.success() {
            return Ok(false);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.contains("WSL 2") || stdout.contains("version: 2"))
    }

    /// Check if distribution is already imported
    async fn is_distro_imported(&self) -> Result<bool> {
        let output = Command::new("wsl")
            .args(&["--list", "--quiet"])
            .output()
            .await
            .context("Failed to list WSL distributions")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.lines().any(|line| line.trim() == self.distro_name))
    }

    /// Import WSL2 distribution
    async fn import_distribution(&self) -> Result<()> {
        log::info!("Importing WSL2 distribution: {}", self.distro_name);

        let image_path = &self.config.image_path;
        let install_path = self.install_dir.join(&self.distro_name);

        let status = Command::new("wsl")
            .args(&[
                "--import",
                &self.distro_name,
                &install_path.to_string_lossy(),
                &image_path.to_string_lossy(),
            ])
            .status()
            .await
            .context("Failed to import WSL2 distribution")?;

        if !status.success() {
            return Err(anyhow::anyhow!("WSL2 import failed"));
        }

        log::info!("WSL2 distribution imported successfully");
        Ok(())
    }

    /// Unregister distribution (for recovery)
    async fn unregister_distribution(&self) -> Result<()> {
        log::info!("Unregistering WSL2 distribution: {}", self.distro_name);

        let status = Command::new("wsl")
            .args(&["--unregister", &self.distro_name])
            .status()
            .await
            .context("Failed to unregister WSL2 distribution")?;

        if !status.success() {
            log::warn!("WSL2 unregister failed (may not exist)");
        }

        Ok(())
    }

    /// Start agentd in WSL2
    async fn start_agentd(&self) -> Result<()> {
        log::info!("Starting agentd in WSL2");

        Command::new("wsl")
            .args(&[
                "-d",
                &self.distro_name,
                "--",
                "/usr/local/bin/agentd",
                "socket",
                "--path",
                "/tmp/agentd.sock",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to start agentd in WSL2")?;

        // Wait for agentd to start
        sleep(Duration::from_secs(2)).await;

        Ok(())
    }

    /// Check if WSL socket is accessible
    async fn check_wsl_socket(&self) -> bool {
        let socket_path = format!("\\\\wsl$\\{}\\tmp\\agentd.sock", self.distro_name);
        PathBuf::from(&socket_path).exists()
    }
}

#[cfg(target_os = "windows")]
#[async_trait::async_trait]
impl VmLauncher for WSL2Launcher {
    async fn start(&self) -> Result<ConnectionInfo> {
        log::info!("Starting WSL2 launcher");

        // Check if WSL2 is available
        if !Self::check_wsl2_available().await? {
            return Err(anyhow::anyhow!(
                "WSL2 not available. Please enable WSL2 in Windows Features."
            ));
        }

        // Check if distribution is imported
        if !self.is_distro_imported().await? {
            self.import_distribution().await?;
        }

        // Start agentd
        self.start_agentd().await?;

        // Wait for socket to become available
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if self.check_wsl_socket().await {
                break;
            }

            if tokio::time::Instant::now() >= deadline {
                // Try recovery: unregister and re-import
                log::warn!("Socket not available, attempting recovery");
                self.unregister_distribution().await?;
                self.import_distribution().await?;
                self.start_agentd().await?;

                // Wait again
                sleep(Duration::from_secs(5)).await;
                if !self.check_wsl_socket().await {
                    return Err(anyhow::anyhow!(
                        "WSL2 socket did not become available after recovery"
                    ));
                }
                break;
            }

            sleep(Duration::from_millis(100)).await;
        }

        log::info!("WSL2 launcher started successfully");

        // Return named pipe connection info
        // The pipe bridge will be started separately
        Ok(ConnectionInfo::NamedPipe {
            name: "\\\\.\\pipe\\MowisAI\\agentd".to_string(),
        })
    }

    async fn stop(&self) -> Result<()> {
        log::info!("Stopping WSL2 distribution");

        let status = Command::new("wsl")
            .args(&["--terminate", &self.distro_name])
            .status()
            .await
            .context("Failed to terminate WSL2 distribution")?;

        if !status.success() {
            log::warn!("WSL2 terminate failed");
        }

        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(self.check_wsl_socket().await)
    }

    async fn connection_info(&self) -> Result<ConnectionInfo> {
        Ok(ConnectionInfo::NamedPipe {
            name: "\\\\.\\pipe\\MowisAI\\agentd".to_string(),
        })
    }
}

// Stub for non-Windows platforms
#[cfg(not(target_os = "windows"))]
pub struct WSL2Launcher;

#[cfg(not(target_os = "windows"))]
impl WSL2Launcher {
    pub fn new(_config: LauncherConfig) -> Result<Self> {
        Err(anyhow::anyhow!("WSL2 launcher only available on Windows"))
    }
}
