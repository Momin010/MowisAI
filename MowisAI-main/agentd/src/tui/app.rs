use crate::orchestration::new_orchestrator::OrchestratorEvent;
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Debug, Clone, PartialEq)]
pub enum AppView {
    Overview,
    SandboxDetail(String),
    AgentDetail(String),
    CommandInput,
    ErrorLog,
}

#[derive(Debug, Clone)]
pub struct SandboxState {
    pub name: String,
    pub active_agents: usize,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct AgentState {
    pub worker_id: usize,
    pub id: String,
    pub sandbox: String,
    pub task_description: String,
    pub status: String,
    pub current_tool: Option<String>,
    pub tool_history: Vec<ToolCallEntry>,
    pub elapsed_secs: u64,
    pub diff_size: usize,
    pub started_tick: u64,
}

#[derive(Debug, Clone)]
pub struct ToolCallEntry {
    pub tool_name: String,
    pub timestamp: u64,
    pub success: bool,
    pub preview: String,
}

#[derive(Debug, Clone, Default)]
pub struct OrchestratorStats {
    pub total_tasks: usize,
    pub completed: usize,
    pub failed: usize,
    pub running: usize,
    pub pending: usize,
    pub elapsed_secs: u64,
    pub agents_spawned: usize,
}

pub struct App {
    pub view: AppView,
    pub sandboxes: Vec<SandboxState>,
    pub agents: Vec<AgentState>,
    pub stats: OrchestratorStats,
    pub errors: Vec<String>,
    pub command_history: Vec<String>,
    pub current_command: String,
    pub should_quit: bool,
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub activity_log: Vec<String>,
    pub orchestrator_done: bool,
    pub tick_count: u64,
    pub layer_messages: Vec<String>,
}

impl App {
    pub fn new() -> Self {
        Self {
            view: AppView::Overview,
            sandboxes: Vec::new(),
            agents: Vec::new(),
            stats: OrchestratorStats::default(),
            errors: Vec::new(),
            command_history: Vec::new(),
            current_command: String::new(),
            should_quit: false,
            selected_index: 0,
            scroll_offset: 0,
            activity_log: Vec::new(),
            orchestrator_done: false,
            tick_count: 0,
            layer_messages: Vec::new(),
        }
    }

    pub fn on_tick(&mut self) {
        self.tick_count += 1;
        self.stats.elapsed_secs = self.tick_count / 10; // 100ms ticks → seconds

        // Update elapsed for running agents
        for agent in &mut self.agents {
            if agent.status == "thinking" || agent.status == "executing_tool" {
                agent.elapsed_secs = self.tick_count.saturating_sub(agent.started_tick) / 10;
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match &self.view {
            AppView::Overview => self.handle_overview_key(key),
            AppView::SandboxDetail(_) => self.handle_detail_key(key),
            AppView::AgentDetail(_) => self.handle_agent_detail_key(key),
            AppView::CommandInput => self.handle_command_key(key),
            AppView::ErrorLog => self.handle_error_log_key(key),
        }
    }

    fn handle_overview_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => self.should_quit = true,
            KeyCode::Char('/') => {
                self.view = AppView::CommandInput;
                self.current_command.clear();
            }
            KeyCode::Char('e') => self.view = AppView::ErrorLog,
            KeyCode::Tab => {}
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = self.sandboxes.len().saturating_sub(1);
                if self.selected_index < max {
                    self.selected_index += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(sb) = self.sandboxes.get(self.selected_index) {
                    let name = sb.name.clone();
                    self.view = AppView::SandboxDetail(name);
                    self.selected_index = 0;
                }
            }
            _ => {}
        }
    }

    fn handle_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => self.should_quit = true,
            KeyCode::Esc => {
                self.view = AppView::Overview;
                self.selected_index = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let AppView::SandboxDetail(ref sb_name) = self.view.clone() {
                    let count = self.agents_for_sandbox(sb_name).len().saturating_sub(1);
                    if self.selected_index < count {
                        self.selected_index += 1;
                    }
                }
            }
            KeyCode::Enter => {
                if let AppView::SandboxDetail(ref sb_name) = self.view.clone() {
                    let agents = self.agents_for_sandbox(sb_name);
                    if let Some(agent) = agents.get(self.selected_index) {
                        let agent_id = agent.id.clone();
                        self.view = AppView::AgentDetail(agent_id);
                        self.selected_index = 0;
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_agent_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => self.should_quit = true,
            KeyCode::Esc => {
                let prev_sandbox = self
                    .agents
                    .iter()
                    .find(|a| {
                        if let AppView::AgentDetail(ref id) = self.view {
                            a.id == *id
                        } else {
                            false
                        }
                    })
                    .map(|a| a.sandbox.clone())
                    .unwrap_or_default();
                self.view = if prev_sandbox.is_empty() {
                    AppView::Overview
                } else {
                    AppView::SandboxDetail(prev_sandbox)
                };
                self.selected_index = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_offset += 1;
            }
            _ => {}
        }
    }

    fn handle_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.view = AppView::Overview;
                self.current_command.clear();
            }
            KeyCode::Enter => {
                let cmd = self.current_command.trim().to_string();
                if !cmd.is_empty() {
                    self.command_history.push(cmd);
                    self.current_command.clear();
                }
                self.view = AppView::Overview;
            }
            KeyCode::Backspace => {
                self.current_command.pop();
            }
            KeyCode::Char(c) => {
                self.current_command.push(c);
            }
            _ => {}
        }
    }

    fn handle_error_log_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                self.view = AppView::Overview;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_offset += 1;
            }
            _ => {}
        }
    }

    pub fn agents_for_sandbox<'a>(&'a self, sandbox_name: &str) -> Vec<&'a AgentState> {
        self.agents
            .iter()
            .filter(|a| a.sandbox == sandbox_name)
            .collect()
    }

    pub fn handle_orchestrator_event(&mut self, event: OrchestratorEvent) {
        match event {
            OrchestratorEvent::TaskStarted {
                worker_id,
                task_id,
                description,
                sandbox,
            } => {
                // Ensure sandbox exists in sandboxes list
                if !self.sandboxes.iter().any(|s| s.name == sandbox) {
                    self.sandboxes.push(SandboxState {
                        name: sandbox.clone(),
                        active_agents: 0,
                        completed_tasks: 0,
                        failed_tasks: 0,
                        status: "active".to_string(),
                    });
                }
                // Increment active agents
                if let Some(sb) = self.sandboxes.iter_mut().find(|s| s.name == sandbox) {
                    sb.active_agents += 1;
                    sb.status = "active".to_string();
                }
                // Add agent
                let short_id = format!("{}..{}", &task_id[..task_id.len().min(4)], worker_id);
                self.agents.push(AgentState {
                    worker_id,
                    id: short_id,
                    sandbox: sandbox.clone(),
                    task_description: description.clone(),
                    status: "thinking".to_string(),
                    current_tool: None,
                    tool_history: Vec::new(),
                    elapsed_secs: 0,
                    diff_size: 0,
                    started_tick: self.tick_count,
                });
                self.stats.agents_spawned += 1;
                self.activity_log
                    .push(format!("Worker {}: started \"{}\"", worker_id, description));
                // Keep log bounded
                if self.activity_log.len() > 100 {
                    self.activity_log.remove(0);
                }
            }
            OrchestratorEvent::ToolCall {
                worker_id,
                tool_name,
                args_preview,
            } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.worker_id == worker_id) {
                    agent.current_tool = Some(tool_name.clone());
                    agent.status = "executing_tool".to_string();
                }
                self.activity_log.push(format!(
                    "Worker {}: {} {}",
                    worker_id, tool_name, args_preview
                ));
                if self.activity_log.len() > 100 {
                    self.activity_log.remove(0);
                }
            }
            OrchestratorEvent::ToolResult {
                worker_id,
                tool_name,
                success,
                preview,
            } => {
                let now = self.tick_count;
                if let Some(agent) = self.agents.iter_mut().find(|a| a.worker_id == worker_id) {
                    agent.tool_history.push(ToolCallEntry {
                        tool_name: tool_name.clone(),
                        timestamp: now,
                        success,
                        preview: preview.chars().take(100).collect(),
                    });
                    agent.current_tool = None;
                    agent.status = "thinking".to_string();
                }
            }
            OrchestratorEvent::TaskCompleted {
                worker_id,
                task_id: _,
                success: _,
                diff_size,
            } => {
                let sandbox_name = self
                    .agents
                    .iter()
                    .find(|a| a.worker_id == worker_id)
                    .map(|a| a.sandbox.clone());
                if let Some(agent) = self.agents.iter_mut().find(|a| a.worker_id == worker_id) {
                    agent.status = "completed".to_string();
                    agent.diff_size = diff_size;
                    agent.current_tool = None;
                }
                if let Some(sb_name) = sandbox_name {
                    if let Some(sb) = self.sandboxes.iter_mut().find(|s| s.name == sb_name) {
                        sb.active_agents = sb.active_agents.saturating_sub(1);
                        sb.completed_tasks += 1;
                        if sb.active_agents == 0 {
                            sb.status = "idle".to_string();
                        }
                    }
                }
                self.stats.completed += 1;
            }
            OrchestratorEvent::TaskFailed {
                worker_id,
                task_id,
                error,
            } => {
                let sandbox_name = self
                    .agents
                    .iter()
                    .find(|a| a.worker_id == worker_id)
                    .map(|a| a.sandbox.clone());
                if let Some(agent) = self.agents.iter_mut().find(|a| a.worker_id == worker_id) {
                    agent.status = "failed".to_string();
                }
                if let Some(sb_name) = sandbox_name {
                    if let Some(sb) = self.sandboxes.iter_mut().find(|s| s.name == sb_name) {
                        sb.active_agents = sb.active_agents.saturating_sub(1);
                        sb.failed_tasks += 1;
                    }
                }
                self.stats.failed += 1;
                self.errors
                    .push(format!("Task {}: {}", task_id, error));
            }
            OrchestratorEvent::StatsUpdate { stats } => {
                self.stats.total_tasks = stats.total_tasks;
                self.stats.completed = stats.completed;
                self.stats.failed = stats.failed;
                self.stats.running = stats.running;
                self.stats.pending = stats.pending;
            }
            OrchestratorEvent::LayerProgress { layer, message } => {
                let msg = format!("Layer {}: {}", layer, message);
                self.layer_messages.push(msg.clone());
                if self.layer_messages.len() > 20 {
                    self.layer_messages.remove(0);
                }
                self.activity_log.push(msg);
                if self.activity_log.len() > 100 {
                    self.activity_log.remove(0);
                }
            }
            OrchestratorEvent::Done => {
                self.orchestrator_done = true;
                self.activity_log
                    .push("Orchestration complete. Press 'q' to quit.".to_string());
            }
        }
    }

    /// Progress percentage (0–100)
    pub fn progress_percent(&self) -> u16 {
        if self.stats.total_tasks == 0 {
            return 0;
        }
        ((self.stats.completed * 100) / self.stats.total_tasks) as u16
    }
}
