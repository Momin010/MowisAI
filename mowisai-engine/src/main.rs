use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use std::path::Path;
use std::fs;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};

mod protocol;
mod container;
mod executor;

use protocol::{TaskRequest, TaskResponse};
use executor::execute_task;
use container::{spawn_persistent_container, exec_in_container, kill_container};

/// Extended request type that includes request_type field for session management
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EngineRequest {
    pub request_type: String,
    pub task_id: Option<String>,
    pub session_id: Option<String>,
    pub sandbox_name: Option<String>,
    pub agent_name: Option<String>,
    pub from_agent: Option<String>,
    pub to_agent: Option<String>,
    pub content: Option<String>,
    pub command: Option<String>,
    pub timeout_secs: Option<u64>,
    pub memory_mb: Option<u64>,
    pub cpu_percent: Option<u64>,
}

/// Message struct for inter-agent communication
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub from: String,
    pub to: String,
    pub content: String,
    pub timestamp: u64,
}

/// Sandbox struct that groups agents and their message bus
#[derive(Debug, Clone)]
pub struct Sandbox {
    pub name: String,
    /// Maps agent_name to session_id
    pub sessions: HashMap<String, String>,
    /// Message inbox per agent
    pub messages: HashMap<String, Vec<Message>>,
}

impl Sandbox {
    /// Create a new sandbox with the given name
    pub fn new(name: String) -> Self {
        Self {
            name,
            sessions: HashMap::new(),
            messages: HashMap::new(),
        }
    }
    
    /// Register an agent in the sandbox
    pub fn join_agent(&mut self, agent_name: String, session_id: String) {
        self.sessions.insert(agent_name.clone(), session_id);
        // Initialize empty inbox for the agent
        self.messages.entry(agent_name).or_insert_with(Vec::new);
    }
    
    /// Send a message from one agent to another
    pub fn send_message(&mut self, from: String, to: String, content: String) -> anyhow::Result<()> {
        // Verify sender exists
        if !self.sessions.contains_key(&from) {
            return Err(anyhow::anyhow!("Sender agent '{}' not found in sandbox", from));
        }
        
        // Verify recipient exists
        if !self.sessions.contains_key(&to) {
            return Err(anyhow::anyhow!("Recipient agent '{}' not found in sandbox", to));
        }
        
        let message = Message {
            from,
            to: to.clone(),
            content,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        
        // Add to recipient's inbox
        self.messages.entry(to).or_insert_with(Vec::new).push(message);
        
        Ok(())
    }
    
    /// Read all messages for an agent and clear their inbox
    pub fn read_messages(&mut self, agent_name: &str) -> Vec<Message> {
        self.messages.remove(agent_name).unwrap_or_default()
    }
    
    /// Get session ID for an agent
    pub fn get_session_id(&self, agent_name: &str) -> Option<&String> {
        self.sessions.get(agent_name)
    }
}

/// Global sandbox registry - thread safe with Arc<Mutex<>>
type SandboxRegistry = Arc<Mutex<HashMap<String, Sandbox>>>;

/// Get the global sandbox registry
fn get_sandbox_registry() -> SandboxRegistry {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<SandboxRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| Arc::new(Mutex::new(HashMap::new()))).clone()
}

/// Socket path for the MowisAI Engine
const SOCKET_PATH: &str = "/tmp/mowisai.sock";

/// Main entry point for the MowisAI Engine
/// 
/// Creates a Unix socket server that listens for task requests from the Electron frontend,
/// executes them in isolated containers, and returns the results.
/// 
/// Run with --interactive flag to enter an interactive shell session instead.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Check for interactive mode
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--interactive" {
        return run_interactive_shell().await;
    }
    
    // Initialize logging
    
    
    println!("🚀 Starting MowisAI Engine v0.1");
    println!("📡 Socket path: {}", SOCKET_PATH);
    
    // Cleanup existing socket file if it exists
    cleanup_socket().await?;
    
    // Validate container environment
    println!("🔍 Validating container environment...");
    if let Err(e) = executor::validate_environment() {
        eprintln!("⚠️  Environment validation failed: {}", e);
        eprintln!("💡 Please run ./setup_rootfs.sh first to setup the container environment");
        // Continue anyway - the first task execution will fail with a clear error
    } else {
        println!("✅ Container environment validated");
    }
    
    // Create Unix socket listener
    let listener = UnixListener::bind(SOCKET_PATH)?;
    println!("✅ Socket created successfully");
    println!("🎯 Waiting for connections...");
    
    // Accept connections in a loop
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                // Spawn a new task to handle each connection concurrently
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream).await {
                        eprintln!("❌ Connection handler error: {}", e);
                    }
                });
            }
            Err(e) => {
                eprintln!("❌ Failed to accept connection: {}", e);
            }
        }
    }
}

/// Run interactive shell mode - enter the container like WSL
async fn run_interactive_shell() -> anyhow::Result<()> {
    println!("🐚 MowisAI Interactive Shell Mode");
    println!("   (type 'exit' to leave the container)\n");
    
    // Validate container environment
    if let Err(e) = executor::validate_environment() {
        eprintln!("⚠️  Environment validation failed: {}", e);
        eprintln!("💡 Please run ./setup_rootfs.sh first to setup the container environment");
        return Err(e);
    }
    
    // Run the interactive shell (blocking operation)
    tokio::task::spawn_blocking(|| {
        container::spawn_interactive_shell("./rootfs", 512, 50)
    }).await??;
    
    Ok(())
}


/// Cleanup existing socket file
async fn cleanup_socket() -> anyhow::Result<()> {
    let socket_path = Path::new(SOCKET_PATH);
    
    if socket_path.exists() {
        println!("🧹 Cleaning up existing socket file...");
        fs::remove_file(socket_path)?;
        println!("✅ Existing socket file removed");
    }
    
    Ok(())
}

/// Handle a single client connection
/// 
/// Reads newline-delimited JSON messages, deserializes them as TaskRequest,
/// executes the task, and writes the TaskResponse back as JSON.
async fn handle_connection(mut stream: UnixStream) -> anyhow::Result<()> {
    let peer_addr = stream.peer_addr()?;
    println!("🔗 New connection from: {:?}", peer_addr);
    
    let (reader, mut writer) = stream.split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    
    // Read lines from the socket
    loop {
        line.clear();
        
        match buf_reader.read_line(&mut line).await {
            Ok(0) => {
                // Connection closed
                println!("👋 Connection closed by client");
                break;
            }
            Ok(_) => {
                // Trim the line and parse JSON
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                
                println!("📨 Received: {}", trimmed);
                
                // Try to parse as EngineRequest first (with request_type)
                match parse_engine_request(trimmed) {
                    Ok(engine_req) => {
                        // Handle based on request_type
                        let response = handle_engine_request(engine_req).await;
                        let response_json = serde_json::to_string(&response)?;
                        writer.write_all(response_json.as_bytes()).await?;
                        writer.write_all(b"\n").await?;
                        writer.flush().await?;
                    }
                    Err(_) => {
                        // Fall back to legacy TaskRequest parsing
                        match parse_request(trimmed) {
                            Ok(request) => {
                                println!("🔄 Executing task {}: {}", request.task_id, request.command);
                                
                                // Execute the task (blocking operation, run in spawn_blocking)
                                let response = tokio::task::spawn_blocking(move || {
                                    execute_task(request)
                                }).await?;
                                
                                // Serialize and send response
                                let response_json = serde_json::to_string(&response)?;
                                writer.write_all(response_json.as_bytes()).await?;
                                writer.write_all(b"\n").await?;
                                writer.flush().await?;
                                
                                println!("✅ Task {} completed with status: {}", response.task_id, response.status);
                            }
                            Err(e) => {
                                eprintln!("❌ Failed to parse request: {}", e);
                                
                                // Send error response
                                let error_response = TaskResponse::error(
                                    "unknown".to_string(),
                                    format!("Invalid request format: {}", e)
                                );
                                let error_json = serde_json::to_string(&error_response)?;
                                writer.write_all(error_json.as_bytes()).await?;
                                writer.write_all(b"\n").await?;
                                writer.flush().await?;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("❌ Error reading from socket: {}", e);
                break;
            }
        }
    }
    
    Ok(())
}

/// Parse a JSON string into an EngineRequest
fn parse_engine_request(json_str: &str) -> anyhow::Result<EngineRequest> {
    let request: EngineRequest = serde_json::from_str(json_str)?;
    Ok(request)
}

/// Handle engine requests based on request_type
async fn handle_engine_request(req: EngineRequest) -> TaskResponse {
    let task_id = req.task_id.clone().unwrap_or_else(|| "unknown".to_string());
    
    match req.request_type.as_str() {
        "create_sandbox" => {
            let sandbox_name = match req.sandbox_name {
                Some(name) => name,
                None => {
                    return TaskResponse::error(task_id, "sandbox_name is required".to_string());
                }
            };
            
            println!("🏖️  Creating sandbox: {}", sandbox_name);
            
            let registry = get_sandbox_registry();
            let mut sandboxes = registry.lock().unwrap();
            
            if sandboxes.contains_key(&sandbox_name) {
                return TaskResponse::error(task_id, format!("Sandbox '{}' already exists", sandbox_name));
            }
            
            let sandbox = Sandbox::new(sandbox_name.clone());
            sandboxes.insert(sandbox_name.clone(), sandbox);
            
            println!("✅ Sandbox {} created", sandbox_name);
            TaskResponse {
                task_id: sandbox_name.clone(),
                session_id: Some(sandbox_name),
                status: "created".to_string(),
                output: "Sandbox created successfully".to_string(),
            }
        }
        
        "join_sandbox" => {
            let sandbox_name = match req.sandbox_name {
                Some(name) => name,
                None => {
                    return TaskResponse::error(task_id, "sandbox_name is required".to_string());
                }
            };
            let agent_name = match req.agent_name {
                Some(name) => name,
                None => {
                    return TaskResponse::error(task_id, "agent_name is required".to_string());
                }
            };
            let memory_mb = req.memory_mb.unwrap_or(512);
            let cpu_percent = req.cpu_percent.unwrap_or(50);
            
            println!("🤖 Agent {} joining sandbox {}", agent_name, sandbox_name);
            
            // First create a persistent session for the agent
            let session_id = format!("{}-{}", sandbox_name, agent_name);
            let session_id_for_spawn = session_id.clone();
            
            let spawn_result = tokio::task::spawn_blocking(move || {
                spawn_persistent_container("./rootfs", &session_id_for_spawn, memory_mb, cpu_percent)
            }).await;
            
            match spawn_result {
                Ok(Ok(_pid)) => {
                    // Register the agent in the sandbox
                    let registry = get_sandbox_registry();
                    let mut sandboxes = registry.lock().unwrap();
                    
                    match sandboxes.get_mut(&sandbox_name) {
                        Some(sandbox) => {
                            sandbox.join_agent(agent_name.clone(), session_id.clone());
                            println!("✅ Agent {} joined sandbox {} with session {}", agent_name, sandbox_name, session_id);
                            TaskResponse {
                                task_id: session_id.clone(),
                                session_id: Some(session_id),
                                status: "joined".to_string(),
                                output: format!("Agent {} joined sandbox {}", agent_name, sandbox_name),
                            }
                        }
                        None => {
                            TaskResponse::error(task_id, format!("Sandbox '{}' not found", sandbox_name))
                        }
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("❌ Failed to create session for agent: {}", e);
                    TaskResponse::error(task_id, format!("Failed to create session: {}", e))
                }
                Err(e) => {
                    eprintln!("❌ Task panicked: {}", e);
                    TaskResponse::error(task_id, format!("Task panicked: {}", e))
                }
            }
        }
        
        "message_send" => {
            let sandbox_name = match req.sandbox_name {
                Some(name) => name,
                None => {
                    return TaskResponse::error(task_id, "sandbox_name is required".to_string());
                }
            };
            let from_agent = match req.from_agent {
                Some(name) => name,
                None => {
                    return TaskResponse::error(task_id, "from_agent is required".to_string());
                }
            };
            let to_agent = match req.to_agent {
                Some(name) => name,
                None => {
                    return TaskResponse::error(task_id, "to_agent is required".to_string());
                }
            };
            let content = match req.content {
                Some(text) => text,
                None => {
                    return TaskResponse::error(task_id, "content is required".to_string());
                }
            };
            
            println!("📤 Message from {} to {} in sandbox {}", from_agent, to_agent, sandbox_name);
            
            let registry = get_sandbox_registry();
            let mut sandboxes = registry.lock().unwrap();
            
            match sandboxes.get_mut(&sandbox_name) {
                Some(sandbox) => {
                    match sandbox.send_message(from_agent.clone(), to_agent.clone(), content) {
                        Ok(()) => {
                            println!("✅ Message sent");
                            TaskResponse {
                                task_id: format!("{}->{}", from_agent, to_agent),
                                session_id: Some(sandbox_name),
                                status: "sent".to_string(),
                                output: format!("Message sent to {}", to_agent),
                            }
                        }
                        Err(e) => {
                            TaskResponse::error(task_id, format!("Failed to send message: {}", e))
                        }
                    }
                }
                None => {
                    TaskResponse::error(task_id, format!("Sandbox '{}' not found", sandbox_name))
                }
            }
        }
        
        "message_read" => {
            let sandbox_name = match req.sandbox_name {
                Some(name) => name,
                None => {
                    return TaskResponse::error(task_id, "sandbox_name is required".to_string());
                }
            };
            let agent_name = match req.agent_name {
                Some(name) => name,
                None => {
                    return TaskResponse::error(task_id, "agent_name is required".to_string());
                }
            };
            
            println!("📥 Reading messages for {} in sandbox {}", agent_name, sandbox_name);
            
            let registry = get_sandbox_registry();
            let mut sandboxes = registry.lock().unwrap();
            
            match sandboxes.get_mut(&sandbox_name) {
                Some(sandbox) => {
                    let messages = sandbox.read_messages(&agent_name);
                    let message_count = messages.len();
                    
                    // Serialize messages to JSON for output
                    match serde_json::to_string(&messages) {
                        Ok(json_output) => {
                            println!("✅ Read {} messages for {}", message_count, agent_name);
                            TaskResponse {
                                task_id: agent_name.clone(),
                                session_id: Some(sandbox_name),
                                status: "read".to_string(),
                                output: json_output,
                            }
                        }
                        Err(e) => {
                            TaskResponse::error(task_id, format!("Failed to serialize messages: {}", e))
                        }
                    }
                }
                None => {
                    TaskResponse::error(task_id, format!("Sandbox '{}' not found", sandbox_name))
                }
            }
        }
        
        "create_session" => {
            let session_id = req.session_id.clone().unwrap_or_else(|| {
                format!("session-{}", uuid::Uuid::new_v4())
            });
            let memory_mb = req.memory_mb.unwrap_or(512);
            let cpu_percent = req.cpu_percent.unwrap_or(50);
            
            println!("🆕 Creating session: {}", session_id);
            
            let session_id_for_spawn = session_id.clone();
            match tokio::task::spawn_blocking(move || {
                spawn_persistent_container("./rootfs", &session_id_for_spawn, memory_mb, cpu_percent)
            }).await {
                Ok(Ok(pid)) => {
                    println!("✅ Session {} created with PID {}", session_id, pid);
                    TaskResponse {
                        task_id: session_id.clone(),
                        session_id: Some(session_id.clone()),
                        status: "created".to_string(),
                        output: format!("Session created with PID {}", pid),
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("❌ Failed to create session: {}", e);
                    TaskResponse::error(task_id, format!("Failed to create session: {}", e))
                }
                Err(e) => {
                    eprintln!("❌ Task panicked: {}", e);
                    TaskResponse::error(task_id, format!("Task panicked: {}", e))
                }
            }
        }
        
        "run_in_session" => {
            let session_id = match req.session_id {
                Some(id) => id,
                None => {
                    return TaskResponse::error(task_id, "session_id is required".to_string());
                }
            };
            let command = match req.command {
                Some(cmd) => cmd,
                None => {
                    return TaskResponse::error(task_id, "command is required".to_string());
                }
            };
            let timeout_secs = req.timeout_secs.unwrap_or(30);
            
            println!("▶️  Running in session {}: {}", session_id, command);
            
            let session_id_for_exec = session_id.clone();
            let command_for_exec = command.clone();
            match tokio::task::spawn_blocking(move || {
                exec_in_container(&session_id_for_exec, &command_for_exec, timeout_secs)
            }).await {
                Ok(Ok(output)) => {
                    println!("✅ Command completed in session {}", session_id);
                    TaskResponse {
                        task_id: session_id,
                        session_id: None,
                        status: "done".to_string(),
                        output,
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("❌ Command failed in session: {}", e);
                    TaskResponse::error(task_id, format!("Command failed: {}", e))
                }
                Err(e) => {
                    eprintln!("❌ Task panicked: {}", e);
                    TaskResponse::error(task_id, format!("Task panicked: {}", e))
                }
            }
        }
        
        "kill_session" => {
            let session_id = match req.session_id {
                Some(id) => id,
                None => {
                    return TaskResponse::error(task_id, "session_id is required".to_string());
                }
            };
            
            println!("💀 Killing session: {}", session_id);
            
            let session_id_for_kill = session_id.clone();
            match tokio::task::spawn_blocking(move || {
                kill_container(&session_id_for_kill)
            }).await {
                Ok(Ok(())) => {
                    println!("✅ Session {} killed", session_id);
                    TaskResponse {
                        task_id: session_id,
                        session_id: None,
                        status: "killed".to_string(),
                        output: "Session terminated".to_string(),
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("❌ Failed to kill session: {}", e);
                    TaskResponse::error(task_id, format!("Failed to kill session: {}", e))
                }
                Err(e) => {
                    eprintln!("❌ Task panicked: {}", e);
                    TaskResponse::error(task_id, format!("Task panicked: {}", e))
                }
            }
        }
        
        _ => {
            TaskResponse::error(task_id, format!("Unknown request_type: {}", req.request_type))
        }
    }
}


/// Parse a JSON string into a TaskRequest
fn parse_request(json_str: &str) -> anyhow::Result<TaskRequest> {
    let request: TaskRequest = serde_json::from_str(json_str)?;
    Ok(request)
}

/// Graceful shutdown handler
async fn shutdown() {
    println!("\n🛑 Shutting down MowisAI Engine...");
    
    // Cleanup socket file
    let _ = cleanup_socket().await;
    
    // Cleanup cgroups
    let _ = container::cleanup_cgroups();
    
    println!("👋 Goodbye!");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use std::path::Path;
    
    #[tokio::test]
    async fn test_parse_request() {
        let json = r#"{"task_id":"test-123","command":"echo hello","timeout_secs":30}"#;
        let request = parse_request(json).unwrap();
        
        assert_eq!(request.task_id, "test-123");
        assert_eq!(request.command, "echo hello");
        assert_eq!(request.timeout_secs, 30);
    }
    
    #[tokio::test]
    async fn test_parse_request_invalid() {
        let json = r#"{"invalid":"json"}"#;
        let result = parse_request(json);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_socket_path_constant() {
        assert_eq!(SOCKET_PATH, "/tmp/mowisai.sock");
        assert!(SOCKET_PATH.starts_with("/tmp/"));
        assert!(SOCKET_PATH.ends_with(".sock"));
    }
    
    #[test]
    fn test_task_response_serialization() {
        let response = TaskResponse::success(
            "test-456".to_string(),
            "output data".to_string()
        );
        
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("test-456"));
        assert!(json.contains("done"));
        assert!(json.contains("output data"));
    }
    
    #[test]
    fn test_task_response_error_serialization() {
        let response = TaskResponse::error(
            "error-123".to_string(),
            "Something went wrong".to_string()
        );
        
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("error-123"));
        assert!(json.contains("error"));
        assert!(json.contains("Something went wrong"));
    }
    
    #[tokio::test]
    async fn test_cleanup_socket_nonexistent() {
        // Should not panic when socket doesn't exist
        let result = cleanup_socket().await;
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_parse_request_with_special_chars() {
        let json = r#"{"task_id":"test-789","command":"echo 'hello world' && ls -la","timeout_secs":45}"#;
        let request = parse_request(json).unwrap();
        
        assert_eq!(request.task_id, "test-789");
        assert_eq!(request.command, "echo 'hello world' && ls -la");
        assert_eq!(request.timeout_secs, 45);
    }
    
    #[test]
    fn test_parse_request_large_timeout() {
        let json = r#"{"task_id":"long-task","command":"sleep 100","timeout_secs":3600}"#;
        let request = parse_request(json).unwrap();
        
        assert_eq!(request.timeout_secs, 3600);
    }
    
    #[test]
    fn test_empty_json_parsing() {
        let json = r#"{}"#;
        let result: Result<TaskRequest, _> = serde_json::from_str(json);
        // Should fail because required fields are missing
        assert!(result.is_err());
    }
    
    #[test]
    fn test_task_request_with_uuid_format() {
        let json = r#"{"task_id":"550e8400-e29b-41d4-a716-446655440000","command":"uname -a","timeout_secs":10}"#;
        let request = parse_request(json).unwrap();
        
        assert_eq!(request.task_id, "550e8400-e29b-41d4-a716-446655440000");
    }
    
    #[test]
    fn test_json_with_newline_delimiter() {
        let json_line = r#"{"task_id":"newline-test","command":"echo test","timeout_secs":5}"#;
        // In real scenario, this would have a newline at the end
        let with_newline = format!("{}\n", json_line);
        
        // Verify the JSON part is valid
        let request: TaskRequest = serde_json::from_str(json_line).unwrap();
        assert_eq!(request.task_id, "newline-test");
        
        // Verify the full string ends with newline
        assert!(with_newline.ends_with('\n'));
    }
    
    // Sandbox tests
    #[test]
    fn test_sandbox_creation() {
        let sandbox = Sandbox::new("test-sandbox".to_string());
        assert_eq!(sandbox.name, "test-sandbox");
        assert!(sandbox.sessions.is_empty());
        assert!(sandbox.messages.is_empty());
    }
    
    #[test]
    fn test_sandbox_join_agent() {
        let mut sandbox = Sandbox::new("test-sandbox".to_string());
        sandbox.join_agent("planner".to_string(), "session-123".to_string());
        
        assert_eq!(sandbox.sessions.get("planner"), Some(&"session-123".to_string()));
        assert!(sandbox.messages.contains_key("planner"));
    }
    
    #[test]
    fn test_sandbox_send_message() {
        let mut sandbox = Sandbox::new("test-sandbox".to_string());
        sandbox.join_agent("sender".to_string(), "session-1".to_string());
        sandbox.join_agent("receiver".to_string(), "session-2".to_string());
        
        let result = sandbox.send_message(
            "sender".to_string(),
            "receiver".to_string(),
            "Hello!".to_string()
        );
        
        assert!(result.is_ok());
        let messages = sandbox.read_messages("receiver");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "Hello!");
        assert_eq!(messages[0].from, "sender");
    }
    
    #[test]
    fn test_sandbox_read_messages_clears_inbox() {
        let mut sandbox = Sandbox::new("test-sandbox".to_string());
        sandbox.join_agent("agent1".to_string(), "session-1".to_string());
        sandbox.join_agent("agent2".to_string(), "session-2".to_string());
        
        sandbox.send_message("agent1".to_string(), "agent2".to_string(), "Msg1".to_string()).unwrap();
        sandbox.send_message("agent1".to_string(), "agent2".to_string(), "Msg2".to_string()).unwrap();
        
        let messages = sandbox.read_messages("agent2");
        assert_eq!(messages.len(), 2);
        
        // Inbox should be empty now
        let empty = sandbox.read_messages("agent2");
        assert!(empty.is_empty());
    }
    
    #[test]
    fn test_sandbox_send_to_nonexistent_agent() {
        let mut sandbox = Sandbox::new("test-sandbox".to_string());
        sandbox.join_agent("sender".to_string(), "session-1".to_string());
        
        let result = sandbox.send_message(
            "sender".to_string(),
            "nonexistent".to_string(),
            "Hello!".to_string()
        );
        
        assert!(result.is_err());
    }
    
    #[test]
    fn test_sandbox_get_session_id() {
        let mut sandbox = Sandbox::new("test-sandbox".to_string());
        sandbox.join_agent("coder".to_string(), "session-coder-123".to_string());
        
        assert_eq!(
            sandbox.get_session_id("coder"),
            Some(&"session-coder-123".to_string())
        );
        assert_eq!(sandbox.get_session_id("nonexistent"), None);
    }
    
    #[test]
    fn test_global_sandbox_registry() {
        let registry1 = get_sandbox_registry();
        let registry2 = get_sandbox_registry();
        
        // Should be the same instance
        assert!(Arc::ptr_eq(&registry1, &registry2));
    }
}
