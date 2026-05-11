use crate::opencode::{AgentEvent, OpenCodeManager, write_opencode_config};
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};

// ─────────────────────────────────────────────────────────────────────────────
// Provider info
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub requires_api_key: bool,
    pub requires_gcp_project: bool,
    pub default_model: String,
}

fn supported_providers() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo {
            id: "anthropic".into(),
            name: "Anthropic".into(),
            requires_api_key: true,
            requires_gcp_project: false,
            default_model: "claude-sonnet-4-20250514".into(),
        },
        ProviderInfo {
            id: "openai".into(),
            name: "OpenAI".into(),
            requires_api_key: true,
            requires_gcp_project: false,
            default_model: "gpt-4o".into(),
        },
        ProviderInfo {
            id: "gemini".into(),
            name: "Google Gemini".into(),
            requires_api_key: true,
            requires_gcp_project: false,
            default_model: "gemini-2.5-pro".into(),
        },
        ProviderInfo {
            id: "groq".into(),
            name: "Groq".into(),
            requires_api_key: true,
            requires_gcp_project: false,
            default_model: "llama-3.3-70b-versatile".into(),
        },
        ProviderInfo {
            id: "openrouter".into(),
            name: "OpenRouter".into(),
            requires_api_key: true,
            requires_gcp_project: false,
            default_model: "anthropic/claude-sonnet-4".into(),
        },
        ProviderInfo {
            id: "vertexai".into(),
            name: "Google Vertex AI".into(),
            requires_api_key: false,
            requires_gcp_project: true,
            default_model: "gemini-2.5-pro".into(),
        },
        ProviderInfo {
            id: "xai".into(),
            name: "xAI".into(),
            requires_api_key: true,
            requires_gcp_project: false,
            default_model: "grok-3".into(),
        },
        ProviderInfo {
            id: "copilot".into(),
            name: "GitHub Copilot".into(),
            requires_api_key: false,
            requires_gcp_project: false,
            default_model: "gpt-4o".into(),
        },
    ]
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn config_path() -> anyhow::Result<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(home).join(".mowisai");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("config.json"))
}

// ─────────────────────────────────────────────────────────────────────────────
// Health — just checks if the opencode binary is found
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub healthy: bool,
    pub version: String,
    pub cwd: String,
}

#[tauri::command]
pub async fn agent_health(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<HealthResponse, String> {
    let mut mgr = state.opencode.lock().await;

    let resource_dir = app_handle
        .path()
        .resource_dir()
        .unwrap_or_else(|_| PathBuf::from("."));

    let binary_found = mgr.find_binary(&resource_dir).is_ok();
    let config = state.config.lock().map_err(|e| e.to_string())?;
    let cwd = config.cwd.clone().unwrap_or_default();

    Ok(HealthResponse {
        healthy: binary_found,
        version: "opencode".into(),
        cwd,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Sessions — in-memory, managed by OpenCodeManager
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub status: String,
    pub message_count: usize,
}

#[tauri::command]
pub async fn agent_create_session(
    state: State<'_, AppState>,
    title: String,
) -> Result<SessionInfo, String> {
    let mgr = state.opencode.lock().await;
    let sess = mgr.create_session(&title).await;
    Ok(SessionInfo {
        id: sess.id,
        title: sess.title,
        created_at: sess.created_at,
        status: sess.status,
        message_count: 0,
    })
}

#[tauri::command]
pub async fn agent_list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionInfo>, String> {
    let mgr = state.opencode.lock().await;
    let sessions = mgr.list_sessions().await;
    Ok(sessions
        .into_iter()
        .map(|s| SessionInfo {
            id: s.id,
            title: s.title,
            created_at: s.created_at,
            status: s.status,
            message_count: s.messages.len(),
        })
        .collect())
}

#[tauri::command]
pub async fn agent_delete_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let mgr = state.opencode.lock().await;
    mgr.delete_session(&session_id)
        .await
        .map_err(|e| e.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Messages — send a prompt, get streaming events, final response
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageInfo {
    pub role: String,
    pub content: String,
    pub timestamp: i64,
}

#[tauri::command]
pub async fn agent_send_message(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    session_id: String,
    text: String,
) -> Result<(), String> {
    // Get config for cwd and provider settings
    let config = {
        let c = state.config.lock().map_err(|e| e.to_string())?;
        c.clone()
    };

    let cwd = config.cwd.clone().unwrap_or_else(|| {
        std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_else(|_| ".".into())
    });

    // Write opencode config with current provider settings
    let provider = config.provider.as_deref().unwrap_or("anthropic");
    let model = config.model.as_deref().unwrap_or("");
    let api_key = config.api_key.as_deref().unwrap_or("");
    let gcp_project = config.gcp_project.as_deref().unwrap_or("");
    write_opencode_config(provider, model, api_key, gcp_project)
        .map_err(|e| e.to_string())?;

    // Create event channel
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();

    // Forward events to the Tauri frontend
    let app = app_handle.clone();
    tokio::spawn(async move {
        while let Some(evt) = event_rx.recv().await {
            let _ = app.emit("agent_event", &evt);
        }
    });

    // Extract binary path and sessions Arc under a single lock, then drop it
    let (binary, sessions) = {
        let mut mgr = state.opencode.lock().await;

        // Make sure binary is found
        if mgr.binary_path().is_none() {
            let resource_dir = app_handle
                .path()
                .resource_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            mgr.find_binary(&resource_dir).map_err(|e| e.to_string())?;
        }

        let binary = mgr
            .binary_path()
            .ok_or("opencode binary not found")?
            .to_path_buf();
        let sessions = mgr.sessions();
        (binary, sessions)
    }; // lock dropped here

    let sid = session_id.clone();
    let prompt = text.clone();

    // Run in background — the frontend gets events via agent_event
    tokio::spawn(async move {
        let mgr_for_run = OpenCodeManager::with_binary_and_sessions(binary, sessions);
        match mgr_for_run.run_prompt(&sid, &prompt, &cwd, Some(event_tx)).await {
            Ok(_) => {
                log::info!("[cmd] opencode completed for session {}", sid);
            }
            Err(e) => {
                log::error!("[cmd] opencode failed for session {}: {:#}", sid, e);
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn agent_list_messages(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<MessageInfo>, String> {
    let mgr = state.opencode.lock().await;
    let sessions = mgr.list_sessions().await;
    let sess = sessions
        .iter()
        .find(|s| s.id == session_id)
        .ok_or("session not found")?;

    Ok(sess
        .messages
        .iter()
        .map(|m| MessageInfo {
            role: m.role.clone(),
            content: m.content.clone(),
            timestamp: m.timestamp,
        })
        .collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// Agent control — abort not supported in process mode (kill process)
// ─────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn agent_abort(session_id: String) -> Result<(), String> {
    log::warn!("[cmd] abort requested for session {} — not yet implemented for process mode", session_id);
    Ok(())
}

#[tauri::command]
pub async fn agent_approve_permission(
    session_id: String,
    permission_id: String,
) -> Result<(), String> {
    log::warn!(
        "[cmd] approve permission {} for session {} — not applicable in process mode",
        permission_id,
        session_id
    );
    Ok(())
}

#[tauri::command]
pub async fn agent_deny_permission(
    session_id: String,
    permission_id: String,
) -> Result<(), String> {
    log::warn!(
        "[cmd] deny permission {} for session {} — not applicable in process mode",
        permission_id,
        session_id
    );
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Agent start/stop — no longer needed (no separate server), kept as no-ops
// ─────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn agent_start(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let resource_dir = app_handle
        .path()
        .resource_dir()
        .unwrap_or_else(|_| PathBuf::from("."));

    let mut mgr = state.opencode.lock().await;
    mgr.find_binary(&resource_dir).map_err(|e| e.to_string())?;

    log::info!("[cmd] opencode binary found: {:?}", mgr.binary_path());
    Ok(())
}

#[tauri::command]
pub async fn agent_stop() -> Result<(), String> {
    // No persistent server to stop
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Provider / config commands
// ─────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn agent_get_providers() -> Result<Vec<ProviderInfo>, String> {
    Ok(supported_providers())
}

#[tauri::command]
pub async fn agent_get_config(state: State<'_, AppState>) -> Result<crate::AppConfig, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    Ok(config.clone())
}

#[tauri::command]
pub async fn get_agent_config(state: State<'_, AppState>) -> Result<crate::AppConfig, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    Ok(config.clone())
}

#[tauri::command]
pub async fn get_agent_port(state: State<'_, AppState>) -> Result<u16, String> {
    // Keep for backwards compat — return a dummy port since we no longer use HTTP
    Ok(0)
}

#[tauri::command]
pub async fn save_agent_config(
    state: State<'_, AppState>,
    config: crate::AppConfig,
) -> Result<(), String> {
    // Persist to disk
    let path = config_path().map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;

    // Write opencode config
    let provider = config.provider.as_deref().unwrap_or("anthropic");
    let model = config.model.as_deref().unwrap_or("");
    let api_key = config.api_key.as_deref().unwrap_or("");
    let gcp_project = config.gcp_project.as_deref().unwrap_or("");
    write_opencode_config(provider, model, api_key, gcp_project)
        .map_err(|e| e.to_string())?;

    // Update in-memory state
    let mut config_guard = state.config.lock().map_err(|e| e.to_string())?;
    *config_guard = config;

    Ok(())
}
