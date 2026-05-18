use crate::tools::common::{Tool, ToolContext};
use serde_json::{json, Value};

const GITHUB_API: &str = "https://api.github.com";

fn token() -> anyhow::Result<String> {
    std::env::var("GITHUB_TOKEN")
        .map_err(|_| anyhow::anyhow!("GITHUB_TOKEN not set — add it to your environment"))
}

fn client(token: &str) -> anyhow::Result<reqwest::blocking::Client> {
    use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", token))
            .map_err(|e| anyhow::anyhow!("invalid token: {}", e))?,
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github.v3+json"),
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("MowisAI-agentd/1.0"));
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
    Ok(json!({ "status": status, "data": body }))
}

fn post(client: &reqwest::blocking::Client, url: &str, body: Value) -> anyhow::Result<Value> {
    let resp = client.post(url).json(&body).send()
        .map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let data: Value = resp.json().unwrap_or(json!(null));
    Ok(json!({ "status": status, "data": data }))
}

fn patch(client: &reqwest::blocking::Client, url: &str, body: Value) -> anyhow::Result<Value> {
    let resp = client.patch(url).json(&body).send()
        .map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let data: Value = resp.json().unwrap_or(json!(null));
    Ok(json!({ "status": status, "data": data }))
}

fn put(client: &reqwest::blocking::Client, url: &str, body: Value) -> anyhow::Result<Value> {
    let resp = client.put(url).json(&body).send()
        .map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let data: Value = resp.json().unwrap_or(json!(null));
    Ok(json!({ "status": status, "data": data }))
}

// ─── github_list_repos ───────────────────────────────────────────────────────

pub struct GithubListReposTool;
impl Tool for GithubListReposTool {
    fn name(&self) -> &'static str { "github_list_repos" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let owner = input["owner"].as_str()
            .ok_or_else(|| anyhow::anyhow!("github_list_repos: missing owner"))?;
        let kind = input["type"].as_str().unwrap_or("all");
        let per_page = input["per_page"].as_u64().unwrap_or(30).min(100);
        let url = format!("{}/users/{}/repos?type={}&per_page={}&sort=updated", GITHUB_API, owner, kind, per_page);
        get(&c, &url)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubListReposTool) }
}

// ─── github_get_repo ─────────────────────────────────────────────────────────

pub struct GithubGetRepoTool;
impl Tool for GithubGetRepoTool {
    fn name(&self) -> &'static str { "github_get_repo" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let owner = input["owner"].as_str().ok_or_else(|| anyhow::anyhow!("missing owner"))?;
        let repo = input["repo"].as_str().ok_or_else(|| anyhow::anyhow!("missing repo"))?;
        get(&c, &format!("{}/repos/{}/{}", GITHUB_API, owner, repo))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubGetRepoTool) }
}

// ─── github_list_issues ──────────────────────────────────────────────────────

pub struct GithubListIssuesTool;
impl Tool for GithubListIssuesTool {
    fn name(&self) -> &'static str { "github_list_issues" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let owner = input["owner"].as_str().ok_or_else(|| anyhow::anyhow!("missing owner"))?;
        let repo = input["repo"].as_str().ok_or_else(|| anyhow::anyhow!("missing repo"))?;
        let state = input["state"].as_str().unwrap_or("open");
        let per_page = input["per_page"].as_u64().unwrap_or(30).min(100);
        let mut url = format!("{}/repos/{}/{}/issues?state={}&per_page={}", GITHUB_API, owner, repo, state, per_page);
        if let Some(labels) = input["labels"].as_str() {
            url.push_str(&format!("&labels={}", labels));
        }
        if let Some(assignee) = input["assignee"].as_str() {
            url.push_str(&format!("&assignee={}", assignee));
        }
        get(&c, &url)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubListIssuesTool) }
}

// ─── github_get_issue ────────────────────────────────────────────────────────

pub struct GithubGetIssueTool;
impl Tool for GithubGetIssueTool {
    fn name(&self) -> &'static str { "github_get_issue" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let owner = input["owner"].as_str().ok_or_else(|| anyhow::anyhow!("missing owner"))?;
        let repo = input["repo"].as_str().ok_or_else(|| anyhow::anyhow!("missing repo"))?;
        let number = input["number"].as_u64().ok_or_else(|| anyhow::anyhow!("missing number"))?;
        get(&c, &format!("{}/repos/{}/{}/issues/{}", GITHUB_API, owner, repo, number))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubGetIssueTool) }
}

// ─── github_create_issue ─────────────────────────────────────────────────────

pub struct GithubCreateIssueTool;
impl Tool for GithubCreateIssueTool {
    fn name(&self) -> &'static str { "github_create_issue" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let owner = input["owner"].as_str().ok_or_else(|| anyhow::anyhow!("missing owner"))?;
        let repo = input["repo"].as_str().ok_or_else(|| anyhow::anyhow!("missing repo"))?;
        let title = input["title"].as_str().ok_or_else(|| anyhow::anyhow!("missing title"))?;
        let mut body = json!({ "title": title });
        if let Some(b) = input["body"].as_str() { body["body"] = json!(b); }
        if let Some(l) = input.get("labels") { body["labels"] = l.clone(); }
        if let Some(a) = input.get("assignees") { body["assignees"] = a.clone(); }
        if let Some(m) = input["milestone"].as_u64() { body["milestone"] = json!(m); }
        post(&c, &format!("{}/repos/{}/{}/issues", GITHUB_API, owner, repo), body)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubCreateIssueTool) }
}

// ─── github_update_issue ─────────────────────────────────────────────────────

pub struct GithubUpdateIssueTool;
impl Tool for GithubUpdateIssueTool {
    fn name(&self) -> &'static str { "github_update_issue" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let owner = input["owner"].as_str().ok_or_else(|| anyhow::anyhow!("missing owner"))?;
        let repo = input["repo"].as_str().ok_or_else(|| anyhow::anyhow!("missing repo"))?;
        let number = input["number"].as_u64().ok_or_else(|| anyhow::anyhow!("missing number"))?;
        let mut body = json!({});
        if let Some(t) = input["title"].as_str() { body["title"] = json!(t); }
        if let Some(b) = input["body"].as_str() { body["body"] = json!(b); }
        if let Some(s) = input["state"].as_str() { body["state"] = json!(s); }
        if let Some(l) = input.get("labels") { body["labels"] = l.clone(); }
        if let Some(a) = input.get("assignees") { body["assignees"] = a.clone(); }
        patch(&c, &format!("{}/repos/{}/{}/issues/{}", GITHUB_API, owner, repo, number), body)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubUpdateIssueTool) }
}

// ─── github_add_issue_comment ────────────────────────────────────────────────

pub struct GithubAddIssueCommentTool;
impl Tool for GithubAddIssueCommentTool {
    fn name(&self) -> &'static str { "github_add_issue_comment" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let owner = input["owner"].as_str().ok_or_else(|| anyhow::anyhow!("missing owner"))?;
        let repo = input["repo"].as_str().ok_or_else(|| anyhow::anyhow!("missing repo"))?;
        let number = input["number"].as_u64().ok_or_else(|| anyhow::anyhow!("missing number"))?;
        let comment = input["body"].as_str().ok_or_else(|| anyhow::anyhow!("missing body"))?;
        post(&c, &format!("{}/repos/{}/{}/issues/{}/comments", GITHUB_API, owner, repo, number), json!({ "body": comment }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubAddIssueCommentTool) }
}

// ─── github_list_pull_requests ───────────────────────────────────────────────

pub struct GithubListPullRequestsTool;
impl Tool for GithubListPullRequestsTool {
    fn name(&self) -> &'static str { "github_list_pull_requests" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let owner = input["owner"].as_str().ok_or_else(|| anyhow::anyhow!("missing owner"))?;
        let repo = input["repo"].as_str().ok_or_else(|| anyhow::anyhow!("missing repo"))?;
        let state = input["state"].as_str().unwrap_or("open");
        let per_page = input["per_page"].as_u64().unwrap_or(30).min(100);
        get(&c, &format!("{}/repos/{}/{}/pulls?state={}&per_page={}", GITHUB_API, owner, repo, state, per_page))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubListPullRequestsTool) }
}

// ─── github_get_pull_request ─────────────────────────────────────────────────

pub struct GithubGetPullRequestTool;
impl Tool for GithubGetPullRequestTool {
    fn name(&self) -> &'static str { "github_get_pull_request" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let owner = input["owner"].as_str().ok_or_else(|| anyhow::anyhow!("missing owner"))?;
        let repo = input["repo"].as_str().ok_or_else(|| anyhow::anyhow!("missing repo"))?;
        let number = input["number"].as_u64().ok_or_else(|| anyhow::anyhow!("missing number"))?;
        get(&c, &format!("{}/repos/{}/{}/pulls/{}", GITHUB_API, owner, repo, number))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubGetPullRequestTool) }
}

// ─── github_create_pull_request ──────────────────────────────────────────────

pub struct GithubCreatePullRequestTool;
impl Tool for GithubCreatePullRequestTool {
    fn name(&self) -> &'static str { "github_create_pull_request" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let owner = input["owner"].as_str().ok_or_else(|| anyhow::anyhow!("missing owner"))?;
        let repo = input["repo"].as_str().ok_or_else(|| anyhow::anyhow!("missing repo"))?;
        let title = input["title"].as_str().ok_or_else(|| anyhow::anyhow!("missing title"))?;
        let head = input["head"].as_str().ok_or_else(|| anyhow::anyhow!("missing head branch"))?;
        let base = input["base"].as_str().unwrap_or("main");
        let mut body = json!({ "title": title, "head": head, "base": base });
        if let Some(b) = input["body"].as_str() { body["body"] = json!(b); }
        if let Some(d) = input["draft"].as_bool() { body["draft"] = json!(d); }
        post(&c, &format!("{}/repos/{}/{}/pulls", GITHUB_API, owner, repo), body)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubCreatePullRequestTool) }
}

// ─── github_merge_pull_request ───────────────────────────────────────────────

pub struct GithubMergePullRequestTool;
impl Tool for GithubMergePullRequestTool {
    fn name(&self) -> &'static str { "github_merge_pull_request" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let owner = input["owner"].as_str().ok_or_else(|| anyhow::anyhow!("missing owner"))?;
        let repo = input["repo"].as_str().ok_or_else(|| anyhow::anyhow!("missing repo"))?;
        let number = input["number"].as_u64().ok_or_else(|| anyhow::anyhow!("missing number"))?;
        let method = input["merge_method"].as_str().unwrap_or("merge");
        let body = json!({ "merge_method": method });
        put(&c, &format!("{}/repos/{}/{}/pulls/{}/merge", GITHUB_API, owner, repo, number), body)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubMergePullRequestTool) }
}

// ─── github_search_code ──────────────────────────────────────────────────────

pub struct GithubSearchCodeTool;
impl Tool for GithubSearchCodeTool {
    fn name(&self) -> &'static str { "github_search_code" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let q = input["query"].as_str().ok_or_else(|| anyhow::anyhow!("missing query"))?;
        let per_page = input["per_page"].as_u64().unwrap_or(20).min(100);
        let encoded = urlencoding::encode(q);
        get(&c, &format!("{}/search/code?q={}&per_page={}", GITHUB_API, encoded, per_page))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubSearchCodeTool) }
}

// ─── github_search_issues ────────────────────────────────────────────────────

pub struct GithubSearchIssuesTool;
impl Tool for GithubSearchIssuesTool {
    fn name(&self) -> &'static str { "github_search_issues" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let q = input["query"].as_str().ok_or_else(|| anyhow::anyhow!("missing query"))?;
        let per_page = input["per_page"].as_u64().unwrap_or(20).min(100);
        let encoded = urlencoding::encode(q);
        get(&c, &format!("{}/search/issues?q={}&per_page={}", GITHUB_API, encoded, per_page))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubSearchIssuesTool) }
}

// ─── github_get_file_contents ────────────────────────────────────────────────

pub struct GithubGetFileContentsTool;
impl Tool for GithubGetFileContentsTool {
    fn name(&self) -> &'static str { "github_get_file_contents" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let owner = input["owner"].as_str().ok_or_else(|| anyhow::anyhow!("missing owner"))?;
        let repo = input["repo"].as_str().ok_or_else(|| anyhow::anyhow!("missing repo"))?;
        let path = input["path"].as_str().ok_or_else(|| anyhow::anyhow!("missing path"))?;
        let mut url = format!("{}/repos/{}/{}/contents/{}", GITHUB_API, owner, repo, path);
        if let Some(r) = input["ref"].as_str() { url.push_str(&format!("?ref={}", r)); }
        let result = get(&c, &url)?;
        // Decode base64 content if present
        if let Some(content_b64) = result["data"]["content"].as_str() {
            let cleaned: String = content_b64.chars().filter(|c| !c.is_whitespace()).collect();
            if let Ok(decoded) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &cleaned) {
                if let Ok(text) = String::from_utf8(decoded) {
                    return Ok(json!({
                        "status": result["status"],
                        "data": result["data"],
                        "decoded_content": text
                    }));
                }
            }
        }
        Ok(result)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubGetFileContentsTool) }
}

// ─── github_list_workflow_runs ───────────────────────────────────────────────

pub struct GithubListWorkflowRunsTool;
impl Tool for GithubListWorkflowRunsTool {
    fn name(&self) -> &'static str { "github_list_workflow_runs" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let owner = input["owner"].as_str().ok_or_else(|| anyhow::anyhow!("missing owner"))?;
        let repo = input["repo"].as_str().ok_or_else(|| anyhow::anyhow!("missing repo"))?;
        let per_page = input["per_page"].as_u64().unwrap_or(10).min(100);
        let mut url = format!("{}/repos/{}/{}/actions/runs?per_page={}", GITHUB_API, owner, repo, per_page);
        if let Some(s) = input["status"].as_str() { url.push_str(&format!("&status={}", s)); }
        if let Some(b) = input["branch"].as_str() { url.push_str(&format!("&branch={}", b)); }
        get(&c, &url)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubListWorkflowRunsTool) }
}

// ─── github_get_commit ───────────────────────────────────────────────────────

pub struct GithubGetCommitTool;
impl Tool for GithubGetCommitTool {
    fn name(&self) -> &'static str { "github_get_commit" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let tok = token()?;
        let c = client(&tok)?;
        let owner = input["owner"].as_str().ok_or_else(|| anyhow::anyhow!("missing owner"))?;
        let repo = input["repo"].as_str().ok_or_else(|| anyhow::anyhow!("missing repo"))?;
        let sha = input["sha"].as_str().ok_or_else(|| anyhow::anyhow!("missing sha"))?;
        get(&c, &format!("{}/repos/{}/{}/commits/{}", GITHUB_API, owner, repo, sha))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(GithubGetCommitTool) }
}

// ─── Factory functions ────────────────────────────────────────────────────────

pub fn create_github_list_repos_tool() -> Box<dyn Tool> { Box::new(GithubListReposTool) }
pub fn create_github_get_repo_tool() -> Box<dyn Tool> { Box::new(GithubGetRepoTool) }
pub fn create_github_list_issues_tool() -> Box<dyn Tool> { Box::new(GithubListIssuesTool) }
pub fn create_github_get_issue_tool() -> Box<dyn Tool> { Box::new(GithubGetIssueTool) }
pub fn create_github_create_issue_tool() -> Box<dyn Tool> { Box::new(GithubCreateIssueTool) }
pub fn create_github_update_issue_tool() -> Box<dyn Tool> { Box::new(GithubUpdateIssueTool) }
pub fn create_github_add_issue_comment_tool() -> Box<dyn Tool> { Box::new(GithubAddIssueCommentTool) }
pub fn create_github_list_pull_requests_tool() -> Box<dyn Tool> { Box::new(GithubListPullRequestsTool) }
pub fn create_github_get_pull_request_tool() -> Box<dyn Tool> { Box::new(GithubGetPullRequestTool) }
pub fn create_github_create_pull_request_tool() -> Box<dyn Tool> { Box::new(GithubCreatePullRequestTool) }
pub fn create_github_merge_pull_request_tool() -> Box<dyn Tool> { Box::new(GithubMergePullRequestTool) }
pub fn create_github_search_code_tool() -> Box<dyn Tool> { Box::new(GithubSearchCodeTool) }
pub fn create_github_search_issues_tool() -> Box<dyn Tool> { Box::new(GithubSearchIssuesTool) }
pub fn create_github_get_file_contents_tool() -> Box<dyn Tool> { Box::new(GithubGetFileContentsTool) }
pub fn create_github_list_workflow_runs_tool() -> Box<dyn Tool> { Box::new(GithubListWorkflowRunsTool) }
pub fn create_github_get_commit_tool() -> Box<dyn Tool> { Box::new(GithubGetCommitTool) }
