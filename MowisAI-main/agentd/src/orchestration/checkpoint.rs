//! Layer 4: Checkpoint system — Save/restore agent state after every tool call
//!
//! IMPORTANT: Checkpoint operations are delegated to agentd via socket API
//! because agentd runs as root (required for overlayfs mounts/chroot) and
//! can access root-owned files in the container's upper layer.
//! The orchestrator runs as a non-root user, so direct file access would fail.

use agentd_protocol::Checkpoint;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

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

/// Checkpoint manager that delegates to agentd via socket API
/// 
/// The agentd socket server runs as root (required for overlayfs/chroot)
/// and can access the root-owned files in container upper directories.
/// The orchestrator (running as a regular user) cannot access these files
/// directly, so we delegate all checkpoint operations to agentd.
pub struct CheckpointManager {
    checkpoint_root: PathBuf,
    socket_path: String,
}

impl CheckpointManager {
    pub fn new(checkpoint_root: PathBuf, socket_path: String) -> Result<Self> {
        std::fs::create_dir_all(&checkpoint_root)
            .context("Failed to create checkpoint root directory")?;

        Ok(Self {
            checkpoint_root,
            socket_path,
        })
    }

    /// Create checkpoint snapshot of agent's upper dir
    /// 
    /// This delegates to agentd via socket API because agentd runs as root
    /// and can access the root-owned files in the container's upper layer.
    pub fn create_snapshot(
        &self,
        agent_id: &str,
        checkpoint_id: u64,
        sandbox_id: &str,
        container_id: &str,
    ) -> Result<PathBuf> {
        let snapshot_dir = self
            .checkpoint_root
            .join(agent_id)
            .join(format!("checkpoint-{}", checkpoint_id));

        // Create the parent directory first (this runs as user, that's fine)
        std::fs::create_dir_all(&snapshot_dir)
            .context("Failed to create snapshot directory")?;

        // Delegate the actual snapshot to agentd via socket
        let request = json!({
            "request_type": "create_checkpoint",
            "sandbox": sandbox_id,
            "container": container_id,
            "checkpoint_dir": snapshot_dir.to_string_lossy().to_string()
        });

        let response = super::socket_roundtrip(&self.socket_path, &request)
            .context("Failed to call create_checkpoint via socket")?;

        if response.get("status").and_then(|s| s.as_str()) != Some("ok") {
            let error = response
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("checkpoint failed");
            return Err(anyhow!("Checkpoint snapshot failed: {}", error));
        }

        Ok(snapshot_dir)
    }

    /// Restore agent's upper dir from checkpoint snapshot
    /// 
    /// This delegates to agentd via socket API because agentd runs as root
    /// and can modify the root-owned files in the container's upper layer.
    pub fn restore_snapshot(
        &self,
        sandbox_id: &str,
        container_id: &str,
        snapshot_path: &Path,
    ) -> Result<()> {
        if !snapshot_path.exists() {
            return Err(anyhow!(
                "Snapshot path does not exist: {}",
                snapshot_path.display()
            ));
        }

        // Delegate the restore to agentd via socket
        let request = json!({
            "request_type": "restore_checkpoint",
            "sandbox": sandbox_id,
            "container": container_id,
            "checkpoint_dir": snapshot_path.to_string_lossy().to_string()
        });

        let response = super::socket_roundtrip(&self.socket_path, &request)
            .context("Failed to call restore_checkpoint via socket")?;

        if response.get("status").and_then(|s| s.as_str()) != Some("ok") {
            let error = response
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("restore failed");
            return Err(anyhow!("Checkpoint restore failed: {}", error));
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
    fn test_checkpoint_manager_paths() {
        let temp_dir = std::env::temp_dir().join("test_checkpoint_manager_paths");
        let manager = CheckpointManager::new(temp_dir.clone(), "/tmp/test.sock".to_string()).unwrap();

        let checkpoint_dir = manager.get_checkpoint_dir("agent-123");
        assert!(checkpoint_dir.to_string_lossy().contains("agent-123"));

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
