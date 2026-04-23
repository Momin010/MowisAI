//! Pre-orchestration complexity classifier.
//!
//! Runs BEFORE the Gemini planner — pure heuristics, zero LLM cost, ~1ms.
//! Classifies the task into one of three modes that control which pipeline
//! layers fire:
//!
//! | Mode     | Layers used            | Typical cost          |
//! |----------|------------------------|-----------------------|
//! | Simple   | 2, 4                   | ~1 Gemini call (agent)|
//! | Standard | 1*, 2, 3, 4, 5, 6(x1) | ~3–4 Gemini calls     |
//! | Full     | 1–7 (current system)   | Current cost          |
//!
//! *Standard uses a constrained planner prompt (1 sandbox, ≤3 agents).
//!
//! The classifier can be overridden with an explicit user flag; see
//! `ComplexityMode::from_str` and the `--mode` CLI argument.

use std::collections::HashSet;

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Which orchestration mode to use for this task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComplexityMode {
    /// Single agent, no planner, no merge, no verification.
    Simple,
    /// Multi-agent within one sandbox, 1 verification round, no cross-sandbox.
    Standard,
    /// Full 7-layer pipeline — multiple sandboxes, full verification loop.
    Full,
}

impl ComplexityMode {
    /// Parse from a CLI string (`simple`, `standard`, `full`).
    /// Case-insensitive. Returns `None` on unknown value.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "simple" => Some(Self::Simple),
            "standard" => Some(Self::Standard),
            "full" => Some(Self::Full),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Standard => "standard",
            Self::Full => "full",
        }
    }
}

impl std::fmt::Display for ComplexityMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Intermediate scoring breakdown — useful for logging / debugging.
#[derive(Debug, Clone)]
pub struct ComplexityScore {
    /// Estimated number of distinct domains inferred from the directory tree
    pub domain_count: usize,
    /// Estimated file count from the directory tree
    pub file_count: usize,
    /// Whether the prompt contains broad-scope keywords
    pub broad_scope: bool,
    /// Whether the prompt contains cross-service / integration keywords
    pub cross_service: bool,
    /// Raw integer score [0, 2]
    pub score: u8,
    /// Final mode derived from score (before any user override)
    pub mode: ComplexityMode,
}

// ─────────────────────────────────────────────────────────────────────────────
// Domain directory names — top-level dirs that indicate separate concerns
// ─────────────────────────────────────────────────────────────────────────────

/// Top-level directory names that count as distinct "domains".
const DOMAIN_DIRS: &[&str] = &[
    "frontend", "backend", "api", "server", "client", "web", "mobile",
    "ios", "android", "infra", "infrastructure", "terraform", "k8s",
    "kubernetes", "docker", "services", "microservices", "packages",
    "apps", "libs", "shared", "common", "core", "platform",
];

// ─────────────────────────────────────────────────────────────────────────────
// Prompt keyword sets
// ─────────────────────────────────────────────────────────────────────────────

/// Broad-scope keywords in the prompt imply large surface area → raise score.
const BROAD_SCOPE_KEYWORDS: &[&str] = &[
    "entire", "all", "whole", "full", "complete", "migrate", "migration",
    "refactor", "rewrite", "architecture", "redesign", "overhaul",
    "system", "platform", "rebuild", "from scratch",
];

/// Cross-service keywords imply touching multiple independent systems → Full.
const CROSS_SERVICE_KEYWORDS: &[&str] = &[
    "integration", "end-to-end", "e2e", "cross-service", "cross service",
    "microservice", "service mesh", "api gateway", "inter-service",
    "frontend.*backend", "backend.*frontend", "full.?stack", "fullstack",
];

/// Single-action keywords in the prompt strongly suggest Mode 1 (Simple).
const SIMPLE_ACTION_KEYWORDS: &[&str] = &[
    "fix", "rename", "typo", "add a", "add an", "update a", "update an",
    "change a", "change an", "delete a", "delete an", "remove a", "remove an",
    "move a", "move an", "correct", "tweak", "adjust", "format",
    "comment", "uncomment", "patch",
];

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Classify the complexity of a task from the user prompt and the pre-scanned
/// directory tree string (the same string produced by [`planner::scan_directory_tree`]).
///
/// This function is pure (no I/O, no async) and runs in ~1ms.
pub fn classify_complexity(prompt: &str, dir_tree: &str) -> ComplexityScore {
    let prompt_lower = prompt.to_lowercase();

    // ── 1. File count ────────────────────────────────────────────────────────
    let file_count = count_files(dir_tree);

    // ── 2. Domain count ──────────────────────────────────────────────────────
    let domain_count = count_domains(dir_tree);

    // ── 3. Prompt signals ────────────────────────────────────────────────────
    let broad_scope = BROAD_SCOPE_KEYWORDS
        .iter()
        .any(|kw| prompt_lower.contains(kw));

    let cross_service = CROSS_SERVICE_KEYWORDS
        .iter()
        .any(|kw| regex_contains(&prompt_lower, kw));

    let simple_action = SIMPLE_ACTION_KEYWORDS
        .iter()
        .any(|kw| prompt_lower.contains(kw));

    // ── 4. Scoring ───────────────────────────────────────────────────────────
    // Cross-service immediately forces Full regardless of other signals.
    let score: u8 = if cross_service {
        2
    } else {
        let mut s: u8 = 0;

        // File count signal
        if file_count > 20 {
            s += 1;
        }
        if file_count > 50 {
            s += 1; // bumps to 2
        }

        // Domain count signal
        if domain_count >= 3 {
            s = s.max(2);
        } else if domain_count == 2 {
            s = s.max(1);
        }

        // Broad scope adds 1 point but won't make it Simple
        if broad_scope {
            s = s.max(1);
        }

        // Simple action with few files → force score down to 0
        if simple_action && file_count <= 20 && domain_count <= 1 {
            s = 0;
        }

        s.min(2)
    };

    let mode = match score {
        0 => ComplexityMode::Simple,
        1 => ComplexityMode::Standard,
        _ => ComplexityMode::Full,
    };

    ComplexityScore {
        domain_count,
        file_count,
        broad_scope,
        cross_service,
        score,
        mode,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Count files by looking for lines that don't end in `/`.
/// Good-enough proxy — we're doing heuristics, not auditing.
fn count_files(dir_tree: &str) -> usize {
    dir_tree
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return false;
            }
            // Strip tree decoration characters to get the bare name
            let name = trimmed
                .trim_start_matches(|c: char| {
                    c == '├' || c == '└' || c == '─' || c == '│' || c == ' '
                });
            // Keep only entries that don't look like pure directory lines
            !name.is_empty() && !name.ends_with('/')
        })
        .count()
}

/// Count distinct domain directories visible in the first two levels of the
/// tree output.  We only look at lines with depth ≤ 2 (0 or 1 level of
/// indentation in typical `tree -L 3` output).
fn count_domains(dir_tree: &str) -> usize {
    let mut seen: HashSet<String> = HashSet::new();

    for line in dir_tree.lines() {
        let trimmed = line.trim().to_lowercase();
        // Remove tree decorators (├──, └──, │, spaces)
        let clean = trimmed
            .trim_start_matches(|c: char| c == '├' || c == '└' || c == '─' || c == '│' || c == ' ')
            .trim_end_matches('/')
            .to_string();

        if DOMAIN_DIRS.contains(&clean.as_str()) {
            seen.insert(clean);
        }
    }

    seen.len()
}

/// Very lightweight "regex contains" — handles `.*` and `.?` patterns without
/// pulling in the regex crate. Only used for the small cross-service list.
fn regex_contains(text: &str, pattern: &str) -> bool {
    // If the pattern has no special chars, it's a plain substring match.
    if !pattern.contains(".*") && !pattern.contains(".?") {
        return text.contains(pattern);
    }

    // Otherwise split on `.*` and check that all parts appear in order.
    // This is intentionally simple — we only need to handle these specific patterns.
    let parts: Vec<&str> = pattern.split(".*").collect();
    if parts.len() == 1 {
        // Only `.?` wildcards — treat as optional single char between words
        let sub = pattern.replace(".?", "");
        return text.contains(&sub);
    }

    let mut pos = 0;
    for part in &parts {
        let sub = part.replace(".?", "");
        if sub.is_empty() {
            continue;
        }
        match text[pos..].find(&sub) {
            Some(idx) => pos += idx + sub.len(),
            None => return false,
        }
    }
    true
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tree(files: &[&str]) -> String {
        files.join("\n")
    }

    #[test]
    fn test_single_html_css_simple() {
        let tree = make_tree(&[
            "index.html",
            "styles.css",
            "script.js",
        ]);
        let score = classify_complexity("fix the nav typo", &tree);
        assert_eq!(score.mode, ComplexityMode::Simple);
        assert_eq!(score.score, 0);
    }

    #[test]
    fn test_small_project_add_feature_simple() {
        let tree = make_tree(&[
            "src/",
            "  main.rs",
            "  lib.rs",
            "Cargo.toml",
        ]);
        let score = classify_complexity("add a new CLI flag for verbose output", &tree);
        assert_eq!(score.mode, ComplexityMode::Simple);
    }

    #[test]
    fn test_medium_project_standard() {
        // ~15 files, single backend dir, moderate task
        let tree = make_tree(&[
            "backend/",
            "  src/",
            "    auth.rs", "    api.rs", "    db.rs", "    models.rs",
            "    routes.rs", "    middleware.rs", "    config.rs",
            "    error.rs", "    main.rs", "    lib.rs",
            "  Cargo.toml",
            "README.md",
            ".env.example",
        ]);
        let score = classify_complexity("add JWT authentication to the API", &tree);
        assert_eq!(score.mode, ComplexityMode::Standard);
    }

    #[test]
    fn test_full_pipeline_cross_service() {
        let tree = make_tree(&[
            "frontend/", "backend/", "infra/",
            "frontend/src/App.tsx",
            "backend/src/main.rs",
            "infra/terraform/main.tf",
        ]);
        let score = classify_complexity(
            "implement full-stack user authentication with frontend and backend integration",
            &tree,
        );
        assert_eq!(score.mode, ComplexityMode::Full);
    }

    #[test]
    fn test_full_pipeline_many_files() {
        let tree: Vec<String> = (0..60).map(|i| format!("src/module_{}.rs", i)).collect();
        let tree_str = tree.join("\n");
        let score = classify_complexity("refactor the entire module system", &tree_str);
        assert_eq!(score.mode, ComplexityMode::Full);
    }

    #[test]
    fn test_full_pipeline_multiple_domains() {
        let tree = make_tree(&[
            "frontend/", "backend/", "mobile/", "infra/",
        ]);
        let score = classify_complexity("update the API endpoint", &tree);
        // 4 domains → Full
        assert_eq!(score.mode, ComplexityMode::Full);
    }

    #[test]
    fn test_mode_from_str() {
        assert_eq!(ComplexityMode::from_str("simple"), Some(ComplexityMode::Simple));
        assert_eq!(ComplexityMode::from_str("STANDARD"), Some(ComplexityMode::Standard));
        assert_eq!(ComplexityMode::from_str("full"), Some(ComplexityMode::Full));
        assert_eq!(ComplexityMode::from_str("unknown"), None);
    }

    #[test]
    fn test_broad_scope_bumps_score() {
        let tree = make_tree(&["src/main.rs", "src/lib.rs"]);
        let score = classify_complexity("migrate the entire codebase to async", &tree);
        // broad scope + only 2 files → Standard (broad_scope prevents Simple)
        assert!(score.broad_scope);
        assert!(score.score >= 1);
    }

    #[test]
    fn test_simple_action_keeps_score_low() {
        let tree = make_tree(&["src/main.rs", "src/config.rs", "README.md"]);
        let score = classify_complexity("rename the config struct", &tree);
        assert_eq!(score.mode, ComplexityMode::Simple);
    }
}
