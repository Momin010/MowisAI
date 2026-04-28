use crate::launcher::{ConnectionInfo, LauncherConfig, VmLauncher};
use anyhow::{Context, Result};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::PathBuf;
use tokio::time::{sleep, Duration};

/// macOS launcher using Virtualization.framework
///
/// Uses Apple's native Virtualization.framework for fast, lightweight VMs
/// with virtio-vsock for socket communication.
#[cfg(target_os = "macos")]
pub struct MacOSLauncher {
    config: LauncherConfig,
    socket_path: PathBuf,
}

#[cfg(target_os = "macos")]
impl MacOSLauncher {
    /// Create a new macOS launcher
    pub fn new(config: LauncherConfig) -> Result<Self> {
        let socket_path = Self::vsock_socket_path()?;
        
        Ok(Self {
            config,
            socket_path,
        })
    }

    /// Get the vsock socket path
    fn vsock_socket_path() -> Result<PathBuf> {
        // Try XDG_RUNTIME_DIR first
        if let Ok(xdg_runtime) = std::env::var("XDG_RUNTIME_DIR") {
            return Ok(PathBuf::from(xdg_runtime).join("agentd-vsock.sock"));
        }

        // Fallback to /tmp
        Ok(PathBuf::from("/tmp/mowisai-vsock.sock"))
    }

    /// Wait for socket to become available
    async fn wait_for_socket(&self, timeout: Duration) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if self.socket_path.exists() {
                // Try to connect to verify it's responsive
                if tokio::net::UnixStream::connect(&self.socket_path).await.is_ok() {
                    return Ok(());
                }
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow::anyhow!(
                    "Socket did not become available within {:?}",
                    timeout
                ));
            }

            sleep(Duration::from_millis(100)).await;
        }
    }

    /// Check if snapshot exists
    fn snapshot_exists(&self) -> bool {
        if let Some(snapshot_path) = &self.config.snapshot_dir.join("macos-vm.snapshot").exists() {
            return *snapshot_path;
        }
        false
    }
}

// FFI declarations for Swift shim
#[cfg(target_os = "macos")]
extern "C" {
    fn mowis_start_vm(
        image_path: *const c_char,
        memory_mb: u64,
        cpu_count: u32,
        socket_path: *mut c_char,
        socket_path_len: u32,
        error_out: *mut *mut c_char,
    ) -> bool;

    fn mowis_stop_vm() -> bool;

    fn mowis_create_snapshot(
        snapshot_path: *const c_char,
        error_out: *mut *mut c_char,
    ) -> bool;

    fn mowis_restore_snapshot(
        snapshot_path: *const c_char,
        error_out: *mut *mut c_char,
    ) -> bool;
}

#[cfg(target_os = "macos")]
#[async_trait::async_trait]
impl VmLauncher for MacOSLauncher {
    async fn start(&self) -> Result<ConnectionInfo> {
        log::info!("Starting macOS VM using Virtualization.framework");
        log::info!("  Image: {:?}", self.config.image_path);
        log::info!("  Memory: {} MB", self.config.memory_mb);
        log::info!("  CPUs: {}", self.config.cpu_count);

        // Check if we can restore from snapshot
        if self.config.enable_snapshots && self.snapshot_exists() {
            log::info!("Restoring from snapshot for fast boot");
            
            let snapshot_path = self.config.snapshot_dir.join("macos-vm.snapshot");
            let snapshot_path_c = CString::new(snapshot_path.to_string_lossy().as_ref())?;
            let mut error_ptr: *mut c_char = std::ptr::null_mut();

            unsafe {
                if !mowis_restore_snapshot(snapshot_path_c.as_ptr(), &mut error_ptr) {
                    let error_msg = if !error_ptr.is_null() {
                        CStr::from_ptr(error_ptr).to_string_lossy().into_owned()
                    } else {
                        "Unknown error".to_string()
                    };
                    return Err(anyhow::anyhow!("Failed to restore snapshot: {}", error_msg));
                }
            }

            // Wait for socket (should be fast with snapshot)
            self.wait_for_socket(Duration::from_secs(5)).await?;
        } else {
            // Full boot
            log::info!("Performing full boot (first time)");

            let image_path_c = CString::new(self.config.image_path.to_string_lossy().as_ref())?;
            let mut socket_path_buf = vec![0u8; 256];
            let mut error_ptr: *mut c_char = std::ptr::null_mut();

            unsafe {
                if !mowis_start_vm(
                    image_path_c.as_ptr(),
                    self.config.memory_mb,
                    self.config.cpu_count,
                    socket_path_buf.as_mut_ptr() as *mut c_char,
                    socket_path_buf.len() as u32,
                    &mut error_ptr,
                ) {
                    let error_msg = if !error_ptr.is_null() {
                        CStr::from_ptr(error_ptr).to_string_lossy().into_owned()
                    } else {
                        "Unknown error".to_string()
                    };
                    return Err(anyhow::anyhow!("Failed to start VM: {}", error_msg));
                }
            }

            // Wait for socket (first boot takes longer)
            self.wait_for_socket(Duration::from_secs(20)).await?;

            // Create snapshot for next time
            if self.config.enable_snapshots {
                log::info!("Creating snapshot for fast future boots");
                
                std::fs::create_dir_all(&self.config.snapshot_dir)?;
                let snapshot_path = self.config.snapshot_dir.join("macos-vm.snapshot");
                let snapshot_path_c = CString::new(snapshot_path.to_string_lossy().as_ref())?;
                let mut error_ptr: *mut c_char = std::ptr::null_mut();

                unsafe {
                    if !mowis_create_snapshot(snapshot_path_c.as_ptr(), &mut error_ptr) {
                        log::warn!("Failed to create snapshot (non-fatal)");
                    }
                }
            }
        }

        log::info!("macOS VM started successfully");

        Ok(ConnectionInfo::Vsock {
            path: self.socket_path.clone(),
        })
    }

    async fn stop(&self) -> Result<()> {
        log::info!("Stopping macOS VM");
        
        unsafe {
            if !mowis_stop_vm() {
                log::warn!("Failed to stop VM gracefully");
            }
        }
        
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        // Try to connect to vsock socket
        match tokio::net::UnixStream::connect(&self.socket_path).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn connection_info(&self) -> Result<ConnectionInfo> {
        Ok(ConnectionInfo::Vsock {
            path: self.socket_path.clone(),
        })
    }
}

// Stub for non-macOS platforms
#[cfg(not(target_os = "macos"))]
pub struct MacOSLauncher;

#[cfg(not(target_os = "macos"))]
impl MacOSLauncher {
    pub fn new(_config: LauncherConfig) -> Result<Self> {
        Err(anyhow::anyhow!("macOS launcher only available on macOS"))
    }
}
