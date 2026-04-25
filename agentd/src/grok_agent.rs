//! Agent loop: xAI Grok ↔ agentd Unix socket (sandbox tools).
//!
//! Uses the OpenAI-compatible xAI API at https://api.x.ai/v1/chat/completions.
//! Tool-calling format mirrors the OpenAI function-calling spec.

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

const MAX_TOOL_ROUNDS: usize = 64;
const HTTP_TIMEOUT_SECS: u64 = 180;
const XAI_BASE_URL: &str = "https://api.x.ai/v1";

/// Run the Grok ↔ agentd tool loop until the model returns a final text answer.
pub fn run(prompt: &str, api_key: &str, model: &str, socket_path: &str) -> Result<()> {
    #[cfg(unix)]
    {
        run_inner(prompt, api_key, model, socket_path)
    }
    #[cfg(not(unix))]
    {
        let _ = (prompt, api_key, model, socket_path);
        Err(anyhow!("grok_agent requires Unix (agentd uses Unix domain sockets)"))
    }
}

#[cfg(unix)]
fn run_inner(prompt: &str, api_key: &str, model: &str, socket_path: &str) -> Result<()> {
    log::info!("[grok] creating sandbox via {} …", socket_path);
    let create_sb = json!({ "request_type": "create_sandbox", "image": "alpine" });
    let sb_resp = socket_roundtrip(socket_path, &create_sb)?;
    let sandbox_id = parse_ok_field(&sb_resp, "sandbox").context("create_sandbox")?;

    log::info!("[grok] creating container…");
    let create_ct = json!({ "request_type": "create_container", "sandbox": &sandbox_id });
    let ct_resp = socket_roundtrip(socket_path, &create_ct)?;
    let container_id = parse_ok_field(&ct_resp, "container").context("create_container")?;

    let url = format!("{}/chat/completions", XAI_BASE_URL);

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .context("reqwest client")?;

    let system_msg = json!({
        "role": "system",
        "content": "You control a Linux sandbox (Alpine) via tools. Use paths under /workspace \
                    when writing files unless the user specifies otherwise. Prefer listing \
                    directories before assuming files exist."
    });

    let mut messages: Vec<Value> = vec![
        system_msg,
        json!({ "role": "user", "content": prompt }),
    ];

    let tools = grok_tool_declarations();

    for round in 0..MAX_TOOL_ROUNDS {
        let body = json!({
            "model": model,
            "messages": messages,
            "tools": tools,
            "tool_choice": "auto",
            "temperature": 0.5
        });

        log::debug!("[grok] round {}: sending request", round);

        let resp = client
            .post(&url)
            .bearer_auth(api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .context("HTTP request to xAI")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(anyhow!("xAI API error {}: {}", status, text));
        }

        let response_json: Value = resp.json().context("parsing xAI response")?;

        let choice = response_json
            .pointer("/choices/0")
            .ok_or_else(|| anyhow!("no choices in xAI response"))?;

        let finish_reason = choice
            .pointer("/finish_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("stop");

        let message = choice
            .get("message")
            .ok_or_else(|| anyhow!("no message in xAI choice"))?;

        // Collect any text content
        if let Some(content) = message.get("content").and_then(|v| v.as_str()) {
            if !content.is_empty() {
                println!("{}", content);
            }
        }

        if finish_reason == "stop" || finish_reason == "end_turn" {
            log::info!("[grok] model finished (stop)");
            break;
        }

        // Handle tool calls
        let tool_calls = match message.get("tool_calls").and_then(|v| v.as_array()) {
            Some(tc) if !tc.is_empty() => tc.clone(),
            _ => break,
        };

        // Add the assistant message with tool_calls to history
        messages.push(message.clone());

        for tool_call in &tool_calls {
            let call_id = tool_call
                .pointer("/id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let fn_name = tool_call
                .pointer("/function/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let fn_args_str = tool_call
                .pointer("/function/arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");

            let fn_args: Value = serde_json::from_str(fn_args_str)
                .unwrap_or_else(|_| json!({}));

            log::info!("[grok] tool call: {} {:?}", fn_name, fn_args);

            let socket_req = build_socket_request(fn_name, &fn_args, &sandbox_id, &container_id);
            let tool_result = match socket_roundtrip(socket_path, &socket_req) {
                Ok(r) => serde_json::to_string_pretty(&r).unwrap_or_else(|_| r.to_string()),
                Err(e) => format!("{{\"error\": \"{}\"}}", e),
            };

            log::info!("[grok] tool result: {:.200}", tool_result);

            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": tool_result
            }));
        }
    }

    log::info!("[grok] cleaning up sandbox {}", sandbox_id);
    let _ = socket_roundtrip(
        socket_path,
        &json!({ "request_type": "destroy_sandbox", "sandbox": sandbox_id }),
    );

    Ok(())
}

// ── Tool declarations (OpenAI function-calling format) ────────────────────────

fn grok_tool_declarations() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a file from the sandbox.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path to read" }
                    },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Write content to a file in the sandbox.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path":    { "type": "string", "description": "File path" },
                        "content": { "type": "string", "description": "Content to write" }
                    },
                    "required": ["path", "content"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "run_command",
                "description": "Execute a shell command in the sandbox.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Shell command to run" }
                    },
                    "required": ["command"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_directory",
                "description": "List files and directories.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Directory path" }
                    },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "delete_file",
                "description": "Delete a file from the sandbox.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path to delete" }
                    },
                    "required": ["path"]
                }
            }
        }
    ])
}

// ── Socket helpers ────────────────────────────────────────────────────────────

fn build_socket_request(fn_name: &str, args: &Value, sandbox_id: &str, container_id: &str) -> Value {
    match fn_name {
        "read_file" => json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "tool": "read_file",
            "args": { "path": args.get("path").and_then(|v| v.as_str()).unwrap_or("") }
        }),
        "write_file" => json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "tool": "write_file",
            "args": {
                "path":    args.get("path").and_then(|v| v.as_str()).unwrap_or(""),
                "content": args.get("content").and_then(|v| v.as_str()).unwrap_or("")
            }
        }),
        "run_command" => json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "tool": "run_command",
            "args": { "command": args.get("command").and_then(|v| v.as_str()).unwrap_or("") }
        }),
        "list_directory" => json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "tool": "list_directory",
            "args": { "path": args.get("path").and_then(|v| v.as_str()).unwrap_or(".") }
        }),
        "delete_file" => json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "tool": "delete_file",
            "args": { "path": args.get("path").and_then(|v| v.as_str()).unwrap_or("") }
        }),
        other => json!({
            "request_type": "invoke_tool",
            "sandbox": sandbox_id,
            "container": container_id,
            "tool": other,
            "args": args
        }),
    }
}

#[cfg(unix)]
fn socket_roundtrip(socket_path: &str, req: &Value) -> Result<Value> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path)
        .with_context(|| format!("connecting to socket {}", socket_path))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(HTTP_TIMEOUT_SECS)))
        .context("set_read_timeout")?;

    let payload = serde_json::to_string(req).context("serialize request")?;
    stream
        .write_all(payload.as_bytes())
        .context("socket write")?;
    stream.write_all(b"\n").context("socket write newline")?;

    let reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .lines()
        .next()
        .ok_or_else(|| anyhow!("empty socket response"))?
        .context("socket read")?
        .clone_into(&mut line);

    serde_json::from_str(&line).context("parse socket response JSON")
}

fn parse_ok_field(resp: &Value, field: &str) -> Result<String> {
    if let Some(e) = resp.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!("socket error: {}", e));
    }
    resp.get(field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("missing '{}' field in response: {}", field, resp))
}

// ── Streaming chat helper (used by TUI) ───────────────────────────────────────

/// Stream a single-turn chat message to Grok and forward chunks via `tx`.
/// Used by the TUI chat mode — not the full tool-calling agent loop.
pub fn stream_chat(
    api_key: &str,
    model: &str,
    messages: &[Value],
    tx: std::sync::mpsc::Sender<crate::tui::event::TuiEvent>,
) -> Result<()> {
    let url = format!("{}/chat/completions", XAI_BASE_URL);

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .context("reqwest client")?;

    let body = json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "temperature": 0.7,
        "max_tokens": 16384
    });

    let resp = client
        .post(&url)
        .bearer_auth(api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .context("HTTP request to xAI")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        let _ = tx.send(crate::tui::event::TuiEvent::GeminiError(
            format!("xAI API error {}: {}", status, text),
        ));
        return Ok(());
    }

    let reader = BufReader::new(resp);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                let _ = tx.send(crate::tui::event::TuiEvent::GeminiError(
                    format!("stream read error: {}", e),
                ));
                return Ok(());
            }
        };

        let data = match line.strip_prefix("data: ") {
            Some(d) => d,
            None => continue,
        };

        if data.trim().is_empty() || data.trim() == "[DONE]" {
            continue;
        }

        if let Ok(json) = serde_json::from_str::<Value>(data) {
            if let Some(content) = json
                .pointer("/choices/0/delta/content")
                .and_then(|v| v.as_str())
            {
                if !content.is_empty() {
                    let _ = tx.send(crate::tui::event::TuiEvent::GeminiChunk(content.to_string()));
                }
            }
        }
    }

    let _ = tx.send(crate::tui::event::TuiEvent::GeminiDone);
    Ok(())
}
