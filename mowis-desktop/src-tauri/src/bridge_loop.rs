use crate::backend::OrchBridge;
use crate::orch_bridge::{self, build_orch_config, start_orch_session};
use crate::state::*;
use crate::types::*;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::mpsc;

pub fn start_bridge(
    app: tauri::AppHandle,
    state: Arc<AppState>,
) -> mpsc::Sender<BridgeCommand> {
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<BridgeCommand>(64);
    let (evt_tx, mut evt_rx) = mpsc::channel::<BridgeEvent>(256);

    let bridge = Arc::clone(&state.bridge);

    // ── 1. We're always ready in local mode — emit connected immediately ───────
    {
        let bridge_clone = Arc::clone(&bridge);
        let evt_tx_clone = evt_tx.clone();
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            // Forward setup-progress events to the frontend (used by OS security
            // mode splash screen when VM boot is added in the future).
            let bridge_prog = Arc::clone(&bridge_clone);
            tauri::async_runtime::spawn(async move {
                let mut rx = bridge_prog.progress_rx.lock().await;
                while let Some(prog) = rx.recv().await {
                    let _ = app_clone.emit("setup_progress", &prog);
                }
            });

            bridge_clone.set_ready();
            let _ = evt_tx_clone.send(BridgeEvent::DaemonConnected).await;
        });
    }

    // ── 2. Command handler ─────────────────────────────────────────────────────
    let evt_tx_for_cmds = evt_tx.clone();
    let state_for_cmds = Arc::clone(&state);
    tauri::async_runtime::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            let tx = evt_tx_for_cmds.clone();
            let st = Arc::clone(&state_for_cmds);

            match cmd {
                BridgeCommand::CheckSocket => {
                    let connected = st.bridge.is_connected();
                    let evt = if connected {
                        BridgeEvent::DaemonConnected
                    } else {
                        BridgeEvent::DaemonDisconnected
                    };
                    let _ = tx.send(evt).await;
                }

                BridgeCommand::StopOrchestration => {
                    use mowis_orchestration::conductor::ConductorCommand;
                    let session = st.orch_session.lock().await;
                    if let Some(s) = session.as_ref() {
                        let _ = s.conductor_tx.send(ConductorCommand::EndConversation).await;
                    }
                    let _ = tx.send(BridgeEvent::OrchestrationComplete).await;
                }

                BridgeCommand::StartOrchestration {
                    session_id,
                    prompt,
                    config,
                    repo_context,
                    ..
                } => {
                    tauri::async_runtime::spawn(async move {
                        run_orch_turn(session_id, prompt, config, repo_context, st, tx).await;
                    });
                }

                BridgeCommand::StartZeroMode { session_id, .. } => {
                    log::warn!("StartZeroMode is deprecated (session: {})", session_id);
                    let _ = tx.send(BridgeEvent::OrchestrationFailed(
                        "Zero mode is deprecated.".into(),
                    )).await;
                }

                BridgeCommand::ContinueZeroMode { session_id, .. } => {
                    log::warn!("ContinueZeroMode is deprecated (session: {})", session_id);
                    let _ = tx.send(BridgeEvent::OrchestrationFailed(
                        "Zero mode is deprecated.".into(),
                    )).await;
                }
            }
        }
    });

    // ── 3. Event consumer — bridge events → Tauri frontend ────────────────────
    let state_clone = Arc::clone(&state);
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = evt_rx.recv().await {
            handle_bridge_event(event, &state_clone, &app_clone).await;
        }
    });

    cmd_tx
}

// ── In-process orchestration turn ────────────────────────────────────────────

async fn run_orch_turn(
    session_id: String,
    prompt: String,
    config: Config,
    repo_context: Option<RepositoryContext>,
    state: Arc<AppState>,
    event_tx: mpsc::Sender<BridgeEvent>,
) {
    // Build OrchConfig from desktop Config.
    let orch_cfg = match build_orch_config(&config) {
        Ok(c) => c,
        Err(e) => {
            let _ = event_tx.send(BridgeEvent::OrchestrationFailed(
                format!("Config error: {}", e),
            )).await;
            return;
        }
    };

    // Resolve workspace: use repo_context path if provided, otherwise a
    // per-session sandbox directory under ~/.local/share/MowisAI/sessions/.
    let workspace = if let Some(ref ctx) = repo_context {
        PathBuf::from(&ctx.project_path)
    } else {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("MowisAI")
            .join("sessions")
            .join(&session_id)
            .join("workspace")
    };
    let _ = std::fs::create_dir_all(&workspace);
    let workspace = workspace.canonicalize().unwrap_or(workspace);

    // save_dest: where save_to_host copies files on user request.
    let save_dest = repo_context
        .as_ref()
        .map(|ctx| PathBuf::from(&ctx.project_path))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Acquire/create the orchestration session.
    // If we already have a session for this session_id, reuse it (multi-turn).
    // If the session_id changed (new session), create a fresh one.
    let mut orch_guard = state.orch_session.lock().await;
    let needs_new = orch_guard
        .as_ref()
        .map(|s| s.session_id != session_id)
        .unwrap_or(true);

    if needs_new {
        // OS Security mode: boot the Alpine VM and route tool calls through vsock.
        // Splash progress events flow via SetupProgress → frontend.
        let vm = if config.os_security {
            match orch_bridge::boot_os_security_vm(Arc::clone(&state.bridge)).await {
                Ok(v) => Some(v),
                Err(e) => {
                    drop(orch_guard);
                    let _ = event_tx.send(BridgeEvent::OrchestrationFailed(
                        format!("OS Security VM boot failed: {}", e),
                    )).await;
                    return;
                }
            }
        } else {
            None
        };

        match start_orch_session(
            session_id.clone(),
            orch_cfg,
            workspace,
            save_dest,
            event_tx.clone(),
            vm,
        ) {
            Ok(session) => {
                *orch_guard = Some(session);
            }
            Err(e) => {
                drop(orch_guard);
                let _ = event_tx.send(BridgeEvent::OrchestrationFailed(
                    format!("Failed to start orchestration: {}", e),
                )).await;
                return;
            }
        }
    }

    let session = orch_guard.as_ref().unwrap();

    // Send message to conductor. The conductor task handles the LLM call and
    // emits all events to the bus (which the subscriber forwards to event_tx).
    match orch_bridge::send_message(session, prompt).await {
        Ok(_reply) => {
            // Events already flowed through EventBus → dispatch → event_tx.
            // OrchestrationComplete is emitted by PlanCompleted or Chat replies.
        }
        Err(e) => {
            drop(orch_guard);
            let _ = event_tx.send(BridgeEvent::OrchestrationFailed(
                format!("Orchestration error: {}", e),
            )).await;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Bridge event handler — Tauri emitter (UNCHANGED)
// ─────────────────────────────────────────────────────────────────────────────

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
            msgs.push(ChatMessage::Agent { content: chunk.clone(), streaming: true, ts: now() });
            drop(msgs);
            if let Err(err) = sync_current_session(state, Some("running"), None) {
                log::warn!("Failed to persist agent chunk: {err}");
            }
            let _ = app.emit("agent_chunk", serde_json::json!({ "chunk": chunk }));
        }

        BridgeEvent::AgentMessage(content) => {
            let mut msgs = state.messages.lock().unwrap();
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
            let _ = app.emit("file_changes", &changes);
        }

        BridgeEvent::ToolCall { worker_id, tool_name, args_preview } => {
            let _ = app.emit("tool_call", serde_json::json!({
                "worker_id": worker_id,
                "tool_name": tool_name,
                "args_preview": args_preview,
            }));
        }

        BridgeEvent::ToolResult { worker_id, tool_name, success, preview } => {
            let _ = app.emit("tool_result", serde_json::json!({
                "worker_id": worker_id,
                "tool_name": tool_name,
                "success": success,
                "preview": preview,
            }));
        }

        BridgeEvent::OrchestrationComplete => {
            {
                let mut msgs = state.messages.lock().unwrap();
                if let Some(last) = msgs.last_mut() {
                    if let ChatMessage::Agent { streaming, .. } = last {
                        *streaming = false;
                    }
                }
            }
            for t in state.tasks.lock().unwrap().values_mut() {
                if t.status == TaskStatus::Running {
                    t.status = TaskStatus::Complete;
                    t.completed_at = Some(now());
                }
            }
            if let Err(err) = sync_current_session(state, Some("done"), Some(Some(now()))) {
                log::warn!("Failed to persist completed session: {err}");
            }
            if let Err(err) = record_usage_for_current(state, "done").and_then(|_| save_state(state)) {
                log::warn!("Failed to persist completed usage: {err}");
            }
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

        BridgeEvent::LayerProgress { layer, message } => {
            let _ = app.emit("layer_progress", serde_json::json!({
                "layer": layer,
                "message": message,
            }));
        }

        BridgeEvent::LlmThinking { agent_id, task_description } => {
            let _ = app.emit("llm_thinking", serde_json::json!({
                "agent_id": agent_id,
                "task_description": task_description,
            }));
        }

        BridgeEvent::AgentStatusChanged { agent_id, task_id, status, sandbox } => {
            let _ = app.emit("agent_status", serde_json::json!({
                "agent_id": agent_id,
                "task_id": task_id,
                "status": status,
                "sandbox": sandbox,
            }));
        }

        BridgeEvent::RoutingDecision { mode, planning_model, execution_model } => {
            let _ = app.emit("routing_decision", serde_json::json!({
                "mode": mode,
                "planning_model": planning_model,
                "execution_model": execution_model,
            }));
        }

        BridgeEvent::WorkspaceReady { project_path, changed_files } => {
            *state.workspace_path.lock().unwrap() = Some(project_path.clone());
            *state.workspace_files.lock().unwrap() = changed_files.clone();
            let _ = app.emit("workspace_ready", serde_json::json!({
                "project_path": project_path,
                "changed_files": changed_files,
            }));
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

// ── Legacy helpers kept for types.rs compatibility ────────────────────────────

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
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
