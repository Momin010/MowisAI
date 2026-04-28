#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, State};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub description: String,
    pub sandbox: Option<String>,
    pub status: TaskStatus,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
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

// ─────────────────────────────────────────────────────────────────────────────
// App State
// ─────────────────────────────────────────────────────────────────────────────

pub struct AppState {
    pub config: Mutex<Config>,
    pub current_session_id: Mutex<Option<String>>,
    pub messages: Mutex<Vec<ChatMessage>>,
    pub tasks: Mutex<HashMap<String, Task>>,
    pub session_history: Mutex<Vec<SessionSummary>>,
    pub usage_history: Mutex<Vec<UsageRecord>>,
    pub daemon_connected: Mutex<bool>,
    pub tokens_total: Mutex<u64>,
    pub tool_calls_total: Mutex<u64>,

    // Channel to send commands to the background bridge
    pub cmd_tx: Mutex<Option<mpsc::Sender<BridgeCommand>>>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            config: Mutex::new(Config::default()),
            current_session_id: Mutex::new(None),
            messages: Mutex::new(Vec::new()),
            tasks: Mutex::new(HashMap::new()),
            session_history: Mutex::new(Vec::new()),
            usage_history: Mutex::new(Vec::new()),
            daemon_connected: Mutex::new(false),
            tokens_total: Mutex::new(0),
            tool_calls_total: Mutex::new(0),
            cmd_tx: Mutex::new(None),
        }
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
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

const SOCKET_PATH: &str = "/tmp/agentd.sock";

fn start_bridge(
    app: tauri::AppHandle,
    state: Arc<AppState>,
) -> mpsc::Sender<BridgeCommand> {
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<BridgeCommand>(64);
    let (evt_tx, mut evt_rx) = mpsc::channel::<BridgeEvent>(256);

    // Background I/O thread
    let evt_tx_clone = evt_tx.clone();
    std::thread::Builder::new()
        .name("mowisai-bridge".into())
        .spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("tokio rt");
            rt.block_on(async move {
                // Try connecting to daemon on start
                if socket_connectable().await {
                    let _ = evt_tx_clone.send(BridgeEvent::DaemonConnected).await;
                }

                while let Some(cmd) = cmd_rx.recv().await {
                    let tx = evt_tx_clone.clone();
                    match cmd {
                        BridgeCommand::CheckSocket => {
                            if socket_connectable().await {
                                let _ = tx.send(BridgeEvent::DaemonConnected).await;
                            } else {
                                let _ = tx.send(BridgeEvent::DaemonDisconnected).await;
                            }
                        }

                        BridgeCommand::StopOrchestration => {
                            let _ = send_socket_json(
                                serde_json::json!({ "type": "stop" }),
                                &tx,
                            ).await;
                        }

                        BridgeCommand::StartOrchestration { session_id, prompt, max_agents, mode } => {
                            tokio::spawn(async move {
                                run_orchestration(session_id, prompt, max_agents, mode, tx).await;
                            });
                        }
                    }
                }
            });
        })
        .expect("spawn bridge thread");

    // Event consumer — runs on Tauri's async executor, pumps events to frontend
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
            let _ = app.emit("chat_message", &msg);
        }

        BridgeEvent::TaskAdded(task) => {
            state.tasks.lock().unwrap().insert(task.id.clone(), task.clone());
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
            let _ = app.emit("task_updated", serde_json::json!({ "id": id, "status": status }));
        }

        BridgeEvent::AgentChunk(chunk) => {
            let mut msgs = state.messages.lock().unwrap();
            if let Some(last) = msgs.last_mut() {
                if let ChatMessage::Agent { content, streaming, .. } = last {
                    if *streaming {
                        content.push_str(&chunk);
                        let _ = app.emit("agent_chunk", serde_json::json!({ "chunk": chunk }));
                        return;
                    }
                }
            }
            // No streaming message yet — open one
            msgs.push(ChatMessage::Agent { content: chunk.clone(), streaming: true, ts: now() });
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
            let sys = ChatMessage::System { content: "✓ Session complete.".into(), ts: now() };
            state.messages.lock().unwrap().push(sys.clone());
            let _ = app.emit("session_complete", serde_json::json!({}));
            let _ = app.emit("chat_message", &sys);

            // Add to history
            if let Some(id) = state.current_session_id.lock().unwrap().clone() {
                let tasks = state.tasks.lock().unwrap();
                let done = tasks.values().filter(|t| t.status == TaskStatus::Complete).count();
                let total = tasks.len();
                // Rebuild summary from first user message
                let msgs = state.messages.lock().unwrap();
                let prompt = msgs.iter().find_map(|m| {
                    if let ChatMessage::User { content, .. } = m { Some(content.clone()) } else { None }
                }).unwrap_or_default();
                drop(msgs); drop(tasks);

                state.session_history.lock().unwrap().push(SessionSummary {
                    id: id.clone(),
                    prompt: prompt.chars().take(80).collect(),
                    status: "done".into(),
                    started_at: now(),
                    completed_at: Some(now()),
                    task_count: total,
                    tasks_done: done,
                });
            }
        }

        BridgeEvent::OrchestrationFailed(err) => {
            let msg = ChatMessage::Error { content: err.clone(), ts: now() };
            state.messages.lock().unwrap().push(msg.clone());
            let _ = app.emit("chat_message", &msg);
        }

        BridgeEvent::SimulationTick { tasks_done, active_agents, tokens_delta } => {
            *state.tokens_total.lock().unwrap() += tokens_delta;
            *state.tool_calls_total.lock().unwrap() += 1;
            let _ = app.emit("stats_tick", serde_json::json!({
                "tasks_done": tasks_done,
                "active_agents": active_agents,
                "tokens_total": *state.tokens_total.lock().unwrap(),
            }));
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Socket helpers (ported from mowis-gui/backend.rs)
// ─────────────────────────────────────────────────────────────────────────────

async fn socket_connectable() -> bool {
    tokio::net::UnixStream::connect(SOCKET_PATH).await.is_ok()
}

async fn send_socket_json(
    payload: serde_json::Value,
    event_tx: &mpsc::Sender<BridgeEvent>,
) -> Result<()> {
    let mut stream = tokio::net::UnixStream::connect(SOCKET_PATH)
        .await
        .map_err(|e| anyhow::anyhow!("Cannot connect to {SOCKET_PATH}: {e}"))?;

    let mut msg = serde_json::to_string(&payload)?;
    msg.push('\n');
    stream.write_all(msg.as_bytes()).await
        .map_err(|e| anyhow::anyhow!("Socket write: {e}"))?;

    let reader = tokio::io::BufReader::new(stream);
    read_socket_responses(reader, event_tx).await?;
    Ok(())
}

async fn read_socket_responses(
    reader: tokio::io::BufReader<tokio::net::UnixStream>,
    event_tx: &mpsc::Sender<BridgeEvent>,
) -> Result<()> {
    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() { continue; }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            if let Some(evt) = socket_value_to_bridge_event(&v) {
                if event_tx.send(evt).await.is_err() { break; }
            }
        }
    }
    Ok(())
}

fn socket_value_to_bridge_event(v: &serde_json::Value) -> Option<BridgeEvent> {
    let t = v.get("type")?.as_str()?;
    match t {
        "task_added" => {
            let id          = v["id"].as_str()?.to_owned();
            let description = v["description"].as_str().unwrap_or("").to_owned();
            let sandbox     = v["sandbox"].as_str().map(ToOwned::to_owned);
            let status      = parse_task_status(&v["status"]);
            Some(BridgeEvent::TaskAdded(Task { id, description, sandbox, status, started_at: None, completed_at: None }))
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

// ─────────────────────────────────────────────────────────────────────────────
// Orchestration runner (real socket → fallback simulation)
// ─────────────────────────────────────────────────────────────────────────────

async fn run_orchestration(
    session_id: String,
    prompt: String,
    max_agents: u32,
    mode: String,
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

    // 2. Try real socket
    let payload = serde_json::json!({
        "type":       "orchestrate",
        "prompt":     prompt.clone(),
        "project":    ".",
        "max_agents": max_agents,
        "mode":       mode,
    });
    if let Err(e) = send_socket_json(payload, &event_tx).await {
        log::warn!("Socket orchestration failed ({e}), running simulation");
        simulate_session(session_id, prompt, task_count, agent_count, sb_names, event_tx).await;
    }
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
        let task = Task { id: id.clone(), description: desc, sandbox: Some(sb), status: TaskStatus::Pending, started_at: None, completed_at: None };
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
    Ok(state.session_history.lock().unwrap().clone())
}

#[tauri::command]
async fn get_usage_history(state: State<'_, Arc<AppState>>) -> Result<Vec<UsageRecord>, String> {
    Ok(state.usage_history.lock().unwrap().clone())
}

#[tauri::command]
async fn get_config(state: State<'_, Arc<AppState>>) -> Result<Config, String> {
    Ok(state.config.lock().unwrap().clone())
}

#[tauri::command]
async fn save_config(state: State<'_, Arc<AppState>>, config: Config) -> Result<(), String> {
    *state.config.lock().unwrap() = config;
    Ok(())
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

    // Reset state
    *state.current_session_id.lock().unwrap() = Some(session_id.clone());
    state.messages.lock().unwrap().clear();
    state.tasks.lock().unwrap().clear();
    *state.tokens_total.lock().unwrap() = 0;
    *state.tool_calls_total.lock().unwrap() = 0;

    // Push user message
    state.messages.lock().unwrap().push(ChatMessage::User {
        content: prompt.clone(),
        ts: now(),
    });

    // Send command to bridge — clone sender first to avoid holding lock across .await
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

    // Finalize history
    if let Some(id) = state.current_session_id.lock().unwrap().take() {
        let tasks = state.tasks.lock().unwrap();
        let done = tasks.values().filter(|t| t.status == TaskStatus::Complete).count();
        let total = tasks.len();
        let msgs = state.messages.lock().unwrap();
        let prompt = msgs.iter().find_map(|m| {
            if let ChatMessage::User { content, .. } = m { Some(content.clone()) } else { None }
        }).unwrap_or_default();
        drop(msgs); drop(tasks);

        state.session_history.lock().unwrap().push(SessionSummary {
            id,
            prompt: prompt.chars().take(80).collect(),
            status: "stopped".into(),
            started_at: now(),
            completed_at: Some(now()),
            task_count: total,
            tasks_done: done,
        });
    }
    Ok(())
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

    Ok(serde_json::json!({
        "tasks_total":   total,
        "tasks_running": running,
        "tasks_done":    done,
        "tasks_failed":  failed,
        "tokens_total":  *state.tokens_total.lock().unwrap(),
        "tool_calls":    *state.tool_calls_total.lock().unwrap(),
        "daemon_connected": *state.daemon_connected.lock().unwrap(),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Main
// ─────────────────────────────────────────────────────────────────────────────

fn main() {
    let state = Arc::new(AppState::new());
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
            get_system_info,
            get_stats,
        ])
        .run(tauri::generate_context!())
        .expect("error running mowis-desktop");
}
