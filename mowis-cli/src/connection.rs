// connection.rs — Unified connection abstraction with full debug logging

use crate::types::{ConnectionInfo, ConnectionKind};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::{timeout, Duration};

#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::net::TcpStream;

// ── Unified stream ────────────────────────────────────────────────────────────

pub enum ConnectionStream {
    #[cfg(unix)]
    Unix(BufReader<UnixStream>),
    Tcp(BufReader<TcpStream>),
    #[cfg(windows)]
    Pipe(BufReader<tokio::net::windows::named_pipe::NamedPipeClient>),
}

impl ConnectionStream {
    pub async fn send_json(&mut self, value: &Value) -> Result<()> {
        let mut line = serde_json::to_string(value)?;
        line.push('\n');
        log::debug!("[conn] >>> {}", line.trim());
        match self {
            #[cfg(unix)]
            Self::Unix(r) => r.get_mut().write_all(line.as_bytes()).await?,
            Self::Tcp(r) => r.get_mut().write_all(line.as_bytes()).await?,
            #[cfg(windows)]
            Self::Pipe(r) => r.get_mut().write_all(line.as_bytes()).await?,
        }
        Ok(())
    }

    pub async fn recv_line(&mut self) -> Result<Option<String>> {
        let mut buf = String::new();
        let n = match self {
            #[cfg(unix)]
            Self::Unix(r) => r.read_line(&mut buf).await?,
            Self::Tcp(r) => r.read_line(&mut buf).await?,
            #[cfg(windows)]
            Self::Pipe(r) => r.read_line(&mut buf).await?,
        };
        if n == 0 {
            log::debug!("[conn] EOF (0 bytes read)");
            return Ok(None);
        }
        log::debug!("[conn] <<< {}", buf.trim());
        Ok(Some(buf.trim_end().to_owned()))
    }

    pub async fn recv_json(&mut self) -> Result<Option<Value>> {
        while let Some(line) = self.recv_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(&line)
                .with_context(|| format!("[conn] parse JSON failed: {line}"))?;
            return Ok(Some(v));
        }
        Ok(None)
    }
}

// ── Connection factory ────────────────────────────────────────────────────────

pub async fn open_connection(info: &ConnectionInfo) -> Result<ConnectionStream> {
    log::info!("[conn] Opening {:?} connection…", info.kind);
    log::debug!("[conn] ConnectionInfo: {:?}", info);

    match info.kind {
        ConnectionKind::UnixSocket => {
            #[cfg(unix)]
            {
                let path = info
                    .socket_path
                    .as_ref()
                    .context("UnixSocket needs socket_path")?;
                log::info!("[conn] Connecting to Unix socket: {}", path.display());
                let stream = UnixStream::connect(path)
                    .await
                    .with_context(|| format!("connect unix {}", path.display()))?;
                log::info!("[conn] Unix socket connected");
                Ok(ConnectionStream::Unix(BufReader::new(stream)))
            }
            #[cfg(not(unix))]
            bail!("UnixSocket connections are only available on Linux")
        }

        ConnectionKind::TcpWithToken => {
            let addr = info
                .tcp_addr
                .as_deref()
                .context("TcpWithToken needs tcp_addr")?;
            log::info!("[conn] Connecting to TCP: {}", addr);
            let stream = timeout(Duration::from_secs(10), TcpStream::connect(addr))
                .await
                .context("TCP connect timed out (10s)")?
                .with_context(|| format!("TCP connect {addr}"))?;
            log::info!("[conn] TCP connected to {}", addr);
            Ok(ConnectionStream::Tcp(BufReader::new(stream)))
        }

        ConnectionKind::NamedPipe => {
            #[cfg(windows)]
            {
                let pipe = info
                    .pipe_name
                    .as_deref()
                    .context("NamedPipe needs pipe_name")?;
                log::info!("[conn] Opening named pipe: {}", pipe);
                use tokio::net::windows::named_pipe::ClientOptions;
                let client = ClientOptions::new()
                    .open(pipe)
                    .with_context(|| format!("open named pipe {pipe}"))?;
                log::info!("[conn] Named pipe connected: {}", pipe);
                Ok(ConnectionStream::Pipe(BufReader::new(client)))
            }
            #[cfg(not(windows))]
            bail!("NamedPipe connections are only available on Windows")
        }
    }
}

// ── TCP probe ─────────────────────────────────────────────────────────────────

pub async fn is_tcp_reachable(addr: &str) -> bool {
    let result = timeout(Duration::from_secs(2), TcpStream::connect(addr)).await;
    let reachable = result.map(|r| r.is_ok()).unwrap_or(false);
    log::trace!("[conn] TCP probe {}: {}", addr, if reachable { "UP" } else { "DOWN" });
    reachable
}
