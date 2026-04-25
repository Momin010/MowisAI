//! New 7-layer orchestration system entry point

use super::scheduler::{Scheduler, SchedulerStats};

/// Events emitted by the orchestrator for real-time TUI updates.
#[derive(Debug, Clone)]
pub enum OrchestratorEvent {
    TaskStarted { worker_id: usize, task_id: String, description: String, sandbox: String },
    ToolCall { worker_id: usize, tool_name: String, args_preview: String },
    ToolResult { worker_id: usize, tool_name: String, success: bool, preview: String },
    TaskCompleted { worker_id: usize, task_id: String, success: bool, diff_size: usize },
    TaskFailed { worker_id: usize, task_id: String, error: String },
    StatsUpdate { stats: SchedulerStats },
    LayerProgress { layer: u8, message: String },
    Done,
}

use super::agent_execution::AgentExecutor;
use super::complexity_classifier::{classify_complexity, ComplexityMode};
use super::health::HealthMonitor;
use super::merge_reviewer::{AgentContribution, ConflictDetector, MergeReviewerAgent, parse_unified_diff};
use super::planner::{plan_task, plan_task_standard, scan_directory_tree_pub};
use super::sandbox_topology::TopologyManager;
use super::verification::VerificationLoop;
use agentd_protocol::{AgentResult, SandboxConfig, SandboxName, SandboxResult, Task, TaskId, VerificationStatus};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Final orchestration output
#[derive(Debug, Clone)]
pub struct FinalOutput {
    pub merged_diff: String,
    pub sandbox_results: HashMap<SandboxName, SandboxResult>,
    pub verification_status: HashMap<SandboxName, VerificationStatus>,
    pub failed_tasks: Vec<FailedTask>,
    pub known_issues: Vec<String>,
    pub summary: String,
    pub total_agents_used: usize,
    pub total_duration_secs: u64,
    pub scheduler_stats: SchedulerStats,
    pub execution_errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FailedTask {
    pub task_id: TaskId,
    pub error: String,
}

/// Main orchestrator configuration
pub struct OrchestratorConfig {
    pub project_id: String,
    pub socket_path: String,
    pub project_root: PathBuf,
    pub overlay_root: PathBuf,
    pub checkpoint_root: PathBuf,
    pub merge_work_dir: PathBuf,
    pub max_agents: usize,
    pub max_verification_rounds: usize,
    /// Optional staging directory for save-all functionality
    pub staging_dir: Option<PathBuf>,
    /// Optional channel sender for real-time TUI progress events
    pub event_tx: Option<std::sync::mpsc::Sender<OrchestratorEvent>>,
    /// Explicit mode override from CLI (`--mode simple|standard|full`).
    /// When `None`, the complexity classifier decides automatically.
    pub mode_override: Option<ComplexityMode>,
}

/// Carries an agent result alongside the human-readable task description
/// so the merge reviewer can understand intent when resolving conflicts.
#[derive(Debug, Clone)]
struct AgentWorkResult {
    result: AgentResult,
    task_description: String,
}

/// New 7-layer orchestrator
pub struct NewOrchestrator {
    config: OrchestratorConfig,
}

impl NewOrchestrator {
    pub fn new(config: OrchestratorConfig) -> Self {
        Self { config }
    }

    /// Run orchestration for a user prompt.
    ///
    /// Routing gate: scans the directory tree heuristically (~1ms, no LLM),
    /// classifies complexity, then dispatches to the appropriate mode executor:
    ///
    /// - **Simple**   â†’ `run_simple()`   â€” 1 agent, no merge, no verification
    /// - **Standard** â†’ `run_standard()` â€” constrained planner, 1 sandbox, 1 verify round
    /// - **Full**     â†’ `run_full()`     â€” full 7-layer pipeline (current behaviour)
    pub async fn run(&self, prompt: &str) -> Result<FinalOutput> {
        let start_time = std::time::Instant::now();

        // Clone sender once so we can move it into closures / submethods without
        // borrowing self for the lifetime of the closure.
        let event_tx: Option<std::sync::mpsc::Sender<OrchestratorEvent>> =
            self.config.event_tx.clone();

        macro_rules! send_event {
            ($ev:expr) => {
                if let Some(ref tx) = event_tx {
                    let _ = tx.send($ev);
                }
            };
        }

        // â”€â”€ Routing gate â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        // Scan directory tree once (reused by planner if we call Standard/Full).
        let dir_tree = scan_directory_tree_pub(&self.config.project_root)
            .unwrap_or_default();

        let mode = if let Some(ref forced) = self.config.mode_override {
            log::info!("Mode override: {}", forced);
            forced.clone()
        } else {
            let score = classify_complexity(prompt, &dir_tree);
            log::info!(
                "Complexity classifier â†’ mode={} (score={}, filesâ‰ˆ{}, domains={}, broad_scope={}, cross_service={})",
                score.mode, score.score, score.file_count, score.domain_count,
                score.broad_scope, score.cross_service
            );
            score.mode
        };

        send_event!(OrchestratorEvent::LayerProgress {
            layer: 0,
            message: format!("Routing mode: {}", mode),
        });

        match mode {
            ComplexityMode::Simple => {
                return self.run_simple(prompt, &dir_tree, start_time, event_tx).await;
            }
            ComplexityMode::Standard => {
                return self.run_standard(prompt, &dir_tree, start_time, event_tx).await;
            }
            ComplexityMode::Full => {
                // Fall through to the full pipeline below
            }
        }

        // â”€â”€ Full mode: Layer 1 â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        send_event!(OrchestratorEvent::LayerProgress { layer: 1, message: "Planning tasks...".into() });
        log::info!("Layer 1: Planning tasks (full mode)...");
        let planner_output = plan_task(prompt, &self.config.project_root, &self.config.project_id)
            .await
            .context("Fast planner failed")?;

        log::info!(
            "  â†’ Generated {} tasks across {} sandboxes",
            planner_output.task_graph.tasks.len(),
            planner_output.sandbox_topology.sandboxes.len()
        );

        // Layer 2: Overlayfs Topology
        send_event!(OrchestratorEvent::LayerProgress { layer: 2, message: "Creating sandbox topology...".into() });
        log::info!("Layer 2: Creating sandbox topology...");
        let topology = TopologyManager::new(
            self.config.project_root.clone(),
            self.config.socket_path.clone(),
        )?;

        for sandbox in &planner_output.sandbox_topology.sandboxes {
            topology.create_sandbox_layer(sandbox).await?;
            log::info!("  â†’ Created sandbox: {}", sandbox.name);
        }

        // Layer 3: Scheduler
        send_event!(OrchestratorEvent::LayerProgress { layer: 3, message: "Initializing scheduler...".into() });
        log::info!("Layer 3: Initializing scheduler...");
        let scheduler = Arc::new(
            Scheduler::new(
                planner_output.task_graph.clone(),
                planner_output.sandbox_hints.clone(),
            )?
        );

        log::info!("  â†’ Scheduler ready with {} tasks", planner_output.task_graph.tasks.len());

        // Layer 4: Agent Execution (TRUE PARALLELISM)
        send_event!(OrchestratorEvent::LayerProgress { layer: 4, message: "Executing tasks with agents...".into() });
        log::info!("Layer 4: Executing tasks with agents...");
        let agent_executor = Arc::new(AgentExecutor::new(
            self.config.project_id.clone(),
            self.config.socket_path.clone(),
            self.config.checkpoint_root.clone(),
        )?);

        let topology = Arc::new(topology);
        let sandbox_agent_results = Arc::new(RwLock::new(HashMap::<SandboxName, Vec<AgentWorkResult>>::new()));
        let agent_count = Arc::new(RwLock::new(0usize));
        let execution_errors = Arc::new(RwLock::new(Vec::<String>::new()));

        // Health monitor: 5-minute heartbeat timeout, open circuit after 5 consecutive failures
        let health_monitor = Arc::new(HealthMonitor::new(300, 5));

        // Create a pool of agent worker tasks (TRUE PARALLELISM)
        // Dynamic agent cap based on available system resources
        let system_cap = 1000; // Upper safety bound
        let max_concurrent_agents = self.config.max_agents.min(system_cap);
        log::info!("  â†’ Agent pool: {} concurrent workers (user requested: {}, system cap: {})",
            max_concurrent_agents, self.config.max_agents, system_cap);

        let mut handles = Vec::new();

        // Clone staging_dir and event_tx before spawn to avoid lifetime issues
        let staging_dir_clone: Option<PathBuf> = self.config.staging_dir.clone();
        let event_tx_clone: Option<std::sync::mpsc::Sender<OrchestratorEvent>> = self.config.event_tx.clone();

        for worker_id in 0..max_concurrent_agents {
            let scheduler_clone = scheduler.clone();
            let topology_clone = topology.clone();
            let executor_clone = agent_executor.clone();
            let results_clone = sandbox_agent_results.clone();
            let count_clone = agent_count.clone();
            let errors_clone = execution_errors.clone();
            let sandboxes = planner_output.sandbox_topology.sandboxes.clone();
            let staging_dir_for_worker = staging_dir_clone.clone();
            let event_tx_for_worker = event_tx_clone.clone();
            let health_clone = health_monitor.clone();

            let handle = tokio::spawn(async move {
                loop {
                    // Try to get a ready task from any sandbox
                    let mut task_found = false;

                    for sandbox in &sandboxes {
                        if let Some(ready_task_id) = scheduler_clone.get_ready_task(&sandbox.name).await {
                            task_found = true;

                            // Check circuit breaker before dispatching
                            if !health_clone.is_sandbox_healthy(&sandbox.name).await {
                                log::warn!("[Worker {}] Circuit open for sandbox {}, skipping", worker_id, sandbox.name);
                                continue;
                            }

                            // Find the task details
                            let task_description = scheduler_clone.get_task_description(&ready_task_id).await
                                .unwrap_or_else(|| "Unknown task".to_string());

                            // Create agent layer (wake sleeping container or create fresh)
                            let agent = match topology_clone
                                .wake_or_create_agent_layer(&sandbox.name, Some(ready_task_id.clone()))
                                .await {
                                    Ok(a) => a,
                                    Err(e) => {
                                        let error_msg = format!("[Worker {}] Failed to create agent: {}", worker_id, e);
                                        log::warn!("{}", error_msg);
                                        {
                                            let mut errors = errors_clone.write().await;
                                            errors.push(error_msg);
                                        }
                                        continue;
                                    }
                                };

                            // Increment agent count
                            {
                                let mut count = count_clone.write().await;
                                *count += 1;
                            }

                            // Send heartbeat
                            health_clone.heartbeat(&agent.agent_id).await;

                            // Notify TUI: task started
                            if let Some(ref tx) = event_tx_for_worker {
                                let _ = tx.send(OrchestratorEvent::TaskStarted {
                                    worker_id,
                                    task_id: ready_task_id.clone(),
                                    description: task_description.clone(),
                                    sandbox: sandbox.name.clone(),
                                });
                            }

                            // Mark task as started
                            if let Err(e) = scheduler_clone.mark_task_started(ready_task_id.clone(), agent.clone()).await {
                                let error_msg = format!("[Worker {}] Failed to mark task started: {}", worker_id, e);
                                log::warn!("{}", error_msg);
                                {
                                    let mut errors = errors_clone.write().await;
                                    errors.push(error_msg);
                                }
                                continue;
                            }

                            // Execute task
                            let system_prompt = format!(
                                "You are an expert software engineer implementing a production-ready system.\n\n\
                                WORKING DIRECTORY: /workspace (contains the actual project files)\n\
                                ALL file operations MUST use /workspace prefix:\n\
                                - read_file: path=/workspace/src/file.rs\n\
                                - write_file: path=/workspace/src/new_file.rs\n\
                                - run_command: cd /workspace && ... (change to /workspace first)\n\n\
                                TASK: {}\n\n\
                                RULES:\n\
                                1. ALWAYS use /workspace prefix for ALL file paths\n\
                                2. Write COMPLETE, WORKING code - not placeholders or stubs\n\
                                3. Implement FULL functionality - don't leave TODOs or empty functions\n\
                                4. Include proper error handling, logging, and documentation\n\
                                5. Write actual implementation in all files - never create empty files\n\
                                6. When writing files, include actual code content\n\
                                7. Test your code mentally before writing\n\
                                8. Use best practices for the language/framework\n\
                                9. When done, verify all files have actual content (not just placeholders)\n\n\
                                Write production-quality code that could be deployed immediately.",
                                sandbox.name
                            );

                            let result = match executor_clone
                                .execute_task(&agent, &task_description, &sandbox.tools, &system_prompt)
                                .await {
                                    Ok(r) => r,
                                    Err(e) => {
                                        let error_msg = format!("[Worker {}] Task '{}' failed: {}", worker_id, task_description, e);
                                        log::warn!("{}", error_msg);
                                        {
                                            let mut errors = errors_clone.write().await;
                                            errors.push(error_msg);
                                        }
                                        continue;
                                    }
                                };

                            // Apply diff to sandbox layer if successful
                            if result.success {
                                if let Some(ref diff) = result.git_diff {
                                    if !diff.is_empty() {
                                        if let Err(e) = topology_clone.apply_diff_to_sandbox(&sandbox.name, diff).await {
                                            log::warn!("[Worker {}] Failed to apply diff to sandbox: {}", worker_id, e);
                                        }
                                    }
                                }
                            }

                            // Notify TUI: task result
                            if let Some(ref tx) = event_tx_for_worker {
                                let diff_size = result.git_diff.as_ref().map(|d| d.len()).unwrap_or(0);
                                if result.success {
                                    let _ = tx.send(OrchestratorEvent::TaskCompleted {
                                        worker_id,
                                        task_id: ready_task_id.clone(),
                                        success: true,
                                        diff_size,
                                    });
                                } else {
                                    let _ = tx.send(OrchestratorEvent::TaskFailed {
                                        worker_id,
                                        task_id: ready_task_id.clone(),
                                        error: result.error.clone().unwrap_or_default(),
                                    });
                                }
                            }

                            // Handle completion
                            if let Err(e) = scheduler_clone.handle_task_completion(result.clone()).await {
                                log::warn!("[Worker {}] Failed to handle completion: {}", worker_id, e);
                            }

                            // Record health outcome
                            if result.success {
                                health_clone.record_success(&sandbox.name).await;
                            } else {
                                health_clone.record_failure(&sandbox.name).await;
                            }
                            health_clone.remove_agent(&agent.agent_id).await;

                            // Store result alongside its task description for the merge reviewer
                            {
                                let mut results = results_clone.write().await;
                                results
                                    .entry(sandbox.name.clone())
                                    .or_insert_with(Vec::new)
                                    .push(AgentWorkResult {
                                        result: result.clone(),
                                        task_description: task_description.clone(),
                                    });
                            }

                            // Stage workspace BEFORE destroying (for save-all functionality)
                            if let Some(ref staging_dir) = staging_dir_for_worker {
                                if let Err(e) = topology_clone.stage_agent_workspace(&agent.agent_id, staging_dir).await {
                                    log::warn!("[Worker {}] Failed to stage workspace: {}", worker_id, e);
                                }
                            }

                            // Sleep container instead of destroying (pool for reuse)
                            let _ = topology_clone.sleep_agent_layer(&agent.agent_id, &sandbox.name).await;

                            log::info!("    âœ“ [Worker {}] Completed: {}", worker_id, task_description);

                            // Break inner loop to try getting another task
                            break;
                        }
                    }

                    // If no tasks found, check if all done
                    if !task_found {
                        let stats = scheduler_clone.get_stats().await;
                        if stats.completed + stats.failed >= stats.total_tasks {
                            // All done - worker can exit
                            break;
                        }
                        // No ready tasks yet, sleep briefly
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                }
            });

            handles.push(handle);
        }

        // Periodic stats updates to TUI
        let scheduler_for_stats = scheduler.clone();
        let event_tx_for_stats = event_tx_clone.clone();
        let stats_handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                let stats = scheduler_for_stats.get_stats().await;
                if let Some(ref tx) = event_tx_for_stats {
                    let _ = tx.send(OrchestratorEvent::StatsUpdate { stats: stats.clone() });
                }
                if stats.completed + stats.failed >= stats.total_tasks && stats.total_tasks > 0 {
                    break;
                }
            }
        });

        // Wait for all workers to complete with timeout
        let worker_timeout = tokio::time::Duration::from_secs(1800); // 30 minute max
        log::info!("  â†’ Waiting for all workers to complete (max 30 minutes)...");

        let all_workers = async {
            for handle in handles {
                let _ = handle.await;
            }
        };

        match tokio::time::timeout(worker_timeout, all_workers).await {
            Ok(_) => log::info!("  â†’ All workers completed successfully"),
            Err(_) => {
                log::warn!("  âš ï¸  Workers timed out after 30 minutes");
            }
        }
        let _ = stats_handle.await;

        // Cleanup sleeping container pool
        let _ = topology.cleanup_sleeping_containers().await;

        let agent_count = *agent_count.read().await;
        let sandbox_agent_results = {
            let results = sandbox_agent_results.read().await;
            results.clone()
        };

        // Get scheduler stats
        let scheduler_stats = scheduler.get_stats().await;
        log::info!("  â†’ Completed: {}/{} tasks", scheduler_stats.completed, scheduler_stats.total_tasks);

        // Layer 5: Intelligent Merge Review (per sandbox)
        log::info!("Layer 5: Reviewing and merging agent contributions per sandbox...");
        let reviewer = MergeReviewerAgent::new(self.config.project_id.clone());
        let mut sandbox_results = HashMap::new();

        for (sandbox_name, agent_work_results) in &sandbox_agent_results {
            // Build structured AgentContribution objects from work results
            let contributions: Vec<AgentContribution> = agent_work_results
                .iter()
                .filter(|awr| awr.result.success)
                .filter(|awr| awr.result.git_diff.as_ref().map_or(false, |d| !d.is_empty()))
                .map(|awr| {
                    let raw_diff = awr.result.git_diff.clone().unwrap_or_default();
                    let file_changes = parse_unified_diff(&raw_diff);
                    AgentContribution {
                        agent_id: awr.result.task_id.clone(),
                        task_id: awr.result.task_id.clone(),
                        task_description: awr.task_description.clone(),
                        file_changes,
                        raw_diff,
                    }
                })
                .collect();

            if contributions.is_empty() {
                log::info!("  â†’ Sandbox {} has no changes", sandbox_name);
                sandbox_results.insert(
                    sandbox_name.clone(),
                    SandboxResult {
                        sandbox_name: sandbox_name.clone(),
                        success: true,
                        merged_diff: None,
                        verification_status: VerificationStatus::NotStarted,
                        timestamp: current_timestamp(),
                    },
                );
                continue;
            }

            log::info!(
                "  â†’ Reviewing {} agent contribution(s) for sandbox: {}",
                contributions.len(),
                sandbox_name
            );

            // Pre-collect raw diffs for fallback before consuming contributions
            let fallback_diff = contributions
                .iter()
                .map(|c| c.raw_diff.clone())
                .collect::<Vec<_>>()
                .join("\n");

            // Detect conflicts between contributions
            let conflicts = ConflictDetector::detect(&contributions);
            if !conflicts.is_empty() {
                log::info!("    â†’ Detected {} conflict(s)", conflicts.len());
            }

            // Review with 5-minute timeout
            let review_timeout = tokio::time::Duration::from_secs(300);
            let review_future = reviewer.review(contributions, conflicts);

            let merged_diff = match tokio::time::timeout(review_timeout, review_future).await {
                Ok(Ok(review_result)) => {
                    log::info!("    âœ“ {}", review_result.summary);
                    review_result.final_diff
                }
                Ok(Err(e)) => {
                    log::warn!(
                        "  âš ï¸  Merge review failed for {}: {}. Falling back to concatenation.",
                        sandbox_name, e
                    );
                    fallback_diff
                }
                Err(_) => {
                    log::warn!(
                        "  âš ï¸  Merge review timed out for {} after 5 minutes. Falling back to concatenation.",
                        sandbox_name
                    );
                    fallback_diff
                }
            };

            sandbox_results.insert(
                sandbox_name.clone(),
                SandboxResult {
                    sandbox_name: sandbox_name.clone(),
                    success: true,
                    merged_diff: Some(merged_diff),
                    verification_status: VerificationStatus::NotStarted,
                    timestamp: current_timestamp(),
                },
            );
        }

        // Layer 6: Verification Loop
        send_event!(OrchestratorEvent::LayerProgress { layer: 6, message: "Verifying sandbox results...".into() });
        log::info!("Layer 6: Verifying sandbox results...");

        let verification_loop = VerificationLoop::new(
            self.config.project_id.clone(),
            self.config.max_verification_rounds,
        );

        let mut verification_status = HashMap::new();
        let known_issues = Vec::new();

        for (sandbox_name, sandbox_result) in &mut sandbox_results {
            if let Some(ref diff) = sandbox_result.merged_diff.clone() {
                if !diff.is_empty() {
                    let original_tasks: Vec<agentd_protocol::Task> = planner_output
                        .task_graph
                        .tasks
                        .iter()
                        .filter(|t| {
                            planner_output
                                .sandbox_hints
                                .get(&t.id)
                                .map_or(false, |s| s == sandbox_name)
                        })
                        .cloned()
                        .collect();

                    // Layer 6 currently runs tests and fix attempts sequentially
                    // inside each sandbox. The outer timeout must be long enough
                    // to allow all tests to complete across all verification rounds.
                    // Formula: max_rounds * (tests_per_round * test_timeout + fix_time)
                    // With 3 rounds, 7 tests, 180s test timeout, ~60s fix time: 3 * (7*180 + 60) = 3960s
                    let verify_timeout = tokio::time::Duration::from_secs(
                        (self.config.max_verification_rounds.max(1) as u64) * 30 * 60
                    );
                    let verify_future = verification_loop.verify_sandbox(
                        sandbox_name,
                        diff,
                        &original_tasks,
                        &topology,
                        &agent_executor,
                    );
                    match tokio::time::timeout(verify_timeout, verify_future).await {
                        Ok(Ok(vr)) => {
                            log::info!(
                                "  âœ“ {} â€” {:?} ({} passed, {} failed, {} rounds)",
                                sandbox_name,
                                vr.status,
                                vr.passed_tests.len(),
                                vr.failed_tests.len(),
                                vr.rounds_completed
                            );
                            if let Some(updated_diff) = vr
                                .updated_diff
                                .as_ref()
                                .filter(|updated| !updated.is_empty())
                            {
                                sandbox_result.merged_diff = Some(updated_diff.clone());
                            }
                            sandbox_result.verification_status = vr.status.clone();
                            verification_status.insert(sandbox_name.clone(), vr.status);
                        }
                        Ok(Err(e)) => {
                            log::warn!("  âš  Verification failed for {}: {}", sandbox_name, e);
                            sandbox_result.verification_status = VerificationStatus::Failed;
                            verification_status.insert(sandbox_name.clone(), VerificationStatus::Failed);
                        }
                        Err(_) => {
                            log::warn!(
                                "  âš  Verification timed out for {} after {} minutes, skipping",
                                sandbox_name,
                                verify_timeout.as_secs() / 60
                            );
                            sandbox_result.verification_status = VerificationStatus::NotStarted;
                            verification_status.insert(sandbox_name.clone(), VerificationStatus::NotStarted);
                        }
                    }
                } else {
                    sandbox_result.verification_status = VerificationStatus::NotStarted;
                    verification_status.insert(sandbox_name.clone(), VerificationStatus::NotStarted);
                }
            } else {
                sandbox_result.verification_status = VerificationStatus::NotStarted;
                verification_status.insert(sandbox_name.clone(), VerificationStatus::NotStarted);
            }
        }

        // Layer 7: Cross-Sandbox Intelligent Merge
        log::info!("Layer 7: Final cross-sandbox intelligent merge...");
        let sandbox_diffs: Vec<(SandboxName, String)> = sandbox_results
            .iter()
            .filter_map(|(name, r)| {
                r.merged_diff
                    .as_ref()
                    .filter(|d| !d.is_empty())
                    .map(|d| (name.clone(), d.clone()))
            })
            .collect();

        let final_merge = if sandbox_diffs.len() > 1 {
            log::info!("  â†’ Cross-sandbox review: {} sandbox diff(s)", sandbox_diffs.len());

            // Treat each sandbox's merged diff as an agent contribution
            let cross_contributions: Vec<AgentContribution> = sandbox_diffs
                .iter()
                .map(|(sandbox_name, diff)| {
                    let file_changes = parse_unified_diff(diff);
                    AgentContribution {
                        agent_id: sandbox_name.clone(),
                        task_id: sandbox_name.clone(),
                        task_description: format!("Merged output from sandbox '{}'", sandbox_name),
                        file_changes,
                        raw_diff: diff.clone(),
                    }
                })
                .collect();

            let fallback_diff = sandbox_diffs
                .iter()
                .map(|(_, d)| d.clone())
                .collect::<Vec<_>>()
                .join("\n");

            let cross_conflicts = ConflictDetector::detect(&cross_contributions);
            if !cross_conflicts.is_empty() {
                log::info!("  â†’ Detected {} cross-sandbox conflict(s)", cross_conflicts.len());
            }

            let merge_timeout = tokio::time::Duration::from_secs(300);
            let review_future = reviewer.review(cross_contributions, cross_conflicts);

            match tokio::time::timeout(merge_timeout, review_future).await {
                Ok(Ok(review_result)) => {
                    log::info!("  â†’ {}", review_result.summary);
                    review_result.final_diff
                }
                Ok(Err(e)) => {
                    log::warn!("  âš ï¸  Cross-sandbox review failed: {}. Using concatenation.", e);
                    fallback_diff
                }
                Err(_) => {
                    log::warn!("  âš ï¸  Cross-sandbox review timed out. Using concatenation.");
                    fallback_diff
                }
            }
        } else if sandbox_diffs.len() == 1 {
            sandbox_diffs.into_iter().next().map(|(_, d)| d).unwrap_or_default()
        } else {
            String::new()
        };

        // Collect failed tasks
        let failed_tasks: Vec<FailedTask> = scheduler
            .get_failed_tasks()
            .await
            .into_iter()
            .map(|(task_id, error)| FailedTask { task_id, error })
            .collect();

        let duration = start_time.elapsed().as_secs();

        let health_status = health_monitor.get_status().await;
        log::info!("\nâœ“ Orchestration complete!");
        log::info!("  Total duration: {}s", duration);
        log::info!("  Agents used: {}", agent_count);
        log::info!("  Tasks completed: {}/{}", scheduler_stats.completed, scheduler_stats.total_tasks);
        log::info!("  Health: {} dead agents, {} open circuits",
            health_status.dead_agents.len(), health_status.open_circuits);
        for (sandbox, state) in &health_status.sandbox_states {
            log::info!("    {} circuit: {:?}", sandbox, state);
        }

        // Signal TUI that orchestration is complete
        if let Some(ref tx) = event_tx_clone {
            let _ = tx.send(OrchestratorEvent::Done);
        }

        let collected_errors = execution_errors.read().await.clone();
        Ok(FinalOutput {
            merged_diff: final_merge,
            sandbox_results,
            verification_status,
            failed_tasks,
            known_issues,
            summary: generate_summary(&scheduler_stats, agent_count, duration),
            total_agents_used: agent_count,
            total_duration_secs: duration,
            scheduler_stats,
            execution_errors: collected_errors,
        })
    }

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Mode 1: Simple â€” single agent, no planner, no merge, no verification
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    async fn run_simple(
        &self,
        prompt: &str,
        _dir_tree: &str,
        start_time: std::time::Instant,
        event_tx: Option<std::sync::mpsc::Sender<OrchestratorEvent>>,
    ) -> Result<FinalOutput> {
        let send_ev = |ev: OrchestratorEvent| {
            if let Some(ref tx) = event_tx { let _ = tx.send(ev); }
        };

        log::info!("=== Mode: SIMPLE (single agent, no planner, no merge, no verification) ===");

        send_ev(OrchestratorEvent::LayerProgress {
            layer: 2,
            message: "Creating single sandbox (simple mode)...".into(),
        });

        // Layer 2: create one minimal sandbox
        let topology = Arc::new(
            TopologyManager::new(
                self.config.project_root.clone(),
                self.config.socket_path.clone(),
            )?
        );

        let sandbox = SandboxConfig {
            name: "main".to_string(),
            scope: ".".to_string(),
            tools: vec![
                "read_file".to_string(),
                "write_file".to_string(),
                "run_command".to_string(),
                "git_commit".to_string(),
            ],
            max_agents: 1,
            image: Some("alpine".to_string()),
        };
        topology.create_sandbox_layer(&sandbox).await
            .context("Simple mode: failed to create sandbox layer")?;

        // Layer 4: single agent execution
        send_ev(OrchestratorEvent::LayerProgress {
            layer: 4,
            message: "Executing task (simple mode)...".into(),
        });

        let executor = AgentExecutor::new(
            self.config.project_id.clone(),
            self.config.socket_path.clone(),
            self.config.checkpoint_root.clone(),
        )?;

        let agent = topology
            .wake_or_create_agent_layer(&sandbox.name, Some("simple-task".to_string()))
            .await
            .context("Simple mode: failed to create agent layer")?;

        let system_prompt = format!(
            "You are an expert software engineer. \
             WORKING DIRECTORY: /workspace. ALL file paths must use /workspace prefix.\n\n\
             TASK: {}\n\n\
             Write complete, production-quality code. No placeholders. No stubs.",
            prompt
        );

        send_ev(OrchestratorEvent::TaskStarted {
            worker_id: 0,
            task_id: "simple-task".to_string(),
            description: prompt.chars().take(80).collect(),
            sandbox: sandbox.name.clone(),
        });

        let result = executor
            .execute_task(&agent, prompt, &sandbox.tools, &system_prompt)
            .await;

        // Capture host-side diff BEFORE sleeping the container.
        // The in-container git diff (agent_result.git_diff) can be empty if the
        // git state inside the container wasn't staged correctly.  The host-side
        // `capture_agent_diff` uses `git diff --no-index` between the project root
        // and the container's overlayfs upper directory â€” more reliable.
        let host_diff = topology.capture_agent_diff(&agent.agent_id).await
            .unwrap_or_default();
        log::info!("  [simple] host diff size: {} bytes", host_diff.len());

        let _ = topology.sleep_agent_layer(&agent.agent_id, &sandbox.name).await;
        let _ = topology.cleanup_sleeping_containers().await;

        let duration = start_time.elapsed().as_secs();

        match result {
            Ok(agent_result) => {
                // Prefer host-side diff; fall back to in-container diff if host is empty
                let diff = if !host_diff.is_empty() {
                    host_diff
                } else {
                    agent_result.git_diff.clone().unwrap_or_default()
                };
                let success = agent_result.success;

                send_ev(OrchestratorEvent::TaskCompleted {
                    worker_id: 0,
                    task_id: "simple-task".to_string(),
                    success,
                    diff_size: diff.len(),
                });
                send_ev(OrchestratorEvent::LayerProgress { layer: 5, message: "Merge (single agent — pass-through)".into() });
                send_ev(OrchestratorEvent::LayerProgress { layer: 6, message: "Verification skipped (simple mode).".into() });
                send_ev(OrchestratorEvent::LayerProgress { layer: 7, message: "Output ready.".into() });
                send_ev(OrchestratorEvent::Done);

                let sandbox_result = SandboxResult {
                    sandbox_name: sandbox.name.clone(),
                    success,
                    merged_diff: if diff.is_empty() { None } else { Some(diff.clone()) },
                    verification_status: VerificationStatus::NotStarted,
                    timestamp: current_timestamp(),
                };

                let mut sandbox_results = HashMap::new();
                sandbox_results.insert(sandbox.name.clone(), sandbox_result);
                let mut verification_status = HashMap::new();
                verification_status.insert(sandbox.name.clone(), VerificationStatus::NotStarted);

                let simple_stats = SchedulerStats {
                    total_tasks: 1,
                    completed: if success { 1 } else { 0 },
                    failed: if success { 0 } else { 1 },
                    running: 0,
                    pending: 0,
                };

                log::info!("âœ“ Simple mode complete in {}s", duration);
                Ok(FinalOutput {
                    merged_diff: diff,
                    sandbox_results,
                    verification_status,
                    failed_tasks: if success {
                        vec![]
                    } else {
                        vec![FailedTask {
                            task_id: "simple-task".to_string(),
                            error: agent_result.error.unwrap_or_else(|| "agent failed".to_string()),
                        }]
                    },
                    known_issues: vec![],
                    summary: format!(
                        "Simple mode: 1 task, 1 agent, {}s. {}",
                        duration,
                        if success { "Success." } else { "Failed." }
                    ),
                    total_agents_used: 1,
                    total_duration_secs: duration,
                    scheduler_stats: simple_stats,
                    execution_errors: vec![],
                })
            }
            Err(e) => {
                send_ev(OrchestratorEvent::TaskFailed {
                    worker_id: 0,
                    task_id: "simple-task".to_string(),
                    error: e.to_string(),
                });
                send_ev(OrchestratorEvent::Done);
                Err(e.context("Simple mode agent execution failed"))
            }
        }
    }

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Mode 2: Standard â€” constrained planner, 1 sandbox, 1 verification round
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    async fn run_standard(
        &self,
        prompt: &str,
        dir_tree: &str,
        start_time: std::time::Instant,
        event_tx: Option<std::sync::mpsc::Sender<OrchestratorEvent>>,
    ) -> Result<FinalOutput> {
        let send_ev = |ev: OrchestratorEvent| {
            if let Some(ref tx) = event_tx { let _ = tx.send(ev); }
        };

        log::info!("=== Mode: STANDARD (constrained planner, 1 sandbox, 1 verify round) ===");

        // Layer 1 (constrained)
        send_ev(OrchestratorEvent::LayerProgress {
            layer: 1,
            message: "Planning tasks (standard mode)...".into(),
        });
        let planner_output = plan_task_standard(
            prompt,
            &self.config.project_root,
            &self.config.project_id,
            dir_tree,
        )
        .await
        .context("Standard planner failed")?;

        log::info!(
            "  â†’ Standard plan: {} tasks, {} sandbox(es)",
            planner_output.task_graph.tasks.len(),
            planner_output.sandbox_topology.sandboxes.len()
        );

        // Layer 2
        send_ev(OrchestratorEvent::LayerProgress {
            layer: 2,
            message: "Creating sandbox topology (standard mode)...".into(),
        });
        let topology = Arc::new(
            TopologyManager::new(
                self.config.project_root.clone(),
                self.config.socket_path.clone(),
            )?
        );
        for sandbox in &planner_output.sandbox_topology.sandboxes {
            topology.create_sandbox_layer(sandbox).await?;
        }

        // Layer 3
        send_ev(OrchestratorEvent::LayerProgress {
            layer: 3,
            message: "Initializing scheduler (standard mode)...".into(),
        });
        let scheduler = Arc::new(
            super::scheduler::Scheduler::new(
                planner_output.task_graph.clone(),
                planner_output.sandbox_hints.clone(),
            )?
        );

        // Layer 4 â€” cap at 3 concurrent agents (standard mode)
        send_ev(OrchestratorEvent::LayerProgress {
            layer: 4,
            message: "Executing tasks (standard mode)...".into(),
        });
        let executor = Arc::new(AgentExecutor::new(
            self.config.project_id.clone(),
            self.config.socket_path.clone(),
            self.config.checkpoint_root.clone(),
        )?);
        let health_monitor = Arc::new(HealthMonitor::new(300, 5));

        let max_concurrent = self.config.max_agents.min(3); // standard mode cap
        let sandbox_agent_results = Arc::new(RwLock::new(
            HashMap::<SandboxName, Vec<AgentWorkResult>>::new()
        ));
        let execution_errors = Arc::new(RwLock::new(Vec::<String>::new()));

        let staging_dir_clone = self.config.staging_dir.clone();
        let event_tx_clone = self.config.event_tx.clone();
        let mut handles = Vec::new();

        for worker_id in 0..max_concurrent {
            let scheduler_clone = scheduler.clone();
            let topology_clone = topology.clone();
            let executor_clone = executor.clone();
            let results_clone = sandbox_agent_results.clone();
            let errors_clone = execution_errors.clone();
            let sandboxes = planner_output.sandbox_topology.sandboxes.clone();
            let staging_dir_for_worker = staging_dir_clone.clone();
            let event_tx_for_worker = event_tx_clone.clone();
            let health_clone = health_monitor.clone();

            handles.push(tokio::spawn(async move {
                loop {
                    let mut task_found = false;
                    for sandbox in &sandboxes {
                        if let Some(ready_task_id) = scheduler_clone.get_ready_task(&sandbox.name).await {
                            task_found = true;
                            if !health_clone.is_sandbox_healthy(&sandbox.name).await {
                                continue;
                            }
                            let task_description = scheduler_clone
                                .get_task_description(&ready_task_id)
                                .await
                                .unwrap_or_else(|| "Unknown task".to_string());

                            let agent = match topology_clone
                                .wake_or_create_agent_layer(&sandbox.name, Some(ready_task_id.clone()))
                                .await
                            {
                                Ok(a) => a,
                                Err(e) => {
                                    errors_clone.write().await.push(format!(
                                        "[Worker {}] Failed to create agent: {}",
                                        worker_id, e
                                    ));
                                    continue;
                                }
                            };

                            health_clone.heartbeat(&agent.agent_id).await;

                            if let Some(ref tx) = event_tx_for_worker {
                                let _ = tx.send(OrchestratorEvent::TaskStarted {
                                    worker_id,
                                    task_id: ready_task_id.clone(),
                                    description: task_description.clone(),
                                    sandbox: sandbox.name.clone(),
                                });
                            }

                            if let Err(e) = scheduler_clone
                                .mark_task_started(ready_task_id.clone(), agent.clone())
                                .await
                            {
                                errors_clone.write().await.push(format!(
                                    "[Worker {}] mark_task_started failed: {}",
                                    worker_id, e
                                ));
                                continue;
                            }

                            let system_prompt = format!(
                                "You are an expert software engineer. \
                                 WORKING DIRECTORY: /workspace. ALL file paths MUST use /workspace prefix.\n\n\
                                 TASK: {}\n\n\
                                 Write complete, production-quality code. No placeholders. No stubs.",
                                task_description
                            );

                            let mut result = match executor_clone
                                .execute_task(&agent, &task_description, &sandbox.tools, &system_prompt)
                                .await
                            {
                                Ok(r) => r,
                                Err(e) => {
                                    errors_clone.write().await.push(format!(
                                        "[Worker {}] Task '{}' failed: {}",
                                        worker_id, task_description, e
                                    ));
                                    continue;
                                }
                            };

                            // Capture host-side diff BEFORE sleeping the agent.
                            // If the in-container git diff is empty (common when the git
                            // state inside the container isn't set up correctly), fall back
                            // to `git diff --no-index` between the project root and the
                            // container's overlayfs upper directory on the host.
                            if result.git_diff.as_ref().map_or(true, |d| d.is_empty()) {
                                if let Ok(host_diff) = topology_clone
                                    .capture_agent_diff(&agent.agent_id)
                                    .await
                                {
                                    if !host_diff.is_empty() {
                                        log::info!(
                                            "  [Worker {}] Using host-side diff ({} bytes)",
                                            worker_id, host_diff.len()
                                        );
                                        result.git_diff = Some(host_diff);
                                    }
                                }
                            }

                            if result.success {
                                if let Some(ref diff) = result.git_diff {
                                    if !diff.is_empty() {
                                        if let Err(e) = topology_clone
                                            .apply_diff_to_sandbox(&sandbox.name, diff)
                                            .await
                                        {
                                            log::warn!("[Worker {}] apply_diff failed: {}", worker_id, e);
                                        }
                                    }
                                }
                            }

                            if let Some(ref tx) = event_tx_for_worker {
                                let diff_size = result.git_diff.as_ref().map(|d| d.len()).unwrap_or(0);
                                if result.success {
                                    let _ = tx.send(OrchestratorEvent::TaskCompleted {
                                        worker_id,
                                        task_id: ready_task_id.clone(),
                                        success: true,
                                        diff_size,
                                    });
                                } else {
                                    let _ = tx.send(OrchestratorEvent::TaskFailed {
                                        worker_id,
                                        task_id: ready_task_id.clone(),
                                        error: result.error.clone().unwrap_or_default(),
                                    });
                                }
                            }

                            let _ = scheduler_clone.handle_task_completion(result.clone()).await;

                            if result.success {
                                health_clone.record_success(&sandbox.name).await;
                            } else {
                                health_clone.record_failure(&sandbox.name).await;
                            }
                            health_clone.remove_agent(&agent.agent_id).await;

                            {
                                let mut results = results_clone.write().await;
                                results
                                    .entry(sandbox.name.clone())
                                    .or_insert_with(Vec::new)
                                    .push(AgentWorkResult {
                                        result: result.clone(),
                                        task_description: task_description.clone(),
                                    });
                            }

                            if let Some(ref staging_dir) = staging_dir_for_worker {
                                if let Err(e) = topology_clone
                                    .stage_agent_workspace(&agent.agent_id, staging_dir)
                                    .await
                                {
                                    log::warn!("[Worker {}] Failed to stage workspace: {}", worker_id, e);
                                }
                            }

                            let _ = topology_clone
                                .sleep_agent_layer(&agent.agent_id, &sandbox.name)
                                .await;
                            break;
                        }
                    }
                    if !task_found {
                        let stats = scheduler_clone.get_stats().await;
                        if stats.completed + stats.failed >= stats.total_tasks {
                            break;
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                }
            }));
        }

        // Wait with a 15-minute cap (standard mode)
        let timeout = tokio::time::Duration::from_secs(900);
        match tokio::time::timeout(timeout, async {
            for h in handles { let _ = h.await; }
        }).await {
            Ok(_) => log::info!("  â†’ Standard workers completed"),
            Err(_) => log::warn!("  âš  Standard workers timed out after 15 minutes"),
        }

        let _ = topology.cleanup_sleeping_containers().await;

        let sandbox_agent_results = sandbox_agent_results.read().await.clone();
        let scheduler_stats = scheduler.get_stats().await;

        // Layer 5: merge
        send_ev(OrchestratorEvent::LayerProgress {
            layer: 5,
            message: "Merging (standard mode)...".into(),
        });
        let reviewer = MergeReviewerAgent::new(self.config.project_id.clone());
        let mut sandbox_results = HashMap::new();

        for (sandbox_name, agent_work_results) in &sandbox_agent_results {
            let contributions: Vec<AgentContribution> = agent_work_results
                .iter()
                .filter(|awr| awr.result.success)
                .filter(|awr| awr.result.git_diff.as_ref().map_or(false, |d| !d.is_empty()))
                .map(|awr| {
                    let raw_diff = awr.result.git_diff.clone().unwrap_or_default();
                    let file_changes = parse_unified_diff(&raw_diff);
                    AgentContribution {
                        agent_id: awr.result.task_id.clone(),
                        task_id: awr.result.task_id.clone(),
                        task_description: awr.task_description.clone(),
                        file_changes,
                        raw_diff,
                    }
                })
                .collect();

            let merged_diff = if contributions.is_empty() {
                None
            } else if contributions.len() == 1 {
                Some(contributions[0].raw_diff.clone())
            } else {
                let fallback = contributions.iter().map(|c| c.raw_diff.clone()).collect::<Vec<_>>().join("\n");
                let conflicts = ConflictDetector::detect(&contributions);
                match tokio::time::timeout(
                    tokio::time::Duration::from_secs(300),
                    reviewer.review(contributions, conflicts),
                ).await {
                    Ok(Ok(r)) => Some(r.final_diff),
                    _ => Some(fallback),
                }
            };

            sandbox_results.insert(
                sandbox_name.clone(),
                SandboxResult {
                    sandbox_name: sandbox_name.clone(),
                    success: true,
                    merged_diff: merged_diff.clone(),
                    verification_status: VerificationStatus::NotStarted,
                    timestamp: current_timestamp(),
                },
            );
        }

        // Layer 6: exactly 1 verification round
        send_ev(OrchestratorEvent::LayerProgress {
            layer: 6,
            message: "Verifying (standard mode, 1 round)...".into(),
        });
        let verification_loop = VerificationLoop::new(self.config.project_id.clone(), 1);
        let mut verification_status = HashMap::new();

        let executor_ref = &*executor;
        let topology_ref = &*topology;

        for (sandbox_name, sandbox_result) in &mut sandbox_results {
            if let Some(ref diff) = sandbox_result.merged_diff.clone() {
                if !diff.is_empty() {
                    let original_tasks: Vec<Task> = planner_output
                        .task_graph
                        .tasks
                        .iter()
                        .filter(|t| {
                            planner_output
                                .sandbox_hints
                                .get(&t.id)
                                .map_or(false, |s| s == sandbox_name)
                        })
                        .cloned()
                        .collect();

                    match tokio::time::timeout(
                        tokio::time::Duration::from_secs(30 * 60),
                        verification_loop.verify_sandbox(
                            sandbox_name,
                            diff,
                            &original_tasks,
                            topology_ref,
                            executor_ref,
                        ),
                    ).await {
                        Ok(Ok(vr)) => {
                            log::info!(
                                "  âœ“ {} â€” {:?} ({} passed, {} failed)",
                                sandbox_name, vr.status,
                                vr.passed_tests.len(), vr.failed_tests.len()
                            );
                            if let Some(ref updated) = vr.updated_diff {
                                if !updated.is_empty() {
                                    sandbox_result.merged_diff = Some(updated.clone());
                                }
                            }
                            sandbox_result.verification_status = vr.status.clone();
                            verification_status.insert(sandbox_name.clone(), vr.status);
                        }
                        Ok(Err(e)) => {
                            log::warn!("  âš  Verification failed for {}: {}", sandbox_name, e);
                            sandbox_result.verification_status = VerificationStatus::Failed;
                            verification_status.insert(sandbox_name.clone(), VerificationStatus::Failed);
                        }
                        Err(_) => {
                            log::warn!("  âš  Verification timed out for {}", sandbox_name);
                            sandbox_result.verification_status = VerificationStatus::NotStarted;
                            verification_status.insert(sandbox_name.clone(), VerificationStatus::NotStarted);
                        }
                    }
                } else {
                    verification_status.insert(sandbox_name.clone(), VerificationStatus::NotStarted);
                }
            } else {
                verification_status.insert(sandbox_name.clone(), VerificationStatus::NotStarted);
            }
        }

        send_ev(OrchestratorEvent::LayerProgress { layer: 7, message: "Final output (single sandbox — no cross-sandbox merge needed).".into() });
        // No Layer 7 cross-sandbox merge needed in standard mode (single sandbox)
        let final_merge = sandbox_results
            .values()
            .find_map(|r| r.merged_diff.clone())
            .unwrap_or_default();

        let failed_tasks: Vec<FailedTask> = scheduler
            .get_failed_tasks()
            .await
            .into_iter()
            .map(|(task_id, error)| FailedTask { task_id, error })
            .collect();

        let duration = start_time.elapsed().as_secs();
        let collected_errors = execution_errors.read().await.clone();
        let agent_count = planner_output.task_graph.tasks.len().min(max_concurrent);

        log::info!("âœ“ Standard mode complete in {}s", duration);

        if let Some(ref tx) = self.config.event_tx {
            let _ = tx.send(OrchestratorEvent::Done);
        }

        Ok(FinalOutput {
            merged_diff: final_merge,
            sandbox_results,
            verification_status,
            failed_tasks,
            known_issues: vec![],
            summary: format!(
                "Standard mode: {}/{} tasks, {} agents, {}s. {} failed.",
                scheduler_stats.completed, scheduler_stats.total_tasks,
                agent_count, duration, scheduler_stats.failed
            ),
            total_agents_used: agent_count,
            total_duration_secs: duration,
            scheduler_stats,
            execution_errors: collected_errors,
        })
    }
}

fn generate_summary(stats: &SchedulerStats, agents: usize, duration: u64) -> String {
    format!(
        "Completed {}/{} tasks using {} agents in {}s. {} failed.",
        stats.completed, stats.total_tasks, agents, duration, stats.failed
    )
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}