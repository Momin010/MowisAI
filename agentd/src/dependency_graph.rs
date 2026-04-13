/// Dependency Graph Analysis and Task Planning
///
/// Analyzes task dependencies to determine parallelization strategy
/// and creates an execution plan for the Global Orchestrator.

use crate::protocol::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// Errors in dependency graph analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphError {
    CyclicDependency(String),
    MissingDependency(String),
    InvalidTask(String),
}

pub type GraphResult<T> = Result<T, GraphError>;

/// Dependency graph analyzer
pub struct DependencyGraphBuilder {
    tasks: HashMap<String, TaskNode>,
}

impl DependencyGraphBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        DependencyGraphBuilder {
            tasks: HashMap::new(),
        }
    }

    /// Add a task to the graph
    pub fn add_task(&mut self, task_id: String, team_type: String, dependencies: Vec<String>) {
        self.tasks.insert(
            task_id.clone(),
            TaskNode {
                task_id,
                depends_on: dependencies,
                team_type,
            },
        );
    }

    /// Build the dependency graph and return execution stages
    pub fn build(self) -> GraphResult<DependencyGraph> {
        // Check for cycles
        self.check_cycles()?;

        // Validate all dependencies exist
        self.validate_dependencies()?;

        // Compute execution stages using topological sort
        let execution_order = self.compute_execution_stages()?;

        Ok(DependencyGraph {
            tasks: self.tasks,
            execution_order,
        })
    }

    /// Check for cyclic dependencies using DFS
    fn check_cycles(&self) -> GraphResult<()> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for task_id in self.tasks.keys() {
            if !visited.contains(task_id) {
                self.dfs_cycle_check(task_id, &mut visited, &mut rec_stack)?;
            }
        }

        Ok(())
    }

    /// Recursive DFS for cycle detection
    fn dfs_cycle_check(
        &self,
        task_id: &str,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
    ) -> GraphResult<()> {
        visited.insert(task_id.to_string());
        rec_stack.insert(task_id.to_string());

        if let Some(task) = self.tasks.get(task_id) {
            for dep in &task.depends_on {
                if !visited.contains(dep) {
                    self.dfs_cycle_check(dep, visited, rec_stack)?;
                } else if rec_stack.contains(dep) {
                    return Err(GraphError::CyclicDependency(format!(
                        "Cycle detected: {} -> {}",
                        task_id, dep
                    )));
                }
            }
        }

        rec_stack.remove(task_id);
        Ok(())
    }

    /// Verify all dependencies exist
    fn validate_dependencies(&self) -> GraphResult<()> {
        for task in self.tasks.values() {
            for dep in &task.depends_on {
                if !self.tasks.contains_key(dep) {
                    return Err(GraphError::MissingDependency(format!(
                        "Task {} depends on missing task {}",
                        task.task_id, dep
                    )));
                }
            }
        }
        Ok(())
    }

    /// Compute execution stages using topological sort (Kahn's algorithm)
    fn compute_execution_stages(&self) -> GraphResult<Vec<Vec<String>>> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();

        // Initialize in-degrees and adjacency list
        for task_id in self.tasks.keys() {
            in_degree.insert(task_id.clone(), 0);
            graph.insert(task_id.clone(), Vec::new());
        }

        // Build reverse dependency graph
        for task in self.tasks.values() {
            for dep in &task.depends_on {
                graph
                    .get_mut(dep)
                    .unwrap()
                    .push(task.task_id.clone());
                *in_degree.get_mut(&task.task_id).unwrap() += 1;
            }
        }

        // Kahn's algorithm
        let mut queue: VecDeque<String> = in_degree
            .iter()
            .filter(|(_, &degree)| degree == 0)
            .map(|(id, _)| id.clone())
            .collect();

        let mut stages = Vec::new();

        while !queue.is_empty() {
            let stage_size = queue.len();
            let mut current_stage = Vec::new();

            for _ in 0..stage_size {
                let task_id = queue.pop_front().unwrap();
                current_stage.push(task_id.clone());

                for dependent in graph.get(&task_id).unwrap() {
                    let degree = in_degree.get_mut(dependent).unwrap();
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(dependent.clone());
                    }
                }
            }

            stages.push(current_stage);
        }

        // Verify all tasks were processed
        if stages.iter().flat_map(|s| s.iter()).count() != self.tasks.len() {
            return Err(GraphError::CyclicDependency(
                "Unable to schedule all tasks".to_string(),
            ));
        }

        Ok(stages)
    }
}

/// Plan complexity analyzer for resource estimation
pub struct ComplexityAnalyzer;

impl ComplexityAnalyzer {
    /// Estimate number of sandboxes needed based on task complexity
    pub fn estimate_sandbox_count(tasks: &[TaskNode], complexity_sum: u32) -> usize {
        // Simple heuristic: one sandbox per 100 complexity units, min 1, max 10
        let base_count = (complexity_sum as usize / 100).max(1).min(10);
        
        // Consider unique team types
        let team_types: HashSet<_> = tasks.iter().map(|t| &t.team_type).collect();
        
        // Allocate at least one sandbox per team type
        base_count.max(team_types.len())
    }

    /// Estimate sandboxes per team type
    pub fn allocate_sandboxes_by_team(
        tasks: &[TaskNode],
        total_sandboxes: usize,
    ) -> HashMap<String, usize> {
        let mut team_task_count: HashMap<String, usize> = HashMap::new();
        for task in tasks {
            *team_task_count.entry(task.team_type.clone()).or_insert(0) += 1;
        }

        let mut allocation = HashMap::new();
        let total_tasks = tasks.len();

        for (team_type, count) in team_task_count {
            let sandboxes_for_team = (total_sandboxes * count / total_tasks).max(1);
            allocation.insert(team_type, sandboxes_for_team);
        }

        allocation
    }

    /// Estimate RAM needed per sandbox
    pub fn estimate_ram_per_sandbox(complexity: u32) -> u64 {
        // Base: 512 MB + 10 MB per complexity unit
        512 * 1_000_000 + (complexity as u64) * 10_000_000
    }

    /// Estimate CPU millis per sandbox
    pub fn estimate_cpu_per_sandbox(complexity: u32) -> u32 {
        // Base: 1000 millis + 10 per complexity unit
        1000 + complexity * 10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_dependency_graph() {
        let mut builder = DependencyGraphBuilder::new();
        builder.add_task("A".to_string(), "backend".to_string(), vec![]);
        builder.add_task(
            "B".to_string(),
            "frontend".to_string(),
            vec!["A".to_string()],
        );
        builder.add_task(
            "C".to_string(),
            "backend".to_string(),
            vec!["A".to_string()],
        );

        let graph = builder.build().unwrap();
        assert_eq!(graph.execution_order.len(), 2);
        assert_eq!(graph.execution_order[0], vec!["A".to_string()]);
        assert!(graph.execution_order[1].contains(&"B".to_string()));
        assert!(graph.execution_order[1].contains(&"C".to_string()));
    }

    #[test]
    fn test_cyclic_dependency_detection() {
        let mut builder = DependencyGraphBuilder::new();
        builder.add_task("A".to_string(), "backend".to_string(), vec!["B".to_string()]);
        builder.add_task("B".to_string(), "frontend".to_string(), vec!["A".to_string()]);

        let result = builder.build();
        assert!(result.is_err());
    }

    #[test]
    fn test_complexity_analyzer() {
        let tasks = vec![
            TaskNode {
                task_id: "1".to_string(),
                depends_on: vec![],
                team_type: "backend".to_string(),
            },
            TaskNode {
                task_id: "2".to_string(),
                depends_on: vec![],
                team_type: "frontend".to_string(),
            },
        ];

        let count = ComplexityAnalyzer::estimate_sandbox_count(&tasks, 150);
        assert!(count >= 2); // at least one per team type
    }
}
