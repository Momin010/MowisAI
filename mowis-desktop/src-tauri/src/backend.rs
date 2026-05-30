// backend.rs — OrchBridge
//
// Thin bridge struct that owns the setup-progress and connection-state
// channels used by the UI. No VM launcher, no agentd socket.
//
// OS Security mode (future): add a VmHandle field here; `start_vm()` will
// boot Alpine via mowis-host::vmm, emit SetupProgress events, then set
// connected = true. Tool calls will route to mowis-executor over vsock.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, watch, Mutex};

// ── Setup progress events ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupProgress {
    pub stage: String,
    pub message: String,
    pub pct: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub timestamp: u64,
}

fn default_kind() -> String {
    "info".into()
}

impl SetupProgress {
    pub fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

// ── Connection state ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionState {
    pub connected: bool,
    pub launcher: String,
    pub addr: String,
}

// ── OrchBridge ────────────────────────────────────────────────────────────────

pub struct OrchBridge {
    pub state_tx: watch::Sender<ConnectionState>,
    pub state_rx: watch::Receiver<ConnectionState>,

    pub progress_tx: mpsc::Sender<SetupProgress>,
    pub progress_rx: Mutex<mpsc::Receiver<SetupProgress>>,
}

impl OrchBridge {
    pub fn new() -> Arc<Self> {
        let (state_tx, state_rx) = watch::channel(ConnectionState {
            connected: false,
            launcher: "local".into(),
            addr: String::new(),
        });
        let (progress_tx, progress_rx) = mpsc::channel(128);

        Arc::new(Self {
            state_tx,
            state_rx,
            progress_tx,
            progress_rx: Mutex::new(progress_rx),
        })
    }

    /// Mark the bridge as ready. Call once on startup (or after VM boot in
    /// OS Security mode).
    pub fn set_ready(&self) {
        let _ = self.state_tx.send(ConnectionState {
            connected: true,
            launcher: "local".into(),
            addr: "in-process".into(),
        });
    }

    pub fn is_connected(&self) -> bool {
        self.state_rx.borrow().connected
    }

    pub async fn emit_detail(
        &self,
        stage: &str,
        message: &str,
        pct: u8,
        kind: &str,
        detail: Option<String>,
    ) {
        let _ = self
            .progress_tx
            .send(SetupProgress {
                stage: stage.into(),
                message: message.into(),
                pct,
                detail,
                kind: kind.into(),
                timestamp: SetupProgress::now_millis(),
            })
            .await;
    }

    pub async fn read_logs(&self) -> String {
        "Local mode — no VM logs.".into()
    }
}
