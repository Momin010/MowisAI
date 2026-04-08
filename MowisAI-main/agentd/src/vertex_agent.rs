//! Agent loop: Vertex AI Gemini 2.5 ↔ agentd Unix socket (sandbox tools).
//!
//! On non-Unix targets the entrypoint returns an error (Unix sockets required).

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::Command;
use std::time::Duration;

const MAX_TOOL_ROUNDS: usize = 64;
const HTTP_TIMEOUT_SECS: u64 = 180;

/// Run the Gemini ↔ agentd tool loop until the model returns a final text answer.
pub fn run(prompt: &str, project_id: &str, socket_path: &str) -> Result<()> {
    #[cfg(unix)]
    {
        run_inner(prompt, project_id, socket_path)
    }
    #[cfg(not(unix))]
    {
        let _ = (prompt, project_id, socket_path);
        Err(anyhow!(
            "vertex_agent requires Unix (agentd uses Unix domain sockets)"
        ))
    }
}

#[cfg(unix)]
fn run_inner(prompt: &str, project_id: &str, socket_path: &str) -> Result<()> {
    println!("[vertex] creating sandbox via {} …", socket_path);
    let create_sb = json!({
        "request_type": "create_sandbox",
        "image": "alpine"
    });
    let sb_resp = socket_roundtrip(socket_path, &create_sb)?;
    let sandbox_id = parse_ok_field(&sb_resp, "sandbox").context("create_sandbox")?;
    println!("[vertex] sandbox id {}", sandbox_id);

    println!("[vertex] creating container…");
    let create_ct = json!({
        "request_type": "create_container",
        "sandbox": &sandbox_id
    });
    let ct_resp = socket_roundtrip(socket_path, &create_ct)?;
    let container_id = parse_ok_field(&ct_resp, "container").context("create_container")?;
    println!("[vertex] container id {}", container_id);

    let url = format!(
        "https://us-central1-aiplatform.googleapis.com/v1/projects/{}/locations/us-central1/publishers/google/models/gemini-2.5-pro:generateContent",
        project_id
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .context("reqwest client")?;

    let system_instruction = json!({
        "parts": [{ "text": "You control a Linux sandbox (Alpine) via tools. Use paths under /workspace when writing files unless the user specifies otherwise. Prefer listing directories before assuming files exist." }]
    });

    let mut contents: Vec<Value> = vec![json!({
        "role": "user",
        "parts": [{ "text": prompt }]
    })];

    let tools = json!([{
        "functionDeclarations": gemini_tool_declarations()
    }]);

    for round in 0..MAX_TOOL_ROUNDS {
        let body = json!({
            "contents": contents,
            "tools": tools,
            "systemInstruction": system_instruction,
            "generationConfig": {
                "temperature": 0.5
            }
        });

        println!("[vertex] → Gemini (round {}) …", round + 1);
        let token = gcloud_access_token()?;
        let http_resp = client
            .post(&url)
            .bearer_auth(&token)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .context("generateContent HTTP")?;

        if !http_resp.status().is_success() {
            let status = http_resp.status();
            let text = http_resp.text().unwrap_or_default();
            return Err(anyhow!("Vertex AI error {}: {}", status, text));
        }

        let response: Value = http_resp.json().context("parse Vertex JSON")?;

        if let Some(block) = response
            .pointer("/promptFeedback/blockReason")
            .and_then(|v| v.as_str())
        {
            return Err(anyhow!("prompt blocked: {}", block));
        }

        let candidate = response
            .get("candidates")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .ok_or_else(|| anyhow!("no candidates in response: {}", response))?;

        let content = candidate
            .get("content")
            .ok_or_else(|| anyhow!("candidate missing content"))?;
        let parts = content
            .get("parts")
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();

        let mut model_parts: Vec<Value> = Vec::new();
        let mut function_calls: Vec<(String, Value)> = Vec::new();

        for part in &parts {
            if let Some(fc) = part.get("functionCall") {
                let name = fc
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = fc.get("args").cloned().unwrap_or(json!({}));
                println!("[tool-call] {} {}", name, args);
                function_calls.push((name, args));
                model_parts.push(part.clone());
            } else if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                if !t.is_empty() {
                    println!("[model]\n{}", t);
                }
                model_parts.push(part.clone());
            } else {
                model_parts.push(part.clone());
            }
        }

        if function_calls.is_empty() {
            println!("[vertex] done (no further tool calls).");
            return Ok(());
        }

        contents.push(json!({
            "role": "model",
            "parts": model_parts
        }));

        let mut response_parts = Vec::new();
        for (name, args) in function_calls {
            let tool_result = invoke_tool_via_socket(
                socket_path,
                &sandbox_id,
                &container_id,
                &name,
                &args,
            )?;
            println!("[tool-result] {} → {}", name, tool_result);
            response_parts.push(json!({
                "functionResponse": {
                    "name": name,
                    "response": tool_result
                }
            }));
        }

        contents.push(json!({
            "role": "user",
            "parts": response_parts
        }));
    }

    Err(anyhow!(
        "exceeded max tool rounds ({}) without final answer",
        MAX_TOOL_ROUNDS
    ))
}

#[cfg(unix)]
fn gcloud_access_token() -> Result<String> {
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
    Ok(t)
}

#[cfg(unix)]
fn gemini_tool_declarations() -> Value {
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

#[cfg(unix)]
fn parse_ok_field(resp: &Value, key: &str) -> Result<String> {
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

#[cfg(unix)]
fn socket_roundtrip(socket_path: &str, req: &Value) -> Result<Value> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path).context("UnixStream::connect")?;
    let mut line = serde_json::to_string(req).context("serialize request")?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .context("write socket")?;
    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .context("read socket")?;
    let trimmed = response_line.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("empty socket response"));
    }
    serde_json::from_str(trimmed).context("parse socket JSON")
}

#[cfg(unix)]
fn invoke_tool_via_socket(
    socket_path: &str,
    sandbox_id: &str,
    container_id: &str,
    tool_name: &str,
    input: &Value,
) -> Result<Value> {
    use std::os::unix::net::UnixStream;

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
