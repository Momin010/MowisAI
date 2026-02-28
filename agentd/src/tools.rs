use serde_json::{json, Value};
use std::fs;

/// Context passed to a tool invocation, includes sandbox ID and other metadata.
pub struct ToolContext {
    pub sandbox_id: u64,
}

/// A trait that all tools must implement.
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value>;
    fn clone_box(&self) -> Box<dyn Tool>;
}

impl Clone for Box<dyn Tool> {
    fn clone(&self) -> Box<dyn Tool> {
        self.clone_box()
    }
}

/// Definition of a tool that can be registered with a sandbox.
pub struct ToolDefinition {
    pub name: String,
}

impl ToolDefinition {
    pub fn new(name: impl Into<String>) -> Self {
        ToolDefinition { name: name.into() }
    }
}

// ============== FILE I/O TOOLS ==============

/// read_file(path: string) -> {content: string, size: number}
pub struct ReadFileTool;
impl Tool for ReadFileTool {
    fn name(&self) -> &'static str { "read_file" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("read_file: missing path"))?;
        let content = fs::read_to_string(path)?;
        Ok(json!({
            "content": content,
            "size": content.len(),
            "success": true
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(ReadFileTool) }
}

/// write_file(path: string, content: string) -> {size: number, success: bool}
pub struct WriteFileTool;
impl Tool for WriteFileTool {
    fn name(&self) -> &'static str { "write_file" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("write_file: missing path"))?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("write_file: missing content"))?;
        fs::write(path, content)?;
        Ok(json!({
            "size": content.len(),
            "success": true
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(WriteFileTool) }
}

/// delete_file(path: string) -> {success: bool}
pub struct DeleteFileTool;
impl Tool for DeleteFileTool {
    fn name(&self) -> &'static str { "delete_file" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("delete_file: missing path"))?;
        fs::remove_file(path)?;
        Ok(json!({ "success": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(DeleteFileTool) }
}

/// list_files(path: string) -> {files: [string], directories: [string]}
pub struct ListFilesTool;
impl Tool for ListFilesTool {
    fn name(&self) -> &'static str { "list_files" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("list_files: missing path"))?;
        let mut files = vec![];
        let mut dirs = vec![];
        for entry in fs::read_dir(path)? {
            let e = entry?;
            let name = e.file_name().to_string_lossy().to_string();
            if e.file_type()?.is_dir() {
                dirs.push(name);
            } else {
                files.push(name);
            }
        }
        Ok(json!({
            "files": files,
            "directories": dirs
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(ListFilesTool) }
}

/// create_directory(path: string) -> {success: bool}
pub struct CreateDirectoryTool;
impl Tool for CreateDirectoryTool {
    fn name(&self) -> &'static str { "create_directory" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("create_directory: missing path"))?;
        fs::create_dir_all(path)?;
        Ok(json!({ "success": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(CreateDirectoryTool) }
}

/// get_file_info(path: string) -> {size: number, is_file: bool, is_dir: bool, modified: string}
pub struct GetFileInfoTool;
impl Tool for GetFileInfoTool {
    fn name(&self) -> &'static str { "get_file_info" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("get_file_info: missing path"))?;
        let meta = fs::metadata(path)?;
        Ok(json!({
            "size": meta.len(),
            "is_file": meta.is_file(),
            "is_dir": meta.is_dir(),
            "modified": format!("{:?}", meta.modified()?)
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GetFileInfoTool) }
}

/// copyfile(from: string, to: string) -> {success: bool}
pub struct CopyFileTool;
impl Tool for CopyFileTool {
    fn name(&self) -> &'static str { "copy_file" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let from = input["from"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("copy_file: missing from"))?;
        let to = input["to"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("copy_file: missing to"))?;
        fs::copy(from, to)?;
        Ok(json!({ "success": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(CopyFileTool) }
}

// ============== COMMAND EXECUTION TOOLS ==============

/// run_command(cmd: string, cwd?: string) -> {exit_code: number, stdout: string, stderr: string}
pub struct RunCommandTool;
impl Tool for RunCommandTool {
    fn name(&self) -> &'static str { "run_command" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let cmd = input["cmd"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("run_command: missing cmd"))?;
        let cwd = input.get("cwd").and_then(|v| v.as_str());
        
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(cmd);
        
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }
        
        let output = command.output()?;
        Ok(json!({
            "exit_code": output.status.code().unwrap_or(-1),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(RunCommandTool) }
}

// ============== AGENT ORCHESTRATION TOOLS ==============

/// spawn_subagent(agent_name: string, prompt: string) -> {agent_id: number}
pub struct SpawnSubagentTool;
impl Tool for SpawnSubagentTool {
    fn name(&self) -> &'static str { "spawn_subagent" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let _name = input["agent_name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("spawn_subagent: missing agent_name"))?;
        let _prompt = input["prompt"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("spawn_subagent: missing prompt"))?;
        Ok(json!({
            "agent_id": 999,
            "success": true
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SpawnSubagentTool) }
}

// ============== DATA/JSON TOOLS ==============

/// json_parse(data: string) -> {parsed: object, error?: string}
pub struct JsonParseTool;
impl Tool for JsonParseTool {
    fn name(&self) -> &'static str { "json_parse" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let data = input["data"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("json_parse: missing data"))?;
        match serde_json::from_str::<Value>(data) {
            Ok(parsed) => Ok(json!({ "parsed": parsed })),
            Err(e) => Ok(json!({ "error": e.to_string() })),
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(JsonParseTool) }
}

/// json_stringify(obj: object) -> {string: string}
pub struct JsonStringifyTool;
impl Tool for JsonStringifyTool {
    fn name(&self) -> &'static str { "json_stringify" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let obj = &input["obj"];
        let string = serde_json::to_string(obj)?;
        Ok(json!({ "string": string }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(JsonStringifyTool) }
}

// ============== HTTP/WEB TOOLS ==============

/// http_get(url: string) -> {status: number, body: string}
pub struct HttpGetTool;
impl Tool for HttpGetTool {
    fn name(&self) -> &'static str { "http_get" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("http_get: missing url"))?;
        
        let output = std::process::Command::new("curl")
            .arg("-s")
            .arg("-w")
            .arg("\n%{http_code}")
            .arg(url)
            .output()
            .map_err(|e| anyhow::anyhow!("curl failed or not installed: {}", e))?;
        
        if !output.status.success() {
            return Err(anyhow::anyhow!("curl command failed"));
        }
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        if lines.is_empty() {
            return Err(anyhow::anyhow!("curl returned empty response"));
        }
        
        let status_code: i64 = lines.last()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow::anyhow!("failed to parse status code"))?;
        let body = lines[..lines.len()-1].join("\n");
        
        Ok(json!({
            "status": status_code,
            "body": body
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(HttpGetTool) }
}

/// http_post(url: string, body: string) -> {status: number, body: string}
pub struct HttpPostTool;
impl Tool for HttpPostTool {
    fn name(&self) -> &'static str { "http_post" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("http_post: missing url"))?;
        let body = input["body"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("http_post: missing body"))?;
        
        let output = std::process::Command::new("curl")
            .arg("-s")
            .arg("-w")
            .arg("\n%{http_code}")
            .arg("-X")
            .arg("POST")
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("-d")
            .arg(body)
            .arg(url)
            .output()
            .map_err(|e| anyhow::anyhow!("curl failed or not installed: {}", e))?;
        
        if !output.status.success() {
            return Err(anyhow::anyhow!("curl command failed"));
        }
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        if lines.is_empty() {
            return Err(anyhow::anyhow!("curl returned empty response"));
        }
        
        let status_code: i64 = lines.last()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow::anyhow!("failed to parse status code"))?;
        let response_body = lines[..lines.len()-1].join("\n");
        
        Ok(json!({
            "status": status_code,
            "body": response_body
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(HttpPostTool) }
}

// ============== ECHO TOOL (basic test) ==============

pub struct EchoTool;
impl Tool for EchoTool {
    fn name(&self) -> &'static str { "echo" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        Ok(json!({ "echo": input.to_string() }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(EchoTool) }
}

// ============== TOOL REGISTRY ==============

/// Create a set of default tools for all sandboxes
pub fn create_default_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(ReadFileTool),
        Box::new(WriteFileTool),
        Box::new(DeleteFileTool),
        Box::new(ListFilesTool),
        Box::new(CreateDirectoryTool),
        Box::new(GetFileInfoTool),
        Box::new(CopyFileTool),
        Box::new(RunCommandTool),
        Box::new(SpawnSubagentTool),
        Box::new(JsonParseTool),
        Box::new(JsonStringifyTool),
        Box::new(HttpGetTool),
        Box::new(HttpPostTool),
        Box::new(EchoTool),
    ]
}
