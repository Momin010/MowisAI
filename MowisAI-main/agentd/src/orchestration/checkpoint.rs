//! Layer 4: Checkpoint system — Save/restore agent state after every tool call

use agentd_protocol::Checkpoint;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Checkpoint log containing all checkpoints for an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointLog {
    pub agent_id: String,
    pub task_id: String,
    pub checkpoints: Vec<Checkpoint>,
    pub log_path: PathBuf,
}

impl CheckpointLog {
    /// Create new checkpoint log
    pub fn new(agent_id: String, task_id: String, log_dir: &Path) -> Result<Self> {
        let log_path = log_dir.join(format!("checkpoint-{}.json", agent_id));

        Ok(Self {
            agent_id,
            task_id,
            checkpoints: Vec::new(),
            log_path,
        })
    }

    /// Load checkpoint log from file
    pub fn load(log_path: &Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(log_path).context("Failed to read checkpoint log")?;
        serde_json::from_str(&content).context("Failed to parse checkpoint log")
    }

    /// Save checkpoint log to file
    pub fn save(&self) -> Result<()> {
        let content = serde_json::to_string_pretty(self).context("Failed to serialize log")?;
        std::fs::write(&self.log_path, content).context("Failed to write checkpoint log")
    }

    /// Add new checkpoint
    pub fn add_checkpoint(&mut self, checkpoint: Checkpoint) -> Result<()> {
        self.checkpoints.push(checkpoint);
        self.save()
    }

    /// Get latest checkpoint
    pub fn latest(&self) -> Option<&Checkpoint> {
        self.checkpoints.last()
    }

    /// Get checkpoint by ID
    pub fn get(&self, id: u64) -> Option<&Checkpoint> {
        self.checkpoints.iter().find(|c| c.id == id)
    }

    /// Prune old checkpoints (keep last N)
    pub fn prune(&mut self, keep_last: usize) -> Result<()> {
        if self.checkpoints.len() > keep_last {
            let to_remove = self.checkpoints.len() - keep_last;
            let removed = self.checkpoints.drain(..to_remove).collect::<Vec<_>>();

            // Delete old checkpoint snapshots
            for checkpoint in removed {
                let snapshot_path = PathBuf::from(&checkpoint.layer_snapshot_path);
                if snapshot_path.exists() {
                    std::fs::remove_dir_all(&snapshot_path)
                        .context("Failed to remove old checkpoint snapshot")?;
                }
            }

            self.save()?;
        }

        Ok(())
    }
}

/// Checkpoint manager for creating/restoring snapshots
pub struct CheckpointManager {
    checkpoint_root: PathBuf,
}

impl CheckpointManager {
    pub fn new(checkpoint_root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&checkpoint_root)
            .context("Failed to create checkpoint root directory")?;

        Ok(Self { checkpoint_root })
    }

    /// Create checkpoint snapshot of agent's upper dir
    pub fn create_snapshot(
        &self,
        agent_id: &str,
        checkpoint_id: u64,
        upper_dir: &Path,
    ) -> Result<PathBuf> {
        let snapshot_dir = self
            .checkpoint_root
            .join(agent_id)
            .join(format!("checkpoint-{}", checkpoint_id));

        std::fs::create_dir_all(&snapshot_dir)
            .context("Failed to create snapshot directory")?;

        #[cfg(target_os = "linux")]
        {
            // Use cp -al for hard-link copy (fast, low disk usage)
            let cp_result = Command::new("cp")
                .arg("-al")
                .arg(upper_dir)
                .arg(&snapshot_dir)
                .output()
                .context("Failed to execute cp command")?;

            if !cp_result.status.success() {
                return Err(anyhow!(
                    "Checkpoint snapshot failed: {}",
                    String::from_utf8_lossy(&cp_result.stderr)
                ));
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Windows fallback - copy directory
            copy_dir_recursive(upper_dir, &snapshot_dir)?;
        }

        Ok(snapshot_dir)
    }

    /// Restore agent's upper dir from checkpoint snapshot
    pub fn restore_snapshot(&self, upper_dir: &Path, snapshot_path: &Path) -> Result<()> {
        // Remove current upper dir contents
        if upper_dir.exists() {
            std::fs::remove_dir_all(upper_dir).context("Failed to remove current upper dir")?;
        }
        std::fs::create_dir_all(upper_dir).context("Failed to recreate upper dir")?;

        #[cfg(target_os = "linux")]
        {
            // Restore using cp -al
            let restore_result = Command::new("cp")
                .arg("-al")
                .arg(format!("{}/*", snapshot_path.display()))
                .arg(upper_dir)
                .output()
                .context("Failed to execute cp command")?;

            if !restore_result.status.success() {
                return Err(anyhow!(
                    "Checkpoint restore failed: {}",
                    String::from_utf8_lossy(&restore_result.stderr)
                ));
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Windows fallback - copy directory
            copy_dir_recursive(snapshot_path, upper_dir)?;
        }

        Ok(())
    }

    /// Get checkpoint directory for agent
    pub fn get_checkpoint_dir(&self, agent_id: &str) -> PathBuf {
        self.checkpoint_root.join(agent_id)
    }

    /// Clean up all checkpoints for agent
    pub fn cleanup_agent_checkpoints(&self, agent_id: &str) -> Result<()> {
        let agent_dir = self.get_checkpoint_dir(agent_id);
        if agent_dir.exists() {
            std::fs::remove_dir_all(&agent_dir)
                .context("Failed to remove agent checkpoint directory")?;
        }
        Ok(())
    }

    /// Delete all snapshots immediately after task success (memory leak fix)
    pub fn cleanup_all_snapshots(&self, checkpoint_log: &CheckpointLog) -> Result<()> {
        for checkpoint in &checkpoint_log.checkpoints {
            let snapshot_path = PathBuf::from(&checkpoint.layer_snapshot_path);
            if snapshot_path.exists() {
                std::fs::remove_dir_all(&snapshot_path)
                    .context("Failed to remove checkpoint snapshot")?;
            }
        }
        Ok(())
    }
}

/// Recursive directory copy (Windows fallback)
#[cfg(not(target_os = "linux"))]
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_log() {
        let temp_dir = std::env::temp_dir().join("test_checkpoint_log");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut log = CheckpointLog::new(
            "agent-123".to_string(),
            "task-456".to_string(),
            &temp_dir,
        )
        .unwrap();

        let checkpoint = Checkpoint {
            id: 0,
            tool_call: "write_file".to_string(),
            tool_args: serde_json::json!({"path": "test.txt"}),
            tool_result: "success".to_string(),
            timestamp: 1234567890,
            layer_snapshot_path: "/tmp/snapshot".to_string(),
        };

        log.add_checkpoint(checkpoint).unwrap();
        assert_eq!(log.checkpoints.len(), 1);

        // Save and reload
        log.save().unwrap();
        let loaded = CheckpointLog::load(&log.log_path).unwrap();
        assert_eq!(loaded.checkpoints.len(), 1);

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_checkpoint_manager() {
        let temp_dir = std::env::temp_dir().join("test_checkpoint_manager");
        let manager = CheckpointManager::new(temp_dir.clone()).unwrap();

        let checkpoint_dir = manager.get_checkpoint_dir("agent-123");
        assert!(checkpoint_dir.to_string_lossy().contains("agent-123"));

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
