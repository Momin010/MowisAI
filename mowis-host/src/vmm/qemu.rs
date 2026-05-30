//! QEMU backend — works on Linux, macOS, and Windows.
//!
//! | Platform | Acceleration           | Guest transport              |
//! |----------|------------------------|------------------------------|
//! | Linux    | KVM (+ TCG fallback)   | vhost-vsock-pci (AF_VSOCK)   |
//! | macOS    | HVF (+ TCG fallback)   | TCP port-forward via SLIRP   |
//! | Windows  | WHPX (+ TCG fallback)  | TCP port-forward via SLIRP   |
//!
//! Install QEMU:
//!   Linux  — `apt install qemu-system-x86`
//!   macOS  — `brew install qemu`
//!   Windows — https://qemu.weilnetz.de/ or `choco install qemu`

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::process::Stdio;
use tokio::process::{Child, Command};

use super::{VmConfig, VmHandle, Vmm};

// ── QemuVmm ───────────────────────────────────────────────────────────────────

pub struct QemuVmm {
    qemu_bin: String,
}

impl QemuVmm {
    pub fn new() -> Self {
        Self {
            qemu_bin: "qemu-system-x86_64".to_string(),
        }
    }

    pub fn with_binary(qemu_bin: impl Into<String>) -> Self {
        Self { qemu_bin: qemu_bin.into() }
    }
}

impl Default for QemuVmm {
    fn default() -> Self {
        Self::new()
    }
}

// ── VmHandle implementations ──────────────────────────────────────────────────

/// Linux handle: guest reachable via AF_VSOCK.
pub struct QemuVsockHandle {
    #[allow(dead_code)]
    child: Child,
    cid: u32,
    port: u32,
}

impl VmHandle for QemuVsockHandle {
    fn guest_cid(&self) -> u32 { self.cid }
    fn executor_port(&self) -> u32 { self.port }
    fn use_tcp(&self) -> bool { false }
}

/// macOS / Windows handle: guest reachable via TCP port-forward.
pub struct QemuTcpHandle {
    #[allow(dead_code)]
    child: Child,
    host_port: u16,
    #[allow(dead_code)]
    executor_port: u32,
}

impl VmHandle for QemuTcpHandle {
    fn guest_cid(&self) -> u32 { 0 }
    fn executor_port(&self) -> u32 { self.host_port as u32 }
    fn executor_host(&self) -> &str { "127.0.0.1" }
    fn use_tcp(&self) -> bool { true }
}

// ── Platform helpers ──────────────────────────────────────────────────────────

/// Acceleration flags, ordered from fastest to most compatible.
fn platform_accel() -> &'static str {
    if cfg!(target_os = "linux") {
        "kvm:tcg"
    } else if cfg!(target_os = "macos") {
        "hvf:tcg"
    } else if cfg!(target_os = "windows") {
        "whpx:tcg"
    } else {
        "tcg"
    }
}

fn build_cmdline(cfg: &VmConfig) -> String {
    let mut parts: Vec<String> = vec![
        "console=ttyS0".into(),
        "reboot=k".into(),
        "panic=1".into(),
        format!("mowis.executor_port={}", cfg.executor_port),
    ];
    parts.extend(cfg.extra_cmdline.iter().cloned());
    parts.join(" ")
}

// ── Vmm impl ──────────────────────────────────────────────────────────────────

#[async_trait]
impl Vmm for QemuVmm {
    async fn boot(&self, cfg: VmConfig) -> Result<Box<dyn VmHandle>> {
        which::which(&self.qemu_bin)
            .with_context(|| format!("`{}` not found — install QEMU first", self.qemu_bin))?;

        let cmdline = build_cmdline(&cfg);

        let mut args: Vec<String> = vec![
            "-machine".into(),
            format!("q35,accel={}", platform_accel()),
            "-cpu".into(),
            "host".into(),
            "-smp".into(),
            cfg.vcpus.to_string(),
            "-m".into(),
            cfg.memory_mb.to_string(),
            "-nographic".into(),
            "-no-reboot".into(),
            "-kernel".into(),
            cfg.kernel.display().to_string(),
            "-initrd".into(),
            cfg.initrd.display().to_string(),
            "-append".into(),
            cmdline,
        ];

        if let Some(rootfs) = &cfg.rootfs {
            args.extend([
                "-drive".into(),
                format!("file={},if=virtio,format=raw", rootfs.display()),
            ]);
        }

        // ── Guest transport device ────────────────────────────────────────────
        //
        // Linux: vhost-vsock-pci gives AF_VSOCK — low latency, no NAT.
        // macOS/Windows: QEMU's SLIRP user-mode network with a TCP port
        // forwarded from the host. The guest executor must listen on TCP
        // cfg.executor_port; the host connects to 127.0.0.1:host_port.

        #[cfg(target_os = "linux")]
        {
            args.extend([
                "-device".into(),
                format!("vhost-vsock-pci,guest-cid={}", cfg.guest_cid),
                "-netdev".into(),
                "user,id=net0".into(),
                "-device".into(),
                "virtio-net-pci,netdev=net0".into(),
            ]);

            tracing::info!(
                cid = cfg.guest_cid,
                port = cfg.executor_port,
                kernel = %cfg.kernel.display(),
                accel = platform_accel(),
                "spawning qemu (vsock)"
            );

            let child = Command::new(&self.qemu_bin)
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .kill_on_drop(true)
                .spawn()
                .with_context(|| format!("spawn {}", self.qemu_bin))?;

            return Ok(Box::new(QemuVsockHandle {
                child,
                cid: cfg.guest_cid,
                port: cfg.executor_port,
            }));
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Pick a host-side port to forward. Re-use executor_port if it's
            // in the ephemeral range; otherwise find a free one.
            let host_port = if cfg.executor_port > 1024 && cfg.executor_port < 65535 {
                cfg.executor_port as u16
            } else {
                free_tcp_port()?
            };

            args.extend([
                "-netdev".into(),
                format!(
                    "user,id=net0,hostfwd=tcp::{host_port}-:{port}",
                    port = cfg.executor_port
                ),
                "-device".into(),
                "virtio-net-pci,netdev=net0".into(),
            ]);

            tracing::info!(
                host_port,
                executor_port = cfg.executor_port,
                kernel = %cfg.kernel.display(),
                accel = platform_accel(),
                "spawning qemu (TCP port-forward)"
            );

            let child = Command::new(&self.qemu_bin)
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .kill_on_drop(true)
                .spawn()
                .with_context(|| format!("spawn {}", self.qemu_bin))?;

            return Ok(Box::new(QemuTcpHandle {
                child,
                host_port,
                executor_port: cfg.executor_port,
            }));
        }
    }

    async fn shutdown(&self, handle: Box<dyn VmHandle>) -> Result<()> {
        // Dropping the handle kills the QEMU child via `kill_on_drop`.
        // A future iteration will send a QMP `system_powerdown` for
        // a graceful ACPI shutdown before falling back to SIGKILL.
        drop(handle);
        Ok(())
    }
}

/// Find a free TCP port by binding to port 0 and reading the assigned port.
#[cfg(not(target_os = "linux"))]
fn free_tcp_port() -> Result<u16> {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").context("bind to find free port")?;
    let port = listener.local_addr()?.port();
    Ok(port)
}
