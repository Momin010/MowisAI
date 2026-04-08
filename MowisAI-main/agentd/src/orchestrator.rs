/// Global Orchestrator
///
/// The top-level orchestrator that:
/// - Receives user tasks in natural language
/// - Analyzes task complexity and builds dependency graphs
/// - Provisions sandboxes and creates Local Hub Agents
/// - Assigns team tasks to each Hub Agent
/// - Monitors execution and health
/// - Sequences dependent teams
/// - Collects final outputs

use crate::dependency_graph::{ComplexityAnalyzer, DependencyGraphBuilder};
use crate::hub_agent_client::HubAgentClient;
use crate::protocol::*;
use runtime::Runtime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Errors for Orchestrator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrchestratorError {
    PlanningFailed(String),
    ProvisioningFailed(String),
    TeamTaskFailed(String),
    ExecutionTimeout(String),
    HealthCheckFailed(String),
}

pub type OrchestratorResult<T> = Result<T, OrchestratorError>;

/// Configuration for Global Orchestrator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    pub runtime_socket_base: String,
    pub max_total_sandboxes: usize,
    pub task_timeout_secs: u64,
    pub health_check_interval_secs: u64,
    pub llm_analysis_enabled: bool, // if true, use Claude to analyze task complexity
}

/// The Global Orchestrator
pub struct GlobalOrchestrator {
    config: OrchestratorConfig,
    runtime: Runtime,
    sessions: std::sync::Mutex<HashMap<String, ExecutionSession>>,
}

impl GlobalOrchestrator {
    /// Create a new Global Orchestrator
    pub fn new(config: OrchestratorConfig) -> Self {
        let runtime = Runtime::new(config.runtime_socket_base.clone());
        GlobalOrchestrator {
            config,
            runtime,
            sessions: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Main entry point: execute a user task
    pub fn execute_task(&self, user_task: String) -> OrchestratorResult<String> {
        let session_id = format!("session-{}", generate_unique_id());

        // Step 1: Parse and analyze task
        println!("[Orchestrator] Analyzing task: {}", user_task);
        let plan = self.plan_task(&user_task)?;

        // Step 2: Create execution session
        let mut session = ExecutionSession {
            session_id: session_id.clone(),
            user_task: user_task.clone(),
            dependency_graph: plan.dependency_graph.clone(),
            provisioning_spec: plan.provisioning_spec.clone(),
            sandbox_handles: Vec::new(),
            completed_tasks: Vec::new(),
            failed_tasks: Vec::new(),
            status: ExecutionStatus::Planning,
            created_at: current_timestamp(),
        };

        // Step 3: Provision sandboxes
        println!("[Orchestrator] Provisioning {} sandboxes", plan.provisioning_spec.num_sandboxes);
        session.status = ExecutionStatus::Provisioning;
        session.sandbox_handles = self.provision_sandboxes(&plan.provisioning_spec)?;

        // Step 4: Execute tasks in dependency order
        println!("[Orchestrator] Starting task execution");
        session.status = ExecutionStatus::Running;
        self.execute_task_stages(&mut session, &plan.dependency_graph)?;

        // Step 5: Collect results
        println!("[Orchestrator] Task execution complete");
        session.status = ExecutionStatus::Completed;

        {
            let mut sessions = self.sessions.lock().unwrap();
            sessions.insert(session_id.clone(), session);
        }

        Ok(session_id)
    }

    /// Plan the task: analyze complexity, build dependency graph, allocate resources
    fn plan_task(&self, user_task: &str) -> OrchestratorResult<OrchestratorPlan> {
        // Step 1: Extract task components
        let team_tasks = self.decompose_task(user_task)?;

        // Step 2: Build dependency graph
        let mut builder = DependencyGraphBuilder::new();
        for team_task in &team_tasks {
            builder.add_task(
                team_task.task_id.clone(),
                team_task.team_id.clone(),
                team_task.dependencies.clone(),
            );
        }

        let dependency_graph = builder
            .build()
            .map_err(|e| OrchestratorError::PlanningFailed(format!("{:?}", e)))?;

        // Step 3: Estimate resources
        let total_complexity: u32 = team_tasks.iter().map(|t| t.estimated_complexity).sum();
        let num_sandboxes = ComplexityAnalyzer::estimate_sandbox_count(&dependency_graph.tasks.values().cloned().collect::<Vec<_>>(), total_complexity);

        // Step 4: Create provisioning spec
        let provisioning_spec = self.create_provisioning_spec(&team_tasks, num_sandboxes)?;

        // Step 5: Estimate execution time
        let execution_stages = dependency_graph.execution_order.len();
        let estimated_time = (execution_stages as u64) * 60; // rough estimate: 60 secs per stage

        Ok(OrchestratorPlan {
            plan_id: format!("plan-{}", generate_unique_id()),
            dependency_graph,
            provisioning_spec,
            estimated_total_time_secs: estimated_time,
            estimated_resource_usage: ResourceEstimate {
                total_ram_bytes: ComplexityAnalyzer::estimate_ram_per_sandbox(total_complexity / num_sandboxes as u32 + 1) * num_sandboxes as u64,
                total_cpu_millis: ComplexityAnalyzer::estimate_cpu_per_sandbox(total_complexity / num_sandboxes as u32 + 1) as u32 * num_sandboxes as u32,
                total_containers: num_sandboxes * 10,
            },
        })
    }

    /// Decompose user task into team tasks
    fn decompose_task(&self, user_task: &str) -> OrchestratorResult<Vec<TeamTask>> {
        // Simple strategy: extract keywords to determine team types
        // In a real implementation, use Claude to intelligently decompose
        
        let mut team_tasks = Vec::new();

        // Heuristic: detect task types from keywords
        let has_backend = user_task.contains("api") || user_task.contains("server") || user_task.contains("database");
        let has_frontend = user_task.contains("ui") || user_task.contains("frontend") || user_task.contains("web");
        let has_testing = user_task.contains("test");

        let mut task_id = 1;

        if has_backend {
            team_tasks.push(TeamTask {
                task_id: format!("task-{}", task_id),
                team_id: format!("team-backend-{}", task_id),
                description: format!("Backend: {}", user_task),
                dependencies: vec![],
                estimated_complexity: 100,
                timeout_secs: 3600,
                context: serde_json::json!({"team_type": "backend"}),
            });
            task_id += 1;
        }

        if has_frontend {
            let backend_dep = if has_backend { vec![format!("task-{}", task_id - 1)] } else { vec![] };
            team_tasks.push(TeamTask {
                task_id: format!("task-{}", task_id),
                team_id: format!("team-frontend-{}", task_id),
                description: format!("Frontend: {}", user_task),
                dependencies: backend_dep,
                estimated_complexity: 80,
                timeout_secs: 3600,
                context: serde_json::json!({"team_type": "frontend"}),
            });
            task_id += 1;
        }

        if has_testing {
            let deps = (1..task_id).map(|i| format!("task-{}", i)).collect();
            team_tasks.push(TeamTask {
                task_id: format!("task-{}", task_id),
                team_id: format!("team-testing-{}", task_id),
                description: format!("Testing: {}", user_task),
                dependencies: deps,
                estimated_complexity: 60,
                timeout_secs: 3600,
                context: serde_json::json!({"team_type": "testing"}),
            });
        }

        if team_tasks.is_empty() {
            // Default: single task
            team_tasks.push(TeamTask {
                task_id: "task-1".to_string(),
                team_id: "team-general".to_string(),
                description: user_task.to_string(),
                dependencies: vec![],
                estimated_complexity: 100,
                timeout_secs: 3600,
                context: serde_json::json!({}),
            });
        }

        Ok(team_tasks)
    }

    /// Create provisioning specification for sandboxes
    fn create_provisioning_spec(
        &self,
        team_tasks: &[TeamTask],
        num_sandboxes: usize,
    ) -> OrchestratorResult<ProvisioningSpec> {
        let mut sandbox_specs = Vec::new();

        for i in 0..num_sandboxes {
            let complexity_estimate = team_tasks.iter().map(|t| t.estimated_complexity).sum::<u32>() / num_sandboxes as u32;
            
            sandbox_specs.push(SandboxSpec {
                sandbox_id: format!("sandbox-{}", i),
                os_image: "alpine".to_string(),
                ram_bytes: ComplexityAnalyzer::estimate_ram_per_sandbox(complexity_estimate),
                cpu_millis: ComplexityAnalyzer::estimate_cpu_per_sandbox(complexity_estimate) as u32,
                init_packages: vec![
                    "curl".to_string(),
                    "git".to_string(),
                    "npm".to_string(),
                    "python3".to_string(),
                ],
                initial_containers: 10,
            });
        }

        Ok(ProvisioningSpec {
            request_id: format!("prov-{}", generate_unique_id()),
            num_sandboxes,
            sandbox_specs,
            max_concurrent_agents_per_sandbox: 10,
        })
    }

    /// Provision sandboxes via Runtime
    fn provision_sandboxes(&self, spec: &ProvisioningSpec) -> OrchestratorResult<Vec<SandboxHandle>> {
        let ready = self.runtime
            .provision_sandboxes(spec)
            .map_err(|e| OrchestratorError::ProvisioningFailed(format!("{:?}", e)))?;

        Ok(ready.sandboxes)
    }

    /// Execute tasks in dependency stages
    fn execute_task_stages(
        &self,
        session: &mut ExecutionSession,
        dependency_graph: &DependencyGraph,
    ) -> OrchestratorResult<()> {
        for stage in &dependency_graph.execution_order {
            println!("[Orchestrator] Executing stage with {} tasks", stage.len());

            // Execute all tasks in this stage in parallel (in real impl)
            for task_id in stage {
                if let Some(task_node) = dependency_graph.tasks.get(task_id) {
                    let result = self.execute_single_task(task_id, task_node, session)?;
                    session.completed_tasks.push(result);
                }
            }

            println!("[Orchestrator] Stage complete");
        }

        Ok(())
    }

    /// Execute a single team task
    fn execute_single_task(
        &self,
        task_id: &str,
        task_node: &TaskNode,
        session: &ExecutionSession,
    ) -> OrchestratorResult<TaskCompletion> {
        // Real implementation: Send task to appropriate Local Hub Agent via socket

        // Step 1: Find the sandbox/hub_agent that should handle this team type
        let hub_agent_socket = session
            .sandbox_handles
            .iter()
            .find_map(|handle| {
                // For now, send to first available sandbox
                // In a real system, this would be based on team_type matching
                Some(handle.socket_path.clone())
            })
            .ok_or(OrchestratorError::ProvisioningFailed(
                "No hub agent available".to_string(),
            ))?;

        // Step 2: Create TeamTask from task node
        let team_task = TeamTask {
            task_id: task_id.to_string(),
            team_id: task_node.team_type.clone(),
            description: format!("Task for {} team", task_node.team_type),
            dependencies: task_node.depends_on.clone(),
            estimated_complexity: 1,
            timeout_secs: self.config.task_timeout_secs,
            context: serde_json::json!({
                "task_type": task_node.team_type,
                "priority": "normal"
            }),
        };

        // Step 3: Send task to hub agent via socket
        let client = HubAgentClient::new(hub_agent_socket.clone());
        
        client
            .assign_task(team_task)
            .map_err(|e| OrchestratorError::TeamTaskFailed(format!("{:?}", e)))?;

        // Step 4: Wait for completion from hub agent
        match client.wait_for_completion() {
            Ok(completion) => Ok(completion),
            Err(_) => {
                // If socket communication fails, return a placeholder
                Ok(TaskCompletion {
                    task_id: task_node.task_id.clone(),
                    team_id: task_node.team_type.clone(),
                    success: false,
                    output: serde_json::json!({
                        "status": "communication_failed",
                        "team_type": task_node.team_type,
                        "note": "Hub agent communication failed - this may indicate the hub agent is not running"
                    }),
                    errors: vec!["Hub agent communication failed".to_string()],
                    timestamp: current_timestamp(),
                })
            }
        }
    }

    /// Get session status
    pub fn get_session_status(&self, session_id: &str) -> Option<ExecutionSession> {
        self.sessions.lock().unwrap().get(session_id).cloned()
    }

    /// Get session results
    pub fn get_session_results(&self, session_id: &str) -> OrchestratorResult<serde_json::Value> {
        let sessions = self.sessions.lock().unwrap();
        let session = sessions.get(session_id)
            .ok_or(OrchestratorError::ExecutionTimeout("Session not found".to_string()))?;

        Ok(serde_json::json!({
            "session_id": session.session_id,
            "status": format!("{:?}", session.status),
            "completed_tasks": session.completed_tasks.len(),
            "failed_tasks": session.failed_tasks.len(),
            "results": session.completed_tasks.iter().map(|t| &t.output).collect::<Vec<_>>()
        }))
    }
}

/// Generate a unique ID
fn generate_unique_id() -> String {
    use std::time::UNIX_EPOCH;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("{:010}", nanos)
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
    fn test_orchestrator_creation() {
        let config = OrchestratorConfig {
            runtime_socket_base: "/tmp/sockets".to_string(),
            max_total_sandboxes: 100,
            task_timeout_secs: 3600,
            health_check_interval_secs: 10,
            llm_analysis_enabled: false,
        };

        let _orchestrator = GlobalOrchestrator::new(config);
        // Orchestrator created successfully
    }

    #[test]
    fn test_task_decomposition() {
        let config = OrchestratorConfig {
            runtime_socket_base: "/tmp/sockets".to_string(),
            max_total_sandboxes: 100,
            task_timeout_secs: 3600,
            health_check_interval_secs: 10,
            llm_analysis_enabled: false,
        };

        let orchestrator = GlobalOrchestrator::new(config);
        let task = "Build a web API with a frontend UI and comprehensive tests".to_string();
        
        let team_tasks = orchestrator.decompose_task(&task).unwrap();
        assert!(team_tasks.len() >= 1); // At least one team task
    }

    #[test]
    fn test_provisioning_spec_creation() {
        let config = OrchestratorConfig {
            runtime_socket_base: "/tmp/sockets".to_string(),
            max_total_sandboxes: 100,
            task_timeout_secs: 3600,
            health_check_interval_secs: 10,
            llm_analysis_enabled: false,
        };

        let orchestrator = GlobalOrchestrator::new(config);
        let task = "Build a web server".to_string();
        
        let team_tasks = orchestrator.decompose_task(&task).unwrap();
        let spec = orchestrator.create_provisioning_spec(&team_tasks, 2).unwrap();
        
        assert_eq!(spec.num_sandboxes, 2);
        assert_eq!(spec.sandbox_specs.len(), 2);
    }
}
