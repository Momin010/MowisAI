use crate::tools::common::{Tool, ToolContext};
use serde_json::{json, Value};

fn token() -> anyhow::Result<String> {
    std::env::var("SENTRY_AUTH_TOKEN")
        .map_err(|_| anyhow::anyhow!("SENTRY_AUTH_TOKEN not set"))
}

fn org() -> anyhow::Result<String> {
    std::env::var("SENTRY_ORG")
        .map_err(|_| anyhow::anyhow!("SENTRY_ORG not set (your Sentry organization slug)"))
}

fn base() -> String {
    std::env::var("SENTRY_BASE_URL").unwrap_or_else(|_| "https://sentry.io".to_string())
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

fn get(c: &reqwest::blocking::Client, url: &str) -> anyhow::Result<Value> {
    let resp = c.get(url).send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let body: Value = resp.json().unwrap_or(json!(null));
    if status >= 400 {
        return Err(anyhow::anyhow!("Sentry API error {}: {:?}", status, body));
    }
    Ok(body)
}

fn put(c: &reqwest::blocking::Client, url: &str, body: Value) -> anyhow::Result<Value> {
    let resp = c.put(url).json(&body).send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let data: Value = resp.json().unwrap_or(json!(null));
    if status >= 400 {
        return Err(anyhow::anyhow!("Sentry API error {}: {:?}", status, data));
    }
    Ok(data)
}

// ─── sentry_list_projects ────────────────────────────────────────────────────

pub struct SentryListProjectsTool;
impl Tool for SentryListProjectsTool {
    fn name(&self) -> &'static str { "sentry_list_projects" }
    fn invoke(&self, _ctx: &ToolContext, _input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let org = org()?;
        let c = client(&tok)?;
        get(&c, &format!("{}/api/0/organizations/{}/projects/", base(), org))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SentryListProjectsTool) }
}

// ─── sentry_list_issues ──────────────────────────────────────────────────────

pub struct SentryListIssuesTool;
impl Tool for SentryListIssuesTool {
    fn name(&self) -> &'static str { "sentry_list_issues" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let org = org()?;
        let c = client(&tok)?;
        let project = input["project"].as_str().ok_or_else(|| anyhow::anyhow!("missing project slug"))?;
        let limit = input["limit"].as_u64().unwrap_or(25).min(100);
        let query = input["query"].as_str().unwrap_or("is:unresolved");
        let encoded_query = urlencoding::encode(query);
        get(&c, &format!(
            "{}/api/0/projects/{}/{}/issues/?query={}&limit={}",
            base(), org, project, encoded_query, limit
        ))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SentryListIssuesTool) }
}

// ─── sentry_get_issue ────────────────────────────────────────────────────────

pub struct SentryGetIssueTool;
impl Tool for SentryGetIssueTool {
    fn name(&self) -> &'static str { "sentry_get_issue" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let issue_id = input["issue_id"].as_str().ok_or_else(|| anyhow::anyhow!("missing issue_id"))?;
        get(&c, &format!("{}/api/0/issues/{}/", base(), issue_id))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SentryGetIssueTool) }
}

// ─── sentry_update_issue ─────────────────────────────────────────────────────

pub struct SentryUpdateIssueTool;
impl Tool for SentryUpdateIssueTool {
    fn name(&self) -> &'static str { "sentry_update_issue" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let issue_id = input["issue_id"].as_str().ok_or_else(|| anyhow::anyhow!("missing issue_id"))?;
        // status: resolved | ignored | unresolved
        let status = input["status"].as_str().ok_or_else(|| anyhow::anyhow!("missing status (resolved|ignored|unresolved)"))?;
        let mut body = json!({ "status": status });
        if let Some(assigned) = input["assignedTo"].as_str() { body["assignedTo"] = json!(assigned); }
        put(&c, &format!("{}/api/0/issues/{}/", base(), issue_id), body)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SentryUpdateIssueTool) }
}

// ─── sentry_list_events ──────────────────────────────────────────────────────

pub struct SentryListEventsTool;
impl Tool for SentryListEventsTool {
    fn name(&self) -> &'static str { "sentry_list_events" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let issue_id = input["issue_id"].as_str().ok_or_else(|| anyhow::anyhow!("missing issue_id"))?;
        let limit = input["limit"].as_u64().unwrap_or(10).min(100);
        get(&c, &format!("{}/api/0/issues/{}/events/?limit={}", base(), issue_id, limit))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SentryListEventsTool) }
}

// ─── sentry_get_event ────────────────────────────────────────────────────────

pub struct SentryGetEventTool;
impl Tool for SentryGetEventTool {
    fn name(&self) -> &'static str { "sentry_get_event" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let org = org()?;
        let c = client(&tok)?;
        let project = input["project"].as_str().ok_or_else(|| anyhow::anyhow!("missing project slug"))?;
        let event_id = input["event_id"].as_str().ok_or_else(|| anyhow::anyhow!("missing event_id"))?;
        get(&c, &format!("{}/api/0/projects/{}/{}/events/{}/", base(), org, project, event_id))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SentryGetEventTool) }
}

// ─── sentry_list_releases ────────────────────────────────────────────────────

pub struct SentryListReleasesTool;
impl Tool for SentryListReleasesTool {
    fn name(&self) -> &'static str { "sentry_list_releases" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let org = org()?;
        let c = client(&tok)?;
        let per_page = input["per_page"].as_u64().unwrap_or(10).min(100);
        get(&c, &format!("{}/api/0/organizations/{}/releases/?per_page={}", base(), org, per_page))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(SentryListReleasesTool) }
}

// ─── Factory functions ────────────────────────────────────────────────────────

pub fn create_sentry_list_projects_tool() -> Box<dyn Tool> { Box::new(SentryListProjectsTool) }
pub fn create_sentry_list_issues_tool() -> Box<dyn Tool> { Box::new(SentryListIssuesTool) }
pub fn create_sentry_get_issue_tool() -> Box<dyn Tool> { Box::new(SentryGetIssueTool) }
pub fn create_sentry_update_issue_tool() -> Box<dyn Tool> { Box::new(SentryUpdateIssueTool) }
pub fn create_sentry_list_events_tool() -> Box<dyn Tool> { Box::new(SentryListEventsTool) }
pub fn create_sentry_get_event_tool() -> Box<dyn Tool> { Box::new(SentryGetEventTool) }
pub fn create_sentry_list_releases_tool() -> Box<dyn Tool> { Box::new(SentryListReleasesTool) }
