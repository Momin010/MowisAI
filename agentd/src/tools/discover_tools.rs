use super::common::{Tool, ToolContext};
use serde_json::{json, Value};

/// Meta-tool that returns all available tools, their parameters, and invocation format.
/// Called once at the start of each agent session so the LLM knows its full capability set.
pub struct DiscoverToolsTool;

impl Tool for DiscoverToolsTool {
    fn name(&self) -> &'static str {
        "discover_tools"
    }

    fn invoke(&self, _ctx: &ToolContext, _input: Value) -> anyhow::Result<Value> {
        let tools = build_tool_catalog();
        Ok(json!({
            "total_tools": tools.len(),
            "categories": get_category_summary(),
            "tools": tools,
            "invocation_format": {
                "description": "To invoke any tool, use the tool call mechanism provided by the LLM API. The tool name and arguments are sent to the agentd daemon via Unix socket as:",
                "socket_request": {
                    "request_type": "invoke_tool",
                    "sandbox": "<sandbox_id>",
                    "container": "<container_id>",
                    "name": "<tool_name>",
                    "input": { "<param>": "<value>" }
                },
                "note": "You do not construct socket requests directly. Use the LLM tool-calling mechanism. This format is shown for reference only."
            },
            "working_directory": "/workspace — ALL file operations must use this prefix",
            "rules": [
                "ALWAYS use /workspace prefix for ALL file paths",
                "Write COMPLETE, WORKING code — not placeholders or stubs",
                "Implement FULL functionality — don't leave TODOs or empty functions",
                "Include proper error handling and logging",
                "Test your code before declaring completion"
            ]
        }))
    }

    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(DiscoverToolsTool)
    }
}

fn get_category_summary() -> Value {
    json!({
        "filesystem": { "count": 11, "tools": "read_file, write_file, append_file, delete_file, copy_file, move_file, list_files, create_directory, delete_directory, get_file_info, file_exists" },
        "shell": { "count": 5, "tools": "run_command, run_script, kill_process, get_env, set_env" },
        "http": { "count": 7, "tools": "http_get, http_post, http_put, http_delete, http_patch, download_file, websocket_send" },
        "data": { "count": 5, "tools": "json_parse, json_stringify, json_query, csv_read, csv_write" },
        "git": { "count": 9, "tools": "git_clone, git_status, git_add, git_commit, git_push, git_pull, git_branch, git_checkout, git_diff" },
        "docker": { "count": 7, "tools": "docker_build, docker_run, docker_stop, docker_ps, docker_logs, docker_exec, docker_pull" },
        "kubernetes": { "count": 6, "tools": "kubectl_apply, kubectl_get, kubectl_delete, kubectl_logs, kubectl_exec, kubectl_describe" },
        "memory": { "count": 6, "tools": "memory_set, memory_get, memory_delete, memory_list, memory_save, memory_load" },
        "secrets": { "count": 2, "tools": "secret_set, secret_get" },
        "package_managers": { "count": 3, "tools": "npm_install, pip_install, cargo_add" },
        "web": { "count": 3, "tools": "web_search, web_fetch, web_screenshot" },
        "agent_coordination": { "count": 6, "tools": "create_channel, send_message, read_messages, broadcast, wait_for, spawn_agent" },
        "dev_tools": { "count": 6, "tools": "lint, test, build, type_check, format, discover_tools" },
        "search": { "count": 4, "tools": "grep, find_files, search_code, read_multiple_files" },
        "utility": { "count": 1, "tools": "echo" }
    })
}

fn build_tool_catalog() -> Vec<Value> {
    vec![
        // ── Filesystem ──────────────────────────────────────────────────────
        tool_entry(
            "read_file",
            "filesystem",
            "Read file contents",
            json!({
                "path": { "type": "string", "required": true, "description": "Absolute path to the file (use /workspace prefix)" }
            }),
        ),
        tool_entry(
            "write_file",
            "filesystem",
            "Write content to a file (creates or overwrites)",
            json!({
                "path": { "type": "string", "required": true, "description": "Absolute path (use /workspace prefix)" },
                "content": { "type": "string", "required": true, "description": "Full file content to write" }
            }),
        ),
        tool_entry(
            "append_file",
            "filesystem",
            "Append content to an existing file",
            json!({
                "path": { "type": "string", "required": true, "description": "Absolute path (use /workspace prefix)" },
                "content": { "type": "string", "required": true, "description": "Content to append" }
            }),
        ),
        tool_entry(
            "delete_file",
            "filesystem",
            "Delete a file",
            json!({
                "path": { "type": "string", "required": true, "description": "Absolute path to delete" }
            }),
        ),
        tool_entry(
            "copy_file",
            "filesystem",
            "Copy a file to a new location",
            json!({
                "source": { "type": "string", "required": true, "description": "Source path" },
                "destination": { "type": "string", "required": true, "description": "Destination path" }
            }),
        ),
        tool_entry(
            "move_file",
            "filesystem",
            "Move/rename a file",
            json!({
                "source": { "type": "string", "required": true, "description": "Source path" },
                "destination": { "type": "string", "required": true, "description": "Destination path" }
            }),
        ),
        tool_entry(
            "list_files",
            "filesystem",
            "List files in a directory",
            json!({
                "path": { "type": "string", "required": true, "description": "Directory path (use /workspace prefix)" },
                "pattern": { "type": "string", "required": false, "description": "Glob pattern filter (optional)" }
            }),
        ),
        tool_entry(
            "create_directory",
            "filesystem",
            "Create a directory (recursive)",
            json!({
                "path": { "type": "string", "required": true, "description": "Directory path to create" }
            }),
        ),
        tool_entry(
            "delete_directory",
            "filesystem",
            "Delete a directory and its contents",
            json!({
                "path": { "type": "string", "required": true, "description": "Directory path to delete" }
            }),
        ),
        tool_entry(
            "get_file_info",
            "filesystem",
            "Get file metadata (size, permissions, timestamps)",
            json!({
                "path": { "type": "string", "required": true, "description": "File path" }
            }),
        ),
        tool_entry(
            "file_exists",
            "filesystem",
            "Check if a file or directory exists",
            json!({
                "path": { "type": "string", "required": true, "description": "Path to check" }
            }),
        ),
        // ── Shell ───────────────────────────────────────────────────────────
        tool_entry(
            "run_command",
            "shell",
            "Execute a shell command inside the container (chroot + namespace isolation)",
            json!({
                "cmd": { "type": "string", "required": true, "description": "Command to execute" },
                "cwd": { "type": "string", "required": false, "description": "Working directory (default: /workspace)" },
                "timeout": { "type": "number", "required": false, "description": "Timeout in seconds (default: 30)" }
            }),
        ),
        tool_entry(
            "run_script",
            "shell",
            "Run an inline script or script file",
            json!({
                "script": { "type": "string", "required": true, "description": "Script content or file path" },
                "interpreter": { "type": "string", "required": false, "description": "Interpreter: sh, python, node (auto-detected)" }
            }),
        ),
        tool_entry(
            "kill_process",
            "shell",
            "Send SIGTERM to a process by PID",
            json!({
                "pid": { "type": "number", "required": true, "description": "Process ID to kill" }
            }),
        ),
        tool_entry(
            "get_env",
            "shell",
            "Get environment variable value",
            json!({
                "name": { "type": "string", "required": true, "description": "Variable name" }
            }),
        ),
        tool_entry(
            "set_env",
            "shell",
            "Set an environment variable for the container",
            json!({
                "name": { "type": "string", "required": true, "description": "Variable name" },
                "value": { "type": "string", "required": true, "description": "Variable value" }
            }),
        ),
        // ── HTTP ────────────────────────────────────────────────────────────
        tool_entry(
            "http_get",
            "http",
            "Make an HTTP GET request",
            json!({
                "url": { "type": "string", "required": true, "description": "URL to fetch" },
                "headers": { "type": "object", "required": false, "description": "Request headers" }
            }),
        ),
        tool_entry(
            "http_post",
            "http",
            "Make an HTTP POST request",
            json!({
                "url": { "type": "string", "required": true, "description": "URL" },
                "body": { "type": "string", "required": false, "description": "Request body" },
                "headers": { "type": "object", "required": false, "description": "Request headers" }
            }),
        ),
        tool_entry(
            "http_put",
            "http",
            "Make an HTTP PUT request",
            json!({
                "url": { "type": "string", "required": true, "description": "URL" },
                "body": { "type": "string", "required": false, "description": "Request body" }
            }),
        ),
        tool_entry(
            "http_delete",
            "http",
            "Make an HTTP DELETE request",
            json!({
                "url": { "type": "string", "required": true, "description": "URL" }
            }),
        ),
        tool_entry(
            "http_patch",
            "http",
            "Make an HTTP PATCH request",
            json!({
                "url": { "type": "string", "required": true, "description": "URL" },
                "body": { "type": "string", "required": false, "description": "Request body" }
            }),
        ),
        tool_entry(
            "download_file",
            "http",
            "Download a file from a URL to a local path",
            json!({
                "url": { "type": "string", "required": true, "description": "URL to download" },
                "destination": { "type": "string", "required": true, "description": "Local path to save to" }
            }),
        ),
        tool_entry(
            "websocket_send",
            "http",
            "Send a message over a WebSocket connection",
            json!({
                "url": { "type": "string", "required": true, "description": "WebSocket URL" },
                "message": { "type": "string", "required": true, "description": "Message to send" }
            }),
        ),
        // ── Data ────────────────────────────────────────────────────────────
        tool_entry(
            "json_parse",
            "data",
            "Parse a JSON string into a structured object",
            json!({
                "input": { "type": "string", "required": true, "description": "JSON string to parse" }
            }),
        ),
        tool_entry(
            "json_stringify",
            "data",
            "Convert a JSON object to a formatted string",
            json!({
                "input": { "type": "object", "required": true, "description": "JSON object to stringify" }
            }),
        ),
        tool_entry(
            "json_query",
            "data",
            "Query a JSON object using a path expression",
            json!({
                "input": { "type": "object", "required": true, "description": "JSON object" },
                "path": { "type": "string", "required": true, "description": "JSONPath expression" }
            }),
        ),
        tool_entry(
            "csv_read",
            "data",
            "Read and parse a CSV file",
            json!({
                "path": { "type": "string", "required": true, "description": "CSV file path" },
                "delimiter": { "type": "string", "required": false, "description": "Delimiter (default: comma)" }
            }),
        ),
        tool_entry(
            "csv_write",
            "data",
            "Write data to a CSV file",
            json!({
                "path": { "type": "string", "required": true, "description": "Output CSV path" },
                "data": { "type": "array", "required": true, "description": "Array of row objects" }
            }),
        ),
        // ── Git ─────────────────────────────────────────────────────────────
        tool_entry(
            "git_clone",
            "git",
            "Clone a Git repository",
            json!({
                "url": { "type": "string", "required": true, "description": "Repository URL" },
                "destination": { "type": "string", "required": false, "description": "Clone destination" }
            }),
        ),
        tool_entry("git_status", "git", "Show working tree status", json!({})),
        tool_entry(
            "git_add",
            "git",
            "Stage files for commit",
            json!({
                "paths": { "type": "array", "required": true, "description": "File paths to stage" }
            }),
        ),
        tool_entry(
            "git_commit",
            "git",
            "Create a commit with a message",
            json!({
                "message": { "type": "string", "required": true, "description": "Commit message" }
            }),
        ),
        tool_entry(
            "git_push",
            "git",
            "Push commits to a remote",
            json!({
                "remote": { "type": "string", "required": false, "description": "Remote name (default: origin)" },
                "branch": { "type": "string", "required": false, "description": "Branch name" }
            }),
        ),
        tool_entry(
            "git_pull",
            "git",
            "Pull from a remote",
            json!({
                "remote": { "type": "string", "required": false, "description": "Remote name" }
            }),
        ),
        tool_entry(
            "git_branch",
            "git",
            "List, create, or delete branches",
            json!({
                "name": { "type": "string", "required": false, "description": "Branch name to create" },
                "delete": { "type": "string", "required": false, "description": "Branch name to delete" }
            }),
        ),
        tool_entry(
            "git_checkout",
            "git",
            "Switch branches or restore files",
            json!({
                "branch": { "type": "string", "required": true, "description": "Branch name" }
            }),
        ),
        tool_entry(
            "git_diff",
            "git",
            "Show changes between commits, working tree, etc.",
            json!({
                "cached": { "type": "boolean", "required": false, "description": "Show staged changes" },
                "commit": { "type": "string", "required": false, "description": "Compare against commit" }
            }),
        ),
        // ── Docker ──────────────────────────────────────────────────────────
        tool_entry(
            "docker_build",
            "docker",
            "Build a Docker image from a Dockerfile",
            json!({
                "path": { "type": "string", "required": true, "description": "Build context path" },
                "tag": { "type": "string", "required": false, "description": "Image tag" }
            }),
        ),
        tool_entry(
            "docker_run",
            "docker",
            "Run a Docker container",
            json!({
                "image": { "type": "string", "required": true, "description": "Image name" },
                "command": { "type": "string", "required": false, "description": "Command to run" },
                "detach": { "type": "boolean", "required": false, "description": "Run in background" }
            }),
        ),
        tool_entry(
            "docker_stop",
            "docker",
            "Stop a running container",
            json!({
                "container": { "type": "string", "required": true, "description": "Container ID or name" }
            }),
        ),
        tool_entry(
            "docker_ps",
            "docker",
            "List running containers",
            json!({
                "all": { "type": "boolean", "required": false, "description": "Include stopped containers" }
            }),
        ),
        tool_entry(
            "docker_logs",
            "docker",
            "Get container logs",
            json!({
                "container": { "type": "string", "required": true, "description": "Container ID or name" },
                "tail": { "type": "number", "required": false, "description": "Number of lines" }
            }),
        ),
        tool_entry(
            "docker_exec",
            "docker",
            "Execute a command in a running container",
            json!({
                "container": { "type": "string", "required": true, "description": "Container ID" },
                "command": { "type": "string", "required": true, "description": "Command to execute" }
            }),
        ),
        tool_entry(
            "docker_pull",
            "docker",
            "Pull a Docker image from a registry",
            json!({
                "image": { "type": "string", "required": true, "description": "Image name" }
            }),
        ),
        // ── Kubernetes ──────────────────────────────────────────────────────
        tool_entry(
            "kubectl_apply",
            "kubernetes",
            "Apply a Kubernetes manifest",
            json!({
                "manifest": { "type": "string", "required": true, "description": "YAML manifest content or file path" }
            }),
        ),
        tool_entry(
            "kubectl_get",
            "kubernetes",
            "Get Kubernetes resources",
            json!({
                "resource": { "type": "string", "required": true, "description": "Resource type (pods, services, etc.)" },
                "namespace": { "type": "string", "required": false, "description": "Namespace" }
            }),
        ),
        tool_entry(
            "kubectl_delete",
            "kubernetes",
            "Delete Kubernetes resources",
            json!({
                "resource": { "type": "string", "required": true, "description": "Resource type" },
                "name": { "type": "string", "required": true, "description": "Resource name" }
            }),
        ),
        tool_entry(
            "kubectl_logs",
            "kubernetes",
            "Get pod logs",
            json!({
                "pod": { "type": "string", "required": true, "description": "Pod name" },
                "namespace": { "type": "string", "required": false, "description": "Namespace" }
            }),
        ),
        tool_entry(
            "kubectl_exec",
            "kubernetes",
            "Execute a command in a pod",
            json!({
                "pod": { "type": "string", "required": true, "description": "Pod name" },
                "command": { "type": "string", "required": true, "description": "Command" }
            }),
        ),
        tool_entry(
            "kubectl_describe",
            "kubernetes",
            "Describe a Kubernetes resource",
            json!({
                "resource": { "type": "string", "required": true, "description": "Resource type" },
                "name": { "type": "string", "required": true, "description": "Resource name" }
            }),
        ),
        // ── Memory ──────────────────────────────────────────────────────────
        tool_entry(
            "memory_set",
            "memory",
            "Store a key-value pair in agent memory",
            json!({
                "key": { "type": "string", "required": true, "description": "Memory key" },
                "value": { "type": "any", "required": true, "description": "Value to store" }
            }),
        ),
        tool_entry(
            "memory_get",
            "memory",
            "Retrieve a value from agent memory",
            json!({
                "key": { "type": "string", "required": true, "description": "Memory key" }
            }),
        ),
        tool_entry(
            "memory_delete",
            "memory",
            "Delete a key from agent memory",
            json!({
                "key": { "type": "string", "required": true, "description": "Memory key" }
            }),
        ),
        tool_entry(
            "memory_list",
            "memory",
            "List all keys in agent memory",
            json!({}),
        ),
        tool_entry("memory_save", "memory", "Persist memory to disk", json!({})),
        tool_entry("memory_load", "memory", "Load memory from disk", json!({})),
        // ── Secrets ─────────────────────────────────────────────────────────
        tool_entry(
            "secret_set",
            "secrets",
            "Store a secret securely",
            json!({
                "key": { "type": "string", "required": true, "description": "Secret name" },
                "value": { "type": "string", "required": true, "description": "Secret value" }
            }),
        ),
        tool_entry(
            "secret_get",
            "secrets",
            "Retrieve a secret",
            json!({
                "key": { "type": "string", "required": true, "description": "Secret name" }
            }),
        ),
        // ── Package Managers ────────────────────────────────────────────────
        tool_entry(
            "npm_install",
            "package_managers",
            "Install npm packages",
            json!({
                "packages": { "type": "array", "required": true, "description": "Package names" },
                "dev": { "type": "boolean", "required": false, "description": "Install as devDependency" }
            }),
        ),
        tool_entry(
            "pip_install",
            "package_managers",
            "Install Python packages via pip",
            json!({
                "packages": { "type": "array", "required": true, "description": "Package names" }
            }),
        ),
        tool_entry(
            "cargo_add",
            "package_managers",
            "Add Rust crate dependencies",
            json!({
                "packages": { "type": "array", "required": true, "description": "Crate names" }
            }),
        ),
        // ── Web ─────────────────────────────────────────────────────────────
        tool_entry(
            "web_search",
            "web",
            "Search the web via DuckDuckGo",
            json!({
                "query": { "type": "string", "required": true, "description": "Search query" }
            }),
        ),
        tool_entry(
            "web_fetch",
            "web",
            "Fetch a web page and return its content",
            json!({
                "url": { "type": "string", "required": true, "description": "URL to fetch" }
            }),
        ),
        tool_entry(
            "web_screenshot",
            "web",
            "Take a screenshot of a web page",
            json!({
                "url": { "type": "string", "required": true, "description": "URL to screenshot" }
            }),
        ),
        // ── Agent Coordination ──────────────────────────────────────────────
        tool_entry(
            "create_channel",
            "agent_coordination",
            "Create a communication channel between agents",
            json!({
                "from_agent": { "type": "string", "required": true, "description": "Sender agent ID" },
                "to_agent": { "type": "string", "required": true, "description": "Receiver agent ID" }
            }),
        ),
        tool_entry(
            "send_message",
            "agent_coordination",
            "Send a message on a channel",
            json!({
                "channel": { "type": "number", "required": true, "description": "Channel ID" },
                "message": { "type": "string", "required": true, "description": "Message payload" }
            }),
        ),
        tool_entry(
            "read_messages",
            "agent_coordination",
            "Read messages from a channel",
            json!({
                "channel": { "type": "number", "required": true, "description": "Channel ID" }
            }),
        ),
        tool_entry(
            "broadcast",
            "agent_coordination",
            "Broadcast a message to all agents",
            json!({
                "message": { "type": "string", "required": true, "description": "Message to broadcast" }
            }),
        ),
        tool_entry(
            "wait_for",
            "agent_coordination",
            "Wait for a message or condition",
            json!({
                "channel": { "type": "number", "required": true, "description": "Channel ID" },
                "timeout": { "type": "number", "required": false, "description": "Timeout in seconds" }
            }),
        ),
        tool_entry(
            "spawn_agent",
            "agent_coordination",
            "Spawn a new sub-agent",
            json!({
                "prompt": { "type": "string", "required": true, "description": "Task prompt for the sub-agent" },
                "tools": { "type": "array", "required": false, "description": "Tools to make available" }
            }),
        ),
        // ── Dev Tools ───────────────────────────────────────────────────────
        tool_entry(
            "lint",
            "dev_tools",
            "Run linter on the codebase",
            json!({
                "path": { "type": "string", "required": false, "description": "Path to lint (default: /workspace)" }
            }),
        ),
        tool_entry(
            "test",
            "dev_tools",
            "Run the test suite",
            json!({
                "path": { "type": "string", "required": false, "description": "Test path or pattern" },
                "framework": { "type": "string", "required": false, "description": "Test framework hint" }
            }),
        ),
        tool_entry(
            "build",
            "dev_tools",
            "Build the project",
            json!({
                "command": { "type": "string", "required": false, "description": "Custom build command" }
            }),
        ),
        tool_entry(
            "type_check",
            "dev_tools",
            "Run type checking",
            json!({
                "path": { "type": "string", "required": false, "description": "Path to check" }
            }),
        ),
        tool_entry(
            "format",
            "dev_tools",
            "Format code using the project formatter",
            json!({
                "path": { "type": "string", "required": false, "description": "Path to format" }
            }),
        ),
        // ── Search ──────────────────────────────────────────────────────────
        tool_entry(
            "grep",
            "search",
            "Search for a pattern in files",
            json!({
                "pattern": { "type": "string", "required": true, "description": "Regex pattern" },
                "path": { "type": "string", "required": false, "description": "Directory to search (default: /workspace)" },
                "include": { "type": "string", "required": false, "description": "File pattern filter (e.g., *.rs)" }
            }),
        ),
        tool_entry(
            "find_files",
            "search",
            "Find files by name pattern",
            json!({
                "pattern": { "type": "string", "required": true, "description": "Glob pattern (e.g., **/*.rs)" },
                "path": { "type": "string", "required": false, "description": "Search root (default: /workspace)" }
            }),
        ),
        tool_entry(
            "search_code",
            "search",
            "Semantic code search",
            json!({
                "query": { "type": "string", "required": true, "description": "Natural language query" },
                "path": { "type": "string", "required": false, "description": "Search root" }
            }),
        ),
        tool_entry(
            "read_multiple_files",
            "search",
            "Read multiple files in one call",
            json!({
                "paths": { "type": "array", "required": true, "description": "Array of file paths" }
            }),
        ),
        // ── Utility ─────────────────────────────────────────────────────────
        tool_entry(
            "echo",
            "utility",
            "Echo input back (for testing)",
            json!({
                "message": { "type": "string", "required": true, "description": "Message to echo" }
            }),
        ),
    ]
}

fn tool_entry(name: &str, category: &str, description: &str, parameters: Value) -> Value {
    json!({
        "name": name,
        "category": category,
        "description": description,
        "parameters": parameters
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_discover_tools_returns_catalog() {
        let tool = DiscoverToolsTool;
        let ctx = ToolContext::new(1, Some(PathBuf::from("/tmp/test")));
        let result = tool.invoke(&ctx, json!({})).unwrap();
        assert!(result["total_tools"].as_u64().unwrap() > 80);
        assert!(result["tools"].as_array().unwrap().len() > 80);
        assert!(result["categories"].is_object());
    }

    #[test]
    fn test_all_tools_have_required_fields() {
        let tools = build_tool_catalog();
        for tool in &tools {
            assert!(tool["name"].as_str().is_some(), "tool missing name");
            assert!(tool["category"].as_str().is_some(), "tool missing category");
            assert!(
                tool["description"].as_str().is_some(),
                "tool missing description"
            );
            assert!(tool["parameters"].is_object(), "tool missing parameters");
        }
    }
}
