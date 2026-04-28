use crate::tools::common::{Tool, ToolContext};
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::time::Duration;

// ============== SHELL TOOLS (5) ==============

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

        // Default timeout: 30 seconds (agents can override)
        let timeout_secs = input.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30);

        // CRITICAL: run_command must always execute in container context (chroot).
        // Running arbitrary commands on the sandbox root would be a privilege escalation.
        let root = ctx.root_path.as_ref()
            .ok_or_else(|| anyhow::anyhow!("run_command: must execute within a container (no root_path)"))?;

        // Execute in a new PID namespace so processes started by the agent are not visible
        // from the host PID namespace. We still use `chroot` for filesystem isolation.
        let mut c = Command::new("unshare");
        let cwd_str = cwd.unwrap_or("/");
        c.arg("--fork")
            .arg("--pid")
            // Provide a correct /proc view for the new PID namespace.
            .arg("--mount-proc")
            .arg("--")
            .arg("chroot")
            .arg(root)
            .arg("/bin/sh")
            .arg("-c")
            .arg(&format!("cd {} 2>/dev/null && {}", cwd_str, cmd))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Spawn the process with timeout
        let mut child = c.spawn()?;
        let timeout_duration = Duration::from_secs(timeout_secs);

        // Wait with timeout using a simple polling approach
        let start = std::time::Instant::now();
        let result = loop {
            match child.try_wait()? {
                Some(status) => {
                    // Process finished
                    let output = child.wait_with_output()?;
                    break Ok(json!({
                        "exit_code": status.code().unwrap_or(-1),
                        "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                        "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
                        "success": status.success(),
                        "timed_out": false
                    }));
                }
                None => {
                    // Still running - check timeout
                    if start.elapsed() > timeout_duration {
                        // Timeout - kill the process
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
                    // Sleep a bit before checking again
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
    fn name(&self) -> &'static str { "run_script" }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let inline_script = input.get("script").and_then(|v| v.as_str());
        let path_str = input.get("path").and_then(|v| v.as_str());
        let language = input.get("language").and_then(|v| v.as_str()).unwrap_or("sh");
        let interpreter = input.get("interpreter").and_then(|v| v.as_str())
            .unwrap_or(match language {
                "python" | "python3" => "/usr/bin/python3",
                "node" | "js" => "/usr/bin/node",
                _ => "/bin/sh",
            });

        // CRITICAL: run_script must always execute in container context (chroot).
        let root = ctx.root_path.as_ref()
            .ok_or_else(|| anyhow::anyhow!("run_script: must execute within a container (no root_path)"))?;

        if let Some(script) = inline_script {
            use std::io::Write;
            let tmp_path = format!("/tmp/_script_{}.sh",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos());
            let host_path = format!("{}{}", root.display(), tmp_path);
            let mut f = std::fs::File::create(&host_path)?;
            f.write_all(script.as_bytes())?;
            drop(f);
            // Run inside new PID namespace for isolation.
            let mut c = Command::new("unshare");
            c.arg("--fork")
                .arg("--pid")
                .arg("--mount-proc")
                .arg("--")
                .arg("chroot")
                .arg(root)
                .arg(interpreter)
                .arg(&tmp_path);
            let output = c.output()?;
            let _ = std::fs::remove_file(&host_path);
            Ok(json!({
                "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
                "exit_code": output.status.code().unwrap_or(-1),
                "success": output.status.success()
            }))
        } else if let Some(p) = path_str {
            let mut c = Command::new("unshare");
            c.arg("--fork")
                .arg("--pid")
                .arg("--mount-proc")
                .arg("--")
                .arg("chroot")
                .arg(root)
                .arg(interpreter)
                .arg(p);
            let output = c.output()?;
            Ok(json!({
                "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
                "exit_code": output.status.code().unwrap_or(-1),
                "success": output.status.success()
            }))
        } else {
            Err(anyhow::anyhow!("run_script: missing path or script"))
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(RunScriptTool) }
}

pub struct KillProcessTool;
impl Tool for KillProcessTool {
    fn name(&self) -> &'static str {
        "kill_process"
    }
    #[cfg(target_os = "linux")]
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let pid_val = input["pid"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("kill_process: missing pid"))?;

        let pid = nix::unistd::Pid::from_raw(pid_val as i32);
        match nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM) {
            Ok(_) => Ok(json!({ "success": true })),
            Err(nix::Error::ESRCH) => Ok(json!({ "success": false, "error": "process not found" })),
            Err(e) => Err(anyhow::anyhow!("kill_process error: {}", e)),
        }
    }
    #[cfg(not(target_os = "linux"))]
    fn invoke(&self, _ctx: &ToolContext, _input: Value) -> anyhow::Result<Value> {
        anyhow::bail!("kill_process is only supported on Linux")
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
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let var = input.get("var").or_else(|| input.get("key"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("get_env: missing var"))?;

        match std::env::var(var) {
            Ok(value) => Ok(json!({ "value": value })),
            Err(_) => Ok(json!({ "value": null })),
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
        let var = input.get("var").or_else(|| input.get("key"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("set_env: missing var"))?;
        let value = input["value"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("set_env: missing value"))?;

        unsafe {
            std::env::set_var(var, value);
        }
        Ok(json!({ "success": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(SetEnvTool)
    }
}
