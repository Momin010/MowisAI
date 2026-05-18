//! Interactive skill creator.
//!
//! Walks the user through a series of questions and produces a `.skill` file.
//! Runs entirely on the host (no sandbox, no LLM API calls needed).

use super::{Skill, SkillContent, SkillMeta, SkillManager};
use anyhow::Result;
use std::io::{self, BufRead, Write};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn prompt(question: &str) -> String {
    print!("{}: ", question);
    io::stdout().flush().ok();
    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line).ok();
    line.trim().to_string()
}

fn prompt_default(question: &str, default: &str) -> String {
    print!("{} [{}]: ", question, default);
    io::stdout().flush().ok();
    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line).ok();
    let trimmed = line.trim().to_string();
    if trimmed.is_empty() { default.to_string() } else { trimmed }
}

fn prompt_multiline(instruction: &str) -> String {
    println!("{}", instruction);
    println!("(Enter each rule on its own line. Type a blank line when done.)");
    let stdin = io::stdin();
    let mut lines = Vec::new();
    for raw in stdin.lock().lines() {
        let line = raw.unwrap_or_default();
        if line.trim().is_empty() { break; }
        lines.push(line);
    }
    lines.join("\n")
}

fn separator() {
    println!("\n{}", "─".repeat(60));
}

// ── Main creator flow ─────────────────────────────────────────────────────────

/// Run the interactive skill creation wizard.
/// Returns the path where the skill was saved.
pub fn run_creator() -> Result<std::path::PathBuf> {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║         MowisAI  Skill  Creator          ║");
    println!("╚══════════════════════════════════════════╝");
    println!("\nThis wizard will help you create a .skill file.");
    println!("Skills are injected into every agent's context and act as the");
    println!("authoritative source of truth for a specific domain.\n");

    separator();
    println!("STEP 1 — Basic information");
    separator();

    let display_name = prompt("Skill name (e.g. UI/UX Style Guide, Python Conventions)");
    if display_name.is_empty() {
        anyhow::bail!("Skill name cannot be empty.");
    }

    // Auto-generate slug from display name
    let suggested_slug = display_name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    let name = prompt_default("Skill ID (used as filename)", &suggested_slug);

    let description = prompt("One-line description");

    let author = prompt_default("Author name", &whoami());

    let tags_raw = prompt("Tags (comma-separated, e.g. frontend,react,typescript)");
    let tags: Vec<String> = tags_raw
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();

    separator();
    println!("STEP 2 — What should agents know?");
    separator();
    println!("\nDescribe the rules, preferences, and patterns agents must follow");
    println!("in this domain. Be specific — vague rules are ignored.\n");
    println!("Examples:");
    println!("  • Always use Tailwind CSS, never plain CSS");
    println!("  • Follow PEP 8 for Python code");
    println!("  • Every API response must include a 'request_id' field\n");

    let content_raw = prompt_multiline("Enter your skill rules:");

    if content_raw.trim().is_empty() {
        anyhow::bail!("Skill content cannot be empty.");
    }

    // Format the content nicely
    let formatted_content = format_content(&content_raw);

    separator();
    println!("STEP 3 — Review");
    separator();

    println!("\n  ID:          {}", name);
    println!("  Name:        {}", display_name);
    println!("  Description: {}", description);
    println!("  Author:      {}", author);
    if !tags.is_empty() {
        println!("  Tags:        {}", tags.join(", "));
    }
    println!("\n  Content preview:");
    for line in formatted_content.lines().take(8) {
        println!("    {}", line);
    }
    if formatted_content.lines().count() > 8 {
        println!("    ... ({} more lines)", formatted_content.lines().count() - 8);
    }

    println!();
    let confirm = prompt_default("Save this skill? (yes/no)", "yes");
    if !confirm.to_lowercase().starts_with('y') {
        anyhow::bail!("Skill creation cancelled.");
    }

    let created = chrono_now();

    let skill = Skill {
        meta: SkillMeta {
            name: name.clone(),
            display_name,
            version: "1".to_string(),
            description,
            created,
            author,
            tags,
            always_load: true,
        },
        content: SkillContent {
            text: formatted_content,
        },
    };

    let manager = SkillManager::new();
    let path = manager.save(&skill)?;

    println!("\n✓ Skill '{}' saved to: {}", name, path.display());
    println!("\nTo verify it's loaded, run:  agentd skills list");
    println!("The skill will be auto-injected into every agent from now on.\n");

    Ok(path)
}

/// Automatically number bare lines and format content as a clean list.
fn format_content(raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().collect();

    // If user already used bullets/numbers, keep their formatting
    let already_formatted = lines.iter().any(|l| {
        let t = l.trim();
        t.starts_with('-') || t.starts_with('*') || t.starts_with('#')
            || (t.len() > 2 && t.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false))
    });

    if already_formatted {
        return raw.trim().to_string();
    }

    // Otherwise auto-number
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{}. {}", i + 1, l.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn chrono_now() -> String {
    // Simple ISO date without a date library dependency
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Rough yyyy-mm-dd calculation (close enough for metadata)
    let days = secs / 86400;
    let mut year = 1970u32;
    let mut remaining_days = days as u32;
    loop {
        let days_in_year = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 366 } else { 365 };
        if remaining_days < days_in_year { break; }
        remaining_days -= days_in_year;
        year += 1;
    }
    let month_days = [31u32, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1u32;
    for &md in &month_days {
        let md = if month == 2 && year % 4 == 0 { 29 } else { md };
        if remaining_days < md { break; }
        remaining_days -= md;
        month += 1;
    }
    let day = remaining_days + 1;
    format!("{}-{:02}-{:02}", year, month, day)
}
