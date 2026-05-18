use crate::tools::common::{Tool, ToolContext};
use serde_json::{json, Value};

const DD_API: &str = "https://api.datadoghq.com/api";

fn creds() -> anyhow::Result<(String, String)> {
    let api_key = std::env::var("DATADOG_API_KEY")
        .map_err(|_| anyhow::anyhow!("DATADOG_API_KEY not set"))?;
    let app_key = std::env::var("DATADOG_APP_KEY")
        .map_err(|_| anyhow::anyhow!("DATADOG_APP_KEY not set"))?;
    Ok((api_key, app_key))
}

fn site() -> String {
    std::env::var("DATADOG_SITE").unwrap_or_else(|_| "datadoghq.com".to_string())
}

fn api_base() -> String {
    format!("https://api.{}/api", site())
}

fn client(api_key: &str, app_key: &str) -> anyhow::Result<reqwest::blocking::Client> {
    use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
    let mut headers = HeaderMap::new();
    headers.insert(
        "DD-API-KEY",
        HeaderValue::from_str(api_key).map_err(|e| anyhow::anyhow!("invalid api key: {}", e))?,
    );
    headers.insert(
        "DD-APPLICATION-KEY",
        HeaderValue::from_str(app_key).map_err(|e| anyhow::anyhow!("invalid app key: {}", e))?,
    );
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
        return Err(anyhow::anyhow!("Datadog API error {}: {:?}", status, body));
    }
    Ok(body)
}

fn post(c: &reqwest::blocking::Client, url: &str, body: Value) -> anyhow::Result<Value> {
    let resp = c.post(url).json(&body).send().map_err(|e| anyhow::anyhow!("request failed: {}", e))?;
    let status = resp.status().as_u16();
    let data: Value = resp.json().unwrap_or(json!(null));
    if status >= 400 {
        return Err(anyhow::anyhow!("Datadog API error {}: {:?}", status, data));
    }
    Ok(data)
}

// ─── datadog_list_monitors ───────────────────────────────────────────────────

pub struct DatadogListMonitorsTool;
impl Tool for DatadogListMonitorsTool {
    fn name(&self) -> &'static str { "datadog_list_monitors" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let (api_key, app_key) = creds()?;
        let c = client(&api_key, &app_key)?;
        let page_size = input["page_size"].as_u64().unwrap_or(25).min(1000);
        let mut url = format!("{}/v1/monitor?page_size={}", api_base(), page_size);
        if let Some(q) = input["query"].as_str() { url.push_str(&format!("&query={}", urlencoding::encode(q))); }
        if let Some(tags) = input["tags"].as_str() { url.push_str(&format!("&monitor_tags={}", urlencoding::encode(tags))); }
        get(&c, &url)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(DatadogListMonitorsTool) }
}

// ─── datadog_get_monitor ─────────────────────────────────────────────────────

pub struct DatadogGetMonitorTool;
impl Tool for DatadogGetMonitorTool {
    fn name(&self) -> &'static str { "datadog_get_monitor" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let (api_key, app_key) = creds()?;
        let c = client(&api_key, &app_key)?;
        let id = input["id"].as_u64().ok_or_else(|| anyhow::anyhow!("missing id (monitor ID)"))?;
        get(&c, &format!("{}/v1/monitor/{}", api_base(), id))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(DatadogGetMonitorTool) }
}

// ─── datadog_query_metrics ───────────────────────────────────────────────────

pub struct DatadogQueryMetricsTool;
impl Tool for DatadogQueryMetricsTool {
    fn name(&self) -> &'static str { "datadog_query_metrics" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let (api_key, app_key) = creds()?;
        let c = client(&api_key, &app_key)?;
        let query = input["query"].as_str().ok_or_else(|| anyhow::anyhow!("missing query (e.g. avg:system.cpu.user{{*}})"))?;
        let from = input["from"].as_i64().ok_or_else(|| anyhow::anyhow!("missing from (unix timestamp)"))?;
        let to = input["to"].as_i64().ok_or_else(|| anyhow::anyhow!("missing to (unix timestamp)"))?;
        get(&c, &format!("{}/v1/query?from={}&to={}&query={}", api_base(), from, to, urlencoding::encode(query)))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(DatadogQueryMetricsTool) }
}

// ─── datadog_list_dashboards ─────────────────────────────────────────────────

pub struct DatadogListDashboardsTool;
impl Tool for DatadogListDashboardsTool {
    fn name(&self) -> &'static str { "datadog_list_dashboards" }
    fn invoke(&self, _ctx: &ToolContext, _input: Value) -> anyhow::Result<Value> {
        let (api_key, app_key) = creds()?;
        let c = client(&api_key, &app_key)?;
        get(&c, &format!("{}/v1/dashboard", api_base()))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(DatadogListDashboardsTool) }
}

// ─── datadog_list_logs ───────────────────────────────────────────────────────

pub struct DatadogListLogsTool;
impl Tool for DatadogListLogsTool {
    fn name(&self) -> &'static str { "datadog_list_logs" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let (api_key, app_key) = creds()?;
        let c = client(&api_key, &app_key)?;
        let query = input["query"].as_str().unwrap_or("*");
        let from = input["from"].as_str().unwrap_or("now-1h");
        let to = input["to"].as_str().unwrap_or("now");
        let limit = input["limit"].as_u64().unwrap_or(25).min(1000);
        post(&c, &format!("{}/v2/logs/events/search", api_base()), json!({
            "filter": { "query": query, "from": from, "to": to },
            "page": { "limit": limit },
            "sort": "timestamp"
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(DatadogListLogsTool) }
}

// ─── datadog_list_events ─────────────────────────────────────────────────────

pub struct DatadogListEventsTool;
impl Tool for DatadogListEventsTool {
    fn name(&self) -> &'static str { "datadog_list_events" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let (api_key, app_key) = creds()?;
        let c = client(&api_key, &app_key)?;
        let start = input["start"].as_i64().ok_or_else(|| anyhow::anyhow!("missing start (unix timestamp)"))?;
        let end = input["end"].as_i64().ok_or_else(|| anyhow::anyhow!("missing end (unix timestamp)"))?;
        let mut url = format!("{}/v1/events?start={}&end={}", api_base(), start, end);
        if let Some(tags) = input["tags"].as_str() { url.push_str(&format!("&tags={}", urlencoding::encode(tags))); }
        if let Some(priority) = input["priority"].as_str() { url.push_str(&format!("&priority={}", priority)); }
        get(&c, &url)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(DatadogListEventsTool) }
}

// ─── datadog_mute_monitor ────────────────────────────────────────────────────

pub struct DatadogMuteMonitorTool;
impl Tool for DatadogMuteMonitorTool {
    fn name(&self) -> &'static str { "datadog_mute_monitor" }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let (api_key, app_key) = creds()?;
        let c = client(&api_key, &app_key)?;
        let id = input["id"].as_u64().ok_or_else(|| anyhow::anyhow!("missing id"))?;
        let mut body = json!({});
        if let Some(end) = input["end"].as_i64() { body["end"] = json!(end); }
        post(&c, &format!("{}/v1/monitor/{}/mute", api_base(), id), body)
    }
    fn clone_box(&self) -> Box<dyn Tool> { Box::new(DatadogMuteMonitorTool) }
}

// ─── Factory functions ────────────────────────────────────────────────────────

pub fn create_datadog_list_monitors_tool() -> Box<dyn Tool> { Box::new(DatadogListMonitorsTool) }
pub fn create_datadog_get_monitor_tool() -> Box<dyn Tool> { Box::new(DatadogGetMonitorTool) }
pub fn create_datadog_query_metrics_tool() -> Box<dyn Tool> { Box::new(DatadogQueryMetricsTool) }
pub fn create_datadog_list_dashboards_tool() -> Box<dyn Tool> { Box::new(DatadogListDashboardsTool) }
pub fn create_datadog_list_logs_tool() -> Box<dyn Tool> { Box::new(DatadogListLogsTool) }
pub fn create_datadog_list_events_tool() -> Box<dyn Tool> { Box::new(DatadogListEventsTool) }
pub fn create_datadog_mute_monitor_tool() -> Box<dyn Tool> { Box::new(DatadogMuteMonitorTool) }
