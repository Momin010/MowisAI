use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};

/// Resource limits allocated to a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub ram_bytes: Option<u64>,
    pub cpu_millis: Option<u64>,
    // GPU support will be added later
}

use anyhow::{Context, Result};
use fastrand;
use nix::mount::{mount, umount2, MntFlags, MsFlags};
use nix::sched;
use nix::sys::resource::{setrlimit, Resource};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::TempDir;

// global counter used to keep IDs unique and monotonically increasing. we
// mix a few random low bits so that the sequence isn't trivially guessable.
lazy_static! {
    static ref SANDBOX_COUNTER: AtomicU64 = AtomicU64::new(init_counter());
    static ref CONTAINER_COUNTER: AtomicU64 = AtomicU64::new(init_counter());
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

/// Container represents an isolated environment created from a sandbox
#[derive(Clone)]
pub struct Container {
    pub id: u64,
    pub upper: PathBuf,
    pub work: PathBuf,
    pub root: PathBuf,
}

/// Prepared data for tool invocation without holding sandbox lock.
/// This allows tool execution to proceed without blocking other operations.
pub struct ToolInvocationPrep {
    pub tool: Box<dyn crate::tools::Tool>,
    pub sandbox_id: u64,
    pub container_root: PathBuf,
    pub policy: Option<crate::security::SecurityPolicy>,
    pub tool_name: String,
}

/// Execute a tool without holding any locks. Returns the tool output or error.
pub fn execute_tool_unlocked(
    prep: ToolInvocationPrep,
    input: serde_json::Value,
) -> Result<serde_json::Value> {
    let ctx = crate::tools::ToolContext {
        sandbox_id: prep.sandbox_id,
        root_path: Some(prep.container_root),
    };
    prep.tool.invoke(&ctx, input)
}

#[derive(Serialize)]
pub struct Sandbox {
    id: u64,
    limits: ResourceLimits,
    // path to the root directory used for chroot/mount namespace
    #[serde(skip)]
    root: TempDir,
    #[serde(skip)]
    tools: HashMap<String, Box<dyn crate::tools::Tool>>,
    #[serde(skip)]
    policy: Option<crate::security::SecurityPolicy>,
    // image path for creating containers
    #[serde(skip)]
    image_path: Option<PathBuf>,
    // sandbox upper directory for overlayfs
    #[serde(skip)]
    sandbox_upper: Option<PathBuf>,
    // true if resource limits were successfully enforced (cgroup writes succeeded)
    #[serde(skip)]
    limits_enforced: bool,
    // containers created from this sandbox
    #[serde(skip)]
    containers: HashMap<u64, Container>,
    // optional project root to bind-mount into containers
    #[serde(skip)]
    project_root: Option<PathBuf>,
    // optional scope path to limit visible files (e.g., "src/frontend/")
    #[serde(skip)]
    scope: Option<String>,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // Clean up containers: unmount submounts first, then root, then delete dirs.
        let has_image = self.image_path.is_some();
        for (_, container) in &self.containers {
            let _ = umount2(&container.root.join("workspace"), MntFlags::MNT_DETACH);
            if has_image {
                let _ = umount2(&container.root.join("dev"),  MntFlags::MNT_DETACH);
                let _ = umount2(&container.root.join("proc"), MntFlags::MNT_DETACH);
            }
            let _ = umount2(&container.root, MntFlags::MNT_DETACH);
            if let Some(base) = container.root.parent() {
                let _ = std::fs::remove_dir_all(base);
            }
        }
        // Unmount the sandbox root (overlayfs or tmpfs)
        let _ = umount2(self.root.path(), MntFlags::MNT_DETACH);
        // Clean up overlayfs upper/work dirs if they exist
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

        // track image path and sandbox upper for container creation
        let mut image_path: Option<PathBuf> = None;
        let mut sandbox_upper: Option<PathBuf> = None;

        // mount image via overlayfs (no copying = no OOM)
        // lower = read-only image rootfs, upper = small tmpfs for writes
        if let Some(img_ref) = image {
            let image_mgr = crate::ImageManager::new(None)?;
            let img_path = image_mgr
                .resolve(img_ref)
                .map_err(|e| anyhow::anyhow!("failed to resolve image {}: {}", img_ref, e))?;

            image_path = Some(img_path.clone());

            let overlay_base = std::env::temp_dir().join(format!("overlay-{}", id));
            let upper = overlay_base.join("upper");
            let work = overlay_base.join("work");
            fs::create_dir_all(&upper).context("create overlay upper")?;
            fs::create_dir_all(&work).context("create overlay work")?;

            sandbox_upper = Some(upper.clone());

            let opts = format!(
                "lowerdir={},upperdir={},workdir={}",
                img_path.display(),
                upper.display(),
                work.display()
            );
            match mount(
                Some("overlay"),
                root.path(),
                Some("overlay"),
                MsFlags::empty(),
                Some(opts.as_str()),
            ) {
                Ok(()) => {
                    log::info!("mounted {} via overlayfs into sandbox {}", img_ref, id);
                }
                Err(e) => {
                    log::warn!("overlayfs mount failed for {} ({}); falling back to rootfs copy", img_ref, e);
                    // Clear and copy instead
                    sandbox_upper = None;
                    copy_dir_recursive(&img_path, root.path())
                        .context("copy image rootfs into sandbox root")?;
                    log::info!("copied {} rootfs into sandbox {}", img_ref, id);
                }
            }
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

        let mut sb = Sandbox {
            id,
            limits,
            root,
            tools: HashMap::new(),
            policy: None,
            image_path,
            sandbox_upper,
            limits_enforced: false,
            containers: HashMap::new(),
            project_root: None,
            scope: None,
        };
        // apply resource limits via cgroups if possible
        if let Err(e) = sb.apply_limits() {
            log::warn!("sandbox {} resource limits not applied: {}", id, e);
            sb.limits_enforced = false;
        } else {
            sb.limits_enforced = true;
        }
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
        SANDBOX_COUNTER
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |old| {
                if old <= prev {
                    Some(prev + 1)
                } else {
                    Some(old)
                }
            })
            .ok();
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
        let mut sb = Sandbox {
            id,
            limits,
            root,
            tools: HashMap::new(),
            policy: None,
            image_path: None,
            sandbox_upper: None,
            limits_enforced: false,
            containers: HashMap::new(),
            project_root: None,
            scope: None,
        };
        if let Err(e) = sb.apply_limits() {
            log::warn!("sandbox {} resource limits not applied: {}", id, e);
            sb.limits_enforced = false;
        } else {
            sb.limits_enforced = true;
        }
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

    /// set the project root to bind-mount into containers
    pub fn set_project_root(&mut self, project_root: PathBuf) {
        self.project_root = Some(project_root);
    }

    /// set the scope path to limit visible files in containers
    pub fn set_scope(&mut self, scope: String) {
        self.scope = Some(scope);
    }

    /// Seed a git repository into the sandbox baseline under /workspace.
    /// All subsequently created containers inherit this repository in their lower layer.
    pub fn seed_git_repo(
        &self,
        repo_url: &str,
        branch: Option<&str>,
        subdir: Option<&str>,
    ) -> Result<()> {
        let target = subdir.unwrap_or("repo");
        let mut cmd = format!(
            "mkdir -p /workspace && cd /workspace && rm -rf '{}' && git clone '{}' '{}'",
            target, repo_url, target
        );
        if let Some(branch) = branch {
            cmd.push_str(&format!(" && cd '{}' && git checkout '{}'", target, branch));
        }

        let output = Command::new("chroot")
            .arg(self.root.path())
            .arg("/bin/sh")
            .arg("-c")
            .arg(&cmd)
            .output()
            .context("seed_git_repo chroot failed")?;

        if output.status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "seed_git_repo failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    /// write resource limits into cgroup v2 if available and running as root.
    fn apply_limits(&mut self) -> Result<()> {
        let cgroup_base = Path::new("/sys/fs/cgroup/agentd");
        if !cgroup_base.exists() {
            if let Err(e) = fs::create_dir_all(cgroup_base) {
                log::warn!(
                    "unable to create cgroup base {}: {}",
                    cgroup_base.display(),
                    e
                );
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

    /// invoke a named tool directly in the sandbox (no container).
    /// this is a convenience wrapper used by unit tests and simple integrations,
    /// it behaves like `invoke_tool_in_container` but uses the sandbox's own root
    /// directory rather than requiring a container id.
    /// DEPRECATED: Use prepare_tool_invocation() + execute_tool_unlocked() for lock-free execution.
    /// This method has incomplete policy checks (only 5 tools covered).
    /// DEPRECATED: Use `prepare_tool_invocation()` + `execute_tool_unlocked()` for lock-free execution.
    /// This method has incomplete policy checks (only 5 tools covered).
    /// Invoke a tool in the sandbox root context.
    pub fn invoke_tool(
        &self,
        name: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value> {
        // reuse the same security policy checks as the container variant
        if let Some(policy) = &self.policy {
            match name {
                "read_file" | "get_file_info" => {
                    let path = input["path"].as_str().unwrap_or("");
                    if !policy.check_file_access(path, crate::security::FileAccessType::Read) {
                        return Err(anyhow::anyhow!(
                            "security policy denied read access to {}",
                            path
                        ));
                    }
                }
                "write_file" | "delete_file" | "create_directory" | "copy_file" => {
                    let path = input["path"]
                        .as_str()
                        .or_else(|| input["dst"].as_str())
                        .unwrap_or("");
                    if !policy.check_file_access(path, crate::security::FileAccessType::Write) {
                        return Err(anyhow::anyhow!(
                            "security policy denied write access to {}",
                            path
                        ));
                    }
                }
                "http_get" | "http_post" => {
                    if !policy.check_network_access(true) {
                        return Err(anyhow::anyhow!(
                            "security policy denied outbound network access"
                        ));
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
            root_path: Some(self.root.path().to_path_buf()),
        };
        let output = tool.invoke(&ctx, input)?;
        Ok(output)
    }

    /// DEPRECATED: Use prepare_tool_invocation() + execute_tool_unlocked() for lock-free execution.
    /// This method has incomplete policy checks (only 5 tools covered).
    /// Attempt to invoke a named tool within a container (uses container root path).
    pub fn invoke_tool_in_container(
        &self,
        container_id: u64,
        name: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value> {
        if let Some(policy) = &self.policy {
            match name {
                "read_file" | "get_file_info" => {
                    let path = input["path"].as_str().unwrap_or("");
                    if !policy.check_file_access(path, crate::security::FileAccessType::Read) {
                        return Err(anyhow::anyhow!(
                            "security policy denied read access to {}",
                            path
                        ));
                    }
                }
                "write_file" | "delete_file" | "create_directory" | "copy_file" => {
                    let path = input["path"]
                        .as_str()
                        .or_else(|| input["dst"].as_str())
                        .unwrap_or("");
                    if !policy.check_file_access(path, crate::security::FileAccessType::Write) {
                        return Err(anyhow::anyhow!(
                            "security policy denied write access to {}",
                            path
                        ));
                    }
                }
                "http_get" | "http_post" => {
                    if !policy.check_network_access(true) {
                        return Err(anyhow::anyhow!(
                            "security policy denied outbound network access"
                        ));
                    }
                }
                _ => {}
            }
        }
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("tool not registered: {}", name))?;
        let container_root = self
            .get_container_root(container_id)
            .ok_or_else(|| anyhow::anyhow!("container {} not found", container_id))?;
        let ctx = crate::tools::ToolContext {
            sandbox_id: self.id,
            root_path: Some(container_root),
        };
        let output = tool.invoke(&ctx, input)?;
        Ok(output)
    }

    pub fn set_policy(&mut self, policy: crate::security::SecurityPolicy) {
        self.policy = Some(policy);
    }

    /// Get the current security policy (if set)
    pub fn policy(&self) -> Option<&crate::security::SecurityPolicy> {
        self.policy.as_ref()
    }

    /// List all container IDs in this sandbox
    pub fn list_containers(&self) -> Vec<u64> {
        self.containers.keys().copied().collect()
    }

    /// Prepare a tool for invocation without holding locks. Returns prep data that can
    /// be executed lock-free via `execute_tool_unlocked`. This enables other operations
    /// to proceed while a slow tool (e.g. git_clone, run_command) is executing.
    pub fn prepare_tool_invocation(
        &self,
        container_id: u64,
        name: &str,
        input: &serde_json::Value,
    ) -> Result<ToolInvocationPrep> {
        // Policy checks (same as before)
        if let Some(policy) = &self.policy {
            match name {
                // File access
                "read_file" | "get_file_info" => {
                    let path = input["path"].as_str().unwrap_or("");
                    if !policy.check_file_access(path, crate::security::FileAccessType::Read) {
                        return Err(anyhow::anyhow!(
                            "security policy denied read access to {}",
                            path
                        ));
                    }
                }
                "write_file" | "delete_file" | "create_directory" | "copy_file" => {
                    let path = input["path"]
                        .as_str()
                        .or_else(|| input["dst"].as_str())
                        .unwrap_or("");
                    if !policy.check_file_access(path, crate::security::FileAccessType::Write) {
                        return Err(anyhow::anyhow!(
                            "security policy denied write access to {}",
                            path
                        ));
                    }
                }
                // Network access
                "http_get" | "http_post" | "http_put" | "http_delete" | "http_patch" |
                "download_file" | "websocket_send" => {
                    if !policy.check_network_access(true) {
                        return Err(anyhow::anyhow!(
                            "security policy denied outbound network access"
                        ));
                    }
                }
                // Shell commands are dangerous - require explicit permission
                "run_command" | "run_script" => {
                    if !policy.check_shell_execution() {
                        return Err(anyhow::anyhow!(
                            "security policy denied shell command execution"
                        ));
                    }
                }
                _ => {}
            }
        }

        // Fetch tool and container
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("tool not registered: {}", name))?
            .clone_box();

        let container_root = self
            .get_container_root(container_id)
            .ok_or_else(|| anyhow::anyhow!("container {} not found", container_id))?
            .clone();

        Ok(ToolInvocationPrep {
            tool,
            sandbox_id: self.id,
            container_root,
            policy: self.policy.clone(),
            tool_name: name.to_string(),
        })
    }

    /// create a new container from this sandbox with overlayfs isolation
    pub fn create_container(&mut self) -> anyhow::Result<u64> {
        // use the same structured ID scheme as sandboxes: monotonic counter + random bits
        const RANDOM_BITS: u64 = 16;
        const RANDOM_MASK: u64 = (1 << RANDOM_BITS) - 1;
        let base = CONTAINER_COUNTER.fetch_add(1, Ordering::SeqCst);
        let rand = fastrand::u64(..(RANDOM_MASK + 1));
        let id = (base << RANDOM_BITS) | (rand & RANDOM_MASK);

        let base = std::env::temp_dir().join(format!("container-{}", id));
        let upper = base.join("upper");
        let work = base.join("work");
        let root = base.join("root");
        fs::create_dir_all(&upper)?;
        fs::create_dir_all(&work)?;
        fs::create_dir_all(&root)?;

        // Make container directories readable by all users for basic access.
        // NOTE: Checkpoint operations now run via agentd socket (privileged),
        // so we don't need 777 permissions here. The orchestrator calls
        // create_checkpoint/restore_checkpoint which run as root in agentd.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            let _ = std::fs::set_permissions(&upper, perms.clone());
            let _ = std::fs::set_permissions(&work, perms.clone());
            let _ = std::fs::set_permissions(&root, perms);
            // Also set permissions on the base directory itself
            let _ = std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o755));
        }

        // build the filesystem for the new container. when the sandbox was
        // created with an image we mount an overlay using that image as the
        // lower layer. if no image was provided (e.g. tests running in an
        // unprivileged environment) fall back to a plain tmpfs rootdir so the
        // container still has an isolated filesystem.
        if let Some(image_lower) = &self.image_path {
            // existing overlay logic
            let lowerdir = if let Some(sb_upper) = &self.sandbox_upper {
                if sb_upper.exists() {
                    format!("{}:{}", sb_upper.display(), image_lower.display())
                } else {
                    format!("{}", image_lower.display())
                }
            } else {
                format!("{}", image_lower.display())
            };

            let opts = format!(
                "lowerdir={},upperdir={},workdir={}",
                lowerdir,
                upper.display(),
                work.display()
            );
            if let Err(e) = mount(
                Some("overlay"),
                &root,
                Some("overlay"),
                MsFlags::empty(),
                Some(opts.as_str()),
            ) {
                return Err(anyhow::anyhow!("overlay mount failed for container: {}", e));
            }
        } else {
            // no image: simple tmpfs mount (nonfatal if it fails)
            if let Err(e) = mount(
                Some("tmpfs"),
                &root,
                Some("tmpfs"),
                MsFlags::empty(),
                None::<&str>,
            ) {
                log::warn!("unable to mount tmpfs for container {}: {}", id, e);
            }
        }

        // DNS
        let etc = root.join("etc");
        fs::create_dir_all(&etc)?;
        let _ = fs::copy("/etc/resolv.conf", etc.join("resolv.conf"));
        let _ = fs::copy("/etc/hosts", etc.join("hosts"));

        // Mount /proc and /dev only when the sandbox has a real image (rootfs).
        // For plain-tmpfs sandboxes (no image) there is no /bin/sh to exec, so
        // the container never needs /proc or /dev populated.  Skipping devtmpfs
        // avoids blocking the kdevtmpfs kernel thread (which populates device
        // nodes asynchronously) and keeps the DashMap write-lock duration short.
        if self.image_path.is_some() {
            let proc_dir = root.join("proc");
            fs::create_dir_all(&proc_dir)?;
            let _ = mount(
                Some("proc"),
                &proc_dir,
                Some("proc"),
                MsFlags::empty(),
                None::<&str>,
            );

            let dev_dir = root.join("dev");
            fs::create_dir_all(&dev_dir)?;
            let _ = mount(
                Some("devtmpfs"),
                &dev_dir,
                Some("devtmpfs"),
                MsFlags::empty(),
                None::<&str>,
            );
        }

        // Mount an isolated overlay workspace if a project root is configured.
        // The project tree is always the read-only lowerdir; each container gets
        // its own writable upper/work dirs so agents never write into the host repo.
        if let Some(project_path) = &self.project_root {
            if project_path.exists() {
                let scope_path = self
                    .scope
                    .as_ref()
                    .map(|scope| scope.trim_matches('/'))
                    .filter(|scope| !scope.is_empty())
                    .map(|scope| project_path.join(scope));

                let workspace_lower = match scope_path {
                    Some(path) if path.exists() => path,
                    Some(path) => {
                        log::warn!(
                            "container {} scope path does not exist: {}, using full project root",
                            id,
                            path.display()
                        );
                        project_path.clone()
                    }
                    None => project_path.clone(),
                };

                let workspace_dir = root.join("workspace");
                let workspace_upper = base.join("workspace-upper");
                let workspace_work = base.join("workspace-work");

                fs::create_dir_all(&workspace_dir)?;
                fs::create_dir_all(&workspace_upper)?;
                fs::create_dir_all(&workspace_work)?;

                let workspace_opts = format!(
                    "lowerdir={},upperdir={},workdir={}",
                    workspace_lower.display(),
                    workspace_upper.display(),
                    workspace_work.display()
                );

                match mount(
                    Some("overlay"),
                    &workspace_dir,
                    Some("overlay"),
                    MsFlags::empty(),
                    Some(workspace_opts.as_str()),
                ) {
                    Ok(()) => {
                        log::info!(
                            "container {} mounted isolated workspace overlay from {}",
                            id,
                            workspace_lower.display()
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "container {} overlayfs workspace mount failed ({}); falling back to copy",
                            id,
                            e
                        );
                        copy_dir_recursive(&workspace_lower, &workspace_dir)
                            .context("copy project root into container workspace")?;
                        log::info!(
                            "container {} copied workspace from {}",
                            id,
                            workspace_lower.display()
                        );
                    }
                }
            }
        }

        self.containers.insert(
            id,
            Container {
                id,
                upper,
                work,
                root,
            },
        );
        Ok(id)
    }

    /// get the root path for a container
    pub fn get_container_root(&self, container_id: u64) -> Option<PathBuf> {
        self.containers.get(&container_id).map(|c| c.root.clone())
    }

    /// get the upper (writable layer) path for a container
    pub fn get_container_upper(&self, container_id: u64) -> Option<PathBuf> {
        self.containers.get(&container_id).map(|c| c.upper.clone())
    }

    /// create a checkpoint snapshot of a container's upper layer
    /// this is called from the privileged socket server context
    pub fn checkpoint_container(&self, container_id: u64, snapshot_dir: &Path) -> anyhow::Result<()> {
        let upper = self.get_container_upper(container_id)
            .ok_or_else(|| anyhow::anyhow!("container {} not found", container_id))?;
        
        // Ensure snapshot directory exists
        std::fs::create_dir_all(snapshot_dir)?;
        
        // Use cp -a to preserve permissions and do a regular copy
        // (not hard links since we want a true snapshot)
        let cp_result = std::process::Command::new("cp")
            .arg("-a")
            .arg(upper.join("."))
            .arg(snapshot_dir)
            .output()?;
        
        if !cp_result.status.success() {
            return Err(anyhow::anyhow!(
                "checkpoint failed: {}",
                String::from_utf8_lossy(&cp_result.stderr)
            ));
        }
        
        Ok(())
    }

    /// restore a container's upper layer from a checkpoint snapshot
    pub fn restore_container(&self, container_id: u64, snapshot_dir: &Path) -> anyhow::Result<()> {
        let upper = self.get_container_upper(container_id)
            .ok_or_else(|| anyhow::anyhow!("container {} not found", container_id))?;
        
        // Remove current upper contents
        if upper.exists() {
            // Make writable first (in case files are read-only)
            let _ = std::process::Command::new("chmod")
                .arg("-R")
                .arg("+w")
                .arg(&upper)
                .output();
            std::fs::remove_dir_all(&upper)?;
        }
        std::fs::create_dir_all(&upper)?;
        
        // Restore from snapshot
        let cp_result = std::process::Command::new("cp")
            .arg("-a")
            .arg(snapshot_dir.join("."))
            .arg(&upper)
            .output()?;
        
        if !cp_result.status.success() {
            return Err(anyhow::anyhow!(
                "restore failed: {}",
                String::from_utf8_lossy(&cp_result.stderr)
            ));
        }
        
        Ok(())
    }

    /// destroy a container and clean up its resources
    pub fn destroy_container(&mut self, container_id: u64) -> anyhow::Result<()> {
        use nix::mount::{umount2, MntFlags};

        if let Some(container) = self.containers.remove(&container_id) {
            // Unmount submounts first to prevent mount-namespace bloat.
            // Workspace is always mounted; proc/dev only exist when the sandbox
            // has a real image (see create_container).
            let _ = umount2(&container.root.join("workspace"), MntFlags::MNT_DETACH);
            if self.image_path.is_some() {
                let _ = umount2(&container.root.join("dev"),  MntFlags::MNT_DETACH);
                let _ = umount2(&container.root.join("proc"), MntFlags::MNT_DETACH);
            }
            // Now unmount the container root itself
            let _ = umount2(&container.root, MntFlags::MNT_DETACH);
            // Remove the container directory tree
            if let Some(base) = container.root.parent() {
                let _ = std::fs::remove_dir_all(base);
            }
            Ok(())
        } else {
            Err(anyhow::anyhow!("container {} not found", container_id))
        }
    }

    /// run a command inside the sandbox, returning stdout output or an error.
    /// this function uses `unshare` + `chroot` to isolate the process.

    pub fn run_command(&self, cmd: &str) -> Result<String> {
        let root_path = self.root.path().to_owned();
        let _sid = self.id; // copy id so closure doesn't borrow self

        // npm requires thread creation which fails in strict namespaces.
        // detect npm commands and run them with chroot only (no namespace isolation).
        let is_npm = cmd.starts_with("npm") || cmd.contains(" npm ");

        // extract policy limits for the closure
        let policy_limits = self
            .policy
            .as_ref()
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
                .env(
                    "PATH",
                    "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
                )
                .pre_exec(move || {
                    // skip namespace isolation for npm commands
                    if !is_npm {
                        // unshare all available namespaces to isolate. if we're not
                        // running as root (common during CI/tests) the call will fail
                        // with EPERM; that's harmless, so log a warning and continue
                        // without isolation rather than returning an error.
                        if let Err(e) = sched::unshare(
                            sched::CloneFlags::CLONE_NEWNS
                                | sched::CloneFlags::CLONE_NEWPID
                                | sched::CloneFlags::CLONE_NEWUSER
                                | sched::CloneFlags::CLONE_NEWNET
                                | sched::CloneFlags::CLONE_NEWIPC
                                | sched::CloneFlags::CLONE_NEWUTS,
                        ) {
                            return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                        }
                    }
                    // chroot to our root path
                    if let Err(e) = nix::unistd::chroot(&root_path) {
                        // if permission denied, we can't isolate; log and continue
                        if e == nix::errno::Errno::EPERM {
                            log::warn!("chroot denied for sandbox {} (running unisolated)", _sid);
                        } else {
                            return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                        }
                    } else {
                        // only change directory if chroot succeeded
                        if let Err(e) = nix::unistd::chdir("/") {
                            return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                        }
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

        let output = output_attempt
            .map_err(|e| anyhow::anyhow!("sandbox {} isolation failed: {}", self.id, e))?;
        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "command failed: {}\nstdout:\n{}\nstderr:\n{}",
                cmd,
                stdout,
                stderr
            ));
        }
        let mut stdout = String::new();
        stdout.push_str(&String::from_utf8_lossy(&output.stdout));
        Ok(stdout)
    }
}

/// Recursively copy `src` directory contents into `dst`.
/// Used as a fallback when overlayfs is unavailable (e.g., Docker-on-overlay).
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let ftype = entry.file_type()?;
        if ftype.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if ftype.is_symlink() {
            let target = fs::read_link(&src_path)?;
            let _ = std::os::unix::fs::symlink(&target, &dst_path);
        } else {
            let _ = fs::copy(&src_path, &dst_path);
        }
    }
    Ok(())
}
