//! Layer 2: Overlayfs Topology — Delegates to agentd socket API
//!
//! This layer coordinates with agentd (via socket) to create:
//! - Sandboxes (agentd creates overlayfs base + package layer)
//! - Containers (agentd creates overlayfs CoW layer per agent)
//!
//! The actual overlayfs mounts are done by agentd (running as root),
//! not by this orchestrator code.

use agentd_protocol::{AgentHandle, LayerLevel, OverlayfsLayer, SandboxConfig, SandboxName, TaskId};
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Sleeping container that can be reused
#[derive(Debug, Clone)]
pub struct SleepingContainer {
    pub agent_id: String,
    pub container_id: String,
    pub sandbox_id: String,
    pub paused_at: u64,
}

/// Staged workspace export before container destruction
#[derive(Debug, Clone)]
pub struct StagedWorkspace {
    pub agent_id: String,
    pub sandbox_name: String,
    pub container_id: String,
    pub staging_path: PathBuf,
}

/// Topology manager - coordinates with agentd socket
#[derive(Debug)]
pub struct TopologyManager {
    /// Socket path for agentd communication
    socket_path: String,

    /// Created sandbox IDs (sandbox_name -> sandbox_id)
    sandboxes: Arc<RwLock<HashMap<SandboxName, String>>>,

    /// Sandbox scope paths (sandbox_name -> scope)
    sandbox_scopes: Arc<RwLock<HashMap<SandboxName, String>>>,

    /// Created container IDs (agent_id -> container_id)
    containers: Arc<RwLock<HashMap<String, String>>>,

    /// Agent sandbox mapping (agent_id -> sandbox_name)
    agent_sandboxes: Arc<RwLock<HashMap<String, SandboxName>>>,

    /// Project root path (for reference)
    project_root: PathBuf,

    /// Staged workspaces for save-all (exported before container destruction)
    staged_workspaces: Arc<RwLock<Vec<StagedWorkspace>>>,

    /// Pool of sleeping containers available for reuse (sandbox_name -> containers)
    sleeping_containers: Arc<RwLock<HashMap<SandboxName, Vec<SleepingContainer>>>>,
}

impl TopologyManager {
    /// Create new topology manager
    pub fn new(project_root: PathBuf, socket_path: String) -> Result<Self> {
        if !project_root.exists() {
            return Err(anyhow!(
                "Project root does not exist: {:?}",
                project_root
            ));
        }

        Ok(Self {
            socket_path,
            sandboxes: Arc::new(RwLock::new(HashMap::new())),
            sandbox_scopes: Arc::new(RwLock::new(HashMap::new())),
            containers: Arc::new(RwLock::new(HashMap::new())),
            agent_sandboxes: Arc::new(RwLock::new(HashMap::new())),
            project_root,
            staged_workspaces: Arc::new(RwLock::new(Vec::new())),
            sleeping_containers: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Get socket path (for tests/mocks)
    pub fn socket_path(&self) -> &str {
        &self.socket_path
    }

    /// Create sandbox via agentd socket API
    pub async fn create_sandbox_layer(&self, config: &SandboxConfig) -> Result<()> {
        // Call agentd to create sandbox with project root mounted.
        let mut request = json!({
            "request_type": "create_sandbox",
            "packages": config.tools.iter()
                .filter_map(|tool| self.tool_to_package(tool))
                .collect::<Vec<_>>(),
            "project_root": self.project_root.to_string_lossy(),
            "scope": config.scope.clone(),
        });

        // Include the "image" field if the sandbox config specifies one.
        // The socket server's SocketRequest uses the field name "image".
        if let Some(ref img) = config.image {
            request["image"] = serde_json::Value::String(img.clone());
        }

        let socket_path = self.socket_path.clone();
        let response = tokio::task::spawn_blocking(move || {
            super::pooled_socket_request(&socket_path, &request)
        }).await.context("spawn_blocking socket_roundtrip")??;

        // Extract sandbox ID from response
        let sandbox_id = super::parse_ok_field(&response, "sandbox")?;

        // Store mapping
        let mut sandboxes = self.sandboxes.write().await;
        sandboxes.insert(config.name.clone(), sandbox_id.clone());
        drop(sandboxes);

        let mut scopes = self.sandbox_scopes.write().await;
        scopes.insert(config.name.clone(), config.scope.clone());
        drop(scopes);

        log::info!("  → Created sandbox: {} (id: {}) with scope: {}",
                 config.name, sandbox_id, config.scope);

        Ok(())
    }

    /// Create agent layer (container) via agentd socket API
    pub async fn create_agent_layer(
        &self,
        sandbox_name: &SandboxName,
        task_id: Option<TaskId>,
    ) -> Result<AgentHandle> {
        // Get sandbox ID
        let sandboxes = self.sandboxes.read().await;
        let sandbox_id = sandboxes
            .get(sandbox_name)
            .ok_or_else(|| anyhow!("Sandbox not found: {}", sandbox_name))?
            .clone();
        drop(sandboxes);

        // Create container via socket
        let request = json!({
            "request_type": "create_container",
            "sandbox": sandbox_id,
        });

        let socket_path = self.socket_path.clone();
        let response = tokio::task::spawn_blocking(move || {
            super::pooled_socket_request(&socket_path, &request)
        }).await.context("spawn_blocking socket_roundtrip")??;

        // Extract container ID
        let container_id = super::parse_ok_field(&response, "container")?;

        let agent_id = Uuid::new_v4().to_string();

        // Store mapping
        let mut containers = self.containers.write().await;
        containers.insert(agent_id.clone(), container_id.clone());
        drop(containers);

        let mut agent_sandboxes = self.agent_sandboxes.write().await;
        agent_sandboxes.insert(agent_id.clone(), sandbox_name.clone());
        drop(agent_sandboxes);

        // Initialize git repo in /workspace before agent starts
        // This creates the "base" commit that diffs will be taken against
        log::info!("  → Initializing git repo in container {}", &container_id[..8]);

        // Step 1: git init
        let init_request = json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "name": "run_command",
            "input": {
                "cmd": "cd /workspace && git init",
                "timeout": 10
            }
        });
        let socket_path = self.socket_path.clone();
        let init_request = init_request.clone();
        tokio::task::spawn_blocking(move || {
            super::pooled_socket_request(&socket_path, &init_request)
        }).await.context("spawn_blocking socket_roundtrip")??;

        // Step 2: git config (needed for commits)
        let config_request = json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "name": "run_command",
            "input": {
                "cmd": "cd /workspace && git config user.email 'agent@mowis.ai' && git config user.name 'MowisAI Agent'",
                "timeout": 10
            }
        });
        let socket_path = self.socket_path.clone();
        let config_request = config_request.clone();
        tokio::task::spawn_blocking(move || {
            super::pooled_socket_request(&socket_path, &config_request)
        }).await.context("spawn_blocking socket_roundtrip")??;

        // Step 3: git add -A (stage everything that already exists)
        let add_request = json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "name": "run_command",
            "input": {
                "cmd": "cd /workspace && git add -A",
                "timeout": 10
            }
        });
        let socket_path = self.socket_path.clone();
        let add_request = add_request.clone();
        tokio::task::spawn_blocking(move || {
            super::pooled_socket_request(&socket_path, &add_request)
        }).await.context("spawn_blocking socket_roundtrip")??;

        // Step 4: create a base commit so HEAD exists.
        // capture_git_diff() uses `git diff --cached HEAD` — without a HEAD
        // (i.e., a repo with no commits) that command silently returns nothing,
        // causing every agent to produce an empty diff and Layer 5/6 to be skipped.
        // `--allow-empty` handles the case where /workspace has no files yet.
        let commit_request = json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "name": "run_command",
            "input": {
                "cmd": "cd /workspace && (git rev-parse HEAD 2>/dev/null || git commit --allow-empty -m 'base')",
                "timeout": 15
            }
        });
        let socket_path = self.socket_path.clone();
        let commit_request = commit_request.clone();
        tokio::task::spawn_blocking(move || {
            super::pooled_socket_request(&socket_path, &commit_request)
        }).await.context("spawn_blocking socket_roundtrip")??;

        log::info!("  → Git repo initialized with base commit");

        // Create AgentHandle
        // IMPORTANT: We use sandbox_id as sandbox_name here because tools need the numeric ID
        // The actual sandbox name is stored separately if needed
        let handle = AgentHandle {
            agent_id: agent_id.clone(),
            sandbox_name: sandbox_id.clone(), // Use ID, not name!
            container_id: container_id.clone(),
            task_id,
            layer: OverlayfsLayer {
                level: LayerLevel::Agent,
                mount_path: format!("/sandbox/{}/container/{}", sandbox_id, container_id),
                upper_dir: format!("/sandbox/{}/container/{}/upper", sandbox_id, container_id),
                work_dir: format!("/sandbox/{}/container/{}/work", sandbox_id, container_id),
                lower_dirs: vec![format!("/sandbox/{}/upper", sandbox_id)],
            },
        };

        Ok(handle)
    }

    /// Capture git diff from container upperdir against the read-only project base
    pub async fn capture_agent_diff(&self, agent_id: &str) -> Result<String> {
        let containers = self.containers.read().await;
        let container_id = containers
            .get(agent_id)
            .ok_or_else(|| anyhow!("Container not found for agent: {}", agent_id))?
            .clone();
        drop(containers);

        let agent_sandboxes = self.agent_sandboxes.read().await;
        let recorded_sandbox = agent_sandboxes
            .get(agent_id)
            .ok_or_else(|| anyhow!("Sandbox not found for agent: {}", agent_id))?
            .clone();
        drop(agent_sandboxes);

        let sandbox_name = if recorded_sandbox.parse::<u64>().is_ok() {
            let sandboxes = self.sandboxes.read().await;
            let resolved = sandboxes
                .iter()
                .find_map(|(name, value)| {
                    if value == &recorded_sandbox {
                        Some(name.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| recorded_sandbox.clone());
            drop(sandboxes);
            resolved
        } else {
            recorded_sandbox
        };

        let scopes = self.sandbox_scopes.read().await;
        let scope = scopes.get(&sandbox_name).cloned().unwrap_or_default();
        drop(scopes);

        let workspace_root = PathBuf::from(format!("/tmp/container-{}/root/workspace", container_id));
        let project_base = if scope.trim().is_empty() || scope == "/" {
            self.project_root.clone()
        } else {
            self.project_root.join(scope.trim_matches('/'))
        };

        if !workspace_root.exists() {
            return Err(anyhow!(
                "Container workspace not found for agent {}: {}",
                agent_id,
                workspace_root.display()
            ));
        }

        if !project_base.exists() {
            return Err(anyhow!(
                "Project base not found for sandbox {}: {}",
                sandbox_name,
                project_base.display()
            ));
        }

        let diff_output = std::process::Command::new("git")
            .arg("diff")
            .arg("--no-index")
            .arg("--binary")
            .arg(&project_base)
            .arg(&workspace_root)
            .output()
            .with_context(|| {
                format!(
                    "Failed to capture diff for sandbox {} container {}",
                    sandbox_name, container_id
                )
            })?;

        if diff_output.status.success() || diff_output.status.code() == Some(1) {
            let stdout = String::from_utf8_lossy(&diff_output.stdout).to_string();
            return Self::normalize_no_index_diff(&stdout, &project_base, &workspace_root);
        }

        Err(anyhow!(
            "Failed to diff container workspace: {}",
            String::from_utf8_lossy(&diff_output.stderr)
        ))
    }

    fn normalize_no_index_diff(raw_diff: &str, project_base: &std::path::Path, workspace_root: &std::path::Path) -> Result<String> {
        // Strip trailing "/." or "/" so "/tmp/foo/." and "/tmp/foo" both normalize the same way.
        let clean = |p: &std::path::Path| -> String {
            let s = p.to_string_lossy().replace('\\', "/");
            let s = s.trim_end_matches('.');
            let s = s.trim_end_matches('/');
            s.to_string()
        };
        let base_str = clean(project_base);
        let workspace_str = clean(workspace_root);
        let base_slash = format!("{}/", base_str);
        let workspace_slash = format!("{}/", workspace_str);
        let mut normalized = String::new();

        for line in raw_diff.lines() {
            let mut updated = line
                .replace(&base_slash, "a/")
                .replace(&workspace_slash, "b/")
                .replace(&base_str, "a")
                .replace(&workspace_str, "b");
            if let Some(rest) = updated.strip_prefix("diff --git ") {
                let mut parts = rest.split_whitespace();
                if let (Some(lhs), Some(rhs)) = (parts.next(), parts.next()) {
                    updated = format!("diff --git {} {}", lhs, rhs);
                }
            }
            normalized.push_str(&updated);
            normalized.push('\n');
        }

        Ok(normalized)
    }

    /// Capture the current sandbox state as a diff against the host project base.
    ///
    /// Layer 6 uses this after applying fix diffs so later verification rounds
    /// and Layer 7 operate on the post-fix sandbox contents.
    pub async fn capture_sandbox_diff(&self, sandbox_name: &SandboxName) -> Result<String> {
        let sandboxes = self.sandboxes.read().await;
        let sandbox_id = sandboxes
            .get(sandbox_name)
            .ok_or_else(|| anyhow!("Sandbox not found: {}", sandbox_name))?
            .clone();
        drop(sandboxes);

        let request = json!({
            "request_type": "create_container",
            "sandbox": sandbox_id,
        });
        let socket_path = self.socket_path.clone();
        let response = tokio::task::spawn_blocking(move || {
            super::pooled_socket_request(&socket_path, &request)
        }).await.context("spawn_blocking socket_roundtrip")??;
        let container_id = super::parse_ok_field(&response, "container")?;

        let scopes = self.sandbox_scopes.read().await;
        let scope = scopes.get(sandbox_name).cloned().unwrap_or_default();
        drop(scopes);

        let workspace_root = PathBuf::from(format!("/tmp/container-{}/root/workspace", container_id));
        let project_base = if scope.trim().is_empty() || scope == "/" {
            self.project_root.clone()
        } else {
            self.project_root.join(scope.trim_matches('/'))
        };

        let diff_result = if !workspace_root.exists() {
            Err(anyhow!(
                "Sandbox workspace not found for {}: {}",
                sandbox_name,
                workspace_root.display()
            ))
        } else if !project_base.exists() {
            Err(anyhow!(
                "Project base not found for sandbox {}: {}",
                sandbox_name,
                project_base.display()
            ))
        } else {
            let diff_output = std::process::Command::new("git")
                .arg("diff")
                .arg("--no-index")
                .arg("--binary")
                .arg(&project_base)
                .arg(&workspace_root)
                .output()
                .with_context(|| {
                    format!(
                        "Failed to capture sandbox diff for {} container {}",
                        sandbox_name, container_id
                    )
                })?;

            if diff_output.status.success() || diff_output.status.code() == Some(1) {
                let stdout = String::from_utf8_lossy(&diff_output.stdout).to_string();
                Self::normalize_no_index_diff(&stdout, &project_base, &workspace_root)
            } else {
                Err(anyhow!(
                    "Failed to diff sandbox workspace: {}",
                    String::from_utf8_lossy(&diff_output.stderr)
                ))
            }
        };

        let cleanup_request = json!({
            "request_type": "destroy_container",
            "sandbox": sandbox_id,
            "container": container_id,
        });
        let socket_path = self.socket_path.clone();
        let _ = tokio::task::spawn_blocking(move || {
            super::pooled_socket_request(&socket_path, &cleanup_request)
        }).await;

        diff_result
    }

    /// Create checkpoint (not implemented - would need agentd support)
    pub async fn create_checkpoint(&self, _agent_id: &str) -> Result<PathBuf> {
        // Checkpoints would require agentd to expose snapshot API
        // For now, we'll skip checkpointing and rely on git for recovery
        Ok(PathBuf::from("/tmp/mock-checkpoint"))
    }

    /// Restore checkpoint (not implemented)
    pub async fn restore_checkpoint(&self, _agent_id: &str, _checkpoint_path: &PathBuf) -> Result<()> {
        // Not implemented - would need agentd support
        Ok(())
    }

    /// Copy workspace files directly from container to host (for --save-all)
    pub async fn copy_workspace_to_host(&self, container_id: &str, sandbox_id: &str, output_dir: &std::path::Path) -> Result<()> {
        log::info!("  📦 Copying workspace from container {} to {:?}", &container_id[..8], output_dir);

        std::fs::create_dir_all(output_dir)
            .with_context(|| format!("Failed to create output directory: {}", output_dir.display()))?;

        let files_copied = self.copy_workspace_dir_recursive(sandbox_id, container_id, "/workspace", output_dir)?;

        log::info!("  ✅ Copied {} files to host", files_copied);
        Ok(())
    }

    fn copy_workspace_dir_recursive(
        &self,
        sandbox_id: &str,
        container_id: &str,
        workspace_path: &str,
        output_dir: &std::path::Path,
    ) -> Result<usize> {
        let list_request = json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "name": "list_files",
            "input": {
                "path": workspace_path,
            }
        });

        let list_response = super::pooled_socket_request(&self.socket_path, &list_request)?;
        let result = list_response
            .get("result")
            .ok_or_else(|| anyhow!("list_files missing result for {}", workspace_path))?;

        let files = result
            .get("files")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let directories = result
            .get("directories")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();

        let mut copied = 0usize;

        for file in files {
            let Some(file_name) = file.as_str() else {
                continue;
            };

            let source_path = format!("{}/{}", workspace_path.trim_end_matches('/'), file_name);
            let read_request = json!({
                "request_type": "invoke_tool",
                "sandbox": sandbox_id,
                "container": container_id,
                "name": "read_file",
                "input": {
                    "path": source_path,
                }
            });

            let read_response = super::pooled_socket_request(&self.socket_path, &read_request)?;
            let content = read_response
                .get("result")
                .and_then(|value| value.get("content"))
                .and_then(|value| value.as_str())
                .ok_or_else(|| anyhow!("read_file missing content for {}", source_path))?;

            let target_path = output_dir.join(file_name);
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create parent directory: {}", parent.display())
                })?;
            }
            std::fs::write(&target_path, content).with_context(|| {
                format!("Failed to write exported file: {}", target_path.display())
            })?;
            copied += 1;
        }

        for directory in directories {
            let Some(directory_name) = directory.as_str() else {
                continue;
            };

            let nested_workspace = format!("{}/{}", workspace_path.trim_end_matches('/'), directory_name);
            let nested_output = output_dir.join(directory_name);
            std::fs::create_dir_all(&nested_output).with_context(|| {
                format!("Failed to create target directory: {}", nested_output.display())
            })?;
            copied += self.copy_workspace_dir_recursive(
                sandbox_id,
                container_id,
                &nested_workspace,
                &nested_output,
            )?;
        }

        Ok(copied)
    }

    /// Stage container workspace for later export (call this BEFORE destroy_agent_layer)
    pub async fn stage_agent_workspace(&self, agent_id: &str, staging_root: &std::path::Path) -> Result<PathBuf> {
        let containers = self.containers.read().await;
        let container_id = containers
            .get(agent_id)
            .ok_or_else(|| anyhow!("Container not found for agent: {}", agent_id))?
            .clone();
        drop(containers);

        let agent_sandboxes = self.agent_sandboxes.read().await;
        let sandbox_name = agent_sandboxes
            .get(agent_id)
            .ok_or_else(|| anyhow!("Sandbox not found for agent: {}", agent_id))?
            .clone();
        drop(agent_sandboxes);

        let sandboxes = self.sandboxes.read().await;
        let sandbox_id = sandboxes
            .get(&sandbox_name)
            .ok_or_else(|| anyhow!("Sandbox not found: {}", sandbox_name))?
            .clone();
        drop(sandboxes);

        // Create unique staging directory for this agent
        let staging_dir = staging_root.join(format!("staged-{}", agent_id.replace(|c: char| !c.is_alphanumeric(), "_")));
        std::fs::create_dir_all(&staging_dir)
            .with_context(|| format!("Failed to create staging directory: {}", staging_dir.display()))?;

        // Copy workspace contents via socket using read_file/list_files (synchronous via socket_roundtrip)
        let files_copied = self.copy_workspace_dir_recursive(&sandbox_id, &container_id, "/workspace", &staging_dir)?;

        // Record the staged workspace
        let mut staged = self.staged_workspaces.write().await;
        staged.push(StagedWorkspace {
            agent_id: agent_id.to_string(),
            sandbox_name: sandbox_name.clone(),
            container_id: container_id.clone(),
            staging_path: staging_dir.clone(),
        });

        log::info!("  📦 Staged {} files from agent {} workspace to {}", files_copied, &agent_id[..8], staging_dir.display());

        Ok(staging_dir)
    }

    /// Export all staged workspaces to the final output directory
    pub fn export_staged_workspaces(&self, output_dir: &std::path::Path) -> Result<HostWorkspaceExportSummary> {
        std::fs::create_dir_all(output_dir)
            .with_context(|| format!("Failed to create output directory: {}", output_dir.display()))?;

        // We need to block_on here because we're in a non-async context
        let rt = tokio::runtime::Runtime::new()?;
        let staged = rt.block_on(async {
            self.staged_workspaces.read().await.clone()
        });

        let mut summary = HostWorkspaceExportSummary::default();
        summary.containers_found = staged.len();

        for staged_ws in staged {
            if staged_ws.staging_path.exists() {
                let files_copied = copy_dir_contents_recursive(&staged_ws.staging_path, output_dir)?;
                summary.workspaces_copied += 1;
                summary.files_copied += files_copied;
                log::info!("  ✅ Exported staged workspace for agent {} ({} files)", &staged_ws.agent_id[..8], files_copied);
            } else {
                log::warn!("  ⚠️ Staged workspace not found: {}", staged_ws.staging_path.display());
            }
        }

        Ok(summary)
    }

    /// Export from staging directory to output (used by CLI when staging was done during orchestration)
    pub fn export_staged_to_output(&self, output_dir: &std::path::Path) -> Result<HostWorkspaceExportSummary> {
        self.export_staged_workspaces(output_dir)
    }

    /// Put a completed agent's container to sleep (keep alive for reuse)
    pub async fn sleep_agent_layer(&self, agent_id: &str, sandbox_name: &SandboxName) -> Result<()> {
        let mut containers = self.containers.write().await;
        let container_id = match containers.remove(agent_id) {
            Some(id) => id,
            None => return Ok(()),
        };
        drop(containers);

        let mut agent_sandboxes = self.agent_sandboxes.write().await;
        agent_sandboxes.remove(agent_id);
        drop(agent_sandboxes);

        let sandboxes = self.sandboxes.read().await;
        let sandbox_id = sandboxes
            .get(sandbox_name)
            .ok_or_else(|| anyhow!("Sandbox not found: {}", sandbox_name))?
            .clone();
        drop(sandboxes);

        // Reset workspace to a clean state for reuse
        let reset_request = json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "name": "run_command",
            "input": {
                "cmd": "cd /workspace && (git reset --hard HEAD 2>/dev/null || true) && (git clean -fd 2>/dev/null || true) && git add -A 2>/dev/null || true",
                "timeout": 15
            }
        });
        let _ = super::pooled_socket_request(&self.socket_path, &reset_request);

        let paused_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let sleeping = SleepingContainer {
            agent_id: agent_id.to_string(),
            container_id,
            sandbox_id,
            paused_at,
        };

        let mut pool = self.sleeping_containers.write().await;
        pool.entry(sandbox_name.clone())
            .or_insert_with(Vec::new)
            .push(sleeping);

        log::info!("  → Container for agent {} sleeping in pool for sandbox {}", &agent_id[..8], sandbox_name);
        Ok(())
    }

    /// Wake a sleeping container or create a new one if none available
    pub async fn wake_or_create_agent_layer(
        &self,
        sandbox_name: &SandboxName,
        task_id: Option<TaskId>,
    ) -> Result<AgentHandle> {
        let sleeping = {
            let mut pool = self.sleeping_containers.write().await;
            pool.get_mut(sandbox_name)
                .and_then(|v| if v.is_empty() { None } else { Some(v.remove(0)) })
        };

        if let Some(sc) = sleeping {
            let new_agent_id = Uuid::new_v4().to_string();

            let mut containers = self.containers.write().await;
            containers.insert(new_agent_id.clone(), sc.container_id.clone());
            drop(containers);

            let mut agent_sandboxes = self.agent_sandboxes.write().await;
            agent_sandboxes.insert(new_agent_id.clone(), sandbox_name.clone());
            drop(agent_sandboxes);

            log::info!("  → Woke container {} for sandbox {} (new agent {})",
                &sc.container_id[..8], sandbox_name, &new_agent_id[..8]);

            let handle = AgentHandle {
                agent_id: new_agent_id,
                sandbox_name: sc.sandbox_id.clone(),
                container_id: sc.container_id.clone(),
                task_id,
                layer: OverlayfsLayer {
                    level: LayerLevel::Agent,
                    mount_path: format!("/sandbox/{}/container/{}", sc.sandbox_id, sc.container_id),
                    upper_dir: format!("/sandbox/{}/container/{}/upper", sc.sandbox_id, sc.container_id),
                    work_dir: format!("/sandbox/{}/container/{}/work", sc.sandbox_id, sc.container_id),
                    lower_dirs: vec![format!("/sandbox/{}/upper", sc.sandbox_id)],
                },
            };
            return Ok(handle);
        }

        self.create_agent_layer(sandbox_name, task_id).await
    }

    /// Destroy all sleeping containers (call on shutdown)
    pub async fn cleanup_sleeping_containers(&self) -> Result<()> {
        let mut pool = self.sleeping_containers.write().await;
        let all: Vec<SleepingContainer> = pool.drain().flat_map(|(_, v)| v).collect();
        drop(pool);

        for sc in all {
            let request = json!({
                "request_type": "destroy_container",
                "sandbox": sc.sandbox_id,
                "container": sc.container_id,
            });
            let socket_path = self.socket_path.clone();
            match tokio::task::spawn_blocking(move || {
                super::pooled_socket_request(&socket_path, &request)
            }).await {
                Ok(Ok(_)) => log::info!("  → Cleaned up sleeping container {}", &sc.container_id[..8]),
                Ok(Err(e)) => log::warn!("  ⚠ Failed to clean up sleeping container {}: {}", &sc.container_id[..8], e),
                Err(e) => log::warn!("  ⚠ Failed to join spawn_blocking for cleanup: {}", e),
            }
        }
        Ok(())
    }

    /// Destroy agent layer (container) — called after agent completes task
    pub async fn destroy_agent_layer(&self, agent_id: &str) -> Result<()> {
        let mut containers = self.containers.write().await;
        let container_id = containers
            .remove(agent_id)
            .ok_or_else(|| anyhow!("Container not found for agent: {}", agent_id))?;
        drop(containers);

        // Get sandbox ID for this container
        let mut agent_sandboxes = self.agent_sandboxes.write().await;
        let sandbox_name = agent_sandboxes
            .remove(agent_id)
            .ok_or_else(|| anyhow!("Sandbox not found for agent: {}", agent_id))?;
        drop(agent_sandboxes);

        let sandboxes = self.sandboxes.read().await;
        let sandbox_id = sandboxes
            .get(&sandbox_name)
            .ok_or_else(|| anyhow!("Sandbox not found: {}", sandbox_name))?
            .clone();
        drop(sandboxes);

        // Destroy container via agentd socket to free resources immediately
        let request = json!({
            "request_type": "destroy_container",
            "sandbox": sandbox_id,
            "container": container_id,
        });

        // Call agentd to destroy the container
        let socket_path = self.socket_path.clone();
        match tokio::task::spawn_blocking(move || {
            super::pooled_socket_request(&socket_path, &request)
        }).await {
            Ok(Ok(_)) => {
                log::info!("  → Destroyed container {} for agent {}", &container_id[..8], &agent_id[..8]);
                Ok(())
            }
            Ok(Err(e)) => {
                // Log error but don't fail — container will be cleaned up with sandbox
                log::warn!("  ⚠ Failed to destroy container {}: {}", &container_id[..8], e);
                Ok(())
            }
            Err(e) => {
                log::warn!("  ⚠ Failed to join spawn_blocking for destroy: {}", e);
                Ok(())
            }
        }
    }

    /// Apply diff to sandbox base layer
    pub async fn apply_diff_to_sandbox(&self, sandbox_name: &SandboxName, diff: &str) -> Result<()> {
        let sandboxes = self.sandboxes.read().await;
        let sandbox_id = sandboxes
            .get(sandbox_name)
            .ok_or_else(|| anyhow!("Sandbox not found: {}", sandbox_name))?
            .clone();
        drop(sandboxes);

        // Create temporary container to apply the diff
        let request = json!({
            "request_type": "create_container",
            "sandbox": sandbox_id,
        });

        let socket_path = self.socket_path.clone();
        let response = tokio::task::spawn_blocking(move || {
            super::pooled_socket_request(&socket_path, &request)
        }).await.context("spawn_blocking socket_roundtrip")??;
        let container_id = super::parse_ok_field(&response, "container")?;

        // Write diff to a temporary file
        let write_request = json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "name": "write_file",
            "input": {
                "path": "/tmp/apply.diff",
                "content": diff
            }
        });
        let socket_path = self.socket_path.clone();
        tokio::task::spawn_blocking(move || {
            super::pooled_socket_request(&socket_path, &write_request)
        }).await.context("spawn_blocking socket_roundtrip")??;

        // Apply the diff with git apply
        // Use --3way to handle conflicts and check if files exist
        let apply_request = json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "name": "run_command",
            "input": {
                "cmd": "cd /workspace && (git apply --3way /tmp/apply.diff 2>/dev/null || git apply --check /tmp/apply.diff 2>/dev/null || echo 'Skipping conflicting diff') && git add -A 2>/dev/null; git commit -m 'apply agent changes' --allow-empty",
                "timeout": 30
            }
        });
        let socket_path = self.socket_path.clone();
        let apply_result = tokio::task::spawn_blocking(move || {
            super::pooled_socket_request(&socket_path, &apply_request)
        }).await.context("spawn_blocking socket_roundtrip")??;

        // Check if apply succeeded
        if let Some(result) = apply_result.get("result") {
            if result.get("exit_code").and_then(|e| e.as_u64()) != Some(0) {
                let stderr = result.get("stderr").and_then(|s| s.as_str()).unwrap_or_default();
                return Err(anyhow!("Failed to apply diff: {}", stderr));
            }
        }

        // Clean up temporary container
        let cleanup_request = json!({
            "request_type": "destroy_container",
            "sandbox": sandbox_id,
            "container": container_id,
        });
        let socket_path = self.socket_path.clone();
        let _ = tokio::task::spawn_blocking(move || {
            super::pooled_socket_request(&socket_path, &cleanup_request)
        }).await;

        Ok(())
    }

    /// Destroy sandbox layer
    pub async fn destroy_sandbox_layer(&self, sandbox_name: &SandboxName) -> Result<()> {
        let mut sandboxes = self.sandboxes.write().await;
        let sandbox_id = sandboxes
            .remove(sandbox_name)
            .ok_or_else(|| anyhow!("Sandbox not found: {}", sandbox_name))?;

        // Call agentd to destroy sandbox
        let request = json!({
            "request_type": "destroy_sandbox",
            "sandbox": sandbox_id,
        });

        let socket_path = self.socket_path.clone();
        tokio::task::spawn_blocking(move || {
            super::pooled_socket_request(&socket_path, &request)
        }).await.context("spawn_blocking socket_roundtrip")??;

        log::info!("  → Destroyed sandbox: {}", sandbox_name);

        Ok(())
    }

    /// Get sandbox info
    pub async fn get_sandbox_info(&self, sandbox_name: &SandboxName) -> Option<SandboxLayerInfo> {
        let sandboxes = self.sandboxes.read().await;
        sandboxes.get(sandbox_name).map(|sandbox_id| SandboxLayerInfo {
            name: sandbox_name.clone(),
            sandbox_id: sandbox_id.clone(),
        })
    }

    /// Map tool name to package name
    fn tool_to_package(&self, tool: &str) -> Option<String> {
        match tool {
            "git_clone" | "git_commit" | "git_diff" | "git_status" => Some("git".to_string()),
            "npm_install" => Some("nodejs npm".to_string()),
            "pip_install" => Some("python3 py3-pip".to_string()),
            "cargo_add" => Some("rust cargo".to_string()),
            "docker_build" | "docker_run" => Some("docker".to_string()),
            "kubectl_apply" | "kubectl_get" => Some("kubectl".to_string()),
            _ => None, // Core tools don't need extra packages
        }
    }

    /// Update sandbox active agent count (not tracked in this version)
    pub async fn update_sandbox_active_agents(
        &self,
        _sandbox_name: &SandboxName,
        _delta: i32,
    ) -> Result<()> {
        // Not implemented - agentd doesn't expose agent count
        Ok(())
    }
}

/// Export staged workspaces from a staging directory to output (for CLI post-orchestration export)
pub fn export_staged_workspaces_from_dir(
    staging_dir: &std::path::Path,
    output_dir: &std::path::Path,
) -> Result<HostWorkspaceExportSummary> {
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory: {}", output_dir.display()))?;

    if !staging_dir.exists() {
        return Ok(HostWorkspaceExportSummary::default());
    }

    let mut summary = HostWorkspaceExportSummary::default();
    let entries = std::fs::read_dir(staging_dir)
        .with_context(|| format!("Failed to read staging directory: {}", staging_dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };

        if !name.starts_with("staged-") || !path.is_dir() {
            continue;
        }

        summary.containers_found += 1;

        let files_copied = copy_dir_contents_recursive(&path, output_dir)?;
        summary.workspaces_copied += 1;
        summary.files_copied += files_copied;
        log::info!("  ✅ Exported staged workspace {} ({} files)", name, files_copied);
    }

    Ok(summary)
}

/// Public info about sandbox layer
#[derive(Debug, Clone)]
pub struct SandboxLayerInfo {
    pub name: SandboxName,
    pub sandbox_id: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HostWorkspaceExportSummary {
    pub containers_found: usize,
    pub workspaces_copied: usize,
    pub files_copied: usize,
}

pub fn export_container_workspaces_to_host(
    container_root: &std::path::Path,
    output_dir: &std::path::Path,
) -> Result<HostWorkspaceExportSummary> {
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory: {}", output_dir.display()))?;

    if !container_root.exists() {
        return Ok(HostWorkspaceExportSummary::default());
    }

    let mut summary = HostWorkspaceExportSummary::default();
    let entries = std::fs::read_dir(container_root)
        .with_context(|| format!("Failed to read container root: {}", container_root.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };

        if !name.starts_with("container-") || !path.is_dir() {
            continue;
        }

        summary.containers_found += 1;

        let workspace = path.join("root").join("workspace");
        if !workspace.is_dir() {
            continue;
        }

        let files_copied = copy_dir_contents_recursive(&workspace, output_dir)?;
        summary.workspaces_copied += 1;
        summary.files_copied += files_copied;
    }

    Ok(summary)
}

fn copy_dir_contents_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<usize> {
    let mut files_copied = 0;

    for entry in std::fs::read_dir(src)
        .with_context(|| format!("Failed to read source directory: {}", src.display()))?
    {
        let entry = entry?;
        let entry_path = entry.path();
        let target_path = dst.join(entry.file_name());

        if entry_path.is_dir() {
            std::fs::create_dir_all(&target_path).with_context(|| {
                format!("Failed to create target directory: {}", target_path.display())
            })?;
            files_copied += copy_dir_contents_recursive(&entry_path, &target_path)?;
            continue;
        }

        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent directory: {}", parent.display())
            })?;
        }

        std::fs::copy(&entry_path, &target_path).with_context(|| {
            format!(
                "Failed to copy {} to {}",
                entry_path.display(),
                target_path.display()
            )
        })?;
        files_copied += 1;
    }

    Ok(files_copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_topology_manager_creation() {
        let project_root = PathBuf::from(".");
        let socket_path = "/tmp/agentd.sock".to_string();

        let manager = TopologyManager::new(project_root, socket_path);
        assert!(manager.is_ok());
    }

    #[tokio::test]
    async fn test_tool_to_package() {
        let manager = TopologyManager::new(
            PathBuf::from("."),
            "/tmp/agentd.sock".to_string(),
        )
        .unwrap();

        assert_eq!(manager.tool_to_package("git_clone"), Some("git".to_string()));
        assert_eq!(
            manager.tool_to_package("npm_install"),
            Some("nodejs npm".to_string())
        );
        assert_eq!(manager.tool_to_package("read_file"), None);
    }

    #[test]
    fn test_export_container_workspaces_to_host_copies_nested_files() {
        let base = tempfile::tempdir().unwrap();
        let container_root = base.path().join("containers");
        let output_dir = base.path().join("output");

        std::fs::create_dir_all(&container_root).unwrap();

        for index in 0..5 {
            let workspace = container_root
                .join(format!("container-{index:08}"))
                .join("root")
                .join("workspace");
            let nested = workspace.join("artifacts").join(format!("agent_{index}"));
            std::fs::create_dir_all(&nested).unwrap();
            std::fs::write(
                nested.join("file.txt"),
                format!("agent {index} file exported to host"),
            )
            .unwrap();
            std::fs::write(
                nested.join("changes.diff"),
                format!("diff --git a/file_{index} b/file_{index}\n+saved\n"),
            )
            .unwrap();
        }

        let summary = export_container_workspaces_to_host(&container_root, &output_dir).unwrap();

        assert_eq!(summary.containers_found, 5);
        assert_eq!(summary.workspaces_copied, 5);
        assert_eq!(summary.files_copied, 10);

        for index in 0..5 {
            let nested = output_dir.join("artifacts").join(format!("agent_{index}"));
            assert!(nested.join("file.txt").exists());
            assert!(nested.join("changes.diff").exists());
        }
    }
}
