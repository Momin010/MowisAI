use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Provider {
    VertexAi,
    Grok,
    Groq,
    Anthropic,
    OpenAi,
    Gemini,
    Mimo,
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::VertexAi => write!(f, "vertex_ai"),
            Provider::Grok => write!(f, "grok"),
            Provider::Groq => write!(f, "groq"),
            Provider::Anthropic => write!(f, "anthropic"),
            Provider::OpenAi => write!(f, "openai"),
            Provider::Gemini => write!(f, "gemini"),
            Provider::Mimo => write!(f, "mimo"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: Provider,
    pub model: String,
    pub vertex_project_id: Option<String>,
    pub api_key: Option<String>,
}

impl LlmConfig {
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct AgentConversation {
    messages: Vec<ConvMessage>,
}

#[derive(Debug, Clone)]
enum ConvMessage {
    User(String),
    AssistantText(String),
    AssistantToolCalls(Vec<ToolCall>),
    ToolResults(Vec<(String, String, serde_json::Value)>),
}

impl AgentConversation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_user(&mut self, text: String) {
        self.messages.push(ConvMessage::User(text));
    }

    pub fn push_assistant(&mut self, text: String) {
        self.messages.push(ConvMessage::AssistantText(text));
    }

    pub fn push_assistant_tool_calls(&mut self, calls: Vec<ToolCall>) {
        self.messages.push(ConvMessage::AssistantToolCalls(calls));
    }

    pub fn push_tool_results(&mut self, results: Vec<(ToolCall, serde_json::Value)>) {
        let normalized: Vec<(String, String, serde_json::Value)> = results
            .into_iter()
            .map(|(tc, result)| (tc.id, tc.name, result))
            .collect();
        self.messages.push(ConvMessage::ToolResults(normalized));
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn round_count(&self) -> usize {
        self.messages
            .iter()
            .filter(|m| matches!(m, ConvMessage::AssistantToolCalls(_)))
            .count()
    }
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

pub struct AgentRoundResult {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub usage: TokenUsage,
}

impl AgentRoundResult {
    pub fn is_final(&self) -> bool {
        self.tool_calls.is_empty()
    }
}

/// Shared HTTP client for all LLM API calls
static HTTP_CLIENT: once_cell::sync::Lazy<reqwest::Client> = once_cell::sync::Lazy::new(|| {
    reqwest::Client::builder()
        .pool_max_idle_per_host(10)
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client")
});

const MAX_LLM_RETRIES: u32 = 3;

fn is_retryable_error(err: &anyhow::Error) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("timeout")
        || msg.contains("connection reset")
        || msg.contains("connection refused")
        || msg.contains("broken pipe")
        || msg.contains("eof")
        || msg.contains("dns")
}

async fn with_retry<F, Fut, T>(operation_name: &str, max_retries: u32, f: F) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut last_err = None;
    for attempt in 0..=max_retries {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                let retryable = is_retryable_error(&e);
                if !retryable || attempt >= max_retries {
                    return Err(e);
                }
                let backoff_ms = 1000 * (1u64 << attempt);
                tracing::warn!(
                    "[{}] Attempt {}/{} failed: {}. Backing off {}ms",
                    operation_name,
                    attempt + 1,
                    max_retries + 1,
                    e,
                    backoff_ms
                );
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("{}: all retries exhausted", operation_name)))
}

pub async fn generate_text(
    llm_config: &LlmConfig,
    system_prompt: &str,
    user_message: &str,
    json_mode: bool,
    temperature: f64,
) -> Result<String> {
    generate_text_with_limit(llm_config, system_prompt, user_message, json_mode, temperature, 16384).await
}

pub async fn generate_text_with_limit(
    llm_config: &LlmConfig,
    system_prompt: &str,
    user_message: &str,
    json_mode: bool,
    temperature: f64,
    max_tokens: u32,
) -> Result<String> {
    let provider_name = format!("{}", llm_config.provider);
    with_retry(
        &format!("generate_text/{}", provider_name),
        MAX_LLM_RETRIES,
        || {
            generate_text_inner(
                llm_config,
                system_prompt,
                user_message,
                json_mode,
                temperature,
                max_tokens,
            )
        },
    )
    .await
}

async fn generate_text_inner(
    llm_config: &LlmConfig,
    system_prompt: &str,
    user_message: &str,
    json_mode: bool,
    temperature: f64,
    max_tokens: u32,
) -> Result<String> {
    match llm_config.provider {
        Provider::VertexAi | Provider::Gemini => {
            generate_text_gemini(llm_config, system_prompt, user_message, json_mode, temperature, max_tokens).await
        }
        Provider::OpenAi | Provider::Grok | Provider::Groq | Provider::Mimo => {
            generate_text_openai_compat(llm_config, system_prompt, user_message, json_mode, temperature, max_tokens).await
        }
        Provider::Anthropic => {
            generate_text_anthropic(llm_config, system_prompt, user_message, temperature, max_tokens).await
        }
    }
}

async fn generate_text_gemini(
    llm_config: &LlmConfig,
    system_prompt: &str,
    user_message: &str,
    json_mode: bool,
    temperature: f64,
    max_tokens: u32,
) -> Result<String> {
    let (url, auth_header) = gemini_url_and_auth(llm_config)?;
    let mut gen_config = serde_json::json!({
        "temperature": temperature,
        "maxOutputTokens": max_tokens,
    });
    if json_mode {
        gen_config
            .as_object_mut()
            .unwrap()
            .insert("responseMimeType".into(), serde_json::json!("application/json"));
    }
    let request_body = serde_json::json!({
        "contents": [{"role": "user", "parts": [{"text": user_message}]}],
        "systemInstruction": {"parts": [{"text": system_prompt}]},
        "generationConfig": gen_config
    });
    let client = &*HTTP_CLIENT;
    let mut req = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .timeout(std::time::Duration::from_secs(900));
    if let Some(header) = auth_header {
        if header.starts_with("goog:") {
            req = req.header("x-goog-api-key", &header[5..]);
        } else {
            req = req.header("Authorization", header);
        }
    }
    let response = req.send().await.context("Gemini request failed")?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Gemini API error ({}): {}", status, body));
    }
    let resp_json: serde_json::Value = response.json().await?;
    extract_gemini_text(&resp_json)
}

fn extract_gemini_text(resp: &serde_json::Value) -> Result<String> {
    resp.get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|parts| parts.as_array())
        .and_then(|arr| {
            let texts: Vec<&str> = arr
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join(""))
            }
        })
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("Gemini: unexpected response format"))
}

fn gemini_url_and_auth(llm_config: &LlmConfig) -> Result<(String, Option<String>)> {
    match llm_config.provider {
        Provider::VertexAi => {
            let project_id = llm_config
                .vertex_project_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Vertex AI requires project_id"))?;
            let token = crate::config::OrchConfig::load()
                .ok()
                .and_then(|_| {
                    std::process::Command::new("gcloud")
                        .args(["auth", "print-access-token"])
                        .output()
                        .ok()
                        .and_then(|o| {
                            if o.status.success() {
                                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
                            } else {
                                None
                            }
                        })
                });
            let model = if llm_config.model.is_empty() {
                "gemini-2.5-pro"
            } else {
                &llm_config.model
            };
            let url = format!(
                "https://us-central1-aiplatform.googleapis.com/v1/projects/{}/locations/us-central1/publishers/google/models/{}:generateContent",
                project_id, model
            );
            Ok((url, token.map(|t| format!("Bearer {}", t))))
        }
        Provider::Gemini => {
            let api_key = llm_config
                .api_key
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Gemini requires api_key"))?;
            let model = if llm_config.model.is_empty() {
                "gemini-2.5-pro"
            } else {
                &llm_config.model
            };
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
                model
            );
            Ok((url, Some(format!("goog:{}", api_key))))
        }
        _ => Err(anyhow::anyhow!("Not a Gemini provider")),
    }
}

async fn generate_text_openai_compat(
    llm_config: &LlmConfig,
    system_prompt: &str,
    user_message: &str,
    json_mode: bool,
    temperature: f64,
    max_tokens: u32,
) -> Result<String> {
    let (url, api_key) = openai_compat_url_and_key(llm_config)?;
    let mut body = serde_json::json!({
        "model": llm_config.model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_message}
        ],
        "temperature": temperature,
        "max_tokens": max_tokens,
    });
    if json_mode {
        body.as_object_mut().unwrap().insert(
            "response_format".into(),
            serde_json::json!({"type": "json_object"}),
        );
    }
    let client = &*HTTP_CLIENT;
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(900))
        .send()
        .await
        .context("OpenAI-compat request failed")?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("{} API error ({}): {}", llm_config.provider, status, text));
    }
    let resp_json: serde_json::Value = response.json().await?;
    resp_json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| anyhow::anyhow!("{}: unexpected response", llm_config.provider))
        .map(|s| s.to_string())
}

fn openai_compat_url_and_key(llm_config: &LlmConfig) -> Result<(String, String)> {
    let api_key = llm_config
        .api_key
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("No API key for {}", llm_config.provider))?;
    let url = match llm_config.provider {
        Provider::OpenAi => "https://api.openai.com/v1/chat/completions".to_string(),
        Provider::Grok => "https://api.x.ai/v1/chat/completions".to_string(),
        Provider::Groq => "https://api.groq.com/openai/v1/chat/completions".to_string(),
        Provider::Mimo => "https://token-plan-ams.xiaomimimo.com/v1/chat/completions".to_string(),
        _ => return Err(anyhow::anyhow!("Not an OpenAI-compat provider")),
    };
    Ok((url, api_key.to_string()))
}

async fn generate_text_anthropic(
    llm_config: &LlmConfig,
    system_prompt: &str,
    user_message: &str,
    temperature: f64,
    max_tokens: u32,
) -> Result<String> {
    let api_key = llm_config
        .api_key
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("No Anthropic API key"))?;
    let body = serde_json::json!({
        "model": llm_config.model,
        "system": system_prompt,
        "messages": [{"role": "user", "content": user_message}],
        "max_tokens": max_tokens,
        "temperature": temperature,
    });
    let client = &*HTTP_CLIENT;
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(900))
        .send()
        .await
        .context("Anthropic request failed")?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Anthropic API error ({}): {}", status, text));
    }
    let resp_json: serde_json::Value = response.json().await?;
    resp_json
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.iter().find(|b| b["type"].as_str() == Some("text")))
        .and_then(|b| b["text"].as_str())
        .ok_or_else(|| anyhow::anyhow!("Anthropic: unexpected response"))
        .map(|s| s.to_string())
}

pub async fn call_agent_round(
    llm_config: &LlmConfig,
    conversation: &AgentConversation,
    tool_schemas: &[serde_json::Value],
    system_prompt: &str,
) -> Result<AgentRoundResult> {
    let provider_name = format!("{}", llm_config.provider);
    with_retry(
        &format!("call_agent_round/{}", provider_name),
        MAX_LLM_RETRIES,
        || call_agent_round_inner(llm_config, conversation, tool_schemas, system_prompt),
    )
    .await
}

async fn call_agent_round_inner(
    llm_config: &LlmConfig,
    conversation: &AgentConversation,
    tool_schemas: &[serde_json::Value],
    system_prompt: &str,
) -> Result<AgentRoundResult> {
    match llm_config.provider {
        Provider::VertexAi | Provider::Gemini => {
            call_agent_round_gemini(llm_config, conversation, tool_schemas, system_prompt).await
        }
        Provider::OpenAi | Provider::Grok | Provider::Groq | Provider::Mimo => {
            call_agent_round_openai_compat(llm_config, conversation, tool_schemas, system_prompt).await
        }
        Provider::Anthropic => {
            call_agent_round_anthropic(llm_config, conversation, tool_schemas, system_prompt).await
        }
    }
}

async fn call_agent_round_gemini(
    llm_config: &LlmConfig,
    conversation: &AgentConversation,
    tool_schemas: &[serde_json::Value],
    system_prompt: &str,
) -> Result<AgentRoundResult> {
    let (url, auth_header) = gemini_url_and_auth(llm_config)?;
    let mut contents: Vec<serde_json::Value> = Vec::new();
    for msg in &conversation.messages {
        match msg {
            ConvMessage::User(text) => {
                contents.push(serde_json::json!({
                    "role": "user",
                    "parts": [{"text": text}]
                }));
            }
            ConvMessage::AssistantText(text) => {
                contents.push(serde_json::json!({
                    "role": "model",
                    "parts": [{"text": text}]
                }));
            }
            ConvMessage::AssistantToolCalls(calls) => {
                let parts: Vec<serde_json::Value> = calls
                    .iter()
                    .map(|tc| {
                        serde_json::json!({
                            "functionCall": {
                                "name": tc.name,
                                "args": tc.args
                            }
                        })
                    })
                    .collect();
                contents.push(serde_json::json!({"role": "model", "parts": parts}));
            }
            ConvMessage::ToolResults(results) => {
                let parts: Vec<serde_json::Value> = results
                    .iter()
                    .map(|(_, name, result)| {
                        serde_json::json!({
                            "functionResponse": {
                                "name": name,
                                "response": result
                            }
                        })
                    })
                    .collect();
                contents.push(serde_json::json!({"role": "user", "parts": parts}));
            }
        }
    }
    let gen_config = serde_json::json!({
        "temperature": 0.7,
        "maxOutputTokens": 65536u32,
    });
    let tools = serde_json::json!([{
        "functionDeclarations": tool_schemas
    }]);
    let request_body = serde_json::json!({
        "contents": contents,
        "systemInstruction": {"parts": [{"text": system_prompt}]},
        "generationConfig": gen_config,
        "tools": tools,
    });
    let client = &*HTTP_CLIENT;
    let mut req = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .timeout(std::time::Duration::from_secs(900));
    if let Some(header) = auth_header {
        if header.starts_with("goog:") {
            req = req.header("x-goog-api-key", &header[5..]);
        } else {
            req = req.header("Authorization", header);
        }
    }
    let response = req.send().await.context("Gemini agent round failed")?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Gemini API error ({}): {}", status, body));
    }
    let resp_json: serde_json::Value = response.json().await?;
    let text = extract_gemini_text(&resp_json).ok();
    let tool_calls = extract_gemini_tool_calls(&resp_json);
    let usage = TokenUsage {
        input_tokens: resp_json["usageMetadata"]["promptTokenCount"].as_u64().unwrap_or(0) as u32,
        output_tokens: resp_json["usageMetadata"]["candidatesTokenCount"].as_u64().unwrap_or(0) as u32,
    };
    Ok(AgentRoundResult { text, tool_calls, usage })
}

fn extract_gemini_tool_calls(resp: &serde_json::Value) -> Vec<ToolCall> {
    resp.get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|parts| parts.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    p.get("functionCall").map(|fc| ToolCall {
                        id: format!("gemini-{}", uuid_simple()),
                        name: fc.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
                        args: fc.get("args").cloned().unwrap_or(serde_json::json!({})),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}", t)
}

async fn call_agent_round_openai_compat(
    llm_config: &LlmConfig,
    conversation: &AgentConversation,
    tool_schemas: &[serde_json::Value],
    system_prompt: &str,
) -> Result<AgentRoundResult> {
    let (url, api_key) = openai_compat_url_and_key(llm_config)?;
    let mut messages: Vec<serde_json::Value> =
        vec![serde_json::json!({"role": "system", "content": system_prompt})];
    for msg in &conversation.messages {
        match msg {
            ConvMessage::User(text) => {
                messages.push(serde_json::json!({"role": "user", "content": text}));
            }
            ConvMessage::AssistantText(text) => {
                messages.push(serde_json::json!({"role": "assistant", "content": text}));
            }
            ConvMessage::AssistantToolCalls(calls) => {
                let tool_calls: Vec<serde_json::Value> = calls
                    .iter()
                    .map(|tc| {
                        serde_json::json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": serde_json::to_string(&tc.args).unwrap_or_default()
                            }
                        })
                    })
                    .collect();
                messages.push(serde_json::json!({
                    "role": "assistant",
                    "tool_calls": tool_calls
                }));
            }
            ConvMessage::ToolResults(results) => {
                for (call_id, name, result) in results {
                    messages.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "content": serde_json::to_string(result).unwrap_or_default()
                    }));
                }
            }
        }
    }
    let tools: Vec<serde_json::Value> = tool_schemas
        .iter()
        .map(|schema| {
            serde_json::json!({
                "type": "function",
                "function": schema
            })
        })
        .collect();
    let body = serde_json::json!({
        "model": llm_config.model,
        "messages": messages,
        "tools": tools,
        "temperature": 0.7,
        "max_tokens": 16384u32,
    });
    let client = &*HTTP_CLIENT;
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(900))
        .send()
        .await
        .context("OpenAI-compat agent round failed")?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("{} API error ({}): {}", llm_config.provider, status, text));
    }
    let resp_json: serde_json::Value = response.json().await?;
    let choice = resp_json.get("choices").and_then(|c| c.get(0));
    let text = choice
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string());
    let tool_calls = choice
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("tool_calls"))
        .and_then(|tc| tc.as_array())
        .map(|arr| {
            arr.iter()
                .map(|tc| ToolCall {
                    id: tc.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string(),
                    name: tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string(),
                    args: tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|a| a.as_str())
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or(serde_json::json!({})),
                })
                .collect()
        })
        .unwrap_or_default();
    let usage = TokenUsage {
        input_tokens: resp_json["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
        output_tokens: resp_json["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
    };
    Ok(AgentRoundResult { text, tool_calls, usage })
}

async fn call_agent_round_anthropic(
    llm_config: &LlmConfig,
    conversation: &AgentConversation,
    tool_schemas: &[serde_json::Value],
    system_prompt: &str,
) -> Result<AgentRoundResult> {
    let api_key = llm_config
        .api_key
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("No Anthropic API key"))?;
    let mut messages: Vec<serde_json::Value> = Vec::new();
    for msg in &conversation.messages {
        match msg {
            ConvMessage::User(text) => {
                messages.push(serde_json::json!({"role": "user", "content": text}));
            }
            ConvMessage::AssistantText(text) => {
                messages.push(serde_json::json!({"role": "assistant", "content": text}));
            }
            ConvMessage::AssistantToolCalls(calls) => {
                let content: Vec<serde_json::Value> = calls
                    .iter()
                    .map(|tc| {
                        serde_json::json!({
                            "type": "tool_use",
                            "id": tc.id,
                            "name": tc.name,
                            "input": tc.args
                        })
                    })
                    .collect();
                messages.push(serde_json::json!({"role": "assistant", "content": content}));
            }
            ConvMessage::ToolResults(results) => {
                let content: Vec<serde_json::Value> = results
                    .iter()
                    .map(|(call_id, _name, result)| {
                        serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": call_id,
                            "content": serde_json::to_string(result).unwrap_or_default()
                        })
                    })
                    .collect();
                messages.push(serde_json::json!({"role": "user", "content": content}));
            }
        }
    }
    let tools: Vec<serde_json::Value> = tool_schemas
        .iter()
        .map(|schema| {
            serde_json::json!({
                "name": schema.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                "description": schema.get("description").and_then(|d| d.as_str()).unwrap_or(""),
                "input_schema": schema.get("parameters").cloned().unwrap_or(serde_json::json!({}))
            })
        })
        .collect();
    let body = serde_json::json!({
        "model": llm_config.model,
        "system": system_prompt,
        "messages": messages,
        "tools": tools,
        "max_tokens": 16384u32,
        "temperature": 0.7,
    });
    let client = &*HTTP_CLIENT;
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(900))
        .send()
        .await
        .context("Anthropic agent round failed")?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Anthropic API error ({}): {}", status, text));
    }
    let resp_json: serde_json::Value = response.json().await?;
    let content = resp_json.get("content").and_then(|c| c.as_array());
    let text = content
        .and_then(|arr| arr.iter().find(|b| b["type"].as_str() == Some("text")))
        .and_then(|b| b["text"].as_str())
        .map(|s| s.to_string());
    let tool_calls = content
        .map(|arr| {
            arr.iter()
                .filter(|b| b["type"].as_str() == Some("tool_use"))
                .map(|b| ToolCall {
                    id: b.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string(),
                    name: b.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
                    args: b.get("input").cloned().unwrap_or(serde_json::json!({})),
                })
                .collect()
        })
        .unwrap_or_default();
    let usage = TokenUsage {
        input_tokens: resp_json["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
        output_tokens: resp_json["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
    };
    Ok(AgentRoundResult { text, tool_calls, usage })
}

pub async fn generate_chat(
    llm_config: &LlmConfig,
    system_prompt: &str,
    history: &[serde_json::Value],
    temperature: f64,
) -> Result<(String, TokenUsage)> {
    let provider_name = format!("{}", llm_config.provider);
    with_retry(
        &format!("generate_chat/{}", provider_name),
        MAX_LLM_RETRIES,
        || generate_chat_inner(llm_config, system_prompt, history, temperature),
    )
    .await
}

async fn generate_chat_inner(
    llm_config: &LlmConfig,
    system_prompt: &str,
    history: &[serde_json::Value],
    temperature: f64,
) -> Result<(String, TokenUsage)> {
    match llm_config.provider {
        Provider::VertexAi | Provider::Gemini => {
            generate_chat_gemini(llm_config, system_prompt, history, temperature).await
        }
        Provider::OpenAi | Provider::Grok | Provider::Groq | Provider::Mimo => {
            generate_chat_openai_compat(llm_config, system_prompt, history, temperature).await
        }
        Provider::Anthropic => {
            generate_chat_anthropic(llm_config, system_prompt, history, temperature).await
        }
    }
}

async fn generate_chat_gemini(
    llm_config: &LlmConfig,
    system_prompt: &str,
    history: &[serde_json::Value],
    temperature: f64,
) -> Result<(String, TokenUsage)> {
    let (url, auth_header) = gemini_url_and_auth(llm_config)?;
    let contents: Vec<serde_json::Value> = history
        .iter()
        .map(|msg| {
            let role = msg["role"].as_str().unwrap_or("user");
            let gemini_role = if role == "assistant" { "model" } else { role };
            let content = msg["content"].as_str().unwrap_or("");
            serde_json::json!({"role": gemini_role, "parts": [{"text": content}]})
        })
        .collect();
    let gen_config = serde_json::json!({
        "temperature": temperature,
        "maxOutputTokens": 16384u32,
    });
    let request_body = serde_json::json!({
        "contents": contents,
        "systemInstruction": {"parts": [{"text": system_prompt}]},
        "generationConfig": gen_config
    });
    let client = &*HTTP_CLIENT;
    let mut req = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .timeout(std::time::Duration::from_secs(900));
    if let Some(header) = auth_header {
        if header.starts_with("goog:") {
            req = req.header("x-goog-api-key", &header[5..]);
        } else {
            req = req.header("Authorization", header);
        }
    }
    let response = req.send().await.context("Gemini chat request failed")?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Gemini API error ({}): {}", status, body));
    }
    let resp_json: serde_json::Value = response.json().await?;
    let text = extract_gemini_text(&resp_json)?;
    let usage = TokenUsage {
        input_tokens: resp_json["usageMetadata"]["promptTokenCount"].as_u64().unwrap_or(0) as u32,
        output_tokens: resp_json["usageMetadata"]["candidatesTokenCount"].as_u64().unwrap_or(0) as u32,
    };
    Ok((text, usage))
}

async fn generate_chat_openai_compat(
    llm_config: &LlmConfig,
    system_prompt: &str,
    history: &[serde_json::Value],
    temperature: f64,
) -> Result<(String, TokenUsage)> {
    let (url, api_key) = openai_compat_url_and_key(llm_config)?;
    let mut messages = vec![serde_json::json!({"role": "system", "content": system_prompt})];
    for msg in history {
        messages.push(msg.clone());
    }
    let body = serde_json::json!({
        "model": llm_config.model,
        "messages": messages,
        "temperature": temperature,
        "max_tokens": 16384u32
    });
    let client = &*HTTP_CLIENT;
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(900))
        .send()
        .await
        .context("OpenAI-compat chat request failed")?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("{} API error ({}): {}", llm_config.provider, status, text));
    }
    let resp_json: serde_json::Value = response.json().await?;
    let text = resp_json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| anyhow::anyhow!("{}: unexpected response in chat", llm_config.provider))
        .map(|s| s.to_string())?;
    let usage = TokenUsage {
        input_tokens: resp_json["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
        output_tokens: resp_json["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
    };
    Ok((text, usage))
}

async fn generate_chat_anthropic(
    llm_config: &LlmConfig,
    system_prompt: &str,
    history: &[serde_json::Value],
    temperature: f64,
) -> Result<(String, TokenUsage)> {
    let api_key = llm_config
        .api_key
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("No Anthropic API key"))?;
    let messages: Vec<serde_json::Value> = history
        .iter()
        .map(|msg| {
            let role = msg["role"].as_str().unwrap_or("user");
            let content = msg["content"].as_str().unwrap_or("");
            serde_json::json!({"role": role, "content": content})
        })
        .collect();
    let body = serde_json::json!({
        "model": llm_config.model,
        "system": system_prompt,
        "messages": messages,
        "max_tokens": 16384u32,
        "temperature": temperature
    });
    let client = &*HTTP_CLIENT;
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(900))
        .send()
        .await
        .context("Anthropic chat request failed")?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Anthropic API error ({}): {}", status, text));
    }
    let resp_json: serde_json::Value = response.json().await?;
    let text = resp_json
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.iter().find(|b| b["type"].as_str() == Some("text")))
        .and_then(|b| b["text"].as_str())
        .ok_or_else(|| anyhow::anyhow!("Anthropic: unexpected response in chat"))
        .map(|s| s.to_string())?;
    let usage = TokenUsage {
        input_tokens: resp_json["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
        output_tokens: resp_json["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
    };
    Ok((text, usage))
}

// ── Streaming ────────────────────────────────────────────────────────────────

use tokio::sync::mpsc;

/// Stream tokens from the LLM one by one via a channel.
/// Returns the full accumulated text and token usage when done.
pub async fn generate_chat_streaming(
    llm_config: &LlmConfig,
    system_prompt: &str,
    history: &[serde_json::Value],
    temperature: f64,
    token_tx: mpsc::UnboundedSender<String>,
) -> Result<(String, TokenUsage)> {
    match llm_config.provider {
        Provider::Anthropic => {
            generate_chat_anthropic_streaming(llm_config, system_prompt, history, temperature, token_tx).await
        }
        Provider::OpenAi | Provider::Grok | Provider::Groq | Provider::Mimo => {
            generate_chat_openai_streaming(llm_config, system_prompt, history, temperature, token_tx).await
        }
        Provider::VertexAi | Provider::Gemini => {
            // Gemini streaming is complex; fall back to non-streaming
            let (text, usage) = generate_chat(llm_config, system_prompt, history, temperature).await?;
            let _ = token_tx.send(text.clone());
            Ok((text, usage))
        }
    }
}

async fn generate_chat_anthropic_streaming(
    llm_config: &LlmConfig,
    system_prompt: &str,
    history: &[serde_json::Value],
    temperature: f64,
    token_tx: mpsc::UnboundedSender<String>,
) -> Result<(String, TokenUsage)> {
    let api_key = llm_config
        .api_key
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("No Anthropic API key"))?;

    let mut messages: Vec<serde_json::Value> = Vec::new();
    for msg in history {
        messages.push(msg.clone());
    }

    let body = serde_json::json!({
        "model": llm_config.model,
        "system": system_prompt,
        "messages": messages,
        "max_tokens": 16384u32,
        "temperature": temperature,
        "stream": true
    });

    let client = &*HTTP_CLIENT;
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(900))
        .send()
        .await
        .context("Anthropic streaming request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Anthropic API error ({}): {}", status, text));
    }

    let mut full_text = String::new();
    let mut stream_usage = TokenUsage::default();
    let mut stream = response.bytes_stream();
    use futures_util::StreamExt;
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("reading stream chunk")?;
        let chunk_str = String::from_utf8_lossy(&chunk);
        buffer.push_str(&chunk_str);

        // Process complete SSE lines
        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.starts_with("data: ") {
                let data = &line[6..];
                if data == "[DONE]" {
                    continue;
                }
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                    let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    // Capture input tokens from message_start
                    if event_type == "message_start" {
                        stream_usage.input_tokens = event["message"]["usage"]["input_tokens"]
                            .as_u64().unwrap_or(0) as u32;
                    }
                    // Capture output tokens from message_delta
                    if event_type == "message_delta" {
                        stream_usage.output_tokens = event["usage"]["output_tokens"]
                            .as_u64().unwrap_or(0) as u32;
                    }
                    // Anthropic SSE: content_block_delta with text delta
                    if let Some(delta) = event.get("delta") {
                        if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                full_text.push_str(text);
                                let _ = token_tx.send(text.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    Ok((full_text, stream_usage))
}

async fn generate_chat_openai_streaming(
    llm_config: &LlmConfig,
    system_prompt: &str,
    history: &[serde_json::Value],
    temperature: f64,
    token_tx: mpsc::UnboundedSender<String>,
) -> Result<(String, TokenUsage)> {
    let (url, api_key) = openai_compat_url_and_key(llm_config)?;

    let mut messages: Vec<serde_json::Value> =
        vec![serde_json::json!({"role": "system", "content": system_prompt})];
    for msg in history {
        messages.push(msg.clone());
    }

    let body = serde_json::json!({
        "model": llm_config.model,
        "messages": messages,
        "max_tokens": 16384u32,
        "temperature": temperature,
        "stream": true,
        "stream_options": {"include_usage": true},
    });

    let client = &*HTTP_CLIENT;
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(900))
        .send()
        .await
        .context("OpenAI streaming request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("{} API error ({}): {}", llm_config.provider, status, text));
    }

    let mut full_text = String::new();
    let mut stream_usage = TokenUsage::default();
    let mut stream = response.bytes_stream();
    use futures_util::StreamExt;
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("reading stream chunk")?;
        let chunk_str = String::from_utf8_lossy(&chunk);
        buffer.push_str(&chunk_str);

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.starts_with("data: ") {
                let data = &line[6..];
                if data == "[DONE]" {
                    continue;
                }
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                    // Capture usage from final usage chunk
                    if let Some(u) = event.get("usage") {
                        stream_usage.input_tokens = u["prompt_tokens"].as_u64().unwrap_or(0) as u32;
                        stream_usage.output_tokens = u["completion_tokens"].as_u64().unwrap_or(0) as u32;
                    }
                    // OpenAI SSE: choices[0].delta.content
                    if let Some(content) = event
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("delta"))
                        .and_then(|d| d.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        if !content.is_empty() {
                            full_text.push_str(content);
                            let _ = token_tx.send(content.to_string());
                        }
                    }
                }
            }
        }
    }

    Ok((full_text, stream_usage))
}

/// Streaming variant of `call_agent_round`: streams assistant text to `token_tx`
/// as it arrives while still collecting any tool calls. Lets the Conductor keep
/// token-by-token streaming AND call tools (e.g. `save_to_host`). Providers
/// without a streaming tool path fall back to a normal round, emitting the full
/// text once so the UI still updates.
pub async fn call_agent_round_streaming(
    llm_config: &LlmConfig,
    conversation: &AgentConversation,
    tool_schemas: &[serde_json::Value],
    system_prompt: &str,
    token_tx: mpsc::UnboundedSender<String>,
) -> Result<AgentRoundResult> {
    match llm_config.provider {
        Provider::OpenAi | Provider::Grok | Provider::Groq | Provider::Mimo => {
            call_agent_round_openai_compat_streaming(
                llm_config,
                conversation,
                tool_schemas,
                system_prompt,
                token_tx,
            )
            .await
        }
        _ => {
            let round = call_agent_round(llm_config, conversation, tool_schemas, system_prompt).await?;
            if let Some(t) = &round.text {
                let _ = token_tx.send(t.clone());
            }
            Ok(round)
        }
    }
}

async fn call_agent_round_openai_compat_streaming(
    llm_config: &LlmConfig,
    conversation: &AgentConversation,
    tool_schemas: &[serde_json::Value],
    system_prompt: &str,
    token_tx: mpsc::UnboundedSender<String>,
) -> Result<AgentRoundResult> {
    let (url, api_key) = openai_compat_url_and_key(llm_config)?;
    let mut messages: Vec<serde_json::Value> =
        vec![serde_json::json!({"role": "system", "content": system_prompt})];
    for msg in &conversation.messages {
        match msg {
            ConvMessage::User(text) => {
                messages.push(serde_json::json!({"role": "user", "content": text}));
            }
            ConvMessage::AssistantText(text) => {
                messages.push(serde_json::json!({"role": "assistant", "content": text}));
            }
            ConvMessage::AssistantToolCalls(calls) => {
                let tool_calls: Vec<serde_json::Value> = calls
                    .iter()
                    .map(|tc| {
                        serde_json::json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": serde_json::to_string(&tc.args).unwrap_or_default()
                            }
                        })
                    })
                    .collect();
                messages.push(serde_json::json!({"role": "assistant", "tool_calls": tool_calls}));
            }
            ConvMessage::ToolResults(results) => {
                for (call_id, _name, result) in results {
                    messages.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "content": serde_json::to_string(result).unwrap_or_default()
                    }));
                }
            }
        }
    }
    let tools: Vec<serde_json::Value> = tool_schemas
        .iter()
        .map(|schema| serde_json::json!({"type": "function", "function": schema}))
        .collect();
    let body = serde_json::json!({
        "model": llm_config.model,
        "messages": messages,
        "tools": tools,
        "temperature": 0.7,
        "max_tokens": 16384u32,
        "stream": true,
        "stream_options": {"include_usage": true},
    });

    let client = &*HTTP_CLIENT;
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(900))
        .send()
        .await
        .context("OpenAI-compat streaming round failed")?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("{} API error ({}): {}", llm_config.provider, status, text));
    }

    let mut full_text = String::new();
    // index -> (id, name, accumulated arguments JSON fragment)
    let mut tool_accum: std::collections::BTreeMap<i64, (String, String, String)> =
        std::collections::BTreeMap::new();
    let mut stream_usage = TokenUsage::default();
    let mut stream = response.bytes_stream();
    use futures_util::StreamExt;
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("reading stream chunk")?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    continue;
                }
                let event = match serde_json::from_str::<serde_json::Value>(data) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                // Capture usage from the final usage-only chunk
                if let Some(u) = event.get("usage") {
                    stream_usage.input_tokens = u["prompt_tokens"].as_u64().unwrap_or(0) as u32;
                    stream_usage.output_tokens = u["completion_tokens"].as_u64().unwrap_or(0) as u32;
                }
                let delta = event
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("delta"));
                let Some(delta) = delta else { continue };

                if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                    if !content.is_empty() {
                        full_text.push_str(content);
                        let _ = token_tx.send(content.to_string());
                    }
                }

                if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                    for tc in tcs {
                        let idx = tc.get("index").and_then(|i| i.as_i64()).unwrap_or(0);
                        let entry = tool_accum.entry(idx).or_default();
                        if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                            if !id.is_empty() {
                                entry.0 = id.to_string();
                            }
                        }
                        if let Some(name) = tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                        {
                            if !name.is_empty() {
                                entry.1 = name.to_string();
                            }
                        }
                        if let Some(args) = tc
                            .get("function")
                            .and_then(|f| f.get("arguments"))
                            .and_then(|a| a.as_str())
                        {
                            entry.2.push_str(args);
                        }
                    }
                }
            }
        }
    }

    let tool_calls: Vec<ToolCall> = tool_accum
        .into_iter()
        .map(|(_, (id, name, args))| ToolCall {
            id,
            name,
            args: serde_json::from_str(&args).unwrap_or_else(|_| serde_json::json!({})),
        })
        .filter(|tc| !tc.name.is_empty())
        .collect();

    let text = if full_text.is_empty() {
        None
    } else {
        Some(full_text)
    };
    Ok(AgentRoundResult { text, tool_calls, usage: stream_usage })
}
