/// Worker Agent
///
/// A worker agent is a specialized task executor that:
/// - Receives assignments from Local Hub Agent
/// - Makes LLM calls (Claude API) to reason about tasks
/// - Invokes tools via agentd socket
/// - Tests its own work
/// - Reports completion back to Hub Agent
/// - Signals idle state to Runtime

use crate::protocol::*;
use runtime::agentd_client::{AgentdClient, InvokeToolParams};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Errors for Worker Agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkerError {
    AssignmentFailed(String),
    LlmCallFailed(String),
    ToolInvocationFailed(String),
    TestFailed(String),
    InvalidState(String),
}

pub type WorkerResult<T> = Result<T, WorkerError>;

/// Configuration for Worker Agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerConfig {
    pub worker_name: String,
    pub team_id: String,
    pub sandbox_id: String,
    pub container_id: String,
    pub agentd_socket: String, // path to agentd unix socket
    pub hub_agent_socket: String, // path to hub agent socket for callbacks
    pub api_key: String, // Claude API key (for LLM calls)
}

/// Worker agent execution state
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkerExecutionState {
    Idle,
    Assigned,
    Thinking,
    ExecutingTool,
    Testing,
    Completed,
    Failed,
}

/// Tool call record from worker execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: serde_json::Value,
    pub success: bool,
    pub timestamp: u64,
}

/// Planning step in worker's reasoning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningStep {
    pub step_number: usize,
    pub reasoning: String,
    pub action: String,
    pub expected_outcome: String,
    pub timestamp: u64,
}

/// Worker Agent instance
pub struct WorkerAgent {
    config: WorkerConfig,
    client: AgentdClient,
    state: std::sync::Mutex<WorkerExecutionState>,
    current_assignment: std::sync::Mutex<Option<WorkerAssignment>>,
    execution_history: std::sync::Mutex<Vec<ToolCallRecord>>,
    planning_steps: std::sync::Mutex<Vec<PlanningStep>>,
    work_output: std::sync::Mutex<Option<serde_json::Value>>,
}

impl WorkerAgent {
    /// Create a new Worker Agent
    pub fn new(config: WorkerConfig) -> Self {
        let client = AgentdClient::new(config.agentd_socket.clone());
        WorkerAgent {
            config,
            client,
            state: std::sync::Mutex::new(WorkerExecutionState::Idle),
            current_assignment: std::sync::Mutex::new(None),
            execution_history: std::sync::Mutex::new(Vec::new()),
            planning_steps: std::sync::Mutex::new(Vec::new()),
            work_output: std::sync::Mutex::new(None),
        }
    }

    /// Receive and accept an assignment
    pub fn receive_assignment(&self, assignment: WorkerAssignment) -> WorkerResult<()> {
        if *self.state.lock().unwrap() != WorkerExecutionState::Idle {
            return Err(WorkerError::InvalidState(
                "Worker not idle".to_string(),
            ));
        }

        *self.current_assignment.lock().unwrap() = Some(assignment);
        *self.state.lock().unwrap() = WorkerExecutionState::Assigned;
        Ok(())
    }

    /// Execute the assigned task
    /// In a real implementation, this would call Claude API
    pub fn execute_task(&self) -> WorkerResult<()> {
        let assignment = self
            .current_assignment
            .lock()
            .unwrap()
            .clone()
            .ok_or(WorkerError::AssignmentFailed("No assignment".to_string()))?;

        // Phase 1: Thinking/Planning
        *self.state.lock().unwrap() = WorkerExecutionState::Thinking;
        self.plan_task(&assignment)?;

        // Phase 2: Execute planned steps
        *self.state.lock().unwrap() = WorkerExecutionState::ExecutingTool;
        self.execute_plan(&assignment)?;

        // Phase 3: Test work
        *self.state.lock().unwrap() = WorkerExecutionState::Testing;
        self.test_work()?;

        *self.state.lock().unwrap() = WorkerExecutionState::Completed;
        Ok(())
    }

    /// Plan the task based on assignment description
    fn plan_task(&self, _assignment: &WorkerAssignment) -> WorkerResult<()> {
        // In a real implementation, this would call Claude to generate a plan
        // For now, we create a simple mock plan

        let steps = vec![
            PlanningStep {
                step_number: 1,
                reasoning: "Read the task description carefully".to_string(),
                action: "Understand requirements".to_string(),
                expected_outcome: "Clear understanding of what needs to be done".to_string(),
                timestamp: current_timestamp(),
            },
            PlanningStep {
                step_number: 2,
                reasoning: "Break down task into executable steps".to_string(),
                action: "Create execution plan".to_string(),
                expected_outcome: "Ordered list of steps to complete".to_string(),
                timestamp: current_timestamp(),
            },
            PlanningStep {
                step_number: 3,
                reasoning: "Execute each step using available tools".to_string(),
                action: "Tool execution".to_string(),
                expected_outcome: "Completed work output".to_string(),
                timestamp: current_timestamp(),
            },
        ];

        *self.planning_steps.lock().unwrap() = steps;
        Ok(())
    }

    /// Execute the planned steps
    fn execute_plan(&self, assignment: &WorkerAssignment) -> WorkerResult<()> {
        let file_contents = self.invoke_file_operation("read", "/task/requirements.md")?;

        let code_output = self.invoke_code_execution(&assignment.task_description)?;

        let mut output = serde_json::json!({
            "task": assignment.task_description,
            "worker": assignment.worker_name,
            "status": "completed",
            "timestamp": current_timestamp(),
            "code_output": code_output,
            "file_contents": file_contents,
        });

        if assignment.tools_available.contains(&"git".to_string()) {
            let git_status = self.invoke_git_operation()?;
            if let Some(obj) = output.as_object_mut() {
                obj.insert("git_status".to_string(), git_status);
            }
        }

        *self.work_output.lock().unwrap() = Some(output);
        Ok(())
    }

    /// Test the completed work
    fn test_work(&self) -> WorkerResult<()> {
        let output = self
            .work_output
            .lock()
            .unwrap()
            .clone()
            .ok_or(WorkerError::TestFailed("No output to test".to_string()))?;

        // Validate output is not null/empty
        if output.is_null() || (output.is_object() && output.as_object().unwrap().is_empty()) {
            return Err(WorkerError::TestFailed("Output is empty".to_string()));
        }

        // Validate status is "completed"
        let status = output
            .get("status")
            .and_then(|v| v.as_str())
            .ok_or(WorkerError::TestFailed("Invalid status field".to_string()))?;

        if status != "completed" {
            return Err(WorkerError::TestFailed(format!("Status is {}", status)));
        }

        Ok(())
    }

    /// Create a completion report
    pub fn create_completion(&self) -> WorkerResult<WorkerCompletion> {
        let assignment = self
            .current_assignment
            .lock()
            .unwrap()
            .clone()
            .ok_or(WorkerError::InvalidState("No assignment".to_string()))?;

        let state = *self.state.lock().unwrap();
        let success = state == WorkerExecutionState::Completed;

        let output = self
            .work_output
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(serde_json::json!({}));

        Ok(WorkerCompletion {
            assignment_id: assignment.assignment_id,
            worker_name: self.config.worker_name.clone(),
            success,
            output,
            errors: vec![], // Would populate with actual errors
            timestamp: current_timestamp(),
        })
    }

    /// Signal idle state to Runtime
    pub fn signal_idle(&self) -> WorkerResult<WorkerIdleSignal> {
        *self.state.lock().unwrap() = WorkerExecutionState::Idle;
        *self.current_assignment.lock().unwrap() = None;
        *self.work_output.lock().unwrap() = None;

        Ok(WorkerIdleSignal {
            worker_name: self.config.worker_name.clone(),
            container_id: self.config.container_id.clone(),
            sandbox_id: self.config.sandbox_id.clone(),
            timestamp: current_timestamp(),
        })
    }

    /// Get current execution state
    pub fn get_state(&self) -> WorkerExecutionState {
        *self.state.lock().unwrap()
    }

    /// Get execution history
    pub fn get_history(&self) -> Vec<ToolCallRecord> {
        self.execution_history.lock().unwrap().clone()
    }

    /// Get planning steps
    pub fn get_planning_steps(&self) -> Vec<PlanningStep> {
        self.planning_steps.lock().unwrap().clone()
    }

    // Real tool invocation methods (via agentd)

    fn invoke_tool_via_agentd(
        &self,
        tool_name: &str,
        input: serde_json::Value,
    ) -> WorkerResult<serde_json::Value> {
        let params = InvokeToolParams {
            sandbox_id: self.config.sandbox_id.clone(),
            container_id: self.config.container_id.clone(),
            tool_name: tool_name.to_string(),
            input: input.clone(),
        };

        match self.client.invoke_tool(params) {
            Ok(response) => {
                let record = ToolCallRecord {
                    tool_name: tool_name.to_string(),
                    input,
                    output: response.output.clone(),
                    success: response.exit_code == 0,
                    timestamp: current_timestamp(),
                };

                self.execution_history.lock().unwrap().push(record);
                Ok(response.output)
            }
            Err(e) => Err(WorkerError::ToolInvocationFailed(format!(
                "Failed to invoke {}: {:?}",
                tool_name, e
            ))),
        }
    }

    fn invoke_file_operation(
        &self,
        operation: &str,
        path: &str,
    ) -> WorkerResult<serde_json::Value> {
        let input = serde_json::json!({
            "operation": operation,
            "path": path
        });

        self.invoke_tool_via_agentd("filesystem", input)
    }

    fn invoke_code_execution(&self, task: &str) -> WorkerResult<serde_json::Value> {
        let input = serde_json::json!({
            "command": task
        });

        self.invoke_tool_via_agentd("shell", input)
    }

    fn invoke_git_operation(&self) -> WorkerResult<serde_json::Value> {
        let input = serde_json::json!({
            "command": "status"
        });

        self.invoke_tool_via_agentd("git", input)
    }
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
    fn test_worker_creation() {
        let config = WorkerConfig {
            worker_name: "Jake".to_string(),
            team_id: "team-1".to_string(),
            sandbox_id: "sb-1".to_string(),
            container_id: "c1".to_string(),
            agentd_socket: "/tmp/agentd.sock".to_string(),
            hub_agent_socket: "/tmp/hub.sock".to_string(),
            api_key: "test-key".to_string(),
        };

        let worker = WorkerAgent::new(config);
        assert_eq!(worker.get_state(), WorkerExecutionState::Idle);
    }

    #[test]
    fn test_state_transitions() {
        // Unit test: tests state transitions without calling real socket operations
        let config = WorkerConfig {
            worker_name: "Jake".to_string(),
            team_id: "team-1".to_string(),
            sandbox_id: "sb-1".to_string(),
            container_id: "c1".to_string(),
            agentd_socket: "/tmp/agentd.sock".to_string(),
            hub_agent_socket: "/tmp/hub.sock".to_string(),
            api_key: "test-key".to_string(),
        };

        let worker = WorkerAgent::new(config);
        assert_eq!(worker.get_state(), WorkerExecutionState::Idle);

        let assignment = WorkerAssignment {
            assignment_id: "a1".to_string(),
            worker_name: "Jake".to_string(),
            task_description: "Build a web server".to_string(),
            system_prompt: "You are an expert developer".to_string(),
            tools_available: vec!["shell".to_string(), "filesystem".to_string()],
            timeout_secs: 3600,
            context: serde_json::json!({}),
        };

        worker.receive_assignment(assignment).unwrap();
        assert_eq!(worker.get_state(), WorkerExecutionState::Assigned);

        // Verify assignment was stored
        assert!(worker.current_assignment.lock().unwrap().is_some());
    }

    #[test]
    #[ignore]
    #[cfg(feature = "integration")]
    fn test_task_execution_integration() {
        // Integration test: requires real agentd running at /tmp/agentd.sock
        // Run with: cargo test --features integration -- --ignored
        let config = WorkerConfig {
            worker_name: "Jake".to_string(),
            team_id: "team-1".to_string(),
            sandbox_id: "sb-1".to_string(),
            container_id: "c1".to_string(),
            agentd_socket: "/tmp/agentd.sock".to_string(),
            hub_agent_socket: "/tmp/hub.sock".to_string(),
            api_key: "test-key".to_string(),
        };

        let worker = WorkerAgent::new(config);

        let assignment = WorkerAssignment {
            assignment_id: "a1".to_string(),
            worker_name: "Jake".to_string(),
            task_description: "Build a web server".to_string(),
            system_prompt: "You are an expert developer".to_string(),
            tools_available: vec!["shell".to_string(), "filesystem".to_string()],
            timeout_secs: 3600,
            context: serde_json::json!({}),
        };

        worker.receive_assignment(assignment).unwrap();
        assert_eq!(worker.get_state(), WorkerExecutionState::Assigned);

        worker.execute_task().unwrap();
        assert_eq!(worker.get_state(), WorkerExecutionState::Completed);

        let completion = worker.create_completion().unwrap();
        assert!(completion.success);
    }

    #[test]
    fn test_idle_signal() {
        let config = WorkerConfig {
            worker_name: "Mike".to_string(),
            team_id: "team-1".to_string(),
            sandbox_id: "sb-1".to_string(),
            container_id: "c2".to_string(),
            agentd_socket: "/tmp/agentd.sock".to_string(),
            hub_agent_socket: "/tmp/hub.sock".to_string(),
            api_key: "test-key".to_string(),
        };

        let worker = WorkerAgent::new(config);

        let signal = worker.signal_idle().unwrap();
        assert_eq!(signal.worker_name, "Mike");
        assert_eq!(worker.get_state(), WorkerExecutionState::Idle);
    }
}
