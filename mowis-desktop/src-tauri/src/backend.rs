// backend.rs — BackendBridge
//
// Owns the platform launcher and one live ConnectionStream.
// Responsibilities:
//   • Start the VM/daemon on first use
//   • Retry connection up to 5 times with exponential backoff
//   • Health-check loop every 10 s; auto-restart on failure
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
    stream: Mutex<Option<ConnectionStream>>,

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
        let (progress_tx, progress_rx) = mpsc::channel(32);

        Arc::new(Self {
            launcher,
            conn_info: Mutex::new(None),
            stream: Mutex::new(None),
            state_tx,
            state_rx,
            progress_tx,
            progress_rx: Mutex::new(progress_rx),
        })
    }

    // ── Startup ───────────────────────────────────────────────────────────────

    /// Boot the runtime and connect. Called once on app startup.
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        self.emit_progress("detecting", "Detecting runtime environment…", 5).await;

        let info = self.connect_with_retry(5).await?;

        self.emit_progress("ready", "Connected to agentd", 100).await;

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
            self.emit_progress(
                "booting",
                &format!("Starting Linux environment (attempt {attempt}/{max_attempts})…"),
                (attempt * 15).min(80) as u8,
            ).await;

            match self.launcher.start().await {
                Ok(info) => {
                    match self.open_stream(&info).await {
                        Ok(stream) => {
                            *self.stream.lock().await = Some(stream);
                            return Ok(info);
                        }
                        Err(e) => {
                            log::warn!("Connection attempt {attempt} failed: {e}");
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Launcher start attempt {attempt} failed: {e}");
                    self.emit_progress(
                        "installing",
                        &format!("Setting up environment… ({e})"),
                        (attempt * 10).min(60) as u8,
                    ).await;
                }
            }

            if attempt < max_attempts {
                sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(16));
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
            sleep(Duration::from_secs(10)).await;

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
                    match self.connect_with_retry(5).await {
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

    /// Send a JSON command to agentd.
    pub async fn send(&self, payload: Value) -> Result<()> {
        let mut guard = self.stream.lock().await;
        let stream = guard.as_mut().context("not connected to daemon")?;
        stream.send_json(&payload).await
    }

    /// Read the next JSON event from agentd (non-blocking poll).
    pub async fn recv_next(&self) -> Result<Option<Value>> {
        let mut guard = self.stream.lock().await;
        let stream = guard.as_mut().context("not connected to daemon")?;
        stream.recv_json().await
    }

    /// Read lines in a loop, calling `callback` for each JSON event until
    /// the connection closes or callback returns false.
    pub async fn stream_events<F>(&self, mut callback: F) -> Result<()>
    where
        F: FnMut(Value) -> bool + Send,
    {
        let mut guard = self.stream.lock().await;
        let stream = guard.as_mut().context("not connected to daemon")?;
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

    // ── Progress helper ───────────────────────────────────────────────────────

    async fn emit_progress(&self, stage: &str, message: &str, pct: u8) {
        let _ = self.progress_tx.send(SetupProgress {
            stage: stage.into(),
            message: message.into(),
            pct,
        }).await;
    }

    pub fn is_connected(&self) -> bool {
        self.state_rx.borrow().connected
    }
}
