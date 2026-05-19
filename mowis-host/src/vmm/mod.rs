//! Hypervisor abstraction.
//!
//! Each backend boots a Linux VM with `mowis-executor` running inside and a
//! vsock device exposed to the host. The host then connects to the executor
//! via [`crate::transport`].
//!
//! MVP backend: `qemu` (Linux/KVM). Apple Virtualization.framework and WSL2
//! land next.

use async_trait::async_trait;

#[cfg(target_os = "linux")]
pub mod qemu;

/// Configuration for booting a guest VM.
#[derive(Debug, Clone)]
pub struct VmConfig {
    /// Path to the Linux kernel image (bzImage / vmlinuz).
    pub kernel: std::path::PathBuf,
    /// Path to the initramfs (cpio.gz) containing `mowis-executor`.
    pub initrd: std::path::PathBuf,
    /// Optional rootfs disk image (qcow2 / raw). If omitted, the initrd is
    /// the only filesystem.
    pub rootfs: Option<std::path::PathBuf>,
    /// Memory in megabytes.
    pub memory_mb: u32,
    /// vCPU count.
    pub vcpus: u32,
    /// Guest CID for AF_VSOCK. Must be unique per running VM on the host.
    /// CIDs < 3 are reserved.
    pub guest_cid: u32,
    /// vsock port the executor inside the guest listens on.
    pub executor_port: u32,
    /// Extra kernel cmdline parameters.
    pub extra_cmdline: Vec<String>,
}

/// Handle to a running VM.
pub trait VmHandle: Send + Sync {
    /// Guest CID for vsock connection.
    fn guest_cid(&self) -> u32;
    /// vsock port the executor listens on.
    fn executor_port(&self) -> u32;
}

/// Hypervisor backend.
#[async_trait]
pub trait Vmm: Send + Sync {
    /// Boot a new VM. Returns once the hypervisor reports the VM is alive
    /// (not necessarily once the executor is reachable — caller should poll
    /// with [`crate::transport::Connection::ping`]).
    async fn boot(&self, cfg: VmConfig) -> anyhow::Result<Box<dyn VmHandle>>;

    /// Gracefully shut down the VM.
    async fn shutdown(&self, handle: Box<dyn VmHandle>) -> anyhow::Result<()>;
}

/// Pick a default backend for the current host platform.
#[cfg(target_os = "linux")]
pub fn default_backend() -> anyhow::Result<Box<dyn Vmm>> {
    Ok(Box::new(qemu::QemuVmm::new()))
}

#[cfg(not(target_os = "linux"))]
pub fn default_backend() -> anyhow::Result<Box<dyn Vmm>> {
    anyhow::bail!(
        "no vmm backend available on this platform yet (MVP supports Linux/QEMU only; \
         Apple Virtualization.framework and WSL2 backends are next)"
    )
}
