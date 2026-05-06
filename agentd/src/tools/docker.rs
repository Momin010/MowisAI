use crate::tools::common::{resolve_path, Tool, ToolContext};
use serde_json::{json, Value};
use std::process::Command;

pub struct DockerBuildTool;
impl Tool for DockerBuildTool {
    fn name(&self) -> &'static str {
        "docker_build"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("docker_build: missing path"))?;
        let tag = input["tag"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("docker_build: missing tag"))?;
        let dockerfile = input
            .get("dockerfile")
            .and_then(|v| v.as_str())
            .unwrap_or("Dockerfile");

        let path = resolve_path(ctx, path_str)?;

        let output = Command::new("docker")
            .arg("build")
            .arg("-f")
            .arg(dockerfile)
            .arg("-t")
            .arg(tag)
            .arg(&path)
            .output()?;

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(DockerBuildTool)
    }
}

pub struct DockerRunTool;
impl Tool for DockerRunTool {
    fn name(&self) -> &'static str {
        "docker_run"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let image = input["image"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("docker_run: missing image"))?;
        let cmd = input.get("cmd").and_then(|v| v.as_str());
        let name = input.get("name").and_then(|v| v.as_str());

        // SECURITY: Validate image doesn't contain special characters
        if image.contains('\0') || image.contains(';') || image.contains('&') || image.contains('|')
        {
            return Err(anyhow::anyhow!("docker_run: invalid image name"));
        }

        let mut command = Command::new("docker");
        command.arg("run").arg("-d");

        // SECURITY: Add default resource limits
        command.arg("--memory").arg("512m");
        command.arg("--cpus").arg("1.0");
        command.arg("--pids-limit").arg("256");
        command.arg("--network").arg("bridge");

        // Auto-remove on exit to prevent container leaks
        command.arg("--rm");

        if let Some(n) = name {
            // SECURITY: Validate container name
            if n.starts_with('-') || n.contains('\0') || n.contains('/') {
                return Err(anyhow::anyhow!("docker_run: invalid container name"));
            }
            command.arg("--name").arg(n);
        }

        if let Some(ports) = input["ports"].as_array() {
            for port in ports {
                if let Some(p) = port.as_str() {
                    command.arg("-p").arg(p);
                }
            }
        }

        if let Some(env) = input["env"].as_array() {
            for e in env {
                if let Some(v) = e.as_str() {
                    command.arg("-e").arg(v);
                }
            }
        }

        command.arg(image);
        if let Some(c) = cmd {
            command.arg(c);
        }

        let output = command.output()?;

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(DockerRunTool)
    }
}

pub struct DockerStopTool;
impl Tool for DockerStopTool {
    fn name(&self) -> &'static str {
        "docker_stop"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let container = input["container"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("docker_stop: missing container"))?;

        let output = Command::new("docker").arg("stop").arg(container).output()?;

        Ok(json!({ "success": output.status.success() }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(DockerStopTool)
    }
}

pub struct DockerPsTool;
impl Tool for DockerPsTool {
    fn name(&self) -> &'static str {
        "docker_ps"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let all = input.get("all").and_then(|v| v.as_bool()).unwrap_or(false);

        let mut cmd = Command::new("docker");
        cmd.arg("ps");
        if all {
            cmd.arg("--all");
        }
        cmd.arg("--format").arg("json");

        let output = cmd.output()?;

        Ok(json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "success": output.status.success()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(DockerPsTool)
    }
}

pub struct DockerLogsTool;
impl Tool for DockerLogsTool {
    fn name(&self) -> &'static str {
        "docker_logs"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let container = input["container"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("docker_logs: missing container"))?;

        let output = Command::new("docker").arg("logs").arg(container).output()?;

        Ok(json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(DockerLogsTool)
    }
}

pub struct DockerExecTool;
impl Tool for DockerExecTool {
    fn name(&self) -> &'static str {
        "docker_exec"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let container = input["container"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("docker_exec: missing container"))?;
        let cmd = input["cmd"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("docker_exec: missing cmd"))?;

        // SECURITY: Validate container name (no flag injection)
        if container.starts_with('-') || container.contains('\0') {
            return Err(anyhow::anyhow!("docker_exec: invalid container name"));
        }

        // SECURITY: Block dangerous commands
        let dangerous = ["rm -rf /", "mkfs", "dd if=", "> /dev/"];
        for pat in &dangerous {
            if cmd.contains(pat) {
                return Err(anyhow::anyhow!(
                    "docker_exec: blocked dangerous command pattern '{}'",
                    pat
                ));
            }
        }

        let output = Command::new("docker")
            .arg("exec")
            .arg("--timeout")
            .arg("30")
            .arg(container)
            .arg("sh")
            .arg("-c")
            .arg(cmd)
            .output()?;

        Ok(json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            "exit_code": output.status.code().unwrap_or(-1),
            "success": output.status.success()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(DockerExecTool)
    }
}

pub struct DockerPullTool;
impl Tool for DockerPullTool {
    fn name(&self) -> &'static str {
        "docker_pull"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let image = input["image"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("docker_pull: missing image"))?;

        let output = Command::new("docker").arg("pull").arg(image).output()?;

        Ok(json!({ "success": output.status.success() }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(DockerPullTool)
    }
}

// ============== KUBERNETES TOOLS (6) ==============
