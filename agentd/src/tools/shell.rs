use crate::tools::common::{resolve_path, Tool, ToolContext};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use serde_json::{json, Value};
use std::env;
use std::process::Command;

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

        let mut command = if let Some(root) = &ctx.root_path {
            let mut c = Command::new("chroot");
            let cwd_str = cwd.unwrap_or("/");
            c.arg(root)
                .arg("/bin/sh")
                .arg("-c")
                .arg(&format!("cd {} 2>/dev/null && {}", cwd_str, cmd));
            c
        } else {
            let mut c = Command::new("sh");
            c.arg("-c").arg(cmd);
            if let Some(dir) = cwd {
                c.current_dir(dir);
            }
            c
        };

        let output = command.output()?;
        Ok(json!({
            "exit_code": output.status.code().unwrap_or(-1),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            "success": output.status.success()
        }))
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

        if let Some(script) = inline_script {
            use std::io::Write;
            let tmp_path = format!("/tmp/_script_{}.sh",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos());
            let host_path = if let Some(root) = &ctx.root_path {
                format!("{}{}", root.display(), tmp_path)
            } else { tmp_path.clone() };
            let mut f = std::fs::File::create(&host_path)?;
            f.write_all(script.as_bytes())?;
            drop(f);
            let output = if let Some(root) = &ctx.root_path {
                let mut c = Command::new("chroot");
                c.arg(root).arg(interpreter).arg(&tmp_path);
                c.output()?
            } else { Command::new(interpreter).arg(&host_path).output()? };
            let _ = std::fs::remove_file(&host_path);
            Ok(json!({
                "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
                "exit_code": output.status.code().unwrap_or(-1),
                "success": output.status.success()
            }))
        } else if let Some(p) = path_str {
            let path = resolve_path(ctx, p);
            let output = if let Some(root) = &ctx.root_path {
                let mut c = Command::new("chroot");
                c.arg(root).arg(interpreter).arg(p);
                c.output()?
            } else { Command::new(interpreter).arg(&path).output()? };
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

        std::env::set_var(var, value);
        Ok(json!({ "success": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(SetEnvTool)
    }
}
