use crate::tools::common::{Tool, ToolContext};
use serde_json::{json, Value};

fn base_url() -> anyhow::Result<String> {
    std::env::var("JIRA_BASE_URL")
        .map_err(|_| anyhow::anyhow!("JIRA_BASE_URL not set (e.g. https://yourorg.atlassian.net)"))
}

fn token() -> anyhow::Result<String> {
    std::env::var("JIRA_API_TOKEN")
        .map_err(|_| anyhow::anyhow!("JIRA_API_TOKEN not set"))
}

fn email() -> anyhow::Result<String> {
    std::env::var("JIRA_EMAIL")
        .map_err(|_| anyhow::anyhow!("JIRA_EMAIL not set (your Atlassian account email)"))
}

fn client(email: &str, token: &str) -> anyhow::Result<reqwest::blocking::Client> {
    use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE};
    let creds = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        format!("{}:{}", email, token),
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Basic {}", creds))
            .map_err(|e| anyhow::anyhow!("invalid credentials: {}", e))?,
    );
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {}", e))
}

fn get(client: &reqwest::blocking::Client, url: &str) -> anyhow::Result<Value> {
    let resp = client.get(url).send()
        .map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let body: Value = resp.json().unwrap_or(json!(null));
    if status >= 400 {
        return Err(anyhow::anyhow!("Jira API error {}: {}", status, body));
    }
    Ok(body)
}

fn post(client: &reqwest::blocking::Client, url: &str, body: Value) -> anyhow::Result<Value> {
    let resp = client.post(url).json(&body).send()
        .map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let data: Value = resp.json().unwrap_or(json!(null));
    if status >= 400 {
        return Err(anyhow::anyhow!("Jira API error {}: {}", status, data));
    }
    Ok(data)
}

fn put(client: &reqwest::blocking::Client, url: &str, body: Value) -> anyhow::Result<Value> {
    let resp = client.put(url).json(&body).send()
        .map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    if status >= 400 {
        let data: Value = resp.json().unwrap_or(json!(null));
        return Err(anyhow::anyhow!("Jira API error {}: {}", status, data));
    }
    Ok(json!({ "status": status, "ok": true }))
}

// ─── jira_list_projects ──────────────────────────────────────────────────────

pub struct JiraListProjectsTool;
impl Tool for JiraListProjectsTool {
    fn name(&self) -> &'static str { "jira_list_projects" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let base = base_url()?;
        let tok = token()?;
        let em = email()?;
        let c = client(&em, &tok)?;
        let max = input["max_results"].as_u64().unwrap_or(50).min(200);
        get(&c, &format!("{}/rest/api/3/project/search?maxResults={}&orderBy=name", base, max))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(JiraListProjectsTool) }
}

// ─── jira_search_issues ──────────────────────────────────────────────────────

pub struct JiraSearchIssuesTool;
impl Tool for JiraSearchIssuesTool {
    fn name(&self) -> &'static str { "jira_search_issues" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let base = base_url()?;
        let tok = token()?;
        let em = email()?;
        let c = client(&em, &tok)?;
        let jql = input["jql"].as_str().ok_or_else(|| anyhow::anyhow!("missing jql (e.g. 'project = ENG AND status = Open')"))?;
        let max = input["max_results"].as_u64().unwrap_or(25).min(100);
        let fields = input["fields"].as_str().unwrap_or("summary,status,assignee,priority,created,updated,description");
        post(&c, &format!("{}/rest/api/3/search", base), json!({
            "jql": jql,
            "maxResults": max,
            "fields": fields.split(',').collect::<Vec<_>>()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(JiraSearchIssuesTool) }
}

// ─── jira_get_issue ──────────────────────────────────────────────────────────

pub struct JiraGetIssueTool;
impl Tool for JiraGetIssueTool {
    fn name(&self) -> &'static str { "jira_get_issue" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let base = base_url()?;
        let tok = token()?;
        let em = email()?;
        let c = client(&em, &tok)?;
        let key = input["key"].as_str().ok_or_else(|| anyhow::anyhow!("missing key (e.g. ENG-123)"))?;
        get(&c, &format!("{}/rest/api/3/issue/{}", base, key))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(JiraGetIssueTool) }
}

// ─── jira_create_issue ───────────────────────────────────────────────────────

pub struct JiraCreateIssueTool;
impl Tool for JiraCreateIssueTool {
    fn name(&self) -> &'static str { "jira_create_issue" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let base = base_url()?;
        let tok = token()?;
        let em = email()?;
        let c = client(&em, &tok)?;
        let project_key = input["project_key"].as_str().ok_or_else(|| anyhow::anyhow!("missing project_key"))?;
        let summary = input["summary"].as_str().ok_or_else(|| anyhow::anyhow!("missing summary"))?;
        let issue_type = input["issue_type"].as_str().unwrap_or("Task");
        let mut fields = json!({
            "project": { "key": project_key },
            "summary": summary,
            "issuetype": { "name": issue_type }
        });
        if let Some(desc) = input["description"].as_str() {
            fields["description"] = json!({
                "type": "doc", "version": 1,
                "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": desc }] }]
            });
        }
        if let Some(priority) = input["priority"].as_str() {
            fields["priority"] = json!({ "name": priority });
        }
        if let Some(assignee) = input["assignee_id"].as_str() {
            fields["assignee"] = json!({ "accountId": assignee });
        }
        post(&c, &format!("{}/rest/api/3/issue", base), json!({ "fields": fields }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(JiraCreateIssueTool) }
}

// ─── jira_update_issue ───────────────────────────────────────────────────────

pub struct JiraUpdateIssueTool;
impl Tool for JiraUpdateIssueTool {
    fn name(&self) -> &'static str { "jira_update_issue" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let base = base_url()?;
        let tok = token()?;
        let em = email()?;
        let c = client(&em, &tok)?;
        let key = input["key"].as_str().ok_or_else(|| anyhow::anyhow!("missing key"))?;
        let mut fields = json!({});
        if let Some(s) = input["summary"].as_str() { fields["summary"] = json!(s); }
        if let Some(p) = input["priority"].as_str() { fields["priority"] = json!({ "name": p }); }
        if let Some(a) = input["assignee_id"].as_str() { fields["assignee"] = json!({ "accountId": a }); }
        put(&c, &format!("{}/rest/api/3/issue/{}", base, key), json!({ "fields": fields }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(JiraUpdateIssueTool) }
}

// ─── jira_add_comment ────────────────────────────────────────────────────────

pub struct JiraAddCommentTool;
impl Tool for JiraAddCommentTool {
    fn name(&self) -> &'static str { "jira_add_comment" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let base = base_url()?;
        let tok = token()?;
        let em = email()?;
        let c = client(&em, &tok)?;
        let key = input["key"].as_str().ok_or_else(|| anyhow::anyhow!("missing key"))?;
        let text = input["body"].as_str().ok_or_else(|| anyhow::anyhow!("missing body"))?;
        let body = json!({
            "body": {
                "type": "doc", "version": 1,
                "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }]
            }
        });
        post(&c, &format!("{}/rest/api/3/issue/{}/comment", base, key), body)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(JiraAddCommentTool) }
}

// ─── jira_transition_issue ───────────────────────────────────────────────────

pub struct JiraTransitionIssueTool;
impl Tool for JiraTransitionIssueTool {
    fn name(&self) -> &'static str { "jira_transition_issue" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let base = base_url()?;
        let tok = token()?;
        let em = email()?;
        let c = client(&em, &tok)?;
        let key = input["key"].as_str().ok_or_else(|| anyhow::anyhow!("missing key"))?;
        let transition_id = input["transition_id"].as_str()
            .ok_or_else(|| anyhow::anyhow!("missing transition_id (use jira_get_transitions to list them)"))?;
        post(&c, &format!("{}/rest/api/3/issue/{}/transitions", base, key),
            json!({ "transition": { "id": transition_id } }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(JiraTransitionIssueTool) }
}

// ─── jira_get_transitions ────────────────────────────────────────────────────

pub struct JiraGetTransitionsTool;
impl Tool for JiraGetTransitionsTool {
    fn name(&self) -> &'static str { "jira_get_transitions" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let base = base_url()?;
        let tok = token()?;
        let em = email()?;
        let c = client(&em, &tok)?;
        let key = input["key"].as_str().ok_or_else(|| anyhow::anyhow!("missing key"))?;
        get(&c, &format!("{}/rest/api/3/issue/{}/transitions", base, key))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(JiraGetTransitionsTool) }
}

// ─── Factory functions ────────────────────────────────────────────────────────

pub fn create_jira_list_projects_tool() -> Box<dyn Tool> { Box::new(JiraListProjectsTool) }
pub fn create_jira_search_issues_tool() -> Box<dyn Tool> { Box::new(JiraSearchIssuesTool) }
pub fn create_jira_get_issue_tool() -> Box<dyn Tool> { Box::new(JiraGetIssueTool) }
pub fn create_jira_create_issue_tool() -> Box<dyn Tool> { Box::new(JiraCreateIssueTool) }
pub fn create_jira_update_issue_tool() -> Box<dyn Tool> { Box::new(JiraUpdateIssueTool) }
pub fn create_jira_add_comment_tool() -> Box<dyn Tool> { Box::new(JiraAddCommentTool) }
pub fn create_jira_transition_issue_tool() -> Box<dyn Tool> { Box::new(JiraTransitionIssueTool) }
pub fn create_jira_get_transitions_tool() -> Box<dyn Tool> { Box::new(JiraGetTransitionsTool) }
