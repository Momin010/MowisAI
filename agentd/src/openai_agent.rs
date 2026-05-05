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
    let mut messages = vec![json!({ "role": "user", "content": prompt })];
    let tools = openai_tool_declarations();

    let result = (|| -> Result<()> {
        for _ in 0..MAX_TOOL_ROUNDS {
            let mut body = json!({
                "model": model,
                "messages": messages,
                "tools": tools,
            });

            // Correct model detection for o1/o3 series
            if model.starts_with("o1") || model.starts_with("o3") {
                body.as_object_mut()
                    .context("body not object")?
                    .insert("max_completion_tokens".to_string(), json!(16384));
            } else {
                body.as_object_mut()
                    .context("body not object")?
                    .insert("max_tokens".to_string(), json!(16384));
            }

            let resp = client
                .post("https://api.openai.com/v1/chat/completions")
                .bearer_auth(api_key)
                .json(&body)
                .send()
                .context("OpenAI API request")?;

            // Check HTTP status before parsing
            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().unwrap_or_default();
                return Err(anyhow::anyhow!("OpenAI API error {}: {}", status, text));
            }

            let response_json: Value = resp.json()?;
            let message = &response_json["choices"][0]["message"];
            messages.push(message.clone());

            if let Some(content) = message["content"].as_str() {
                println!("{}", content);
            }

            if let Some(tool_calls) = message["tool_calls"].as_array() {
                for call in tool_calls {
                    let id = call["id"].as_str().unwrap_or("unknown");
                    let name = call["function"]["name"].as_str().unwrap_or("unknown");
                    let args: Value = serde_json::from_str(
                        call["function"]["arguments"].as_str().unwrap_or("{}"),
                    )
                    .unwrap_or(json!({}));

                    let result = crate::vertex_agent::invoke_tool_via_socket(
                        socket_path,
                        &sandbox_id,
                        &container_id,
                        name,
                        &args,
                    )?;

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
    })();

    // Always clean up sandbox
    let destroy_req = json!({ "request_type": "destroy_sandbox", "sandbox": &sandbox_id });
    let _ = crate::vertex_agent::socket_roundtrip(socket_path, &destroy_req);

    result
}

fn openai_tool_declarations() -> Value {
    use serde_json::json;
    // Use the detailed Gemini tool declarations and convert to OpenAI format
    let gemini_tools = crate::vertex_agent::gemini_tool_declarations();
    let mut openai_tools = Vec::new();

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

                    openai_tools.push(json!({
                        "type": "function",
                        "function": {
                            "name": name,
                            "description": desc,
                            "parameters": params
                        }
                    }));
                }
            }
        }
    }

    // Fallback: if conversion failed, use simple list
    if openai_tools.is_empty() {
        for name in crate::tool_registry::list_all_tools() {
            openai_tools.push(json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": format!("Execute {} tool", name),
                    "parameters": { "type": "object", "properties": {} }
                }
            }));
        }
    }

    json!(openai_tools)
}

pub fn stream_chat_custom(
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

    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": true,
    });

    if model.starts_with("o1") || model.starts_with("o3") {
        body.as_object_mut()
            .unwrap()
            .insert("max_completion_tokens".to_string(), json!(8192));
    } else {
        body.as_object_mut()
            .unwrap()
            .insert("max_tokens".to_string(), json!(4096));
    }

    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .context("OpenAI streaming request")?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().unwrap_or_default();
        let _ = tx.send(TuiEvent::GeminiError(format!(
            "OpenAI API error {}: {}",
            status, text
        )));
        return Ok(());
    }

    let reader = BufReader::new(response);
    for line in reader.lines() {
        let line = line?;
        if line.starts_with("data: ") {
            let data = &line[6..];
            if data.trim() == "[DONE]" {
                break;
            }
            if let Ok(json) = serde_json::from_str::<Value>(data) {
                if let Some(delta) = json["choices"][0]["delta"]["content"].as_str() {
                    let _ = tx.send(TuiEvent::GeminiChunk(delta.to_string()));
                }
            }
        }
    }

    let _ = tx.send(TuiEvent::GeminiDone);
    Ok(())
}
