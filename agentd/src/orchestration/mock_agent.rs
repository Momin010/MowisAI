//! Mock agent executor for testing without LLM API calls
//!
//! This module provides a simulated agent that behaves exactly like a real agent
//! but uses deterministic pre-defined workflows instead of calling Gemini.
//! Perfect for testing the entire orchestration stack for $0 cost.

use super::sandbox_topology::TopologyManager;
use agentd_protocol::{AgentHandle, AgentResult, Checkpoint};
use anyhow::Result;
use serde_json::json;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Mock agent executor for testing
pub struct MockAgentExecutor {
    /// Optional failure rate (0.0 - 1.0) for testing error handling
    failure_rate: f64,
    /// Delay per tool call in milliseconds (simulates real work)
    tool_delay_ms: u64,
    /// Verbose mode
    verbose: bool,
}

impl MockAgentExecutor {
    /// Create new mock agent executor
    pub fn new(failure_rate: f64, tool_delay_ms: u64, verbose: bool, checkpoint_root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&checkpoint_root)?;
        Ok(Self {
            failure_rate,
            tool_delay_ms,
            verbose,
        })
    }

    /// Execute task with mock agent
    pub async fn execute_task(
        &self,
        agent: &AgentHandle,
        task_index: usize,
        total_tasks: usize,
        topology: &TopologyManager,
    ) -> Result<AgentResult> {
        if self.verbose {
            log::info!("\n┌─────────────────────────────────────────────────────────");
            log::info!("│ 🤖 MOCK AGENT: {}", &agent.agent_id[..8]);
            log::info!("│ 📋 TASK: {} / {}", task_index + 1, total_tasks);
            log::info!("└─────────────────────────────────────────────────────────");
        }

        let sandbox_id = &agent.sandbox_name;
        let container_id = &agent.container_id;

        // Simulate tool calls
        let mut checkpoints = Vec::new();

        // Tool 1: Create a file
        tokio::time::sleep(tokio::time::Duration::from_millis(self.tool_delay_ms)).await;

        let file_content = format!(
            "// File created by mock agent {}\n// Task: {} / {}\n// Timestamp: {}\n\nconsole.log('Hello from mock agent!');\nconsole.log('Task {} completed successfully');\n",
            &agent.agent_id[..8],
            task_index + 1,
            total_tasks,
            current_timestamp(),
            task_index + 1
        );

        let write_request = json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "name": "write_file",
            "input": {
                "path": format!("/workspace/file_{}.js", task_index),
                "content": file_content
            }
        });

        let write_result = super::socket_roundtrip(topology.socket_path(), &write_request)?;

        if self.verbose {
            log::info!("  🔧 Mock tool: write_file /workspace/file_{}.js", task_index);
            log::info!("  ✅ Success");
        }

        checkpoints.push(Checkpoint {
            id: 0,
            tool_call: "write_file".to_string(),
            tool_args: json!({ "path": format!("/workspace/file_{}.js", task_index) }),
            tool_result: serde_json::to_string(&write_result)?,
            timestamp: current_timestamp(),
            layer_snapshot_path: format!("/tmp/checkpoint-{}-0", agent.agent_id),
        });

        // Tool 2: Run ls command
        tokio::time::sleep(tokio::time::Duration::from_millis(self.tool_delay_ms)).await;

        let ls_request = json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "name": "run_command",
            "input": {
                "cmd": "cd /workspace && ls -la",
                "timeout": 10
            }
        });

        let ls_result = super::socket_roundtrip(topology.socket_path(), &ls_request)?;

        if self.verbose {
            log::info!("  🔧 Mock tool: run_command ls -la");
            log::info!("  ✅ Success");
        }

        checkpoints.push(Checkpoint {
            id: 1,
            tool_call: "run_command".to_string(),
            tool_args: json!({ "cmd": "cd /workspace && ls -la" }),
            tool_result: serde_json::to_string(&ls_result)?,
            timestamp: current_timestamp(),
            layer_snapshot_path: format!("/tmp/checkpoint-{}-1", agent.agent_id),
        });

        // Random failure if configured
        if rand::random::<f64>() < self.failure_rate {
            return Ok(AgentResult {
                task_id: agent.task_id.clone().unwrap_or_default(),
                success: false,
                git_diff: None,
                error: Some("Simulated random failure".to_string()),
                checkpoint_log: checkpoints,
                timestamp: current_timestamp(),
            });
        }

        // Capture git diff
        let git_diff = self.capture_git_diff(agent, topology).await?;

        if self.verbose {
            if !git_diff.is_empty() {
                log::info!("\n  📝 DIFF GENERATED ({} bytes):", git_diff.len());
                log::info!("  ┌─────────────────────────────────────────");
                for line in git_diff.lines().take(10) {
                    log::info!("  │ {}", line);
                }
                log::info!("  └─────────────────────────────────────────");
            } else {
                log::info!("\n  ℹ️  No changes detected");
            }
        }

        Ok(AgentResult {
            task_id: agent.task_id.clone().unwrap_or_default(),
            success: true,
            git_diff: Some(git_diff),
            error: None,
            checkpoint_log: checkpoints,
            timestamp: current_timestamp(),
        })
    }

    /// Execute a verification test task — reads workspace files and returns
    /// pass/fail based on `failure_rate`. Used by `SimulatedVerificationLoop`.
    pub async fn execute_verification_task(
        &self,
        agent: &AgentHandle,
        test_description: &str,
        topology: &TopologyManager,
        failure_rate: f64,
    ) -> Result<AgentResult> {
        if self.verbose {
            log::info!(
                "  [VERIFY] Mock test agent {}: {}",
                &agent.agent_id[..8],
                test_description
            );
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(self.tool_delay_ms)).await;

        // Simulate reading files from the sandbox to verify they exist
        let ls_request = serde_json::json!({
            "request_type": "invoke_tool",
            "sandbox": &agent.sandbox_name,
            "container": &agent.container_id,
            "name": "run_command",
            "input": {
                "cmd": "cd /workspace && ls -la 2>&1 || true",
                "timeout": 10
            }
        });

        let _ls_result = super::socket_roundtrip(topology.socket_path(), &ls_request)?;

        if self.verbose {
            log::info!("  [VERIFY]   → ls /workspace: ok");
        }

        // Determine pass/fail based on failure_rate
        if rand::random::<f64>() < failure_rate {
            log::info!(
                "  [VERIFY]   → Test FAILED (simulated, rate={:.0}%)",
                failure_rate * 100.0
            );
            return Ok(AgentResult {
                task_id: agent.task_id.clone().unwrap_or_default(),
                success: false,
                git_diff: None,
                error: Some(format!(
                    "Simulated test failure for: {}",
                    test_description
                )),
                checkpoint_log: vec![],
                timestamp: current_timestamp(),
            });
        }

        log::info!("  [VERIFY]   → Test PASSED");
        Ok(AgentResult {
            task_id: agent.task_id.clone().unwrap_or_default(),
            success: true,
            git_diff: None,
            error: None,
            checkpoint_log: vec![],
            timestamp: current_timestamp(),
        })
    }

    /// Execute a fix task — writes a mock fix file and returns success.
    pub async fn execute_fix_task(
        &self,
        agent: &AgentHandle,
        fix_description: &str,
        topology: &TopologyManager,
    ) -> Result<AgentResult> {
        if self.verbose {
            log::info!(
                "  [VERIFY] Mock fix agent {}: {}",
                &agent.agent_id[..8],
                fix_description
            );
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(self.tool_delay_ms)).await;

        // Write a mock fix marker file so the sandbox has evidence of fix application
        let fix_content = format!(
            "// Fix applied by mock agent {}\n// Fix: {}\n// Timestamp: {}\n",
            &agent.agent_id[..8],
            fix_description,
            current_timestamp()
        );

        let write_request = serde_json::json!({
            "request_type": "invoke_tool",
            "sandbox": &agent.sandbox_name,
            "container": &agent.container_id,
            "name": "write_file",
            "input": {
                "path": format!("/workspace/.fix_{}.txt", &agent.agent_id[..8]),
                "content": fix_content
            }
        });

        let _write_result = super::socket_roundtrip(topology.socket_path(), &write_request)?;

        if self.verbose {
            log::info!("  [VERIFY]   → Fix applied successfully");
        }

        Ok(AgentResult {
            task_id: agent.task_id.clone().unwrap_or_default(),
            success: true,
            git_diff: Some(format!(
                "+++ fix applied: {}\n+{}\n",
                fix_description, fix_content
            )),
            error: None,
            checkpoint_log: vec![],
            timestamp: current_timestamp(),
        })
    }

    /// Capture git diff from agent layer
    async fn capture_git_diff(&self, agent: &AgentHandle, topology: &TopologyManager) -> Result<String> {
        let sandbox_id = &agent.sandbox_name;
        let container_id = &agent.container_id;

        // Stage all changes
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

        super::socket_roundtrip(topology.socket_path(), &add_request)
            .map_err(|e| anyhow::anyhow!("Failed to stage changes: {}", e))?;

        // Get diff
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

        let diff_response = super::socket_roundtrip(topology.socket_path(), &diff_request)
            .map_err(|e| anyhow::anyhow!("Failed to get diff: {}", e))?;

        if let Some(result) = diff_response.get("result") {
            if let Some(stdout) = result.get("stdout").and_then(|o| o.as_str()) {
                return Ok(stdout.to_string());
            }
        }

        Ok(String::new())
    }
}

/// Get current Unix timestamp
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}