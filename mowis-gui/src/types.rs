use serde::{Deserialize, Serialize};

// ── Task graph ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Pending,
    Running,
    Complete,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub description: String,
    pub sandbox: Option<String>,
    pub status: TaskStatus,
}

// ── Chat messages ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum MessageRole {
    User,
    Agent,
    System,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: MessageRole,
    /// Full accumulated content (including streamed chunks).
    pub content: String,
    /// True while the agent is still streaming this message.
    pub streaming: bool,
    pub timestamp: chrono::DateTime<chrono::Local>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            streaming: false,
            timestamp: chrono::Local::now(),
        }
    }

    pub fn agent_start() -> Self {
        Self {
            role: MessageRole::Agent,
            content: String::new(),
            streaming: true,
            timestamp: chrono::Local::now(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
            streaming: false,
            timestamp: chrono::Local::now(),
        }
    }
}

// ── Diff view ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum DiffLineKind {
    Added,
    Removed,
    Context,
    Header,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub content: String,
    pub kind: DiffLineKind,
}

#[derive(Debug, Clone)]
pub struct FileDiff {
    pub path: String,
    pub additions: usize,
    pub deletions: usize,
    pub lines: Vec<DiffLine>,
}

impl FileDiff {
    pub fn parse(path: impl Into<String>, raw_diff: &str) -> Self {
        let path = path.into();
        let mut lines = Vec::new();
        let mut additions = 0usize;
        let mut deletions = 0usize;

        for line in raw_diff.lines() {
            let kind = if line.starts_with("@@") {
                DiffLineKind::Header
            } else if line.starts_with('+') {
                additions += 1;
                DiffLineKind::Added
            } else if line.starts_with('-') {
                deletions += 1;
                DiffLineKind::Removed
            } else {
                DiffLineKind::Context
            };
            lines.push(DiffLine { content: line.to_string(), kind });
        }

        Self { path, additions, deletions, lines }
    }
}

// ── Backend <-> Frontend events ───────────────────────────────────────────────

#[derive(Debug)]
pub enum BackendEvent {
    DaemonStarting,
    DaemonProgress { message: String, percent: Option<u8> },
    DaemonStarted,
    DaemonFailed(String),
    /// A new task appeared in the task graph.
    TaskAdded(Task),
    /// A task changed status.
    TaskUpdated { id: String, status: TaskStatus },
    /// Full streaming chunk from the agent.
    AgentChunk(String),
    /// A follow-up agent reply (non-streaming).
    AgentMessage(String),
    /// A file diff was written / updated by an agent.
    DiffUpdated(FileDiff),
    /// Orchestration finished.
    OrchestrationComplete,
    /// Orchestration failed with an error.
    OrchestrationFailed(String),
}

#[derive(Debug)]
pub enum FrontendCommand {
    StartOrchestration { prompt: String },
    SendFollowUp { content: String },
    StopOrchestration,
}
