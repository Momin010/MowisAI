//! mowis-host — host-side library that boots a guest VM, pulls images, and
//! talks to `mowis-executor` over vsock.
//!
//! This crate is the replacement for the old `mowis-desktop` QEMU + serial
//! bridge plus the agentd-internal image puller. The architecture is:
//!
//! ```text
//!   user host (mac/win/linux)
//!     │
//!     ├─ image::pull        — skopeo-based OCI -> rootfs.tar.gz
//!     ├─ vmm::Vmm trait     — boot, shutdown a single Linux VM
//!     │     ├─ qemu::QemuVmm (Linux KVM, MVP)
//!     │     ├─ apple_vz::AppleVmm  (TODO — Apple Virtualization.framework)
//!     │     └─ wsl::WslVmm         (TODO — WSL2/hvsocket)
//!     └─ transport::Connection — vsock wrapper around mowis-protocol
//!
//!   guest VM
//!     └─ mowis-executor — vsock server, sandbox primitives, tool registry
//! ```

pub mod image;
pub mod initrd;
pub mod transport;
pub mod vmm;

pub use mowis_protocol as protocol;
