use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use crate::tui::event::TuiEvent;

const MAX_TOOL_ROUNDS: usize = 64;

/// Run the Anthropic tool loop until completion.
pub fn run(prompt: &str, api_key: &str, model: &str, socket_path: &str) -> Result<()> {
    // 1. Setup Sandbox and Container
    let create_sb = json!({ "request_type": "create_sandbox", "image": "alpine" });
    let sb_resp = crate::vertex_agent::socket_roundtrip(socket_path, &create_sb)?;
    let sandbox_id = crate::vertex_agent::parse_ok_field(&sb_resp, "sandbox")?;

    let create_ct = json!({ "request_type": "create_container", "sandbox": &sandbox_id });
    let ct_resp = crate::vertex_agent::socket_roundtrip(socket_path, &create_ct)?;
    let container_id = crate::vertex_agent::parse_ok_field(&ct_resp, "container")?;

    // 2. Initialize Conversation
    let client = reqwest::blocking::Client::new();
    let mut messages = vec![json!({
        "role": "user",
        "content": prompt
    })];

    let system_prompt = "You control a Linux sandbox via tools. Use paths under /workspace.";
    let tools = anthropic_tool_declarations();

    for _ in 0..MAX_TOOL_ROUNDS {
        let mut body = json!({
            "model": model,
            "system": system_prompt,
            "messages": messages,
            "max_tokens": 8192,
            "tools": tools,
        });

        if model.contains("3-7") {
            body.as_object_mut().unwrap().insert("thinking".to_string(), json!({"type": "enabled", "budget_tokens": 4096}));
            body.as_object_mut().unwrap().insert("max_tokens".to_string(), json!(12288));
        }

        let resp = client.post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .context("Anthropic Tool Request")?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Anthropic Error: {}", resp.text()?));
        }

        let response_json: Value = resp.json()?;
        let content_blocks = response_json["content"].as_array().ok_or_else(|| anyhow::anyhow!("No content"))?;
        
        messages.push(json!({
            "role": "assistant",
            "content": content_blocks
        }));

        let mut tool_results = Vec::new();
        for block in content_blocks {
            if block["type"] == "text" {
                if let Some(text) = block["text"].as_str() { println!("{}", text); }
            } else if block["type"] == "tool_use" {
                let tool_name = block["name"].as_str().unwrap_or("");
                let tool_input = block["input"].clone();
                let call_id = block["id"].as_str().unwrap_or("");

                log::info!("[anthropic] tool call: {} {:?}", tool_name, tool_input);
                
                let result = crate::vertex_agent::invoke_tool_via_socket(socket_path, &sandbox_id, &container_id, tool_name, &tool_input)?;
                
                tool_results.push(json!({
                    "type": "tool_result",
                    "tool_use_id": call_id,
                    "content": serde_json::to_string(&result)?
                }));
            }
        }

        if tool_results.is_empty() { break; }
        messages.push(json!({ "role": "user", "content": tool_results }));
    }
    Ok(())
}

fn anthropic_tool_declarations() -> Value {
    // Maps the 75 project tools to Anthropic's input_schema format
    let mut anthropic_tools = Vec::new();
    let tool_list = crate::tool_registry::list_all_tools();
    
    for name in tool_list {
        // In a full implementation, we'd pull the JSON schema from the registry.
        // For now, we stub the structure.
        anthropic_tools.push(json!({
            "name": name,
            "description": format!("Execute {} tool", name),
            "input_schema": { "type": "object", "properties": {} }
        }));
    }
    json!(anthropic_tools)
}

pub fn stream_chat(
    api_key: &str,
    model: &str,
    messages: &[Value],
    tx: std::sync::mpsc::Sender<TuiEvent>,
) -> Result<()> {
    let client = reqwest::blocking::Client::new();
    
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "max_tokens": 8192,
    });

    // Support Anthropic Thinking for 3.7 Sonnet
    if model.contains("3-7") {
        body.as_object_mut().unwrap().insert("thinking".to_string(), json!({
            "type": "enabled",
            "budget_tokens": 4096
        }));
        // max_tokens must be greater than thinking budget
        body.as_object_mut().unwrap().insert("max_tokens".to_string(), json!(12288));
    }

    let resp = client.post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .context("Anthropic Request")?;

    if !resp.status().is_success() {
        let _ = tx.send(TuiEvent::GeminiError(format!("Anthropic Error {}: {}", resp.status(), resp.text()?)));
        return Ok(());
    }

    let reader = BufReader::new(resp);
    for line in reader.lines() {
        let line = line?;
        if let Some(data) = line.strip_prefix("data: ") {
            if let Ok(event) = serde_json::from_str::<Value>(data) {
                if let Some(text) = event.pointer("/delta/text").and_then(|v| v.as_str()) {
                    let _ = tx.send(TuiEvent::GeminiChunk(text.to_string()));
                }
                // Handle thinking blocks if visible in delta
                if let Some(thinking) = event.pointer("/delta/thinking").and_then(|v| v.as_str()) {
                    let _ = tx.send(TuiEvent::GeminiChunk(format!("\n[Thinking] {}", thinking)));
                }
            }
        }
    }

    let _ = tx.send(TuiEvent::GeminiDone);
    Ok(())
}