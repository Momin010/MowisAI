use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::Path;

use crate::sandbox::Sandbox;

pub fn invoke(sandbox: &Sandbox, tool: &str, input: Value) -> Result<Value> {
    match tool {
        // Filesystem
        "read_file" => read_file(sandbox, input),
        "write_file" => write_file(sandbox, input),
        "append_file" => append_file(sandbox, input),
        "delete_file" => delete_file(sandbox, input),
        "copy_file" => copy_file(sandbox, input),
        "move_file" => move_file(sandbox, input),
        "list_files" | "list_dir" => list_dir(sandbox, input),
        "create_directory" => create_directory(sandbox, input),
        "delete_directory" => delete_directory(sandbox, input),
        "get_file_info" => get_file_info(sandbox, input),
        "file_exists" => file_exists(sandbox, input),
        // Shell
        "run_command" => run_command(sandbox, input),
        "run_script" => run_script(sandbox, input),
        // HTTP
        "http_get" => http_get(sandbox, input),
        "http_post" => http_post(input),
        "http_put" => http_put(input),
        "http_delete" => http_delete(input),
        "http_patch" => http_patch(input),
        // Git
        "git_clone" => git_clone(sandbox, input),
        "git_status" => git_status(sandbox, input),
        "git_add" => git_add(sandbox, input),
        "git_commit" => git_commit(sandbox, input),
        "git_diff" => git_diff(sandbox, input),
        "git_branch" => git_branch(sandbox, input),
        "git_checkout" => git_checkout(sandbox, input),
        // Search
        "grep" => grep(sandbox, input),
        "find_files" => find_files(sandbox, input),
        // Utility
        "echo" => echo(input),
        other => Err(anyhow!("tool `{other}` not implemented in executor")),
    }
}

fn resolved_path(sandbox: &Sandbox, rel: &str) -> std::path::PathBuf {
    let trimmed = rel.trim_start_matches('/');
    sandbox.root_path().join(trimmed)
}

// ── Filesystem ────────────────────────────────────────────────────────────────

fn read_file(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("read_file: missing `path`"))?;
    let resolved = resolved_path(sandbox, path);
    let meta = std::fs::metadata(&resolved)?;
    if meta.is_dir() {
        return Ok(json!({"error": "is a directory", "path": path, "success": false}));
    }
    if meta.len() > 10 * 1024 * 1024 {
        return Err(anyhow!("file too large (>10MB): {}", resolved.display()));
    }
    let contents = std::fs::read_to_string(&resolved)?;
    Ok(json!({"content": contents, "size": contents.len(), "success": true}))
}

fn write_file(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("write_file: missing `path`"))?;
    let contents = input.get("contents").or_else(|| input.get("content")).and_then(Value::as_str)
        .ok_or_else(|| anyhow!("write_file: missing `contents`"))?;
    let resolved = resolved_path(sandbox, path);
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&resolved, contents)?;
    Ok(json!({"path": path, "bytes": contents.len(), "success": true}))
}

fn append_file(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("append_file: missing `path`"))?;
    let contents = input.get("contents").or_else(|| input.get("content")).and_then(Value::as_str)
        .ok_or_else(|| anyhow!("append_file: missing `contents`"))?;
    let resolved = resolved_path(sandbox, path);
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&resolved)?;
    file.write_all(contents.as_bytes())?;
    Ok(json!({"path": path, "bytes": contents.len(), "success": true}))
}

fn delete_file(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("delete_file: missing `path`"))?;
    let resolved = resolved_path(sandbox, path);
    std::fs::remove_file(&resolved)?;
    Ok(json!({"path": path, "success": true}))
}

fn copy_file(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let from = input.get("from").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("copy_file: missing `from`"))?;
    let to = input.get("to").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("copy_file: missing `to`"))?;
    let src = resolved_path(sandbox, from);
    let dst = resolved_path(sandbox, to);
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(&src, &dst)?;
    Ok(json!({"from": from, "to": to, "success": true}))
}

fn move_file(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let from = input.get("from").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("move_file: missing `from`"))?;
    let to = input.get("to").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("move_file: missing `to`"))?;
    let src = resolved_path(sandbox, from);
    let dst = resolved_path(sandbox, to);
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&src, &dst)?;
    Ok(json!({"from": from, "to": to, "success": true}))
}

fn list_dir(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str).unwrap_or("/");
    let resolved = resolved_path(sandbox, path);
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&resolved)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        entries.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "is_dir": ft.is_dir(),
            "is_symlink": ft.is_symlink(),
        }));
    }
    Ok(json!({"path": path, "entries": entries, "success": true}))
}

fn create_directory(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("create_directory: missing `path`"))?;
    let resolved = resolved_path(sandbox, path);
    std::fs::create_dir_all(&resolved)?;
    Ok(json!({"path": path, "success": true}))
}

fn delete_directory(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("delete_directory: missing `path`"))?;
    let resolved = resolved_path(sandbox, path);
    std::fs::remove_dir_all(&resolved)?;
    Ok(json!({"path": path, "success": true}))
}

fn get_file_info(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("get_file_info: missing `path`"))?;
    let resolved = resolved_path(sandbox, path);
    let meta = std::fs::metadata(&resolved)?;
    Ok(json!({
        "path": path,
        "size": meta.len(),
        "is_dir": meta.is_dir(),
        "is_file": meta.is_file(),
        "readonly": meta.permissions().readonly(),
        "success": true
    }))
}

fn file_exists(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("file_exists: missing `path`"))?;
    let resolved = resolved_path(sandbox, path);
    Ok(json!({"path": path, "exists": resolved.exists(), "success": true}))
}

// ── Shell ─────────────────────────────────────────────────────────────────────

fn run_command(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let cmd = input.get("cmd").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("run_command: missing `cmd`"))?;
    let cwd = input.get("cwd").and_then(Value::as_str);
    let args: Vec<String> = input.get("args").and_then(|a| serde_json::from_value(a.clone()).ok()).unwrap_or_default();
    let env: Vec<(String, String)> = input.get("env").and_then(|e| serde_json::from_value(e.clone()).ok()).unwrap_or_default();

    let resolved_cwd = cwd.map(|c| resolved_path(sandbox, c));

    let mut command = std::process::Command::new("/bin/sh");
    command.arg("-c");
    command.arg(cmd);
    if !args.is_empty() {
        command.args(&args);
    }
    if let Some(ref c) = resolved_cwd {
        command.current_dir(c);
    }
    for (k, v) in &env {
        command.env(k, v);
    }

    let output = command.output()?;
    Ok(json!({
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "success": output.status.success()
    }))
}

fn run_script(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str);
    let script = input.get("script").and_then(Value::as_str);
    let interpreter = input.get("interpreter").and_then(Value::as_str).unwrap_or("sh");

    let resolved = if let Some(p) = path {
        resolved_path(sandbox, p)
    } else if let Some(s) = script {
        let tmp = sandbox.root_path().join("tmp").join("_script");
        std::fs::create_dir_all(tmp.parent().unwrap())?;
        std::fs::write(&tmp, s)?;
        tmp
    } else {
        return Err(anyhow!("run_script: need `path` or `script`"));
    };

    let output = std::process::Command::new(interpreter)
        .arg(resolved.to_string_lossy().to_string())
        .output()?;
    Ok(json!({
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "success": output.status.success()
    }))
}

// ── HTTP ──────────────────────────────────────────────────────────────────────

fn http_get(_sandbox: &Sandbox, input: Value) -> Result<Value> {
    let url = input.get("url").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("http_get: missing `url`"))?;
    let client = reqwest::blocking::Client::new();
    let resp = client.get(url).send()?;
    let status = resp.status().as_u16();
    let body = resp.text()?;
    Ok(json!({"status": status, "body": body, "success": status < 400}))
}

fn http_post(input: Value) -> Result<Value> {
    let url = input.get("url").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("http_post: missing `url`"))?;
    let body = input.get("body").and_then(Value::as_str).unwrap_or("");
    let client = reqwest::blocking::Client::new();
    let resp = client.post(url).body(body.to_string()).send()?;
    let status = resp.status().as_u16();
    let resp_body = resp.text()?;
    Ok(json!({"status": status, "body": resp_body, "success": status < 400}))
}

fn http_put(input: Value) -> Result<Value> {
    let url = input.get("url").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("http_put: missing `url`"))?;
    let body = input.get("body").and_then(Value::as_str).unwrap_or("");
    let client = reqwest::blocking::Client::new();
    let resp = client.put(url).body(body.to_string()).send()?;
    let status = resp.status().as_u16();
    let resp_body = resp.text()?;
    Ok(json!({"status": status, "body": resp_body, "success": status < 400}))
}

fn http_delete(input: Value) -> Result<Value> {
    let url = input.get("url").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("http_delete: missing `url`"))?;
    let client = reqwest::blocking::Client::new();
    let resp = client.delete(url).send()?;
    let status = resp.status().as_u16();
    let body = resp.text()?;
    Ok(json!({"status": status, "body": body, "success": status < 400}))
}

fn http_patch(input: Value) -> Result<Value> {
    let url = input.get("url").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("http_patch: missing `url`"))?;
    let body = input.get("body").and_then(Value::as_str).unwrap_or("");
    let client = reqwest::blocking::Client::new();
    let resp = client.patch(url).body(body.to_string()).send()?;
    let status = resp.status().as_u16();
    let resp_body = resp.text()?;
    Ok(json!({"status": status, "body": resp_body, "success": status < 400}))
}

// ── Git ───────────────────────────────────────────────────────────────────────

fn git_command(sandbox: &Sandbox, args: &[&str], cwd_path: Option<&str>) -> Result<Value> {
    let cwd = cwd_path.map(|p| resolved_path(sandbox, p));
    let mut cmd = std::process::Command::new("git");
    cmd.args(args);
    if let Some(ref c) = cwd {
        cmd.current_dir(c);
    } else {
        cmd.current_dir(sandbox.root_path());
    }
    let output = cmd.output()?;
    Ok(json!({
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "success": output.status.success()
    }))
}

fn git_clone(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let repo = input.get("repo").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("git_clone: missing `repo`"))?;
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("git_clone: missing `path`"))?;
    let resolved = resolved_path(sandbox, path);
    let output = std::process::Command::new("git")
        .args(["clone", repo, &resolved.to_string_lossy()])
        .output()?;
    Ok(json!({
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "success": output.status.success()
    }))
}

fn git_status(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str);
    git_command(sandbox, &["status", "--porcelain"], path)
}

fn git_add(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("git_add: missing `path`"))?;
    let files: Vec<String> = input.get("files").and_then(|f| serde_json::from_value(f.clone()).ok())
        .ok_or_else(|| anyhow!("git_add: missing `files`"))?;
    let resolved = resolved_path(sandbox, path);
    let mut args = vec!["add"];
    let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    args.extend_from_slice(&file_refs);
    let output = std::process::Command::new("git")
        .args(&args)
        .current_dir(&resolved)
        .output()?;
    Ok(json!({
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "success": output.status.success()
    }))
}

fn git_commit(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("git_commit: missing `path`"))?;
    let message = input.get("message").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("git_commit: missing `message`"))?;
    let resolved = resolved_path(sandbox, path);
    let output = std::process::Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(&resolved)
        .output()?;
    Ok(json!({
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "success": output.status.success()
    }))
}

fn git_diff(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str);
    git_command(sandbox, &["diff"], path)
}

fn git_branch(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str);
    let name = input.get("name").and_then(Value::as_str);
    if let Some(n) = name {
        git_command(sandbox, &["branch", n], path)
    } else {
        git_command(sandbox, &["branch"], path)
    }
}

fn git_checkout(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("git_checkout: missing `path`"))?;
    let branch = input.get("branch").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("git_checkout: missing `branch`"))?;
    let resolved = resolved_path(sandbox, path);
    let output = std::process::Command::new("git")
        .args(["checkout", branch])
        .current_dir(&resolved)
        .output()?;
    Ok(json!({
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "success": output.status.success()
    }))
}

// ── Search ────────────────────────────────────────────────────────────────────

fn grep(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let pattern = input.get("pattern").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("grep: missing `pattern`"))?;
    let path = input.get("path").and_then(Value::as_str).unwrap_or(".");
    let resolved = resolved_path(sandbox, path);
    let include = input.get("include").and_then(Value::as_str);
    let max_results = input.get("max_results").and_then(Value::as_u64).unwrap_or(100) as usize;

    let mut cmd = std::process::Command::new("grep");
    cmd.args(["-rn", "--color=never", pattern]);
    if let Some(inc) = include {
        cmd.args(["--include", inc]);
    }
    cmd.arg(resolved.to_string_lossy().to_string());

    let output = cmd.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().take(max_results).collect();
    Ok(json!({
        "matches": lines,
        "count": lines.len(),
        "truncated": stdout.lines().count() > max_results,
        "success": true
    }))
}

fn find_files(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let pattern = input.get("pattern").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("find_files: missing `pattern`"))?;
    let path = input.get("path").and_then(Value::as_str).unwrap_or(".");
    let max_depth = input.get("max_depth").and_then(Value::as_u64).unwrap_or(10);
    let resolved = resolved_path(sandbox, path);

    let output = std::process::Command::new("find")
        .arg(resolved.to_string_lossy().to_string())
        .args(["-maxdepth", &max_depth.to_string()])
        .args(["-name", pattern])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files: Vec<&str> = stdout.lines().collect();
    Ok(json!({
        "files": files,
        "count": files.len(),
        "success": true
    }))
}

// ── Utility ───────────────────────────────────────────────────────────────────

fn echo(input: Value) -> Result<Value> {
    let message = input.get("message").and_then(Value::as_str).unwrap_or("");
    Ok(json!({"output": message, "success": true}))
}
