use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Running,
    Complete,
    Failed,
}

impl Default for TaskStatus {
    fn default() -> Self {
        TaskStatus::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub description: String,
    pub sandbox: Option<String>,
    #[serde(default)]
    pub status: TaskStatus,
    #[serde(default)]
    pub started_at: Option<u64>,
    #[serde(default)]
    pub completed_at: Option<u64>,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub views: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatMessage {
    User {
        content: String,
        ts: u64,
    },
    Agent {
        content: String,
        streaming: bool,
        ts: u64,
    },
    System {
        content: String,
        ts: u64,
    },
    Plan {
        sandboxes: Vec<String>,
        task_count: usize,
        agent_count: usize,
        mode: String,
        ts: u64,
    },
    Error {
        content: String,
        ts: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub prompt: String,
    pub status: String,
    pub started_at: u64,
    pub completed_at: Option<u64>,
    pub task_count: usize,
    pub tasks_done: usize,
    #[serde(default)]
    pub tokens_total: u64,
    #[serde(default)]
    pub duration_secs: Option<u64>,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub socket_path: String,
    pub max_agents: u32,
    pub mode: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub gcp_project: String,
    /// Vertex AI region (e.g. "us-central1"). Only used when provider == "vertex".
    #[serde(default = "default_gcp_region")]
    pub gcp_region: String,
    /// Path to a GCP service account JSON key file for Vertex AI authentication.
    /// When set, this takes precedence over Application Default Credentials.
    #[serde(default)]
    pub gcp_service_account_key_path: String,
    /// When true, agent writes are isolated in a tmpfs-style temp directory.
    /// The original project (lower_dir) is never modified by agents.
    #[serde(default = "default_sandbox_enabled")]
    pub sandbox_enabled: bool,
    /// Execution model for Layer 4 agent tool-calling loops.
    /// When empty, falls back to `model` so single-model setups work unchanged.
    #[serde(default)]
    pub execution_model: String,
}

pub fn default_sandbox_enabled() -> bool {
    true
}
pub fn default_gcp_region() -> String {
    "us-central1".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            socket_path: "/tmp/agentd.sock".into(),
            max_agents: 100,
            mode: "auto".into(),
            provider: "gemini".into(),
            model: "gemini-2.0-flash".into(),
            api_key: String::new(),
            gcp_project: String::new(),
            gcp_region: default_gcp_region(),
            gcp_service_account_key_path: String::new(),
            sandbox_enabled: true,
            execution_model: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub session_id: String,
    pub prompt_short: String,
    pub ts: u64,
    pub task_count: usize,
    pub tokens: u64,
    pub tool_calls: u64,
    pub duration_secs: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitRepositoryInfo {
    pub path: String,
    pub name: String,
    pub branch: Option<String>,
    pub remote_url: Option<String>,
    pub source: String,
    pub repo_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAttachment {
    pub data_url: String,
    pub media_type: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryContext {
    pub project_path: String,
    pub repo_source: String,
    pub repo_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub summary: SessionSummary,
    pub messages: Vec<ChatMessage>,
    pub tasks: Vec<Task>,
    pub tokens_total: u64,
    pub tool_calls_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub messages: Vec<ChatMessage>,
    pub tasks: Vec<Task>,
    pub tokens_total: u64,
    pub tool_calls_total: u64,
}

impl From<SessionRecord> for SessionDetail {
    fn from(record: SessionRecord) -> Self {
        SessionDetail {
            summary: record.summary,
            messages: record.messages,
            tasks: record.tasks,
            tokens_total: record.tokens_total,
            tool_calls_total: record.tool_calls_total,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedState {
    pub version: u32,
    pub config: Config,
    pub current_session_id: Option<String>,
    pub sessions: HashMap<String, SessionRecord>,
    pub session_history: Vec<SessionSummary>,
    pub usage_history: Vec<UsageRecord>,
}

impl Default for PersistedState {
    fn default() -> Self {
        PersistedState {
            version: 1,
            config: Config::default(),
            current_session_id: None,
            sessions: HashMap::new(),
            session_history: Vec::new(),
            usage_history: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub action: FileAction,
    #[serde(default)]
    pub lines_added: usize,
    #[serde(default)]
    pub lines_deleted: usize,
    /// Best-effort previous file content (for diff view).
    #[serde(default)]
    pub before_content: Option<String>,
    #[serde(default)]
    pub content: Option<String>, // Full file content for diff view
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FileAction {
    Created,
    Modified,
    Deleted,
    Read,
    Moved,
}

#[derive(Debug)]
pub enum BridgeCommand {
    StartOrchestration {
        session_id: String,
        prompt: String,
        max_agents: u32,
        mode: String,
        repo_context: Option<RepositoryContext>,
        config: Config,
        /// Conversation history for context-aware responses. Each entry has "role" and "content".
        #[allow(dead_code)]
        conversation_history: Option<Vec<serde_json::Value>>,
    },
    /// Zero-Protection mode — deprecated, use agent_send_message instead.
    StartZeroMode {
        session_id: String,
        prompt: String,
        config: Config,
        workspace: serde_json::Value,
        images: Vec<ImageAttachment>,
    },
    /// Continue an existing zero mode session — deprecated, use agent_send_message instead.
    ContinueZeroMode {
        session_id: String,
        message: String,
        config: Config,
        workspace: serde_json::Value,
        images: Vec<ImageAttachment>,
    },
    StopOrchestration,
    CheckSocket,
}

#[derive(Debug)]
pub enum BridgeEvent {
    DaemonConnected,
    DaemonDisconnected,
    TaskAdded(Task),
    TaskUpdated {
        id: String,
        status: TaskStatus,
    },
    AgentChunk(String),
    AgentMessage(String),
    PlanReady {
        sandboxes: Vec<String>,
        task_count: usize,
        agent_count: usize,
        mode: String,
    },
    OrchestrationComplete,
    OrchestrationFailed(String),
    SimulationTick {
        tasks_done: usize,
        active_agents: usize,
        tokens_delta: u64,
    },
    /// Compact file change summary to show in chat (icon + filename)
    FileChanges(Vec<FileChange>),
    /// Structured tool call event (agent invoked a tool)
    ToolCall {
        worker_id: usize,
        tool_name: String,
        args_preview: String,
    },
    /// Structured tool result event (tool returned)
    ToolResult {
        worker_id: usize,
        tool_name: String,
        success: bool,
        preview: String,
    },
    /// Orchestration layer progress update
    LayerProgress {
        layer: u8,
        message: String,
    },
    /// LLM is thinking inside an agent tool loop
    LlmThinking {
        agent_id: String,
        task_description: String,
    },
    /// Agent status changed (running / complete / failed)
    AgentStatusChanged {
        agent_id: String,
        task_id: String,
        status: String,
        sandbox: String,
    },
    /// Routing gate decision emitted at the start of each orchestration run.
    RoutingDecision {
        mode: String,
        planning_model: String,
        execution_model: String,
    },
}
