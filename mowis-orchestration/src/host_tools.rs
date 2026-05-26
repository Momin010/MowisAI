use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

/// Execute a tool call directly on the host (no VM, no chroot).
/// This is the "local" mode — tools run in the host's filesystem.
pub fn execute_tool_local(tool_name: &str, input: &Value, work_dir: &PathBuf) -> Result<Value> {
    tracing::debug!(tool = tool_name, "executing tool locally");

    let result = match tool_name {
        "read_file" => read_file(input, work_dir),
        "write_file" => write_file(input, work_dir),
        "append_file" => append_file(input, work_dir),
        "delete_file" => delete_file(input, work_dir),
        "list_files" | "list_dir" => list_dir(input, work_dir),
        "create_directory" => create_directory(input, work_dir),
        "file_exists" => file_exists(input, work_dir),
        "get_file_info" => get_file_info(input, work_dir),
        "copy_file" => copy_file(input, work_dir),
        "move_file" => move_file(input, work_dir),
        "run_command" => run_command(input, work_dir),
        "run_script" => run_script(input, work_dir),
        "grep" => grep(input, work_dir),
        "find_files" => find_files(input, work_dir),
        "git_status" => git_command(&["status", "--porcelain"], work_dir, None),
        "git_add" => git_add(input, work_dir),
        "git_commit" => git_commit(input, work_dir),
        "git_diff" => git_command(&["diff"], work_dir, None),
        "git_branch" => git_branch(input, work_dir),
        "git_checkout" => git_checkout(input, work_dir),
        "http_get" => http_get(input),
        "http_post" => http_post(input),
        "send_input" => Ok(json!({"success": false, "error": "send_input not supported in local mode"})),
        _ => Err(anyhow::anyhow!("tool `{}` not implemented for local execution", tool_name)),
    };

    match result {
        Ok(v) => {
            tracing::debug!(tool = tool_name, "tool executed successfully");
            Ok(v)
        }
        Err(e) => {
            tracing::warn!(tool = tool_name, error = %e, "tool execution failed");
            Ok(json!({"error": e.to_string(), "success": false}))
        }
    }
}

/// Resolve a crew-supplied path *inside* the sandbox `work_dir`.
///
/// `work_dir` is treated as the sandbox root: an absolute path like
/// `/index.html` maps to `work_dir/index.html` (crews say "/" to mean the
/// project root, not the host filesystem root), and `..` components can never
/// climb above `work_dir`. Without this, absolute paths escaped the sandbox and
/// wrote to the host filesystem root.
fn resolve_path(path: &str, work_dir: &PathBuf) -> PathBuf {
    use std::path::Component;
    let mut rel: Vec<std::ffi::OsString> = Vec::new();
    for comp in PathBuf::from(path).components() {
        match comp {
            // Drop any prefix/root/`.` — re-root everything into work_dir.
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            // `..` pops within the sandbox but is clamped at the root.
            Component::ParentDir => {
                rel.pop();
            }
            Component::Normal(c) => rel.push(c.to_os_string()),
        }
    }
    let mut out = work_dir.clone();
    for c in rel {
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_paths_are_jailed_into_work_dir() {
        let wd = PathBuf::from("/tmp/sandbox");
        assert_eq!(resolve_path("/index.html", &wd), PathBuf::from("/tmp/sandbox/index.html"));
        assert_eq!(resolve_path("index.html", &wd), PathBuf::from("/tmp/sandbox/index.html"));
        assert_eq!(resolve_path("src/app.js", &wd), PathBuf::from("/tmp/sandbox/src/app.js"));
    }

    #[test]
    fn parent_dir_cannot_escape_sandbox() {
        let wd = PathBuf::from("/tmp/sandbox");
        assert_eq!(resolve_path("../../etc/passwd", &wd), PathBuf::from("/tmp/sandbox/etc/passwd"));
        assert_eq!(resolve_path("a/../b.txt", &wd), PathBuf::from("/tmp/sandbox/b.txt"));
    }
}

fn read_file(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("read_file: missing `path`"))?;
    let resolved = resolve_path(path, work_dir);
    let contents = std::fs::read_to_string(&resolved)
        .with_context(|| format!("read_file: {}", resolved.display()))?;
    let size = contents.len();
    Ok(json!({"content": contents, "size": size, "success": true}))
}

fn write_file(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("write_file: missing `path`"))?;
    let contents = input.get("contents").or_else(|| input.get("content")).and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("write_file: missing `contents`"))?;
    let resolved = resolve_path(path, work_dir);
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&resolved, contents)
        .with_context(|| format!("write_file: {}", resolved.display()))?;
    tracing::info!(path = %resolved.display(), bytes = contents.len(), "wrote file");
    Ok(json!({"path": path, "bytes": contents.len(), "success": true}))
}

fn append_file(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("append_file: missing `path`"))?;
    let contents = input.get("contents").or_else(|| input.get("content")).and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("append_file: missing `contents`"))?;
    let resolved = resolve_path(path, work_dir);
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&resolved)?;
    file.write_all(contents.as_bytes())?;
    Ok(json!({"path": path, "bytes": contents.len(), "success": true}))
}

fn delete_file(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("delete_file: missing `path`"))?;
    let resolved = resolve_path(path, work_dir);
    std::fs::remove_file(&resolved)?;
    Ok(json!({"path": path, "success": true}))
}

fn list_dir(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str).unwrap_or(".");
    let resolved = resolve_path(path, work_dir);
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
    Ok(json!({"path": path, "entries": entries, "count": entries.len(), "success": true}))
}

fn create_directory(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("create_directory: missing `path`"))?;
    let resolved = resolve_path(path, work_dir);
    std::fs::create_dir_all(&resolved)?;
    Ok(json!({"path": path, "success": true}))
}

fn file_exists(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("file_exists: missing `path`"))?;
    let resolved = resolve_path(path, work_dir);
    Ok(json!({"path": path, "exists": resolved.exists(), "success": true}))
}

fn get_file_info(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("get_file_info: missing `path`"))?;
    let resolved = resolve_path(path, work_dir);
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

fn copy_file(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let from = input.get("from").or_else(|| input.get("src")).and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("copy_file: missing `from`"))?;
    let to = input.get("to").or_else(|| input.get("dst")).and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("copy_file: missing `to`"))?;
    let src = resolve_path(from, work_dir);
    let dst = resolve_path(to, work_dir);
    if let Some(parent) = dst.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::copy(&src, &dst)?;
    Ok(json!({"from": from, "to": to, "success": true}))
}

fn move_file(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let from = input.get("from").or_else(|| input.get("src")).and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("move_file: missing `from`"))?;
    let to = input.get("to").or_else(|| input.get("dst")).and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("move_file: missing `to`"))?;
    let src = resolve_path(from, work_dir);
    let dst = resolve_path(to, work_dir);
    if let Some(parent) = dst.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::rename(&src, &dst)?;
    Ok(json!({"from": from, "to": to, "success": true}))
}

fn run_command(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let cmd = input.get("cmd").and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("run_command: missing `cmd`"))?;
    let cwd = input.get("cwd").and_then(Value::as_str);
    let resolved_cwd = cwd.map(|c| resolve_path(c, work_dir)).unwrap_or_else(|| work_dir.clone());

    tracing::info!(cmd = cmd, cwd = %resolved_cwd.display(), "running command");

    // Provide a git identity so that any `git commit` invoked through run_command
    // works on a bare machine where user.email/user.name are not configured.
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(&resolved_cwd)
        .env("GIT_AUTHOR_NAME", "MowisAI")
        .env("GIT_AUTHOR_EMAIL", "agent@mowis.ai")
        .env("GIT_COMMITTER_NAME", "MowisAI")
        .env("GIT_COMMITTER_EMAIL", "agent@mowis.ai")
        .output()
        .with_context(|| format!("run_command: {}", cmd))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    tracing::debug!(cmd = cmd, exit_code = exit_code, stdout_len = stdout.len(), stderr_len = stderr.len(), "command finished");

    Ok(json!({
        "exit_code": exit_code,
        "stdout": stdout,
        "stderr": stderr,
        "success": output.status.success()
    }))
}

fn run_script(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str);
    let script = input.get("script").and_then(Value::as_str);
    let interpreter = input.get("interpreter").and_then(Value::as_str).unwrap_or("sh");

    let resolved = if let Some(p) = path {
        resolve_path(p, work_dir)
    } else if let Some(s) = script {
        let tmp = work_dir.join("_script_tmp.sh");
        std::fs::write(&tmp, s)?;
        tmp
    } else {
        return Err(anyhow::anyhow!("run_script: need `path` or `script`"));
    };

    let output = Command::new(interpreter)
        .arg(&resolved)
        .current_dir(work_dir)
        .output()?;

    Ok(json!({
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "success": output.status.success()
    }))
}

fn grep(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let pattern = input.get("pattern").and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("grep: missing `pattern`"))?;
    let path = input.get("path").and_then(Value::as_str).unwrap_or(".");
    let resolved = resolve_path(path, work_dir);
    let include = input.get("include").and_then(Value::as_str);

    let mut cmd = Command::new("grep");
    cmd.args(["-rn", "--color=never", pattern]);
    if let Some(inc) = include { cmd.args(["--include", inc]); }
    cmd.arg(&resolved);

    let output = cmd.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let matches: Vec<&str> = stdout.lines().take(100).collect();
    Ok(json!({"matches": matches, "count": matches.len(), "success": true}))
}

fn find_files(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let pattern = input.get("pattern").and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("find_files: missing `pattern`"))?;
    let path = input.get("path").and_then(Value::as_str).unwrap_or(".");
    let resolved = resolve_path(path, work_dir);

    let output = Command::new("find")
        .arg(&resolved)
        .args(["-maxdepth", "10"])
        .args(["-name", pattern])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files: Vec<&str> = stdout.lines().collect();
    Ok(json!({"files": files, "count": files.len(), "success": true}))
}

fn git_command(args: &[&str], work_dir: &PathBuf, cwd: Option<&str>) -> Result<Value> {
    let resolved_cwd = cwd.map(|c| resolve_path(c, work_dir)).unwrap_or_else(|| work_dir.clone());
    let output = Command::new("git")
        .args(args)
        .current_dir(&resolved_cwd)
        .output()?;
    Ok(json!({
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "success": output.status.success()
    }))
}

fn git_add(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str).unwrap_or(".");
    let files: Vec<String> = input.get("files").and_then(|f| serde_json::from_value(f.clone()).ok())
        .unwrap_or_else(|| vec![".".into()]);
    let resolved = resolve_path(path, work_dir);
    let mut args = vec!["add"];
    let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    args.extend_from_slice(&file_refs);
    let output = Command::new("git").args(&args).current_dir(&resolved).output()?;
    Ok(json!({
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "success": output.status.success()
    }))
}

fn git_commit(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str).unwrap_or(".");
    let message = input.get("message").and_then(Value::as_str).unwrap_or("auto-commit");
    let resolved = resolve_path(path, work_dir);
    let output = Command::new("git")
        .args(["-c", "user.email=agent@mowis.ai", "-c", "user.name=MowisAI", "commit", "-m", message])
        .current_dir(&resolved)
        .output()?;
    Ok(json!({
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "success": output.status.success()
    }))
}

fn git_branch(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str);
    let name = input.get("name").and_then(Value::as_str);
    let resolved = path.map(|p| resolve_path(p, work_dir)).unwrap_or_else(|| work_dir.clone());
    let args = if let Some(n) = name { vec!["branch", n] } else { vec!["branch"] };
    let output = Command::new("git").args(&args).current_dir(&resolved).output()?;
    Ok(json!({
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "success": output.status.success()
    }))
}

fn git_checkout(input: &Value, work_dir: &PathBuf) -> Result<Value> {
    let path = input.get("path").and_then(Value::as_str).unwrap_or(".");
    let branch = input.get("branch").and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("git_checkout: missing `branch`"))?;
    let resolved = resolve_path(path, work_dir);
    let output = Command::new("git")
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

fn http_get(input: &Value) -> Result<Value> {
    let url = input.get("url").and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("http_get: missing `url`"))?;
    let client = reqwest::blocking::Client::new();
    let resp = client.get(url).timeout(std::time::Duration::from_secs(30)).send()?;
    let status = resp.status().as_u16();
    let body = resp.text()?;
    Ok(json!({"status": status, "body": body, "success": status < 400}))
}

fn http_post(input: &Value) -> Result<Value> {
    let url = input.get("url").and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("http_post: missing `url`"))?;
    let body = input.get("body").and_then(Value::as_str).unwrap_or("");
    let client = reqwest::blocking::Client::new();
    let resp = client.post(url).body(body.to_string()).timeout(std::time::Duration::from_secs(30)).send()?;
    let status = resp.status().as_u16();
    let resp_body = resp.text()?;
    Ok(json!({"status": status, "body": resp_body, "success": status < 400}))
}
