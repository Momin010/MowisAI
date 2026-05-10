use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::mpsc;

// ─────────────────────────────────────────────────────────────────────────────
// Types — mirror the Go HTTP API
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub healthy: bool,
    pub version: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub parent_session_id: Option<String>,
    pub title: String,
    pub message_count: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub cost: Option<f64>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    #[serde(default)]
    pub parts: Vec<ContentPart>,
    pub model: Option<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "reasoning")]
    Reasoning { text: String },
    #[serde(rename = "tool_call")]
    ToolCall {
        call_id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        call_id: String,
        name: String,
        content: String,
        is_error: bool,
    },
    #[serde(rename = "finish")]
    Finish { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: String,
    pub session_id: String,
    pub tool_name: String,
    pub description: String,
    pub action: String,
    pub params: Option<serde_json::Value>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SseEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: serde_json::Value,
}

// ─────────────────────────────────────────────────────────────────────────────
// AgentClient — HTTP client for mowis-agent
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AgentClient {
    base_url: String,
    http: reqwest::Client,
}

impl AgentClient {
    pub fn new(port: u16) -> Self {
        Self {
            base_url: format!("http://localhost:{}", port),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(300))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    pub fn with_base_url(base_url: String) -> Self {
        Self {
            base_url,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(300))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    // ── Health ───────────────────────────────────────────────────────────

    pub async fn health(&self) -> Result<HealthResponse> {
        let resp = self
            .http
            .get(format!("{}/health", self.base_url))
            .send()
            .await
            .context("health check request failed")?;
        resp.json().await.context("health check parse failed")
    }

    // ── Sessions ─────────────────────────────────────────────────────────

    pub async fn create_session(&self, title: &str) -> Result<Session> {
        let resp = self
            .http
            .post(format!("{}/session", self.base_url))
            .json(&serde_json::json!({ "title": title }))
            .send()
            .await
            .context("create session request failed")?;
        resp.json().await.context("create session parse failed")
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        let resp = self
            .http
            .get(format!("{}/session", self.base_url))
            .send()
            .await
            .context("list sessions request failed")?;
        resp.json().await.context("list sessions parse failed")
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Session> {
        let resp = self
            .http
            .get(format!("{}/session/{}", self.base_url, session_id))
            .send()
            .await
            .context("get session request failed")?;
        resp.json().await.context("get session parse failed")
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        self.http
            .delete(format!("{}/session/{}", self.base_url, session_id))
            .send()
            .await
            .context("delete session request failed")?;
        Ok(())
    }

    // ── Messages ─────────────────────────────────────────────────────────

    pub async fn list_messages(&self, session_id: &str) -> Result<Vec<AgentMessage>> {
        let resp = self
            .http
            .get(format!("{}/session/{}/message", self.base_url, session_id))
            .send()
            .await
            .context("list messages request failed")?;
        resp.json().await.context("list messages parse failed")
    }

    pub async fn send_message(
        &self,
        session_id: &str,
        text: &str,
    ) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(format!(
                "{}/session/{}/message",
                self.base_url, session_id
            ))
            .json(&serde_json::json!({ "text": text }))
            .send()
            .await
            .context("send message request failed")?;
        resp.json().await.context("send message parse failed")
    }

    pub async fn send_message_async(&self, session_id: &str, text: &str) -> Result<()> {
        self.http
            .post(format!(
                "{}/session/{}/message/async",
                self.base_url, session_id
            ))
            .json(&serde_json::json!({ "text": text }))
            .send()
            .await
            .context("send message async request failed")?;
        Ok(())
    }

    // ── Agent control ────────────────────────────────────────────────────

    pub async fn abort(&self, session_id: &str) -> Result<()> {
        self.http
            .post(format!("{}/session/{}/abort", self.base_url, session_id))
            .send()
            .await
            .context("abort request failed")?;
        Ok(())
    }

    pub async fn approve_permission(&self, session_id: &str, perm_id: &str) -> Result<()> {
        self.http
            .post(format!(
                "{}/session/{}/permission/{}",
                self.base_url, session_id, perm_id
            ))
            .json(&serde_json::json!({ "approve": true, "persist": false }))
            .send()
            .await
            .context("approve permission request failed")?;
        Ok(())
    }

    pub async fn deny_permission(&self, session_id: &str, perm_id: &str) -> Result<()> {
        self.http
            .post(format!(
                "{}/session/{}/permission/{}",
                self.base_url, session_id, perm_id
            ))
            .json(&serde_json::json!({ "approve": false, "persist": false }))
            .send()
            .await
            .context("deny permission request failed")?;
        Ok(())
    }

    // ── SSE event stream ─────────────────────────────────────────────────

    pub async fn subscribe_events(
        &self,
    ) -> Result<mpsc::Receiver<SseEvent>> {
        let url = format!("{}/event", self.base_url);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("SSE connect failed")?;

        let (tx, rx) = mpsc::channel(256);

        tokio::spawn(async move {
            let mut stream = resp.bytes_stream();
            use futures_util::StreamExt;
            let mut buf = Vec::new();
            let mut current_event = String::new();

            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(_) => break,
                };
                buf.extend_from_slice(&chunk);

                while let Some(newline_pos) = buf.iter().position(|&b| b == b'\n') {
                    let line = buf[..newline_pos].to_vec();
                    buf = buf[newline_pos + 1..].to_vec();

                    let line_str = String::from_utf8_lossy(&line);

                    if line_str.starts_with("event: ") {
                        current_event = line_str[7..].trim().to_string();
                    } else if line_str.starts_with("data: ") {
                        let data = &line_str[6..];
                        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(data) {
                            let evt = SseEvent {
                                event_type: current_event.clone(),
                                payload,
                            };
                            if tx.send(evt).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        });

        Ok(rx)
    }
}
