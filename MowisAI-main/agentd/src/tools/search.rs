use crate::tools::common::{resolve_path, Tool, ToolContext};
use glob::Pattern;
use regex::Regex;
use serde_json::{json, Value};
use std::fs;
use walkdir::WalkDir;

fn matches_include(path: &std::path::Path, include: &str) -> bool {
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if let Ok(pat) = Pattern::new(include) {
        pat.matches(&filename)
    } else {
        true
    }
}

pub struct GrepTool;
impl Tool for GrepTool {
    fn name(&self) -> &'static str {
        "grep"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("grep: missing pattern"))?;
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("grep: missing path"))?;
        let include = input.get("include").and_then(|v| v.as_str());
        let max_results = input
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(100) as usize;

        let base = resolve_path(ctx, path_str);
        let regex = Regex::new(pattern)?;

        let mut matches = Vec::new();
        let mut files_searched = 0usize;

        'outer: for entry in WalkDir::new(&base) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            if let Some(inc) = include {
                if !matches_include(entry.path(), inc) {
                    continue;
                }
            }
            let content = match fs::read_to_string(entry.path()) {
                Ok(c) => c,
                Err(_) => continue,
            };
            files_searched += 1;
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if regex.is_match(line) {
                    let context_before = if i > 0 { lines[i - 1] } else { "" };
                    let context_after = if i + 1 < lines.len() { lines[i + 1] } else { "" };
                    matches.push(json!({
                        "file": entry.path().to_string_lossy().to_string(),
                        "line": i + 1,
                        "content": line,
                        "context_before": context_before,
                        "context_after": context_after
                    }));
                    if matches.len() >= max_results {
                        break 'outer;
                    }
                }
            }
        }

        let total = matches.len();
        Ok(json!({
            "matches": matches,
            "total_matches": total,
            "files_searched": files_searched
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(GrepTool)
    }
}

pub struct FindFilesTool;
impl Tool for FindFilesTool {
    fn name(&self) -> &'static str {
        "find_files"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("find_files: missing pattern"))?;
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("find_files: missing path"))?;
        let max_depth = input
            .get("max_depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        let base = resolve_path(ctx, path_str);
        let pat = Pattern::new(pattern).unwrap_or_else(|_| Pattern::new("*").unwrap());

        let mut files = Vec::new();

        for entry in WalkDir::new(&base).max_depth(max_depth) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().to_string();
            if pat.matches(&name) {
                files.push(entry.path().to_string_lossy().to_string());
            }
        }

        let count = files.len();
        Ok(json!({ "files": files, "count": count }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(FindFilesTool)
    }
}

pub struct SearchCodeTool;
impl Tool for SearchCodeTool {
    fn name(&self) -> &'static str {
        "search_code"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("search_code: missing query"))?;
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("search_code: missing path"))?;
        let include = input.get("include").and_then(|v| v.as_str());
        let context_lines = input
            .get("context_lines")
            .and_then(|v| v.as_u64())
            .unwrap_or(2) as usize;

        let base = resolve_path(ctx, path_str);
        let query_lower = query.to_lowercase();

        let mut results = Vec::new();
        let mut total = 0usize;

        for entry in WalkDir::new(&base) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            if let Some(inc) = include {
                if !matches_include(entry.path(), inc) {
                    continue;
                }
            }
            let content = match fs::read_to_string(entry.path()) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if line.to_lowercase().contains(&query_lower) {
                    total += 1;
                    let start = i.saturating_sub(context_lines);
                    let end = (i + context_lines + 1).min(lines.len());
                    let context: Vec<String> =
                        lines[start..end].iter().map(|l| l.to_string()).collect();
                    results.push(json!({
                        "file": entry.path().to_string_lossy().to_string(),
                        "line": i + 1,
                        "match": line,
                        "context": context
                    }));
                }
            }
        }

        Ok(json!({ "results": results, "total": total }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(SearchCodeTool)
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

        let mut files = Vec::new();
        let mut total_read = 0usize;
        let mut failed = 0usize;

        for path_val in paths {
            let path_str = path_val.as_str().unwrap_or("");
            let path = resolve_path(ctx, path_str);
            match fs::read_to_string(&path) {
                Ok(content) => {
                    let size = content.len();
                    total_read += 1;
                    files.push(json!({
                        "path": path_str,
                        "content": content,
                        "size": size,
                        "success": true
                    }));
                }
                Err(e) => {
                    failed += 1;
                    files.push(json!({
                        "path": path_str,
                        "content": null,
                        "error": e.to_string(),
                        "success": false
                    }));
                }
            }
        }

        Ok(json!({ "files": files, "total_read": total_read, "failed": failed }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(ReadMultipleFilesTool)
    }
}
