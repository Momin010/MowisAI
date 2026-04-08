use crate::tools::common::{resolve_path, Tool, ToolContext};
use serde_json::{json, Value};
use std::process::Command;

// ============== CODE ANALYSIS TOOLS (5) ==============

pub struct LintTool;
impl Tool for LintTool {
    fn name(&self) -> &'static str {
        "lint"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("lint: missing path"))?;
        let language = input
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("auto");

        let path = resolve_path(ctx, path_str);

        let (linter, cmd, output) = match language {
            "js" | "javascript" | "typescript" | "auto" => {
                let cmd = format!("timeout 5 eslint {} < /dev/null", path.display());
                let output = Command::new("sh").arg("-c").arg(&cmd).output();
                ("eslint", cmd, output)
            }
            "python" => {
                let cmd = format!("timeout 5 pylint {} < /dev/null", path.display());
                let output = Command::new("sh").arg("-c").arg(&cmd).output();
                ("pylint", cmd, output)
            }
            "rust" => {
                let cmd = format!("timeout 5 cargo clippy -- {} < /dev/null", path.display());
                let output = Command::new("sh").arg("-c").arg(&cmd).output();
                ("cargo clippy", cmd, output)
            }
            _ => {
                return Ok(json!({ "success": false, "output": "unknown language" }));
            }
        };

        match output {
            Ok(out) => Ok(json!({
                "success": out.status.success(),
                "output": String::from_utf8_lossy(&out.stdout).to_string()
            })),
            Err(e) => Ok(json!({
                "success": false,
                "output": format!("{} not available: {}", linter, e)
            })),
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(LintTool)
    }
}

pub struct TestTool;
impl Tool for TestTool {
    fn name(&self) -> &'static str {
        "test"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("test: missing path"))?;

        let command = input.get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("echo \"tests passed\"");

        let path = resolve_path(ctx, path_str);

        let timeout_cmd = format!("timeout 10 {}", command);
        let output = Command::new("sh")
            .arg("-c")
            .arg(&timeout_cmd)
            .current_dir(&path)
            .output()?;

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(TestTool)
    }
}

pub struct BuildTool;
impl Tool for BuildTool {
    fn name(&self) -> &'static str {
        "build"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("build: missing path"))?;

        let command = input.get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("echo \"build ok\"");

        let path = resolve_path(ctx, path_str);

        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&path)
            .output()?;

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(BuildTool)
    }
}

pub struct TypeCheckTool;
impl Tool for TypeCheckTool {
    fn name(&self) -> &'static str {
        "type_check"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("type_check: missing path"))?;
        let language = input
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("typescript");

        let path = resolve_path(ctx, path_str);

        let output = match language {
            "typescript" | "ts" => {
                let cmd = format!("timeout 5 tsc --noEmit {} < /dev/null", path.display());
                Command::new("sh").arg("-c").arg(&cmd).output()
            }
            _ => {
                return Ok(json!({ "success": false, "output": "unsupported language" }));
            }
        };

        match output {
            Ok(out) => Ok(json!({
                "success": out.status.success(),
                "output": String::from_utf8_lossy(&out.stdout).to_string()
            })),
            Err(e) => Ok(json!({
                "success": false,
                "output": format!("type check failed: {}", e)
            })),
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(TypeCheckTool)
    }
}

pub struct FormatTool;
impl Tool for FormatTool {
    fn name(&self) -> &'static str {
        "format"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("format: missing path"))?;
        let language = input
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("auto");

        let path = resolve_path(ctx, path_str);

        let output = match language {
            "javascript" | "js" | "typescript" | "ts" | "auto" => {
                let cmd = format!("timeout 5 prettier --write {} < /dev/null", path.display());
                Command::new("sh").arg("-c").arg(&cmd).output()
            }
            "python" => {
                let cmd = format!("timeout 5 black {} < /dev/null", path.display());
                Command::new("sh").arg("-c").arg(&cmd).output()
            }
            "rust" => {
                let cmd = format!("timeout 5 rustfmt {} < /dev/null", path.display());
                Command::new("sh").arg("-c").arg(&cmd).output()
            }
            _ => {
                return Ok(json!({ "success": false, "output": "unsupported language" }));
            }
        };

        match output {
            Ok(out) => Ok(json!({
                "success": out.status.success(),
                "output": String::from_utf8_lossy(&out.stdout).to_string()
            })),
            Err(e) => Ok(json!({
                "success": false,
                "output": format!("format failed: {}", e)
            })),
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(FormatTool)
    }
}

// ============== ECHO TOOL (Legacy) ==============
