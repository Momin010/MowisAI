use crate::tools::common::{Tool, ToolContext};
use serde_json::{json, Value};

const PD_API: &str = "https://api.pagerduty.com";

fn token() -> anyhow::Result<String> {
    std::env::var("PAGERDUTY_TOKEN")
        .map_err(|_| anyhow::anyhow!("PAGERDUTY_TOKEN not set"))
}

fn client(token: &str) -> anyhow::Result<reqwest::blocking::Client> {
    use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Token token={}", token))
            .map_err(|e| anyhow::anyhow!("invalid token: {}", e))?,
    );
    headers.insert(ACCEPT, HeaderValue::from_static("application/vnd.pagerduty+json;version=2"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
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
        return Err(anyhow::anyhow!("PagerDuty API error {}: {}", status, body["error"]["message"].as_str().unwrap_or("")));
    }
    Ok(body)
}

fn post(c: &reqwest::blocking::Client, url: &str, body: Value) -> anyhow::Result<Value> {
    let resp = c.post(url).json(&body).send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let data: Value = resp.json().unwrap_or(json!(null));
    if status >= 400 {
        return Err(anyhow::anyhow!("PagerDuty API error {}: {}", status, data));
    }
    Ok(data)
}

fn put(c: &reqwest::blocking::Client, url: &str, body: Value) -> anyhow::Result<Value> {
    let resp = c.put(url).json(&body).send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let data: Value = resp.json().unwrap_or(json!(null));
    if status >= 400 {
        return Err(anyhow::anyhow!("PagerDuty API error {}: {}", status, data));
    }
    Ok(data)
}

// ─── pagerduty_list_incidents ────────────────────────────────────────────────

pub struct PagerDutyListIncidentsTool;
impl Tool for PagerDutyListIncidentsTool {
    fn name(&self) -> &'static str { "pagerduty_list_incidents" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let limit = input["limit"].as_u64().unwrap_or(25).min(100);
        let status = input["status"].as_str().unwrap_or("triggered,acknowledged");
        let mut url = format!("{}/incidents?limit={}&statuses[]={}", PD_API, limit, status);
        if let Some(service) = input["service_id"].as_str() {
            url.push_str(&format!("&service_ids[]={}", service));
        }
        if let Some(urgency) = input["urgency"].as_str() {
            url.push_str(&format!("&urgencies[]={}", urgency));
        }
        get(&c, &url)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(PagerDutyListIncidentsTool) }
}

// ─── pagerduty_get_incident ──────────────────────────────────────────────────

pub struct PagerDutyGetIncidentTool;
impl Tool for PagerDutyGetIncidentTool {
    fn name(&self) -> &'static str { "pagerduty_get_incident" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("missing id"))?;
        get(&c, &format!("{}/incidents/{}", PD_API, id))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(PagerDutyGetIncidentTool) }
}

// ─── pagerduty_acknowledge_incident ─────────────────────────────────────────

pub struct PagerDutyAcknowledgeIncidentTool;
impl Tool for PagerDutyAcknowledgeIncidentTool {
    fn name(&self) -> &'static str { "pagerduty_acknowledge_incident" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("missing id"))?;
        let from_email = input["from"].as_str().ok_or_else(|| anyhow::anyhow!("missing from (email of acknowledging user)"))?;
        let resp = c.put(format!("{}/incidents/{}", PD_API, id))
            .header("From", from_email)
            .json(&json!({ "incident": { "type": "incident", "status": "acknowledged" } }))
            .send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
        let status = resp.status().as_u16();
        let data: Value = resp.json().unwrap_or(json!(null));
        if status >= 400 {
            return Err(anyhow::anyhow!("PagerDuty API error {}: {}", status, data));
        }
        Ok(data)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(PagerDutyAcknowledgeIncidentTool) }
}

// ─── pagerduty_resolve_incident ──────────────────────────────────────────────

pub struct PagerDutyResolveIncidentTool;
impl Tool for PagerDutyResolveIncidentTool {
    fn name(&self) -> &'static str { "pagerduty_resolve_incident" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("missing id"))?;
        let from_email = input["from"].as_str().ok_or_else(|| anyhow::anyhow!("missing from (email)"))?;
        let resp = c.put(format!("{}/incidents/{}", PD_API, id))
            .header("From", from_email)
            .json(&json!({ "incident": { "type": "incident", "status": "resolved" } }))
            .send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
        let status = resp.status().as_u16();
        let data: Value = resp.json().unwrap_or(json!(null));
        if status >= 400 {
            return Err(anyhow::anyhow!("PagerDuty API error {}: {}", status, data));
        }
        Ok(data)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(PagerDutyResolveIncidentTool) }
}

// ─── pagerduty_add_note ──────────────────────────────────────────────────────

pub struct PagerDutyAddNoteTool;
impl Tool for PagerDutyAddNoteTool {
    fn name(&self) -> &'static str { "pagerduty_add_note" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("missing id"))?;
        let content = input["content"].as_str().ok_or_else(|| anyhow::anyhow!("missing content"))?;
        let from_email = input["from"].as_str().ok_or_else(|| anyhow::anyhow!("missing from (email)"))?;
        let resp = c.post(format!("{}/incidents/{}/notes", PD_API, id))
            .header("From", from_email)
            .json(&json!({ "note": { "content": content } }))
            .send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
        let status = resp.status().as_u16();
        let data: Value = resp.json().unwrap_or(json!(null));
        if status >= 400 {
            return Err(anyhow::anyhow!("PagerDuty API error {}: {}", status, data));
        }
        Ok(data)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(PagerDutyAddNoteTool) }
}

// ─── pagerduty_list_services ─────────────────────────────────────────────────

pub struct PagerDutyListServicesTool;
impl Tool for PagerDutyListServicesTool {
    fn name(&self) -> &'static str { "pagerduty_list_services" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let limit = input["limit"].as_u64().unwrap_or(25).min(100);
        get(&c, &format!("{}/services?limit={}", PD_API, limit))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(PagerDutyListServicesTool) }
}

// ─── pagerduty_list_oncalls ──────────────────────────────────────────────────

pub struct PagerDutyListOnCallsTool;
impl Tool for PagerDutyListOnCallsTool {
    fn name(&self) -> &'static str { "pagerduty_list_oncalls" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let mut url = format!("{}/oncalls?", PD_API);
        if let Some(schedule) = input["schedule_id"].as_str() {
            url.push_str(&format!("schedule_ids[]={}&", schedule));
        }
        if let Some(user) = input["user_id"].as_str() {
            url.push_str(&format!("user_ids[]={}&", user));
        }
        url.pop(); // remove trailing &
        get(&c, &url)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(PagerDutyListOnCallsTool) }
}

// ─── Factory functions ────────────────────────────────────────────────────────

pub fn create_pagerduty_list_incidents_tool() -> Box<dyn Tool> { Box::new(PagerDutyListIncidentsTool) }
pub fn create_pagerduty_get_incident_tool() -> Box<dyn Tool> { Box::new(PagerDutyGetIncidentTool) }
pub fn create_pagerduty_acknowledge_incident_tool() -> Box<dyn Tool> { Box::new(PagerDutyAcknowledgeIncidentTool) }
pub fn create_pagerduty_resolve_incident_tool() -> Box<dyn Tool> { Box::new(PagerDutyResolveIncidentTool) }
pub fn create_pagerduty_add_note_tool() -> Box<dyn Tool> { Box::new(PagerDutyAddNoteTool) }
pub fn create_pagerduty_list_services_tool() -> Box<dyn Tool> { Box::new(PagerDutyListServicesTool) }
pub fn create_pagerduty_list_oncalls_tool() -> Box<dyn Tool> { Box::new(PagerDutyListOnCallsTool) }
