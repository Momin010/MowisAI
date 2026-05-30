//! Internal types for new 7-layer orchestration system

use agentd_protocol::{SandboxName, AgentHandle};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use tokio::sync::RwLock;

/// Dependency counter for scheduler (Layer 3)
#[derive(Debug)]
pub struct DepCounter {
    pub count: AtomicUsize,
}

impl DepCounter {
    pub fn new(count: usize) -> Self {
        Self {
            count: AtomicUsize::new(count),
        }
    }
}

/// Sandbox state tracker (Layer 2)
#[derive(Debug, Clone)]
pub struct SandboxState {
    pub name: SandboxName,
    pub base_layer_path: String,
    pub sandbox_layer_path: String,
    pub scope: String,
    pub tools: Vec<String>,
    pub max_agents: u32,
    pub active_agents: u32,
    pub idle_agents: Vec<AgentHandle>,
}

/// Agent pool for sandbox (Layer 2)
#[derive(Debug)]
pub struct AgentPool {
    pub agents: RwLock<Vec<AgentHandle>>,
    pub max_size: u32,
}

impl AgentPool {
    pub fn new(max_size: u32) -> Self {
        Self {
            agents: RwLock::new(Vec::new()),
            max_size,
        }
    }

    pub async fn take_idle(&self) -> Option<AgentHandle> {
        let mut agents = self.agents.write().await;
        agents.pop()
    }

    pub async fn return_idle(&self, agent: AgentHandle) {
        let mut agents = self.agents.write().await;
        if (agents.len() as u32) < self.max_size {
            agents.push(agent);
        }
    }

    pub async fn size(&self) -> usize {
        self.agents.read().await.len()
    }
}

/// Merge tree node for parallel merge (Layer 5)
#[derive(Debug, Clone)]
pub enum MergeNode {
    Leaf { diff: String },
    Branch { left: Box<MergeNode>, right: Box<MergeNode> },
}

/// Verification test task (Layer 6)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationTask {
    pub test_id: String,
    pub description: String,
    pub command: String,
    pub expected_result: Option<String>,
}

/// Verification function (Layer 6) - planned once, executed every round
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationFunction {
    pub id: String,
    pub description: String,
    pub command: String,
    pub expected_schema: Option<String>,
    pub assertion: Option<String>,
    pub timeout_secs: u64,
}

/// Result of running a verification function
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfResult {
    pub vf_id: String,
    pub passed: bool,
    pub output: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
}

/// Fix task generated from verification failure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixTask {
    pub id: String,
    pub description: String,
    pub target_sandbox: SandboxName,
    pub related_vf_id: String,
    pub failure_output: String,
}

/// Merge result (Layer 5)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeResult {
    pub success: bool,
    pub merged_diff: String,
    pub conflicts: Vec<MergeConflict>,
    pub strategy_used: MergeStrategy,
}

/// Merge conflict information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeConflict {
    pub file_path: String,
    pub description: String,
    pub severity: ConflictSeverity,
    pub resolved: bool,
    pub resolution: Option<String>,
}

/// Conflict severity levels
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConflictSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Merge strategies
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MergeStrategy {
    Auto,
    Manual,
    LlmAssisted,
}

/// Agent contribution to a merge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContribution {
    pub agent_id: String,
    pub task_id: String,
    pub diff: String,
    pub files_changed: Vec<String>,
    pub lines_added: u32,
    pub lines_removed: u32,
}

/// Health status of an agent
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentHealth {
    Healthy,
    Degraded,
    Unresponsive,
    Failed,
}

/// Circuit breaker states
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

/// Project context for interactive orchestration sessions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectContext {
    pub project_id: String,
    pub description: String,
    pub languages: Vec<String>,
    pub frameworks: Vec<String>,
    pub notes: Vec<String>,
}

/// Warm sandbox state for session persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxWarmState {
    pub sandbox_id: String,
    pub container_id: Option<String>,
    pub paused_at: u64,
}
