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

        let path = resolve_path(ctx, path_str)?;

        // SECURITY: Use Command args instead of shell string interpolation
        let (linter, output) = match language {
            "js" | "javascript" | "typescript" | "auto" => {
                let output = Command::new("timeout")
                    .arg("5")
                    .arg("eslint")
                    .arg(&path)
                    .output();
                ("eslint", output)
            }
            "python" => {
                let output = Command::new("timeout")
                    .arg("5")
                    .arg("pylint")
                    .arg(&path)
                    .output();
                ("pylint", output)
            }
            "rust" => {
                let output = Command::new("timeout")
                    .arg("5")
                    .arg("cargo")
                    .arg("clippy")
                    .arg("--")
                    .arg(&path)
                    .output();
                ("cargo clippy", output)
            }
            _ => {
                return Ok(json!({ "success": false, "output": "unknown language" }));
            }
        };

        match output {
            Ok(out) => Ok(json!({
                "success": out.status.success(),
                "output": String::from_utf8_lossy(&out.stdout).to_string(),
                "linter": linter
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
        let path_str = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let command = input.get("command").and_then(|v| v.as_str());

        let path = resolve_path(ctx, path_str)?;

        let output = if let Some(cmd) = command {
            // SECURITY: Parse command into args instead of passing to sh -c
            let parts: Vec<&str> = cmd.split_whitespace().collect();
            if parts.is_empty() {
                return Err(anyhow::anyhow!("test: empty command"));
            }
            let mut c = Command::new("timeout");
            c.arg("30");
            for part in &parts {
                c.arg(part);
            }
            c.current_dir(&path);
            c.output()
        } else {
            // Default: try cargo test, npm test, pytest
            let output = Command::new("timeout")
                .arg("30")
                .arg("cargo")
                .arg("test")
                .current_dir(&path)
                .output();
            if let Ok(ref out) = output {
                if out.status.success() || std::path::Path::new(&path).join("Cargo.toml").exists() {
                    return match output {
                        Ok(out) => Ok(json!({
                            "success": out.status.success(),
                            "stdout": String::from_utf8_lossy(&out.stdout).to_string(),
                            "stderr": String::from_utf8_lossy(&out.stderr).to_string(),
                            "runner": "cargo"
                        })),
                        Err(e) => Ok(json!({ "success": false, "error": e.to_string() })),
                    };
                }
            }
            Command::new("timeout")
                .arg("30")
                .arg("npm")
                .arg("test")
                .current_dir(&path)
                .output()
        };

        match output {
            Ok(out) => Ok(json!({
                "success": out.status.success(),
                "stdout": String::from_utf8_lossy(&out.stdout).to_string(),
                "stderr": String::from_utf8_lossy(&out.stderr).to_string()
            })),
            Err(e) => Ok(json!({ "success": false, "error": e.to_string() })),
        }
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
        let path_str = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let command = input.get("command").and_then(|v| v.as_str());

        let path = resolve_path(ctx, path_str)?;

        let output = if let Some(cmd) = command {
            // SECURITY: Parse command into args instead of passing to sh -c
            let parts: Vec<&str> = cmd.split_whitespace().collect();
            if parts.is_empty() {
                return Err(anyhow::anyhow!("build: empty command"));
            }
            let mut c = Command::new("timeout");
            c.arg("120");
            for part in &parts {
                c.arg(part);
            }
            c.current_dir(&path);
            c.output()
        } else {
            // Auto-detect build system
            if std::path::Path::new(&path).join("Cargo.toml").exists() {
                Command::new("timeout")
                    .arg("120")
                    .arg("cargo")
                    .arg("build")
                    .current_dir(&path)
                    .output()
            } else if std::path::Path::new(&path).join("package.json").exists() {
                Command::new("timeout")
                    .arg("120")
                    .arg("npm")
                    .arg("run")
                    .arg("build")
                    .current_dir(&path)
                    .output()
            } else if std::path::Path::new(&path).join("Makefile").exists() {
                Command::new("timeout")
                    .arg("120")
                    .arg("make")
                    .current_dir(&path)
                    .output()
            } else {
                return Ok(
                    json!({ "success": false, "error": "No build system detected (Cargo.toml, package.json, Makefile)" }),
                );
            }
        };

        match output {
            Ok(out) => Ok(json!({
                "success": out.status.success(),
                "stdout": String::from_utf8_lossy(&out.stdout).to_string(),
                "stderr": String::from_utf8_lossy(&out.stderr).to_string()
            })),
            Err(e) => Ok(json!({ "success": false, "error": e.to_string() })),
        }
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
        let path_str = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let language = input
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("auto");

        let path = resolve_path(ctx, path_str)?;

        // SECURITY: Use Command args instead of shell interpolation
        let output = match language {
            "typescript" | "ts" | "auto" => {
                let has_tsconfig = std::path::Path::new(&path).join("tsconfig.json").exists();
                if has_tsconfig {
                    Command::new("timeout")
                        .arg("30")
                        .arg("npx")
                        .arg("tsc")
                        .arg("--noEmit")
                        .current_dir(&path)
                        .output()
                } else {
                    return Ok(
                        json!({ "success": true, "output": "No tsconfig.json found, skipping TypeScript check" }),
                    );
                }
            }
            "python" | "py" => Command::new("timeout")
                .arg("30")
                .arg("mypy")
                .arg(&path)
                .output(),
            "rust" | "rs" => Command::new("timeout")
                .arg("60")
                .arg("cargo")
                .arg("check")
                .current_dir(&path)
                .output(),
            _ => {
                return Ok(
                    json!({ "success": false, "output": format!("Unsupported language: {}", language) }),
                )
            }
        };

        match output {
            Ok(out) => Ok(json!({
                "success": out.status.success(),
                "output": String::from_utf8_lossy(&out.stdout).to_string(),
                "stderr": String::from_utf8_lossy(&out.stderr).to_string()
            })),
            Err(e) => Ok(json!({ "success": false, "error": e.to_string() })),
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
        let path_str = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let language = input
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("auto");

        let path = resolve_path(ctx, path_str)?;

        // SECURITY: Use Command args instead of shell interpolation
        let output = match language {
            "rust" | "rs" | "auto" => {
                if std::path::Path::new(&path).join("Cargo.toml").exists() {
                    Command::new("timeout")
                        .arg("30")
                        .arg("cargo")
                        .arg("fmt")
                        .current_dir(&path)
                        .output()
                } else {
                    Command::new("timeout")
                        .arg("30")
                        .arg("prettier")
                        .arg("--write")
                        .arg(&path)
                        .output()
                }
            }
            "python" | "py" => Command::new("timeout")
                .arg("30")
                .arg("black")
                .arg(&path)
                .output(),
            "js" | "javascript" | "typescript" | "ts" => Command::new("timeout")
                .arg("30")
                .arg("prettier")
                .arg("--write")
                .arg(&path)
                .output(),
            _ => {
                return Ok(
                    json!({ "success": false, "output": format!("Unsupported language: {}", language) }),
                )
            }
        };

        match output {
            Ok(out) => Ok(json!({
                "success": out.status.success(),
                "output": String::from_utf8_lossy(&out.stdout).to_string(),
                "stderr": String::from_utf8_lossy(&out.stderr).to_string()
            })),
            Err(e) => Ok(json!({ "success": false, "error": e.to_string() })),
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(FormatTool)
    }
}
