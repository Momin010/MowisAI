use crate::tools::common::{resolve_path, validate_url_for_http, Tool, ToolContext};
use serde_json::{json, Value};
use std::process::Command;
use urlencoding;

pub struct WebSearchTool;
impl Tool for WebSearchTool {
    fn name(&self) -> &'static str {
        "web_search"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("web_search: missing query"))?;

        let encoded = urlencoding::encode(query);
        let url = format!("https://api.duckduckgo.com/?q={}&format=json", encoded);

        let output = Command::new("curl")
            .arg("-s")
            .arg("--connect-timeout")
            .arg("10")
            .arg("--max-time")
            .arg("30")
            .arg("--proto")
            .arg("=https")
            .arg(&url)
            .output()?;

        let body = String::from_utf8_lossy(&output.stdout).to_string();
        match serde_json::from_str::<Value>(&body) {
            Ok(json) => Ok(json!({ "results": json })),
            Err(_) => Ok(json!({ "results": [] })),
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(WebSearchTool)
    }
}

pub struct WebFetchTool;
impl Tool for WebFetchTool {
    fn name(&self) -> &'static str {
        "web_fetch"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("web_fetch: missing url"))?;

        // SECURITY: Validate URL against SSRF
        validate_url_for_http(url)?;

        let output = Command::new("curl")
            .arg("-L")
            .arg("--max-redirs")
            .arg("5")
            .arg("--connect-timeout")
            .arg("10")
            .arg("--max-time")
            .arg("60")
            .arg("--proto")
            .arg("=http,https")
            .arg("--limit-rate")
            .arg("5m")
            .arg(url)
            .output()?;

        Ok(json!({
            "content": String::from_utf8_lossy(&output.stdout).to_string(),
            "status": if output.status.success() { 200 } else { 0 }
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(WebFetchTool)
    }
}

pub struct WebScreenshotTool;
impl Tool for WebScreenshotTool {
    fn name(&self) -> &'static str {
        "web_screenshot"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("web_screenshot: missing url"))?;

        // SECURITY: Validate URL
        validate_url_for_http(url)?;

        let output_str = input["output"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("web_screenshot: missing output"))?;

        let output_path = resolve_path(ctx, output_str)?;

        // Try chromium, use the actual URL
        let cmd_result = Command::new("chromium")
            .arg("--headless")
            .arg("--disable-gpu")
            .arg("--no-sandbox")
            .arg("--disable-dev-shm-usage")
            .arg(&format!("--screenshot={}", output_path.display()))
            .arg(url)
            .output();

        match cmd_result {
            Ok(output) => {
                let error_val = if output.status.success() {
                    json!(null)
                } else {
                    json!(String::from_utf8_lossy(&output.stderr).to_string())
                };
                Ok(json!({
                    "success": output.status.success(),
                    "path": output_path.to_string_lossy().to_string(),
                    "error": error_val
                }))
            }
            Err(e) => Ok(json!({
                "success": false,
                "error": format!("chromium not available: {}", e)
            })),
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(WebScreenshotTool)
    }
}
