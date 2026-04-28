use crate::connection::DaemonConnection;
use agentd_protocol::{SocketRequest, SocketResponse};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::{sleep, Duration};

/// TCP connection with auth token to agentd
///
/// Used by QEMU launcher for VM communication.
/// Implements newline-delimited JSON framing with auth handshake.
pub struct TcpTokenConnection {
    addr: SocketAddr,
    token: String,
    stream: Option<TcpStream>,
    max_retries: usize,
    retry_delay: Duration,
}

impl TcpTokenConnection {
    /// Create a new TCP+token connection
    pub fn new(addr: SocketAddr, token: String) -> Self {
        Self {
            addr,
            token,
            stream: None,
            max_retries: 5,
            retry_delay: Duration::from_secs(1),
        }
    }

    /// Attempt to connect with retry logic
    async fn connect_with_retry(&mut self) -> Result<()> {
        let mut attempts = 0;

        loop {
            match TcpStream::connect(&self.addr).await {
                Ok(stream) => {
                    self.stream = Some(stream);
                    log::debug!("Connected to TCP socket at {}", self.addr);
                    
                    // Perform auth handshake
                    self.auth_handshake().await?;
                    
                    return Ok(());
                }
                Err(e) => {
                    attempts += 1;
                    if attempts >= self.max_retries {
                        return Err(e).context(format!(
                            "Failed to connect to {} after {} attempts",
                            self.addr, self.max_retries
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

    /// Perform auth handshake
    async fn auth_handshake(&mut self) -> Result<()> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Not connected"))?;

        // Send auth token as first message
        let auth_msg = serde_json::json!({
            "type": "auth",
            "token": self.token,
        });

        let mut msg = serde_json::to_string(&auth_msg)?;
        msg.push('\n');

        stream
            .write_all(msg.as_bytes())
            .await
            .context("Failed to send auth token")?;

        stream.flush().await.context("Failed to flush stream")?;

        // Read auth response
        let mut reader = BufReader::new(stream);
        let mut line = String::new();

        reader
            .read_line(&mut line)
            .await
            .context("Failed to read auth response")?;

        let response: serde_json::Value = serde_json::from_str(&line)
            .context("Failed to parse auth response")?;

        if response.get("status").and_then(|v| v.as_str()) == Some("authenticated") {
            log::info!("Authentication successful");
            Ok(())
        } else {
            Err(anyhow::anyhow!("Authentication failed: {:?}", response))
        }
    }
}

#[async_trait::async_trait]
impl DaemonConnection for TcpTokenConnection {
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

        // Write to stream
        stream
            .write_all(json.as_bytes())
            .await
            .context("Failed to write request to stream")?;

        stream.flush().await.context("Failed to flush stream")?;

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
            .context("Failed to read response from stream")?;

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
            log::debug!("Closed TCP connection");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_token_connection_creation() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let conn = TcpTokenConnection::new(addr, "test-token".to_string());
        
        assert_eq!(conn.addr, addr);
        assert_eq!(conn.token, "test-token");
        assert_eq!(conn.max_retries, 5);
        assert!(conn.stream.is_none());
    }
}
