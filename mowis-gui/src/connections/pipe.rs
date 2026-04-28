use crate::connection::DaemonConnection;
use agentd_protocol::{SocketRequest, SocketResponse};
use anyhow::{Context, Result};

/// Named pipe connection to agentd (Windows only)
///
/// Used by WSL2 launcher for communication with agentd running in WSL2.
#[cfg(target_os = "windows")]
pub struct NamedPipeConnection {
    pipe_name: String,
    stream: Option<tokio::net::windows::named_pipe::NamedPipeClient>,
    max_retries: usize,
    retry_delay: tokio::time::Duration,
}

#[cfg(target_os = "windows")]
impl NamedPipeConnection {
    /// Create a new named pipe connection
    pub fn new(pipe_name: String) -> Self {
        Self {
            pipe_name,
            stream: None,
            max_retries: 5,
            retry_delay: tokio::time::Duration::from_secs(1),
        }
    }

    /// Attempt to connect with retry logic
    async fn connect_with_retry(&mut self) -> Result<()> {
        use tokio::net::windows::named_pipe::ClientOptions;
        use tokio::time::sleep;

        let mut attempts = 0;

        loop {
            match ClientOptions::new().open(&self.pipe_name) {
                Ok(stream) => {
                    self.stream = Some(stream);
                    log::debug!("Connected to named pipe: {}", self.pipe_name);
                    return Ok(());
                }
                Err(e) => {
                    attempts += 1;
                    if attempts >= self.max_retries {
                        return Err(e).context(format!(
                            "Failed to connect to {} after {} attempts",
                            self.pipe_name, self.max_retries
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

#[cfg(target_os = "windows")]
#[async_trait::async_trait]
impl DaemonConnection for NamedPipeConnection {
    async fn connect(&mut self) -> Result<()> {
        self.connect_with_retry().await
    }

    async fn send_request(&mut self, req: SocketRequest) -> Result<()> {
        use tokio::io::AsyncWriteExt;

        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Not connected"))?;

        // Serialize request to JSON with newline delimiter
        let mut json = serde_json::to_string(&req)
            .context("Failed to serialize request")?;
        json.push('\n');

        // Write to pipe
        stream
            .write_all(json.as_bytes())
            .await
            .context("Failed to write request to pipe")?;

        stream.flush().await.context("Failed to flush pipe")?;

        Ok(())
    }

    async fn recv_response(&mut self) -> Result<SocketResponse> {
        use tokio::io::{AsyncBufReadExt, BufReader};

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
            .context("Failed to read response from pipe")?;

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
            log::debug!("Closed named pipe connection");
        }
        Ok(())
    }
}

// Stub for non-Windows platforms
#[cfg(not(target_os = "windows"))]
pub struct NamedPipeConnection;

#[cfg(not(target_os = "windows"))]
impl NamedPipeConnection {
    pub fn new(_pipe_name: String) -> Self {
        Self
    }
}

#[cfg(not(target_os = "windows"))]
#[async_trait::async_trait]
impl DaemonConnection for NamedPipeConnection {
    async fn connect(&mut self) -> Result<()> {
        Err(anyhow::anyhow!("Named pipe connection only available on Windows"))
    }

    async fn send_request(&mut self, _req: SocketRequest) -> Result<()> {
        Err(anyhow::anyhow!("Named pipe connection only available on Windows"))
    }

    async fn recv_response(&mut self) -> Result<SocketResponse> {
        Err(anyhow::anyhow!("Named pipe connection only available on Windows"))
    }

    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}
