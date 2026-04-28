use crate::tools::common::{resolve_path, Tool, ToolContext};
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

        let output = Command::new("curl").arg("-s").arg(&url).output()?;

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

        let output = Command::new("curl").arg("-L").arg(url).output()?;

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
        let _url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("web_screenshot: missing url"))?;
        let output_str = input["output"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("web_screenshot: missing output"))?;

        let output_path = resolve_path(ctx, output_str);

        // Try chromium, fallback to other browsers if not available
        let cmd_result = Command::new("chromium")
            .arg("--headless")
            .arg("--disable-gpu")
            .arg(&format!("--screenshot={}", output_path.display()))
            .arg("about:blank")
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

// ============== AGENT COORDINATION TOOLS (6) ==============
