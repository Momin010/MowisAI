use crate::backend::BackendBridge;
use crate::sandbox::SandboxInfo;
use crate::types::*;
use crate::zero_mode;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

// ─────────────────────────────────────────────────────────────────────────────
// App State
// ─────────────────────────────────────────────────────────────────────────────

pub struct AppState {
    pub bridge: Arc<BackendBridge>,
    pub config: Mutex<Config>,
    pub current_session_id: Mutex<Option<String>>,
    pub messages: Mutex<Vec<ChatMessage>>,
    pub tasks: Mutex<HashMap<String, Task>>,
    pub sessions: Mutex<HashMap<String, SessionRecord>>,
    pub session_history: Mutex<Vec<SessionSummary>>,
    pub usage_history: Mutex<Vec<UsageRecord>>,
    pub daemon_connected: Mutex<bool>,
    pub tokens_total: Mutex<u64>,
    pub tool_calls_total: Mutex<u64>,
    pub storage_path: PathBuf,

    // Channel to send commands to the background bridge
    pub cmd_tx: Mutex<Option<mpsc::Sender<BridgeCommand>>>,

    // Active soft sandbox for the current session (None when sandbox is off or no session).
    pub active_sandbox: Mutex<Option<SandboxInfo>>,

    // Workspace created by zero mode (None when not in zero mode).
    pub zero_workspace: Mutex<Option<zero_mode::ZeroWorkspaceInfo>>,
}

impl AppState {
    pub fn new(bridge: Arc<BackendBridge>) -> Self {
        let storage_path = default_storage_path();
        let persisted = load_persisted_state(&storage_path);
        let current_session_id = persisted.current_session_id.clone();
        let current_record = current_session_id
            .as_ref()
            .and_then(|id| persisted.sessions.get(id))
            .cloned();
        let current_tasks = current_record
            .as_ref()
            .map(|record| {
                record
                    .tasks
                    .iter()
                    .cloned()
                    .map(|task| (task.id.clone(), task))
                    .collect()
            })
            .unwrap_or_default();

        AppState {
            bridge,
            config: Mutex::new(persisted.config),
            current_session_id: Mutex::new(current_session_id),
            messages: Mutex::new(
                current_record
                    .as_ref()
                    .map(|record| record.messages.clone())
                    .unwrap_or_default(),
            ),
            tasks: Mutex::new(current_tasks),
            sessions: Mutex::new(persisted.sessions),
            session_history: Mutex::new(persisted.session_history),
            usage_history: Mutex::new(persisted.usage_history),
            daemon_connected: Mutex::new(false),
            tokens_total: Mutex::new(
                current_record
                    .as_ref()
                    .map(|record| record.tokens_total)
                    .unwrap_or_default(),
            ),
            tool_calls_total: Mutex::new(
                current_record
                    .as_ref()
                    .map(|record| record.tool_calls_total)
                    .unwrap_or_default(),
            ),
            storage_path,
            cmd_tx: Mutex::new(None),
            active_sandbox: Mutex::new(None),
            zero_workspace: Mutex::new(None),
        }
    }
}

pub fn now() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => 0,
    }
}

pub fn default_storage_path() -> PathBuf {
    let base = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("MowisAI").join("desktop-state.json")
}

pub fn load_persisted_state(path: &Path) -> PersistedState {
    match fs::read_to_string(path) {
        Ok(raw) => match serde_json::from_str::<PersistedState>(&raw) {
            Ok(state) => state,
            Err(err) => {
                log::warn!(
                    "Failed to parse persisted desktop state at {}: {err}",
                    path.display()
                );
                PersistedState::default()
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => PersistedState::default(),
        Err(err) => {
            log::warn!(
                "Failed to read persisted desktop state at {}: {err}",
                path.display()
            );
            PersistedState::default()
        }
    }
}

pub fn write_persisted_state(path: &Path, state: &PersistedState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create state directory {}: {err}", parent.display()))?;
    }

    let encoded = serde_json::to_string_pretty(state)
        .map_err(|err| format!("encode desktop state: {err}"))?;
    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, encoded)
        .map_err(|err| format!("write temporary state file {}: {err}", tmp_path.display()))?;
    if let Err(rename_err) = fs::rename(&tmp_path, path) {
        if path.exists() {
            fs::remove_file(path).map_err(|err| {
                format!(
                    "remove old state file {} after rename failed ({rename_err}): {err}",
                    path.display()
                )
            })?;
            fs::rename(&tmp_path, path)
                .map_err(|err| format!("replace state file {}: {err}", path.display()))?;
        } else {
            return Err(format!(
                "replace state file {}: {rename_err}",
                path.display()
            ));
        }
    }
    Ok(())
}

pub fn lock_err(name: &str) -> String {
    format!("state lock poisoned: {name}")
}

pub fn save_state(state: &AppState) -> Result<(), String> {
    let persisted = PersistedState {
        version: 1,
        config: state.config.lock().map_err(|_| lock_err("config"))?.clone(),
        current_session_id: state
            .current_session_id
            .lock()
            .map_err(|_| lock_err("current_session_id"))?
            .clone(),
        sessions: state
            .sessions
            .lock()
            .map_err(|_| lock_err("sessions"))?
            .clone(),
        session_history: state
            .session_history
            .lock()
            .map_err(|_| lock_err("session_history"))?
            .clone(),
        usage_history: state
            .usage_history
            .lock()
            .map_err(|_| lock_err("usage_history"))?
            .clone(),
    };
    write_persisted_state(&state.storage_path, &persisted)
}

pub fn task_counts(tasks: &HashMap<String, Task>) -> (usize, usize) {
    let done = tasks
        .values()
        .filter(|task| task.status == TaskStatus::Complete)
        .count();
    (tasks.len(), done)
}

pub fn prompt_from_messages(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .find_map(|message| {
            if let ChatMessage::User { content, .. } = message {
                Some(content.clone())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

pub fn upsert_history(history: &mut Vec<SessionSummary>, summary: SessionSummary) {
    if let Some(existing) = history.iter_mut().find(|item| item.id == summary.id) {
        *existing = summary;
    } else {
        history.push(summary);
    }
}

pub fn sync_current_session(
    state: &AppState,
    status: Option<&str>,
    completed_at: Option<Option<u64>>,
) -> Result<(), String> {
    let session_id = state
        .current_session_id
        .lock()
        .map_err(|_| lock_err("current_session_id"))?
        .clone();
    let Some(session_id) = session_id else {
        return save_state(state);
    };

    let messages = state
        .messages
        .lock()
        .map_err(|_| lock_err("messages"))?
        .clone();
    let tasks_map = state.tasks.lock().map_err(|_| lock_err("tasks"))?.clone();
    let tasks: Vec<Task> = tasks_map.values().cloned().collect();
    let (task_count, tasks_done) = task_counts(&tasks_map);
    let tokens_total = *state
        .tokens_total
        .lock()
        .map_err(|_| lock_err("tokens_total"))?;
    let tool_calls_total = *state
        .tool_calls_total
        .lock()
        .map_err(|_| lock_err("tool_calls_total"))?;
    let prompt = prompt_from_messages(&messages);

    let summary = {
        let mut sessions = state.sessions.lock().map_err(|_| lock_err("sessions"))?;
        let record = sessions
            .entry(session_id.clone())
            .or_insert_with(|| SessionRecord {
                summary: SessionSummary {
                    id: session_id.clone(),
                    prompt: prompt.chars().take(80).collect(),
                    status: "running".into(),
                    started_at: now(),
                    completed_at: None,
                    task_count,
                    tasks_done,
                    tokens_total: 0,
                    duration_secs: None,
                    mode: None,
                },
                messages: Vec::new(),
                tasks: Vec::new(),
                tokens_total: 0,
                tool_calls_total: 0,
            });

        record.messages = messages;
        record.tasks = tasks;
        record.tokens_total = tokens_total;
        record.tool_calls_total = tool_calls_total;
        record.summary.prompt = prompt.chars().take(80).collect();
        record.summary.task_count = task_count;
        record.summary.tasks_done = tasks_done;
        record.summary.tokens_total = tokens_total;
        if status.is_some() {
            record.summary.duration_secs = Some(now().saturating_sub(record.summary.started_at));
        }
        if let Some(next_status) = status {
            record.summary.status = next_status.to_owned();
        }
        if let Some(next_completed_at) = completed_at {
            record.summary.completed_at = next_completed_at;
        }
        record.summary.clone()
    };

    {
        let mut history = state
            .session_history
            .lock()
            .map_err(|_| lock_err("session_history"))?;
        upsert_history(&mut history, summary);
        history.sort_by_key(|item| item.started_at);
    }

    save_state(state)
}

pub fn record_usage_for_current(state: &AppState, status: &str) -> Result<(), String> {
    let session_id = state
        .current_session_id
        .lock()
        .map_err(|_| lock_err("current_session_id"))?
        .clone();
    let Some(session_id) = session_id else {
        return Ok(());
    };

    let sessions = state.sessions.lock().map_err(|_| lock_err("sessions"))?;
    let Some(record) = sessions.get(&session_id).cloned() else {
        return Ok(());
    };
    drop(sessions);

    let duration_secs = now().saturating_sub(record.summary.started_at);
    let usage = UsageRecord {
        session_id: session_id.clone(),
        prompt_short: record.summary.prompt.clone(),
        ts: record.summary.completed_at.unwrap_or_else(now),
        task_count: record.summary.task_count,
        tokens: record.tokens_total,
        tool_calls: record.tool_calls_total,
        duration_secs,
        status: status.to_owned(),
    };

    let mut history = state
        .usage_history
        .lock()
        .map_err(|_| lock_err("usage_history"))?;
    if let Some(existing) = history
        .iter_mut()
        .find(|item| item.session_id == session_id)
    {
        *existing = usage;
    } else {
        history.push(usage);
    }
    history.sort_by_key(|item| item.ts);
    Ok(())
}
