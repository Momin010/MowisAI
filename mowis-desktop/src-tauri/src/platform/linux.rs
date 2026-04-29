// platform/linux.rs — Linux direct launcher
//
// On Linux the agentd binary runs natively on the host OS.
// No VM, no container overhead. We connect straight to the Unix socket
// that agentd creates at /tmp/agentd.sock.
//
// The user (or systemd unit / wrapper script) is responsible for starting
// agentd before launching the desktop app. The desktop detects it and
// connects; if it's not running we surface a "start daemon" button in the UI.

use crate::platform::{ConnectionInfo, ConnectionKind, VmLauncher};
use crate::platform::connection::is_unix_reachable;
use anyhow::{bail, Result};
use async_trait::async_trait;
use std::path::PathBuf;

const SOCKET_PATH: &str = "/tmp/agentd.sock";

pub struct LinuxDirectLauncher {
    socket_path: PathBuf,
}

impl LinuxDirectLauncher {
    pub fn new() -> Self {
        Self {
            socket_path: PathBuf::from(SOCKET_PATH),
        }
    }
}

#[async_trait]
impl VmLauncher for LinuxDirectLauncher {
    fn name(&self) -> &str { "Linux direct" }

    async fn start(&self) -> Result<ConnectionInfo> {
        // Verify the socket is up (agentd must already be running).
        if !is_unix_reachable(&self.socket_path).await {
            bail!(
                "agentd socket not found at {}. \
                 Start the daemon with: sudo agentd socket --path {}",
                self.socket_path.display(),
                self.socket_path.display(),
            );
        }
        Ok(ConnectionInfo {
            kind: ConnectionKind::UnixSocket,
            socket_path: Some(self.socket_path.clone()),
            tcp_addr: None,
            pipe_name: None,
            auth_token: None, // Unix socket — OS-level ACL is the auth
        })
    }

    async fn stop(&self) -> Result<()> {
        // We don't own the daemon on Linux — user manages it.
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(is_unix_reachable(&self.socket_path).await)
    }
}
