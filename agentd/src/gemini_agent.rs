use crate::tui::event::TuiEvent;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};

pub fn run(prompt: &str, api_key: &str, model: &str, socket_path: &str) -> Result<()> {
    // 1. Setup Sandbox and Container
    let create_sb = json!({ "request_type": "create_sandbox", "image": "alpine" });
    let sb_resp = crate::vertex_agent::socket_roundtrip(socket_path, &create_sb)?;
    let sandbox_id = crate::vertex_agent::parse_ok_field(&sb_resp, "sandbox")?;

    let create_ct = json!({ "request_type": "create_container", "sandbox": &sandbox_id });
    let ct_resp = crate::vertex_agent::socket_roundtrip(socket_path, &create_ct)?;
    let container_id = crate::vertex_agent::parse_ok_field(&ct_resp, "container")?;

    // Use header-based auth instead of URL query parameter
    let client = reqwest::blocking::Client::new();
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
        model
    );

    let mut contents = vec![json!({
        "role": "user",
        "parts": [{ "text": prompt }]
    })];

    let tools =
        json!([{ "functionDeclarations": crate::vertex_agent::gemini_tool_declarations() }]);

    let result = (|| -> Result<()> {
        for _ in 0..64 {
            let body = json!({ "contents": contents, "tools": tools });
            let resp = client
                .post(&url)
                .header("x-goog-api-key", api_key)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .context("Gemini API request")?;

            if !resp.status().is_success() {
                return Err(anyhow::anyhow!(
                    "Gemini API error {}: {}",
                    resp.status(),
                    resp.text()?
                ));
            }

            let response_json: Value = resp.json()?;
            let candidate = &response_json["candidates"][0];

            // Safe extraction - don't unwrap
            let message_parts = candidate["content"]["parts"].as_array().ok_or_else(|| {
                anyhow::anyhow!(
                    "Gemini: missing content.parts in response (possibly safety blocked)"
                )
            })?;

            contents.push(candidate["content"].clone());

            let mut tool_responses = Vec::new();
            for part in message_parts {
                if let Some(text) = part["text"].as_str() {
                    println!("{}", text);
                }
                if let Some(call) = part.get("functionCall") {
                    let name = call["name"].as_str().unwrap_or("unknown");
                    let args = call["args"].clone();

                    let result = crate::vertex_agent::invoke_tool_via_socket(
                        socket_path,
                        &sandbox_id,
                        &container_id,
                        name,
                        &args,
                    )?;

                    tool_responses.push(json!({
                        "functionResponse": {
                            "name": name,
                            "response": result
                        }
                    }));
                }
            }

            if tool_responses.is_empty() {
                break;
            }
            contents.push(json!({ "role": "user", "parts": tool_responses }));
        }
        Ok(())
    })();

    // Always clean up sandbox
    let destroy_req = json!({ "request_type": "destroy_sandbox", "sandbox": &sandbox_id });
    let _ = crate::vertex_agent::socket_roundtrip(socket_path, &destroy_req);

    result
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

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse",
        model
    );

    let contents: Vec<Value> = messages
        .iter()
        .map(|m| {
            let role = m["role"].as_str().unwrap_or("user");
            let text = m["content"].as_str().unwrap_or("");
            json!({ "role": role, "parts": [{ "text": text }] })
        })
        .collect();

    let body = json!({ "contents": contents });

    let response = client
        .post(&url)
        .header("x-goog-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .context("Gemini streaming request")?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().unwrap_or_default();
        let _ = tx.send(TuiEvent::GeminiError(format!(
            "Gemini API error {}: {}",
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
                if let Some(text) = json["candidates"][0]["content"]["parts"][0]["text"].as_str() {
                    let _ = tx.send(TuiEvent::GeminiChunk(text.to_string()));
                }
            }
        }
    }

    let _ = tx.send(TuiEvent::GeminiDone);
    Ok(())
}
