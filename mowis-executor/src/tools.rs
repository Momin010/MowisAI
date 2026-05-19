//! Tool registry for the executor.
//!
//! MVP scope: a tiny set sufficient to prove the host<->guest transport works.
//! The full set of 28 tools from `agentd/src/tools/` will be ported once the
//! transport is validated.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::Path;

use crate::sandbox::Sandbox;

pub fn invoke(sandbox: &Sandbox, tool: &str, input: Value) -> Result<Value> {
    match tool {
        "read_file" => read_file(sandbox, input),
        "write_file" => write_file(sandbox, input),
        "list_dir" => list_dir(sandbox, input),
        other => Err(anyhow!("tool `{other}` not implemented in MVP executor")),
    }
}

fn resolved_path(sandbox: &Sandbox, rel: &str) -> std::path::PathBuf {
    let trimmed = rel.trim_start_matches('/');
    sandbox.root_path().join(trimmed)
}

fn read_file(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("read_file: missing `path`"))?;
    let resolved = resolved_path(sandbox, path);
    let contents = std::fs::read_to_string(&resolved)
        .map_err(|e| anyhow!("read_file `{}`: {}", resolved.display(), e))?;
    Ok(json!({ "path": path, "contents": contents }))
}

fn write_file(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("write_file: missing `path`"))?;
    let contents = input
        .get("contents")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("write_file: missing `contents`"))?;
    let resolved = resolved_path(sandbox, path);
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&resolved, contents)
        .map_err(|e| anyhow!("write_file `{}`: {}", resolved.display(), e))?;
    Ok(json!({ "path": path, "bytes": contents.len() }))
}

fn list_dir(sandbox: &Sandbox, input: Value) -> Result<Value> {
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("/");
    let resolved = resolved_path(sandbox, path);
    let entries = list_entries(&resolved)?;
    Ok(json!({ "path": path, "entries": entries }))
}

fn list_entries(dir: &Path) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| anyhow!("list_dir `{}`: {}", dir.display(), e))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        out.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "is_dir": file_type.is_dir(),
            "is_symlink": file_type.is_symlink(),
        }));
    }
    Ok(out)
}
