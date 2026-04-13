use crate::tools::common::{resolve_path, Tool, ToolContext};
use serde_json::{json, Value};
use std::fs;
use std::process::Command;

pub struct KubectlApplyTool;
impl Tool for KubectlApplyTool {
    fn name(&self) -> &'static str {
        "kubectl_apply"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let manifest = input["manifest"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("kubectl_apply: missing manifest"))?;
        let namespace = input.get("namespace").and_then(|v| v.as_str());

        let manifest_path = resolve_path(ctx, "manifest.yaml");
        fs::write(&manifest_path, manifest)?;

        let mut cmd = Command::new("kubectl");
        cmd.arg("apply").arg("-f").arg(&manifest_path);
        if let Some(ns) = namespace {
            cmd.arg("-n").arg(ns);
        }

        let output = cmd.output()?;

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(KubectlApplyTool)
    }
}

pub struct KubectlGetTool;
impl Tool for KubectlGetTool {
    fn name(&self) -> &'static str {
        "kubectl_get"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let resource = input["resource"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("kubectl_get: missing resource"))?;
        let namespace = input.get("namespace").and_then(|v| v.as_str());
        let format = input
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("json");

        let mut cmd = Command::new("kubectl");
        cmd.arg("get").arg(resource).arg("-o").arg(format);
        if let Some(ns) = namespace {
            cmd.arg("-n").arg(ns);
        }

        let output = cmd.output()?;

        Ok(json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "success": output.status.success()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(KubectlGetTool)
    }
}

pub struct KubectlDeleteTool;
impl Tool for KubectlDeleteTool {
    fn name(&self) -> &'static str {
        "kubectl_delete"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let resource = input["resource"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("kubectl_delete: missing resource"))?;
        let name = input["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("kubectl_delete: missing name"))?;
        let namespace = input.get("namespace").and_then(|v| v.as_str());

        let mut cmd = Command::new("kubectl");
        cmd.arg("delete").arg(resource).arg(name);
        if let Some(ns) = namespace {
            cmd.arg("-n").arg(ns);
        }

        let output = cmd.output()?;

        Ok(json!({ "success": output.status.success() }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(KubectlDeleteTool)
    }
}

pub struct KubectlLogsTool;
impl Tool for KubectlLogsTool {
    fn name(&self) -> &'static str {
        "kubectl_logs"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let pod = input["pod"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("kubectl_logs: missing pod"))?;
        let namespace = input.get("namespace").and_then(|v| v.as_str());
        let container = input.get("container").and_then(|v| v.as_str());

        let mut cmd = Command::new("kubectl");
        cmd.arg("logs").arg(pod);
        if let Some(ns) = namespace {
            cmd.arg("-n").arg(ns);
        }
        if let Some(c) = container {
            cmd.arg("-c").arg(c);
        }

        let output = cmd.output()?;

        Ok(json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(KubectlLogsTool)
    }
}

pub struct KubectlExecTool;
impl Tool for KubectlExecTool {
    fn name(&self) -> &'static str {
        "kubectl_exec"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let pod = input["pod"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("kubectl_exec: missing pod"))?;
        let cmd = input["cmd"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("kubectl_exec: missing cmd"))?;
        let namespace = input.get("namespace").and_then(|v| v.as_str());

        let mut command = Command::new("kubectl");
        command.arg("exec").arg(pod);
        if let Some(ns) = namespace {
            command.arg("-n").arg(ns);
        }
        command.arg("--").arg("sh").arg("-c").arg(cmd);

        let output = command.output()?;

        Ok(json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(KubectlExecTool)
    }
}

pub struct KubectlDescribeTool;
impl Tool for KubectlDescribeTool {
    fn name(&self) -> &'static str {
        "kubectl_describe"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let resource = input["resource"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("kubectl_describe: missing resource"))?;
        let name = input["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("kubectl_describe: missing name"))?;
        let namespace = input.get("namespace").and_then(|v| v.as_str());

        let mut cmd = Command::new("kubectl");
        cmd.arg("describe").arg(resource).arg(name);
        if let Some(ns) = namespace {
            cmd.arg("-n").arg(ns);
        }

        let output = cmd.output()?;

        Ok(json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(KubectlDescribeTool)
    }
}

// ============== MEMORY TOOLS (6) ==============
