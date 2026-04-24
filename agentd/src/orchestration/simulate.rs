//! Simulation command — full 7-layer orchestration at $0 cost
//!
//! Uses mock agents (no LLM calls) to drive the EXACT same pipeline used in
//! production.  Every layer runs with real code; only the "call Gemini" part is
//! replaced with deterministic file writes so you can iterate freely.
//!
//! Layer map
//! ─────────
//! 0  Project setup   — create project dir, git init
//! 1  Planner         — generate mock task-graph (no LLM)
//! 2  Topology        — create sandbox layers via agentd socket
//! 3  Scheduler       — event-driven dep-counter dispatch
//! 4  Agent execution — mock agents write real files via socket
//! 5  Parallel merge  — tree-pattern merge of per-sandbox agent diffs
//! 6  Verification    — optional, driven by mock verify agents
//! 7  Final output    — write merged result to --output-dir on disk

use super::merge_worker::ParallelMergeCoordinator;
use super::mock_agent::MockAgentExecutor;
use super::sandbox_topology::TopologyManager;
use super::scheduler::Scheduler;
use super::verification::{VerificationFunction, VerificationPlan, VerificationResult};
use agentd_protocol::{AgentResult, SandboxConfig, SandboxName, Task, TaskGraph, TaskId};
use anyhow::{Context, Result};
use clap::Parser;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

// ─────────────────────────────────────────────────────────────────────────────
// SimulatedVerificationPlanner
// ─────────────────────────────────────────────────────────────────────────────

pub struct SimulatedVerificationPlanner {
    max_rounds: usize,
    pub max_test_execution_time: u64,
}

impl SimulatedVerificationPlanner {
    pub fn new(max_rounds: usize) -> Self {
        Self { max_rounds, max_test_execution_time: 10 }
    }

    pub async fn generate_test_tasks(
        &self,
        sandbox_name: &SandboxName,
        _merged_diff: &str,
        _original_tasks: &[Task],
    ) -> Result<VerificationPlan> {
        log::info!("[L6] Generating test tasks for sandbox: {}", sandbox_name);

        let vfs = vec![
            VerificationFunction {
                id: format!("test-{}-file-exists", sandbox_name),
                description: format!("Verify files exist in sandbox {}", sandbox_name),
                command: "ls -la /workspace".to_string(),
                expected_schema: None,
                assertion: None,
                deps: vec![],
            },
            VerificationFunction {
                id: format!("test-{}-content-valid", sandbox_name),
                description: format!("Verify JS files in sandbox {}", sandbox_name),
                command: "find /workspace -name '*.js' -type f | head -5".to_string(),
                expected_schema: None,
                assertion: None,
                deps: vec![format!("test-{}-file-exists", sandbox_name)],
            },
        ];

        log::info!("[L6] Generated {} test tasks", vfs.len());
        Ok(VerificationPlan { sandbox_name: sandbox_name.clone(), vfs })
    }

    pub async fn generate_fix_tasks(
        &self,
        failed_test_id: &TaskId,
        _test_description: &str,
        failure_output: &str,
    ) -> Result<Vec<Task>> {
        log::info!("[L6] Generating fix tasks for failed test: {}", failed_test_id);
        Ok(vec![Task {
            id: format!("fix-{}", failed_test_id),
            description: format!(
                "Apply fix for test failure: {}",
                failure_output.lines().next().unwrap_or("unknown error")
                    .chars().take(80).collect::<String>()
            ),
            deps: vec![],
            hint: None,
        }])
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SimulatedVerificationLoop
// ─────────────────────────────────────────────────────────────────────────────

pub struct SimulatedVerificationLoop {
    planner: SimulatedVerificationPlanner,
    executor: Arc<MockAgentExecutor>,
    topology: Arc<TopologyManager>,
    verify_failure_rate: f64,
}

impl SimulatedVerificationLoop {
    pub fn new(
        planner: SimulatedVerificationPlanner,
        executor: Arc<MockAgentExecutor>,
        topology: Arc<TopologyManager>,
        verify_failure_rate: f64,
    ) -> Self {
        Self { planner, executor, topology, verify_failure_rate }
    }

    pub async fn verify_sandbox(
        &self,
        sandbox_name: &SandboxName,
        merged_diff: &str,
        original_tasks: &[Task],
    ) -> Result<VerificationResult> {
        let max_rounds = self.planner.max_rounds;
        let mut passed_tests: Vec<TaskId> = Vec::new();
        let mut failed_tests: Vec<TaskId> = Vec::new();
        let mut rounds_completed = 0;

        log::info!(
            "[L6] Starting verification for sandbox: {} (diff {} bytes, {} tasks)",
            sandbox_name, merged_diff.len(), original_tasks.len()
        );

        for round in 0..max_rounds {
            rounds_completed = round + 1;
            passed_tests.clear();
            failed_tests.clear();

            log::info!("[L6] Round {}/{}", round + 1, max_rounds);

            let plan = self.planner
                .generate_test_tasks(sandbox_name, merged_diff, original_tasks)
                .await?;

            let mut round_failures: Vec<(TaskId, String, String)> = Vec::new();

            for vf in &plan.vfs {
                log::info!("[L6]   Running: {} ({})", vf.id, vf.command);

                let agent = match self.topology
                    .create_agent_layer(sandbox_name, Some(vf.id.clone()))
                    .await
                {
                    Ok(a) => a,
                    Err(e) => {
                        log::error!("[L6] Failed to create agent for {}: {}", vf.id, e);
                        failed_tests.push(vf.id.clone());
                        round_failures.push((vf.id.clone(), vf.description.clone(), e.to_string()));
                        continue;
                    }
                };

                let result = match tokio::time::timeout(
                    std::time::Duration::from_secs(self.planner.max_test_execution_time),
                    self.executor.execute_verification_task(
                        &agent, &vf.description, &self.topology, self.verify_failure_rate,
                    ),
                ).await {
                    Ok(r) => r,
                    Err(_) => Err(anyhow::anyhow!("VF timeout after {}s", self.planner.max_test_execution_time)),
                };

                let _ = self.topology.destroy_agent_layer(&agent.agent_id).await;

                match result {
                    Ok(r) if r.success => {
                        log::info!("[L6]   ✓ {} — PASS", vf.id);
                        passed_tests.push(vf.id.clone());
                    }
                    Ok(r) => {
                        let err = r.error.unwrap_or_else(|| "VF failed".to_string());
                        log::info!("[L6]   ✗ {} — FAIL: {}", vf.id, err);
                        failed_tests.push(vf.id.clone());
                        round_failures.push((vf.id.clone(), vf.description.clone(), err));
                    }
                    Err(e) => {
                        log::info!("[L6]   ✗ {} — ERROR: {}", vf.id, e);
                        failed_tests.push(vf.id.clone());
                        round_failures.push((vf.id.clone(), vf.description.clone(), e.to_string()));
                    }
                }
            }

            if round_failures.is_empty() {
                log::info!("[L6]   ✓ All {} tests passed in round {}", passed_tests.len(), round + 1);
                break;
            }

            log::info!("[L6]   ⚠ {} test(s) failed in round {}", round_failures.len(), round + 1);

            if round < max_rounds - 1 {
                for (test_id, desc, error) in &round_failures {
                    let fix_tasks = self.planner
                        .generate_fix_tasks(test_id, desc, error)
                        .await
                        .unwrap_or_default();

                    for fix_task in fix_tasks {
                        let agent = match self.topology
                            .create_agent_layer(sandbox_name, Some(fix_task.id.clone()))
                            .await
                        {
                            Ok(a) => a,
                            Err(e) => {
                                log::warn!("[L6] Failed to create fix agent for {}: {}", fix_task.id, e);
                                continue;
                            }
                        };

                        match tokio::time::timeout(
                            std::time::Duration::from_secs(self.planner.max_test_execution_time),
                            self.executor.execute_fix_task(&agent, &fix_task.description, &self.topology),
                        ).await {
                            Ok(Ok(r)) if r.success => {
                                log::info!("[L6]   ✓ Fix {} succeeded", fix_task.id);
                            }
                            Ok(Ok(r)) => {
                                log::warn!("[L6]   ✗ Fix {} failed: {}", fix_task.id, r.error.unwrap_or_default());
                            }
                            Ok(Err(e)) => {
                                log::warn!("[L6]   ✗ Fix {} error: {}", fix_task.id, e);
                            }
                            Err(_) => {
                                log::warn!("[L6]   ✗ Fix {} timed out", fix_task.id);
                            }
                        }

                        let _ = self.topology.destroy_agent_layer(&agent.agent_id).await;
                    }
                }
            }
        }

        let status = super::verification::determine_status(
            &failed_tests, &passed_tests, rounds_completed, max_rounds
        );

        Ok(VerificationResult {
            status,
            passed_tests,
            failed_tests,
            rounds_completed,
            updated_diff: Some(merged_diff.to_string()),
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SimulateCommand
// ─────────────────────────────────────────────────────────────────────────────

/// Run full 7-layer orchestration simulation with mock agents ($0 cost)
#[derive(Parser, Debug)]
pub struct SimulateCommand {
    /// Path to agentd socket
    #[arg(long, default_value = "/tmp/agentd.sock")]
    pub socket: String,

    /// Number of tasks to simulate
    #[arg(long, default_value = "4")]
    pub tasks: usize,

    /// Number of sandboxes to create
    #[arg(long, default_value = "1")]
    pub sandboxes: usize,

    /// Maximum concurrent agents
    #[arg(long, default_value = "4")]
    pub max_agents: usize,

    /// Random failure rate (0.0 - 1.0)
    #[arg(long, default_value = "0.0")]
    pub failure_rate: f64,

    /// Delay per tool call in milliseconds
    #[arg(long, default_value = "50")]
    pub tool_delay: u64,

    /// Project root — the mock workspace that agents read/write.
    /// A temporary git repo is created here if it doesn't exist.
    #[arg(long, default_value = "/tmp/mock-project")]
    pub project_root: PathBuf,

    /// Where to write the final merged output (Layer 7).
    /// All agent-written files are applied here.
    /// Defaults to <project_root>/mowisai-output/.
    #[arg(long)]
    pub output_dir: Option<PathBuf>,

    /// Verbose output (shows every tool call and diff)
    #[arg(long, short, default_value = "false")]
    pub verbose: bool,

    /// Enable Layer 6 verification
    #[arg(long, default_value = "false")]
    pub verify: bool,

    /// Verification failure rate (0.0 - 1.0)
    #[arg(long, default_value = "0.0")]
    pub verify_failure_rate: f64,

    /// Skip Layer 7 — don't write output to disk
    #[arg(long, default_value = "false")]
    pub no_save: bool,
}

impl SimulateCommand {
    pub async fn run(&self) -> Result<()> {
        let start_time = std::time::Instant::now();
        let output_dir = self.output_dir.clone().unwrap_or_else(|| {
            self.project_root.join("mowisai-output")
        });

        // ── Banner ────────────────────────────────────────────────────────────
        log::info!("🚀 MowisAI Simulation — full 7-layer pipeline");
        log::info!("═══════════════════════════════════════════════");
        log::info!("  tasks:        {}", self.tasks);
        log::info!("  sandboxes:    {}", self.sandboxes);
        log::info!("  max agents:   {}", self.max_agents);
        log::info!("  failure rate: {:.0}%", self.failure_rate * 100.0);
        log::info!("  tool delay:   {}ms", self.tool_delay);
        log::info!("  project root: {}", self.project_root.display());
        log::info!("  output dir:   {}", output_dir.display());
        if self.verify {
            log::info!("  verification: ON (failure rate {:.0}%)", self.verify_failure_rate * 100.0);
        }
        log::info!("═══════════════════════════════════════════════\n");

        // ── Layer 0: Project setup ────────────────────────────────────────────
        log::info!("Layer 0: Setting up project workspace...");
        self.setup_project_workspace()?;
        log::info!("  ✓ Project workspace ready at {}", self.project_root.display());

        // ── Layer 1: Task graph ───────────────────────────────────────────────
        log::info!("\nLayer 1: Building mock task graph...");
        let (task_graph, sandbox_hints) = self.build_task_graph();
        log::info!("  ✓ {} tasks across {} sandbox(es)", task_graph.tasks.len(), self.sandboxes);
        if self.verbose {
            for t in &task_graph.tasks {
                log::info!(
                    "    task {}: \"{}\" deps={:?} hint={:?}",
                    t.id, t.description, t.deps, t.hint
                );
            }
        }

        // ── Layer 2: Sandbox topology ─────────────────────────────────────────
        log::info!("\nLayer 2: Creating sandbox topology...");
        let topology = Arc::new(
            TopologyManager::new(self.project_root.clone(), self.socket.clone())?
        );
        let sandbox_configs = self.create_sandboxes(&topology).await?;
        log::info!("  ✓ {} sandbox(es) created", sandbox_configs.len());

        // ── Layer 3: Scheduler ────────────────────────────────────────────────
        log::info!("\nLayer 3: Initializing event-driven scheduler...");
        let scheduler = Arc::new(
            Scheduler::new(task_graph.clone(), sandbox_hints)?
        );
        log::info!("  ✓ Scheduler ready ({} tasks queued)", task_graph.tasks.len());

        // ── Layer 4: Agent execution ──────────────────────────────────────────
        log::info!("\nLayer 4: Executing tasks with mock agents...");

        let mock_executor = Arc::new(MockAgentExecutor::new(
            self.failure_rate,
            self.tool_delay,
            self.verbose,
            self.project_root.join(".checkpoints"),
            self.socket.clone(),
        )?);

        // sandbox_name → list of AgentResult (includes git_diff)
        let agent_results: Arc<tokio::sync::RwLock<HashMap<SandboxName, Vec<AgentResult>>>> =
            Arc::new(tokio::sync::RwLock::new(HashMap::new()));

        let task_index = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let total_tasks = self.tasks;
        let mut handles = Vec::new();

        for worker_id in 0..self.max_agents {
            let scheduler_clone   = scheduler.clone();
            let topology_clone    = topology.clone();
            let executor_clone    = mock_executor.clone();
            let results_clone     = agent_results.clone();
            let task_index_clone  = task_index.clone();
            let sandbox_configs_c = sandbox_configs.clone();

            handles.push(tokio::spawn(async move {
                loop {
                    let mut found = false;

                    for sandbox in &sandbox_configs_c {
                        if let Some(task_id) = scheduler_clone.get_ready_task(&sandbox.name).await {
                            found = true;
                            let idx = task_index_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                            let agent = match topology_clone
                                .create_agent_layer(&sandbox.name, Some(task_id.clone()))
                                .await
                            {
                                Ok(a) => a,
                                Err(e) => {
                                    log::warn!("[L4][W{}] create_agent_layer failed: {}", worker_id, e);
                                    continue;
                                }
                            };

                            if let Err(e) = scheduler_clone.mark_task_started(task_id.clone(), agent.clone()).await {
                                log::warn!("[L4][W{}] mark_task_started failed: {}", worker_id, e);
                                continue;
                            }

                            let result = match executor_clone
                                .execute_task(&agent, idx, total_tasks, &topology_clone)
                                .await
                            {
                                Ok(r) => r,
                                Err(e) => {
                                    log::warn!("[L4][W{}] execute_task failed: {}", worker_id, e);
                                    continue;
                                }
                            };

                            // Apply diff to sandbox layer so later agents and
                            // verification see accumulated changes
                            if result.success {
                                if let Some(ref diff) = result.git_diff {
                                    if !diff.is_empty() {
                                        if let Err(e) = topology_clone
                                            .apply_diff_to_sandbox(&sandbox.name, diff)
                                            .await
                                        {
                                            log::warn!("[L4][W{}] apply_diff_to_sandbox failed: {}", worker_id, e);
                                        }
                                    }
                                }
                            }

                            if let Err(e) = scheduler_clone.handle_task_completion(result.clone()).await {
                                log::warn!("[L4][W{}] handle_task_completion failed: {}", worker_id, e);
                            }

                            // Collect result for Layer 5
                            {
                                let mut map = results_clone.write().await;
                                map.entry(sandbox.name.clone())
                                    .or_insert_with(Vec::new)
                                    .push(result);
                            }

                            let _ = topology_clone.destroy_agent_layer(&agent.agent_id).await;
                            log::info!("[L4][W{}] ✓ Task {} done ({}/{})", worker_id, task_id, idx + 1, total_tasks);
                            break;
                        }
                    }

                    if !found {
                        let stats = scheduler_clone.get_stats().await;
                        if stats.completed + stats.failed >= stats.total_tasks {
                            break;
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    }
                }
            }));
        }

        for h in handles { let _ = h.await; }

        let stats = scheduler.get_stats().await;
        log::info!(
            "\n  ✓ Layer 4 complete — {} completed, {} failed (of {})",
            stats.completed, stats.failed, stats.total_tasks
        );

        let agent_results_map = agent_results.read().await.clone();

        // ── Layer 5: Parallel merge ───────────────────────────────────────────
        log::info!("\nLayer 5: Merging agent diffs (tree-pattern merge)...");

        let merge_work_dir = std::env::temp_dir().join("mowisai-sim-merge");
        let mut per_sandbox_merged: HashMap<SandboxName, String> = HashMap::new();

        for (sandbox_name, results) in &agent_results_map {
            let diffs: Vec<String> = results
                .iter()
                .filter(|r| r.success)
                .filter_map(|r| r.git_diff.clone())
                .filter(|d| !d.is_empty())
                .collect();

            log::info!(
                "  [L5] Sandbox {}: {} successful diffs to merge",
                sandbox_name,
                diffs.len()
            );

            if diffs.is_empty() {
                log::warn!("  [L5] No diffs for sandbox {} — skipping", sandbox_name);
                per_sandbox_merged.insert(sandbox_name.clone(), String::new());
                continue;
            }

            if diffs.len() == 1 {
                log::info!("  [L5] Single diff for sandbox {} — no merge needed", sandbox_name);
                per_sandbox_merged.insert(sandbox_name.clone(), diffs[0].clone());
                continue;
            }

            let coordinator = ParallelMergeCoordinator::new(
                // project_id empty string — conflict repair uses LLM only on actual
                // git-apply failure; in simulation with mock diffs that shouldn't happen.
                "".to_string(),
                merge_work_dir.clone(),
                self.project_root.clone(),
            )?;

            match coordinator.merge_diffs(diffs).await {
                Ok(merge_result) => {
                    log::info!(
                        "  [L5] ✓ Sandbox {}: merged {} bytes, {} conflicts resolved, {} unresolved",
                        sandbox_name,
                        merge_result.merged_diff.len(),
                        merge_result.conflicts_resolved,
                        merge_result.unresolved_conflicts.len()
                    );
                    if !merge_result.unresolved_conflicts.is_empty() {
                        for c in &merge_result.unresolved_conflicts {
                            log::warn!("  [L5]   unresolved: {}", c);
                        }
                    }
                    per_sandbox_merged.insert(sandbox_name.clone(), merge_result.merged_diff);
                }
                Err(e) => {
                    log::warn!("  [L5] Merge failed for sandbox {}: {} — using concatenated fallback", sandbox_name, e);
                    // Fallback: concatenate all diffs
                    let fallback: String = results.iter()
                        .filter(|r| r.success)
                        .filter_map(|r| r.git_diff.clone())
                        .collect::<Vec<_>>()
                        .join("\n");
                    per_sandbox_merged.insert(sandbox_name.clone(), fallback);
                }
            }
        }

        // ── Layer 6: Verification (optional) ─────────────────────────────────
        if self.verify {
            log::info!("\nLayer 6: Verifying sandbox results...");

            let planner = SimulatedVerificationPlanner::new(3);
            let verification = SimulatedVerificationLoop::new(
                planner,
                mock_executor.clone(),
                topology.clone(),
                self.verify_failure_rate,
            );

            for (sandbox_name, merged_diff) in &per_sandbox_merged {
                if merged_diff.is_empty() {
                    log::info!("  [L6] Sandbox {} — no diff, skipping verification", sandbox_name);
                    continue;
                }

                let original_tasks: Vec<Task> = task_graph.tasks.iter().cloned().collect();

                match verification.verify_sandbox(sandbox_name, merged_diff, &original_tasks).await {
                    Ok(vr) => {
                        log::info!(
                            "  [L6] ✓ Sandbox {}: {:?} — {} passed, {} failed, {} round(s)",
                            sandbox_name, vr.status,
                            vr.passed_tests.len(), vr.failed_tests.len(),
                            vr.rounds_completed
                        );
                    }
                    Err(e) => {
                        log::warn!("  [L6] Sandbox {} verification error: {}", sandbox_name, e);
                    }
                }
            }
        } else {
            log::info!("\nLayer 6: Skipped (use --verify to enable)");
        }

        // ── Layer 7: Final output ─────────────────────────────────────────────
        if self.no_save {
            log::info!("\nLayer 7: Skipped (--no-save)");
        } else {
            log::info!("\nLayer 7: Writing final output to disk...");

            // Cross-sandbox merge: concatenate all sandbox diffs into one final diff.
            // In real multi-sandbox mode a proper cross-sandbox merge would happen here;
            // for simulation concatenation is fine since mock files don't conflict.
            let final_diff: String = per_sandbox_merged.values()
                .filter(|d| !d.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");

            let total_diff_bytes = final_diff.len();

            if final_diff.is_empty() {
                log::warn!("  [L7] No diffs collected — nothing to write");
                log::warn!("  [L7] This usually means the agentd socket wasn't reachable");
                log::warn!("  [L7] Make sure agentd socket is running: sudo ./target/debug/agentd socket --path {}", self.socket);
            } else {
                log::info!(
                    "  [L7] Final diff: {} bytes across {} sandbox(es)",
                    total_diff_bytes,
                    per_sandbox_merged.len()
                );

                self.write_output(&output_dir, &final_diff, &per_sandbox_merged)?;

                log::info!("  [L7] ✅ Output written to {}", output_dir.display());
                log::info!("  [L7]    Files: see {}/", output_dir.display());
                log::info!("  [L7]    Patch: {}/mowisai_output.patch", output_dir.display());
            }
        }

        // ── Summary ───────────────────────────────────────────────────────────
        let duration = start_time.elapsed();

        log::info!("\n✅ Simulation complete!");
        log::info!("═══════════════════════════════════════════════");
        log::info!("  ⏱  Total time:    {:?}", duration);
        log::info!("  ✅ Completed:     {} / {} tasks", stats.completed, stats.total_tasks);
        log::info!("  ❌ Failed:        {} tasks", stats.failed);
        log::info!(
            "  📊 Success rate:  {:.1}%",
            if stats.total_tasks > 0 {
                (stats.completed as f64 / stats.total_tasks as f64) * 100.0
            } else { 0.0 }
        );
        log::info!(
            "  📝 Diff size:     {} bytes across {} sandbox(es)",
            per_sandbox_merged.values().map(|d| d.len()).sum::<usize>(),
            per_sandbox_merged.len()
        );
        if !self.no_save {
            log::info!("  💾 Output dir:    {}", output_dir.display());
        }
        log::info!("═══════════════════════════════════════════════");

        if stats.failed > 0 {
            log::info!("\nFailed tasks:");
            for (task_id, error) in scheduler.get_failed_tasks().await {
                log::info!("  - {}: {}", task_id, error);
            }
        }

        // Machine-readable output (grep-friendly for CI)
        println!("SIMULATE_TOTAL_MS={}", duration.as_millis());
        println!("SIMULATE_COMPLETED={}", stats.completed);
        println!("SIMULATE_FAILED={}", stats.failed);
        println!("SIMULATE_DIFF_BYTES={}", per_sandbox_merged.values().map(|d| d.len()).sum::<usize>());
        if !self.no_save {
            println!("SIMULATE_OUTPUT_DIR={}", output_dir.display());
        }

        // ── Cleanup ───────────────────────────────────────────────────────────
        log::info!("\n🧹 Cleaning up sandboxes...");
        for sandbox in &sandbox_configs {
            let _ = topology.destroy_sandbox_layer(&sandbox.name).await;
        }
        let _ = std::fs::remove_dir_all(&merge_work_dir);

        Ok(())
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Layer 0: Create project workspace with a git repo and initial commit.
    fn setup_project_workspace(&self) -> Result<()> {
        std::fs::create_dir_all(&self.project_root)
            .context("Failed to create project root")?;

        // git init (idempotent)
        let _ = std::process::Command::new("git")
            .arg("init").arg("-q")
            .current_dir(&self.project_root)
            .output();

        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "simulation@mowis.ai"])
            .current_dir(&self.project_root)
            .output();

        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "Simulation Agent"])
            .current_dir(&self.project_root)
            .output();

        // Create initial commit only if HEAD doesn't exist yet
        let head_exists = std::process::Command::new("git")
            .args(["rev-parse", "--verify", "HEAD"])
            .current_dir(&self.project_root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if !head_exists {
            // Write a README so the commit has content
            std::fs::write(
                self.project_root.join("README.md"),
                "# Mock project\nGenerated by MowisAI simulation.\n",
            )?;

            let _ = std::process::Command::new("git")
                .args(["add", "-A"])
                .current_dir(&self.project_root)
                .output();

            let _ = std::process::Command::new("git")
                .args(["commit", "-m", "initial"])
                .current_dir(&self.project_root)
                .output();
        }

        Ok(())
    }

    /// Layer 1: Build deterministic mock task graph.
    fn build_task_graph(&self) -> (TaskGraph, HashMap<SandboxName, SandboxName>) {
        let mut tasks = Vec::new();
        let mut sandbox_hints = HashMap::new();

        for i in 0..self.tasks {
            // Linear dep chain for first 5 tasks — stresses the dep-counter scheduler
            let deps = if i > 0 && i < 5 { vec![format!("t{}", i - 1)] } else { vec![] };
            let sandbox_name = format!("sandbox_{}", i % self.sandboxes);

            tasks.push(Task {
                id: format!("t{}", i),
                description: format!(
                    "Write module {} — implements feature set {}",
                    i, (b'A' + (i % 26) as u8) as char
                ),
                deps,
                hint: Some(sandbox_name.clone()),
            });

            sandbox_hints.insert(format!("t{}", i), sandbox_name);
        }

        (TaskGraph { tasks }, sandbox_hints)
    }

    /// Layer 2: Create sandbox configs and register them with the topology manager.
    async fn create_sandboxes(&self, topology: &TopologyManager) -> Result<Vec<SandboxConfig>> {
        let mut configs = Vec::new();

        for i in 0..self.sandboxes {
            let config = SandboxConfig {
                name: format!("sandbox_{}", i),
                scope: "/".to_string(),
                tools: vec![
                    "read_file".to_string(),
                    "write_file".to_string(),
                    "run_command".to_string(),
                ],
                max_agents: (self.max_agents / self.sandboxes).max(1),
            };

            topology.create_sandbox_layer(&config).await
                .with_context(|| format!("Failed to create sandbox_{}", i))?;

            log::info!("  → sandbox_{} created", i);
            configs.push(config);
        }

        Ok(configs)
    }

    /// Layer 7: Write the final merged diff and individual files to output_dir.
    ///
    /// Strategy:
    /// 1. Write the raw diff as a .patch file (always works)
    /// 2. Try `git apply` inside the output_dir repo (produces real files)
    /// 3. If git apply fails, write a per-sandbox breakdown for debugging
    fn write_output(
        &self,
        output_dir: &PathBuf,
        final_diff: &str,
        per_sandbox: &HashMap<SandboxName, String>,
    ) -> Result<()> {
        std::fs::create_dir_all(output_dir)
            .with_context(|| format!("Failed to create output dir {}", output_dir.display()))?;

        // Always write the raw patch file
        let patch_path = output_dir.join("mowisai_output.patch");
        std::fs::write(&patch_path, final_diff)
            .context("Failed to write mowisai_output.patch")?;
        log::info!("  [L7] Wrote patch: {}", patch_path.display());

        // Write per-sandbox patches for debugging
        let sandboxes_dir = output_dir.join("per_sandbox");
        std::fs::create_dir_all(&sandboxes_dir).ok();
        for (sandbox_name, diff) in per_sandbox {
            if !diff.is_empty() {
                let sb_patch = sandboxes_dir.join(format!("{}.patch", sandbox_name));
                let _ = std::fs::write(&sb_patch, diff);
                log::info!("  [L7]   sandbox patch: {}", sb_patch.display());
            }
        }

        // Set up git repo in output_dir so git apply works
        let has_git = output_dir.join(".git").exists();
        if !has_git {
            let _ = std::process::Command::new("git")
                .arg("init").arg("-q")
                .current_dir(output_dir)
                .output();
            let _ = std::process::Command::new("git")
                .args(["config", "user.email", "layer7@mowis.ai"])
                .current_dir(output_dir)
                .output();
            let _ = std::process::Command::new("git")
                .args(["config", "user.name", "MowisAI Layer7"])
                .current_dir(output_dir)
                .output();
            // Seed with README so there's a HEAD to diff against
            std::fs::write(
                output_dir.join("README.md"),
                "# MowisAI Output\nGenerated by simulation.\n",
            ).ok();
            let _ = std::process::Command::new("git")
                .args(["add", "-A"])
                .current_dir(output_dir)
                .output();
            let _ = std::process::Command::new("git")
                .args(["commit", "-m", "init"])
                .current_dir(output_dir)
                .output();
        }

        // Try git apply
        let apply = self.try_git_apply_to_dir(output_dir, final_diff);
        if apply {
            log::info!("  [L7] ✅ git apply succeeded — files written to {}", output_dir.display());
            // List written files
            if let Ok(out) = std::process::Command::new("git")
                .args(["diff", "--name-only", "HEAD"])
                .current_dir(output_dir)
                .output()
            {
                let files = String::from_utf8_lossy(&out.stdout);
                for f in files.lines().take(20) {
                    log::info!("  [L7]   + {}", f);
                }
                let count = files.lines().count();
                if count > 20 {
                    log::info!("  [L7]   ... and {} more files", count - 20);
                }
            }
        } else {
            log::warn!(
                "  [L7] git apply failed — the raw patch is still at {}",
                patch_path.display()
            );
            log::warn!("  [L7] To apply manually: cd {} && git apply mowisai_output.patch", output_dir.display());
        }

        // Write a human-readable summary
        let summary_path = output_dir.join("SIMULATION_SUMMARY.txt");
        let summary = format!(
            "MowisAI Simulation Output\n\
             =========================\n\
             Sandboxes:  {}\n\
             Diff bytes: {}\n\
             Patch file: mowisai_output.patch\n\
             \n\
             Per-sandbox patches in: per_sandbox/\n",
            per_sandbox.len(),
            final_diff.len(),
        );
        let _ = std::fs::write(&summary_path, &summary);

        Ok(())
    }

    fn try_git_apply_to_dir(&self, dir: &PathBuf, diff: &str) -> bool {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let mut child = match Command::new("git")
            .args(["apply", "--whitespace=nowarn", "--allow-empty", "-"])
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => return false,
        };

        if let Some(mut stdin) = child.stdin.take() {
            if stdin.write_all(diff.as_bytes()).is_err() {
                return false;
            }
        }

        child.wait().map(|s| s.success()).unwrap_or(false)
    }
}
