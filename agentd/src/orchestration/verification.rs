//! Layer 6: Verification Loop — Automated testing and fix re-injection

use super::agent_execution::AgentExecutor;
use super::sandbox_topology::TopologyManager;
use super::types::VerificationTask;
use agentd_protocol::VerificationStatus;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Generates a test task graph from a merged sandbox diff
pub struct VerificationPlanner {
    project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestPlan {
    pub tasks: Vec<VerificationTask>,
}

impl VerificationPlanner {
    pub fn new(project_id: String) -> Self {
        Self { project_id }
    }

    /// Ask Gemini to generate test tasks based on the merged diff
    pub async fn generate_test_plan(
        &self,
        sandbox_name: &str,
        merged_diff: &str,
        original_tasks: &[agentd_protocol::Task],
    ) -> Result<TestPlan> {
        let access_token = crate::claude_integration::gcloud_access_token()?;
        let url = crate::vertex_agent::vertex_generate_url(&self.project_id);

        let original_task_descriptions = original_tasks
            .iter()
            .map(|t| format!("- {}", t.description))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "You are a QA automation expert. Review this diff and generate test tasks to verify it works.\n\n\
             ORIGINAL TASKS:\n{}\n\n\
             MERGED DIFF:\n{}\n\n\
             Output a JSON array of test tasks to verify this code. \n\
             Each task must have: \n\
             - id: e.g. 'test-1'\n\
             - description: specific shell command to run or script to write and run\n\n\
             Respond ONLY with a raw JSON array of objects. No markdown wrapping.",
            original_task_descriptions, merged_diff
        );

        let request_body = json!({
            "contents": [{
                "role": "user",
                "parts": [{"text": prompt}]
            }],
            "systemInstruction": {
                "parts": [{
                    "text": "You are a QA planning agent. Output ONLY valid raw JSON array."
                }]
            },
            "generationConfig": crate::vertex_agent::vertex_generation_config_json(0.2)
        });

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .context("Failed to call Gemini for test plan")?;

        if !response.status().is_success() {
            return Err(anyhow!("Gemini API error: {}", response.status()));
        }

        let response_json: serde_json::Value = response.json().await?;
        let text = response_json
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.get(0))
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow!("Invalid Gemini response structure"))?;

        // Try parsing directly first
        let clean_json = if text.trim().starts_with("```json") {
            let start = text.find("```json").unwrap() + 7;
            let end = text.rfind("```").unwrap();
            &text[start..end]
        } else if text.trim().starts_with("```") {
            let start = text.find("```").unwrap() + 3;
            let end = text.rfind("```").unwrap();
            &text[start..end]
        } else {
            text
        };

        let tasks: Vec<VerificationTask> = serde_json::from_str(clean_json.trim())
            .context("Failed to parse test plan JSON from Gemini output")?;

        Ok(TestPlan { tasks })
    }
}

/// The verification loop that executes tests and re-injects fixes
pub struct VerificationLoop {
    project_id: String,
    max_rounds: usize,
}

#[derive(Debug, Clone)]
pub struct VerificationResult {
    pub status: VerificationStatus,
    pub passed_tests: Vec<String>,
    pub failed_tests: Vec<String>,
    pub updated_diff: Option<String>,
    pub rounds_completed: usize,
}

impl VerificationLoop {
    pub fn new(project_id: String, max_rounds: usize) -> Self {
        Self {
            project_id,
            max_rounds,
        }
    }

    /// Run the verification loop for a sandbox
    pub async fn verify_sandbox(
        &self,
        sandbox_name: &str,
        merged_diff: &str,
        original_tasks: &[agentd_protocol::Task],
        topology: &TopologyManager,
        executor: &AgentExecutor,
    ) -> Result<VerificationResult> {
        let planner = VerificationPlanner::new(self.project_id.clone());

        // 1. Generate test plan
        let plan = planner.generate_test_plan(sandbox_name, merged_diff, original_tasks).await?;

        if plan.tasks.is_empty() {
            return Ok(VerificationResult {
                status: VerificationStatus::Passed,
                passed_tests: Vec::new(),
                failed_tests: Vec::new(),
                updated_diff: None,
                rounds_completed: 0,
            });
        }

        let mut passed_tests = Vec::new();
        let mut failed_tests = Vec::new();
        let mut rounds_completed = 0;
        let mut current_diff = merged_diff.to_string();

        // 2. Execute verification rounds
        for round in 0..self.max_rounds {
            rounds_completed = round + 1;
            passed_tests.clear();
            failed_tests.clear();

            log::info!("  → Starting verification round {}/{} for {}", rounds_completed, self.max_rounds, sandbox_name);

            // Execute each test task sequentially for now (can be parallelized later)
            for test_task in &plan.tasks {
                let test_tools = vec![
                    "run_command".to_string(),
                    "read_file".to_string(),
                    "list_files".to_string(),
                    "write_file".to_string(),
                ];

                let test_prompt = format!(
                    "You are a verification agent in a production environment.\n\
                    WORKING DIRECTORY: /workspace (You MUST `cd /workspace` before running tests).\n\
                    Run this test:\n{}\n\n\
                    Use run_command to execute tests (e.g., `cd /workspace && cargo test`). \
                    Report success or failure.",
                    test_task.description
                );

                let test_agent = topology.wake_or_create_agent_layer(sandbox_name, Some(test_task.id.clone())).await?;
                
                let result = executor.execute_task(
                    &test_agent,
                    &test_task.description,
                    &test_tools,
                    &test_prompt
                ).await;

                let _ = topology.sleep_agent_layer(&test_agent.agent_id, sandbox_name).await;

                match result {
                    Ok(r) if r.success => passed_tests.push(test_task.id.clone()),
                    Ok(r) => {
                        log::warn!("    ⚠ Test failed: {}", test_task.id);
                        failed_tests.push(test_task.id.clone());
                        
                        // 3. Generate fix task for failure
                        let fix_tools = vec![
                            "read_file".to_string(),
                            "write_file".to_string(),
                            "run_command".to_string(),
                            "grep".to_string(),
                            "list_files".to_string(),
                        ];

                        let fix_prompt = format!(
                            "Fix this test failure: {}\n\
                             WORKING DIRECTORY: /workspace. All project files are in /workspace. \n\
                             Remember to `cd /workspace` if running commands.", 
                            test_task.description
                        );

                        let fix_agent = topology.wake_or_create_agent_layer(sandbox_name, Some(format!("fix-{}", test_task.id))).await?;
                        
                        let fix_result = executor.execute_task(
                            &fix_agent,
                            &format!("Fix failing test: {}", test_task.description),
                            &fix_tools,
                            &fix_prompt
                        ).await;

                        if let Ok(fr) = fix_result {
                            if fr.success {
                                if let Some(diff) = fr.git_diff {
                                    if !diff.is_empty() {
                                        // Apply fix directly to sandbox
                                        if let Err(e) = topology.apply_diff_to_sandbox(sandbox_name, &diff).await {
                                            log::warn!("    ⚠ Failed to apply fix diff: {}", e);
                                        } else {
                                            log::info!("    ✓ Applied fix for {}", test_task.id);
                                            // Update our running diff
                                            current_diff = format!("{}\n{}", current_diff, diff);
                                        }
                                    }
                                }
                            }
                        }
                        
                        let _ = topology.sleep_agent_layer(&fix_agent.agent_id, sandbox_name).await;
                    },
                    Err(_) => {
                        log::warn!("    ⚠ Test execution failed: {}", test_task.id);
                        failed_tests.push(test_task.id.clone());
                    }
                }
            }

            // If all tests passed in this round, we are done
            if failed_tests.is_empty() {
                return Ok(VerificationResult {
                    status: VerificationStatus::Passed,
                    passed_tests,
                    failed_tests: Vec::new(),
                    updated_diff: Some(current_diff),
                    rounds_completed,
                });
            }
            
            // Otherwise, we loop to the next round to re-run the tests
        }

        // Exceeded max rounds
        Ok(VerificationResult {
            status: VerificationStatus::PartiallyVerified,
            passed_tests,
            failed_tests,
            updated_diff: Some(current_diff),
            rounds_completed,
        })
    }
}
