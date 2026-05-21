use crate::config::MowisConfig;
use crate::orchestration::ComplexityMode;
use crate::tui::event::{OrchActivityEvent, TuiEvent};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Intent classifier (consolidated from former intent.rs) ──────────────────

/// Whether the user wants to build something or just chat.
#[derive(Debug, Clone, PartialEq)]
pub enum UserIntent {
    Chat,
    Build,
}

/// Classify a user message as Chat or Build.
pub fn classify_intent(message: &str) -> UserIntent {
    let lower = message.to_lowercase();

    let hard_chat_patterns: &[&str] = &[
        "what is ", "what are ", "what does ", "what's ",
        "how does ", "how do ", "how is ",
        "why does ", "why is ", "why are ", "why do ",
        "explain ", "can you explain",
        "tell me about", "tell me what",
        "describe ", "definition of",
        "difference between", "compare ",
        "what should i", "should i use", "which is better",
        "pros and cons", "advantages of", "disadvantages of",
    ];

    let is_hard_chat = hard_chat_patterns.iter().any(|p| lower.contains(p));

    let strong_build: &[&str] = &[
        "create a ", "create an ", "create the ",
        "build a ", "build an ", "build the ",
        "make a ", "make an ", "make the ",
        "implement ", "write a ", "write an ", "write the ",
        "develop a ", "develop an ", "develop the ",
        "generate ", "scaffold ",
        "code me", "code a ", "code an ", "code the ",
        "can you code", "can you make", "can you build", "can you create",
        "can you write", "can you develop", "can you implement",
        "can you generate", "can you set up",
        "add ", "remove ", "delete ", "rename ",
        "refactor ", "rewrite ", "redesign ", "restructure ",
        "update ", "upgrade ", "migrate ", "port ",
        "fix ", "patch ", "resolve ", "debug ",
        "move ", "extract ", "split ", "merge ",
        "replace ", "convert ", "transform ",
        "set up ", "setup ", "configure ", "install ",
        "initialize ", "init ", "bootstrap ",
        "new feature", "add feature", "add support for",
        "integrate ", "connect ", "hook up",
        "wire up", "plug in",
    ];

    let weak_build: &[&str] = &[
        "website", "web app", "webapp", "landing page", "landing-page",
        "dashboard", "admin panel", "portfolio", "blog", "e-commerce",
        "mobile app", "cli tool", "rest api", "graphql", "crud",
        "microservice", "chatbot", "plugin", "extension", "script",
        "api", "endpoint", "route", "controller", "service",
        "database", "schema", "migration", "model",
        "component", "module", "function", "class",
        "test", "tests", "spec", "auth", "authentication",
        "login", "signup", "register", "session", "jwt",
        "ui", "form", "button", "page", "layout",
        "deploy", "dockerfile", "ci/cd", "pipeline",
        "for my ", "for our ", "for the ",
        "my app", "my project", "my codebase", "my code", "my company",
        "the app", "the project", "the codebase",
        "the backend", "the frontend", "the api",
    ];

    let strong_chat: &[&str] = &[
        "explain", "understand", "what is", "how does",
        "tell me", "describe", "clarify",
        "opinion", "think about", "advice", "recommend",
        "best practice", "should i", "is it possible",
        "can i", "would you", "could you explain",
        "help me understand",
    ];

    let strong_build_score: u32 =
        strong_build.iter().filter(|k| lower.contains(*k)).count() as u32 * 2;
    let build_score: u32 = strong_build_score
        + weak_build.iter().filter(|k| lower.contains(*k)).count() as u32;

    let chat_score: u32 = strong_chat.iter().filter(|k| lower.contains(*k)).count() as u32 * 2;

    if is_hard_chat && strong_build_score == 0 {
        return UserIntent::Chat;
    }

    if build_score > 0 && build_score >= chat_score {
        UserIntent::Build
    } else if chat_score > build_score {
        UserIntent::Chat
    } else {
        UserIntent::Chat
    }
}

// ── Public types consumed by ui.rs ───────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum MainView {
    Chat,
    Orchestration,
    Development,
    Shell,
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

    // Skill creator mode
    pub skill_creator_active: bool,

    // PTY Shell
    pub shell_input_tx: Option<mpsc::Sender<crate::tui::shell::ShellInput>>,
    pub shell_output: Vec<String>,
    pub shell_focused: bool,
    pub shell_scroll_offset: usize,

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
            skill_creator_active: false,
            shell_input_tx: None,
            shell_output: Vec::new(),
            shell_focused: false,
            shell_scroll_offset: 0,
            should_quit: false,
            cwd,
        };

        // Auto-start PTY shell
        match crate::tui::shell::PtyShell::spawn(&app.cwd) {
            Ok(shell) => {
                app.shell_input_tx = Some(shell.input_tx.clone());
                app.shell_output.push("[MowisAI] Shell started. Press Tab to focus/unfocus.".to_string());
                let event_tx = app.event_tx.clone();
                // Forward shell events to TUI event loop
                std::thread::spawn(move || {
                    while let Ok(ev) = shell.event_rx.recv() {
                        match ev {
                            crate::tui::shell::ShellEvent::Output(text) => {
                                if let Some(ref tx) = event_tx {
                                    let _ = tx.send(TuiEvent::ShellOutput(text));
                                }
                            }
                            crate::tui::shell::ShellEvent::Exited(code) => {
                                if let Some(ref tx) = event_tx {
                                    let _ = tx.send(TuiEvent::ShellExited(code));
                                }
                            }
                        }
                    }
                });
            }
            Err(e) => {
                app.shell_output.push(format!("[MowisAI] Shell failed to start: {}", e));
                app.shell_output.push("[MowisAI] Run agentd with sudo for PTY support.".to_string());
            }
        }

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

    pub fn on_skill_saved(&mut self, path: String) {
        self.skill_creator_active = false;
        self.messages.push(ChatMessage {
            role: MessageRole::System,
            content: format!(
                "✓ Skill saved to: {}\n\
                 It will be auto-injected into every agent from now on.\n\
                 Run /skill list to see all installed skills.",
                path
            ),
        });
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
                self.orch_log.push(format!("  [{}] {}", agent_id.chars().take(8).collect::<String>(), tool_name));
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
                self.orch_log.push(format!("FAILED [{}]: {}", agent_id.chars().take(8).collect::<String>(), error));
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

    pub fn on_shell_output(&mut self, text: String) {
        // Split into lines and add to shell output buffer
        for line in text.split('\n') {
            self.shell_output.push(line.to_string());
        }
        // Cap to 5000 lines
        if self.shell_output.len() > 5000 {
            self.shell_output.drain(0..1000);
        }
        // Auto-scroll to bottom when shell is focused
        if self.shell_focused {
            self.shell_scroll_offset = 0;
        }
    }

    pub fn on_shell_exit(&mut self, code: i32) {
        self.shell_output.push(format!("[MowisAI] Shell exited with code {}. Press 'r' to restart.", code));
        self.shell_focused = false;
        self.shell_input_tx = None;
    }

    pub fn restart_shell(&mut self) {
        self.shell_output.clear();
        match crate::tui::shell::PtyShell::spawn(&self.cwd) {
            Ok(shell) => {
                self.shell_input_tx = Some(shell.input_tx.clone());
                self.shell_output.push("[MowisAI] Shell restarted. Press Tab to focus.".to_string());
                let event_tx = self.event_tx.clone();
                std::thread::spawn(move || {
                    while let Ok(ev) = shell.event_rx.recv() {
                        match ev {
                            crate::tui::shell::ShellEvent::Output(text) => {
                                if let Some(ref tx) = event_tx {
                                    let _ = tx.send(TuiEvent::ShellOutput(text));
                                }
                            }
                            crate::tui::shell::ShellEvent::Exited(code) => {
                                if let Some(ref tx) = event_tx {
                                    let _ = tx.send(TuiEvent::ShellExited(code));
                                }
                            }
                        }
                    }
                });
            }
            Err(e) => {
                self.shell_output.push(format!("[MowisAI] Shell failed to restart: {}", e));
            }
        }
    }

    // ── Keyboard input ────────────────────────────────────────────────────────

    pub fn handle_key(&mut self, key: KeyEvent) {
        // Save selector overlay intercepts all input when active
        if self.save_selector.is_some() {
            self.handle_save_selector_key(key);
            return;
        }

        // When shell is focused, forward ALL keyboard input to the PTY
        if self.shell_focused && self.shell_input_tx.is_some() {
            match key.code {
                // Tab unfocuses the shell
                KeyCode::Tab => {
                    self.shell_focused = false;
                    return;
                }
                // Ctrl+C sends SIGINT via the PTY (just send the byte)
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let _ = self.shell_input_tx.as_ref().unwrap().send(
                        crate::tui::shell::ShellInput::Data(vec![0x03]) // ETX = Ctrl+C
                    );
                    return;
                }
                // Ctrl+D sends EOF
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let _ = self.shell_input_tx.as_ref().unwrap().send(
                        crate::tui::shell::ShellInput::Data(vec![0x04]) // EOT = Ctrl+D
                    );
                    return;
                }
                // Ctrl+Z sends SIGTSTP
                KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let _ = self.shell_input_tx.as_ref().unwrap().send(
                        crate::tui::shell::ShellInput::Data(vec![0x1a]) // SUB = Ctrl+Z
                    );
                    return;
                }
                // Up arrow
                KeyCode::Up => {
                    let _ = self.shell_input_tx.as_ref().unwrap().send(
                        crate::tui::shell::ShellInput::Data(vec![0x1b, 0x5b, 0x41]) // ESC [ A
                    );
                    return;
                }
                // Down arrow
                KeyCode::Down => {
                    let _ = self.shell_input_tx.as_ref().unwrap().send(
                        crate::tui::shell::ShellInput::Data(vec![0x1b, 0x5b, 0x42]) // ESC [ B
                    );
                    return;
                }
                // Right arrow
                KeyCode::Right => {
                    let _ = self.shell_input_tx.as_ref().unwrap().send(
                        crate::tui::shell::ShellInput::Data(vec![0x1b, 0x5b, 0x43]) // ESC [ C
                    );
                    return;
                }
                // Left arrow
                KeyCode::Left => {
                    let _ = self.shell_input_tx.as_ref().unwrap().send(
                        crate::tui::shell::ShellInput::Data(vec![0x1b, 0x5b, 0x44]) // ESC [ D
                    );
                    return;
                }
                // Enter
                KeyCode::Enter => {
                    let _ = self.shell_input_tx.as_ref().unwrap().send(
                        crate::tui::shell::ShellInput::Data(vec![0x0a]) // LF
                    );
                    return;
                }
                // Backspace
                KeyCode::Backspace => {
                    let _ = self.shell_input_tx.as_ref().unwrap().send(
                        crate::tui::shell::ShellInput::Data(vec![0x7f]) // DEL
                    );
                    return;
                }
                // Tab character (literal)
                KeyCode::Char('\t') => {
                    let _ = self.shell_input_tx.as_ref().unwrap().send(
                        crate::tui::shell::ShellInput::Data(vec![0x09])
                    );
                    return;
                }
                // Any other character
                KeyCode::Char(c) => {
                    let _ = self.shell_input_tx.as_ref().unwrap().send(
                        crate::tui::shell::ShellInput::Data(c.to_string().into_bytes())
                    );
                    return;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('r') if self.view_mode == MainView::Shell && !self.shell_focused && self.shell_input_tx.is_none() => {
                self.restart_shell();
            }
            KeyCode::Tab if !self.is_loading => {
                self.view_mode = match self.view_mode {
                    MainView::Chat => {
                        if self.dev_mode_active { MainView::Orchestration } else { MainView::Shell }
                    }
                    MainView::Orchestration => MainView::Development,
                    MainView::Development => MainView::Shell,
                    MainView::Shell => MainView::Chat,
                };
                // When entering Shell view, auto-focus the shell
                if self.view_mode == MainView::Shell && self.shell_input_tx.is_some() {
                    self.shell_focused = true;
                } else {
                    self.shell_focused = false;
                }
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
                // Use char-aware insertion
                let byte_pos = self.input_text
                    .char_indices()
                    .nth(self.input_cursor)
                    .map(|(pos, _)| pos)
                    .unwrap_or(self.input_text.len());
                self.input_text.insert(byte_pos, c);
                self.input_cursor += 1;
            }
            KeyCode::Backspace => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    let byte_pos = self.input_text
                        .char_indices()
                        .nth(self.input_cursor)
                        .map(|(pos, _)| pos)
                        .unwrap_or(0);
                    if byte_pos < self.input_text.len() {
                        self.input_text.remove(byte_pos);
                    }
                }
            }
            KeyCode::Left => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                }
            }
            KeyCode::Right => {
                let char_count = self.input_text.chars().count();
                if self.input_cursor < char_count {
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

    fn start_chat(&mut self, _message: String) {
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
        let skill_mode = self.skill_creator_active;
        let history: Vec<serde_json::Value> = self
            .messages
            .iter()
            .filter_map(|msg| match msg.role {
                MessageRole::User => Some(serde_json::json!({
                    "role": "user",
                    "content": msg.content.clone(),
                })),
                MessageRole::Assistant => Some(serde_json::json!({
                    "role": "assistant",
                    "content": msg.content.clone(),
                })),
                MessageRole::System => None,
            })
            .collect();

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

                let system_prompt = if skill_mode {
                    crate::skills::creator::SKILL_CREATOR_SYSTEM_PROMPT.to_string()
                } else {
                    "You are MowisAI, an AI coding assistant. Answer the user's question helpfully and concisely.".to_string()
                };

                match crate::orchestration::provider_client::generate_chat(
                    &llm_config,
                    &system_prompt,
                    &history,
                    0.7,
                )
                .await
                {
                    Ok(response) => {
                        // In skill creator mode, detect and save the skill block,
                        // then send a clean version of the response to the chat.
                        if skill_mode {
                            if let Some(result) = crate::skills::creator::try_save_skill_from_response(&response) {
                                let save_msg = match result {
                                    Ok(path) => TuiEvent::SkillSaved(path.display().to_string()),
                                    Err(e) => TuiEvent::GeminiError(
                                        format!("Skill block found but failed to save: {}", e)
                                    ),
                                };
                                let _ = tx.send(save_msg);
                            }
                            // Strip the raw <skill>…</skill> block from the visible chat message
                            let display = strip_skill_block(&response);
                            let _ = tx.send(TuiEvent::GeminiChunk(display));
                        } else {
                            let _ = tx.send(TuiEvent::GeminiChunk(response));
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(TuiEvent::GeminiError(e.to_string()));
                    }
                }
                let _ = tx.send(TuiEvent::GeminiDone);
            });
        });
    }

    /// Kick off the skill creator conversation: LLM sends the first question.
    fn start_skill_creator_intro(&mut self) {
        self.is_loading = true;

        let tx = match &self.event_tx {
            Some(t) => t.clone(),
            None => {
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

                // Seed message: tell LLM the user wants to start creating a skill
                let seed = [serde_json::json!({
                    "role": "user",
                    "content": "I want to create a skill."
                })];

                match crate::orchestration::provider_client::generate_chat(
                    &llm_config,
                    crate::skills::creator::SKILL_CREATOR_SYSTEM_PROMPT,
                    &seed,
                    0.7,
                ).await {
                    Ok(response) => {
                        let _ = tx.send(TuiEvent::GeminiChunk(strip_skill_block(&response)));
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
                let overlay_root = std::env::temp_dir().join("mowisai-overlay");
                let checkpoint_root = std::env::temp_dir().join("mowisai-checkpoints");
                let merge_work_dir = std::env::temp_dir().join("mowisai-merge");

                // Create event channel so orchestrator can send progress to TUI
                let (orch_event_tx, orch_event_rx) = std::sync::mpsc::channel::<crate::orchestration::new_orchestrator::OrchestratorEvent>();

                // Forward orchestrator events to TUI in a background thread
                let tx_clone = tx.clone();
                std::thread::spawn(move || {
                    while let Ok(ev) = orch_event_rx.recv() {
                        let tui_ev = match ev {
                            crate::orchestration::new_orchestrator::OrchestratorEvent::TaskStarted { task_id, description, .. } => {
                                Some(OrchActivityEvent::AgentStarted { agent_id: task_id, description })
                            }
                            crate::orchestration::new_orchestrator::OrchestratorEvent::ToolCall { tool_name, .. } => {
                                Some(OrchActivityEvent::ToolCall { agent_id: String::new(), tool_name })
                            }
                            crate::orchestration::new_orchestrator::OrchestratorEvent::TaskCompleted { success, .. } => {
                                Some(if success {
                                    OrchActivityEvent::AgentCompleted { agent_id: String::new() }
                                } else {
                                    OrchActivityEvent::AgentFailed { agent_id: String::new(), error: "task failed".into() }
                                })
                            }
                            crate::orchestration::new_orchestrator::OrchestratorEvent::TaskFailed { error, .. } => {
                                Some(OrchActivityEvent::AgentFailed { agent_id: String::new(), error })
                            }
                            crate::orchestration::new_orchestrator::OrchestratorEvent::LayerProgress { layer, message } => {
                                Some(OrchActivityEvent::LayerProgress { layer, message })
                            }
                            crate::orchestration::new_orchestrator::OrchestratorEvent::LlmThinking { agent_id, task_description } => {
                                let _ = tx_clone.send(TuiEvent::OrchEvent(OrchActivityEvent::ToolCall {
                                    agent_id,
                                    tool_name: format!("thinking: {}", task_description),
                                }));
                                None
                            }
                            crate::orchestration::new_orchestrator::OrchestratorEvent::ToolResult { tool_name, success, preview, .. } => {
                                let _ = tx_clone.send(TuiEvent::OrchEvent(OrchActivityEvent::ToolCall {
                                    agent_id: String::new(),
                                    tool_name: format!("{}: {} {}", tool_name, if success { "✓" } else { "✗" }, preview.chars().take(60).collect::<String>()),
                                }));
                                None
                            }
                            _ => None,
                        };
                        if let Some(ev) = tui_ev {
                            let _ = tx_clone.send(TuiEvent::OrchEvent(ev));
                        }
                    }
                });

                let orch_config = crate::orchestration::OrchestratorConfig {
                    llm_config,
                    execution_llm_config: None,
                    socket_path: config.socket_path.clone(),
                    project_root,
                    overlay_root,
                    checkpoint_root,
                    merge_work_dir,
                    max_agents: 100,
                    max_verification_rounds: 3,
                    staging_dir: None,
                    event_tx: Some(orch_event_tx),
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
\n\
Skills:\n\
  /skill create      Create a skill with AI guidance\n\
  /skill list        List installed skills\n\
  /skill remove <n>  Remove a skill by name\n\
  /skill cancel      Exit skill creator mode\n\
\n\
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
            "/skill create" | "/skill new" | "/skills create" | "/skills new" => {
                self.skill_creator_active = true;
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "🎯 Skill Creator activated.\n\
                        The AI will guide you through creating a .skill file.\n\
                        Skills are domain-specific rules injected into every agent's context.\n\
                        Type /skill cancel at any time to exit.".to_string(),
                });
                // Trigger the LLM to open the conversation
                self.start_skill_creator_intro();
            }
            "/skill cancel" | "/skills cancel" => {
                self.skill_creator_active = false;
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "Skill creator cancelled.".to_string(),
                });
            }
            "/skill list" | "/skills list" | "/skills" => {
                let skills = crate::skills::SkillManager::new().load_all();
                if skills.is_empty() {
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "No skills installed.\n\
                            Type /skill create to create one with the AI.".to_string(),
                    });
                } else {
                    let lines: Vec<String> = skills.iter()
                        .map(|s| format!("  {:<20} v{}  {}", s.meta.name, s.meta.version, s.meta.description))
                        .collect();
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: format!(
                            "Installed skills ({}):\n{}\n\nThese are injected into every agent's context.",
                            skills.len(),
                            lines.join("\n")
                        ),
                    });
                }
            }
            other if other.starts_with("/skill remove ") || other.starts_with("/skills remove ") => {
                let name = other.trim_start_matches("/skills remove ").trim_start_matches("/skill remove ").trim();
                match crate::skills::SkillManager::new().remove(name) {
                    Ok(()) => self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: format!("✓ Skill '{}' removed.", name),
                    }),
                    Err(e) => self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: format!("Failed to remove skill '{}': {}", name, e),
                    }),
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

        // Initialize a git repo so git apply can create new files
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(&target_path)
            .output();

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
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // Fall back to writing as raw patch
                let patch_path = target_path.join("mowisai_output.patch");
                match std::fs::write(&patch_path, diff.as_bytes()) {
                    Ok(_) => {
                        self.messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: format!(
                                "git apply failed ({}): {}\nDiff saved to {}\nTo apply manually: cd {} && git apply mowisai_output.patch",
                                output.status.code().unwrap_or(-1),
                                stderr.trim(),
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

/// Remove the raw `<skill>…</skill>` TOML block from an LLM response before
/// showing it in the chat UI. The LLM's surrounding explanation is kept.
fn strip_skill_block(text: &str) -> String {
    if let (Some(start), Some(end)) = (text.find("<skill>"), text.find("</skill>")) {
        let before = text[..start].trim_end();
        let after = text[end + 8..].trim_start();
        let result = match (before.is_empty(), after.is_empty()) {
            (true, true) => String::new(),
            (true, false) => after.to_string(),
            (false, true) => before.to_string(),
            (false, false) => format!("{}\n\n{}", before, after),
        };
        if result.is_empty() {
            "✓ Skill generated and saved.".to_string()
        } else {
            result
        }
    } else {
        text.to_string()
    }
}
