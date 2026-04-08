/// Local Hub Agent
///
/// Runs inside each sandbox. Responsible for:
/// - Receiving team-level task from Global Orchestrator
/// - Breaking down task into worker assignments
/// - Managing worker agent lifecycle
/// - Coordinating with peer Hub Agents via sockets
/// - Running integration tests on combined output
/// - Reporting completion back to Global Orchestrator

use crate::protocol::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use std::os::unix::net::UnixListener;
use std::io::{Read, Write};
use std::fs;

/// Errors for Hub Agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HubAgentError {
    TaskBreakdownFailed(String),
    WorkerAssignmentFailed(String),
    WorkerNotFound(String),
    IntegrationTestFailed(String),
    PeerCommunicationFailed(String),
}

pub type HubAgentResult<T> = Result<T, HubAgentError>;

/// Configuration for Local Hub Agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubAgentConfig {
    pub team_id: String,
    pub sandbox_id: String,
    pub max_workers: usize,
    pub socket_path: String,
    pub peer_sockets: HashMap<String, String>, // team_id -> socket_path
}

/// Worker entry tracked by the Hub Agent
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkerEntry {
    pub name: String,
    pub container_id: String,
    pub assignment: Option<WorkerAssignment>,
    pub completion: Option<WorkerCompletion>,
    pub status: WorkerStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum WorkerStatus {
    Idle,
    Assigned,
    Running,
    Completed,
    Failed,
}

/// Local Hub Agent instance
pub struct LocalHubAgent {
    config: HubAgentConfig,
    workers: Arc<Mutex<HashMap<String, WorkerEntry>>>,
    team_task: Arc<Mutex<Option<TeamTask>>>,
    completion: Arc<Mutex<Option<TaskCompletion>>>,
    api_contracts: Arc<Mutex<HashMap<String, ApiContract>>>,
}

impl LocalHubAgent {
    /// Create a new Local Hub Agent
    pub fn new(config: HubAgentConfig) -> Self {
        LocalHubAgent {
            config,
            workers: Arc::new(Mutex::new(HashMap::new())),
            team_task: Arc::new(Mutex::new(None)),
            completion: Arc::new(Mutex::new(None)),
            api_contracts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Initialize worker pool (called after sandbox is ready)
    pub fn init_worker_pool(&self, container_ids: Vec<String>) -> HubAgentResult<()> {
        let mut workers = self.workers.lock().unwrap();

        for (i, container_id) in container_ids.iter().enumerate() {
            let worker_name = self.generate_worker_name(i);
            workers.insert(
                worker_name.clone(),
                WorkerEntry {
                    name: worker_name,
                    container_id: container_id.clone(),
                    assignment: None,
                    completion: None,
                    status: WorkerStatus::Idle,
                },
            );
        }

        Ok(())
    }

    /// Receive team task from Global Orchestrator
    pub fn receive_team_task(&self, task: TeamTask) -> HubAgentResult<()> {
        let mut team_task = self.team_task.lock().unwrap();
        *team_task = Some(task);
        Ok(())
    }

    /// Break down team task into worker assignments
    pub fn break_down_task(&self) -> HubAgentResult<Vec<WorkerAssignment>> {
        let team_task = self.team_task.lock().unwrap();
        let task = team_task
            .as_ref()
            .ok_or(HubAgentError::TaskBreakdownFailed("No task assigned".to_string()))?;

        // Simple strategy: divide task description into N subtasks
        let workers = self.workers.lock().unwrap();
        let num_workers = workers.len();

        if num_workers == 0 {
            return Err(HubAgentError::TaskBreakdownFailed(
                "No workers available".to_string(),
            ));
        }

        let mut assignments = Vec::new();
        let subtask_descriptions = self.split_task_description(&task.description, num_workers);

        for (i, worker_entry) in workers.values().enumerate() {
            let assignment = WorkerAssignment {
                assignment_id: format!("{}-{}", task.task_id, i),
                worker_name: worker_entry.name.clone(),
                task_description: subtask_descriptions
                    .get(i)
                    .unwrap_or(&task.description)
                    .clone(),
                system_prompt: self.generate_system_prompt(&worker_entry.name),
                tools_available: vec![
                    "shell".to_string(),
                    "filesystem".to_string(),
                    "git".to_string(),
                    "http".to_string(),
                ],
                timeout_secs: task.timeout_secs,
                context: task.context.clone(),
            };
            assignments.push(assignment);
        }

        Ok(assignments)
    }

    /// Assign work to an idle worker
    pub fn assign_to_worker(&self, assignment: WorkerAssignment) -> HubAgentResult<()> {
        let mut workers = self.workers.lock().unwrap();

        let worker = workers
            .get_mut(&assignment.worker_name)
            .ok_or(HubAgentError::WorkerNotFound(assignment.worker_name.clone()))?;

        if worker.status != WorkerStatus::Idle {
            return Err(HubAgentError::WorkerAssignmentFailed(format!(
                "Worker {} not idle",
                assignment.worker_name
            )));
        }

        worker.assignment = Some(assignment);
        worker.status = WorkerStatus::Assigned;
        Ok(())
    }

    /// Record worker completion
    pub fn record_worker_completion(&self, completion: WorkerCompletion) -> HubAgentResult<()> {
        let mut workers = self.workers.lock().unwrap();

        let worker = workers
            .get_mut(&completion.worker_name)
            .ok_or(HubAgentError::WorkerNotFound(completion.worker_name.clone()))?;

        worker.completion = Some(completion.clone());
        worker.status = if completion.success {
            WorkerStatus::Completed
        } else {
            WorkerStatus::Failed
        };

        Ok(())
    }

    /// Get all completed worker outputs
    pub fn collect_outputs(&self) -> Vec<serde_json::Value> {
        let workers = self.workers.lock().unwrap();
        workers
            .values()
            .filter_map(|w| w.completion.as_ref().map(|c| &c.output))
            .cloned()
            .collect()
    }

    /// Run integration tests on combined output
    pub fn run_integration_tests(&self) -> HubAgentResult<bool> {
        let outputs = self.collect_outputs();

        // Simple validation: ensure all outputs are non-empty
        let all_valid = outputs.iter().all(|o| !o.is_null());

        if !all_valid {
            return Err(HubAgentError::IntegrationTestFailed(
                "Some worker outputs are empty".to_string(),
            ));
        }

        // TODO: Add more sophisticated integration tests based on task type
        Ok(true)
    }

    /// Create a task completion report for Global Orchestrator
    pub fn create_completion_report(&self) -> HubAgentResult<TaskCompletion> {
        let team_task = self.team_task.lock().unwrap();
        let task = team_task
            .as_ref()
            .ok_or(HubAgentError::TaskBreakdownFailed("No task to report".to_string()))?;

        let workers = self.workers.lock().unwrap();
        let mut failed_workers = Vec::new();
        for w in workers.values() {
            if w.status == WorkerStatus::Failed {
                if let Some(c) = w.completion.as_ref() {
                    failed_workers.extend(c.errors.clone());
                }
            }
        }

        let all_success = workers.values().all(|w| w.status == WorkerStatus::Completed);

        let combined_output = serde_json::json!({
            "workers": self.collect_outputs(),
            "metadata": {
                "team_id": self.config.team_id,
                "worker_count": workers.len(),
                "timestamp": current_timestamp(),
            }
        });

        Ok(TaskCompletion {
            task_id: task.task_id.clone(),
            team_id: self.config.team_id.clone(),
            success: all_success,
            output: combined_output,
            errors: failed_workers,
            timestamp: current_timestamp(),
        })
    }

    /// Listen for RPC calls from peer Hub Agents (now implemented by start_socket_server)
    pub fn handle_peer_rpc(&self, rpc: InterTeamRpc) -> HubAgentResult<InterTeamRpcResponse> {
        // This method is now called by the socket server thread
        // See start_socket_server() for the actual socket implementation

        match rpc.method.as_str() {
            "get_api_contract" => {
                let contracts = self.api_contracts.lock().unwrap();
                let contract_id = rpc
                    .params
                    .get("contract_id")
                    .and_then(|v| v.as_str())
                    .ok_or(HubAgentError::PeerCommunicationFailed(
                        "Missing contract_id".to_string(),
                    ))?;

                if let Some(contract) = contracts.get(contract_id) {
                    Ok(InterTeamRpcResponse {
                        call_id: rpc.call_id,
                        success: true,
                        result: serde_json::to_value(contract).unwrap_or_default(),
                        error: None,
                    })
                } else {
                    Ok(InterTeamRpcResponse {
                        call_id: rpc.call_id,
                        success: false,
                        result: serde_json::Value::Null,
                        error: Some("Contract not found".to_string()),
                    })
                }
            }
            _ => Err(HubAgentError::PeerCommunicationFailed(format!(
                "Unknown RPC method: {}",
                rpc.method
            ))),
        }
    }

    /// Register an API contract for other teams to discover
    pub fn register_api_contract(&self, contract: ApiContract) -> HubAgentResult<()> {
        let mut contracts = self.api_contracts.lock().unwrap();
        contracts.insert(contract.contract_id.clone(), contract);
        Ok(())
    }

    /// Start socket server for peer Hub Agent RPC communication
    pub fn start_socket_server(&self) -> HubAgentResult<()> {
        // Remove socket if it already exists (from previous runs)
        let _ = fs::remove_file(&self.config.socket_path);

        // Create Unix domain socket
        let listener = UnixListener::bind(&self.config.socket_path)
            .map_err(|e| HubAgentError::PeerCommunicationFailed(
                format!("Failed to bind socket: {}", e)
            ))?;

        // Spawn thread to accept incoming connections
        let socket_path = self.config.socket_path.clone();
        
        // Clone Arc pointers for thread
        let workers_clone = Arc::clone(&self.workers);
        let team_task_clone = Arc::clone(&self.team_task);
        let completion_clone = Arc::clone(&self.completion);
        let api_contracts_clone = Arc::clone(&self.api_contracts);
        let team_id = self.config.team_id.clone();

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(mut socket) => {
                        // Clone Arc pointers for each connection
                        let workers_c = Arc::clone(&workers_clone);
                        let team_task_c = Arc::clone(&team_task_clone);
                        let completion_c = Arc::clone(&completion_clone);
                        let api_contracts_c = Arc::clone(&api_contracts_clone);
                        let team_id_c = team_id.clone();

                        std::thread::spawn(move || {
                            let mut buffer = [0; 8192];
                            match socket.read(&mut buffer) {
                                Ok(n) => {
                                    if n > 0 {
                                        let request_str = String::from_utf8_lossy(&buffer[..n]);
                                        
                                        // Parse JSON RPC request
                                        match serde_json::from_str::<InterTeamRpc>(&request_str) {
                                            Ok(rpc_request) => {
                                                // Handle the RPC call
                                                let response = match rpc_request.method.as_str() {
                                                    "get_api_contract" => {
                                                        let contracts = api_contracts_c.lock().unwrap();
                                                        let contract_id = rpc_request
                                                            .params
                                                            .get("contract_id")
                                                            .and_then(|v| v.as_str());

                                                        match contract_id {
                                                            Some(id) if contracts.contains_key(id) => {
                                                                InterTeamRpcResponse {
                                                                    call_id: rpc_request.call_id,
                                                                    success: true,
                                                                    result: serde_json::to_value(
                                                                        contracts.get(id).unwrap()
                                                                    ).unwrap_or_default(),
                                                                    error: None,
                                                                }
                                                            }
                                                            _ => {
                                                                InterTeamRpcResponse {
                                                                    call_id: rpc_request.call_id,
                                                                    success: false,
                                                                    result: serde_json::Value::Null,
                                                                    error: Some("Contract not found".to_string()),
                                                                }
                                                            }
                                                        }
                                                    }
                                                    "get_team_status" => {
                                                        let workers = workers_c.lock().unwrap();
                                                        let idle_count = workers
                                                            .values()
                                                            .filter(|w| w.status == WorkerStatus::Idle)
                                                            .count();

                                                        InterTeamRpcResponse {
                                                            call_id: rpc_request.call_id,
                                                            success: true,
                                                            result: serde_json::json!({
                                                                "team_id": team_id_c.clone(),
                                                                "total_workers": workers.len(),
                                                                "idle_workers": idle_count,
                                                            }),
                                                            error: None,
                                                        }
                                                    }
                                                    _ => {
                                                        InterTeamRpcResponse {
                                                            call_id: rpc_request.call_id,
                                                            success: false,
                                                            result: serde_json::Value::Null,
                                                            error: Some(format!(
                                                                "Unknown method: {}",
                                                                rpc_request.method
                                                            )),
                                                        }
                                                    }
                                                };

                                                // Send JSON response
                                                if let Ok(response_json) =
                                                    serde_json::to_string(&response)
                                                {
                                                    let _ = socket.write_all(response_json.as_bytes());
                                                }
                                            }
                                            Err(_) => {
                                                // Invalid JSON, send error response
                                                let error_response = serde_json::json!({
                                                    "error": "Invalid JSON RPC request"
                                                });
                                                let _ = socket.write_all(
                                                    error_response.to_string().as_bytes()
                                                );
                                            }
                                        }
                                    }
                                }
                                Err(_) => {}
                            }
                        });
                    }
                    Err(_) => continue,
                }
            }
        });

        Ok(())
    }

    // Helper methods

    /// Generate a unique worker name (Jake, Mike, Sarah, etc.)
    fn generate_worker_name(&self, index: usize) -> String {
        let names = ["Jake", "Mike", "Sarah", "Alex", "Chris", "Jordan", "Morgan", "Casey", "Devon", "Riley"];
        let name = names.get(index % names.len()).unwrap_or(&"Worker");
        format!("{}-{}", name, index)
    }

    /// Split task description into N subtasks
    fn split_task_description(&self, description: &str, n: usize) -> Vec<String> {
        if n <= 1 {
            return vec![description.to_string()];
        }

        // Simple split: divide by sentences or lines
        let parts: Vec<&str> = description.split(". ").collect();
        let mut subtasks = Vec::new();
        let items_per_task = (parts.len() / n).max(1);

        for i in 0..n {
            let start = i * items_per_task;
            let end = if i == n - 1 {
                parts.len()
            } else {
                (i + 1) * items_per_task
            };

            if start < parts.len() {
                let subtask = parts[start..end].join(". ");
                subtasks.push(subtask);
            }
        }

        if subtasks.is_empty() {
            vec![description.to_string()]
        } else {
            subtasks
        }
    }

    /// Generate a system prompt for a worker
    fn generate_system_prompt(&self, worker_name: &str) -> String {
        format!(
            "You are {}, a software development specialist. Your task is to complete assigned work \
             with high quality. Use available tools to read files, execute code, and test your work. \
             Report all results clearly.",
            worker_name
        )
    }

    /// Get current worker status
    pub fn get_worker_status(&self, worker_name: &str) -> Option<WorkerStatus> {
        self.workers
            .lock()
            .unwrap()
            .get(worker_name)
            .map(|w| w.status)
    }

    /// List all workers
    pub fn list_workers(&self) -> Vec<String> {
        self.workers
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect()
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
    fn test_hub_agent_creation() {
        let config = HubAgentConfig {
            team_id: "team-1".to_string(),
            sandbox_id: "sb-1".to_string(),
            max_workers: 5,
            socket_path: "/tmp/hub-1.sock".to_string(),
            peer_sockets: HashMap::new(),
        };

        let hub = LocalHubAgent::new(config);
        assert_eq!(hub.list_workers().len(), 0);
    }

    #[test]
    fn test_worker_pool_initialization() {
        let config = HubAgentConfig {
            team_id: "team-1".to_string(),
            sandbox_id: "sb-1".to_string(),
            max_workers: 5,
            socket_path: "/tmp/hub-1.sock".to_string(),
            peer_sockets: HashMap::new(),
        };

        let hub = LocalHubAgent::new(config);
        let containers = vec!["c1".to_string(), "c2".to_string(), "c3".to_string()];
        hub.init_worker_pool(containers).unwrap();

        assert_eq!(hub.list_workers().len(), 3);
    }

    #[test]
    fn test_task_breakdown() {
        let config = HubAgentConfig {
            team_id: "team-1".to_string(),
            sandbox_id: "sb-1".to_string(),
            max_workers: 5,
            socket_path: "/tmp/hub-1.sock".to_string(),
            peer_sockets: HashMap::new(),
        };

        let hub = LocalHubAgent::new(config);
        hub.init_worker_pool(vec!["c1".to_string(), "c2".to_string()])
            .unwrap();

        let task = TeamTask {
            task_id: "task-1".to_string(),
            team_id: "team-1".to_string(),
            description: "Build API. Create database. Write tests.".to_string(),
            dependencies: vec![],
            estimated_complexity: 100,
            timeout_secs: 3600,
            context: serde_json::json!({}),
        };

        hub.receive_team_task(task).unwrap();
        let assignments = hub.break_down_task().unwrap();
        assert_eq!(assignments.len(), 2);
    }
}
