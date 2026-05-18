//! Skills system — domain-specific knowledge files that are automatically
//! injected into every agent's context at startup.
//!
//! Skills live in `~/.mowisai/skills/*.skill` (TOML format).
//! They are loaded at boot and prepended to the system instruction sent to the LLM.

pub mod creator;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ── Skill file format ─────────────────────────────────────────────────────────

/// A parsed `.skill` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub meta: SkillMeta,
    pub content: SkillContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    /// Unique identifier (slug). Used for filenames and references.
    pub name: String,
    /// Human-readable title shown in `skill list`.
    pub display_name: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// ISO date string when the skill was created.
    #[serde(default)]
    pub created: String,
    /// Author name or email.
    #[serde(default)]
    pub author: String,
    /// Descriptive tags (e.g. ["frontend", "react", "ui"]).
    #[serde(default)]
    pub tags: Vec<String>,
    /// If true, always inject this skill regardless of task context.
    #[serde(default = "yes")]
    pub always_load: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillContent {
    /// The actual instructions injected into the agent's system prompt.
    pub text: String,
}

fn default_version() -> String { "1".to_string() }
fn yes() -> bool { true }

impl Skill {
    /// Parse a `.skill` file from disk.
    pub fn from_file(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading skill file {}", path.display()))?;
        toml::from_str(&raw)
            .with_context(|| format!("parsing skill file {}", path.display()))
    }

    /// Write this skill to a `.skill` file.
    pub fn save_to(&self, dir: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let filename = format!("{}.skill", self.meta.name);
        let path = dir.join(&filename);
        let serialized = toml::to_string_pretty(self)
            .context("serializing skill")?;
        std::fs::write(&path, serialized)
            .with_context(|| format!("writing skill to {}", path.display()))?;
        Ok(path)
    }

    /// One-line summary for `skill list`.
    pub fn summary(&self) -> String {
        format!(
            "{:20} v{}  {}",
            self.meta.display_name,
            self.meta.version,
            self.meta.description,
        )
    }

    /// Format this skill's content for injection into an LLM system prompt.
    pub fn to_prompt_block(&self) -> String {
        format!(
            "## Skill: {} ({})\n{}\n",
            self.meta.display_name,
            self.meta.name,
            self.content.text.trim()
        )
    }
}

// ── SkillManager ─────────────────────────────────────────────────────────────

/// Manages the skill library on the host filesystem.
pub struct SkillManager {
    dir: PathBuf,
}

impl SkillManager {
    pub fn new() -> Self {
        Self { dir: skills_dir() }
    }

    pub fn with_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Load all `.skill` files from the skills directory.
    pub fn load_all(&self) -> Vec<Skill> {
        if !self.dir.exists() {
            return Vec::new();
        }
        let mut skills = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "skill").unwrap_or(false) {
                    match Skill::from_file(&path) {
                        Ok(s) => skills.push(s),
                        Err(e) => log::warn!("Skipping malformed skill {}: {}", path.display(), e),
                    }
                }
            }
        }
        // Deterministic order by name
        skills.sort_by(|a, b| a.meta.name.cmp(&b.meta.name));
        skills
    }

    /// Install (copy) a `.skill` file into the skills directory.
    pub fn install(&self, source_path: &Path) -> Result<PathBuf> {
        let skill = Skill::from_file(source_path)?;
        let dest = skill.save_to(&self.dir)?;
        Ok(dest)
    }

    /// Remove a skill by name.
    pub fn remove(&self, name: &str) -> Result<()> {
        let path = self.dir.join(format!("{}.skill", name));
        if !path.exists() {
            anyhow::bail!("skill '{}' not found in {}", name, self.dir.display());
        }
        std::fs::remove_file(&path)
            .with_context(|| format!("removing {}", path.display()))
    }

    /// Save a skill to the skills directory.
    pub fn save(&self, skill: &Skill) -> Result<PathBuf> {
        skill.save_to(&self.dir)
    }

    /// Load a single skill by name.
    pub fn get(&self, name: &str) -> Option<Skill> {
        let path = self.dir.join(format!("{}.skill", name));
        Skill::from_file(&path).ok()
    }

    pub fn dir(&self) -> &Path { &self.dir }
}

/// Return the skills directory: `~/.mowisai/skills/`
pub fn skills_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".mowisai")
        .join("skills")
}

/// Build the system-prompt injection block from all loaded skills.
/// Returns an empty string if no skills are loaded.
pub fn build_skills_context(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let blocks: Vec<String> = skills
        .iter()
        .filter(|s| s.meta.always_load)
        .map(|s| s.to_prompt_block())
        .collect();
    if blocks.is_empty() {
        return String::new();
    }
    format!(
        "\n\n---\n# Loaded Skills\nThe following skills contain domain-specific rules that you MUST follow. They are the authoritative source of truth for their domains.\n\n{}\n---",
        blocks.join("\n")
    )
}
