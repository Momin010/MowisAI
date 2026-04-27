use super::ConnectionInfo;
use anyhow::{anyhow, Result};
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};

// ── ConnectionStream ──────────────────────────────────────────────────────────

/// Unified stream type wrapping the platform-specific transport.
/// Implements AsyncRead + AsyncWrite + Unpin so it drops straight into
/// the existing `do_socket_io` generic.
pub enum ConnectionStream {
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
    Tcp(tokio::net::TcpStream),
    #[cfg(windows)]
    Pipe(tokio::net::windows::named_pipe::NamedPipeClient),
}

impl AsyncRead for ConnectionStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => Pin::new(s).poll_read(cx, buf),
            Self::Tcp(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(windows)]
            Self::Pipe(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ConnectionStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => Pin::new(s).poll_write(cx, buf),
            Self::Tcp(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(windows)]
            Self::Pipe(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => Pin::new(s).poll_flush(cx),
            Self::Tcp(s) => Pin::new(s).poll_flush(cx),
            #[cfg(windows)]
            Self::Pipe(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => Pin::new(s).poll_shutdown(cx),
            Self::Tcp(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(windows)]
            Self::Pipe(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

// ── Factory ───────────────────────────────────────────────────────────────────

/// Open and authenticate a connection described by `info`.
/// Callers can then pass the returned stream to `do_socket_io`.
pub async fn open_connection(info: &ConnectionInfo) -> Result<ConnectionStream> {
    match info {
        #[cfg(unix)]
        ConnectionInfo::UnixSocket { path } => {
            let stream = tokio::net::UnixStream::connect(path)
                .await
                .map_err(|e| anyhow!("Unix socket connect to {:?} failed: {e}", path))?;
            Ok(ConnectionStream::Unix(stream))
        }

        #[cfg(windows)]
        ConnectionInfo::NamedPipe { name } => {
            let stream = tokio::net::windows::named_pipe::ClientOptions::new()
                .open(name)
                .map_err(|e| anyhow!("Named pipe connect to {name} failed: {e}"))?;
            Ok(ConnectionStream::Pipe(stream))
        }

        ConnectionInfo::TcpWithToken { addr, token } => {
            let stream = tcp_connect_with_auth(*addr, token).await?;
            Ok(ConnectionStream::Tcp(stream))
        }
    }
}

// ── TCP auth handshake ────────────────────────────────────────────────────────

/// Connect via TCP and complete the token auth handshake before returning
/// the ready-to-use stream.
///
/// Protocol: send `{"type":"auth","token":"<TOKEN>"}\n`, expect back
/// `{"status":"authenticated"}\n`.
async fn tcp_connect_with_auth(addr: SocketAddr, token: &str) -> Result<tokio::net::TcpStream> {
    use tokio::io::AsyncReadExt;

    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .map_err(|e| anyhow!("TCP connect to {addr} failed: {e}"))?;

    // Send auth message (no serde to avoid round-trip allocations for this one line).
    let auth_line = format!("{{\"type\":\"auth\",\"token\":\"{token}\"}}\n");
    stream
        .write_all(auth_line.as_bytes())
        .await
        .map_err(|e| anyhow!("Failed to send auth token to {addr}: {e}"))?;

    // Read the response one byte at a time to avoid over-reading into the stream.
    let mut resp_bytes: Vec<u8> = Vec::with_capacity(64);
    let mut byte = [0u8; 1];
    loop {
        let n = stream
            .read(&mut byte)
            .await
            .map_err(|e| anyhow!("Failed to read auth response from {addr}: {e}"))?;
        if n == 0 {
            return Err(anyhow!("Connection to {addr} closed during auth handshake"));
        }
        if byte[0] == b'\n' {
            break;
        }
        resp_bytes.push(byte[0]);
    }

    let resp: serde_json::Value = serde_json::from_slice(&resp_bytes)
        .map_err(|e| anyhow!("Invalid auth response JSON from {addr}: {e}"))?;

    if resp.get("status").and_then(|v| v.as_str()) != Some("authenticated") {
        return Err(anyhow!(
            "Authentication rejected by daemon at {addr}: {:?}",
            resp
        ));
    }

    Ok(stream)
}
