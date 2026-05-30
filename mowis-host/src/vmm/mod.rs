//! Hypervisor abstraction.
//!
//! Each backend boots a Linux VM with `mowis-executor` running inside and a
//! transport device exposed to the host. The host then connects to the executor
//! via [`crate::transport`].
//!
//! | Backend          | Platform         | Transport                          |
//! |------------------|------------------|------------------------------------|
//! | `qemu::QemuVmm`  | Linux/macOS/Win  | vsock (Linux) or TCP port-forward  |
//!
//! Additional backends (Apple Virtualization.framework, WSL2 hvsocket) are
//! planned but not yet implemented.

use async_trait::async_trait;

pub mod qemu;

/// Configuration for booting a guest VM.
#[derive(Debug, Clone)]
pub struct VmConfig {
    /// Path to the Linux kernel image (bzImage / vmlinuz).
    pub kernel: std::path::PathBuf,
    /// Path to the initramfs (cpio.gz) containing `mowis-executor`.
    pub initrd: std::path::PathBuf,
    /// Optional rootfs disk image (qcow2 / raw).
    pub rootfs: Option<std::path::PathBuf>,
    /// Memory in megabytes.
    pub memory_mb: u32,
    /// vCPU count.
    pub vcpus: u32,
    /// Guest CID for AF_VSOCK (Linux host only). Must be unique per running VM.
    pub guest_cid: u32,
    /// Port the executor inside the guest listens on.
    /// vsock port on Linux; TCP port on macOS/Windows.
    pub executor_port: u32,
    /// Extra kernel cmdline parameters.
    pub extra_cmdline: Vec<String>,
}

/// Handle to a running VM.
pub trait VmHandle: Send + Sync {
    /// Guest CID (vsock, Linux only).
    fn guest_cid(&self) -> u32;
    /// Port the executor listens on.
    fn executor_port(&self) -> u32;
    /// Hostname for TCP connections (non-Linux). Empty string on Linux.
    fn executor_host(&self) -> &str {
        ""
    }
    /// True when TCP should be used instead of vsock.
    fn use_tcp(&self) -> bool {
        !cfg!(target_os = "linux")
    }
}

/// Hypervisor backend.
#[async_trait]
pub trait Vmm: Send + Sync {
    /// Boot a new VM. Returns once the hypervisor has started the VM
    /// (caller should poll `transport::Connection::ping` until executor is ready).
    async fn boot(&self, cfg: VmConfig) -> anyhow::Result<Box<dyn VmHandle>>;

    /// Shut down the VM.
    async fn shutdown(&self, handle: Box<dyn VmHandle>) -> anyhow::Result<()>;
}

/// Return the best available VMM backend for the current platform.
/// QEMU works on Linux, macOS, and Windows — install it via your package
/// manager (`apt install qemu-system-x86`, `brew install qemu`,
/// or download from qemu.org on Windows).
pub fn default_backend() -> anyhow::Result<Box<dyn Vmm>> {
    Ok(Box::new(qemu::QemuVmm::new()))
}
