use crate::tools::common::{Tool, ToolContext};
use serde_json::{json, Value};

const LINEAR_API: &str = "https://api.linear.app/graphql";

fn token() -> anyhow::Result<String> {
    std::env::var("LINEAR_API_KEY")
        .map_err(|_| anyhow::anyhow!("LINEAR_API_KEY not set — add it to your environment"))
}

fn client(token: &str) -> anyhow::Result<reqwest::blocking::Client> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(token)
            .map_err(|e| anyhow::anyhow!("invalid token: {}", e))?,
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {}", e))
}

fn graphql(client: &reqwest::blocking::Client, query: &str, variables: Value) -> anyhow::Result<Value> {
    let resp = client
        .post(LINEAR_API)
        .json(&json!({ "query": query, "variables": variables }))
        .send()
        .map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let body: Value = resp.json().unwrap_or(json!(null));
    if let Some(errors) = body.get("errors") {
        return Err(anyhow::anyhow!("GraphQL errors: {}", errors));
    }
    Ok(json!({ "status": status, "data": body["data"] }))
}

// ─── linear_list_teams ───────────────────────────────────────────────────────

pub struct LinearListTeamsTool;
impl Tool for LinearListTeamsTool {
    fn name(&self) -> &'static str { "linear_list_teams" }
    fn invoke(&self, _ctx: &ToolContext, _input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        graphql(&c, r#"query { teams { nodes { id name key description } } }"#, json!({}))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(LinearListTeamsTool) }
}

// ─── linear_list_issues ──────────────────────────────────────────────────────

pub struct LinearListIssuesTool;
impl Tool for LinearListIssuesTool {
    fn name(&self) -> &'static str { "linear_list_issues" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let first = input["first"].as_u64().unwrap_or(25).min(100);
        let mut filter = json!({});
        if let Some(team_id) = input["team_id"].as_str() {
            filter["team"] = json!({ "id": { "eq": team_id } });
        }
        if let Some(state) = input["state"].as_str() {
            filter["state"] = json!({ "name": { "eq": state } });
        }
        if let Some(assignee) = input["assignee_email"].as_str() {
            filter["assignee"] = json!({ "email": { "eq": assignee } });
        }
        let query = r#"
            query($first: Int, $filter: IssueFilter) {
                issues(first: $first, filter: $filter, orderBy: updatedAt) {
                    nodes {
                        id identifier title description state { name } priority
                        assignee { name email } team { name } createdAt updatedAt url
                    }
                }
            }
        "#;
        graphql(&c, query, json!({ "first": first, "filter": filter }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(LinearListIssuesTool) }
}

// ─── linear_get_issue ────────────────────────────────────────────────────────

pub struct LinearGetIssueTool;
impl Tool for LinearGetIssueTool {
    fn name(&self) -> &'static str { "linear_get_issue" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("missing id (Linear issue ID or identifier like ENG-123)"))?;
        let query = r#"
            query($id: String!) {
                issue(id: $id) {
                    id identifier title description state { name }
                    priority assignee { name email } team { name }
                    comments { nodes { id body user { name } createdAt } }
                    createdAt updatedAt url
                }
            }
        "#;
        graphql(&c, query, json!({ "id": id }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(LinearGetIssueTool) }
}

// ─── linear_create_issue ─────────────────────────────────────────────────────

pub struct LinearCreateIssueTool;
impl Tool for LinearCreateIssueTool {
    fn name(&self) -> &'static str { "linear_create_issue" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let team_id = input["team_id"].as_str().ok_or_else(|| anyhow::anyhow!("missing team_id"))?;
        let title = input["title"].as_str().ok_or_else(|| anyhow::anyhow!("missing title"))?;
        let mut vars = json!({ "teamId": team_id, "title": title });
        if let Some(d) = input["description"].as_str() { vars["description"] = json!(d); }
        if let Some(p) = input["priority"].as_u64() { vars["priority"] = json!(p); }
        if let Some(a) = input["assignee_id"].as_str() { vars["assigneeId"] = json!(a); }
        if let Some(s) = input["state_id"].as_str() { vars["stateId"] = json!(s); }
        let query = r#"
            mutation($teamId: String!, $title: String!, $description: String, $priority: Int, $assigneeId: String, $stateId: String) {
                issueCreate(input: { teamId: $teamId, title: $title, description: $description, priority: $priority, assigneeId: $assigneeId, stateId: $stateId }) {
                    success issue { id identifier title url }
                }
            }
        "#;
        graphql(&c, query, vars)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(LinearCreateIssueTool) }
}

// ─── linear_update_issue ─────────────────────────────────────────────────────

pub struct LinearUpdateIssueTool;
impl Tool for LinearUpdateIssueTool {
    fn name(&self) -> &'static str { "linear_update_issue" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("missing id"))?;
        let mut update = json!({});
        if let Some(t) = input["title"].as_str() { update["title"] = json!(t); }
        if let Some(d) = input["description"].as_str() { update["description"] = json!(d); }
        if let Some(p) = input["priority"].as_u64() { update["priority"] = json!(p); }
        if let Some(s) = input["state_id"].as_str() { update["stateId"] = json!(s); }
        if let Some(a) = input["assignee_id"].as_str() { update["assigneeId"] = json!(a); }
        let query = r#"
            mutation($id: String!, $update: IssueUpdateInput!) {
                issueUpdate(id: $id, input: $update) {
                    success issue { id identifier title state { name } url }
                }
            }
        "#;
        graphql(&c, query, json!({ "id": id, "update": update }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(LinearUpdateIssueTool) }
}

// ─── linear_add_comment ──────────────────────────────────────────────────────

pub struct LinearAddCommentTool;
impl Tool for LinearAddCommentTool {
    fn name(&self) -> &'static str { "linear_add_comment" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let issue_id = input["issue_id"].as_str().ok_or_else(|| anyhow::anyhow!("missing issue_id"))?;
        let body = input["body"].as_str().ok_or_else(|| anyhow::anyhow!("missing body"))?;
        let query = r#"
            mutation($issueId: String!, $body: String!) {
                commentCreate(input: { issueId: $issueId, body: $body }) {
                    success comment { id body createdAt }
                }
            }
        "#;
        graphql(&c, query, json!({ "issueId": issue_id, "body": body }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(LinearAddCommentTool) }
}

// ─── linear_list_projects ────────────────────────────────────────────────────

pub struct LinearListProjectsTool;
impl Tool for LinearListProjectsTool {
    fn name(&self) -> &'static str { "linear_list_projects" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let first = input["first"].as_u64().unwrap_or(25).min(100);
        let query = r#"
            query($first: Int) {
                projects(first: $first) {
                    nodes { id name description state progress startDate targetDate teams { nodes { name } } }
                }
            }
        "#;
        graphql(&c, query, json!({ "first": first }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(LinearListProjectsTool) }
}

// ─── linear_list_workflow_states ─────────────────────────────────────────────

pub struct LinearListWorkflowStatesTool;
impl Tool for LinearListWorkflowStatesTool {
    fn name(&self) -> &'static str { "linear_list_workflow_states" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let mut filter = json!({});
        if let Some(team_id) = input["team_id"].as_str() {
            filter["team"] = json!({ "id": { "eq": team_id } });
        }
        let query = r#"
            query($filter: WorkflowStateFilter) {
                workflowStates(filter: $filter) {
                    nodes { id name type color team { name } }
                }
            }
        "#;
        graphql(&c, query, json!({ "filter": filter }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(LinearListWorkflowStatesTool) }
}

// ─── Factory functions ────────────────────────────────────────────────────────

pub fn create_linear_list_teams_tool() -> Box<dyn Tool> { Box::new(LinearListTeamsTool) }
pub fn create_linear_list_issues_tool() -> Box<dyn Tool> { Box::new(LinearListIssuesTool) }
pub fn create_linear_get_issue_tool() -> Box<dyn Tool> { Box::new(LinearGetIssueTool) }
pub fn create_linear_create_issue_tool() -> Box<dyn Tool> { Box::new(LinearCreateIssueTool) }
pub fn create_linear_update_issue_tool() -> Box<dyn Tool> { Box::new(LinearUpdateIssueTool) }
pub fn create_linear_add_comment_tool() -> Box<dyn Tool> { Box::new(LinearAddCommentTool) }
pub fn create_linear_list_projects_tool() -> Box<dyn Tool> { Box::new(LinearListProjectsTool) }
pub fn create_linear_list_workflow_states_tool() -> Box<dyn Tool> { Box::new(LinearListWorkflowStatesTool) }
