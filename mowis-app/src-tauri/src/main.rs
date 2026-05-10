#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent;
mod commands;
mod process;

use commands::*;
use process::AgentManager;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub agent_port: Option<u16>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub gcp_project: Option<String>,
    pub cwd: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            agent_port: None,
            provider: None,
            model: None,
            api_key: None,
            gcp_project: None,
            cwd: None,
        }
    }
}

pub struct AppState {
    pub agent_manager: Mutex<Option<AgentManager>>,
    pub agent_port: Mutex<u16>,
    pub config: Mutex<AppConfig>,
}

impl AppState {
    pub fn new() -> Self {
        let config = Self::load_config_from_disk().unwrap_or_default();
        let port = config.agent_port.unwrap_or(process::DEFAULT_AGENT_PORT);
        Self {
            agent_manager: Mutex::new(None),
            agent_port: Mutex::new(port),
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
        .run(tauri::generate_context!())
        .expect("error running mowis-app");
}
