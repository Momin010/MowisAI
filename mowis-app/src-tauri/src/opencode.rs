use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// The final response from opencode -f json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeResponse {
    pub response: String,
}

/// Session stored in memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub messages: Vec<ChatMessage>,
    pub created_at: i64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp: i64,
}

/// Manages opencode binary location and process spawning.
pub struct OpenCodeManager {
    binary_path: Option<PathBuf>,
    sessions: Arc<Mutex<Vec<Session>>>,
}

impl OpenCodeManager {
    pub fn new() -> Self {
        Self {
            binary_path: None,
            sessions: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create a manager with a known binary path and shared sessions.
    /// Used for spawning from background tasks that already hold the sessions Arc.
    pub fn with_binary_and_sessions(
        binary: PathBuf,
        sessions: Arc<Mutex<Vec<Session>>>,
    ) -> Self {
        Self {
            binary_path: Some(binary),
            sessions,
        }
    }

    pub fn sessions(&self) -> Arc<Mutex<Vec<Session>>> {
        self.sessions.clone()
    }

    /// Find the opencode binary in the resource directory or PATH.
    pub fn find_binary(&mut self, resource_dir: &Path) -> Result<PathBuf> {
        let names = if cfg!(target_os = "windows") {
            vec!["opencode.exe", "opencode"]
        } else {
            vec!["opencode"]
        };

        // Check resource directory first
        for name in &names {
            let path = resource_dir.join(name);
            if path.exists() {
                self.binary_path = Some(path.clone());
                return Ok(path);
            }
        }

        // Check next to the executable
        if let Ok(exe_dir) = std::env::current_exe() {
            if let Some(dir) = exe_dir.parent() {
                for name in &names {
                    let path = dir.join(name);
                    if path.exists() {
                        self.binary_path = Some(path.clone());
                        return Ok(path);
                    }
                }
            }
        }

        // Check PATH
        if let Ok(path) = which::which("opencode") {
            self.binary_path = Some(path.clone());
            return Ok(path);
        }

        anyhow::bail!("opencode binary not found in resources, executable directory, or PATH")
    }

    /// Get the cached binary path.
    pub fn binary_path(&self) -> Option<&Path> {
        self.binary_path.as_deref()
    }

    /// Create a new session.
    pub async fn create_session(&self, title: &str) -> Session {
        let session = Session {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.to_string(),
            messages: Vec::new(),
            created_at: now_unix(),
            status: "idle".to_string(),
        };
        let mut sessions = self.sessions.lock().await;
        sessions.push(session.clone());
        session
    }

    /// List all sessions.
    pub async fn list_sessions(&self) -> Vec<Session> {
        let sessions = self.sessions.lock().await;
        sessions.clone()
    }

    /// Delete a session.
    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        sessions.retain(|s| s.id != session_id);
        Ok(())
    }

    /// Run opencode with a prompt. Returns the final response text.
    /// The cwd is the working directory where opencode will operate.
    pub async fn run_prompt(
        &self,
        session_id: &str,
        prompt: &str,
        cwd: &str,
        event_tx: Option<tokio::sync::mpsc::UnboundedSender<AgentEvent>>,
    ) -> Result<String> {
        let binary = self.binary_path
            .as_ref()
            .context("opencode binary not found — call find_binary first")?;

        // Add user message to session
        {
            let mut sessions = self.sessions.lock().await;
            if let Some(sess) = sessions.iter_mut().find(|s| s.id == session_id) {
                sess.messages.push(ChatMessage {
                    role: "user".into(),
                    content: prompt.to_string(),
                    timestamp: now_unix(),
                });
                sess.status = "running".into();
            }
        }

        if let Some(ref tx) = event_tx {
            let _ = tx.send(AgentEvent::Status {
                session_id: session_id.to_string(),
                status: "thinking".into(),
            });
        }

        let mut child = Command::new(binary)
            .arg("-p")
            .arg(prompt)
            .arg("-f")
            .arg("json")
            .arg("-q")
            .arg("-c")
            .arg(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("failed to spawn opencode")?;

        // Read stderr in background for progress/errors
        let stderr_tx = event_tx.clone();
        let sid = session_id.to_string();
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    log::info!("[opencode:stderr] {}", line);
                    if let Some(ref tx) = stderr_tx {
                        let _ = tx.send(AgentEvent::Progress {
                            session_id: sid.clone(),
                            text: line,
                        });
                    }
                }
            });
        }

        // Read stdout — this is where the JSON response comes
        let mut stdout_buf = String::new();
        if let Some(stdout) = child.stdout.take() {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                log::info!("[opencode:stdout] {}", line);
                stdout_buf.push_str(&line);
                stdout_buf.push('\n');
            }
        }

        // Wait for process to exit
        let status = child.wait().await.context("opencode process failed")?;

        let response_text = if status.success() {
            // Try to parse JSON response
            let trimmed = stdout_buf.trim();
            if trimmed.starts_with('{') {
                match serde_json::from_str::<OpenCodeResponse>(trimmed) {
                    Ok(resp) => resp.response,
                    Err(_) => {
                        // Not valid JSON, return raw output
                        trimmed.to_string()
                    }
                }
            } else {
                // Plain text output
                trimmed.to_string()
            }
        } else {
            let err_msg = format!("opencode exited with status: {}", status);
            if !stdout_buf.trim().is_empty() {
                format!("{}\n\nOutput:\n{}", err_msg, stdout_buf.trim())
            } else {
                err_msg
            }
        };

        // Add assistant message to session
        {
            let mut sessions = self.sessions.lock().await;
            if let Some(sess) = sessions.iter_mut().find(|s| s.id == session_id) {
                sess.messages.push(ChatMessage {
                    role: "assistant".into(),
                    content: response_text.clone(),
                    timestamp: now_unix(),
                });
                sess.status = if status.success() { "done" } else { "error" }.into();
            }
        }

        if let Some(ref tx) = event_tx {
            let _ = tx.send(AgentEvent::Completed {
                session_id: session_id.to_string(),
                response: response_text.clone(),
                success: status.success(),
            });
        }

        Ok(response_text)
    }
}

/// Events emitted during agent execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    #[serde(rename = "status")]
    Status {
        session_id: String,
        status: String,
    },
    #[serde(rename = "progress")]
    Progress {
        session_id: String,
        text: String,
    },
    #[serde(rename = "completed")]
    Completed {
        session_id: String,
        response: String,
        success: bool,
    },
}

/// Write the opencode config file so it picks up the right provider/model/key.
pub fn write_opencode_config(
    provider: &str,
    model: &str,
    api_key: &str,
    gcp_project: &str,
) -> Result<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());

    let config_dir = PathBuf::from(&home).join(".config").join("opencode");
    std::fs::create_dir_all(&config_dir).context("create opencode config dir")?;
    let config_path = config_dir.join("config.json");

    // Map our provider names to opencode's provider names and env var conventions
    let (oc_provider, env_key) = match provider {
        "anthropic" => ("anthropic", "ANTHROPIC_API_KEY"),
        "openai" => ("openai", "OPENAI_API_KEY"),
        "gemini" => ("gemini", "GEMINI_API_KEY"),
        "groq" => ("groq", "GROQ_API_KEY"),
        "xai" => ("xai", "XAI_API_KEY"),
        "openrouter" => ("openrouter", "OPENROUTER_API_KEY"),
        "vertexai" => ("vertexai", ""),
        "copilot" => ("copilot", ""),
        "azure" => ("azure", "AZURE_OPENAI_API_KEY"),
        "bedrock" => ("bedrock", ""),
        other => (other, ""),
    };

    // Set the API key as environment variable (opencode reads env vars)
    if !env_key.is_empty() && !api_key.is_empty() {
        std::env::set_var(env_key, api_key);
    }

    // Also set GCP project if needed
    if !gcp_project.is_empty() {
        std::env::set_var("GOOGLE_CLOUD_PROJECT", gcp_project);
        std::env::set_var("VERTEXAI_PROJECT", gcp_project);
    }

    // Build config JSON
    let mut config = serde_json::json!({
        "providers": {},
        "agents": {}
    });

    // Set the provider config
    if !api_key.is_empty() {
        config["providers"][oc_provider] = serde_json::json!({
            "apiKey": api_key
        });
    }

    // Set the model for the coder agent
    if !model.is_empty() {
        config["agents"]["coder"] = serde_json::json!({
            "model": model,
            "maxTokens": 16384
        });
    }

    let json = serde_json::to_string_pretty(&config).context("serialize opencode config")?;
    std::fs::write(&config_path, json).context("write opencode config")?;

    log::info!("[opencode] wrote config to {}", config_path.display());

    Ok(config_path)
}
