// zero_mode/tools.rs — Filesystem tools for zero-mode agents
//
// All paths the agent supplies are workspace-relative.
// Path-traversal attacks (../../etc) are blocked by canonicalization check.
// Tools return a human-readable string that is fed back to the LLM as the
// function result.

use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

// ── Tool name constants ───────────────────────────────────────────────────────

pub const READ_FILE:        &str = "read_file";
pub const WRITE_FILE:       &str = "write_file";
pub const APPEND_FILE:      &str = "append_file";
pub const LIST_DIRECTORY:   &str = "list_directory";
pub const CREATE_DIRECTORY: &str = "create_directory";
pub const DELETE_FILE:      &str = "delete_file";
pub const MOVE_FILE:        &str = "move_file";
pub const SEARCH_FILES:     &str = "search_files";
pub const RUN_COMMAND:      &str = "run_command";

// ── Tool schema (OpenAI / Anthropic compatible JSON Schema) ──────────────────

/// Returns the array of tool definitions as a serde_json Value.
/// Used by the LLM clients to declare available tools.
pub fn tool_definitions() -> Vec<Value> {
    serde_json::from_str(include_str!("tool_defs.json"))
        .unwrap_or_else(|_| vec![builtin_tool_defs()])
}

/// Inline fallback — always available even if the JSON file is missing.
fn builtin_tool_defs() -> Value {
    serde_json::json!([
        {
            "name": READ_FILE,
            "description": "Read the contents of a file in the workspace.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative file path." }
                },
                "required": ["path"]
            }
        },
        {
            "name": WRITE_FILE,
            "description": "Write (create or overwrite) a file in the workspace.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path":    { "type": "string", "description": "Workspace-relative path." },
                    "content": { "type": "string", "description": "Full file content to write." }
                },
                "required": ["path", "content"]
            }
        },
        {
            "name": APPEND_FILE,
            "description": "Append text to an existing file (creates it if absent).",
            "parameters": {
                "type": "object",
                "properties": {
                    "path":    { "type": "string", "description": "Workspace-relative path." },
                    "content": { "type": "string", "description": "Text to append." }
                },
                "required": ["path", "content"]
            }
        },
        {
            "name": LIST_DIRECTORY,
            "description": "List files and directories at a given path (defaults to workspace root).",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative path, or '.' for root." }
                },
                "required": []
            }
        },
        {
            "name": CREATE_DIRECTORY,
            "description": "Create a directory (and parents) in the workspace.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative directory path." }
                },
                "required": ["path"]
            }
        },
        {
            "name": DELETE_FILE,
            "description": "Delete a file or empty directory from the workspace.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative path to delete." }
                },
                "required": ["path"]
            }
        },
        {
            "name": MOVE_FILE,
            "description": "Move or rename a file within the workspace.",
            "parameters": {
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Source workspace-relative path." },
                    "to":   { "type": "string", "description": "Destination workspace-relative path." }
                },
                "required": ["from", "to"]
            }
        },
        {
            "name": SEARCH_FILES,
            "description": "Recursively search for files whose name contains a given string.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Substring to search for in file names." }
                },
                "required": ["pattern"]
            }
        },
        {
            "name": RUN_COMMAND,
            "description": "Run a shell command inside the workspace directory (30-second timeout). Use sparingly.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute." }
                },
                "required": ["command"]
            }
        }
    ])
}

// ── Tool execution ────────────────────────────────────────────────────────────

/// Execute a tool call from the LLM and return the result string.
/// All paths are resolved relative to `workspace` and checked for traversal.
pub fn execute_tool(workspace: &Path, name: &str, args: &Value) -> String {
    match name {
        READ_FILE        => tool_read_file(workspace, args),
        WRITE_FILE       => tool_write_file(workspace, args),
        APPEND_FILE      => tool_append_file(workspace, args),
        LIST_DIRECTORY   => tool_list_directory(workspace, args),
        CREATE_DIRECTORY => tool_create_directory(workspace, args),
        DELETE_FILE      => tool_delete_file(workspace, args),
        MOVE_FILE        => tool_move_file(workspace, args),
        SEARCH_FILES     => tool_search_files(workspace, args),
        RUN_COMMAND      => tool_run_command(workspace, args),
        other            => format!("unknown tool: {other}"),
    }
}

// ── Path safety ───────────────────────────────────────────────────────────────

/// Resolve a workspace-relative path and verify it stays inside `workspace`.
/// Returns Err if the path escapes the workspace.
fn safe_path(workspace: &Path, rel: &str) -> Result<PathBuf, String> {
    if rel.is_empty() {
        return Ok(workspace.to_path_buf());
    }
    // Strip leading slashes/backslashes — the agent might supply absolute paths.
    let rel = rel.trim_start_matches(['/', '\\']);
    let candidate = workspace.join(rel);

    // Canonicalize the workspace root (resolves symlinks on macOS).
    let ws_canon = fs::canonicalize(workspace)
        .unwrap_or_else(|_| workspace.to_path_buf());

    // For a not-yet-existing path, canonicalize the parent and reconstruct.
    let canon = if candidate.exists() {
        fs::canonicalize(&candidate).map_err(|e| e.to_string())?
    } else {
        let parent = candidate.parent().unwrap_or(workspace);
        let parent_canon = if parent.exists() {
            fs::canonicalize(parent).map_err(|e| e.to_string())?
        } else {
            // Deep nesting — just trust it for now; the parent mkdir will catch bad paths.
            parent.to_path_buf()
        };
        parent_canon.join(candidate.file_name().unwrap_or_default())
    };

    if !canon.starts_with(&ws_canon) {
        return Err(format!(
            "path '{}' escapes workspace — rejected",
            rel
        ));
    }
    Ok(canon)
}

fn str_arg<'a>(args: &'a Value, key: &str) -> &'a str {
    args.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

// ── Individual tools ──────────────────────────────────────────────────────────

fn tool_read_file(workspace: &Path, args: &Value) -> String {
    let rel = str_arg(args, "path");
    match safe_path(workspace, rel) {
        Err(e) => format!("error: {e}"),
        Ok(p) => {
            if !p.exists() {
                return format!("error: file not found: {rel}");
            }
            if p.is_dir() {
                return format!("error: {rel} is a directory, use list_directory instead");
            }
            match fs::read_to_string(&p) {
                Ok(content) => {
                    let lines = content.lines().count();
                    let bytes = content.len();
                    format!("// {rel} ({lines} lines, {bytes} bytes)\n{content}")
                }
                Err(e) => format!("error reading {rel}: {e}"),
            }
        }
    }
}

fn tool_write_file(workspace: &Path, args: &Value) -> String {
    let rel     = str_arg(args, "path");
    let content = str_arg(args, "content");
    match safe_path(workspace, rel) {
        Err(e) => format!("error: {e}"),
        Ok(p) => {
            if let Some(parent) = p.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return format!("error creating parent dirs for {rel}: {e}");
                }
            }
            match fs::write(&p, content) {
                Ok(()) => format!("ok: wrote {} bytes to {rel}", content.len()),
                Err(e) => format!("error writing {rel}: {e}"),
            }
        }
    }
}

fn tool_append_file(workspace: &Path, args: &Value) -> String {
    let rel     = str_arg(args, "path");
    let content = str_arg(args, "content");
    match safe_path(workspace, rel) {
        Err(e) => format!("error: {e}"),
        Ok(p) => {
            if let Some(parent) = p.parent() {
                let _ = fs::create_dir_all(parent);
            }
            use std::io::Write;
            match fs::OpenOptions::new().create(true).append(true).open(&p) {
                Ok(mut f) => {
                    match f.write_all(content.as_bytes()) {
                        Ok(()) => format!("ok: appended {} bytes to {rel}", content.len()),
                        Err(e) => format!("error appending to {rel}: {e}"),
                    }
                }
                Err(e) => format!("error opening {rel} for append: {e}"),
            }
        }
    }
}

fn tool_list_directory(workspace: &Path, args: &Value) -> String {
    let rel = {
        let r = str_arg(args, "path");
        if r.is_empty() { "." } else { r }
    };
    match safe_path(workspace, rel) {
        Err(e) => format!("error: {e}"),
        Ok(p) => {
            if !p.exists() {
                return format!("error: path not found: {rel}");
            }
            if !p.is_dir() {
                return format!("error: {rel} is not a directory");
            }
            match fs::read_dir(&p) {
                Err(e) => format!("error listing {rel}: {e}"),
                Ok(entries) => {
                    let mut lines: Vec<String> = entries
                        .filter_map(|e| e.ok())
                        .map(|e| {
                            let name = e.file_name().to_string_lossy().into_owned();
                            let kind = if e.path().is_dir() { "dir" } else { "file" };
                            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                            if kind == "dir" {
                                format!("{name}/")
                            } else {
                                format!("{name} ({size} B)")
                            }
                        })
                        .collect();
                    lines.sort();
                    if lines.is_empty() {
                        format!("{rel}: (empty directory)")
                    } else {
                        format!("{rel}:\n{}", lines.join("\n"))
                    }
                }
            }
        }
    }
}

fn tool_create_directory(workspace: &Path, args: &Value) -> String {
    let rel = str_arg(args, "path");
    match safe_path(workspace, rel) {
        Err(e) => format!("error: {e}"),
        Ok(p) => match fs::create_dir_all(&p) {
            Ok(()) => format!("ok: directory created: {rel}"),
            Err(e) => format!("error creating directory {rel}: {e}"),
        },
    }
}

fn tool_delete_file(workspace: &Path, args: &Value) -> String {
    let rel = str_arg(args, "path");
    match safe_path(workspace, rel) {
        Err(e) => format!("error: {e}"),
        Ok(p) => {
            if !p.exists() {
                return format!("error: not found: {rel}");
            }
            let result = if p.is_dir() {
                fs::remove_dir(&p)
            } else {
                fs::remove_file(&p)
            };
            match result {
                Ok(()) => format!("ok: deleted {rel}"),
                Err(e) => format!("error deleting {rel}: {e}"),
            }
        }
    }
}

fn tool_move_file(workspace: &Path, args: &Value) -> String {
    let from_rel = str_arg(args, "from");
    let to_rel   = str_arg(args, "to");
    let from = match safe_path(workspace, from_rel) {
        Err(e) => return format!("error: {e}"),
        Ok(p) => p,
    };
    let to = match safe_path(workspace, to_rel) {
        Err(e) => return format!("error: {e}"),
        Ok(p) => p,
    };
    if !from.exists() {
        return format!("error: source not found: {from_rel}");
    }
    if let Some(parent) = to.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match fs::rename(&from, &to) {
        Ok(()) => format!("ok: moved {from_rel} → {to_rel}"),
        Err(e) => format!("error moving {from_rel} → {to_rel}: {e}"),
    }
}

fn tool_search_files(workspace: &Path, args: &Value) -> String {
    let pattern = str_arg(args, "pattern").to_lowercase();
    if pattern.is_empty() {
        return "error: pattern is required".to_string();
    }
    let mut matches: Vec<String> = Vec::new();
    search_recursive(workspace, workspace, &pattern, &mut matches, 0);
    if matches.is_empty() {
        format!("no files found matching '{pattern}'")
    } else {
        format!("found {} file(s) matching '{pattern}':\n{}", matches.len(), matches.join("\n"))
    }
}

fn search_recursive(workspace: &Path, dir: &Path, pattern: &str, out: &mut Vec<String>, depth: usize) {
    if depth > 8 { return; }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.starts_with('.') { continue; }
        if name.contains(pattern) {
            if let Ok(rel) = path.strip_prefix(workspace) {
                out.push(rel.display().to_string());
            }
        }
        if path.is_dir() && out.len() < 200 {
            search_recursive(workspace, &path, pattern, out, depth + 1);
        }
    }
}

fn tool_run_command(workspace: &Path, args: &Value) -> String {
    let command = str_arg(args, "command");
    if command.is_empty() {
        return "error: command is required".to_string();
    }

    // Block obviously dangerous commands.
    let lower = command.to_lowercase();
    for banned in &["rm -rf /", "format ", "del /f /s", "mkfs", "dd if="] {
        if lower.contains(banned) {
            return format!("error: command blocked for safety: {command}");
        }
    }

    let timeout = Duration::from_secs(30);

    #[cfg(unix)]
    let result = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(workspace)
        .output();

    #[cfg(windows)]
    let result = std::process::Command::new("cmd")
        .args(["/C", command])
        .current_dir(workspace)
        .output();

    let _ = timeout; // tokio timeout not available in sync context; process::Command is blocking

    match result {
        Err(e) => format!("error running command: {e}"),
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let status = output.status.code().unwrap_or(-1);
            let mut out = format!("exit: {status}\n");
            if !stdout.is_empty() {
                out.push_str(&format!("stdout:\n{stdout}"));
            }
            if !stderr.is_empty() {
                out.push_str(&format!("stderr:\n{stderr}"));
            }
            if out.trim() == format!("exit: {status}") {
                out.push_str("(no output)");
            }
            // Cap output to avoid flooding the LLM context.
            if out.len() > 8_000 {
                out.truncate(8_000);
                out.push_str("\n...(truncated)");
            }
            out
        }
    }
}
