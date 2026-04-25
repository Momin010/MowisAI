use crate::tools::common::{resolve_path, Tool, ToolContext};
use serde_json::{json, Value};
use std::fs;

// ============== FILESYSTEM TOOLS (11) ==============

pub struct ReadFileTool;
impl Tool for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("read_file: missing path"))?;
        let path = resolve_path(ctx, path_str);
        let content = fs::read_to_string(&path)?;
        Ok(json!({
            "content": content,
            "size": content.len(),
            "success": true
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(ReadFileTool)
    }
}

pub struct WriteFileTool;
impl Tool for WriteFileTool {
    fn name(&self) -> &'static str {
        "write_file"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("write_file: missing path"))?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("write_file: missing content"))?;

        let path = resolve_path(ctx, path_str);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content)?;
        Ok(json!({
            "size": content.len(),
            "path": path.to_string_lossy().to_string(),
            "success": true
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(WriteFileTool)
    }
}

pub struct AppendFileTool;
impl Tool for AppendFileTool {
    fn name(&self) -> &'static str {
        "append_file"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("append_file: missing path"))?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("append_file: missing content"))?;

        let path = resolve_path(ctx, path_str);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)?;
        use std::io::Write;
        file.write_all(content.as_bytes())?;
        Ok(json!({ "success": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(AppendFileTool)
    }
}

pub struct DeleteFileTool;
impl Tool for DeleteFileTool {
    fn name(&self) -> &'static str {
        "delete_file"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("delete_file: missing path"))?;
        let path = resolve_path(ctx, path_str);
        fs::remove_file(&path)?;
        Ok(json!({ "success": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(DeleteFileTool)
    }
}

pub struct CopyFileTool;
impl Tool for CopyFileTool {
    fn name(&self) -> &'static str {
        "copy_file"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let from_str = input.get("from").or_else(|| input.get("src"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("copy_file: missing from/src"))?;
        let to_str = input.get("to").or_else(|| input.get("dst"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("copy_file: missing to/dst"))?;

        let from = resolve_path(ctx, from_str);
        let to = resolve_path(ctx, to_str);

        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&from, &to)?;
        Ok(json!({ "success": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(CopyFileTool)
    }
}

pub struct MoveFileTool;
impl Tool for MoveFileTool {
    fn name(&self) -> &'static str {
        "move_file"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let from_str = input.get("from").or_else(|| input.get("src"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("move_file: missing from/src"))?;
        let to_str = input.get("to").or_else(|| input.get("dst"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("move_file: missing to/dst"))?;

        let from = resolve_path(ctx, from_str);
        let to = resolve_path(ctx, to_str);

        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&from, &to)?;
        Ok(json!({ "success": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(MoveFileTool)
    }
}

pub struct ListFilesTool;
impl Tool for ListFilesTool {
    fn name(&self) -> &'static str {
        "list_files"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("list_files: missing path"))?;
        let path = resolve_path(ctx, path_str);

        let mut files = vec![];
        let mut dirs = vec![];

        for entry in fs::read_dir(&path)? {
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
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(ListFilesTool)
    }
}

pub struct CreateDirectoryTool;
impl Tool for CreateDirectoryTool {
    fn name(&self) -> &'static str {
        "create_directory"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("create_directory: missing path"))?;
        let path = resolve_path(ctx, path_str);
        fs::create_dir_all(&path)?;
        Ok(json!({ "success": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(CreateDirectoryTool)
    }
}

pub struct DeleteDirectoryTool;
impl Tool for DeleteDirectoryTool {
    fn name(&self) -> &'static str {
        "delete_directory"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("delete_directory: missing path"))?;
        let path = resolve_path(ctx, path_str);
        fs::remove_dir_all(&path)?;
        Ok(json!({ "success": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(DeleteDirectoryTool)
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
        let path = resolve_path(ctx, path_str);
        let meta = fs::metadata(&path)?;
        Ok(json!({
            "size": meta.len(),
            "is_file": meta.is_file(),
            "is_dir": meta.is_dir(),
            "modified": format!("{:?}", meta.modified()?)
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(GetFileInfoTool)
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
        let path = resolve_path(ctx, path_str);
        Ok(json!({ "exists": path.exists() }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(FileExistsTool)
    }
}
