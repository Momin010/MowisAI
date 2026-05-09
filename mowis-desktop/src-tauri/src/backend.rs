// backend.rs — BackendBridge
//
// Owns the platform launcher and connection info for reaching agentd.
// Opens a fresh TCP/socket connection per request, matching agentd's
// one-request-per-connection protocol.
//
// Responsibilities:
//   • Start the VM/daemon on first use
//   • Retry connection up to 5 times with exponential backoff
//   • Health-check loop every 30 s; auto-restart on failure (3 retries)
//   • Send JSON commands to agentd and stream back JSON events
//   • Emit SetupProgress events to the Tauri frontend during first boot

use crate::platform::{
    connection::{open_connection, ConnectionStream},
    create_launcher, ConnectionInfo, VmLauncher,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch, Mutex};
use tokio::time::sleep;

// ── Setup progress events (emitted to the UI during first boot) ───────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupProgress {
    pub stage: String,   // "detecting" | "installing" | "booting" | "ready" | "error"
    pub message: String,
    pub pct: u8,         // 0..100
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,  // raw command output / response
    #[serde(default = "default_kind")]
    pub kind: String,    // "info" | "command" | "output" | "success" | "error" | "warning"
    pub timestamp: u64,  // millis since epoch
}

fn default_kind() -> String { "info".into() }

impl SetupProgress {
    pub fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

// ── Connection state broadcast ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionState {
    pub connected: bool,
    pub launcher: String,
    pub addr: String,
}

// ── BackendBridge ─────────────────────────────────────────────────────────────

pub struct BackendBridge {
    launcher: Box<dyn VmLauncher>,
    conn_info: Mutex<Option<ConnectionInfo>>,
    /// Per-request stream for long-lived operations (orchestrate streaming).
    /// Not used for the initial connection — each send/recv opens a fresh stream.
    active_stream: Mutex<Option<ConnectionStream>>,

    // Broadcast channel for connection state changes.
    pub state_tx: watch::Sender<ConnectionState>,
    pub state_rx: watch::Receiver<ConnectionState>,

    // Channel for setup-progress events (polled by Tauri commands).
    pub progress_tx: mpsc::Sender<SetupProgress>,
    pub progress_rx: Mutex<mpsc::Receiver<SetupProgress>>,
}

impl BackendBridge {
    pub fn new() -> Arc<Self> {
        let launcher = create_launcher();
        let (state_tx, state_rx) = watch::channel(ConnectionState {
            connected: false,
            launcher: "pending".into(),
            addr: String::new(),
        });
        let (progress_tx, progress_rx) = mpsc::channel(128);

        Arc::new(Self {
            launcher,
            conn_info: Mutex::new(None),
            active_stream: Mutex::new(None),
            state_tx,
            state_rx,
            progress_tx,
            progress_rx: Mutex::new(progress_rx),
        })
    }

    // ── Startup ───────────────────────────────────────────────────────────────

    /// Boot the runtime and connect. Called once on app startup.
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        self.emit_detail("detecting", "Detecting runtime environment…", 5, "info", None).await;
        self.emit_detail("detecting", &format!("Platform: {} / {}", std::env::consts::OS, std::env::consts::ARCH), 6, "info", None).await;
        self.emit_detail("detecting", &format!("Launcher: {}", self.launcher.name()), 8, "info", None).await;

        let info = match self.connect_with_retry(5).await {
            Ok(info) => info,
            Err(e) => {
                // Surface the full error chain to the frontend so the user can read it.
                let msg = format!("{:#}", e);
                self.emit_detail("error", &msg, 0, "error", None).await;
                return Err(e);
            }
        };

        let addr = info.tcp_addr.as_deref()
            .or(info.socket_path.as_ref().map(|p| p.to_str().unwrap_or("")))
            .unwrap_or("");
        self.emit_detail("ready", "Bridge connected", 95, "success", Some(format!("Address: {}", addr))).await;
        self.emit_detail("ready", "Connected to agentd", 100, "success", None).await;

        *self.conn_info.lock().await = Some(info.clone());
        let _ = self.state_tx.send(ConnectionState {
            connected: true,
            launcher: self.launcher.name().into(),
            addr: info.tcp_addr.or(
                info.socket_path.map(|p| p.display().to_string())
            ).unwrap_or_default(),
        });

        // Spawn health-check loop in the background.
        let bridge = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            bridge.health_loop().await;
        });

        Ok(())
    }

    // ── Retry logic ───────────────────────────────────────────────────────────

    /// Try to start the launcher and open a connection, retrying on failure.
    async fn connect_with_retry(self: &Arc<Self>, max_attempts: u32) -> Result<ConnectionInfo> {
        let mut delay = Duration::from_secs(1);

        for attempt in 1..=max_attempts {
            self.emit_detail(
                "booting",
                &format!("Attempt {attempt}/{max_attempts}: launching {}…", self.launcher.name()),
                (attempt * 15).min(80) as u8,
                "info",
                None,
            ).await;

            match self.launcher.start(Some(self.progress_tx.clone())).await {
                Ok(info) => {
                    self.emit_detail(
                        "booting",
                        "Launcher started, verifying connection…",
                        85,
                        "info",
                        None,
                    ).await;
                    // Verify connectivity by opening (and immediately dropping) a test stream.
                    // We don't keep a persistent connection because agentd uses a
                    // one-request-per-connection protocol — each send() opens a fresh stream.
                    match self.open_stream(&info).await {
                        Ok(_test_stream) => {
                            return Ok(info);
                        }
                        Err(e) => {
                            log::warn!("Connection attempt {attempt} failed: {e}");
                            self.emit_detail(
                                "installing",
                                &format!("Connection attempt {attempt} failed"),
                                (attempt * 10).min(60) as u8,
                                "error",
                                Some(format!("{:#}", e)),
                            ).await;
                        }
                    }
                }
                Err(e) => {
                    // Use {:#} to include the full anyhow error chain with context.
                    let full = format!("{:#}", e);
                    log::warn!("Launcher start attempt {attempt} failed: {full}");
                    self.emit_detail(
                        "installing",
                        &format!("Setup attempt {attempt}/{max_attempts} failed"),
                        (attempt * 10).min(60) as u8,
                        "error",
                        Some(full),
                    ).await;
                }
            }

            if attempt < max_attempts {
                self.emit_detail(
                    "booting",
                    &format!("Retrying in {}s…", delay.as_secs()),
                    (attempt * 10).min(60) as u8,
                    "warning",
                    None,
                ).await;
                sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(10));
            }
        }

        anyhow::bail!(
            "Failed to connect to agentd after {max_attempts} attempts. \
             Check that WSL2 / QEMU / agentd are properly configured."
        )
    }

    async fn open_stream(&self, info: &ConnectionInfo) -> Result<ConnectionStream> {
        open_connection(info).await.context("open connection stream")
    }

    // ── Health-check loop ─────────────────────────────────────────────────────

    async fn health_loop(self: Arc<Self>) {
        loop {
            sleep(Duration::from_secs(30)).await;

            match self.launcher.health_check().await {
                Ok(true) => {} // all good
                Ok(false) | Err(_) => {
                    log::warn!("[{}] Health check failed — restarting", self.launcher.name());
                    let _ = self.state_tx.send(ConnectionState {
                        connected: false,
                        launcher: self.launcher.name().into(),
                        addr: String::new(),
                    });

                    // Attempt to reconnect.
                    match self.connect_with_retry(3).await {
                        Ok(info) => {
                            log::info!("[{}] Reconnected after failure", self.launcher.name());
                            *self.conn_info.lock().await = Some(info.clone());
                            let _ = self.state_tx.send(ConnectionState {
                                connected: true,
                                launcher: self.launcher.name().into(),
                                addr: info.tcp_addr.unwrap_or_default(),
                            });
                        }
                        Err(e) => {
                            log::error!("Reconnect failed: {e}");
                        }
                    }
                }
            }
        }
    }

    // ── Send / receive ────────────────────────────────────────────────────────

    /// Open a fresh connection to agentd.
    async fn fresh_stream(&self) -> Result<ConnectionStream> {
        let info = self.conn_info.lock().await;
        let info = info.as_ref().context("not connected to daemon")?;
        open_connection(info).await.context("open fresh connection to agentd")
    }

    /// Send a JSON command to agentd on a fresh connection.
    /// The connection is stored as the active stream so subsequent
    /// `recv_next` calls read from the same session.
    pub async fn send(&self, payload: Value) -> Result<()> {
        let mut stream = self.fresh_stream().await?;
        stream.send_json(&payload).await?;
        *self.active_stream.lock().await = Some(stream);
        Ok(())
    }

    /// Read the next JSON event from the active agentd session.
    pub async fn recv_next(&self) -> Result<Option<Value>> {
        let mut guard = self.active_stream.lock().await;
        let stream = guard.as_mut().context("no active agentd session")?;
        stream.recv_json().await
    }

    /// Read lines in a loop, calling `callback` for each JSON event until
    /// the connection closes or callback returns false.
    pub async fn stream_events<F>(&self, mut callback: F) -> Result<()>
    where
        F: FnMut(Value) -> bool + Send,
    {
        let mut guard = self.active_stream.lock().await;
        let stream = guard.as_mut().context("no active agentd session")?;
        loop {
            match stream.recv_json().await? {
                None => break,
                Some(v) => {
                    if !callback(v) {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    // ── Progress helpers ──────────────────────────────────────────────────────

    async fn emit_progress(&self, stage: &str, message: &str, pct: u8) {
        let _ = self.progress_tx.send(SetupProgress {
            stage: stage.into(),
            message: message.into(),
            pct,
            detail: None,
            kind: "info".into(),
            timestamp: SetupProgress::now_millis(),
        }).await;
    }

    /// Emit a detailed log line (command, output, success, error, warning, info).
    pub async fn emit_detail(&self, stage: &str, message: &str, pct: u8, kind: &str, detail: Option<String>) {
        let _ = self.progress_tx.send(SetupProgress {
            stage: stage.into(),
            message: message.into(),
            pct,
            detail,
            kind: kind.into(),
            timestamp: SetupProgress::now_millis(),
        }).await;
    }

    pub fn is_connected(&self) -> bool {
        self.state_rx.borrow().connected
    }

    /// Retrieve diagnostic logs from the Linux environment.
    pub async fn read_logs(&self) -> String {
        self.launcher.read_logs().await
    }
}
