use crate::sandbox::{ResourceLimits, Sandbox};
use serde::{Deserialize, Serialize};

/// Configuration used when spawning a new agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub model: String, // placeholder handle
    pub tools: Vec<String>,
    pub resources: ResourceLimits,
}

/// Result returned by running an agent or a step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub success: bool,
    pub output: Option<String>,
}

pub struct Agent {
    sandbox: Sandbox,
    config: AgentConfig,
}

impl Agent {
    pub fn spawn(config: AgentConfig) -> Result<Self, anyhow::Error> {
        let sandbox = Sandbox::new(config.resources.clone())?;
        Ok(Agent { sandbox, config })
    }

    pub fn run(&mut self, prompt: &str) -> Result<AgentResult, anyhow::Error> {
        // placeholder: just echo the prompt
        Ok(AgentResult {
            success: true,
            output: Some(prompt.to_string()),
        })
    }

    pub fn spawn_subagent(&self, config: AgentConfig) -> Result<Agent, anyhow::Error> {
        // create a sandbox derived from the parent to enforce the inheritance law
        let child_sandbox = self.sandbox.spawn_child(config.resources.clone())?;
        let agent = Agent {
            sandbox: child_sandbox,
            config,
        };
        log::info!(
            "spawned subagent {} from parent {}",
            agent.sandbox.id(),
            self.sandbox.id()
        );
        Ok(agent)
    }
}
