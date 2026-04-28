use crate::connection::DaemonConnection;
use agentd_protocol::{SocketRequest, SocketResponse};
use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::time::{sleep, Duration};

/// Unix socket connection to agentd
///
/// Implements newline-delimited JSON framing over a Unix domain socket.
/// Includes connection retry logic for robustness.
pub struct UnixSocketConnection {
    socket_path: PathBuf,
    stream: Option<UnixStream>,
    max_retries: usize,
    retry_delay: Duration,
}

impl UnixSocketConnection {
    /// Create a new Unix socket connection
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            stream: None,
            max_retries: 5,
            retry_delay: Duration::from_secs(1),
        }
    }

    /// Attempt to connect with retry logic
    async fn connect_with_retry(&mut self) -> Result<()> {
        let mut attempts = 0;

        loop {
            match UnixStream::connect(&self.socket_path).await {
                Ok(stream) => {
                    self.stream = Some(stream);
                    log::debug!("Connected to Unix socket at {:?}", self.socket_path);
                    return Ok(());
                }
                Err(e) => {
                    attempts += 1;
                    if attempts >= self.max_retries {
                        return Err(e).context(format!(
                            "Failed to connect to {:?} after {} attempts",
                            self.socket_path, self.max_retries
                        ));
                    }
                    log::warn!(
                        "Connection attempt {} failed, retrying in {:?}...",
                        attempts,
                        self.retry_delay
                    );
                    sleep(self.retry_delay).await;
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl DaemonConnection for UnixSocketConnection {
    async fn connect(&mut self) -> Result<()> {
        self.connect_with_retry().await
    }

    async fn send_request(&mut self, req: SocketRequest) -> Result<()> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Not connected"))?;

        // Serialize request to JSON with newline delimiter
        let mut json = serde_json::to_string(&req)
            .context("Failed to serialize request")?;
        json.push('\n');

        // Write to socket
        stream
            .write_all(json.as_bytes())
            .await
            .context("Failed to write request to socket")?;

        stream.flush().await.context("Failed to flush socket")?;

        Ok(())
    }

    async fn recv_response(&mut self) -> Result<SocketResponse> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Not connected"))?;

        // Read newline-delimited JSON response
        let mut reader = BufReader::new(stream);
        let mut line = String::new();

        reader
            .read_line(&mut line)
            .await
            .context("Failed to read response from socket")?;

        if line.is_empty() {
            return Err(anyhow::anyhow!("Connection closed by server"));
        }

        // Parse JSON response
        let response: SocketResponse = serde_json::from_str(&line)
            .context("Failed to parse response JSON")?;

        Ok(response)
    }

    async fn close(&mut self) -> Result<()> {
        if let Some(stream) = self.stream.take() {
            drop(stream);
            log::debug!("Closed Unix socket connection");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unix_socket_connection_creation() {
        let conn = UnixSocketConnection::new(PathBuf::from("/tmp/test.sock"));
        assert_eq!(conn.socket_path, PathBuf::from("/tmp/test.sock"));
        assert_eq!(conn.max_retries, 5);
        assert!(conn.stream.is_none());
    }
}
