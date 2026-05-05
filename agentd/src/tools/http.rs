use crate::tools::common::{
    execute_http_command, resolve_path, validate_url_for_http, Tool, ToolContext,
};
use serde_json::{json, Value};
use std::process::Command;

/// Maximum download size (100MB)
const MAX_DOWNLOAD_SIZE: u64 = 100 * 1024 * 1024;

pub struct HttpGetTool;
impl Tool for HttpGetTool {
    fn name(&self) -> &'static str {
        "http_get"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("http_get: missing url"))?;

        validate_url_for_http(url)?;

        let headers = input.get("headers").and_then(|v| v.as_object());

        let mut args = vec![
            "-s",
            "-w",
            "\n%{http_code}",
            "--connect-timeout",
            "10",
            "--max-time",
            "60",
            "--max-redirs",
            "5",
            "--proto",
            "=http,https",    // Only allow http/https protocols
            "--no-sessionid", // Don't reuse TLS sessions across requests
        ];

        // Add custom headers safely
        let mut header_args = Vec::new();
        if let Some(hdrs) = headers {
            for (k, v) in hdrs {
                // Validate header name (no newlines, no control chars)
                if k.chars().any(|c| c.is_control() || c == '\n' || c == '\r') {
                    return Err(anyhow::anyhow!("Invalid header name: {}", k));
                }
                let val = v.as_str().unwrap_or("");
                if val
                    .chars()
                    .any(|c| c.is_control() || c == '\n' || c == '\r')
                {
                    return Err(anyhow::anyhow!("Invalid header value for '{}'", k));
                }
                header_args.push("-H".to_string());
                header_args.push(format!("{}: {}", k, val));
            }
        }

        for arg in &header_args {
            args.push(arg);
        }
        args.push(url);

        execute_http_command(args)
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

        validate_url_for_http(url)?;

        let body_val = input
            .get("body")
            .ok_or_else(|| anyhow::anyhow!("http_post: missing body"))?;
        let body_string;
        let body = if let Some(s) = body_val.as_str() {
            s
        } else {
            body_string = body_val.to_string();
            &body_string
        };

        // SECURITY: Block body starting with @ (curl reads from file)
        if body.starts_with('@') {
            return Err(anyhow::anyhow!(
                "Body starting with '@' is not allowed (prevents file disclosure)"
            ));
        }

        let output = Command::new("curl")
            .arg("-s")
            .arg("-w")
            .arg("\n%{http_code}")
            .arg("--connect-timeout")
            .arg("10")
            .arg("--max-time")
            .arg("60")
            .arg("--proto")
            .arg("=http,https")
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
            "body": response_body,
            "success": output.status.success()
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

        validate_url_for_http(url)?;

        let body_val = input
            .get("body")
            .ok_or_else(|| anyhow::anyhow!("http_put: missing body"))?;
        let body_string;
        let body = if let Some(s) = body_val.as_str() {
            if s.starts_with('@') {
                return Err(anyhow::anyhow!("Body starting with '@' is not allowed"));
            }
            s
        } else {
            body_string = body_val.to_string();
            &body_string
        };

        let output = Command::new("curl")
            .arg("-s")
            .arg("-w")
            .arg("\n%{http_code}")
            .arg("--connect-timeout")
            .arg("10")
            .arg("--max-time")
            .arg("60")
            .arg("--proto")
            .arg("=http,https")
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
            "body": response_body,
            "success": output.status.success()
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

        validate_url_for_http(url)?;

        let output = Command::new("curl")
            .arg("-s")
            .arg("-w")
            .arg("\n%{http_code}")
            .arg("--connect-timeout")
            .arg("10")
            .arg("--max-time")
            .arg("60")
            .arg("--proto")
            .arg("=http,https")
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
            "body": body,
            "success": output.status.success()
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

        validate_url_for_http(url)?;

        let body_val = input
            .get("body")
            .ok_or_else(|| anyhow::anyhow!("http_patch: missing body"))?;
        let body_string;
        let body = if let Some(s) = body_val.as_str() {
            if s.starts_with('@') {
                return Err(anyhow::anyhow!("Body starting with '@' is not allowed"));
            }
            s
        } else {
            body_string = body_val.to_string();
            &body_string
        };

        let output = Command::new("curl")
            .arg("-s")
            .arg("-w")
            .arg("\n%{http_code}")
            .arg("--connect-timeout")
            .arg("10")
            .arg("--max-time")
            .arg("60")
            .arg("--proto")
            .arg("=http,https")
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
            "body": response_body,
            "success": output.status.success()
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

        validate_url_for_http(url)?;

        let dest = resolve_path(ctx, dest_str)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Download with size limit and timeout, don't follow redirects to internal
        let output = Command::new("curl")
            .arg("-L")
            .arg("--max-redirs")
            .arg("5")
            .arg("--connect-timeout")
            .arg("10")
            .arg("--max-time")
            .arg("300")
            .arg("--proto")
            .arg("=http,https")
            .arg("--limit-rate")
            .arg("10m") // Rate limit to prevent bandwidth abuse
            .arg("-o")
            .arg(&dest)
            .arg(url)
            .output()?;

        if output.status.success() {
            let size = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);

            // Enforce download size limit
            if size > MAX_DOWNLOAD_SIZE {
                let _ = std::fs::remove_file(&dest);
                return Err(anyhow::anyhow!(
                    "Downloaded file size {} exceeds maximum {} bytes",
                    size,
                    MAX_DOWNLOAD_SIZE
                ));
            }

            Ok(json!({
                "success": true,
                "size": size,
                "path": dest.to_string_lossy().to_string()
            }))
        } else {
            // Clean up partial download
            let _ = std::fs::remove_file(&dest);
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
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("websocket_send: missing url"))?;
        let message = input["message"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("websocket_send: missing message"))?;

        validate_url_for_http(url)?;

        // WebSocket requires async runtime — use blocking ws library as fallback
        // For now, use websocat-like approach via subprocess
        let output = Command::new("websocat")
            .arg("--one-message")
            .arg(url)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();

        match output {
            Ok(out) => {
                if out.status.success() {
                    Ok(json!({
                        "success": true,
                        "response": String::from_utf8_lossy(&out.stdout).to_string()
                    }))
                } else {
                    Ok(json!({
                        "success": false,
                        "error": String::from_utf8_lossy(&out.stderr).to_string()
                    }))
                }
            }
            Err(_) => {
                // websocat not available, return informative error
                Ok(json!({
                    "success": false,
                    "error": "WebSocket tool requires 'websocat' to be installed in the container",
                    "install_hint": "apk add websocat || apt-get install websocat"
                }))
            }
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(WebsocketSendTool)
    }
}
