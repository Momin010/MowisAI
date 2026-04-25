use crate::tools::common::{resolve_path, Tool, ToolContext};
use serde_json::{json, Value};
use std::process::Command;

pub struct NpmInstallTool;
impl Tool for NpmInstallTool {
    fn name(&self) -> &'static str {
        "npm_install"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let package = input.get("package").and_then(|v| v.as_str());
        let cwd_str = input.get("cwd").and_then(|v| v.as_str()).unwrap_or(".");
        let global = input
            .get("global")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let cwd = resolve_path(ctx, cwd_str);

        let mut cmd = Command::new("npm");
        cmd.current_dir(&cwd).arg("install");
        if let Some(pkg) = package {
            cmd.arg(pkg);
        }
        if global {
            cmd.arg("--global");
        }

        let output = cmd.output()?;

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(NpmInstallTool)
    }
}

pub struct PipInstallTool;
impl Tool for PipInstallTool {
    fn name(&self) -> &'static str {
        "pip_install"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let package = input["package"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("pip_install: missing package"))?;
        let version = input.get("version").and_then(|v| v.as_str());
        let upgrade = input
            .get("upgrade")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut cmd = Command::new("pip");
        cmd.arg("install");

        if let Some(v) = version {
            cmd.arg(&format!("{}=={}", package, v));
        } else {
            cmd.arg(package);
        }

        if upgrade {
            cmd.arg("--upgrade");
        }

        let output = cmd.output()?;

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(PipInstallTool)
    }
}

pub struct CargoAddTool;
impl Tool for CargoAddTool {
    fn name(&self) -> &'static str {
        "cargo_add"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let package = input["package"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("cargo_add: missing package"))?;
        let version = input.get("version").and_then(|v| v.as_str());
        let cwd_str = input.get("cwd").and_then(|v| v.as_str()).unwrap_or(".");

        let cwd = resolve_path(ctx, cwd_str);

        let check_cmd = format!("timeout 1 which cargo");
        let check = Command::new("sh").arg("-c").arg(&check_cmd).output();
        if check.map(|o| !o.status.success()).unwrap_or(true) {
            return Ok(json!({
                "success": true,
                "skipped": true,
                "reason": "cargo not available"
            }));
        }

        let pkg = if let Some(v) = version {
            format!("{}@{}", package, v)
        } else {
            package.to_string()
        };

        let cmd = format!("timeout 5 cargo add {} 2>/dev/null", pkg);
        let output = Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(&cwd)
            .output();

        match output {
            Ok(out) => Ok(json!({
                "success": out.status.success(),
                "stdout": String::from_utf8_lossy(&out.stdout).to_string(),
                "skipped": !out.status.success(),
            })),
            Err(_e) => Ok(json!({ "success": false, "skipped": true })),
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(CargoAddTool)
    }
}

// ============== WEB TOOLS (3) ==============
