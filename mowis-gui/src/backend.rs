use crate::platform::{self, ConnectionTarget, DaemonPlatform, SetupProgress};
use crate::types::{BackendEvent, FileDiff, FrontendCommand, Task, TaskStatus};
use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

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
    let mut daemon = platform::create_platform();

    // Forward setup progress to the UI as DaemonSetup events.
    let (setup_tx, mut setup_rx) = mpsc::channel::<SetupProgress>(32);
    let ui_tx = event_tx.clone();
    tokio::spawn(async move {
        while let Some(p) = setup_rx.recv().await {
            let _ = ui_tx.send(BackendEvent::SetupProgress(p)).await;
        }
    });

    match daemon.ensure_running(setup_tx).await {
        Ok(()) => {
            let _ = event_tx.send(BackendEvent::DaemonStarted).await;
        }
        Err(e) => {
            let _ = event_tx
                .send(BackendEvent::DaemonFailed(e.to_string()))
                .await;
        }
    }

    // Git diff watcher.
    let watcher_tx = event_tx.clone();
    let watcher_dir = project_dir.clone();
    let target = daemon.connection_target();
    tokio::spawn(async move {
        run_git_watcher(watcher_dir, watcher_tx).await;
    });

    run_command_handler(command_rx, event_tx, daemon, target).await;
}

// ── Git diff watcher ─────────────────────────────────────────────────────────

async fn run_git_watcher(project_dir: String, event_tx: mpsc::Sender<BackendEvent>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    interval.tick().await; // skip immediate first tick

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
    daemon: Box<dyn DaemonPlatform>,
    target: ConnectionTarget,
) {
    while let Some(cmd) = command_rx.recv().await {
        match cmd {
            FrontendCommand::StartOrchestration { prompt } => {
                let tx = event_tx.clone();
                let t = target.clone();
                tokio::spawn(async move {
                    handle_start_orchestration(prompt, tx, t).await;
                });
            }
            FrontendCommand::StopOrchestration => {
                let _ = send_socket_json(
                    serde_json::json!({ "type": "stop" }),
                    &event_tx,
                    &target,
                )
                .await;
            }
            FrontendCommand::SendFollowUp { content: _ } => {
                log::warn!("SendFollowUp not yet implemented");
            }
        }
    }
    drop(daemon);
}

// ── Socket IO ─────────────────────────────────────────────────────────────────

/// Open a stream to the daemon — UnixStream on Linux/macOS-native,
/// TcpStream when the daemon runs inside a VM on macOS/Windows.
async fn send_socket_json(
    payload: serde_json::Value,
    event_tx: &mpsc::Sender<BackendEvent>,
    target: &ConnectionTarget,
) -> Result<()> {
    match target {
        #[cfg(unix)]
        ConnectionTarget::UnixSocket(path) => {
            let stream = tokio::net::UnixStream::connect(path)
                .await
                .map_err(|e| anyhow::anyhow!("Cannot connect to {path}: {e}"))?;
            do_socket_io(stream, payload, event_tx).await
        }
        ConnectionTarget::Tcp { port } => {
            let stream = tokio::net::TcpStream::connect(("127.0.0.1", *port))
                .await
                .map_err(|e| anyhow::anyhow!("Cannot connect to 127.0.0.1:{port}: {e}"))?;
            do_socket_io(stream, payload, event_tx).await
        }
    }
}

/// Generic over stream type — works with UnixStream, TcpStream, or any
/// type that implements AsyncRead + AsyncWrite.
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
        "agent_chunk" => Some(BackendEvent::AgentChunk(
            v["content"].as_str()?.to_owned(),
        )),
        "agent_message" => Some(BackendEvent::AgentMessage(
            v["content"].as_str()?.to_owned(),
        )),
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
    target: ConnectionTarget,
) {
    let payload = serde_json::json!({
        "type":       "orchestrate",
        "prompt":     prompt,
        "project":    ".",
        "max_agents": 100,
    });

    if let Err(e) = send_socket_json(payload, &event_tx, &target).await {
        log::warn!("Could not deliver orchestrate command to socket: {e}");
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
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    }

    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t1".into(), status: TaskStatus::Running }
    );
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    for chunk in &[
        "Understood. Analysing your request…\n",
        "Breaking the work into parallel tasks.\n",
        "Agents are spinning up inside isolated sandboxes.\n",
        "I will keep you updated as each task completes.\n",
    ] {
        send_or_return!(event_tx, BackendEvent::AgentChunk(chunk.to_string()));
        tokio::time::sleep(std::time::Duration::from_millis(180)).await;
    }

    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t1".into(), status: TaskStatus::Complete }
    );

    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t2".into(), status: TaskStatus::Running }
    );
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t2".into(), status: TaskStatus::Complete }
    );

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
