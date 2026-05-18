use crate::tools::common::{Tool, ToolContext};
use serde_json::{json, Value};

const VERCEL_API: &str = "https://api.vercel.com";

fn token() -> anyhow::Result<String> {
    std::env::var("VERCEL_TOKEN")
        .map_err(|_| anyhow::anyhow!("VERCEL_TOKEN not set"))
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

fn team_param(input: &Value) -> String {
    input["team_id"].as_str()
        .map(|t| format!("?teamId={}", t))
        .unwrap_or_default()
}

fn get(c: &reqwest::blocking::Client, url: &str) -> anyhow::Result<Value> {
    let resp = c.get(url).send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let body: Value = resp.json().unwrap_or(json!(null));
    if status >= 400 {
        return Err(anyhow::anyhow!("Vercel API error {}: {}", status, body["error"]["message"].as_str().unwrap_or("")));
    }
    Ok(body)
}

fn post(c: &reqwest::blocking::Client, url: &str, body: Value) -> anyhow::Result<Value> {
    let resp = c.post(url).json(&body).send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let data: Value = resp.json().unwrap_or(json!(null));
    if status >= 400 {
        return Err(anyhow::anyhow!("Vercel API error {}: {}", status, data["error"]["message"].as_str().unwrap_or("")));
    }
    Ok(data)
}

// ─── vercel_list_projects ────────────────────────────────────────────────────

pub struct VercelListProjectsTool;
impl Tool for VercelListProjectsTool {
    fn name(&self) -> &'static str { "vercel_list_projects" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let limit = input["limit"].as_u64().unwrap_or(20).min(100);
        let team = team_param(&input);
        get(&c, &format!("{}/v9/projects{}{}limit={}", VERCEL_API, team,
            if team.is_empty() { "?" } else { "&" }, limit))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(VercelListProjectsTool) }
}

// ─── vercel_get_project ──────────────────────────────────────────────────────

pub struct VercelGetProjectTool;
impl Tool for VercelGetProjectTool {
    fn name(&self) -> &'static str { "vercel_get_project" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("missing id (project name or ID)"))?;
        let team = team_param(&input);
        get(&c, &format!("{}/v9/projects/{}{}", VERCEL_API, id, team))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(VercelGetProjectTool) }
}

// ─── vercel_list_deployments ─────────────────────────────────────────────────

pub struct VercelListDeploymentsTool;
impl Tool for VercelListDeploymentsTool {
    fn name(&self) -> &'static str { "vercel_list_deployments" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let limit = input["limit"].as_u64().unwrap_or(10).min(100);
        let team = team_param(&input);
        let sep = if team.is_empty() { "?" } else { "&" };
        let mut url = format!("{}/v6/deployments{}{}", VERCEL_API, team, sep);
        url.push_str(&format!("limit={}", limit));
        if let Some(project_id) = input["project_id"].as_str() {
            url.push_str(&format!("&projectId={}", project_id));
        }
        if let Some(state) = input["state"].as_str() {
            url.push_str(&format!("&state={}", state));
        }
        get(&c, &url)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(VercelListDeploymentsTool) }
}

// ─── vercel_get_deployment ───────────────────────────────────────────────────

pub struct VercelGetDeploymentTool;
impl Tool for VercelGetDeploymentTool {
    fn name(&self) -> &'static str { "vercel_get_deployment" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("missing id (deployment ID or URL)"))?;
        let team = team_param(&input);
        get(&c, &format!("{}/v13/deployments/{}{}", VERCEL_API, id, team))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(VercelGetDeploymentTool) }
}

// ─── vercel_get_deployment_logs ──────────────────────────────────────────────

pub struct VercelGetDeploymentLogsTool;
impl Tool for VercelGetDeploymentLogsTool {
    fn name(&self) -> &'static str { "vercel_get_deployment_logs" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("missing id"))?;
        let team = team_param(&input);
        get(&c, &format!("{}/v2/deployments/{}/events{}", VERCEL_API, id, team))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(VercelGetDeploymentLogsTool) }
}

// ─── vercel_cancel_deployment ────────────────────────────────────────────────

pub struct VercelCancelDeploymentTool;
impl Tool for VercelCancelDeploymentTool {
    fn name(&self) -> &'static str { "vercel_cancel_deployment" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("missing id"))?;
        let team = team_param(&input);
        let resp = c.patch(format!("{}/v12/deployments/{}/cancel{}", VERCEL_API, id, team))
            .send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
        let status = resp.status().as_u16();
        let data: Value = resp.json().unwrap_or(json!(null));
        if status >= 400 {
            return Err(anyhow::anyhow!("Vercel API error {}: {}", status, data));
        }
        Ok(data)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(VercelCancelDeploymentTool) }
}

// ─── vercel_list_domains ─────────────────────────────────────────────────────

pub struct VercelListDomainsTool;
impl Tool for VercelListDomainsTool {
    fn name(&self) -> &'static str { "vercel_list_domains" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let team = team_param(&input);
        get(&c, &format!("{}/v5/domains{}", VERCEL_API, team))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(VercelListDomainsTool) }
}

// ─── vercel_list_env_vars ────────────────────────────────────────────────────

pub struct VercelListEnvVarsTool;
impl Tool for VercelListEnvVarsTool {
    fn name(&self) -> &'static str { "vercel_list_env_vars" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let project = input["project_id"].as_str().ok_or_else(|| anyhow::anyhow!("missing project_id"))?;
        let team = team_param(&input);
        get(&c, &format!("{}/v9/projects/{}/env{}", VERCEL_API, project, team))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(VercelListEnvVarsTool) }
}

// ─── vercel_create_deployment ────────────────────────────────────────────────

pub struct VercelCreateDeploymentTool;
impl Tool for VercelCreateDeploymentTool {
    fn name(&self) -> &'static str { "vercel_create_deployment" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let project_name = input["project"].as_str().ok_or_else(|| anyhow::anyhow!("missing project"))?;
        let git_source = input.get("git_source").cloned().unwrap_or(json!(null));
        let team = team_param(&input);
        let sep = if team.is_empty() { "?" } else { "&" };
        let mut body = json!({ "name": project_name });
        if !git_source.is_null() { body["gitSource"] = git_source; }
        if let Some(env) = input.get("env") { body["env"] = env.clone(); }
        post(&c, &format!("{}/v13/deployments{}{}", VERCEL_API, team, sep), body)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(VercelCreateDeploymentTool) }
}

// ─── Factory functions ────────────────────────────────────────────────────────

pub fn create_vercel_list_projects_tool() -> Box<dyn Tool> { Box::new(VercelListProjectsTool) }
pub fn create_vercel_get_project_tool() -> Box<dyn Tool> { Box::new(VercelGetProjectTool) }
pub fn create_vercel_list_deployments_tool() -> Box<dyn Tool> { Box::new(VercelListDeploymentsTool) }
pub fn create_vercel_get_deployment_tool() -> Box<dyn Tool> { Box::new(VercelGetDeploymentTool) }
pub fn create_vercel_get_deployment_logs_tool() -> Box<dyn Tool> { Box::new(VercelGetDeploymentLogsTool) }
pub fn create_vercel_cancel_deployment_tool() -> Box<dyn Tool> { Box::new(VercelCancelDeploymentTool) }
pub fn create_vercel_list_domains_tool() -> Box<dyn Tool> { Box::new(VercelListDomainsTool) }
pub fn create_vercel_list_env_vars_tool() -> Box<dyn Tool> { Box::new(VercelListEnvVarsTool) }
pub fn create_vercel_create_deployment_tool() -> Box<dyn Tool> { Box::new(VercelCreateDeploymentTool) }
