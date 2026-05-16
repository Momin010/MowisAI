use crate::backend::BackendBridge;
use crate::state::*;
use crate::types::*;
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
    let state_for_cmds = Arc::clone(&state);
    let app_for_cmds = app.clone();
    std::thread::Builder::new()
        .name("mowisai-bridge".into())
        .spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("tokio rt");
            rt.block_on(async move {
                while let Some(cmd) = cmd_rx.recv().await {
                    let tx = evt_tx_clone.clone();
                    let b = Arc::clone(&bridge_for_cmds);
                    let st = Arc::clone(&state_for_cmds);
                    let app_cmd = app_for_cmds.clone();
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

                        BridgeCommand::StartOrchestration { session_id, prompt, max_agents, mode, repo_context, config, conversation_history } => {
                            let b_sub = Arc::clone(&b);
                            let st_sub = Arc::clone(&st);
                            let app_sub = app_cmd.clone();
                            tokio::spawn(async move {
                                run_orchestration(session_id, prompt, max_agents, mode, repo_context, config, b, tx, conversation_history).await;
                            });
                            tokio::spawn(async move {
                                run_event_subscription(b_sub, app_sub, st_sub).await;
                            });
                        }

                        BridgeCommand::StartZeroMode { session_id, .. } => {
                            log::warn!("StartZeroMode is deprecated (session: {})", session_id);
                            let _ = tx.send(BridgeEvent::OrchestrationFailed(
                                format!("Zero mode is deprecated for session {}.", session_id)
                            )).await;
                        }

                        BridgeCommand::ContinueZeroMode { session_id, .. } => {
                            log::warn!("ContinueZeroMode is deprecated (session: {})", session_id);
                            let _ = tx.send(BridgeEvent::OrchestrationFailed(
                                format!("Zero mode is deprecated for session {}.", session_id)
                            )).await;
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
            if let Err(err) = sync_current_session(state, Some("done"), Some(Some(now()))) {
                log::warn!("Failed to persist completed session: {err}");
            }
            if let Err(err) = record_usage_for_current(state, "done").and_then(|_| save_state(state)) {
                log::warn!("Failed to persist completed usage: {err}");
            }
            // No "Session complete." system message — just signal the frontend
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
        "agent_message" => {
            let content = v["content"].as_str()?;
            // Suppress internal layer progress messages from reaching the UI
            if content.starts_with("— ") || content.starts_with("\u{2014} ") {
                None
            } else {
                Some(BridgeEvent::AgentMessage(content.to_owned()))
            }
        }
        "complete"      => Some(BridgeEvent::OrchestrationComplete),
        "error"         => Some(BridgeEvent::OrchestrationFailed(
            v["message"].as_str().unwrap_or("unknown error").to_owned(),
        )),
        "tool_call"     => {
            let worker_id = v["worker_id"].as_u64().unwrap_or(0) as usize;
            let tool_name = v["tool_name"].as_str().unwrap_or("").to_owned();
            let args_preview = v["args_preview"].as_str().unwrap_or("").to_owned();
            Some(BridgeEvent::ToolCall { worker_id, tool_name, args_preview })
        }
        "tool_result"   => {
            let worker_id = v["worker_id"].as_u64().unwrap_or(0) as usize;
            let tool_name = v["tool_name"].as_str().unwrap_or("").to_owned();
            let success = v["success"].as_bool().unwrap_or(false);
            let preview = v["preview"].as_str().unwrap_or("").to_owned();
            Some(BridgeEvent::ToolResult { worker_id, tool_name, success, preview })
        }
        "stats"         => {
            // Stats events from the orchestrator — emit as a simulation tick
            let completed = v["completed"].as_u64().unwrap_or(0) as usize;
            let running = v["running"].as_u64().unwrap_or(0) as usize;
            Some(BridgeEvent::SimulationTick {
                tasks_done: completed,
                active_agents: running,
                tokens_delta: 0,
            })
        }
        "layer_progress" => {
            let layer = v["layer"].as_u64().unwrap_or(0) as u8;
            let message = v["message"].as_str().unwrap_or("").to_owned();
            Some(BridgeEvent::LayerProgress { layer, message })
        }
        "llm_thinking" => {
            let agent_id = v["agent_id"].as_str().unwrap_or("").to_owned();
            let task_description = v["task_description"].as_str().unwrap_or("").to_owned();
            Some(BridgeEvent::LlmThinking { agent_id, task_description })
        }
        "agent_status" => {
            let agent_id = v["agent_id"].as_str().unwrap_or("").to_owned();
            let task_id = v["task_id"].as_str().unwrap_or("").to_owned();
            let status = v["status"].as_str().unwrap_or("").to_owned();
            let sandbox = v["sandbox"].as_str().unwrap_or("").to_owned();
            Some(BridgeEvent::AgentStatusChanged { agent_id, task_id, status, sandbox })
        }
        "routing_decision" => {
            let mode = v["mode"].as_str().unwrap_or("auto").to_owned();
            let planning_model = v["planning_model"].as_str().unwrap_or("").to_owned();
            let execution_model = v["execution_model"].as_str().unwrap_or("").to_owned();
            Some(BridgeEvent::RoutingDecision { mode, planning_model, execution_model })
        }
        "workspace_ready" => {
            let project_path = v["project_path"].as_str().unwrap_or("").to_owned();
            let changed_files = v["changed_files"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|x| x.as_str().map(ToOwned::to_owned)).collect())
                .unwrap_or_default();
            Some(BridgeEvent::WorkspaceReady { project_path, changed_files })
        }
        _ => None,
    }
}

/// Try to extract a bridge event from any JSON value, including socket server
/// error responses that use the `{"status": "error", "error": "..."}` format.
pub fn socket_value_to_bridge_event_lenient(v: &serde_json::Value) -> Option<BridgeEvent> {
    // First try the standard type-based mapping
    if let Some(evt) = socket_value_to_bridge_event(v) {
        return Some(evt);
    }
    // Fall back to socket server error format
    if v.get("status").and_then(|s| s.as_str()) == Some("error") {
        let msg = v["error"].as_str().unwrap_or("unknown agentd error").to_owned();
        return Some(BridgeEvent::OrchestrationFailed(msg));
    }
    None
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
    config: Config,
    bridge: Arc<BackendBridge>,
    event_tx: mpsc::Sender<BridgeEvent>,
    conversation_history: Option<Vec<serde_json::Value>>,
) {
    // NOTE: Plan card is no longer emitted here. The real orchestrator inside
    // agentd streams actual events (task_added, tool_call, agent_message, etc.)
    // which provide accurate information instead of hardcoded fakes.

    // Try real bridge connection (WSL2 / QEMU / native socket)
    if bridge.is_connected() {
        // 2a. Sync provider config to agentd before orchestrating
        if !config.api_key.is_empty() || !config.gcp_project.is_empty() || config.provider == "vertex" {
            let set_cfg = serde_json::json!({
                "request_type": "set_config",
                "provider":    config.provider,
                "model":       config.model,
                "api_key":     if config.api_key.is_empty() { None } else { Some(&config.api_key) },
                "gcp_project_id": if config.gcp_project.is_empty() { None } else { Some(&config.gcp_project) },
            });
            match bridge.send(set_cfg).await {
                Ok(()) => match bridge.recv_next().await {
                    Ok(Some(resp)) => {
                        if resp.get("status").and_then(|s| s.as_str()) == Some("ok") {
                            log::info!("[bridge] Synced provider config to agentd before orchestration");
                        } else {
                            let err = resp["error"].as_str().unwrap_or("unknown");
                            log::warn!("[bridge] agentd rejected config sync: {}", err);
                        }
                    }
                    Ok(None) => log::warn!("[bridge] agentd closed during config sync"),
                    Err(e) => log::warn!("[bridge] Config sync recv error: {}", e),
                },
                Err(e) => log::warn!("[bridge] Config sync send failed: {}", e),
            }
        }

        // 2b. Send orchestrate command
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
            "conversation_history": conversation_history,
        });
        match bridge.send(payload).await {
            Ok(()) => {
                // Stream JSON events until the daemon closes the connection.
                loop {
                    match bridge.recv_next().await {
                        Ok(Some(v)) => {
                            log::debug!("[bridge] recv: {}", v);
                            if let Some(evt) = socket_value_to_bridge_event_lenient(&v) {
                                let is_done = matches!(&evt, BridgeEvent::OrchestrationComplete | BridgeEvent::OrchestrationFailed(_));
                                if event_tx.send(evt).await.is_err() { return; }
                                if is_done { return; }
                            }
                        }
                        Ok(None) => {
                            log::error!("[bridge] agentd closed the connection unexpectedly (EOF)");
                            let _ = event_tx.send(BridgeEvent::OrchestrationFailed(
                                "agentd closed the connection. Check /tmp/agentd.log inside WSL for details.".into()
                            )).await;
                            return;
                        }
                        Err(e) => {
                            log::error!("[bridge] recv error: {e}");
                            let _ = event_tx.send(BridgeEvent::OrchestrationFailed(
                                format!("Connection to agentd failed: {e}")
                            )).await;
                            return;
                        }
                    }
                }
            }
            Err(e) => {
                log::error!("[bridge] send orchestrate failed: {e}");
                let _ = event_tx.send(BridgeEvent::OrchestrationFailed(
                    format!("Failed to send orchestrate command: {e}")
                )).await;
                return;
            }
        }
    } else {
        log::error!("Daemon not connected — cannot start orchestration");
        let _ = event_tx.send(BridgeEvent::OrchestrationFailed(
            "Daemon is not connected. Check the setup terminal for errors.".into()
        )).await;
        return;
    }
}

// NOTE: simulate_session has been PERMANENTLY DELETED.
// If you see a simulation running, the binary is STALE — rebuild with:
//   cd mowis-desktop && cargo tauri build
// The compiler will error if any code path tries to call simulate_session.

// ─────────────────────────────────────────────────────────────────────────────
// Event subscription (second socket connection for live streaming)
// ─────────────────────────────────────────────────────────────────────────────

/// Opens a dedicated socket connection to agentd, subscribes to the event
/// stream, and re-emits every event as a Tauri "stream_event" event with the
/// raw JSON payload. Runs until the connection closes or SessionComplete arrives.
pub async fn run_event_subscription(
    bridge: Arc<crate::backend::BackendBridge>,
    app: tauri::AppHandle,
    state: Arc<crate::state::AppState>,
) {
    let mut stream = match bridge.open_fresh_stream().await {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[bridge] Event subscription: could not open stream: {}", e);
            return;
        }
    };

    let subscribe_msg = serde_json::json!({"request_type": "subscribe_events"});
    if let Err(e) = stream.send_json(&subscribe_msg).await {
        log::warn!("[bridge] Event subscription: send subscribe_events failed: {}", e);
        return;
    }

    log::info!("[bridge] Event subscription stream active");

    loop {
        match stream.recv_json().await {
            Ok(Some(v)) => {
                let raw = v.to_string();
                let _ = app.emit("stream_event", &raw);

                let event_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match event_type {
                    "AgentStarted" => {
                        let agent_id = v.get("agent_id")
                            .and_then(|a| a.as_str())
                            .unwrap_or("")
                            .to_string();
                        let sandbox = v.get("sandbox")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !agent_id.is_empty() {
                            state.agent_states.lock().unwrap().insert(agent_id, v.clone());
                        }
                        if !sandbox.is_empty() {
                            let mut ids = state.active_sandbox_ids.lock().unwrap();
                            if !ids.contains(&sandbox) {
                                ids.push(sandbox);
                            }
                        }
                    }
                    "AgentCompleted" | "AgentFailed" => {
                        let agent_id = v.get("agent_id")
                            .and_then(|a| a.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !agent_id.is_empty() {
                            state.agent_states.lock().unwrap().insert(agent_id, v.clone());
                        }
                    }
                    "SessionComplete" => {
                        state.active_sandbox_ids.lock().unwrap().clear();
                        break;
                    }
                    _ => {}
                }
            }
            Ok(None) => {
                log::info!("[bridge] Event subscription: stream EOF");
                break;
            }
            Err(e) => {
                log::warn!("[bridge] Event subscription: stream error: {}", e);
                break;
            }
        }
    }

    log::info!("[bridge] Event subscription ended");
}
