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

#[async_trait::async_trait]
impl ToolTransport for VsockTransport {
    async fn invoke_tool(&self, payload: Payload) -> Result<Payload> {
        self.conn.call(payload).await
    }
}
