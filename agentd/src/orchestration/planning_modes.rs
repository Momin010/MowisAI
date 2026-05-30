//! Planning Modes — Different strategies for task decomposition and execution
//!
//! Each mode optimizes for different goals:
//! - Fast: Minimal planning, maximum parallelism, best for simple tasks
//! - Thorough: Deep analysis, dependency optimization, best for complex tasks  
//! - Adaptive: Starts fast, escalates to thorough if needed
//! - Auto: ML-based selection of mode based on task characteristics
//! - Stream: Real-time planning with progressive refinement

use agentd_protocol::{SandboxConfig, SandboxTopology, Task, TaskGraph};
use serde::{Deserialize, Serialize};

/// Planning mode selection
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlanningMode {
    /// Minimal planning, max parallelism. Good for simple/batch tasks.
    /// - Shell scan only, no LLM for topology
    /// - Single sandbox, all agents
    /// - Skip verification
    Fast,

    /// Deep analysis with dependency optimization.
    /// - Full LLM planning with code analysis
    /// - Multi-sandbox topology
    /// - Full verification loop
    /// - Merge conflict resolution
    Thorough,

    /// Starts fast, escalates if complexity detected.
    /// - Begins with Fast mode
    /// - Monitors error rate and task complexity
    /// - Switches to Thorough if errors exceed threshold
    Adaptive,

    /// Automatic mode selection based on task analysis.
    /// - Analyzes prompt keywords, file count, language diversity
    /// - Selects optimal mode automatically
    /// - Can combine modes for different task phases
    Auto,

    /// Real-time streaming mode with progressive refinement.
    /// - Plans and executes simultaneously
    /// - Streams results to UI as they complete
    /// - Refines plan based on intermediate results
    Stream,
}

impl PlanningMode {
    /// Parse from string (case-insensitive)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "fast" | "quick" | "speed" => Some(Self::Fast),
            "thorough" | "deep" | "careful" | "full" => Some(Self::Thorough),
            "adaptive" | "smart" | "auto-escalate" => Some(Self::Adaptive),
            "auto" | "automatic" | "ai" => Some(Self::Auto),
            "stream" | "streaming" | "realtime" | "real-time" => Some(Self::Stream),
            _ => None,
        }
    }

    /// Get max agents per sandbox for this mode
    pub fn max_agents_per_sandbox(&self) -> u32 {
        match self {
            Self::Fast => 100,
            Self::Thorough => 50,
            Self::Adaptive => 75,
            Self::Auto => 60,
            Self::Stream => 30,
        }
    }

    /// Get max sandboxes for this mode
    pub fn max_sandboxes(&self) -> u32 {
        match self {
            Self::Fast => 1,
            Self::Thorough => 10,
            Self::Adaptive => 5,
            Self::Auto => 8,
            Self::Stream => 3,
        }
    }

    /// Whether to run verification after completion
    pub fn should_verify(&self) -> bool {
        match self {
            Self::Fast => false,
            Self::Thorough => true,
            Self::Adaptive => true,
            Self::Auto => true,
            Self::Stream => false,
        }
    }

    /// Whether to run merge conflict resolution
    pub fn should_merge(&self) -> bool {
        match self {
            Self::Fast => false,
            Self::Thorough => true,
            Self::Adaptive => true,
            Self::Auto => true,
            Self::Stream => false,
        }
    }

    /// Max verification rounds
    pub fn max_verify_rounds(&self) -> usize {
        match self {
            Self::Fast => 0,
            Self::Thorough => 5,
            Self::Adaptive => 3,
            Self::Auto => 3,
            Self::Stream => 1,
        }
    }

    /// Task timeout multiplier (1.0 = default)
    pub fn timeout_multiplier(&self) -> f64 {
        match self {
            Self::Fast => 0.5,
            Self::Thorough => 2.0,
            Self::Adaptive => 1.5,
            Self::Auto => 1.5,
            Self::Stream => 1.0,
        }
    }

    /// Whether to use LLM for planning (vs shell scan only)
    pub fn uses_llm_planning(&self) -> bool {
        match self {
            Self::Fast => false,
            Self::Thorough => true,
            Self::Adaptive => true,
            Self::Auto => true,
            Self::Stream => true,
        }
    }
}

/// Configuration for planning behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningConfig {
    pub mode: PlanningMode,
    /// Override max agents per sandbox
    pub max_agents_override: Option<u32>,
    /// Override max sandboxes
    pub max_sandboxes_override: Option<u32>,
    /// Force verification even in Fast mode
    pub force_verification: bool,
    /// Force merge even in Fast mode
    pub force_merge: bool,
    /// Custom system prompt for agents
    pub custom_system_prompt: Option<String>,
    /// Agent template to use
    pub agent_template: Option<String>,
    /// Budget limit in USD (0 = unlimited)
    pub budget_limit_usd: f64,
    /// Preferred model (overrides auto-selection)
    pub preferred_model: Option<String>,
    /// Enable streaming to UI
    pub enable_streaming: bool,
    /// Workspace scope (restrict to specific directories)
    pub workspace_scope: Option<Vec<String>>,
    /// Exclude patterns (glob patterns to skip)
    pub exclude_patterns: Vec<String>,
    /// Priority (1-10, higher = more resources)
    pub priority: u32,
    /// Enable agent-to-agent communication
    pub enable_agent_communication: bool,
    /// Enable memory persistence across sessions
    pub enable_memory: bool,
    /// Custom environment variables for agents
    pub env_vars: Vec<(String, String)>,
}

impl Default for PlanningConfig {
    fn default() -> Self {
        Self {
            mode: PlanningMode::Auto,
            max_agents_override: None,
            max_sandboxes_override: None,
            force_verification: false,
            force_merge: false,
            custom_system_prompt: None,
            agent_template: None,
            budget_limit_usd: 0.0,
            preferred_model: None,
            enable_streaming: true,
            workspace_scope: None,
            exclude_patterns: vec![
                ".git/**".to_string(),
                "node_modules/**".to_string(),
                "target/**".to_string(),
                "__pycache__/**".to_string(),
                "*.pyc".to_string(),
                ".env".to_string(),
            ],
            priority: 5,
            enable_agent_communication: false,
            enable_memory: true,
            env_vars: Vec::new(),
        }
    }
}

/// Analyze a task prompt and recommend the best planning mode
pub fn recommend_mode(prompt: &str, file_count: usize, language_count: usize) -> PlanningMode {
    let lower = prompt.to_lowercase();

    // Simple heuristics for mode selection
    let complexity_score = calculate_complexity(&lower, file_count, language_count);

    if complexity_score <= 2 {
        PlanningMode::Fast
    } else if complexity_score <= 5 {
        PlanningMode::Adaptive
    } else {
        PlanningMode::Thorough
    }
}

fn calculate_complexity(prompt: &str, file_count: usize, language_count: usize) -> u32 {
    let mut score = 0u32;

    // File count contribution
    if file_count > 10 {
        score += 1;
    }
    if file_count > 30 {
        score += 1;
    }
    if file_count > 100 {
        score += 2;
    }

    // Language diversity
    if language_count > 2 {
        score += 1;
    }
    if language_count > 4 {
        score += 1;
    }

    // Complexity keywords
    let complex_keywords = [
        "refactor",
        "architecture",
        "migration",
        "database",
        "schema",
        "security",
        "authentication",
        "authorization",
        "encryption",
        "distributed",
        "microservice",
        "kubernetes",
        "docker",
        "ci/cd",
        "pipeline",
        "deployment",
        "infrastructure",
        "testing",
        "integration test",
        "e2e",
        "performance",
        "optimization",
        "caching",
        "scaling",
        "concurrency",
    ];

    for keyword in &complex_keywords {
        if prompt.contains(keyword) {
            score += 1;
        }
    }

    // Simple keywords (reduce complexity)
    let simple_keywords = [
        "fix typo",
        "rename",
        "update comment",
        "add comment",
        "change color",
        "update readme",
        "bump version",
    ];

    for keyword in &simple_keywords {
        if prompt.contains(keyword) {
            score = score.saturating_sub(2);
        }
    }

    score
}

/// Adaptive mode state tracker
pub struct AdaptiveState {
    pub current_mode: PlanningMode,
    pub error_count: u32,
    pub success_count: u32,
    pub escalation_threshold: f64,
}

impl AdaptiveState {
    pub fn new() -> Self {
        Self {
            current_mode: PlanningMode::Fast,
            error_count: 0,
            success_count: 0,
            escalation_threshold: 0.3, // 30% error rate triggers escalation
        }
    }

    /// Report task result and potentially escalate
    pub fn report_result(&mut self, success: bool) {
        if success {
            self.success_count += 1;
        } else {
            self.error_count += 1;
        }

        let total = self.error_count + self.success_count;
        if total >= 3 {
            let error_rate = self.error_count as f64 / total as f64;
            if error_rate > self.escalation_threshold && self.current_mode == PlanningMode::Fast {
                log::info!(
                    "[Adaptive] Escalating from Fast to Thorough (error rate: {:.1}%)",
                    error_rate * 100.0
                );
                self.current_mode = PlanningMode::Thorough;
            }
        }
    }

    pub fn should_escalate(&self) -> bool {
        self.current_mode == PlanningMode::Fast && self.error_count > 0
    }
}
