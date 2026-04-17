//! Simulation command for end-to-end testing without LLM calls
//!
//! This command runs the entire 7-layer orchestration system with mock agents
//! to test scheduling, overlayfs propagation, merging, and all other components
//! for $0 cost. Perfect for development and debugging.

use super::mock_agent::MockAgentExecutor;
use super::sandbox_topology::TopologyManager;
use super::scheduler::{Scheduler, SchedulerStats};
use super::verification::{VerificationPlan, VerificationResult};
use agentd_protocol::{SandboxConfig, SandboxName, Task, TaskGraph, TaskId, VerificationStatus};
use anyhow::{Context, Result};
use clap::Parser;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

// ─────────────────────────────────────────────────────────────────────────────
// SimulatedVerificationPlanner
// ─────────────────────────────────────────────────────────────────────────────

/// Mock verification planner that returns deterministic test tasks without LLM calls
pub struct SimulatedVerificationPlanner {
    max_rounds: usize,
    /// Per-test agent execution timeout in seconds
    pub max_test_execution_time: u64,
}

impl SimulatedVerificationPlanner {
    pub fn new(max_rounds: usize) -> Self {
        Self {
            max_rounds,
            max_test_execution_time: 10,
        }
    }

    /// Generate deterministic test tasks based on sandbox content
    pub async fn generate_test_tasks(
        &self,
        sandbox_name: &SandboxName,
        _merged_diff: &str,
        _original_tasks: &[Task],
    ) -> Result<VerificationPlan> {
        log::info!(
            "[VERIFY] Generating simulated test tasks for sandbox: {}",
            sandbox_name
        );

        let test_tasks = TaskGraph {
            tasks: vec![
                Task {
                    id: format!("test-{}-file-exists", sandbox_name),
                    description: format!(
                        "Verify files exist in sandbox {}",
                        sandbox_name
                    ),
                    deps: vec![],
                    hint: None,
                },
                Task {
                    id: format!("test-{}-content-valid", sandbox_name),
                    description: format!(
                        "Verify file content is valid JS in sandbox {}",
                        sandbox_name
                    ),
                    deps: vec![format!("test-{}-file-exists", sandbox_name)],
                    hint: None,
                },
            ],
        };

        log::info!(
            "[VERIFY] Generated {} deterministic test tasks",
            test_tasks.tasks.len()
        );

        Ok(VerificationPlan {
            test_tasks,
            sandbox_name: sandbox_name.clone(),
        })
    }

    /// Generate deterministic fix tasks for failed tests
    pub async fn generate_fix_tasks(
        &self,
        failed_test_id: &TaskId,
        _test_description: &str,
        failure_output: &str,
    ) -> Result<Vec<Task>> {
        log::info!(
            "[VERIFY] Generating simulated fix tasks for failed test: {}",
            failed_test_id
        );

        Ok(vec![Task {
            id: format!("fix-{}", failed_test_id),
            description: format!(
                "Apply fix for test failure: {}",
                failure_output
                    .lines()
                    .next()
                    .unwrap_or("unknown error")
                    .chars()
                    .take(80)
                    .collect::<String>()
            ),
            deps: vec![],
            hint: None,
        }])
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SimulatedVerificationLoop
// ─────────────────────────────────────────────────────────────────────────────

/// Verification loop that uses mock agents instead of real LLM-backed agents.
///
/// Mirrors the logic of `VerificationLoop` (Layer 6) but drives execution
/// through `MockAgentExecutor` so the entire verification path can be exercised
/// at $0 cost.
pub struct SimulatedVerificationLoop {
    planner: SimulatedVerificationPlanner,
    executor: Arc<MockAgentExecutor>,
    topology: Arc<TopologyManager>,
    /// Probability (0.0–1.0) that a single test task is marked as failed
    verify_failure_rate: f64,
}

impl SimulatedVerificationLoop {
    pub fn new(
        planner: SimulatedVerificationPlanner,
        executor: Arc<MockAgentExecutor>,
        topology: Arc<TopologyManager>,
        verify_failure_rate: f64,
    ) -> Self {
        Self {
            planner,
            executor,
            topology,
            verify_failure_rate,
        }
    }

    /// Run the verification loop for a single sandbox.
    ///
    /// Follows the same multi-round retry logic as `VerificationLoop::verify_sandbox`:
    /// - Per-round clearing of pass/fail lists (no double-counting)
    /// - Fix tasks generated and applied when tests fail
    /// - Stops early if all tests pass
    /// - Returns `PartiallyVerified` when max rounds are exhausted with failures
    pub async fn verify_sandbox(
        &self,
        sandbox_name: &SandboxName,
        merged_diff: &str,
        original_tasks: &[Task],
    ) -> Result<VerificationResult> {
        let max_rounds = self.planner.max_rounds;

        // Per-round tracking; cleared at start of each round to reflect only
        // the most recent round's outcome (mirrors VerificationLoop behaviour).
        let mut passed_tests: Vec<TaskId> = Vec::new();
        let mut failed_tests: Vec<TaskId> = Vec::new();
        let mut rounds_completed = 0;

        log::info!(
            "[VERIFY] Starting simulated verification for sandbox: {}, diff_len: {}, tasks: {}",
            sandbox_name,
            merged_diff.len(),
            original_tasks.len()
        );

        for round in 0..max_rounds {
            rounds_completed = round + 1;

            // Reset each round — only the last round's results count.
            passed_tests.clear();
            failed_tests.clear();

            log::info!(
                "[VERIFY] Round {}/{} — generating deterministic test tasks",
                round + 1,
                max_rounds
            );

            let plan = self
                .planner
                .generate_test_tasks(sandbox_name, merged_diff, original_tasks)
                .await?;

            log::info!(
                "[VERIFY] {} test tasks to run this round",
                plan.test_tasks.tasks.len()
            );

            let mut round_failures: Vec<(TaskId, String, String)> = Vec::new();

            for test_task in &plan.test_tasks.tasks {
                log::info!("[VERIFY] Running test task: {}", test_task.id);

                let agent = match self
                    .topology
                    .create_agent_layer(sandbox_name, Some(test_task.id.clone()))
                    .await
                {
                    Ok(a) => a,
                    Err(e) => {
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

                let result = match tokio::time::timeout(
                    std::time::Duration::from_secs(self.planner.max_test_execution_time),
                    self.executor.execute_verification_task(
                        &agent,
                        &test_task.description,
                        &self.topology,
                        self.verify_failure_rate,
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
                        Err(anyhow::anyhow!(
                            "Test execution timeout after {}s",
                            self.planner.max_test_execution_time
                        ))
                    }
                };

                let _ = self.topology.destroy_agent_layer(&agent.agent_id).await;

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
                "  ⚠ {} test(s) failed in round {}",
                round_failures.len(),
                round + 1
            );

            // Generate and execute fix tasks (not on the last round)
            if round < max_rounds - 1 {
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

                    for fix_task in fix_tasks {
                        let agent = match self
                            .topology
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

                        let fix_result = match tokio::time::timeout(
                            std::time::Duration::from_secs(self.planner.max_test_execution_time),
                            self.executor.execute_fix_task(
                                &agent,
                                &fix_task.description,
                                &self.topology,
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
                                Err(anyhow::anyhow!("Fix execution timeout"))
                            }
                        };

                        match fix_result {
                            Ok(r) if r.success => {
                                log::info!(
                                    "[VERIFY] Fix task {} succeeded for sandbox {}",
                                    fix_task.id,
                                    sandbox_name
                                );
                            }
                            Ok(r) => {
                                log::warn!(
                                    "[VERIFY] Fix task {} failed: {}",
                                    fix_task.id,
                                    r.error.unwrap_or_default()
                                );
                            }
                            Err(e) => {
                                log::warn!("[VERIFY] Fix task {} error: {}", fix_task.id, e);
                            }
                        }

                        let _ = self.topology.destroy_agent_layer(&agent.agent_id).await;
                    }
                }
            }
        }

        let status = sim_determine_status(&failed_tests, &passed_tests, rounds_completed, max_rounds);

        Ok(VerificationResult {
            status,
            passed_tests,
            failed_tests,
            rounds_completed,
        })
    }
}

/// Determine verification status from the last round's results.
/// Mirrors `determine_status` in `verification.rs`.
fn sim_determine_status(
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

// ─────────────────────────────────────────────────────────────────────────────
// SimulateCommand
// ─────────────────────────────────────────────────────────────────────────────

/// Run full orchestration simulation with mock agents
#[derive(Parser, Debug)]
pub struct SimulateCommand {
    /// Path to agentd socket
    #[arg(long, default_value = "/tmp/agentd.sock")]
    pub socket: String,

    /// Number of tasks to simulate
    #[arg(long, default_value = "10")]
    pub tasks: usize,

    /// Number of sandboxes to create
    #[arg(long, default_value = "1")]
    pub sandboxes: usize,

    /// Maximum concurrent agents
    #[arg(long, default_value = "20")]
    pub max_agents: usize,

    /// Random failure rate (0.0 - 1.0)
    #[arg(long, default_value = "0.0")]
    pub failure_rate: f64,

    /// Delay per tool call in milliseconds
    #[arg(long, default_value = "100")]
    pub tool_delay: u64,

    /// Project root directory
    #[arg(long, default_value = "/tmp/mock-project")]
    pub project_root: PathBuf,

    /// Verbose output
    #[arg(long, short, default_value = "false")]
    pub verbose: bool,

    /// Enable Layer 6 verification testing
    #[arg(long, default_value = "false")]
    pub verify: bool,

    /// Verification failure rate (0.0 - 1.0)
    #[arg(long, default_value = "0.0")]
    pub verify_failure_rate: f64,
}

impl SimulateCommand {
    pub async fn run(&self) -> Result<()> {
        let start_time = std::time::Instant::now();

        log::info!("🚀 MowisAI Orchestration Simulation");
        log::info!("═══════════════════════════════════");
        log::info!("📋 Tasks: {}", self.tasks);
        log::info!("📦 Sandboxes: {}", self.sandboxes);
        log::info!("🤖 Max agents: {}", self.max_agents);
        log::info!("⚡ Failure rate: {}%", self.failure_rate * 100.0);
        log::info!("⏱️  Tool delay: {}ms", self.tool_delay);
        if self.verify {
            log::info!(
                "🔍 Verification: enabled (failure rate: {:.0}%)",
                self.verify_failure_rate * 100.0
            );
        }
        log::info!("═══════════════════════════════════\n");

        // Create project directory if it doesn't exist
        std::fs::create_dir_all(&self.project_root)?;

        // Initialize git repo in project root
        std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .current_dir(&self.project_root)
            .output()?;

        std::process::Command::new("git")
            .args(&["config", "user.email", "simulation@mowis.ai"])
            .current_dir(&self.project_root)
            .output()?;

        std::process::Command::new("git")
            .args(&["config", "user.name", "Simulation Agent"])
            .current_dir(&self.project_root)
            .output()?;

        // Create initial commit
        std::process::Command::new("git")
            .args(&["commit", "--allow-empty", "-m", "initial"])
            .current_dir(&self.project_root)
            .output()?;

        log::info!("Layer 1: Generating mock task graph...");

        // Generate mock task graph with dependencies
        let mut tasks = Vec::new();
        let mut sandbox_hints = HashMap::new();

        for i in 0..self.tasks {
            let mut deps = Vec::new();

            // Create linear dependency chain for first 5 tasks
            if i > 0 && i < 5 {
                deps.push(format!("t{}", i - 1));
            }

            // Assign to sandbox round-robin
            let sandbox_index = i % self.sandboxes;
            let sandbox_name = format!("sandbox_{}", sandbox_index);

            tasks.push(Task {
                id: format!("t{}", i),
                description: format!("Mock task {}", i + 1),
                deps,
                hint: Some(sandbox_name.clone()),
            });

            sandbox_hints.insert(format!("t{}", i), sandbox_name);
        }

        let task_graph = TaskGraph { tasks };

        log::info!("  → Generated {} tasks", task_graph.tasks.len());

        log::info!("\nLayer 2: Creating sandbox topology...");

        let topology = TopologyManager::new(
            self.project_root.clone(),
            self.socket.clone(),
        )?;

        let mut sandbox_configs = Vec::new();

        for i in 0..self.sandboxes {
            let config = SandboxConfig {
                name: format!("sandbox_{}", i),
                scope: "/".to_string(),
                tools: vec![
                    "read_file".to_string(),
                    "write_file".to_string(),
                    "run_command".to_string(),
                ],
                max_agents: self.max_agents / self.sandboxes,
            };

            topology.create_sandbox_layer(&config).await?;
            sandbox_configs.push(config);
        }

        log::info!("\nLayer 3: Initializing scheduler...");

        let scheduler = Arc::new(Scheduler::new(task_graph.clone(), sandbox_hints)?);

        log::info!("  → Scheduler ready");

        log::info!("\nLayer 4: Executing tasks with mock agents...");

        let mock_executor = Arc::new(MockAgentExecutor::new(
            self.failure_rate,
            self.tool_delay,
            self.verbose,
            self.project_root.join(".checkpoints"),
            self.socket.clone(),
        )?);

        let topology = Arc::new(topology);
        let task_index = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let total_tasks = self.tasks;

        let mut handles = Vec::new();

        log::info!("  → Spawning {} mock agent workers...", self.max_agents);

        for worker_id in 0..self.max_agents {
            let scheduler_clone = scheduler.clone();
            let topology_clone = topology.clone();
            let executor_clone = mock_executor.clone();
            let task_index_clone = task_index.clone();
            let sandbox_configs_clone = sandbox_configs.clone();

            let handle = tokio::spawn(async move {
                loop {
                    let mut task_found = false;

                    for sandbox in &sandbox_configs_clone {
                        if let Some(ready_task_id) = scheduler_clone.get_ready_task(&sandbox.name).await {
                            task_found = true;

                            let idx = task_index_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                            let agent = match topology_clone
                                .create_agent_layer(&sandbox.name, Some(ready_task_id.clone()))
                                .await {
                                    Ok(a) => a,
                                    Err(e) => {
                                        log::warn!("[Worker {}] Failed to create agent: {}", worker_id, e);
                                        continue;
                                    }
                                };

                            if let Err(e) = scheduler_clone.mark_task_started(ready_task_id.clone(), agent.clone()).await {
                                log::warn!("[Worker {}] Failed to mark task started: {}", worker_id, e);
                                continue;
                            }

                            let result = match executor_clone
                                .execute_task(&agent, idx, total_tasks, &topology_clone)
                                .await {
                                    Ok(r) => r,
                                    Err(e) => {
                                        log::warn!("[Worker {}] Task execution failed: {}", worker_id, e);
                                        continue;
                                    }
                                };

                            // Apply diff to sandbox
                            if result.success {
                                if let Some(ref diff) = result.git_diff {
                                    if !diff.is_empty() {
                                        if let Err(e) = topology_clone.apply_diff_to_sandbox(&sandbox.name, diff).await {
                                            log::warn!("[Worker {}] Failed to apply diff: {}", worker_id, e);
                                        }
                                    }
                                }
                            }

                            if let Err(e) = scheduler_clone.handle_task_completion(result.clone()).await {
                                log::warn!("[Worker {}] Failed to handle completion: {}", worker_id, e);
                            }

                            let _ = topology_clone.destroy_agent_layer(&agent.agent_id).await;

                            log::info!("    ✓ [Worker {}] Completed task {}", worker_id, idx + 1);

                            break;
                        }
                    }

                    if !task_found {
                        let stats = scheduler_clone.get_stats().await;
                        if stats.completed + stats.failed >= stats.total_tasks {
                            break;
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    }
                }
            });

            handles.push(handle);
        }

        log::info!("  → Waiting for all tasks to complete...\n");

        for handle in handles {
            let _ = handle.await;
        }

        let stats = scheduler.get_stats().await;
        let duration = start_time.elapsed();

        log::info!("\n✅ Simulation complete!");
        log::info!("═══════════════════════════════════");
        log::info!("⏱️  Total time: {:?}", duration);
        log::info!("✅ Completed: {} tasks", stats.completed);
        log::info!("❌ Failed: {} tasks", stats.failed);
        log::info!(
            "📊 Success rate: {:.1}%",
            (stats.completed as f64 / stats.total_tasks as f64) * 100.0
        );
        log::info!("═══════════════════════════════════");

        if stats.failed > 0 {
            log::info!("\nFailed tasks:");
            for (task_id, error) in scheduler.get_failed_tasks().await {
                log::info!("  - {}: {}", task_id, error);
            }
        }

        // Layer 6: Verification (optional — enabled via --verify)
        if self.verify {
            log::info!("\nLayer 6: Verifying sandbox results...");

            let verification_planner = SimulatedVerificationPlanner::new(3); // 3 max rounds
            let verification = SimulatedVerificationLoop::new(
                verification_planner,
                mock_executor.clone(),
                topology.clone(),
                self.verify_failure_rate,
            );

            for sandbox in &sandbox_configs {
                let merged_diff = format!("Mock diff for sandbox {}", sandbox.name);

                let result = verification
                    .verify_sandbox(&sandbox.name, &merged_diff, &task_graph.tasks)
                    .await?;

                log::info!(
                    "  → Sandbox {}: {:?} ({} passed, {} failed, {} round(s))",
                    sandbox.name,
                    result.status,
                    result.passed_tests.len(),
                    result.failed_tests.len(),
                    result.rounds_completed
                );
            }
        }

        // Cleanup
        log::info!("\n🧹 Cleaning up sandboxes...");
        for sandbox in &sandbox_configs {
            let _ = topology.destroy_sandbox_layer(&sandbox.name).await;
        }

        Ok(())
    }
}
