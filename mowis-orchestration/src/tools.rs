use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::plan::Tier;

pub struct ToolGateway {
    transport: Arc<Mutex<Option<Box<dyn ToolTransport>>>>,
    tier: Tier,
    allowlist: ToolAllowlist,
    sandbox_id: String,
    agent_id: String,
}

impl std::fmt::Debug for ToolGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolGateway")
            .field("tier", &self.tier)
            .field("sandbox_id", &self.sandbox_id)
            .field("agent_id", &self.agent_id)
            .finish()
    }
}

impl Clone for ToolGateway {
    fn clone(&self) -> Self {
        Self {
            transport: self.transport.clone(),
            tier: self.tier.clone(),
            allowlist: self.allowlist.clone(),
            sandbox_id: self.sandbox_id.clone(),
            agent_id: self.agent_id.clone(),
        }
    }
}

#[async_trait::async_trait]
pub trait ToolTransport: Send + Sync {
    async fn invoke_tool(&self, payload: mowis_protocol::Payload) -> Result<mowis_protocol::Payload>;
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolAllowlist {
    pub allowed: Vec<String>,
    pub denied_prefixes: Vec<String>,
}

impl ToolAllowlist {
    pub fn allows(&self, tool_name: &str) -> bool {
        if self.denied_prefixes.iter().any(|d| tool_name.starts_with(d)) {
            return false;
        }
        self.allowed.is_empty() || self.allowed.iter().any(|a| a == tool_name)
    }
}

pub fn conductor_allowlist() -> ToolAllowlist {
    ToolAllowlist {
        allowed: vec![
            "read_file".into(),
            "list_files".into(),
            "list_dir".into(),
            "grep".into(),
            "find_files".into(),
            "file_exists".into(),
            "get_file_info".into(),
        ],
        denied_prefixes: vec![],
    }
}

pub fn crew_allowlist() -> ToolAllowlist {
    ToolAllowlist {
        allowed: vec![],
        denied_prefixes: vec![],
    }
}

impl ToolGateway {
    pub fn new(
        tier: Tier,
        allowlist: ToolAllowlist,
        sandbox_id: String,
        agent_id: String,
    ) -> Self {
        Self {
            transport: Arc::new(Mutex::new(None)),
            tier,
            allowlist,
            sandbox_id,
            agent_id,
        }
    }

    pub async fn set_transport(&self, transport: Box<dyn ToolTransport>) {
        let mut t = self.transport.lock().await;
        *t = Some(transport);
    }

    pub async fn invoke(
        &self,
        call: crate::providers::ToolCall,
    ) -> Result<serde_json::Value> {
        if !self.allowlist.allows(&call.name) {
            return Ok(serde_json::json!({"error": format!("tool `{}` is forbidden for {:?} tier", call.name, self.tier)}));
        }

        let transport = self.transport.lock().await;
        let transport = transport
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("ToolGateway: no transport configured"))?;

        let payload = mowis_protocol::Payload::InvokeToolAsAgent {
            parent_sandbox_id: self.sandbox_id.clone(),
            agent_id: self.agent_id.clone(),
            tool: call.name.clone(),
            input: call.args.clone(),
            caller_tier: format!("{:?}", self.tier).to_lowercase(),
        };

        match transport.invoke_tool(payload).await? {
            mowis_protocol::Payload::ToolResult { output } => Ok(output),
            mowis_protocol::Payload::Error { message } => {
                Ok(serde_json::json!({"error": message}))
            }
            other => Err(anyhow::anyhow!("unexpected response from executor: {:?}", other)),
        }
    }
}
