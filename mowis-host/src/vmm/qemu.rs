//! QEMU/KVM backend (Linux host).
//!
//! Spawns `qemu-system-x86_64` with `vhost-vsock-pci` so the guest is reachable
//! over AF_VSOCK at the configured CID. The MVP boots a minimal Linux image
//! (kernel + initrd containing `mowis-executor`) — that bundle is built
//! separately and out of scope for this crate.

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::process::Stdio;
use tokio::process::{Child, Command};

use super::{VmConfig, VmHandle, Vmm};

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
        Self {
            qemu_bin: qemu_bin.into(),
        }
    }
}

impl Default for QemuVmm {
    fn default() -> Self {
        Self::new()
    }
}

pub struct QemuHandle {
    // Held for `kill_on_drop`; dropping this handle terminates the VM.
    #[allow(dead_code)]
    child: Child,
    cid: u32,
    port: u32,
}

impl VmHandle for QemuHandle {
    fn guest_cid(&self) -> u32 {
        self.cid
    }
    fn executor_port(&self) -> u32 {
        self.port
    }
}

#[async_trait]
impl Vmm for QemuVmm {
    async fn boot(&self, cfg: VmConfig) -> Result<Box<dyn VmHandle>> {
        which::which(&self.qemu_bin)
            .with_context(|| format!("`{}` not found on PATH", self.qemu_bin))?;

        let cmdline = build_cmdline(&cfg);
        let mut args: Vec<String> = vec![
            "-machine".into(),
            "q35,accel=kvm:tcg".into(),
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
            "-device".into(),
            format!("vhost-vsock-pci,guest-cid={}", cfg.guest_cid),
            // Reuse the host's network for simple egress; we'll tighten this
            // later when we wire host->guest workspace sharing via virtiofs.
            "-netdev".into(),
            "user,id=net0".into(),
            "-device".into(),
            "virtio-net-pci,netdev=net0".into(),
        ];

        if let Some(rootfs) = &cfg.rootfs {
            args.push("-drive".into());
            args.push(format!(
                "file={},if=virtio,format=raw",
                rootfs.display()
            ));
        }

        tracing::info!(
            cid = cfg.guest_cid,
            port = cfg.executor_port,
            kernel = %cfg.kernel.display(),
            "spawning qemu"
        );

        // Inherit stdout/stderr so kernel boot messages and the guest
        // executor's tracing output (which goes to the serial console via
        // `-nographic` + `console=ttyS0`) show up in our log. `Stdio::piped`
        // without a reader caused QEMU to block once buffers filled and made
        // every guest-side failure invisible.
        let child = Command::new(&self.qemu_bin)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("spawn {} {:?}", self.qemu_bin, args))?;

        Ok(Box::new(QemuHandle {
            child,
            cid: cfg.guest_cid,
            port: cfg.executor_port,
        }))
    }

    async fn shutdown(&self, handle: Box<dyn VmHandle>) -> Result<()> {
        // We can't safely downcast Box<dyn VmHandle> without RTTI; for the MVP,
        // shutdown is "drop the handle" which kills the QEMU child via
        // `kill_on_drop`. A future iteration will use the QMP socket for a
        // graceful shutdown.
        drop(handle);
        Ok(())
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

impl Drop for QemuHandle {
    fn drop(&mut self) {
        // tokio::process::Child with kill_on_drop already SIGKILLs the child;
        // nothing else to do.
    }
}
