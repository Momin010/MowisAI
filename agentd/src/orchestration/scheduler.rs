//! Layer 3: Scheduler — Event-driven task dispatcher
//!
//! Maintains dependency counters, dispatches tasks when ready, sandbox-aware routing.
//! Failed tasks properly unlock dependents (marking them as skipped).

use agentd_protocol::{
    AgentHandle, AgentResult, SandboxName, Task, TaskGraph, TaskId,
};
use anyhow::Result;
use dashmap::DashMap;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

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

    /// Failed tasks (task_id -> error message)
    failed: Arc<RwLock<HashMap<TaskId, String>>>,

    /// Skipped tasks (failed dependency chain)
    skipped: Arc<RwLock<HashSet<TaskId>>>,

    /// Idle agent pools per sandbox
    idle_agents: Arc<RwLock<HashMap<SandboxName, VecDeque<AgentHandle>>>>,

    /// Task results
    results: Arc<RwLock<HashMap<TaskId, AgentResult>>>,

    /// Total task count for quick completion check
    total_tasks: usize,
}

impl Scheduler {
    /// Create new scheduler from task graph and sandbox topology
    pub fn new(
        task_graph: TaskGraph,
        sandbox_hints: HashMap<TaskId, SandboxName>,
    ) -> Result<Self> {
        let total_tasks = task_graph.tasks.len();

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
            skipped: Arc::new(RwLock::new(HashSet::new())),
            idle_agents: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(HashMap::new())),
            total_tasks,
        })
    }

    /// Get task description by ID
    pub async fn get_task_description(&self, task_id: &TaskId) -> Option<String> {
        self.task_graph.get(task_id).map(|t| t.description.clone())
    }

    /// Get next ready task for a specific sandbox — O(1) lookup from per-sandbox queue
    pub async fn get_ready_task(&self, sandbox_name: &SandboxName) -> Option<TaskId> {
        let mut queues = self.ready_queues.write().await;
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

    /// Handle task completion — decrements dependent task counters.
    /// On failure: marks dependents as skipped (they cannot proceed).
    pub async fn handle_task_completion(&self, result: AgentResult) -> Result<()> {
        let task_id = result.task_id.clone();

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
            {
                let mut failed = self.failed.write().await;
                failed.insert(
                    task_id.clone(),
                    result.error.clone().unwrap_or_else(|| "Unknown error".to_string()),
                );
            }

            // CRITICAL FIX: On failure, skip all dependents (they cannot proceed)
            self.skip_dependents(&task_id).await;
            return Ok(());
        }

        // Mark as completed
        {
            let mut completed = self.completed.write().await;
            completed.insert(task_id.clone());
        }

        // Decrement counters of dependent tasks and enqueue them if ready
        self.unlock_dependents(&task_id).await;

        Ok(())
    }

    /// Handle task failure explicitly (alternative to handle_task_completion)
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

        // Skip all dependents
        self.skip_dependents(&task_id).await;

        Ok(())
    }

    /// Recursively skip all tasks that depend on a failed task
    async fn skip_dependents(&self, failed_task_id: &TaskId) {
        let mut to_skip = VecDeque::new();
        if let Some(deps) = self.dependents.get(failed_task_id) {
            for dep_id in deps {
                to_skip.push_back(dep_id.clone());
            }
        }

        // BFS to skip all transitive dependents
        while let Some(task_id) = to_skip.pop_front() {
            // Check if this task was already completed or skipped
            {
                let completed = self.completed.read().await;
                let skipped = self.skipped.read().await;
                let failed = self.failed.read().await;
                if completed.contains(&task_id) || skipped.contains(&task_id) || failed.contains_key(&task_id) {
                    continue;
                }
            }

            // Check if all of this task's dependencies are resolved (completed/skipped/failed)
            // If any dependency failed, this task must be skipped
            let should_skip = if let Some(task) = self.task_graph.get(&task_id) {
                let completed = self.completed.read().await;
                let failed = self.failed.read().await;
                let skipped = self.skipped.read().await;
                task.deps.iter().any(|dep| {
                    failed.contains_key(dep) || skipped.contains(dep)
                })
            } else {
                true
            };

            if should_skip {
                {
                    let mut skipped = self.skipped.write().await;
                    skipped.insert(task_id.clone());
                }

                // Also remove from ready queues if it was there
                {
                    let mut queues = self.ready_queues.write().await;
                    for queue in queues.values_mut() {
                        queue.retain(|id| id != &task_id);
                    }
                }

                // Cascade to this task's dependents
                if let Some(deps) = self.dependents.get(&task_id) {
                    for dep_id in deps {
                        to_skip.push_back(dep_id.clone());
                    }
                }
            }
        }
    }

    /// Unlock dependents of a successfully completed task
    async fn unlock_dependents(&self, completed_task_id: &TaskId) {
        if let Some(deps) = self.dependents.get(completed_task_id) {
            let mut queues = self.ready_queues.write().await;

            for dep_task_id in deps {
                // Don't unlock tasks that are already failed or skipped
                {
                    let failed = self.failed.read().await;
                    let skipped = self.skipped.read().await;
                    if failed.contains_key(dep_task_id) || skipped.contains(dep_task_id) {
                        continue;
                    }
                }

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

    /// Check if all tasks are resolved (completed, failed, skipped, or in-flight).
    /// Returns true when there are no pending tasks left to dispatch.
    pub async fn is_complete(&self) -> bool {
        // If any ready queues still have tasks, we're not done
        {
            let queues = self.ready_queues.read().await;
            let ready_count: usize = queues.values().map(|q| q.len()).sum();
            if ready_count > 0 {
                return false;
            }
        }

        // If any non-terminal task still has unmet deps, we're not done
        let completed = self.completed.read().await;
        let failed = self.failed.read().await;
        let skipped = self.skipped.read().await;

        !self.dep_counter.iter().any(|entry| {
            let count = entry.value().load(Ordering::SeqCst);
            if count == 0 {
                return false;
            }
            let task_id = entry.key();
            !completed.contains(task_id)
                && !failed.contains_key(task_id)
                && !skipped.contains(task_id)
        })
    }

    /// Get completion stats
    pub async fn get_stats(&self) -> SchedulerStats {
        let completed = self.completed.read().await;
        let failed = self.failed.read().await;
        let skipped = self.skipped.read().await;
        let running = self.running.read().await;

        SchedulerStats {
            total_tasks: self.total_tasks,
            completed: completed.len(),
            failed: failed.len(),
            skipped: skipped.len(),
            running: running.len(),
            pending: self.total_tasks
                .saturating_sub(completed.len() + failed.len() + skipped.len() + running.len()),
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

    /// Get skipped tasks
    pub async fn get_skipped_tasks(&self) -> HashSet<TaskId> {
        self.skipped.read().await.clone()
    }

    /// Reset a task for retry (move from failed back to ready)
    pub async fn retry_task(&self, task_id: &TaskId) -> Result<bool> {
        // Only retry failed tasks
        {
            let mut failed = self.failed.write().await;
            if failed.remove(task_id).is_none() {
                return Ok(false);
            }
        }

        // Reset the dependency counter for this task
        if let Some(task) = self.task_graph.get(task_id) {
            if let Some(counter) = self.dep_counter.get(task_id) {
                // Check if all deps are satisfied
                let completed = self.completed.read().await;
                let unsatisfied = task.deps.iter().filter(|d| !completed.contains(*d)).count();
                counter.store(unsatisfied, Ordering::SeqCst);

                if unsatisfied == 0 {
                    let sandbox_name = self.sandbox_hints.get(task_id)
                        .cloned()
                        .unwrap_or_else(|| "default".to_string());
                    let mut queues = self.ready_queues.write().await;
                    queues
                        .entry(sandbox_name)
                        .or_insert_with(VecDeque::new)
                        .push_back(task_id.clone());
                }
            }
        }

        Ok(true)
    }
}

/// Scheduler statistics
#[derive(Debug, Clone)]
pub struct SchedulerStats {
    pub total_tasks: usize,
    pub completed: usize,
    pub failed: usize,
    pub skipped: usize,
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

    #[tokio::test]
    async fn test_failed_task_skips_dependents() {
        let tasks = vec![
            Task {
                id: "t1".to_string(),
                description: "Root task".to_string(),
                deps: vec![],
                hint: Some("sb1".to_string()),
            },
            Task {
                id: "t2".to_string(),
                description: "Depends on t1".to_string(),
                deps: vec!["t1".to_string()],
                hint: Some("sb1".to_string()),
            },
            Task {
                id: "t3".to_string(),
                description: "Depends on t2".to_string(),
                deps: vec!["t2".to_string()],
                hint: Some("sb1".to_string()),
            },
            Task {
                id: "t4".to_string(),
                description: "Independent task".to_string(),
                deps: vec![],
                hint: Some("sb1".to_string()),
            },
        ];

        let task_graph = TaskGraph { tasks };
        let mut hints = HashMap::new();
        for id in &["t1", "t2", "t3", "t4"] {
            hints.insert(id.to_string(), "sb1".to_string());
        }

        let scheduler = Scheduler::new(task_graph, hints).unwrap();

        // t1 and t4 should be ready
        let t1 = scheduler.get_ready_task(&"sb1".to_string()).await;
        assert_eq!(t1, Some("t1".to_string()));
        let t4 = scheduler.get_ready_task(&"sb1".to_string()).await;
        assert_eq!(t4, Some("t4".to_string()));

        // Fail t1
        let result = AgentResult {
            task_id: "t1".to_string(),
            success: false,
            git_diff: None,
            error: Some("something broke".to_string()),
            checkpoint_log: vec![],
            timestamp: 0,
        };
        scheduler.handle_task_completion(result).await.unwrap();

        // t2 and t3 should be skipped (not in ready queue)
        let nothing = scheduler.get_ready_task(&"sb1".to_string()).await;
        assert_eq!(nothing, None);

        let stats = scheduler.get_stats().await;
        assert_eq!(stats.failed, 1);  // t1
        assert_eq!(stats.skipped, 2); // t2, t3
        assert!(scheduler.is_complete().await); // all 4 resolved
    }

    #[tokio::test]
    async fn test_retry_failed_task() {
        let tasks = vec![
            Task {
                id: "t1".to_string(),
                description: "Retryable task".to_string(),
                deps: vec![],
                hint: Some("sb1".to_string()),
            },
        ];

        let task_graph = TaskGraph { tasks };
        let mut hints = HashMap::new();
        hints.insert("t1".to_string(), "sb1".to_string());

        let scheduler = Scheduler::new(task_graph, hints).unwrap();

        // Pop t1
        scheduler.get_ready_task(&"sb1".to_string()).await;

        // Fail t1
        let result = AgentResult {
            task_id: "t1".to_string(),
            success: false,
            git_diff: None,
            error: Some("fail".to_string()),
            checkpoint_log: vec![],
            timestamp: 0,
        };
        scheduler.handle_task_completion(result).await.unwrap();
        assert_eq!(scheduler.get_stats().await.failed, 1);

        // Retry
        let ok = scheduler.retry_task(&"t1".to_string()).await.unwrap();
        assert!(ok);
        assert_eq!(scheduler.get_stats().await.failed, 0);

        // Should be back in queue
        let ready = scheduler.get_ready_task(&"sb1".to_string()).await;
        assert_eq!(ready, Some("t1".to_string()));
    }
}
