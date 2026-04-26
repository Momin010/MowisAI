//! Layer 4: Agent Execution — provider-agnostic tool-calling loop with checkpoint system

use super::checkpoint::{CheckpointLog, CheckpointManager};
use super::provider_client::{AgentConversation, AgentRoundResult, LlmConfig, ToolCall};
use agentd_protocol::{AgentHandle, AgentResult, Checkpoint};
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Global flag for verbose output
static VERBOSE_MODE: AtomicBool = AtomicBool::new(false);

pub fn set_verbose(enabled: bool) {
    VERBOSE_MODE.store(enabled, Ordering::Relaxed);
}

pub fn is_verbose() -> bool {
    VERBOSE_MODE.load(Ordering::Relaxed)
}

/// Agent executor with checkpoint support
pub struct AgentExecutor {
    llm_config: LlmConfig,
    socket_path: String,
    checkpoint_manager: CheckpointManager,
    max_tool_rounds: usize,
    max_tier1_retries: usize,
    max_tier2_retries: usize,
}

impl AgentExecutor {
    pub fn new(
        llm_config: LlmConfig,
        socket_path: String,
        checkpoint_root: PathBuf,
    ) -> Result<Self> {
        Ok(Self {
            llm_config,
            socket_path: socket_path.clone(),
            checkpoint_manager: CheckpointManager::new(checkpoint_root, socket_path)?,
            max_tool_rounds: super::MAX_TOOL_ROUNDS,
            max_tier1_retries: 3,
            max_tier2_retries: 2,
        })
    }

    /// Execute task with agent
    pub async fn execute_task(
        &self,
        agent: &AgentHandle,
        task_description: &str,
        tools: &[String],
        system_prompt: &str,
    ) -> Result<AgentResult> {
        if is_verbose() {
            log::info!("\n┌─────────────────────────────────────────────────────────");
            log::info!("│ 🤖 AGENT: {}", &agent.agent_id[..8.min(agent.agent_id.len())]);
            log::info!("│ 📋 TASK: {}", task_description);
            log::info!("│ 🛠️  TOOLS: {:?}", tools);
            log::info!("└─────────────────────────────────────────────────────────");
        }
        let log_dir = self.checkpoint_manager.get_checkpoint_dir(&agent.agent_id);
        std::fs::create_dir_all(&log_dir)?;

        let mut checkpoint_log = CheckpointLog::new(
            agent.agent_id.clone(),
            agent.task_id.clone().unwrap_or_default(),
            &log_dir,
        )?;

        // Main execution loop with tier 2 retry support
        for tier2_attempt in 0..=self.max_tier2_retries {
            match self
                .execute_with_checkpoints(
                    agent,
                    task_description,
                    tools,
                    system_prompt,
                    &mut checkpoint_log,
                )
                .await
            {
                Ok(result) => {
                    self.checkpoint_manager
                        .cleanup_agent_checkpoints(&agent.agent_id)
                        .ok();
                    return Ok(result);
                }
                Err(e) if tier2_attempt < self.max_tier2_retries => {
                    log::warn!(
                        "Agent execution failed (tier 2 retry {}/{}): {}",
                        tier2_attempt + 1,
                        self.max_tier2_retries,
                        e
                    );

                    // Tier 2: Restore from last checkpoint
                    if let Some(last_checkpoint) = checkpoint_log.latest() {
                        let snapshot_path =
                            PathBuf::from(&last_checkpoint.layer_snapshot_path);

                        if snapshot_path.exists() {
                            match self.checkpoint_manager.restore_snapshot(
                                &agent.sandbox_name,
                                &agent.container_id,
                                &snapshot_path,
                            ) {
                                Ok(()) => {
                                    log::info!(
                                        "  ✓ Restored checkpoint {} for agent {}",
                                        last_checkpoint.id,
                                        &agent.agent_id[..8.min(agent.agent_id.len())]
                                    );
                                }
                                Err(restore_err) => {
                                    log::warn!(
                                        "  ⚠ Checkpoint restore failed: {}",
                                        restore_err
                                    );
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    return Ok(AgentResult {
                        task_id: agent.task_id.clone().unwrap_or_default(),
                        success: false,
                        git_diff: None,
                        error: Some(format!(
                            "Task failed after {} retries: {}",
                            tier2_attempt + 1,
                            e
                        )),
                        checkpoint_log: checkpoint_log.checkpoints.clone(),
                        timestamp: current_timestamp(),
                    });
                }
            }
        }

        Ok(AgentResult {
            task_id: agent.task_id.clone().unwrap_or_default(),
            success: false,
            git_diff: None,
            error: Some("Max retries exceeded".to_string()),
            checkpoint_log: checkpoint_log.checkpoints.clone(),
            timestamp: current_timestamp(),
        })
    }

    /// Execute with checkpoint support using the provider-agnostic tool loop.
    async fn execute_with_checkpoints(
        &self,
        agent: &AgentHandle,
        task_description: &str,
        tools: &[String],
        system_prompt: &str,
        checkpoint_log: &mut CheckpointLog,
    ) -> Result<AgentResult> {
        let mut conversation = AgentConversation::new();

        // Initial user message
        conversation.push_user(format!(
            "IMPORTANT: All file paths MUST start with /workspace (the project directory).\n\n\
            Task: {}\n\n\
            You have access to these tools: {:?}\n\n\
            Complete this task using the available tools.\n\n\
            When using file tools, always use absolute paths starting with /workspace:\n\
            - read_file: path=/workspace/src/file.rs\n\
            - write_file: path=/workspace/src/new_file.rs\n\
            - run_command: Use 'cd /workspace && ...' to change directory first",
            task_description,
            tools
        ));

        // Restore from checkpoint if any
        if checkpoint_log.latest().is_some() {
            let checkpoint_summary = checkpoint_log
                .checkpoints
                .iter()
                .map(|c| {
                    format!(
                        "- {} with args {:?} -> {}",
                        c.tool_call, c.tool_args, c.tool_result
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");

            conversation.push_user(format!(
                "Previous work (recovered from checkpoint):\n{}",
                checkpoint_summary
            ));
        }

        let enhanced_system_prompt = format!(
            "{}\n\n\
            CRITICAL REQUIREMENTS:\n\
            - NEVER write empty or placeholder files. Every file must contain REAL, WORKING code.\n\
            - NEVER use TODO comments or stub implementations.\n\
            - NEVER write files with only imports/empty classes.\n\
            - Every function must have a complete implementation.\n\
            - Every file you write must be production-ready code that compiles and runs.\n\
            - If you don't know how to implement something, read the codebase first using read_file.\n\
            - Examples of FORBIDDEN patterns:\n\
              ❌ write_file(\"auth.py\", \"\")  # Empty file\n\
              ❌ write_file(\"auth.py\", \"# TODO: implement auth\")  # Placeholder\n\
              ❌ write_file(\"auth.py\", \"def login(): pass\")  # Stub\n\
            - Examples of REQUIRED patterns:\n\
              ✅ write_file(\"auth.py\", \"import jwt\\ndef login(user, pwd):\\n  # Real JWT implementation...\")\n\
            \n\
            COMMAND EXECUTION RULES:\n\
            - Commands have a 30-second timeout by default. Long-running commands will be killed.\n\
            - NEVER run servers or long-running processes (npm start, python app.py, etc.).\n\
            - Only write server code and configuration. Do NOT start the server.\n\
            - To test if a server works, write a test file instead of running it.\n\
            - If you must run a command that takes >30s, add {{\"timeout\": 120}} to increase timeout.\n\
            - Examples:\n\
              ❌ run_command(\"npm start\")  # Will hang and timeout\n\
              ❌ run_command(\"python app.py\")  # Will hang and timeout\n\
              ✅ write_file(\"server.js\", \"...server code...\")  # Just write the code\n\
              ✅ run_command(\"npm install\", {{\"timeout\": 120}})  # Install is OK with longer timeout\n\
            \n\
            If you need to create multiple files, implement each one fully before moving to the next.",
            system_prompt
        );

        // Tool-calling loop
        for _ in 0..self.max_tool_rounds {
            let round_result = super::provider_client::call_agent_round(
                &self.llm_config,
                &enhanced_system_prompt,
                &mut conversation,
                tools,
                0.7,
            )
            .await
            .context("LLM call failed in agent tool loop")?;

            if round_result.tool_calls.is_empty() {
                // Agent finished — capture text and git diff
                let final_text = round_result.text.unwrap_or_default();

                if is_verbose() {
                    log::info!("\n  💬 Agent says: {}", final_text);
                }

                let git_diff = self.capture_git_diff(agent).await.ok();

                if is_verbose() {
                    if let Some(ref diff) = git_diff {
                        log::info!("\n  📝 DIFF GENERATED ({} bytes):", diff.len());
                        log::info!("  ┌─────────────────────────────────────────");
                        for line in diff.lines().take(20) {
                            log::info!("  │ {}", line);
                        }
                        if diff.lines().count() > 20 {
                            log::info!(
                                "  │ ... ({} more lines)",
                                diff.lines().count() - 20
                            );
                        }
                        log::info!("  └─────────────────────────────────────────");
                    } else {
                        log::info!("\n  ⚠️  No git diff generated");
                    }
                }

                return Ok(AgentResult {
                    task_id: agent.task_id.clone().unwrap_or_default(),
                    success: true,
                    git_diff,
                    error: None,
                    checkpoint_log: checkpoint_log.checkpoints.clone(),
                    timestamp: current_timestamp(),
                });
            }

            // Execute each tool call, creating a checkpoint after each success
            let mut round_results: Vec<(ToolCall, Value)> = Vec::new();

            for tool_call in round_result.tool_calls {
                if is_verbose() {
                    log::info!(
                        "  🔧 Calling tool: {} with args: {}",
                        tool_call.name,
                        tool_call.args
                    );
                }

                let tool_result = self
                    .execute_tool_with_retry(
                        &agent.sandbox_name,
                        &agent.container_id,
                        &tool_call.name,
                        &tool_call.args,
                    )
                    .await?;

                // Checkpoint after every successful tool call
                let checkpoint_id = checkpoint_log.checkpoints.len() as u64;
                let snapshot_path = self.checkpoint_manager.create_snapshot(
                    &agent.agent_id,
                    checkpoint_id,
                    &agent.sandbox_name,
                    &agent.container_id,
                )?;

                let checkpoint = Checkpoint {
                    id: checkpoint_id,
                    tool_call: tool_call.name.clone(),
                    tool_args: tool_call.args.clone(),
                    tool_result: serde_json::to_string(&tool_result)?,
                    timestamp: current_timestamp(),
                    layer_snapshot_path: snapshot_path.to_string_lossy().to_string(),
                };

                checkpoint_log.add_checkpoint(checkpoint)?;
                checkpoint_log.prune(5)?;

                if is_verbose() {
                    let result_str =
                        serde_json::to_string_pretty(&tool_result).unwrap_or_default();
                    let preview = if result_str.len() > 200 {
                        format!("{}... ({} bytes)", &result_str[..200], result_str.len())
                    } else {
                        result_str
                    };
                    log::info!("  ✅ Result: {}", preview);
                }

                round_results.push((tool_call, tool_result));
            }

            // Feed all tool results back to the conversation in one batch
            conversation.push_tool_results(round_results);
        }

        Err(anyhow!("Max tool rounds exceeded"))
    }

    /// Execute tool with tier 1 retry support
    async fn execute_tool_with_retry(
        &self,
        sandbox_name: &str,
        container_id: &str,
        tool_name: &str,
        tool_args: &Value,
    ) -> Result<Value> {
        for attempt in 0..=self.max_tier1_retries {
            match self
                .execute_tool(sandbox_name, container_id, tool_name, tool_args)
                .await
            {
                Ok(result) => return Ok(result),
                Err(e) if attempt < self.max_tier1_retries => {
                    log::warn!(
                        "Tool {} failed (tier 1 retry {}/{}): {}",
                        tool_name,
                        attempt + 1,
                        self.max_tier1_retries,
                        e
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
                Err(e) => return Err(e),
            }
        }

        Err(anyhow!("Tool execution failed after retries"))
    }

    /// Execute single tool via agentd socket
    async fn execute_tool(
        &self,
        sandbox_name: &str,
        container_id: &str,
        tool_name: &str,
        tool_args: &Value,
    ) -> Result<Value> {
        let result = super::invoke_tool_via_socket(
            &self.socket_path,
            sandbox_name,
            container_id,
            tool_name,
            tool_args,
        )?;

        if let Some(error) = result.get("error") {
            return Err(anyhow!("Tool error: {}", error));
        }

        Ok(result)
    }

    /// Capture git diff from agent layer via socket API
    async fn capture_git_diff(&self, agent: &AgentHandle) -> Result<String> {
        let sandbox_id = &agent.sandbox_name;
        let container_id = &agent.container_id;

        let add_request = json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "name": "run_command",
            "input": {
                "cmd": "cd /workspace && git add -A",
                "timeout": 30
            }
        });
        let add_response = super::socket_roundtrip(&self.socket_path, &add_request)?;
        if is_verbose() {
            log::info!("  🔧 Staging all changes with git add -A");
            if let Some(result) = add_response.get("result") {
                if let Some(stderr) = result.get("stderr").and_then(|s| s.as_str()) {
                    if !stderr.trim().is_empty() {
                        log::info!("  ℹ️  git add stderr: {}", stderr);
                    }
                }
            }
        }

        let diff_request = json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "name": "run_command",
            "input": {
                "cmd": "cd /workspace && (git rev-parse HEAD 2>/dev/null && git diff --cached HEAD || git diff --cached)",
                "timeout": 60
            }
        });
        let diff_response = super::socket_roundtrip(&self.socket_path, &diff_request)?;
        if let Some(result) = diff_response.get("result") {
            if let Some(stdout) = result.get("stdout").and_then(|o| o.as_str()) {
                if !stdout.trim().is_empty() {
                    if is_verbose() {
                        log::info!("  ✅ Captured {} bytes of diff", stdout.len());
                    }
                    return Ok(stdout.to_string());
                }
            }
            if let Some(stderr) = result.get("stderr").and_then(|s| s.as_str()) {
                if !stderr.trim().is_empty() && stderr.contains("fatal") {
                    return Err(anyhow!("git diff failed: {}", stderr));
                }
            }
        }
        if is_verbose() {
            log::info!("  ℹ️  No changes detected (empty diff)");
        }
        Ok(String::new())
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_timestamp_is_nonzero() {
        assert!(current_timestamp() > 0);
    }
}
