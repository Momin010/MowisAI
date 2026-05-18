//! LLM-driven skill creator.
//!
//! The LLM asks the user questions through natural conversation, then generates
//! a complete .skill TOML block wrapped in <skill>...</skill> tags.
//! The system detects those tags, parses the TOML, and saves the file.
//!
//! Works in two modes:
//!   - Terminal (run_llm_creator): blocking stdin/stdout loop
//!   - TUI (system prompt + tag detection in app.rs): embedded in chat

use super::{Skill, SkillManager};
use anyhow::Result;
use serde_json::{json, Value};
use std::io::{self, Write};
use std::path::PathBuf;

// ── System prompt given to the LLM when in skill-creator mode ────────────────

pub const SKILL_CREATOR_SYSTEM_PROMPT: &str = r#"You are MowisAI Skill Creator — an expert at capturing domain knowledge into concise, actionable skill files.

A .skill file is injected into every AI agent's system prompt as the authoritative source of truth for a domain. Good skills contain SPECIFIC, ACTIONABLE rules — not vague platitudes.

Your job:
1. Ask the user what domain their skill covers (UI/UX, Python conventions, database design, API standards, etc.)
2. Ask focused follow-up questions (3-5 total) to understand their SPECIFIC preferences
3. When you have enough information, generate the complete skill

Rules for good skill content:
- Specific beats vague ("Use Tailwind CSS" not "Write clean styles")
- Include library/framework preferences ("shadcn/ui", "Zod for validation")
- Capture non-negotiables and hard constraints
- Cover naming, structure, error handling, testing patterns where relevant

When you have enough information, output the TOML block wrapped EXACTLY like this (no extra text before or after the tags on those lines):

<skill>
[meta]
name = "slug-here"
display_name = "Human Readable Name"
version = "1"
description = "One-line description of what this skill governs"
created = "2026-05-18"
always_load = true
author = "user"
tags = ["tag1", "tag2"]

[content]
text = """
## Domain Name

Clear rules agents must follow:

1. First specific rule
2. Second specific rule
"""
</skill>

After generating the skill block, write one short sentence confirming what was captured.
Ask one or two questions at a time — not a long list. Be conversational."#;

// ── Tag extraction ────────────────────────────────────────────────────────────

/// Extract TOML content from `<skill>…</skill>` tags in an LLM response.
pub fn extract_skill_toml(text: &str) -> Option<String> {
    let start = text.find("<skill>")?;
    let end = text.find("</skill>")?;
    if end <= start { return None; }
    let inner = text[start + 7..end].trim().to_string();
    if inner.is_empty() { None } else { Some(inner) }
}

/// Try to parse a TOML string into a Skill.
pub fn parse_skill_from_toml(toml_str: &str) -> Result<Skill> {
    toml::from_str(toml_str).map_err(|e| anyhow::anyhow!("Failed to parse skill TOML: {}", e))
}

/// Detect, parse, and save a skill embedded in an LLM response.
/// Returns the saved path if a skill was found and saved successfully.
pub fn try_save_skill_from_response(response: &str) -> Option<Result<PathBuf>> {
    let toml_str = extract_skill_toml(response)?;
    let result = parse_skill_from_toml(&toml_str)
        .and_then(|skill| SkillManager::new().save(&skill));
    Some(result)
}

// ── Terminal (blocking) creator ───────────────────────────────────────────────

/// Run a full LLM-driven skill creation session in the terminal.
/// Requires an `LlmConfig` — reads from environment or config file.
pub fn run_llm_creator(
    llm_config: &crate::orchestration::provider_client::LlmConfig,
) -> Result<PathBuf> {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   MowisAI Skill Creator  (LLM-powered)   ║");
    println!("╚══════════════════════════════════════════╝");
    println!("\nThe AI will guide you through creating a .skill file.");
    println!("Type your answers naturally. Type /done when finished, /quit to cancel.\n");

    let rt = tokio::runtime::Runtime::new()?;
    let mut history: Vec<Value> = Vec::new();

    // Seed: ask the LLM to open the conversation
    history.push(json!({
        "role": "user",
        "content": "Let's create a skill."
    }));

    loop {
        // Send to LLM
        let response = rt.block_on(crate::orchestration::provider_client::generate_chat(
            llm_config,
            SKILL_CREATOR_SYSTEM_PROMPT,
            &history,
            0.7,
        ))?;

        // Record assistant turn
        history.push(json!({
            "role": "assistant",
            "content": response.clone()
        }));

        // Print the LLM's message (strip the <skill>…</skill> block for display)
        let display = if response.contains("<skill>") {
            let before = response.find("<skill>").map(|i| &response[..i]).unwrap_or("");
            let after = response.find("</skill>")
                .map(|i| response.get(i + 8..).unwrap_or(""))
                .unwrap_or("");
            format!("{}{}", before.trim_end(), after.trim_start()).trim().to_string()
        } else {
            response.clone()
        };

        println!("\nAI: {}\n", display);

        // Did the LLM produce a skill?
        if let Some(result) = try_save_skill_from_response(&response) {
            match result {
                Ok(path) => {
                    println!("✓ Skill saved to: {}", path.display());
                    println!("\nRun `agentd skills list` to confirm, or `/skill list` in the TUI.");
                    return Ok(path);
                }
                Err(e) => {
                    eprintln!("Warning: skill block found but failed to save: {}", e);
                    eprintln!("The LLM may have generated invalid TOML. Try again or fix manually.\n");
                }
            }
        }

        // Read user input
        print!("You: ");
        io::stdout().flush().ok();
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) | Err(_) => break,
            _ => {}
        }
        let trimmed = input.trim();
        if trimmed.is_empty() { continue; }
        if trimmed == "/quit" || trimmed == "/cancel" {
            anyhow::bail!("Skill creation cancelled.");
        }

        history.push(json!({
            "role": "user",
            "content": trimmed
        }));
    }

    anyhow::bail!("Skill creation ended without producing a skill.")
}
