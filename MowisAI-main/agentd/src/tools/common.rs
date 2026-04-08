use lazy_static::lazy_static;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

/// Context passed to a tool invocation
pub struct ToolContext {
    pub sandbox_id: u64,
    pub root_path: Option<PathBuf>,
}

/// A trait that all tools must implement
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

/// Definition of a tool that can be registered with a sandbox
pub struct ToolDefinition {
    pub name: String,
}

impl ToolDefinition {
    pub fn new(name: impl Into<String>) -> Self {
        ToolDefinition { name: name.into() }
    }
}

/// Helper to resolve paths against container root
pub fn resolve_path(ctx: &ToolContext, path: &str) -> PathBuf {
    let base = ctx
        .root_path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/tmp".to_string());

    if path.starts_with('/') {
        PathBuf::from(format!("{}/{}", base, path.trim_start_matches('/')))
    } else {
        PathBuf::from(format!("{}/{}", base, path))
    }
}

pub fn execute_http_command(cmd: Vec<&str>) -> anyhow::Result<Value> {
    let output = Command::new("curl").args(&cmd).output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    if lines.is_empty() {
        return Ok(json!({ "status": 0, "body": "" }));
    }

    let status_code: i64 = lines.last().and_then(|s| s.parse().ok()).unwrap_or(0);
    let body = lines[..lines.len().saturating_sub(1)].join("\n");

    Ok(json!({
        "status": status_code,
        "body": body,
        "headers": {}
    }))
}

// Global memory store for memory tools
lazy_static! {
    pub static ref MEMORY_STORE: Mutex<HashMap<String, Value>> = Mutex::new(HashMap::new());
    pub static ref SECRET_STORE: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
    pub static ref CHANNELS: Mutex<HashMap<String, Vec<Value>>> = Mutex::new(HashMap::new());
}
