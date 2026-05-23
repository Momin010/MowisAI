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
pub const PROTOCOL_VERSION: u32 = 2;

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

    // ---- Agent overlay management (host -> guest) ----
    CreateAgentOverlay {
        parent_sandbox_id: String,
        agent_id: String,
        limits: ResourceLimits,
    },
    MergeAgentOverlay {
        parent_sandbox_id: String,
        agent_id: String,
    },
    DiscardAgentOverlay {
        parent_sandbox_id: String,
        agent_id: String,
    },
    InvokeToolAsAgent {
        parent_sandbox_id: String,
        agent_id: String,
        tool: String,
        input: serde_json::Value,
        caller_tier: String,
    },

    // ---- Codebase management (host -> guest) ----
    UploadCodebase {
        sandbox_id: String,
        archive_b64: String,
        file_count: u32,
    },
    HealthCheck,

    // ---- Interactive shell (host -> guest) ----
    SendInput {
        sandbox_id: String,
        agent_id: String,
        input: String,
    },
    InteractiveStatus {
        sandbox_id: String,
        agent_id: String,
    },

    // ---- Agent overlay responses (guest -> host) ----
    AgentOverlayCreated {
        agent_id: String,
    },
    AgentOverlayMerged {
        agent_id: String,
        changed_paths: Vec<String>,
    },
    AgentOverlayDiscarded {
        agent_id: String,
    },

    // ---- Codebase responses (guest -> host) ----
    CodebaseUploaded {
        sandbox_id: String,
        file_count: u32,
    },
    HealthOk {
        uptime_secs: u64,
        sandbox_count: usize,
    },

    // ---- Interactive shell responses (guest -> host) ----
    InteractivePrompt {
        agent_id: String,
        prompt: String,
        waiting: bool,
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

    #[test]
    fn roundtrip_create_agent_overlay() {
        let e = Envelope::new(
            10,
            Payload::CreateAgentOverlay {
                parent_sandbox_id: "sb-1".into(),
                agent_id: "ag-001".into(),
                limits: ResourceLimits {
                    ram_bytes: Some(512 * 1024 * 1024),
                    cpu_millis: Some(1000),
                },
            },
        );
        let line = e.to_line().unwrap();
        let back = Envelope::from_line(&line).unwrap();
        assert_eq!(back.id, 10);
        match back.payload {
            Payload::CreateAgentOverlay {
                parent_sandbox_id,
                agent_id,
                limits,
            } => {
                assert_eq!(parent_sandbox_id, "sb-1");
                assert_eq!(agent_id, "ag-001");
                assert_eq!(limits.ram_bytes, Some(512 * 1024 * 1024));
            }
            _ => panic!("wrong payload"),
        }
    }

    #[test]
    fn roundtrip_merge_agent_overlay() {
        let e = Envelope::new(
            11,
            Payload::MergeAgentOverlay {
                parent_sandbox_id: "sb-1".into(),
                agent_id: "ag-001".into(),
            },
        );
        let line = e.to_line().unwrap();
        let back = Envelope::from_line(&line).unwrap();
        assert_eq!(back.id, 11);
        match back.payload {
            Payload::MergeAgentOverlay {
                parent_sandbox_id,
                agent_id,
            } => {
                assert_eq!(parent_sandbox_id, "sb-1");
                assert_eq!(agent_id, "ag-001");
            }
            _ => panic!("wrong payload"),
        }
    }

    #[test]
    fn roundtrip_discard_agent_overlay() {
        let e = Envelope::new(
            12,
            Payload::DiscardAgentOverlay {
                parent_sandbox_id: "sb-1".into(),
                agent_id: "ag-002".into(),
            },
        );
        let line = e.to_line().unwrap();
        let back = Envelope::from_line(&line).unwrap();
        assert_eq!(back.id, 12);
        match back.payload {
            Payload::DiscardAgentOverlay {
                parent_sandbox_id,
                agent_id,
            } => {
                assert_eq!(parent_sandbox_id, "sb-1");
                assert_eq!(agent_id, "ag-002");
            }
            _ => panic!("wrong payload"),
        }
    }

    #[test]
    fn roundtrip_invoke_tool_as_agent() {
        let e = Envelope::new(
            13,
            Payload::InvokeToolAsAgent {
                parent_sandbox_id: "sb-1".into(),
                agent_id: "ag-001".into(),
                tool: "read_file".into(),
                input: serde_json::json!({"path": "/src/main.rs"}),
                caller_tier: "crew".into(),
            },
        );
        let line = e.to_line().unwrap();
        let back = Envelope::from_line(&line).unwrap();
        assert_eq!(back.id, 13);
        match back.payload {
            Payload::InvokeToolAsAgent {
                parent_sandbox_id,
                agent_id,
                tool,
                input,
                caller_tier,
            } => {
                assert_eq!(parent_sandbox_id, "sb-1");
                assert_eq!(agent_id, "ag-001");
                assert_eq!(tool, "read_file");
                assert_eq!(input, serde_json::json!({"path": "/src/main.rs"}));
                assert_eq!(caller_tier, "crew");
            }
            _ => panic!("wrong payload"),
        }
    }

    #[test]
    fn roundtrip_agent_overlay_created() {
        let e = Envelope::new(14, Payload::AgentOverlayCreated { agent_id: "ag-001".into() });
        let line = e.to_line().unwrap();
        let back = Envelope::from_line(&line).unwrap();
        assert_eq!(back.id, 14);
        match back.payload {
            Payload::AgentOverlayCreated { agent_id } => assert_eq!(agent_id, "ag-001"),
            _ => panic!("wrong payload"),
        }
    }

    #[test]
    fn roundtrip_agent_overlay_merged() {
        let e = Envelope::new(
            15,
            Payload::AgentOverlayMerged {
                agent_id: "ag-001".into(),
                changed_paths: vec!["src/main.rs".into(), "Cargo.toml".into()],
            },
        );
        let line = e.to_line().unwrap();
        let back = Envelope::from_line(&line).unwrap();
        assert_eq!(back.id, 15);
        match back.payload {
            Payload::AgentOverlayMerged {
                agent_id,
                changed_paths,
            } => {
                assert_eq!(agent_id, "ag-001");
                assert_eq!(changed_paths, vec!["src/main.rs", "Cargo.toml"]);
            }
            _ => panic!("wrong payload"),
        }
    }

    #[test]
    fn roundtrip_agent_overlay_discarded() {
        let e = Envelope::new(16, Payload::AgentOverlayDiscarded { agent_id: "ag-002".into() });
        let line = e.to_line().unwrap();
        let back = Envelope::from_line(&line).unwrap();
        assert_eq!(back.id, 16);
        match back.payload {
            Payload::AgentOverlayDiscarded { agent_id } => assert_eq!(agent_id, "ag-002"),
            _ => panic!("wrong payload"),
        }
    }

    #[test]
    fn protocol_version_is_2() {
        assert_eq!(PROTOCOL_VERSION, 2);
    }

    #[test]
    fn roundtrip_upload_codebase() {
        let e = Envelope::new(
            20,
            Payload::UploadCodebase {
                sandbox_id: "sb-1".into(),
                archive_b64: "dGVzdA==".into(), // "test" in base64
                file_count: 5,
            },
        );
        let line = e.to_line().unwrap();
        let back = Envelope::from_line(&line).unwrap();
        assert_eq!(back.id, 20);
        match back.payload {
            Payload::UploadCodebase {
                sandbox_id,
                archive_b64,
                file_count,
            } => {
                assert_eq!(sandbox_id, "sb-1");
                assert_eq!(archive_b64, "dGVzdA==");
                assert_eq!(file_count, 5);
            }
            _ => panic!("wrong payload"),
        }
    }

    #[test]
    fn roundtrip_codebase_uploaded() {
        let e = Envelope::new(
            21,
            Payload::CodebaseUploaded {
                sandbox_id: "sb-1".into(),
                file_count: 42,
            },
        );
        let line = e.to_line().unwrap();
        let back = Envelope::from_line(&line).unwrap();
        assert_eq!(back.id, 21);
        match back.payload {
            Payload::CodebaseUploaded {
                sandbox_id,
                file_count,
            } => {
                assert_eq!(sandbox_id, "sb-1");
                assert_eq!(file_count, 42);
            }
            _ => panic!("wrong payload"),
        }
    }

    #[test]
    fn roundtrip_health_check() {
        let e = Envelope::new(22, Payload::HealthCheck);
        let line = e.to_line().unwrap();
        let back = Envelope::from_line(&line).unwrap();
        assert_eq!(back.id, 22);
        assert!(matches!(back.payload, Payload::HealthCheck));
    }

    #[test]
    fn roundtrip_health_ok() {
        let e = Envelope::new(
            23,
            Payload::HealthOk {
                uptime_secs: 3600,
                sandbox_count: 3,
            },
        );
        let line = e.to_line().unwrap();
        let back = Envelope::from_line(&line).unwrap();
        assert_eq!(back.id, 23);
        match back.payload {
            Payload::HealthOk {
                uptime_secs,
                sandbox_count,
            } => {
                assert_eq!(uptime_secs, 3600);
                assert_eq!(sandbox_count, 3);
            }
            _ => panic!("wrong payload"),
        }
    }
}
