/// Client for communicating with Local Hub Agents
///
/// Provides methods to:
/// - Send team tasks
/// - Receive task completions
/// - Query team status
/// - Get API contracts

use crate::protocol::*;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// Errors for Hub Agent Client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HubAgentClientError {
    ConnectionFailed(String),
    RequestFailed(String),
    ResponseInvalid(String),
    Timeout,
}

pub type HubAgentClientResult<T> = Result<T, HubAgentClientError>;

/// Client for Hub Agent socket communication
pub struct HubAgentClient {
    socket_path: String,
    request_timeout: Duration,
}

/// Request wrapper for hub agent communication
#[derive(Debug, Serialize, Deserialize)]
struct HubAgentRequest {
    request_type: String, // "assign_task", "query_status", etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    task: Option<TeamTask>,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

/// Response wrapper for hub agent communication
#[derive(Debug, Serialize, Deserialize)]
struct HubAgentResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    completion: Option<TaskCompletion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl HubAgentClient {
    /// Create a new Hub Agent Client
    pub fn new(socket_path: String) -> Self {
        HubAgentClient {
            socket_path,
            request_timeout: Duration::from_secs(30),
        }
    }

    /// Send a team task to the hub agent
    pub fn assign_task(&self, task: TeamTask) -> HubAgentClientResult<()> {
        let request = HubAgentRequest {
            request_type: "assign_task".to_string(),
            task: Some(task),
            params: None,
        };

        let request_json = serde_json::to_string(&request)
            .map_err(|e| HubAgentClientError::RequestFailed(format!("{:?}", e)))?;

        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| HubAgentClientError::ConnectionFailed(format!("{:?}", e)))?;

        stream
            .set_read_timeout(Some(self.request_timeout))
            .map_err(|e| HubAgentClientError::ConnectionFailed(format!("{:?}", e)))?;

        stream
            .write_all(request_json.as_bytes())
            .map_err(|e| HubAgentClientError::RequestFailed(format!("{:?}", e)))?;

        Ok(())
    }

    /// Wait for task completion from the hub agent
    pub fn wait_for_completion(&self) -> HubAgentClientResult<TaskCompletion> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| HubAgentClientError::ConnectionFailed(format!("{:?}", e)))?;

        stream
            .set_read_timeout(Some(self.request_timeout))
            .map_err(|e| HubAgentClientError::ConnectionFailed(format!("{:?}", e)))?;

        let request = HubAgentRequest {
            request_type: "get_completion".to_string(),
            task: None,
            params: None,
        };

        let request_json = serde_json::to_string(&request)
            .map_err(|e| HubAgentClientError::RequestFailed(format!("{:?}", e)))?;

        stream
            .write_all(request_json.as_bytes())
            .map_err(|e| HubAgentClientError::RequestFailed(format!("{:?}", e)))?;

        let mut buffer = [0; 16384];
        match stream.read(&mut buffer) {
            Ok(n) if n > 0 => {
                let response_str = String::from_utf8_lossy(&buffer[..n]);
                let response: HubAgentResponse = serde_json::from_str(&response_str)
                    .map_err(|e| HubAgentClientError::ResponseInvalid(format!("{:?}", e)))?;

                if response.success {
                    response
                        .completion
                        .ok_or(HubAgentClientError::ResponseInvalid(
                            "No completion in response".to_string(),
                        ))
                } else {
                    Err(HubAgentClientError::RequestFailed(
                        response.error.unwrap_or_default(),
                    ))
                }
            }
            Ok(_) => Err(HubAgentClientError::ResponseInvalid(
                "Empty response".to_string(),
            )),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                Err(HubAgentClientError::Timeout)
            }
            Err(e) => Err(HubAgentClientError::RequestFailed(format!("{:?}", e))),
        }
    }

    /// Query team status
    pub fn get_status(&self) -> HubAgentClientResult<serde_json::Value> {
        let request = HubAgentRequest {
            request_type: "get_status".to_string(),
            task: None,
            params: None,
        };

        let request_json = serde_json::to_string(&request)
            .map_err(|e| HubAgentClientError::RequestFailed(format!("{:?}", e)))?;

        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| HubAgentClientError::ConnectionFailed(format!("{:?}", e)))?;

        stream
            .set_read_timeout(Some(self.request_timeout))
            .map_err(|e| HubAgentClientError::ConnectionFailed(format!("{:?}", e)))?;

        stream
            .write_all(request_json.as_bytes())
            .map_err(|e| HubAgentClientError::RequestFailed(format!("{:?}", e)))?;

        let mut buffer = [0; 8192];
        match stream.read(&mut buffer) {
            Ok(n) if n > 0 => {
                let response_str = String::from_utf8_lossy(&buffer[..n]);
                let response: HubAgentResponse = serde_json::from_str(&response_str)
                    .map_err(|e| HubAgentClientError::ResponseInvalid(format!("{:?}", e)))?;

                if response.success {
                    Ok(response.status.unwrap_or(serde_json::json!({})))
                } else {
                    Err(HubAgentClientError::RequestFailed(
                        response.error.unwrap_or_default(),
                    ))
                }
            }
            Ok(_) => Err(HubAgentClientError::ResponseInvalid(
                "Empty response".to_string(),
            )),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                Err(HubAgentClientError::Timeout)
            }
            Err(e) => Err(HubAgentClientError::RequestFailed(format!("{:?}", e))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn test_hub_agent_client_assign_task() {
        let client = HubAgentClient::new("/tmp/hub-agent-test.sock".to_string());

        let task = TeamTask {
            task_id: "task-1".to_string(),
            team_id: "team-1".to_string(),
            description: "Test task".to_string(),
            dependencies: vec![],
            estimated_complexity: 1,
            timeout_secs: 30,
            context: serde_json::json!({}),
        };

        // This test requires a real hub agent running
        let _ = client.assign_task(task);
    }

    #[test]
    #[ignore]
    fn test_hub_agent_client_status() {
        let client = HubAgentClient::new("/tmp/hub-agent-test.sock".to_string());

        // This test requires a real hub agent running
        let _ = client.get_status();
    }
}
