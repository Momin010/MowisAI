use crate::connection::DaemonConnection;
use crate::launcher::{ConnectionInfo, VmLauncher};
use crate::platform::Platform;
use crate::types::{BackendEvent, FileDiff, FrontendCommand, Task, TaskStatus};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{interval, Duration};

// ── Helper macro (must be defined before the async fns that use it) ───────────

/// Send a `BackendEvent` through an `mpsc::Sender`; return early from the
/// surrounding `async fn` if the receiver has been dropped.
macro_rules! send_or_return {
    ($tx:expr, $event:expr) => {
        if $tx.send($event).await.is_err() {
            return;
        }
    };
}

// ── Public surface ─────────────────────────────────────────────────────────────

/// Bridge between the egui main thread and the async agentd daemon.
///
/// The GUI creates one `Backend` at startup, polls `event_rx` on every frame,
/// and sends commands down `command_tx`.  All heavy work (process spawning,
/// socket I/O, git polling) runs on a dedicated OS thread with its own tokio
/// runtime so the egui render loop is never blocked.
pub struct Backend {
    /// Events flowing from the background worker → GUI.
    pub event_rx: mpsc::Receiver<BackendEvent>,
    /// Commands flowing from the GUI → background worker.
    pub command_tx: mpsc::Sender<FrontendCommand>,
}

impl Backend {
    /// Spin up the tokio runtime in a dedicated OS thread and return
    /// immediately.  The caller owns the channel ends; the runtime owns
    /// everything else.
    pub fn spawn(project_dir: impl Into<String>) -> Self {
        let project_dir = project_dir.into();

        let (event_tx, event_rx) = mpsc::channel::<BackendEvent>(256);
        let (command_tx, command_rx) = mpsc::channel::<FrontendCommand>(256);

        std::thread::Builder::new()
            .name("mowisai-backend".into())
            .spawn(move || {
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(r) => r,
                    Err(e) => {
                        // Cannot send the event because the channel requires
                        // async; best we can do is log and exit the thread.
                        log::error!("Failed to create tokio runtime: {e}");
                        return;
                    }
                };
                rt.block_on(run(project_dir, event_tx, command_rx));
            })
            .expect("Failed to spawn backend thread");

        Backend { event_rx, command_tx }
    }
}

// ── Top-level async entry point ────────────────────────────────────────────────

async fn run(
    project_dir: String,
    event_tx: mpsc::Sender<BackendEvent>,
    command_rx: mpsc::Receiver<FrontendCommand>,
) {
    // 1. Select platform-specific launcher
    let _ = event_tx.send(BackendEvent::DaemonStarting).await;
    let _ = event_tx.send(BackendEvent::DaemonProgress {
        message: "Selecting launcher for your platform...".to_string(),
        percent: Some(10),
    }).await;

    let launcher = match select_launcher() {
        Ok(l) => Arc::new(l),
        Err(e) => {
            let _ = event_tx.send(BackendEvent::DaemonFailed(e.to_string())).await;
            log::error!("Failed to select launcher: {e}");
            return;
        }
    };

    // 2. Launch daemon (or verify it is already running)
    let _ = event_tx.send(BackendEvent::DaemonProgress {
        message: "Starting AI engine (first time may take 15-20 seconds)...".to_string(),
        percent: Some(30),
    }).await;

    let connection_info = match launcher.start().await {
        Ok(info) => {
            let _ = event_tx.send(BackendEvent::DaemonProgress {
                message: "AI engine started successfully".to_string(),
                percent: Some(70),
            }).await;
            let _ = event_tx.send(BackendEvent::DaemonStarted).await;
            info
        }
        Err(e) => {
            let _ = event_tx.send(BackendEvent::DaemonFailed(e.to_string())).await;
            log::error!("Failed to start daemon: {e}");
            return;
        }
    };

    // 3. Create connection to daemon
    let _ = event_tx.send(BackendEvent::DaemonProgress {
        message: "Establishing connection...".to_string(),
        percent: Some(90),
    }).await;

    let connection = match create_connection(connection_info) {
        Ok(conn) => Arc::new(Mutex::new(conn)),
        Err(e) => {
            let _ = event_tx.send(BackendEvent::DaemonFailed(e.to_string())).await;
            log::error!("Failed to create connection: {e}");
            return;
        }
    };

    let _ = event_tx.send(BackendEvent::DaemonProgress {
        message: "Ready".to_string(),
        percent: Some(100),
    }).await;

    // 4. Health check polling (every 10s)
    let health_tx = event_tx.clone();
    let health_launcher = launcher.clone();
    tokio::spawn(async move {
        run_health_check(health_launcher, health_tx).await;
    });

    // 5. Git diff watcher (independent loop)
    let watcher_tx = event_tx.clone();
    let watcher_dir = project_dir.clone();
    tokio::spawn(async move {
        run_git_watcher(watcher_dir, watcher_tx).await;
    });

    // 6. Command handler (drives the main loop)
    run_command_handler(command_rx, event_tx, connection).await;
}

// ── 1. Launcher selection ──────────────────────────────────────────────────────

/// Select the appropriate launcher based on the current platform
fn select_launcher() -> Result<Box<dyn VmLauncher>> {
    let platform = Platform::current();
    
    match platform {
        Platform::Linux => {
            #[cfg(target_os = "linux")]
            {
                log::info!("Using Linux direct launcher");
                let launcher = crate::launchers::linux::LinuxDirectLauncher::new()?;
                Ok(Box::new(launcher))
            }
            #[cfg(not(target_os = "linux"))]
            {
                log::warn!("Linux launcher not available on this platform, using QEMU");
                let config = crate::launcher::LauncherConfig::default();
                let launcher = crate::launchers::qemu::QEMULauncher::new(config)?;
                Ok(Box::new(launcher))
            }
        }
        Platform::MacOS => {
            #[cfg(target_os = "macos")]
            {
                log::info!("Using macOS Virtualization.framework launcher");
                let config = crate::launcher::LauncherConfig::default();
                let launcher = crate::launchers::macos::MacOSLauncher::new(config)?;
                Ok(Box::new(launcher))
            }
            #[cfg(not(target_os = "macos"))]
            {
                log::warn!("macOS launcher not available, falling back to QEMU");
                let config = crate::launcher::LauncherConfig::default();
                let launcher = crate::launchers::qemu::QEMULauncher::new(config)?;
                Ok(Box::new(launcher))
            }
        }
        Platform::Windows => {
            #[cfg(target_os = "windows")]
            {
                // Try WSL2 first, fall back to QEMU if unavailable
                log::info!("Checking WSL2 availability...");
                
                // Quick check if WSL2 is available
                let wsl_available = std::process::Command::new("wsl")
                    .arg("--status")
                    .output()
                    .map(|output| output.status.success())
                    .unwrap_or(false);
                
                if wsl_available {
                    log::info!("WSL2 available, using WSL2 launcher");
                    let config = crate::launcher::LauncherConfig::default();
                    match crate::launchers::wsl2::WSL2Launcher::new(config) {
                        Ok(launcher) => return Ok(Box::new(launcher)),
                        Err(e) => {
                            log::warn!("WSL2 launcher failed to initialize: {}, falling back to QEMU", e);
                        }
                    }
                }
                
                // Fall back to QEMU
                log::info!("Using QEMU fallback launcher");
                let config = crate::launcher::LauncherConfig::default();
                let launcher = crate::launchers::qemu::QEMULauncher::new(config)?;
                Ok(Box::new(launcher))
            }
            #[cfg(not(target_os = "windows"))]
            {
                log::warn!("Windows launcher not available, falling back to QEMU");
                let config = crate::launcher::LauncherConfig::default();
                let launcher = crate::launchers::qemu::QEMULauncher::new(config)?;
                Ok(Box::new(launcher))
            }
        }
    }
}

/// Create a connection based on the connection info
fn create_connection(info: ConnectionInfo) -> Result<Box<dyn DaemonConnection>> {
    match info {
        ConnectionInfo::UnixSocket { path: _ } => {
            #[cfg(unix)]
            {
                log::info!("Creating Unix socket connection to {:?}", _path);
                let conn = crate::connections::unix::UnixSocketConnection::new(_path);
                Ok(Box::new(conn))
            }
            #[cfg(not(unix))]
            {
                Err(anyhow::anyhow!("Unix socket connection not available on this platform"))
            }
        }
        ConnectionInfo::Vsock { path: _ } => {
            #[cfg(unix)]
            {
                // Vsock is exposed as Unix socket on host side
                log::info!("Creating vsock connection (Unix socket) to {:?}", _path);
                let conn = crate::connections::unix::UnixSocketConnection::new(_path);
                Ok(Box::new(conn))
            }
            #[cfg(not(unix))]
            {
                Err(anyhow::anyhow!("Vsock connection not available on this platform"))
            }
        }
        ConnectionInfo::NamedPipe { name } => {
            #[cfg(target_os = "windows")]
            {
                log::info!("Creating named pipe connection to {}", name);
                let conn = crate::connections::pipe::NamedPipeConnection::new(name);
                Ok(Box::new(conn))
            }
            #[cfg(not(target_os = "windows"))]
            {
                Err(anyhow::anyhow!("Named pipe connection only available on Windows"))
            }
        }
        ConnectionInfo::TcpWithToken { addr, token } => {
            log::info!("Creating TCP+token connection to {}", addr);
            let conn = crate::connections::tcp::TcpTokenConnection::new(addr, token);
            Ok(Box::new(conn))
        }
    }
}

// ── 2. Health check polling ────────────────────────────────────────────────────

async fn run_health_check(
    launcher: Arc<Box<dyn VmLauncher>>,
    event_tx: mpsc::Sender<BackendEvent>,
) {
    let mut interval = interval(Duration::from_secs(10));
    // Skip the first immediate tick
    interval.tick().await;

    loop {
        interval.tick().await;

        match launcher.health_check().await {
            Ok(true) => {
                log::debug!("Health check passed");
            }
            Ok(false) => {
                log::warn!("Health check failed: daemon not responding");
                let _ = event_tx.send(BackendEvent::DaemonFailed(
                    "Daemon health check failed".to_string()
                )).await;
            }
            Err(e) => {
                log::error!("Health check error: {e}");
                let _ = event_tx.send(BackendEvent::DaemonFailed(
                    format!("Health check error: {e}")
                )).await;
            }
        }
    }
}

// ── 2. Git diff watcher ────────────────────────────────────────────────────────

async fn run_git_watcher(project_dir: String, event_tx: mpsc::Sender<BackendEvent>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    // The first tick fires immediately; skip it so we don't run before the
    // daemon has had a chance to make any changes.
    interval.tick().await;

    loop {
        interval.tick().await;

        if let Err(e) = poll_git_diffs(&project_dir, &event_tx).await {
            log::warn!("git diff poll failed: {e}");
        }
    }
}

async fn poll_git_diffs(
    project_dir: &str,
    event_tx: &mpsc::Sender<BackendEvent>,
) -> Result<()> {
    // List files that changed relative to HEAD.
    let name_output = tokio::process::Command::new("git")
        .args(["diff", "HEAD", "--name-only"])
        .current_dir(project_dir)
        .output()
        .await?;

    if !name_output.status.success() {
        // Not a git repo, no commits yet, or git not available — silently skip.
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&name_output.stdout);
    let changed_files: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();

    for path in changed_files {
        let diff_output = tokio::process::Command::new("git")
            .args(["diff", "HEAD", "--", path])
            .current_dir(project_dir)
            .output()
            .await?;

        if !diff_output.status.success() {
            continue;
        }

        let raw = String::from_utf8_lossy(&diff_output.stdout);
        if raw.trim().is_empty() {
            continue;
        }

        let diff = FileDiff::parse(path, &raw);
        if event_tx.send(BackendEvent::DiffUpdated(diff)).await.is_err() {
            // GUI has shut down.
            return Ok(());
        }
    }

    Ok(())
}

// ── 3. Command handler ─────────────────────────────────────────────────────────

async fn run_command_handler(
    mut command_rx: mpsc::Receiver<FrontendCommand>,
    event_tx: mpsc::Sender<BackendEvent>,
    connection: Arc<Mutex<Box<dyn DaemonConnection>>>,
) {
    while let Some(cmd) = command_rx.recv().await {
        match cmd {
            FrontendCommand::StartOrchestration { prompt } => {
                let tx = event_tx.clone();
                let conn = connection.clone();
                tokio::spawn(async move {
                    handle_start_orchestration(prompt, tx, conn).await;
                });
            }

            FrontendCommand::StopOrchestration => {
                // Best-effort: send a stop message and ignore errors
                let conn = connection.clone();
                tokio::spawn(async move {
                    if let Err(e) = send_stop_command(conn).await {
                        log::warn!("Failed to send stop command: {e}");
                    }
                });
            }

            FrontendCommand::SendFollowUp { content: _ } => {
                log::warn!("SendFollowUp not yet implemented");
            }
        }
    }
}

// ── 4. Socket communication ────────────────────────────────────────────────────

/// Send a stop command to the daemon
async fn send_stop_command(
    connection: Arc<Mutex<Box<dyn DaemonConnection>>>,
) -> Result<()> {
    let mut conn = connection.lock().await;
    
    // Connect if not already connected
    conn.connect().await?;
    
    // Send stop request
    let req = agentd_protocol::SocketRequest {
        id: uuid::Uuid::new_v4().to_string(),
        method: "stop".to_string(),
        params: serde_json::json!({}),
    };
    
    conn.send_request(req).await?;
    
    Ok(())
}

/// Send an orchestration request and read responses
async fn send_orchestration_request(
    prompt: String,
    connection: Arc<Mutex<Box<dyn DaemonConnection>>>,
    event_tx: &mpsc::Sender<BackendEvent>,
) -> Result<()> {
    let mut conn = connection.lock().await;
    
    // Connect with retry logic
    if let Err(e) = conn.connect().await {
        log::error!("Failed to connect to daemon: {e}");
        return Err(e);
    }
    
    // Send orchestration request
    let req = agentd_protocol::SocketRequest {
        id: uuid::Uuid::new_v4().to_string(),
        method: "orchestrate".to_string(),
        params: serde_json::json!({
            "prompt": prompt,
            "project": ".",
            "max_agents": 100,
        }),
    };
    
    conn.send_request(req).await?;
    
    // Read responses in a loop
    loop {
        match conn.recv_response().await {
            Ok(response) => {
                if let Some(event) = response_to_event(&response) {
                    if event_tx.send(event).await.is_err() {
                        // GUI has shut down
                        break;
                    }
                    
                    // Check if orchestration is complete
                    if matches!(response.result.get("type").and_then(|v| v.as_str()), Some("complete")) {
                        break;
                    }
                } else {
                    log::debug!("Unrecognized response: {:?}", response);
                }
            }
            Err(e) => {
                log::error!("Failed to receive response: {e}");
                return Err(e);
            }
        }
    }
    
    Ok(())
}

/// Map a SocketResponse to a BackendEvent
fn response_to_event(response: &agentd_protocol::SocketResponse) -> Option<BackendEvent> {
    let result = &response.result;
    let msg_type = result.get("type")?.as_str()?;

    match msg_type {
        "task_added" => {
            let id = result["id"].as_str()?.to_owned();
            let description = result["description"].as_str().unwrap_or("").to_owned();
            let sandbox = result["sandbox"].as_str().map(ToOwned::to_owned);
            let status = parse_task_status(&result["status"]);
            Some(BackendEvent::TaskAdded(Task { id, description, sandbox, status }))
        }

        "task_updated" => {
            let id = result["id"].as_str()?.to_owned();
            let status = parse_task_status(&result["status"]);
            Some(BackendEvent::TaskUpdated { id, status })
        }

        "agent_chunk" => {
            let chunk = result["content"].as_str()?.to_owned();
            Some(BackendEvent::AgentChunk(chunk))
        }

        "agent_message" => {
            let msg = result["content"].as_str()?.to_owned();
            Some(BackendEvent::AgentMessage(msg))
        }

        "complete" => Some(BackendEvent::OrchestrationComplete),

        "error" => {
            let msg = result["message"].as_str().unwrap_or("unknown error").to_owned();
            Some(BackendEvent::OrchestrationFailed(msg))
        }

        _ => None,
    }
}

fn parse_task_status(v: &serde_json::Value) -> TaskStatus {
    match v.as_str().unwrap_or("pending") {
        "running" => TaskStatus::Running,
        "complete" => TaskStatus::Complete,
        "failed" => TaskStatus::Failed(String::new()),
        _ => TaskStatus::Pending,
    }
}

// ── StartOrchestration handler ─────────────────────────────────────────────────

async fn handle_start_orchestration(
    prompt: String,
    event_tx: mpsc::Sender<BackendEvent>,
    connection: Arc<Mutex<Box<dyn DaemonConnection>>>,
) {
    // 1. Try to send the real orchestration request
    if let Err(e) = send_orchestration_request(prompt.clone(), connection, &event_tx).await {
        log::warn!("Could not deliver orchestrate command: {e}");
    }

    // 2. Simulated task stream — makes the UI functional during development
    //    before the real socket protocol is complete.
    simulate_task_stream(prompt, event_tx).await;
}

/// Emit a handful of synthetic events so that the GUI task panel and chat view
/// render correctly during development.
async fn simulate_task_stream(prompt: String, event_tx: mpsc::Sender<BackendEvent>) {
    let tasks = [
        Task {
            id: "t1".into(),
            description: format!("Analyse: {prompt}"),
            sandbox: Some("backend".into()),
            status: TaskStatus::Pending,
        },
        Task {
            id: "t2".into(),
            description: "Plan implementation steps".into(),
            sandbox: Some("backend".into()),
            status: TaskStatus::Pending,
        },
        Task {
            id: "t3".into(),
            description: "Write code".into(),
            sandbox: Some("backend".into()),
            status: TaskStatus::Pending,
        },
    ];

    // Announce all tasks first.
    for task in &tasks {
        if event_tx.send(BackendEvent::TaskAdded(task.clone())).await.is_err() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    }

    // ── t1: running → stream reply → complete ───────────────────────────────
    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t1".into(), status: TaskStatus::Running }
    );
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let reply_chunks = [
        "Understood. Analysing your request…\n",
        "Breaking the work into parallel tasks.\n",
        "Agents are spinning up inside isolated sandboxes.\n",
        "I will keep you updated as each task completes.\n",
    ];
    for chunk in reply_chunks {
        if event_tx.send(BackendEvent::AgentChunk(chunk.to_owned())).await.is_err() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(180)).await;
    }

    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t1".into(), status: TaskStatus::Complete }
    );

    // ── t2: running → complete ───────────────────────────────────────────────
    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t2".into(), status: TaskStatus::Running }
    );
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t2".into(), status: TaskStatus::Complete }
    );

    // ── t3: running → complete ───────────────────────────────────────────────
    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t3".into(), status: TaskStatus::Running }
    );
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t3".into(), status: TaskStatus::Complete }
    );

    let _ = event_tx.send(BackendEvent::OrchestrationComplete).await;
}
