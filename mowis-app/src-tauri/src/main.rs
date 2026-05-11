#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod opencode;

use commands::*;
use opencode::OpenCodeManager;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::Manager;
use tokio::sync::Mutex as TokioMutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub gcp_project: Option<String>,
    pub cwd: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            provider: None,
            model: None,
            api_key: None,
            gcp_project: None,
            cwd: None,
        }
    }
}

pub struct AppState {
    pub opencode: TokioMutex<OpenCodeManager>,
    pub config: Mutex<AppConfig>,
}

impl AppState {
    pub fn new() -> Self {
        let config = Self::load_config_from_disk().unwrap_or_default();
        Self {
            opencode: TokioMutex::new(OpenCodeManager::new()),
            config: Mutex::new(config),
        }
    }

    fn config_path() -> anyhow::Result<std::path::PathBuf> {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_else(|_| ".".to_string());
        let dir = std::path::PathBuf::from(home).join(".mowisai");
        std::fs::create_dir_all(&dir)?;
        Ok(dir.join("config.json"))
    }

    fn load_config_from_disk() -> anyhow::Result<AppConfig> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(AppConfig::default());
        }
        let data = std::fs::read_to_string(&path)?;
        let config: AppConfig = serde_json::from_str(&data)?;
        Ok(config)
    }
}

fn main() {
    env_logger::init();

    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            agent_health,
            agent_create_session,
            agent_list_sessions,
            agent_delete_session,
            agent_send_message,
            agent_list_messages,
            agent_abort,
            agent_approve_permission,
            agent_deny_permission,
            agent_start,
            agent_stop,
            agent_get_providers,
            agent_get_config,
            get_agent_port,
            save_agent_config,
            get_agent_config,
        ])
        .setup(|app| {
            // Find opencode binary on startup
            let resource_dir = app
                .path()
                .resource_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));

            // Get an owned AppHandle so we can move it into the async block
            let handle = app.handle().clone();

            tauri::async_runtime::spawn(async move {
                let state = handle.state::<AppState>();
                let mut mgr = state.opencode.lock().await;
                match mgr.find_binary(&resource_dir) {
                    Ok(path) => {
                        log::info!("[setup] opencode binary found: {}", path.display());
                    }
                    Err(e) => {
                        log::warn!("[setup] opencode binary not found: {:#}", e);
                    }
                }
                drop(mgr);

                // Write opencode config from saved settings
                let config = state.config.lock().ok().map(|c| c.clone());
                if let Some(config) = config {
                    let provider = config.provider.as_deref().unwrap_or("anthropic");
                    let model = config.model.as_deref().unwrap_or("");
                    let api_key = config.api_key.as_deref().unwrap_or("");
                    let gcp_project = config.gcp_project.as_deref().unwrap_or("");
                    if let Err(e) = opencode::write_opencode_config(provider, model, api_key, gcp_project) {
                        log::warn!("[setup] failed to write opencode config: {:#}", e);
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error running mowis-app");
}
