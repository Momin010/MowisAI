use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};

/// Guest VM backend scaffold.
///
/// Today this starts a long-lived process inside the sandbox root filesystem via `chroot`.
/// This is *not* a hypervisor boot yet (no qemu/firecracker integration).
/// It exists so the socket server can route lifecycle by backend and we can replace the
/// internals with a real VM boot later without changing the protocol.
pub fn boot_guest_os_scaffold(root: &Path, image_hint: &str) -> Result<u32> {
    // Prefer a real init if the image provides it.
    let init_candidates = [
        "/sbin/init",
        "/bin/init",
        "/usr/sbin/init",
        "/etc/init.d/rcS",
    ];

    let mut chosen: Option<&str> = None;
    for c in init_candidates {
        if root.join(c.trim_start_matches('/')).exists() {
            chosen = Some(c);
            break;
        }
    }

    // Alpine usually doesn't ship init; run a minimal keepalive loop.
    let (cmd, args) = if let Some(c) = chosen {
        // Some init scripts expect to be PID1; we run it as the main process.
        (c.to_string(), vec![])
    } else {
        let _ = image_hint; // for future per-distro choices
        (
            "/bin/sh".to_string(),
            vec!["-lc".to_string(), "while true; do sleep 10; done".to_string()],
        )
    };

    let mut command = Command::new("chroot");
    command
        .arg(root)
        .arg(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let child = command.spawn().context("failed to spawn guest scaffold process")?;
    let pid = child.id();

    // Detach: we intentionally do not keep the `Child` handle.
    // On `destroy_sandbox`, the supervisor PID is killed.
    Ok(pid)
}

pub fn stop_guest_os(pid: u32) -> Result<()> {
    // Best-effort kill. We do not require SIGKILL; SIGTERM is enough for scaffold.
    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;
        kill(Pid::from_raw(pid as i32), Signal::SIGTERM)
            .map_err(|e| anyhow!("failed to stop guest pid {}: {}", pid, e))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Err(anyhow!("stop_guest_os scaffold requires Unix"))
    }
}

