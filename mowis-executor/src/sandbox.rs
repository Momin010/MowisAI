//! Sandbox primitives for the guest executor.
//!
//! Ported and simplified from `agentd/src/sandbox.rs`. The MVP supports:
//!   - empty tmpfs sandbox (no image)
//!   - overlayfs-backed sandbox when a rootfs path is provided
//!   - chroot + namespace isolation for `run_command`
//!   - cgroup v2 memory/cpu limits when available
//!
//! Tool dispatch (filesystem, git, etc.) is intentionally left out of this
//! file — see `tools.rs`. Container nesting (sandbox->container layers) is
//! also out of scope for the MVP and will be ported once the transport is
//! proven end-to-end.

use anyhow::{Context, Result};
use nix::mount::{mount, umount2, MntFlags, MsFlags};
use nix::sched;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::TempDir;

use mowis_protocol::ResourceLimits;

static SANDBOX_COUNTER: AtomicU64 = AtomicU64::new(1);

pub struct Sandbox {
    pub id: String,
    root: TempDir,
    overlay_dirs: Option<OverlayDirs>,
    limits: ResourceLimits,
}

struct OverlayDirs {
    upper: PathBuf,
    // Held so Drop can locate the parent dir for cleanup.
    #[allow(dead_code)]
    work: PathBuf,
}

impl Sandbox {
    /// Create a sandbox. If `image_rootfs` is `Some(path)`, mount an overlayfs
    /// with that path as the read-only lower layer. Otherwise use plain tmpfs.
    pub fn create(
        requested_id: Option<String>,
        image_rootfs: Option<&Path>,
        limits: ResourceLimits,
    ) -> Result<Self> {
        let id = requested_id.unwrap_or_else(|| {
            let n = SANDBOX_COUNTER.fetch_add(1, Ordering::SeqCst);
            format!("sb-{n}")
        });

        let root = tempfile::tempdir().context("create sandbox root")?;
        let overlay_dirs = match image_rootfs {
            Some(lower) if lower.exists() => {
                let base = std::env::temp_dir().join(format!("mowis-overlay-{}", id));
                let upper = base.join("upper");
                let work = base.join("work");
                std::fs::create_dir_all(&upper)?;
                std::fs::create_dir_all(&work)?;

                let opts = format!(
                    "lowerdir={},upperdir={},workdir={}",
                    lower.display(),
                    upper.display(),
                    work.display()
                );
                mount(
                    Some("overlay"),
                    root.path(),
                    Some("overlay"),
                    MsFlags::empty(),
                    Some(opts.as_str()),
                )
                .with_context(|| format!("mount overlayfs for sandbox {id}"))?;
                tracing::info!(sandbox = %id, lower = %lower.display(), "overlayfs mounted");
                Some(OverlayDirs { upper, work })
            }
            Some(missing) => {
                anyhow::bail!("image_rootfs does not exist: {}", missing.display());
            }
            None => {
                // Empty tmpfs — fine for trivial exec, but commands need a shell
                // present in some rootfs to actually run. The host should pass
                // an image_rootfs for non-trivial work.
                if let Err(e) = mount(
                    Some("tmpfs"),
                    root.path(),
                    Some("tmpfs"),
                    MsFlags::empty(),
                    None::<&str>,
                ) {
                    tracing::warn!(sandbox = %id, error = %e, "tmpfs mount failed (likely unprivileged)");
                }
                None
            }
        };

        let sb = Sandbox {
            id,
            root,
            overlay_dirs,
            limits,
        };
        sb.apply_cgroup_limits();
        Ok(sb)
    }

    pub fn root_path(&self) -> &Path {
        self.root.path()
    }

    fn apply_cgroup_limits(&self) {
        let cgroup_base = Path::new("/sys/fs/cgroup/mowis");
        if !cgroup_base.exists() && std::fs::create_dir_all(cgroup_base).is_err() {
            return;
        }
        let cg = cgroup_base.join(format!("sandbox-{}", self.id));
        if std::fs::create_dir_all(&cg).is_err() {
            return;
        }
        if let Some(ram) = self.limits.ram_bytes {
            let _ = std::fs::write(cg.join("memory.max"), ram.to_string());
        }
        if let Some(cpu_millis) = self.limits.cpu_millis {
            let quota = cpu_millis * 100;
            let _ = std::fs::write(cg.join("cpu.max"), format!("{} 100000", quota));
        }
    }

    /// Run a command inside the sandbox under chroot + new namespaces.
    /// Returns (exit_code, stdout, stderr).
    pub fn run_command(
        &self,
        cmd: &str,
        args: &[String],
        env: &[(String, String)],
    ) -> Result<(i32, Vec<u8>, Vec<u8>)> {
        let root_path = self.root.path().to_owned();
        let mut command = Command::new(cmd);
        command.args(args);
        command.env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        );
        for (k, v) in env {
            command.env(k, v);
        }

        unsafe {
            command.pre_exec(move || {
                sched::unshare(
                    sched::CloneFlags::CLONE_NEWNS
                        | sched::CloneFlags::CLONE_NEWPID
                        | sched::CloneFlags::CLONE_NEWUSER
                        | sched::CloneFlags::CLONE_NEWNET
                        | sched::CloneFlags::CLONE_NEWIPC
                        | sched::CloneFlags::CLONE_NEWUTS,
                )
                .map_err(std::io::Error::other)?;
                nix::unistd::chroot(&root_path).map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!("chroot failed: {e}"),
                    )
                })?;
                nix::unistd::chdir("/").map_err(std::io::Error::other)?;
                Ok(())
            });
        }

        let output = command
            .output()
            .with_context(|| format!("spawn `{cmd}` in sandbox {}", self.id))?;
        Ok((
            output.status.code().unwrap_or(-1),
            output.stdout,
            output.stderr,
        ))
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = umount2(self.root.path(), MntFlags::MNT_DETACH);
        if let Some(dirs) = &self.overlay_dirs {
            if let Some(base) = dirs.upper.parent() {
                let _ = std::fs::remove_dir_all(base);
            }
        }
        let cg = Path::new("/sys/fs/cgroup/mowis").join(format!("sandbox-{}", self.id));
        let _ = std::fs::remove_dir(cg);
    }
}
