use crate::tools::common::{resolve_path, Tool, ToolContext, MAX_LIST_ENTRIES, MAX_READ_SIZE};
use serde_json::{json, Value};
use std::fs;

pub struct ReadFileTool;
impl Tool for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("read_file: missing path"))?;
        let path = resolve_path(ctx, path_str)?;

        // Enforce file size limit to prevent OOM
        let meta = fs::metadata(&path)?;
        if meta.is_dir() {
            return Ok(json!({
                "error": "This is a directory, not a file. Use the 'list_files' tool to list directory contents.",
                "path": path_str,
                "is_directory": true,
                "success": false
            }));
        }
        if meta.len() > MAX_READ_SIZE {
            return Err(anyhow::anyhow!(
                "File size {} bytes exceeds maximum read size {} bytes",
                meta.len(),
                MAX_READ_SIZE
            ));
        }

        // Use symlink_metadata to detect symlinks
        let symlink_meta = fs::symlink_metadata(&path)?;
        if symlink_meta.file_type().is_symlink() {
            // Read the symlink target for informational purposes
            let target = fs::read_link(&path)?;
            return Ok(json!({
                "content": format!("SYMLINK -> {}", target.display()),
                "size": 0,
                "is_symlink": true,
                "target": target.to_string_lossy().to_string(),
                "success": true
            }));
        }

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

        // Enforce content size limit
        if content.len() > crate::tools::common::MAX_WRITE_SIZE {
            return Err(anyhow::anyhow!(
                "Content size {} exceeds maximum write size {} bytes",
                content.len(),
                crate::tools::common::MAX_WRITE_SIZE
            ));
        }

        let path = resolve_path(ctx, path_str)?;

        // Check if target is a symlink (prevent writing through symlinks)
        if path.exists() {
            let meta = fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() {
                return Err(anyhow::anyhow!(
                    "Cannot write through symlink: {}",
                    path.display()
                ));
            }
        }

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

        if content.len() > crate::tools::common::MAX_WRITE_SIZE {
            return Err(anyhow::anyhow!(
                "Content size {} exceeds maximum write size",
                content.len()
            ));
        }

        let path = resolve_path(ctx, path_str)?;

        // Check if target is a symlink
        if path.exists() {
            let meta = fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() {
                return Err(anyhow::anyhow!("Cannot append through symlink"));
            }
        }

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
        let path = resolve_path(ctx, path_str)?;

        // Prevent deleting symlink targets (only removes the link itself)
        if !path.exists() && !fs::symlink_metadata(&path).is_ok() {
            return Err(anyhow::anyhow!("File not found: {}", path.display()));
        }

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
        let from_str = input
            .get("from")
            .or_else(|| input.get("src"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("copy_file: missing from/src"))?;
        let to_str = input
            .get("to")
            .or_else(|| input.get("dst"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("copy_file: missing to/dst"))?;

        let from = resolve_path(ctx, from_str)?;
        let to = resolve_path(ctx, to_str)?;

        // Check source is not a symlink
        if from.exists() {
            let meta = fs::symlink_metadata(&from)?;
            if meta.file_type().is_symlink() {
                return Err(anyhow::anyhow!("Cannot copy from symlink source"));
            }
        }

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
        let from_str = input
            .get("from")
            .or_else(|| input.get("src"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("move_file: missing from/src"))?;
        let to_str = input
            .get("to")
            .or_else(|| input.get("dst"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("move_file: missing to/dst"))?;

        let from = resolve_path(ctx, from_str)?;
        let to = resolve_path(ctx, to_str)?;

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
        let path = resolve_path(ctx, path_str)?;

        let mut files = vec![];
        let mut dirs = vec![];
        let mut symlinks = vec![];
        let mut count = 0usize;

        for entry in fs::read_dir(&path)? {
            if count >= MAX_LIST_ENTRIES {
                break;
            }
            let e = entry?;
            let name = e.file_name().to_string_lossy().to_string();
            let ftype = e.file_type()?;
            if ftype.is_dir() {
                dirs.push(name);
            } else if ftype.is_symlink() {
                symlinks.push(name);
            } else {
                files.push(name);
            }
            count += 1;
        }

        Ok(json!({
            "files": files,
            "directories": dirs,
            "symlinks": symlinks,
            "truncated": count >= MAX_LIST_ENTRIES
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
        let path = resolve_path(ctx, path_str)?;
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
        let path = resolve_path(ctx, path_str)?;

        // Safety: prevent deleting the container root itself
        if let Some(root) = &ctx.root_path {
            let canonical_root = root.canonicalize().unwrap_or_else(|_| root.clone());
            let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());
            if canonical_path == canonical_root {
                return Err(anyhow::anyhow!(
                    "Cannot delete the container root directory"
                ));
            }
        }

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
        let path = resolve_path(ctx, path_str)?;

        // Use symlink_metadata to not follow symlinks
        let meta = fs::symlink_metadata(&path)?;
        let is_symlink = meta.file_type().is_symlink();

        let symlink_target = if is_symlink {
            fs::read_link(&path)
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        } else {
            None
        };

        Ok(json!({
            "size": meta.len(),
            "is_file": meta.is_file(),
            "is_dir": meta.is_dir(),
            "is_symlink": is_symlink,
            "symlink_target": symlink_target,
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
        let path = resolve_path(ctx, path_str)?;
        Ok(json!({ "exists": path.exists() || fs::symlink_metadata(&path).is_ok() }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(FileExistsTool)
    }
}
