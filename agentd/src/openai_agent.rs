use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use crate::tui::event::TuiEvent;

const MAX_TOOL_ROUNDS: usize = 64;

pub fn run(prompt: &str, api_key: &str, model: &str, socket_path: &str) -> Result<()> {
    // 1. Setup Sandbox and Container
    let create_sb = json!({ "request_type": "create_sandbox", "image": "alpine" });
    let sb_resp = crate::vertex_agent::socket_roundtrip(socket_path, &create_sb)?;
    let sandbox_id = crate::vertex_agent::parse_ok_field(&sb_resp, "sandbox")?;

    let create_ct = json!({ "request_type": "create_container", "sandbox": &sandbox_id });
    let ct_resp = crate::vertex_agent::socket_roundtrip(socket_path, &create_ct)?;
    let container_id = crate::vertex_agent::parse_ok_field(&ct_resp, "container")?;

    let client = reqwest::blocking::Client::new();
    let mut messages = vec![json!({ "role": "user", "content": prompt })];
    let tools = openai_tool_declarations();

    for _ in 0..MAX_TOOL_ROUNDS {
        let mut body = json!({
            "model": model,
            "messages": messages,
            "tools": tools,
        });

        if model.starts_with("o") {
            body.as_object_mut().unwrap().insert("max_completion_tokens".to_string(), json!(8192));
        } else {
            body.as_object_mut().unwrap().insert("max_tokens".to_string(), json!(4096));
        }

        let resp = client.post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(api_key)
            .json(&body)
            .send()?;

        let response_json: Value = resp.json()?;
        let message = &response_json["choices"][0]["message"];
        messages.push(message.clone());

        if let Some(content) = message["content"].as_str() { println!("{}", content); }

        if let Some(tool_calls) = message["tool_calls"].as_array() {
            for call in tool_calls {
                let id = call["id"].as_str().unwrap();
                let name = call["function"]["name"].as_str().unwrap();
                let args: Value = serde_json::from_str(call["function"]["arguments"].as_str().unwrap_or("{}"))?;

                let result = crate::vertex_agent::invoke_tool_via_socket(socket_path, &sandbox_id, &container_id, name, &args)?;
                
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": serde_json::to_string(&result)?
                }));
            }
        } else {
            break;
        }
    }
    Ok(())
}

fn openai_tool_declarations() -> Value {
    let mut openai_tools = Vec::new();
    for name in crate::tool_registry::list_all_tools() {
        openai_tools.push(json!({
            "type": "function",
            "function": {
                "name": name,
                "description": format!("Run {} tool", name),
                "parameters": { "type": "object", "properties": {} }
            }
        }));
    }
    json!(openai_tools)
}

pub fn stream_chat_custom(
    api_key: &str,
    model: &str,
    messages: &[Value],
    url: &str,
    tx: std::sync::mpsc::Sender<TuiEvent>,
) -> Result<()> {
    let client = reqwest::blocking::Client::new();
    
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": true,
    });

    // o1/o3 reasoning handling
    if model.starts_with("o") {
        // OpenAI o-series doesn't support 'max_tokens' or 'temperature' in some versions
        body.as_object_mut().unwrap().insert("max_completion_tokens".to_string(), json!(8192));
    } else {
        body.as_object_mut().unwrap().insert("max_tokens".to_string(), json!(4096));
        body.as_object_mut().unwrap().insert("temperature".to_string(), json!(0.7));
    }

    let resp = client.post(url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .context("OpenAI API Request")?;

    if !resp.status().is_success() {
        let _ = tx.send(TuiEvent::GeminiError(format!("OpenAI Error: {}", resp.text()?)));
        return Ok(());
    }

    let reader = BufReader::new(resp);
    for line in reader.lines() {
        let line = line?;
        if let Some(data) = line.strip_prefix("data: ") {
            if data.trim() == "[DONE]" { break; }
            if let Ok(json) = serde_json::from_str::<Value>(data) {
                if let Some(content) = json.pointer("/choices/0/delta/content").and_then(|v| v.as_str()) {
                    let _ = tx.send(TuiEvent::GeminiChunk(content.to_string()));
                }
            }
        }
    }

    let _ = tx.send(TuiEvent::GeminiDone);
    Ok(())
}