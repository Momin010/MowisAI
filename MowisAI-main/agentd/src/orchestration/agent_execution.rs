//! Layer 4: Agent Execution — Gemini tool-calling loop with checkpoint system

use super::checkpoint::{CheckpointLog, CheckpointManager};
use agentd_protocol::{AgentHandle, AgentResult, Checkpoint, TaskId};
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Global flag for verbose output
static VERBOSE_MODE: AtomicBool = AtomicBool::new(false);

/// Enable or disable verbose mode
pub fn set_verbose(enabled: bool) {
    VERBOSE_MODE.store(enabled, Ordering::Relaxed);
}

/// Check if verbose mode is enabled
pub fn is_verbose() -> bool {
    VERBOSE_MODE.load(Ordering::Relaxed)
}

/// Agent executor with checkpoint support
pub struct AgentExecutor {
    project_id: String,
    socket_path: String,
    checkpoint_manager: CheckpointManager,
    max_tool_rounds: usize,
    max_tier1_retries: usize,
    max_tier2_retries: usize,
}

impl AgentExecutor {
    pub fn new(
        project_id: String,
        socket_path: String,
        checkpoint_root: PathBuf,
    ) -> Result<Self> {
        Ok(Self {
            project_id,
            socket_path,
            checkpoint_manager: CheckpointManager::new(checkpoint_root)?,
            max_tool_rounds: super::MAX_TOOL_ROUNDS,
            max_tier1_retries: 3,
            max_tier2_retries: 2,
        })
    }

    /// Get the host path to the container's upper directory.
    /// This is the actual path on the host filesystem, not the in-container path.
    /// The agent's layer.upper_dir stores the in-container path (/sandbox/...),
    /// but checkpoints run on the host where we need /tmp/container-{id}/upper.
    fn get_host_upper_dir(&self, agent: &AgentHandle) -> PathBuf {
        // Container ID is already the full ID (numeric with random bits)
        // The host stores containers at /tmp/container-{container_id}/upper
        PathBuf::from(format!("/tmp/container-{}/upper", agent.container_id))
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
            log::info!("│ 🤖 AGENT: {}", &agent.agent_id[..8]);
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
                    // Success - cleanup checkpoints
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
                        // Use host path, not in-container path
                        let upper_dir = self.get_host_upper_dir(agent);
                        let snapshot_path = PathBuf::from(&last_checkpoint.layer_snapshot_path);

                        if snapshot_path.exists() {
                            match self.checkpoint_manager.restore_snapshot(&upper_dir, &snapshot_path) {
                                Ok(()) => {
                                    log::info!("  ✓ Restored checkpoint {} for agent {}",
                                        last_checkpoint.id, &agent.agent_id[..8]);
                                }
                                Err(restore_err) => {
                                    log::warn!("  ⚠ Checkpoint restore failed: {}", restore_err);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    // Tier 3: Escalate to human
                    return Ok(AgentResult {
                        task_id: agent.task_id.clone().unwrap_or_default(),
                        success: false,
                        git_diff: None,
                        error: Some(format!("Task failed after {} retries: {}", tier2_attempt + 1, e)),
                        checkpoint_log: checkpoint_log.checkpoints.clone(),
                        timestamp: current_timestamp(),
                    });
                }
            }
        }

        // Should not reach here
        Ok(AgentResult {
            task_id: agent.task_id.clone().unwrap_or_default(),
            success: false,
            git_diff: None,
            error: Some("Max retries exceeded".to_string()),
            checkpoint_log: checkpoint_log.checkpoints.clone(),
            timestamp: current_timestamp(),
        })
    }

    /// Execute with checkpoint support
    async fn execute_with_checkpoints(
        &self,
        agent: &AgentHandle,
        task_description: &str,
        tools: &[String],
        system_prompt: &str,
        checkpoint_log: &mut CheckpointLog,
    ) -> Result<AgentResult> {
        let access_token = super::gcloud_access_token()?;
        let url = super::vertex_generate_url(&self.project_id);

        let mut conversation: Vec<Value> = vec![json!({
            "role": "user",
            "parts": [{"text": format!(
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
            )}]
        })];

        // Restore from checkpoint if any
        if let Some(last_checkpoint) = checkpoint_log.latest() {
            // Add checkpoint history to context
            let checkpoint_summary = checkpoint_log
                .checkpoints
                .iter()
                .map(|c| format!("- {} with args {:?} -> {}", c.tool_call, c.tool_args, c.tool_result))
                .collect::<Vec<_>>()
                .join("\n");

            conversation.push(json!({
                "role": "user",
                "parts": [{"text": format!("Previous work (recovered from checkpoint):\n{}", checkpoint_summary)}]
            }));
        }

        // Enhanced system prompt that forbids placeholders
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
        for round in 0..self.max_tool_rounds {
            let request_body = json!({
                "contents": conversation,
                "systemInstruction": {
                    "parts": [{"text": enhanced_system_prompt}]
                },
                "tools": [{
                    "functionDeclarations": build_tool_declarations(tools)
                }],
                "generationConfig": super::vertex_generation_config(0.7)
            });

            let client = reqwest::Client::new();
            let response = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", access_token))
                .header("Content-Type", "application/json")
                .json(&request_body)
                .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS))
                .send()
                .await
                .context("Failed to send request to Gemini")?;

            if !response.status().is_success() {
                let error_text = response.text().await.unwrap_or_default();
                return Err(anyhow!("Gemini API error: {}", error_text));
            }

            let response_json: Value = response.json().await.context("Failed to parse Gemini response")?;

            // Extract candidate
            let candidate = response_json
                .get("candidates")
                .and_then(|c| c.get(0))
                .ok_or_else(|| anyhow!("No candidates in response"))?;

            let content = candidate
                .get("content")
                .ok_or_else(|| anyhow!("No content in candidate"))?;

            // Add assistant response to conversation
            conversation.push(content.clone());

            // Check for function calls
            let parts = content
                .get("parts")
                .and_then(|p| p.as_array())
                .ok_or_else(|| anyhow!("No parts in content"))?;

            let mut function_responses = Vec::new();
            let mut has_function_calls = false;

            for part in parts {
                if let Some(function_call) = part.get("functionCall") {
                    has_function_calls = true;
                    let tool_name = function_call
                        .get("name")
                        .and_then(|n| n.as_str())
                        .ok_or_else(|| anyhow!("Missing function name"))?;

                    let tool_args = function_call
                        .get("args")
                        .cloned()
                        .unwrap_or(json!({}));

                    if is_verbose() {
                        log::info!("  🔧 Calling tool: {} with args: {}", tool_name, tool_args);
                    }

                    // Execute tool with tier 1 retry support
                    let tool_result = self
                        .execute_tool_with_retry(
                            &agent.sandbox_name,
                            &agent.container_id,
                            tool_name,
                            &tool_args,
                        )
                        .await?;

        // Create checkpoint after successful tool call
        let checkpoint_id = checkpoint_log.checkpoints.len() as u64;

        // Create actual snapshot of agent's upper layer (using host path)
        let upper_dir = self.get_host_upper_dir(agent);
        let snapshot_path = self.checkpoint_manager.create_snapshot(
            &agent.agent_id,
            checkpoint_id,
            &upper_dir,
        )?;

        let checkpoint = Checkpoint {
            id: checkpoint_id,
            tool_call: tool_name.to_string(),
            tool_args: tool_args.clone(),
            tool_result: serde_json::to_string(&tool_result)?,
            timestamp: current_timestamp(),
            layer_snapshot_path: snapshot_path.to_string_lossy().to_string(),
        };

        checkpoint_log.add_checkpoint(checkpoint)?;

        // Prune old checkpoints (keep last 5)
        checkpoint_log.prune(5)?;

                    if is_verbose() {
                        // Show tool result preview
                        let result_str = serde_json::to_string_pretty(&tool_result).unwrap_or_default();
                        let preview = if result_str.len() > 200 {
                            format!("{}... ({} bytes)", &result_str[..200], result_str.len())
                        } else {
                            result_str
                        };
                        log::info!("  ✅ Result: {}", preview);
                    }

                    function_responses.push(json!({
                        "functionResponse": {
                            "name": tool_name,
                            "response": tool_result
                        }
                    }));
                }
            }

            if !has_function_calls {
                // Agent finished - extract final text and git diff
                let final_text = parts
                    .iter()
                    .find_map(|p| p.get("text").and_then(|t| t.as_str()))
                    .unwrap_or("");

                if is_verbose() {
                    log::info!("\n  💬 Agent says: {}", final_text);
                }

                // Capture git diff from agent's layer
                let git_diff = self.capture_git_diff(agent).await.ok();

                if is_verbose() {
                    if let Some(ref diff) = git_diff {
                        log::info!("\n  📝 DIFF GENERATED ({} bytes):", diff.len());
                        log::info!("  ┌─────────────────────────────────────────");
                        for line in diff.lines().take(20) {
                            log::info!("  │ {}", line);
                        }
                        if diff.lines().count() > 20 {
                            log::info!("  │ ... ({} more lines)", diff.lines().count() - 20);
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

            // Add function responses to conversation
            if !function_responses.is_empty() {
                conversation.push(json!({
                    "role": "function",
                    "parts": function_responses
                }));
            }
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
        let result =
            super::invoke_tool_via_socket(&self.socket_path, sandbox_name, container_id, tool_name, tool_args)?;

        if let Some(error) = result.get("error") {
            return Err(anyhow!("Tool error: {}", error));
        }

        Ok(result)
    }

    /// Capture git diff from agent layer via socket API
    async fn capture_git_diff(&self, agent: &AgentHandle) -> Result<String> {
        // CRITICAL: Git commands must run INSIDE the container, not on host
        // The git repo was initialized inside the container via chroot,
        // so we must use the socket API to run git commands inside the container

        let sandbox_id = &agent.sandbox_name; // This is actually sandbox ID
        let container_id = &agent.container_id;

        // Step 1: Stage all changes (git add -A)
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

        // Step 2: Get diff against last commit
        // Use --cached to see staged changes (since we just ran git add -A)
        let diff_request = json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "name": "run_command",
            "input": {
                "cmd": "cd /workspace && git diff --cached HEAD",
                "timeout": 60
            }
        });

        let diff_response = super::socket_roundtrip(&self.socket_path, &diff_request)?;

        // Extract diff from run_command result (stdout field)
        if let Some(result) = diff_response.get("result") {
            if let Some(stdout) = result.get("stdout").and_then(|o| o.as_str()) {
                if !stdout.trim().is_empty() {
                    if is_verbose() {
                        log::info!("  ✅ Captured {} bytes of diff", stdout.len());
                    }
                    return Ok(stdout.to_string());
                }
            }

            // Check for errors
            if let Some(stderr) = result.get("stderr").and_then(|s| s.as_str()) {
                if !stderr.trim().is_empty() && stderr.contains("fatal") {
                    return Err(anyhow!("git diff failed: {}", stderr));
                }
            }
        }

        // No diff found (agent made no changes)
        if is_verbose() {
            log::info!("  ℹ️  No changes detected (empty diff)");
        }
        Ok(String::new())
    }
}

/// Build tool declarations for Gemini
fn build_tool_declarations(tools: &[String]) -> Vec<Value> {
    let all_tools = super::gemini_tool_declarations();
    let empty_vec = vec![];
    let all_tools_array = all_tools.as_array().unwrap_or(&empty_vec);

    all_tools_array
        .iter()
        .filter(|tool| {
            tool.get("name")
                .and_then(|n| n.as_str())
                .map(|name| tools.contains(&name.to_string()))
                .unwrap_or(false)
        })
        .cloned()
        .collect()
}

/// Get current Unix timestamp
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
    fn test_build_tool_declarations() {
        let tools = vec!["read_file".to_string(), "write_file".to_string()];
        let declarations = build_tool_declarations(&tools);
        assert_eq!(declarations.len(), 2);
    }
}
