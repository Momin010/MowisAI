use anyhow::{Context, Result};
use dashmap::DashMap;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use crate::audit::{AuditEvent, EventType};
use crate::buckets::BucketStore;
use crate::memory::AgentMemory;
use crate::security::SecurityPolicy;
use crate::tool_registry;
use crate::vm_backend::{boot_vm, exec_in_vm, stop_vm, VmHandle};
use crate::{ResourceLimits, Sandbox};

use crate::config::MowisConfig;
use crate::orchestration::complexity_classifier::ComplexityMode;
use crate::orchestration::new_orchestrator::{
    NewOrchestrator, OrchestratorConfig, OrchestratorEvent,
};
use crate::orchestration::provider_client::LlmConfig;

const FAST_WORKERS: usize = 64;
pub(crate) const SLOW_WORKERS: usize = 128;

/// The actual socket path this server is bound to, set at startup.
/// Used by `handle_orchestrate_streaming` to pass the correct path to the
/// orchestrator (instead of relying on the config file which may have a stale default).
static ACTUAL_SOCKET_PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();

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
    #[serde(alias = "type")]
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

    // ── Orchestrate-specific fields ─────────────────────────────────────────
    /// Prompt for orchestrate requests (alias for command)
    pub prompt: Option<String>,
    /// Project path for orchestrate requests (alias for project_root)
    pub project: Option<String>,
    /// Max agents override for orchestrate requests
    pub max_agents: Option<u64>,
    /// Mode override for orchestrate requests (simple/standard/full/auto)
    pub mode: Option<String>,

    // ── Config sync fields ──────────────────────────────────────────────────
    /// Provider name for set_config requests (e.g., "gemini", "vertex_ai", "open_ai")
    pub provider: Option<String>,
    /// Model name for set_config requests
    pub model: Option<String>,
    /// Plaintext API key for set_config requests (agentd encrypts before saving)
    pub api_key: Option<String>,
    /// GCP project ID for set_config requests (Vertex AI)
    pub gcp_project_id: Option<String>,

    // ── Extra fields from desktop ───────────────────────────────────────────
    /// Repository source (e.g., "local", "github")
    pub repo_source: Option<String>,
    /// Repository URL (for GitHub repos)
    pub repo_url: Option<String>,
    /// Git workflow policy for agents
    pub git_policy: Option<String>,
    /// Conversation history from the desktop for context-aware responses.
    /// Each entry has "role" ("user"/"assistant") and "content".
    pub conversation_history: Option<Vec<Value>>,
}

#[derive(Debug, Serialize)]
pub struct SocketResponse {
    pub status: String,
    pub result: Option<Value>,
    pub error: Option<String>,
}

impl SocketResponse {
    fn ok(result: Option<Value>) -> Self {
        SocketResponse {
            status: "ok".into(),
            result,
            error: None,
        }
    }
    fn err<E: ToString>(e: E) -> Self {
        SocketResponse {
            status: "error".into(),
            result: None,
            error: Some(e.to_string()),
        }
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

    // CRITICAL FIX: Read stdout and stderr CONCURRENTLY to prevent deadlock.
    // Sequential reading causes pipe buffer deadlock when one stream fills up.
    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();

    let stdout_thread = stdout_handle.map(|stdout| {
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().flatten() {
                log::info!("[sandbox] {}", line);
            }
        })
    });

    let stderr_thread = stderr_handle.map(|stderr| {
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().flatten() {
                log::warn!("[sandbox] {}", line);
            }
        })
    });

    // Wait for both reader threads to finish
    if let Some(t) = stdout_thread {
        let _ = t.join();
    }
    if let Some(t) = stderr_thread {
        let _ = t.join();
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
        "git",
        "curl",
        "wget",
        "bash",
        "ca-certificates",
        "openssh-client",
        // runtimes
        "python3",
        "py3-pip",
        "nodejs",
        "npm",
        // container stack (guest OS requirement)
        // Note: exact package names vary by distro; we resolve per distro below where needed.
        "docker",
        "containerd",
        "runc",
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
        "/etc/ssl/certs/ca-certificates.crt", // Debian/Ubuntu bundle
        "/etc/pki/tls/certs/ca-bundle.crt",   // RedHat/CentOS bundle
        "/etc/ssl/certs/ca-bundle.crt",       // Alpine alternative
    ];

    let ca_dest = root.join("etc/ssl/certs");
    std::fs::create_dir_all(&ca_dest).ok();

    // Try to copy the first available CA bundle
    for src in &ca_src_paths {
        if std::path::Path::new(src).exists() {
            let dest = ca_dest.join("ca-certificates.crt");
            let _ = std::fs::copy(src, &dest);
            if dest.exists() {
                break; // Successfully copied, stop trying other paths
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

    chroot_run_streaming(root, &install_cmd).context("package installation failed")?;

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
        "create_sandbox" | "list" | "get_audit_stats" | "get_anomalies" | "agent_spawn"
        | "orchestrate" | "chat" | "set_config" | "get_config" => Ok(()),
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
        "list_containers" | "get_policy" | "register_tool" | "bucket_put" | "bucket_get"
        | "memory_set" | "memory_get" | "memory_save" | "memory_load" | "create_channel" => {
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
        "list" | "list_containers" | "get_policy" | "get_audit_stats" | "get_anomalies"
        | "agent_status" | "bucket_get" | "memory_get" | "memory_load" | "read_messages" => {
            RequestLane::Fast
        }
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
            let image = req.image.clone(); // None means plain tmpfs (no skopeo needed)
            let backend = req.backend.clone().unwrap_or_else(|| "chroot".to_string());
            let limits = ResourceLimits {
                ram_bytes: req.ram,
                cpu_millis: req.cpu,
            };
            let seed_repo_url = req.seed_repo_url.clone();
            let seed_repo_branch = req.seed_repo_branch.clone();
            let seed_repo_subdir = req.seed_repo_subdir.clone();

            let mut sb = match Sandbox::new_with_image(limits, image.as_deref()) {
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
            let image_label = image.as_deref().unwrap_or("(none)");

            log::info!("sandbox created: {}", id);
            log::info!(
                "[agentd] Setting up sandbox {} with image '{}'",
                id,
                image_label
            );

            if let Err(e) = install_packages_in_image(&root, image_label, extra) {
                log::warn!("sandbox {} package install warning: {}", id, e);
                // Non-fatal: continue even if some optional packages failed.
                // Core failures will be caught when the first tool runs.
            }

            // Optional: seed a repository into sandbox baseline so all containers share it.
            if let Some(repo_url) = seed_repo_url.as_ref() {
                log::info!("[agentd] Seeding repo {} into sandbox {} ...", repo_url, id);
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
                            log::warn!(
                                "sandbox {} scope path does not exist: {}, using full project",
                                id,
                                scoped_path.display()
                            );
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
                            nix::mount::MsFlags::MS_BIND | nix::mount::MsFlags::MS_RDONLY, // Read-only mount for safety
                            None::<&str>,
                        ) {
                            log::warn!("sandbox {} failed to bind-mount project root: {}", id, e);
                        } else {
                            let scope_info = req
                                .scope
                                .as_ref()
                                .map(|s| format!(" (scope: {})", s))
                                .unwrap_or_default();
                            log::info!(
                                "[agentd] Mounted {} into sandbox {} /workspace{}",
                                mount_source.display(),
                                id,
                                scope_info
                            );
                        }
                    }
                } else {
                    log::warn!(
                        "sandbox {} project_root does not exist: {}",
                        id,
                        project_root
                    );
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
                match boot_vm(id.to_string(), &root, image.as_deref().unwrap_or("alpine")) {
                    Ok(handle) => {
                        VM_HANDLES.insert(id, handle);
                        log::info!(
                            "[agentd] guest_vm QEMU boot pid={} sandbox={} port=?",
                            id,
                            id
                        );
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
            SocketResponse::ok(Some(
                json!({ "sandbox": id.to_string(), "backend": backend }),
            ))
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
                    Err(e) => {
                        return SocketResponse::err(format!("create_container failed: {}", e))
                    }
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
                None => {
                    return SocketResponse::err(
                        "missing container id — create one first with create_container",
                    )
                }
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
                            Err(e) => {
                                return SocketResponse::err(format!(
                                    "vm tool {} failed: {}",
                                    name, e
                                ))
                            }
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
            let ids: Vec<String> = SANDBOXES
                .iter()
                .map(|item| item.key().to_string())
                .collect();
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
                    let cids: Vec<String> = sb
                        .list_containers()
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
                },
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
                let backend = SANDBOX_BACKENDS
                    .get(&id)
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
                Some(tool) => match SANDBOXES.get_mut(&sandbox_id) {
                    Some(mut sb_ref) => {
                        sb_ref.register_tool(tool);
                        SocketResponse::ok(None)
                    }
                    None => SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
                },
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
                "permissive" => SecurityPolicy::default_permissive(),
                other => {
                    return SocketResponse::err(format!(
                        "unknown policy '{}' — use 'restrictive' or 'permissive'",
                        other
                    ))
                }
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
        "get_anomalies" => SocketResponse::ok(Some(AUDITOR.detect_anomalies())),

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
                crate::channels::Message {
                    from,
                    to: 0,
                    payload,
                },
            ) {
                Ok(_) => SocketResponse::ok(None),
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
            let key = req.name.clone().unwrap_or_default();
            let value = req.command.clone().unwrap_or_default();

            let bucket_path = match SANDBOXES.get(&sandbox_id) {
                Some(sb) => sb.root_path().join("buckets"),
                None => return SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
            };
            match BucketStore::new(bucket_path) {
                Ok(mut bs) => match bs.put(&key, &value) {
                    Ok(()) => SocketResponse::ok(None),
                    Err(e) => SocketResponse::err(e),
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
                    Ok(None) => SocketResponse::err("key not found"),
                    Err(e) => SocketResponse::err(e),
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
            let key = req.name.clone().unwrap_or_default();
            let value = req.input.clone().unwrap_or(json!(null));
            MEMORY_STORE
                .entry(sandbox_id)
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
                Ok(()) => SocketResponse::ok(None),
                Err(e) => SocketResponse::err(e),
            }
        }

        "memory_load" => {
            let sandbox_id = match req.sandbox.as_ref().and_then(parse_id) {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox id"),
            };
            let json_val = match PERSISTENCE.load_agent_memory(sandbox_id) {
                Ok(j) => j,
                Err(e) => return SocketResponse::err(e),
            };
            let agent_mem = match AgentMemory::deserialize_from_json(&json_val) {
                Ok(m) => m,
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
                },
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
                },
                None => SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
            }
        }

        // ── set_config ─────────────────────────────────────────────────────
        // Sync provider settings from the desktop app to agentd's config.toml.
        // Accepts plaintext API key and encrypts it before saving.
        "set_config" => {
            let mut config = match MowisConfig::load() {
                Ok(Some(c)) => c,
                Ok(None) => MowisConfig::default(),
                Err(e) => return SocketResponse::err(format!("load config: {}", e)),
            };

            if let Some(ref provider_str) = req.provider {
                match serde_json::from_str::<crate::config::AiProvider>(&format!("\"{}\"", provider_str)) {
                    Ok(p) => config.provider = p,
                    Err(_) => return SocketResponse::err(format!("invalid provider: {}", provider_str)),
                }
            }

            if let Some(ref model) = req.model {
                config.model = model.clone();
                match config.provider {
                    crate::config::AiProvider::Gemini => config.gemini_model = model.clone(),
                    crate::config::AiProvider::OpenAi => config.openai_model = model.clone(),
                    crate::config::AiProvider::Anthropic => config.anthropic_model = model.clone(),
                    crate::config::AiProvider::Grok => config.grok_model = model.clone(),
                    crate::config::AiProvider::Groq => config.groq_model = model.clone(),
                    crate::config::AiProvider::Mimo => config.mimo_model = model.clone(),
                    crate::config::AiProvider::VertexAi => {}
                }
            }

            if let Some(ref project_id) = req.gcp_project_id {
                config.gcp_project_id = project_id.clone();
            }

            if let Some(ref api_key) = req.api_key {
                if !api_key.is_empty() {
                    match crate::crypto::encrypt(api_key) {
                        Ok(encrypted) => {
                            match config.provider {
                                crate::config::AiProvider::Gemini => config.gemini_api_key_enc = Some(encrypted),
                                crate::config::AiProvider::OpenAi => config.openai_api_key_enc = Some(encrypted),
                                crate::config::AiProvider::Anthropic => config.anthropic_api_key_enc = Some(encrypted),
                                crate::config::AiProvider::Grok => config.grok_api_key_enc = Some(encrypted),
                                crate::config::AiProvider::Groq => config.groq_api_key_enc = Some(encrypted),
                                crate::config::AiProvider::Mimo => config.mimo_api_key_enc = Some(encrypted),
                                crate::config::AiProvider::VertexAi => {}
                            }
                        }
                        Err(e) => return SocketResponse::err(format!("encrypt api_key: {}", e)),
                    }
                }
            }

            match config.save() {
                Ok(()) => SocketResponse::ok(Some(json!({
                    "provider": format!("{}", config.provider),
                    "model": config.model,
                    "config_path": MowisConfig::config_path().display().to_string(),
                }))),
                Err(e) => SocketResponse::err(format!("save config: {}", e)),
            }
        }

        // ── get_config ─────────────────────────────────────────────────────
        "get_config" => {
            match MowisConfig::load() {
                Ok(Some(config)) => {
                    SocketResponse::ok(Some(json!({
                        "provider": format!("{}", config.provider),
                        "model": config.model,
                        "gcp_project_id": config.gcp_project_id,
                        "has_api_key": match config.provider {
                            crate::config::AiProvider::VertexAi => true,
                            crate::config::AiProvider::Gemini => config.gemini_api_key_enc.is_some(),
                            crate::config::AiProvider::OpenAi => config.openai_api_key_enc.is_some(),
                            crate::config::AiProvider::Anthropic => config.anthropic_api_key_enc.is_some(),
                            crate::config::AiProvider::Grok => config.grok_api_key_enc.is_some(),
                            crate::config::AiProvider::Groq => config.groq_api_key_enc.is_some(),
                            crate::config::AiProvider::Mimo => config.mimo_api_key_enc.is_some(),
                        },
                        "socket_path": config.socket_path,
                        "max_agents": config.max_agents,
                        "version": crate::version::VERSION,
                        "build_number": crate::version::BUILD_NUMBER,
                    })))
                }
                Ok(None) => SocketResponse::ok(Some(json!({"configured": false}))),
                Err(e) => SocketResponse::err(format!("load config: {}", e)),
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

// ── Orchestrate streaming handler ─────────────────────────────────────────────

/// Convert an OrchestratorEvent to a JSON value that the desktop bridge understands.
fn orchestrator_event_to_json(event: &OrchestratorEvent) -> Option<serde_json::Value> {
    match event {
        OrchestratorEvent::TaskStarted { task_id, description, sandbox, .. } => {
            Some(json!({
                "type": "task_added",
                "id": task_id,
                "description": description,
                "sandbox": sandbox,
                "status": "running"
            }))
        }
        OrchestratorEvent::TaskCompleted { task_id, success, diff_size, .. } => {
            Some(json!({
                "type": "task_updated",
                "id": task_id,
                "status": if *success { "complete" } else { "failed" },
                "diff_size": diff_size
            }))
        }
        OrchestratorEvent::TaskFailed { task_id, error, .. } => {
            Some(json!({
                "type": "task_updated",
                "id": task_id,
                "status": "failed",
                "error": error
            }))
        }
        OrchestratorEvent::LayerProgress { layer, message } => {
            Some(json!({
                "type": "layer_progress",
                "layer": layer,
                "message": message
            }))
        }
        OrchestratorEvent::StatsUpdate { stats } => {
            Some(json!({
                "type": "stats",
                "total": stats.total_tasks,
                "completed": stats.completed,
                "failed": stats.failed,
                "running": stats.running,
                "pending": stats.pending
            }))
        }
        OrchestratorEvent::ToolCall { worker_id, tool_name, args_preview } => {
            Some(json!({
                "type": "tool_call",
                "worker_id": worker_id,
                "tool_name": tool_name,
                "args_preview": args_preview
            }))
        }
        OrchestratorEvent::ToolResult { worker_id, tool_name, success, preview } => {
            Some(json!({
                "type": "tool_result",
                "worker_id": worker_id,
                "tool_name": tool_name,
                "success": success,
                "preview": preview
            }))
        }
        OrchestratorEvent::ChatResponse { text } => {
            Some(json!({
                "type": "agent_message",
                "content": text
            }))
        }
        OrchestratorEvent::LlmThinking { agent_id, task_description } => {
            Some(json!({
                "type": "llm_thinking",
                "agent_id": agent_id,
                "task_description": task_description
            }))
        }
        OrchestratorEvent::LlmChunk { agent_id, chunk } => {
            Some(json!({
                "type": "agent_chunk",
                "agent_id": agent_id,
                "content": chunk
            }))
        }
        OrchestratorEvent::SandboxCreated { name, agent_count } => {
            Some(json!({
                "type": "sandbox_created",
                "name": name,
                "agent_count": agent_count
            }))
        }
        OrchestratorEvent::AgentStatusChanged { agent_id, task_id, status, sandbox } => {
            Some(json!({
                "type": "agent_status",
                "agent_id": agent_id,
                "task_id": task_id,
                "status": status,
                "sandbox": sandbox
            }))
        }
        OrchestratorEvent::Done => None, // Handled separately
    }
}

/// Handle an orchestrate request with streaming responses.
/// Unlike regular requests that return one response, this keeps the connection
/// open and streams JSON events as the orchestrator progresses.
fn handle_orchestrate_streaming(
    mut stream: UnixStream,
    req: SocketRequest,
) -> Result<()> {
    // Clear the short timeouts set by configure_stream() — orchestration is
    // long-lived and the planner LLM call alone can take 30+ seconds.
    let _ = stream.set_read_timeout(None);
    let _ = stream.set_write_timeout(None);

    // Extract parameters from the request
    let prompt = req.prompt
        .or(req.command)
        .unwrap_or_default();
    let project_root = req.project
        .or(req.project_root)
        .unwrap_or_else(|| ".".to_string());
    let max_agents = req.max_agents
        .or_else(|| req.input.as_ref().and_then(|v| v.as_u64()))
        .unwrap_or(50) as usize;
    let mode_str = req.mode
        .or(req.name)
        .unwrap_or_else(|| "auto".to_string());

    log::info!("[orchestrate] Starting orchestration: prompt='{}', project='{}', max_agents={}, mode='{}'",
        prompt.chars().take(60).collect::<String>(), project_root, max_agents, mode_str);

    // Load config from disk
    let config = match MowisConfig::load() {
        Ok(Some(c)) => c,
        Ok(None) => {
            let err_resp = json!({"type": "error", "message": "No mowisai config found. Run setup first."});
            write_socket_json(&mut stream, &err_resp)?;
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return Ok(());
        }
        Err(e) => {
            let err_resp = json!({"type": "error", "message": format!("Failed to load config: {}", e)});
            write_socket_json(&mut stream, &err_resp)?;
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return Ok(());
        }
    };

    // Build LLM config
    let llm_config = match LlmConfig::from_config(&config) {
        Ok(c) => c,
        Err(e) => {
            let err_resp = json!({"type": "error", "message": format!("LLM config error: {}", e)});
            write_socket_json(&mut stream, &err_resp)?;
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return Ok(());
        }
    };

    // Parse mode override
    let mode_override = match mode_str.as_str() {
        "simple" => Some(ComplexityMode::Simple),
        "standard" => Some(ComplexityMode::Standard),
        "full" => Some(ComplexityMode::Full),
        _ => None, // "auto" or anything else → let classifier decide
    };

    // Create orchestrator config — use the actual socket path this server is
    // bound to, not the config file value (which may have a stale default).
    let actual_socket = ACTUAL_SOCKET_PATH
        .get()
        .cloned()
        .unwrap_or_else(|| config.socket_path.clone());
    let project = std::path::PathBuf::from(&project_root);
    let orchestrator_config = OrchestratorConfig {
        llm_config,
        socket_path: actual_socket,
        project_root: project.clone(),
        overlay_root: std::path::PathBuf::from(&config.overlay_root),
        checkpoint_root: std::path::PathBuf::from(&config.checkpoint_root),
        merge_work_dir: std::path::PathBuf::from(&config.merge_work_dir),
        max_agents,
        max_verification_rounds: 3,
        staging_dir: None,
        event_tx: None, // Will be set below
        mode_override,
    };

    // Create tokio runtime for the async orchestrator
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            let err_resp = json!({"type": "error", "message": format!("Failed to create runtime: {}", e)});
            write_socket_json(&mut stream, &err_resp)?;
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return Ok(());
        }
    };

    // Create event channel (std::sync for cross-thread, bridged to tokio below)
    let (event_tx, event_rx) = std::sync::mpsc::channel::<OrchestratorEvent>();

    // Set the event sender on the config
    let mut orchestrator_config = orchestrator_config;
    orchestrator_config.event_tx = Some(event_tx);

    // Separate channel for reporting orchestrator errors back to the streaming loop,
    // so the error message can be sent to the desktop before the connection closes.
    let (err_report_tx, err_report_rx) = std::sync::mpsc::channel::<String>();

    // Run orchestrator in a separate thread, stream events back on this thread
    let orchestrator_thread = {
        let prompt = prompt.clone();
        std::thread::Builder::new()
            .name("orchestrator-worker".into())
            .spawn(move || {
                rt.block_on(async move {
                    let orchestrator = NewOrchestrator::new(orchestrator_config);
                    match orchestrator.run(&prompt).await {
                        Ok(output) => {
                            log::info!("[orchestrate] Completed: {} agents, {}s",
                                output.total_agents_used, output.total_duration_secs);
                        }
                        Err(e) => {
                            let msg = format!("{:#}", e);
                            log::error!("[orchestrate] Failed: {}", msg);
                            let _ = err_report_tx.send(msg);
                        }
                    }
                });
            })
    };

    // Stream events from the orchestrator to the socket client
    loop {
        match event_rx.recv() {
            Ok(event) => {
                let is_done = matches!(event, OrchestratorEvent::Done);

                if let Some(json_val) = orchestrator_event_to_json(&event) {
                    if let Err(e) = write_socket_json(&mut stream, &json_val) {
                        log::warn!("[orchestrate] Failed to write event: {}", e);
                        break;
                    }
                }

                if is_done {
                    break;
                }
            }
            Err(_) => {
                // Channel closed — orchestrator finished (or crashed).
                // Check if there was an error message.
                if let Ok(err_msg) = err_report_rx.try_recv() {
                    let err_json = json!({"type": "error", "message": err_msg});
                    let _ = write_socket_json(&mut stream, &err_json);
                }
                log::info!("[orchestrate] Event channel closed");
                break;
            }
        }
    }

    // Wait for orchestrator thread to finish
    if let Ok(thread) = orchestrator_thread {
        let _ = thread.join();
    }

    // Send completion event
    let complete = json!({"type": "complete"});
    let _ = write_socket_json(&mut stream, &complete);

    // Close connection
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

/// Handle a chat request — direct multi-turn LLM call, no orchestration.
/// Returns a single JSON response: {"type": "chat_response", "content": "..."} or
/// {"type": "delegate_build", "build_instructions": "...", "summary_for_user": "..."}
/// when the LLM determines the user wants to build something.
fn handle_chat_streaming(
    mut stream: UnixStream,
    req: SocketRequest,
) -> Result<()> {
    let _ = stream.set_read_timeout(None);
    let _ = stream.set_write_timeout(None);

    let history = req.conversation_history.unwrap_or_default();

    let config = match MowisConfig::load() {
        Ok(Some(c)) => c,
        Ok(None) => {
            let err = json!({"type": "error", "message": "No config found"});
            write_socket_json(&mut stream, &err)?;
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return Ok(());
        }
        Err(e) => {
            let err = json!({"type": "error", "message": format!("Config error: {}", e)});
            write_socket_json(&mut stream, &err)?;
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return Ok(());
        }
    };

    let llm_config = match LlmConfig::from_config(&config) {
        Ok(c) => c,
        Err(e) => {
            let err = json!({"type": "error", "message": format!("LLM config error: {}", e)});
            write_socket_json(&mut stream, &err)?;
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return Ok(());
        }
    };

    let system_prompt = r#"You are MowisAI, an AI assistant that can both chat and build software.

IMPORTANT RULES:
1. For normal conversation (greetings, questions, opinions, etc.) — respond naturally and directly.
2. When the user asks you to BUILD, CREATE, IMPLEMENT, CODE, FIX, or DEPLOY something that requires writing files or running commands — you MUST respond with EXACTLY this JSON format and nothing else:

{"action": "build", "instructions": "<detailed instructions for the coding agent describing exactly what to build, including all requirements the user mentioned in the conversation>", "summary": "<brief 1-sentence acknowledgment to show the user before building starts>"}

3. When responding normally (not building), just respond with plain text. Do NOT wrap normal responses in JSON.
4. You remember the full conversation. Reference previous messages when relevant.
5. Never use emojis."#;

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            let err = json!({"type": "error", "message": format!("Runtime error: {}", e)});
            write_socket_json(&mut stream, &err)?;
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return Ok(());
        }
    };

    let result = rt.block_on(async {
        crate::orchestration::provider_client::generate_chat(
            &llm_config,
            system_prompt,
            &history,
            0.7,
        ).await
    });

    match result {
        Ok(response_text) => {
            let trimmed = response_text.trim();
            if trimmed.starts_with('{') {
                if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                    if parsed.get("action").and_then(|a| a.as_str()) == Some("build") {
                        let instructions = parsed["instructions"].as_str().unwrap_or("").to_string();
                        let summary = parsed["summary"].as_str().unwrap_or("Starting build...").to_string();
                        let resp = json!({
                            "type": "delegate_build",
                            "build_instructions": instructions,
                            "summary_for_user": summary
                        });
                        write_socket_json(&mut stream, &resp)?;
                        let _ = stream.shutdown(std::net::Shutdown::Both);
                        return Ok(());
                    }
                }
            }

            let resp = json!({
                "type": "chat_response",
                "content": response_text
            });
            write_socket_json(&mut stream, &resp)?;
        }
        Err(e) => {
            let err = json!({"type": "error", "message": format!("LLM error: {}", e)});
            write_socket_json(&mut stream, &err)?;
        }
    }

    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

/// Write a JSON value followed by newline to the socket stream.
fn write_socket_json(stream: &mut UnixStream, value: &serde_json::Value) -> Result<()> {
    let text = serde_json::to_string(value).context("serialize JSON")?;
    stream.write_all(text.as_bytes()).context("write JSON")?;
    stream.write_all(b"\n").context("write newline")?;
    stream.flush().context("flush")?;
    Ok(())
}

fn process_job(job: WorkerJob) -> Result<()> {
    let WorkerJob { mut connection } = job;

    // Orchestrate requests use streaming: keep connection open, send multiple JSON events
    if connection.request.request_type == "orchestrate" {
        return handle_orchestrate_streaming(connection.stream, connection.request);
    }

    // Chat requests: direct LLM call, no orchestration
    if connection.request.request_type == "chat" {
        return handle_chat_streaming(connection.stream, connection.request);
    }

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

/// Shared work queue that supports multiple concurrent consumers
struct WorkQueue {
    queue: Mutex<VecDeque<WorkerJob>>,
    condvar: Condvar,
    shutdown: AtomicBool,
}

impl WorkQueue {
    fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            condvar: Condvar::new(),
            shutdown: AtomicBool::new(false),
        }
    }

    fn push(&self, job: WorkerJob) {
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        q.push_back(job);
        self.condvar.notify_one();
    }

    fn pop(&self) -> Option<WorkerJob> {
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(job) = q.pop_front() {
                return Some(job);
            }
            if self.shutdown.load(Ordering::Relaxed) {
                return None;
            }
            q = self.condvar.wait(q).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        self.condvar.notify_all();
    }
}

fn spawn_worker_pool(
    name: &str,
    workers: usize,
    queue: Arc<WorkQueue>,
) -> Vec<thread::JoinHandle<()>> {
    (0..workers)
        .map(|idx| {
            let queue = Arc::clone(&queue);
            let worker_name = format!("{}-{}", name, idx);
            thread::Builder::new()
                .name(worker_name)
                .spawn(move || loop {
                    match queue.pop() {
                        Some(job) => {
                            if let Err(e) = process_job(job) {
                                log::warn!("connection error: {}", e);
                            }
                        }
                        None => break, // Shutdown signal
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
    let _ = ACTUAL_SOCKET_PATH.set(path.to_string());
    let listener = create_listener(path)?;

    let fast_queue = Arc::new(WorkQueue::new());
    let slow_queue = Arc::new(WorkQueue::new());

    let _fast_workers = spawn_worker_pool("socket-fast", FAST_WORKERS, Arc::clone(&fast_queue));
    let _slow_workers = spawn_worker_pool("socket-slow", SLOW_WORKERS, Arc::clone(&slow_queue));

    log::info!("Socket server listening on {}", path);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => match read_connection_request(stream) {
                Ok(Some(connection)) => {
                    let lane = classify_request(&connection.request);
                    let job = WorkerJob { connection };
                    match lane {
                        RequestLane::Fast => fast_queue.push(job),
                        RequestLane::Slow => slow_queue.push(job),
                    };
                }
                Ok(None) => {}
                Err(e) => log::warn!("connection error: {}", e),
            },
            Err(e) => log::warn!("accept error: {}", e),
        }
    }

    // Signal workers to shut down
    fast_queue.shutdown();
    slow_queue.shutdown();

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

pub fn run(socket_path: &str) -> Result<()> {
    // Setup logging
    let log_path = std::path::PathBuf::from("/tmp/agentd.log");
    let _ = crate::logging::init(&log_path);

    // Store the actual socket path so handle_orchestrate_streaming can use it
    // instead of relying on the config file's socket_path field.
    let _ = ACTUAL_SOCKET_PATH.set(socket_path.to_string());

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
        let id_str = resp.result.unwrap()["sandbox"]
            .as_str()
            .unwrap()
            .to_string();
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
