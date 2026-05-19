//! Build a minimal initramfs (cpio.gz) that boots straight into
//! `mowis-executor` as PID 1.
//!
//! Shells out to `cpio` and `gzip` because they're standard on every Linux
//! host and the alternative — reimplementing the cpio "newc" archive format —
//! would add code for no practical benefit. A future iteration can swap in a
//! pure-Rust cpio writer (the `nodes-in-staging` directory layout is already
//! a stable interface).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tokio::process::Command;

/// Pack `executor_bin` into a bootable initramfs at `output`.
///
/// Layout inside the cpio:
///
/// ```text
///   /init           <- the executor binary (chmod 755)
///   /proc /sys /dev /tmp /run /dev/pts   <- empty mount points
/// ```
///
/// The executor self-detects PID 1 and mounts those at startup.
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&init_path, std::fs::Permissions::from_mode(0o755))?;
    }

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    tracing::info!(
        staging = %root.display(),
        output = %output.display(),
        "packing cpio.gz"
    );

    // `find . -print | cpio -o -H newc | gzip > out`
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
