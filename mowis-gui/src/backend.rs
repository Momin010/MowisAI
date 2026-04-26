use crate::types::{BackendEvent, FileDiff, FrontendCommand, Task, TaskStatus};
use anyhow::Result;
use tokio::sync::mpsc;

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
                        // Cannot send the event because the channel requires async;
                        // best we can do is log and exit the thread.
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
    // 1. Launch daemon (or verify it is already running).
    let _child = match ensure_daemon(&event_tx).await {
        Ok(child) => child,
        Err(e) => {
            let _ = event_tx.send(BackendEvent::DaemonFailed(e.to_string())).await;
            // Continue — the user might fix the issue and retry later.
            None
        }
    };

    // 2. Git diff watcher (independent loop).
    let watcher_tx = event_tx.clone();
    let watcher_dir = project_dir.clone();
    tokio::spawn(async move {
        run_git_watcher(watcher_dir, watcher_tx).await;
    });

    // 3. Command handler (drives the main loop).
    run_command_handler(project_dir, command_rx, event_tx).await;
}

// ── 1. Daemon launcher ─────────────────────────────────────────────────────────

const SOCKET_PATH: &str = "/tmp/agentd.sock";

/// Returns `Ok(Some(child))` if we spawned a new process,
/// `Ok(None)` if the daemon was already running.
async fn ensure_daemon(
    event_tx: &mpsc::Sender<BackendEvent>,
) -> Result<Option<tokio::process::Child>> {
    // Fast path: socket already exists and is connectable.
    if socket_connectable().await {
        log::info!("agentd already running at {SOCKET_PATH}");
        let _ = event_tx.send(BackendEvent::DaemonStarted).await;
        return Ok(None);
    }

    // Locate the agentd binary.
    let bin = locate_binary("agentd");
    log::info!("Launching daemon: {}", bin.display());

    let child = tokio::process::Command::new(&bin)
        .args(["socket", "--path", SOCKET_PATH])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn agentd ({bin:?}): {e}"))?;

    // Wait up to 2 s for the socket to appear.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if tokio::fs::metadata(SOCKET_PATH).await.is_ok() && socket_connectable().await {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow::anyhow!(
                "agentd did not create socket at {SOCKET_PATH} within 2 seconds"
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let _ = event_tx.send(BackendEvent::DaemonStarted).await;
    Ok(Some(child))
}

/// Try a quick connection to the Unix socket; returns true on success.
async fn socket_connectable() -> bool {
    tokio::net::UnixStream::connect(SOCKET_PATH).await.is_ok()
}

/// Find the agentd binary, falling back to `./agentd` in the cwd.
fn locate_binary(name: &str) -> std::path::PathBuf {
    which::which(name).unwrap_or_else(|_| std::path::PathBuf::from(format!("./{name}")))
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
    let output = tokio::process::Command::new("git")
        .args(["diff", "HEAD", "--name-only"])
        .current_dir(project_dir)
        .output()
        .await?;

    if !output.status.success() {
        return Ok(()); // Not a git repo or no commits yet — silently skip.
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
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
    _project_dir: String,
    mut command_rx: mpsc::Receiver<FrontendCommand>,
    event_tx: mpsc::Sender<BackendEvent>,
) {
    while let Some(cmd) = command_rx.recv().await {
        match cmd {
            FrontendCommand::StartOrchestration { prompt } => {
                let tx = event_tx.clone();
                let p = prompt.clone();
                tokio::spawn(async move {
                    handle_start_orchestration(p, tx).await;
                });
            }

            FrontendCommand::StopOrchestration => {
                // Best-effort: send a stop message to the socket and ignore errors.
                let _ = send_socket_json(
                    serde_json::json!({ "type": "stop" }),
                    &event_tx,
                )
                .await;
            }

            FrontendCommand::SendFollowUp { content: _ } => {
                log::warn!("SendFollowUp not yet implemented");
            }
        }
    }
}

// ── 4. Socket communication ────────────────────────────────────────────────────

/// Write a single JSON message (followed by a newline) to the agentd socket.
async fn send_socket_json(
    payload: serde_json::Value,
    event_tx: &mpsc::Sender<BackendEvent>,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let mut stream = tokio::net::UnixStream::connect(SOCKET_PATH)
        .await
        .map_err(|e| anyhow::anyhow!("Cannot connect to {SOCKET_PATH}: {e}"))?;

    let mut msg = serde_json::to_string(&payload)?;
    msg.push('\n');

    stream
        .write_all(msg.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("Socket write failed: {e}"))?;

    // Read any immediate response lines.
    let reader = tokio::io::BufReader::new(stream);
    read_socket_responses(reader, event_tx).await?;

    Ok(())
}

/// Drain newline-delimited JSON responses from the socket until EOF or error,
/// converting known message shapes into `BackendEvent`s.
async fn read_socket_responses(
    reader: tokio::io::BufReader<tokio::net::UnixStream>,
    event_tx: &mpsc::Sender<BackendEvent>,
) -> Result<()> {
    use tokio::io::AsyncBufReadExt;

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
            Err(e) => {
                log::warn!("Failed to parse socket line as JSON: {e} — line: {line}");
            }
        }
    }

    Ok(())
}

/// Map a JSON value received from the socket into a `BackendEvent` where
/// the shape is recognised; returns `None` for unknown messages.
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

        "agent_chunk" => {
            let chunk = v["content"].as_str()?.to_owned();
            Some(BackendEvent::AgentChunk(chunk))
        }

        "agent_message" => {
            let msg = v["content"].as_str()?.to_owned();
            Some(BackendEvent::AgentMessage(msg))
        }

        "complete" => Some(BackendEvent::OrchestrationComplete),

        "error" => {
            let msg = v["message"].as_str().unwrap_or("unknown error").to_owned();
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

async fn handle_start_orchestration(prompt: String, event_tx: mpsc::Sender<BackendEvent>) {
    // 1. Forward the command to the socket (best-effort — the daemon might not
    //    be running yet, or the full protocol might not be wired).
    let payload = serde_json::json!({
        "type":       "orchestrate",
        "prompt":     prompt,
        "project":    ".",
        "max_agents": 100,
    });

    if let Err(e) = send_socket_json(payload, &event_tx).await {
        log::warn!("Could not deliver orchestrate command to socket: {e}");
        // Fall through to the simulated stream so the UI stays responsive
        // even before the real socket protocol is wired up.
    }

    // 2. Simulated task stream — keeps the UI working end-to-end during
    //    development, before the real agentd socket protocol is complete.
    simulate_task_stream(prompt, event_tx).await;
}

/// Emit a handful of synthetic events so that the GUI task/chat panels render
/// correctly during development.  These do NOT replace real socket events —
/// once the daemon produces `task_added` / `agent_chunk` messages they will
/// appear alongside (or instead of) these.
async fn simulate_task_stream(prompt: String, event_tx: mpsc::Sender<BackendEvent>) {
    // Synthetic task graph based on the user's prompt.
    let tasks = vec![
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

    // Mark t1 running then complete.
    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t1".into(), status: TaskStatus::Running }
    );
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    // Stream some agent chunks simulating a reply.
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

    // t2 running → complete.
    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t2".into(), status: TaskStatus::Running }
    );
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    send_or_return!(
        event_tx,
        BackendEvent::TaskUpdated { id: "t2".into(), status: TaskStatus::Complete }
    );

    // t3 running → complete.
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

/// Early-return helper for the simulation loop — avoids a cascade of
/// `if .is_err() { return; }` checks.
macro_rules! send_or_return {
    ($tx:expr, $event:expr) => {
        if $tx.send($event).await.is_err() {
            return;
        }
    };
}

// The macro must be defined before it is used, but Rust macros are
// textually scoped within a module so `send_or_return!` is only
// visible inside this file.  Re-export prevention is not needed for
// a private macro.
use send_or_return;
