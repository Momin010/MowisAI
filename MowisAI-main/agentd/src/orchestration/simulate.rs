//! Simulation command for end-to-end testing without LLM calls
//!
//! This command runs the entire 7-layer orchestration system with mock agents
//! to test scheduling, overlayfs propagation, merging, and all other components
//! for $0 cost. Perfect for development and debugging.

use super::mock_agent::MockAgentExecutor;
use super::sandbox_topology::TopologyManager;
use super::scheduler::{Scheduler, SchedulerStats};
use agentd_protocol::{SandboxConfig, Task, TaskGraph};
use anyhow::{Context, Result};
use clap::Parser;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

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

        let scheduler = Arc::new(Scheduler::new(task_graph, sandbox_hints)?);

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
        log::info!("📊 Success rate: {:.1}%", (stats.completed as f64 / stats.total_tasks as f64) * 100.0);
        log::info!("═══════════════════════════════════");

        if stats.failed > 0 {
            log::info!("\nFailed tasks:");
            for (task_id, error) in scheduler.get_failed_tasks().await {
                log::info!("  - {}: {}", task_id, error);
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