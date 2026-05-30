use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::events::{Event, EventBus};
use crate::plan::{PlanId, TaskId};

#[derive(Debug)]
pub struct Merger {
    plan_id: PlanId,
    bus: EventBus,
    /// agent_id -> list of changed paths from their overlay
    agent_changes: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeResult {
    pub success: bool,
    pub merged_files: Vec<String>,
    pub conflicts: Vec<MergeConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeConflict {
    pub path: String,
    pub agent_a: String,
    pub agent_b: String,
    pub resolution: Option<String>,
}

impl Merger {
    pub fn new(plan_id: PlanId, bus: EventBus) -> Self {
        Self {
            plan_id,
            bus,
            agent_changes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Record that an agent completed with these changed paths.
    pub async fn record_agent_completion(&self, agent_id: String, changed_paths: Vec<String>) {
        let mut changes = self.agent_changes.lock().await;
        changes.insert(agent_id, changed_paths);
    }

    /// Merge all completed agent overlays into the base.
    /// Returns the list of all merged file paths.
    pub async fn merge_all(&self, base_path: &Path) -> Result<MergeResult> {
        let changes = self.agent_changes.lock().await;
        let mut all_files: Vec<String> = Vec::new();
        let mut conflicts: Vec<MergeConflict> = Vec::new();

        // Collect all changed files across agents
        let mut file_agents: HashMap<String, Vec<String>> = HashMap::new();
        for (agent_id, paths) in changes.iter() {
            for path in paths {
                file_agents
                    .entry(path.clone())
                    .or_default()
                    .push(agent_id.clone());
            }
        }

        // Check for conflicts (same file changed by multiple agents)
        for (path, agents) in &file_agents {
            if agents.len() > 1 {
                conflicts.push(MergeConflict {
                    path: path.clone(),
                    agent_a: agents[0].clone(),
                    agent_b: agents[1].clone(),
                    resolution: None,
                });
            }
            all_files.push(path.clone());
        }

        if conflicts.is_empty() {
            tracing::info!(files = all_files.len(), "merge completed cleanly");
        } else {
            tracing::warn!(
                conflicts = conflicts.len(),
                "merge has conflicts that need resolution"
            );
        }

        Ok(MergeResult {
            success: conflicts.is_empty(),
            merged_files: all_files,
            conflicts,
        })
    }

    /// Promote a single agent's upper dir changes to the base.
    /// This is called after the agent's work is validated.
    pub async fn promote_agent(
        &self,
        agent_upper_dir: &Path,
        base_dir: &Path,
        agent_id: &str,
    ) -> Result<Vec<String>> {
        let mut changed = Vec::new();
        copy_dir_recursive(agent_upper_dir, base_dir, &mut changed)?;

        self.bus.emit(Event::MergeCompleted {
            plan_id: self.plan_id.clone(),
            agent_id: agent_id.to_string(),
        });

        tracing::info!(
            agent = agent_id,
            files = changed.len(),
            "agent overlay promoted to base"
        );
        Ok(changed)
    }
}

fn copy_dir_recursive(src: &Path, dst_base: &Path, changed: &mut Vec<String>) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let rel = src_path.strip_prefix(src).unwrap();
        let dst_path = dst_base.join(rel);

        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            copy_dir_recursive(&src_path, dst_base, changed)?;
        } else {
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src_path, &dst_path)?;
            changed.push(rel.to_string_lossy().to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_merge_no_conflicts() {
        let bus = EventBus::new();
        let plan_id = PlanId("test".into());
        let merger = Merger::new(plan_id, bus);

        merger
            .record_agent_completion(
                "ag-1".into(),
                vec!["src/main.rs".into(), "Cargo.toml".into()],
            )
            .await;
        merger
            .record_agent_completion(
                "ag-2".into(),
                vec!["src/lib.rs".into(), "tests/test.rs".into()],
            )
            .await;

        let result = merger.merge_all(Path::new("/tmp")).await.unwrap();
        assert!(result.success);
        assert_eq!(result.merged_files.len(), 4);
        assert!(result.conflicts.is_empty());
    }

    #[tokio::test]
    async fn test_merge_with_conflicts() {
        let bus = EventBus::new();
        let plan_id = PlanId("test".into());
        let merger = Merger::new(plan_id, bus);

        merger
            .record_agent_completion(
                "ag-1".into(),
                vec!["src/main.rs".into(), "src/lib.rs".into()],
            )
            .await;
        merger
            .record_agent_completion(
                "ag-2".into(),
                vec!["src/main.rs".into(), "src/utils.rs".into()],
            )
            .await;

        let result = merger.merge_all(Path::new("/tmp")).await.unwrap();
        assert!(!result.success);
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(result.conflicts[0].path, "src/main.rs");
    }
}
