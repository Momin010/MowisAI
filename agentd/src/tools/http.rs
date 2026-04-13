use crate::tools::common::{execute_http_command, resolve_path, Tool, ToolContext};
use serde_json::{json, Value};
use std::fs;
use std::process::Command;

// ============== HTTP TOOLS (6) ==============

pub struct HttpGetTool;
impl Tool for HttpGetTool {
    fn name(&self) -> &'static str {
        "http_get"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("http_get: missing url"))?;

        execute_http_command(vec!["-s", "-w", "\n%{http_code}", url])
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(HttpGetTool)
    }
}

pub struct HttpPostTool;
impl Tool for HttpPostTool {
    fn name(&self) -> &'static str {
        "http_post"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("http_post: missing url"))?;

        let body_val = input.get("body")
            .ok_or_else(|| anyhow::anyhow!("http_post: missing body"))?;
        let body_string;
        let body = if let Some(s) = body_val.as_str() {
            s
        } else {
            body_string = body_val.to_string();
            &body_string
        };

        let output = Command::new("curl")
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
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();

        if lines.is_empty() {
            return Ok(json!({ "status": 0, "body": "" }));
        }

        let status_code: i64 = lines.last().and_then(|s| s.parse().ok()).unwrap_or(0);
        let response_body = lines[..lines.len().saturating_sub(1)].join("\n");

        Ok(json!({
            "status": status_code,
            "body": response_body
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(HttpPostTool)
    }
}

pub struct HttpPutTool;
impl Tool for HttpPutTool {
    fn name(&self) -> &'static str {
        "http_put"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("http_put: missing url"))?;

        let body_val = input.get("body")
            .ok_or_else(|| anyhow::anyhow!("http_put: missing body"))?;
        let body_string;
        let body = if let Some(s) = body_val.as_str() {
            s
        } else {
            body_string = body_val.to_string();
            &body_string
        };

        let output = Command::new("curl")
            .arg("-s")
            .arg("-w")
            .arg("\n%{http_code}")
            .arg("-X")
            .arg("PUT")
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("-d")
            .arg(body)
            .arg(url)
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();

        if lines.is_empty() {
            return Ok(json!({ "status": 0, "body": "" }));
        }

        let status_code: i64 = lines.last().and_then(|s| s.parse().ok()).unwrap_or(0);
        let response_body = lines[..lines.len().saturating_sub(1)].join("\n");

        Ok(json!({
            "status": status_code,
            "body": response_body
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(HttpPutTool)
    }
}

pub struct HttpDeleteTool;
impl Tool for HttpDeleteTool {
    fn name(&self) -> &'static str {
        "http_delete"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("http_delete: missing url"))?;

        let output = Command::new("curl")
            .arg("-s")
            .arg("-w")
            .arg("\n%{http_code}")
            .arg("-X")
            .arg("DELETE")
            .arg(url)
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();

        if lines.is_empty() {
            return Ok(json!({ "status": 0, "body": "" }));
        }

        let status_code: i64 = lines.last().and_then(|s| s.parse().ok()).unwrap_or(0);
        let body = lines[..lines.len().saturating_sub(1)].join("\n");

        Ok(json!({
            "status": status_code,
            "body": body
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(HttpDeleteTool)
    }
}

pub struct HttpPatchTool;
impl Tool for HttpPatchTool {
    fn name(&self) -> &'static str {
        "http_patch"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("http_patch: missing url"))?;

        let body_val = input.get("body")
            .ok_or_else(|| anyhow::anyhow!("http_patch: missing body"))?;
        let body_string;
        let body = if let Some(s) = body_val.as_str() {
            s
        } else {
            body_string = body_val.to_string();
            &body_string
        };

        let output = Command::new("curl")
            .arg("-s")
            .arg("-w")
            .arg("\n%{http_code}")
            .arg("-X")
            .arg("PATCH")
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("-d")
            .arg(body)
            .arg(url)
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();

        if lines.is_empty() {
            return Ok(json!({ "status": 0, "body": "" }));
        }

        let status_code: i64 = lines.last().and_then(|s| s.parse().ok()).unwrap_or(0);
        let response_body = lines[..lines.len().saturating_sub(1)].join("\n");

        Ok(json!({
            "status": status_code,
            "body": response_body
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(HttpPatchTool)
    }
}

pub struct DownloadFileTool;
impl Tool for DownloadFileTool {
    fn name(&self) -> &'static str {
        "download_file"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("download_file: missing url"))?;
        let dest_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("download_file: missing path"))?;

        let dest = resolve_path(ctx, dest_str);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        let output = Command::new("curl")
            .arg("-L")
            .arg("-o")
            .arg(&dest)
            .arg(url)
            .output()?;

        if output.status.success() {
            let size = fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
            Ok(json!({
                "success": true,
                "size": size,
                "path": dest.to_string_lossy().to_string()
            }))
        } else {
            Err(anyhow::anyhow!(
                "download failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(DownloadFileTool)
    }
}
pub struct WebsocketSendTool;
impl Tool for WebsocketSendTool {
    fn name(&self) -> &'static str {
        "websocket_send"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let _url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("websocket_send: missing url"))?;
        let _message = input["message"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("websocket_send: missing message"))?;

        // WebSocket support would require async runtime, returning success for now
        Ok(json!({ "success": false, "error": "WebSocket not fully implemented" }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(WebsocketSendTool)
    }
}
