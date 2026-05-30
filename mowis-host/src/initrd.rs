//! Build a minimal initramfs (cpio.gz) that boots straight into
//! `mowis-executor` as PID 1.
//!
//! Uses a pure-Rust cpio "newc" writer and flate2 — no external tools
//! required on any platform (Linux, macOS, Windows).

use std::collections::HashSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use flate2::{write::GzEncoder, Compression};

// ── Public entry point ────────────────────────────────────────────────────────

/// Pack `executor_bin` into a bootable initramfs at `output`.
///
/// Layout inside the cpio:
/// ```text
///   /init                   ← executor binary, chmod 755
///   /lib/... /lib64/...     ← glibc + dynamic linker (Linux host only)
///   /lib/modules/.../vsock  ← vsock kernel modules (Linux host only)
///   /bin/busybox            ← busybox if present on host
///   /bin/{sh,ls,...}        ← symlinks / copies to busybox
///   /proc /sys /dev /tmp /run /dev/pts  ← empty mount points
/// ```
pub async fn build(executor_bin: &Path, output: &Path) -> Result<()> {
    let executor_bin = executor_bin
        .canonicalize()
        .with_context(|| format!("resolve executor path {}", executor_bin.display()))?;

    let executor_data = std::fs::read(&executor_bin)
        .with_context(|| format!("read executor {}", executor_bin.display()))?;

    let libs = ldd_dependencies(&executor_bin).await?;
    if libs.is_empty() {
        tracing::info!("executor appears statically linked (or non-Linux host); no libs bundled");
    } else {
        tracing::info!(count = libs.len(), "bundling shared libraries");
    }

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let out_file =
        std::fs::File::create(output).with_context(|| format!("create {}", output.display()))?;
    let gz = GzEncoder::new(out_file, Compression::best());
    let mut cpio = CpioWriter::new(gz);

    // Standard mount-point directories.
    for dir in &[
        "proc", "sys", "dev", "dev/pts", "tmp", "run", "bin", "lib", "lib64",
    ] {
        cpio.add_dir(dir)?;
    }

    // /init = the executor binary (PID 1).
    cpio.add_file("init", &executor_data, 0o755)?;

    // Shared libraries (discovered on Linux host; empty otherwise).
    for lib in &libs {
        let rel = lib.strip_prefix("/").unwrap_or(lib);
        // Ensure every parent directory appears before the file.
        let mut ancestor = PathBuf::new();
        for component in rel.components().map(|c| c.as_os_str()) {
            ancestor.push(component);
            if ancestor != rel {
                cpio.add_dir_if_new(&ancestor.display().to_string())?;
            }
        }
        let data = std::fs::read(lib)
            .with_context(|| format!("read lib {}", lib.display()))?;
        cpio.add_file(&rel.display().to_string(), &data, 0o755)?;
    }

    // Busybox + applet symlinks / copies.
    bundle_busybox(&mut cpio)?;

    // vsock kernel modules — Linux host only (the modules live in /lib/modules).
    bundle_vsock_modules(&mut cpio).await?;

    cpio.finish()?;

    tracing::info!(output = %output.display(), "initramfs written");
    Ok(())
}

// ── Pure-Rust cpio "newc" writer ──────────────────────────────────────────────

struct CpioWriter<W: Write> {
    inner: W,
    ino: u32,
    dirs: HashSet<String>,
}

impl<W: Write> CpioWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            ino: 1,
            dirs: HashSet::new(),
        }
    }

    /// Add a directory (idempotent within this writer).
    fn add_dir(&mut self, name: &str) -> io::Result<()> {
        if self.dirs.contains(name) {
            return Ok(());
        }
        self.dirs.insert(name.to_string());
        self.write_entry(0o040_755, 2, name, b"")
    }

    /// Add a directory only if it hasn't been added yet.
    fn add_dir_if_new(&mut self, name: &str) -> io::Result<()> {
        self.add_dir(name)
    }

    /// Add a regular file.
    fn add_file(&mut self, name: &str, data: &[u8], perms: u32) -> io::Result<()> {
        let mode = 0o100_000 | (perms & 0o777);
        self.write_entry(mode, 1, name, data)
    }

    /// Add a symbolic link (target is the link destination, not terminated).
    fn add_symlink(&mut self, name: &str, target: &str) -> io::Result<()> {
        // Mode 0120777 = symlink
        self.write_entry(0o120_777, 1, name, target.as_bytes())
    }

    /// Write the TRAILER!!! entry and flush.
    fn finish(mut self) -> io::Result<()> {
        self.write_entry(0, 1, "TRAILER!!!", b"")?;
        self.inner.flush()
    }

    /// Low-level: write a single cpio "newc" record.
    ///
    /// Header layout (110 bytes):
    ///   magic(6) ino(8) mode(8) uid(8) gid(8) nlink(8) mtime(8)
    ///   filesize(8) devmaj(8) devmin(8) rdevmaj(8) rdevmin(8) namesize(8) check(8)
    fn write_entry(&mut self, mode: u32, nlink: u32, name: &str, data: &[u8]) -> io::Result<()> {
        let ino = self.ino;
        self.ino += 1;

        let namesize = name.len() + 1; // include null terminator
        let filesize = data.len();

        // 110-byte fixed header (all numeric fields are zero-padded 8-char hex).
        let header = format!(
            "070701{ino:08X}{mode:08X}0000000000000000{nlink:08X}00000000\
             {filesize:08X}000000000000000000000000000000000{namesize:08X}00000000",
        );
        debug_assert_eq!(header.len(), 110, "cpio header must be exactly 110 bytes");
        self.inner.write_all(header.as_bytes())?;

        // Name + NUL byte.
        self.inner.write_all(name.as_bytes())?;
        self.inner.write_all(b"\0")?;

        // Pad (header + name) to 4-byte boundary.
        let after_name = 110 + namesize;
        let name_pad = (4 - after_name % 4) % 4;
        self.inner.write_all(&[0u8; 4][..name_pad])?;

        // File data.
        if !data.is_empty() {
            self.inner.write_all(data)?;
            let data_pad = (4 - filesize % 4) % 4;
            self.inner.write_all(&[0u8; 4][..data_pad])?;
        }

        Ok(())
    }
}

// ── Busybox bundling (cross-platform) ─────────────────────────────────────────

/// Copy a static `busybox` binary into the initramfs and add applet entries.
///
/// On all platforms we look for a Linux busybox binary in standard locations.
/// If not found, we emit a warning and continue — the executor binary is still
/// PID 1 and handles its own tool calls.
///
/// Applet entries are written as symlinks (cpio symlinks are platform-neutral;
/// we never touch the host filesystem here).
fn bundle_busybox<W: Write>(cpio: &mut CpioWriter<W>) -> Result<()> {
    let candidates: &[&str] = &["/bin/busybox", "/usr/bin/busybox"];
    let src = candidates.iter().copied().map(Path::new).find(|p| p.exists());

    let Some(src) = src else {
        tracing::info!(
            "busybox not found on host; guest userspace will rely on executor only"
        );
        return Ok(());
    };

    let data = std::fs::read(src)
        .with_context(|| format!("read busybox from {}", src.display()))?;
    cpio.add_file("bin/busybox", &data, 0o755)?;

    let applets = [
        "sh", "ash", "echo", "ls", "cat", "mkdir", "rm", "rmdir", "mv", "cp",
        "ln", "mount", "umount", "ps", "kill", "ip", "true", "false", "sleep",
        "env", "id", "uname", "head", "tail", "grep", "find", "touch", "stat",
        "df", "free",
    ];
    for applet in applets {
        // cpio symlinks are always portable — no host filesystem operations.
        cpio.add_symlink(&format!("bin/{applet}"), "busybox")?;
    }

    tracing::info!(
        src = %src.display(),
        applets = applets.len(),
        "bundled busybox"
    );
    Ok(())
}

// ── Shared library discovery ──────────────────────────────────────────────────

/// Discover shared library dependencies of `bin` using platform-native tools.
///
/// | Platform | Tool     | Notes                                          |
/// |----------|----------|------------------------------------------------|
/// | Linux    | `ldd`    | Standard; parses `name => /path (0xaddr)`.    |
/// | macOS    | `otool`  | Returns macOS dylib paths (cross-build: empty). |
/// | Windows  | (none)   | Always returns empty; assume static musl.      |
///
/// When building an initrd on macOS or Windows, the executor binary is a
/// Linux ELF binary that cannot be introspected by host tools. In practice,
/// the executor should be compiled as a static musl binary for distribution.
async fn ldd_dependencies(bin: &Path) -> Result<Vec<PathBuf>> {
    #[cfg(target_os = "linux")]
    return ldd_linux(bin).await;

    #[cfg(target_os = "macos")]
    return otool_macos(bin).await;

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = bin;
        tracing::info!(
            "Windows host: assuming statically-linked executor; no shared libs bundled"
        );
        return Ok(Vec::new());
    }
}

#[cfg(target_os = "linux")]
async fn ldd_linux(bin: &Path) -> Result<Vec<PathBuf>> {
    use std::collections::BTreeSet;
    use tokio::process::Command;

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
        let line = line.trim();
        if line.is_empty() || line.starts_with("linux-vdso") {
            continue;
        }
        let path = if let Some(idx) = line.find("=> ") {
            let rest = &line[idx + 3..];
            rest.split_whitespace().next().unwrap_or("")
        } else if line.starts_with('/') {
            line.split_whitespace().next().unwrap_or("")
        } else {
            continue
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

#[cfg(target_os = "macos")]
async fn otool_macos(bin: &Path) -> Result<Vec<PathBuf>> {
    use tokio::process::Command;
    use std::collections::BTreeSet;

    // `otool -L` lists macOS dylib dependencies. If the binary is a Linux
    // ELF (cross-build scenario), otool will fail — treat that as static.
    let output = match Command::new("otool").arg("-L").arg(bin).output().await {
        Ok(o) if o.status.success() => o,
        _ => {
            tracing::info!(
                "otool could not inspect executor (likely a Linux ELF on macOS host); \
                 assuming static binary"
            );
            return Ok(Vec::new());
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut paths = Vec::new();
    for line in stdout.lines().skip(1) {
        // Format: "\t/path/to/lib (compatibility version X.Y.Z, ...)"
        let line = line.trim();
        if let Some(path_part) = line.split_whitespace().next() {
            let p = PathBuf::from(path_part);
            if p.is_absolute() && p.exists() {
                paths.push(p);
            }
        }
    }
    Ok(paths)
}

// ── vsock kernel module bundling (Linux host only) ────────────────────────────

/// Bundle vsock kernel modules into the initramfs so the guest can load them.
///
/// On macOS and Windows, vsock modules are not present on the host (they live
/// in the Linux kernel's `/lib/modules`). The guest VM should be configured
/// with vsock support compiled into the kernel (`CONFIG_VHOST_VSOCK=y`) or
/// the QEMU TCP forwarding transport is used instead.
async fn bundle_vsock_modules<W: Write>(cpio: &mut CpioWriter<W>) -> Result<()> {
    #[cfg(target_os = "linux")]
    return bundle_vsock_modules_linux(cpio).await;

    #[cfg(not(target_os = "linux"))]
    {
        let _ = cpio;
        tracing::info!(
            "non-Linux host: vsock kernel modules not bundled; \
             ensure guest kernel has vsock built-in (CONFIG_VHOST_VSOCK=y) \
             or use TCP transport"
        );
        return Ok(());
    }
}

#[cfg(target_os = "linux")]
async fn bundle_vsock_modules_linux<W: Write>(cpio: &mut CpioWriter<W>) -> Result<()> {
    use tokio::process::Command;

    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .context("read /proc/sys/kernel/osrelease")?;
    let release = release.trim();

    let src_dir =
        PathBuf::from(format!("/lib/modules/{release}/kernel/net/vmw_vsock"));
    if !src_dir.exists() {
        tracing::warn!(
            path = %src_dir.display(),
            "vsock module directory missing; guest will fail to bind AF_VSOCK"
        );
        return Ok(());
    }

    let dest_prefix = format!("lib/modules/{release}/kernel/net/vmw_vsock");
    // Ensure ancestor directories appear in the cpio.
    for dir in [
        "lib/modules",
        &format!("lib/modules/{release}"),
        &format!("lib/modules/{release}/kernel"),
        &format!("lib/modules/{release}/kernel/net"),
        &dest_prefix,
    ] {
        cpio.add_dir_if_new(dir)?;
    }

    let modules = [
        "vsock",
        "vmw_vsock_virtio_transport_common",
        "vmw_vsock_virtio_transport",
    ];
    let mut bundled = 0usize;

    for name in modules {
        let dest_name = format!("{dest_prefix}/{name}.ko");
        let mut found = false;

        for ext in ["ko", "ko.gz", "ko.zst", "ko.xz"] {
            let candidate = src_dir.join(format!("{name}.{ext}"));
            if !candidate.exists() {
                continue;
            }
            let data = decompress_module(&candidate, ext).await.with_context(|| {
                format!("decompress {} ({})", candidate.display(), ext)
            })?;
            cpio.add_file(&dest_name, &data, 0o644)?;
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

#[cfg(target_os = "linux")]
async fn decompress_module(src: &Path, ext: &str) -> Result<Vec<u8>> {
    use std::io::Read;
    use tokio::process::Command;

    match ext {
        "ko" => Ok(std::fs::read(src)?),
        "ko.gz" => {
            let f = std::fs::File::open(src)?;
            let mut decoder = flate2::read::GzDecoder::new(f);
            let mut buf = Vec::new();
            decoder.read_to_end(&mut buf)?;
            Ok(buf)
        }
        "ko.zst" => {
            which::which("zstd")
                .context("`zstd` not found on PATH (needed to decompress .ko.zst)")?;
            let out = Command::new("zstd")
                .args(["-d", "-c"])
                .arg(src)
                .output()
                .await
                .context("spawn zstd")?;
            if !out.status.success() {
                anyhow::bail!("zstd -d failed: exit {}", out.status);
            }
            Ok(out.stdout)
        }
        "ko.xz" => {
            which::which("xz")
                .context("`xz` not found on PATH (needed to decompress .ko.xz)")?;
            let out = Command::new("xz")
                .args(["-d", "-c"])
                .arg(src)
                .output()
                .await
                .context("spawn xz")?;
            if !out.status.success() {
                anyhow::bail!("xz -d failed: exit {}", out.status);
            }
            Ok(out.stdout)
        }
        other => anyhow::bail!("unknown module extension `{other}`"),
    }
}

/// Best-effort: find the running host's kernel image.
pub fn default_kernel() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
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
    }
    None
}
