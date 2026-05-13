//! Provider-agnostic LLM client for orchestration.
//!
//! Provides a unified interface over all supported AI providers:
//! Vertex AI, Gemini (standalone API key), OpenAI, Grok, Groq, Anthropic.
//!
//! Two entry points:
//! - `generate_text` — text/JSON completion (planner, merge reviewer, verification)
//! - `call_agent_round` — one round of the tool-calling loop (agent execution)

use crate::config::AiProvider;
use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use serde_json::{json, Value};

/// Shared HTTP client for all LLM API calls (reuses connections and TLS context)
static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .pool_max_idle_per_host(10)
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client")
});

/// Atomic counter for generating unique tool call IDs
static TOOL_CALL_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// ── LlmConfig ────────────────────────────────────────────────────────────────

/// Provider credentials + model selection for a single orchestration run.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: AiProvider,
    pub model: String,
    pub vertex_project_id: Option<String>,
    pub api_key: Option<String>,
}

impl LlmConfig {
    /// Build from the user's saved `MowisConfig`.
    pub fn from_config(config: &crate::config::MowisConfig) -> Result<Self> {
        match config.provider {
            AiProvider::VertexAi => Ok(Self {
                provider: AiProvider::VertexAi,
                model: if config.model.is_empty() {
                    "gemini-2.5-pro".into()
                } else {
                    config.model.clone()
                },
                vertex_project_id: Some(config.gcp_project_id.clone()),
                api_key: None,
            }),
            AiProvider::Gemini => {
                let api_key = config.gemini_api_key()?;
                Ok(Self {
                    provider: AiProvider::Gemini,
                    model: if config.gemini_model.is_empty() {
                        "gemini-2.5-pro".into()
                    } else {
                        config.gemini_model.clone()
                    },
                    vertex_project_id: None,
                    api_key: Some(api_key),
                })
            }
            AiProvider::OpenAi => {
                let api_key = config.openai_api_key()?;
                Ok(Self {
                    provider: AiProvider::OpenAi,
                    model: config.openai_model.clone(),
                    vertex_project_id: None,
                    api_key: Some(api_key),
                })
            }
            AiProvider::Grok => {
                let api_key = config.grok_api_key()?;
                Ok(Self {
                    provider: AiProvider::Grok,
                    model: config.grok_model.clone(),
                    vertex_project_id: None,
                    api_key: Some(api_key),
                })
            }
            AiProvider::Groq => {
                let api_key = config.groq_api_key()?;
                Ok(Self {
                    provider: AiProvider::Groq,
                    model: config.groq_model.clone(),
                    vertex_project_id: None,
                    api_key: Some(api_key),
                })
            }
            AiProvider::Anthropic => {
                let api_key = config.anthropic_api_key()?;
                Ok(Self {
                    provider: AiProvider::Anthropic,
                    model: config.anthropic_model.clone(),
                    vertex_project_id: None,
                    api_key: Some(api_key),
                })
            }
            AiProvider::Mimo => {
                let api_key = config.mimo_api_key()?;
                Ok(Self {
                    provider: AiProvider::Mimo,
                    model: if config.mimo_model.is_empty() {
                        "mimo-v2.5-pro".into()
                    } else {
                        config.mimo_model.clone()
                    },
                    vertex_project_id: None,
                    api_key: Some(api_key),
                })
            }
        }
    }

    /// Return a clone of this config with a different model ID.
    /// Useful for per-role routing (planning vs. execution model).
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Convenience constructor for Vertex AI from a project ID (CLI compat).
    pub fn vertex(project_id: impl Into<String>) -> Self {
        Self {
            provider: AiProvider::VertexAi,
            model: "gemini-2.5-pro".into(),
            vertex_project_id: Some(project_id.into()),
            api_key: None,
        }
    }
}

// ── Normalized Conversation ───────────────────────────────────────────────────

/// A single tool call returned by the assistant.
#[derive(Debug, Clone)]
pub struct ToolCall {
    /// Provider-specific call ID (used for OpenAI `tool_call_id` / Anthropic `tool_use_id`).
    pub id: String,
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Clone)]
enum ConvMessage {
    User(String),
    AssistantText(String),
    AssistantToolCalls(Vec<ToolCall>),
    /// (call_id, tool_name, result_value) — one entry per tool call in the round.
    ToolResults(Vec<(String, String, Value)>),
}

/// Provider-agnostic conversation history for the agent tool-calling loop.
#[derive(Debug, Clone, Default)]
pub struct AgentConversation {
    messages: Vec<ConvMessage>,
}

impl AgentConversation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_user(&mut self, text: String) {
        self.messages.push(ConvMessage::User(text));
    }

    /// Append all tool results from one round as a single conversation turn.
    pub fn push_tool_results(&mut self, results: Vec<(ToolCall, Value)>) {
        let normalized: Vec<(String, String, Value)> = results
            .into_iter()
            .map(|(tc, result)| (tc.id, tc.name, result))
            .collect();
        self.messages.push(ConvMessage::ToolResults(normalized));
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

/// Output from a single agent round.
pub struct AgentRoundResult {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
}

// ── Retry with Exponential Backoff ──────────────────────────────────────────

/// Maximum number of retries for transient LLM API errors
const MAX_LLM_RETRIES: u32 = 3;

/// Check if an HTTP status code is retryable (429 rate limit, 5xx server error)
fn is_retryable_status(status: u16) -> bool {
    status == 429 || status == 500 || status == 502 || status == 503 || status == 504
}

/// Check if an error is retryable (network errors, timeouts)
fn is_retryable_error(err: &anyhow::Error) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("timeout")
        || msg.contains("connection reset")
        || msg.contains("connection refused")
        || msg.contains("broken pipe")
        || msg.contains("eof")
        || msg.contains("dns")
}

/// Execute an async operation with exponential backoff retry for transient errors.
///
/// Retries on:
/// - HTTP 429 (rate limit) with Retry-After header support
/// - HTTP 5xx (server errors)
/// - Network errors (timeout, connection reset, etc.)
///
/// Does NOT retry on:
/// - HTTP 4xx (client errors like 401, 403, 404)
/// - Parse errors, auth errors, etc.
async fn with_retry<F, Fut, T>(operation_name: &str, max_retries: u32, f: F) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut last_err = None;

    for attempt in 0..=max_retries {
        match f().await {
            Ok(result) => {
                if attempt > 0 {
                    log::info!("[{}] Succeeded on attempt {}", operation_name, attempt + 1);
                }
                return Ok(result);
            }
            Err(e) => {
                let retryable = is_retryable_error(&e);

                if !retryable || attempt >= max_retries {
                    if attempt > 0 {
                        log::warn!(
                            "[{}] Failed after {} attempts: {}",
                            operation_name,
                            attempt + 1,
                            e
                        );
                    }
                    return Err(e);
                }

                // Exponential backoff: 1s, 2s, 4s, ...
                let backoff_ms = 1000 * (1u64 << attempt);
                log::warn!(
                    "[{}] Attempt {}/{} failed (retryable): {}. Backing off {}ms",
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

    Err(last_err.unwrap_or_else(|| anyhow!("{}: all retries exhausted", operation_name)))
}

// ── Text Generation ───────────────────────────────────────────────────────────

/// Multi-turn chat completion — takes full conversation history.
/// Each message in `history` has "role" ("user"/"assistant"/"system") and "content".
/// Used by the desktop Main LLM for context-aware conversation.
pub async fn generate_chat(
    llm_config: &LlmConfig,
    system_prompt: &str,
    history: &[Value],
    temperature: f64,
) -> Result<String> {
    let provider_name = format!("{:?}", llm_config.provider);
    with_retry(&format!("generate_chat/{}", provider_name), MAX_LLM_RETRIES, || {
        generate_chat_inner(llm_config, system_prompt, history, temperature)
    }).await
}

async fn generate_chat_inner(
    llm_config: &LlmConfig,
    system_prompt: &str,
    history: &[Value],
    temperature: f64,
) -> Result<String> {
    match llm_config.provider {
        AiProvider::VertexAi | AiProvider::Gemini => {
            generate_chat_gemini(llm_config, system_prompt, history, temperature).await
        }
        AiProvider::OpenAi | AiProvider::Grok | AiProvider::Groq | AiProvider::Mimo => {
            generate_chat_openai_compat(llm_config, system_prompt, history, temperature).await
        }
        AiProvider::Anthropic => {
            generate_chat_anthropic(llm_config, system_prompt, history, temperature).await
        }
    }
}

async fn generate_chat_gemini(
    llm_config: &LlmConfig,
    system_prompt: &str,
    history: &[Value],
    temperature: f64,
) -> Result<String> {
    let (url, auth_header) = gemini_url_and_auth(llm_config)?;

    let contents: Vec<Value> = history.iter().map(|msg| {
        let role = msg["role"].as_str().unwrap_or("user");
        let gemini_role = if role == "assistant" { "model" } else { role };
        let content = msg["content"].as_str().unwrap_or("");
        json!({"role": gemini_role, "parts": [{"text": content}]})
    }).collect();

    let mut gen_config = json!({
        "temperature": temperature,
        "maxOutputTokens": 16384_u32,
    });
    if super::VERTEX_THINKING_BUDGET_TOKENS > 0 {
        gen_config.as_object_mut().unwrap().insert(
            "thinkingConfig".into(),
            json!({ "thinkingBudget": super::VERTEX_THINKING_BUDGET_TOKENS }),
        );
    }

    let request_body = json!({
        "contents": contents,
        "systemInstruction": {"parts": [{"text": system_prompt}]},
        "generationConfig": gen_config
    });

    let client = &*HTTP_CLIENT;
    let mut req = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS));

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
        return Err(anyhow!("Gemini API error ({}): {}", status, body));
    }

    let resp_json: Value = response.json().await.context("parse Gemini chat response")?;
    extract_gemini_text(&resp_json)
}

async fn generate_chat_openai_compat(
    llm_config: &LlmConfig,
    system_prompt: &str,
    history: &[Value],
    temperature: f64,
) -> Result<String> {
    let url = openai_compat_url(llm_config);
    let api_key = llm_config.api_key.as_deref()
        .ok_or_else(|| anyhow!("No API key configured for {}", llm_config.provider))?;

    let mut messages = vec![json!({"role": "system", "content": system_prompt})];
    for msg in history {
        messages.push(msg.clone());
    }

    let body = json!({
        "model": llm_config.model,
        "messages": messages,
        "temperature": temperature,
        "max_tokens": 16384_u32
    });

    let client = &*HTTP_CLIENT;
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS))
        .send()
        .await
        .context("OpenAI-compat chat request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow!("{} API error ({}): {}", llm_config.provider, status, text));
    }

    let resp_json: Value = response.json().await.context("parse OpenAI-compat chat response")?;
    resp_json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| anyhow!("{}: unexpected response in chat", llm_config.provider))
        .map(|s| s.to_string())
}

async fn generate_chat_anthropic(
    llm_config: &LlmConfig,
    system_prompt: &str,
    history: &[Value],
    temperature: f64,
) -> Result<String> {
    let api_key = llm_config.api_key.as_deref()
        .ok_or_else(|| anyhow!("No Anthropic API key configured"))?;

    let messages: Vec<Value> = history.iter().map(|msg| {
        let role = msg["role"].as_str().unwrap_or("user");
        let content = msg["content"].as_str().unwrap_or("");
        json!({"role": role, "content": content})
    }).collect();

    let body = json!({
        "model": llm_config.model,
        "system": system_prompt,
        "messages": messages,
        "max_tokens": 16384_u32,
        "temperature": temperature
    });

    let client = &*HTTP_CLIENT;
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS))
        .send()
        .await
        .context("Anthropic chat request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow!("Anthropic API error ({}): {}", status, text));
    }

    let resp_json: Value = response.json().await.context("parse Anthropic chat response")?;
    resp_json
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.iter().find(|b| b["type"].as_str() == Some("text")))
        .and_then(|b| b["text"].as_str())
        .ok_or_else(|| anyhow!("Anthropic: unexpected response in chat"))
        .map(|s| s.to_string())
}

/// Generate a text (or JSON) completion — no tool calling.
///
/// Used by: planner, merge reviewer, verification planner, fix-task generator.
/// Includes automatic retry with exponential backoff for transient errors.
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
    // Wrap with retry for transient errors
    let provider_name = format!("{:?}", llm_config.provider);
    with_retry(&format!("generate_text/{}", provider_name), MAX_LLM_RETRIES, || {
        generate_text_with_limit_inner(llm_config, system_prompt, user_message, json_mode, temperature, max_tokens)
    }).await
}

async fn generate_text_with_limit_inner(
    llm_config: &LlmConfig,
    system_prompt: &str,
    user_message: &str,
    json_mode: bool,
    temperature: f64,
    max_tokens: u32,
) -> Result<String> {
    match llm_config.provider {
        AiProvider::VertexAi | AiProvider::Gemini => {
            generate_text_gemini(llm_config, system_prompt, user_message, json_mode, temperature, max_tokens)
                .await
        }
        AiProvider::OpenAi | AiProvider::Grok | AiProvider::Groq | AiProvider::Mimo => {
            generate_text_openai_compat(
                llm_config,
                system_prompt,
                user_message,
                json_mode,
                temperature,
                max_tokens,
            )
            .await
        }
        AiProvider::Anthropic => {
            generate_text_anthropic(llm_config, system_prompt, user_message, json_mode, temperature, max_tokens).await
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

    let mut gen_config = json!({
        "temperature": temperature,
        "maxOutputTokens": max_tokens,
    });
    if json_mode {
        gen_config
            .as_object_mut()
            .unwrap()
            .insert("responseMimeType".into(), json!("application/json"));
    }
    if super::VERTEX_THINKING_BUDGET_TOKENS > 0 {
        gen_config.as_object_mut().unwrap().insert(
            "thinkingConfig".into(),
            json!({ "thinkingBudget": super::VERTEX_THINKING_BUDGET_TOKENS }),
        );
    }

    let request_body = json!({
        "contents": [{"role": "user", "parts": [{"text": user_message}]}],
        "systemInstruction": {"parts": [{"text": system_prompt}]},
        "generationConfig": gen_config
    });

    let client = &*HTTP_CLIENT;
    let mut req = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS));

    if let Some(header) = auth_header {
        if header.starts_with("goog:") {
            // Gemini uses x-goog-api-key header, not Authorization
            req = req.header("x-goog-api-key", &header[5..]);
        } else {
            req = req.header("Authorization", header);
        }
    }

    let response = req
        .send()
        .await
        .context("Gemini generate_text request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("Gemini API error ({}): {}", status, body));
    }

    let resp_json: Value = response
        .json()
        .await
        .context("parse Gemini generate_text response")?;

    extract_gemini_text(&resp_json)
}

async fn generate_text_openai_compat(
    llm_config: &LlmConfig,
    system_prompt: &str,
    user_message: &str,
    json_mode: bool,
    temperature: f64,
    max_tokens: u32,
) -> Result<String> {
    let url = openai_compat_url(llm_config);
    let api_key = llm_config
        .api_key
        .as_deref()
        .ok_or_else(|| anyhow!("No API key configured for {}", llm_config.provider))?;

    let mut body = json!({
        "model": llm_config.model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_message}
        ],
        "temperature": temperature,
        "max_tokens": max_tokens
    });

    if json_mode {
        body.as_object_mut().unwrap().insert(
            "response_format".into(),
            json!({ "type": "json_object" }),
        );
    }

    let client = &*HTTP_CLIENT;
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS))
        .send()
        .await
        .context("OpenAI-compat generate_text request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "{} API error ({}): {}",
            llm_config.provider,
            status,
            text
        ));
    }

    let resp_json: Value = response
        .json()
        .await
        .context("parse OpenAI-compat generate_text response")?;

    resp_json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| {
            anyhow!(
                "{}: unexpected response structure in generate_text",
                llm_config.provider
            )
        })
        .map(|s| s.to_string())
}

async fn generate_text_anthropic(
    llm_config: &LlmConfig,
    system_prompt: &str,
    user_message: &str,
    json_mode: bool,
    temperature: f64,
    max_tokens: u32,
) -> Result<String> {
    let api_key = llm_config
        .api_key
        .as_deref()
        .ok_or_else(|| anyhow!("No Anthropic API key configured"))?;

    // Anthropic has no native json_mode — use assistant prefill with "{" to
    // force the model to begin a JSON object, guaranteeing parseable output.
    let messages = if json_mode {
        json!([
            {"role": "user", "content": user_message},
            {"role": "assistant", "content": "{"}
        ])
    } else {
        json!([{"role": "user", "content": user_message}])
    };

    let body = json!({
        "model": llm_config.model,
        "system": system_prompt,
        "messages": messages,
        "max_tokens": max_tokens,
        "temperature": temperature
    });

    let client = &*HTTP_CLIENT;
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS))
        .send()
        .await
        .context("Anthropic generate_text request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow!("Anthropic API error ({}): {}", status, text));
    }

    let resp_json: Value = response
        .json()
        .await
        .context("parse Anthropic generate_text response")?;

    let text = resp_json
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| {
            arr.iter().find(|b| {
                b.get("type").and_then(|t| t.as_str()) == Some("text")
            })
        })
        .and_then(|b| b.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("Anthropic: unexpected response structure in generate_text"))?;

    // When json_mode prefill was used the model continues from "{", so restore it.
    if json_mode {
        // Avoid double-brace if model already starts with {
        if text.starts_with('{') {
            Ok(text.to_string())
        } else {
            Ok(format!("{{{}", text))
        }
    } else {
        Ok(text.to_string())
    }
}

// ── Tool-calling loop ─────────────────────────────────────────────────────────

/// Execute one round of the agent tool-calling loop.
///
/// Appends the assistant's response turn to `conversation`. Returns the tool
/// calls to execute (empty vec = agent finished with a text reply). The caller
/// executes the tools, then calls `conversation.push_tool_results()` before
/// calling this function again for the next round.
pub async fn call_agent_round(
    llm_config: &LlmConfig,
    system_prompt: &str,
    conversation: &mut AgentConversation,
    allowed_tools: &[String],
    temperature: f64,
) -> Result<AgentRoundResult> {
    match llm_config.provider {
        AiProvider::VertexAi | AiProvider::Gemini => {
            call_agent_round_gemini(
                llm_config,
                system_prompt,
                conversation,
                allowed_tools,
                temperature,
            )
            .await
        }
        AiProvider::OpenAi | AiProvider::Grok | AiProvider::Groq | AiProvider::Mimo => {
            call_agent_round_openai_compat(
                llm_config,
                system_prompt,
                conversation,
                allowed_tools,
                temperature,
            )
            .await
        }
        AiProvider::Anthropic => {
            call_agent_round_anthropic(
                llm_config,
                system_prompt,
                conversation,
                allowed_tools,
                temperature,
            )
            .await
        }
    }
}

async fn call_agent_round_gemini(
    llm_config: &LlmConfig,
    system_prompt: &str,
    conversation: &mut AgentConversation,
    allowed_tools: &[String],
    temperature: f64,
) -> Result<AgentRoundResult> {
    let (url, auth_header) = gemini_url_and_auth(llm_config)?;

    let contents = conversation_to_gemini(conversation);
    let tool_decls = build_gemini_tool_declarations(allowed_tools);

    let mut gen_config = json!({
        "temperature": temperature,
        "maxOutputTokens": super::VERTEX_MAX_OUTPUT_TOKENS,
    });
    if super::VERTEX_THINKING_BUDGET_TOKENS > 0 {
        gen_config.as_object_mut().unwrap().insert(
            "thinkingConfig".into(),
            json!({ "thinkingBudget": super::VERTEX_THINKING_BUDGET_TOKENS }),
        );
    }

    let request_body = json!({
        "contents": contents,
        "systemInstruction": {"parts": [{"text": system_prompt}]},
        "tools": [{"functionDeclarations": tool_decls}],
        "generationConfig": gen_config
    });

    let client = &*HTTP_CLIENT;
    let mut req = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS));

    if let Some(header) = auth_header {
        if header.starts_with("goog:") {
            // Gemini uses x-goog-api-key header, not Authorization
            req = req.header("x-goog-api-key", &header[5..]);
        } else {
            req = req.header("Authorization", header);
        }
    }

    let response = req
        .send()
        .await
        .context("Gemini call_agent_round request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("Gemini API error ({}): {}", status, body));
    }

    let resp_json: Value = response
        .json()
        .await
        .context("parse Gemini call_agent_round response")?;

    let content = resp_json
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .ok_or_else(|| anyhow!("Gemini: no candidates in call_agent_round response"))?;

    let parts = content
        .get("parts")
        .and_then(|p| p.as_array())
        .ok_or_else(|| anyhow!("Gemini: no parts in call_agent_round response"))?;

    let mut text: Option<String> = None;
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    for part in parts {
        if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
            if !t.is_empty() {
                text = Some(t.to_string());
            }
        } else if let Some(fc) = part.get("functionCall") {
            let name = fc
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let args = fc.get("args").cloned().unwrap_or(json!({}));
            let seq = TOOL_CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let id = format!("call_{}_{}", name, seq);
            tool_calls.push(ToolCall { id, name, args });
        }
    }

    if tool_calls.is_empty() {
        if let Some(ref t) = text {
            conversation.messages.push(ConvMessage::AssistantText(t.clone()));
        }
    } else {
        conversation
            .messages
            .push(ConvMessage::AssistantToolCalls(tool_calls.clone()));
    }

    Ok(AgentRoundResult { text, tool_calls })
}

async fn call_agent_round_openai_compat(
    llm_config: &LlmConfig,
    system_prompt: &str,
    conversation: &mut AgentConversation,
    allowed_tools: &[String],
    temperature: f64,
) -> Result<AgentRoundResult> {
    let url = openai_compat_url(llm_config);
    let api_key = llm_config
        .api_key
        .as_deref()
        .ok_or_else(|| anyhow!("No API key for {}", llm_config.provider))?;

    let messages = conversation_to_openai(conversation, system_prompt);
    let tools = build_openai_tool_declarations(allowed_tools);

    let body = json!({
        "model": llm_config.model,
        "messages": messages,
        "tools": tools,
        "tool_choice": "auto",
        "temperature": temperature,
        "max_tokens": 16384
    });

    let client = &*HTTP_CLIENT;
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS))
        .send()
        .await
        .context("OpenAI-compat call_agent_round request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "{} API error ({}): {}",
            llm_config.provider,
            status,
            text
        ));
    }

    let resp_json: Value = response
        .json()
        .await
        .context("parse OpenAI-compat call_agent_round response")?;

    let message = resp_json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .ok_or_else(|| {
            anyhow!(
                "{}: no choices in call_agent_round response",
                llm_config.provider
            )
        })?;

    let text = message
        .get("content")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let tool_calls: Vec<ToolCall> = message
        .get("tool_calls")
        .and_then(|tc| tc.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|tc| {
                    let id = tc.get("id")?.as_str()?.to_string();
                    let func = tc.get("function")?;
                    let name = func.get("name")?.as_str()?.to_string();
                    let args_str = func.get("arguments")?.as_str().unwrap_or("{}");
                    let args = serde_json::from_str(args_str).unwrap_or(json!({}));
                    Some(ToolCall { id, name, args })
                })
                .collect()
        })
        .unwrap_or_default();

    if tool_calls.is_empty() {
        if let Some(ref t) = text {
            conversation
                .messages
                .push(ConvMessage::AssistantText(t.clone()));
        }
    } else {
        conversation
            .messages
            .push(ConvMessage::AssistantToolCalls(tool_calls.clone()));
    }

    Ok(AgentRoundResult { text, tool_calls })
}

async fn call_agent_round_anthropic(
    llm_config: &LlmConfig,
    system_prompt: &str,
    conversation: &mut AgentConversation,
    allowed_tools: &[String],
    temperature: f64,
) -> Result<AgentRoundResult> {
    let api_key = llm_config
        .api_key
        .as_deref()
        .ok_or_else(|| anyhow!("No Anthropic API key configured"))?;

    let messages = conversation_to_anthropic(conversation);
    let tools = build_anthropic_tool_declarations(allowed_tools);

    let body = json!({
        "model": llm_config.model,
        "system": system_prompt,
        "messages": messages,
        "tools": tools,
        "max_tokens": 8192,
        "temperature": temperature
    });

    let client = &*HTTP_CLIENT;
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS))
        .send()
        .await
        .context("Anthropic call_agent_round request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow!("Anthropic API error ({}): {}", status, text));
    }

    let resp_json: Value = response
        .json()
        .await
        .context("parse Anthropic call_agent_round response")?;

    let content_blocks = resp_json
        .get("content")
        .and_then(|c| c.as_array())
        .ok_or_else(|| anyhow!("Anthropic: no content in call_agent_round response"))?;

    let mut text: Option<String> = None;
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    for block in content_blocks {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                    if !t.is_empty() {
                        text = Some(t.to_string());
                    }
                }
            }
            Some("tool_use") => {
                let id = block
                    .get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = block
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = block.get("input").cloned().unwrap_or(json!({}));
                tool_calls.push(ToolCall { id, name, args });
            }
            _ => {}
        }
    }

    if tool_calls.is_empty() {
        if let Some(ref t) = text {
            conversation
                .messages
                .push(ConvMessage::AssistantText(t.clone()));
        }
    } else {
        conversation
            .messages
            .push(ConvMessage::AssistantToolCalls(tool_calls.clone()));
    }

    Ok(AgentRoundResult { text, tool_calls })
}

// ── Conversation Serializers ──────────────────────────────────────────────────

fn conversation_to_gemini(conv: &AgentConversation) -> Vec<Value> {
    let mut result = Vec::new();
    for msg in &conv.messages {
        match msg {
            ConvMessage::User(text) => {
                result.push(json!({"role": "user", "parts": [{"text": text}]}));
            }
            ConvMessage::AssistantText(text) => {
                result.push(json!({"role": "model", "parts": [{"text": text}]}));
            }
            ConvMessage::AssistantToolCalls(calls) => {
                let parts: Vec<Value> = calls
                    .iter()
                    .map(|tc| json!({"functionCall": {"name": tc.name, "args": tc.args}}))
                    .collect();
                result.push(json!({"role": "model", "parts": parts}));
            }
            ConvMessage::ToolResults(results) => {
                let parts: Vec<Value> = results
                    .iter()
                    .map(|(_, name, content)| {
                        json!({"functionResponse": {"name": name, "response": content}})
                    })
                    .collect();
                result.push(json!({"role": "function", "parts": parts}));
            }
        }
    }
    result
}

fn conversation_to_openai(conv: &AgentConversation, system_prompt: &str) -> Vec<Value> {
    let mut result = vec![json!({"role": "system", "content": system_prompt})];
    for msg in &conv.messages {
        match msg {
            ConvMessage::User(text) => {
                result.push(json!({"role": "user", "content": text}));
            }
            ConvMessage::AssistantText(text) => {
                result.push(json!({"role": "assistant", "content": text}));
            }
            ConvMessage::AssistantToolCalls(calls) => {
                let tool_calls: Vec<Value> = calls
                    .iter()
                    .map(|tc| {
                        json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": serde_json::to_string(&tc.args)
                                    .unwrap_or_else(|_| "{}".into())
                            }
                        })
                    })
                    .collect();
                result.push(
                    json!({"role": "assistant", "content": null, "tool_calls": tool_calls}),
                );
            }
            ConvMessage::ToolResults(results) => {
                for (call_id, name, content) in results {
                    result.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "name": name,
                        "content": serde_json::to_string(content)
                            .unwrap_or_else(|_| "{}".into())
                    }));
                }
            }
        }
    }
    result
}

fn conversation_to_anthropic(conv: &AgentConversation) -> Vec<Value> {
    let mut result = Vec::new();
    for msg in &conv.messages {
        match msg {
            ConvMessage::User(text) => {
                result.push(json!({"role": "user", "content": text}));
            }
            ConvMessage::AssistantText(text) => {
                result.push(json!({"role": "assistant", "content": text}));
            }
            ConvMessage::AssistantToolCalls(calls) => {
                let content: Vec<Value> = calls
                    .iter()
                    .map(|tc| {
                        json!({"type": "tool_use", "id": tc.id, "name": tc.name, "input": tc.args})
                    })
                    .collect();
                result.push(json!({"role": "assistant", "content": content}));
            }
            ConvMessage::ToolResults(results) => {
                let content: Vec<Value> = results
                    .iter()
                    .map(|(call_id, _, content)| {
                        json!({
                            "type": "tool_result",
                            "tool_use_id": call_id,
                            "content": serde_json::to_string(content)
                                .unwrap_or_else(|_| "{}".into())
                        })
                    })
                    .collect();
                // Anthropic tool results use the "user" role
                result.push(json!({"role": "user", "content": content}));
            }
        }
    }
    result
}

// ── Tool Declaration Builders ─────────────────────────────────────────────────

fn build_gemini_tool_declarations(allowed_tools: &[String]) -> Vec<Value> {
    filter_tools_from_registry(allowed_tools)
}

fn build_openai_tool_declarations(allowed_tools: &[String]) -> Vec<Value> {
    filter_tools_from_registry(allowed_tools)
        .into_iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                    "description": tool.get("description").and_then(|d| d.as_str()).unwrap_or(""),
                    "parameters": tool.get("parameters")
                        .cloned()
                        .unwrap_or(json!({"type": "object", "properties": {}}))
                }
            })
        })
        .collect()
}

fn build_anthropic_tool_declarations(allowed_tools: &[String]) -> Vec<Value> {
    filter_tools_from_registry(allowed_tools)
        .into_iter()
        .map(|tool| {
            json!({
                "name": tool.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                "description": tool.get("description").and_then(|d| d.as_str()).unwrap_or(""),
                "input_schema": tool.get("parameters")
                    .cloned()
                    .unwrap_or(json!({"type": "object", "properties": {}}))
            })
        })
        .collect()
}

fn filter_tools_from_registry(allowed_tools: &[String]) -> Vec<Value> {
    let all_tools = super::gemini_tool_declarations();
    let empty_vec = vec![];
    let all_tools_array = all_tools.as_array().unwrap_or(&empty_vec);
    all_tools_array
        .iter()
        .filter(|tool| {
            tool.get("name")
                .and_then(|n| n.as_str())
                .map(|name| allowed_tools.iter().any(|t| t == name))
                .unwrap_or(false)
        })
        .cloned()
        .collect()
}

// ── URL + Auth Helpers ────────────────────────────────────────────────────────

fn gemini_url_and_auth(llm_config: &LlmConfig) -> Result<(String, Option<String>)> {
    match llm_config.provider {
        AiProvider::VertexAi => {
            let project_id = llm_config
                .vertex_project_id
                .as_deref()
                .ok_or_else(|| anyhow!("Vertex AI: no GCP project ID configured"))?;
            let token = super::gcloud_access_token()?;
            let url = super::vertex_generate_url(project_id, &llm_config.model);
            Ok((url, Some(format!("Bearer {}", token))))
        }
        AiProvider::Gemini => {
            let api_key = llm_config
                .api_key
                .as_deref()
                .ok_or_else(|| anyhow!("Gemini: no API key configured"))?;
            // SECURITY: Use header instead of URL query parameter for API key
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
                llm_config.model
            );
            // Prefix with "goog:" to signal caller to use x-goog-api-key header
            Ok((url, Some(format!("goog:{}", api_key))))
        }
        _ => Err(anyhow!(
            "gemini_url_and_auth called for non-Gemini provider: {}",
            llm_config.provider
        )),
    }
}

fn openai_compat_url(llm_config: &LlmConfig) -> String {
    match llm_config.provider {
        AiProvider::OpenAi => "https://api.openai.com/v1/chat/completions".into(),
        AiProvider::Grok => "https://api.x.ai/v1/chat/completions".into(),
        AiProvider::Groq => "https://api.groq.com/openai/v1/chat/completions".into(),
        AiProvider::Mimo => "https://token-plan-ams.xiaomimimo.com/v1/chat/completions".into(),
        _ => "https://api.openai.com/v1/chat/completions".into(),
    }
}

// ── Response Parsing Helpers ──────────────────────────────────────────────────

fn extract_gemini_text(resp_json: &Value) -> Result<String> {
    resp_json
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("Gemini: unexpected response structure (missing candidates[0].content.parts[0].text)"))
        .map(|s| s.to_string())
}

// ── Streaming Agent Round ─────────────────────────────────────────────────────

/// Execute one round of the agent tool-calling loop with streaming text output.
///
/// Identical to `call_agent_round` except it calls `on_chunk` for each text delta
/// as the model streams it, enabling real-time display in the desktop UI.
/// Tool calls (which have no streaming text) are collected and returned normally.
pub async fn call_agent_round_streaming<F>(
    llm_config: &LlmConfig,
    system_prompt: &str,
    conversation: &mut AgentConversation,
    allowed_tools: &[String],
    temperature: f64,
    on_chunk: F,
) -> Result<AgentRoundResult>
where
    F: Fn(String) + Send + 'static,
{
    let boxed: Box<dyn Fn(String) + Send> = Box::new(on_chunk);
    match llm_config.provider {
        AiProvider::VertexAi | AiProvider::Gemini => {
            call_agent_round_gemini_streaming(
                llm_config, system_prompt, conversation, allowed_tools, temperature, boxed,
            ).await
        }
        AiProvider::OpenAi | AiProvider::Grok | AiProvider::Groq | AiProvider::Mimo => {
            call_agent_round_openai_streaming(
                llm_config, system_prompt, conversation, allowed_tools, temperature, boxed,
            ).await
        }
        AiProvider::Anthropic => {
            call_agent_round_anthropic_streaming(
                llm_config, system_prompt, conversation, allowed_tools, temperature, boxed,
            ).await
        }
    }
}

// ── SSE helper ────────────────────────────────────────────────────────────────

/// Extract the next complete line from buf, consuming it. Returns None when no
/// complete line (ending with \n) is present yet.
fn pop_line(buf: &mut String) -> Option<String> {
    buf.find('\n').map(|pos| {
        let line = buf[..pos].trim_end_matches('\r').to_string();
        *buf = buf[pos + 1..].to_string();
        line
    })
}

// ── Gemini/Vertex streaming ───────────────────────────────────────────────────

async fn call_agent_round_gemini_streaming(
    llm_config: &LlmConfig,
    system_prompt: &str,
    conversation: &mut AgentConversation,
    allowed_tools: &[String],
    temperature: f64,
    on_chunk: Box<dyn Fn(String) + Send>,
) -> Result<AgentRoundResult> {
    let (base_url, auth_header) = gemini_url_and_auth(llm_config)?;
    // Switch from generateContent to streamGenerateContent with SSE output
    let url = base_url
        .replace(":generateContent", ":streamGenerateContent")
        + "?alt=sse";

    let contents = conversation_to_gemini(conversation);
    let tool_decls = build_gemini_tool_declarations(allowed_tools);

    let mut gen_config = json!({
        "temperature": temperature,
        "maxOutputTokens": super::VERTEX_MAX_OUTPUT_TOKENS,
    });
    if super::VERTEX_THINKING_BUDGET_TOKENS > 0 {
        gen_config.as_object_mut().unwrap().insert(
            "thinkingConfig".into(),
            json!({ "thinkingBudget": super::VERTEX_THINKING_BUDGET_TOKENS }),
        );
    }

    let request_body = json!({
        "contents": contents,
        "systemInstruction": {"parts": [{"text": system_prompt}]},
        "tools": [{"functionDeclarations": tool_decls}],
        "generationConfig": gen_config
    });

    let client = &*HTTP_CLIENT;
    let mut req = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS));

    if let Some(header) = auth_header {
        if header.starts_with("goog:") {
            req = req.header("x-goog-api-key", &header[5..]);
        } else {
            req = req.header("Authorization", header);
        }
    }

    let mut response = req
        .send()
        .await
        .context("Gemini streaming request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("Gemini API error ({}): {}", status, body));
    }

    let mut buf = String::new();
    let mut full_text = String::new();
    let mut latest_tool_calls: Vec<ToolCall> = Vec::new();

    while let Some(chunk) = response.chunk().await.context("Gemini streaming read failed")? {
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(line) = pop_line(&mut buf) {
            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(val) = serde_json::from_str::<Value>(data) {
                    if let Some(parts) = val
                        .pointer("/candidates/0/content/parts")
                        .and_then(|p| p.as_array())
                    {
                        let mut round_tools: Vec<ToolCall> = Vec::new();
                        for part in parts {
                            if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                                if !t.is_empty() {
                                    on_chunk(t.to_string());
                                    full_text.push_str(t);
                                }
                            }
                            if let Some(fc) = part.get("functionCall") {
                                let name = fc
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let args = fc.get("args").cloned().unwrap_or(json!({}));
                                let seq = TOOL_CALL_COUNTER
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                let id = format!("call_{}_{}", name, seq);
                                round_tools.push(ToolCall { id, name, args });
                            }
                        }
                        if !round_tools.is_empty() {
                            latest_tool_calls = round_tools;
                        }
                    }
                }
            }
        }
    }

    let text = if full_text.is_empty() { None } else { Some(full_text) };
    if latest_tool_calls.is_empty() {
        if let Some(ref t) = text {
            conversation
                .messages
                .push(ConvMessage::AssistantText(t.clone()));
        }
    } else {
        conversation
            .messages
            .push(ConvMessage::AssistantToolCalls(latest_tool_calls.clone()));
    }
    Ok(AgentRoundResult { text, tool_calls: latest_tool_calls })
}

// ── OpenAI-compatible streaming ───────────────────────────────────────────────

async fn call_agent_round_openai_streaming(
    llm_config: &LlmConfig,
    system_prompt: &str,
    conversation: &mut AgentConversation,
    allowed_tools: &[String],
    temperature: f64,
    on_chunk: Box<dyn Fn(String) + Send>,
) -> Result<AgentRoundResult> {
    let url = openai_compat_url(llm_config);
    let api_key = llm_config
        .api_key
        .as_deref()
        .ok_or_else(|| anyhow!("No API key for {}", llm_config.provider))?;

    let messages = conversation_to_openai(conversation, system_prompt);
    let tools = build_openai_tool_declarations(allowed_tools);

    let body = json!({
        "model": llm_config.model,
        "messages": messages,
        "tools": tools,
        "tool_choice": "auto",
        "temperature": temperature,
        "max_tokens": 16384,
        "stream": true
    });

    let client = &*HTTP_CLIENT;
    let mut response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS))
        .send()
        .await
        .context("OpenAI-compat streaming request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "{} API error ({}): {}",
            llm_config.provider,
            status,
            text
        ));
    }

    let mut buf = String::new();
    let mut full_text = String::new();
    // index → (id, name, accumulated_args_json)
    let mut tool_map: std::collections::HashMap<usize, (String, String, String)> =
        std::collections::HashMap::new();

    'outer: while let Some(chunk) = response
        .chunk()
        .await
        .context("OpenAI streaming read failed")?
    {
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(line) = pop_line(&mut buf) {
            let line = line.trim().to_string();
            if line == "data: [DONE]" {
                break 'outer;
            }
            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(val) = serde_json::from_str::<Value>(data) {
                    if let Some(delta) = val.pointer("/choices/0/delta") {
                        if let Some(content) =
                            delta.get("content").and_then(|c| c.as_str())
                        {
                            if !content.is_empty() {
                                on_chunk(content.to_string());
                                full_text.push_str(content);
                            }
                        }
                        if let Some(tc_arr) =
                            delta.get("tool_calls").and_then(|tc| tc.as_array())
                        {
                            for tc_delta in tc_arr {
                                let index = tc_delta
                                    .get("index")
                                    .and_then(|i| i.as_u64())
                                    .unwrap_or(0) as usize;
                                let entry = tool_map
                                    .entry(index)
                                    .or_insert_with(|| (String::new(), String::new(), String::new()));
                                if let Some(id) =
                                    tc_delta.get("id").and_then(|i| i.as_str())
                                {
                                    entry.0 = id.to_string();
                                }
                                if let Some(func) = tc_delta.get("function") {
                                    if let Some(name) =
                                        func.get("name").and_then(|n| n.as_str())
                                    {
                                        entry.1 = name.to_string();
                                    }
                                    if let Some(args) =
                                        func.get("arguments").and_then(|a| a.as_str())
                                    {
                                        entry.2.push_str(args);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut tool_call_list: Vec<(usize, ToolCall)> = tool_map
        .into_iter()
        .map(|(idx, (id, name, args_str))| {
            let args = serde_json::from_str::<Value>(&args_str).unwrap_or(json!({}));
            (idx, ToolCall { id, name, args })
        })
        .collect();
    tool_call_list.sort_by_key(|(idx, _)| *idx);
    let tool_calls: Vec<ToolCall> = tool_call_list.into_iter().map(|(_, tc)| tc).collect();

    let text = if full_text.is_empty() { None } else { Some(full_text) };

    if tool_calls.is_empty() {
        if let Some(ref t) = text {
            conversation
                .messages
                .push(ConvMessage::AssistantText(t.clone()));
        }
    } else {
        conversation
            .messages
            .push(ConvMessage::AssistantToolCalls(tool_calls.clone()));
    }
    Ok(AgentRoundResult { text, tool_calls })
}

// ── Anthropic streaming ───────────────────────────────────────────────────────

async fn call_agent_round_anthropic_streaming(
    llm_config: &LlmConfig,
    system_prompt: &str,
    conversation: &mut AgentConversation,
    allowed_tools: &[String],
    temperature: f64,
    on_chunk: Box<dyn Fn(String) + Send>,
) -> Result<AgentRoundResult> {
    let api_key = llm_config
        .api_key
        .as_deref()
        .ok_or_else(|| anyhow!("No Anthropic API key configured"))?;

    let messages = conversation_to_anthropic(conversation);
    let tools = build_anthropic_tool_declarations(allowed_tools);

    let body = json!({
        "model": llm_config.model,
        "system": system_prompt,
        "messages": messages,
        "tools": tools,
        "max_tokens": 8192,
        "temperature": temperature,
        "stream": true
    });

    let client = &*HTTP_CLIENT;
    let mut response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS))
        .send()
        .await
        .context("Anthropic streaming request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow!("Anthropic API error ({}): {}", status, text));
    }

    let mut buf = String::new();
    let mut full_text = String::new();
    // block_index → (id, name, accumulated_partial_json)
    let mut tool_blocks: std::collections::HashMap<usize, (String, String, String)> =
        std::collections::HashMap::new();

    while let Some(chunk) = response
        .chunk()
        .await
        .context("Anthropic streaming read failed")?
    {
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(line) = pop_line(&mut buf) {
            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(val) = serde_json::from_str::<Value>(data) {
                    match val.get("type").and_then(|t| t.as_str()) {
                        Some("content_block_start") => {
                            let idx = val
                                .get("index")
                                .and_then(|i| i.as_u64())
                                .unwrap_or(0) as usize;
                            if let Some(block) = val.get("content_block") {
                                if block
                                    .get("type")
                                    .and_then(|t| t.as_str())
                                    == Some("tool_use")
                                {
                                    let id = block
                                        .get("id")
                                        .and_then(|i| i.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let name = block
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    tool_blocks.insert(idx, (id, name, String::new()));
                                }
                            }
                        }
                        Some("content_block_delta") => {
                            let idx = val
                                .get("index")
                                .and_then(|i| i.as_u64())
                                .unwrap_or(0) as usize;
                            if let Some(delta) = val.get("delta") {
                                match delta
                                    .get("type")
                                    .and_then(|t| t.as_str())
                                {
                                    Some("text_delta") => {
                                        if let Some(text) =
                                            delta.get("text").and_then(|t| t.as_str())
                                        {
                                            if !text.is_empty() {
                                                on_chunk(text.to_string());
                                                full_text.push_str(text);
                                            }
                                        }
                                    }
                                    Some("input_json_delta") => {
                                        if let Some(partial) = delta
                                            .get("partial_json")
                                            .and_then(|p| p.as_str())
                                        {
                                            if let Some(entry) = tool_blocks.get_mut(&idx) {
                                                entry.2.push_str(partial);
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let mut tool_call_list: Vec<(usize, ToolCall)> = tool_blocks
        .into_iter()
        .map(|(idx, (id, name, args_str))| {
            let args = serde_json::from_str::<Value>(&args_str).unwrap_or(json!({}));
            (idx, ToolCall { id, name, args })
        })
        .collect();
    tool_call_list.sort_by_key(|(idx, _)| *idx);
    let tool_calls: Vec<ToolCall> = tool_call_list.into_iter().map(|(_, tc)| tc).collect();

    let text = if full_text.is_empty() { None } else { Some(full_text) };

    if tool_calls.is_empty() {
        if let Some(ref t) = text {
            conversation
                .messages
                .push(ConvMessage::AssistantText(t.clone()));
        }
    } else {
        conversation
            .messages
            .push(ConvMessage::AssistantToolCalls(tool_calls.clone()));
    }
    Ok(AgentRoundResult { text, tool_calls })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversation_to_gemini_basic() {
        let mut conv = AgentConversation::new();
        conv.push_user("hello".into());
        let gemini = conversation_to_gemini(&conv);
        assert_eq!(gemini.len(), 1);
        assert_eq!(gemini[0]["role"], "user");
        assert_eq!(gemini[0]["parts"][0]["text"], "hello");
    }

    #[test]
    fn test_conversation_to_openai_includes_system() {
        let mut conv = AgentConversation::new();
        conv.push_user("task".into());
        let msgs = conversation_to_openai(&conv, "you are an assistant");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
    }

    #[test]
    fn test_conversation_to_anthropic_tool_results_use_user_role() {
        let mut conv = AgentConversation::new();
        conv.push_user("do the thing".into());
        conv.messages.push(ConvMessage::AssistantToolCalls(vec![ToolCall {
            id: "c1".into(),
            name: "read_file".into(),
            args: json!({"path": "/workspace/foo.rs"}),
        }]));
        conv.push_tool_results(vec![(
            ToolCall {
                id: "c1".into(),
                name: "read_file".into(),
                args: json!({}),
            },
            json!({"content": "file content"}),
        )]);
        let msgs = conversation_to_anthropic(&conv);
        // user, assistant (tool_use), user (tool_result)
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[2]["role"], "user");
        assert_eq!(msgs[2]["content"][0]["type"], "tool_result");
    }

    #[test]
    fn test_build_openai_tool_declarations_wraps_in_function() {
        let tools = build_openai_tool_declarations(&["read_file".to_string()]);
        assert!(!tools.is_empty());
        assert_eq!(tools[0]["type"], "function");
        assert!(tools[0]["function"]["name"].as_str().is_some());
    }

    #[test]
    fn test_build_anthropic_tool_declarations_uses_input_schema() {
        let tools = build_anthropic_tool_declarations(&["write_file".to_string()]);
        assert!(!tools.is_empty());
        assert!(tools[0].get("input_schema").is_some());
    }

    #[test]
    fn test_llm_config_vertex_constructor() {
        let cfg = LlmConfig::vertex("my-project");
        assert!(matches!(cfg.provider, AiProvider::VertexAi));
        assert_eq!(cfg.vertex_project_id.as_deref(), Some("my-project"));
    }
}
