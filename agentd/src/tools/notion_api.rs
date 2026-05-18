use crate::tools::common::{Tool, ToolContext};
use serde_json::{json, Value};

const NOTION_API: &str = "https://api.notion.com/v1";
const NOTION_VERSION: &str = "2022-06-28";

fn token() -> anyhow::Result<String> {
    std::env::var("NOTION_TOKEN")
        .map_err(|_| anyhow::anyhow!("NOTION_TOKEN not set (integration token from notion.com/my-integrations)"))
}

fn client(token: &str) -> anyhow::Result<reqwest::blocking::Client> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", token))
            .map_err(|e| anyhow::anyhow!("invalid token: {}", e))?,
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        "Notion-Version",
        HeaderValue::from_static(NOTION_VERSION),
    );
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {}", e))
}

fn get(c: &reqwest::blocking::Client, url: &str) -> anyhow::Result<Value> {
    let resp = c.get(url).send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let body: Value = resp.json().unwrap_or(json!(null));
    if status >= 400 {
        return Err(anyhow::anyhow!("Notion API error {}: {}", status, body["message"].as_str().unwrap_or("")));
    }
    Ok(body)
}

fn post(c: &reqwest::blocking::Client, url: &str, body: Value) -> anyhow::Result<Value> {
    let resp = c.post(url).json(&body).send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let data: Value = resp.json().unwrap_or(json!(null));
    if status >= 400 {
        return Err(anyhow::anyhow!("Notion API error {}: {}", status, data["message"].as_str().unwrap_or("")));
    }
    Ok(data)
}

fn patch(c: &reqwest::blocking::Client, url: &str, body: Value) -> anyhow::Result<Value> {
    let resp = c.patch(url).json(&body).send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let data: Value = resp.json().unwrap_or(json!(null));
    if status >= 400 {
        return Err(anyhow::anyhow!("Notion API error {}: {}", status, data["message"].as_str().unwrap_or("")));
    }
    Ok(data)
}

fn text_block(text: &str) -> Value {
    json!({
        "object": "block",
        "type": "paragraph",
        "paragraph": {
            "rich_text": [{ "type": "text", "text": { "content": text } }]
        }
    })
}

// ─── notion_search ───────────────────────────────────────────────────────────

pub struct NotionSearchTool;
impl Tool for NotionSearchTool {
    fn name(&self) -> &'static str { "notion_search" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let query = input["query"].as_str().unwrap_or("");
        let mut body = json!({ "query": query, "page_size": input["page_size"].as_u64().unwrap_or(10).min(100) });
        if let Some(filter_type) = input["filter_type"].as_str() {
            body["filter"] = json!({ "value": filter_type, "property": "object" });
        }
        post(&c, &format!("{}/search", NOTION_API), body)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(NotionSearchTool) }
}

// ─── notion_get_page ─────────────────────────────────────────────────────────

pub struct NotionGetPageTool;
impl Tool for NotionGetPageTool {
    fn name(&self) -> &'static str { "notion_get_page" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let page_id = input["page_id"].as_str().ok_or_else(|| anyhow::anyhow!("missing page_id"))?;
        let page = get(&c, &format!("{}/pages/{}", NOTION_API, page_id))?;
        // Optionally fetch blocks
        let blocks = get(&c, &format!("{}/blocks/{}/children?page_size=100", NOTION_API, page_id)).ok();
        Ok(json!({ "page": page, "blocks": blocks }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(NotionGetPageTool) }
}

// ─── notion_create_page ──────────────────────────────────────────────────────

pub struct NotionCreatePageTool;
impl Tool for NotionCreatePageTool {
    fn name(&self) -> &'static str { "notion_create_page" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let title = input["title"].as_str().ok_or_else(|| anyhow::anyhow!("missing title"))?;
        // Parent must be either a page_id or database_id
        let parent = if let Some(db_id) = input["database_id"].as_str() {
            json!({ "database_id": db_id })
        } else if let Some(page_id) = input["parent_page_id"].as_str() {
            json!({ "page_id": page_id })
        } else {
            return Err(anyhow::anyhow!("provide either database_id or parent_page_id"));
        };
        let mut properties = json!({
            "title": {
                "title": [{ "text": { "content": title } }]
            }
        });
        // Merge any extra properties provided
        if let Some(extra) = input.get("properties").and_then(|p| p.as_object()) {
            for (k, v) in extra {
                properties[k] = v.clone();
            }
        }
        let mut body = json!({ "parent": parent, "properties": properties });
        if let Some(content) = input["content"].as_str() {
            body["children"] = json!([text_block(content)]);
        }
        post(&c, &format!("{}/pages", NOTION_API), body)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(NotionCreatePageTool) }
}

// ─── notion_append_block ─────────────────────────────────────────────────────

pub struct NotionAppendBlockTool;
impl Tool for NotionAppendBlockTool {
    fn name(&self) -> &'static str { "notion_append_block" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let block_id = input["block_id"].as_str().ok_or_else(|| anyhow::anyhow!("missing block_id (page or block ID)"))?;
        let children = if let Some(c) = input.get("children").and_then(|v| v.as_array()) {
            json!(c)
        } else if let Some(text) = input["text"].as_str() {
            json!([text_block(text)])
        } else {
            return Err(anyhow::anyhow!("provide either children (array of blocks) or text"));
        };
        patch(&c, &format!("{}/blocks/{}/children", NOTION_API, block_id), json!({ "children": children }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(NotionAppendBlockTool) }
}

// ─── notion_query_database ───────────────────────────────────────────────────

pub struct NotionQueryDatabaseTool;
impl Tool for NotionQueryDatabaseTool {
    fn name(&self) -> &'static str { "notion_query_database" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let db_id = input["database_id"].as_str().ok_or_else(|| anyhow::anyhow!("missing database_id"))?;
        let page_size = input["page_size"].as_u64().unwrap_or(25).min(100);
        let mut body = json!({ "page_size": page_size });
        if let Some(filter) = input.get("filter") { body["filter"] = filter.clone(); }
        if let Some(sorts) = input.get("sorts") { body["sorts"] = sorts.clone(); }
        post(&c, &format!("{}/databases/{}/query", NOTION_API, db_id), body)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(NotionQueryDatabaseTool) }
}

// ─── notion_update_page ──────────────────────────────────────────────────────

pub struct NotionUpdatePageTool;
impl Tool for NotionUpdatePageTool {
    fn name(&self) -> &'static str { "notion_update_page" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let page_id = input["page_id"].as_str().ok_or_else(|| anyhow::anyhow!("missing page_id"))?;
        let mut body = json!({});
        if let Some(props) = input.get("properties") { body["properties"] = props.clone(); }
        if let Some(archived) = input["archived"].as_bool() { body["archived"] = json!(archived); }
        patch(&c, &format!("{}/pages/{}", NOTION_API, page_id), body)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(NotionUpdatePageTool) }
}

// ─── Factory functions ────────────────────────────────────────────────────────

pub fn create_notion_search_tool() -> Box<dyn Tool> { Box::new(NotionSearchTool) }
pub fn create_notion_get_page_tool() -> Box<dyn Tool> { Box::new(NotionGetPageTool) }
pub fn create_notion_create_page_tool() -> Box<dyn Tool> { Box::new(NotionCreatePageTool) }
pub fn create_notion_append_block_tool() -> Box<dyn Tool> { Box::new(NotionAppendBlockTool) }
pub fn create_notion_query_database_tool() -> Box<dyn Tool> { Box::new(NotionQueryDatabaseTool) }
pub fn create_notion_update_page_tool() -> Box<dyn Tool> { Box::new(NotionUpdatePageTool) }
