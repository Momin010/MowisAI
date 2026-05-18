use crate::tools::common::{Tool, ToolContext};
use serde_json::{json, Value};

const SLACK_API: &str = "https://slack.com/api";

fn token() -> anyhow::Result<String> {
    std::env::var("SLACK_BOT_TOKEN")
        .map_err(|_| anyhow::anyhow!("SLACK_BOT_TOKEN not set — add it to your environment"))
}

fn client(token: &str) -> anyhow::Result<reqwest::blocking::Client> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", token))
            .map_err(|e| anyhow::anyhow!("invalid token: {}", e))?,
    );
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {}", e))
}

fn check_ok(body: &Value) -> anyhow::Result<()> {
    if body["ok"].as_bool() != Some(true) {
        let err = body["error"].as_str().unwrap_or("unknown error");
        return Err(anyhow::anyhow!("Slack API error: {}", err));
    }
    Ok(())
}

// ─── slack_post_message ──────────────────────────────────────────────────────

pub struct SlackPostMessageTool;
impl Tool for SlackPostMessageTool {
    fn name(&self) -> &'static str { "slack_post_message" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let channel = input["channel"].as_str().ok_or_else(|| anyhow::anyhow!("missing channel"))?;
        let text = input["text"].as_str().ok_or_else(|| anyhow::anyhow!("missing text"))?;
        let mut body = json!({ "channel": channel, "text": text });
        if let Some(thread_ts) = input["thread_ts"].as_str() { body["thread_ts"] = json!(thread_ts); }
        if let Some(blocks) = input.get("blocks") { body["blocks"] = blocks.clone(); }
        if let Some(username) = input["username"].as_str() { body["username"] = json!(username); }
        let resp: Value = c.post(format!("{}/chat.postMessage", SLACK_API))
            .json(&body).send()
            .map_err(|e| anyhow::anyhow!("request failed: {}", e))?
            .json().map_err(|e| anyhow::anyhow!("failed to parse response: {}", e))?;
        check_ok(&resp)?;
        Ok(json!({ "ts": resp["ts"], "channel": resp["channel"], "ok": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SlackPostMessageTool) }
}

// ─── slack_list_channels ─────────────────────────────────────────────────────

pub struct SlackListChannelsTool;
impl Tool for SlackListChannelsTool {
    fn name(&self) -> &'static str { "slack_list_channels" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let limit = input["limit"].as_u64().unwrap_or(100).min(1000);
        let exclude_archived = input["exclude_archived"].as_bool().unwrap_or(true);
        let types = input["types"].as_str().unwrap_or("public_channel");
        let resp: Value = c.get(format!("{}/conversations.list", SLACK_API))
            .query(&[
                ("limit", limit.to_string()),
                ("exclude_archived", exclude_archived.to_string()),
                ("types", types.to_string()),
            ])
            .send()
            .map_err(|e| anyhow::anyhow!("request failed: {}", e))?
            .json().map_err(|e| anyhow::anyhow!("failed to parse response: {}", e))?;
        check_ok(&resp)?;
        Ok(json!({ "channels": resp["channels"], "next_cursor": resp["response_metadata"]["next_cursor"] }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SlackListChannelsTool) }
}

// ─── slack_get_channel_history ───────────────────────────────────────────────

pub struct SlackGetChannelHistoryTool;
impl Tool for SlackGetChannelHistoryTool {
    fn name(&self) -> &'static str { "slack_get_channel_history" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let channel = input["channel"].as_str().ok_or_else(|| anyhow::anyhow!("missing channel"))?;
        let limit = input["limit"].as_u64().unwrap_or(50).min(200);
        let mut params = vec![
            ("channel".to_string(), channel.to_string()),
            ("limit".to_string(), limit.to_string()),
        ];
        if let Some(oldest) = input["oldest"].as_str() { params.push(("oldest".to_string(), oldest.to_string())); }
        if let Some(latest) = input["latest"].as_str() { params.push(("latest".to_string(), latest.to_string())); }
        let resp: Value = c.get(format!("{}/conversations.history", SLACK_API))
            .query(&params).send()
            .map_err(|e| anyhow::anyhow!("request failed: {}", e))?
            .json().map_err(|e| anyhow::anyhow!("failed to parse response: {}", e))?;
        check_ok(&resp)?;
        Ok(json!({ "messages": resp["messages"], "has_more": resp["has_more"] }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SlackGetChannelHistoryTool) }
}

// ─── slack_search_messages ───────────────────────────────────────────────────

pub struct SlackSearchMessagesTool;
impl Tool for SlackSearchMessagesTool {
    fn name(&self) -> &'static str { "slack_search_messages" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let query = input["query"].as_str().ok_or_else(|| anyhow::anyhow!("missing query"))?;
        let count = input["count"].as_u64().unwrap_or(20).min(100);
        let resp: Value = c.get(format!("{}/search.messages", SLACK_API))
            .query(&[("query", query), ("count", &count.to_string())])
            .send()
            .map_err(|e| anyhow::anyhow!("request failed: {}", e))?
            .json().map_err(|e| anyhow::anyhow!("failed to parse response: {}", e))?;
        check_ok(&resp)?;
        Ok(json!({
            "total": resp["messages"]["total"],
            "matches": resp["messages"]["matches"]
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SlackSearchMessagesTool) }
}

// ─── slack_add_reaction ──────────────────────────────────────────────────────

pub struct SlackAddReactionTool;
impl Tool for SlackAddReactionTool {
    fn name(&self) -> &'static str { "slack_add_reaction" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let channel = input["channel"].as_str().ok_or_else(|| anyhow::anyhow!("missing channel"))?;
        let timestamp = input["timestamp"].as_str().ok_or_else(|| anyhow::anyhow!("missing timestamp"))?;
        let name = input["name"].as_str().ok_or_else(|| anyhow::anyhow!("missing name (emoji name without colons)"))?;
        let resp: Value = c.post(format!("{}/reactions.add", SLACK_API))
            .json(&json!({ "channel": channel, "timestamp": timestamp, "name": name }))
            .send()
            .map_err(|e| anyhow::anyhow!("request failed: {}", e))?
            .json().map_err(|e| anyhow::anyhow!("failed to parse response: {}", e))?;
        check_ok(&resp)?;
        Ok(json!({ "ok": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SlackAddReactionTool) }
}

// ─── slack_upload_file ───────────────────────────────────────────────────────
// Uses Slack's files.getUploadURLExternal + files.completeUploadExternal (v2 API)
// Falls back to posting content as a code block message for simplicity.

pub struct SlackUploadFileTool;
impl Tool for SlackUploadFileTool {
    fn name(&self) -> &'static str { "slack_upload_file" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let channel = input["channels"].as_str().ok_or_else(|| anyhow::anyhow!("missing channels"))?;
        let content = input["content"].as_str().ok_or_else(|| anyhow::anyhow!("missing content"))?;
        let filename = input["filename"].as_str().unwrap_or("snippet.txt");
        let title = input["title"].as_str().unwrap_or(filename);
        // Step 1: Get upload URL
        let url_resp: Value = c.get(format!("{}/files.getUploadURLExternal", SLACK_API))
            .query(&[("filename", filename), ("length", &content.len().to_string())])
            .send()
            .map_err(|e| anyhow::anyhow!("request failed: {}", e))?
            .json().map_err(|e| anyhow::anyhow!("failed to parse response: {}", e))?;
        check_ok(&url_resp)?;
        let upload_url = url_resp["upload_url"].as_str()
            .ok_or_else(|| anyhow::anyhow!("no upload_url in response"))?;
        let file_id = url_resp["file_id"].as_str()
            .ok_or_else(|| anyhow::anyhow!("no file_id in response"))?;
        // Step 2: Upload the content
        c.post(upload_url)
            .header("Content-Type", "application/octet-stream")
            .body(content.to_string())
            .send()
            .map_err(|e| anyhow::anyhow!("upload failed: {}", e))?;
        // Step 3: Complete the upload
        let complete_resp: Value = c.post(format!("{}/files.completeUploadExternal", SLACK_API))
            .json(&json!({
                "files": [{ "id": file_id, "title": title }],
                "channel_id": channel
            }))
            .send()
            .map_err(|e| anyhow::anyhow!("request failed: {}", e))?
            .json().map_err(|e| anyhow::anyhow!("failed to parse response: {}", e))?;
        check_ok(&complete_resp)?;
        Ok(json!({ "file_id": file_id, "ok": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SlackUploadFileTool) }
}

// ─── slack_get_user_info ─────────────────────────────────────────────────────

pub struct SlackGetUserInfoTool;
impl Tool for SlackGetUserInfoTool {
    fn name(&self) -> &'static str { "slack_get_user_info" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let user = input["user"].as_str().ok_or_else(|| anyhow::anyhow!("missing user (Slack user ID)"))?;
        let resp: Value = c.get(format!("{}/users.info", SLACK_API))
            .query(&[("user", user)])
            .send()
            .map_err(|e| anyhow::anyhow!("request failed: {}", e))?
            .json().map_err(|e| anyhow::anyhow!("failed to parse response: {}", e))?;
        check_ok(&resp)?;
        Ok(json!({ "user": resp["user"] }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SlackGetUserInfoTool) }
}

// ─── slack_update_message ────────────────────────────────────────────────────

pub struct SlackUpdateMessageTool;
impl Tool for SlackUpdateMessageTool {
    fn name(&self) -> &'static str { "slack_update_message" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let channel = input["channel"].as_str().ok_or_else(|| anyhow::anyhow!("missing channel"))?;
        let ts = input["ts"].as_str().ok_or_else(|| anyhow::anyhow!("missing ts (message timestamp)"))?;
        let text = input["text"].as_str().ok_or_else(|| anyhow::anyhow!("missing text"))?;
        let mut body = json!({ "channel": channel, "ts": ts, "text": text });
        if let Some(blocks) = input.get("blocks") { body["blocks"] = blocks.clone(); }
        let resp: Value = c.post(format!("{}/chat.update", SLACK_API))
            .json(&body).send()
            .map_err(|e| anyhow::anyhow!("request failed: {}", e))?
            .json().map_err(|e| anyhow::anyhow!("failed to parse response: {}", e))?;
        check_ok(&resp)?;
        Ok(json!({ "ts": resp["ts"], "ok": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SlackUpdateMessageTool) }
}

// ─── Factory functions ────────────────────────────────────────────────────────

pub fn create_slack_post_message_tool() -> Box<dyn Tool> { Box::new(SlackPostMessageTool) }
pub fn create_slack_list_channels_tool() -> Box<dyn Tool> { Box::new(SlackListChannelsTool) }
pub fn create_slack_get_channel_history_tool() -> Box<dyn Tool> { Box::new(SlackGetChannelHistoryTool) }
pub fn create_slack_search_messages_tool() -> Box<dyn Tool> { Box::new(SlackSearchMessagesTool) }
pub fn create_slack_add_reaction_tool() -> Box<dyn Tool> { Box::new(SlackAddReactionTool) }
pub fn create_slack_upload_file_tool() -> Box<dyn Tool> { Box::new(SlackUploadFileTool) }
pub fn create_slack_get_user_info_tool() -> Box<dyn Tool> { Box::new(SlackGetUserInfoTool) }
pub fn create_slack_update_message_tool() -> Box<dyn Tool> { Box::new(SlackUpdateMessageTool) }
