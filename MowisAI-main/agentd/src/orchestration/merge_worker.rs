//! Layer 5: Parallel Merge — Tree-pattern merge workers with LLM conflict repair

use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::task::JoinHandle;

/// Merge worker result
#[derive(Debug, Clone)]
pub struct MergeResult {
    pub success: bool,
    pub merged_diff: String,
    pub conflicts_resolved: usize,
    pub unresolved_conflicts: Vec<String>,
}

/// Parallel merge coordinator
pub struct ParallelMergeCoordinator {
    project_id: String,
    work_dir: PathBuf,
    base_repo_path: PathBuf,
    max_conflict_retries: usize,
}

impl ParallelMergeCoordinator {
    pub fn new(project_id: String, work_dir: PathBuf, base_repo_path: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&work_dir).context("Failed to create merge work directory")?;

        Ok(Self {
            project_id,
            work_dir,
            base_repo_path,
            max_conflict_retries: 3,
        })
    }

    /// Merge multiple agent diffs using tree-pattern parallel merge
    pub async fn merge_diffs(&self, diffs: Vec<String>) -> Result<MergeResult> {
        if diffs.is_empty() {
            return Ok(MergeResult {
                success: true,
                merged_diff: String::new(),
                conflicts_resolved: 0,
                unresolved_conflicts: Vec::new(),
            });
        }

        if diffs.len() == 1 {
            return Ok(MergeResult {
                success: true,
                merged_diff: diffs[0].clone(),
                conflicts_resolved: 0,
                unresolved_conflicts: Vec::new(),
            });
        }

        // Tree-pattern merge
        let mut current_level = diffs;
        let mut total_conflicts_resolved = 0;
        let mut all_unresolved = Vec::new();

        while current_level.len() > 1 {
            let mut next_level = Vec::new();
            let mut merge_tasks: Vec<JoinHandle<Result<MergeResult>>> = Vec::new();

            // Pair up diffs and spawn merge workers
            for chunk in current_level.chunks(2) {
                if chunk.len() == 2 {
                    let diff1 = chunk[0].clone();
                    let diff2 = chunk[1].clone();
                    let project_id = self.project_id.clone();
                    let work_dir = self.work_dir.clone();
                    let base_repo_path = self.base_repo_path.clone();
                    let max_retries = self.max_conflict_retries;

                    let task = tokio::spawn(async move {
                        merge_two_diffs(&diff1, &diff2, &project_id, &work_dir, &base_repo_path, max_retries).await
                    });

                    merge_tasks.push(task);
                } else {
                    // Odd one out - carry forward to next level
                    next_level.push(chunk[0].clone());
                }
            }

            // Wait for all merge tasks to complete
            for task in merge_tasks {
                let result = task.await.context("Merge task panicked")??;
                total_conflicts_resolved += result.conflicts_resolved;
                all_unresolved.extend(result.unresolved_conflicts);

                if result.success {
                    next_level.push(result.merged_diff);
                } else {
                    return Ok(MergeResult {
                        success: false,
                        merged_diff: String::new(),
                        conflicts_resolved: total_conflicts_resolved,
                        unresolved_conflicts: all_unresolved,
                    });
                }
            }

            current_level = next_level;
        }

        Ok(MergeResult {
            success: true,
            merged_diff: current_level.into_iter().next().unwrap_or_default(),
            conflicts_resolved: total_conflicts_resolved,
            unresolved_conflicts: all_unresolved,
        })
    }
}

/// Merge two diffs with LLM conflict repair
async fn merge_two_diffs(
    diff1: &str,
    diff2: &str,
    project_id: &str,
    work_dir: &Path,
    base_repo_path: &Path,
    max_retries: usize,
) -> Result<MergeResult> {
    // Create temporary directory for this merge
    let merge_id = uuid::Uuid::new_v4().to_string();
    let merge_dir = work_dir.join(format!("merge-{}", merge_id));
    let repo_dir = merge_dir.join("repo");
    std::fs::create_dir_all(&merge_dir)?;

    // Write diffs to files
    let diff1_path = merge_dir.join("diff1.patch");
    let diff2_path = merge_dir.join("diff2.patch");
    std::fs::write(&diff1_path, diff1)?;
    std::fs::write(&diff2_path, diff2)?;

    // Try to apply both diffs in sequence
    let mut conflicts_resolved = 0;
    let mut unresolved_conflicts = Vec::new();

    #[cfg(target_os = "linux")]
    {
        prepare_merge_repo(&repo_dir, base_repo_path)?;

        let base_commit = current_head(&repo_dir)?;

        // Apply diff1
        let apply1 = apply_diff(&repo_dir, &diff1_path)?;
        if !apply1.success {
            // Conflict in diff1 - try repair
            match repair_conflict(diff1, diff2, &apply1.conflict_text, project_id, max_retries).await {
                Ok(repaired) => {
                    let repaired_path = merge_dir.join("diff1_repaired.patch");
                    std::fs::write(&repaired_path, &repaired)?;
                    let apply_repaired = apply_diff(&repo_dir, &repaired_path)?;

                    if apply_repaired.success {
                        conflicts_resolved += 1;
                    } else {
                        unresolved_conflicts.push(apply_repaired.conflict_text);
                    }
                }
                Err(e) => {
                    unresolved_conflicts.push(format!("Repair failed: {}", e));
                }
            }
        }

        // Apply diff2
        let apply2 = apply_diff(&repo_dir, &diff2_path)?;
        if !apply2.success {
            // Conflict in diff2 - try repair
            match repair_conflict(diff1, diff2, &apply2.conflict_text, project_id, max_retries).await {
                Ok(repaired) => {
                    let repaired_path = merge_dir.join("diff2_repaired.patch");
                    std::fs::write(&repaired_path, &repaired)?;
                    let apply_repaired = apply_diff(&repo_dir, &repaired_path)?;

                    if apply_repaired.success {
                        conflicts_resolved += 1;
                    } else {
                        unresolved_conflicts.push(apply_repaired.conflict_text);
                    }
                }
                Err(e) => {
                    unresolved_conflicts.push(format!("Repair failed: {}", e));
                }
            }
        }

        // Generate final merged diff
        let final_diff = capture_merged_diff(&repo_dir, &base_commit)?;

        // Cleanup
        std::fs::remove_dir_all(&merge_dir).ok();

        Ok(MergeResult {
            success: unresolved_conflicts.is_empty(),
            merged_diff: final_diff,
            conflicts_resolved,
            unresolved_conflicts,
        })
    }

    #[cfg(not(target_os = "linux"))]
    {
        // Windows fallback - simple concatenation (no conflict detection)
        let merged = format!("{}\n{}", diff1, diff2);
        std::fs::remove_dir_all(&merge_dir).ok();

        Ok(MergeResult {
            success: true,
            merged_diff: merged,
            conflicts_resolved: 0,
            unresolved_conflicts: Vec::new(),
        })
    }
}

#[cfg(target_os = "linux")]
struct ApplyResult {
    success: bool,
    conflict_text: String,
}

#[cfg(target_os = "linux")]
fn prepare_merge_repo(merge_dir: &Path, base_repo_path: &Path) -> Result<()> {
    let clone_result = Command::new("git")
        .arg("clone")
        .arg(base_repo_path)
        .arg(merge_dir)
        .output()
        .context("Failed to clone base repo into merge dir")?;

    if !clone_result.status.success() {
        return Err(anyhow!(
            "Failed to clone base repo: {}",
            String::from_utf8_lossy(&clone_result.stderr)
        ));
    }

    let email_result = Command::new("git")
        .args(["config", "user.email", "merge@mowis.ai"])
        .current_dir(merge_dir)
        .output()
        .context("Failed to configure merge repo email")?;
    if !email_result.status.success() {
        return Err(anyhow!(
            "Failed to configure merge repo email: {}",
            String::from_utf8_lossy(&email_result.stderr)
        ));
    }

    let name_result = Command::new("git")
        .args(["config", "user.name", "MowisAI Merge"])
        .current_dir(merge_dir)
        .output()
        .context("Failed to configure merge repo user")?;
    if !name_result.status.success() {
        return Err(anyhow!(
            "Failed to configure merge repo user: {}",
            String::from_utf8_lossy(&name_result.stderr)
        ));
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn current_head(work_dir: &Path) -> Result<String> {
    let head_result = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(work_dir)
        .output()
        .context("Failed to resolve merge base HEAD")?;

    if !head_result.status.success() {
        return Err(anyhow!(
            "Failed to resolve merge base HEAD: {}",
            String::from_utf8_lossy(&head_result.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&head_result.stdout).trim().to_string())
}

#[cfg(target_os = "linux")]
fn apply_diff(work_dir: &Path, diff_path: &Path) -> Result<ApplyResult> {
    let apply_result = Command::new("git")
        .arg("apply")
        .arg(diff_path)
        .current_dir(work_dir)
        .output()
        .context("Failed to run git apply")?;

    if apply_result.status.success() {
        Ok(ApplyResult {
            success: true,
            conflict_text: String::new(),
        })
    } else {
        Ok(ApplyResult {
            success: false,
            conflict_text: String::from_utf8_lossy(&apply_result.stderr).to_string(),
        })
    }
}

#[cfg(target_os = "linux")]
fn capture_merged_diff(work_dir: &Path, base_commit: &str) -> Result<String> {
    // Stage all changes first
    Command::new("git")
        .arg("add")
        .arg("-A")
        .current_dir(work_dir)
        .output()
        .context("Failed to stage changes")?;

    // Get diff of staged changes
    let diff_result = Command::new("git")
        .arg("diff")
        .arg("--cached")
        .arg(base_commit)
        .current_dir(work_dir)
        .output()
        .context("Failed to capture merged diff")?;

    if diff_result.status.success() {
        Ok(String::from_utf8_lossy(&diff_result.stdout).to_string())
    } else {
        Err(anyhow!(
            "Failed to capture diff: {}",
            String::from_utf8_lossy(&diff_result.stderr)
        ))
    }
}

/// Repair merge conflict using LLM
async fn repair_conflict(
    diff1: &str,
    diff2: &str,
    conflict_text: &str,
    project_id: &str,
    max_retries: usize,
) -> Result<String> {
    let access_token = super::gcloud_access_token()?;
    let url = super::vertex_generate_url(project_id);

    let system_prompt = r#"You are a merge conflict resolver. Given two diffs and a conflict message, produce a repaired patch that resolves the conflict.

Output ONLY the repaired git patch, with no explanations or markdown formatting."#;

    let user_message = format!(
        "Diff 1:\n{}\n\nDiff 2:\n{}\n\nConflict:\n{}\n\nProduce a repaired patch that resolves this conflict:",
        diff1, diff2, conflict_text
    );

    for attempt in 0..max_retries {
        let request_body = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": user_message}]
                }
            ],
            "systemInstruction": {
                "parts": [{"text": system_prompt}]
            },
            "generationConfig": super::vertex_generation_config(0.3)
        });

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS))
            .send()
            .await
            .context("Failed to send conflict repair request")?;

        if !response.status().is_success() {
            if attempt < max_retries - 1 {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                continue;
            }
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Gemini API error: {}", error_text));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse repair response")?;

        let text = response_json
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.get(0))
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow!("Invalid repair response structure"))?;

        return Ok(text.to_string());
    }

    Err(anyhow!("Failed to repair conflict after {} attempts", max_retries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_merge_empty_diffs() {
        let temp_dir = std::env::temp_dir().join("test_merge");
        let coordinator = ParallelMergeCoordinator::new(
            "test-project".to_string(),
            temp_dir.clone(),
            temp_dir.clone(),
        )
        .unwrap();

        let result = coordinator.merge_diffs(vec![]).await.unwrap();
        assert!(result.success);
        assert_eq!(result.merged_diff, "");

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_merge_single_diff() {
        let temp_dir = std::env::temp_dir().join("test_merge_single");
        let coordinator = ParallelMergeCoordinator::new(
            "test-project".to_string(),
            temp_dir.clone(),
            temp_dir.clone(),
        )
        .unwrap();

        let diffs = vec!["diff content".to_string()];
        let result = coordinator.merge_diffs(diffs).await.unwrap();
        assert!(result.success);
        assert_eq!(result.merged_diff, "diff content");

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
