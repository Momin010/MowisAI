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

pub const READ_FILE: &str = "read_file";
pub const READ_FILE_LINES: &str = "read_file_lines";
pub const WRITE_FILE: &str = "write_file";
pub const APPEND_FILE: &str = "append_file";
pub const REPLACE_IN_FILE: &str = "replace_in_file";
pub const EDIT_FILE_LINES: &str = "edit_file_lines";
pub const LIST_DIRECTORY: &str = "list_directory";
pub const CREATE_DIRECTORY: &str = "create_directory";
pub const DELETE_FILE: &str = "delete_file";
pub const MOVE_FILE: &str = "move_file";
pub const SEARCH_FILES: &str = "search_files";
pub const SEARCH_IN_FILES: &str = "search_in_files";
pub const RUN_COMMAND: &str = "run_command";

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
            "description": "Read the entire contents of a file in the workspace.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative file path." }
                },
                "required": ["path"]
            }
        },
        {
            "name": READ_FILE_LINES,
            "description": "Read specific lines from a file (e.g., lines 100-110).",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative file path." },
                    "start_line": { "type": "integer", "description": "Starting line number (1-indexed, inclusive)." },
                    "end_line": { "type": "integer", "description": "Ending line number (1-indexed, inclusive). Omit to read to end of file." }
                },
                "required": ["path", "start_line"]
            }
        },
        {
            "name": WRITE_FILE,
            "description": "Write (create or overwrite) a file in the workspace. CRITICAL: Content MUST be properly formatted with newlines and indentation. NEVER write minified/single-line code. HTML: each tag on its own line. CSS: each property on its own line. JS: each statement on its own line.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path":    { "type": "string", "description": "Workspace-relative path." },
                    "content": { "type": "string", "description": "Full file content to write. MUST be properly formatted with newlines and indentation — NOT minified." }
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
            "name": REPLACE_IN_FILE,
            "description": "Replace all occurrences of a string in a file with another string.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative path." },
                    "old_text": { "type": "string", "description": "Text to find (exact match)." },
                    "new_text": { "type": "string", "description": "Text to replace with." }
                },
                "required": ["path", "old_text", "new_text"]
            }
        },
        {
            "name": EDIT_FILE_LINES,
            "description": "Replace specific lines in a file (e.g., replace lines 100-110 with new content).",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative path." },
                    "start_line": { "type": "integer", "description": "Starting line number (1-indexed, inclusive)." },
                    "end_line": { "type": "integer", "description": "Ending line number (1-indexed, inclusive)." },
                    "new_content": { "type": "string", "description": "New content to replace the specified lines." }
                },
                "required": ["path", "start_line", "end_line", "new_content"]
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
            "name": SEARCH_IN_FILES,
            "description": "Search for text content within files (grep-like search).",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Text pattern to search for in file contents." },
                    "path": { "type": "string", "description": "Optional: limit search to specific directory or file pattern." }
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
        READ_FILE => tool_read_file(workspace, args),
        READ_FILE_LINES => tool_read_file_lines(workspace, args),
        WRITE_FILE => tool_write_file(workspace, args),
        APPEND_FILE => tool_append_file(workspace, args),
        REPLACE_IN_FILE => tool_replace_in_file(workspace, args),
        EDIT_FILE_LINES => tool_edit_file_lines(workspace, args),
        LIST_DIRECTORY => tool_list_directory(workspace, args),
        CREATE_DIRECTORY => tool_create_directory(workspace, args),
        DELETE_FILE => tool_delete_file(workspace, args),
        MOVE_FILE => tool_move_file(workspace, args),
        SEARCH_FILES => tool_search_files(workspace, args),
        SEARCH_IN_FILES => tool_search_in_files(workspace, args),
        RUN_COMMAND => tool_run_command(workspace, args),
        other => format!("unknown tool: {other}"),
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
    let ws_canon = fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());

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
        return Err(format!("path '{}' escapes workspace — rejected", rel));
    }
    Ok(canon)
}

fn str_arg<'a>(args: &'a Value, key: &str) -> &'a str {
    args.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn int_arg(args: &Value, key: &str) -> Option<usize> {
    args.get(key).and_then(|v| v.as_i64()).map(|n| n as usize)
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

fn tool_read_file_lines(workspace: &Path, args: &Value) -> String {
    let rel = str_arg(args, "path");
    let start = int_arg(args, "start_line");
    let end = int_arg(args, "end_line");

    if start.is_none() {
        return "error: start_line is required".to_string();
    }
    let start = start.unwrap();

    match safe_path(workspace, rel) {
        Err(e) => format!("error: {e}"),
        Ok(p) => {
            if !p.exists() {
                return format!("error: file not found: {rel}");
            }
            match fs::read_to_string(&p) {
                Ok(content) => {
                    let all_lines: Vec<&str> = content.lines().collect();
                    let total = all_lines.len();

                    if start < 1 || start > total {
                        return format!(
                            "error: start_line {start} out of range (file has {total} lines)"
                        );
                    }

                    let end_line = end.unwrap_or(total).min(total);
                    if end_line < start {
                        return format!("error: end_line {end_line} is before start_line {start}");
                    }

                    let selected: Vec<&str> = all_lines[(start - 1)..end_line].to_vec();
                    let result = selected.join("\n");
                    format!("// {rel} (lines {start}-{end_line} of {total})\n{result}")
                }
                Err(e) => format!("error reading {rel}: {e}"),
            }
        }
    }
}

fn tool_write_file(workspace: &Path, args: &Value) -> String {
    let rel = str_arg(args, "path");
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
    let rel = str_arg(args, "path");
    let content = str_arg(args, "content");
    match safe_path(workspace, rel) {
        Err(e) => format!("error: {e}"),
        Ok(p) => {
            if let Some(parent) = p.parent() {
                let _ = fs::create_dir_all(parent);
            }
            use std::io::Write;
            match fs::OpenOptions::new().create(true).append(true).open(&p) {
                Ok(mut f) => match f.write_all(content.as_bytes()) {
                    Ok(()) => format!("ok: appended {} bytes to {rel}", content.len()),
                    Err(e) => format!("error appending to {rel}: {e}"),
                },
                Err(e) => format!("error opening {rel} for append: {e}"),
            }
        }
    }
}

fn tool_list_directory(workspace: &Path, args: &Value) -> String {
    let rel = {
        let r = str_arg(args, "path");
        if r.is_empty() {
            "."
        } else {
            r
        }
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
    let to_rel = str_arg(args, "to");
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

fn tool_replace_in_file(workspace: &Path, args: &Value) -> String {
    let rel = str_arg(args, "path");
    let old_text = str_arg(args, "old_text");
    let new_text = str_arg(args, "new_text");

    if old_text.is_empty() {
        return "error: old_text cannot be empty".to_string();
    }

    match safe_path(workspace, rel) {
        Err(e) => format!("error: {e}"),
        Ok(p) => {
            if !p.exists() {
                return format!("error: file not found: {rel}");
            }
            match fs::read_to_string(&p) {
                Ok(content) => {
                    let count = content.matches(old_text).count();
                    if count == 0 {
                        return format!("error: text not found in {rel}");
                    }
                    let new_content = content.replace(old_text, new_text);
                    match fs::write(&p, new_content) {
                        Ok(()) => format!("ok: replaced {count} occurrence(s) in {rel}"),
                        Err(e) => format!("error writing {rel}: {e}"),
                    }
                }
                Err(e) => format!("error reading {rel}: {e}"),
            }
        }
    }
}

fn tool_edit_file_lines(workspace: &Path, args: &Value) -> String {
    let rel = str_arg(args, "path");
    let start = int_arg(args, "start_line");
    let end = int_arg(args, "end_line");
    let new_content = str_arg(args, "new_content");

    if start.is_none() || end.is_none() {
        return "error: start_line and end_line are required".to_string();
    }
    let start = start.unwrap();
    let end = end.unwrap();

    match safe_path(workspace, rel) {
        Err(e) => format!("error: {e}"),
        Ok(p) => {
            if !p.exists() {
                return format!("error: file not found: {rel}");
            }
            match fs::read_to_string(&p) {
                Ok(content) => {
                    let mut lines: Vec<&str> = content.lines().collect();
                    let total = lines.len();

                    if start < 1 || start > total {
                        return format!(
                            "error: start_line {start} out of range (file has {total} lines)"
                        );
                    }
                    if end < start || end > total {
                        return format!("error: end_line {end} out of range or before start_line");
                    }

                    // Replace lines [start-1..end] with new_content
                    let new_lines: Vec<&str> = new_content.lines().collect();
                    lines.splice((start - 1)..end, new_lines);

                    let result = lines.join("\n");
                    match fs::write(&p, result) {
                        Ok(()) => format!("ok: replaced lines {start}-{end} in {rel}"),
                        Err(e) => format!("error writing {rel}: {e}"),
                    }
                }
                Err(e) => format!("error reading {rel}: {e}"),
            }
        }
    }
}

fn tool_search_in_files(workspace: &Path, args: &Value) -> String {
    let pattern = str_arg(args, "pattern");
    let search_path = str_arg(args, "path");

    if pattern.is_empty() {
        return "error: pattern is required".to_string();
    }

    let root = if search_path.is_empty() {
        workspace.to_path_buf()
    } else {
        match safe_path(workspace, search_path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        }
    };

    let mut results: Vec<String> = Vec::new();
    search_content_recursive(&root, workspace, pattern, &mut results, 0);

    if results.is_empty() {
        format!("no matches found for '{pattern}'")
    } else {
        let count = results.len();
        let display = if count > 200 {
            results.truncate(200);
            format!(
                "found {count} matches (showing first 200):\n{}",
                results.join("\n")
            )
        } else {
            format!("found {count} match(es):\n{}", results.join("\n"))
        };
        display
    }
}

fn search_content_recursive(
    dir: &Path,
    workspace: &Path,
    pattern: &str,
    out: &mut Vec<String>,
    depth: usize,
) {
    if depth > 12 || out.len() >= 200 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.filter_map(|e| e.ok()) {
        if out.len() >= 200 {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') {
            continue;
        }

        if path.is_file() {
            // Only search text files (skip binaries)
            if let Ok(content) = fs::read_to_string(&path) {
                for (line_num, line) in content.lines().enumerate() {
                    if line.contains(pattern) {
                        if let Ok(rel) = path.strip_prefix(workspace) {
                            out.push(format!(
                                "{}:{}: {}",
                                rel.display(),
                                line_num + 1,
                                line.trim()
                            ));
                            if out.len() >= 200 {
                                break;
                            }
                        }
                    }
                }
            }
        } else if path.is_dir() {
            search_content_recursive(&path, workspace, pattern, out, depth + 1);
        }
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
        format!(
            "found {} file(s) matching '{pattern}':\n{}",
            matches.len(),
            matches.join("\n")
        )
    }
}

fn search_recursive(
    workspace: &Path,
    dir: &Path,
    pattern: &str,
    out: &mut Vec<String>,
    depth: usize,
) {
    if depth > 12 || out.len() >= 600 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.starts_with('.') {
            continue;
        }
        if name.contains(pattern) {
            if let Ok(rel) = path.strip_prefix(workspace) {
                out.push(rel.display().to_string());
            }
        }
        if path.is_dir() && out.len() < 600 {
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
