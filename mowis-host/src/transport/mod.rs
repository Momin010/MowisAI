//! Guest VM transport layer.
//!
//! Two connection types are available:
//!
//! | Type            | Platform     | When to use                               |
//! |-----------------|--------------|-------------------------------------------|
//! | `VsockConnection` | Linux only | KVM/QEMU with `vhost-vsock-pci` device    |
//! | `TcpConnection`   | All         | QEMU user-mode network with port-forward  |
//!
//! Both expose the same `call` / `call_streaming` interface. The top-level
//! `Connection` enum wraps them for callers that don't care about the transport.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Result;
use dashmap::DashMap;
use mowis_protocol::{Envelope, Payload};
use tokio::sync::mpsc;

// ── Shared types ──────────────────────────────────────────────────────────────

type Inflight = Arc<DashMap<u64, mpsc::UnboundedSender<Payload>>>;

fn is_terminal(p: &Payload) -> bool {
    matches!(
        p,
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
            | Payload::CodebaseUploaded { .. }
            | Payload::HealthOk { .. }
            | Payload::InteractivePrompt { .. }
    )
}

// ── vsock connection (Linux only) ─────────────────────────────────────────────

#[cfg(target_os = "linux")]
pub struct VsockConnection {
    next_id: AtomicU64,
    writer: Arc<tokio::sync::Mutex<tokio::io::WriteHalf<tokio_vsock::VsockStream>>>,
    inflight: Inflight,
}

#[cfg(target_os = "linux")]
impl VsockConnection {
    async fn from_stream(stream: tokio_vsock::VsockStream) -> Self {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let (read_half, write_half) = tokio::io::split(stream);
        let inflight: Inflight = Arc::new(DashMap::new());
        let inflight_clone = inflight.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        tracing::info!("guest closed vsock connection");
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "vsock read loop error");
                        break;
                    }
                }
                if let Ok(env) = Envelope::from_line(&line) {
                    let terminal = is_terminal(&env.payload);
                    if let Some(tx) = inflight_clone.get(&env.id) {
                        let _ = tx.send(env.payload);
                    }
                    if terminal {
                        inflight_clone.remove(&env.id);
                    }
                }
            }
        });
        Self {
            next_id: AtomicU64::new(1),
            writer: Arc::new(tokio::sync::Mutex::new(write_half)),
            inflight,
        }
    }

    pub async fn call(&self, payload: Payload) -> Result<Payload> {
        let mut rx = self.call_streaming(payload).await?;
        rx.recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("connection closed before response"))
    }

    pub async fn call_streaming(&self, payload: Payload) -> Result<mpsc::UnboundedReceiver<Payload>> {
        use tokio::io::AsyncWriteExt;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::unbounded_channel();
        self.inflight.insert(id, tx);
        let line = Envelope::new(id, payload).to_line()?;
        let mut w = self.writer.lock().await;
        w.write_all(line.as_bytes()).await?;
        w.flush().await?;
        Ok(rx)
    }

    pub async fn ping(&self) -> Result<(String, u32)> {
        match self.call(Payload::Ping).await? {
            Payload::Pong { version, protocol } => Ok((version, protocol)),
            other => anyhow::bail!("unexpected payload to Ping: {other:?}"),
        }
    }
}

#[cfg(target_os = "linux")]
pub async fn connect_vsock(cid: u32, port: u32) -> Result<VsockConnection> {
    let addr = tokio_vsock::VsockAddr::new(cid, port);
    let stream = tokio_vsock::VsockStream::connect(addr)
        .await
        .with_context(|| format!("vsock connect cid={cid} port={port}"))?;
    Ok(VsockConnection::from_stream(stream).await)
}

// ── TCP connection (all platforms) ────────────────────────────────────────────

pub struct TcpConnection {
    next_id: AtomicU64,
    writer: Arc<tokio::sync::Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>,
    inflight: Inflight,
}

impl TcpConnection {
    async fn from_stream(stream: tokio::net::TcpStream) -> Self {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let (read_half, write_half) = tokio::io::split(stream);
        let inflight: Inflight = Arc::new(DashMap::new());
        let inflight_clone = inflight.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        tracing::info!("guest closed TCP connection");
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "TCP read loop error");
                        break;
                    }
                }
                if let Ok(env) = Envelope::from_line(&line) {
                    let terminal = is_terminal(&env.payload);
                    if let Some(tx) = inflight_clone.get(&env.id) {
                        let _ = tx.send(env.payload);
                    }
                    if terminal {
                        inflight_clone.remove(&env.id);
                    }
                }
            }
        });
        Self {
            next_id: AtomicU64::new(1),
            writer: Arc::new(tokio::sync::Mutex::new(write_half)),
            inflight,
        }
    }

    pub async fn call(&self, payload: Payload) -> Result<Payload> {
        let mut rx = self.call_streaming(payload).await?;
        rx.recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("connection closed before response"))
    }

    pub async fn call_streaming(&self, payload: Payload) -> Result<mpsc::UnboundedReceiver<Payload>> {
        use tokio::io::AsyncWriteExt;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::unbounded_channel();
        self.inflight.insert(id, tx);
        let line = Envelope::new(id, payload).to_line()?;
        let mut w = self.writer.lock().await;
        w.write_all(line.as_bytes()).await?;
        w.flush().await?;
        Ok(rx)
    }

    pub async fn ping(&self) -> Result<(String, u32)> {
        match self.call(Payload::Ping).await? {
            Payload::Pong { version, protocol } => Ok((version, protocol)),
            other => anyhow::bail!("unexpected payload to Ping: {other:?}"),
        }
    }
}

pub async fn connect_tcp(host: &str, port: u16) -> Result<TcpConnection> {
    let stream = tokio::net::TcpStream::connect((host, port))
        .await
        .with_context(|| format!("TCP connect to {host}:{port}"))?;
    Ok(TcpConnection::from_stream(stream).await)
}

// ── Unified Connection enum ───────────────────────────────────────────────────

/// Platform-agnostic VM connection. Use `connect_vsock` on Linux or
/// `connect_tcp` on any platform via QEMU port forwarding.
pub enum Connection {
    #[cfg(target_os = "linux")]
    Vsock(VsockConnection),
    Tcp(TcpConnection),
}

impl Connection {
    /// Send a one-shot request and await the first response.
    pub async fn call(&self, payload: Payload) -> Result<Payload> {
        match self {
            #[cfg(target_os = "linux")]
            Connection::Vsock(c) => c.call(payload).await,
            Connection::Tcp(c) => c.call(payload).await,
        }
    }

    /// Send a request and stream all responses for it.
    pub async fn call_streaming(
        &self,
        payload: Payload,
    ) -> Result<mpsc::UnboundedReceiver<Payload>> {
        match self {
            #[cfg(target_os = "linux")]
            Connection::Vsock(c) => c.call_streaming(payload).await,
            Connection::Tcp(c) => c.call_streaming(payload).await,
        }
    }

    /// Ping the guest; returns (version, protocol).
    pub async fn ping(&self) -> Result<(String, u32)> {
        match self {
            #[cfg(target_os = "linux")]
            Connection::Vsock(c) => c.ping().await,
            Connection::Tcp(c) => c.ping().await,
        }
    }
}

// ── Convenience constructors ─────────────────────────────────────────────────

/// Connect over vsock (Linux only, KVM/QEMU with `vhost-vsock-pci`).
#[cfg(target_os = "linux")]
pub async fn connect(cid: u32, port: u32) -> Result<Connection> {
    Ok(Connection::Vsock(connect_vsock(cid, port).await?))
}

/// Connect over TCP (all platforms, QEMU user-mode port forwarding).
pub async fn connect_tcp_conn(host: &str, port: u16) -> Result<Connection> {
    Ok(Connection::Tcp(connect_tcp(host, port).await?))
}

use anyhow::Context;
