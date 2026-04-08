//! Layer 5: Intelligent Merge Reviewer — LLM-powered diff analysis and conflict resolution
//!
//! Replaces the brute-force `git apply` merge system with a structured reviewer that
//! parses unified diffs, detects semantic conflicts, and uses Gemini to make intelligent
//! merge decisions.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

// ── Diff Analysis Types ──────────────────────────────────────────────────────

/// A single file change extracted from a unified diff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub file_path: String,
    pub change_type: ChangeType,
    pub hunks: Vec<DiffHunk>,
    pub raw_diff: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChangeType {
    Added,
    Modified,
    Deleted,
    Renamed { from: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub content: String,
}

/// An agent's complete contribution to a merge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContribution {
    pub agent_id: String,
    pub task_id: String,
    pub task_description: String,
    pub file_changes: Vec<FileChange>,
    pub raw_diff: String,
}

// ── Conflict Detection Types ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeConflict {
    pub conflict_type: ConflictType,
    pub file_path: String,
    pub agents_involved: Vec<String>,
    pub description: String,
    pub severity: ConflictSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictType {
    FileOverlap,
    DeleteModify,
    SemanticConflict,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConflictSeverity {
    Low,
    Medium,
    High,
    Critical,
}

// ── Review Decision Types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeDecision {
    pub file_path: String,
    pub action: MergeAction,
    pub final_content: Option<String>,
    pub reasoning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MergeAction {
    Accept { from_agent: String },
    Merge,
    Reject { reason: String },
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewResult {
    pub decisions: Vec<MergeDecision>,
    pub final_diff: String,
    pub conflicts_resolved: usize,
    pub files_accepted: usize,
    pub files_rejected: usize,
    pub summary: String,
}

// ── Diff Parser ──────────────────────────────────────────────────────────────

/// Parse a standard unified git diff into individual FileChange objects.
/// Handles `diff --git` headers, new/deleted/renamed files, and @@ hunk headers.
pub fn parse_unified_diff(raw_diff: &str) -> Vec<FileChange> {
    let mut changes = Vec::new();
    let lines: Vec<&str> = raw_diff.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        if !lines[i].starts_with("diff --git ") {
            i += 1;
            continue;
        }

        let file_start = i;

        // Find end of this file's section (next "diff --git" or EOF)
        let file_end = {
            let mut end = lines.len();
            for j in (file_start + 1)..lines.len() {
                if lines[j].starts_with("diff --git ") {
                    end = j;
                    break;
                }
            }
            end
        };

        let file_lines = &lines[file_start..file_end];
        let raw_diff_section = file_lines.join("\n");

        if let Some(fc) = parse_single_file_diff(file_lines, raw_diff_section) {
            changes.push(fc);
        }

        i = file_end;
    }

    changes
}

fn parse_single_file_diff(file_lines: &[&str], raw_diff: String) -> Option<FileChange> {
    if file_lines.is_empty() {
        return None;
    }

    // Extract file path from "diff --git a/PATH b/PATH"
    let header = file_lines[0];
    let file_path = extract_path_from_diff_header(header)?;

    let mut change_type = ChangeType::Modified;
    let mut rename_from: Option<String> = None;
    let mut hunks = Vec::new();
    let mut j = 1;

    // Parse metadata lines and hunks
    while j < file_lines.len() {
        let line = file_lines[j];

        if line.starts_with("new file mode") {
            change_type = ChangeType::Added;
            j += 1;
        } else if line.starts_with("deleted file mode") {
            change_type = ChangeType::Deleted;
            j += 1;
        } else if line.starts_with("rename from ") {
            rename_from = Some(line["rename from ".len()..].to_string());
            j += 1;
        } else if line.starts_with("rename to ") {
            if let Some(from) = rename_from.take() {
                change_type = ChangeType::Renamed { from };
            }
            j += 1;
        } else if line.starts_with("--- ") {
            // Check for /dev/null (new file marker)
            if line.trim_end() == "--- /dev/null" {
                change_type = ChangeType::Added;
            }
            j += 1;
        } else if line.starts_with("+++ ") {
            // Check for /dev/null (deleted file marker)
            if line.trim_end() == "+++ /dev/null" {
                change_type = ChangeType::Deleted;
            }
            j += 1;
        } else if line.starts_with("@@ ") {
            // Parse hunk header and collect hunk content
            if let Some((old_start, old_count, new_start, new_count)) = parse_hunk_header(line) {
                let hunk_line_start = j;
                j += 1;
                // Collect hunk content until next hunk header
                while j < file_lines.len() && !file_lines[j].starts_with("@@ ") {
                    j += 1;
                }
                let hunk_content = file_lines[hunk_line_start..j].join("\n");
                hunks.push(DiffHunk {
                    old_start,
                    old_count,
                    new_start,
                    new_count,
                    content: hunk_content,
                });
            } else {
                j += 1;
            }
        } else {
            j += 1;
        }
    }

    Some(FileChange {
        file_path,
        change_type,
        hunks,
        raw_diff,
    })
}

/// Extract the destination file path from a "diff --git a/X b/Y" header.
fn extract_path_from_diff_header(header: &str) -> Option<String> {
    // Header format: "diff --git a/PATH b/PATH"
    let rest = header.strip_prefix("diff --git a/")?;

    // Find " b/" separator — use rfind to handle paths containing " b/"
    if let Some(b_pos) = rest.rfind(" b/") {
        return Some(rest[b_pos + 3..].to_string());
    }

    // Fallback: take everything after "a/"
    Some(rest.to_string())
}

/// Parse "@@ -old_start,old_count +new_start,new_count @@" hunk header.
fn parse_hunk_header(line: &str) -> Option<(u32, u32, u32, u32)> {
    let rest = line.strip_prefix("@@ ")?;
    let end_pos = rest.find(" @@")?;
    let ranges = &rest[..end_pos];

    let mut parts = ranges.splitn(2, ' ');
    let old_part = parts.next()?.strip_prefix('-')?;
    let new_part = parts.next()?.strip_prefix('+')?;

    let (old_start, old_count) = parse_range(old_part)?;
    let (new_start, new_count) = parse_range(new_part)?;

    Some((old_start, old_count, new_start, new_count))
}

/// Parse "start,count" or "start" (count defaults to 1) range string.
fn parse_range(s: &str) -> Option<(u32, u32)> {
    if let Some(comma) = s.find(',') {
        let start: u32 = s[..comma].parse().ok()?;
        let count: u32 = s[comma + 1..].parse().ok()?;
        Some((start, count))
    } else {
        let start: u32 = s.parse().ok()?;
        Some((start, 1))
    }
}

// ── Conflict Detector ────────────────────────────────────────────────────────

/// Detects conflicts between multiple agent contributions by analyzing
/// which files were touched and whether their hunks overlap.
pub struct ConflictDetector;

impl ConflictDetector {
    /// Detect conflicts from a set of agent contributions.
    /// Returns a list of conflicts sorted by severity (Critical first).
    pub fn detect(contributions: &[AgentContribution]) -> Vec<MergeConflict> {
        // Build a map: file_path -> Vec<(contribution_index, FileChange)>
        let mut file_map: HashMap<String, Vec<(usize, &FileChange)>> = HashMap::new();

        for (idx, contribution) in contributions.iter().enumerate() {
            for file_change in &contribution.file_changes {
                file_map
                    .entry(file_change.file_path.clone())
                    .or_insert_with(Vec::new)
                    .push((idx, file_change));
            }
        }

        let mut conflicts = Vec::new();

        for (file_path, agent_entries) in &file_map {
            if agent_entries.len() <= 1 {
                // Only one agent touched this file — no conflict
                continue;
            }

            let agents_involved: Vec<String> = agent_entries
                .iter()
                .map(|(idx, _)| contributions[*idx].agent_id.clone())
                .collect();

            // Check for delete-vs-modify conflict
            let has_delete = agent_entries
                .iter()
                .any(|(_, fc)| matches!(fc.change_type, ChangeType::Deleted));
            let has_non_delete = agent_entries
                .iter()
                .any(|(_, fc)| !matches!(fc.change_type, ChangeType::Deleted));

            if has_delete && has_non_delete {
                conflicts.push(MergeConflict {
                    conflict_type: ConflictType::DeleteModify,
                    file_path: file_path.clone(),
                    agents_involved,
                    description: format!(
                        "Agent wants to delete '{}' while another modifies it",
                        file_path
                    ),
                    severity: ConflictSeverity::Critical,
                });
                continue;
            }

            // Check for overlapping hunks between different agents
            let severity = if hunks_overlap_between_agents(agent_entries) {
                ConflictSeverity::Medium
            } else {
                ConflictSeverity::Low
            };

            conflicts.push(MergeConflict {
                conflict_type: ConflictType::FileOverlap,
                file_path: file_path.clone(),
                agents_involved,
                description: format!(
                    "Multiple agents modified '{}' ({})",
                    file_path,
                    if severity == ConflictSeverity::Medium {
                        "overlapping line ranges — needs LLM review"
                    } else {
                        "non-overlapping hunks — likely auto-mergeable"
                    }
                ),
                severity,
            });
        }

        // Sort Critical → High → Medium → Low
        conflicts.sort_by(|a, b| b.severity.cmp(&a.severity));
        conflicts
    }
}

/// Check whether any hunk from agent A overlaps with any hunk from agent B
/// (for any pair of distinct agents in the list).
fn hunks_overlap_between_agents(agent_entries: &[(usize, &FileChange)]) -> bool {
    for i in 0..agent_entries.len() {
        for j in (i + 1)..agent_entries.len() {
            let (agent_i, fc_i) = &agent_entries[i];
            let (agent_j, fc_j) = &agent_entries[j];

            if agent_i == agent_j {
                continue;
            }

            for hi in &fc_i.hunks {
                let i_end = hi.old_start + hi.old_count;
                for hj in &fc_j.hunks {
                    let j_end = hj.old_start + hj.old_count;
                    // Overlap: neither range ends before the other starts
                    if hi.old_start < j_end && hj.old_start < i_end {
                        return true;
                    }
                }
            }
        }
    }
    false
}

// ── Merge Reviewer Agent ─────────────────────────────────────────────────────

/// LLM-powered merge reviewer. Auto-accepts conflict-free changes and uses
/// Gemini to resolve any detected conflicts.
pub struct MergeReviewerAgent {
    project_id: String,
    max_retries: usize,
}

impl MergeReviewerAgent {
    pub fn new(project_id: String) -> Self {
        Self {
            project_id,
            max_retries: 3,
        }
    }

    /// Review and merge a set of agent contributions.
    ///
    /// Non-conflicting files are auto-accepted without LLM calls.
    /// Conflicting files are resolved with a single Gemini call.
    pub async fn review(
        &self,
        contributions: Vec<AgentContribution>,
        conflicts: Vec<MergeConflict>,
    ) -> Result<ReviewResult> {
        if contributions.is_empty() {
            return Ok(ReviewResult {
                decisions: Vec::new(),
                final_diff: String::new(),
                conflicts_resolved: 0,
                files_accepted: 0,
                files_rejected: 0,
                summary: "No contributions to review.".to_string(),
            });
        }

        // Build per-file lookup: file_path -> Vec<(agent_id, task_description, FileChange)>
        let mut file_map: HashMap<String, Vec<(String, String, FileChange)>> = HashMap::new();
        for contrib in &contributions {
            for fc in &contrib.file_changes {
                file_map
                    .entry(fc.file_path.clone())
                    .or_insert_with(Vec::new)
                    .push((
                        contrib.agent_id.clone(),
                        contrib.task_description.clone(),
                        fc.clone(),
                    ));
            }
        }

        // Build set of conflicting file paths for fast lookup
        let conflict_file_paths: std::collections::HashSet<String> =
            conflicts.iter().map(|c| c.file_path.clone()).collect();

        // Phase 1 — Auto-accept non-conflicting files (no LLM call)
        let mut all_decisions: Vec<MergeDecision> = Vec::new();
        let mut diff_parts: Vec<String> = Vec::new();
        let mut files_accepted = 0usize;

        for (file_path, file_contribs) in &file_map {
            if conflict_file_paths.contains(file_path) {
                continue; // Will be handled in Phase 2
            }
            // Single-agent file — auto-accept
            if let Some((agent_id, _, fc)) = file_contribs.first() {
                all_decisions.push(MergeDecision {
                    file_path: file_path.clone(),
                    action: MergeAction::Accept { from_agent: agent_id.clone() },
                    final_content: None,
                    reasoning: "Single agent modification — auto-accepted without LLM.".to_string(),
                });
                if !fc.raw_diff.is_empty() {
                    diff_parts.push(fc.raw_diff.clone());
                }
                files_accepted += 1;
            }
        }

        // If no conflicts, return early without any Gemini call
        if conflicts.is_empty() {
            let final_diff = diff_parts.join("\n");
            let summary = format!(
                "No conflicts detected. Auto-accepted {} files.",
                files_accepted
            );
            println!("✅ MergeReviewer: {}", summary);
            return Ok(ReviewResult {
                decisions: all_decisions,
                final_diff,
                conflicts_resolved: 0,
                files_accepted,
                files_rejected: 0,
                summary,
            });
        }

        // Phase 2 — Resolve conflicts with ONE Gemini call
        println!(
            "🔍 MergeReviewer: Sending {} conflict(s) to Gemini for review...",
            conflicts.len()
        );

        let prompt = self.build_conflict_prompt(&conflicts, &file_map, &contributions);

        let llm_raw = self.call_gemini(&prompt).await?;
        let conflict_decisions = parse_llm_decisions(&llm_raw, &conflicts);

        let mut conflicts_resolved = 0usize;
        let mut files_rejected = 0usize;

        for decision in &conflict_decisions {
            let diff_contribution =
                build_diff_for_decision(decision, &file_map);

            match &decision.action {
                MergeAction::Accept { .. } | MergeAction::Merge => {
                    if let Some(diff) = diff_contribution {
                        if !diff.is_empty() {
                            diff_parts.push(diff);
                        }
                    }
                    files_accepted += 1;
                    conflicts_resolved += 1;
                }
                MergeAction::Delete => {
                    // No diff to add — file is deleted
                    conflicts_resolved += 1;
                }
                MergeAction::Reject { .. } => {
                    files_rejected += 1;
                    conflicts_resolved += 1;
                }
            }
        }

        all_decisions.extend(conflict_decisions);

        let final_diff = diff_parts.join("\n");
        let summary = format!(
            "Reviewed {} contributions: {} files auto-accepted, {} conflicts resolved by LLM, {} rejected.",
            contributions.len(),
            files_accepted - (conflicts_resolved - files_rejected),
            conflicts_resolved,
            files_rejected
        );

        println!("✅ MergeReviewer: {}", summary);

        Ok(ReviewResult {
            decisions: all_decisions,
            final_diff,
            conflicts_resolved,
            files_accepted,
            files_rejected,
            summary,
        })
    }

    /// Build a structured prompt for Gemini to resolve all conflicts in one call.
    fn build_conflict_prompt(
        &self,
        conflicts: &[MergeConflict],
        file_map: &HashMap<String, Vec<(String, String, FileChange)>>,
        contributions: &[AgentContribution],
    ) -> String {
        let all_agent_ids: Vec<String> =
            contributions.iter().map(|c| c.agent_id.clone()).collect();

        let conflict_sections: Vec<String> = conflicts
            .iter()
            .map(|conflict| {
                let entries = file_map
                    .get(&conflict.file_path)
                    .cloned()
                    .unwrap_or_default();

                let agent_sections: Vec<String> = entries
                    .iter()
                    .map(|(agent_id, task_desc, fc)| {
                        let change_label = match &fc.change_type {
                            ChangeType::Added => "Added (new file)".to_string(),
                            ChangeType::Modified => "Modified".to_string(),
                            ChangeType::Deleted => "Deleted".to_string(),
                            ChangeType::Renamed { from } => format!("Renamed from '{}'", from),
                        };
                        format!(
                            "  Agent: {}\n  Task: {}\n  Change: {}\n  Diff:\n{}\n",
                            agent_id,
                            task_desc,
                            change_label,
                            fc.raw_diff
                                .lines()
                                .map(|l| format!("    {}", l))
                                .collect::<Vec<_>>()
                                .join("\n")
                        )
                    })
                    .collect();

                format!(
                    "FILE: {}\nSeverity: {:?}\nConflict: {}\n\nAgent contributions:\n{}",
                    conflict.file_path,
                    conflict.severity,
                    conflict.description,
                    agent_sections.join("\n")
                )
            })
            .collect();

        format!(
            r#"You are a senior engineer resolving merge conflicts from parallel AI agents working on the same codebase.

Agent IDs: {}

For each conflict below, decide how to resolve it. Output a JSON array of decisions.

CONFLICTS:
{}

Respond with a JSON array where each element is:
{{
  "file_path": "exact file path",
  "action": "accept" | "merge" | "reject" | "delete",
  "agent_id": "agent-id-to-accept-from",
  "merged_content": "complete merged file content (only when action is merge)",
  "reasoning": "brief explanation"
}}

Resolution rules:
- "accept": Pick one agent's version wholesale. Set "agent_id" to the chosen agent. Use when one agent's change clearly supersedes the other.
- "merge": Combine both agents' changes. Provide the COMPLETE merged file content in "merged_content". Use when changes are complementary.
- "delete": File should be deleted. Use when a delete conflicts with a modify and deletion is correct.
- "reject": Drop all changes to this file. Last resort only.

Guidelines:
- If agents modified different functions in the same file, MERGE them (provide full merged content)
- If agents contradict each other, ACCEPT the one that matches the overall task intent
- Prefer delete conflicts → accept the modify unless deletion is clearly right
- Always preserve working code over broken code

Output ONLY the JSON array, no other text."#,
            all_agent_ids.join(", "),
            conflict_sections.join("\n\n---\n\n")
        )
    }

    /// Call Gemini with the conflict resolution prompt and return the raw response text.
    async fn call_gemini(&self, prompt: &str) -> Result<String> {
        let access_token = super::gcloud_access_token()?;
        let url = super::vertex_generate_url(&self.project_id);

        let request_body = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": prompt}]
                }
            ],
            "systemInstruction": {
                "parts": [{
                    "text": "You are a senior software engineer resolving merge conflicts. Respond ONLY with a valid JSON array."
                }]
            },
            "generationConfig": super::vertex_generation_config_json(0.2)
        });

        let client = reqwest::Client::new();
        let mut last_err: Option<anyhow::Error> = None;

        for attempt in 0..self.max_retries {
            let response = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", access_token))
                .header("Content-Type", "application/json")
                .json(&request_body)
                .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS))
                .send()
                .await
                .context("Failed to send conflict review request to Gemini")?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                let err = anyhow!("Gemini API returned {}: {}", status, body);
                if attempt < self.max_retries - 1 {
                    println!("⚠️  MergeReviewer attempt {}/{} failed: {}", attempt + 1, self.max_retries, status);
                    tokio::time::sleep(tokio::time::Duration::from_secs(2u64.pow(attempt as u32))).await;
                    last_err = Some(err);
                    continue;
                }
                return Err(err);
            }

            let response_json: serde_json::Value = response
                .json()
                .await
                .context("Failed to parse Gemini response JSON")?;

            let text = response_json
                .get("candidates")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("content"))
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.get(0))
                .and_then(|p| p.get("text"))
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow!("Unexpected Gemini response structure — missing candidates[0].content.parts[0].text"))?;

            return Ok(text.to_string());
        }

        Err(last_err.unwrap_or_else(|| anyhow!("Gemini call failed after {} attempts", self.max_retries)))
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Parse the JSON array returned by Gemini into MergeDecisions.
/// Falls back gracefully: unresolved conflicts get a fallback Accept decision.
fn parse_llm_decisions(
    raw_text: &str,
    conflicts: &[MergeConflict],
) -> Vec<MergeDecision> {
    // Strip markdown code fences if present
    let cleaned = raw_text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let items: Vec<serde_json::Value> = match serde_json::from_str(cleaned) {
        Ok(v) => v,
        Err(e) => {
            println!("⚠️  MergeReviewer: failed to parse LLM JSON response: {}", e);
            return fallback_decisions(conflicts);
        }
    };

    let mut decisions: Vec<MergeDecision> = items
        .iter()
        .filter_map(|item| {
            let file_path = item.get("file_path")?.as_str()?.to_string();
            let action_str = item.get("action")?.as_str()?;
            let reasoning = item
                .get("reasoning")
                .and_then(|v| v.as_str())
                .unwrap_or("LLM decision — no reasoning provided")
                .to_string();
            let final_content = item
                .get("merged_content")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let agent_id = item
                .get("agent_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let action = match action_str {
                "accept" => MergeAction::Accept { from_agent: agent_id },
                "merge" => MergeAction::Merge,
                "delete" => MergeAction::Delete,
                "reject" | _ => MergeAction::Reject { reason: reasoning.clone() },
            };

            Some(MergeDecision {
                file_path,
                action,
                final_content,
                reasoning,
            })
        })
        .collect();

    // Ensure every conflict file got a decision — fill gaps with fallback
    for conflict in conflicts {
        if !decisions.iter().any(|d| d.file_path == conflict.file_path) {
            let fallback_agent = conflict.agents_involved.first().cloned().unwrap_or_default();
            println!(
                "⚠️  MergeReviewer: LLM did not decide on '{}' — falling back to first agent",
                conflict.file_path
            );
            decisions.push(MergeDecision {
                file_path: conflict.file_path.clone(),
                action: MergeAction::Accept { from_agent: fallback_agent },
                final_content: None,
                reasoning: "Fallback: LLM did not provide a decision for this file. Accepting first agent's version.".to_string(),
            });
        }
    }

    decisions
}

/// Generate fallback Accept decisions (first agent) for all conflicts.
fn fallback_decisions(conflicts: &[MergeConflict]) -> Vec<MergeDecision> {
    conflicts
        .iter()
        .map(|conflict| {
            let fallback_agent = conflict.agents_involved.first().cloned().unwrap_or_default();
            MergeDecision {
                file_path: conflict.file_path.clone(),
                action: MergeAction::Accept { from_agent: fallback_agent },
                final_content: None,
                reasoning: "Fallback: LLM response was unparseable. Accepting first agent's version.".to_string(),
            }
        })
        .collect()
}

/// Build the diff contribution for a given decision.
/// Returns `Some(diff_text)` for Accept/Merge, `None` for Delete/Reject.
fn build_diff_for_decision(
    decision: &MergeDecision,
    file_map: &HashMap<String, Vec<(String, String, FileChange)>>,
) -> Option<String> {
    let entries = file_map.get(&decision.file_path)?;

    match &decision.action {
        MergeAction::Accept { from_agent } => {
            // Find the accepted agent's diff for this file
            entries
                .iter()
                .find(|(aid, _, _)| aid == from_agent)
                .or_else(|| entries.first()) // fallback to first if agent not found
                .map(|(_, _, fc)| fc.raw_diff.clone())
                .filter(|d| !d.is_empty())
        }

        MergeAction::Merge => {
            // If LLM provided merged content, synthesize a diff from it
            if let Some(ref content) = decision.final_content {
                if !content.is_empty() {
                    return Some(synthesize_diff_from_content(&decision.file_path, content));
                }
            }
            // Fallback: use the first agent's diff
            entries
                .first()
                .map(|(_, _, fc)| fc.raw_diff.clone())
                .filter(|d| !d.is_empty())
        }

        MergeAction::Delete | MergeAction::Reject { .. } => None,
    }
}

/// Synthesize a unified diff from full file content (used for LLM-merged files).
/// Treats the merged content as a full file replacement.
fn synthesize_diff_from_content(file_path: &str, content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let line_count = lines.len();

    let plus_lines: String = lines
        .iter()
        .map(|l| format!("+{}", l))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "diff --git a/{path} b/{path}\n\
         index 0000000..0000001 100644\n\
         --- a/{path}\n\
         +++ b/{path}\n\
         @@ -0,0 +1,{count} @@\n\
         {lines}\n",
        path = file_path,
        count = line_count,
        lines = plus_lines,
    )
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_DIFF: &str = r#"diff --git a/src/main.rs b/src/main.rs
index abc1234..def5678 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,5 +1,7 @@
 fn main() {
-    println!("hello");
+    println!("hello, world");
+    println!("goodbye");
 }
diff --git a/src/lib.rs b/src/lib.rs
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/src/lib.rs
@@ -0,0 +1,3 @@
+pub fn greet() {
+    println!("hi");
+}
"#;

    #[test]
    fn test_parse_unified_diff_basic() {
        let changes = parse_unified_diff(SAMPLE_DIFF);
        assert_eq!(changes.len(), 2);

        let main_rs = changes.iter().find(|c| c.file_path == "src/main.rs").unwrap();
        assert!(matches!(main_rs.change_type, ChangeType::Modified));
        assert_eq!(main_rs.hunks.len(), 1);
        assert_eq!(main_rs.hunks[0].old_start, 1);
        assert_eq!(main_rs.hunks[0].old_count, 5);
        assert_eq!(main_rs.hunks[0].new_start, 1);
        assert_eq!(main_rs.hunks[0].new_count, 7);

        let lib_rs = changes.iter().find(|c| c.file_path == "src/lib.rs").unwrap();
        assert!(matches!(lib_rs.change_type, ChangeType::Added));
        assert_eq!(lib_rs.hunks.len(), 1);
    }

    #[test]
    fn test_parse_empty_diff() {
        let changes = parse_unified_diff("");
        assert!(changes.is_empty());
    }

    #[test]
    fn test_conflict_detector_no_conflict() {
        let contrib = AgentContribution {
            agent_id: "agent-1".to_string(),
            task_id: "task-1".to_string(),
            task_description: "Add feature".to_string(),
            file_changes: parse_unified_diff(SAMPLE_DIFF),
            raw_diff: SAMPLE_DIFF.to_string(),
        };
        let conflicts = ConflictDetector::detect(&[contrib]);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_conflict_detector_same_file_different_hunks() {
        let diff_a = "diff --git a/src/foo.rs b/src/foo.rs\n--- a/src/foo.rs\n+++ b/src/foo.rs\n@@ -1,3 +1,4 @@\n+added line\n line1\n line2\n line3\n";
        let diff_b = "diff --git a/src/foo.rs b/src/foo.rs\n--- a/src/foo.rs\n+++ b/src/foo.rs\n@@ -100,3 +100,4 @@\n line100\n line101\n+added line\n line102\n";

        let contrib_a = AgentContribution {
            agent_id: "agent-1".to_string(),
            task_id: "task-1".to_string(),
            task_description: "Add header".to_string(),
            file_changes: parse_unified_diff(diff_a),
            raw_diff: diff_a.to_string(),
        };
        let contrib_b = AgentContribution {
            agent_id: "agent-2".to_string(),
            task_id: "task-2".to_string(),
            task_description: "Add footer".to_string(),
            file_changes: parse_unified_diff(diff_b),
            raw_diff: diff_b.to_string(),
        };

        let conflicts = ConflictDetector::detect(&[contrib_a, contrib_b]);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].severity, ConflictSeverity::Low);
        assert_eq!(conflicts[0].file_path, "src/foo.rs");
    }

    #[test]
    fn test_conflict_detector_delete_modify() {
        let diff_modify = "diff --git a/src/foo.rs b/src/foo.rs\n--- a/src/foo.rs\n+++ b/src/foo.rs\n@@ -1,3 +1,4 @@\n+added\n line1\n line2\n line3\n";
        let diff_delete = "diff --git a/src/foo.rs b/src/foo.rs\ndeleted file mode 100644\n--- a/src/foo.rs\n+++ /dev/null\n@@ -1,3 +0,0 @@\n-line1\n-line2\n-line3\n";

        let contrib_modify = AgentContribution {
            agent_id: "agent-1".to_string(),
            task_id: "task-1".to_string(),
            task_description: "Add feature".to_string(),
            file_changes: parse_unified_diff(diff_modify),
            raw_diff: diff_modify.to_string(),
        };
        let contrib_delete = AgentContribution {
            agent_id: "agent-2".to_string(),
            task_id: "task-2".to_string(),
            task_description: "Remove module".to_string(),
            file_changes: parse_unified_diff(diff_delete),
            raw_diff: diff_delete.to_string(),
        };

        let conflicts = ConflictDetector::detect(&[contrib_modify, contrib_delete]);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].severity, ConflictSeverity::Critical);
        assert!(matches!(conflicts[0].conflict_type, ConflictType::DeleteModify));
    }

    #[test]
    fn test_severity_ordering() {
        assert!(ConflictSeverity::Critical > ConflictSeverity::High);
        assert!(ConflictSeverity::High > ConflictSeverity::Medium);
        assert!(ConflictSeverity::Medium > ConflictSeverity::Low);
    }

    #[test]
    fn test_synthesize_diff_from_content() {
        let content = "fn main() {\n    println!(\"hello\");\n}";
        let diff = synthesize_diff_from_content("src/main.rs", content);
        assert!(diff.contains("diff --git a/src/main.rs b/src/main.rs"));
        assert!(diff.contains("+fn main()"));
        assert!(diff.contains("@@ -0,0 +1,3 @@"));
    }
}
