use crate::sandbox::{ResourceLimits, Sandbox};
use crate::tool_registry;
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
    /// Socket path for communicating with agentd
    pub socket_path: Option<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model: "gemini-2.5-pro".to_string(),
            tools: Vec::new(),
            resources: ResourceLimits::default(),
            system_prompt: None,
            max_rounds: 50,
            socket_path: Some("/tmp/agentd.sock".to_string()),
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
    /// Tools registered with this agent
    registered_tools: Vec<String>,
}

impl Agent {
    pub fn spawn(config: AgentConfig) -> Result<Self, anyhow::Error> {
        let sandbox = Sandbox::new(config.resources.clone())?;

        // Register all configured tools with the sandbox
        let mut registered_tools = Vec::new();
        if config.tools.is_empty() {
            // Register all available tools
            for tool in tool_registry::create_all_tools() {
                let name = tool.name().to_string();
                sandbox.register_tool(tool);
                registered_tools.push(name);
            }
        } else {
            // Register only specified tools
            let all_tools = tool_registry::create_all_tools();
            for tool in all_tools {
                if config.tools.contains(&tool.name().to_string()) {
                    let name = tool.name().to_string();
                    sandbox.register_tool(tool);
                    registered_tools.push(name);
                }
            }
        }

        log::info!(
            "Spawned agent in sandbox {} with model {} and {}/{} tools",
            sandbox.id(),
            config.model,
            registered_tools.len(),
            config.tools.len().max(75),
        );

        Ok(Agent {
            sandbox,
            config,
            registered_tools,
        })
    }

    /// Run the agent with a prompt. Executes a single tool invocation step.
    ///
    /// For full multi-round tool-calling loops, use the orchestration layer's
    /// `AgentExecutor` which manages the LLM conversation and tool dispatch.
    /// This method provides direct single-step execution.
    pub fn run(&mut self, prompt: &str) -> Result<AgentResult, anyhow::Error> {
        let mut tools_invoked = Vec::new();

        let system_prompt =
            self.config.system_prompt.as_deref().unwrap_or(
                "You are a helpful AI agent. Use the available tools to complete tasks.",
            );

        log::info!(
            "Agent {} starting execution: prompt='{}', tools={}, max_rounds={}",
            self.sandbox.id(),
            if prompt.len() > 100 {
                &prompt[..100]
            } else {
                prompt
            },
            self.registered_tools.len(),
            self.config.max_rounds,
        );

        // Build the tool invocation context
        let ctx = crate::tools::common::ToolContext::new(
            self.sandbox.id(),
            Some(self.sandbox.root_path().to_path_buf()),
        );

        // Execute the prompt as a single tool call if it matches a tool pattern
        // For complex multi-step tasks, the orchestration layer handles the loop
        let output = if prompt.starts_with("run:") {
            // Direct command execution
            let cmd = prompt.strip_prefix("run:").unwrap_or(prompt).trim();
            let input = serde_json::json!({"cmd": cmd, "cwd": "/workspace"});
            match self.sandbox.invoke_tool_by_name("run_command", &ctx, input) {
                Ok(result) => {
                    tools_invoked.push("run_command".to_string());
                    serde_json::to_string_pretty(&result).unwrap_or_default()
                }
                Err(e) => format!("Tool error: {}", e),
            }
        } else if prompt.starts_with("read:") {
            let path = prompt.strip_prefix("read:").unwrap_or(prompt).trim();
            let input = serde_json::json!({"path": path});
            match self.sandbox.invoke_tool_by_name("read_file", &ctx, input) {
                Ok(result) => {
                    tools_invoked.push("read_file".to_string());
                    serde_json::to_string_pretty(&result).unwrap_or_default()
                }
                Err(e) => format!("Tool error: {}", e),
            }
        } else if prompt.starts_with("write:") {
            // Format: write:path:content
            let parts: Vec<&str> = prompt
                .strip_prefix("write:")
                .unwrap_or("")
                .splitn(2, ':')
                .collect();
            if parts.len() == 2 {
                let input = serde_json::json!({"path": parts[0].trim(), "content": parts[1]});
                match self.sandbox.invoke_tool_by_name("write_file", &ctx, input) {
                    Ok(result) => {
                        tools_invoked.push("write_file".to_string());
                        serde_json::to_string_pretty(&result).unwrap_or_default()
                    }
                    Err(e) => format!("Tool error: {}", e),
                }
            } else {
                return Err(anyhow::anyhow!(
                    "Invalid write format. Use write:path:content"
                ));
            }
        } else {
            // For non-direct commands, return the prompt for the orchestration layer to handle
            prompt.to_string()
        };

        // Determine success based on tool invocation results
        let success = if tools_invoked.is_empty() {
            true // No tools invoked, just forwarding prompt
        } else if let Ok(result_json) = serde_json::from_str::<serde_json::Value>(&output) {
            result_json
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(true)
        } else {
            true
        };

        // Capture any changes made
        let git_diff = self.capture_diff().ok();

        Ok(AgentResult {
            success,
            output: Some(output),
            git_diff,
            error: None,
            rounds_executed: 1,
            tools_invoked,
        })
    }

    /// Invoke a specific tool with input
    pub fn invoke_tool(
        &self,
        tool_name: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, anyhow::Error> {
        let ctx = crate::tools::common::ToolContext::new(
            self.sandbox.id(),
            Some(self.sandbox.root_path().to_path_buf()),
        );
        self.sandbox.invoke_tool_by_name(tool_name, &ctx, input)
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
        let agent = Agent::spawn(config)?;
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

    /// Get list of registered tools
    pub fn tools(&self) -> &[String] {
        &self.registered_tools
    }

    /// Get the sandbox root path
    pub fn root_path(&self) -> &std::path::Path {
        self.sandbox.root_path()
    }
}
