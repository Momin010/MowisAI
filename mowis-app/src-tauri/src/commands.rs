use crate::agent::{AgentMessage, AgentClient, HealthResponse, Session};
use crate::process::{AgentManager, LogSender};
use crate::AppState;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use std::path::PathBuf;

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
            id: "openai".into(),
            name: "OpenAI".into(),
            requires_api_key: true,
            requires_gcp_project: false,
            default_model: "gpt-4o".into(),
        },
        ProviderInfo {
            id: "anthropic".into(),
            name: "Anthropic".into(),
            requires_api_key: true,
            requires_gcp_project: false,
            default_model: "claude-sonnet-4-20250514".into(),
        },
        ProviderInfo {
            id: "google".into(),
            name: "Google (Vertex AI)".into(),
            requires_api_key: false,
            requires_gcp_project: true,
            default_model: "gemini-2.5-pro".into(),
        },
        ProviderInfo {
            id: "openrouter".into(),
            name: "OpenRouter".into(),
            requires_api_key: true,
            requires_gcp_project: false,
            default_model: "anthropic/claude-sonnet-4".into(),
        },
    ]
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn get_client(state: &AppState) -> Result<AgentClient> {
    let mgr_guard = state
        .agent_manager
        .lock()
        .map_err(|e| anyhow::anyhow!("lock poisoned: {}", e))?;
    if let Some(ref mgr) = *mgr_guard {
        return Ok(mgr.client().clone());
    }
    drop(mgr_guard);

    let port = *state
        .agent_port
        .lock()
        .map_err(|e| anyhow::anyhow!("lock poisoned: {}", e))?;
    Ok(AgentClient::new(port))
}

fn config_path() -> Result<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(home).join(".mowisai");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("config.json"))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tauri commands
// ─────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn agent_health(state: State<'_, AppState>) -> Result<HealthResponse, String> {
    let client = get_client(&state).map_err(|e| e.to_string())?;
    client.health().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_create_session(
    state: State<'_, AppState>,
    title: String,
) -> Result<Session, String> {
    let client = get_client(&state).map_err(|e| e.to_string())?;
    client
        .create_session(&title)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_list_sessions(state: State<'_, AppState>) -> Result<Vec<Session>, String> {
    let client = get_client(&state).map_err(|e| e.to_string())?;
    client.list_sessions().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_delete_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let client = get_client(&state).map_err(|e| e.to_string())?;
    client
        .delete_session(&session_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_send_message(
    state: State<'_, AppState>,
    session_id: String,
    text: String,
) -> Result<(), String> {
    let client = get_client(&state).map_err(|e| e.to_string())?;
    client
        .send_message_async(&session_id, &text)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_list_messages(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<AgentMessage>, String> {
    let client = get_client(&state).map_err(|e| e.to_string())?;
    client
        .list_messages(&session_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_abort(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let client = get_client(&state).map_err(|e| e.to_string())?;
    client.abort(&session_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_approve_permission(
    state: State<'_, AppState>,
    session_id: String,
    permission_id: String,
) -> Result<(), String> {
    let client = get_client(&state).map_err(|e| e.to_string())?;
    client
        .approve_permission(&session_id, &permission_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_deny_permission(
    state: State<'_, AppState>,
    session_id: String,
    permission_id: String,
) -> Result<(), String> {
    let client = get_client(&state).map_err(|e| e.to_string())?;
    client
        .deny_permission(&session_id, &permission_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_start(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<u16, String> {
    let resource_dir = app_handle
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?;

    let port = *state
        .agent_port
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?;

    let mut mgr = AgentManager::new(port);

    // We use an unbounded channel to collect logs but don't stream them to the frontend
    // in this simplified version. The logs go to the logger via emit().
    let (log_tx, _log_rx): (LogSender, _) = tokio::sync::mpsc::unbounded_channel();

    mgr.start(&resource_dir, Some(log_tx))
        .await
        .map_err(|e| e.to_string())?;

    let actual_port = mgr.port();

    let mut port_guard = state
        .agent_port
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?;
    *port_guard = actual_port;

    let mut mgr_guard = state
        .agent_manager
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?;
    *mgr_guard = Some(mgr);

    Ok(actual_port)
}

#[tauri::command]
pub async fn agent_stop(state: State<'_, AppState>) -> Result<(), String> {
    let mut mgr_guard = state
        .agent_manager
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?;
    if let Some(ref mut mgr) = *mgr_guard {
        // We need to call async stop, but we're in a sync Mutex context.
        // Use tokio::runtime::Handle to block_on within the async command.
        let handle = tokio::runtime::Handle::current();
        handle.block_on(mgr.stop());
    }
    *mgr_guard = None;
    Ok(())
}

#[tauri::command]
pub async fn agent_get_providers() -> Result<Vec<ProviderInfo>, String> {
    Ok(supported_providers())
}

#[tauri::command]
pub async fn get_agent_port(state: State<'_, AppState>) -> Result<u16, String> {
    let port = *state
        .agent_port
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?;
    Ok(port)
}

#[tauri::command]
pub async fn agent_get_config(state: State<'_, AppState>) -> Result<crate::AppConfig, String> {
    let config = state
        .config
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?;
    Ok(config.clone())
}

#[tauri::command]
pub async fn get_agent_config(state: State<'_, AppState>) -> Result<crate::AppConfig, String> {
    let config = state
        .config
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?;
    Ok(config.clone())
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

    // Update in-memory state
    let mut port_guard = state
        .agent_port
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?;
    if let Some(p) = config.agent_port {
        *port_guard = p;
    }

    let mut config_guard = state
        .config
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?;
    *config_guard = config;

    Ok(())
}
