use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use crate::tui::event::TuiEvent;

pub fn run(prompt: &str, api_key: &str, model: &str, socket_path: &str) -> Result<()> {
    // 1. Setup Sandbox and Container
    let create_sb = json!({ "request_type": "create_sandbox", "image": "alpine" });
    let sb_resp = crate::vertex_agent::socket_roundtrip(socket_path, &create_sb)?;
    let sandbox_id = crate::vertex_agent::parse_ok_field(&sb_resp, "sandbox")?;

    let create_ct = json!({ "request_type": "create_container", "sandbox": &sandbox_id });
    let ct_resp = crate::vertex_agent::socket_roundtrip(socket_path, &create_ct)?;
    let container_id = crate::vertex_agent::parse_ok_field(&ct_resp, "container")?;

    let client = reqwest::blocking::Client::new();
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}", model, api_key);
    
    let mut contents = vec![json!({
        "role": "user",
        "parts": [{ "text": prompt }]
    })];

    let tools = json!([{ "functionDeclarations": crate::vertex_agent::gemini_tool_declarations() }]);

    for _ in 0..64 {
        let body = json!({ "contents": contents, "tools": tools });
        let resp = client.post(&url).json(&body).send()?;
        let response_json: Value = resp.json()?;

        let candidate = &response_json["candidates"][0];
        let message_parts = candidate["content"]["parts"].as_array().unwrap();
        contents.push(candidate["content"].clone());

        let mut tool_responses = Vec::new();
        for part in message_parts {
            if let Some(text) = part["text"].as_str() { println!("{}", text); }
            if let Some(call) = part.get("functionCall") {
                let name = call["name"].as_str().unwrap();
                let args = call["args"].clone();
                
                let result = crate::vertex_agent::invoke_tool_via_socket(socket_path, &sandbox_id, &container_id, name, &args)?;
                
                tool_responses.push(json!({
                    "functionResponse": {
                        "name": name,
                        "response": result
                    }
                }));
            }
        }

        if tool_responses.is_empty() { break; }
        contents.push(json!({ "role": "user", "parts": tool_responses }));
    }
    Ok(())
}

pub fn stream_chat(
    api_key: &str,
    model: &str,
    contents: &[Value],
    tx: std::sync::mpsc::Sender<TuiEvent>,
) -> Result<()> {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?key={}",
        model, api_key
    );

    let client = reqwest::blocking::Client::new();
    let body = json!({
        "contents": contents,
        "generationConfig": {
            "temperature": 0.7,
            "maxOutputTokens": 8192
        }
    });

    let resp = client.post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .context("Gemini API Request")?;

    if !resp.status().is_success() {
        let _ = tx.send(TuiEvent::GeminiError(format!("Gemini Error: {}", resp.text()?)));
        return Ok(());
    }

    let reader = BufReader::new(resp);
    for line in reader.lines() {
        let line = line?;
        // Gemini SSE format is different; usually returns objects in a list or chunks
        if line.starts_with("data: ") {
            let data = &line[6..];
            if let Ok(json) = serde_json::from_str::<Value>(data) {
                if let Some(text) = json.pointer("/candidates/0/content/parts/0/text").and_then(|v| v.as_str()) {
                    let _ = tx.send(TuiEvent::GeminiChunk(text.to_string()));
                }
            }
        }
    }

    let _ = tx.send(TuiEvent::GeminiDone);
    Ok(())
}