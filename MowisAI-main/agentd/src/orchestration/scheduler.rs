//! Layer 3: Scheduler — Event-driven task dispatcher
//!
//! Maintains dependency counters, dispatches tasks when ready, sandbox-aware routing

use agentd_protocol::{
    AgentHandle, AgentResult, SandboxName, SchedulerMessage, Task, TaskGraph, TaskId,
};
use anyhow::{anyhow, Result};
use dashmap::DashMap;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// Task scheduler with event-driven dispatch
#[derive(Debug)]
pub struct Scheduler {
    /// Task graph
    task_graph: Arc<HashMap<TaskId, Task>>,

    /// Dependency counters — decremented on task completion
    dep_counter: Arc<DashMap<TaskId, AtomicUsize>>,

    /// Reverse dependency map: task -> tasks that depend on it
    dependents: Arc<HashMap<TaskId, Vec<TaskId>>>,

    /// Sandbox assignment hints (from planner)
    sandbox_hints: Arc<HashMap<TaskId, SandboxName>>,

    /// Per-sandbox ready queues — tasks ready to run in each sandbox
    ready_queues: Arc<RwLock<HashMap<SandboxName, VecDeque<TaskId>>>>,

    /// Currently running tasks
    running: Arc<RwLock<HashMap<TaskId, AgentHandle>>>,

    /// Completed tasks
    completed: Arc<RwLock<HashSet<TaskId>>>,

    /// Failed tasks
    failed: Arc<RwLock<HashMap<TaskId, String>>>,

    /// Idle agent pools per sandbox
    idle_agents: Arc<RwLock<HashMap<SandboxName, VecDeque<AgentHandle>>>>,

    /// Task results
    results: Arc<RwLock<HashMap<TaskId, AgentResult>>>,
}

impl Scheduler {
    /// Create new scheduler from task graph and sandbox topology
    pub fn new(
        task_graph: TaskGraph,
        sandbox_hints: HashMap<TaskId, SandboxName>,
    ) -> Result<Self> {
        // Build task map
        let mut task_map = HashMap::new();
        for task in task_graph.tasks.iter() {
            task_map.insert(task.id.clone(), task.clone());
        }

        // Build dependency counters
        let dep_counter = DashMap::new();
        for task in task_graph.tasks.iter() {
            dep_counter.insert(task.id.clone(), AtomicUsize::new(task.deps.len()));
        }

        // Build reverse dependency map (task -> who depends on it)
        let mut dependents: HashMap<TaskId, Vec<TaskId>> = HashMap::new();
        for task in task_graph.tasks.iter() {
            for dep_id in &task.deps {
                dependents
                    .entry(dep_id.clone())
                    .or_insert_with(Vec::new)
                    .push(task.id.clone());
            }
        }

        // Initialize per-sandbox ready queues
        let mut ready_queues: HashMap<SandboxName, VecDeque<TaskId>> = HashMap::new();

        // Enqueue tasks that have no dependencies into their respective sandbox queues
        for task in task_graph.tasks.iter() {
            if task.deps.is_empty() {
                let sandbox_name = sandbox_hints.get(&task.id)
                    .cloned()
                    .unwrap_or_else(|| "default".to_string());

                ready_queues
                    .entry(sandbox_name)
                    .or_insert_with(VecDeque::new)
                    .push_back(task.id.clone());
            }
        }

        Ok(Self {
            task_graph: Arc::new(task_map),
            dep_counter: Arc::new(dep_counter),
            dependents: Arc::new(dependents),
            sandbox_hints: Arc::new(sandbox_hints),
            ready_queues: Arc::new(RwLock::new(ready_queues)),
            running: Arc::new(RwLock::new(HashMap::new())),
            completed: Arc::new(RwLock::new(HashSet::new())),
            failed: Arc::new(RwLock::new(HashMap::new())),
            idle_agents: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Get task description by ID
    pub async fn get_task_description(&self, task_id: &TaskId) -> Option<String> {
        self.task_graph.get(task_id).map(|t| t.description.clone())
    }

    /// Get next ready task for a specific sandbox — O(1) lookup from per-sandbox queue
    pub async fn get_ready_task(&self, sandbox_name: &SandboxName) -> Option<TaskId> {
        let mut queues = self.ready_queues.write().await;

        // Pop from this sandbox's queue
        if let Some(queue) = queues.get_mut(sandbox_name) {
            return queue.pop_front();
        }

        None
    }

    /// Mark task as started
    pub async fn mark_task_started(&self, task_id: TaskId, agent: AgentHandle) -> Result<()> {
        let mut running = self.running.write().await;
        running.insert(task_id, agent);
        Ok(())
    }

    /// Handle task completion — decrements dependent task counters
    pub async fn handle_task_completion(&self, result: AgentResult) -> Result<()> {
        let task_id = result.task_id.clone();

        // Mark as completed
        {
            let mut completed = self.completed.write().await;
            completed.insert(task_id.clone());
        }

        // Remove from running
        {
            let mut running = self.running.write().await;
            running.remove(&task_id);
        }

        // Store result
        {
            let mut results = self.results.write().await;
            results.insert(task_id.clone(), result.clone());
        }

        if !result.success {
            // Mark as failed
            let mut failed = self.failed.write().await;
            failed.insert(
                task_id.clone(),
                result.error.unwrap_or_else(|| "Unknown error".to_string()),
            );
            return Ok(());
        }

        // Decrement counters of dependent tasks and enqueue them if ready
        if let Some(deps) = self.dependents.get(&task_id) {
            let mut queues = self.ready_queues.write().await;

            for dep_task_id in deps {
                if let Some(counter) = self.dep_counter.get(dep_task_id) {
                    let prev = counter.fetch_sub(1, Ordering::SeqCst);
                    if prev == 1 {
                        // Counter hit 0 — task is ready, enqueue to its sandbox queue
                        let sandbox_name = self.sandbox_hints.get(dep_task_id)
                            .cloned()
                            .unwrap_or_else(|| "default".to_string());

                        queues
                            .entry(sandbox_name)
                            .or_insert_with(VecDeque::new)
                            .push_back(dep_task_id.clone());
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle task failure
    pub async fn handle_task_failure(&self, task_id: TaskId, error: String) -> Result<()> {
        // Mark as failed
        {
            let mut failed = self.failed.write().await;
            failed.insert(task_id.clone(), error);
        }

        // Remove from running
        {
            let mut running = self.running.write().await;
            running.remove(&task_id);
        }

        Ok(())
    }

    /// Register idle agent for a sandbox
    pub async fn register_idle_agent(&self, agent: AgentHandle) {
        let mut idle = self.idle_agents.write().await;
        idle.entry(agent.sandbox_name.clone())
            .or_insert_with(VecDeque::new)
            .push_back(agent);
    }

    /// Get idle agent for sandbox
    pub async fn get_idle_agent(&self, sandbox_name: &SandboxName) -> Option<AgentHandle> {
        let mut idle = self.idle_agents.write().await;
        idle.get_mut(sandbox_name)?.pop_front()
    }

    /// Get task info
    pub fn get_task(&self, task_id: &TaskId) -> Option<Task> {
        self.task_graph.get(task_id).cloned()
    }

    /// Get sandbox hint for task
    pub fn get_sandbox_hint(&self, task_id: &TaskId) -> Option<SandboxName> {
        self.sandbox_hints.get(task_id).cloned()
    }

    /// Check if all tasks are complete
    pub async fn is_complete(&self) -> bool {
        let completed = self.completed.read().await;
        let failed = self.failed.read().await;
        completed.len() + failed.len() == self.task_graph.len()
    }

    /// Get completion stats
    pub async fn get_stats(&self) -> SchedulerStats {
        let completed = self.completed.read().await;
        let failed = self.failed.read().await;
        let running = self.running.read().await;

        SchedulerStats {
            total_tasks: self.task_graph.len(),
            completed: completed.len(),
            failed: failed.len(),
            running: running.len(),
            pending: self
                .task_graph
                .len()
                .saturating_sub(completed.len() + failed.len() + running.len()),
        }
    }

    /// Get all completed results
    pub async fn get_results(&self) -> HashMap<TaskId, AgentResult> {
        self.results.read().await.clone()
    }

    /// Get failed tasks
    pub async fn get_failed_tasks(&self) -> HashMap<TaskId, String> {
        self.failed.read().await.clone()
    }
}

/// Scheduler statistics
#[derive(Debug, Clone)]
pub struct SchedulerStats {
    pub total_tasks: usize,
    pub completed: usize,
    pub failed: usize,
    pub running: usize,
    pub pending: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_scheduler_basic() {
        let tasks = vec![
            Task {
                id: "t1".to_string(),
                description: "Task 1".to_string(),
                deps: vec![],
                hint: Some("sandbox1".to_string()),
            },
            Task {
                id: "t2".to_string(),
                description: "Task 2".to_string(),
                deps: vec!["t1".to_string()],
                hint: Some("sandbox1".to_string()),
            },
        ];

        let task_graph = TaskGraph { tasks };
        let mut hints = HashMap::new();
        hints.insert("t1".to_string(), "sandbox1".to_string());
        hints.insert("t2".to_string(), "sandbox1".to_string());

        let scheduler = Scheduler::new(task_graph, hints).unwrap();

        // t1 should be ready immediately
        let ready = scheduler.get_ready_task(&"sandbox1".to_string()).await;
        assert_eq!(ready, Some("t1".to_string()));

        // Complete t1
        let result = AgentResult {
            task_id: "t1".to_string(),
            success: true,
            git_diff: Some("diff".to_string()),
            error: None,
            checkpoint_log: vec![],
            timestamp: 0,
        };

        scheduler.handle_task_completion(result).await.unwrap();

        // Now t2 should be ready
        let ready = scheduler.get_ready_task(&"sandbox1".to_string()).await;
        assert_eq!(ready, Some("t2".to_string()));
    }

    #[tokio::test]
    async fn test_scheduler_stats() {
        let tasks = vec![
            Task {
                id: "t1".to_string(),
                description: "Task 1".to_string(),
                deps: vec![],
                hint: None,
            },
            Task {
                id: "t2".to_string(),
                description: "Task 2".to_string(),
                deps: vec![],
                hint: None,
            },
        ];

        let task_graph = TaskGraph { tasks };
        let scheduler = Scheduler::new(task_graph, HashMap::new()).unwrap();

        let stats = scheduler.get_stats().await;
        assert_eq!(stats.total_tasks, 2);
        assert_eq!(stats.pending, 2);
        assert_eq!(stats.completed, 0);
    }
}
