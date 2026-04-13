//! Layer 6: Verification Loop — Test task generation and failure re-injection

use super::agent_execution::AgentExecutor;
use super::sandbox_topology::TopologyManager;
use agentd_protocol::{SandboxName, Task, TaskGraph, TaskId, VerificationStatus};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

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
}

/// Verification planner
pub struct VerificationPlanner {
    project_id: String,
    max_rounds: usize,
}

impl VerificationPlanner {
    pub fn new(project_id: String, max_rounds: usize) -> Self {
        Self {
            project_id,
            max_rounds,
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
            .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS))
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

        // Parse JSON response
        let json_str = extract_json(text);

        #[derive(Deserialize)]
        struct VerificationJson {
            test_tasks: TaskGraph,
        }

        let parsed: VerificationJson =
            serde_json::from_str(json_str).context("Failed to parse verification JSON")?;

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
            .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS))
            .send()
            .await
            .context("Failed to send fix task request")?;

        if !response.status().is_success() {
            return Ok(vec![]); // Return empty on error
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

        let parsed: FixTasksJson =
            serde_json::from_str(json_str).unwrap_or(FixTasksJson { fix_tasks: vec![] });

        Ok(parsed.fix_tasks)
    }
}

/// Extract JSON from text (handle markdown code blocks)
fn extract_json(text: &str) -> &str {
    if text.contains("```json") {
        text.split("```json")
            .nth(1)
            .and_then(|s| s.split("```").next())
            .unwrap_or(text)
            .trim()
    } else if text.contains("```") {
        text.split("```")
            .nth(1)
            .and_then(|s| s.split("```").next())
            .unwrap_or(text)
            .trim()
    } else {
        text.trim()
    }
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

    /// Run verification loop for a sandbox
    pub async fn verify_sandbox(
        &self,
        sandbox_name: &SandboxName,
        merged_diff: &str,
        original_tasks: &[Task],
        topology: &TopologyManager,
        agent_executor: &AgentExecutor,
    ) -> Result<VerificationResult> {
        let mut passed_tests = Vec::new();
        let mut failed_tests = Vec::new();
        let mut rounds_completed = 0;

        for round in 0..self.max_rounds {
            rounds_completed = round + 1;
            log::info!("  → Verification round {}/{}", round + 1, self.max_rounds);

            // Generate test tasks (LLM call)
            let plan = self.planner
                .generate_test_tasks(sandbox_name, merged_diff, original_tasks)
                .await?;

            // Execute each test task
            let mut round_failures = Vec::new();
            let test_tools = vec![
                "run_command".to_string(),
                "read_file".to_string(),
                "list_files".to_string(),
            ];

            for test_task in &plan.test_tasks.tasks {
                let agent = match topology
                    .create_agent_layer(sandbox_name, Some(test_task.id.clone()))
                    .await
                {
                    Ok(a) => a,
                    Err(e) => {
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

                let result = agent_executor
                    .execute_task(&agent, &test_task.description, &test_tools, &test_prompt)
                    .await;

                let _ = topology.destroy_agent_layer(&agent.agent_id).await;

                match result {
                    Ok(r) if r.success => {
                        passed_tests.push(test_task.id.clone());
                    }
                    Ok(r) => {
                        let error = r.error.unwrap_or_else(|| "Test failed".to_string());
                        failed_tests.push(test_task.id.clone());
                        round_failures.push((
                            test_task.id.clone(),
                            test_task.description.clone(),
                            error,
                        ));
                    }
                    Err(e) => {
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
                log::info!("  ✓ All {} tests passed in round {}", passed_tests.len(), round + 1);
                break;
            }

            log::info!("  ⚠ {} tests failed in round {}", round_failures.len(), round + 1);

            // Generate and execute fix tasks for failures
            if round < self.max_rounds - 1 {
                let fix_tools = vec![
                    "read_file".to_string(),
                    "write_file".to_string(),
                    "run_command".to_string(),
                    "grep".to_string(),
                ];

                for (test_id, desc, error) in &round_failures {
                    let fix_tasks = self.planner
                        .generate_fix_tasks(test_id, desc, error)
                        .await
                        .unwrap_or_default();

                    for fix_task in fix_tasks {
                        let agent = match topology
                            .create_agent_layer(sandbox_name, Some(fix_task.id.clone()))
                            .await
                        {
                            Ok(a) => a,
                            Err(e) => {
                                log::warn!("  ⚠ Failed to create fix agent: {}", e);
                                continue;
                            }
                        };

                        let fix_prompt = format!("Fix this issue: {}", fix_task.description);
                        let _ = agent_executor
                            .execute_task(&agent, &fix_task.description, &fix_tools, &fix_prompt)
                            .await;

                        let _ = topology.destroy_agent_layer(&agent.agent_id).await;
                    }
                }
            }
        }

        let status = if failed_tests.is_empty() {
            VerificationStatus::Passed
        } else if !passed_tests.is_empty() {
            VerificationStatus::PartiallyVerified
        } else {
            VerificationStatus::Failed
        };

        Ok(VerificationResult {
            status,
            passed_tests,
            failed_tests,
            rounds_completed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json() {
        let input = r#"```json
{"test": "value"}
```"#;
        let result = extract_json(input);
        assert_eq!(result, r#"{"test": "value"}"#);
    }

    #[test]
    fn test_extract_json_plain() {
        let input = r#"{"test": "value"}"#;
        let result = extract_json(input);
        assert_eq!(result, r#"{"test": "value"}"#);
    }
}
