use crate::tools::common::{resolve_path, Tool, ToolContext};
use serde_json::{json, Value};
use std::process::Command;

/// Validate kubernetes resource/name to prevent injection
fn validate_k8s_name(name: &str, field: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        return Err(anyhow::anyhow!("{} cannot be empty", field));
    }
    if name.starts_with('-') {
        return Err(anyhow::anyhow!("{} cannot start with '-'", field));
    }
    if name.contains('\0') || name.contains(';') || name.contains('&') || name.contains('|') {
        return Err(anyhow::anyhow!("{} contains invalid characters", field));
    }
    Ok(())
}

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
        let dry_run = input
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // SECURITY: Validate manifest size
        if manifest.len() > 1024 * 1024 {
            return Err(anyhow::anyhow!(
                "kubectl_apply: manifest too large (max 1MB)"
            ));
        }

        let manifest_path = resolve_path(ctx, "manifest.yaml")?;
        std::fs::write(&manifest_path, manifest)?;

        let mut cmd = Command::new("kubectl");
        cmd.arg("apply").arg("-f").arg(&manifest_path);
        if let Some(ns) = namespace {
            validate_k8s_name(ns, "namespace")?;
            cmd.arg("-n").arg(ns);
        }
        if dry_run {
            cmd.arg("--dry-run=client");
        }

        let output = cmd.output()?;

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string()
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

        validate_k8s_name(resource, "resource")?;

        let mut cmd = Command::new("kubectl");
        cmd.arg("get").arg("--").arg(resource).arg("-o").arg(format);
        if let Some(ns) = namespace {
            validate_k8s_name(ns, "namespace")?;
            cmd.arg("-n").arg(ns);
        }

        let output = cmd.output()?;

        Ok(json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
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
        let dry_run = input
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        validate_k8s_name(resource, "resource")?;
        validate_k8s_name(name, "name")?;

        let mut cmd = Command::new("kubectl");
        cmd.arg("delete").arg("--").arg(resource).arg(name);
        if let Some(ns) = namespace {
            validate_k8s_name(ns, "namespace")?;
            cmd.arg("-n").arg(ns);
        }
        if dry_run {
            cmd.arg("--dry-run=client");
        }

        let output = cmd.output()?;

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string()
        }))
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
        let tail = input.get("tail").and_then(|v| v.as_u64());

        validate_k8s_name(pod, "pod")?;

        let mut cmd = Command::new("kubectl");
        cmd.arg("logs").arg(pod);
        if let Some(ns) = namespace {
            validate_k8s_name(ns, "namespace")?;
            cmd.arg("-n").arg(ns);
        }
        if let Some(c) = container {
            validate_k8s_name(c, "container")?;
            cmd.arg("-c").arg(c);
        }
        if let Some(n) = tail {
            cmd.arg("--tail").arg(n.to_string());
        }

        let output = cmd.output()?;

        Ok(json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            "success": output.status.success()
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
        let container = input.get("container").and_then(|v| v.as_str());

        validate_k8s_name(pod, "pod")?;

        let mut command = Command::new("kubectl");
        command.arg("exec").arg(pod);
        if let Some(ns) = namespace {
            validate_k8s_name(ns, "namespace")?;
            command.arg("-n").arg(ns);
        }
        if let Some(c) = container {
            validate_k8s_name(c, "container")?;
            command.arg("-c").arg(c);
        }
        // SECURITY: Use -- to separate kubectl args from the command
        command.arg("--").arg("sh").arg("-c").arg(cmd);

        let output = command.output()?;

        Ok(json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            "exit_code": output.status.code().unwrap_or(-1),
            "success": output.status.success()
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
        let name = input.get("name").and_then(|v| v.as_str());
        let namespace = input.get("namespace").and_then(|v| v.as_str());

        validate_k8s_name(resource, "resource")?;

        let mut cmd = Command::new("kubectl");
        cmd.arg("describe").arg("--").arg(resource);
        if let Some(n) = name {
            validate_k8s_name(n, "name")?;
            cmd.arg(n);
        }
        if let Some(ns) = namespace {
            validate_k8s_name(ns, "namespace")?;
            cmd.arg("-n").arg(ns);
        }

        let output = cmd.output()?;

        Ok(json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            "success": output.status.success()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(KubectlDescribeTool)
    }
}
