use anyhow::Result;
use agentd_protocol::{SocketRequest, SocketResponse};

/// Daemon connection trait
///
/// Abstracts connection establishment and message framing for different
/// transport mechanisms (Unix socket, vsock, named pipe, TCP).
#[async_trait::async_trait]
pub trait DaemonConnection: Send + Sync {
    /// Establish connection to agentd
    async fn connect(&mut self) -> Result<()>;

    /// Send a JSON-RPC request
    async fn send_request(&mut self, req: SocketRequest) -> Result<()>;

    /// Receive a JSON-RPC response (blocking until available)
    async fn recv_response(&mut self) -> Result<SocketResponse>;

    /// Close the connection
    async fn close(&mut self) -> Result<()>;
}
