use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub healthy: bool,
    pub version: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub message_count: i64,
    #[serde(default)]
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    #[serde(default)]
    pub parts: Vec<serde_json::Value>,
    pub created_at: Option<i64>,
}

pub struct AgentClient {
    base_url: String,
    http: reqwest::Client,
}

impl AgentClient {
    pub fn new(port: u16) -> Self {
        Self {
            base_url: format!("http://127.0.0.1:{}", port),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(300))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    pub async fn health(&self) -> Result<HealthResponse> {
        let resp = self
            .http
            .get(format!("{}/health", self.base_url))
            .send()
            .await
            .context("health check failed")?;
        resp.json().await.context("health parse failed")
    }

    pub async fn create_session(&self, title: &str) -> Result<Session> {
        let resp = self
            .http
            .post(format!("{}/session", self.base_url))
            .json(&serde_json::json!({ "title": title }))
            .send()
            .await
            .context("create session failed")?;
        resp.json().await.context("create session parse failed")
    }

    pub async fn send_message(&self, session_id: &str, text: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(format!("{}/session/{}/message", self.base_url, session_id))
            .json(&serde_json::json!({ "text": text }))
            .send()
            .await
            .context("send message failed")?;
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
            .context("send message async failed")?;
        Ok(())
    }

    pub async fn list_messages(&self, session_id: &str) -> Result<Vec<AgentMessage>> {
        let resp = self
            .http
            .get(format!("{}/session/{}/message", self.base_url, session_id))
            .send()
            .await
            .context("list messages failed")?;
        resp.json().await.context("list messages parse failed")
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        let resp = self
            .http
            .get(format!("{}/session", self.base_url))
            .send()
            .await
            .context("list sessions failed")?;
        resp.json().await.context("list sessions parse failed")
    }

    pub async fn abort(&self, session_id: &str) -> Result<()> {
        self.http
            .post(format!("{}/session/{}/abort", self.base_url, session_id))
            .send()
            .await
            .context("abort failed")?;
        Ok(())
    }
}
