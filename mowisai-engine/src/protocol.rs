use serde::{Deserialize, Serialize};

/// Request structure for executing a task in the container
/// 
/// This struct represents a task request sent from the Electron frontend
/// to the MowisAI Engine via the Unix socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRequest {
    /// Unique identifier for this task
    pub task_id: String,
    
    /// The shell command to execute inside the container
    pub command: String,
    
    /// Maximum time allowed for execution in seconds
    pub timeout_secs: u64,
    
    /// Memory limit in MB (optional, defaults to 512)
    pub memory_mb: Option<u64>,
    
    /// CPU limit as percentage (optional, defaults to 50)
    pub cpu_percent: Option<u64>,
    
    /// Disk limit in MB (optional)
    pub disk_mb: Option<u64>,
    
    /// Request type: "exec", "create_session", "run_in_session", or "kill_session"
    pub request_type: String,
    
    /// Session ID for persistent container sessions (required for run_in_session and kill_session)
    pub session_id: Option<String>,
}



/// Response structure indicating task execution result
/// 
/// This struct represents the response sent back to the Electron frontend
/// after task execution completes, fails, or times out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResponse {
    /// The task ID from the original request
    pub task_id: String,
    
    /// Execution status: "running", "done", or "error"
    pub status: String,
    
    /// Combined stdout and stderr output, or error message
    pub output: String,
    
    /// Session ID for persistent container sessions (returned by create_session)
    pub session_id: Option<String>,
}


impl TaskRequest {
    /// Create a new task request with the given parameters
    pub fn new(task_id: String, command: String, timeout_secs: u64) -> Self {
        Self {
            task_id,
            command,
            timeout_secs,
            memory_mb: None,
            cpu_percent: None,
            disk_mb: None,
            request_type: "exec".to_string(),
            session_id: None,
        }
    }

    
    /// Create a new task request with resource limits
    pub fn with_resources(
        task_id: String, 
        command: String, 
        timeout_secs: u64,
        memory_mb: u64,
        cpu_percent: u64,
    ) -> Self {
        Self {
            task_id,
            command,
            timeout_secs,
            memory_mb: Some(memory_mb),
            cpu_percent: Some(cpu_percent),
            disk_mb: None,
            request_type: "exec".to_string(),
            session_id: None,
        }
    }

    
    /// Get memory limit with default value
    pub fn memory_mb_or_default(&self) -> u64 {
        self.memory_mb.unwrap_or(512)
    }
    
    /// Get CPU limit with default value
    pub fn cpu_percent_or_default(&self) -> u64 {
        self.cpu_percent.unwrap_or(50)
    }
}


impl TaskResponse {
    /// Create a successful task response
    pub fn success(task_id: String, output: String) -> Self {
        Self {
            task_id,
            status: "done".to_string(),
            output,
            session_id: None,
        }
    }
    
    /// Create an error task response
    pub fn error(task_id: String, error_message: String) -> Self {
        Self {
            task_id,
            status: "error".to_string(),
            output: error_message,
            session_id: None,
        }
    }
    
    /// Create a running status response
    pub fn running(task_id: String) -> Self {
        Self {
            task_id,
            status: "running".to_string(),
            output: String::new(),
            session_id: None,
        }
    }
    
    /// Create a response with session ID (for create_session)
    pub fn with_session(task_id: String, session_id: String) -> Self {
        Self {
            task_id,
            status: "done".to_string(),
            output: format!("Session created: {}", session_id),
            session_id: Some(session_id),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_task_request_serialization() {
        let request = TaskRequest::new(
            "test-123".to_string(),
            "echo hello".to_string(),
            30
        );
        
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("test-123"));
        assert!(json.contains("echo hello"));
        assert!(json.contains("30"));
    }
    
    #[test]
    fn test_task_request_deserialization() {
        let json = r#"{"task_id":"test-456","command":"ls -la","timeout_secs":60}"#;
        let request: TaskRequest = serde_json::from_str(json).unwrap();
        
        assert_eq!(request.task_id, "test-456");
        assert_eq!(request.command, "ls -la");
        assert_eq!(request.timeout_secs, 60);
    }
    
    #[test]
    fn test_task_response_serialization() {
        let response = TaskResponse::success(
            "test-123".to_string(),
            "hello world".to_string()
        );
        
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("done"));
        assert!(json.contains("hello world"));
        assert!(json.contains("test-123"));
    }
    
    #[test]
    fn test_task_response_deserialization() {
        let json = r#"{"task_id":"test-789","status":"done","output":"success output"}"#;
        let response: TaskResponse = serde_json::from_str(json).unwrap();
        
        assert_eq!(response.task_id, "test-789");
        assert_eq!(response.status, "done");
        assert_eq!(response.output, "success output");
    }
    
    #[test]
    fn test_task_response_error() {
        let response = TaskResponse::error(
            "test-456".to_string(),
            "command not found".to_string()
        );
        
        assert_eq!(response.status, "error");
        assert_eq!(response.output, "command not found");
        assert_eq!(response.task_id, "test-456");
    }
    
    #[test]
    fn test_task_response_running() {
        let response = TaskResponse::running("test-999".to_string());
        
        assert_eq!(response.status, "running");
        assert_eq!(response.task_id, "test-999");
        assert!(response.output.is_empty());
    }
    
    #[test]
    fn test_task_request_new_constructor() {
        let request = TaskRequest::new(
            "uuid-123".to_string(),
            "cat /etc/passwd".to_string(),
            120
        );
        
        assert_eq!(request.task_id, "uuid-123");
        assert_eq!(request.command, "cat /etc/passwd");
        assert_eq!(request.timeout_secs, 120);
    }
    
    #[test]
    fn test_invalid_json_deserialization() {
        let json = r#"{"invalid_field":"value"}"#;
        let result: Result<TaskRequest, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_task_response_with_special_characters() {
        let output = "Error: file not found\nPath: /tmp/test\nCode: 404";
        let response = TaskResponse::error(
            "test-special".to_string(),
            output.to_string()
        );
        
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: TaskResponse = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.output, output);
        assert_eq!(deserialized.status, "error");
    }
    
    #[test]
    fn test_roundtrip_serialization() {
        let original_request = TaskRequest::new(
            "roundtrip-001".to_string(),
            "echo 'test message'".to_string(),
            45
        );
        
        let json = serde_json::to_string(&original_request).unwrap();
        let deserialized: TaskRequest = serde_json::from_str(&json).unwrap();
        
        assert_eq!(original_request.task_id, deserialized.task_id);
        assert_eq!(original_request.command, deserialized.command);
        assert_eq!(original_request.timeout_secs, deserialized.timeout_secs);
    }
}
