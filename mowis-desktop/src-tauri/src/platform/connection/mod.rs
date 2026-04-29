// platform/connection/mod.rs — Unified connection abstraction
//
// ConnectionStream wraps whichever transport the current platform uses:
//   UnixSocket  — Linux native (agentd directly on host)
//   NamedPipe   — Windows WSL2 bridge  (\\.\pipe\MowisAI\agentd)
//   TcpWithToken— QEMU / WSL2 TCP relay (127.0.0.1:port + 256-bit token)
//
// All variants expose the same send/recv_lines interface so the rest of
// the codebase never needs to know which transport is active.

use crate::platform::{auth, ConnectionInfo, ConnectionKind};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::{timeout, Duration};

// ── Platform-specific stream types ───────────────────────────────────────────

#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::net::TcpStream;
#[cfg(windows)]
use tokio::net::windows::named_pipe::ClientOptions;

// ── Unified stream ────────────────────────────────────────────────────────────

pub enum ConnectionStream {
    #[cfg(unix)]
    Unix(BufReader<UnixStream>),
    Tcp(BufReader<TcpStream>),
    #[cfg(windows)]
    Pipe(BufReader<tokio::net::windows::named_pipe::NamedPipeClient>),
}

impl ConnectionStream {
    /// Send a JSON value followed by a newline.
    pub async fn send_json(&mut self, value: &Value) -> Result<()> {
        let mut line = serde_json::to_string(value)?;
        line.push('\n');
        match self {
            #[cfg(unix)]
            Self::Unix(r) => r.get_mut().write_all(line.as_bytes()).await?,
            Self::Tcp(r) => r.get_mut().write_all(line.as_bytes()).await?,
            #[cfg(windows)]
            Self::Pipe(r) => r.get_mut().write_all(line.as_bytes()).await?,
        }
        Ok(())
    }

    /// Read the next non-empty line as a raw string.
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
            return Ok(None); // EOF
        }
        Ok(Some(buf.trim_end().to_owned()))
    }

    /// Read the next line and parse it as JSON.
    pub async fn recv_json(&mut self) -> Result<Option<Value>> {
        while let Some(line) = self.recv_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(&line)
                .with_context(|| format!("parse JSON: {line}"))?;
            return Ok(Some(v));
        }
        Ok(None)
    }
}

// ── Connection factory ────────────────────────────────────────────────────────

/// Open a connection described by `info` and perform the auth handshake
/// (for TCP and NamedPipe connections).
pub async fn open_connection(info: &ConnectionInfo) -> Result<ConnectionStream> {
    match info.kind {
        // ── Linux direct Unix socket ─────────────────────────────────────────
        ConnectionKind::UnixSocket => {
            #[cfg(unix)]
            {
                let path = info
                    .socket_path
                    .as_ref()
                    .context("UnixSocket connection needs socket_path")?;
                let stream = UnixStream::connect(path)
                    .await
                    .with_context(|| format!("connect unix {}", path.display()))?;
                Ok(ConnectionStream::Unix(BufReader::new(stream)))
            }
            #[cfg(not(unix))]
            bail!("UnixSocket connections are only available on Linux")
        }

        // ── TCP + auth token (QEMU / WSL2 relay) ────────────────────────────
        ConnectionKind::TcpWithToken => {
            let addr = info
                .tcp_addr
                .as_deref()
                .context("TcpWithToken connection needs tcp_addr")?;
            let token = info
                .auth_token
                .as_deref()
                .context("TcpWithToken connection needs auth_token")?;

            let stream = timeout(Duration::from_secs(10), TcpStream::connect(addr))
                .await
                .context("TCP connect timed out")?
                .with_context(|| format!("TCP connect {addr}"))?;

            let mut conn = ConnectionStream::Tcp(BufReader::new(stream));
            tcp_auth_handshake(&mut conn, token).await?;
            Ok(conn)
        }

        // ── Windows named pipe (WSL2 socat bridge) ───────────────────────────
        ConnectionKind::NamedPipe => {
            #[cfg(windows)]
            {
                let pipe = info
                    .pipe_name
                    .as_deref()
                    .context("NamedPipe connection needs pipe_name")?;
                let token = info
                    .auth_token
                    .as_deref()
                    .context("NamedPipe connection needs auth_token")?;

                let client = ClientOptions::new()
                    .open(pipe)
                    .with_context(|| format!("open named pipe {pipe}"))?;
                let mut conn = ConnectionStream::Pipe(BufReader::new(client));
                tcp_auth_handshake(&mut conn, token).await?;
                Ok(conn)
            }
            #[cfg(not(windows))]
            bail!("NamedPipe connections are only available on Windows")
        }
    }
}

// ── Auth handshake (shared by TCP and NamedPipe) ──────────────────────────────

/// Send `{"type":"auth","token":"<hex>"}` and expect `{"status":"authenticated"}`.
async fn tcp_auth_handshake(conn: &mut ConnectionStream, token: &str) -> Result<()> {
    conn.send_json(&serde_json::json!({ "type": "auth", "token": token }))
        .await
        .context("send auth token")?;

    let resp = timeout(Duration::from_secs(5), conn.recv_json())
        .await
        .context("auth response timed out")?
        .context("recv auth response")?
        .context("connection closed before auth response")?;

    match resp.get("status").and_then(Value::as_str) {
        Some("authenticated") => Ok(()),
        Some(s) => bail!("Auth rejected by daemon: {s}"),
        None => bail!("Unexpected auth response: {resp}"),
    }
}

// ── Connection probe (no auth — just TCP SYN) ─────────────────────────────────

pub async fn is_tcp_reachable(addr: &str) -> bool {
    timeout(Duration::from_secs(2), TcpStream::connect(addr))
        .await
        .map(|r| r.is_ok())
        .unwrap_or(false)
}

#[cfg(unix)]
pub async fn is_unix_reachable(path: &std::path::Path) -> bool {
    timeout(Duration::from_secs(2), UnixStream::connect(path))
        .await
        .map(|r| r.is_ok())
        .unwrap_or(false)
}

#[cfg(windows)]
pub async fn is_pipe_reachable(pipe: &str) -> bool {
    ClientOptions::new().open(pipe).is_ok()
}
