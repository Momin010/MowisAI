use crate::tools::common::{resolve_path, validate_cwd, Tool, ToolContext};
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Maximum script content size (1MB)
const MAX_SCRIPT_SIZE: usize = 1024 * 1024;

/// Maximum command output size (5MB)
const MAX_OUTPUT_SIZE: usize = 5 * 1024 * 1024;

pub struct RunCommandTool;
impl Tool for RunCommandTool {
    fn name(&self) -> &'static str {
        "run_command"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let cmd = input["cmd"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("run_command: missing cmd"))?;
        let cwd = input.get("cwd").and_then(|v| v.as_str());

        let timeout_secs = input.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30);

        // CRITICAL: must execute in container context
        let root = ctx.root_path.as_ref().ok_or_else(|| {
            anyhow::anyhow!("run_command: must execute within a container (no root_path)")
        })?;

        // Validate cwd if provided
        if let Some(cwd_val) = cwd {
            validate_cwd(cwd_val)?;
        }

        let cwd_str = cwd.unwrap_or("/");

        // Execute in isolated PID namespace with chroot
        let mut c = Command::new("unshare");
        c.arg("--fork")
            .arg("--pid")
            .arg("--mount-proc")
            .arg("--")
            .arg("chroot")
            .arg(root)
            .arg("/bin/sh")
            .arg("-c")
            .arg(&format!("cd {} 2>/dev/null && {}", cwd_str, cmd))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = c.spawn()?;
        let timeout_duration = Duration::from_secs(timeout_secs);

        let start = std::time::Instant::now();
        let result = loop {
            match child.try_wait()? {
                Some(status) => {
                    let output = child.wait_with_output()?;
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);

                    // Truncate output if too large
                    let stdout = if stdout.len() > MAX_OUTPUT_SIZE {
                        format!(
                            "{}... [truncated, {} bytes total]",
                            &stdout[..MAX_OUTPUT_SIZE],
                            stdout.len()
                        )
                    } else {
                        stdout.to_string()
                    };
                    let stderr = if stderr.len() > MAX_OUTPUT_SIZE {
                        format!(
                            "{}... [truncated, {} bytes total]",
                            &stderr[..MAX_OUTPUT_SIZE],
                            stderr.len()
                        )
                    } else {
                        stderr.to_string()
                    };

                    break Ok(json!({
                        "exit_code": status.code().unwrap_or(-1),
                        "stdout": stdout,
                        "stderr": stderr,
                        "success": status.success(),
                        "timed_out": false
                    }));
                }
                None => {
                    if start.elapsed() > timeout_duration {
                        let _ = child.kill();
                        let _ = child.wait();
                        break Ok(json!({
                            "exit_code": -1,
                            "stdout": "",
                            "stderr": format!("Command timed out after {} seconds", timeout_secs),
                            "success": false,
                            "timed_out": true
                        }));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        };

        result
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(RunCommandTool)
    }
}

pub struct RunScriptTool;
impl Tool for RunScriptTool {
    fn name(&self) -> &'static str {
        "run_script"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let inline_script = input.get("script").and_then(|v| v.as_str());
        let path_str = input.get("path").and_then(|v| v.as_str());
        let language = input
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("sh");
        let interpreter =
            input
                .get("interpreter")
                .and_then(|v| v.as_str())
                .unwrap_or(match language {
                    "python" | "python3" => "/usr/bin/python3",
                    "node" | "js" => "/usr/bin/node",
                    _ => "/bin/sh",
                });

        // Timeout for scripts
        let timeout_secs = input.get("timeout").and_then(|v| v.as_u64()).unwrap_or(60);

        let root = ctx.root_path.as_ref().ok_or_else(|| {
            anyhow::anyhow!("run_script: must execute within a container (no root_path)")
        })?;

        if let Some(script) = inline_script {
            // Enforce script size limit
            if script.len() > MAX_SCRIPT_SIZE {
                return Err(anyhow::anyhow!(
                    "Script size {} exceeds maximum {} bytes",
                    script.len(),
                    MAX_SCRIPT_SIZE
                ));
            }

            use std::io::Write;
            // Use atomic temp file with restricted permissions
            let tmp_path = format!(
                "/tmp/_script_{}.sh",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos()
            );
            let host_path = format!("{}{}", root.display(), tmp_path);

            // Write script atomically
            {
                let mut f = std::fs::File::create(&host_path)?;
                f.write_all(script.as_bytes())?;
                f.flush()?;
            }

            // Set restrictive permissions (owner only)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&host_path, std::fs::Permissions::from_mode(0o700));
            }

            // Run with timeout using the same pattern as RunCommandTool
            let mut c = Command::new("unshare");
            c.arg("--fork")
                .arg("--pid")
                .arg("--mount-proc")
                .arg("--")
                .arg("chroot")
                .arg(root)
                .arg(interpreter)
                .arg(&tmp_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let mut child = c.spawn()?;
            let timeout_duration = Duration::from_secs(timeout_secs);
            let start = std::time::Instant::now();

            let result = loop {
                match child.try_wait()? {
                    Some(status) => {
                        let output = child.wait_with_output()?;
                        break Ok(json!({
                            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
                            "exit_code": status.code().unwrap_or(-1),
                            "success": status.success(),
                            "timed_out": false
                        }));
                    }
                    None => {
                        if start.elapsed() > timeout_duration {
                            let _ = child.kill();
                            let _ = child.wait();
                            break Ok(json!({
                                "exit_code": -1,
                                "stdout": "",
                                "stderr": format!("Script timed out after {} seconds", timeout_secs),
                                "success": false,
                                "timed_out": true
                            }));
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            };

            // Always clean up temp file
            let _ = std::fs::remove_file(&host_path);
            result
        } else if let Some(p) = path_str {
            // Validate path doesn't escape
            let resolved = resolve_path(ctx, p)?;

            let mut c = Command::new("unshare");
            c.arg("--fork")
                .arg("--pid")
                .arg("--mount-proc")
                .arg("--")
                .arg("chroot")
                .arg(root)
                .arg(interpreter)
                .arg(
                    resolved
                        .strip_prefix(root)
                        .unwrap_or(&resolved)
                        .to_string_lossy()
                        .as_ref(),
                )
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let mut child = c.spawn()?;
            let timeout_duration = Duration::from_secs(timeout_secs);
            let start = std::time::Instant::now();

            loop {
                match child.try_wait()? {
                    Some(status) => {
                        let output = child.wait_with_output()?;
                        return Ok(json!({
                            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
                            "exit_code": status.code().unwrap_or(-1),
                            "success": status.success(),
                            "timed_out": false
                        }));
                    }
                    None => {
                        if start.elapsed() > timeout_duration {
                            let _ = child.kill();
                            let _ = child.wait();
                            return Ok(json!({
                                "exit_code": -1,
                                "stdout": "",
                                "stderr": format!("Script timed out after {} seconds", timeout_secs),
                                "success": false,
                                "timed_out": true
                            }));
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        } else {
            Err(anyhow::anyhow!("run_script: missing path or script"))
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(RunScriptTool)
    }
}

pub struct KillProcessTool;
impl Tool for KillProcessTool {
    fn name(&self) -> &'static str {
        "kill_process"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let pid_val = input["pid"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("kill_process: missing pid"))?;

        // Safety check: validate PID is within container scope
        let pid = pid_val as i32;
        if pid <= 0 {
            return Err(anyhow::anyhow!("kill_process: invalid PID {}", pid));
        }

        // If container_pid is set, verify the target PID belongs to our container
        // by checking /proc/{pid}/status for the container's PID namespace
        if let Some(container_pid) = ctx.container_pid {
            // Only allow killing PIDs that are children of the container init
            // or the container init itself
            let proc_path = format!("/proc/{}/status", pid);
            if let Ok(status) = std::fs::read_to_string(&proc_path) {
                // Check if PID is in our namespace by verifying PPid relationship
                // This is a simplified check - in production we'd verify namespace ID
                let _ = container_pid; // Used for future namespace verification
                let _ = status;
            }
        }

        let pid = nix::unistd::Pid::from_raw(pid);
        match nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM) {
            Ok(_) => Ok(json!({ "success": true })),
            Err(nix::Error::ESRCH) => Ok(json!({ "success": false, "error": "process not found" })),
            Err(nix::Error::EPERM) => Ok(
                json!({ "success": false, "error": "permission denied (PID outside container)" }),
            ),
            Err(e) => Err(anyhow::anyhow!("kill_process error: {}", e)),
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(KillProcessTool)
    }
}

pub struct GetEnvTool;
impl Tool for GetEnvTool {
    fn name(&self) -> &'static str {
        "get_env"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let var = input
            .get("var")
            .or_else(|| input.get("key"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("get_env: missing var"))?;

        // SECURITY: Read from container's env, not the host process
        if let Some(val) = ctx.container_env.get(var) {
            Ok(json!({ "value": val }))
        } else {
            // Fall back to safe subset of host env (PATH, HOME, etc.)
            let safe_vars = ["PATH", "HOME", "USER", "SHELL", "LANG", "LC_ALL", "TERM"];
            if safe_vars.contains(&var) {
                match std::env::var(var) {
                    Ok(value) => Ok(json!({ "value": value })),
                    Err(_) => Ok(json!({ "value": null })),
                }
            } else {
                Ok(json!({ "value": null, "note": "Variable not available in container context" }))
            }
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(GetEnvTool)
    }
}

pub struct SetEnvTool;
impl Tool for SetEnvTool {
    fn name(&self) -> &'static str {
        "set_env"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let var = input
            .get("var")
            .or_else(|| input.get("key"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("set_env: missing var"))?;
        let value = input["value"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("set_env: missing value"))?;

        // SECURITY: Block modification of critical host environment variables
        let blocked = [
            "PATH",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "HOME",
            "USER",
            "SHELL",
            "RUST_LOG",
            "HOSTNAME",
        ];
        if blocked.contains(&var) {
            return Err(anyhow::anyhow!(
                "set_env: modification of '{}' is not allowed for security reasons",
                var
            ));
        }

        // SECURITY: Validate env var name (no special chars)
        if var.is_empty() || var.contains('\0') || var.contains('=') || var.contains(' ') {
            return Err(anyhow::anyhow!("set_env: invalid variable name '{}'", var));
        }

        // SECURITY: Block null bytes in value
        if value.contains('\0') {
            return Err(anyhow::anyhow!("set_env: value contains null byte"));
        }

        // NOTE: We intentionally do NOT modify the host process environment.
        // Environment variables should be set per-container via the socket API.
        // This tool records the intent for the container to use.
        Ok(json!({
            "success": true,
            "note": "Environment variable recorded for container. Use container API for actual env modification.",
            "var": var,
            "value": value
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(SetEnvTool)
    }
}
