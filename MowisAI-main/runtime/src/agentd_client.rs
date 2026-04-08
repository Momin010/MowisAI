/// agentd Socket Client
///
/// Provides a client interface to communicate with agentd via Unix sockets.
/// All real infrastructure operations go through this module.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// Errors returned by agentd operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentdClientError {
    ConnectionFailed(String),
    SendFailed(String),
    ReceiveFailed(String),
    Timeout,
    InvalidResponse(String),
    SandboxCreationFailed(String),
    ContainerCreationFailed(String),
    ToolInvocationFailed(String),
    SerializationError(String),
}

pub type AgentdClientResult<T> = Result<T, AgentdClientError>;

/// Request sent to agentd via socket
#[derive(Debug, Serialize, Deserialize)]
pub struct AgentdRequest {
    pub method: String,
    pub params: serde_json::Value,
    pub id: String,
}

/// Response received from agentd
#[derive(Debug, Serialize, Deserialize)]
pub struct AgentdResponse {
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub id: String,
}

/// Sandbox creation parameters
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSandboxParams {
    pub sandbox_id: String,
    pub os_image: String,
    pub ram_bytes: u64,
    pub cpu_millis: u32,
    pub packages: Vec<String>,
}

/// Sandbox creation response
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSandboxResponse {
    pub sandbox_id: String,
    pub path: String,
    pub pid: u32,
}

/// Container creation parameters
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateContainerParams {
    pub sandbox_id: String,
    pub container_id: String,
}

/// Container creation response
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateContainerResponse {
    pub container_id: String,
    pub sandbox_id: String,
    pub pid: u32,
    pub rootfs_path: String,
}

/// Tool invocation parameters
#[derive(Debug, Serialize, Deserialize)]
pub struct InvokeToolParams {
    pub sandbox_id: String,
    pub container_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
}

/// Tool invocation response
#[derive(Debug, Serialize, Deserialize)]
pub struct InvokeToolResponse {
    pub output: serde_json::Value,
    pub exit_code: i32,
    pub stderr: String,
}

/// Container control action
#[derive(Debug, Serialize, Deserialize)]
pub enum ContainerControlAction {
    Pause,
    Resume,
    Terminate,
}

/// Container control parameters
#[derive(Debug, Serialize, Deserialize)]
pub struct ContainerControlParams {
    pub sandbox_id: String,
    pub container_id: String,
    pub action: ContainerControlAction,
}

/// The agentd client
pub struct AgentdClient {
    socket_path: String,
    request_timeout: Duration,
}

impl AgentdClient {
    /// Create a new agentd client
    pub fn new(socket_path: String) -> Self {
        AgentdClient {
            socket_path,
            request_timeout: Duration::from_secs(30),
        }
    }

    /// Set request timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Create a new sandbox
    pub fn create_sandbox(&self, params: CreateSandboxParams) -> AgentdClientResult<CreateSandboxResponse> {
        let request = AgentdRequest {
            method: "create_sandbox".to_string(),
            params: serde_json::to_value(&params)
                .map_err(|e| AgentdClientError::SerializationError(e.to_string()))?,
            id: generate_request_id(),
        };

        let response = self.send_request(&request)?;
        let result = response
            .result
            .ok_or(AgentdClientError::SandboxCreationFailed(
                response.error.unwrap_or_default(),
            ))?;

        serde_json::from_value(result)
            .map_err(|e| AgentdClientError::InvalidResponse(e.to_string()))
    }

    /// Create a container in a sandbox
    pub fn create_container(&self, params: CreateContainerParams) -> AgentdClientResult<CreateContainerResponse> {
        let request = AgentdRequest {
            method: "create_container".to_string(),
            params: serde_json::to_value(&params)
                .map_err(|e| AgentdClientError::SerializationError(e.to_string()))?,
            id: generate_request_id(),
        };

        let response = self.send_request(&request)?;
        let result = response
            .result
            .ok_or(AgentdClientError::ContainerCreationFailed(
                response.error.unwrap_or_default(),
            ))?;

        serde_json::from_value(result)
            .map_err(|e| AgentdClientError::InvalidResponse(e.to_string()))
    }

    /// Invoke a tool in a container
    pub fn invoke_tool(&self, params: InvokeToolParams) -> AgentdClientResult<InvokeToolResponse> {
        let request = AgentdRequest {
            method: "invoke_tool".to_string(),
            params: serde_json::to_value(&params)
                .map_err(|e| AgentdClientError::SerializationError(e.to_string()))?,
            id: generate_request_id(),
        };

        let response = self.send_request(&request)?;
        let result = response
            .result
            .ok_or(AgentdClientError::ToolInvocationFailed(
                response.error.unwrap_or_default(),
            ))?;

        serde_json::from_value(result)
            .map_err(|e| AgentdClientError::InvalidResponse(e.to_string()))
    }

    /// Control a container (pause/resume/terminate)
    pub fn control_container(&self, params: ContainerControlParams) -> AgentdClientResult<()> {
        let request = AgentdRequest {
            method: "control_container".to_string(),
            params: serde_json::to_value(&params)
                .map_err(|e| AgentdClientError::SerializationError(e.to_string()))?,
            id: generate_request_id(),
        };

        let response = self.send_request(&request)?;
        if response.error.is_some() {
            return Err(AgentdClientError::InvalidResponse(
                response.error.unwrap_or_default(),
            ));
        }

        Ok(())
    }

    /// Destroy a sandbox
    pub fn destroy_sandbox(&self, sandbox_id: &str) -> AgentdClientResult<()> {
        let request = AgentdRequest {
            method: "destroy_sandbox".to_string(),
            params: serde_json::json!({"sandbox_id": sandbox_id}),
            id: generate_request_id(),
        };

        let response = self.send_request(&request)?;
        if response.error.is_some() {
            return Err(AgentdClientError::InvalidResponse(
                response.error.unwrap_or_default(),
            ));
        }

        Ok(())
    }

    /// Send a request and get response
    fn send_request(&self, request: &AgentdRequest) -> AgentdClientResult<AgentdResponse> {
        // Connect to agentd socket
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| AgentdClientError::ConnectionFailed(e.to_string()))?;

        stream
            .set_read_timeout(Some(self.request_timeout))
            .map_err(|e| AgentdClientError::ConnectionFailed(e.to_string()))?;

        stream
            .set_write_timeout(Some(self.request_timeout))
            .map_err(|e| AgentdClientError::ConnectionFailed(e.to_string()))?;

        // Serialize and send request
        let request_json = serde_json::to_string(request)
            .map_err(|e| AgentdClientError::SerializationError(e.to_string()))?;
        let request_bytes = format!("{}\n", request_json);

        stream
            .write_all(request_bytes.as_bytes())
            .map_err(|e| AgentdClientError::SendFailed(e.to_string()))?;

        // Read response line-by-line (until \n) to avoid blocking on persistent connections
        let mut reader = BufReader::new(&stream);
        let mut response_buf = String::new();
        reader
            .read_line(&mut response_buf)
            .map_err(|e| AgentdClientError::ReceiveFailed(e.to_string()))?;

        // Parse response
        let response: AgentdResponse = serde_json::from_str(&response_buf)
            .map_err(|e| AgentdClientError::InvalidResponse(e.to_string()))?;

        Ok(response)
    }
}

/// Generate a unique request ID
fn generate_request_id() -> String {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("req-{}", nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = AgentdClient::new("/tmp/agentd.sock".to_string());
        assert_eq!(client.socket_path, "/tmp/agentd.sock");
    }

    #[test]
    fn test_request_id_generation() {
        let id1 = generate_request_id();
        let id2 = generate_request_id();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("req-"));
    }

    #[test]
    fn test_serialization() {
        let params = CreateSandboxParams {
            sandbox_id: "sb-1".to_string(),
            os_image: "alpine".to_string(),
            ram_bytes: 1_000_000_000,
            cpu_millis: 1000,
            packages: vec!["curl".to_string()],
        };

        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json.get("sandbox_id").unwrap().as_str().unwrap(), "sb-1");
    }
}
