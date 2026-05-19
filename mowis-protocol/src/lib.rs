//! Wire protocol between `mowis-host` (host) and `mowis-executor` (guest).
//!
//! Transport is AF_VSOCK; framing is newline-delimited JSON. Every line on the
//! wire is one `Envelope`. Requests flow host -> guest; responses and streaming
//! events flow guest -> host. Multiple responses may share a request `id`
//! (e.g. stdout chunks followed by an `ExitCode`).
//!
//! Kept intentionally minimal for the MVP: lifecycle, sandbox CRUD, exec.
//! Tool dispatch reuses `serde_json::Value` so we don't have to redefine the
//! per-tool schemas — the executor's tool registry validates them.

use serde::{Deserialize, Serialize};

/// Default vsock port the guest executor listens on.
pub const DEFAULT_VSOCK_PORT: u32 = 5252;

/// Protocol version. Bump on any wire-format-breaking change.
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Correlation id. Host picks for requests; guest echoes for responses.
    pub id: u64,
    pub payload: Payload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Payload {
    // ---- Requests (host -> guest) ----
    Ping,
    Version,
    Shutdown,
    CreateSandbox(SandboxSpec),
    DestroySandbox {
        sandbox_id: String,
    },
    ListSandboxes,
    Exec(ExecRequest),
    InvokeTool {
        sandbox_id: String,
        tool: String,
        input: serde_json::Value,
    },

    // ---- Responses / events (guest -> host) ----
    Pong {
        version: String,
        protocol: u32,
    },
    SandboxCreated {
        sandbox_id: String,
    },
    SandboxDestroyed {
        sandbox_id: String,
    },
    SandboxList {
        sandboxes: Vec<SandboxInfo>,
    },
    Stdout {
        data: String,
    },
    Stderr {
        data: String,
    },
    ExitCode {
        code: i32,
    },
    ToolResult {
        output: serde_json::Value,
    },
    Error {
        message: String,
    },
}

/// Spec for a sandbox the guest should create.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxSpec {
    /// Optional caller-supplied id. Guest may override if it collides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_id: Option<String>,
    /// Optional rootfs to use as the overlay lower layer. If `None`, the guest
    /// uses an empty tmpfs (suitable for trivial exec tests).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_rootfs: Option<String>,
    #[serde(default)]
    pub limits: ResourceLimits,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ram_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_millis: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxInfo {
    pub sandbox_id: String,
    pub root_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecRequest {
    /// If set, runs inside the named sandbox (chroot+namespaces).
    /// If `None`, runs in the executor's own namespace (host-side of the VM).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_id: Option<String>,
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("encode: {0}")]
    Encode(serde_json::Error),
    #[error("decode: {0}")]
    Decode(serde_json::Error),
    #[error("connection closed")]
    Closed,
}

impl Envelope {
    pub fn new(id: u64, payload: Payload) -> Self {
        Self { id, payload }
    }

    /// Encode this envelope as a JSON line (terminating `\n` included).
    pub fn to_line(&self) -> Result<String, ProtocolError> {
        let mut s = serde_json::to_string(self).map_err(ProtocolError::Encode)?;
        s.push('\n');
        Ok(s)
    }

    /// Decode one envelope from a JSON line (trailing `\n` optional).
    pub fn from_line(line: &str) -> Result<Self, ProtocolError> {
        serde_json::from_str(line.trim_end_matches('\n')).map_err(ProtocolError::Decode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_ping() {
        let e = Envelope::new(1, Payload::Ping);
        let line = e.to_line().unwrap();
        let back = Envelope::from_line(&line).unwrap();
        assert_eq!(back.id, 1);
        assert!(matches!(back.payload, Payload::Ping));
    }

    #[test]
    fn roundtrip_create_sandbox() {
        let spec = SandboxSpec {
            sandbox_id: Some("sb1".into()),
            image_rootfs: None,
            limits: ResourceLimits {
                ram_bytes: Some(1 << 30),
                cpu_millis: Some(2000),
            },
        };
        let e = Envelope::new(42, Payload::CreateSandbox(spec));
        let line = e.to_line().unwrap();
        let back = Envelope::from_line(&line).unwrap();
        assert_eq!(back.id, 42);
        match back.payload {
            Payload::CreateSandbox(s) => {
                assert_eq!(s.sandbox_id.as_deref(), Some("sb1"));
                assert_eq!(s.limits.ram_bytes, Some(1 << 30));
            }
            _ => panic!("wrong payload"),
        }
    }

    #[test]
    fn roundtrip_exec_then_stream() {
        let req = Envelope::new(
            7,
            Payload::Exec(ExecRequest {
                sandbox_id: Some("sb1".into()),
                cmd: "/bin/ls".into(),
                args: vec!["-la".into()],
                env: vec![],
            }),
        );
        let line = req.to_line().unwrap();
        let back = Envelope::from_line(&line).unwrap();
        assert_eq!(back.id, 7);

        let chunk = Envelope::new(
            7,
            Payload::Stdout {
                data: "hello\n".into(),
            },
        );
        let line = chunk.to_line().unwrap();
        assert!(line.ends_with('\n'));
        let back = Envelope::from_line(&line).unwrap();
        match back.payload {
            Payload::Stdout { data } => assert_eq!(data, "hello\n"),
            _ => panic!(),
        }
    }
}
