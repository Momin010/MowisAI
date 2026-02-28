use serde::{Deserialize, Serialize};
use lazy_static::lazy_static;

/// Resource limits allocated to a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub ram_bytes: Option<u64>,
    pub cpu_millis: Option<u64>,
    // GPU support will be added later
}

use std::sync::atomic::{AtomicU64, Ordering};
use anyhow::{Context, Result};
use tempfile::TempDir;
use fastrand;
use nix::sched;
use nix::mount::{mount, MsFlags, umount2, MntFlags};
use nix::sys::resource::{setrlimit, Resource};
use std::process::Command;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::fs;

// global counter used to keep IDs unique and monotonically increasing. we
// mix a few random low bits so that the sequence isn't trivially guessable.
lazy_static! {
    static ref SANDBOX_COUNTER: AtomicU64 = AtomicU64::new(init_counter());
}

/// initialize the atomic counter with a time-derived seed so that early IDs
/// vary between process invocations. we use seconds since epoch shifted left
/// so the high bits encode time and the low bits will later be filled with a
/// small random component during id generation.
fn init_counter() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // put the timestamp in the high 48 bits, leave room for random low bits
    now << 16
}

/// A handle representing an isolated sandbox environment.
use std::collections::HashMap;

/// recursively copy all files and directories from src to dest
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    if !src.is_dir() {
        return Err(anyhow::anyhow!("source is not a directory: {}", src.display()));
    }
    
    for entry in fs::read_dir(src).context("read image dir")? {
        let entry = entry.context("read dir entry")?;
        let src_path = entry.path();
        let file_name = entry.file_name();
        let dest_path = dest.join(&file_name);
        
        if entry.file_type().context("get file type")?.is_dir() {
            fs::create_dir(&dest_path).ok(); // ignore if exists
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            // copy file, preserving permissions
            fs::copy(&src_path, &dest_path).context("copy file")?;
        }
    }
    Ok(())
}


#[derive(Serialize)]
pub struct Sandbox {
    id: u64,
    limits: ResourceLimits,
    // path to the root directory used for chroot/mount namespace
    #[serde(skip)]
    root: TempDir,
    // persisted bucket directory for this sandbox
    #[serde(skip)]
    bucket_dir: PathBuf,
    #[serde(skip)]
    tools: HashMap<String, Box<dyn crate::tools::Tool>>,
    #[serde(skip)]
    policy: Option<crate::security::SecurityPolicy>,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // unmount the root path (overlayfs or tmpfs)
        let _ = umount2(self.root.path(), MntFlags::MNT_DETACH);
        // clean up overlayfs directories if they exist
        let overlay_base = std::env::temp_dir().join(format!("overlay-{}", self.id));
        let _ = std::fs::remove_dir_all(&overlay_base);
    }
}

impl Sandbox {
    /// create a new sandbox with the given limits and optional image.
    /// if image is specified, it will be mounted into the sandbox root.
    /// otherwise an empty tmpfs is used.
    pub fn new(limits: ResourceLimits) -> Result<Self> {
        Self::new_with_image(limits, None)
    }

    /// create a new sandbox with limits and a specific image reference.
    pub fn new_with_image(limits: ResourceLimits, image: Option<&str>) -> Result<Self> {
        // bump the counter and then mix in a handful of random bits in the
        // low positions. this ensures ids are unique and increasing but not
        // trivially predictable (e.g. 1,2,3...). using 16 random bits gives
        // 65k possibilities per counter step, ample for our purposes.
        const RANDOM_BITS: u64 = 16;
        const RANDOM_MASK: u64 = (1 << RANDOM_BITS) - 1;
        let base = SANDBOX_COUNTER.fetch_add(1, Ordering::SeqCst);
        let rand = fastrand::u64(..(RANDOM_MASK + 1));
        let id = (base << RANDOM_BITS) | (rand & RANDOM_MASK);

        // create temporary directory to act as root for this sandbox
        let root = tempfile::tempdir().context("failed to create sandbox root")?;

        // mount image via overlayfs (no copying = no OOM)
        // lower = read-only image rootfs, upper = small tmpfs for writes
        if let Some(img_ref) = image {
            let image_mgr = crate::ImageManager::new(None)?;
            let img_path = image_mgr.resolve(img_ref)
                .map_err(|e| anyhow::anyhow!("failed to resolve image {}: {}", img_ref, e))?;

            let overlay_base = std::env::temp_dir().join(format!("overlay-{}", id));
            let upper = overlay_base.join("upper");
            let work  = overlay_base.join("work");
            fs::create_dir_all(&upper).context("create overlay upper")?;
            fs::create_dir_all(&work).context("create overlay work")?;

            let opts = format!(
                "lowerdir={},upperdir={},workdir={}",
                img_path.display(), upper.display(), work.display()
            );
            mount(
                Some("overlay"),
                root.path(),
                Some("overlay"),
                MsFlags::empty(),
                Some(opts.as_str()),
            ).map_err(|e| anyhow::anyhow!(
                "overlayfs mount failed for {}: {} (must run as root)", img_ref, e
            ))?;
            log::info!("mounted {} via overlayfs into sandbox {}", img_ref, id);
        } else {
            // no image: plain empty tmpfs
            if let Err(e) = mount(
                Some("tmpfs"),
                root.path(),
                Some("tmpfs"),
                MsFlags::empty(),
                None::<&str>,
            ) {
                log::warn!("unable to mount tmpfs for sandbox {}: {}", id, e);
            }
        }
        
        let bucket_dir = root.path().join("buckets");
        fs::create_dir_all(&bucket_dir)?;
        let mut sb = Sandbox { id, limits, root, bucket_dir, tools: HashMap::new(), policy: None };
        // apply resource limits via cgroups if possible
        sb.apply_limits()?;
        Ok(sb)
    }

    /// restore a sandbox from persisted metadata, keeping the supplied id.
    /// This will also advance the global counter to avoid collisions.
    pub fn restore(id: u64, limits: ResourceLimits) -> Result<Self> {
        // ensure counter is ahead of any restored identifier. when we encoded
        // the id we shifted the original counter left RANDOM_BITS, so we need
        // to account for that when bumping the global counter. this also
        // prevents a restored sandbox from accidentally reusing a future id.
        const RANDOM_BITS: u64 = 16;
        let prev = id >> RANDOM_BITS; // drop the random low bits
        SANDBOX_COUNTER.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |old| {
            if old <= prev { Some(prev + 1) } else { Some(old) }
        }).ok();
        // create directory similar to new
        let root = tempfile::tempdir().context("failed to create sandbox root")?;
        if let Err(e) = mount(
            Some("tmpfs"),
            root.path(),
            Some("tmpfs"),
            MsFlags::empty(),
            None::<&str>,
        ) {
            log::warn!("unable to mount tmpfs for sandbox {}: {}", id, e);
        }
        let bucket_dir = root.path().join("buckets");
        fs::create_dir_all(&bucket_dir)?;
        let mut sb = Sandbox { id, limits, root, bucket_dir, tools: HashMap::new(), policy: None };
        sb.apply_limits()?;
        Ok(sb)
    }

    /// spawn a child sandbox with a subset of this sandbox's resources.
    pub fn spawn_child(&self, limits: ResourceLimits) -> Result<Sandbox> {
        // enforce inheritance rules: child may not exceed parent limits
        let clamp = |parent: Option<u64>, child: Option<u64>| match (parent, child) {
            (Some(p), Some(c)) => Some(std::cmp::min(p, c)),
            (Some(p), None) => Some(p),
            (None, c) => c,
        };
        let child_limits = ResourceLimits {
            ram_bytes: clamp(self.limits.ram_bytes, limits.ram_bytes),
            cpu_millis: clamp(self.limits.cpu_millis, limits.cpu_millis),
        };
        let mut child = Sandbox::new(child_limits)?;
        // inherit tool registrations by cloning references (tools are sync + send)
        for (name, tool) in &self.tools {
            // note: ability to clone trait object may require tool.clone_box; skip for now
            // for simplicity we keep same reference pointer (shared tools)
            child.tools.insert(name.clone(), tool.clone_box());
        }
        Ok(child)
    }

    /// retrieve the sandbox identifier
    pub fn id(&self) -> u64 {
        self.id
    }

    /// inspect the configured limits (read-only reference)
    pub fn limits(&self) -> &ResourceLimits {
        &self.limits
    }

    /// path to the root directory backing the sandbox.
    pub fn root_path(&self) -> &std::path::Path {
        self.root.path()
    }

    /// write resource limits into cgroup v2 if available and running as root.
    fn apply_limits(&mut self) -> Result<()> {
        let cgroup_base = Path::new("/sys/fs/cgroup/agentd");
        if !cgroup_base.exists() {
            if let Err(e) = fs::create_dir_all(cgroup_base) {
                log::warn!("unable to create cgroup base {}: {}", cgroup_base.display(), e);
                return Ok(());
            }
        }
        let cg = cgroup_base.join(format!("sandbox-{}", self.id));
        if let Err(e) = fs::create_dir_all(&cg) {
            log::warn!("cannot create cgroup {:?}: {}", cg, e);
            return Ok(());
        }
        // set memory.max if specified
        if let Some(ram) = self.limits.ram_bytes {
            let path = cg.join("memory.max");
            if let Err(e) = fs::write(&path, ram.to_string()) {
                log::warn!("failed to set memory.max: {}", e);
            }
        }
        // set cpu.max in microseconds per period
        if let Some(cpu_millis) = self.limits.cpu_millis {
            let path = cg.join("cpu.max");
            // convert millis to quota/period, simple static period of 100000
            let quota = cpu_millis * 100;
            let content = format!("{} 100000", quota);
            if let Err(e) = fs::write(&path, content) {
                log::warn!("failed to set cpu.max: {}", e);
            }
        }
        Ok(())
    }

    /// register a tool with the sandbox. the tool will be available for invocation.
    pub fn register_tool(&mut self, tool: Box<dyn crate::tools::Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// attempt to invoke a named tool with given JSON input.
    pub fn invoke_tool(&self, name: &str, input: serde_json::Value) -> Result<serde_json::Value> {
        if let Some(policy) = &self.policy {
            match name {
                "read_file" | "get_file_info" => {
                    let path = input["path"].as_str().unwrap_or("");
                    if !policy.check_file_access(path, crate::security::FileAccessType::Read) {
                        return Err(anyhow::anyhow!("security policy denied read access to {}", path));
                    }
                }
                "write_file" | "delete_file" | "create_directory" | "copy_file" => {
                    let path = input["path"].as_str()
                        .or_else(|| input["dst"].as_str())
                        .unwrap_or("");
                    if !policy.check_file_access(path, crate::security::FileAccessType::Write) {
                        return Err(anyhow::anyhow!("security policy denied write access to {}", path));
                    }
                }
                "http_get" | "http_post" => {
                    if !policy.check_network_access(true) {
                        return Err(anyhow::anyhow!("security policy denied outbound network access"));
                    }
                }
                _ => {}
            }
        }
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("tool not registered: {}", name))?;
        let ctx = crate::tools::ToolContext {
            sandbox_id: self.id,
        };
        let output = tool.invoke(&ctx, input)?;
        Ok(output)
    }

    pub fn set_policy(&mut self, policy: crate::security::SecurityPolicy) {
        self.policy = Some(policy);
    }

    pub fn policy(&self) -> Option<&crate::security::SecurityPolicy> {
        self.policy.as_ref()
    }

    /// run a command inside the sandbox, returning stdout output or an error.
    /// this function uses `unshare` + `chroot` to isolate the process.
    pub fn run_command(&self, cmd: &str) -> Result<String> {
        let root_path = self.root.path().to_owned();
        let sid = self.id; // copy id so closure doesn't borrow self
        
        // npm requires thread creation which fails in strict namespaces.
        // detect npm commands and run them with chroot only (no namespace isolation).
        let is_npm = cmd.starts_with("npm") || cmd.contains(" npm ");
        
        // extract policy limits for the closure
        let policy_limits = self.policy.as_ref()
            .map(|p| p.resource_limits.clone())
            .unwrap_or(crate::security::ResourceSecurityLimits {
                max_memory_mb: None,
                max_cpu_percent: None,
                max_open_files: None,
                max_processes: None,
            });
        
        let output_attempt = unsafe {
            Command::new("/bin/sh")
                .arg("-c")
                .arg(cmd)
                .env("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
                .pre_exec(move || {
                    // skip namespace isolation for npm commands
                    if !is_npm {
                        // unshare mount and pid namespaces to isolate. if we're not
                        // running as root (common during CI/tests) the call will fail
                        // with EPERM; that's harmless, so log a warning and continue
                        // without isolation rather than returning an error.
                        if let Err(e) = sched::unshare(
                            sched::CloneFlags::CLONE_NEWNS | sched::CloneFlags::CLONE_NEWPID,
                        ) {
                            return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                        }
                    }
                    // chroot to our root path
                    if let Err(e) = nix::unistd::chroot(&root_path) {
                        // e is Errno
                        if e == nix::errno::Errno::EPERM {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::PermissionDenied,
                                "chroot denied",
                            ));
                        }
                        return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                    }
                    if let Err(e) = nix::unistd::chdir("/") {
                        return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                    }
                    // apply resource limits from security policy if set
                    // max memory (RLIMIT_AS = virtual address space)
                    if let Some(mem_mb) = policy_limits.max_memory_mb {
                        let bytes = mem_mb * 1024 * 1024;
                        let _ = setrlimit(Resource::RLIMIT_AS, bytes, bytes);
                    }
                    // max open files (RLIMIT_NOFILE)
                    if let Some(files) = policy_limits.max_open_files {
                        let _ = setrlimit(Resource::RLIMIT_NOFILE, files as u64, files as u64);
                    }
                    // max processes (RLIMIT_NPROC)
                    if let Some(procs) = policy_limits.max_processes {
                        let _ = setrlimit(Resource::RLIMIT_NPROC, procs as u64, procs as u64);
                    }
                    Ok(())
                })
                .output()
        };

        let output = output_attempt.map_err(|e| {
            anyhow::anyhow!("sandbox {} isolation failed: {}", self.id, e)
        })?;
        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "command failed: {}\nstdout:\n{}\nstderr:\n{}",
                cmd, stdout, stderr
            ));
        }
        let mut stdout = String::new();
        stdout.push_str(&String::from_utf8_lossy(&output.stdout));
        Ok(stdout)
    }
}
