//! Init-mode helpers: mount the essential virtual filesystems and load the
//! kernel modules the vsock server needs before opening a socket. Used when
//! the executor runs as PID 1 inside an initramfs (no other init system).

use std::ffi::CString;
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

/// Load the vsock kernel modules from `/lib/modules/<uname-r>/kernel/net/vmw_vsock/`.
///
/// The initramfs builder copies these `.ko` files in (decompressing if needed).
/// They have to be loaded in dependency order:
///   1. `vsock`                              — core protocol
///   2. `vmw_vsock_virtio_transport_common`  — shared virtio glue
///   3. `vmw_vsock_virtio_transport`         — guest-side transport, binds to
///                                             the virtio-vsock PCI device
///
/// Without these, the executor's `VsockListener::bind` fails because the
/// kernel has no transport registered for AF_VSOCK.
pub fn load_vsock_modules() {
    let release = match std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        Ok(s) => s.trim().to_string(),
        Err(e) => {
            tracing::warn!(error = %e, "read kernel release; skipping module load");
            return;
        }
    };
    let dir = format!("/lib/modules/{release}/kernel/net/vmw_vsock");

    for name in [
        "vsock",
        "vmw_vsock_virtio_transport_common",
        "vmw_vsock_virtio_transport",
    ] {
        let path = format!("{dir}/{name}.ko");
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(module = name, path, error = %e, "module file missing");
                continue;
            }
        };
        match init_module(&data, "") {
            Ok(()) => tracing::info!(module = name, bytes = data.len(), "loaded"),
            Err(e) if e.raw_os_error() == Some(libc::EEXIST) => {
                tracing::info!(module = name, "already loaded");
            }
            Err(e) => {
                tracing::warn!(module = name, error = %e, "init_module failed");
            }
        }
    }
}

fn init_module(image: &[u8], params: &str) -> std::io::Result<()> {
    let params_c = CString::new(params).unwrap();
    let r = unsafe {
        libc::syscall(
            libc::SYS_init_module,
            image.as_ptr() as *const libc::c_void,
            image.len() as libc::c_ulong,
            params_c.as_ptr(),
        )
    };
    if r < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
