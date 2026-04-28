/// Protocol definitions for inter-component communication
///
/// Shared between `agentd` (libagent) and `runtime` to avoid a circular dependency.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Task representation passed from Global Orchestrator to Local Hub Agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamTask {
    pub task_id: String,
    pub team_id: String,
    pub description: String,
    pub dependencies: Vec<String>, // task_ids this depends on
    pub estimated_complexity: u32,
    pub timeout_secs: u64,
    pub context: serde_json::Value, // arbitrary context for the task
}

/// Provisioning request sent from Global Orchestrator to Runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisioningSpec {
    pub request_id: String,
    pub num_sandboxes: usize,
    pub sandbox_specs: Vec<SandboxSpec>,
    pub max_concurrent_agents_per_sandbox: usize,
}

/// Specification for a single sandbox
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxSpec {
    pub sandbox_id: String,
    pub os_image: String, // "alpine" or "debian"
    pub ram_bytes: u64,
    pub cpu_millis: u32,
    pub init_packages: Vec<String>, // curl, git, npm, etc.
    pub initial_containers: usize,
}

/// Response from Runtime when provisioning is complete
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisioningReady {
    pub request_id: String,
    pub sandboxes: Vec<SandboxHandle>,
    pub timestamp: u64,
}

/// Handle to a provisioned sandbox
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxHandle {
    pub sandbox_id: String,
    pub socket_path: String, // /sandbox/{sandbox_id}/agent.sock
    pub containers: Vec<ContainerHandle>,
}

/// Handle to a container within a sandbox
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerHandle {
    pub container_id: String,
    pub sandbox_id: String,
    pub status: ContainerStatus,
}

/// Container states
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContainerStatus {
    Creating,
    Ready,
    Active,
    Paused,
    Terminated,
}

/// Health status of a sandbox
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxHealthStatus {
    pub sandbox_id: String,
    pub hub_agent_alive: bool,
    pub container_states: HashMap<String, ContainerStatus>,
    pub ram_usage_bytes: u64,
    pub cpu_usage_millis: u32,
    pub timestamp: u64,
}

/// Message sent from Local Hub Agent back to Global Orchestrator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCompletion {
    pub task_id: String,
    pub team_id: String,
    pub success: bool,
    pub output: serde_json::Value,
    pub errors: Vec<String>,
    pub timestamp: u64,
}

/// Subtask assignment from Local Hub Agent to Worker Agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerAssignment {
    pub assignment_id: String,
    pub worker_name: String, // e.g., "Jake", "Mike"
    pub task_description: String,
    pub system_prompt: String, // LLM system prompt for this worker
    pub tools_available: Vec<String>, // tool names worker can invoke
    pub timeout_secs: u64,
    pub context: serde_json::Value,
}

/// Signal from Worker Agent indicating completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerCompletion {
    pub assignment_id: String,
    pub worker_name: String,
    pub success: bool,
    pub output: serde_json::Value,
    pub errors: Vec<String>,
    pub timestamp: u64,
}

/// Signal from Worker Agent indicating idle state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerIdleSignal {
    pub worker_name: String,
    pub container_id: String,
    pub sandbox_id: String,
    pub timestamp: u64,
}

/// Request from Worker Agent to Runtime for resource increase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRequest {
    pub request_id: String,
    pub sandbox_id: String,
    pub resource_type: ResourceType,
    pub amount: u64,
}

/// Types of resources that can be requested
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResourceType {
    RAM,
    CPU,
}

/// Cross-team contract: shared API specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiContract {
    pub contract_id: String,
    pub providing_team_id: String,
    pub consuming_team_ids: Vec<String>,
    pub endpoint_spec: serde_json::Value, // e.g., {"port": 3000, "routes": [...]}
    pub schema: serde_json::Value, // Request/response schemas
    pub created_at: u64,
}

/// RPC call from one Local Hub Agent to another
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterTeamRpc {
    pub call_id: String,
    pub method: String, // e.g., "get_api_contract", "signal_deployment_ready"
    pub params: serde_json::Value,
    pub timeout_secs: u64,
}

/// RPC response between Local Hub Agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterTeamRpcResponse {
    pub call_id: String,
    pub success: bool,
    pub result: serde_json::Value,
    pub error: Option<String>,
}

/// Request to pause/resume containers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerControlRequest {
    pub sandbox_id: String,
    pub container_ids: Vec<String>,
    pub action: ContainerAction,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContainerAction {
    Pause,
    Resume,
    Terminate,
}

/// Dependency graph for task sequencing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyGraph {
    pub tasks: HashMap<String, TaskNode>,
    pub execution_order: Vec<Vec<String>>, // stages; each stage contains independent tasks
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    pub task_id: String,
    pub depends_on: Vec<String>,
    pub team_type: String, // e.g., "frontend", "backend", "data-processing"
}

// ============================================================================
// NEW 7-LAYER ORCHESTRATION TYPES
// ============================================================================

/// Task ID type for new orchestration system
pub type TaskId = String;

/// Sandbox name type
pub type SandboxName = String;

/// Task in the new task graph system (Layer 1 output)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub description: String,
    pub deps: Vec<TaskId>,
    pub hint: Option<String>, // Sandbox hint (e.g., "backend", "frontend")
}

/// Task graph produced by Fast Planner (Layer 1)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGraph {
    pub tasks: Vec<Task>,
}

/// Sandbox configuration in topology (Layer 1 output)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub name: SandboxName,
    pub scope: String, // Filesystem scope (e.g., "src/backend/")
    pub tools: Vec<String>, // Tools available to agents in this sandbox
    pub max_agents: usize,
    /// Optional OS image to pass to agentd create_sandbox (e.g. "alpine").
    /// When None, agentd creates a plain tmpfs sandbox (no image needed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

/// Sandbox topology produced by Fast Planner (Layer 1)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxTopology {
    pub sandboxes: Vec<SandboxConfig>,
}

/// Checkpoint saved after each tool call (Layer 4)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: u64,
    pub tool_call: String,
    pub tool_args: serde_json::Value,
    pub tool_result: String,
    pub timestamp: u64,
    pub layer_snapshot_path: String, // Path to CoW layer snapshot
}

/// Agent execution result (Layer 4 output)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub task_id: TaskId,
    pub success: bool,
    pub git_diff: Option<String>, // Clean git diff on success
    pub error: Option<String>,
    pub checkpoint_log: Vec<Checkpoint>,
    pub timestamp: u64,
}

/// Sandbox execution result (Layer 5 output)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    pub sandbox_name: SandboxName,
    pub success: bool,
    pub merged_diff: Option<String>, // Merged and verified diff
    pub verification_status: VerificationStatus,
    pub timestamp: u64,
}

/// Verification status (Layer 6)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum VerificationStatus {
    NotStarted,
    Running,
    Passed,
    PartiallyVerified,
    Failed,
}

/// Scheduler messages for task dispatch (Layer 3)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SchedulerMessage {
    TaskReady(TaskId),
    TaskStarted { task_id: TaskId, agent_id: String, sandbox_name: SandboxName },
    TaskCompleted { task_id: TaskId, result: AgentResult },
    TaskFailed { task_id: TaskId, error: String },
    AgentIdle { agent_id: String, sandbox_name: SandboxName },
    Shutdown,
}

/// Overlayfs layer information (Layer 2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayfsLayer {
    pub level: LayerLevel,
    pub mount_path: String,
    pub upper_dir: String,
    pub work_dir: String,
    pub lower_dirs: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum LayerLevel {
    Base,     // Level 0: Full repo, read-only
    Sandbox,  // Level 1: CoW per sandbox
    Agent,    // Level 2: CoW per agent
}

/// Agent handle for tracking running agents (Layer 2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHandle {
    pub agent_id: String,
    pub sandbox_name: SandboxName,
    pub container_id: String,
    pub task_id: Option<TaskId>,
    pub layer: OverlayfsLayer,
}

/// Session state for tracking task execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSession {
    pub session_id: String,
    pub user_task: String,
    pub dependency_graph: DependencyGraph,
    pub provisioning_spec: ProvisioningSpec,
    pub sandbox_handles: Vec<SandboxHandle>,
    pub completed_tasks: Vec<TaskCompletion>,
    pub failed_tasks: Vec<TaskCompletion>,
    pub status: ExecutionStatus,
    pub created_at: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionStatus {
    Planning,
    Provisioning,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Global Orchestrator's decision output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorPlan {
    pub plan_id: String,
    pub dependency_graph: DependencyGraph,
    pub provisioning_spec: ProvisioningSpec,
    pub estimated_total_time_secs: u64,
    pub estimated_resource_usage: ResourceEstimate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceEstimate {
    pub total_ram_bytes: u64,
    pub total_cpu_millis: u32,
    pub total_containers: usize,
}

// ============================================================================
// SOCKET PROTOCOL TYPES (for mowis-gui communication)
// ============================================================================

/// Socket request from GUI to agentd
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocketRequest {
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

/// Socket response from agentd to GUI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocketResponse {
    pub id: String,
    pub result: serde_json::Value,
}
