//! Multi-sandbox orchestration: New 7-layer orchestration system

// NEW 7-LAYER ARCHITECTURE
pub mod types;
pub mod sandbox_topology;
pub mod scheduler;
pub mod planner;
pub mod checkpoint;
pub mod merge_worker;
pub mod verification;
pub mod agent_execution;
pub mod new_orchestrator;
pub mod mock_agent;
pub mod simulate;

// Re-export main types
pub use new_orchestrator::{NewOrchestrator, OrchestratorConfig, FinalOutput};
pub use agent_execution::{set_verbose, is_verbose};

// KEEP: Still needed files
pub mod session_store;
pub mod sandbox_profiles;

/// Long-running generateContent calls (large outputs / tool loops).
pub(crate) const HTTP_TIMEOUT_SECS: u64 = 900;

/// Safety cap for tool-calling loops only (each round is one API call). Raise if needed.
pub(crate) const MAX_TOOL_ROUNDS: usize = 256;

/// Context-gatherer tool rounds (Layer 1).
pub(crate) const MAX_CONTEXT_GATHER_ROUNDS: usize = 128;

/// `maxOutputTokens` for Vertex `generateContent`. The API still applies per-model server-side limits.
pub(crate) const VERTEX_MAX_OUTPUT_TOKENS: u32 = 65_536;

/// Gemini 2.5 “thinking” budget (tokens). Omit by setting to 0 if your endpoint rejects the field.
pub(crate) const VERTEX_THINKING_BUDGET_TOKENS: u32 = 24_576;

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enable/disable verbose orchestration logging (socket/HTTP payloads, round timings, etc).
/// Normal mode prints only high-signal CLI events (tool calls / file ops).
pub fn set_debug(enabled: bool) {
    DEBUG_ENABLED.store(enabled, Ordering::Relaxed);
}

pub(crate) fn debug_enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

/// Standard generation block for text / tools (no JSON mode).
pub(crate) fn vertex_generation_config(temperature: f64) -> Value {
    if VERTEX_THINKING_BUDGET_TOKENS == 0 {
        return json!({
            "temperature": temperature,
            "maxOutputTokens": VERTEX_MAX_OUTPUT_TOKENS
        });
    }
    json!({
        "temperature": temperature,
        "maxOutputTokens": VERTEX_MAX_OUTPUT_TOKENS,
        "thinkingConfig": {
            "thinkingBudget": VERTEX_THINKING_BUDGET_TOKENS
        }
    })
}

/// Like [`vertex_generation_config`] but requests JSON-only responses (architect / planner).
pub(crate) fn vertex_generation_config_json(temperature: f64) -> Value {
    if VERTEX_THINKING_BUDGET_TOKENS == 0 {
        return json!({
            "temperature": temperature,
            "maxOutputTokens": VERTEX_MAX_OUTPUT_TOKENS,
            "responseMimeType": "application/json"
        });
    }
    json!({
        "temperature": temperature,
        "maxOutputTokens": VERTEX_MAX_OUTPUT_TOKENS,
        "responseMimeType": "application/json",
        "thinkingConfig": {
            "thinkingBudget": VERTEX_THINKING_BUDGET_TOKENS
        }
    })
}

pub(crate) fn trace(msg: &str) {
    if !debug_enabled() {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    println!("[orchestration:{}] {}", ts, msg);
}

// ── Vertex / gcloud (shared by planner, agent_runner, orchestrator) ────────

#[cfg(unix)]
pub(crate) fn gcloud_access_token() -> anyhow::Result<String> {
    use anyhow::{anyhow, Context};
    use std::process::Command;
    trace("gcloud auth print-access-token: starting");
    let out = Command::new("gcloud")
        .args(["auth", "print-access-token"])
        .output()
        .context("spawn gcloud — is it installed and on PATH?")?;
    if !out.status.success() {
        return Err(anyhow!(
            "gcloud auth print-access-token failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let s = String::from_utf8(out.stdout).context("token utf-8")?;
    let t = s.trim().to_string();
    if t.is_empty() {
        return Err(anyhow!("empty access token from gcloud"));
    }
    trace(&format!(
        "gcloud auth print-access-token: OAuth access token length={} chars (not Gemini output)",
        t.len()
    ));
    Ok(t)
}

#[cfg(not(unix))]
pub(crate) fn gcloud_access_token() -> anyhow::Result<String> {
    Err(anyhow::anyhow!(
        "orchestration requires Unix (agentd uses Unix domain sockets)"
    ))
}

pub(crate) fn vertex_generate_url(project_id: &str) -> String {
    format!(
        "https://us-central1-aiplatform.googleapis.com/v1/projects/{}/locations/us-central1/publishers/google/models/gemini-2.5-pro:generateContent",
        project_id
    )
}

/// Same five tools as `vertex_agent.rs` / agentd socket.
pub(crate) fn gemini_tool_declarations() -> serde_json::Value {
    use serde_json::json;
    json!([
        {
            "name": "read_file",
            "description": "Read a file from the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to read" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "write_file",
            "description": "Write text content to a file path in the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to write" },
                    "content": { "type": "string", "description": "Text content to write" }
                },
                "required": ["path", "content"]
            }
        },
        {
            "name": "append_file",
            "description": "Append text content to a file path in the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to append to" },
                    "content": { "type": "string", "description": "Text content to append" }
                },
                "required": ["path", "content"]
            }
        },
        {
            "name": "delete_file",
            "description": "Delete a file from the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to delete" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "copy_file",
            "description": "Copy a file from one path to another inside the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Source file path" },
                    "to": { "type": "string", "description": "Destination file path" }
                },
                "required": ["from", "to"]
            }
        },
        {
            "name": "move_file",
            "description": "Move (rename) a file from one path to another inside the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Source file path" },
                    "to": { "type": "string", "description": "Destination file path" }
                },
                "required": ["from", "to"]
            }
        },
        {
            "name": "list_files",
            "description": "List files and subdirectories in a directory.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path to list" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "create_directory",
            "description": "Create a directory (and parents) in the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path to create" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "delete_directory",
            "description": "Delete a directory and its contents from the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path to delete" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "get_file_info",
            "description": "Get information about a file in the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to inspect" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "file_exists",
            "description": "Check whether a file exists in the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to check" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "run_command",
            "description": "Run a shell command inside the sandbox (chroot).",
            "parameters": {
                "type": "object",
                "properties": {
                    "cmd": { "type": "string", "description": "Shell command to run" },
                    "cwd": { "type": "string", "description": "Optional working directory" }
                },
                "required": ["cmd"]
            }
        },
        {
            "name": "run_script",
            "description": "Run a script inside the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Script path" },
                    "interpreter": { "type": "string", "description": "Optional interpreter (e.g. python3, bash)" },
                    "script": { "type": "string", "description": "Optional inline script content" },
                    "language": { "type": "string", "description": "Optional script language" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "kill_process",
            "description": "Kill a process by PID inside the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pid": { "type": "integer", "description": "Process ID to kill" }
                },
                "required": ["pid"]
            }
        },
        {
            "name": "get_env",
            "description": "Get an environment variable inside the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "var": { "type": "string", "description": "Environment variable name" }
                },
                "required": ["var"]
            }
        },
        {
            "name": "set_env",
            "description": "Set an environment variable inside the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "var": { "type": "string", "description": "Environment variable name" },
                    "value": { "type": "string", "description": "Environment variable value" }
                },
                "required": ["var", "value"]
            }
        },
        {
            "name": "http_get",
            "description": "Perform an HTTP GET request.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to GET" }
                },
                "required": ["url"]
            }
        },
        {
            "name": "http_post",
            "description": "Perform an HTTP POST request.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to POST" },
                    "body": { "type": "string", "description": "Request body" }
                },
                "required": ["url", "body"]
            }
        },
        {
            "name": "http_put",
            "description": "Perform an HTTP PUT request.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to PUT" },
                    "body": { "type": "string", "description": "Request body" }
                },
                "required": ["url", "body"]
            }
        },
        {
            "name": "http_delete",
            "description": "Perform an HTTP DELETE request.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to DELETE" }
                },
                "required": ["url"]
            }
        },
        {
            "name": "http_patch",
            "description": "Perform an HTTP PATCH request.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to PATCH" },
                    "body": { "type": "string", "description": "Request body" }
                },
                "required": ["url", "body"]
            }
        },
        {
            "name": "download_file",
            "description": "Download a file from a URL into the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "File URL to download" },
                    "path": { "type": "string", "description": "Destination path in the sandbox" }
                },
                "required": ["url", "path"]
            }
        },
        {
            "name": "websocket_send",
            "description": "Send a message to a WebSocket URL.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "WebSocket URL" },
                    "message": { "type": "string", "description": "Message to send" }
                },
                "required": ["url", "message"]
            }
        },
        {
            "name": "json_parse",
            "description": "Parse JSON text into a JSON object/value.",
            "parameters": {
                "type": "object",
                "properties": {
                    "data": { "type": "string", "description": "JSON input string" }
                },
                "required": ["data"]
            }
        },
        {
            "name": "json_stringify",
            "description": "Stringify JSON value into text.",
            "parameters": {
                "type": "object",
                "properties": {
                    "data": { "type": "string", "description": "JSON input value (as string or JSON)" }
                },
                "required": ["data"]
            }
        },
        {
            "name": "json_query",
            "description": "Query a JSON value using a path expression.",
            "parameters": {
                "type": "object",
                "properties": {
                    "data": { "type": "string", "description": "JSON data to query" },
                    "path": { "type": "string", "description": "Query path" }
                },
                "required": ["data", "path"]
            }
        },
        {
            "name": "csv_read",
            "description": "Read a CSV file from the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "CSV file path" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "csv_write",
            "description": "Write CSV rows to a file in the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "CSV output path" },
                    "rows": { "type": "array", "description": "CSV rows" }
                },
                "required": ["path", "rows"]
            }
        },
        {
            "name": "git_clone",
            "description": "Clone a git repository into the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "repo": { "type": "string", "description": "Repository URL" },
                    "path": { "type": "string", "description": "Destination path" }
                },
                "required": ["repo", "path"]
            }
        },
        {
            "name": "git_status",
            "description": "Get git status for a repository path.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "git_add",
            "description": "Stage files in a git repository.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" },
                    "files": { "type": "array", "description": "Files to stage" }
                },
                "required": ["path", "files"]
            }
        },
        {
            "name": "git_commit",
            "description": "Create a git commit in a repository.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" },
                    "message": { "type": "string", "description": "Commit message" }
                },
                "required": ["path", "message"]
            }
        },
        {
            "name": "git_push",
            "description": "Push commits to a remote repository.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" },
                    "remote": { "type": "string", "description": "Remote name (e.g. origin)" },
                    "branch": { "type": "string", "description": "Branch name" }
                },
                "required": ["path", "remote", "branch"]
            }
        },
        {
            "name": "git_pull",
            "description": "Pull updates from a remote repository.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" },
                    "remote": { "type": "string", "description": "Remote name (e.g. origin)" },
                    "branch": { "type": "string", "description": "Branch name" }
                },
                "required": ["path", "remote", "branch"]
            }
        },
        {
            "name": "git_branch",
            "description": "Create or list branches in a git repository.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" },
                    "name": { "type": "string", "description": "Optional branch name" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "git_checkout",
            "description": "Checkout a branch in a git repository.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" },
                    "branch": { "type": "string", "description": "Branch name" }
                },
                "required": ["path", "branch"]
            }
        },
        {
            "name": "git_diff",
            "description": "Get git diff for a repository path.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "docker_build",
            "description": "Build a Docker image.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Docker build context path" },
                    "tag": { "type": "string", "description": "Image tag" }
                },
                "required": ["path", "tag"]
            }
        },
        {
            "name": "docker_run",
            "description": "Run a Docker image.",
            "parameters": {
                "type": "object",
                "properties": {
                    "image": { "type": "string", "description": "Docker image name" },
                    "cmd": { "type": "string", "description": "Optional command override" },
                    "name": { "type": "string", "description": "Optional container name" }
                },
                "required": ["image"]
            }
        },
        {
            "name": "docker_stop",
            "description": "Stop a Docker container.",
            "parameters": {
                "type": "object",
                "properties": {
                    "container": { "type": "string", "description": "Container id/name" }
                },
                "required": ["container"]
            }
        },
        {
            "name": "docker_ps",
            "description": "List Docker containers.",
            "parameters": {
                "type": "object",
                "properties": {
                    "all": { "type": "boolean", "description": "Optional: include stopped containers" }
                },
                "required": []
            }
        },
        {
            "name": "docker_logs",
            "description": "Get logs for a Docker container.",
            "parameters": {
                "type": "object",
                "properties": {
                    "container": { "type": "string", "description": "Container id/name" }
                },
                "required": ["container"]
            }
        },
        {
            "name": "docker_exec",
            "description": "Execute a command inside a Docker container.",
            "parameters": {
                "type": "object",
                "properties": {
                    "container": { "type": "string", "description": "Container id/name" },
                    "cmd": { "type": "string", "description": "Command to execute" }
                },
                "required": ["container", "cmd"]
            }
        },
        {
            "name": "docker_pull",
            "description": "Pull a Docker image.",
            "parameters": {
                "type": "object",
                "properties": {
                    "image": { "type": "string", "description": "Image name" }
                },
                "required": ["image"]
            }
        },
        {
            "name": "kubectl_apply",
            "description": "Apply a Kubernetes manifest.",
            "parameters": {
                "type": "object",
                "properties": {
                    "manifest": { "type": "string", "description": "Kubernetes manifest YAML" }
                },
                "required": ["manifest"]
            }
        },
        {
            "name": "kubectl_get",
            "description": "Get Kubernetes resources.",
            "parameters": {
                "type": "object",
                "properties": {
                    "resource": { "type": "string", "description": "Resource type (e.g. pods)" }
                },
                "required": ["resource"]
            }
        },
        {
            "name": "kubectl_delete",
            "description": "Delete a Kubernetes resource.",
            "parameters": {
                "type": "object",
                "properties": {
                    "resource": { "type": "string", "description": "Resource type" },
                    "name": { "type": "string", "description": "Resource name" }
                },
                "required": ["resource", "name"]
            }
        },
        {
            "name": "kubectl_logs",
            "description": "Fetch logs from a Kubernetes pod.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pod": { "type": "string", "description": "Pod name" }
                },
                "required": ["pod"]
            }
        },
        {
            "name": "kubectl_exec",
            "description": "Execute a command in a Kubernetes pod.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pod": { "type": "string", "description": "Pod name" },
                    "cmd": { "type": "string", "description": "Command to execute" }
                },
                "required": ["pod", "cmd"]
            }
        },
        {
            "name": "kubectl_describe",
            "description": "Describe a Kubernetes resource.",
            "parameters": {
                "type": "object",
                "properties": {
                    "resource": { "type": "string", "description": "Resource type" },
                    "name": { "type": "string", "description": "Resource name" }
                },
                "required": ["resource", "name"]
            }
        },
        {
            "name": "memory_set",
            "description": "Store a key/value in persistent memory.",
            "parameters": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Memory key" },
                    "value": { "type": "string", "description": "Memory value" }
                },
                "required": ["key", "value"]
            }
        },
        {
            "name": "memory_get",
            "description": "Retrieve a value from persistent memory by key.",
            "parameters": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Memory key" }
                },
                "required": ["key"]
            }
        },
        {
            "name": "memory_delete",
            "description": "Delete a key from persistent memory.",
            "parameters": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Memory key" }
                },
                "required": ["key"]
            }
        },
        {
            "name": "memory_list",
            "description": "List all keys in persistent memory.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        },
        {
            "name": "memory_save",
            "description": "Save memory contents to a file.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Save path" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "memory_load",
            "description": "Load memory contents from a file.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Load path" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "secret_set",
            "description": "Store a secret value.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Secret name" },
                    "value": { "type": "string", "description": "Secret value" }
                },
                "required": ["name", "value"]
            }
        },
        {
            "name": "secret_get",
            "description": "Retrieve a secret value by name.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Secret name" }
                },
                "required": ["name"]
            }
        },
        {
            "name": "npm_install",
            "description": "Install an npm package (optionally from a working directory).",
            "parameters": {
                "type": "object",
                "properties": {
                    "package": { "type": "string", "description": "Optional package name" },
                    "cwd": { "type": "string", "description": "Optional working directory" }
                },
                "required": []
            }
        },
        {
            "name": "pip_install",
            "description": "Install a Python package via pip (optionally with version).",
            "parameters": {
                "type": "object",
                "properties": {
                    "package": { "type": "string", "description": "Python package name" },
                    "version": { "type": "string", "description": "Optional version" }
                },
                "required": ["package"]
            }
        },
        {
            "name": "cargo_add",
            "description": "Add a dependency to a Rust project via cargo-edit.",
            "parameters": {
                "type": "object",
                "properties": {
                    "package": { "type": "string", "description": "Crate/package to add" },
                    "cwd": { "type": "string", "description": "Optional working directory" }
                },
                "required": ["package"]
            }
        },
        {
            "name": "web_search",
            "description": "Search the web for a query.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "web_fetch",
            "description": "Fetch a URL from the web.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to fetch" }
                },
                "required": ["url"]
            }
        },
        {
            "name": "web_screenshot",
            "description": "Take a screenshot of a URL.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to screenshot" },
                    "output": { "type": "string", "description": "Output path/filename" }
                },
                "required": ["url", "output"]
            }
        },
        {
            "name": "create_channel",
            "description": "Create a message bus channel.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Channel name" }
                },
                "required": ["name"]
            }
        },
        {
            "name": "send_message",
            "description": "Send a message to a channel on the message bus.",
            "parameters": {
                "type": "object",
                "properties": {
                    "channel": { "type": "string", "description": "Channel name" },
                    "message": { "type": "string", "description": "Message content" }
                },
                "required": ["channel", "message"]
            }
        },
        {
            "name": "read_messages",
            "description": "Read messages from a channel on the message bus.",
            "parameters": {
                "type": "object",
                "properties": {
                    "channel": { "type": "string", "description": "Channel name" }
                },
                "required": ["channel"]
            }
        },
        {
            "name": "broadcast",
            "description": "Broadcast a message to all subscribers in the message bus.",
            "parameters": {
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "Message content" }
                },
                "required": ["message"]
            }
        },
        {
            "name": "wait_for",
            "description": "Wait for messages on a channel for an optional timeout.",
            "parameters": {
                "type": "object",
                "properties": {
                    "channel": { "type": "string", "description": "Channel name" },
                    "timeout": { "type": "integer", "description": "Optional timeout in milliseconds/seconds" }
                },
                "required": ["channel"]
            }
        },
        {
            "name": "spawn_agent",
            "description": "Spawn a new agent task via the orchestrator/message bus.",
            "parameters": {
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "Agent task description/instruction" },
                    "tools": { "type": "array", "description": "Optional list of tool names" }
                },
                "required": ["task"]
            }
        },
        {
            "name": "lint",
            "description": "Run a linter over a project path.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project path" },
                    "language": { "type": "string", "description": "Optional language hint" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "test",
            "description": "Run tests for a project.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project path" },
                    "command": { "type": "string", "description": "Optional custom test command" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "build",
            "description": "Build a project (optionally with a custom command).",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project path" },
                    "command": { "type": "string", "description": "Optional custom build command" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "type_check",
            "description": "Run a type checker over a project.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project path" },
                    "language": { "type": "string", "description": "Optional language hint" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "format",
            "description": "Format code in a project path.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project path" },
                    "language": { "type": "string", "description": "Optional language hint" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "echo",
            "description": "Echo the provided message.",
            "parameters": {
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "Message to echo" }
                },
                "required": ["message"]
            }
        }
    ])
}

// ── Socket protocol (matches vertex_agent / socket_server) ───────────────────

#[cfg(not(unix))]
pub(crate) fn socket_roundtrip(
    _socket_path: &str,
    _req: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    Err(anyhow::anyhow!(
        "orchestration requires Unix (agentd uses Unix domain sockets)"
    ))
}

#[cfg(unix)]
pub(crate) fn socket_roundtrip(
    socket_path: &str,
    req: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    use anyhow::{anyhow, Context};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let req_type = req
        .get("request_type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let start = std::time::Instant::now();
    trace(&format!(
        "socket request -> {} (path={})",
        req_type, socket_path
    ));

    let mut line = serde_json::to_string(req).context("serialize request")?;
    line.push('\n');

    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 0..=3 {
        let mut stream = UnixStream::connect(socket_path).context("UnixStream::connect")?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .context("set read timeout")?;
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(10)))
            .context("set write timeout")?;

        let write_result = (|| -> anyhow::Result<()> {
            let mut bytes_written = 0;
            let bytes = line.as_bytes();
            while bytes_written < bytes.len() {
                match stream.write(&bytes[bytes_written..]) {
                    Ok(0) => return Err(anyhow!("write socket: socket closed by server")),
                    Ok(n) => bytes_written += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::ConnectionReset
                            || e.kind() == std::io::ErrorKind::BrokenPipe =>
                    {
                        return Err(anyhow!("write socket: {}", e));
                    }
                    Err(e) => return Err(anyhow!("write socket: {}", e)),
                }
            }
            stream.flush().map_err(|e| anyhow!("flush socket: {}", e))?;
            Ok(())
        })();

        if let Err(err) = write_result {
            let msg = err.to_string();
            if attempt < 3
                && (msg.contains("connection reset")
                    || msg.contains("socket closed")
                    || msg.contains("broken pipe"))
            {
                std::thread::sleep(std::time::Duration::from_millis(100));
                last_error = Some(err);
                continue;
            }
            return Err(err);
        }

        let mut reader = BufReader::new(&mut stream);
        let mut response_line = String::new();

        let read_result = match reader.read_line(&mut response_line) {
            Ok(0) => Err(anyhow!("read socket: socket closed by server")),
            Ok(_) => Ok(()),
            Err(e)
                if e.kind() == std::io::ErrorKind::ConnectionReset
                    || e.kind() == std::io::ErrorKind::BrokenPipe =>
            {
                Err(anyhow!("read socket: {}", e))
            }
            Err(e) => Err(anyhow!("read socket: {}", e)),
        };

        if let Err(err) = read_result {
            let msg = err.to_string();
            if attempt < 3
                && (msg.contains("connection reset")
                    || msg.contains("socket closed")
                    || msg.contains("broken pipe"))
            {
                std::thread::sleep(std::time::Duration::from_millis(100));
                last_error = Some(err);
                continue;
            }
            return Err(err);
        }

        let trimmed = response_line.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("empty socket response for request: {}", req_type));
        }

        let parsed: serde_json::Value = serde_json::from_str(trimmed)
            .context(format!("parse socket JSON (request: {})", req_type))?;
        let status = parsed
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        if status == "error" {
            if let Some(error_msg) = parsed.get("error").and_then(|e| e.as_str()) {
                eprintln!("⚠️  Socket error for {}: {}", req_type, error_msg);
            }
        }

        trace(&format!(
            "socket response <- {} status={} elapsed_ms={}",
            req_type,
            status,
            start.elapsed().as_millis()
        ));

        return Ok(parsed);
    }

    Err(last_error.unwrap_or_else(|| anyhow!("socket request failed after retries")))
}

#[cfg(not(unix))]
pub(crate) fn parse_ok_field(
    _resp: &serde_json::Value,
    _key: &str,
) -> anyhow::Result<String> {
    Err(anyhow::anyhow!(
        "orchestration requires Unix (agentd uses Unix domain sockets)"
    ))
}

#[cfg(unix)]
pub(crate) fn parse_ok_field(resp: &serde_json::Value, key: &str) -> anyhow::Result<String> {
    use anyhow::anyhow;
    if resp.get("status").and_then(|s| s.as_str()) != Some("ok") {
        let err = resp
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("unknown socket error");
        return Err(anyhow!("socket: {}", err));
    }
    let result = resp
        .get("result")
        .ok_or_else(|| anyhow!("socket response missing result"))?;
    if let Some(s) = result.get(key).and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    if let Some(n) = result.get(key).and_then(|v| v.as_u64()) {
        return Ok(n.to_string());
    }
    Err(anyhow!("result missing string/number field '{}'", key))
}

#[cfg(not(unix))]
pub(crate) fn invoke_tool_via_socket(
    _socket_path: &str,
    _sandbox_id: &str,
    _container_id: &str,
    _tool_name: &str,
    _input: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    Err(anyhow::anyhow!(
        "orchestration requires Unix (agentd uses Unix domain sockets)"
    ))
}

#[cfg(unix)]
pub(crate) fn invoke_tool_via_socket(
    socket_path: &str,
    sandbox_id: &str,
    container_id: &str,
    tool_name: &str,
    input: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    use serde_json::json;
    // Allow only tools that are registered in the agentd tool registry.
    // This keeps the allow-list in sync with the real set of executable tools.
    let allowed = crate::tool_registry::list_all_tools();
    if !allowed.contains(&tool_name) {
        return Ok(json!({
            "error": format!("unknown tool '{}'", tool_name),
            "success": false
        }));
    }
    let req = json!({
        "request_type": "invoke_tool",
        "sandbox": sandbox_id,
        "container": container_id,
        "name": tool_name,
        "input": input
    });
    trace(&format!(
        "invoke_tool request: tool={} sandbox={} container={}",
        tool_name, sandbox_id, container_id
    ));
    let resp = socket_roundtrip(socket_path, &req)?;
    if resp.get("status").and_then(|s| s.as_str()) == Some("ok") {
        Ok(resp.get("result").cloned().unwrap_or(json!({})))
    } else {
        let err = resp
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("tool error");
        Ok(json!({ "error": err, "success": false }))
    }
}
