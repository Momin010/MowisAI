//! Layer 6: Verification Loop — Test task generation and failure re-injection

use super::agent_execution::AgentExecutor;
use super::sandbox_topology::TopologyManager;
use agentd_protocol::{SandboxName, Task, TaskGraph, TaskId, VerificationStatus};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Verification planner output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationPlan {
    pub test_tasks: TaskGraph,
    pub sandbox_name: SandboxName,
}

/// Verification result
#[derive(Debug, Clone)]
pub struct VerificationResult {
    pub status: VerificationStatus,
    pub passed_tests: Vec<TaskId>,
    pub failed_tests: Vec<TaskId>,
    pub rounds_completed: usize,
    pub updated_diff: Option<String>,
}

/// Verification planner
pub struct VerificationPlanner {
    project_id: String,
    max_rounds: usize,
    /// Per-test agent execution timeout in seconds (default: 60)
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

    /// Generate verification test task graph for a sandbox
    pub async fn generate_test_tasks(
        &self,
        sandbox_name: &SandboxName,
        merged_diff: &str,
        original_tasks: &[Task],
    ) -> Result<VerificationPlan> {
        let access_token = super::gcloud_access_token()?;
        let url = super::vertex_generate_url(&self.project_id);

        let system_prompt = r#"You are a verification test planner. Given a merged diff and original task descriptions, generate a test task graph to verify the implementation.

Output a JSON object with this structure:
{
  "test_tasks": {
    "tasks": [
      {"id": "test-1", "description": "run unit tests", "deps": [], "hint": null},
      {"id": "test-2", "description": "check linting", "deps": [], "hint": null},
      {"id": "test-3", "description": "integration test", "deps": ["test-1"], "hint": null}
    ]
  }
}

Test types to include:
- Unit tests (if code changes present)
- Integration tests (if API/interface changes)
- Linting/formatting checks
- Type checking (for typed languages)
- Build verification

Keep test tasks small and focused. Use deps to order tests properly.
"#;

        let task_summaries = original_tasks
            .iter()
            .map(|t| format!("- {}: {}", t.id, t.description))
            .collect::<Vec<_>>()
            .join("\n");

        let user_message = format!(
            "Sandbox: {}\n\nOriginal tasks:\n{}\n\nMerged diff:\n{}\n\nGenerate verification test tasks:",
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
            "generationConfig": super::vertex_generation_config_json(0.2)
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
            .context("Failed to send verification planning request")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Gemini API error: {}", error_text));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse verification response")?;

        let text = response_json
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.get(0))
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow!("Invalid verification response structure"))?;

        let json_str = extract_json(text);

        #[derive(Deserialize)]
        struct VerificationJson {
            test_tasks: TaskGraph,
        }

        let parsed: VerificationJson =
            serde_json::from_str(&json_str).context("Failed to parse verification JSON")?;

        Ok(VerificationPlan {
            test_tasks: parsed.test_tasks,
            sandbox_name: sandbox_name.clone(),
        })
    }

    /// Generate fix tasks from test failures
    pub async fn generate_fix_tasks(
        &self,
        failed_test_id: &TaskId,
        test_description: &str,
        failure_output: &str,
    ) -> Result<Vec<Task>> {
        let access_token = super::gcloud_access_token()?;
        let url = super::vertex_generate_url(&self.project_id);

        let system_prompt = r#"You are a test failure analyzer. Given a failed test and its output, generate fix tasks.

Output JSON in this format:
{
  "fix_tasks": [
    {"id": "fix-1", "description": "fix the identified bug", "deps": [], "hint": null}
  ]
}

Each fix task should be specific and actionable.
"#;

        let user_message = format!(
            "Failed test: {}\nDescription: {}\n\nFailure output:\n{}\n\nGenerate fix tasks:",
            failed_test_id, test_description, failure_output
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

        // CR fix: log parse errors instead of silently returning empty
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

/// Extract JSON from text (handle markdown code blocks).
///
/// Strategy (in order):
/// 1. All fenced code blocks (` ```json ` and ` ``` `) — returns first valid JSON.
/// 2. Whole trimmed text, if it is valid JSON.
/// 3. Scan for every top-level `{...}` object in the text and return the first
///    one that is valid JSON (not just the first one found, avoiding the bug
///    where a preceding invalid snippet caused the scan to abort early).
/// 4. Fallback — return trimmed text and let the caller surface the parse error.
pub(crate) fn extract_json(text: &str) -> String {
    // 1. Collect all fenced-code-block candidates, in order.
    let mut candidates: Vec<&str> = Vec::new();
    let mut rest = text;
    while let Some(fence_start) = rest.find("```") {
        let after_fence = &rest[fence_start + 3..];
        // Skip the optional language tag (everything up to the first newline)
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

    // 2. Try the whole trimmed text as plain JSON.
    let trimmed = text.trim();
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return trimmed.to_string();
    }

    // 3. Scan for every top-level `{...}` brace object and return the first
    //    that is valid JSON.  We continue past objects that fail JSON validation
    //    so that a preceding invalid snippet does not block the real payload.
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
                // This object was not valid JSON — advance past it and keep looking.
                search_pos = end;
            }
            None => break, // Unclosed brace — no more complete objects possible.
        }
    }

    // 4. Fallback — return trimmed text and let the caller surface the parse error.
    trimmed.to_string()
}

/// Determine final verification status after all rounds complete.
///
/// Per the architecture spec Layer 6 rule:
/// - If the last round had no failures → `Passed`
/// - If max rounds were exhausted with remaining failures → `PartiallyVerified`
/// - If there are no test tasks at all → `NotStarted`
/// - Otherwise (unexpected) → `Failed`
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
        // Last round everything passed.
        return VerificationStatus::Passed;
    }
    if rounds_completed >= max_rounds {
        // Per spec: max rounds exhausted → PartiallyVerified regardless of pass/fail mix.
        return VerificationStatus::PartiallyVerified;
    }
    VerificationStatus::Failed
}

/// Verification loop controller
pub struct VerificationLoop {
    planner: VerificationPlanner,
    max_rounds: usize,
}

impl VerificationLoop {
    pub fn new(project_id: String, max_rounds: usize) -> Self {
        Self {
            planner: VerificationPlanner::new(project_id, max_rounds),
            max_rounds,
        }
    }

    /// Override the per-task execution timeout (builder style).
    ///
    /// CR fix: `max_test_execution_time` was previously only reachable via the
    /// internal `VerificationPlanner` field. This method exposes it through
    /// `VerificationLoop` so callers can control the timeout without reaching
    /// into private state.
    pub fn with_test_timeout(mut self, secs: u64) -> Self {
        self.planner.max_test_execution_time = secs;
        self
    }

    /// Run verification loop for a sandbox
    pub async fn verify_sandbox(
        &self,
        sandbox_name: &SandboxName,
        merged_diff: &str,
        original_tasks: &[Task],
        topology: &TopologyManager,
        agent_executor: &AgentExecutor,
    ) -> Result<VerificationResult> {
        let mut current_merged_diff = merged_diff.to_string();
        // Per-round vectors; cleared at the start of each round so the final
        // result reflects only what happened in the last completed round.
        let mut passed_tests: Vec<TaskId> = Vec::new();
        let mut failed_tests: Vec<TaskId> = Vec::new();
        let mut rounds_completed = 0;

        log::info!(
            "[VERIFY] Starting for sandbox: {}, diff_len: {}, tasks: {}",
            sandbox_name,
            current_merged_diff.len(),
            original_tasks.len()
        );

        for round in 0..self.max_rounds {
            rounds_completed = round + 1;

            // CR fix: reset per round so status reflects the *last* round only,
            // preventing double-counting of results across rounds.
            passed_tests.clear();
            failed_tests.clear();

            log::info!(
                "[VERIFY] Round {}/{} — calling Gemini for test tasks",
                round + 1,
                self.max_rounds
            );

            let plan = self
                .planner
                .generate_test_tasks(sandbox_name, &current_merged_diff, original_tasks)
                .await?;
            log::info!(
                "[VERIFY] Gemini returned {} test tasks",
                plan.test_tasks.tasks.len()
            );

            let mut round_failures: Vec<(TaskId, String, String)> = Vec::new();
            let test_tools = vec![
                "run_command".to_string(),
                "read_file".to_string(),
                "list_files".to_string(),
            ];

            for test_task in &plan.test_tasks.tasks {
                log::info!("[VERIFY] Running test task: {}", test_task.id);

                let agent = match topology
                    .create_agent_layer(sandbox_name, Some(test_task.id.clone()))
                    .await
                {
                    Ok(a) => a,
                    Err(e) => {
                        // CR fix: log with full context so the failure is visible.
                        log::error!(
                            "[VERIFY] Failed to create agent for test {} in sandbox {}: {}",
                            test_task.id,
                            sandbox_name,
                            e
                        );
                        failed_tests.push(test_task.id.clone());
                        round_failures.push((
                            test_task.id.clone(),
                            test_task.description.clone(),
                            e.to_string(),
                        ));
                        continue;
                    }
                };

                let test_prompt = format!(
                    "You are a test verification agent. Run this test:\n{}\n\n\
                    Use run_command to execute tests. Report success or failure.",
                    test_task.description
                );

                // CR note: tokio::time::timeout cancels the future on expiry, so
                // Layer 4's tier-2/3 recovery paths are bypassed.  This is an
                // acceptable trade-off inside the verification loop because test
                // agents should never need multi-tier recovery; the outer loop
                // handles persistent failures by injecting fix tasks.
                let result = match tokio::time::timeout(
                    std::time::Duration::from_secs(self.planner.max_test_execution_time),
                    agent_executor.execute_task(
                        &agent,
                        &test_task.description,
                        &test_tools,
                        &test_prompt,
                    ),
                )
                .await
                {
                    Ok(r) => r,
                    Err(_) => {
                        log::warn!(
                            "[VERIFY] Test task {} timed out after {}s in sandbox {}",
                            test_task.id,
                            self.planner.max_test_execution_time,
                            sandbox_name
                        );
                        Err(anyhow!(
                            "Test execution timeout after {}s",
                            self.planner.max_test_execution_time
                        ))
                    }
                };

                let _ = topology.destroy_agent_layer(&agent.agent_id).await;

                match result {
                    Ok(r) if r.success => {
                        log::info!(
                            "[VERIFY] Test task {} finished: success=true",
                            test_task.id
                        );
                        passed_tests.push(test_task.id.clone());
                    }
                    Ok(r) => {
                        let error = r.error.unwrap_or_else(|| "Test failed".to_string());
                        log::info!(
                            "[VERIFY] Test task {} finished: success=false — {}",
                            test_task.id,
                            error
                        );
                        failed_tests.push(test_task.id.clone());
                        round_failures.push((
                            test_task.id.clone(),
                            test_task.description.clone(),
                            error,
                        ));
                    }
                    Err(e) => {
                        log::info!(
                            "[VERIFY] Test task {} finished: success=false — {}",
                            test_task.id,
                            e
                        );
                        failed_tests.push(test_task.id.clone());
                        round_failures.push((
                            test_task.id.clone(),
                            test_task.description.clone(),
                            e.to_string(),
                        ));
                    }
                }
            }

            if round_failures.is_empty() {
                log::info!(
                    "  ✓ All {} tests passed in round {}",
                    passed_tests.len(),
                    round + 1
                );
                break;
            }

            log::info!(
                "  ⚠ {} tests failed in round {}",
                round_failures.len(),
                round + 1
            );

            // Generate and execute fix tasks for failures (not on the last round)
            if round < self.max_rounds - 1 {
                let fix_tools = vec![
                    "read_file".to_string(),
                    "write_file".to_string(),
                    "run_command".to_string(),
                    "grep".to_string(),
                ];

                for (test_id, desc, error) in &round_failures {
                    log::info!(
                        "[VERIFY] Generating fix tasks for failed test: {}",
                        test_id
                    );
                    let fix_tasks = self
                        .planner
                        .generate_fix_tasks(test_id, desc, error)
                        .await
                        .unwrap_or_default();

                    if fix_tasks.is_empty() {
                        log::warn!(
                            "[VERIFY] No fix tasks generated for failed test {} in sandbox {}",
                            test_id,
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

                        let fix_prompt =
                            format!("Fix this issue: {}", fix_task.description);

                        let fix_result = match tokio::time::timeout(
                            std::time::Duration::from_secs(self.planner.max_test_execution_time),
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
                                                fix_task.id, sandbox_name, e
                                            );
                                        } else {
                                            log::info!(
                                                "[VERIFY] Applied fix diff for {} to sandbox {}",
                                                fix_task.id, sandbox_name
                                            );
                                            match topology.capture_sandbox_diff(sandbox_name).await {
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
                                                        "[VERIFY] Applied fix {}, but failed to refresh sandbox diff for {}: {}",
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
                                fix_task.id, e
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
}

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
        // First code block is not valid JSON, second is.
        let input = "```\nnot json\n```\n```json\n{\"ok\": true}\n```";
        assert_eq!(extract_json(input), r#"{"ok": true}"#);
    }

    #[test]
    fn test_extract_json_skips_invalid_brace_object_finds_valid() {
        // CR fix: brace scanner must continue past invalid objects, not stop at first.
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
        // Falls through to fallback — caller gets trimmed empty string
        assert_eq!(extract_json(input), "");
    }

    #[test]
    fn test_extract_json_multiple_objects_returns_first_valid() {
        let input = r#"preamble {"a": 1} suffix {"b": 2}"#;
        assert_eq!(extract_json(input), r#"{"a": 1}"#);
    }

    #[test]
    fn test_extract_json_real_gemini_style_response() {
        // Mimics a typical Gemini response with thinking text + code block.
        let input = r#"Let me analyze this and generate test tasks.

```json
{
  "test_tasks": {
    "tasks": [
      {"id": "test-1", "description": "run cargo test", "deps": [], "hint": null}
    ]
  }
}
```

These tests cover the main implementation."#;
        let result = extract_json(input);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.get("test_tasks").is_some());
    }

    #[test]
    fn test_extract_json_valid_array_falls_through_to_trimmed() {
        // Arrays are valid JSON but the brace scanner won't find them.
        // Step 2 (whole trimmed text) should return them directly.
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
        // CR fix: spec says max-rounds exhaustion → PartiallyVerified, not Failed.
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
        // Even all-failed after max rounds → PartiallyVerified per spec.
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
        // Fails on round 1 of 3 (rounds_completed < max_rounds).
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
        // Simulate: round 1 passes t1, fails t2.
        // Round 2 (after fix): passes t2, but t1 regresses.
        // Per-round clearing means only round 2 results are present.
        // rounds_completed(2) < max_rounds(3) and failed_tests is non-empty → Failed.
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
}
