use std::sync::Arc;

use anyhow::Result;
use crate::transport::Connection;
use mowis_orchestration::tools::ToolTransport;
use mowis_protocol::Payload;

pub struct VsockTransport {
    conn: Arc<Connection>,
}

impl VsockTransport {
    pub fn new(conn: Connection) -> Self {
        Self { conn: Arc::new(conn) }
    }
}

impl VsockTransport {
    pub fn from_arc(conn: Arc<Connection>) -> Self {
        Self { conn }
    }
}

#[async_trait::async_trait]
impl ToolTransport for VsockTransport {
    async fn invoke_tool(&self, payload: Payload) -> Result<Payload> {
        self.conn.call(payload).await
    }
}

/// Build a transport factory backed by a shared vsock connection. Each crew
/// agent gets a fresh `VsockTransport` wrapping the same `Connection`. The
/// `work_dir` arg is ignored — the VM owns the filesystem.
pub fn vsock_transport_factory(
    conn: Arc<Connection>,
) -> mowis_orchestration::tools::TransportFactory {
    Arc::new(move |_work_dir| Box::new(VsockTransport::from_arc(conn.clone())))
}
