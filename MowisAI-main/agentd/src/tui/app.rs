use crate::config::MowisConfig;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::mpsc;
use super::event::{OrchActivityEvent, TuiEvent};

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
}

impl App {
    pub fn new(config: MowisConfig) -> Self {
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
        };

        app.messages.push(ChatMessage {
            role: MessageRole::System,
            content: format!(
                "Welcome to MowisAI! Type your message below and press Enter.\n\
                 Project: {} | Model: {}\n\
                 Type /help for commands, /quit or Ctrl+C to exit.",
                app.config.gcp_project_id, app.config.model
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

        match intent {
            crate::intent::UserIntent::Chat => {
                self.is_loading = true;
                self.send_to_gemini(text);
            }
            crate::intent::UserIntent::Build => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "\u{1f528} Build request detected \u{2014} launching orchestration...".into(),
                    timestamp: now(),
                });
                self.start_orchestration(text);
            }
        }
    }

    fn handle_command(&mut self, cmd: &str) {
        match cmd {
            "/quit" | "/exit" | "/q" => self.should_quit = true,
            "/clear" => {
                self.messages.clear();
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "Chat cleared.".into(),
                    timestamp: now(),
                });
            }
            "/help" => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "Commands:\n  /quit     \u{2014} Exit MowisAI\n  /clear    \u{2014} Clear chat history\n  /config   \u{2014} Show current configuration\n  /help     \u{2014} Show this message".into(),
                    timestamp: now(),
                });
            }
            "/config" => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!(
                        "Configuration:\n  Project: {}\n  Model: {}\n  Socket: {}\n  Max Agents: {}",
                        self.config.gcp_project_id,
                        self.config.model,
                        self.config.socket_path,
                        self.config.max_agents
                    ),
                    timestamp: now(),
                });
            }
            _ => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!("Unknown command: {}. Type /help for available commands.", cmd),
                    timestamp: now(),
                });
            }
        }
    }

    fn send_to_gemini(&mut self, user_text: String) {
        self.conversation_history.push(serde_json::json!({
            "role": "user",
            "parts": [{ "text": &user_text }]
        }));

        let project_id = self.config.gcp_project_id.clone();
        let model = self.config.model.clone();
        let contents = self.conversation_history.clone();
        let tx = self.event_tx.clone();

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

    fn start_orchestration(&mut self, prompt: String) {
        self.orchestrating = true;
        self.is_loading = true;
        self.agents.clear();

        let config = self.config.clone();
        let tx = self.event_tx.clone();

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
                };

                let orchestrator = crate::orchestration::NewOrchestrator::new(orch_config);

                // Forward orchestrator events to TUI (skip Done — main thread sends OrchDone)
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
                                "Orchestration complete!\n\nSummary: {}\nTasks: {} total, {} completed, {} failed",
                                output.summary,
                                output.scheduler_stats.total_tasks,
                                output.scheduler_stats.completed,
                                output.scheduler_stats.failed,
                            );
                            let _ = tx.send(TuiEvent::GeminiChunk(summary));
                            let _ = tx.send(TuiEvent::GeminiDone);
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
                self.conversation_history.push(serde_json::json!({
                    "role": "model",
                    "parts": [{ "text": content }]
                }));
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
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!("  \u{25b8} Agent started: {}", description),
                    timestamp: now(),
                });
            }
            OrchActivityEvent::ToolCall { ref agent_id, ref tool_name } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| &a.agent_id == agent_id) {
                    agent.status = "executing_tool".into();
                    agent.current_tool = Some(tool_name.clone());
                }
            }
            OrchActivityEvent::AgentCompleted { ref agent_id } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| &a.agent_id == agent_id) {
                    agent.status = "completed".into();
                }
            }
            OrchActivityEvent::AgentFailed { ref agent_id, ref error } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| &a.agent_id == agent_id) {
                    agent.status = "failed".into();
                }
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!("  \u{2717} Agent {} failed: {}", agent_id, error),
                    timestamp: now(),
                });
            }
            OrchActivityEvent::LayerProgress { layer, ref message } => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!("  Layer {}: {}", layer, message),
                    timestamp: now(),
                });
            }
        }
    }

    pub fn on_orch_done(&mut self) {
        self.orchestrating = false;
        self.is_loading = false;
        self.agents.clear();
    }

    pub fn on_gemini_error(&mut self, error: String) {
        self.is_loading = false;
        self.messages.push(ChatMessage {
            role: MessageRole::System,
            content: format!("Error: {}", error),
            timestamp: now(),
        });
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
