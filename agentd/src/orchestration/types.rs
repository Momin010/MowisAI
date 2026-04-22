//! Internal types for new 7-layer orchestration system
//! Also contains legacy types from old 5-layer system (marked DEPRECATED)

use agentd_protocol::{SandboxName, AgentHandle};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use tokio::sync::RwLock;

// ============================================================================
// DEPRECATED: OLD 5-LAYER ARCHITECTURE TYPES (kept for orchestrator.rs)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ProjectContext {
    pub file_tree: String,
    pub relevant_files: Vec<String>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ImplementationBlueprint {
    pub sandboxes: Vec<SandboxConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct SandboxConfig {
    pub name: String,
    pub scope: String,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct SandboxExecutionPlan {
    pub sandbox_id: String,
    pub tasks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct SandboxResult {
    pub sandbox_id: String,
    pub success: bool,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct SandboxWarmState {
    pub merge_container_id: String,
    pub worker_containers: HashMap<String, String>,
}

// ============================================================================
// NEW 7-LAYER ARCHITECTURE TYPES
// ============================================================================

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
    pub max_agents: usize,
    pub active_agents: usize,
    pub idle_agents: Vec<AgentHandle>,
}

/// Agent pool for sandbox (Layer 2)
#[derive(Debug)]
pub struct AgentPool {
    pub agents: RwLock<Vec<AgentHandle>>,
    pub max_size: usize,
}

impl AgentPool {
    pub fn new(max_size: usize) -> Self {
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
        agents.push(agent);
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
#[derive(Debug, Clone)]
pub struct VerificationTask {
    pub test_id: String,
    pub description: String,
    pub command: String,
    pub expected_result: String,
}
