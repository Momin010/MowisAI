//! Internal types for new 7-layer orchestration system

use agentd_protocol::{SandboxName, AgentHandle};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use tokio::sync::RwLock;

/// Dependency counter for scheduler (Layer 3)
#[derive(Debug)]
pub struct DepCounter {
    pub count: AtomicUsize,
}

impl DepCounter {
    pub fn new(count: usize) -> Self {
        Self {
            count: AtomicUsize::new(count),
        }
    }
}

/// Sandbox state tracker (Layer 2)
#[derive(Debug, Clone)]
pub struct SandboxState {
    pub name: SandboxName,
    pub base_layer_path: String,
    pub sandbox_layer_path: String,
    pub scope: String,
    pub tools: Vec<String>,
    pub max_agents: u32,
    pub active_agents: u32,
    pub idle_agents: Vec<AgentHandle>,
}

/// Agent pool for sandbox (Layer 2)
#[derive(Debug)]
pub struct AgentPool {
    pub agents: RwLock<Vec<AgentHandle>>,
    pub max_size: u32,
}

impl AgentPool {
    pub fn new(max_size: u32) -> Self {
        Self {
            agents: RwLock::new(Vec::new()),
            max_size,
        }
    }

    pub async fn take_idle(&self) -> Option<AgentHandle> {
        let mut agents = self.agents.write().await;
        agents.pop()
    }

    pub async fn return_idle(&self, agent: AgentHandle) {
        let mut agents = self.agents.write().await;
        if (agents.len() as u32) < self.max_size {
            agents.push(agent);
        }
    }

    pub async fn size(&self) -> usize {
        self.agents.read().await.len()
    }
}

/// Merge tree node for parallel merge (Layer 5)
#[derive(Debug, Clone)]
pub enum MergeNode {
    Leaf { diff: String },
    Branch { left: Box<MergeNode>, right: Box<MergeNode> },
}

/// Verification test task (Layer 6)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationTask {
    pub test_id: String,
    pub description: String,
    pub command: String,
    pub expected_result: Option<String>,
}

/// Verification function (Layer 6) - planned once, executed every round
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationFunction {
    pub id: String,
    pub description: String,
    pub command: String,
    pub expected_schema: Option<String>,
    pub assertion: Option<String>,
    pub timeout_secs: u64,
}

/// Result of running a verification function
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfResult {
    pub vf_id: String,
    pub passed: bool,
    pub output: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
}

/// Fix task generated from verification failure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixTask {
    pub id: String,
    pub description: String,
    pub target_sandbox: SandboxName,
    pub related_vf_id: String,
    pub failure_output: String,
}

/// Merge result (Layer 5)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeResult {
    pub success: bool,
    pub merged_diff: String,
    pub conflicts: Vec<MergeConflict>,
    pub strategy_used: MergeStrategy,
}

/// Merge conflict information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeConflict {
    pub file_path: String,
    pub description: String,
    pub severity: ConflictSeverity,
    pub resolved: bool,
    pub resolution: Option<String>,
}

/// Conflict severity levels
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConflictSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Merge strategies
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MergeStrategy {
    Auto,
    Manual,
    LlmAssisted,
}

/// Agent contribution to a merge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContribution {
    pub agent_id: String,
    pub task_id: String,
    pub diff: String,
    pub files_changed: Vec<String>,
    pub lines_added: u32,
    pub lines_removed: u32,
}

/// Health status of an agent
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentHealth {
    Healthy,
    Degraded,
    Unresponsive,
    Failed,
}

/// Circuit breaker states
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

/// Project context for interactive orchestration sessions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectContext {
    pub project_id: String,
    pub description: String,
    pub languages: Vec<String>,
    pub frameworks: Vec<String>,
    pub notes: Vec<String>,
}

/// Warm sandbox state for session persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxWarmState {
    pub sandbox_id: String,
    pub container_id: Option<String>,
    pub paused_at: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Complexity classifier (consolidated from former complexity_classifier.rs)
// ─────────────────────────────────────────────────────────────────────────────

use std::collections::HashSet;

/// Which orchestration mode to use for this task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComplexityMode {
    /// Single agent, no planner, no merge, no verification.
    Simple,
    /// Multi-agent within one sandbox, 1 verification round, no cross-sandbox.
    Standard,
    /// Full 7-layer pipeline — multiple sandboxes, full verification loop.
    Full,
}

impl ComplexityMode {
    /// Parse from a CLI string (`simple`, `standard`, `full`).
    /// Case-insensitive. Returns `None` on unknown value.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "simple" => Some(Self::Simple),
            "standard" => Some(Self::Standard),
            "full" => Some(Self::Full),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Standard => "standard",
            Self::Full => "full",
        }
    }
}

impl std::fmt::Display for ComplexityMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Intermediate scoring breakdown — useful for logging / debugging.
#[derive(Debug, Clone)]
pub struct ComplexityScore {
    /// Estimated number of distinct domains inferred from the directory tree
    pub domain_count: usize,
    /// Estimated file count from the directory tree
    pub file_count: usize,
    /// Whether the prompt contains broad-scope keywords
    pub broad_scope: bool,
    /// Whether the prompt contains cross-service / integration keywords
    pub cross_service: bool,
    /// Raw integer score [0, 2]
    pub score: u8,
    /// Final mode derived from score (before any user override)
    pub mode: ComplexityMode,
}

/// Phrases that indicate the user wants to build something brand new from
/// scratch. When these appear, the existing codebase size is irrelevant and
/// should not inflate the complexity score.
const GREENFIELD_KEYWORDS: &[&str] = &[
    "for my ", "for our ",
    "a new ", "an new ", "brand new", "from scratch",
    "a website", "a web app", "a webapp", "a landing page",
    "a dashboard", "a portfolio", "a blog", "an app",
    "a mobile app", "a cli", "a script", "a tool",
    "a chatbot", "a bot", "a game", "an api",
    "a rest api", "a graphql api",
    "a plugin", "an extension",
    "a page", "a form", "a component",
    "a service", "a microservice",
];

/// Phrases that indicate the request is for a single, self-contained output.
const SINGLE_ARTIFACT_KEYWORDS: &[&str] = &[
    "website", "landing page", "web page", "webpage",
    "portfolio", "blog", "dashboard", "admin panel",
    "mobile app", "ios app", "android app",
    "chrome extension", "browser extension", "vscode plugin",
    "cli tool", "command line tool",
    "a script", "a bot", "a chatbot",
    "a game", "a calculator",
];

/// Top-level directory names that count as distinct "domains".
const DOMAIN_DIRS: &[&str] = &[
    "frontend", "backend", "api", "server", "client", "web", "mobile",
    "ios", "android", "infra", "infrastructure", "terraform", "k8s",
    "kubernetes", "docker", "services", "microservices", "packages",
    "apps", "libs", "shared", "common", "core", "platform",
];

/// Broad-scope keywords in the prompt imply large surface area → raise score.
const BROAD_SCOPE_KEYWORDS: &[&str] = &[
    "entire", "all", "whole", "full", "complete", "migrate", "migration",
    "refactor", "rewrite", "architecture", "redesign", "overhaul",
    "system", "platform", "rebuild", "from scratch",
];

/// Cross-service keywords imply touching multiple independent systems → Full.
const CROSS_SERVICE_KEYWORDS: &[&str] = &[
    "integration", "end-to-end", "e2e", "cross-service", "cross service",
    "microservice", "service mesh", "api gateway", "inter-service",
    "frontend.*backend", "backend.*frontend", "full.?stack", "fullstack",
];

/// Single-action keywords in the prompt strongly suggest Mode 1 (Simple).
const SIMPLE_ACTION_KEYWORDS: &[&str] = &[
    "fix", "rename", "typo", "add a", "add an", "update a", "update an",
    "change a", "change an", "delete a", "delete an", "remove a", "remove an",
    "move a", "move an", "correct", "tweak", "adjust", "format",
    "comment", "uncomment", "patch",
];

/// Classify the complexity of a task from the user prompt and the pre-scanned
/// directory tree string. Pure heuristics, no I/O, ~1ms.
pub fn classify_complexity(prompt: &str, dir_tree: &str) -> ComplexityScore {
    let prompt_lower = prompt.to_lowercase();

    let file_count = count_files(dir_tree);
    let domain_count = count_domains(dir_tree);

    let broad_scope = BROAD_SCOPE_KEYWORDS
        .iter()
        .any(|kw| prompt_lower.contains(kw));

    let cross_service = CROSS_SERVICE_KEYWORDS
        .iter()
        .any(|kw| regex_contains(&prompt_lower, kw));

    let simple_action = SIMPLE_ACTION_KEYWORDS
        .iter()
        .any(|kw| prompt_lower.contains(kw));

    let is_greenfield = GREENFIELD_KEYWORDS
        .iter()
        .any(|kw| prompt_lower.contains(kw));

    let is_single_artifact = SINGLE_ARTIFACT_KEYWORDS
        .iter()
        .any(|kw| prompt_lower.contains(kw));

    let score: u8 = if cross_service {
        2
    } else {
        let mut s: u8 = 0;

        if !is_greenfield {
            if file_count > 10 {
                s += 1;
            }
            if file_count > 50 {
                s += 1;
            }
            if domain_count >= 3 {
                s = s.max(2);
            } else if domain_count == 2 {
                s = s.max(1);
            }
        }

        if broad_scope && !is_single_artifact {
            s = s.max(1);
        }

        if is_single_artifact && s > 1 {
            s = 1;
        }

        if simple_action && file_count <= 20 && domain_count <= 1 {
            s = 0;
        }

        s.min(2)
    };

    let mode = match score {
        0 => ComplexityMode::Simple,
        1 => ComplexityMode::Standard,
        _ => ComplexityMode::Full,
    };

    ComplexityScore {
        domain_count,
        file_count,
        broad_scope,
        cross_service,
        score,
        mode,
    }
}

fn count_files(dir_tree: &str) -> usize {
    dir_tree
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return false;
            }
            let name = trimmed
                .trim_start_matches(|c: char| {
                    c == '├' || c == '└' || c == '─' || c == '│' || c == ' '
                });
            !name.is_empty() && !name.ends_with('/')
        })
        .count()
}

fn count_domains(dir_tree: &str) -> usize {
    let mut seen: HashSet<String> = HashSet::new();

    for line in dir_tree.lines() {
        let trimmed = line.trim().to_lowercase();
        let clean = trimmed
            .trim_start_matches(|c: char| c == '├' || c == '└' || c == '─' || c == '│' || c == ' ')
            .trim_end_matches('/')
            .to_string();

        if DOMAIN_DIRS.contains(&clean.as_str()) {
            seen.insert(clean);
        }
    }

    seen.len()
}

fn regex_contains(text: &str, pattern: &str) -> bool {
    if !pattern.contains(".*") && !pattern.contains(".?") {
        return text.contains(pattern);
    }

    let parts: Vec<&str> = pattern.split(".*").collect();
    if parts.len() == 1 {
        let sub = pattern.replace(".?", "");
        return text.contains(&sub);
    }

    let mut pos = 0;
    for part in &parts {
        let sub = part.replace(".?", "");
        if sub.is_empty() {
            continue;
        }
        match text[pos..].find(&sub) {
            Some(idx) => pos += idx + sub.len(),
            None => return false,
        }
    }
    true
}
