use crate::config::MowisConfig;
use crate::intent::{classify_intent, UserIntent};
use crate::orchestration::ComplexityMode;
use crate::tui::event::{OrchActivityEvent, TuiEvent};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Public types consumed by ui.rs ───────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum MainView {
    Chat,
    Orchestration,
    Development,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub agent_id: String,
    pub description: String,
    pub status: String,
    pub current_tool: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SaveSelector {
    pub selected: usize,
    pub typing_path: bool,
    pub custom_path_input: String,
}

impl SaveSelector {
    fn new() -> Self {
        Self { selected: 0, typing_path: false, custom_path_input: String::new() }
    }
}

// ── App state ─────────────────────────────────────────────────────────────────

pub struct App {
    pub config: MowisConfig,
    pub socket_pid: Option<u32>,
    pub event_tx: Option<mpsc::Sender<TuiEvent>>,

    // Views
    pub view_mode: MainView,
    pub dev_mode_active: bool,

    // Chat
    pub messages: Vec<ChatMessage>,
    pub input_text: String,
    pub input_cursor: usize,
    pub scroll_offset: usize,

    // Loading / spinner
    pub is_loading: bool,
    pub spinner_frame: usize,
    pub orchestrating: bool,

    // Orchestration dashboard
    pub orch_log: Vec<String>,
    pub orch_layer: u8,
    pub orch_total: usize,
    pub orch_completed: usize,
    pub agents: Vec<AgentInfo>,

    // Development log
    pub dev_log: Vec<(String, String, u64)>,

    // Diff / save flow
    pub pending_diff: Option<String>,
    pub save_selector: Option<SaveSelector>,

    // Complexity mode override
    pub mode_override: Option<ComplexityMode>,

    // Misc
    pub should_quit: bool,
    pub cwd: String,
}

impl App {
    pub fn new(config: MowisConfig, socket_pid: Option<u32>) -> Self {
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string());

        let mut app = Self {
            config,
            socket_pid,
            event_tx: None,
            view_mode: MainView::Chat,
            dev_mode_active: false,
            messages: Vec::new(),
            input_text: String::new(),
            input_cursor: 0,
            scroll_offset: 0,
            is_loading: false,
            spinner_frame: 0,
            orchestrating: false,
            orch_log: Vec::new(),
            orch_layer: 0,
            orch_total: 0,
            orch_completed: 0,
            agents: Vec::new(),
            dev_log: Vec::new(),
            pending_diff: None,
            save_selector: None,
            mode_override: None,
            should_quit: false,
            cwd,
        };

        // Welcome message
        let socket_status = if socket_pid.is_some() {
            "Socket server running."
        } else {
            "No socket server - run /launch to start one."
        };
        app.messages.push(ChatMessage {
            role: MessageRole::System,
            content: format!(
                "Welcome to MowisAI! Type your message and press Enter.\n\
                 {}\n\
                 Describe what you want to build - orchestration triggers automatically.\n\
                 Type /help for commands, /mode to control orchestration depth, /quit to exit.",
                socket_status
            ),
        });

        app
    }

    // ── Tick / spinner ────────────────────────────────────────────────────────

    pub fn on_tick(&mut self) {
        if self.is_loading {
            self.spinner_frame = self.spinner_frame.wrapping_add(1);
        }
    }

    // ── Gemini streaming ──────────────────────────────────────────────────────

    pub fn on_gemini_chunk(&mut self, text: String) {
        match self.messages.last_mut() {
            Some(msg) if msg.role == MessageRole::Assistant => {
                msg.content.push_str(&text);
            }
            _ => {
                self.messages.push(ChatMessage {
                    role: MessageRole::Assistant,
                    content: text,
                });
            }
        }
    }

    pub fn on_gemini_done(&mut self) {
        self.is_loading = false;
    }

    pub fn on_gemini_error(&mut self, err: String) {
        self.is_loading = false;
        self.messages.push(ChatMessage {
            role: MessageRole::System,
            content: format!("Error: {}", err),
        });
    }

    // ── Orchestration events ──────────────────────────────────────────────────

    pub fn on_orch_event(&mut self, ev: OrchActivityEvent) {
        match ev {
            OrchActivityEvent::AgentStarted { agent_id, description } => {
                self.agents.push(AgentInfo {
                    agent_id,
                    description,
                    status: "thinking".to_string(),
                    current_tool: None,
                });
            }
            OrchActivityEvent::ToolCall { agent_id, tool_name } => {
                if let Some(a) = self.agents.iter_mut().find(|a| a.agent_id == agent_id) {
                    a.status = "executing_tool".to_string();
                    a.current_tool = Some(tool_name.clone());
                }
                self.orch_log.push(format!("  [{}] {}", &agent_id[..agent_id.len().min(8)], tool_name));
            }
            OrchActivityEvent::AgentCompleted { agent_id } => {
                if let Some(a) = self.agents.iter_mut().find(|a| a.agent_id == agent_id) {
                    a.status = "completed".to_string();
                    a.current_tool = None;
                }
                self.orch_completed += 1;
            }
            OrchActivityEvent::AgentFailed { agent_id, error } => {
                if let Some(a) = self.agents.iter_mut().find(|a| a.agent_id == agent_id) {
                    a.status = "failed".to_string();
                    a.current_tool = None;
                }
                self.orch_log.push(format!("FAILED [{}]: {}", &agent_id[..agent_id.len().min(8)], error));
            }
            OrchActivityEvent::LayerProgress { layer, message } => {
                self.orch_layer = layer;
                self.orch_log.push(format!("[Layer {}] {}", layer, message));
            }
            OrchActivityEvent::StatsUpdate { total, completed, .. } => {
                self.orch_total = total;
                self.orch_completed = completed;
            }
        }
    }

    pub fn on_orch_complete(&mut self, diff: String, summary: String) {
        self.pending_diff = Some(diff);
        self.messages.push(ChatMessage {
            role: MessageRole::Assistant,
            content: summary,
        });
        self.save_selector = Some(SaveSelector::new());
    }

    pub fn on_orch_done(&mut self) {
        self.is_loading = false;
        self.orchestrating = false;

        if self.pending_diff.is_none() {
            self.messages.push(ChatMessage {
                role: MessageRole::System,
                content: "No code changes were captured.\n\
                    This can happen when:\n\
                    - The agent wrote files but the diff couldn't be captured (check logs)\n\
                    - The agent answered in text without writing any files\n\
                    - The socket server lost the container state\n\
                    Tip: Run /development to see full agent logs, or try /mode full."
                    .to_string(),
            });
        }
    }

    // ── Dev log ───────────────────────────────────────────────────────────────

    pub fn on_log_entry(&mut self, level: String, message: String, timestamp: u64) {
        self.dev_log.push((level, message, timestamp));
        // Cap to last 2000 entries to avoid unbounded growth
        if self.dev_log.len() > 2000 {
            self.dev_log.drain(0..500);
        }
    }

    // ── Keyboard input ────────────────────────────────────────────────────────

    pub fn handle_key(&mut self, key: KeyEvent) {
        // Save selector overlay intercepts all input when active
        if self.save_selector.is_some() {
            self.handle_save_selector_key(key);
            return;
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Tab if !self.is_loading => {
                self.view_mode = match self.view_mode {
                    MainView::Chat => {
                        if self.dev_mode_active { MainView::Orchestration } else { MainView::Chat }
                    }
                    MainView::Orchestration => MainView::Development,
                    MainView::Development => MainView::Chat,
                };
            }
            KeyCode::Enter if !self.is_loading => {
                let text = self.input_text.trim().to_string();
                if !text.is_empty() {
                    self.input_text.clear();
                    self.input_cursor = 0;
                    self.scroll_offset = 0;
                    if text.starts_with('/') {
                        self.handle_command(&text);
                    } else {
                        self.handle_user_input(text);
                    }
                }
            }
            KeyCode::Char(c) => {
                self.input_text.insert(self.input_cursor, c);
                self.input_cursor += 1;
            }
            KeyCode::Backspace => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    self.input_text.remove(self.input_cursor);
                }
            }
            KeyCode::Left => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                }
            }
            KeyCode::Right => {
                if self.input_cursor < self.input_text.len() {
                    self.input_cursor += 1;
                }
            }
            KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
            }
            KeyCode::Down => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            _ => {}
        }
    }

    fn handle_save_selector_key(&mut self, key: KeyEvent) {
        let sel = match self.save_selector.as_mut() {
            Some(s) => s,
            None => return,
        };

        if sel.typing_path {
            match key.code {
                KeyCode::Enter => {
                    let path = sel.custom_path_input.trim().to_string();
                    let diff = self.pending_diff.clone().unwrap_or_default();
                    self.save_selector = None;
                    self.apply_diff_to_path(&diff, &path);
                }
                KeyCode::Esc => {
                    sel.typing_path = false;
                    sel.custom_path_input.clear();
                }
                KeyCode::Char(c) => sel.custom_path_input.push(c),
                KeyCode::Backspace => { sel.custom_path_input.pop(); }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let sel = self.save_selector.as_mut().unwrap();
                if sel.selected > 0 { sel.selected -= 1; }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let sel = self.save_selector.as_mut().unwrap();
                if sel.selected < 2 { sel.selected += 1; }
            }
            KeyCode::Char('1') => { self.save_selector.as_mut().unwrap().selected = 0; }
            KeyCode::Char('2') => { self.save_selector.as_mut().unwrap().selected = 1; }
            KeyCode::Char('3') => { self.save_selector.as_mut().unwrap().selected = 2; }
            KeyCode::Enter => {
                let selected = self.save_selector.as_ref().unwrap().selected;
                let diff = self.pending_diff.clone().unwrap_or_default();
                match selected {
                    0 => {
                        // Current directory
                        let path = self.cwd.clone();
                        self.save_selector = None;
                        self.pending_diff = None;
                        self.apply_diff_to_path(&diff, &path);
                    }
                    1 => {
                        // Type a path
                        self.save_selector.as_mut().unwrap().typing_path = true;
                    }
                    2 => {
                        // Create new folder
                        let ts = now_secs();
                        let path = format!("{}/mowisai-output-{}", self.cwd, ts);
                        self.save_selector = None;
                        self.pending_diff = None;
                        self.apply_diff_to_path(&diff, &path);
                    }
                    _ => {}
                }
            }
            KeyCode::Esc => {
                self.save_selector = None;
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "Save cancelled. Use /save to save later or /discard to discard.".to_string(),
                });
            }
            _ => {}
        }
    }

    // ── User input → orchestration or chat ───────────────────────────────────

    fn handle_user_input(&mut self, text: String) {
        self.messages.push(ChatMessage { role: MessageRole::User, content: text.clone() });

        match classify_intent(&text) {
            UserIntent::Build => {
                let mode_label = match &self.mode_override {
                    Some(ComplexityMode::Simple) => " [simple mode]",
                    Some(ComplexityMode::Standard) => " [standard mode]",
                    Some(ComplexityMode::Full) => " [full mode]",
                    None => "",
                };
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!("Build request detected{} -- launching orchestration...", mode_label),
                });
                self.start_orchestration(text);
            }
            UserIntent::Chat => {
                self.start_chat(text);
            }
        }
    }

    fn start_chat(&mut self, message: String) {
        self.is_loading = true;

        let tx = match &self.event_tx {
            Some(t) => t.clone(),
            None => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "Cannot send chat: no event channel.".to_string(),
                });
                self.is_loading = false;
                return;
            }
        };

        let config = self.config.clone();

        std::thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(TuiEvent::GeminiError(e.to_string()));
                    let _ = tx.send(TuiEvent::GeminiDone);
                    return;
                }
            };

            rt.block_on(async move {
                let llm_config = match crate::orchestration::provider_client::LlmConfig::from_config(&config) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.send(TuiEvent::GeminiError(e.to_string()));
                        let _ = tx.send(TuiEvent::GeminiDone);
                        return;
                    }
                };

                let system_prompt = "You are MowisAI, an AI coding assistant. Answer the user's question helpfully and concisely.";

                match crate::orchestration::provider_client::generate_text(
                    &llm_config,
                    system_prompt,
                    &message,
                    false,
                    0.7,
                )
                .await
                {
                    Ok(response) => {
                        let _ = tx.send(TuiEvent::GeminiChunk(response));
                    }
                    Err(e) => {
                        let _ = tx.send(TuiEvent::GeminiError(e.to_string()));
                    }
                }
                let _ = tx.send(TuiEvent::GeminiDone);
            });
        });
    }

    fn start_orchestration(&mut self, prompt: String) {
        self.is_loading = true;
        self.orchestrating = true;
        self.view_mode = MainView::Orchestration;
        self.orch_log.clear();
        self.agents.clear();
        self.orch_layer = 0;
        self.orch_total = 0;
        self.orch_completed = 0;

        let tx = match &self.event_tx {
            Some(t) => t.clone(),
            None => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "Cannot start orchestration: no event channel.".to_string(),
                });
                self.is_loading = false;
                self.orchestrating = false;
                return;
            }
        };

        let config = self.config.clone();
        let mode = self.mode_override.clone();

        std::thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(TuiEvent::GeminiError(e.to_string()));
                    return;
                }
            };

            rt.block_on(async move {
                let llm_config = match crate::orchestration::provider_client::LlmConfig::from_config(&config) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.send(TuiEvent::GeminiError(e.to_string()));
                        let _ = tx.send(TuiEvent::OrchDone);
                        return;
                    }
                };

                let project_root = std::env::current_dir().unwrap_or_else(|_| ".".into());
                let overlay_root = project_root.join(".mowisai/overlay");
                let checkpoint_root = project_root.join(".mowisai/checkpoints");
                let merge_work_dir = project_root.join(".mowisai/merge");

                let orch_config = crate::orchestration::OrchestratorConfig {
                    llm_config,
                    socket_path: config.socket_path.clone(),
                    project_root,
                    overlay_root,
                    checkpoint_root,
                    merge_work_dir,
                    max_agents: 100,
                    max_verification_rounds: 3,
                    staging_dir: None,
                    event_tx: None,
                    mode_override: mode,
                };

                let orchestrator = crate::orchestration::NewOrchestrator::new(orch_config);

                match orchestrator.run(&prompt).await {
                    Ok(output) => {
                        let _ = tx.send(TuiEvent::OrchComplete {
                            diff: output.merged_diff,
                            summary: output.summary,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(TuiEvent::GeminiError(e.to_string()));
                    }
                }
                let _ = tx.send(TuiEvent::OrchDone);
            });
        });
    }

    // ── Commands ──────────────────────────────────────────────────────────────

    fn handle_command(&mut self, cmd: &str) {
        match cmd {
            "/quit" | "/exit" | "/q" => {
                if let Some(pid) = self.socket_pid {
                    let _ = std::process::Command::new("kill").arg(pid.to_string()).output();
                }
                self.should_quit = true;
            }
            "/clear" => {
                self.messages.clear();
                self.orch_log.clear();
            }
            "/help" => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "\
Commands:\n\
  /quit /exit /q     Exit MowisAI\n\
  /clear             Clear chat history\n\
  /help              Show this help\n\
  /mode auto         Auto-detect complexity (default)\n\
  /mode simple       Force single-agent mode\n\
  /mode standard     Force standard planner (<= 3 agents)\n\
  /mode full         Force full 7-layer pipeline\n\
  /launch            Connect to or start socket server\n\
  /kill-socket       Stop socket server\n\
  /socket status     Show socket server status\n\
  /socket restart    Restart socket server\n\
  /development       Toggle development log view\n\
  /discard           Discard pending diff\n\
  /save [path]       Save pending diff to path\n\
  Tab                Cycle views (Chat / Orchestration / Dev)\n\
  Ctrl+C             Quit".to_string(),
                });
            }
            "/mode auto" => {
                self.mode_override = None;
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "Mode: auto (complexity classifier picks the pipeline)".to_string(),
                });
            }
            "/mode simple" => {
                self.mode_override = Some(ComplexityMode::Simple);
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "Mode locked: simple -- 1 agent, no planner, no merge, no verification.".to_string(),
                });
            }
            "/mode standard" => {
                self.mode_override = Some(ComplexityMode::Standard);
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "Mode locked: standard -- constrained planner, <= 3 agents, 1 verification round.".to_string(),
                });
            }
            "/mode full" => {
                self.mode_override = Some(ComplexityMode::Full);
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "Mode locked: full -- complete 7-layer pipeline (all sandboxes, full verification).".to_string(),
                });
            }
            "/kill-socket" => {
                if let Some(pid) = self.socket_pid {
                    let _ = std::process::Command::new("kill").arg(pid.to_string()).output();
                    self.socket_pid = None;
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Socket server stopped. Run /launch to restart.".to_string(),
                    });
                } else {
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Socket server is not running.".to_string(),
                    });
                }
            }
            "/socket status" => {
                let status = if self.socket_pid.is_some() { "RUNNING" } else { "STOPPED" };
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!(
                        "Socket Server: {}  |  PID: {}  |  Path: {}",
                        status,
                        self.socket_pid.map_or("N/A".to_string(), |p| p.to_string()),
                        self.config.socket_path,
                    ),
                });
            }
            "/socket restart" => {
                if let Some(pid) = self.socket_pid.take() {
                    let _ = std::process::Command::new("kill").arg(pid.to_string()).output();
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                match crate::start_socket_server_daemon(&self.config.socket_path) {
                    Ok(new_pid) => {
                        self.socket_pid = Some(new_pid);
                        self.messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: format!("Socket server restarted (PID: {})", new_pid),
                        });
                    }
                    Err(e) => {
                        self.messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: format!("Failed to restart socket server: {}", e),
                        });
                    }
                }
            }
            "/development" => {
                self.dev_mode_active = !self.dev_mode_active;
                if self.dev_mode_active {
                    self.view_mode = MainView::Development;
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Development mode ON -- showing all internal logs. Tab cycles views. /development to toggle off.".to_string(),
                    });
                } else {
                    self.view_mode = MainView::Chat;
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Development mode OFF.".to_string(),
                    });
                }
            }
            "/launch" => {
                // Try to connect to existing socket first
                if std::path::Path::new(&self.config.socket_path).exists() {
                    match std::fs::read_to_string(
                        dirs::home_dir()
                            .unwrap_or_default()
                            .join(".mowisai")
                            .join(".socket-server.pid"),
                    ) {
                        Ok(s) => {
                            if let Ok(pid) = s.trim().parse::<u32>() {
                                self.socket_pid = Some(pid);
                                self.messages.push(ChatMessage {
                                    role: MessageRole::System,
                                    content: format!("Connected to existing socket server (PID: {})", pid),
                                });
                                return;
                            }
                        }
                        Err(_) => {
                            self.messages.push(ChatMessage {
                                role: MessageRole::System,
                                content: "Socket file exists but PID unknown -- server appears running.".to_string(),
                            });
                            return;
                        }
                    }
                }

                match crate::start_socket_server_daemon(&self.config.socket_path) {
                    Ok(pid) => {
                        self.socket_pid = Some(pid);
                        self.messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: format!("Socket server started (PID: {})", pid),
                        });
                    }
                    Err(e) => {
                        self.messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: format!("Failed to start socket server: {}", e),
                        });
                    }
                }
            }
            "/discard" => {
                if self.pending_diff.is_some() {
                    self.pending_diff = None;
                    self.save_selector = None;
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Generated code discarded.".to_string(),
                    });
                } else {
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Nothing to discard -- no pending diff.".to_string(),
                    });
                }
            }
            other => {
                if other.starts_with("/save") {
                    let path_arg = other.trim_start_matches("/save").trim();
                    if let Some(diff) = self.pending_diff.clone() {
                        let target = if path_arg.is_empty() {
                            self.cwd.clone()
                        } else {
                            path_arg.to_string()
                        };
                        self.pending_diff = None;
                        self.save_selector = None;
                        self.apply_diff_to_path(&diff, &target);
                    } else {
                        self.messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: "Nothing to save -- run a build first.".to_string(),
                        });
                    }
                } else {
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: format!("Unknown command: {}. Type /help for commands.", other),
                    });
                }
            }
        }
    }

    // ── Diff application ──────────────────────────────────────────────────────

    fn apply_diff_to_path(&mut self, diff: &str, target: &str) {
        let target_path = std::path::PathBuf::from(target);
        if let Err(e) = std::fs::create_dir_all(&target_path) {
            self.messages.push(ChatMessage {
                role: MessageRole::System,
                content: format!("Failed to create directory {}: {}", target_path.display(), e),
            });
            return;
        }

        // Try git apply
        let result = std::process::Command::new("git")
            .args(["apply", "--whitespace=nowarn", "-"])
            .current_dir(&target_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(diff.as_bytes());
                }
                child.wait_with_output()
            });

        match result {
            Ok(output) if output.status.success() => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!("Code saved to {} via git apply.", target_path.display()),
                });
            }
            _ => {
                // Fall back to writing as raw patch
                let patch_path = target_path.join("mowisai_output.patch");
                match std::fs::write(&patch_path, diff.as_bytes()) {
                    Ok(_) => {
                        self.messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: format!(
                                "Diff saved to {}\n(git apply failed -- saved as raw patch file)\n\
                                 To apply manually: cd {} && git apply mowisai_output.patch",
                                patch_path.display(),
                                target_path.display(),
                            ),
                        });
                    }
                    Err(e) => {
                        self.messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: format!("Failed to write patch file: {}", e),
                        });
                    }
                }
            }
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
