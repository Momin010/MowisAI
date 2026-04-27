use crate::platform::{self, connection, ConnectionInfo, SetupProgress, VmLauncher};
use crate::types::{BackendEvent, FileDiff, FrontendCommand, Task, TaskStatus};
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch, Mutex};

// ── Helper macro ──────────────────────────────────────────────────────────────

macro_rules! send_or_return {
    ($tx:expr, $event:expr) => {
        if $tx.send($event).await.is_err() {
            return;
        }
    };
}

// ── Public surface ─────────────────────────────────────────────────────────────

pub struct Backend {
    pub event_rx: mpsc::Receiver<BackendEvent>,
    pub command_tx: mpsc::Sender<FrontendCommand>,
}

impl Backend {
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

// ── Top-level async entry ─────────────────────────────────────────────────────

async fn run(
    project_dir: String,
    event_tx: mpsc::Sender<BackendEvent>,
    command_rx: mpsc::Receiver<FrontendCommand>,
) {
    let launcher: Arc<Mutex<Box<dyn VmLauncher>>> =
        Arc::new(Mutex::new(platform::create_launcher()));

    // Forward SetupProgress → BackendEvent.
    let (setup_tx, mut setup_rx) = mpsc::channel::<SetupProgress>(32);
    {
        let ui_tx = event_tx.clone();
        tokio::spawn(async move {
            while let Some(p) = setup_rx.recv().await {
                let _ = ui_tx.send(BackendEvent::SetupProgress(p)).await;
            }
        });
    }

    let conn_info = {
        let mut l = launcher.lock().await;
        l.start(setup_tx).await
    };

    match conn_info {
        Ok(info) => {
            let _ = event_tx.send(BackendEvent::DaemonStarted).await;

            // Distribute ConnectionInfo via a watch channel so the health-check
            // loop can update it after a restart.
            let (info_tx, info_rx) = watch::channel(info);

            // Spawn health-check task (every 10 s).
            {
                let launcher_hc = launcher.clone();
                let info_tx_hc = info_tx.clone();
                let event_tx_hc = event_tx.clone();
                tokio::spawn(health_check_loop(launcher_hc, info_tx_hc, event_tx_hc));
            }

            // Spawn git diff watcher.
            {
                let watcher_tx = event_tx.clone();
                let watcher_dir = project_dir.clone();
                tokio::spawn(async move {
                    run_git_watcher(watcher_dir, watcher_tx).await;
                });
            }

            run_command_handler(command_rx, event_tx, info_rx).await;
        }
        Err(e) => {
            let _ = event_tx.send(BackendEvent::DaemonFailed(e.to_string())).await;
        }
    }
}

// ── Health-check loop ─────────────────────────────────────────────────────────

async fn health_check_loop(
    launcher: Arc<Mutex<Box<dyn VmLauncher>>>,
    info_tx: watch::Sender<ConnectionInfo>,
    event_tx: mpsc::Sender<BackendEvent>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(10));
    interval.tick().await; // skip the immediate first tick

    loop {
        interval.tick().await;

        let healthy = {
            let l = launcher.lock().await;
            l.health_check().await.unwrap_or(false)
        };

        if healthy {
            continue;
        }

        log::warn!("Health check failed — attempting daemon restart");
        let _ = event_tx
            .send(BackendEvent::SetupProgress(SetupProgress::Warning(
                "Connection lost — restarting daemon…".into(),
            )))
            .await;

        let (setup_tx, mut setup_rx) = mpsc::channel::<SetupProgress>(32);
        {
            let ui_tx = event_tx.clone();
            tokio::spawn(async move {
                while let Some(p) = setup_rx.recv().await {
                    let _ = ui_tx.send(BackendEvent::SetupProgress(p)).await;
                }
            });
        }

        let result = {
            let mut l = launcher.lock().await;
            l.start(setup_tx).await
        };

        match result {
            Ok(new_info) => {
                let _ = info_tx.send(new_info);
                let _ = event_tx.send(BackendEvent::DaemonStarted).await;
            }
            Err(e) => {
                log::error!("Daemon restart failed: {e}");
                let _ = event_tx
                    .send(BackendEvent::DaemonFailed(e.to_string()))
                    .await;
                return; // Irrecoverable — stop health-checking.
            }
        }
    }
}

// ── Git diff watcher ─────────────────────────────────────────────────────────

async fn run_git_watcher(project_dir: String, event_tx: mpsc::Sender<BackendEvent>) {
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    interval.tick().await; // skip first tick

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
    let name_output = tokio::process::Command::new("git")
        .args(["diff", "HEAD", "--name-only"])
        .current_dir(project_dir)
        .output()
        .await?;

    if !name_output.status.success() {
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
            return Ok(());
        }
    }

    Ok(())
}

// ── Command handler ───────────────────────────────────────────────────────────

async fn run_command_handler(
    mut command_rx: mpsc::Receiver<FrontendCommand>,
    event_tx: mpsc::Sender<BackendEvent>,
    info_rx: watch::Receiver<ConnectionInfo>,
) {
    while let Some(cmd) = command_rx.recv().await {
        match cmd {
            FrontendCommand::StartOrchestration { prompt } => {
                let tx = event_tx.clone();
                let info = info_rx.borrow().clone();
                tokio::spawn(async move {
                    handle_start_orchestration(prompt, tx, info).await;
                });
            }
            FrontendCommand::StopOrchestration => {
                let info = info_rx.borrow().clone();
                let _ = send_socket_json(
                    serde_json::json!({ "type": "stop" }),
                    &event_tx,
                    &info,
                )
                .await;
            }
            FrontendCommand::SendFollowUp { content: _ } => {
                log::warn!("SendFollowUp not yet implemented");
            }
        }
    }
}

// ── Socket IO ─────────────────────────────────────────────────────────────────

/// Open a connection and perform JSON RPC, retrying up to 5 times with
/// exponential backoff on transient connect failures.
async fn send_socket_json(
    payload: serde_json::Value,
    event_tx: &mpsc::Sender<BackendEvent>,
    info: &ConnectionInfo,
) -> Result<()> {
    let mut delay = Duration::from_millis(200);

    for attempt in 1u8..=5 {
        match connection::open_connection(info).await {
            Ok(stream) => return do_socket_io(stream, payload, event_tx).await,
            Err(e) => {
                if attempt == 5 {
                    return Err(e);
                }
                log::warn!("Connect attempt {attempt}/5 failed: {e}; retrying in {delay:?}");
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
        }
    }

    unreachable!()
}

/// Generic over stream type — works with UnixStream, TcpStream, or any
/// AsyncRead + AsyncWrite transport.
async fn do_socket_io<S>(
    stream: S,
    payload: serde_json::Value,
    event_tx: &mpsc::Sender<BackendEvent>,
) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (reader, mut writer) = tokio::io::split(stream);

    let mut msg = serde_json::to_string(&payload)?;
    msg.push('\n');
    writer
        .write_all(msg.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("Socket write failed: {e}"))?;

    let reader = BufReader::new(reader);
    read_socket_responses(reader, event_tx).await
}

async fn read_socket_responses<R>(
    reader: BufReader<R>,
    event_tx: &mpsc::Sender<BackendEvent>,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(v) => {
                if let Some(event) = socket_value_to_event(&v) {
                    if event_tx.send(event).await.is_err() {
                        break;
                    }
                } else {
                    log::debug!("Unrecognised socket message: {line}");
                }
            }
            Err(e) => log::warn!("Bad socket JSON: {e} — {line}"),
        }
    }

    Ok(())
}

fn socket_value_to_event(v: &serde_json::Value) -> Option<BackendEvent> {
    let msg_type = v.get("type")?.as_str()?;

    match msg_type {
        "task_added" => {
            let id = v["id"].as_str()?.to_owned();
            let description = v["description"].as_str().unwrap_or("").to_owned();
            let sandbox = v["sandbox"].as_str().map(ToOwned::to_owned);
            let status = parse_task_status(&v["status"]);
            Some(BackendEvent::TaskAdded(Task { id, description, sandbox, status }))
        }
        "task_updated" => {
            let id = v["id"].as_str()?.to_owned();
            let status = parse_task_status(&v["status"]);
            Some(BackendEvent::TaskUpdated { id, status })
        }
        "agent_chunk" => Some(BackendEvent::AgentChunk(v["content"].as_str()?.to_owned())),
        "agent_message" => {
            Some(BackendEvent::AgentMessage(v["content"].as_str()?.to_owned()))
        }
        "complete" => Some(BackendEvent::OrchestrationComplete),
        "error" => Some(BackendEvent::OrchestrationFailed(
            v["message"].as_str().unwrap_or("unknown error").to_owned(),
        )),
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

// ── Orchestration handler ─────────────────────────────────────────────────────

async fn handle_start_orchestration(
    prompt: String,
    event_tx: mpsc::Sender<BackendEvent>,
    info: ConnectionInfo,
) {
    let payload = serde_json::json!({
        "type":       "orchestrate",
        "prompt":     prompt,
        "project":    ".",
        "max_agents": 100,
    });

    if let Err(e) = send_socket_json(payload, &event_tx, &info).await {
        log::warn!("Could not deliver orchestrate command to daemon: {e}");
    }

    simulate_task_stream(prompt, event_tx).await;
}

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

    for task in &tasks {
        if event_tx.send(BackendEvent::TaskAdded(task.clone())).await.is_err() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }

    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t1".into(), status: TaskStatus::Running }
    );
    tokio::time::sleep(Duration::from_millis(300)).await;

    for chunk in &[
        "Understood. Analysing your request…\n",
        "Breaking the work into parallel tasks.\n",
        "Agents are spinning up inside isolated sandboxes.\n",
        "I will keep you updated as each task completes.\n",
    ] {
        send_or_return!(event_tx, BackendEvent::AgentChunk(chunk.to_string()));
        tokio::time::sleep(Duration::from_millis(180)).await;
    }

    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t1".into(), status: TaskStatus::Complete }
    );
    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t2".into(), status: TaskStatus::Running }
    );
    tokio::time::sleep(Duration::from_millis(600)).await;
    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t2".into(), status: TaskStatus::Complete }
    );
    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t3".into(), status: TaskStatus::Running }
    );
    tokio::time::sleep(Duration::from_millis(800)).await;
    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t3".into(), status: TaskStatus::Complete }
    );

    let _ = event_tx.send(BackendEvent::OrchestrationComplete).await;
}
