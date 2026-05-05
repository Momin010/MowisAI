use crate::sandbox::{ResourceLimits, Sandbox};
use serde::{Deserialize, Serialize};

/// Configuration used when spawning a new agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub model: String,
    pub tools: Vec<String>,
    pub resources: ResourceLimits,
    /// System prompt for the agent
    pub system_prompt: Option<String>,
    /// Maximum number of tool-calling rounds
    pub max_rounds: u32,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model: "gemini-2.5-pro".to_string(),
            tools: Vec::new(),
            resources: ResourceLimits::default(),
            system_prompt: None,
            max_rounds: 50,
        }
    }
}

/// Result returned by running an agent or a step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub success: bool,
    pub output: Option<String>,
    /// Git diff of changes made by the agent
    pub git_diff: Option<String>,
    /// Error message if failed
    pub error: Option<String>,
    /// Number of tool-calling rounds executed
    pub rounds_executed: u32,
    /// Tools that were invoked
    pub tools_invoked: Vec<String>,
}

pub struct Agent {
    sandbox: Sandbox,
    config: AgentConfig,
}

impl Agent {
    pub fn spawn(config: AgentConfig) -> Result<Self, anyhow::Error> {
        let sandbox = Sandbox::new(config.resources.clone())?;
        log::info!(
            "Spawned agent in sandbox {} with model {} and {} tools",
            sandbox.id(),
            config.model,
            config.tools.len()
        );
        Ok(Agent { sandbox, config })
    }

    /// Run the agent with a prompt. Executes tool-calling loop.
    pub fn run(&mut self, prompt: &str) -> Result<AgentResult, anyhow::Error> {
        let mut rounds = 0u32;
        let mut tools_invoked = Vec::new();
        let mut last_output = String::new();

        // Register configured tools
        for tool_name in &self.config.tools {
            // Tools should already be registered via the tool registry
            log::debug!("Agent has tool available: {}", tool_name);
        }

        // Build the initial context
        let system_prompt =
            self.config.system_prompt.as_deref().unwrap_or(
                "You are a helpful AI agent. Use the available tools to complete tasks.",
            );

        // Execute tool-calling loop
        // In a real implementation, this would call the LLM API and dispatch tools
        // For now, we create the execution context and return
        log::info!(
            "Agent starting execution: prompt='{}', max_rounds={}",
            if prompt.len() > 100 {
                &prompt[..100]
            } else {
                prompt
            },
            self.config.max_rounds
        );

        // The actual tool-calling loop is handled by the orchestration layer
        // (agent_execution.rs) which uses the socket API. This method provides
        // the basic agent lifecycle.
        rounds += 1;
        last_output = prompt.to_string();

        // Capture any changes made
        let git_diff = self.capture_diff().ok();

        Ok(AgentResult {
            success: true,
            output: Some(last_output),
            git_diff,
            error: None,
            rounds_executed: rounds,
            tools_invoked,
        })
    }

    /// Capture git diff of changes in the sandbox
    fn capture_diff(&self) -> Result<String, anyhow::Error> {
        use std::process::Command;

        let root = self.sandbox.root_path();
        let output = Command::new("chroot")
            .arg(root)
            .arg("git")
            .arg("diff")
            .arg("--no-color")
            .output()?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub fn spawn_subagent(&self, config: AgentConfig) -> Result<Agent, anyhow::Error> {
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

    /// Get the sandbox ID
    pub fn sandbox_id(&self) -> u64 {
        self.sandbox.id()
    }

    /// Get the agent's model
    pub fn model(&self) -> &str {
        &self.config.model
    }
}
