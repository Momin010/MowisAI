use crate::tools::common::{Tool, ToolContext};
use serde_json::{json, Value};
use std::fs;

pub struct SpawnAgentTool;
impl Tool for SpawnAgentTool {
    fn name(&self) -> &'static str {
        "spawn_agent"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let task = input.get("task").and_then(|v| v.as_str()).unwrap_or("");
        let _tools = input.get("tools").and_then(|v| v.as_array());

        // Generate a string agent ID (project invariant: IDs always String)
        let agent_id = format!("agent-{}-{}", ctx.sandbox_id, fastrand::u64(1..u64::MAX));

        Ok(json!({
            "success": true,
            "agent_id": agent_id,
            "task": task,
            "status": "spawned"
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(SpawnAgentTool)
    }
}

pub struct EchoTool;
impl Tool for EchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let message = input
            .get("message")
            .or_else(|| input.get("input"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        Ok(json!({ "output": message, "success": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(EchoTool)
    }
}

pub struct ReadMultipleFilesTool;
impl Tool for ReadMultipleFilesTool {
    fn name(&self) -> &'static str {
        "read_multiple_files"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let paths = input["paths"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("read_multiple_files: missing paths"))?;

        let mut results = Vec::new();
        for path_val in paths {
            if let Some(path_str) = path_val.as_str() {
                let path = crate::tools::common::resolve_path(ctx, path_str)?;
                match fs::read_to_string(&path) {
                    Ok(content) => results.push(json!({
                        "path": path_str,
                        "content": content,
                        "success": true
                    })),
                    Err(e) => results.push(json!({
                        "path": path_str,
                        "error": e.to_string(),
                        "success": false
                    })),
                }
            }
        }

        Ok(json!({ "files": results }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(ReadMultipleFilesTool)
    }
}

pub struct FileExistsTool;
impl Tool for FileExistsTool {
    fn name(&self) -> &'static str {
        "file_exists"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("file_exists: missing path"))?;
        let path = crate::tools::common::resolve_path(ctx, path_str)?;
        Ok(json!({ "exists": path.exists() || fs::symlink_metadata(&path).is_ok() }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(FileExistsTool)
    }
}

pub struct GetFileInfoTool;
impl Tool for GetFileInfoTool {
    fn name(&self) -> &'static str {
        "get_file_info"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("get_file_info: missing path"))?;
        let path = crate::tools::common::resolve_path(ctx, path_str)?;
        let meta = fs::symlink_metadata(&path)?;
        let is_symlink = meta.file_type().is_symlink();
        Ok(json!({
            "size": meta.len(),
            "is_file": meta.is_file(),
            "is_dir": meta.is_dir(),
            "is_symlink": is_symlink,
            "modified": format!("{:?}", meta.modified()?)
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(GetFileInfoTool)
    }
}
