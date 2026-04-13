use anyhow::{Context, Result};
use dashmap::DashMap;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use crate::audit::{AuditEvent, EventType};
use crate::buckets::BucketStore;
use crate::memory::AgentMemory;
use crate::security::SecurityPolicy;
use crate::tool_registry;
use crate::{ResourceLimits, Sandbox};
use crate::vm_backend::{boot_vm, exec_in_vm, stop_vm, VmHandle};

const MAX_CONNECTIONS: usize = 2048;
const FAST_WORKERS: usize = 64;
const SLOW_WORKERS: usize = 128;

lazy_static! {
    // shared state across threads; wrap map in Arc for cheap cloning
    static ref SANDBOXES: DashMap<u64, Sandbox> = DashMap::new();
    static ref AUDITOR: crate::audit::SecurityAuditor = {
        let audit_path = if cfg!(test) {
            "/tmp/agentd-test/log/audit.log"
        } else {
            "/var/log/agentd/audit.log"
        };
        let _ = std::fs::create_dir_all(std::path::Path::new(audit_path).parent().unwrap());
        crate::audit::SecurityAuditor::new(std::path::Path::new(audit_path))
            .expect("failed to initialize audit logger")
    };
    static ref PERSISTENCE: crate::persistence::PersistenceManager = {
        let persist_path = if cfg!(test) {
            "/tmp/agentd-test/lib"
        } else {
            "/var/lib/agentd"
        };
        let _ = std::fs::create_dir_all(persist_path);
        crate::persistence::PersistenceManager::new(std::path::Path::new(persist_path))
    };
    static ref MEMORY_STORE: DashMap<u64, AgentMemory> = DashMap::new();
    static ref COORDINATOR: crate::agent_loop::AgentCoordinator =
        crate::agent_loop::AgentCoordinator::new();

    // Guest OS supervisor processes (scaffold backend — deprecated).
    static ref GUEST_SUPERVISORS: DashMap<u64, u32> = DashMap::new();
    static ref SANDBOX_BACKENDS: DashMap<u64, String> = DashMap::new();
    // VM handles (new guest_vm backend).
    static ref VM_HANDLES: DashMap<u64, VmHandle> = DashMap::new();

}

// ── Wire types ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct SocketRequest {
    pub request_type: String,
    /// Sandbox ID – accepted as either a string ("123") or bare number (123)
    pub sandbox: Option<Value>,
    pub ram: Option<u64>,
    pub cpu: Option<u64>,
    /// OS image to use, e.g. "alpine" (required for create_sandbox)
    pub image: Option<String>,
    /// Container ID – same flexible parsing as sandbox
    pub container: Option<Value>,
    /// Extra packages to install on top of core packages
    pub packages: Option<Vec<String>>,
    /// Optional Git repository URL to seed into sandbox baseline (/workspace)
    pub seed_repo_url: Option<String>,
    /// Optional branch or ref for repo seeding
    pub seed_repo_branch: Option<String>,
    /// Optional subdirectory inside /workspace where repo should be cloned
    pub seed_repo_subdir: Option<String>,
    /// Optional local directory to bind-mount into /workspace (for development workflows)
    pub project_root: Option<String>,
    /// Optional scope path to limit what files this sandbox can see (e.g., "src/frontend/")
    pub scope: Option<String>,
    pub name: Option<String>,
    pub input: Option<Value>,
    pub command: Option<String>,
    pub to: Option<u64>,
    pub channel: Option<u64>,
    pub agent: Option<u64>,
    /// Execution backend for the sandbox lifecycle (e.g. "chroot", "guest_vm").
    /// If omitted, defaults to "chroot".
    pub backend: Option<String>,
    /// Checkpoint ID for create_checkpoint and restore_checkpoint operations
    pub checkpoint_id: Option<u64>,
    /// Checkpoint directory path for create_checkpoint and restore_checkpoint
    pub checkpoint_dir: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SocketResponse {
    pub status: String,
    pub result: Option<Value>,
    pub error: Option<String>,
}

impl SocketResponse {
    fn ok(result: Option<Value>) -> Self {
        SocketResponse { status: "ok".into(), result, error: None }
    }
    fn err<E: ToString>(e: E) -> Self {
        SocketResponse { status: "error".into(), result: None, error: Some(e.to_string()) }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Accept sandbox/container IDs as either a JSON string or a JSON number.
fn parse_id(val: &Value) -> Option<u64> {
    match val {
        Value::String(s) => s.parse::<u64>().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

/// Run a single command inside the OS image via chroot and stream its output
/// to stdout in real time so the caller can see progress (e.g. apk install).
/// Returns an error if the command exits non-zero.
fn chroot_run_streaming(root: &std::path::Path, cmd: &str) -> Result<()> {
    use std::io::BufRead;
    use std::process::{Command, Stdio};

    // Copy DNS config into the image so network calls work
    let etc = root.join("etc");
    std::fs::create_dir_all(&etc).ok();
    let _ = std::fs::copy("/etc/resolv.conf", etc.join("resolv.conf"));
    let _ = std::fs::copy("/etc/hosts", etc.join("hosts"));

    let mut child = Command::new("chroot")
        .arg(root)
        .arg("/bin/sh")
        .arg("-c")
        .arg(cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("chroot spawn failed")?;

    // Stream stdout
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().flatten() {
            log::info!("[sandbox] {}", line);
        }
    }
    // Stream stderr
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().flatten() {
            log::warn!("[sandbox] {}", line);
        }
    }

    let status = child.wait().context("chroot wait failed")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("command exited with status: {}", status))
    }
}

/// Install packages inside the OS image. Detects the package manager from the
/// image name. Always installs the core set first, then any extras requested.
/// Live output is streamed to stdout so the user can see progress.
fn install_packages_in_image(
    root: &std::path::Path,
    image_hint: &str,
    extra_packages: &[String],
) -> Result<()> {
    // Core packages every container needs – git, shell utilities, runtimes
    let core = [
        // tooling
        "git", "curl", "wget", "bash", "ca-certificates", "openssh-client",
        // runtimes
        "python3", "py3-pip", "nodejs", "npm",
        // container stack (guest OS requirement)
        // Note: exact package names vary by distro; we resolve per distro below where needed.
        "docker", "containerd", "runc",
    ];

    let is_alpine = image_hint.contains("alpine") || image_hint.is_empty();
    let is_debian = image_hint.contains("ubuntu") || image_hint.contains("debian");

    let all_packages: Vec<&str> = {
        let mut v: Vec<&str> = core.iter().copied().collect();
        v.extend(extra_packages.iter().map(|s| s.as_str()));
        v
    };

    log::info!("[sandbox] Installing packages: {}", all_packages.join(" "));

    let install_cmd = if is_alpine {
        // For Alpine, set up repositories to use HTTP instead of HTTPS to avoid TLS bootstrap issues
        // Then run apk add
        // Alpine package set for Docker engine.
        let alpine_pkgs = all_packages
            .iter()
            .map(|p| match *p {
                "docker" => "docker",
                "containerd" => "containerd",
                "runc" => "runc",
                other => other,
            })
            .collect::<Vec<&str>>()
            .join(" ");
        format!(
            "echo 'http://dl-cdn.alpinelinux.org/alpine/v3.23/main' > /etc/apk/repositories && \
             echo 'http://dl-cdn.alpinelinux.org/alpine/v3.23/community' >> /etc/apk/repositories && \
             apk add --no-cache {}",
            alpine_pkgs
        )
    } else if is_debian {
        // Debian/Ubuntu package set for Docker engine (distro packages).
        let deb_pkgs = all_packages
            .iter()
            .map(|p| match *p {
                "docker" => "docker.io",
                "containerd" => "containerd",
                "runc" => "runc",
                other => other,
            })
            .collect::<Vec<&str>>()
            .join(" ");
        format!(
            "apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends {}",
            deb_pkgs
        )
    } else {
        // Unknown image – try apk first (with HTTP repos to avoid TLS bootstrap), fall back to apt
        format!(
            "echo 'http://dl-cdn.alpinelinux.org/alpine/v3.23/main' > /etc/apk/repositories && \
             echo 'http://dl-cdn.alpinelinux.org/alpine/v3.23/community' >> /etc/apk/repositories && \
             apk add --no-cache {apk_pkgs} 2>/dev/null || (apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y {apt_pkgs})",
            apk_pkgs = all_packages.join(" "),
            apt_pkgs = all_packages
                .iter()
                .map(|p| if *p == "docker" { "docker.io" } else { p })
                .collect::<Vec<_>>()
                .join(" ")
        )
    };

    // Copy CA certificates into chroot so apk can verify TLS
    // Try multiple common locations to find system CA certificates
    let ca_src_paths = vec![
        "/etc/ssl/certs/ca-certificates.crt",      // Debian/Ubuntu bundle
        "/etc/pki/tls/certs/ca-bundle.crt",        // RedHat/CentOS bundle
        "/etc/ssl/certs/ca-bundle.crt",            // Alpine alternative
    ];
    
    let ca_dest = root.join("etc/ssl/certs");
    std::fs::create_dir_all(&ca_dest).ok();
    
    // Try to copy the first available CA bundle
    for src in &ca_src_paths {
        if std::path::Path::new(src).exists() {
            let dest = ca_dest.join("ca-certificates.crt");
            let _ = std::fs::copy(src, &dest);
            if dest.exists() {
                break;  // Successfully copied, stop trying other paths
            }
        }
    }
    
    // Also copy individual cert files if ca-certificates.crt exists
    if std::path::Path::new("/etc/ssl/certs").exists() {
        if let Ok(entries) = std::fs::read_dir("/etc/ssl/certs") {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_file() {
                        let filename = entry.file_name();
                        if let Some(name) = filename.to_str() {
                            // Copy .pem and .crt files
                            if name.ends_with(".pem") || name.ends_with(".crt") {
                                let _ = std::fs::copy(entry.path(), ca_dest.join(name));
                            }
                        }
                    }
                }
            }
        }
    }

    chroot_run_streaming(root, &install_cmd)
        .context("package installation failed")?;

    log::info!("[sandbox] Packages installed.");
    Ok(())
}

// ── Request handler ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RequestLane {
    Fast,
    Slow,
}

fn validate_request(req: &SocketRequest) -> Result<(), String> {
    match req.request_type.as_str() {
        "create_sandbox" | "list" | "get_audit_stats" | "get_anomalies" | "agent_spawn" => Ok(()),
        "create_container" => {
            if req.sandbox.is_none() {
                Err("create_container: missing sandbox id".to_string())
            } else {
                Ok(())
            }
        }
        "invoke_tool" => {
            if req.sandbox.is_none() {
                Err("invoke_tool: missing sandbox id".to_string())
            } else if req.container.is_none() {
                Err("invoke_tool: missing container id".to_string())
            } else if req.name.as_ref().map(|n| n.is_empty()).unwrap_or(true) {
                Err("invoke_tool: missing tool name".to_string())
            } else {
                Ok(())
            }
        }
        "destroy_sandbox" => {
            if req.sandbox.is_none() {
                Err("destroy_sandbox: missing sandbox id".to_string())
            } else {
                Ok(())
            }
        }
        "destroy_container" => {
            if req.sandbox.is_none() {
                Err("destroy_container: missing sandbox id".to_string())
            } else if req.container.is_none() {
                Err("destroy_container: missing container id".to_string())
            } else {
                Ok(())
            }
        }
        "create_checkpoint" => {
            if req.sandbox.is_none() {
                Err("create_checkpoint: missing sandbox id".to_string())
            } else if req.container.is_none() {
                Err("create_checkpoint: missing container id".to_string())
            } else if req.checkpoint_dir.is_none() {
                Err("create_checkpoint: missing checkpoint_dir".to_string())
            } else {
                Ok(())
            }
        }
        "restore_checkpoint" => {
            if req.sandbox.is_none() {
                Err("restore_checkpoint: missing sandbox id".to_string())
            } else if req.container.is_none() {
                Err("restore_checkpoint: missing container id".to_string())
            } else if req.checkpoint_dir.is_none() {
                Err("restore_checkpoint: missing checkpoint_dir".to_string())
            } else {
                Ok(())
            }
        }
        "set_policy" => {
            if req.sandbox.is_none() {
                Err("set_policy: missing sandbox id".to_string())
            } else if req.name.as_ref().map(|n| n.is_empty()).unwrap_or(true) {
                Err("set_policy: missing policy name".to_string())
            } else {
                Ok(())
            }
        }
        "list_containers" | "get_policy" | "register_tool" | "bucket_put" | "bucket_get" | "memory_set" | "memory_get" | "memory_save" | "memory_load" | "create_channel" => {
            if req.sandbox.is_none() {
                Err(format!("{}: missing sandbox id", req.request_type))
            } else {
                Ok(())
            }
        }
        "send_message" | "read_messages" => {
            if req.sandbox.is_none() {
                Err(format!("{}: missing sandbox id", req.request_type))
            } else {
                Ok(())
            }
        }
        "agent_run" | "agent_status" => {
            if req.agent.is_none() {
                Err(format!("{}: missing agent id", req.request_type))
            } else {
                Ok(())
            }
        }
        other => Err(format!("unknown request type: {}", other)),
    }
}

fn classify_request(req: &SocketRequest) -> RequestLane {
    match req.request_type.as_str() {
        "list" | "list_containers" | "get_policy" | "get_audit_stats" | "get_anomalies" | "agent_status" | "bucket_get" | "memory_get" | "memory_load" | "read_messages" => RequestLane::Fast,
        _ => RequestLane::Slow,
    }
}

fn handle_request(req: SocketRequest) -> SocketResponse {
    if let Err(err) = validate_request(&req) {
        return SocketResponse::err(err);
    }

    match req.request_type.as_str() {

        // ── create_sandbox ──────────────────────────────────────────────────
        // 1. Create sandbox with the given OS image (required).
        // 2. Install core packages + any extras via chroot into that image.
        //    Live output streams to the server's stdout so callers see progress.
        // 3. Every container created from this sandbox will inherit those packages
        //    via overlayfs (lower layer = sandbox upper, which has the packages).
        "create_sandbox" => {
            let image = req.image.clone().unwrap_or_else(|| "alpine".to_string());
            let backend = req.backend.clone().unwrap_or_else(|| "chroot".to_string());
            let limits = ResourceLimits { ram_bytes: req.ram, cpu_millis: req.cpu };
            let seed_repo_url = req.seed_repo_url.clone();
            let seed_repo_branch = req.seed_repo_branch.clone();
            let seed_repo_subdir = req.seed_repo_subdir.clone();

            let mut sb = match Sandbox::new_with_image(limits, Some(&image)) {
                Ok(s) => s,
                Err(e) => {
                    let _ = AUDITOR.record_event(
                        AuditEvent::new(EventType::SandboxCreated, 0, "sandbox creation failed")
                            .with_result(format!("failed: {}", e)),
                    );
                    return SocketResponse::err(format!("create_sandbox failed: {}", e));
                }
            };

            let id = sb.id();
            let root = sb.root_path().to_owned();
            let extra = req.packages.as_deref().unwrap_or(&[]);

            log::info!("sandbox created: {}", id);
            log::info!("[agentd] Setting up sandbox {} with image '{}'", id, image);

            if let Err(e) = install_packages_in_image(&root, &image, extra) {
                log::warn!("sandbox {} package install warning: {}", id, e);
                // Non-fatal: continue even if some optional packages failed.
                // Core failures will be caught when the first tool runs.
            }

            // Optional: seed a repository into sandbox baseline so all containers share it.
            if let Some(repo_url) = seed_repo_url.as_ref() {
                log::info!(
                    "[agentd] Seeding repo {} into sandbox {} ...",
                    repo_url, id
                );
                if let Err(e) = sb.seed_git_repo(
                    repo_url,
                    seed_repo_branch.as_deref(),
                    seed_repo_subdir.as_deref(),
                ) {
                    log::warn!("sandbox {} repo seed warning: {}", id, e);
                }
            }

            // Optional: bind-mount local project directory into /workspace
            if let Some(project_root) = req.project_root.as_ref() {
                let project_path = std::path::Path::new(project_root);
                if project_path.exists() {
                    // Store in sandbox so containers can also mount it
                    sb.set_project_root(project_path.to_path_buf());

                    // Determine mount source based on scope
                    let mount_source = if let Some(scope) = req.scope.as_ref() {
                        let scoped_path = project_path.join(scope);
                        if scoped_path.exists() {
                            sb.set_scope(scope.clone());
                            scoped_path
                        } else {
                            log::warn!("sandbox {} scope path does not exist: {}, using full project", id, scoped_path.display());
                            project_path.to_path_buf()
                        }
                    } else {
                        project_path.to_path_buf()
                    };

                    let workspace_dir = root.join("workspace");
                    if let Err(e) = std::fs::create_dir_all(&workspace_dir) {
                        log::warn!("sandbox {} failed to create /workspace: {}", id, e);
                    } else {
                        // Bind mount the project directory (or scoped subdir) into /workspace
                        if let Err(e) = nix::mount::mount(
                            Some(&mount_source),
                            &workspace_dir,
                            Some("none"),
                            nix::mount::MsFlags::MS_BIND | nix::mount::MsFlags::MS_RDONLY,  // Read-only mount for safety
                            None::<&str>,
                        ) {
                            log::warn!("sandbox {} failed to bind-mount project root: {}", id, e);
                        } else {
                            let scope_info = req.scope.as_ref().map(|s| format!(" (scope: {})", s)).unwrap_or_default();
                            log::info!("[agentd] Mounted {} into sandbox {} /workspace{}", mount_source.display(), id, scope_info);
                        }
                    }
                } else {
                    log::warn!("sandbox {} project_root does not exist: {}", id, project_root);
                }
            }

            // Register all tools into the sandbox
            for tool in tool_registry::create_all_tools() {
                sb.register_tool(tool);
            }

            SANDBOXES.insert(id, sb);

            // Store backend selection and optionally boot guest scaffold.
            SANDBOX_BACKENDS.insert(id, backend.clone());

            if backend == "guest_vm" {
                match boot_vm(id.to_string(), &root, &image) {
                    Ok(handle) => {
                        VM_HANDLES.insert(id, handle);
                        log::info!("[agentd] guest_vm QEMU boot pid={} sandbox={} port=?", id, id);
                    }
                    Err(e) => {
                        log::warn!("guest_vm boot failed sandbox={} (continuing): {}", id, e);
                    }
                }
            }

            let _ = AUDITOR.record_event(
                AuditEvent::new(EventType::SandboxCreated, 0, "sandbox created")
                    .with_target(id)
                    .with_result("success"),
            );

            log::info!("[agentd] Sandbox {} ready.", id);
            SocketResponse::ok(Some(json!({ "sandbox": id.to_string(), "backend": backend })))
        }

        // ── create_container ────────────────────────────────────────────────
        // Creates an overlayfs container on top of the sandbox image layer.
        // The sandbox upper (with installed packages) is the lower layer, so
        // every container automatically has all core packages available.
        "create_container" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };

            let container_id = match SANDBOXES.get_mut(&sandbox_id) {
                Some(mut sb_ref) => match sb_ref.create_container() {
                    Ok(id) => id,
                    Err(e) => return SocketResponse::err(format!("create_container failed: {}", e)),
                },
                None => return SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
            };

            SocketResponse::ok(Some(json!({ "container": container_id.to_string() })))
        }

        // ── invoke_tool ─────────────────────────────────────────────────────
        // All tool execution happens inside the container via chroot.
        // Nothing runs on the host OS.
        // IMPORTANT: Lock is released before tool execution to allow other operations.
        "invoke_tool" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            let container_id = match req.container.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing container id — create one first with create_container"),
            };
            let name = match req.name {
                Some(ref n) if !n.is_empty() => n.clone(),
                _ => return SocketResponse::err("missing tool name"),
            };
            let input = req.input.clone().unwrap_or(json!({}));

            // VM backend routing
            if let Some(backend) = SANDBOX_BACKENDS.get(&sandbox_id) {
                if backend.value() == "guest_vm" {
                    if let Some(handle) = VM_HANDLES.get(&sandbox_id) {
                        match exec_in_vm(&handle, &name, input.clone()) {
                            Ok(result) => return SocketResponse::ok(Some(result)),
                            Err(e) => return SocketResponse::err(format!("vm tool {} failed: {}", name, e)),
                        }
                    }
                }
            }

            // CRITICAL FIX: Prepare tool while holding lock, then drop lock before execution.
            // This prevents long-running tools from blocking all other operations.
            let prep_result = match SANDBOXES.get(&sandbox_id) {
                Some(sb) => sb.prepare_tool_invocation(container_id, &name, &input),
                None => return SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
            };

            match prep_result {
                Ok(prep) => {
                    // Execute tool WITHOUT holding SANDBOXES lock.
                    // Other requests can proceed in parallel.
                    let result = crate::sandbox::execute_tool_unlocked(prep, input);

                    // Re-acquire lock only for audit logging (should be fast).
                    if result.is_ok() {
                        let _ = AUDITOR.record_event(
                            AuditEvent::new(EventType::ToolInvoked, sandbox_id, "tool invoked")
                                .with_details(json!({ "tool": name })),
                        );
                    }

                    match result {
                        Ok(val) => SocketResponse::ok(Some(val)),
                        Err(e) => {
                            let err_str = e.to_string();
                            let event_type = if err_str.contains("security policy denied") {
                                EventType::SecurityViolation
                            } else {
                                EventType::ToolFailed
                            };
                            let _ = AUDITOR.record_event(
                                AuditEvent::new(event_type, sandbox_id, "tool error")
                                    .with_details(json!({ "tool": name, "error": err_str })),
                            );
                            SocketResponse::err(e)
                        }
                    }
                }
                Err(e) => {
                    let err_str = e.to_string();
                    let event_type = if err_str.contains("security policy denied") {
                        EventType::SecurityViolation
                    } else {
                        EventType::ToolFailed
                    };
                    let _ = AUDITOR.record_event(
                        AuditEvent::new(event_type, sandbox_id, "tool prep failed")
                            .with_details(json!({ "tool": name, "error": err_str })),
                    );
                    SocketResponse::err(e)
                }
            }
        }

        // ── list ────────────────────────────────────────────────────────────
        "list" => {
            let ids: Vec<String> = SANDBOXES.iter().map(|item| item.key().to_string()).collect();
            SocketResponse::ok(Some(json!(ids)))
        }

        // ── list_containers ────────────────────────────────────────────────
        "list_containers" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            match SANDBOXES.get(&sandbox_id) {
                Some(sb) => {
                    let cids: Vec<String> = sb.list_containers()
                        .iter()
                        .map(|id| id.to_string())
                        .collect();
                    SocketResponse::ok(Some(json!(cids)))
                }
                None => SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
            }
        }

        // ── destroy_container ────────────────────────────────────────────────
        "destroy_container" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            let container_id = match req.container.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing container id"),
            };
            match SANDBOXES.get_mut(&sandbox_id) {
                Some(mut sb) => match sb.destroy_container(container_id) {
                    Ok(_) => SocketResponse::ok(None),
                    Err(e) => SocketResponse::err(format!("destroy_container failed: {}", e)),
                }
                None => SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
            }
        }

        // ── destroy_sandbox ─────────────────────────────────────────────────
        "destroy_sandbox" => {
            let id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            if SANDBOXES.remove(&id).is_some() {
                // If a guest_vm scaffold was started, stop it best-effort.
                let backend = SANDBOX_BACKENDS.get(&id)
                    .map(|e| e.value().clone())
                    .unwrap_or_default();

                if backend == "guest_vm" {
                    if let Some((_, handle)) = VM_HANDLES.remove(&id) {
                        let _ = stop_vm(&handle);
                    }
                }
                SANDBOX_BACKENDS.remove(&id);
                let _ = AUDITOR.record_event(
                    AuditEvent::new(EventType::SandboxDestroyed, 0, "sandbox destroyed")
                        .with_target(id),
                );
                SocketResponse::ok(None)
            } else {
                SocketResponse::err(format!("sandbox {} not found", id))
            }
        }

        // ── register_tool ───────────────────────────────────────────────────
        "register_tool" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            let name = req.name.clone().unwrap_or_default();
            match tool_registry::get_tool(&name) {
                Some(tool) => {
                     match SANDBOXES.get_mut(&sandbox_id) {
                         Some(mut sb_ref) => {
                             sb_ref.register_tool(tool);
                             SocketResponse::ok(None)
                         }
                        None => SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
                    }
                }
                None => SocketResponse::err(format!("unknown tool: {}", name)),
            }
        }

        // ── set_policy / get_policy ─────────────────────────────────────────
        "set_policy" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            let name = req.name.clone().unwrap_or_default();
            let policy = match name.as_str() {
                "restrictive" => SecurityPolicy::default_restrictive(),
                "permissive"  => SecurityPolicy::default_permissive(),
                other => return SocketResponse::err(format!("unknown policy '{}' — use 'restrictive' or 'permissive'", other)),
            };
             match SANDBOXES.get_mut(&sandbox_id) {
                 Some(mut sb_ref) => {
                     sb_ref.set_policy(policy);
                    let _ = AUDITOR.record_event(
                        AuditEvent::new(EventType::Custom("PolicySet".into()), 0, "policy set")
                            .with_target(sandbox_id)
                            .with_details(json!({ "policy": name })),
                    );
                    SocketResponse::ok(None)
                }
                None => SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
            }
        }

        "get_policy" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            match SANDBOXES.get(&sandbox_id) {
                Some(sb) => match sb.policy() {
                    Some(policy) => match serde_json::to_value(policy) {
                        Ok(val) => SocketResponse::ok(Some(val)),
                        Err(e) => SocketResponse::err(format!("serialize policy: {}", e)),
                    },
                    None => SocketResponse::err("no policy set for sandbox"),
                },
                None => SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
            }
        }

        // ── audit ────────────────────────────────────────────────────────────
        "get_audit_stats" => SocketResponse::ok(Some(AUDITOR.get_stats())),
        "get_anomalies"   => SocketResponse::ok(Some(AUDITOR.detect_anomalies())),

        // ── channels ─────────────────────────────────────────────────────────
        "create_channel" => {
            let from = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            let to = match req.to {
                Some(id) => id,
                None => return SocketResponse::err("missing 'to' sandbox id"),
            };
            let id = crate::channels::create_channel(from, to);
            SocketResponse::ok(Some(json!({ "channel": id })))
        }

        "send_message" => {
            let from = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            let channel_id = match req.channel {
                Some(id) => id,
                None => return SocketResponse::err("missing channel id"),
            };
            let payload = req.command.clone().unwrap_or_default();
            match crate::channels::send_message(
                channel_id,
                crate::channels::Message { from, to: 0, payload },
            ) {
                Ok(_)  => SocketResponse::ok(None),
                Err(e) => SocketResponse::err(e),
            }
        }

        "read_messages" => {
            let channel_id = match req.channel {
                Some(id) => id,
                None => return SocketResponse::err("missing channel id"),
            };
            match crate::channels::read_messages(channel_id) {
                Ok(msgs) => {
                    let out: Vec<_> = msgs
                        .iter()
                        .map(|m| json!({ "from": m.from, "to": m.to, "payload": m.payload }))
                        .collect();
                    SocketResponse::ok(Some(json!(out)))
                }
                Err(e) => SocketResponse::err(e),
            }
        }

        // ── bucket store ─────────────────────────────────────────────────────
        "bucket_put" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            let key   = req.name.clone().unwrap_or_default();
            let value = req.command.clone().unwrap_or_default();

            let bucket_path = match SANDBOXES.get(&sandbox_id) {
                Some(sb) => sb.root_path().join("buckets"),
                None => return SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
            };
            match BucketStore::new(bucket_path) {
                Ok(mut bs) => match bs.put(&key, &value) {
                    Ok(())  => SocketResponse::ok(None),
                    Err(e)  => SocketResponse::err(e),
                },
                Err(e) => SocketResponse::err(e),
            }
        }

        "bucket_get" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            let key = req.name.clone().unwrap_or_default();

            let bucket_path = match SANDBOXES.get(&sandbox_id) {
                Some(sb) => sb.root_path().join("buckets"),
                None => return SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
            };
            match BucketStore::new(bucket_path) {
                Ok(bs) => match bs.get(&key) {
                    Ok(Some(v)) => SocketResponse::ok(Some(json!({ "value": v }))),
                    Ok(None)    => SocketResponse::err("key not found"),
                    Err(e)      => SocketResponse::err(e),
                },
                Err(e) => SocketResponse::err(e),
            }
        }

        // ── memory ────────────────────────────────────────────────────────────
        "memory_set" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            let key   = req.name.clone().unwrap_or_default();
            let value = req.input.clone().unwrap_or(json!(null));
            MEMORY_STORE.entry(sandbox_id)
               .or_insert_with(|| AgentMemory::new(sandbox_id, sandbox_id))
               .short_term
               .set_context(key, value);
            SocketResponse::ok(None)
        }

        "memory_get" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            let key = req.name.clone().unwrap_or_default();
            let val = MEMORY_STORE
                .get(&sandbox_id)
                .and_then(|m| m.short_term.get_context(&key).cloned())
                .unwrap_or(json!(null));
            SocketResponse::ok(Some(json!({ "value": val })))
        }

        "memory_save" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            let json_val = match MEMORY_STORE.get(&sandbox_id) {
                Some(m) => match m.serialize_to_json() {
                    Ok(j) => j,
                    Err(e) => return SocketResponse::err(e),
                },
                None => return SocketResponse::err("no memory found for sandbox"),
            };
            match PERSISTENCE.save_agent_memory(sandbox_id, &json_val) {
                Ok(())  => SocketResponse::ok(None),
                Err(e)  => SocketResponse::err(e),
            }
        }

        "memory_load" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            let json_val = match PERSISTENCE.load_agent_memory(sandbox_id) {
                Ok(j)  => j,
                Err(e) => return SocketResponse::err(e),
            };
            let agent_mem = match AgentMemory::deserialize_from_json(&json_val) {
                Ok(m)  => m,
                Err(e) => return SocketResponse::err(e),
            };
            MEMORY_STORE.insert(sandbox_id, agent_mem);
            SocketResponse::ok(Some(json_val))
        }

        // ── agent coordination ────────────────────────────────────────────────
        "agent_spawn" => {
            let max_iter = req.input.as_ref().and_then(|v| v.as_u64()).unwrap_or(10) as usize;
            let agent_id = COORDINATOR.spawn_agent(max_iter);
            SocketResponse::ok(Some(json!({ "agent": agent_id })))
        }

        "agent_run" => {
            let agent_id = match req.agent {
                Some(id) => id,
                None => return SocketResponse::err("missing agent id"),
            };
            let prompt = req.command.clone().unwrap_or_default();
            let tools: Vec<Box<dyn crate::tools::Tool>> = tool_registry::create_all_tools();
            match COORDINATOR.run_agent(agent_id, &prompt, &tools) {
                Ok(Some(result)) => SocketResponse::ok(Some(json!({ "result": result }))),
                Ok(None) => SocketResponse::err(format!("agent {} not found", agent_id)),
                Err(e) => SocketResponse::err(e),
            }
        }

        "agent_status" => {
            let agent_id = match req.agent {
                Some(id) => id,
                None => return SocketResponse::err("missing agent id"),
            };
            match COORDINATOR.get_agent_status(agent_id) {
                Some(status) => SocketResponse::ok(Some(status)),
                None => SocketResponse::err(format!("agent {} not found", agent_id)),
            }
        }

        // ── create_checkpoint ───────────────────────────────────────────────
        // Creates a checkpoint snapshot of a container's upper layer.
        // This runs in the privileged agentd context, so it can access root-owned files.
        "create_checkpoint" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            let container_id = match req.container.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing container id"),
            };
            let checkpoint_dir = match req.checkpoint_dir.as_ref() {
                Some(d) => std::path::PathBuf::from(d),
                None => return SocketResponse::err("missing checkpoint_dir"),
            };

            match SANDBOXES.get(&sandbox_id) {
                Some(sb) => match sb.checkpoint_container(container_id, &checkpoint_dir) {
                    Ok(_) => SocketResponse::ok(Some(json!({
                        "checkpoint_dir": checkpoint_dir.to_string_lossy().to_string()
                    }))),
                    Err(e) => SocketResponse::err(format!("checkpoint failed: {}", e)),
                }
                None => SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
            }
        }

        // ── restore_checkpoint ──────────────────────────────────────────────
        // Restores a container's upper layer from a checkpoint snapshot.
        // This runs in the privileged agentd context.
        "restore_checkpoint" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            let container_id = match req.container.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing container id"),
            };
            let checkpoint_dir = match req.checkpoint_dir.as_ref() {
                Some(d) => std::path::PathBuf::from(d),
                None => return SocketResponse::err("missing checkpoint_dir"),
            };

            match SANDBOXES.get(&sandbox_id) {
                Some(sb) => match sb.restore_container(container_id, &checkpoint_dir) {
                    Ok(_) => SocketResponse::ok(Some(json!({
                        "restored_from": checkpoint_dir.to_string_lossy().to_string()
                    }))),
                    Err(e) => SocketResponse::err(format!("restore failed: {}", e)),
                }
                None => SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
            }
        }

        other => SocketResponse::err(format!("unknown request type '{}'", other)),
    }
}

// ── Connection handling ───────────────────────────────────────────────────────

struct ParsedConnection {
    stream: UnixStream,
    request: SocketRequest,
}

struct WorkerJob {
    connection: ParsedConnection,
}

fn configure_stream(stream: &UnixStream) -> Result<()> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .context("set read timeout")?;
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(10)))
        .context("set write timeout")?;
    Ok(())
}

fn read_connection_request(mut stream: UnixStream) -> Result<Option<ParsedConnection>> {
    configure_stream(&stream)?;

    let mut reader = BufReader::new(&stream);
    let mut buffer = String::new();

    match reader.read_line(&mut buffer) {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(e) => return Err(e).context("read request"),
    }

    if buffer.trim().is_empty() {
        return Ok(None);
    }

    let request = match serde_json::from_str(&buffer) {
        Ok(r) => r,
        Err(e) => {
            write_socket_response(
                &mut stream,
                &SocketResponse::err(format!("invalid JSON: {}", e)),
            )?;
            return Ok(None);
        }
    };

    Ok(Some(ParsedConnection { stream, request }))
}

fn write_socket_response(stream: &mut UnixStream, response: &SocketResponse) -> Result<()> {
    let text = serde_json::to_string(response).context("serialize response")?;

    let mut bytes_written = 0;
    let bytes = text.as_bytes();
    while bytes_written < bytes.len() {
        match stream.write(&bytes[bytes_written..]) {
            Ok(0) => return Ok(()),
            Ok(n) => bytes_written += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e).context("write response"),
        }
    }

    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

fn process_job(job: WorkerJob) -> Result<()> {
    let WorkerJob { mut connection } = job;
    let response = handle_request(connection.request);
    let result = write_socket_response(&mut connection.stream, &response);
    let _ = connection.stream.shutdown(std::net::Shutdown::Both);
    result
}

fn handle_connection(stream: UnixStream) -> Result<()> {
    if let Some(connection) = read_connection_request(stream)? {
        process_job(WorkerJob { connection })?;
    }
    Ok(())
}

fn spawn_worker_pool(
    name: &str,
    workers: usize,
    receiver: Arc<Mutex<mpsc::Receiver<WorkerJob>>>,
) -> Vec<thread::JoinHandle<()>> {
    (0..workers)
        .map(|idx| {
            let receiver = Arc::clone(&receiver);
            let worker_name = format!("{}-{}", name, idx);
            thread::Builder::new()
                .name(worker_name)
                .spawn(move || loop {
                    let job = {
                        let locked = receiver.lock().unwrap();
                        locked.recv()
                    };
                    match job {
                        Ok(job) => {
                            if let Err(e) = process_job(job) {
                                log::warn!("connection error: {}", e);
                            }
                        }
                        Err(_) => break,
                    }
                })
                .expect("failed to spawn socket worker")
        })
        .collect()
}

fn create_listener(path: &str) -> Result<UnixListener> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path).context("bind unix socket")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o666);
        std::fs::set_permissions(path, perms).context("chmod socket")?;
    }
    Ok(listener)
}

pub fn run_server(path: &str) -> Result<()> {
    std::fs::create_dir_all("/var/log/agentd").ok();
    let _ = PERSISTENCE.init();
    let listener = create_listener(path)?;

    let (fast_tx, fast_rx) = mpsc::channel::<WorkerJob>();
    let (slow_tx, slow_rx) = mpsc::channel::<WorkerJob>();
    let fast_rx = Arc::new(Mutex::new(fast_rx));
    let slow_rx = Arc::new(Mutex::new(slow_rx));

    let _fast_workers = spawn_worker_pool("socket-fast", FAST_WORKERS, Arc::clone(&fast_rx));
    let _slow_workers = spawn_worker_pool("socket-slow", SLOW_WORKERS, Arc::clone(&slow_rx));

    log::info!("Socket server listening on {}", path);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => match read_connection_request(stream) {
                Ok(Some(connection)) => {
                    let lane = classify_request(&connection.request);
                    let job = WorkerJob { connection };
                    let send_result = match lane {
                        RequestLane::Fast => fast_tx.send(job),
                        RequestLane::Slow => slow_tx.send(job),
                    };
                    if let Err(e) = send_result {
                        log::warn!("dispatch error: {}", e);
                    }
                }
                Ok(None) => {}
                Err(e) => log::warn!("connection error: {}", e),
            },
            Err(e) => log::warn!("accept error: {}", e),
        }
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

pub fn run(socket_path: &str) -> Result<()> {
    // Setup logging
    let log_path = std::path::PathBuf::from("/tmp/agentd.log");
    let _ = crate::logging::init(&log_path);

    // Check gcloud - temporarily disabled for testing
    // if !Command::new("gcloud").arg("--version").output().is_ok() {
    //     eprintln!("gcloud CLI not found. Install it and try again.");
    //     std::process::exit(1);
    // }

    // Bind listener
    let listener = UnixListener::bind(socket_path)?;
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o666))?;

    println!("Socket server listening on {}", socket_path);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(|| {
                    if let Err(e) = handle_connection(stream) {
                        eprintln!("Connection error: {}", e);
                    }
                });
            }
            Err(e) => eprintln!("Accept error: {}", e),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::NamedTempFile;

    fn clear_store() {
        SANDBOXES.clear();
    }

    fn create_test_sandbox() -> u64 {
        let resp = handle_request(SocketRequest {
            request_type: "create_sandbox".into(),
            ..Default::default()
        });
        assert_eq!(resp.status, "ok");
        resp.result.unwrap()["sandbox"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
    }

    fn setup_sandbox_with_tool(tool: Box<dyn crate::tools::Tool>) -> u64 {
        let id = create_test_sandbox();
         if let Some(mut sb_ref) = SANDBOXES.get_mut(&id) {
            sb_ref.register_tool(tool);
        }
        id
    }

    #[test]
    fn listener_permission_bits() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        let listener = create_listener(path).expect("bind");
        let metadata = fs::metadata(path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(metadata.permissions().mode() & 0o777, 0o666);
        }
        drop(listener);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_socket_response_ok() {
        let resp = SocketResponse::ok(Some(json!({ "test": "value" })));
        assert_eq!(resp.status, "ok");
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_socket_response_err() {
        let resp = SocketResponse::err("something went wrong");
        assert_eq!(resp.status, "error");
        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_parse_id_string() {
        let v = json!("12345");
        assert_eq!(parse_id(&v), Some(12345u64));
    }

    #[test]
    fn test_parse_id_number() {
        let v = json!(12345u64);
        assert_eq!(parse_id(&v), Some(12345u64));
    }

    #[test]
    fn test_parse_id_invalid() {
        let v = json!("not-a-number");
        assert_eq!(parse_id(&v), None);
    }

    #[test]
    fn create_list_and_destroy() {
        clear_store();
        let resp = handle_request(SocketRequest {
            request_type: "create_sandbox".into(),
            ..Default::default()
        });
        assert_eq!(resp.status, "ok");
        let id_str = resp.result.unwrap()["sandbox"].as_str().unwrap().to_string();
        let id = id_str.parse::<u64>().unwrap();

        // Verify listed
        let list = handle_request(SocketRequest {
            request_type: "list".into(),
            ..Default::default()
        });
        let ids: Vec<String> = serde_json::from_value(list.result.unwrap()).unwrap();
        assert!(ids.contains(&id_str));

        // Destroy by string id
        let destroy = handle_request(SocketRequest {
            request_type: "destroy_sandbox".into(),
            sandbox: Some(json!(id_str)),
            ..Default::default()
        });
        assert_eq!(destroy.status, "ok");

        // Verify gone
        let list2 = handle_request(SocketRequest {
            request_type: "list".into(),
            ..Default::default()
        });
        let ids2: Vec<String> = serde_json::from_value(list2.result.unwrap()).unwrap();
        assert!(!ids2.contains(&id_str));
    }

    #[test]
    fn destroy_by_numeric_id() {
        clear_store();
        let id = create_test_sandbox();
        let resp = handle_request(SocketRequest {
            request_type: "destroy_sandbox".into(),
            sandbox: Some(json!(id)),
            ..Default::default()
        });
        assert_eq!(resp.status, "ok");
    }

    #[test]
    fn unknown_request_returns_error() {
        clear_store();
        let resp = handle_request(SocketRequest {
            request_type: "does_not_exist".into(),
            ..Default::default()
        });
        assert_eq!(resp.status, "error");
    }

    #[test]
    fn invoke_tool_without_container_returns_error() {
        clear_store();
        let id = create_test_sandbox();
        let resp = handle_request(SocketRequest {
            request_type: "invoke_tool".into(),
            sandbox: Some(json!(id.to_string())),
            name: Some("run_command".into()),
            input: Some(json!({ "cmd": "echo hello" })),
            ..Default::default()
        });
        assert_eq!(resp.status, "error");
        assert!(resp.error.unwrap().contains("container"));
    }

    #[test]
    fn test_multiple_sandboxes_unique_ids() {
        clear_store();
        let id1 = create_test_sandbox();
        let id2 = create_test_sandbox();
        let id3 = create_test_sandbox();
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_sandbox_returns_string_id() {
        clear_store();
        let resp = handle_request(SocketRequest {
            request_type: "create_sandbox".into(),
            ..Default::default()
        });
        assert_eq!(resp.status, "ok");
        let result = resp.result.unwrap();
        assert!(result["sandbox"].is_string());
        assert!(result["sandbox"].as_str().unwrap().parse::<u64>().is_ok());
    }

    #[test]
    fn test_missing_sandbox_errors_cleanly() {
        clear_store();
        let resp = handle_request(SocketRequest {
            request_type: "invoke_tool".into(),
            sandbox: Some(json!("99999999")),
            container: Some(json!("88888888")),
            name: Some("run_command".into()),
            input: Some(json!({ "cmd": "echo hi" })),
            ..Default::default()
        });
        assert_eq!(resp.status, "error");
        assert!(resp.error.unwrap().contains("not found"));
    }

    #[test]
    fn test_create_channel() {
        clear_store();
        let sb1 = create_test_sandbox();
        let sb2 = create_test_sandbox();
        let resp = handle_request(SocketRequest {
            request_type: "create_channel".into(),
            sandbox: Some(json!(sb1.to_string())),
            to: Some(sb2),
            ..Default::default()
        });
        assert_eq!(resp.status, "ok");
        assert!(resp.result.unwrap()["channel"].is_number());
    }

    #[test]
    fn test_unknown_policy_errors() {
        clear_store();
        let id = create_test_sandbox();
        let resp = handle_request(SocketRequest {
            request_type: "set_policy".into(),
            sandbox: Some(json!(id.to_string())),
            name: Some("nonexistent_policy".into()),
            ..Default::default()
        });
        assert_eq!(resp.status, "error");
    }
}