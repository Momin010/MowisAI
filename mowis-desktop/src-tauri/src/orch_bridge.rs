//! In-process orchestration bridge.
//!
//! Mirrors exactly what the TUI (mowis-host/src/tui/app.rs) does:
//! Conductor + Critic + EventBus all run in-process. OrchEvents are converted
//! to BridgeEvents and forwarded to the Tauri frontend via mpsc.
//!
//! OS Security mode (future): pass a VM connection to the captain so tool
//! calls execute inside Alpine via mowis-executor over vsock.

use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;

use mowis_orchestration::config::{ModelRef, OrchConfig, ProviderCreds};
use mowis_orchestration::conductor::{Conductor, ConductorCommand, ConductorReply};
use mowis_orchestration::critic::Critic;
use mowis_orchestration::events::{ConductorReplyKind, Event as OrchEvent, EventBus};
use mowis_orchestration::plan::Tier;
use mowis_orchestration::providers::Provider;

use crate::state::now;
use crate::types::{BridgeEvent, Config, Task, TaskStatus};

// ── Config conversion ─────────────────────────────────────────────────────────

pub fn build_orch_config(config: &Config) -> Result<OrchConfig> {
    let provider = match config.provider.as_str() {
        "anthropic" => Provider::Anthropic,
        "openai" => Provider::OpenAi,
        "gemini" | "google" => Provider::Gemini,
        "vertex" => Provider::VertexAi,
        other => anyhow::bail!("unknown provider: {}", other),
    };

    let api_key_enc = if !config.api_key.is_empty() {
        Some(mowis_orchestration::crypto::encrypt(&config.api_key)?)
    } else {
        None
    };

    let project_id = if !config.gcp_project.is_empty() {
        Some(config.gcp_project.clone())
    } else {
        None
    };

    let mut providers = HashMap::new();
    providers.insert(
        provider.clone(),
        ProviderCreds { api_key_enc, project_id },
    );

    let planning_model = config.model.clone();
    let execution_model = if config.execution_model.is_empty() {
        config.model.clone()
    } else {
        config.execution_model.clone()
    };

    let mut tiers = HashMap::new();
    tiers.insert(Tier::Conductor, ModelRef { provider: provider.clone(), model: planning_model.clone() });
    tiers.insert(Tier::Critic,    ModelRef { provider: provider.clone(), model: planning_model.clone() });
    tiers.insert(Tier::Captain,   ModelRef { provider: provider.clone(), model: planning_model.clone() });
    tiers.insert(Tier::Crew,      ModelRef { provider: provider.clone(), model: execution_model });

    let plans_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("MowisAI")
        .join("plans");

    Ok(OrchConfig {
        providers,
        tiers,
        sandbox: Default::default(),
        plans_dir,
    })
}

// ── OrchEvent → BridgeEvent ───────────────────────────────────────────────────

pub async fn dispatch(event: OrchEvent, tx: &mpsc::Sender<BridgeEvent>) {
    let to_send: Option<BridgeEvent> = match event {
        OrchEvent::StreamToken { text } => Some(BridgeEvent::AgentChunk(text)),

        OrchEvent::StreamDone => None,

        OrchEvent::ConductorReply { text, kind } => match kind {
            ConductorReplyKind::Error => Some(BridgeEvent::OrchestrationFailed(text)),
            _ if !text.is_empty() => Some(BridgeEvent::AgentMessage(text)),
            _ => None,
        },

        OrchEvent::PlanDrafted { plan_id, version } => Some(BridgeEvent::PlanReady {
            sandboxes: vec![plan_id.0],
            task_count: 0,
            agent_count: 0,
            mode: format!("v{}", version),
        }),

        OrchEvent::PlanRevised { plan_id, version } => Some(BridgeEvent::PlanReady {
            sandboxes: vec![plan_id.0],
            task_count: 0,
            agent_count: 0,
            mode: format!("v{} (revised)", version),
        }),

        OrchEvent::CriticVerdict { verdict, version, .. } => {
            Some(BridgeEvent::LayerProgress {
                layer: 2,
                message: format!("Critic (v{}): {:?}", version, verdict),
            })
        }

        OrchEvent::CaptainStarted { plan_id, sandbox_id } => {
            Some(BridgeEvent::AgentStatusChanged {
                agent_id: "captain".into(),
                task_id: plan_id.0,
                status: "running".into(),
                sandbox: sandbox_id,
            })
        }

        OrchEvent::CrewStarted { plan_id, task_id, agent_id } => {
            let task = Task {
                id: task_id.0.clone(),
                description: format!("Task {}", task_id.0),
                sandbox: Some(plan_id.0.clone()),
                status: TaskStatus::Running,
                started_at: Some(now()),
                completed_at: None,
                files: vec![],
                summary: None,
                views: vec![],
            };
            let _ = tx.send(BridgeEvent::TaskAdded(task)).await;
            Some(BridgeEvent::AgentStatusChanged {
                agent_id,
                task_id: task_id.0,
                status: "running".into(),
                sandbox: plan_id.0,
            })
        }

        OrchEvent::CrewToolSummary { agent_id, text, tool_name, success } => {
            Some(BridgeEvent::ToolResult {
                worker_id: 0,
                tool_name,
                success,
                preview: format!("[{}] {}", agent_id, text),
            })
        }

        OrchEvent::CrewDone { plan_id: _, agent_id, summary } => {
            let _ = tx.send(BridgeEvent::TaskUpdated {
                id: agent_id.clone(),
                status: TaskStatus::Complete,
            }).await;
            Some(BridgeEvent::AgentStatusChanged {
                agent_id,
                task_id: String::new(),
                status: "complete".into(),
                sandbox: String::new(),
            })
        }

        OrchEvent::CrewFailed { plan_id: _, agent_id, reason } => {
            let _ = tx.send(BridgeEvent::TaskUpdated {
                id: agent_id.clone(),
                status: TaskStatus::Failed,
            }).await;
            Some(BridgeEvent::AgentStatusChanged {
                agent_id,
                task_id: String::new(),
                status: "failed".into(),
                sandbox: reason,
            })
        }

        OrchEvent::MergeCompleted { agent_id, .. } => Some(BridgeEvent::ToolCall {
            worker_id: 0,
            tool_name: "merge_overlay".into(),
            args_preview: agent_id,
        }),

        OrchEvent::PlanCompleted { .. } => Some(BridgeEvent::OrchestrationComplete),

        OrchEvent::PlanFailed { reason, .. } => Some(BridgeEvent::OrchestrationFailed(reason)),

        OrchEvent::TokensUsed { input_tokens, output_tokens, .. } => {
            let total = (input_tokens + output_tokens) as u64;
            Some(BridgeEvent::SimulationTick {
                tasks_done: 0,
                active_agents: 1,
                tokens_delta: total,
            })
        }

        OrchEvent::CaptainStatusUpdate { status } => {
            let in_flight = status.in_flight.len();
            let completed = status.completed.len();
            Some(BridgeEvent::SimulationTick {
                tasks_done: completed,
                active_agents: in_flight,
                tokens_delta: 0,
            })
        }

        // Events we don't need to surface to the frontend.
        OrchEvent::PlanApproved { .. }
        | OrchEvent::PlanSuperseded { .. }
        | OrchEvent::CriticReviewing { .. }
        | OrchEvent::UserApproved { .. }
        | OrchEvent::UserOverride { .. }
        | OrchEvent::UserCancelled { .. }
        | OrchEvent::UserMessageReceived { .. }
        | OrchEvent::MergeStarted { .. }
        | OrchEvent::TaskInjected { .. }
        | OrchEvent::ConversationEnded
        | OrchEvent::CaptainShutdown { .. } => None,
    };

    if let Some(evt) = to_send {
        let _ = tx.send(evt).await;
    }
}

// ── Active orchestration session ──────────────────────────────────────────────

/// Holds the live Conductor command channel and EventBus for a session.
/// Created once per session and persisted in AppState across conversation turns.
pub struct OrchSession {
    /// Send ConductorCommands here (UserMessage, CriticVerdict, EndConversation).
    pub conductor_tx: mpsc::Sender<ConductorCommand>,
    /// Clone to subscribe to live orchestration events.
    pub bus: EventBus,
    /// Session id this was created for.
    pub session_id: String,
    /// VM handle when OS Security mode is on. Kept alive for the session;
    /// drops shut down the VM when the session ends.
    pub _vm_handle: Option<Box<dyn mowis_host::vmm::VmHandle>>,
}

/// Start a new orchestration session. Spawns Conductor + Critic tasks,
/// subscribes to the EventBus, and forwards every event to `event_tx`.
///
/// When `vm` is `Some`, the captain's transport is rebuilt as a vsock factory
/// pointing at mowis-executor inside the guest. The VmHandle is kept alive for
/// the session lifetime — dropping it shuts the VM down.
pub fn start_orch_session(
    session_id: String,
    cfg: OrchConfig,
    workspace: PathBuf,
    save_dest: PathBuf,
    event_tx: mpsc::Sender<BridgeEvent>,
    vm: Option<(Box<dyn mowis_host::vmm::VmHandle>, std::sync::Arc<mowis_host::transport::Connection>)>,
) -> Result<OrchSession> {
    let bus = EventBus::new();

    // Subscribe to EventBus and forward events to the Tauri bridge channel.
    let mut bus_rx = bus.subscribe();
    let fwd_tx = event_tx.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match bus_rx.recv().await {
                Ok(ev) => {
                    dispatch(ev, &fwd_tx).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });

    // Create Conductor.
    let (mut conductor, _) = Conductor::new(&cfg, bus.clone())?;
    conductor.set_workspace(workspace, save_dest);

    // OS Security mode: swap the captain's transport factory to vsock so
    // every crew tool call routes through mowis-executor inside the guest.
    let vm_handle = if let Some((handle, conn)) = vm {
        let factory = mowis_host::vsock_transport::vsock_transport_factory(conn);
        conductor.set_transport_factory(factory);
        Some(handle)
    } else {
        None
    };

    // Conductor task — processes one message at a time.
    let (conductor_cmd_tx, mut conductor_cmd_rx) = mpsc::channel::<ConductorCommand>(32);
    let bus_for_conductor = bus.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(cmd) = conductor_cmd_rx.recv().await {
            match cmd {
                ConductorCommand::UserMessage { text, reply_tx } => {
                    match conductor.handle_user_message(text).await {
                        Ok(reply) => {
                            // The reply is already emitted to the bus as ConductorReply event.
                            // We just need to notify the caller that we're done.
                            let _ = reply_tx.send(reply);
                        }
                        Err(e) => {
                            log::error!("Conductor error: {}", e);
                            let _ = reply_tx.send(ConductorReply::Error {
                                message: e.to_string(),
                            });
                        }
                    }
                }
                ConductorCommand::CriticVerdict { plan_id, version, verdict } => {
                    let _ = conductor.handle_critic_verdict(plan_id, version, verdict).await;
                }
                ConductorCommand::EndConversation => {
                    bus_for_conductor.emit(OrchEvent::ConversationEnded);
                    break;
                }
            }
        }
    });

    // Critic task — runs in background, listens on bus.
    let mut critic = Critic::new(&cfg, bus.clone())?;
    tauri::async_runtime::spawn(async move {
        if let Err(e) = critic.run().await {
            log::error!("Critic exited with error: {}", e);
        }
    });

    Ok(OrchSession {
        conductor_tx: conductor_cmd_tx,
        bus,
        session_id,
        _vm_handle: vm_handle,
    })
}

// ── VM boot for OS Security mode ──────────────────────────────────────────────

/// Boot Alpine via mowis-host::vmm and connect to mowis-executor over vsock.
/// Emits SetupProgress events to `bridge` so the desktop splash shows live progress.
pub async fn boot_os_security_vm(
    bridge: std::sync::Arc<crate::backend::OrchBridge>,
) -> Result<(Box<dyn mowis_host::vmm::VmHandle>, std::sync::Arc<mowis_host::transport::Connection>)> {
    bridge.emit_detail("detecting", "Locating mowis-executor binary…", 5, "info", None).await;
    let executor = find_executor_binary()
        .ok_or_else(|| anyhow::anyhow!("mowis-executor binary not found"))?;
    bridge.emit_detail("detecting", &format!("Found executor: {}", executor.display()), 10, "info", None).await;

    let initrd_path = std::env::temp_dir().join("mowis-initrd.cpio.gz");
    if !initrd_path.exists() {
        bridge.emit_detail("installing", "Building initramfs…", 25, "info", None).await;
        mowis_host::initrd::build(&executor, &initrd_path).await?;
        bridge.emit_detail("installing", &format!("Initramfs built: {}", initrd_path.display()), 40, "success", None).await;
    } else {
        bridge.emit_detail("installing", "Using cached initramfs", 40, "info", None).await;
    }

    bridge.emit_detail("booting", "Locating Linux kernel…", 50, "info", None).await;
    let kernel = mowis_host::initrd::default_kernel()
        .ok_or_else(|| anyhow::anyhow!("no /boot/vmlinuz-* found — install linux-image"))?;
    bridge.emit_detail("booting", &format!("Kernel: {}", kernel.display()), 55, "info", None).await;

    bridge.emit_detail("booting", "Starting Alpine VM…", 65, "info", None).await;
    let backend = mowis_host::vmm::default_backend()?;
    let handle = backend.boot(mowis_host::vmm::VmConfig {
        kernel,
        initrd: initrd_path,
        rootfs: None,
        memory_mb: 2048,
        vcpus: 2,
        guest_cid: 42,
        executor_port: mowis_host::protocol::DEFAULT_VSOCK_PORT,
        extra_cmdline: vec![],
    }).await?;
    bridge.emit_detail("booting", &format!("VM booted: cid={} port={}", handle.guest_cid(), handle.executor_port()), 80, "success", None).await;

    bridge.emit_detail("booting", "Waiting for executor to come online…", 85, "info", None).await;
    let conn = wait_for_executor(handle.as_ref(), 30).await?;
    bridge.emit_detail("ready", "OS Security mode online", 100, "success", None).await;

    Ok((handle, std::sync::Arc::new(conn)))
}

fn find_executor_binary() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("target/release/mowis-executor"),
        PathBuf::from("target/debug/mowis-executor"),
        PathBuf::from("/usr/local/bin/mowis-executor"),
    ];
    candidates.iter().find(|p| p.exists()).cloned()
}

async fn wait_for_executor(
    handle: &dyn mowis_host::vmm::VmHandle,
    max_secs: u64,
) -> Result<mowis_host::transport::Connection> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(max_secs);
    loop {
        let conn_result = if handle.use_tcp() {
            let host = handle.executor_host();
            let port = handle.executor_port() as u16;
            mowis_host::transport::connect_tcp_conn(host, port).await
        } else {
            #[cfg(target_os = "linux")]
            { mowis_host::transport::connect(handle.guest_cid(), handle.executor_port()).await }
            #[cfg(not(target_os = "linux"))]
            { Err(anyhow::anyhow!("vsock not available on this platform")) }
        };
        if let Ok(conn) = conn_result {
            if conn.ping().await.is_ok() {
                return Ok(conn);
            }
        }
        if std::time::Instant::now() > deadline {
            anyhow::bail!("executor did not become ready within {}s", max_secs);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// Send a user message to the conductor and wait for its reply.
/// The reply content is already forwarded to the frontend via the EventBus
/// subscriber; this just signals completion.
pub async fn send_message(
    session: &OrchSession,
    message: String,
) -> Result<ConductorReply> {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    session.conductor_tx.send(ConductorCommand::UserMessage {
        text: message,
        reply_tx,
    }).await.map_err(|_| anyhow::anyhow!("conductor task has shut down"))?;

    reply_rx.await.map_err(|_| anyhow::anyhow!("conductor reply channel closed"))
}
