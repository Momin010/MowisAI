//! vsock client transport.
//!
//! Wraps the host-side vsock stream and exposes a typed request/response
//! interface over `mowis-protocol`. Multiple responses can come back for a
//! single request (e.g. stdout / stderr lines followed by an `ExitCode`); the
//! [`Connection::call_streaming`] API surfaces that as an `mpsc::Receiver`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use dashmap::DashMap;
use mowis_protocol::{Envelope, Payload};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};

// dashmap is intentionally a hard dep on this module (used both Linux and
// non-Linux test builds) so we re-export under a feature-free path.
#[cfg(target_os = "linux")]
type Stream = tokio_vsock::VsockStream;

#[cfg(target_os = "linux")]
pub async fn connect(cid: u32, port: u32) -> Result<Connection> {
    let addr = tokio_vsock::VsockAddr::new(cid, port);
    let stream = tokio_vsock::VsockStream::connect(addr)
        .await
        .with_context(|| format!("vsock connect cid={cid} port={port}"))?;
    Ok(Connection::from_stream(stream))
}

#[cfg(not(target_os = "linux"))]
pub async fn connect(_cid: u32, _port: u32) -> Result<Connection> {
    anyhow::bail!("vsock transport not implemented on this platform yet")
}

/// Channel of streaming responses for one in-flight request.
type Inflight = Arc<DashMap<u64, mpsc::UnboundedSender<Payload>>>;

pub struct Connection {
    next_id: AtomicU64,
    #[cfg(target_os = "linux")]
    writer: Arc<Mutex<tokio::io::WriteHalf<Stream>>>,
    inflight: Inflight,
}

impl Connection {
    #[cfg(target_os = "linux")]
    fn from_stream(stream: Stream) -> Self {
        let (read_half, write_half) = tokio::io::split(stream);
        let inflight: Inflight = Arc::new(DashMap::new());
        let inflight_clone = inflight.clone();
        tokio::spawn(async move {
            if let Err(e) = read_loop(read_half, inflight_clone).await {
                tracing::warn!(error = %e, "vsock read loop terminated");
            }
        });
        Self {
            next_id: AtomicU64::new(1),
            writer: Arc::new(Mutex::new(write_half)),
            inflight,
        }
    }

    /// Send a one-shot request expecting a single response payload.
    pub async fn call(&self, payload: Payload) -> Result<Payload> {
        let mut rx = self.call_streaming(payload).await?;
        rx.recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("connection closed before response"))
    }

    /// Send a request and stream all responses sharing its id back through
    /// the returned receiver. The receiver closes when an `ExitCode`,
    /// `ToolResult`, or `Error` payload arrives, or when the connection
    /// drops.
    pub async fn call_streaming(
        &self,
        payload: Payload,
    ) -> Result<mpsc::UnboundedReceiver<Payload>> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::unbounded_channel();
        self.inflight.insert(id, tx);
        let env = Envelope::new(id, payload);
        let line = env.to_line()?;
        #[cfg(target_os = "linux")]
        {
            let mut w = self.writer.lock().await;
            w.write_all(line.as_bytes()).await?;
            w.flush().await?;
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = line;
            self.inflight.remove(&id);
            anyhow::bail!("vsock not supported on this platform");
        }
        Ok(rx)
    }

    /// Convenience: ping the guest, returning (version, protocol).
    pub async fn ping(&self) -> Result<(String, u32)> {
        match self.call(Payload::Ping).await? {
            Payload::Pong { version, protocol } => Ok((version, protocol)),
            other => anyhow::bail!("unexpected payload to Ping: {other:?}"),
        }
    }
}

#[cfg(target_os = "linux")]
async fn read_loop(
    read_half: tokio::io::ReadHalf<Stream>,
    inflight: Inflight,
) -> Result<()> {
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            tracing::info!("guest closed vsock connection");
            return Ok(());
        }
        let env = Envelope::from_line(&line)?;
        let terminal = matches!(
            env.payload,
            Payload::ExitCode { .. }
                | Payload::ToolResult { .. }
                | Payload::Error { .. }
                | Payload::Pong { .. }
                | Payload::SandboxCreated { .. }
                | Payload::SandboxDestroyed { .. }
                | Payload::SandboxList { .. }
                | Payload::AgentOverlayCreated { .. }
                | Payload::AgentOverlayMerged { .. }
                | Payload::AgentOverlayDiscarded { .. }
        );
        if let Some(tx) = inflight.get(&env.id) {
            let _ = tx.send(env.payload);
        }
        if terminal {
            inflight.remove(&env.id);
        }
    }
}
