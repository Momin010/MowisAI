use anyhow::{Context, Result};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Mutex;

use crate::{ResourceLimits, Sandbox};
use crate::security::SecurityPolicy;
use crate::tools::{
    EchoTool, ReadFileTool, WriteFileTool, DeleteFileTool, ListFilesTool,
    CreateDirectoryTool, GetFileInfoTool, CopyFileTool, RunCommandTool,
    HttpGetTool, HttpPostTool, JsonParseTool, JsonStringifyTool,
};
use crate::audit::{AuditEvent, EventType};
use crate::channels;
use crate::buckets::BucketStore;
use crate::memory::AgentMemory;

lazy_static! {
    /// In-memory sandbox cache
    static ref SANDBOXES: Mutex<HashMap<u64, Sandbox>> = Mutex::new(HashMap::new());
    static ref AUDITOR: crate::audit::SecurityAuditor = 
        crate::audit::SecurityAuditor::new(std::path::Path::new("/var/log/agentd/audit.log"))
        .expect("failed to initialize audit logger");
    static ref PERSISTENCE: crate::persistence::PersistenceManager = 
        crate::persistence::PersistenceManager::new(std::path::Path::new("/var/lib/agentd"));
    static ref MEMORY_STORE: Mutex<HashMap<u64, AgentMemory>> = 
        Mutex::new(HashMap::new());
    static ref COORDINATOR: Mutex<crate::agent_loop::AgentCoordinator> = 
        Mutex::new(crate::agent_loop::AgentCoordinator::new());
}

#[derive(Debug, Deserialize, Default)]
pub struct SocketRequest {
    pub request_type: String,
    pub sandbox: Option<u64>,
    pub ram: Option<u64>,
    pub cpu: Option<u64>,
    pub image: Option<String>,
    pub packages: Option<Vec<String>>,
    pub name: Option<String>,
    pub input: Option<Value>,
    pub command: Option<String>,
    pub to: Option<u64>,
    pub channel: Option<u64>,
    pub agent: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct SocketResponse {
    pub status: String,
    pub result: Option<Value>,
    pub error: Option<String>,
}

impl SocketResponse {
    fn ok(result: Option<Value>) -> Self {
        SocketResponse {
            status: "ok".into(),
            result,
            error: None,
        }
    }

    fn err<E: ToString>(e: E) -> Self {
        SocketResponse {
            status: "error".into(),
            result: None,
            error: Some(e.to_string()),
        }
    }
}

fn handle_request(req: SocketRequest) -> SocketResponse {
    match req.request_type.as_str() {
        "create_sandbox" => {
            let limits = ResourceLimits {
                ram_bytes: req.ram,
                cpu_millis: req.cpu,
            };
            let image_ref = req.image.as_deref();
            match Sandbox::new_with_image(limits, image_ref) {
                Ok(mut sb) => {
                    let id = sb.id();

                    if let Some(pkgs) = &req.packages {
                        if !pkgs.is_empty() {
                            let image_hint = req.image.as_deref().unwrap_or_default().to_lowercase();
                            let install_cmd = if image_hint.contains("alpine") {
                                format!("apk add --no-cache {}", pkgs.join(" "))
                            } else if image_hint.contains("ubuntu") || image_hint.contains("debian") {
                                format!("apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y {}", pkgs.join(" "))
                            } else {
                                format!("apk add --no-cache {} || (apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y {})", pkgs.join(" "), pkgs.join(" "))
                            };

                            let root = sb.root_path().to_owned();
                            let cmd_str = install_cmd.clone();
                            let resolv_dst = root.join("etc/resolv.conf");
                            let _ = std::fs::copy("/etc/resolv.conf", &resolv_dst);
                            let output = match std::process::Command::new("chroot")
                                .arg(&root)
                                .arg("/bin/sh")
                                .arg("-c")
                                .arg(&cmd_str)
                                .output() {
                                    Ok(o) => o,
                                    Err(e) => return SocketResponse::err(format!("failed to run chroot: {}", e)),
                                };
                            if !output.status.success() {
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                let stderr = String::from_utf8_lossy(&output.stderr);
                                return SocketResponse::err(format!(
                                    "package install failed:\nstdout: {}\nstderr: {}", stdout, stderr
                                ));
                            }
                            log::info!("packages installed for sandbox {}", id);
                        }
                    }

                    let mut store = SANDBOXES.lock().unwrap();
                    store.insert(id, sb);
                    let _ = AUDITOR.record_event(AuditEvent::new(EventType::SandboxCreated, 0, "sandbox created").with_target(id).with_result("success"));
                    SocketResponse::ok(Some(json!({"sandbox": id})))
                }
                Err(e) => {
                    let _ = AUDITOR.record_event(AuditEvent::new(EventType::SandboxCreated, 0, "sandbox creation failed").with_result(format!("failed: {}", e)));
                    SocketResponse::err(e)
                }
            }
        }
        "list" => {
            let store = SANDBOXES.lock().unwrap();
            let ids: Vec<u64> = store.keys().copied().collect();
            SocketResponse::ok(Some(json!(ids)))
        }
        "destroy_sandbox" => {
            if let Some(id) = req.sandbox {
                let mut store = SANDBOXES.lock().unwrap();
                if let Some(sb) = store.remove(&id) {
                    drop(sb);
                    let _ = AUDITOR.record_event(AuditEvent::new(EventType::SandboxDestroyed, 0, "sandbox destroyed").with_target(id));
                    SocketResponse::ok(None)
                } else {
                    SocketResponse::err(format!("sandbox {} not found", id))
                }
            } else {
                SocketResponse::err("missing sandbox id")
            }
        }
        "invoke_tool" => {
            if let Some(id) = req.sandbox {
                let name = req.name.clone().unwrap_or_default();
                let input = req.input.clone().unwrap_or(json!(null));
                let mut store = SANDBOXES.lock().unwrap();
                if let Some(sb) = store.get(&id) {
                    match sb.invoke_tool(&name, input) {
                        Ok(val) => {
                            let _ = AUDITOR.record_event(AuditEvent::new(EventType::ToolInvoked, id, "tool invoked").with_details(json!({"tool": name})));
                            SocketResponse::ok(Some(val))
                        }
                        Err(e) => {
                            let err_str = e.to_string();
                            if err_str.contains("security policy denied") {
                                let _ = AUDITOR.record_event(AuditEvent::new(EventType::SecurityViolation, id, "security policy denied").with_details(json!({"tool": name, "error": err_str})));
                            } else {
                                let _ = AUDITOR.record_event(AuditEvent::new(EventType::ToolFailed, id, "tool failed").with_details(json!({"tool": name, "error": err_str})));
                            }
                            SocketResponse::err(e)
                        }
                    }
                } else {
                    SocketResponse::err(format!("sandbox {} not found", id))
                }
            } else {
                SocketResponse::err("missing sandbox id")
            }
        }
        "run" => {
            if let Some(id) = req.sandbox {
                let cmd = req.command.clone().unwrap_or_default();
                let mut store = SANDBOXES.lock().unwrap();
                if let Some(sb) = store.get(&id) {
                    match sb.run_command(&cmd) {
                        Ok(out) => SocketResponse::ok(Some(json!({"output": out}))),
                        Err(e) => SocketResponse::err(e),
                    }
                } else {
                    SocketResponse::err(format!("sandbox {} not found", id))
                }
            } else {
                SocketResponse::err("missing sandbox id")
            }
        }
        "register_tool" => {
            if let Some(id) = req.sandbox {
                let name = req.name.clone().unwrap_or_default();
                let mut store = SANDBOXES.lock().unwrap();
                if let Some(sb) = store.get_mut(&id) {
                    let tool: Box<dyn crate::tools::Tool> = match name.as_str() {
                        "read_file" => Box::new(ReadFileTool {}),
                        "write_file" => Box::new(WriteFileTool {}),
                        "delete_file" => Box::new(DeleteFileTool {}),
                        "list_files" => Box::new(ListFilesTool {}),
                        "create_directory" => Box::new(CreateDirectoryTool {}),
                        "get_file_info" => Box::new(GetFileInfoTool {}),
                        "copy_file" => Box::new(CopyFileTool {}),
                        "run_command" => Box::new(RunCommandTool {}),
                        "http_get" => Box::new(HttpGetTool {}),
                        "http_post" => Box::new(HttpPostTool {}),
                        "json_parse" => Box::new(JsonParseTool {}),
                        "json_stringify" => Box::new(JsonStringifyTool {}),
                        "echo" => Box::new(EchoTool {}),
                        _ => return SocketResponse::err(format!("unknown tool: {}", name)),
                    };
                    sb.register_tool(tool);
                    SocketResponse::ok(None)
                } else {
                    SocketResponse::err(format!("sandbox {} not found", id))
                }
            } else {
                SocketResponse::err("missing sandbox id")
            }
        }
        "set_policy" => {
            if let Some(id) = req.sandbox {
                let name = req.name.clone().unwrap_or_default();
                let mut store = SANDBOXES.lock().unwrap();
                if let Some(sb) = store.get_mut(&id) {
                    let policy = match name.as_str() {
                        "restrictive" => SecurityPolicy::default_restrictive(),
                        "permissive" => SecurityPolicy::default_permissive(),
                        _ => return SocketResponse::err(format!("unknown policy: {}", name)),
                    };
                    sb.set_policy(policy);
                    let _ = AUDITOR.record_event(AuditEvent::new(EventType::Custom("PolicySet".into()), 0, "policy set").with_target(id).with_details(json!({"policy": name})));
                    SocketResponse::ok(None)
                } else {
                    SocketResponse::err(format!("sandbox {} not found", id))
                }
            } else {
                SocketResponse::err("missing sandbox id")
            }
        }
        "get_policy" => {
            if let Some(id) = req.sandbox {
                let store = SANDBOXES.lock().unwrap();
                if let Some(sb) = store.get(&id) {
                    match sb.policy() {
                        Some(policy) => {
                            match serde_json::to_value(policy) {
                                Ok(val) => SocketResponse::ok(Some(val)),
                                Err(e) => SocketResponse::err(format!("failed to serialize policy: {}", e)),
                            }
                        }
                        None => SocketResponse::err("no policy set for sandbox"),
                    }
                } else {
                    SocketResponse::err(format!("sandbox {} not found", id))
                }
            } else {
                SocketResponse::err("missing sandbox id")
            }
        }
        "get_audit_stats" => {
            SocketResponse::ok(Some(AUDITOR.get_stats()))
        }
        "get_anomalies" => {
            SocketResponse::ok(Some(AUDITOR.detect_anomalies()))
        }
        "create_channel" => {
            let from = match req.sandbox {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox"),
            };
            let to = match req.to {
                Some(id) => id,
                None => return SocketResponse::err("missing to"),
            };
            let id = crate::channels::create_channel(from, to);
            SocketResponse::ok(Some(json!({"channel": id})))
        }
        "send_message" => {
            let from = match req.sandbox {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox"),
            };
            let channel_id = match req.channel {
                Some(id) => id,
                None => return SocketResponse::err("missing channel"),
            };
            let payload = req.command.clone().unwrap_or_default();
            match crate::channels::send_message(channel_id, crate::channels::Message { from, to: 0, payload }) {
                Ok(_) => SocketResponse::ok(None),
                Err(e) => SocketResponse::err(e),
            }
        }
        "read_messages" => {
            let channel_id = match req.channel {
                Some(id) => id,
                None => return SocketResponse::err("missing channel"),
            };
            match crate::channels::read_messages(channel_id) {
                Ok(msgs) => {
                    let out: Vec<_> = msgs.iter().map(|m| json!({"from": m.from, "to": m.to, "payload": m.payload})).collect();
                    SocketResponse::ok(Some(json!(out)))
                }
                Err(e) => SocketResponse::err(e),
            }
        }
        "bucket_put" => {
            let sandbox_id = match req.sandbox {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox"),
            };
            let key = req.name.clone().unwrap_or_default();
            let value = req.command.clone().unwrap_or_default();
            let store = SANDBOXES.lock().unwrap();
            let sb = match store.get(&sandbox_id) {
                Some(s) => s,
                None => return SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
            };
            let bucket_path = sb.root_path().join("buckets");
            drop(store);
            let mut store = match BucketStore::new(bucket_path) {
                Ok(s) => s,
                Err(e) => return SocketResponse::err(e),
            };
            match store.put(&key, &value) {
                Ok(()) => SocketResponse::ok(None),
                Err(e) => SocketResponse::err(e),
            }
        }
        "bucket_get" => {
            let sandbox_id = match req.sandbox {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox"),
            };
            let key = req.name.clone().unwrap_or_default();
            let store = SANDBOXES.lock().unwrap();
            let sb = match store.get(&sandbox_id) {
                Some(s) => s,
                None => return SocketResponse::err(format!("sandbox {} not found", sandbox_id)),
            };
            let bucket_path = sb.root_path().join("buckets");
            drop(store);
            let store = match BucketStore::new(bucket_path) {
                Ok(s) => s,
                Err(e) => return SocketResponse::err(e),
            };
            match store.get(&key) {
                Ok(Some(value)) => SocketResponse::ok(Some(json!({"value": value}))),
                Ok(None) => SocketResponse::err("key not found"),
                Err(e) => SocketResponse::err(e),
            }
        }
        "memory_set" => {
            let sandbox_id = match req.sandbox {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox"),
            };
            let key = req.name.clone().unwrap_or_default();
            let value = req.input.clone().unwrap_or(json!(null));
            let mut mem = MEMORY_STORE.lock().unwrap();
            let agent_mem = mem.entry(sandbox_id).or_insert_with(|| AgentMemory::new(sandbox_id, sandbox_id));
            agent_mem.short_term.set_context(key, value);
            SocketResponse::ok(None)
        }
        "memory_get" => {
            let sandbox_id = match req.sandbox {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox"),
            };
            let key = req.name.clone().unwrap_or_default();
            let mem = MEMORY_STORE.lock().unwrap();
            match mem.get(&sandbox_id) {
                Some(agent_mem) => {
                    match agent_mem.short_term.get_context(&key) {
                        Some(value) => SocketResponse::ok(Some(json!({"value": value}))),
                        None => SocketResponse::ok(Some(json!({"value": null}))),
                    }
                }
                None => SocketResponse::ok(Some(json!({"value": null}))),
            }
        }
        "memory_save" => {
            let sandbox_id = match req.sandbox {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox"),
            };
            let mem = MEMORY_STORE.lock().unwrap();
            let agent_mem = match mem.get(&sandbox_id) {
                Some(m) => m,
                None => return SocketResponse::err("no memory found for sandbox"),
            };
            let json = match agent_mem.serialize_to_json() {
                Ok(j) => j,
                Err(e) => return SocketResponse::err(e),
            };
            drop(mem);
            match PERSISTENCE.save_agent_memory(sandbox_id, &json) {
                Ok(()) => SocketResponse::ok(None),
                Err(e) => SocketResponse::err(e),
            }
        }
        "memory_load" => {
            let sandbox_id = match req.sandbox {
                Some(id) => id,
                None => return SocketResponse::err("missing sandbox"),
            };
            let json = match PERSISTENCE.load_agent_memory(sandbox_id) {
                Ok(j) => j,
                Err(e) => return SocketResponse::err(e),
            };
            let mut mem = MEMORY_STORE.lock().unwrap();
            let agent_mem = match AgentMemory::deserialize_from_json(&json) {
                Ok(m) => m,
                Err(e) => return SocketResponse::err(e),
            };
            mem.insert(sandbox_id, agent_mem);
            SocketResponse::ok(Some(json))
        }
        "agent_spawn" => {
            let max_iter = req.input.as_ref()
                .and_then(|v| v.as_u64())
                .unwrap_or(10) as usize;
            let agent_id = COORDINATOR.lock().unwrap().spawn_agent(max_iter);
            SocketResponse::ok(Some(json!({"agent": agent_id})))
        }
        "agent_run" => {
            let agent_id = match req.agent { Some(id) => id, None => return SocketResponse::err("missing agent") };
            let prompt = req.command.clone().unwrap_or_default();
            let mut coord = COORDINATOR.lock().unwrap();
            if let Some(agent) = coord.get_agent(agent_id) {
                let tools: Vec<Box<dyn crate::tools::Tool>> = vec![
                    Box::new(crate::tools::EchoTool {}),
                    Box::new(crate::tools::RunCommandTool {}),
                    Box::new(crate::tools::ReadFileTool {}),
                    Box::new(crate::tools::WriteFileTool {}),
                    Box::new(crate::tools::ListFilesTool {}),
                    Box::new(crate::tools::HttpGetTool {}),
                ];
                match agent.run(&prompt, &tools) {
                    Ok(result) => SocketResponse::ok(Some(json!({"result": result}))),
                    Err(e) => SocketResponse::err(e),
                }
            } else {
                SocketResponse::err(format!("agent {} not found", agent_id))
            }
        }
        "agent_status" => {
            let agent_id = match req.agent { Some(id) => id, None => return SocketResponse::err("missing agent") };
            let coord = COORDINATOR.lock().unwrap();
            match coord.agents.get(&agent_id) {
                Some(agent) => SocketResponse::ok(Some(agent.status())),
                None => SocketResponse::err(format!("agent {} not found", agent_id)),
            }
        }
        _ => SocketResponse::err(format!("unknown request type '{}'", req.request_type)),
    }
}

fn handle_connection(mut stream: UnixStream) -> Result<()> {
    let mut reader = BufReader::new(&stream);
    let mut buffer = String::new();
    reader.read_line(&mut buffer).context("read request")?;

    if buffer.is_empty() {
        return Ok(());
    }

    let req: SocketRequest = serde_json::from_str(&buffer).context("parse request")?;
    let resp = handle_request(req);
    let text = serde_json::to_string(&resp)?;
    stream.write_all(text.as_bytes())?;
    stream.write_all(b"\n")?;
    Ok(())
}

/// internal helper: bind a UnixListener and set permissive permissions so
/// non-root clients can connect even if the server runs as root.
fn create_listener(path: &str) -> Result<UnixListener> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path).context("bind socket")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o666);
        std::fs::set_permissions(path, perms).context("chmod socket")?;
    }
    Ok(listener)
}

/// Start Unix socket server at given path
pub fn run_server(path: &str) -> Result<()> {
    std::fs::create_dir_all("/var/log/agentd")?;
    let _ = PERSISTENCE.init();
    let listener = create_listener(path)?;

    println!("Socket server listening on {}", path);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                std::thread::spawn(|| {
                    if let Err(e) = handle_connection(stream) {
                        eprintln!("connection error: {}", e);
                    }
                });
            }
            Err(e) => eprintln!("accept error: {}", e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::NamedTempFile;

    fn clear_store() {
        SANDBOXES.lock().unwrap().clear();
    }

    #[test]
    fn listener_permission_bits() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        let listener = create_listener(path).expect("bind");
        let metadata = fs::metadata(path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(metadata.permissions().mode() & 0o777, 0o666);
        }
        drop(listener);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_socket_response() {
        let resp = SocketResponse::ok(Some(json!({"test": "value"})));
        assert_eq!(resp.status, "ok");
        assert!(resp.result.is_some());
    }

    #[test]
    fn create_list_and_destroy() {
        clear_store();
        let resp = handle_request(SocketRequest {
            request_type: "create_sandbox".into(),
            ..Default::default()
        });
        assert_eq!(resp.status, "ok");
        let id = resp.result.unwrap()["sandbox"].as_u64().unwrap();

        let resp2 = handle_request(SocketRequest {
            request_type: "list".into(),
            ..Default::default()
        });
        assert_eq!(resp2.result.unwrap(), json!([id]));

        let resp3 = handle_request(SocketRequest {
            request_type: "destroy_sandbox".into(),
            sandbox: Some(id),
            ..Default::default()
        });
        assert_eq!(resp3.status, "ok");

        let resp4 = handle_request(SocketRequest {
            request_type: "list".into(),
            ..Default::default()
        });
        assert_eq!(resp4.result.unwrap(), json!([]));
    }

    #[test]
    fn run_and_invoke_via_socket() {
        clear_store();
        let resp = handle_request(SocketRequest {
            request_type: "create_sandbox".into(),
            ..Default::default()
        });
        assert_eq!(resp.status, "ok");
        let id = resp.result.unwrap()["sandbox"].as_u64().unwrap();

        {
            let mut store = SANDBOXES.lock().unwrap();
            let sb = store.get_mut(&id).unwrap();
            sb.register_tool(Box::new(EchoTool {}));
        }

        let run_resp = handle_request(SocketRequest {
            request_type: "run".into(),
            sandbox: Some(id),
            command: Some("echo hello".into()),
            ..Default::default()
        });
        if run_resp.status == "ok" {
            assert!(run_resp.result.unwrap()["output"].as_str().unwrap().contains("hello"));
        } else {
            eprintln!("run command failed as expected: {:?}", run_resp.error);
        }

        let inv_resp = handle_request(SocketRequest {
            request_type: "invoke_tool".into(),
            sandbox: Some(id),
            name: Some("echo".into()),
            input: Some(json!("yo")),
            ..Default::default()
        });
        assert_eq!(inv_resp.status, "ok");
        assert_eq!(inv_resp.result.unwrap(), json!({"echo": "\"yo\""}));
    }

    #[test]
    fn unknown_request_error() {
        clear_store();
        let resp = handle_request(SocketRequest {
            request_type: "foo".into(),
            ..Default::default()
        });
        assert_eq!(resp.status, "error");
    }
}
