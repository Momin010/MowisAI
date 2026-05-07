use crate::sandbox;
use crate::sandbox::SandboxInfo;
use crate::state::*;
use crate::types::*;
use crate::agent_client;
use crate::platform;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tauri::{Emitter, Manager, State};
use tokio::process::Command;
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Git helpers (used by validate_git_repository and clone_github_repo)
// ─────────────────────────────────────────────────────────────────────────────

async fn run_git_command(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let git = which::which("git").map_err(|_| "git was not found on PATH".to_string())?;
    let mut cmd = Command::new(git);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd
        .output()
        .await
        .map_err(|err| format!("run git {}: {err}", args.join(" ")))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        Err(if detail.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            detail
        })
    }
}

async fn optional_git_command(args: &[&str], cwd: &Path) -> Option<String> {
    match run_git_command(args, Some(cwd)).await {
        Ok(value) if !value.is_empty() => Some(value),
        _ => None,
    }
}

async fn collect_git_repository_info(
    path: PathBuf,
    source: &str,
    repo_url: Option<String>,
) -> Result<GitRepositoryInfo, String> {
    let canonical = fs::canonicalize(&path)
        .map_err(|err| format!("read repository path {}: {err}", path.display()))?;
    if !canonical.is_dir() {
        return Err(format!("{} is not a folder", canonical.display()));
    }

    let inside = run_git_command(&["rev-parse", "--is-inside-work-tree"], Some(&canonical)).await?;
    if inside.trim() != "true" {
        return Err(format!("{} is not a Git repository", canonical.display()));
    }

    let top_level = run_git_command(&["rev-parse", "--show-toplevel"], Some(&canonical)).await?;
    let root = fs::canonicalize(top_level.trim())
        .map_err(|err| format!("resolve repository root {}: {err}", top_level.trim()))?;
    let name = root
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "repository".to_string());
    let branch = optional_git_command(&["rev-parse", "--abbrev-ref", "HEAD"], &root).await;
    let remote_url = optional_git_command(&["config", "--get", "remote.origin.url"], &root).await;

    Ok(GitRepositoryInfo {
        path: path_to_string(&root),
        name,
        branch,
        remote_url,
        source: source.to_string(),
        repo_url,
    })
}

fn strip_extended_path_prefix(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with(r"\\?\") {
        PathBuf::from(s[4..].to_string())
    } else {
        path
    }
}

fn path_to_string(path: &Path) -> String {
    let s = path.display().to_string();
    if s.starts_with(r"\\?\") {
        s[4..].to_string()
    } else {
        s
    }
}

fn parse_github_repo_url(raw: &str) -> Result<(String, String), String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Paste a GitHub repository URL".to_string());
    }

    let path = if let Some(rest) = trimmed.strip_prefix("https://github.com/") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        rest
    } else {
        return Err("Use a GitHub HTTPS or SSH repository URL".to_string());
    };

    let without_fragment = path.split(['#', '?']).next().unwrap_or(path);
    let trimmed_path = without_fragment.trim_matches('/');
    let clean = trimmed_path.strip_suffix(".git").unwrap_or(trimmed_path);
    let parts: Vec<&str> = clean.split('/').collect();
    if parts.len() != 2 || !is_valid_github_segment(parts[0]) || !is_valid_github_segment(parts[1]) {
        return Err("Use a repository URL like https://github.com/owner/repo".to_string());
    }

    Ok((parts[0].to_string(), parts[1].to_string()))
}

fn is_valid_github_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
}

fn is_non_empty_dir(path: &Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    if !path.is_dir() {
        return Err(format!("{} already exists and is not a folder", path.display()));
    }
    let mut entries = fs::read_dir(path)
        .map_err(|err| format!("read destination {}: {err}", path.display()))?;
    Ok(entries.next().is_some())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tauri Commands
// ─────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn validate_git_repository(path: String) -> Result<GitRepositoryInfo, String> {
    collect_git_repository_info(PathBuf::from(path), "local", None).await
}

#[tauri::command]
pub async fn clone_github_repo(
    repo_url: String,
    destination_parent: String,
) -> Result<GitRepositoryInfo, String> {
    let (_owner, repo_name) = parse_github_repo_url(&repo_url)?;
    let parent = fs::canonicalize(PathBuf::from(&destination_parent))
        .map_err(|err| format!("Destination folder not found: {err}"))?;
    let parent = strip_extended_path_prefix(parent);
    if !parent.is_dir() {
        return Err(format!("{} is not a folder", parent.display()));
    }

    let target = parent.join(&repo_name);
    if is_non_empty_dir(&target)? {
        return Err(format!("{} already exists and is not empty", target.display()));
    }

    let git = which::which("git").map_err(|_| {
        "Git is not installed. Download it from https://git-scm.com/downloads".to_string()
    })?;
    let output = Command::new(git)
        .arg("clone")
        .arg(repo_url.trim())
        .arg(&target)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|err| format!("run git clone: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(if detail.is_empty() {
            "git clone failed".to_string()
        } else {
            detail
        });
    }

    collect_git_repository_info(target, "github", Some(repo_url.trim().to_string())).await
}

#[tauri::command]
pub async fn get_messages(state: State<'_, Arc<AppState>>) -> Result<Vec<ChatMessage>, String> {
    Ok(state.messages.lock().unwrap().clone())
}

#[tauri::command]
pub async fn get_tasks(state: State<'_, Arc<AppState>>) -> Result<Vec<Task>, String> {
    Ok(state.tasks.lock().unwrap().values().cloned().collect())
}

#[tauri::command]
pub async fn get_session_history(state: State<'_, Arc<AppState>>) -> Result<Vec<SessionSummary>, String> {
    let mut history = state.session_history.lock().unwrap().clone();
    history.sort_by_key(|item| item.started_at);
    Ok(history)
}

#[tauri::command]
pub async fn get_usage_history(state: State<'_, Arc<AppState>>) -> Result<Vec<UsageRecord>, String> {
    let mut history = state.usage_history.lock().unwrap().clone();
    history.sort_by_key(|item| item.ts);
    Ok(history)
}

#[tauri::command]
pub async fn get_config(state: State<'_, Arc<AppState>>) -> Result<Config, String> {
    Ok(state.config.lock().unwrap().clone())
}

#[tauri::command]
pub async fn save_config(state: State<'_, Arc<AppState>>, config: Config) -> Result<(), String> {
    *state.config.lock().unwrap() = config;
    save_state(&state)
}

#[tauri::command]
pub async fn get_daemon_status(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    Ok(*state.daemon_connected.lock().unwrap())
}

#[tauri::command]
pub async fn check_daemon(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    // Clone sender outside the lock to avoid holding MutexGuard across .await
    let tx_opt = state.cmd_tx.lock().unwrap().clone();
    if let Some(tx) = tx_opt {
        let _ = tx.send(BridgeCommand::CheckSocket).await;
    }
    Ok(*state.daemon_connected.lock().unwrap())
}

#[tauri::command]
pub async fn start_session(
    state: State<'_, Arc<AppState>>,
    prompt: String,
    mode: Option<String>,
    project_path: Option<String>,
    repo_url: Option<String>,
    repo_source: Option<String>,
    _images: Option<Vec<ImageAttachment>>,
) -> Result<String, String> {
    let session_id = Uuid::new_v4().to_string();
    let cfg = state.config.lock().unwrap().clone();
    let resolved_mode = mode.unwrap_or_else(|| cfg.mode.clone());

    // Discard any sandbox left over from a previous session.
    {
        let prev = state.active_sandbox.lock().unwrap().take();
        if let Some(sb) = prev {
            if let Err(err) = sandbox::destroy_sandbox(&sb.id) {
                log::warn!("Failed to clean up previous sandbox {}: {err}", sb.id);
            }
        }
    }

    // Resolve repo context, optionally redirecting project_path to a sandbox upper_dir.
    // NOTE: zero mode does NOT use repo_context (it writes directly to a workspace on disk).
    // Clone before the if-else because the else branch moves project_path.
    let _project_path_zero = project_path.clone();
    let repo_context = if resolved_mode == "zero" {
        None
    } else {
        project_path
        .filter(|path| !path.trim().is_empty())
        .map(|path| -> Result<RepositoryContext, String> {
            if cfg.sandbox_enabled {
                let lower = std::path::Path::new(&path);
                match sandbox::create_sandbox(lower) {
                    Ok(info) => {
                        let upper = info.upper_dir.clone();
                        log::info!("Sandbox created: id={} upper={}", info.id, upper);
                        *state.active_sandbox.lock().unwrap() = Some(info);
                        Ok(RepositoryContext {
                            project_path: upper,
                            repo_source: repo_source.unwrap_or_else(|| "local".to_string()),
                            repo_url,
                        })
                    }
                    Err(err) => {
                        log::warn!("Sandbox creation failed ({err}), falling back to direct access");
                        Ok(RepositoryContext {
                            project_path: path,
                            repo_source: repo_source.unwrap_or_else(|| "local".to_string()),
                            repo_url,
                        })
                    }
                }
            } else {
                Ok(RepositoryContext {
                    project_path: path,
                    repo_source: repo_source.unwrap_or_else(|| "local".to_string()),
                    repo_url,
                })
            }
        })
        .transpose()?
    };
    let started_at = now();

    // Reset state
    *state.current_session_id.lock().unwrap() = Some(session_id.clone());
    state.messages.lock().unwrap().clear();
    state.tasks.lock().unwrap().clear();
    *state.tokens_total.lock().unwrap() = 0;
    *state.tool_calls_total.lock().unwrap() = 0;

    // Push user message
    state.messages.lock().unwrap().push(ChatMessage::User {
        content: prompt.clone(),
        ts: started_at,
    });

    // Send command to bridge — clone sender first to avoid holding lock across .await
    let summary = SessionSummary {
        id: session_id.clone(),
        prompt: prompt.chars().take(80).collect(),
        status: "running".into(),
        started_at,
        completed_at: None,
        task_count: 0,
        tasks_done: 0,
        tokens_total: 0,
        duration_secs: None,
        mode: Some(resolved_mode.clone()),
    };
    state.sessions.lock().unwrap().insert(session_id.clone(), SessionRecord {
        summary: summary.clone(),
        messages: state.messages.lock().unwrap().clone(),
        tasks: Vec::new(),
        tokens_total: 0,
        tool_calls_total: 0,
    });
    {
        let mut history = state.session_history.lock().unwrap();
        upsert_history(&mut history, summary);
    }
    save_state(&state)?;

    let tx_opt = state.cmd_tx.lock().unwrap().clone();
    if let Some(tx) = tx_opt {
        if resolved_mode == "zero" {
            // Zero mode is deprecated — use agent_send_message instead.
            log::warn!("Zero mode is deprecated for session {}", session_id);
            let _ = tx.send(BridgeCommand::StartOrchestration {
                session_id: session_id.clone(),
                prompt,
                max_agents: cfg.max_agents,
                mode: "auto".to_string(),
                repo_context,
            }).await;
        } else {
            let _ = tx.send(BridgeCommand::StartOrchestration {
                session_id: session_id.clone(),
                prompt,
                max_agents: cfg.max_agents,
                mode: resolved_mode,
                repo_context,
            }).await;
        }
    }

    Ok(session_id)
}

#[tauri::command]
pub async fn stop_session(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let tx_opt = state.cmd_tx.lock().unwrap().clone();
    if let Some(tx) = tx_opt {
        let _ = tx.send(BridgeCommand::StopOrchestration).await;
    }

    // Discard sandbox on stop.
    {
        let sb = state.active_sandbox.lock().unwrap().take();
        if let Some(info) = sb {
            if let Err(err) = sandbox::destroy_sandbox(&info.id) {
                log::warn!("Failed to destroy sandbox {} on stop: {err}", info.id);
            }
        }
    }

    {
        let mut msgs = state.messages.lock().unwrap();
        msgs.push(ChatMessage::System { content: "Session stopped.".into(), ts: now() });
    }
    sync_current_session(&state, Some("stopped"), Some(Some(now())))?;
    record_usage_for_current(&state, "stopped")?;
    save_state(&state)
}

#[tauri::command]
pub async fn send_message(
    state: State<'_, Arc<AppState>>,
    message: String,
    images: Option<Vec<ImageAttachment>>,
) -> Result<(), String> {
    let session_id = state.current_session_id.lock().unwrap().clone()
        .ok_or_else(|| "No active session".to_string())?;

    let cfg = state.config.lock().unwrap().clone();

    // Add user message to history
    state.messages.lock().unwrap().push(ChatMessage::User {
        content: message.clone(),
        ts: now(),
    });

    // Send to bridge — deprecated zero mode, falls back to orchestration
    let tx_opt = state.cmd_tx.lock().unwrap().clone();
    if let Some(tx) = tx_opt {
        let _ = tx.send(BridgeCommand::ContinueZeroMode {
            session_id,
            message,
            config: cfg,
            workspace: serde_json::Value::Null,
            images: images.unwrap_or_default(),
        }).await;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_current_session(state: State<'_, Arc<AppState>>) -> Result<Option<SessionDetail>, String> {
    let session_id = state.current_session_id.lock().unwrap().clone();
    let Some(session_id) = session_id else {
        return Ok(None);
    };
    let sessions = state.sessions.lock().unwrap();
    Ok(sessions.get(&session_id).cloned().map(SessionDetail::from))
}

#[tauri::command]
pub async fn load_session(state: State<'_, Arc<AppState>>, session_id: String) -> Result<SessionDetail, String> {
    let record = {
        let sessions = state.sessions.lock().unwrap();
        sessions
            .get(&session_id)
            .cloned()
            .ok_or_else(|| format!("session not found: {session_id}"))?
    };

    *state.current_session_id.lock().unwrap() = Some(session_id);
    *state.messages.lock().unwrap() = record.messages.clone();
    *state.tasks.lock().unwrap() = record
        .tasks
        .iter()
        .cloned()
        .map(|task| (task.id.clone(), task))
        .collect();
    *state.tokens_total.lock().unwrap() = record.tokens_total;
    *state.tool_calls_total.lock().unwrap() = record.tool_calls_total;
    save_state(&state)?;

    Ok(SessionDetail::from(record))
}

#[tauri::command]
pub async fn clear_current_session(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    *state.current_session_id.lock().unwrap() = None;
    state.messages.lock().unwrap().clear();
    state.tasks.lock().unwrap().clear();
    *state.tokens_total.lock().unwrap() = 0;
    *state.tool_calls_total.lock().unwrap() = 0;
    save_state(&state)
}

/// Return the zero-mode workspace — deprecated, returns null.
#[tauri::command]
pub async fn get_zero_workspace() -> Result<Option<serde_json::Value>, String> {
    Ok(None)
}

/// Return the base directory where all zero-mode workspaces are created — deprecated.
#[tauri::command]
pub async fn get_zero_workspace_base() -> Result<String, String> {
    Ok(String::new())
}

/// Return the active sandbox for the current session, or null if none.
#[tauri::command]
pub async fn get_sandbox_status(state: State<'_, Arc<AppState>>) -> Result<Option<SandboxInfo>, String> {
    Ok(state.active_sandbox.lock().unwrap().clone())
}

/// Discard the active sandbox immediately (removes the upper_dir from disk).
#[tauri::command]
pub async fn discard_sandbox(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let sb = state.active_sandbox.lock().unwrap().take();
    if let Some(info) = sb {
        sandbox::destroy_sandbox(&info.id).map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// Return the size in bytes of the sandbox upper_dir (0 when no sandbox is active).
#[tauri::command]
pub async fn get_sandbox_size(state: State<'_, Arc<AppState>>) -> Result<u64, String> {
    let guard = state.active_sandbox.lock().unwrap();
    Ok(guard.as_ref().map(sandbox::upper_dir_size).unwrap_or(0))
}

#[tauri::command]
pub async fn window_control(app: tauri::AppHandle, action: String) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;

    match action.as_str() {
        "close" => window.close().map_err(|err| format!("close window: {err}")),
        "minimize" => window.minimize().map_err(|err| format!("minimize window: {err}")),
        "toggle_maximize" => {
            let is_maximized = window
                .is_maximized()
                .map_err(|err| format!("read maximize state: {err}"))?;
            if is_maximized {
                window.unmaximize().map_err(|err| format!("unmaximize window: {err}"))
            } else {
                window.maximize().map_err(|err| format!("maximize window: {err}"))
            }
        }
        other => Err(format!("unknown window action: {other}")),
    }
}

#[tauri::command]
pub async fn get_connection_state(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let cs = state.bridge.state_rx.borrow().clone();
    Ok(serde_json::json!({
        "connected": cs.connected,
        "launcher":  cs.launcher,
        "addr":      cs.addr,
    }))
}

#[tauri::command]
pub async fn get_engine_logs(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    Ok(state.bridge.read_logs().await)
}

#[tauri::command]
pub async fn get_system_info() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "os":      std::env::consts::OS,
        "arch":    std::env::consts::ARCH,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[tauri::command]
pub async fn get_stats(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let tasks = state.tasks.lock().unwrap();
    let running  = tasks.values().filter(|t| t.status == TaskStatus::Running).count();
    let done     = tasks.values().filter(|t| t.status == TaskStatus::Complete).count();
    let failed   = tasks.values().filter(|t| t.status == TaskStatus::Failed).count();
    let total    = tasks.len();
    drop(tasks);

    let usage_history = state.usage_history.lock().unwrap().clone();
    let lifetime_tokens: u64 = usage_history.iter().map(|item| item.tokens).sum();
    let lifetime_tool_calls: u64 = usage_history.iter().map(|item| item.tool_calls).sum();
    let lifetime_tasks: usize = usage_history.iter().map(|item| item.task_count).sum();
    let lifetime_duration_secs: u64 = usage_history.iter().map(|item| item.duration_secs).sum();
    let current_tokens = *state.tokens_total.lock().unwrap();
    let current_tool_calls = *state.tool_calls_total.lock().unwrap();
    let current_is_running = {
        let current_id = state.current_session_id.lock().unwrap().clone();
        let sessions = state.sessions.lock().unwrap();
        current_id
            .and_then(|id| sessions.get(&id).map(|record| record.summary.status == "running"))
            .unwrap_or(false)
    };
    let active_tokens = if current_is_running { current_tokens } else { 0 };
    let active_tool_calls = if current_is_running { current_tool_calls } else { 0 };
    let active_tasks = if current_is_running { done } else { 0 };

    Ok(serde_json::json!({
        "tasks_total":   total,
        "tasks_running": running,
        "tasks_done":    done,
        "tasks_failed":  failed,
        "tokens_total":  current_tokens,
        "tool_calls":    current_tool_calls,
        "lifetime_tokens": lifetime_tokens + active_tokens,
        "lifetime_tool_calls": lifetime_tool_calls + active_tool_calls,
        "lifetime_tasks": lifetime_tasks + active_tasks,
        "lifetime_duration_secs": lifetime_duration_secs,
        "daemon_connected": *state.daemon_connected.lock().unwrap(),
    }))
}

// ── Developer Mode Commands ───────────────────────────────────────────────────

#[tauri::command]
pub async fn get_developer_config() -> Result<platform::developer_mode::DeveloperConfig, String> {
    Ok(platform::developer_mode::DeveloperConfig::load_or_default())
}

#[tauri::command]
pub async fn save_developer_config(config: platform::developer_mode::DeveloperConfig) -> Result<(), String> {
    config.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn validate_developer_config(config: platform::developer_mode::DeveloperConfig) -> Result<Vec<String>, String> {
    Ok(config.validate())
}

#[tauri::command]
pub async fn start_developer_bootstrap(
    config: platform::developer_mode::DeveloperConfig,
) -> Result<String, String> {
    let warnings = config.validate();
    if !warnings.is_empty() {
        return Err(format!(
            "Configuration has issues:\n{}",
            warnings.join("\n")
        ));
    }

    config.save().map_err(|e| format!("Failed to save config: {}", e))?;

    Ok(
        "Configuration saved. The application will restart and automatically bootstrap \
         the QEMU VM. This process takes ~60–90 seconds:\n\n\
         1. QEMU starts with your ISO and disk\n\
         2. Alpine Linux boots and auto-logs in\n\
         3. Network is activated (DHCP)\n\
         4. Persistent disk is mounted\n\
         5. socat is installed\n\
         6. agentd starts and bridges to TCP port\n\n\
         The app will reconnect automatically once the VM is ready."
            .to_string(),
    )
}

#[tauri::command]
pub async fn clear_developer_config() -> Result<(), String> {
    let path = platform::developer_mode::DeveloperConfig::config_file_path();
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("Failed to remove config: {}", e))?;
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Agent commands — talk to mowis-agent via HTTP
// ─────────────────────────────────────────────────────────────────────────────

fn get_agent_client(state: &State<'_, Arc<AppState>>) -> Result<agent_client::AgentClient, String> {
    let mgr = state.agent_manager.lock().map_err(|e| {
        log::error!("[cmd] Failed to lock agent_manager mutex: {}", e);
        e.to_string()
    })?;
    let mgr = mgr.as_ref().ok_or_else(|| {
        log::warn!("[cmd] Agent not initialized — agent_manager is None");
        "Agent not initialized".to_string()
    })?;
    Ok(mgr.client().clone())
}

#[tauri::command]
pub async fn agent_health(state: State<'_, Arc<AppState>>) -> Result<agent_client::HealthResponse, String> {
    let client = get_agent_client(&state)?;
    match client.health().await {
        Ok(resp) => {
            log::info!("[cmd] agent_health OK: v{}, healthy={}", resp.version, resp.healthy);
            Ok(resp)
        }
        Err(e) => {
            log::warn!("[cmd] agent_health FAILED: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn agent_create_session(
    state: State<'_, Arc<AppState>>,
    title: String,
) -> Result<agent_client::Session, String> {
    let client = get_agent_client(&state)?;
    log::info!("[cmd] Creating agent session: {}", title);
    match client.create_session(&title).await {
        Ok(sess) => {
            log::info!("[cmd] Session created: {}", sess.id);
            Ok(sess)
        }
        Err(e) => {
            log::error!("[cmd] Create session failed: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn agent_list_sessions(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<agent_client::Session>, String> {
    let client = get_agent_client(&state)?;
    client.list_sessions().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_send_message(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    text: String,
    background: Option<bool>,
) -> Result<serde_json::Value, String> {
    let client = get_agent_client(&state)?;
    let preview = if text.len() > 80 { &text[..80] } else { &text };
    log::info!("[cmd] Sending message to session {}: \"{}\" (async={})", session_id, preview, background.unwrap_or(false));
    if background.unwrap_or(false) {
        client.send_message_async(&session_id, &text).await.map_err(|e| {
            log::error!("[cmd] send_message_async failed: {}", e);
            e.to_string()
        })?;
        Ok(serde_json::json!({ "status": "accepted" }))
    } else {
        let result = client.send_message(&session_id, &text).await.map_err(|e| {
            log::error!("[cmd] send_message failed: {}", e);
            e.to_string()
        })?;
        log::info!("[cmd] Message sent, response received");
        Ok(result)
    }
}

#[tauri::command]
pub async fn agent_abort(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<(), String> {
    let client = get_agent_client(&state)?;
    client.abort(&session_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_approve_permission(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    permission_id: String,
) -> Result<(), String> {
    let client = get_agent_client(&state)?;
    client.approve_permission(&session_id, &permission_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_deny_permission(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    permission_id: String,
) -> Result<(), String> {
    let client = get_agent_client(&state)?;
    client.deny_permission(&session_id, &permission_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_delete_session(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<(), String> {
    let client = get_agent_client(&state)?;
    client.delete_session(&session_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_list_messages(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<Vec<agent_client::AgentMessage>, String> {
    let client = get_agent_client(&state)?;
    client.list_messages(&session_id).await.map_err(|e| e.to_string())
}

/// Explicitly start (or connect to) the mowis-agent subprocess.
/// Emits `agent_startup_log` events with `{ text, level }` as it progresses
/// so the frontend can display live logs in the startup modal.
#[tauri::command]
pub async fn agent_start(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    // Release lock before any await — sync Mutex must not be held across await points.
    let maybe_client = {
        let lock = state.agent_manager.lock().map_err(|e| e.to_string())?;
        lock.as_ref().map(|m| (m.client().clone(), m.port()))
    };

    if let Some((client, port)) = maybe_client {
        if let Ok(health) = client.health().await {
            if health.healthy {
                let _ = app.emit("agent_startup_log", serde_json::json!({
                    "text": format!("Agent already running on port {} (v{})", port, health.version),
                    "level": "success"
                }));
                return Ok(serde_json::json!({ "port": port, "already_running": true }));
            }
        }
    }

    let resource_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_default();

    let _ = app.emit("agent_startup_log", serde_json::json!({
        "text": format!("Searching for mowis-agent in: {}", resource_dir.display()),
        "level": "info"
    }));
    let _ = app.emit("agent_startup_log", serde_json::json!({
        "text": format!(
            "Scanning ports {}–{} for a running instance...",
            crate::agent_manager::DEFAULT_AGENT_PORT,
            crate::agent_manager::DEFAULT_AGENT_PORT + 9
        ),
        "level": "info"
    }));

    let mut mgr = crate::agent_manager::AgentManager::new(crate::agent_manager::DEFAULT_AGENT_PORT);

    match mgr.start(&resource_dir).await {
        Ok(()) => {
            let port = mgr.port();
            let _ = app.emit("agent_startup_log", serde_json::json!({
                "text": format!("mowis-agent ready on port {}", port),
                "level": "success"
            }));
            *state.agent_manager.lock().map_err(|e| e.to_string())? = Some(mgr);
            Ok(serde_json::json!({ "port": port }))
        }
        Err(e) => {
            let msg = format!("{:#}", e);
            let _ = app.emit("agent_startup_log", serde_json::json!({
                "text": msg.clone(),
                "level": "error"
            }));
            Err(msg)
        }
    }
}
