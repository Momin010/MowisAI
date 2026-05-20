use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::collections::HashMap;

use crate::config::ModelRef;
use crate::providers::Provider;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub plan_id: PlanId,
    pub created_at: String,
    #[serde(alias = "conversation")]
    pub conversation_id: String,
    pub current_version: u32,
    pub status: PlanStatus,
    pub user_goal: String,
    #[serde(default)]
    pub overview: String,
    #[serde(default)]
    pub task_graph: TaskGraph,
    #[serde(default)]
    pub sandbox_config: SandboxConfig,
    #[serde(default)]
    pub models_config: ModelsConfig,
    #[serde(default)]
    pub tools_config: ToolsConfig,
    #[serde(skip)]
    pub plans_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlanStatus {
    Draft,
    AwaitingUser,
    Approved,
    Running,
    Done,
    Aborted,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskGraph {
    pub tasks: Vec<TaskNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    pub id: TaskId,
    pub title: String,
    pub description: String,
    pub deps: Vec<TaskId>,
    pub model_tier: ModelTier,
    pub tool_budget: u32,
    pub files_hint: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ModelTier {
    Fast,
    Mid,
    Flagship,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub image_rootfs: String,
    pub ram_mb: u32,
    pub cpu_millis: u32,
    pub overlay_ram_mb: u32,
    pub overlay_cpu_millis: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsConfig {
    pub tiers: HashMap<String, ModelRef>,
    pub task_overrides: HashMap<String, TaskModelOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskModelOverride {
    pub tier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfig {
    pub allow_extra: HashMap<String, Vec<String>>,
    pub deny: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PlanId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TaskId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tier {
    Conductor,
    Critic,
    Captain,
    Crew,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAllowlist {
    pub allowed: Vec<String>,
    pub denied: Vec<String>,
}

impl ToolAllowlist {
    pub fn allows(&self, tool_name: &str) -> bool {
        if self.denied.iter().any(|d| tool_name.starts_with(d)) {
            return false;
        }
        self.allowed.is_empty() || self.allowed.iter().any(|a| a == tool_name)
    }
}

impl Plan {
    pub fn new_draft(plan_id: PlanId, user_goal: &str, conversation_id: &str) -> Self {
        Self {
            plan_id,
            created_at: chrono::Utc::now().to_rfc3339(),
            conversation_id: conversation_id.to_string(),
            current_version: 1,
            status: PlanStatus::Draft,
            user_goal: user_goal.to_string(),
            overview: String::new(),
            task_graph: TaskGraph { tasks: vec![] },
            sandbox_config: SandboxConfig::default(),
            models_config: ModelsConfig::default(),
            tools_config: ToolsConfig::default(),
            plans_dir: PathBuf::from(".mowis/plans"),
        }
    }

    pub fn load(plans_dir: &Path, plan_id: &PlanId) -> Result<Self> {
        let plan_dir = plans_dir.join(&plan_id.0);
        let plan_toml = std::fs::read_to_string(plan_dir.join("plan.toml"))?;
        let mut plan: Plan = toml::from_str(&plan_toml)?;
        plan.plans_dir = plans_dir.to_path_buf();

        let overview = std::fs::read_to_string(plan_dir.join("overview.md")).unwrap_or_default();
        plan.overview = overview;

        let tasks_toml = std::fs::read_to_string(plan_dir.join("tasks.toml")).unwrap_or_default();
        if !tasks_toml.is_empty() {
            let task_graph: TaskGraph = toml::from_str(&tasks_toml)?;
            plan.task_graph = task_graph;
        }

        let sandbox_toml = std::fs::read_to_string(plan_dir.join("sandbox.toml")).unwrap_or_default();
        if !sandbox_toml.is_empty() {
            plan.sandbox_config = toml::from_str(&sandbox_toml)?;
        }

        let models_toml = std::fs::read_to_string(plan_dir.join("models.toml")).unwrap_or_default();
        if !models_toml.is_empty() {
            plan.models_config = toml::from_str(&models_toml)?;
        }

        let tools_toml = std::fs::read_to_string(plan_dir.join("tools.toml")).unwrap_or_default();
        if !tools_toml.is_empty() {
            plan.tools_config = toml::from_str(&tools_toml)?;
        }

        Ok(plan)
    }

    pub fn save(&self) -> Result<()> {
        let plan_dir = self.plans_dir.join(&self.plan_id.0);
        std::fs::create_dir_all(&plan_dir)?;

        Self::atomic_write(&plan_dir.join("plan.toml"), &toml::to_string_pretty(&PlanToml {
            plan_id: self.plan_id.clone(),
            created_at: self.created_at.clone(),
            conversation_id: self.conversation_id.clone(),
            current_version: self.current_version,
            status: self.status.clone(),
            user_goal: self.user_goal.clone(),
        })?)?;

        Self::atomic_write(&plan_dir.join("overview.md"), &self.overview)?;
        Self::atomic_write(&plan_dir.join("tasks.toml"), &toml::to_string_pretty(&self.task_graph)?)?;
        Self::atomic_write(&plan_dir.join("sandbox.toml"), &toml::to_string_pretty(&self.sandbox_config)?)?;
        Self::atomic_write(&plan_dir.join("models.toml"), &toml::to_string_pretty(&self.models_config)?)?;
        Self::atomic_write(&plan_dir.join("tools.toml"), &toml::to_string_pretty(&self.tools_config)?)?;

        Ok(())
    }

    fn atomic_write(path: &Path, content: &str) -> Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, content)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn snapshot_to_history(&mut self) -> Result<()> {
        let plan_dir = self.plans_dir.join(&self.plan_id.0);
        let history_dir = plan_dir.join("history").join(format!("v{}", self.current_version));
        std::fs::create_dir_all(&history_dir)?;

        for name in &["overview.md", "tasks.toml", "sandbox.toml", "models.toml", "tools.toml"] {
            let src = plan_dir.join(name);
            if src.exists() {
                std::fs::copy(&src, history_dir.join(name))?;
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        let mut seen = std::collections::HashSet::new();
        for task in &self.task_graph.tasks {
            if !seen.insert(&task.id) {
                anyhow::bail!("duplicate task id: {}", task.id.0);
            }
            for dep in &task.deps {
                if !self.task_graph.tasks.iter().any(|t| &t.id == dep) {
                    anyhow::bail!("task {} depends on unknown task {}", task.id.0, dep.0);
                }
            }
        }
        if self.has_cycle() {
            anyhow::bail!("task graph contains a cycle");
        }
        Ok(())
    }

    fn has_cycle(&self) -> bool {
        let mut visited = std::collections::HashSet::new();
        let mut in_stack = std::collections::HashSet::new();

        for task in &self.task_graph.tasks {
            if self.dfs_cycle(&task.id, &mut visited, &mut in_stack) {
                return true;
            }
        }
        false
    }

    fn dfs_cycle(
        &self,
        task_id: &TaskId,
        visited: &mut std::collections::HashSet<TaskId>,
        in_stack: &mut std::collections::HashSet<TaskId>,
    ) -> bool {
        if in_stack.contains(task_id) {
            return true;
        }
        if visited.contains(task_id) {
            return false;
        }
        visited.insert(task_id.clone());
        in_stack.insert(task_id.clone());

        if let Some(task) = self.task_graph.tasks.iter().find(|t| &t.id == task_id) {
            for dep in &task.deps {
                if self.dfs_cycle(dep, visited, in_stack) {
                    return true;
                }
            }
        }

        in_stack.remove(task_id);
        false
    }

    pub fn task_graph(&self) -> &TaskGraph {
        &self.task_graph
    }

    pub fn sandbox_spec(&self) -> &SandboxConfig {
        &self.sandbox_config
    }

    pub fn model_for(&self, tier: &Tier, task: Option<&TaskId>) -> ModelRef {
        if let Some(task_id) = task {
            if let Some(task_node) = self.task_graph.tasks.iter().find(|t| &t.id == task_id) {
                if let Some(overrides) = self.models_config.task_overrides.get(&task_id.0) {
                    if let Some(ref tier_str) = overrides.tier {
                        let key = format!("tier.{}", tier_str);
                        if let Some(model_ref) = self.models_config.tiers.get(&key) {
                            return model_ref.clone();
                        }
                    }
                }
            }
        }
        let key = format!("tier.{:?}", tier).to_lowercase();
        self.models_config
            .tiers
            .get(&key)
            .cloned()
            .unwrap_or_else(|| ModelRef {
                provider: Provider::Anthropic,
                model: "claude-haiku-4-5-20251001".into(),
            })
    }

    pub fn tool_allowlist(&self, tier: &Tier) -> ToolAllowlist {
        let tier_key = format!("{:?}", tier).to_lowercase();
        let allow_extra = self
            .tools_config
            .allow_extra
            .get(&tier_key)
            .cloned()
            .unwrap_or_default();
        let denied = self
            .tools_config
            .deny
            .get(&tier_key)
            .cloned()
            .unwrap_or_default();
        ToolAllowlist {
            allowed: allow_extra,
            denied,
        }
    }
}

#[derive(Serialize)]
struct PlanToml {
    plan_id: PlanId,
    created_at: String,
    conversation_id: String,
    current_version: u32,
    status: PlanStatus,
    user_goal: String,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            image_rootfs: ".mowis-cache/rootfs/ubuntu-24.04".into(),
            ram_mb: 8192,
            cpu_millis: 4000,
            overlay_ram_mb: 1024,
            overlay_cpu_millis: 1000,
        }
    }
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self {
            tiers: HashMap::new(),
            task_overrides: HashMap::new(),
        }
    }
}

impl std::fmt::Display for PlanStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanStatus::Draft => write!(f, "draft"),
            PlanStatus::AwaitingUser => write!(f, "awaiting_user"),
            PlanStatus::Approved => write!(f, "approved"),
            PlanStatus::Running => write!(f, "running"),
            PlanStatus::Done => write!(f, "done"),
            PlanStatus::Aborted => write!(f, "aborted"),
        }
    }
}
