use crate::config::MowisConfig;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::mpsc;
use super::event::TuiEvent;

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

        self.is_loading = true;
        self.send_to_gemini(text);
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
            .name("gemini-request".into())
            .spawn(move || {
                let result = call_gemini_batch(&project_id, &model, &contents);
                if let Some(tx) = tx {
                    match result {
                        Ok(response_text) => {
                            let _ = tx.send(TuiEvent::GeminiChunk(response_text));
                            let _ = tx.send(TuiEvent::GeminiDone);
                        }
                        Err(e) => {
                            let _ = tx.send(TuiEvent::GeminiError(e.to_string()));
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
        self.is_loading = false;
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

fn call_gemini_batch(project_id: &str, model: &str, contents: &[serde_json::Value]) -> anyhow::Result<String> {
    use anyhow::{anyhow, Context};

    let url = format!(
        "https://us-central1-aiplatform.googleapis.com/v1/projects/{}/locations/us-central1/publishers/google/models/{}:generateContent",
        project_id, model
    );

    let token_output = std::process::Command::new("gcloud")
        .args(["auth", "print-access-token"])
        .output()
        .context("gcloud not found")?;
    if !token_output.status.success() {
        return Err(anyhow!("gcloud auth failed"));
    }
    let token = String::from_utf8(token_output.stdout)?.trim().to_string();

    let system_instruction = serde_json::json!({
        "parts": [{
            "text": "You are MowisAI, an AI coding assistant. You help users with software development tasks. For simple questions, answer directly. When the user asks you to build, create, or modify code, describe what you would do and how you would approach it. Be concise and technical."
        }]
    });

    let body = serde_json::json!({
        "contents": contents,
        "systemInstruction": system_instruction,
        "generationConfig": {
            "temperature": 0.7,
            "maxOutputTokens": 8192
        }
    });

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let resp = client
        .post(&url)
        .bearer_auth(&token)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .context("Gemini HTTP request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        return Err(anyhow!("Gemini API error {}: {}", status, text));
    }

    let json: serde_json::Value = resp.json()?;
    let text = json
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(|v| v.as_str())
        .unwrap_or("(no response)")
        .to_string();

    Ok(text)
}
