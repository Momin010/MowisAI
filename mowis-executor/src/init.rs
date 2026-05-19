//! Init-mode helpers: mount the essential virtual filesystems before the
//! vsock server starts. Used when the executor runs as PID 1 inside an
//! initramfs (no other init system available).

use std::path::Path;

use nix::mount::{mount, MsFlags};

pub fn mount_essentials() -> anyhow::Result<()> {
    for (target, fstype, source) in [
        ("/proc", "proc", "proc"),
        ("/sys", "sysfs", "sysfs"),
        ("/dev", "devtmpfs", "devtmpfs"),
        ("/tmp", "tmpfs", "tmpfs"),
        ("/run", "tmpfs", "tmpfs"),
    ] {
        if !Path::new(target).exists() {
            if let Err(e) = std::fs::create_dir_all(target) {
                tracing::warn!(target, error = %e, "create mountpoint");
                continue;
            }
        }
        match mount(
            Some(source),
            target,
            Some(fstype),
            MsFlags::empty(),
            None::<&str>,
        ) {
            Ok(()) => tracing::info!(target, fstype, "mounted"),
            Err(e) => tracing::warn!(target, fstype, error = %e, "mount failed"),
        }
    }
    // /dev/pts is conventional and shells expect it.
    let _ = std::fs::create_dir_all("/dev/pts");
    let _ = mount(
        Some("devpts"),
        "/dev/pts",
        Some("devpts"),
        MsFlags::empty(),
        None::<&str>,
    );
    Ok(())
}
