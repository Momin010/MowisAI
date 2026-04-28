use crate::launcher::{ConnectionInfo, LauncherConfig, VmLauncher};
use anyhow::{Context, Result};
use rand::Rng;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::time::{sleep, Duration};

/// QEMU launcher - fallback VM launcher using QEMU
///
/// Used when platform-specific virtualization is unavailable:
/// - macOS without Virtualization.framework
/// - Windows without WSL2
/// - Linux (as alternative to direct launch)
pub struct QEMULauncher {
    config: LauncherConfig,
    qemu_binary: PathBuf,
    auth_token: String,
    tcp_port: u16,
}

impl QEMULauncher {
    /// Create a new QEMU launcher
    pub fn new(config: LauncherConfig) -> Result<Self> {
        let qemu_binary = Self::locate_qemu_binary()?;
        let auth_token = crate::auth::generate_auth_token();
        let tcp_port = Self::find_free_port()?;

        Ok(Self {
            config,
            qemu_binary,
            auth_token,
            tcp_port,
        })
    }

    /// Locate QEMU binary
    fn locate_qemu_binary() -> Result<PathBuf> {
        // Try bundled QEMU first
        if let Some(bundled) = Self::bundled_qemu_path() {
            if bundled.exists() {
                return Ok(bundled);
            }
        }

        // Fall back to system QEMU
        let arch = std::env::consts::ARCH;
        let qemu_name = match arch {
            "x86_64" => "qemu-system-x86_64",
            "aarch64" => "qemu-system-aarch64",
            _ => "qemu-system-x86_64",
        };

        which::which(qemu_name)
            .context(format!("QEMU binary '{}' not found in PATH", qemu_name))
    }

    /// Get path to bundled QEMU binary
    fn bundled_qemu_path() -> Option<PathBuf> {
        // On macOS, look in app bundle
        #[cfg(target_os = "macos")]
        {
            if let Ok(exe) = std::env::current_exe() {
                let bundle_dir = exe.parent()?.parent()?.join("Resources");
                let arch = std::env::consts::ARCH;
                let qemu_name = format!("qemu-system-{}", arch);
                let qemu_path = bundle_dir.join(&qemu_name);
                if qemu_path.exists() {
                    return Some(qemu_path);
                }
            }
        }

        // On Windows, look next to executable
        #[cfg(target_os = "windows")]
        {
            if let Ok(exe) = std::env::current_exe() {
                let exe_dir = exe.parent()?;
                let qemu_path = exe_dir.join("qemu-system-x86_64.exe");
                if qemu_path.exists() {
                    return Some(qemu_path);
                }
            }
        }

        None
    }

    /// Find a free ephemeral port
    fn find_free_port() -> Result<u16> {
        // Try random ports in ephemeral range (49152-65535)
        let mut rng = rand::thread_rng();
        
        for _ in 0..10 {
            let port = rng.gen_range(49152..65535);
            if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
                drop(listener);
                return Ok(port);
            }
        }

        Err(anyhow::anyhow!("Could not find free port"))
    }

    /// Wait for TCP port to become available
    async fn wait_for_tcp(&self, timeout: Duration) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;
        let addr: SocketAddr = format!("127.0.0.1:{}", self.tcp_port).parse()?;

        loop {
            if tokio::net::TcpStream::connect(&addr).await.is_ok() {
                return Ok(());
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow::anyhow!(
                    "TCP port {} did not become available within {:?}",
                    self.tcp_port,
                    timeout
                ));
            }

            sleep(Duration::from_millis(100)).await;
        }
    }

    /// Build QEMU command line arguments
    fn build_qemu_args(&self) -> Vec<String> {
        let mut args = vec![];

        // Memory
        args.push("-m".to_string());
        args.push(format!("{}M", self.config.memory_mb));

        // CPUs
        args.push("-smp".to_string());
        args.push(self.config.cpu_count.to_string());

        // Disk image
        args.push("-drive".to_string());
        args.push(format!(
            "file={},format=qcow2,if=virtio",
            self.config.image_path.display()
        ));

        // Network with port forwarding
        args.push("-netdev".to_string());
        args.push(format!(
            "user,id=net0,hostfwd=tcp:127.0.0.1:{}-:8080",
            self.tcp_port
        ));
        args.push("-device".to_string());
        args.push("virtio-net-pci,netdev=net0".to_string());

        // No graphics
        args.push("-nographic".to_string());

        // Accelerator (KVM on Linux, HVF on macOS)
        #[cfg(target_os = "linux")]
        {
            if std::path::Path::new("/dev/kvm").exists() {
                args.push("-accel".to_string());
                args.push("kvm".to_string());
            }
        }

        #[cfg(target_os = "macos")]
        {
            args.push("-accel".to_string());
            args.push("hvf".to_string());
        }

        args
    }
}

#[async_trait::async_trait]
impl VmLauncher for QEMULauncher {
    async fn start(&self) -> Result<ConnectionInfo> {
        log::info!("Starting QEMU VM");
        log::info!("  Image: {:?}", self.config.image_path);
        log::info!("  Memory: {} MB", self.config.memory_mb);
        log::info!("  CPUs: {}", self.config.cpu_count);
        log::info!("  TCP port: {}", self.tcp_port);

        // Write auth token
        crate::auth::write_auth_token(&self.auth_token)?;

        // Build QEMU command
        let args = self.build_qemu_args();
        log::debug!("QEMU command: {:?} {:?}", self.qemu_binary, args);

        // Spawn QEMU process
        let mut child = tokio::process::Command::new(&self.qemu_binary)
            .args(&args)
            .env("AGENTD_AUTH_REQUIRED", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn QEMU process")?;

        // Wait for TCP port to become available (up to 30 seconds)
        if let Err(e) = self.wait_for_tcp(Duration::from_secs(30)).await {
            // Kill the QEMU process if it failed to start
            let _ = child.kill().await;
            return Err(e);
        }

        log::info!("QEMU VM started successfully");

        Ok(ConnectionInfo::TcpWithToken {
            addr: format!("127.0.0.1:{}", self.tcp_port).parse()?,
            token: self.auth_token.clone(),
        })
    }

    async fn stop(&self) -> Result<()> {
        // QEMU child process is not stored in self, so nothing to stop
        // The process will be terminated when the handle is dropped
        log::info!("QEMU VM stop requested (process managed externally)");
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        // Try to connect to TCP port
        let addr: SocketAddr = format!("127.0.0.1:{}", self.tcp_port).parse()?;
        match tokio::net::TcpStream::connect(&addr).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn connection_info(&self) -> Result<ConnectionInfo> {
        Ok(ConnectionInfo::TcpWithToken {
            addr: format!("127.0.0.1:{}", self.tcp_port).parse()?,
            token: self.auth_token.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_free_port() {
        let port = QEMULauncher::find_free_port().unwrap();
        assert!(port >= 49152 && port < 65535);
    }

    #[test]
    fn test_build_qemu_args() {
        let config = LauncherConfig {
            image_path: PathBuf::from("/tmp/test.qcow2"),
            memory_mb: 512,
            cpu_count: 2,
            ..Default::default()
        };

        let launcher = QEMULauncher {
            config,
            qemu_binary: PathBuf::from("qemu-system-x86_64"),
            auth_token: "test-token".to_string(),
            tcp_port: 50000,
        };

        let args = launcher.build_qemu_args();
        
        assert!(args.contains(&"-m".to_string()));
        assert!(args.contains(&"512M".to_string()));
        assert!(args.contains(&"-smp".to_string()));
        assert!(args.contains(&"2".to_string()));
    }
}
