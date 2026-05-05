use crate::tui::event::TuiEvent;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};

const MAX_TOOL_ROUNDS: usize = 64;

pub fn run(prompt: &str, api_key: &str, model: &str, socket_path: &str) -> Result<()> {
    // 1. Setup Sandbox and Container
    let create_sb = json!({ "request_type": "create_sandbox", "image": "alpine" });
    let sb_resp = crate::vertex_agent::socket_roundtrip(socket_path, &create_sb)?;
    let sandbox_id = crate::vertex_agent::parse_ok_field(&sb_resp, "sandbox")?;

    let create_ct = json!({ "request_type": "create_container", "sandbox": &sandbox_id });
    let ct_resp = crate::vertex_agent::socket_roundtrip(socket_path, &create_ct)?;
    let container_id = crate::vertex_agent::parse_ok_field(&ct_resp, "container")?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let mut messages = vec![json!({
        "role": "user",
        "content": prompt
    })];

    let system_prompt = "You control a Linux sandbox via tools. Use paths under /workspace.";
    let tools = anthropic_tool_declarations();

    let result = (|| -> Result<()> {
        for _ in 0..MAX_TOOL_ROUNDS {
            let mut body = json!({
                "model": model,
                "system": system_prompt,
                "messages": messages,
                "max_tokens": 8192,
                "tools": tools,
            });

            if model.contains("3-7") {
                body.as_object_mut().context("body not object")?.insert(
                    "thinking".to_string(),
                    json!({"type": "enabled", "budget_tokens": 4096}),
                );
                body.as_object_mut()
                    .context("body not object")?
                    .insert("max_tokens".to_string(), json!(12288));
            }

            let resp = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send()
                .context("Anthropic Tool Request")?;

            if !resp.status().is_success() {
                return Err(anyhow::anyhow!(
                    "Anthropic Error {}: {}",
                    resp.status(),
                    resp.text()?
                ));
            }

            let response_json: Value = resp.json()?;
            let content_blocks = response_json["content"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("No content in Anthropic response"))?;

            messages.push(json!({
                "role": "assistant",
                "content": content_blocks
            }));

            let mut tool_results = Vec::new();
            for block in content_blocks {
                if block["type"] == "text" {
                    if let Some(text) = block["text"].as_str() {
                        println!("{}", text);
                    }
                } else if block["type"] == "tool_use" {
                    let tool_name = block["name"].as_str().unwrap_or("");
                    let tool_input = block["input"].clone();
                    let call_id = block["id"].as_str().unwrap_or("");

                    if tool_name.is_empty() {
                        continue;
                    }

                    log::info!("[anthropic] tool call: {} {:?}", tool_name, tool_input);

                    let result = crate::vertex_agent::invoke_tool_via_socket(
                        socket_path,
                        &sandbox_id,
                        &container_id,
                        tool_name,
                        &tool_input,
                    )?;

                    tool_results.push(json!({
                        "type": "tool_result",
                        "tool_use_id": call_id,
                        "content": serde_json::to_string(&result)?
                    }));
                }
            }

            if tool_results.is_empty() {
                break;
            }
            messages.push(json!({ "role": "user", "content": tool_results }));
        }
        Ok(())
    })();

    // Always clean up sandbox
    let destroy_req = json!({ "request_type": "destroy_sandbox", "sandbox": &sandbox_id });
    let _ = crate::vertex_agent::socket_roundtrip(socket_path, &destroy_req);

    result
}

fn anthropic_tool_declarations() -> Value {
    // Use the detailed Gemini tool declarations and convert to Anthropic format
    let gemini_tools = crate::vertex_agent::gemini_tool_declarations();
    let mut anthropic_tools = Vec::new();

    if let Some(declarations) = gemini_tools.as_array() {
        for decl in declarations {
            if let Some(funcs) = decl.get("functionDeclarations").and_then(|f| f.as_array()) {
                for func in funcs {
                    let name = func["name"].as_str().unwrap_or("unknown");
                    let desc = func["description"].as_str().unwrap_or("");
                    let params = func
                        .get("parameters")
                        .cloned()
                        .unwrap_or(json!({"type": "object", "properties": {}}));

                    anthropic_tools.push(json!({
                        "name": name,
                        "description": desc,
                        "input_schema": params
                    }));
                }
            }
        }
    }

    // Fallback
    if anthropic_tools.is_empty() {
        let tool_list = crate::tool_registry::list_all_tools();
        for name in tool_list {
            anthropic_tools.push(json!({
                "name": name,
                "description": format!("Execute {} tool", name),
                "input_schema": { "type": "object", "properties": {} }
            }));
        }
    }

    json!(anthropic_tools)
}

pub fn stream_chat(
    api_key: &str,
    model: &str,
    messages: &[Value],
    tx: std::sync::mpsc::Sender<TuiEvent>,
    _tools: Option<&Value>,
) -> Result<()> {
    use reqwest::blocking::Client;
    use std::time::Duration;

    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;

    let body = json!({
        "model": model,
        "max_tokens": 4096,
        "stream": true,
        "messages": messages
    });

    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .context("Anthropic streaming request")?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().unwrap_or_default();
        let _ = tx.send(TuiEvent::GeminiError(format!(
            "Anthropic API error {}: {}",
            status, text
        )));
        return Ok(());
    }

    let reader = BufReader::new(response);
    for line in reader.lines() {
        let line = line?;
        if line.starts_with("data: ") {
            let data = &line[6..];
            if let Ok(json) = serde_json::from_str::<Value>(data) {
                let event_type = json["type"].as_str().unwrap_or("");
                match event_type {
                    "content_block_delta" => {
                        if let Some(text) = json["delta"]["text"].as_str() {
                            let _ = tx.send(TuiEvent::GeminiChunk(text.to_string()));
                        }
                    }
                    "message_stop" => break,
                    _ => {}
                }
            }
        }
    }

    let _ = tx.send(TuiEvent::GeminiDone);
    Ok(())
}
