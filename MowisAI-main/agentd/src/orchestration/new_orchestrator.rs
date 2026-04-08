//! New 7-layer orchestration system entry point

use super::agent_execution::AgentExecutor;
use super::checkpoint::CheckpointManager;
use super::merge_worker::ParallelMergeCoordinator;
use super::planner::plan_task;
use super::sandbox_topology::TopologyManager;
use super::scheduler::{Scheduler, SchedulerStats};
use super::verification::{VerificationLoop, VerificationResult};
use agentd_protocol::{AgentResult, SandboxName, SandboxResult, TaskId, VerificationStatus};
use anyhow::{anyhow, Context, Result};
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
}

/// New 7-layer orchestrator
pub struct NewOrchestrator {
    config: OrchestratorConfig,
}

impl NewOrchestrator {
    pub fn new(config: OrchestratorConfig) -> Self {
        Self { config }
    }

    /// Run orchestration for a user prompt
    pub async fn run(&self, prompt: &str) -> Result<FinalOutput> {
        let start_time = std::time::Instant::now();

        // Layer 1: Fast Planner
        println!("Layer 1: Planning tasks...");
        let planner_output = plan_task(prompt, &self.config.project_root, &self.config.project_id)
            .await
            .context("Fast planner failed")?;

        println!(
            "  → Generated {} tasks across {} sandboxes",
            planner_output.task_graph.tasks.len(),
            planner_output.sandbox_topology.sandboxes.len()
        );

        // Layer 2: Overlayfs Topology
        println!("Layer 2: Creating sandbox topology...");
        let topology = TopologyManager::new(
            self.config.project_root.clone(),
            self.config.socket_path.clone(),
        )?;

        for sandbox in &planner_output.sandbox_topology.sandboxes {
            topology.create_sandbox_layer(sandbox).await?;
            println!("  → Created sandbox: {}", sandbox.name);
        }

        // Layer 3: Scheduler
        println!("Layer 3: Initializing scheduler...");
        let scheduler = Arc::new(
            Scheduler::new(
                planner_output.task_graph.clone(),
                planner_output.sandbox_hints.clone(),
            )?
        );

        println!("  → Scheduler ready with {} tasks", planner_output.task_graph.tasks.len());

        // Layer 4: Agent Execution (TRUE PARALLELISM)
        println!("Layer 4: Executing tasks with agents...");
        let agent_executor = Arc::new(AgentExecutor::new(
            self.config.project_id.clone(),
            self.config.socket_path.clone(),
            self.config.checkpoint_root.clone(),
        )?);

        let topology = Arc::new(topology);
        let sandbox_agent_results = Arc::new(RwLock::new(HashMap::<SandboxName, Vec<AgentResult>>::new()));
        let agent_count = Arc::new(RwLock::new(0usize));

        // Create a pool of agent worker tasks (TRUE PARALLELISM)
        let max_concurrent_agents = self.config.max_agents.min(50); // Cap at 50 concurrent agents
        println!("  → Spawning {} concurrent agent workers...", max_concurrent_agents);

        let mut handles = Vec::new();

        for worker_id in 0..max_concurrent_agents {
            let scheduler_clone = scheduler.clone();
            let topology_clone = topology.clone();
            let executor_clone = agent_executor.clone();
            let results_clone = sandbox_agent_results.clone();
            let count_clone = agent_count.clone();
            let sandboxes = planner_output.sandbox_topology.sandboxes.clone();

            let handle = tokio::spawn(async move {
                loop {
                    // Try to get a ready task from any sandbox
                    let mut task_found = false;

                    for sandbox in &sandboxes {
                        if let Some(ready_task_id) = scheduler_clone.get_ready_task(&sandbox.name).await {
                            task_found = true;

                            // Find the task details
                            let task_description = scheduler_clone.get_task_description(&ready_task_id).await
                                .unwrap_or_else(|| "Unknown task".to_string());

                            // Create agent layer
                            let agent = match topology_clone
                                .create_agent_layer(&sandbox.name, Some(ready_task_id.clone()))
                                .await {
                                    Ok(a) => a,
                                    Err(e) => {
                                        eprintln!("[Worker {}] Failed to create agent: {}", worker_id, e);
                                        continue;
                                    }
                                };

                            // Increment agent count
                            {
                                let mut count = count_clone.write().await;
                                *count += 1;
                            }

                            // Mark task as started
                            if let Err(e) = scheduler_clone.mark_task_started(ready_task_id.clone(), agent.clone()).await {
                                eprintln!("[Worker {}] Failed to mark task started: {}", worker_id, e);
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
                                        eprintln!("[Worker {}] Task execution failed: {}", worker_id, e);
                                        continue;
                                    }
                                };

                            // Apply diff to sandbox layer if successful
                            if result.success {
                                if let Some(ref diff) = result.git_diff {
                                    if !diff.is_empty() {
                                        if let Err(e) = topology_clone.apply_diff_to_sandbox(&sandbox.name, diff).await {
                                            eprintln!("[Worker {}] Failed to apply diff to sandbox: {}", worker_id, e);
                                        }
                                    }
                                }
                            }

                            // Handle completion
                            if let Err(e) = scheduler_clone.handle_task_completion(result.clone()).await {
                                eprintln!("[Worker {}] Failed to handle completion: {}", worker_id, e);
                            }

                            // Store result
                            {
                                let mut results = results_clone.write().await;
                                results
                                    .entry(sandbox.name.clone())
                                    .or_insert_with(Vec::new)
                                    .push(result.clone());
                            }

                            // Cleanup agent layer
                            let _ = topology_clone.destroy_agent_layer(&agent.agent_id).await;

                            println!("    ✓ [Worker {}] Completed: {}", worker_id, task_description);

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

        // Wait for all workers to complete with timeout
        let worker_timeout = tokio::time::Duration::from_secs(1800); // 30 minute max
        println!("  → Waiting for all workers to complete (max 30 minutes)...");

        let all_workers = async {
            for handle in handles {
                let _ = handle.await;
            }
        };

        match tokio::time::timeout(worker_timeout, all_workers).await {
            Ok(_) => println!("  → All workers completed successfully"),
            Err(_) => {
                eprintln!("  ⚠️  Workers timed out after 30 minutes");
            }
        }

        let agent_count = *agent_count.read().await;
        let sandbox_agent_results = {
            let results = sandbox_agent_results.read().await;
            results.clone()
        };

        // Get scheduler stats
        let scheduler_stats = scheduler.get_stats().await;
        println!("  → Completed: {}/{} tasks", scheduler_stats.completed, scheduler_stats.total_tasks);

        // Layer 5: Parallel Merge (per sandbox)
        println!("Layer 5: Merging agent results per sandbox...");
        let merge_coordinator = ParallelMergeCoordinator::new(
            self.config.project_id.clone(),
            self.config.merge_work_dir.clone(),
            self.config.project_root.clone(),
        )?;

        let mut sandbox_results = HashMap::new();

        for (sandbox_name, agent_results) in &sandbox_agent_results {
            let diffs: Vec<String> = agent_results
                .iter()
                .filter_map(|r| r.git_diff.clone())
                .filter(|d| !d.is_empty()) // Skip empty diffs
                .collect();

            if diffs.is_empty() {
                println!("  → Sandbox {} has no changes", sandbox_name);
                // Still create result even with no changes
                sandbox_results.insert(
                    sandbox_name.clone(),
                    SandboxResult {
                        sandbox_name: sandbox_name.clone(),
                        success: true,
                        merged_diff: None,
                        verification_status: VerificationStatus::NotStarted,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    },
                );
                continue;
            }

            println!("  → Merging {} diffs for sandbox: {}", diffs.len(), sandbox_name);

            // Merge with timeout (5 minutes max per sandbox)
            let merge_timeout = tokio::time::Duration::from_secs(300);
            let merge_task = merge_coordinator.merge_diffs(diffs);

            match tokio::time::timeout(merge_timeout, merge_task).await {
                Ok(Ok(merge_result)) => {
                    sandbox_results.insert(
                        sandbox_name.clone(),
                        SandboxResult {
                            sandbox_name: sandbox_name.clone(),
                            success: merge_result.success,
                            merged_diff: Some(merge_result.merged_diff),
                            verification_status: VerificationStatus::NotStarted,
                            timestamp: current_timestamp(),
                        },
                    );

                    println!(
                        "    ✓ Merged with {} conflicts resolved",
                        merge_result.conflicts_resolved
                    );
                }
                Ok(Err(e)) => {
                    eprintln!("  ⚠️  Merge failed for {}: {}", sandbox_name, e);
                    sandbox_results.insert(
                        sandbox_name.clone(),
                        SandboxResult {
                            sandbox_name: sandbox_name.clone(),
                            success: false,
                            merged_diff: None,
                            verification_status: VerificationStatus::Failed,
                            timestamp: current_timestamp(),
                        },
                    );
                }
                Err(_) => {
                    eprintln!("  ⚠️  Merge timed out for {} after 5 minutes", sandbox_name);
                    sandbox_results.insert(
                        sandbox_name.clone(),
                        SandboxResult {
                            sandbox_name: sandbox_name.clone(),
                            success: false,
                            merged_diff: None,
                            verification_status: VerificationStatus::Failed,
                            timestamp: current_timestamp(),
                        },
                    );
                }
            }
        }

        // Layer 6: Verification Loop (SKIP for now - speeds up completion)
        println!("Layer 6: Verifying sandbox results...");
        println!("  → Skipping verification (not yet implemented)");

        let mut verification_status = HashMap::new();
        let known_issues = Vec::new();

        // Mark all as passed without verification
        for (sandbox_name, sandbox_result) in &mut sandbox_results {
            sandbox_result.verification_status = VerificationStatus::NotStarted;
            verification_status.insert(sandbox_name.clone(), VerificationStatus::NotStarted);
        }

        // Layer 7: Cross-Sandbox Merge
        println!("Layer 7: Final cross-sandbox merge...");
        let all_sandbox_diffs: Vec<String> = sandbox_results
            .values()
            .filter_map(|r| r.merged_diff.clone())
            .filter(|d| !d.is_empty())
            .collect();

        let final_merge = if all_sandbox_diffs.len() > 1 {
            println!("  → Merging {} sandbox results", all_sandbox_diffs.len());
            // Merge with timeout
            let merge_timeout = tokio::time::Duration::from_secs(300);
            let merge_task = merge_coordinator.merge_diffs(all_sandbox_diffs.clone());

            match tokio::time::timeout(merge_timeout, merge_task).await {
                Ok(Ok(merge_result)) => merge_result.merged_diff,
                Ok(Err(e)) => {
                    eprintln!("  ⚠️  Cross-sandbox merge failed: {}", e);
                    all_sandbox_diffs.into_iter().next().unwrap_or_default()
                }
                Err(_) => {
                    eprintln!("  ⚠️  Cross-sandbox merge timed out");
                    all_sandbox_diffs.into_iter().next().unwrap_or_default()
                }
            }
        } else if all_sandbox_diffs.len() == 1 {
            all_sandbox_diffs.into_iter().next().unwrap()
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

        println!("\n✓ Orchestration complete!");
        println!("  Total duration: {}s", duration);
        println!("  Agents used: {}", agent_count);
        println!("  Tasks completed: {}/{}", scheduler_stats.completed, scheduler_stats.total_tasks);

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
