//! Streaming Support — Real-time response streaming to desktop app
//!
//! Provides a streaming protocol over the Unix socket that the desktop app
//! can connect to for real-time updates during orchestration.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

/// Events streamed to the desktop app
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    /// Orchestration started
    Started {
        session_id: String,
        task: String,
        mode: String,
        timestamp: u64,
    },
    /// Planning phase completed
    PlanReady {
        task_count: usize,
        sandbox_count: usize,
        estimated_agents: usize,
        timestamp: u64,
    },
    /// Agent spawned
    AgentSpawned {
        agent_id: String,
        sandbox_name: String,
        task_description: String,
        timestamp: u64,
    },
    /// Agent made progress (tool call)
    AgentProgress {
        agent_id: String,
        tool_name: String,
        round: u32,
        timestamp: u64,
    },
    /// Every LLM request
    LlmCallStarted {
        agent_id: String,
        provider: String,
        model: String,
        prompt_tokens_estimate: u64,
        timestamp: u64,
    },
    /// Every LLM response
    LlmCallCompleted {
        agent_id: String,
        provider: String,
        model: String,
        completion_tokens: u64,
        duration_ms: u64,
        timestamp: u64,
    },
    /// File operation inside any sandbox/agent layer
    FileOperation {
        sandbox_name: String,
        agent_id: Option<String>,
        operation: String,
        path: String,
        metadata: Value,
        timestamp: u64,
    },
    /// Command execution inside any sandbox
    CommandExecution {
        sandbox_name: String,
        agent_id: Option<String>,
        command: String,
        exit_code: Option<i32>,
        duration_ms: Option<u64>,
        output_preview: String,
        timestamp: u64,
    },
    /// Agent completed task
    AgentCompleted {
        agent_id: String,
        success: bool,
        duration_ms: u64,
        timestamp: u64,
    },
    /// Verification round started
    VerifyRound {
        round: usize,
        sandbox_name: String,
        timestamp: u64,
    },
    /// Test result
    TestResult {
        test_id: String,
        passed: bool,
        output: String,
        timestamp: u64,
    },
    /// Merge progress
    MergeProgress {
        stage: String,
        progress_percent: f32,
        timestamp: u64,
    },
    /// Final result
    Completed {
        success: bool,
        total_duration_ms: u64,
        tasks_completed: usize,
        tasks_failed: usize,
        tasks_skipped: usize,
        total_cost_usd: f64,
        diff_summary: String,
        timestamp: u64,
    },
    /// Error occurred
    Error {
        message: String,
        recoverable: bool,
        timestamp: u64,
    },
    /// Log message
    Log {
        level: String,
        message: String,
        timestamp: u64,
    },
    /// Progress update
    Progress {
        percent: f32,
        message: String,
        timestamp: u64,
    },
    /// Cost update
    CostUpdate {
        current_cost_usd: f64,
        budget_remaining_usd: f64,
        tokens_used: u64,
        timestamp: u64,
    },
    /// Heartbeat (keep connection alive)
    Heartbeat { timestamp: u64 },
}

/// Stream writer that sends events to connected desktop apps
pub struct StreamWriter {
    connections: Arc<Mutex<Vec<UnixStream>>>,
}

impl StreamWriter {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Add a new connection
    pub fn add_connection(&self, stream: UnixStream) {
        if let Ok(mut conns) = self.connections.lock() {
            conns.push(stream);
        }
    }

    /// Send an event to all connected clients
    pub fn send_event(&self, event: &StreamEvent) {
        let data = match serde_json::to_string(event) {
            Ok(d) => d,
            Err(_) => return,
        };

        let mut line = data;
        line.push('\n');

        if let Ok(mut conns) = self.connections.lock() {
            conns.retain_mut(|stream| stream.write_all(line.as_bytes()).is_ok());
        }
    }

    /// Get number of connected clients
    pub fn connection_count(&self) -> usize {
        self.connections.lock().map(|c| c.len()).unwrap_or(0)
    }
}

impl Default for StreamWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to create common events
pub fn started_event(session_id: &str, task: &str, mode: &str) -> StreamEvent {
    StreamEvent::Started {
        session_id: session_id.to_string(),
        task: task.to_string(),
        mode: mode.to_string(),
        timestamp: now_ms(),
    }
}

pub fn completed_event(
    success: bool,
    duration_ms: u64,
    completed: usize,
    failed: usize,
    skipped: usize,
    cost: f64,
    diff_summary: &str,
) -> StreamEvent {
    StreamEvent::Completed {
        success,
        total_duration_ms: duration_ms,
        tasks_completed: completed,
        tasks_failed: failed,
        tasks_skipped: skipped,
        total_cost_usd: cost,
        diff_summary: diff_summary.to_string(),
        timestamp: now_ms(),
    }
}

pub fn progress_event(percent: f32, message: &str) -> StreamEvent {
    StreamEvent::Progress {
        percent,
        message: message.to_string(),
        timestamp: now_ms(),
    }
}

pub fn log_event(level: &str, message: &str) -> StreamEvent {
    StreamEvent::Log {
        level: level.to_string(),
        message: message.to_string(),
        timestamp: now_ms(),
    }
}

pub fn error_event(message: &str, recoverable: bool) -> StreamEvent {
    StreamEvent::Error {
        message: message.to_string(),
        recoverable,
        timestamp: now_ms(),
    }
}

pub fn cost_update_event(cost: f64, remaining: f64, tokens: u64) -> StreamEvent {
    StreamEvent::CostUpdate {
        current_cost_usd: cost,
        budget_remaining_usd: remaining,
        tokens_used: tokens,
        timestamp: now_ms(),
    }
}

pub fn heartbeat_event() -> StreamEvent {
    StreamEvent::Heartbeat {
        timestamp: now_ms(),
    }
}

pub fn file_operation_event(
    sandbox_name: &str,
    agent_id: Option<&str>,
    operation: &str,
    path: &str,
    metadata: Value,
) -> StreamEvent {
    StreamEvent::FileOperation {
        sandbox_name: sandbox_name.to_string(),
        agent_id: agent_id.map(|v| v.to_string()),
        operation: operation.to_string(),
        path: path.to_string(),
        metadata,
        timestamp: now_ms(),
    }
}

pub fn command_execution_event(
    sandbox_name: &str,
    agent_id: Option<&str>,
    command: &str,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
    output_preview: &str,
) -> StreamEvent {
    StreamEvent::CommandExecution {
        sandbox_name: sandbox_name.to_string(),
        agent_id: agent_id.map(|v| v.to_string()),
        command: command.to_string(),
        exit_code,
        duration_ms,
        output_preview: output_preview.to_string(),
        timestamp: now_ms(),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
