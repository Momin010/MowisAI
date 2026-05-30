//! vsock server loop.
//!
//! Listens on a fixed vsock port, accepts one host connection at a time
//! (the MVP architecture has one host driver), and serves the
//! `mowis-protocol` envelope stream.

use std::sync::Arc;

use anyhow::{Context, Result};
use dashmap::DashMap;
use mowis_protocol::{Envelope, ExecRequest, Payload, SandboxInfo, SandboxSpec};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tokio_vsock::{VsockAddr, VsockListener, VsockStream, VMADDR_CID_ANY};

use crate::sandbox::Sandbox;
use crate::tools;

// Interactive shell state
static INTERACTIVE_STDIN: tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<Vec<u8>>>> =
    tokio::sync::Mutex::const_new(None);
static INTERACTIVE_WAITING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static INTERACTIVE_PROMPT: tokio::sync::Mutex<String> =
    tokio::sync::Mutex::const_new(String::new());

type SandboxMap = Arc<DashMap<String, Arc<Sandbox>>>;

pub async fn serve(port: u32) -> Result<()> {
    let mut listener = VsockListener::bind(VsockAddr::new(VMADDR_CID_ANY, port))
        .with_context(|| format!("bind vsock port {port}"))?;
    tracing::info!(port, "listening on vsock");

    let sandboxes: SandboxMap = Arc::new(DashMap::new());

    loop {
        let (stream, addr) = listener.accept().await.context("accept vsock conn")?;
        tracing::info!(?addr, "host connected");
        let sandboxes = sandboxes.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, sandboxes).await {
                tracing::warn!(error = %e, "connection terminated");
            }
        });
    }
}

async fn handle_connection(stream: VsockStream, sandboxes: SandboxMap) -> Result<()> {
    let (read_half, write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);
    let writer = Arc::new(Mutex::new(write_half));

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            tracing::info!("host disconnected");
            return Ok(());
        }
        let env = match Envelope::from_line(&line) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(error = %e, raw = %line.trim_end(), "decode failed");
                send(&writer, Envelope::new(0, Payload::Error { message: e.to_string() })).await?;
                continue;
            }
        };
        let writer = writer.clone();
        let sandboxes = sandboxes.clone();
        tokio::spawn(async move {
            if let Err(e) = dispatch(env, writer, sandboxes).await {
                tracing::warn!(error = %e, "dispatch failed");
            }
        });
    }
}

async fn dispatch(
    env: Envelope,
    writer: Arc<Mutex<tokio::io::WriteHalf<VsockStream>>>,
    sandboxes: SandboxMap,
) -> Result<()> {
    let id = env.id;
    match env.payload {
        Payload::Ping => {
            send(
                &writer,
                Envelope::new(
                    id,
                    Payload::Pong {
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        protocol: mowis_protocol::PROTOCOL_VERSION,
                    },
                ),
            )
            .await?;
        }
        Payload::Version => {
            send(
                &writer,
                Envelope::new(
                    id,
                    Payload::Pong {
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        protocol: mowis_protocol::PROTOCOL_VERSION,
                    },
                ),
            )
            .await?;
        }
        Payload::Shutdown => {
            send(&writer, Envelope::new(id, Payload::ExitCode { code: 0 })).await?;
            tracing::info!("shutdown requested");
            std::process::exit(0);
        }
        Payload::CreateSandbox(spec) => create_sandbox(id, spec, &writer, &sandboxes).await?,
        Payload::DestroySandbox { sandbox_id } => {
            if sandboxes.remove(&sandbox_id).is_some() {
                send(
                    &writer,
                    Envelope::new(id, Payload::SandboxDestroyed { sandbox_id }),
                )
                .await?;
            } else {
                send_error(&writer, id, format!("no sandbox `{sandbox_id}`")).await?;
            }
        }
        Payload::ListSandboxes => {
            let listing: Vec<SandboxInfo> = sandboxes
                .iter()
                .map(|e| SandboxInfo {
                    sandbox_id: e.key().clone(),
                    root_path: e.value().root_path().display().to_string(),
                })
                .collect();
            send(
                &writer,
                Envelope::new(
                    id,
                    Payload::SandboxList {
                        sandboxes: listing,
                    },
                ),
            )
            .await?;
        }
        Payload::Exec(req) => exec(id, req, &writer, &sandboxes).await?,
        Payload::InvokeTool {
            sandbox_id,
            tool,
            input,
        } => {
            let sb = match sandboxes.get(&sandbox_id) {
                Some(s) => s.clone(),
                None => {
                    send_error(&writer, id, format!("no sandbox `{sandbox_id}`")).await?;
                    return Ok(());
                }
            };
            match tokio::task::spawn_blocking(move || tools::invoke(&sb, &tool, input)).await? {
                Ok(output) => {
                    send(&writer, Envelope::new(id, Payload::ToolResult { output })).await?;
                }
                Err(e) => send_error(&writer, id, e.to_string()).await?,
            }
        }
        Payload::CreateAgentOverlay {
            parent_sandbox_id,
            agent_id,
            limits,
        } => {
            let parent = match sandboxes.get(&parent_sandbox_id) {
                Some(s) => s.clone(),
                None => {
                    send_error(&writer, id, format!("no sandbox `{parent_sandbox_id}`")).await?;
                    return Ok(());
                }
            };
            let agent_id_clone = agent_id.clone();
            match tokio::task::spawn_blocking(move || {
                crate::sandbox::Sandbox::create_agent_overlay(&parent, &agent_id_clone, limits)
            })
            .await?
            {
                Ok(overlay_sb) => {
                    let overlay_id = overlay_sb.id.clone();
                    sandboxes.insert(overlay_id, Arc::new(overlay_sb));
                    send(
                        &writer,
                        Envelope::new(id, Payload::AgentOverlayCreated { agent_id }),
                    )
                    .await?;
                }
                Err(e) => send_error(&writer, id, e.to_string()).await?,
            }
        }
        Payload::MergeAgentOverlay {
            parent_sandbox_id,
            agent_id,
        } => {
            let parent = match sandboxes.get(&parent_sandbox_id) {
                Some(s) => s.clone(),
                None => {
                    send_error(&writer, id, format!("no sandbox `{parent_sandbox_id}`")).await?;
                    return Ok(());
                }
            };
            let overlay_key = format!("{}:{}", parent_sandbox_id, agent_id);
            let overlay = match sandboxes.get(&overlay_key) {
                Some(s) => s.clone(),
                None => {
                    send_error(&writer, id, format!("no agent overlay `{agent_id}`")).await?;
                    return Ok(());
                }
            };
            let agent_id_clone = agent_id.clone();
            match tokio::task::spawn_blocking(move || {
                crate::sandbox::merge_overlay(&parent, &overlay)
            })
            .await?
            {
                Ok(changed_paths) => {
                    sandboxes.remove(&overlay_key);
                    send(
                        &writer,
                        Envelope::new(
                            id,
                            Payload::AgentOverlayMerged {
                                agent_id: agent_id_clone,
                                changed_paths,
                            },
                        ),
                    )
                    .await?;
                }
                Err(e) => send_error(&writer, id, e.to_string()).await?,
            }
        }
        Payload::DiscardAgentOverlay {
            parent_sandbox_id,
            agent_id,
        } => {
            let overlay_key = format!("{}:{}", parent_sandbox_id, agent_id);
            if sandboxes.remove(&overlay_key).is_some() {
                send(
                    &writer,
                    Envelope::new(id, Payload::AgentOverlayDiscarded { agent_id }),
                )
                .await?;
            } else {
                send_error(&writer, id, format!("no agent overlay `{agent_id}`")).await?;
            }
        }
        Payload::InvokeToolAsAgent {
            parent_sandbox_id,
            agent_id,
            tool,
            input,
            caller_tier: _,
        } => {
            let overlay_key = format!("{}:{}", parent_sandbox_id, agent_id);
            let sb = match sandboxes.get(&overlay_key) {
                Some(s) => s.clone(),
                None => {
                    send_error(&writer, id, format!("no agent overlay `{agent_id}`")).await?;
                    return Ok(());
                }
            };
            match tokio::task::spawn_blocking(move || tools::invoke(&sb, &tool, input)).await? {
                Ok(output) => {
                    send(&writer, Envelope::new(id, Payload::ToolResult { output })).await?;
                }
                Err(e) => send_error(&writer, id, e.to_string()).await?,
            }
        }
        Payload::UploadCodebase {
            sandbox_id,
            archive_b64,
            file_count,
        } => {
            let sb = match sandboxes.get(&sandbox_id) {
                Some(s) => s.clone(),
                None => {
                    send_error(&writer, id, format!("no sandbox `{sandbox_id}`")).await?;
                    return Ok(());
                }
            };
            match tokio::task::spawn_blocking(move || {
                upload_codebase(&sb, &archive_b64)
            })
            .await?
            {
                Ok(uploaded_count) => {
                    send(
                        &writer,
                        Envelope::new(
                            id,
                            Payload::CodebaseUploaded {
                                sandbox_id,
                                file_count: uploaded_count,
                            },
                        ),
                    )
                    .await?;
                }
                Err(e) => send_error(&writer, id, e.to_string()).await?,
            }
        }
        Payload::HealthCheck => {
            let sandbox_count = sandboxes.len();
            send(
                &writer,
                Envelope::new(
                    id,
                    Payload::HealthOk {
                        uptime_secs: 0,
                        sandbox_count,
                    },
                ),
            )
            .await?;
        }
        Payload::SendInput {
            sandbox_id,
            agent_id,
            input,
        } => {
            let overlay_key = format!("{}:{}", sandbox_id, agent_id);
            if sandboxes.contains_key(&overlay_key) {
                // Write input to the interactive stdin channel
                let mut stdin_lock = INTERACTIVE_STDIN.lock().await;
                if let Some(tx) = stdin_lock.as_ref() {
                    let _ = tx.send(format!("{}\n", input).into_bytes());
                    send(
                        &writer,
                        Envelope::new(id, Payload::ToolResult {
                            output: serde_json::json!({"success": true, "message": "input sent"}),
                        }),
                    )
                    .await?;
                } else {
                    send_error(&writer, id, "no interactive command running".into()).await?;
                }
            } else {
                send_error(&writer, id, format!("no agent overlay `{agent_id}`")).await?;
            }
        }
        Payload::InteractiveStatus {
            sandbox_id,
            agent_id,
        } => {
            let overlay_key = format!("{}:{}", sandbox_id, agent_id);
            if sandboxes.contains_key(&overlay_key) {
                let waiting = INTERACTIVE_WAITING.load(std::sync::atomic::Ordering::Relaxed);
                let prompt = INTERACTIVE_PROMPT.lock().await.clone();
                send(
                    &writer,
                    Envelope::new(
                        id,
                        Payload::InteractivePrompt {
                            agent_id,
                            prompt,
                            waiting,
                        },
                    ),
                )
                .await?;
            } else {
                send_error(&writer, id, format!("no agent overlay `{agent_id}`")).await?;
            }
        }
        // Guest-side payloads should never arrive from the host; ignore.
        Payload::Pong { .. }
        | Payload::SandboxCreated { .. }
        | Payload::SandboxDestroyed { .. }
        | Payload::SandboxList { .. }
        | Payload::Stdout { .. }
        | Payload::Stderr { .. }
        | Payload::ExitCode { .. }
        | Payload::ToolResult { .. }
        | Payload::AgentOverlayCreated { .. }
        | Payload::AgentOverlayMerged { .. }
        | Payload::AgentOverlayDiscarded { .. }
        | Payload::CodebaseUploaded { .. }
        | Payload::HealthOk { .. }
        | Payload::InteractivePrompt { .. }
        | Payload::Error { .. } => {
            tracing::warn!(?id, "received unexpected guest-side payload from host");
        }
    }
    Ok(())
}

async fn create_sandbox(
    id: u64,
    spec: SandboxSpec,
    writer: &Arc<Mutex<tokio::io::WriteHalf<VsockStream>>>,
    sandboxes: &SandboxMap,
) -> Result<()> {
    let rootfs: Option<std::path::PathBuf> = spec.image_rootfs.map(std::path::PathBuf::from);
    let requested_id = spec.sandbox_id;
    let limits = spec.limits;
    let sandbox = tokio::task::spawn_blocking(move || {
        Sandbox::create(requested_id, rootfs.as_deref(), limits)
    })
    .await?;
    match sandbox {
        Ok(sb) => {
            let sandbox_id = sb.id.clone();
            sandboxes.insert(sandbox_id.clone(), Arc::new(sb));
            send(
                writer,
                Envelope::new(id, Payload::SandboxCreated { sandbox_id }),
            )
            .await?;
        }
        Err(e) => send_error(writer, id, e.to_string()).await?,
    }
    Ok(())
}

async fn exec(
    id: u64,
    req: ExecRequest,
    writer: &Arc<Mutex<tokio::io::WriteHalf<VsockStream>>>,
    sandboxes: &SandboxMap,
) -> Result<()> {
    let result = if let Some(sid) = req.sandbox_id.clone() {
        let sb = match sandboxes.get(&sid) {
            Some(s) => s.clone(),
            None => {
                send_error(writer, id, format!("no sandbox `{sid}`")).await?;
                return Ok(());
            }
        };
        let req_clone = req.clone();
        tokio::task::spawn_blocking(move || sb.run_command(&req_clone.cmd, &req_clone.args, &req_clone.env))
            .await?
    } else {
        // Unsandboxed exec inside the VM (useful for setup/debug from host).
        tokio::task::spawn_blocking(move || {
            let mut cmd = std::process::Command::new(&req.cmd);
            cmd.args(&req.args);
            for (k, v) in &req.env {
                cmd.env(k, v);
            }
            let out = cmd.output()?;
            Ok::<_, anyhow::Error>((out.status.code().unwrap_or(-1), out.stdout, out.stderr))
        })
        .await?
    };

    match result {
        Ok((code, stdout, stderr)) => {
            if !stdout.is_empty() {
                send(
                    writer,
                    Envelope::new(
                        id,
                        Payload::Stdout {
                            data: String::from_utf8_lossy(&stdout).into_owned(),
                        },
                    ),
                )
                .await?;
            }
            if !stderr.is_empty() {
                send(
                    writer,
                    Envelope::new(
                        id,
                        Payload::Stderr {
                            data: String::from_utf8_lossy(&stderr).into_owned(),
                        },
                    ),
                )
                .await?;
            }
            send(writer, Envelope::new(id, Payload::ExitCode { code })).await?;
        }
        Err(e) => send_error(writer, id, e.to_string()).await?,
    }
    Ok(())
}

async fn send(
    writer: &Arc<Mutex<tokio::io::WriteHalf<VsockStream>>>,
    env: Envelope,
) -> Result<()> {
    let line = env.to_line()?;
    let mut w = writer.lock().await;
    w.write_all(line.as_bytes()).await?;
    w.flush().await?;
    Ok(())
}

async fn send_error(
    writer: &Arc<Mutex<tokio::io::WriteHalf<VsockStream>>>,
    id: u64,
    message: String,
) -> Result<()> {
    send(writer, Envelope::new(id, Payload::Error { message })).await
}

fn upload_codebase(sandbox: &Sandbox, archive_b64: &str) -> Result<u32> {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    use std::io::Read;

    let compressed = BASE64.decode(archive_b64).context("base64 decode archive")?;
    let mut decoder = flate2::read::GzDecoder::new(&compressed[..]);
    let mut tar_data = Vec::new();
    decoder.read_to_end(&mut tar_data).context("gzip decompress")?;

    let root = sandbox.root_path();
    let mut file_count = 0u32;
    let mut archive = tar::Archive::new(&tar_data[..]);
    for entry in archive.entries().context("read tar entries")? {
        let mut entry = entry.context("read tar entry")?;
        let path = entry.path().context("entry path")?.to_path_buf();
        // Security: reject absolute paths and path traversal
        if path.is_absolute()
            || path.components().any(|c| matches!(c, std::path::Component::ParentDir))
        {
            continue;
        }
        let dest = root.join(&path);
        entry.unpack(&dest).context("unpack entry")?;
        file_count += 1;
    }
    tracing::info!(path = %root.display(), files = file_count, "codebase uploaded");
    Ok(file_count)
}
