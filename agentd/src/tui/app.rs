use crate::config::MowisConfig;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::mpsc;
use super::event::{OrchActivityEvent, TuiEvent};

#[derive(Debug, Clone, PartialEq)]
pub enum MainView {
    Chat,
    Orchestration,
    Development,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SaveOption {
    CurrentDir,
    SpecificPath,
    CreateFolder,
}

#[derive(Debug, Clone)]
pub struct SaveSelector {
    pub selected: usize,
    pub custom_path_input: String,
    pub custom_path_cursor: usize,
    pub typing_path: bool,
}

#[derive(Debug, Clone)]
pub struct AgentActivity {
    pub agent_id: String,
    pub status: String,
    pub description: String,
    pub current_tool: Option<String>,
    pub elapsed_secs: u64,
}

pub struct App {
    pub config: MowisConfig,
    pub messages: Vec<ChatMessage>,
    pub input_text: String,
    pub input_cursor: usize,
    pub should_quit: bool,
    pub is_loading: bool,
    pub spinner_frame: usize,
    pub scroll_offset: usize,
    pub tick_count: u64,
    pub event_tx: Option<mpsc::Sender<TuiEvent>>,
    pub agents: Vec<AgentActivity>,
    pub orchestrating: bool,
    pub conversation_history: Vec<serde_json::Value>,
    pub cwd: String,
    pub view_mode: MainView,
    pub orch_log: Vec<String>,
    pub orch_layer: u8,
    pub orch_completed: usize,
    pub orch_total: usize,
    pub orchestrator_mode_enabled: bool,
    pub socket_pid: Option<u32>,
    pub dev_log: Vec<(String, String, u64)>,
    pub dev_mode_active: bool,
    /// Explicit mode override set via /mode command.
    /// None = auto-classify via complexity_classifier.
    /// Some(mode) = force that mode for every orchestration run until cleared.
    pub mode_override: Option<crate::orchestration::ComplexityMode>,
    /// The unified diff from the last successful orchestration run, waiting to
    /// be saved.  Set by `on_orch_complete()`, cleared once the user saves or
    /// discards it via `/save` / `/discard`.
    pub pending_diff: Option<String>,
    /// Interactive save selector overlay (replaces awaiting_save_path).
    pub save_selector: Option<SaveSelector>,
}

impl App {
    pub fn new(config: MowisConfig, socket_pid: Option<u32>) -> Self {
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string());

        let mut app = Self {
            config,
            messages: Vec::new(),
            input_text: String::new(),
            input_cursor: 0,
            should_quit: false,
            is_loading: false,
            spinner_frame: 0,
            scroll_offset: 0,
            tick_count: 0,
            event_tx: None,
            agents: Vec::new(),
            orchestrating: false,
            conversation_history: Vec::new(),
            cwd,
            view_mode: MainView::Chat,
            orch_log: Vec::new(),
            orch_layer: 0,
            orch_completed: 0,
            orch_total: 0,
            orchestrator_mode_enabled: false,
            socket_pid,
            dev_log: Vec::with_capacity(1000),
            dev_mode_active: false,
            mode_override: None,
            pending_diff: None,
            save_selector: None,
        };

        let welcome_line2 = match app.config.provider {
            crate::config::AiProvider::VertexAi => format!(
                "Provider: Vertex AI | Project: {} | Model: {}",
                app.config.gcp_project_id, app.config.model
            ),
            crate::config::AiProvider::Grok => format!(
                "Provider: Grok AI (xAI) | Model: {}",
                app.config.model
            ),
            crate::config::AiProvider::Groq => format!(
                "Provider: Groq (High-speed) | Model: {}",
                app.config.model
            ),
            crate::config::AiProvider::Anthropic => format!(
                "Provider: Anthropic | Model: {}",
                app.config.model
            ),
            crate::config::AiProvider::OpenAi => format!(
                "Provider: OpenAI | Model: {}",
                app.config.model
            ),
            crate::config::AiProvider::Gemini => format!(
                "Provider: Gemini API | Model: {}",
                app.config.model
            ),
        };

        app.messages.push(ChatMessage {
            role: MessageRole::System,
            content: format!(
                "Welcome to MowisAI! Type your message below and press Enter.\n\
                 {}\n\
                 Just describe what you want to build — orchestration triggers automatically.\n\
                 Type /help for commands, /mode to control orchestration depth, /quit to exit.”,
                welcome_line2
            ),
            timestamp: now(),
        });

        app
    }

    pub fn on_tick(&mut self) {
        self.tick_count += 1;
        if self.is_loading {
            self.spinner_frame = (self.spinner_frame + 1) % 8;
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        // Save selector overlay intercepts all keys when active
        if let Some(ref mut sel) = self.save_selector {
            match key.code {
                KeyCode::Esc => {
                    self.save_selector = None;
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Save cancelled. Run /save to try again, or /discard to throw away.".into(),
                        timestamp: now(),
                    });
                }
                KeyCode::Up => {
                    if !sel.typing_path {
                        sel.selected = sel.selected.saturating_sub(1);
                    }
                }
                KeyCode::Down => {
                    if !sel.typing_path {
                        sel.selected = (sel.selected + 1).min(2);
                    }
                }
                KeyCode::Enter => {
                    if sel.selected == 1 && !sel.typing_path {
                        sel.typing_path = true;
                    } else {
                        let selected = sel.selected;
                        let custom_path = sel.custom_path_input.clone();
                        self.save_selector = None;
                        self.execute_save_option(selected, custom_path);
                    }
                }
                KeyCode::Char(c) if sel.typing_path => {
                    sel.custom_path_input.insert(sel.custom_path_cursor, c);
                    sel.custom_path_cursor += c.len_utf8();
                }
                KeyCode::Backspace if sel.typing_path => {
                    if sel.custom_path_cursor > 0 {
                        let prev = sel.custom_path_input[..sel.custom_path_cursor]
                            .char_indices()
                            .next_back()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        sel.custom_path_input.remove(prev);
                        sel.custom_path_cursor = prev;
                    }
                }
                _ => {}
            }
            return;
        }

        // Tab cycles through views during orchestration or dev mode
        if key.code == KeyCode::Tab && (self.orchestrating || self.dev_mode_active) {
            self.view_mode = match self.view_mode {
                MainView::Chat => MainView::Orchestration,
                MainView::Orchestration => MainView::Development,
                MainView::Development => MainView::Chat,
            };
            return;
        }

        if self.is_loading {
            if key.code == KeyCode::Esc {
                self.is_loading = false;
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "Request cancelled.".into(),
                    timestamp: now(),
                });
            }
            return;
        }

        match key.code {
            KeyCode::Enter => self.submit_input(),
            KeyCode::Backspace => {
                if self.input_cursor > 0 {
                    let prev = self.input_text[..self.input_cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.input_text.remove(prev);
                    self.input_cursor = prev;
                }
            }
            KeyCode::Delete => {
                if self.input_cursor < self.input_text.len() {
                    self.input_text.remove(self.input_cursor);
                }
            }
            KeyCode::Left => {
                if self.input_cursor > 0 {
                    self.input_cursor = self.input_text[..self.input_cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
            }
            KeyCode::Right => {
                if self.input_cursor < self.input_text.len() {
                    self.input_cursor = self.input_text[self.input_cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.input_cursor + i)
                        .unwrap_or(self.input_text.len());
                }
            }
            KeyCode::Home => self.input_cursor = 0,
            KeyCode::End => self.input_cursor = self.input_text.len(),
            KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_add(3);
            }
            KeyCode::Down => {
                self.scroll_offset = self.scroll_offset.saturating_sub(3);
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_add(10);
            }
            KeyCode::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
            }
            KeyCode::Char(c) => {
                self.input_text.insert(self.input_cursor, c);
                self.input_cursor += c.len_utf8();
            }
            KeyCode::Tab => {
                self.input_text.insert_str(self.input_cursor, "  ");
                self.input_cursor += 2;
            }
            _ => {}
        }
    }
    fn execute_save_option(&mut self, option: usize, custom_path: String) {
        match option {
            0 => {
                // Save to current directory
                self.handle_save(".".to_string());
            }
            1 => {
                // Save to specific path
                if custom_path.trim().is_empty() {
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "No path entered. Code discarded. Run the build again to retry.".into(),
                        timestamp: now(),
                    });
                    self.pending_diff = None;
                } else {
                    self.handle_save(custom_path);
                }
            }
            2 => {
                // Create folder in CWD
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let folder_name = format!("mowisai-output-{}", ts);
                self.handle_save(folder_name);
            }
            _ => {}
        }
    }


    fn submit_input(&mut self) {
        let text = self.input_text.trim().to_string();
        if text.is_empty() {
            return;
        }

        self.input_text.clear();
        self.input_cursor = 0;
        self.scroll_offset = 0;

        if text.starts_with('/') {
            self.handle_command(&text);
            return;
        }

        self.messages.push(ChatMessage {
            role: MessageRole::User,
            content: text.clone(),
            timestamp: now(),
        });

        let intent = crate::intent::classify_intent(&text);

        // Orchestrator mode forces all input to Build regardless of classification
        let intent = if self.orchestrator_mode_enabled {
            crate::intent::UserIntent::Build
        } else {
            intent
        };

        match intent {
            crate::intent::UserIntent::Chat => {
                self.is_loading = true;
                self.send_to_gemini(text);
            }
            crate::intent::UserIntent::Build => {
                let mode_label = match &self.mode_override {
                    Some(m) => format!(" (mode: {})", m),
                    None => " (auto-classify)".to_string(),
                };
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!(
                        "ðŸ”¨ Build request detected{} â€” launching orchestration...",
                        mode_label
                    ),
                    timestamp: now(),
                });
                self.start_orchestration(text);
            }
        }
    }

    fn handle_command(&mut self, cmd: &str) {
        match cmd {
            "/quit" | "/exit" | "/q" => {
                // Kill socket server and exit
                if let Some(pid) = self.socket_pid {
                    log::info!("Killing socket server (PID: {})", pid);
                    let _ = std::process::Command::new("kill").arg(pid.to_string()).output();
                    // Delete PID file
                    if let Some(config_dir) = std::env::home_dir() {
                        let pid_file = config_dir.join(".mowisai").join(".socket-server.pid");
                        let _ = std::fs::remove_file(pid_file);
                    }
                }
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "Everything stopped. Goodbye!".into(),
                    timestamp: now(),
                });
                self.should_quit = true;
            }
            "/clear" => {
                self.messages.clear();
                self.conversation_history.clear();
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "Chat cleared.".into(),
                    timestamp: now(),
                });
            }
            "/help" => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "Commands:\n\
                        \n  â”€â”€ Saving Output â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€\
                        \n  /save .                â€” Apply generated code into the CURRENT directory\
                        \n  /save <folder>         â€” Write generated code into a new folder\
                        \n  /discard               â€” Throw away the pending generated code\
                        \n\
                        \n  â”€â”€ Orchestration â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€\
                        \n  /mode simple           â€” Force Simple mode (1 agent, no verification)\
                        \n  /mode standard         â€” Force Standard mode (few agents, 1 verify round)\
                        \n  /mode full             â€” Force Full mode (complete 7-layer pipeline)\
                        \n  /mode auto             â€” Auto-classify mode (default, uses complexity scorer)\
                        \n  /mode                  â€” Show current mode override\
                        \n  /orchestrator          â€” Toggle: force ALL messages to trigger orchestration\
                        \n\
                        \n  â”€â”€ General â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€\
                        \n  /clear                 â€” Clear chat history\
                        \n  /config                â€” Show current configuration\
                        \n  /version               â€” Show version info\
                        \n  /development           â€” Toggle development log view\
                        \n  /kill-socket           â€” Kill the socket server\
                        \n  /socket status         â€” Show socket server status\
                        \n  /socket restart        â€” Restart the socket server\
                        \n  /quit                  â€” Exit MowisAI\
                        \n  /help                  â€” Show this message".into(),
                    timestamp: now(),
                });
            }
            "/config" => {
                let mode_str = match &self.mode_override {
                    Some(m) => format!("forced â†’ {}", m),
                    None => "auto (complexity classifier)".to_string(),
                };
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!(
                        "Configuration:\n  Version: {}\n  Project: {}\n  Model: {}\n  Socket: {}\n  Max Agents: {}\n  Orchestrator Mode: {}\n  Complexity Mode: {}\n  Socket PID: {}",
                        crate::version::full_version(),
                        self.config.gcp_project_id,
                        self.config.model,
                        self.config.socket_path,
                        self.config.max_agents,
                        if self.orchestrator_mode_enabled { "ON âœ“" } else { "OFF" },
                        mode_str,
                        self.socket_pid.map_or("unknown".to_string(), |p| p.to_string())
                    ),
                    timestamp: now(),
                });
            }
            "/setup" => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "To re-run the setup wizard:\n 1. Exit MowisAI (/quit)\n 2. Delete the config: rm ~/.mowisai/config.toml\n 3. Restart the application.".into(),
                    timestamp: now(),
                });
            }
            "/version" => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!(
                        "MowisAI {}\n  OS: {}-{}\n  Build: {}",
                        crate::version::full_version(),
                        std::env::consts::OS,
                        std::env::consts::ARCH,
                        crate::version::build_type()
                    ),
                    timestamp: now(),
                });
            }
            "/orchestrator" => {
                self.orchestrator_mode_enabled = !self.orchestrator_mode_enabled;
                let status = if self.orchestrator_mode_enabled { "ON âœ“" } else { "OFF" };
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!(
                        "ðŸ”§ Orchestrator Mode: {}\n\
                         All subsequent messages will trigger orchestration (no chat responses).\n\
                         Use /orchestrator again to disable.",
                        status
                    ),
                    timestamp: now(),
                });
            }
            "/mode" => {
                let current = match &self.mode_override {
                    Some(m) => format!("forced â†’ {}", m),
                    None => "auto (complexity classifier)".to_string(),
                };
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!(
                        "Current orchestration mode: {}\n\
                         \n  /mode simple    â€” 1 agent, no planner, no merge, no verification\
                         \n  /mode standard  â€” constrained planner, â‰¤3 agents, 1 verify round\
                         \n  /mode full      â€” complete 7-layer pipeline\
                         \n  /mode auto      â€” auto-classify based on task complexity (default)",
                        current
                    ),
                    timestamp: now(),
                });
            }
            "/mode auto" => {
                self.mode_override = None;
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "âœ“ Mode: auto â€” complexity classifier will decide Simple/Standard/Full automatically.".into(),
                    timestamp: now(),
                });
            }
            "/mode simple" => {
                self.mode_override = Some(crate::orchestration::ComplexityMode::Simple);
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "âœ“ Mode locked: simple\n\
                        Next build request â†’ 1 agent, no planner, no merge, no verification.\n\
                        Use /mode auto to return to automatic classification.".into(),
                    timestamp: now(),
                });
            }
            "/mode standard" => {
                self.mode_override = Some(crate::orchestration::ComplexityMode::Standard);
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "âœ“ Mode locked: standard\n\
                        Next build request â†’ constrained planner, â‰¤3 agents, 1 verification round.\n\
                        Use /mode auto to return to automatic classification.".into(),
                    timestamp: now(),
                });
            }
            "/mode full" => {
                self.mode_override = Some(crate::orchestration::ComplexityMode::Full);
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "âœ“ Mode locked: full\n\
                        Next build request â†’ complete 7-layer pipeline (all sandboxes, full verification).\n\
                        Use /mode auto to return to automatic classification.".into(),
                    timestamp: now(),
                });
            }
            "/kill-socket" => {
                if let Some(pid) = self.socket_pid {
                    log::info!("Killing socket server (PID: {})", pid);
                    let _ = std::process::Command::new("kill").arg(pid.to_string()).output();
                    // Delete PID file
                    if let Some(config_dir) = std::env::home_dir() {
                        let pid_file = config_dir.join(".mowisai").join(".socket-server.pid");
                        let _ = std::fs::remove_file(pid_file);
                    }
                    self.socket_pid = None;
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Socket server stopped. Run /launch to restart.".into(),
                        timestamp: now(),
                    });
                } else {
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Socket server is not running.".into(),
                        timestamp: now(),
                    });
                }
            }
            "/socket status" => {
                let status = if self.socket_pid.is_some() { "RUNNING âœ“" } else { "STOPPED âœ—" };
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!(
                        "Socket Server Status:\n  Status: {}\n  PID: {}\n  Path: {}",
                        status,
                        self.socket_pid.map_or("N/A".to_string(), |p| p.to_string()),
                        self.config.socket_path
                    ),
                    timestamp: now(),
                });
            }
            "/socket restart" => {
                if let Some(pid) = self.socket_pid {
                    log::info!("Restarting socket server (PID: {})", pid);
                    let _ = std::process::Command::new("kill").arg(pid.to_string()).output();
                    // Delete PID file
                    if let Some(config_dir) = std::env::home_dir() {
                        let pid_file = config_dir.join(".mowisai").join(".socket-server.pid");
                        let _ = std::fs::remove_file(pid_file);
                    }
                    self.socket_pid = None;

                    std::thread::sleep(std::time::Duration::from_millis(500));

                    // Start new socket server
                    match crate::start_socket_server_daemon(&self.config.socket_path) {
                        Ok(new_pid) => {
                            self.socket_pid = Some(new_pid);
                            self.messages.push(ChatMessage {
                                role: MessageRole::System,
                                content: format!("âœ“ Socket server restarted successfully (PID: {})", new_pid),
                                timestamp: now(),
                            });
                        }
                        Err(e) => {
                            self.messages.push(ChatMessage {
                                role: MessageRole::System,
                                content: format!("âœ— Failed to restart socket server: {}", e),
                                timestamp: now(),
                            });
                        }
                    }
                } else {
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Socket server is not running. Use /launch to start it.".into(),
                        timestamp: now(),
                    });
                }
            }
            "/development" => {
                self.dev_mode_active = !self.dev_mode_active;
                if self.dev_mode_active {
                    self.view_mode = MainView::Development;
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Development mode ON â€” showing all internal logs. Tab cycles Chat â†’ Orchestration â†’ Development. /development to toggle off.".into(),
                        timestamp: now(),
                    });
                } else {
                    self.view_mode = MainView::Chat;
                    self.dev_log.clear();
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Development mode OFF.".into(),
                        timestamp: now(),
                    });
                }
            }
            "/launch" => {
                if self.socket_pid.is_some() {
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Socket server is already running.".into(),
                        timestamp: now(),
                    });
                } else {
                    // Check if socket is responsive (might be running but we don't have PID)
                    if crate::socket_is_responsive(&self.config.socket_path) {
                        // Try to get the PID
                        match crate::get_socket_server_pid() {
                            Ok(pid) => {
                                self.socket_pid = Some(pid);
                                let _ = crate::save_socket_pid(pid);
                                self.messages.push(ChatMessage {
                                    role: MessageRole::System,
                                    content: format!("âœ“ Connected to existing socket server (PID: {})", pid),
                                    timestamp: now(),
                                });
                            }
                            Err(_) => {
                                self.messages.push(ChatMessage {
                                    role: MessageRole::System,
                                    content: "âœ“ Socket server is responsive but PID unknown".into(),
                                    timestamp: now(),
                                });
                            }
                        }
                    } else {
                        // Start new socket server
                        match crate::start_socket_server_daemon(&self.config.socket_path) {
                            Ok(pid) => {
                                self.socket_pid = Some(pid);
                                self.messages.push(ChatMessage {
                                    role: MessageRole::System,
                                    content: format!("ðŸš€ Socket server started successfully (PID: {})", pid),
                                    timestamp: now(),
                                });
                            }
                            Err(e) => {
                                self.messages.push(ChatMessage {
                                    role: MessageRole::System,
                                    content: format!("âœ— Failed to start socket server: {}", e),
                                    timestamp: now(),
                                });
                            }
                        }
                    }
                }
            }
            "/discard" => {
                if self.pending_diff.is_some() {
                    self.pending_diff = None;
                    self.save_selector = None;
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "ðŸ—‘ï¸  Generated code discarded.".into(),
                        timestamp: now(),
                    });
                } else {
                    self.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Nothing to discard â€” no pending diff.".into(),
                        timestamp: now(),
                    });
                }
            }
            _ => {
                // Check if it's a /save command (can have a path argument)
                if cmd.starts_with("/save") {
                    let path_arg = cmd.trim_start_matches("/save").trim();
                    self.handle_save(path_arg.to_string());
                    return;
                }
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!("Unknown command: {}. Type /help for available commands.", cmd),
                    timestamp: now(),
                });
            }
        }
    }

    /// Apply the pending diff to `path_arg`.
    /// - `""` or `"."` â†’ current directory (git apply)
    /// - anything else  â†’ create that folder then write files from the diff
    fn handle_save(&mut self, path_arg: String) {
        let diff = match self.pending_diff.take() {
            Some(d) => d,
            None => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "Nothing to save â€” run a build first.".into(),
                    timestamp: now(),
                });
                return;
            }
        };
        // (save_selector is already cleared by execute_save_option before calling handle_save)

        let target = if path_arg.is_empty() || path_arg == "." {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        } else {
            // Could be absolute or relative
            let p = std::path::PathBuf::from(&path_arg);
            if p.is_absolute() { p } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from("."))
                    .join(&path_arg)
            }
        };

        // Try to create the target directory if it doesn't exist
        if !target.exists() {
            if let Err(e) = std::fs::create_dir_all(&target) {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!("âœ— Failed to create directory {}: {}", target.display(), e),
                    timestamp: now(),
                });
                // Put the diff back so they can try again
                self.pending_diff = Some(diff);
                self.save_selector = Some(SaveSelector { selected: 0, custom_path_input: String::new(), custom_path_cursor: 0, typing_path: false });
                return;
            }
        }

        let target_str = target.display().to_string();

        // Attempt 1: try `git apply` inside the target directory (works when
        // the target is inside a git repo and the diff has git-style paths).
        if Self::try_git_apply(&target, &diff) {
            self.messages.push(ChatMessage {
                role: MessageRole::System,
                content: format!("âœ… Code saved to {} via git apply.", target_str),
                timestamp: now(),
            });
            return;
        }

        // Attempt 2: write the raw diff as a .patch file so nothing is lost.
        let patch_path = target.join("mowisai_output.patch");
        match std::fs::write(&patch_path, &diff) {
            Ok(()) => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!(
                        "âœ… Diff saved to {}\n\
                         (git apply failed â€” saved as raw patch file instead)\n\
                         To apply manually: cd {} && git apply mowisai_output.patch",
                        patch_path.display(),
                        target_str
                    ),
                    timestamp: now(),
                });
            }
            Err(e) => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!("âœ— Failed to write patch file: {}", e),
                    timestamp: now(),
                });
                // Put the diff back so they can try a different path
                self.pending_diff = Some(diff);
                self.save_selector = Some(SaveSelector { selected: 0, custom_path_input: String::new(), custom_path_cursor: 0, typing_path: false });
            }
        }
    }

    /// Run `git apply` with the given diff inside `dir`.  Returns true on success.
    fn try_git_apply(dir: &std::path::Path, diff: &str) -> bool {
        use std::io::Write;
        use std::process::{Command, Stdio};

        // Feed the diff on stdin to avoid temp-file races
        let mut child = match Command::new("git")
            .args(["apply", "--whitespace=nowarn", "-"])
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => return false,
        };

        if let Some(mut stdin) = child.stdin.take() {
            if stdin.write_all(diff.as_bytes()).is_err() {
                return false;
            }
        }

        child.wait().map(|s| s.success()).unwrap_or(false)
    }

    fn send_to_gemini(&mut self, user_text: String) {
        let tx = self.event_tx.clone();

        match self.config.provider {
            crate::config::AiProvider::Grok => {
                // xAI uses the OpenAI message format (role/content objects).
                self.conversation_history.push(serde_json::json!({
                    "role": "user",
                    "content": user_text
                }));

                let api_key = match self.config.grok_api_key() {
                    Ok(k) => k,
                    Err(e) => {
                        self.messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: format!("Error loading Grok API key: {}", e),
                            timestamp: now(),
                        });
                        self.is_loading = false;
                        return;
                    }
                };
                let model = self.config.model.clone();
                let messages = self.conversation_history.clone();

                std::thread::Builder::new()
                    .name("grok-stream".into())
                    .spawn(move || {
                        if let Some(tx) = tx {
                            if let Err(e) = crate::grok_agent::stream_chat(&api_key, &model, &messages, tx.clone()) {
                                let _ = tx.send(TuiEvent::GeminiError(e.to_string()));
                            }
                        }
                    })
                    .ok();
            }
            crate::config::AiProvider::Groq => {
                self.conversation_history.push(serde_json::json!({
                    "role": "user",
                    "content": user_text
                }));

                let api_key = match self.config.groq_api_key() {
                    Ok(k) => k,
                    Err(e) => {
                        self.messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: format!("Error loading Groq API key: {}", e),
                            timestamp: now(),
                        });
                        self.is_loading = false;
                        return;
                    }
                };
                let model = self.config.model.clone();
                let messages = self.conversation_history.clone();

                std::thread::Builder::new()
                    .name("groq-stream".into())
                    .spawn(move || {
                        if let Some(tx) = tx {
                            if let Err(e) = crate::groq_agent::stream_chat(&api_key, &model, &messages, tx.clone()) {
                                let _ = tx.send(TuiEvent::GeminiError(e.to_string()));
                            }
                        }
                    })
                    .ok();
            }
            crate::config::AiProvider::Anthropic => {
                self.conversation_history.push(serde_json::json!({
                    "role": "user",
                    "content": user_text
                }));

                let api_key = self.config.anthropic_api_key().unwrap_or_default();
                let model = self.config.model.clone();
                let messages = self.conversation_history.clone();

                std::thread::Builder::new()
                    .name("anthropic-stream".into())
                    .spawn(move || {
                        if let Some(tx) = tx {
                            if let Err(e) = crate::anthropic_agent::stream_chat(&api_key, &model, &messages, tx.clone()) {
                                let _ = tx.send(TuiEvent::GeminiError(e.to_string()));
                            }
                        }
                    })
                    .ok();
            }
            crate::config::AiProvider::OpenAi => {
                self.conversation_history.push(serde_json::json!({
                    "role": "user",
                    "content": user_text
                }));

                let api_key = self.config.openai_api_key().unwrap_or_default();
                let model = self.config.model.clone();
                let messages = self.conversation_history.clone();

                std::thread::Builder::new()
                    .name("openai-stream".into())
                    .spawn(move || {
                        if let Some(tx) = tx {
                            // Use Groq's stream helper as it is OpenAI compatible
                            let url = "https://api.openai.com/v1/chat/completions";
                            if let Err(e) = crate::openai_agent::stream_chat_custom(&api_key, &model, &messages, url, tx.clone()) {
                                let _ = tx.send(TuiEvent::GeminiError(e.to_string()));
                            }
                        }
                    })
                    .ok();
            }
            crate::config::AiProvider::Gemini => {
                self.conversation_history.push(serde_json::json!({
                    "role": "user",
                    "parts": [{ "text": user_text }]
                }));

                let api_key = self.config.gemini_api_key().unwrap_or_default();
                let model = self.config.model.clone();
                let contents = self.conversation_history.clone();

                std::thread::Builder::new()
                    .name("gemini-api-stream".into())
                    .spawn(move || {
                        if let Some(tx) = tx {
                            if let Err(e) = crate::gemini_agent::stream_chat(&api_key, &model, &contents, tx.clone()) {
                                let _ = tx.send(TuiEvent::GeminiError(e.to_string()));
                            }
                        }
                    })
                    .ok();
            }
            crate::config::AiProvider::VertexAi => {
                // Vertex AI uses Gemini content format (role / parts).
                self.conversation_history.push(serde_json::json!({
                    "role": "user",
                    "parts": [{ "text": user_text }]
                }));

                let project_id = self.config.gcp_project_id.clone();
                let model = self.config.model.clone();
                let contents = self.conversation_history.clone();

                std::thread::Builder::new()
                    .name("gemini-stream".into())
                    .spawn(move || {
                        if let Some(tx) = tx {
                            if let Err(e) = call_gemini_streaming(&project_id, &model, &contents, tx.clone()) {
                                let _ = tx.send(TuiEvent::GeminiError(e.to_string()));
                            }
                        }
                    })
                    .ok();
            }
        }
    }

    fn start_orchestration(&mut self, prompt: String) {
        self.orchestrating = true;
        self.is_loading = true;
        self.agents.clear();
        self.orch_log.clear();
        self.orch_layer = 0;
        self.orch_completed = 0;
        self.orch_total = 0;
        self.view_mode = MainView::Orchestration;

        let config = self.config.clone();
        let tx = self.event_tx.clone();
        let mode_override = self.mode_override.clone();

        std::thread::Builder::new()
            .name("orchestrator".into())
            .spawn(move || {
                let (orch_event_tx, orch_event_rx) = std::sync::mpsc::channel();

                let project_root = std::env::current_dir().unwrap_or_default();
                let orch_config = crate::orchestration::OrchestratorConfig {
                    project_id: config.gcp_project_id.clone(),
                    socket_path: config.socket_path.clone(),
                    project_root,
                    overlay_root: std::path::PathBuf::from(&config.overlay_root),
                    checkpoint_root: std::path::PathBuf::from(&config.checkpoint_root),
                    merge_work_dir: std::path::PathBuf::from(&config.merge_work_dir),
                    max_agents: config.max_agents,
                    max_verification_rounds: 3,
                    staging_dir: None,
                    event_tx: Some(orch_event_tx),
                    mode_override, // None = auto-classify; Some(m) = forced by /mode command
                };

                let orchestrator = crate::orchestration::NewOrchestrator::new(orch_config);

                // Forward orchestrator events to TUI (skip Done â€” main thread sends OrchDone)
                if let Some(ref tx) = tx {
                    let tx_clone = tx.clone();
                    std::thread::spawn(move || {
                        for event in orch_event_rx {
                            let tui_event = match &event {
                                crate::orchestration::OrchestratorEvent::TaskStarted {
                                    worker_id,
                                    description,
                                    ..
                                } => TuiEvent::OrchEvent(OrchActivityEvent::AgentStarted {
                                    agent_id: format!("agent-{}", worker_id),
                                    description: description.clone(),
                                }),
                                crate::orchestration::OrchestratorEvent::ToolCall {
                                    worker_id,
                                    tool_name,
                                    ..
                                } => TuiEvent::OrchEvent(OrchActivityEvent::ToolCall {
                                    agent_id: format!("agent-{}", worker_id),
                                    tool_name: tool_name.clone(),
                                }),
                                crate::orchestration::OrchestratorEvent::TaskCompleted {
                                    worker_id,
                                    ..
                                } => TuiEvent::OrchEvent(OrchActivityEvent::AgentCompleted {
                                    agent_id: format!("agent-{}", worker_id),
                                }),
                                crate::orchestration::OrchestratorEvent::TaskFailed {
                                    worker_id,
                                    error,
                                    ..
                                } => TuiEvent::OrchEvent(OrchActivityEvent::AgentFailed {
                                    agent_id: format!("agent-{}", worker_id),
                                    error: error.clone(),
                                }),
                                crate::orchestration::OrchestratorEvent::LayerProgress {
                                    layer,
                                    message,
                                } => TuiEvent::OrchEvent(OrchActivityEvent::LayerProgress {
                                    layer: *layer,
                                    message: message.clone(),
                                }),
                                crate::orchestration::OrchestratorEvent::StatsUpdate { stats } => {
                                    TuiEvent::OrchEvent(OrchActivityEvent::StatsUpdate {
                                        total: stats.total_tasks,
                                        completed: stats.completed,
                                        failed: stats.failed,
                                    })
                                }
                                crate::orchestration::OrchestratorEvent::Done => continue,
                                _ => continue,
                            };
                            if tx_clone.send(tui_event).is_err() {
                                break;
                            }
                        }
                    });
                }

                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        if let Some(tx) = tx {
                            let _ = tx.send(TuiEvent::GeminiError(
                                format!("Failed to build tokio runtime: {}", e),
                            ));
                            let _ = tx.send(TuiEvent::OrchDone);
                        }
                        return;
                    }
                };

                match runtime.block_on(orchestrator.run(&prompt)) {
                    Ok(output) => {
                        if let Some(ref tx) = tx {
                            let summary = format!(
                                "âœ… Orchestration complete!\n\nSummary: {}\nTasks: {} total, {} completed, {} failed",
                                output.summary,
                                output.scheduler_stats.total_tasks,
                                output.scheduler_stats.completed,
                                output.scheduler_stats.failed,
                            );
                            // Send the diff + summary so the TUI can prompt for
                            // a save path.  OrchComplete is handled before
                            // OrchDone so pending_diff is set first.
                            let _ = tx.send(TuiEvent::OrchComplete {
                                diff: output.merged_diff,
                                summary,
                            });
                            let _ = tx.send(TuiEvent::OrchDone);
                        }
                    }
                    Err(e) => {
                        if let Some(ref tx) = tx {
                            let _ = tx.send(TuiEvent::GeminiError(
                                format!("Orchestration failed: {}", e),
                            ));
                            let _ = tx.send(TuiEvent::OrchDone);
                        }
                    }
                }
            })
            .ok();
    }

    pub fn on_gemini_chunk(&mut self, text: String) {
        if !self.is_loading {
            return;
        }
        if let Some(last) = self.messages.last_mut() {
            if last.role == MessageRole::Assistant {
                last.content.push_str(&text);
                return;
            }
        }
        self.messages.push(ChatMessage {
            role: MessageRole::Assistant,
            content: text,
            timestamp: now(),
        });
    }

    pub fn on_gemini_done(&mut self) {
        if !self.orchestrating {
            self.is_loading = false;
        }
        if let Some(last) = self.messages.last() {
            if last.role == MessageRole::Assistant {
                let content = last.content.clone();
                let history_msg = match self.config.provider {
                    crate::config::AiProvider::Grok | 
                    crate::config::AiProvider::Groq |
                    crate::config::AiProvider::Anthropic |
                    crate::config::AiProvider::OpenAi => serde_json::json!({
                        "role": "assistant",
                        "content": content
                    }),
                    crate::config::AiProvider::VertexAi | crate::config::AiProvider::Gemini => serde_json::json!({
                        "role": "model",
                        "parts": [{ "text": content }]
                    }),
                };
                self.conversation_history.push(history_msg);
            }
        }
    }

    pub fn on_orch_event(&mut self, event: OrchActivityEvent) {
        match event {
            OrchActivityEvent::AgentStarted { ref agent_id, ref description } => {
                self.agents.push(AgentActivity {
                    agent_id: agent_id.clone(),
                    status: "thinking".into(),
                    description: description.clone(),
                    current_tool: None,
                    elapsed_secs: 0,
                });
                let msg = format!("\u{25b8} Agent started: {}", description);
                self.orch_log.push(msg.clone());
                if self.orch_log.len() > 200 {
                    self.orch_log.remove(0);
                }
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!("  {}", msg),
                    timestamp: now(),
                });
            }
            OrchActivityEvent::ToolCall { ref agent_id, ref tool_name } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| &a.agent_id == agent_id) {
                    agent.status = "executing_tool".into();
                    agent.current_tool = Some(tool_name.clone());
                }
                let msg = format!("  \u{1f527} [{}] tool: {}", &agent_id[..agent_id.len().min(12)], tool_name);
                self.orch_log.push(msg);
                if self.orch_log.len() > 200 {
                    self.orch_log.remove(0);
                }
            }
            OrchActivityEvent::AgentCompleted { ref agent_id } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| &a.agent_id == agent_id) {
                    agent.status = "completed".into();
                }
                self.orch_completed += 1;
                let msg = format!("\u{2713} Agent {} done", &agent_id[..agent_id.len().min(12)]);
                self.orch_log.push(msg);
                if self.orch_log.len() > 200 {
                    self.orch_log.remove(0);
                }
            }
            OrchActivityEvent::AgentFailed { ref agent_id, ref error } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| &a.agent_id == agent_id) {
                    agent.status = "failed".into();
                }
                self.orch_completed += 1;
                let msg = format!("\u{2717} {} failed: {}", agent_id, error);
                self.orch_log.push(msg.clone());
                if self.orch_log.len() > 200 {
                    self.orch_log.remove(0);
                }
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!("  {}", msg),
                    timestamp: now(),
                });
            }
            OrchActivityEvent::LayerProgress { layer, ref message } => {
                self.orch_layer = layer;
                let msg = format!("[Layer {}] {}", layer, message);
                self.orch_log.push(msg.clone());
                if self.orch_log.len() > 200 {
                    self.orch_log.remove(0);
                }
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!("  Layer {}: {}", layer, message),
                    timestamp: now(),
                });
            }
            OrchActivityEvent::StatsUpdate { total, completed, failed: _ } => {
                self.orch_total = total;
                self.orch_completed = completed;
            }
        }
    }

    /// Called when orchestration finishes successfully.  Stores the diff and
    /// shows the save prompt.  `on_orch_done` fires immediately after.
    pub fn on_orch_complete(&mut self, diff: String, summary: String) {
        // Show the summary as an assistant message
        self.messages.push(ChatMessage {
            role: MessageRole::Assistant,
            content: summary,
            timestamp: now(),
        });

        if diff.trim().is_empty() {
            // Empty diff â€” agent may have run but diff capture failed, or the agent
            // genuinely made no changes (e.g. it only answered in text).
            self.messages.push(ChatMessage {
                role: MessageRole::System,
                content: "âš ï¸  No code changes were captured (empty diff).\n\
                    \nThis can happen when:\n\
                    â€¢ The agent wrote files but the diff couldn't be captured (check logs)\n\
                    â€¢ The agent answered in text without writing any files\n\
                    â€¢ The socket server lost the container state\n\
                    \nTip: Run /development to see full agent logs on the next run, \
                    or try /mode full for more robust multi-agent execution."
                    .into(),
                timestamp: now(),
            });
        } else {
            self.pending_diff = Some(diff);
            // Open the interactive save selector overlay
            self.save_selector = Some(SaveSelector {
                selected: 0,
                custom_path_input: String::new(),
                custom_path_cursor: 0,
                typing_path: false,
            });
            self.messages.push(ChatMessage {
                role: MessageRole::System,
                content: "Code generated! Use Up/Down to choose where to save, Enter to confirm.".into(),
                timestamp: now(),
            });
        }
    }

    pub fn on_orch_done(&mut self) {
        self.orchestrating = false;
        self.is_loading = false;
        self.agents.clear();
        self.orch_log.clear();
        self.orch_layer = 0;
        self.orch_completed = 0;
        self.orch_total = 0;
        self.view_mode = MainView::Chat;
        self.save_selector = None;
    }

    pub fn on_gemini_error(&mut self, error: String) {
        self.is_loading = false;
        let mut content = format!("Error: {}", error);

        if error.contains("Incorrect API key") || error.contains("401") || error.contains("invalid_api_key") {
            let portal = match self.config.provider {
                crate::config::AiProvider::VertexAi => "Google Cloud Console",
                crate::config::AiProvider::Grok => "console.x.ai",
                crate::config::AiProvider::Groq => "console.groq.com",
                crate::config::AiProvider::Anthropic => "console.anthropic.com",
                crate::config::AiProvider::OpenAi => "platform.openai.com",
                crate::config::AiProvider::Gemini => "aistudio.google.com",
            };
            content.push_str(&format!(
                "\n\nHint: Your API key appears to be invalid for {}. Check your credentials at {}. Type /setup to reset.",
                self.config.provider, portal
            ));
        }

        self.messages.push(ChatMessage {
            role: MessageRole::System,
            content,
            timestamp: now(),
        });
    }

    pub fn on_log_entry(&mut self, level: String, message: String, timestamp: u64) {
        self.dev_log.push((level, message, timestamp));
        if self.dev_log.len() > 1000 {
            self.dev_log.remove(0);
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn get_access_token() -> anyhow::Result<String> {
    use anyhow::Context;
    let output = std::process::Command::new("gcloud")
        .args(["auth", "print-access-token"])
        .output()
        .context("gcloud not found")?;
    if !output.status.success() {
        anyhow::bail!("gcloud auth failed");
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn call_gemini_streaming(
    project_id: &str,
    model: &str,
    contents: &[serde_json::Value],
    tx: mpsc::Sender<TuiEvent>,
) -> anyhow::Result<()> {
    let url = format!(
        "https://us-central1-aiplatform.googleapis.com/v1/projects/{}/locations/us-central1/publishers/google/models/{}:streamGenerateContent?alt=sse",
        project_id, model
    );

    let token = get_access_token()?;

    let system_instruction = serde_json::json!({
        "parts": [{
            "text": "You are MowisAI, an AI coding assistant. You help users with software development tasks. Be concise and technical. For simple questions, answer directly. When the user asks you to build, create, or modify code at scale, indicate that orchestration mode should be used."
        }]
    });

    let body = serde_json::json!({
        "contents": contents,
        "systemInstruction": system_instruction,
        "generationConfig": {
            "temperature": 0.7,
            "maxOutputTokens": 16384
        }
    });

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let resp = client
        .post(&url)
        .bearer_auth(&token)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        let _ = tx.send(TuiEvent::GeminiError(format!("API error {}: {}", status, text)));
        return Ok(());
    }

    use std::io::{BufRead, BufReader};
    let reader = BufReader::new(resp);

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                let _ = tx.send(TuiEvent::GeminiError(format!("Stream read error: {}", e)));
                return Ok(());
            }
        };

        if let Some(data) = line.strip_prefix("data: ") {
            if data.trim().is_empty() || data.trim() == "[DONE]" {
                continue;
            }
            match serde_json::from_str::<serde_json::Value>(data) {
                Ok(json) => {
                    if let Some(text) = json
                        .pointer("/candidates/0/content/parts/0/text")
                        .and_then(|v| v.as_str())
                    {
                        if !text.is_empty() {
                            let _ = tx.send(TuiEvent::GeminiChunk(text.to_string()));
                        }
                    }
                }
                Err(_) => continue,
            }
        }
    }

    let _ = tx.send(TuiEvent::GeminiDone);
    Ok(())
}