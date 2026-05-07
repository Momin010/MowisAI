#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod platform;
mod backend;
mod sandbox;
mod agent_client;
mod agent_manager;
mod types;
mod state;
mod bridge_loop;
mod commands;

use backend::BackendBridge;
use bridge_loop::start_bridge;
use commands::*;
use state::AppState;
use std::sync::Arc;

fn main() {
    let bridge = BackendBridge::new();
    let state = Arc::new(AppState::new(bridge));
    let state_for_setup = Arc::clone(&state);

    tauri::Builder::default()
        .manage(state)
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let cmd_tx = start_bridge(app.handle().clone(), Arc::clone(&state_for_setup));
            *state_for_setup.cmd_tx.lock().unwrap() = Some(cmd_tx);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_messages,
            get_tasks,
            get_session_history,
            get_usage_history,
            get_config,
            save_config,
            validate_git_repository,
            clone_github_repo,
            get_daemon_status,
            check_daemon,
            start_session,
            stop_session,
            send_message,
            get_current_session,
            load_session,
            clear_current_session,
            window_control,
            get_system_info,
            get_stats,
            get_connection_state,
            get_engine_logs,
            get_sandbox_status,
            discard_sandbox,
            get_sandbox_size,
            get_zero_workspace,
            get_zero_workspace_base,
            get_developer_config,
            save_developer_config,
            validate_developer_config,
            start_developer_bootstrap,
            clear_developer_config,
            // New agent commands
            agent_create_session,
            agent_send_message,
            agent_abort,
            agent_approve_permission,
            agent_deny_permission,
            agent_list_sessions,
            agent_health,
            agent_delete_session,
            agent_list_messages,
            agent_start,
        ])
        .run(tauri::generate_context!())
        .expect("error running mowis-desktop");
}
