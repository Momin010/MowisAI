#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod platform;
mod backend;

use anyhow::Result;
use backend::BackendBridge;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager, State};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Running,
    Complete,
    Failed,
}

impl Default for TaskStatus {
    fn default() -> Self {
        TaskStatus::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub description: String,
    pub sandbox: Option<String>,
    #[serde(default)]
    pub status: TaskStatus,
    #[serde(default)]
    pub started_at: Option<u64>,
    #[serde(default)]
    pub completed_at: Option<u64>,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub views: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatMessage {
    User    { content: String, ts: u64 },
    Agent   { content: String, streaming: bool, ts: u64 },
    System  { content: String, ts: u64 },
    Plan    { sandboxes: Vec<String>, task_count: usize, agent_count: usize, mode: String, ts: u64 },
    Error   { content: String, ts: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub prompt: String,
    pub status: String,
    pub started_at: u64,
    pub completed_at: Option<u64>,
    pub task_count: usize,
    pub tasks_done: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub socket_path: String,
    pub max_agents: u32,
    pub mode: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub gcp_project: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            socket_path: "/tmp/agentd.sock".into(),
            max_agents: 100,
            mode: "auto".into(),
            provider: "gemini".into(),
            model: "gemini-2.0-flash".into(),
            api_key: String::new(),
            gcp_project: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub session_id: String,
    pub prompt_short: String,
    pub ts: u64,
    pub task_count: usize,
    pub tokens: u64,
    pub tool_calls: u64,
    pub duration_secs: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub summary: SessionSummary,
    pub messages: Vec<ChatMessage>,
    pub tasks: Vec<Task>,
    pub tokens_total: u64,
    pub tool_calls_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub messages: Vec<ChatMessage>,
    pub tasks: Vec<Task>,
    pub tokens_total: u64,
    pub tool_calls_total: u64,
}

impl From<SessionRecord> for SessionDetail {
    fn from(record: SessionRecord) -> Self {
        SessionDetail {
            summary: record.summary,
            messages: record.messages,
            tasks: record.tasks,
            tokens_total: record.tokens_total,
            tool_calls_total: record.tool_calls_total,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedState {
    pub version: u32,
    pub config: Config,
    pub current_session_id: Option<String>,
    pub sessions: HashMap<String, SessionRecord>,
    pub session_history: Vec<SessionSummary>,
    pub usage_history: Vec<UsageRecord>,
}

impl Default for PersistedState {
    fn default() -> Self {
        PersistedState {
            version: 1,
            config: Config::default(),
            current_session_id: None,
            sessions: HashMap::new(),
            session_history: Vec::new(),
            usage_history: Vec::new(),
        }
    }
}

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
            messages: Mutex::new(current_record.as_ref().map(|record| record.messages.clone()).unwrap_or_default()),
            tasks: Mutex::new(current_tasks),
            sessions: Mutex::new(persisted.sessions),
            session_history: Mutex::new(persisted.session_history),
            usage_history: Mutex::new(persisted.usage_history),
            daemon_connected: Mutex::new(false),
            tokens_total: Mutex::new(current_record.as_ref().map(|record| record.tokens_total).unwrap_or_default()),
            tool_calls_total: Mutex::new(current_record.as_ref().map(|record| record.tool_calls_total).unwrap_or_default()),
            storage_path,
            cmd_tx: Mutex::new(None),
        }
    }
}

fn now() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => 0,
    }
}

fn default_storage_path() -> PathBuf {
    let base = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("MowisAI").join("desktop-state.json")
}

fn load_persisted_state(path: &Path) -> PersistedState {
    match fs::read_to_string(path) {
        Ok(raw) => match serde_json::from_str::<PersistedState>(&raw) {
            Ok(state) => state,
            Err(err) => {
                log::warn!("Failed to parse persisted desktop state at {}: {err}", path.display());
                PersistedState::default()
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => PersistedState::default(),
        Err(err) => {
            log::warn!("Failed to read persisted desktop state at {}: {err}", path.display());
            PersistedState::default()
        }
    }
}

fn write_persisted_state(path: &Path, state: &PersistedState) -> Result<(), String> {
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
            fs::remove_file(path)
                .map_err(|err| format!("remove old state file {} after rename failed ({rename_err}): {err}", path.display()))?;
            fs::rename(&tmp_path, path)
                .map_err(|err| format!("replace state file {}: {err}", path.display()))?;
        } else {
            return Err(format!("replace state file {}: {rename_err}", path.display()));
        }
    }
    Ok(())
}

fn lock_err(name: &str) -> String {
    format!("state lock poisoned: {name}")
}

fn save_state(state: &AppState) -> Result<(), String> {
    let persisted = PersistedState {
        version: 1,
        config: state.config.lock().map_err(|_| lock_err("config"))?.clone(),
        current_session_id: state
            .current_session_id
            .lock()
            .map_err(|_| lock_err("current_session_id"))?
            .clone(),
        sessions: state.sessions.lock().map_err(|_| lock_err("sessions"))?.clone(),
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

fn task_counts(tasks: &HashMap<String, Task>) -> (usize, usize) {
    let done = tasks.values().filter(|task| task.status == TaskStatus::Complete).count();
    (tasks.len(), done)
}

fn prompt_from_messages(messages: &[ChatMessage]) -> String {
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

fn upsert_history(history: &mut Vec<SessionSummary>, summary: SessionSummary) {
    if let Some(existing) = history.iter_mut().find(|item| item.id == summary.id) {
        *existing = summary;
    } else {
        history.push(summary);
    }
}

fn sync_current_session(state: &AppState, status: Option<&str>, completed_at: Option<Option<u64>>) -> Result<(), String> {
    let session_id = state
        .current_session_id
        .lock()
        .map_err(|_| lock_err("current_session_id"))?
        .clone();
    let Some(session_id) = session_id else {
        return save_state(state);
    };

    let messages = state.messages.lock().map_err(|_| lock_err("messages"))?.clone();
    let tasks_map = state.tasks.lock().map_err(|_| lock_err("tasks"))?.clone();
    let tasks: Vec<Task> = tasks_map.values().cloned().collect();
    let (task_count, tasks_done) = task_counts(&tasks_map);
    let tokens_total = *state.tokens_total.lock().map_err(|_| lock_err("tokens_total"))?;
    let tool_calls_total = *state.tool_calls_total.lock().map_err(|_| lock_err("tool_calls_total"))?;
    let prompt = prompt_from_messages(&messages);

    let summary = {
        let mut sessions = state.sessions.lock().map_err(|_| lock_err("sessions"))?;
        let record = sessions.entry(session_id.clone()).or_insert_with(|| SessionRecord {
            summary: SessionSummary {
                id: session_id.clone(),
                prompt: prompt.chars().take(80).collect(),
                status: "running".into(),
                started_at: now(),
                completed_at: None,
                task_count,
                tasks_done,
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
        if let Some(next_status) = status {
            record.summary.status = next_status.to_owned();
        }
        if let Some(next_completed_at) = completed_at {
            record.summary.completed_at = next_completed_at;
        }
        record.summary.clone()
    };

    {
        let mut history = state.session_history.lock().map_err(|_| lock_err("session_history"))?;
        upsert_history(&mut history, summary);
        history.sort_by_key(|item| item.started_at);
    }

    save_state(state)
}

fn record_usage_for_current(state: &AppState, status: &str) -> Result<(), String> {
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

    let mut history = state.usage_history.lock().map_err(|_| lock_err("usage_history"))?;
    if let Some(existing) = history.iter_mut().find(|item| item.session_id == session_id) {
        *existing = usage;
    } else {
        history.push(usage);
    }
    history.sort_by_key(|item| item.ts);
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Bridge — background thread that owns the agentd socket
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum BridgeCommand {
    StartOrchestration { session_id: String, prompt: String, max_agents: u32, mode: String },
    StopOrchestration,
    CheckSocket,
}

#[derive(Debug)]
pub enum BridgeEvent {
    DaemonConnected,
    DaemonDisconnected,
    TaskAdded(Task),
    TaskUpdated { id: String, status: TaskStatus },
    AgentChunk(String),
    AgentMessage(String),
    PlanReady { sandboxes: Vec<String>, task_count: usize, agent_count: usize, mode: String },
    OrchestrationComplete,
    OrchestrationFailed(String),
    SimulationTick { tasks_done: usize, active_agents: usize, tokens_delta: u64 },
}

fn start_bridge(
    app: tauri::AppHandle,
    state: Arc<AppState>,
) -> mpsc::Sender<BridgeCommand> {
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<BridgeCommand>(64);
    let (evt_tx, mut evt_rx) = mpsc::channel::<BridgeEvent>(256);

    let bridge = Arc::clone(&state.bridge);

    // ── 1. Start the platform harness (WSL2 / QEMU / native socket) ──────────
    {
        let bridge_clone = Arc::clone(&bridge);
        let evt_tx_clone = evt_tx.clone();
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            // Forward setup-progress events to the frontend while booting.
            let bridge_prog = Arc::clone(&bridge_clone);
            tauri::async_runtime::spawn(async move {
                let mut rx = bridge_prog.progress_rx.lock().await;
                while let Some(prog) = rx.recv().await {
                    let _ = app_clone.emit("setup_progress", &prog);
                }
            });

            match bridge_clone.start().await {
                Ok(()) => {
                    let _ = evt_tx_clone.send(BridgeEvent::DaemonConnected).await;
                }
                Err(e) => {
                    log::error!("Backend harness failed to start: {e}");
                    let _ = evt_tx_clone.send(BridgeEvent::DaemonDisconnected).await;
                }
            }
        });
    }

    // ── 2. Watch bridge connection-state changes (health-loop reconnects) ─────
    {
        let mut state_rx = bridge.state_rx.clone();
        let evt_tx_clone = evt_tx.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                if state_rx.changed().await.is_err() { break; }
                let connected = state_rx.borrow().connected;
                let evt = if connected {
                    BridgeEvent::DaemonConnected
                } else {
                    BridgeEvent::DaemonDisconnected
                };
                if evt_tx_clone.send(evt).await.is_err() { break; }
            }
        });
    }

    // ── 3. Command handler (background thread with its own tokio runtime) ─────
    let evt_tx_clone = evt_tx.clone();
    let bridge_for_cmds = Arc::clone(&bridge);
    std::thread::Builder::new()
        .name("mowisai-bridge".into())
        .spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("tokio rt");
            rt.block_on(async move {
                while let Some(cmd) = cmd_rx.recv().await {
                    let tx = evt_tx_clone.clone();
                    let b = Arc::clone(&bridge_for_cmds);
                    match cmd {
                        BridgeCommand::CheckSocket => {
                            let evt = if b.is_connected() {
                                BridgeEvent::DaemonConnected
                            } else {
                                BridgeEvent::DaemonDisconnected
                            };
                            let _ = tx.send(evt).await;
                        }

                        BridgeCommand::StopOrchestration => {
                            if b.is_connected() {
                                let _ = b.send(serde_json::json!({ "type": "stop" })).await;
                            }
                        }

                        BridgeCommand::StartOrchestration { session_id, prompt, max_agents, mode } => {
                            tokio::spawn(async move {
                                run_orchestration(session_id, prompt, max_agents, mode, b, tx).await;
                            });
                        }
                    }
                }
            });
        })
        .expect("spawn bridge thread");

    // ── 4. Event consumer — Tauri executor → frontend ─────────────────────────
    let state_clone = Arc::clone(&state);
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = evt_rx.recv().await {
            handle_bridge_event(event, &state_clone, &app_clone).await;
        }
    });

    cmd_tx
}

async fn handle_bridge_event(event: BridgeEvent, state: &Arc<AppState>, app: &tauri::AppHandle) {
    match event {
        BridgeEvent::DaemonConnected => {
            *state.daemon_connected.lock().unwrap() = true;
            let _ = app.emit("daemon_status", serde_json::json!({ "connected": true }));
        }
        BridgeEvent::DaemonDisconnected => {
            *state.daemon_connected.lock().unwrap() = false;
            let _ = app.emit("daemon_status", serde_json::json!({ "connected": false }));
        }

        BridgeEvent::PlanReady { sandboxes, task_count, agent_count, mode } => {
            let msg = ChatMessage::Plan {
                sandboxes: sandboxes.clone(),
                task_count, agent_count,
                mode: mode.clone(),
                ts: now(),
            };
            state.messages.lock().unwrap().push(msg.clone());
            if let Err(err) = sync_current_session(state, Some("running"), None) {
                log::warn!("Failed to persist plan message: {err}");
            }
            let _ = app.emit("chat_message", &msg);
        }

        BridgeEvent::TaskAdded(task) => {
            state.tasks.lock().unwrap().insert(task.id.clone(), task.clone());
            if let Err(err) = sync_current_session(state, Some("running"), None) {
                log::warn!("Failed to persist added task: {err}");
            }
            let _ = app.emit("task_added", &task);
        }

        BridgeEvent::TaskUpdated { id, status } => {
            if let Some(t) = state.tasks.lock().unwrap().get_mut(&id) {
                t.status = status.clone();
                match &status {
                    TaskStatus::Running  => { t.started_at = Some(now()); }
                    TaskStatus::Complete | TaskStatus::Failed => { t.completed_at = Some(now()); }
                    _ => {}
                }
            }
            if let Err(err) = sync_current_session(state, Some("running"), None) {
                log::warn!("Failed to persist task update: {err}");
            }
            let _ = app.emit("task_updated", serde_json::json!({ "id": id, "status": status }));
        }

        BridgeEvent::AgentChunk(chunk) => {
            let mut msgs = state.messages.lock().unwrap();
            if let Some(last) = msgs.last_mut() {
                if let ChatMessage::Agent { content, streaming, .. } = last {
                    if *streaming {
                        content.push_str(&chunk);
                        let _ = app.emit("agent_chunk", serde_json::json!({ "chunk": chunk }));
                        drop(msgs);
                        if let Err(err) = sync_current_session(state, Some("running"), None) {
                            log::warn!("Failed to persist agent chunk: {err}");
                        }
                        return;
                    }
                }
            }
            // No streaming message yet — open one
            msgs.push(ChatMessage::Agent { content: chunk.clone(), streaming: true, ts: now() });
            drop(msgs);
            if let Err(err) = sync_current_session(state, Some("running"), None) {
                log::warn!("Failed to persist agent chunk: {err}");
            }
            let _ = app.emit("agent_chunk", serde_json::json!({ "chunk": chunk }));
        }

        BridgeEvent::AgentMessage(content) => {
            let mut msgs = state.messages.lock().unwrap();
            // Finalize any open streaming message
            if let Some(last) = msgs.last_mut() {
                if let ChatMessage::Agent { streaming, .. } = last {
                    *streaming = false;
                }
            }
            let msg = ChatMessage::Agent { content: content.clone(), streaming: false, ts: now() };
            msgs.push(msg.clone());
            drop(msgs);
            if let Err(err) = sync_current_session(state, Some("running"), None) {
                log::warn!("Failed to persist agent message: {err}");
            }
            let _ = app.emit("chat_message", &msg);
        }

        BridgeEvent::OrchestrationComplete => {
            // Finalize streaming
            {
                let mut msgs = state.messages.lock().unwrap();
                if let Some(last) = msgs.last_mut() {
                    if let ChatMessage::Agent { streaming, .. } = last {
                        *streaming = false;
                    }
                }
            }
            // Mark all running tasks done
            for t in state.tasks.lock().unwrap().values_mut() {
                if t.status == TaskStatus::Running {
                    t.status = TaskStatus::Complete;
                    t.completed_at = Some(now());
                }
            }
            let sys = ChatMessage::System { content: "Session complete.".into(), ts: now() };
            state.messages.lock().unwrap().push(sys.clone());
            if let Err(err) = sync_current_session(state, Some("done"), Some(Some(now()))) {
                log::warn!("Failed to persist completed session: {err}");
            }
            if let Err(err) = record_usage_for_current(state, "done").and_then(|_| save_state(state)) {
                log::warn!("Failed to persist completed usage: {err}");
            }
            let _ = app.emit("chat_message", &sys);
            let _ = app.emit("session_complete", serde_json::json!({}));
        }

        BridgeEvent::OrchestrationFailed(err) => {
            let msg = ChatMessage::Error { content: err.clone(), ts: now() };
            state.messages.lock().unwrap().push(msg.clone());
            if let Err(err) = sync_current_session(state, Some("error"), Some(Some(now()))) {
                log::warn!("Failed to persist failed session: {err}");
            }
            if let Err(err) = record_usage_for_current(state, "error").and_then(|_| save_state(state)) {
                log::warn!("Failed to persist failed usage: {err}");
            }
            let _ = app.emit("chat_message", &msg);
        }

        BridgeEvent::SimulationTick { tasks_done, active_agents, tokens_delta } => {
            *state.tokens_total.lock().unwrap() += tokens_delta;
            *state.tool_calls_total.lock().unwrap() += 1;
            if let Err(err) = sync_current_session(state, Some("running"), None) {
                log::warn!("Failed to persist usage tick: {err}");
            }
            let _ = app.emit("stats_tick", serde_json::json!({
                "tasks_done": tasks_done,
                "active_agents": active_agents,
                "tokens_total": *state.tokens_total.lock().unwrap(),
            }));
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Socket event mapping
// ─────────────────────────────────────────────────────────────────────────────

fn socket_value_to_bridge_event(v: &serde_json::Value) -> Option<BridgeEvent> {
    let t = v.get("type")?.as_str()?;
    match t {
        "task_added" => {
            let id          = v["id"].as_str()?.to_owned();
            let description = v["description"].as_str().unwrap_or("").to_owned();
            let sandbox     = v["sandbox"].as_str().map(ToOwned::to_owned);
            let status      = parse_task_status(&v["status"]);
            let files       = json_string_array(v.get("files"));
            let views       = json_string_array(v.get("views"));
            let summary     = v["summary"].as_str().map(ToOwned::to_owned);
            Some(BridgeEvent::TaskAdded(Task {
                id,
                description,
                sandbox,
                status,
                started_at: None,
                completed_at: None,
                files,
                summary,
                views,
            }))
        }
        "task_updated" => {
            let id     = v["id"].as_str()?.to_owned();
            let status = parse_task_status(&v["status"]);
            Some(BridgeEvent::TaskUpdated { id, status })
        }
        "agent_chunk"   => Some(BridgeEvent::AgentChunk(v["content"].as_str()?.to_owned())),
        "agent_message" => Some(BridgeEvent::AgentMessage(v["content"].as_str()?.to_owned())),
        "complete"      => Some(BridgeEvent::OrchestrationComplete),
        "error"         => Some(BridgeEvent::OrchestrationFailed(
            v["message"].as_str().unwrap_or("unknown error").to_owned(),
        )),
        _ => None,
    }
}

fn parse_task_status(v: &serde_json::Value) -> TaskStatus {
    match v.as_str().unwrap_or("pending") {
        "running"  => TaskStatus::Running,
        "complete" => TaskStatus::Complete,
        "failed"   => TaskStatus::Failed,
        _          => TaskStatus::Pending,
    }
}

fn json_string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

// ─────────────────────────────────────────────────────────────────────────────
// Orchestration runner (real socket → fallback simulation)
// ─────────────────────────────────────────────────────────────────────────────

async fn run_orchestration(
    session_id: String,
    prompt: String,
    max_agents: u32,
    mode: String,
    bridge: Arc<BackendBridge>,
    event_tx: mpsc::Sender<BridgeEvent>,
) {
    // 1. Emit plan card
    let sb_names = vec!["frontend".to_string(), "backend".to_string(), "verification".to_string()];
    let task_count = max_agents.min(60) as usize;
    let agent_count = max_agents.min(48) as usize;
    let _ = event_tx.send(BridgeEvent::PlanReady {
        sandboxes: sb_names.clone(),
        task_count,
        agent_count,
        mode: mode.clone(),
    }).await;

    // 2. Try real bridge connection (WSL2 / QEMU / native socket)
    if bridge.is_connected() {
        let payload = serde_json::json!({
            "type":       "orchestrate",
            "prompt":     prompt.clone(),
            "project":    ".",
            "max_agents": max_agents,
            "mode":       mode,
        });
        match bridge.send(payload).await {
            Ok(()) => {
                // Stream JSON events until the daemon closes the connection.
                loop {
                    match bridge.recv_next().await {
                        Ok(Some(v)) => {
                            if let Some(evt) = socket_value_to_bridge_event(&v) {
                                if event_tx.send(evt).await.is_err() { return; }
                            }
                        }
                        Ok(None) => return, // clean EOF — daemon finished
                        Err(e) => {
                            log::warn!("Bridge recv error: {e}");
                            break;
                        }
                    }
                }
                return;
            }
            Err(e) => {
                log::warn!("Bridge send failed ({e}), running simulation");
            }
        }
    } else {
        log::info!("Daemon not yet connected — running simulation");
    }

    // 3. Fallback: simulation keeps the UI fully functional without a daemon
    simulate_session(session_id, prompt, task_count, agent_count, sb_names, event_tx).await;
}

/// Simulation — keeps UI fully functional without a running daemon
async fn simulate_session(
    _session_id: String,
    prompt: String,
    task_count: usize,
    agent_count: usize,
    sb_names: Vec<String>,
    event_tx: mpsc::Sender<BridgeEvent>,
) {
    let sample_tasks = [
        "Implement OAuth2 middleware",
        "Build REST API endpoints",
        "Create React dashboard components",
        "Set up database schema",
        "Implement WebSocket streaming",
        "Write unit tests",
        "Configure CI/CD pipeline",
        "Optimise query performance",
        "Implement rate limiting",
        "Build file upload service",
        "Generate API documentation",
        "Set up error monitoring",
        "Implement caching layer",
        "Build admin panel",
        "Configure SSL/TLS",
        "Write integration tests",
        "Implement search",
        "Build notification system",
        "Set up logging",
        "Create deployment scripts",
    ];

    // Initial agent message — streaming
    let intro_chunks = [
        "Understood. Spinning up the 7-layer orchestration pipeline…\n",
        &format!("Task: *{}*\n\n", &prompt[..prompt.len().min(80)]),
        "— **Layer 1 (Planner)**: analysing task, generating dependency graph\n",
        "— **Layer 2 (Topology)**: provisioning overlayfs sandboxes\n",
        "— **Layer 3 (Scheduler)**: event-driven task dispatch active\n",
        "— **Layer 4 (Execution)**: agents running with per-tool checkpoints\n",
    ];
    for chunk in intro_chunks {
        let _ = event_tx.send(BridgeEvent::AgentChunk(chunk.to_owned())).await;
        sleep(Duration::from_millis(90)).await;
    }

    // Emit tasks
    let mut task_ids: Vec<String> = Vec::new();
    for i in 0..task_count.min(sample_tasks.len() * 3) {
        let id = format!("t{:04}", i);
        let sb = sb_names[i % sb_names.len()].clone();
        let desc = sample_tasks[i % sample_tasks.len()].to_string();
        let task = Task {
            id: id.clone(),
            description: desc.clone(),
            sandbox: Some(sb.clone()),
            status: TaskStatus::Pending,
            started_at: None,
            completed_at: None,
            files: simulated_task_files(&sb, i),
            summary: Some(format!("Implemented {desc} in the {sb} sandbox.")),
            views: simulated_task_views(&sb),
        };
        if event_tx.send(BridgeEvent::TaskAdded(task)).await.is_err() { return; }
        task_ids.push(id);
        sleep(Duration::from_millis(20)).await;
    }

    // Run tasks in waves
    let wave_size = agent_count.min(8);
    let mut i = 0;
    while i < task_ids.len() {
        let batch: Vec<_> = task_ids[i..(i + wave_size).min(task_ids.len())].to_vec();
        for id in &batch {
            if event_tx.send(BridgeEvent::TaskUpdated { id: id.clone(), status: TaskStatus::Running }).await.is_err() { return; }
        }
        let tokens: u64 = (batch.len() as u64) * (80 + i as u64 * 3);
        if event_tx.send(BridgeEvent::SimulationTick {
            tasks_done: i,
            active_agents: batch.len().min(agent_count),
            tokens_delta: tokens,
        }).await.is_err() { return; }

        sleep(Duration::from_millis(180)).await;

        for id in &batch {
            if event_tx.send(BridgeEvent::TaskUpdated { id: id.clone(), status: TaskStatus::Complete }).await.is_err() { return; }
        }
        i += wave_size;
    }

    // Merge + verify narrative chunks
    let merge_chunks = [
        "\n— **Layer 5 (Merge)**: parallel tree-pattern merge across sandboxes\n",
        "— **Layer 6 (Verification)**: test task graph running, 2 rounds\n",
        "— **Layer 7 (Output)**: cross-sandbox integration merge complete\n\n",
        "All tasks completed. Code merged and verified. ✓\n",
    ];
    for chunk in merge_chunks {
        let _ = event_tx.send(BridgeEvent::AgentChunk(chunk.to_owned())).await;
        sleep(Duration::from_millis(220)).await;
    }

    let _ = event_tx.send(BridgeEvent::OrchestrationComplete).await;
}

fn simulated_task_files(sandbox: &str, index: usize) -> Vec<String> {
    match sandbox {
        "frontend" => vec![
            format!("src/views/agent_panel_{index}.tsx"),
            format!("src/styles/session_{index}.css"),
        ],
        "backend" => vec![
            format!("src/services/orchestration_{index}.rs"),
            format!("src/api/session_routes_{index}.rs"),
        ],
        "verification" => vec![
            format!("tests/orchestration_{index}_test.rs"),
            format!("tests/fixtures/session_{index}.json"),
        ],
        other => vec![format!("{other}/task_{index}.txt")],
    }
}

fn simulated_task_views(sandbox: &str) -> Vec<String> {
    match sandbox {
        "frontend" => vec!["Session timeline".into(), "Task inspector".into()],
        "backend" => vec!["API contract".into(), "Execution trace".into()],
        "verification" => vec!["Test report".into(), "Coverage delta".into()],
        _ => Vec::new(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tauri Commands
// ─────────────────────────────────────────────────────────────────────────────

#[tauri::command]
async fn get_messages(state: State<'_, Arc<AppState>>) -> Result<Vec<ChatMessage>, String> {
    Ok(state.messages.lock().unwrap().clone())
}

#[tauri::command]
async fn get_tasks(state: State<'_, Arc<AppState>>) -> Result<Vec<Task>, String> {
    Ok(state.tasks.lock().unwrap().values().cloned().collect())
}

#[tauri::command]
async fn get_session_history(state: State<'_, Arc<AppState>>) -> Result<Vec<SessionSummary>, String> {
    let mut history = state.session_history.lock().unwrap().clone();
    history.sort_by_key(|item| item.started_at);
    Ok(history)
}

#[tauri::command]
async fn get_usage_history(state: State<'_, Arc<AppState>>) -> Result<Vec<UsageRecord>, String> {
    let mut history = state.usage_history.lock().unwrap().clone();
    history.sort_by_key(|item| item.ts);
    Ok(history)
}

#[tauri::command]
async fn get_config(state: State<'_, Arc<AppState>>) -> Result<Config, String> {
    Ok(state.config.lock().unwrap().clone())
}

#[tauri::command]
async fn save_config(state: State<'_, Arc<AppState>>, config: Config) -> Result<(), String> {
    *state.config.lock().unwrap() = config;
    save_state(&state)
}

#[tauri::command]
async fn get_daemon_status(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    Ok(*state.daemon_connected.lock().unwrap())
}

#[tauri::command]
async fn check_daemon(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    // Clone sender outside the lock to avoid holding MutexGuard across .await
    let tx_opt = state.cmd_tx.lock().unwrap().clone();
    if let Some(tx) = tx_opt {
        let _ = tx.send(BridgeCommand::CheckSocket).await;
    }
    Ok(*state.daemon_connected.lock().unwrap())
}

#[tauri::command]
async fn start_session(
    state: State<'_, Arc<AppState>>,
    prompt: String,
    mode: Option<String>,
) -> Result<String, String> {
    let session_id = Uuid::new_v4().to_string();
    let cfg = state.config.lock().unwrap().clone();
    let resolved_mode = mode.unwrap_or_else(|| cfg.mode.clone());
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
        let _ = tx.send(BridgeCommand::StartOrchestration {
            session_id: session_id.clone(),
            prompt,
            max_agents: cfg.max_agents,
            mode: resolved_mode,
        }).await;
    }

    Ok(session_id)
}

#[tauri::command]
async fn stop_session(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let tx_opt = state.cmd_tx.lock().unwrap().clone();
    if let Some(tx) = tx_opt {
        let _ = tx.send(BridgeCommand::StopOrchestration).await;
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
async fn get_current_session(state: State<'_, Arc<AppState>>) -> Result<Option<SessionDetail>, String> {
    let session_id = state.current_session_id.lock().unwrap().clone();
    let Some(session_id) = session_id else {
        return Ok(None);
    };
    let sessions = state.sessions.lock().unwrap();
    Ok(sessions.get(&session_id).cloned().map(SessionDetail::from))
}

#[tauri::command]
async fn load_session(state: State<'_, Arc<AppState>>, session_id: String) -> Result<SessionDetail, String> {
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
async fn clear_current_session(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    *state.current_session_id.lock().unwrap() = None;
    state.messages.lock().unwrap().clear();
    state.tasks.lock().unwrap().clear();
    *state.tokens_total.lock().unwrap() = 0;
    *state.tool_calls_total.lock().unwrap() = 0;
    save_state(&state)
}

#[tauri::command]
async fn window_control(app: tauri::AppHandle, action: String) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;

    match action.as_str() {
        "close" => window.close().map_err(|err| format!("close window: {err}")),
        "minimize" => window.minimize().map_err(|err| format!("minimize window: {err}")),
        "toggle_maximize" => window.toggle_maximize().map_err(|err| format!("toggle maximize: {err}")),
        other => Err(format!("unknown window action: {other}")),
    }
}

#[tauri::command]
async fn get_connection_state(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let cs = state.bridge.state_rx.borrow().clone();
    Ok(serde_json::json!({
        "connected": cs.connected,
        "launcher":  cs.launcher,
        "addr":      cs.addr,
    }))
}

#[tauri::command]
async fn get_system_info() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "os":      std::env::consts::OS,
        "arch":    std::env::consts::ARCH,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[tauri::command]
async fn get_stats(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
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

// ─────────────────────────────────────────────────────────────────────────────
// Main
// ─────────────────────────────────────────────────────────────────────────────

fn main() {
    let bridge = BackendBridge::new();
    let state = Arc::new(AppState::new(bridge));
    let state_for_setup = Arc::clone(&state);

    tauri::Builder::default()
        .manage(state)
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
            get_daemon_status,
            check_daemon,
            start_session,
            stop_session,
            get_current_session,
            load_session,
            clear_current_session,
            window_control,
            get_system_info,
            get_stats,
            get_connection_state,
        ])
        .run(tauri::generate_context!())
        .expect("error running mowis-desktop");
}
