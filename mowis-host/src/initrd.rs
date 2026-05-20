//! Build a minimal initramfs (cpio.gz) that boots straight into
//! `mowis-executor` as PID 1.
//!
//! Shells out to `cpio` and `gzip` because they're standard on every Linux
//! host and the alternative — reimplementing the cpio "newc" archive format —
//! would add code for no practical benefit. A future iteration can swap in a
//! pure-Rust cpio writer (the `nodes-in-staging` directory layout is already
//! a stable interface).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tokio::process::Command;

/// Pack `executor_bin` into a bootable initramfs at `output`.
///
/// Layout inside the cpio:
///
/// ```text
///   /init           <- the executor binary (chmod 755)
///   /lib/...        <- glibc + ldd-discovered shared libraries
///   /lib64/...      <- dynamic linker (ld-linux-*.so.2)
///   /proc /sys /dev /tmp /run /dev/pts   <- empty mount points
/// ```
///
/// The executor self-detects PID 1 and mounts those at startup. Dynamic
/// libraries are auto-bundled by running `ldd` on the executor binary.
pub async fn build(executor_bin: &Path, output: &Path) -> Result<()> {
    which::which("cpio").context("`cpio` not found on PATH")?;
    which::which("gzip").context("`gzip` not found on PATH")?;

    let executor_bin = executor_bin
        .canonicalize()
        .with_context(|| format!("resolve executor path {}", executor_bin.display()))?;

    let staging = tempfile::tempdir().context("create staging dir")?;
    let root = staging.path();

    // Mount points the executor's init mode will populate.
    for d in ["proc", "sys", "dev", "dev/pts", "tmp", "run"] {
        std::fs::create_dir_all(root.join(d))?;
    }

    // /init = the executor binary.
    let init_path = root.join("init");
    std::fs::copy(&executor_bin, &init_path).context("copy executor into staging")?;
    chmod_exec(&init_path)?;

    // Bundle shared libraries discovered by ldd. Glibc-linked binaries need
    // the dynamic linker (/lib64/ld-linux-x86-64.so.2) plus libc.so.6 and
    // friends to start at all. Without these, the kernel prints
    // "Failed to execute /init (error -2)" and panics.
    let libs = ldd_dependencies(&executor_bin).await?;
    if libs.is_empty() {
        tracing::info!("executor appears to be statically linked; no libs bundled");
    } else {
        tracing::info!(count = libs.len(), "bundling shared libraries");
        for lib in &libs {
            let rel = lib.strip_prefix("/").unwrap_or(lib);
            let dest = root.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(lib, &dest)
                .with_context(|| format!("copy lib {} -> {}", lib.display(), dest.display()))?;
            chmod_exec(&dest)?;
        }
    }

    // Bundle vsock kernel modules so the guest can register an AF_VSOCK
    // transport. Without these the executor's bind fails with "No such device"
    // because the host kernel has vsock built as a module and the initramfs
    // has no /lib/modules to modprobe from.
    bundle_vsock_modules(root).await?;

    // Bundle busybox + symlinks so the guest has a usable userspace. Without
    // this, `mowisd exec -- /bin/ls /` fails because the initramfs is just
    // /init, libs, and vsock modules — no /bin, no /usr.
    bundle_busybox(root)?;

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    tracing::info!(
        staging = %root.display(),
        output = %output.display(),
        "packing cpio.gz"
    );

    let pipeline = format!(
        "cd {staging} && find . -print | cpio -o -H newc 2>/dev/null | gzip -9 > {output}",
        staging = shell_quote(root),
        output = shell_quote(output),
    );
    let status = Command::new("sh")
        .arg("-c")
        .arg(&pipeline)
        .status()
        .await
        .context("spawn cpio|gzip")?;
    if !status.success() {
        anyhow::bail!("cpio pipeline failed: exit {status}");
    }
    Ok(())
}

/// Run `ldd <bin>` and return the absolute paths of every shared library it
/// reports. Returns an empty Vec when the binary is statically linked (ldd
/// prints "not a dynamic executable") or when ldd isn't available.
async fn ldd_dependencies(bin: &Path) -> Result<Vec<PathBuf>> {
    let ldd = match which::which("ldd") {
        Ok(p) => p,
        Err(_) => {
            tracing::warn!("ldd not found; assuming statically linked executor");
            return Ok(Vec::new());
        }
    };
    let output = Command::new(&ldd)
        .arg(bin)
        .output()
        .await
        .with_context(|| format!("spawn {}", ldd.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not a dynamic executable") {
            return Ok(Vec::new());
        }
        anyhow::bail!("ldd failed: {stderr}");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut paths: BTreeSet<PathBuf> = BTreeSet::new();
    for line in stdout.lines() {
        // Lines come in three shapes:
        //   "\tlibc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0xaddr)"
        //   "\t/lib64/ld-linux-x86-64.so.2 (0xaddr)"
        //   "\tlinux-vdso.so.1 (0xaddr)"   <- skip (kernel-provided, no file)
        let line = line.trim();
        if line.is_empty() || line.starts_with("linux-vdso") {
            continue;
        }
        let path = if let Some(idx) = line.find("=> ") {
            // "name => /path (0xaddr)" -> "/path"
            let rest = &line[idx + 3..];
            rest.split_whitespace().next().unwrap_or("")
        } else if line.starts_with('/') {
            // "/lib64/ld-linux-... (0xaddr)" -> "/lib64/ld-linux-..."
            line.split_whitespace().next().unwrap_or("")
        } else {
            continue;
        };
        if path.is_empty() || path == "(0x" || path == "not" {
            continue;
        }
        let p = PathBuf::from(path);
        if p.is_absolute() && p.exists() {
            paths.insert(p);
        }
    }
    Ok(paths.into_iter().collect())
}

/// Copy the vsock kernel modules into the initramfs at the path the executor
/// looks for them. Handles plain `.ko` and compressed `.ko.zst` / `.ko.gz` /
/// `.ko.xz` by shelling out to the matching decompressor (already installed
/// alongside cpio on every distro we care about).
async fn bundle_vsock_modules(staging_root: &Path) -> Result<()> {
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .context("read /proc/sys/kernel/osrelease")?;
    let release = release.trim();
    let src_dir = PathBuf::from(format!(
        "/lib/modules/{release}/kernel/net/vmw_vsock"
    ));
    if !src_dir.exists() {
        tracing::warn!(
            path = %src_dir.display(),
            "vsock module directory missing; guest will fail to bind AF_VSOCK"
        );
        return Ok(());
    }
    let dest_dir = staging_root.join(format!(
        "lib/modules/{release}/kernel/net/vmw_vsock"
    ));
    std::fs::create_dir_all(&dest_dir)?;

    let modules = [
        "vsock",
        "vmw_vsock_virtio_transport_common",
        "vmw_vsock_virtio_transport",
    ];
    let mut bundled = 0;
    for name in modules {
        let dest = dest_dir.join(format!("{name}.ko"));
        let mut found = false;
        for ext in ["ko", "ko.zst", "ko.gz", "ko.xz"] {
            let candidate = src_dir.join(format!("{name}.{ext}"));
            if !candidate.exists() {
                continue;
            }
            decompress_to(&candidate, &dest, ext).await.with_context(|| {
                format!("decompress {} -> {}", candidate.display(), dest.display())
            })?;
            tracing::info!(module = name, src = %candidate.display(), "bundled");
            bundled += 1;
            found = true;
            break;
        }
        if !found {
            tracing::warn!(module = name, dir = %src_dir.display(), "module not found");
        }
    }
    if bundled == 0 {
        tracing::warn!("no vsock modules bundled; guest will not have AF_VSOCK");
    }
    Ok(())
}

async fn decompress_to(src: &Path, dest: &Path, ext: &str) -> Result<()> {
    let cmd = match ext {
        "ko" => {
            std::fs::copy(src, dest)?;
            return Ok(());
        }
        "ko.zst" => "zstd",
        "ko.gz" => "gzip",
        "ko.xz" => "xz",
        other => anyhow::bail!("unknown module extension `{other}`"),
    };
    which::which(cmd)
        .with_context(|| format!("`{cmd}` not found on PATH (needed to decompress {ext})"))?;
    let status = Command::new(cmd)
        .arg("-d")
        .arg("-c")
        .arg(src)
        .stdout(std::process::Stdio::from(std::fs::File::create(dest)?))
        .status()
        .await
        .with_context(|| format!("spawn {cmd}"))?;
    if !status.success() {
        anyhow::bail!("{cmd} -d failed: exit {status}");
    }
    Ok(())
}

/// Copy a static `busybox` binary into the initramfs and symlink common
/// command names to it. Lets the guest run `/bin/ls`, `/bin/echo`, `/bin/sh`
/// etc. without bundling all of coreutils.
fn bundle_busybox(staging_root: &Path) -> Result<()> {
    let candidates = ["/bin/busybox", "/usr/bin/busybox"];
    let src = candidates.iter().map(Path::new).find(|p| p.exists());
    let Some(src) = src else {
        tracing::warn!("busybox not found on PATH; guest will have no userspace tools");
        return Ok(());
    };

    let bin_dir = staging_root.join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let dest = bin_dir.join("busybox");
    std::fs::copy(src, &dest)
        .with_context(|| format!("copy busybox {} -> {}", src.display(), dest.display()))?;
    chmod_exec(&dest)?;

    let applets = [
        "sh", "ash", "echo", "ls", "cat", "mkdir", "rm", "rmdir", "mv", "cp",
        "ln", "mount", "umount", "ps", "kill", "ip", "ifconfig", "true",
        "false", "sleep", "env", "id", "uname", "head", "tail", "grep",
        "find", "touch", "stat", "df", "free",
    ];
    for name in applets {
        let link = bin_dir.join(name);
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink("busybox", &link).with_context(|| {
            format!("symlink {} -> busybox", link.display())
        })?;
    }
    tracing::info!(src = %src.display(), applets = applets.len(), "bundled busybox");
    Ok(())
}

#[cfg(unix)]
fn chmod_exec(p: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn shell_quote(p: impl AsRef<Path>) -> String {
    let s = p.as_ref().display().to_string();
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Best-effort: find the running host's kernel image. Useful default for
/// `mowisd boot --kernel`.
pub fn default_kernel() -> Option<PathBuf> {
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok()?;
    let release = release.trim();
    let candidate = PathBuf::from(format!("/boot/vmlinuz-{release}"));
    if candidate.exists() {
        return Some(candidate);
    }
    let fallback = PathBuf::from("/boot/vmlinuz");
    if fallback.exists() {
        return Some(fallback);
    }
    None
}
