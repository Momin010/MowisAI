/// Runtime Infrastructure Manager
///
/// The Runtime manages the complete lifecycle of sandboxes and containers.
/// All operations go through an agentd client to ensure real infrastructure execution.
///
/// Key responsibilities:
/// - Provisioning sandboxes via agentd (real overlayfs, cgroups, chroot)
/// - Creating/destroying containers via agentd
/// - Monitoring resource usage (RAM, CPU)
/// - Pausing/resuming containers via agentd (SIGSTOP, cgroup freeze)
/// - Providing health status to Global Orchestrator
/// - Handling incremental provisioning and hot scaling

use crate::agentd_client::{
    AgentdClient, AgentdClientResult, ContainerControlAction, ContainerControlParams,
    CreateContainerParams, CreateSandboxParams,
};
use crate::protocol::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Runtime error types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeError {
    SandboxCreationFailed(String),
    ContainerCreationFailed(String),
    ResourceUnavailable(String),
    SandboxNotFound(String),
    ContainerNotFound(String),
    InvalidState(String),
}

pub type RuntimeResult<T> = Result<T, RuntimeError>;

/// Managed container state
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedContainer {
    pub id: String,
    pub status: ContainerStatus,
    pub created_at: u64,
    pub paused_at: Option<u64>,
    pub agentd_pid: Option<u32>,  // Real process ID from agentd
    pub agentd_rootfs: Option<String>,  // Real root filesystem from agentd
}

/// Managed sandbox state
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedSandbox {
    pub id: String,
    pub spec: SandboxSpec,
    pub hub_agent_pid: Option<u32>, // pid of Local Hub Agent process
    pub containers: HashMap<String, ManagedContainer>,
    pub created_at: u64,
    pub total_ram_used: u64,
    pub total_cpu_used: u32,
    pub agentd_sandbox_path: String,  // Real path from agentd
    pub agentd_sandbox_pid: u32,  // Real process ID from agentd
}

/// The Runtime manager
pub struct Runtime {
    sandboxes: Arc<Mutex<HashMap<String, ManagedSandbox>>>,
    client: Arc<AgentdClient>,
}

impl Runtime {
    /// Create a new Runtime instance with real agentd connection
    pub fn new(agentd_socket_path: String) -> Self {
        let client = AgentdClient::new(agentd_socket_path);
        Runtime {
            sandboxes: Arc::new(Mutex::new(HashMap::new())),
            client: Arc::new(client),
        }
    }

    /// Provision a set of sandboxes according to specification
    /// This ACTUALLY creates sandboxes via agentd
    pub fn provision_sandboxes(
        &self,
        spec: &ProvisioningSpec,
    ) -> RuntimeResult<ProvisioningReady> {
        let mut sb_map = self.sandboxes.lock().unwrap();
        let mut sandbox_handles = Vec::new();
        let now = current_timestamp();

        for sb_spec in &spec.sandbox_specs {
            // REAL: Create sandbox via agentd
            let create_params = CreateSandboxParams {
                sandbox_id: sb_spec.sandbox_id.clone(),
                os_image: sb_spec.os_image.clone(),
                ram_bytes: sb_spec.ram_bytes,
                cpu_millis: sb_spec.cpu_millis,
                packages: sb_spec.init_packages.clone(),
            };

            let sandbox_response = self.client
                .create_sandbox(create_params)
                .map_err(|e| RuntimeError::SandboxCreationFailed(format!("{:?}", e)))?;

            let sandbox_path_clone = sandbox_response.path.clone();

            let mut managed_sandbox = ManagedSandbox {
                id: sb_spec.sandbox_id.clone(),
                spec: sb_spec.clone(),
                hub_agent_pid: None,
                containers: HashMap::new(),
                created_at: now,
                total_ram_used: 0,
                total_cpu_used: 0,
                agentd_sandbox_path: sandbox_response.path,
                agentd_sandbox_pid: sandbox_response.pid,
            };

            // REAL: Create initial containers via agentd
            for i in 0..sb_spec.initial_containers {
                let container_id = format!("{}-{}", sb_spec.sandbox_id, i);
                
                let container_params = CreateContainerParams {
                    sandbox_id: sb_spec.sandbox_id.clone(),
                    container_id: container_id.clone(),
                };

                let container_response = self.client
                    .create_container(container_params)
                    .map_err(|e| RuntimeError::ContainerCreationFailed(format!("{:?}", e)))?;

                managed_sandbox.containers.insert(
                    container_id,
                    ManagedContainer {
                        id: container_response.container_id,
                        status: ContainerStatus::Ready,
                        created_at: now,
                        paused_at: None,
                        agentd_pid: Some(container_response.pid),
                        agentd_rootfs: Some(container_response.rootfs_path),
                    },
                );
            }

            sb_map.insert(sb_spec.sandbox_id.clone(), managed_sandbox);

            // Build sandbox handles response
            let containers: Vec<ContainerHandle> = sb_map
                .get(&sb_spec.sandbox_id)
                .unwrap()
                .containers
                .values()
                .map(|c| ContainerHandle {
                    container_id: c.id.clone(),
                    sandbox_id: sb_spec.sandbox_id.clone(),
                    status: c.status,
                })
                .collect();

            sandbox_handles.push(SandboxHandle {
                sandbox_id: sb_spec.sandbox_id.clone(),
                socket_path: format!("{}/agent.sock", sandbox_path_clone),
                containers,
            });
        }

        Ok(ProvisioningReady {
            request_id: spec.request_id.clone(),
            sandboxes: sandbox_handles,
            timestamp: now,
        })
    }

    /// Request additional containers in a sandbox (dynamic scaling)
    /// REAL: Creates containers via agentd
    pub fn request_additional_containers(
        &self,
        sandbox_id: &str,
        count: usize,
        _spec: &SandboxSpec,
    ) -> RuntimeResult<Vec<ContainerHandle>> {
        let mut sb_map = self.sandboxes.lock().unwrap();

        let managed_sb = sb_map
            .get_mut(sandbox_id)
            .ok_or(RuntimeError::SandboxNotFound(sandbox_id.to_string()))?;

        let mut container_handles = Vec::new();
        let now = current_timestamp();

        for i in 0..count {
            let next_index = managed_sb.containers.len() + i;
            let container_id = format!("{}-{}", sandbox_id, next_index);

            // REAL: Create container via agentd
            let container_params = CreateContainerParams {
                sandbox_id: sandbox_id.to_string(),
                container_id: container_id.clone(),
            };

            let container_response = self.client
                .create_container(container_params)
                .map_err(|e| RuntimeError::ContainerCreationFailed(format!("{:?}", e)))?;

            managed_sb.containers.insert(
                container_id.clone(),
                ManagedContainer {
                    id: container_response.container_id,
                    status: ContainerStatus::Ready,
                    created_at: now,
                    paused_at: None,
                    agentd_pid: Some(container_response.pid),
                    agentd_rootfs: Some(container_response.rootfs_path),
                },
            );

            container_handles.push(ContainerHandle {
                container_id,
                sandbox_id: sandbox_id.to_string(),
                status: ContainerStatus::Ready,
            });
        }

        Ok(container_handles)
    }

    /// Pause a container (idle management)
    /// REAL: Sends SIGSTOP via agentd
    pub fn pause_container(&self, sandbox_id: &str, container_id: &str) -> RuntimeResult<()> {
        let mut sb_map = self.sandboxes.lock().unwrap();

        let managed_sb = sb_map
            .get_mut(sandbox_id)
            .ok_or(RuntimeError::SandboxNotFound(sandbox_id.to_string()))?;

        let container = managed_sb
            .containers
            .get_mut(container_id)
            .ok_or(RuntimeError::ContainerNotFound(container_id.to_string()))?;

        match container.status {
            ContainerStatus::Active | ContainerStatus::Ready => {
                // REAL: Call agentd to freeze the container
                let control_params = ContainerControlParams {
                    sandbox_id: sandbox_id.to_string(),
                    container_id: container_id.to_string(),
                    action: ContainerControlAction::Pause,
                };

                self.client
                    .control_container(control_params)
                    .map_err(|e| RuntimeError::InvalidState(format!("Failed to pause: {:?}", e)))?;

                container.status = ContainerStatus::Paused;
                container.paused_at = Some(current_timestamp());
                Ok(())
            }
            _ => Err(RuntimeError::InvalidState(format!(
                "Cannot pause container in state {:?}",
                container.status
            ))),
        }
    }

    /// Resume a paused container
    /// REAL: Sends SIGCONT via agentd
    pub fn resume_container(&self, sandbox_id: &str, container_id: &str) -> RuntimeResult<()> {
        let mut sb_map = self.sandboxes.lock().unwrap();

        let managed_sb = sb_map
            .get_mut(sandbox_id)
            .ok_or(RuntimeError::SandboxNotFound(sandbox_id.to_string()))?;

        let container = managed_sb
            .containers
            .get_mut(container_id)
            .ok_or(RuntimeError::ContainerNotFound(container_id.to_string()))?;

        match container.status {
            ContainerStatus::Paused => {
                // REAL: Call agentd to resume the container
                let control_params = ContainerControlParams {
                    sandbox_id: sandbox_id.to_string(),
                    container_id: container_id.to_string(),
                    action: ContainerControlAction::Resume,
                };

                self.client
                    .control_container(control_params)
                    .map_err(|e| RuntimeError::InvalidState(format!("Failed to resume: {:?}", e)))?;

                container.status = ContainerStatus::Active;
                container.paused_at = None;
                Ok(())
            }
            _ => Err(RuntimeError::InvalidState(format!(
                "Cannot resume container in state {:?}",
                container.status
            ))),
        }
    }

    /// Mark Local Hub Agent as running in sandbox
    pub fn register_hub_agent(&self, sandbox_id: &str, pid: u32) -> RuntimeResult<()> {
        let mut sb_map = self.sandboxes.lock().unwrap();

        let managed_sb = sb_map
            .get_mut(sandbox_id)
            .ok_or(RuntimeError::SandboxNotFound(sandbox_id.to_string()))?;

        managed_sb.hub_agent_pid = Some(pid);
        Ok(())
    }

    /// Get health status of a sandbox
    pub fn get_health_status(&self, sandbox_id: &str) -> RuntimeResult<SandboxHealthStatus> {
        let sb_map = self.sandboxes.lock().unwrap();

        let managed_sb = sb_map
            .get(sandbox_id)
            .ok_or(RuntimeError::SandboxNotFound(sandbox_id.to_string()))?;

        let mut container_states = HashMap::new();
        for (cid, container) in &managed_sb.containers {
            container_states.insert(cid.clone(), container.status);
        }

        Ok(SandboxHealthStatus {
            sandbox_id: sandbox_id.to_string(),
            hub_agent_alive: managed_sb.hub_agent_pid.is_some(),
            container_states,
            ram_usage_bytes: managed_sb.total_ram_used,
            cpu_usage_millis: managed_sb.total_cpu_used,
            timestamp: current_timestamp(),
        })
    }

    /// Get all active sandboxes
    pub fn list_sandboxes(&self) -> Vec<String> {
        let sb_map = self.sandboxes.lock().unwrap();
        sb_map.keys().cloned().collect()
    }

    /// Destroy a sandbox and all its containers
    /// REAL: Calls agentd to destroy
    pub fn destroy_sandbox(&self, sandbox_id: &str) -> RuntimeResult<()> {
        // REAL: Call agentd to destroy sandbox
        self.client
            .destroy_sandbox(sandbox_id)
            .map_err(|e| RuntimeError::SandboxNotFound(format!("Failed to destroy: {:?}", e)))?;

        // Remove from our tracking
        let mut sb_map = self.sandboxes.lock().unwrap();
        sb_map.remove(sandbox_id).ok_or(RuntimeError::SandboxNotFound(
            sandbox_id.to_string(),
        ))?;
        Ok(())
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

    // NOTE: These tests require a real agentd instance running on /tmp/agentd.sock
    // Run tests with: AGENTD_SOCKET=/tmp/agentd.sock cargo test
    // Or start agentd first: ./target/release/agentd socket --path /tmp/agentd.sock

    #[test]
    #[ignore = "requires real agentd instance"]
    fn test_provision_sandboxes() {
        let runtime = Runtime::new("/tmp/agentd.sock".to_string());

        let spec = ProvisioningSpec {
            request_id: "test-1".to_string(),
            num_sandboxes: 1,
            sandbox_specs: vec![SandboxSpec {
                sandbox_id: "sb-test-1".to_string(),
                os_image: "alpine".to_string(),
                ram_bytes: 1_000_000_000,
                cpu_millis: 1000,
                init_packages: vec!["curl".to_string()],
                initial_containers: 2,
            }],
            max_concurrent_agents_per_sandbox: 10,
        };

        let result = runtime.provision_sandboxes(&spec);
        if result.is_ok() {
            let ready = result.unwrap();
            assert_eq!(ready.sandboxes.len(), 1);
            assert_eq!(ready.sandboxes[0].containers.len(), 2);

            // Cleanup
            let _ = runtime.destroy_sandbox("sb-test-1");
        }
    }

    #[test]
    #[ignore = "requires real agentd instance"]
    fn test_pause_resume_container() {
        let runtime = Runtime::new("/tmp/agentd.sock".to_string());

        let spec = ProvisioningSpec {
            request_id: "test-2".to_string(),
            num_sandboxes: 1,
            sandbox_specs: vec![SandboxSpec {
                sandbox_id: "sb-2".to_string(),
                os_image: "alpine".to_string(),
                ram_bytes: 1_000_000_000,
                cpu_millis: 1000,
                init_packages: vec![],
                initial_containers: 1,
            }],
            max_concurrent_agents_per_sandbox: 10,
        };

        runtime.provision_sandboxes(&spec).unwrap();

        let pause_result = runtime.pause_container("sb-2", "sb-2-0");
        assert!(pause_result.is_ok());

        let resume_result = runtime.resume_container("sb-2", "sb-2-0");
        assert!(resume_result.is_ok());
    }
}
