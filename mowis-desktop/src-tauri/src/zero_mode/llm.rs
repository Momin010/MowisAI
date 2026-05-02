// zero_mode/llm.rs — Direct LLM provider clients
//
// Supports: Gemini (Google AI Studio), Anthropic, OpenAI, xAI Grok, Groq
// Each provider uses a different wire format for tool calling.
// Non-streaming requests are used; text is chunked artificially by the caller.
//
// call_llm() is the single entry point — dispatches based on config.provider.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::Config;

// ── Common message types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone)]
pub struct ToolCallRequest {
    /// Provider-issued id (for Anthropic/OpenAI round-trips).
    pub id: String,
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Clone)]
pub enum MessageContent {
    Text(String),
    ToolCall(ToolCallRequest),
    ToolResult { call_id: String, content: String },
}

#[derive(Debug, Clone)]
pub struct LlmMessage {
    pub role: Role,
    pub parts: Vec<MessageContent>,
}

impl LlmMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self { role: Role::User, parts: vec![MessageContent::Text(text.into())] }
    }
    pub fn system(text: impl Into<String>) -> Self {
        Self { role: Role::System, parts: vec![MessageContent::Text(text.into())] }
    }
    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self { role: Role::Assistant, parts: vec![MessageContent::Text(text.into())] }
    }
    pub fn assistant_calls(calls: Vec<ToolCallRequest>) -> Self {
        Self { role: Role::Assistant, parts: calls.into_iter().map(MessageContent::ToolCall).collect() }
    }
    pub fn tool_results(results: Vec<(String, String)>) -> Self {
        Self {
            role: Role::Tool,
            parts: results.into_iter().map(|(id, content)| MessageContent::ToolResult { call_id: id, content }).collect(),
        }
    }
}

/// Response from a single LLM call.
#[derive(Debug, Default)]
pub struct LlmResponse {
    /// Text content (may be empty when only tool calls are returned).
    pub text: String,
    /// Tool calls the model wants to execute.
    pub tool_calls: Vec<ToolCallRequest>,
    /// True when the model has finished (no more tool calls expected after returning text).
    pub finished: bool,
}

// ── Public entry point ────────────────────────────────────────────────────────

pub async fn call_llm(
    config: &Config,
    system_prompt: &str,
    messages: &[LlmMessage],
    tool_defs: &[Value],
) -> Result<LlmResponse> {
    match config.provider.as_str() {
        "gemini"    => call_gemini(config, system_prompt, messages, tool_defs).await,
        "vertex"    => call_vertex(config, system_prompt, messages, tool_defs).await,
        "anthropic" => call_anthropic(config, system_prompt, messages, tool_defs).await,
        "openai"    => call_openai_compat("https://api.openai.com/v1", config, system_prompt, messages, tool_defs).await,
        "grok"      => call_openai_compat("https://api.x.ai/v1", config, system_prompt, messages, tool_defs).await,
        "groq"      => call_openai_compat("https://api.groq.com/openai/v1", config, system_prompt, messages, tool_defs).await,
        other       => bail!("unsupported provider for zero mode: {other}"),
    }
}

// ── Gemini ────────────────────────────────────────────────────────────────────

async fn call_gemini(
    config: &Config,
    system_prompt: &str,
    messages: &[LlmMessage],
    tool_defs: &[Value],
) -> Result<LlmResponse> {
    // Try config first, then fall back to environment variable
    let key = if !config.api_key.is_empty() {
        config.api_key.clone()
    } else {
        std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("GOOGLE_API_KEY"))
            .unwrap_or_default()
    };
    
    if key.is_empty() {
        bail!("Gemini API key is not set — configure it in Settings or set GEMINI_API_KEY environment variable");
    }

    let model = if config.model.is_empty() { "gemini-2.0-flash" } else { config.model.as_str() };
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={key}"
    );

    // Build contents array (no system role in Gemini — it's a top-level field).
    let mut contents: Vec<Value> = Vec::new();
    for msg in messages {
        match msg.role {
            Role::System => {} // handled separately
            Role::User => {
                let text = msg.parts.iter().filter_map(|p| if let MessageContent::Text(t) = p { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("\n");
                contents.push(json!({ "role": "user", "parts": [{ "text": text }] }));
            }
            Role::Assistant => {
                let mut parts: Vec<Value> = Vec::new();
                for part in &msg.parts {
                    match part {
                        MessageContent::Text(t) => parts.push(json!({ "text": t })),
                        MessageContent::ToolCall(tc) => {
                            parts.push(json!({ "functionCall": { "name": tc.name, "args": tc.args } }));
                        }
                        _ => {}
                    }
                }
                contents.push(json!({ "role": "model", "parts": parts }));
            }
            Role::Tool => {
                let mut parts: Vec<Value> = Vec::new();
                for part in &msg.parts {
                    if let MessageContent::ToolResult { content, .. } = part {
                        // Gemini expects the functionResponse part — name must match the call.
                        // We embed all results; name is extracted per-part from call_id in the caller.
                        parts.push(json!({
                            "functionResponse": {
                                "name": "tool_result",
                                "response": { "content": content }
                            }
                        }));
                    }
                }
                contents.push(json!({ "role": "user", "parts": parts }));
            }
        }
    }

    // Function declarations from tool_defs.
    let func_decls: Vec<Value> = tool_defs.iter().map(|t| {
        json!({
            "name": t["name"],
            "description": t["description"],
            "parameters": t["parameters"]
        })
    }).collect();

    let body = json!({
        "system_instruction": { "parts": [{ "text": system_prompt }] },
        "contents": contents,
        "tools": [{ "function_declarations": func_decls }],
        "generation_config": { "max_output_tokens": 8192, "temperature": 0.2 }
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Gemini HTTP request failed")?;

    let status = resp.status();
    let json: Value = resp.json().await.context("decode Gemini response")?;

    if !status.is_success() {
        let msg = json["error"]["message"].as_str().unwrap_or("unknown error");
        bail!("Gemini API error {status}: {msg}");
    }

    parse_gemini_response(&json)
}

// ── Vertex AI (Gemini on Vertex) ─────────────────────────────────────────────

async fn call_vertex(
    config: &Config,
    system_prompt: &str,
    messages: &[LlmMessage],
    tool_defs: &[Value],
) -> Result<LlmResponse> {
    // For Vertex, we authenticate via Application Default Credentials (gcloud, service account, metadata).
    // api_key is ignored.
    let project = config.gcp_project.trim();
    if project.is_empty() {
        bail!("Vertex AI requires a GCP Project — set it in Settings");
    }
    let region = config.gcp_region.trim();
    if region.is_empty() {
        bail!("Vertex AI requires a GCP Region — set it in Settings (e.g. us-central1)");
    }

    let model = if config.model.is_empty() { "gemini-2.0-flash" } else { config.model.as_str() };

    // Vertex endpoint (GenerateContent):
    // POST https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}/publishers/google/models/{model}:generateContent
    let url = format!(
        "https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}/publishers/google/models/{model}:generateContent"
    );

    // Build contents array (same structure as Gemini generateContent).
    let mut contents: Vec<Value> = Vec::new();
    for msg in messages {
        match msg.role {
            Role::System => {}
            Role::User => {
                let text = msg.parts.iter().filter_map(|p| if let MessageContent::Text(t) = p { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("\n");
                contents.push(json!({ "role": "user", "parts": [{ "text": text }] }));
            }
            Role::Assistant => {
                let mut parts: Vec<Value> = Vec::new();
                for part in &msg.parts {
                    match part {
                        MessageContent::Text(t) => parts.push(json!({ "text": t })),
                        MessageContent::ToolCall(tc) => {
                            parts.push(json!({ "functionCall": { "name": tc.name, "args": tc.args } }));
                        }
                        _ => {}
                    }
                }
                contents.push(json!({ "role": "model", "parts": parts }));
            }
            Role::Tool => {
                let mut parts: Vec<Value> = Vec::new();
                for part in &msg.parts {
                    if let MessageContent::ToolResult { content, .. } = part {
                        parts.push(json!({
                            "functionResponse": {
                                "name": "tool_result",
                                "response": { "content": content }
                            }
                        }));
                    }
                }
                contents.push(json!({ "role": "user", "parts": parts }));
            }
        }
    }

    let func_decls: Vec<Value> = tool_defs.iter().map(|t| {
        json!({
            "name": t["name"],
            "description": t["description"],
            "parameters": t["parameters"]
        })
    }).collect();

    let body = json!({
        "system_instruction": { "parts": [{ "text": system_prompt }] },
        "contents": contents,
        "tools": [{ "function_declarations": func_decls }],
        "generation_config": { "max_output_tokens": 8192, "temperature": 0.2 }
    });

    // Acquire bearer token
    let provider = gcp_auth::provider().await.context("initialize GCP auth provider")?;
    let token = provider
        .token(&["https://www.googleapis.com/auth/cloud-platform"])
        .await
        .context("get GCP access token")?;

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(token.as_str())
        .json(&body)
        .send()
        .await
        .context("Vertex AI HTTP request failed")?;

    let status = resp.status();
    let json: Value = resp.json().await.context("decode Vertex AI response")?;
    if !status.is_success() {
        let msg = json["error"]["message"].as_str().unwrap_or("unknown error");
        bail!("Vertex AI error {status}: {msg}");
    }

    // Vertex generateContent returns the same candidate structure we already parse for Gemini.
    parse_gemini_response(&json)
}

fn parse_gemini_response(json: &Value) -> Result<LlmResponse> {
    let candidate = json["candidates"].get(0).ok_or_else(|| anyhow::anyhow!("no candidates in Gemini response"))?;
    let finish    = candidate["finishReason"].as_str().unwrap_or("STOP");
    let parts     = candidate["content"]["parts"].as_array().cloned().unwrap_or_default();

    let mut text = String::new();
    let mut tool_calls: Vec<ToolCallRequest> = Vec::new();

    for (i, part) in parts.iter().enumerate() {
        if let Some(t) = part["text"].as_str() {
            text.push_str(t);
        }
        if let Some(fc) = part.get("functionCall") {
            let name = fc["name"].as_str().unwrap_or("").to_owned();
            let args = fc["args"].clone();
            tool_calls.push(ToolCallRequest {
                id: format!("gemini-{i}"),
                name,
                args,
            });
        }
    }

    Ok(LlmResponse {
        text,
        tool_calls,
        finished: finish == "STOP" || finish == "MAX_TOKENS",
    })
}

// ── Anthropic ─────────────────────────────────────────────────────────────────

async fn call_anthropic(
    config: &Config,
    system_prompt: &str,
    messages: &[LlmMessage],
    tool_defs: &[Value],
) -> Result<LlmResponse> {
    // Try config first, then fall back to environment variable
    let key = if !config.api_key.is_empty() {
        config.api_key.clone()
    } else {
        std::env::var("ANTHROPIC_API_KEY").unwrap_or_default()
    };
    
    if key.is_empty() {
        bail!("Anthropic API key is not set — configure it in Settings or set ANTHROPIC_API_KEY environment variable");
    }

    let model = if config.model.is_empty() { "claude-sonnet-4-6" } else { config.model.as_str() };

    // Build messages array (Anthropic uses alternating user/assistant).
    let mut anth_messages: Vec<Value> = Vec::new();

    for msg in messages {
        match msg.role {
            Role::System => {} // top-level system field
            Role::User => {
                let text = msg.parts.iter().filter_map(|p| if let MessageContent::Text(t) = p { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("\n");
                anth_messages.push(json!({ "role": "user", "content": text }));
            }
            Role::Assistant => {
                let mut content: Vec<Value> = Vec::new();
                for part in &msg.parts {
                    match part {
                        MessageContent::Text(t) if !t.is_empty() => {
                            content.push(json!({ "type": "text", "text": t }));
                        }
                        MessageContent::ToolCall(tc) => {
                            content.push(json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.name,
                                "input": tc.args
                            }));
                        }
                        _ => {}
                    }
                }
                if !content.is_empty() {
                    anth_messages.push(json!({ "role": "assistant", "content": content }));
                }
            }
            Role::Tool => {
                let mut content: Vec<Value> = Vec::new();
                for part in &msg.parts {
                    if let MessageContent::ToolResult { call_id, content: result } = part {
                        content.push(json!({
                            "type": "tool_result",
                            "tool_use_id": call_id,
                            "content": result
                        }));
                    }
                }
                if !content.is_empty() {
                    anth_messages.push(json!({ "role": "user", "content": content }));
                }
            }
        }
    }

    // Tool definitions for Anthropic.
    let tools: Vec<Value> = tool_defs.iter().map(|t| {
        json!({
            "name": t["name"],
            "description": t["description"],
            "input_schema": t["parameters"]
        })
    }).collect();

    let body = json!({
        "model": model,
        "system": system_prompt,
        "messages": anth_messages,
        "tools": tools,
        "max_tokens": 8096
    });

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", key.as_str())
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .context("Anthropic HTTP request failed")?;

    let status = resp.status();
    let json: Value = resp.json().await.context("decode Anthropic response")?;

    if !status.is_success() {
        let msg = json["error"]["message"].as_str().unwrap_or("unknown error");
        bail!("Anthropic API error {status}: {msg}");
    }

    parse_anthropic_response(&json)
}

fn parse_anthropic_response(json: &Value) -> Result<LlmResponse> {
    let stop_reason = json["stop_reason"].as_str().unwrap_or("end_turn");
    let content = json["content"].as_array().cloned().unwrap_or_default();

    let mut text = String::new();
    let mut tool_calls: Vec<ToolCallRequest> = Vec::new();

    for block in &content {
        match block["type"].as_str().unwrap_or("") {
            "text"     => { text.push_str(block["text"].as_str().unwrap_or("")); }
            "tool_use" => {
                tool_calls.push(ToolCallRequest {
                    id:   block["id"].as_str().unwrap_or("").to_owned(),
                    name: block["name"].as_str().unwrap_or("").to_owned(),
                    args: block["input"].clone(),
                });
            }
            _ => {}
        }
    }

    Ok(LlmResponse {
        text,
        tool_calls,
        finished: stop_reason == "end_turn" || stop_reason == "max_tokens",
    })
}

// ── OpenAI-compatible (OpenAI, Grok, Groq) ───────────────────────────────────

async fn call_openai_compat(
    base_url: &str,
    config: &Config,
    system_prompt: &str,
    messages: &[LlmMessage],
    tool_defs: &[Value],
) -> Result<LlmResponse> {
    // Try config first, then fall back to environment variable
    let key = if !config.api_key.is_empty() {
        config.api_key.clone()
    } else {
        // Fall back to environment variables based on provider
        let env_var = match config.provider.as_str() {
            "groq" => "GROQ_API_KEY",
            "grok" => "XAI_API_KEY",
            "openai" => "OPENAI_API_KEY",
            _ => "API_KEY",
        };
        std::env::var(env_var).unwrap_or_default()
    };
    
    if key.is_empty() {
        let env_hint = match config.provider.as_str() {
            "groq" => "GROQ_API_KEY",
            "grok" => "XAI_API_KEY",
            "openai" => "OPENAI_API_KEY",
            _ => "API_KEY",
        };
        bail!("API key is not set — configure it in Settings or set {env_hint} environment variable");
    }

    let default_model = match config.provider.as_str() {
        "grok" => "grok-3-mini",
        "groq" => "llama-3.3-70b-versatile",
        _      => "gpt-4o-mini",
    };
    let model = if config.model.is_empty() { default_model } else { config.model.as_str() };

    // Build messages array.
    let mut oai_messages: Vec<Value> = vec![
        json!({ "role": "system", "content": system_prompt })
    ];

    for msg in messages {
        match msg.role {
            Role::System => {} // already added above
            Role::User => {
                let text = msg.parts.iter().filter_map(|p| if let MessageContent::Text(t) = p { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("\n");
                oai_messages.push(json!({ "role": "user", "content": text }));
            }
            Role::Assistant => {
                let text: String = msg.parts.iter().filter_map(|p| if let MessageContent::Text(t) = p { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("");
                let tool_calls: Vec<Value> = msg.parts.iter().filter_map(|p| {
                    if let MessageContent::ToolCall(tc) = p {
                        Some(json!({
                            "id": tc.id,
                            "type": "function",
                            "function": { "name": tc.name, "arguments": tc.args.to_string() }
                        }))
                    } else { None }
                }).collect();

                let mut m = json!({ "role": "assistant", "content": if text.is_empty() { Value::Null } else { Value::String(text) } });
                if !tool_calls.is_empty() {
                    m["tool_calls"] = Value::Array(tool_calls);
                }
                oai_messages.push(m);
            }
            Role::Tool => {
                for part in &msg.parts {
                    if let MessageContent::ToolResult { call_id, content } = part {
                        oai_messages.push(json!({
                            "role": "tool",
                            "tool_call_id": call_id,
                            "content": content
                        }));
                    }
                }
            }
        }
    }

    // Tool definitions (OpenAI format).
    let tools: Vec<Value> = tool_defs.iter().map(|t| {
        json!({
            "type": "function",
            "function": {
                "name": t["name"],
                "description": t["description"],
                "parameters": t["parameters"]
            }
        })
    }).collect();

    let body = json!({
        "model": model,
        "messages": oai_messages,
        "tools": tools,
        "tool_choice": "auto",
        "max_tokens": 4096
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base_url}/chat/completions"))
        .bearer_auth(key.as_str())
        .json(&body)
        .send()
        .await
        .context("OpenAI-compat HTTP request failed")?;

    let status = resp.status();
    let json: Value = resp.json().await.context("decode OpenAI-compat response")?;

    if !status.is_success() {
        let msg = json["error"]["message"].as_str().unwrap_or("unknown error");
        bail!("API error {status}: {msg}");
    }

    parse_openai_response(&json)
}

fn parse_openai_response(json: &Value) -> Result<LlmResponse> {
    let choice      = json["choices"].get(0).ok_or_else(|| anyhow::anyhow!("no choices in response"))?;
    let finish      = choice["finish_reason"].as_str().unwrap_or("stop");
    let message     = &choice["message"];
    let text        = message["content"].as_str().unwrap_or("").to_owned();
    let raw_calls   = message["tool_calls"].as_array().cloned().unwrap_or_default();

    let tool_calls = raw_calls.iter().map(|tc| {
        let id   = tc["id"].as_str().unwrap_or("").to_owned();
        let name = tc["function"]["name"].as_str().unwrap_or("").to_owned();
        let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
        let args = serde_json::from_str(args_str).unwrap_or_else(|_| json!({}));
        ToolCallRequest { id, name, args }
    }).collect();

    Ok(LlmResponse {
        text,
        tool_calls,
        finished: finish == "stop" || finish == "length",
    })
}
