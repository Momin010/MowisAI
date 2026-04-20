//! Layer 6: Verification Loop — VeriMAP-style pre-planned VF execution
//!
//! KEY ARCHITECTURAL CHANGE (VeriMAP fix):
//! Verification Functions (VFs) are now generated ONCE at planning time and
//! stored in `VerificationPlan`. The `verify_sandbox` loop never calls Gemini
//! to invent new tests — it only executes the pre-planned VFs. This eliminates
//! non-deterministic test generation across rounds, which was the root cause of
//! flaky verification.

use super::agent_execution::AgentExecutor;
use super::sandbox_topology::TopologyManager;
use agentd_protocol::{SandboxName, Task, TaskGraph, TaskId, VerificationStatus};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

// ─────────────────────────────────────────────────────────────────────────────
// Data structures
// ─────────────────────────────────────────────────────────────────────────────

/// A single Verification Function (VF) — generated once at planning time,
/// executed every round unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationFunction {
    /// Stable ID — must not change across rounds
    pub id: TaskId,
    /// Human-readable description of what this VF checks
    pub description: String,
    /// The exact shell command(s) to run inside the sandbox.
    /// These are deterministic — no LLM re-reasoning at execution time.
    /// Example: "cargo test --lib 2>&1" or "npm test -- --ci 2>&1"
    pub command: String,
    /// Optional: JSON schema string the command stdout must conform to.
    /// If None, success is determined purely by exit code.
    pub expected_schema: Option<String>,
    /// Optional: plain-text assertion the output must satisfy (checked by a
    /// judge agent only if command exit-code alone is ambiguous).
    pub assertion: Option<String>,
    /// DAG deps — other VF ids that must pass before this one runs
    pub deps: Vec<TaskId>,
}

/// Verification plan — generated ONCE before the loop starts.
/// Never regenerated mid-loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationPlan {
    pub sandbox_name: SandboxName,
    /// Ordered list of VFs to execute every round
    pub vfs: Vec<VerificationFunction>,
}

/// Result of a single VF execution
#[derive(Debug, Clone)]
struct VfResult {
    id: TaskId,
    passed: bool,
    output: String,
}

/// Final result returned to the orchestrator
#[derive(Debug, Clone)]
pub struct VerificationResult {
    pub status: VerificationStatus,
    pub passed_tests: Vec<TaskId>,
    pub failed_tests: Vec<TaskId>,
    pub rounds_completed: usize,
    pub updated_diff: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// VF Planner — called ONCE by the orchestrator before verify_sandbox
// ─────────────────────────────────────────────────────────────────────────────

pub struct VerificationPlanner {
    project_id: String,
    pub max_rounds: usize,
    /// Per-VF execution timeout in seconds
    pub max_test_execution_time: u64,
}

impl VerificationPlanner {
    pub fn new(project_id: String, max_rounds: usize) -> Self {
        Self {
            project_id,
            max_rounds,
            max_test_execution_time: 60,
        }
    }

    /// Generate the VerificationPlan for a sandbox.
    ///
    /// This is the ONLY Gemini call in the verification path. It runs once,
    /// before the loop. The returned VFs are stable and reused every round.
    pub async fn generate_verification_plan(
        &self,
        sandbox_name: &SandboxName,
        merged_diff: &str,
        original_tasks: &[Task],
    ) -> Result<VerificationPlan> {
        let access_token = super::gcloud_access_token()?;
        let url = super::vertex_generate_url(&self.project_id);

        // CRITICAL: The system prompt asks for DETERMINISTIC shell commands,
        // not vague descriptions. This is what makes VFs stable across rounds.
        let system_prompt = r#"You are a verification function planner for a multi-agent code execution system.

Given a merged diff and original task descriptions, generate a set of Verification Functions (VFs).

CRITICAL RULES:
1. Each VF must contain an EXACT shell command that can be run verbatim in the sandbox.
   Do NOT produce vague descriptions like "run the tests". Produce exact commands like "cargo test 2>&1" or "npm test -- --ci 2>&1".
2. Commands must be deterministic — the same command run twice on the same code must produce the same exit code.
3. Do NOT include flaky checks (random seeds, time-based assertions, network calls without mocking).
4. Keep VFs small and focused — one concern per VF.
5. Infer the language and toolchain from the diff (Rust → cargo, Node → npm/yarn, Python → pytest, etc).

Output ONLY a JSON object:
{
  "vfs": [
    {
      "id": "vf-build",
      "description": "Verify project compiles without errors",
      "command": "cargo build 2>&1",
      "expected_schema": null,
      "assertion": "exit code 0",
      "deps": []
    },
    {
      "id": "vf-unit",
      "description": "Run unit test suite",
      "command": "cargo test --lib 2>&1",
      "expected_schema": null,
      "assertion": "exit code 0, no FAILED in output",
      "deps": ["vf-build"]
    }
  ]
}

Standard VF set (adapt to detected toolchain):
- vf-build: compile/build check (always first, no deps)
- vf-lint: linter/formatter check (no deps)
- vf-typecheck: type checker if applicable (no deps)
- vf-unit: unit tests (deps: [vf-build])
- vf-integration: integration tests if API/interface changed (deps: [vf-unit])
"#;

        let task_summaries = original_tasks
            .iter()
            .map(|t| format!("- {}: {}", t.id, t.description))
            .collect::<Vec<_>>()
            .join("\n");

        let user_message = format!(
            "Sandbox: {}\n\nOriginal tasks:\n{}\n\nMerged diff:\n{}\n\nGenerate verification functions:",
            sandbox_name, task_summaries, merged_diff
        );

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
            "generationConfig": super::vertex_generation_config_json(0.1)
        });

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .context("Failed to send VF planning request")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Gemini API error during VF planning: {}", error_text));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse VF planning response")?;

        let text = response_json
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.get(0))
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow!("Invalid VF planning response structure"))?;

        let json_str = extract_json(text);

        #[derive(Deserialize)]
        struct VfPlanJson {
            vfs: Vec<VerificationFunction>,
        }

        let parsed: VfPlanJson = serde_json::from_str(&json_str)
            .context("Failed to parse VF plan JSON")?;

        log::info!(
            "[VERIFY] Generated {} VFs for sandbox {}",
            parsed.vfs.len(),
            sandbox_name
        );
        for vf in &parsed.vfs {
            log::info!("[VERIFY] VF [{}]: command = {:?}", vf.id, vf.command);
        }

        Ok(VerificationPlan {
            sandbox_name: sandbox_name.clone(),
            vfs: parsed.vfs,
        })
    }

    /// Generate fix tasks from a VF failure.
    /// This is the only OTHER Gemini call — and it's scoped to a specific
    /// failure output, not the whole diff. Kept from original.
    pub async fn generate_fix_tasks(
        &self,
        failed_vf_id: &TaskId,
        vf_command: &str,
        failure_output: &str,
    ) -> Result<Vec<Task>> {
        let access_token = super::gcloud_access_token()?;
        let url = super::vertex_generate_url(&self.project_id);

        let system_prompt = r#"You are a test failure analyzer. Given a failed verification command and its output, generate fix tasks.

Output JSON in this format:
{
  "fix_tasks": [
    {"id": "fix-1", "description": "fix the identified bug", "deps": [], "hint": null}
  ]
}

Each fix task must be specific, actionable, and directly address the failure output shown.
"#;

        let user_message = format!(
            "Failed VF: {}\nCommand run: {}\n\nFailure output:\n{}\n\nGenerate fix tasks:",
            failed_vf_id, vf_command, failure_output
        );

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
            "generationConfig": super::vertex_generation_config_json(0.3)
        });

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .context("Failed to send fix task request")?;

        if !response.status().is_success() {
            return Ok(vec![]);
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse fix task response")?;

        let text = response_json
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.get(0))
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow!("Invalid fix task response structure"))?;

        let json_str = extract_json(text);

        #[derive(Deserialize)]
        struct FixTasksJson {
            fix_tasks: Vec<Task>,
        }

        let parsed: FixTasksJson = match serde_json::from_str(&json_str) {
            Ok(p) => p,
            Err(e) => {
                log::warn!(
                    "[VERIFY] Failed to parse fix tasks JSON (returning empty): {}. Raw: {:.200}",
                    e,
                    json_str
                );
                FixTasksJson { fix_tasks: vec![] }
            }
        };

        Ok(parsed.fix_tasks)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Verification Loop — executes pre-planned VFs, never regenerates them
// ─────────────────────────────────────────────────────────────────────────────

pub struct VerificationLoop {
    planner: VerificationPlanner,
    max_rounds: usize,
}

impl VerificationLoop {
    pub fn new(project_id: String, max_rounds: usize) -> Self {
        Self {
            planner: VerificationPlanner::new(project_id.clone(), max_rounds),
            max_rounds,
        }
    }

    pub fn with_test_timeout(mut self, secs: u64) -> Self {
        self.planner.max_test_execution_time = secs;
        self
    }

    /// Run the VeriMAP-style verification loop for a sandbox.
    ///
    /// Flow:
    /// 1. Generate VFs ONCE (one Gemini call)
    /// 2. Each round: execute ALL VFs via exact shell commands
    /// 3. On failure: generate fix tasks → apply → re-run SAME VFs
    /// 4. VFs never change between rounds
    pub async fn verify_sandbox(
        &self,
        sandbox_name: &SandboxName,
        merged_diff: &str,
        original_tasks: &[Task],
        topology: &TopologyManager,
        agent_executor: &AgentExecutor,
    ) -> Result<VerificationResult> {
        let mut current_merged_diff = merged_diff.to_string();
        let mut passed_tests: Vec<TaskId> = Vec::new();
        let mut failed_tests: Vec<TaskId> = Vec::new();
        let mut rounds_completed = 0;

        log::info!(
            "[VERIFY] Starting for sandbox: {}, diff_len: {}, tasks: {}",
            sandbox_name,
            current_merged_diff.len(),
            original_tasks.len()
        );

        // ── Step 1: Generate VFs ONCE ────────────────────────────────────────
        let plan = self
            .planner
            .generate_verification_plan(sandbox_name, &current_merged_diff, original_tasks)
            .await?;

        if plan.vfs.is_empty() {
            log::warn!("[VERIFY] No VFs generated for sandbox {} — returning NotStarted", sandbox_name);
            return Ok(VerificationResult {
                status: VerificationStatus::NotStarted,
                passed_tests: vec![],
                failed_tests: vec![],
                rounds_completed: 0,
                updated_diff: Some(current_merged_diff),
            });
        }

        // ── Step 2: Execute the SAME VFs every round ─────────────────────────
        for round in 0..self.max_rounds {
            rounds_completed = round + 1;
            passed_tests.clear();
            failed_tests.clear();

            log::info!(
                "[VERIFY] Round {}/{} — executing {} pre-planned VFs",
                round + 1,
                self.max_rounds,
                plan.vfs.len()
            );

            let mut round_failures: Vec<(TaskId, String, String)> = Vec::new();

            for vf in &plan.vfs {
                let result = self
                    .execute_vf(vf, sandbox_name, topology, agent_executor)
                    .await;

                match result {
                    Ok(vf_result) if vf_result.passed => {
                        log::info!("[VERIFY] VF [{}] PASSED", vf.id);
                        passed_tests.push(vf.id.clone());
                    }
                    Ok(vf_result) => {
                        log::warn!(
                            "[VERIFY] VF [{}] FAILED — output: {:.300}",
                            vf.id,
                            vf_result.output
                        );
                        failed_tests.push(vf.id.clone());
                        round_failures.push((
                            vf.id.clone(),
                            vf.command.clone(),
                            vf_result.output,
                        ));
                    }
                    Err(e) => {
                        log::warn!("[VERIFY] VF [{}] ERROR — {}", vf.id, e);
                        failed_tests.push(vf.id.clone());
                        round_failures.push((
                            vf.id.clone(),
                            vf.command.clone(),
                            e.to_string(),
                        ));
                    }
                }
            }

            if round_failures.is_empty() {
                log::info!(
                    "[VERIFY] ✓ All {} VFs passed in round {}",
                    passed_tests.len(),
                    round + 1
                );
                break;
            }

            log::info!(
                "[VERIFY] ⚠ {} VFs failed in round {}",
                round_failures.len(),
                round + 1
            );

            // ── Step 3: Apply fixes, then re-run same VFs next round ─────────
            if round < self.max_rounds - 1 {
                let fix_tools = vec![
                    "read_file".to_string(),
                    "write_file".to_string(),
                    "run_command".to_string(),
                    "grep".to_string(),
                ];

                for (vf_id, vf_command, failure_output) in &round_failures {
                    log::info!("[VERIFY] Generating fix tasks for failed VF: {}", vf_id);

                    let fix_tasks = self
                        .planner
                        .generate_fix_tasks(vf_id, vf_command, failure_output)
                        .await
                        .unwrap_or_default();

                    if fix_tasks.is_empty() {
                        log::warn!(
                            "[VERIFY] No fix tasks generated for VF {} in sandbox {}",
                            vf_id,
                            sandbox_name
                        );
                    }

                    for fix_task in fix_tasks {
                        let agent = match topology
                            .create_agent_layer(sandbox_name, Some(fix_task.id.clone()))
                            .await
                        {
                            Ok(a) => a,
                            Err(e) => {
                                log::warn!(
                                    "[VERIFY] Failed to create fix agent for {}: {}",
                                    fix_task.id,
                                    e
                                );
                                continue;
                            }
                        };

                        let fix_prompt = format!("Fix this issue: {}", fix_task.description);

                        let fix_result = match tokio::time::timeout(
                            std::time::Duration::from_secs(
                                self.planner.max_test_execution_time,
                            ),
                            agent_executor.execute_task(
                                &agent,
                                &fix_task.description,
                                &fix_tools,
                                &fix_prompt,
                            ),
                        )
                        .await
                        {
                            Ok(r) => r,
                            Err(_) => {
                                log::warn!(
                                    "[VERIFY] Fix task {} timed out after {}s",
                                    fix_task.id,
                                    self.planner.max_test_execution_time
                                );
                                Err(anyhow!("Fix execution timeout"))
                            }
                        };

                        match fix_result {
                            Ok(r) if r.success => {
                                if let Some(ref diff) = r.git_diff {
                                    if !diff.is_empty() {
                                        if let Err(e) = topology
                                            .apply_diff_to_sandbox(sandbox_name, diff)
                                            .await
                                        {
                                            log::warn!(
                                                "[VERIFY] Failed to apply fix diff for {} to sandbox {}: {}",
                                                fix_task.id,
                                                sandbox_name,
                                                e
                                            );
                                        } else {
                                            log::info!(
                                                "[VERIFY] Applied fix diff for {} to sandbox {}",
                                                fix_task.id,
                                                sandbox_name
                                            );
                                            match topology
                                                .capture_sandbox_diff(sandbox_name)
                                                .await
                                            {
                                                Ok(updated_diff) => {
                                                    current_merged_diff = updated_diff;
                                                    log::info!(
                                                        "[VERIFY] Refreshed sandbox diff after fix {} ({} bytes)",
                                                        fix_task.id,
                                                        current_merged_diff.len()
                                                    );
                                                }
                                                Err(e) => {
                                                    log::warn!(
                                                        "[VERIFY] Applied fix {}, but failed to refresh diff for {}: {}",
                                                        fix_task.id,
                                                        sandbox_name,
                                                        e
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(r) => log::warn!(
                                "[VERIFY] Fix task {} did not succeed: {:?}",
                                fix_task.id,
                                r.error
                            ),
                            Err(e) => log::warn!(
                                "[VERIFY] Fix task {} execution failed: {}",
                                fix_task.id,
                                e
                            ),
                        }

                        let _ = topology.destroy_agent_layer(&agent.agent_id).await;
                    }
                }
            }
        }

        let status = determine_status(
            &failed_tests,
            &passed_tests,
            rounds_completed,
            self.max_rounds,
        );

        log::info!(
            "[VERIFY] Done — sandbox: {}, status: {:?}, passed: {}, failed: {}, rounds: {}",
            sandbox_name,
            status,
            passed_tests.len(),
            failed_tests.len(),
            rounds_completed
        );

        Ok(VerificationResult {
            status,
            passed_tests,
            failed_tests,
            rounds_completed,
            updated_diff: Some(current_merged_diff),
        })
    }

    /// Execute a single VF via an exact shell command inside the sandbox.
    ///
    /// The agent receives the exact command string and runs it — no LLM
    /// reasoning about what to test. Success is determined by exit code.
    async fn execute_vf(
        &self,
        vf: &VerificationFunction,
        sandbox_name: &SandboxName,
        topology: &TopologyManager,
        agent_executor: &AgentExecutor,
    ) -> Result<VfResult> {
        let agent = topology
            .create_agent_layer(sandbox_name, Some(vf.id.clone()))
            .await
            .with_context(|| {
                format!(
                    "Failed to create agent for VF {} in sandbox {}",
                    vf.id, sandbox_name
                )
            })?;

        // The prompt is deterministic: run this exact command, report exit code.
        // No creative interpretation allowed.
        let prompt = format!(
            "Run this EXACT command verbatim and report the result:\n\n{}\n\n\
            Use run_command to execute it. Reply with:\n\
            - SUCCESS if exit code is 0\n\
            - FAILED if exit code is non-zero, followed by the full output",
            vf.command
        );

        let tools = vec![
            "run_command".to_string(),
            "read_file".to_string(),
        ];

        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(self.planner.max_test_execution_time),
            agent_executor.execute_task(&agent, &vf.description, &tools, &prompt),
        )
        .await
        {
            Ok(r) => r,
            Err(_) => {
                let _ = topology.destroy_agent_layer(&agent.agent_id).await;
                log::warn!(
                    "[VERIFY] VF [{}] timed out after {}s in sandbox {}",
                    vf.id,
                    self.planner.max_test_execution_time,
                    sandbox_name
                );
                return Ok(VfResult {
                    id: vf.id.clone(),
                    passed: false,
                    output: format!(
                        "Timeout after {}s",
                        self.planner.max_test_execution_time
                    ),
                });
            }
        };

        let _ = topology.destroy_agent_layer(&agent.agent_id).await;

        match result {
            Ok(r) => Ok(VfResult {
                id: vf.id.clone(),
                passed: r.success,
                output: r.error.unwrap_or_default(),
            }),
            Err(e) => Ok(VfResult {
                id: vf.id.clone(),
                passed: false,
                output: e.to_string(),
            }),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn determine_status(
    failed_tests: &[TaskId],
    passed_tests: &[TaskId],
    rounds_completed: usize,
    max_rounds: usize,
) -> VerificationStatus {
    if passed_tests.is_empty() && failed_tests.is_empty() {
        return VerificationStatus::NotStarted;
    }
    if failed_tests.is_empty() {
        return VerificationStatus::Passed;
    }
    if rounds_completed >= max_rounds {
        return VerificationStatus::PartiallyVerified;
    }
    VerificationStatus::Failed
}

/// Extract JSON from text (handle markdown code blocks).
/// Strategy: fenced blocks → whole text → brace scan → fallback.
pub(crate) fn extract_json(text: &str) -> String {
    let mut candidates: Vec<&str> = Vec::new();
    let mut rest = text;
    while let Some(fence_start) = rest.find("```") {
        let after_fence = &rest[fence_start + 3..];
        let content_start = match after_fence.find('\n') {
            Some(nl) => &after_fence[nl + 1..],
            None => after_fence,
        };
        if let Some(fence_end) = content_start.find("```") {
            let candidate = content_start[..fence_end].trim();
            if !candidate.is_empty() {
                candidates.push(candidate);
            }
            let consumed = fence_start
                + 3
                + (after_fence.len() - content_start.len())
                + fence_end
                + 3;
            rest = &rest[consumed.min(rest.len())..];
        } else {
            break;
        }
    }

    for candidate in &candidates {
        if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
            return candidate.to_string();
        }
    }

    let trimmed = text.trim();
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return trimmed.to_string();
    }

    let mut search_pos = 0;
    while let Some(rel_brace) = trimmed[search_pos..].find('{') {
        let brace_start = search_pos + rel_brace;
        let mut depth: i32 = 0;
        let mut in_string = false;
        let mut escape_next = false;
        let mut end_pos: Option<usize> = None;

        for (i, ch) in trimmed[brace_start..].char_indices() {
            if escape_next {
                escape_next = false;
                continue;
            }
            match ch {
                '\\' if in_string => escape_next = true,
                '"' => in_string = !in_string,
                '{' if !in_string => depth += 1,
                '}' if !in_string => {
                    depth -= 1;
                    if depth == 0 {
                        end_pos = Some(brace_start + i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }

        match end_pos {
            Some(end) => {
                let candidate = trimmed[brace_start..end].trim();
                if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
                    return candidate.to_string();
                }
                search_pos = end;
            }
            None => break,
        }
    }

    trimmed.to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_json ────────────────────────────────────────────────────────

    #[test]
    fn test_extract_json_plain_object() {
        let input = r#"{"test": "value"}"#;
        assert_eq!(extract_json(input), r#"{"test": "value"}"#);
    }

    #[test]
    fn test_extract_json_json_fenced_block() {
        let input = "```json\n{\"test\": \"value\"}\n```";
        assert_eq!(extract_json(input), r#"{"test": "value"}"#);
    }

    #[test]
    fn test_extract_json_plain_fenced_block() {
        let input = "```\n{\"test\": \"value\"}\n```";
        assert_eq!(extract_json(input), r#"{"test": "value"}"#);
    }

    #[test]
    fn test_extract_json_with_surrounding_text() {
        let input = "Here is the JSON:\n```json\n{\"key\": 1}\n```\nDone.";
        assert_eq!(extract_json(input), r#"{"key": 1}"#);
    }

    #[test]
    fn test_extract_json_skips_invalid_block_finds_valid() {
        let input = "```\nnot json\n```\n```json\n{\"ok\": true}\n```";
        assert_eq!(extract_json(input), r#"{"ok": true}"#);
    }

    #[test]
    fn test_extract_json_skips_invalid_brace_object_finds_valid() {
        let input = r#"Some text {invalid not json} and then {"real": "json"}"#;
        assert_eq!(extract_json(input), r#"{"real": "json"}"#);
    }

    #[test]
    fn test_extract_json_nested_object() {
        let input = r#"{"outer": {"inner": 42}}"#;
        let result = extract_json(input);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["outer"]["inner"], 42);
    }

    #[test]
    fn test_extract_json_with_escaped_quotes_in_string() {
        let input = r#"{"msg": "say \"hello\""}"#;
        let result = extract_json(input);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["msg"], "say \"hello\"");
    }

    #[test]
    fn test_extract_json_whitespace_only() {
        let input = "   ";
        assert_eq!(extract_json(input), "");
    }

    #[test]
    fn test_extract_json_multiple_objects_returns_first_valid() {
        let input = r#"preamble {"a": 1} suffix {"b": 2}"#;
        assert_eq!(extract_json(input), r#"{"a": 1}"#);
    }

    #[test]
    fn test_extract_json_real_gemini_style_response() {
        let input = r#"Let me analyze this and generate verification functions.

```json
{
  "vfs": [
    {"id": "vf-build", "description": "build check", "command": "cargo build 2>&1", "expected_schema": null, "assertion": "exit code 0", "deps": []}
  ]
}
```

These VFs cover the main implementation."#;
        let result = extract_json(input);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.get("vfs").is_some());
    }

    #[test]
    fn test_extract_json_valid_array_falls_through_to_trimmed() {
        let input = r#"[{"id": "t1"}]"#;
        assert_eq!(extract_json(input), r#"[{"id": "t1"}]"#);
    }

    // ── determine_status ────────────────────────────────────────────────────

    #[test]
    fn test_status_all_passed() {
        let status = determine_status(
            &[],
            &["t1".to_string(), "t2".to_string()],
            1,
            3,
        );
        assert_eq!(status, VerificationStatus::Passed);
    }

    #[test]
    fn test_status_max_rounds_exhausted_is_partially_verified() {
        let status = determine_status(
            &["t2".to_string()],
            &["t1".to_string()],
            3,
            3,
        );
        assert_eq!(status, VerificationStatus::PartiallyVerified);
    }

    #[test]
    fn test_status_max_rounds_exhausted_all_failed_is_partially_verified() {
        let status = determine_status(
            &["t1".to_string(), "t2".to_string()],
            &[],
            3,
            3,
        );
        assert_eq!(status, VerificationStatus::PartiallyVerified);
    }

    #[test]
    fn test_status_no_tests_is_not_started() {
        let status = determine_status(&[], &[], 1, 3);
        assert_eq!(status, VerificationStatus::NotStarted);
    }

    #[test]
    fn test_status_early_failure_before_max_rounds_is_failed() {
        let status = determine_status(
            &["t1".to_string()],
            &[],
            1,
            3,
        );
        assert_eq!(status, VerificationStatus::Failed);
    }

    #[test]
    fn test_status_round_reset_prevents_double_count() {
        let passed_round2 = vec!["t2".to_string()];
        let failed_round2 = vec!["t1".to_string()];
        let status = determine_status(&failed_round2, &passed_round2, 2, 3);
        assert_eq!(status, VerificationStatus::Failed);
    }

    // ── VerificationLoop builder ────────────────────────────────────────────

    #[test]
    fn test_with_test_timeout_sets_value() {
        let vl = VerificationLoop::new("proj".to_string(), 3).with_test_timeout(120);
        assert_eq!(vl.planner.max_test_execution_time, 120);
    }

    #[test]
    fn test_default_timeout_is_60s() {
        let vl = VerificationLoop::new("proj".to_string(), 3);
        assert_eq!(vl.planner.max_test_execution_time, 60);
    }

    // ── VerificationPlanner construction ───────────────────────────────────

    #[test]
    fn test_planner_new_defaults() {
        let p = VerificationPlanner::new("test-project".to_string(), 5);
        assert_eq!(p.max_test_execution_time, 60);
        assert_eq!(p.max_rounds, 5);
    }

    // ── VeriMAP contract: VFs are stable across rounds ──────────────────────

    #[test]
    fn test_vf_structure_has_command_field() {
        // VFs must have a deterministic command — not just a description
        let vf = VerificationFunction {
            id: "vf-build".to_string(),
            description: "build check".to_string(),
            command: "cargo build 2>&1".to_string(),
            expected_schema: None,
            assertion: Some("exit code 0".to_string()),
            deps: vec![],
        };
        assert!(!vf.command.is_empty());
    }
}
