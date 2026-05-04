// types.rs — Core types shared across all platform launchers

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::mpsc;

// ── Connection descriptor returned by VmLauncher::start() ────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub kind: ConnectionKind,
    pub socket_path: Option<PathBuf>,
    pub tcp_addr: Option<String>,
    pub pipe_name: Option<String>,
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConnectionKind {
    UnixSocket,
    NamedPipe,
    TcpWithToken,
}

// ── Progress events (emitted during boot) ────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootProgress {
    pub stage: String,
    pub message: String,
    pub pct: u8,
    pub kind: String,
    pub detail: Option<String>,
}

pub type ProgressSender = mpsc::Sender<BootProgress>;

// ── Platform launcher trait ───────────────────────────────────────────────────

#[async_trait]
pub trait VmLauncher: Send + Sync {
    async fn start(&self, progress: Option<ProgressSender>) -> Result<ConnectionInfo>;
    async fn stop(&self) -> Result<()>;
    async fn health_check(&self) -> Result<bool>;
    fn name(&self) -> &str;
    async fn read_logs(&self) -> String {
        "Log collection not available.".into()
    }
}

// ── Helper: emit a progress event ────────────────────────────────────────────

pub async fn emit(
    pw: &Option<ProgressSender>,
    stage: &str,
    message: &str,
    pct: u8,
    kind: &str,
    detail: Option<String>,
) {
    let event = BootProgress {
        stage: stage.into(),
        message: message.into(),
        pct,
        kind: kind.into(),
        detail,
    };
    // Print to terminal immediately
    let prefix = match kind {
        "command" => "▶",
        "output" => " ",
        "success" => "✓",
        "error" => "✗",
        "warning" => "⚠",
        _ => "•",
    };
    let detail_str = event.detail.as_deref().unwrap_or("");
    if detail_str.is_empty() {
        eprintln!("  {:>3}% [{}] {} {}", pct, stage, prefix, message);
    } else {
        eprintln!("  {:>3}% [{}] {} {}", pct, stage, prefix, message);
        for line in detail_str.lines().take(20) {
            eprintln!("        │ {}", line);
        }
    }
    if let Some(tx) = pw {
        let _ = tx.send(event).await;
    }
}
