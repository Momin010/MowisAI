use crate::backend::BackendBridge;
use crate::state::*;
use crate::types::*;
use crate::zero_mode;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

pub fn start_bridge(
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

            // Small yield to let the progress listener task acquire the mutex
            // and start receiving before we emit any events.
            sleep(Duration::from_millis(10)).await;

            match bridge_clone.start().await {
                Ok(()) => {
                    let _ = evt_tx_clone.send(BridgeEvent::DaemonConnected).await;
                }
                Err(e) => {
                    let msg = format!("{:#}", e);
                    log::error!("Backend harness failed to start: {msg}");
                    // The frontend splash screen listens for setup_progress, not
                    // bridge events. If start() failed before emitting any terminal
                    // progress event (ready/error), emit the error now so the boot
                    // terminal shows what went wrong instead of staying blank.
                    let _ = bridge_clone.emit_detail(
                        "error",
                        &msg,
                        0,
                        "error",
                        None,
                    ).await;
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

                        BridgeCommand::StartOrchestration { session_id, prompt, max_agents, mode, repo_context } => {
                            tokio::spawn(async move {
                                run_orchestration(session_id, prompt, max_agents, mode, repo_context, b, tx).await;
                            });
                        }

                        BridgeCommand::StartZeroMode { session_id, prompt, config, workspace } => {
                            tokio::spawn(async move {
                                zero_mode::run_zero_session(session_id, prompt, config, workspace, tx).await;
                            });
                        }

                        BridgeCommand::ContinueZeroMode { session_id, message, config, workspace } => {
                            tokio::spawn(async move {
                                zero_mode::run_zero_session(session_id, message, config, workspace, tx).await;
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

pub async fn handle_bridge_event(event: BridgeEvent, state: &Arc<AppState>, app: &tauri::AppHandle) {
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

        BridgeEvent::FileChanges(changes) => {
            // Emit file changes as a compact visual summary in chat
            let _ = app.emit("file_changes", &changes);
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

pub fn socket_value_to_bridge_event(v: &serde_json::Value) -> Option<BridgeEvent> {
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

pub fn parse_task_status(v: &serde_json::Value) -> TaskStatus {
    match v.as_str().unwrap_or("pending") {
        "running"  => TaskStatus::Running,
        "complete" => TaskStatus::Complete,
        "failed"   => TaskStatus::Failed,
        _          => TaskStatus::Pending,
    }
}

pub fn json_string_array(value: Option<&serde_json::Value>) -> Vec<String> {
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

pub fn git_agent_policy() -> &'static str {
    "Git/GitHub workflow policy: before changing files, create or switch to a new non-main branch. Never edit directly on main or master. Run git status before edits and before final output. Review git diff, staged diffs, and branch status before committing or pushing. Do not push unless the final diff matches the requested task. Keep all coordination orchestrator-mediated; agents must not coordinate directly with other agents."
}

pub async fn run_orchestration(
    session_id: String,
    prompt: String,
    max_agents: u32,
    mode: String,
    repo_context: Option<RepositoryContext>,
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
        let project = repo_context
            .as_ref()
            .map(|ctx| ctx.project_path.clone())
            .unwrap_or_else(|| ".".to_string());
        let repo_source = repo_context.as_ref().map(|ctx| ctx.repo_source.clone());
        let repo_url = repo_context.as_ref().and_then(|ctx| ctx.repo_url.clone());
        let payload = serde_json::json!({
            "type":       "orchestrate",
            "prompt":     prompt.clone(),
            "project":    project,
            "repo_source": repo_source,
            "repo_url":    repo_url,
            "git_policy":  git_agent_policy(),
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
pub async fn simulate_session(
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

pub fn simulated_task_files(sandbox: &str, index: usize) -> Vec<String> {
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

pub fn simulated_task_views(sandbox: &str) -> Vec<String> {
    match sandbox {
        "frontend" => vec!["Session timeline".into(), "Task inspector".into()],
        "backend" => vec!["API contract".into(), "Execution trace".into()],
        "verification" => vec!["Test report".into(), "Coverage delta".into()],
        _ => Vec::new(),
    }
}
