//! Agent loop: Groq ↔ agentd Unix socket (sandbox tools).
//!
//! Uses the OpenAI-compatible Groq API at https://api.groq.com/openai/v1.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::time::Duration;
use crate::tui::event::TuiEvent;

const GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";

/// Stream a single-turn chat message to Groq and forward chunks via `tx`.
pub fn stream_chat(
    api_key: &str,
    model: &str,
    messages: &[Value],
    tx: std::sync::mpsc::Sender<TuiEvent>,
) -> Result<()> {
    let url = format!("{}/chat/completions", GROQ_BASE_URL);

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .context("reqwest client")?;

    let body = json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "temperature": 0.5,
        "max_tokens": 8192
    });

    let resp = client
        .post(&url)
        .bearer_auth(api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .context("HTTP request to Groq")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        let _ = tx.send(TuiEvent::GeminiError(
            format!("Groq API error {}: {}", status, text),
        ));
        return Ok(());
    }

    let reader = BufReader::new(resp);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                let _ = tx.send(TuiEvent::GeminiError(format!("stream read error: {}", e)));
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
                    let _ = tx.send(TuiEvent::GeminiChunk(content.to_string()));
                }
            }
        }
    }

    let _ = tx.send(TuiEvent::GeminiDone);
    Ok(())
}