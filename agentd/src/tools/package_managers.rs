use crate::tools::common::{resolve_path, Tool, ToolContext};
use serde_json::{json, Value};
use std::process::Command;

/// Timeout for package installation (120 seconds)
const INSTALL_TIMEOUT: &str = "120";

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

        let cwd = resolve_path(ctx, cwd_str)?;

        let mut cmd = Command::new("timeout");
        cmd.arg(INSTALL_TIMEOUT).arg("npm").arg("install");
        cmd.current_dir(&cwd);
        if let Some(pkg) = package {
            cmd.arg(pkg);
        }
        if global {
            cmd.arg("--global");
        }

        let output = cmd.output()?;

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            "timed_out": output.status.code().is_none() || output.status.code() == Some(124)
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

        // SECURITY: Validate package name
        if package.contains('\0') || package.contains(';') || package.contains('&') {
            return Err(anyhow::anyhow!("pip_install: invalid package name"));
        }

        let mut cmd = Command::new("timeout");
        cmd.arg(INSTALL_TIMEOUT).arg("pip").arg("install");

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
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            "timed_out": output.status.code().is_none() || output.status.code() == Some(124)
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

        let cwd = resolve_path(ctx, cwd_str)?;

        // SECURITY: Validate package name
        let valid_pkg = package.chars().all(|c| {
            c.is_alphanumeric() || c == '-' || c == '_' || c == '@' || c == '/' || c == '.'
        });
        if !valid_pkg || package.is_empty() {
            return Err(anyhow::anyhow!(
                "cargo_add: invalid package name '{}'",
                package
            ));
        }

        // Check cargo availability
        let check = Command::new("which").arg("cargo").output();
        if check.map(|o| !o.status.success()).unwrap_or(true) {
            return Ok(json!({
                "success": true,
                "skipped": true,
                "reason": "cargo not available"
            }));
        }

        let pkg_spec = if let Some(v) = version {
            let valid_ver = v.chars().all(|c| {
                c.is_alphanumeric()
                    || c == '.'
                    || c == '-'
                    || c == '+'
                    || c == '^'
                    || c == '~'
                    || c == '='
                    || c == ' '
                    || c == ','
            });
            if !valid_ver {
                return Err(anyhow::anyhow!("cargo_add: invalid version '{}'", v));
            }
            format!("{}@{}", package, v)
        } else {
            package.to_string()
        };

        let output = Command::new("timeout")
            .arg(INSTALL_TIMEOUT)
            .arg("cargo")
            .arg("add")
            .arg(&pkg_spec)
            .current_dir(&cwd)
            .output();

        match output {
            Ok(out) => Ok(json!({
                "success": out.status.success(),
                "stdout": String::from_utf8_lossy(&out.stdout).to_string(),
                "stderr": String::from_utf8_lossy(&out.stderr).to_string(),
                "skipped": !out.status.success(),
            })),
            Err(_e) => Ok(json!({ "success": false, "skipped": true })),
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(CargoAddTool)
    }
}
