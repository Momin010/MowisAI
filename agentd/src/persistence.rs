use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Persisted sandbox configuration and metadata
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedSandbox {
    pub id: u64,
    pub created_at: u64,
    pub last_updated: u64,
    pub ram_bytes: Option<u64>,
    pub cpu_millis: Option<u64>,
    pub root_path: String,
    pub metadata: HashMap<String, Value>,
    pub tools_registered: Vec<String>,
    pub state_summary: String,
}

/// State persistence manager for sandbox and agent state
pub struct PersistenceManager {
    base_path: PathBuf,
}

impl PersistenceManager {
    pub fn new(base_path: &Path) -> Self {
        PersistenceManager {
            base_path: base_path.to_path_buf(),
        }
    }

    /// Ensure persistence directory exists
    pub fn init(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.base_path)?;
        fs::create_dir_all(self.base_path.join("sandboxes"))?;
        fs::create_dir_all(self.base_path.join("agents"))?;
        fs::create_dir_all(self.base_path.join("memory"))?;
        Ok(())
    }

    /// Save sandbox state to disk
    pub fn save_sandbox(&self, sandbox: &PersistedSandbox) -> anyhow::Result<()> {
        let path = self
            .base_path
            .join("sandboxes")
            .join(format!("sandbox_{}.json", sandbox.id));
        let json = serde_json::to_string_pretty(sandbox)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Load sandbox state from disk
    pub fn load_sandbox(&self, sandbox_id: u64) -> anyhow::Result<PersistedSandbox> {
        let path = self
            .base_path
            .join("sandboxes")
            .join(format!("sandbox_{}.json", sandbox_id));
        let json = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }

    /// List all saved sandboxes
    pub fn list_sandboxes(&self) -> anyhow::Result<Vec<u64>> {
        let dir = self.base_path.join("sandboxes");
        let mut ids = vec![];
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".json") && name.starts_with("sandbox_") {
                    if let Some(id_str) = name
                        .strip_prefix("sandbox_")
                        .and_then(|s| s.strip_suffix(".json").map(|s| s.to_string()))
                    {
                        if let Ok(id) = id_str.parse::<u64>() {
                            ids.push(id);
                        }
                    }
                }
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// Delete sandbox state from disk
    pub fn delete_sandbox(&self, sandbox_id: u64) -> anyhow::Result<()> {
        let path = self
            .base_path
            .join("sandboxes")
            .join(format!("sandbox_{}.json", sandbox_id));
        fs::remove_file(path)?;
        Ok(())
    }

    /// Save agent state to disk
    pub fn save_agent_memory(&self, agent_id: u64, memory_json: &Value) -> anyhow::Result<()> {
        let path = self
            .base_path
            .join("memory")
            .join(format!("agent_{}.json", agent_id));
        let json = serde_json::to_string_pretty(memory_json)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Load agent memory from disk
    pub fn load_agent_memory(&self, agent_id: u64) -> anyhow::Result<Value> {
        let path = self
            .base_path
            .join("memory")
            .join(format!("agent_{}.json", agent_id));
        let json = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }

    /// Check if sandbox exists on disk
    pub fn sandbox_exists(&self, sandbox_id: u64) -> bool {
        let path = self
            .base_path
            .join("sandboxes")
            .join(format!("sandbox_{}.json", sandbox_id));
        path.exists()
    }

    /// Check if agent memory exists on disk
    pub fn agent_exists(&self, agent_id: u64) -> bool {
        let path = self
            .base_path
            .join("memory")
            .join(format!("agent_{}.json", agent_id));
        path.exists()
    }
}

/// Checkpoint system for periodic state snapshots
pub struct Checkpointer {
    persistence: PersistenceManager,
    checkpoint_interval: usize,
}

impl Checkpointer {
    pub fn new(base_path: &Path, checkpoint_interval: usize) -> Self {
        Checkpointer {
            persistence: PersistenceManager::new(base_path),
            checkpoint_interval,
        }
    }

    pub fn init(&self) -> anyhow::Result<()> {
        self.persistence.init()?;
        fs::create_dir_all(self.persistence.base_path.join("checkpoints"))?;
        Ok(())
    }

    pub fn save_checkpoint(&self, checkpoint_id: String, data: &Value) -> anyhow::Result<()> {
        let path = self
            .persistence
            .base_path
            .join("checkpoints")
            .join(format!("checkpoint_{}.json", checkpoint_id));
        let json = serde_json::to_string_pretty(data)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load_checkpoint(&self, checkpoint_id: &str) -> anyhow::Result<Value> {
        let path = self
            .persistence
            .base_path
            .join("checkpoints")
            .join(format!("checkpoint_{}.json", checkpoint_id));
        let json = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn list_checkpoints(&self) -> anyhow::Result<Vec<String>> {
        let dir = self.persistence.base_path.join("checkpoints");
        let mut ids = vec![];
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".json") && name.starts_with("checkpoint_") {
                    if let Some(id) = name
                        .strip_prefix("checkpoint_")
                        .and_then(|s| s.strip_suffix(".json"))
                    {
                        ids.push(id.to_string());
                    }
                }
            }
        }
        ids.sort();
        Ok(ids)
    }
}

/// Write-ahead logging for durability
pub struct WriteAheadLog {
    log_path: PathBuf,
}

impl WriteAheadLog {
    pub fn new(base_path: &Path) -> Self {
        let log_path = base_path.join("wal.log");
        WriteAheadLog { log_path }
    }

    pub fn init(&self) -> anyhow::Result<()> {
        if !self.log_path.exists() {
            fs::write(&self.log_path, "")?;
        }
        Ok(())
    }

    pub fn append(&self, entry: &Value) -> anyhow::Result<()> {
        let line = serde_json::to_string(entry)? + "\n";
        let mut content = fs::read_to_string(&self.log_path).unwrap_or_default();
        content.push_str(&line);
        fs::write(&self.log_path, content)?;
        Ok(())
    }

    pub fn read_all(&self) -> anyhow::Result<Vec<Value>> {
        if !self.log_path.exists() {
            return Ok(vec![]);
        }
        let content = fs::read_to_string(&self.log_path)?;
        let mut entries = vec![];
        for line in content.lines() {
            if !line.is_empty() {
                if let Ok(entry) = serde_json::from_str::<Value>(line) {
                    entries.push(entry);
                }
            }
        }
        Ok(entries)
    }

    pub fn clear(&self) -> anyhow::Result<()> {
        fs::write(&self.log_path, "")?;
        Ok(())
    }
}

/// Recovery journal for crash recovery
pub struct RecoveryJournal {
    journal_path: PathBuf,
}

impl RecoveryJournal {
    pub fn new(base_path: &Path) -> Self {
        RecoveryJournal {
            journal_path: base_path.join("recovery.json"),
        }
    }

    pub fn init(&self) -> anyhow::Result<()> {
        if !self.journal_path.exists() {
            let empty = json!({
                "last_checkpoint": null,
                "pending_operations": [],
                "version": 1
            });
            fs::write(&self.journal_path, serde_json::to_string_pretty(&empty)?)?;
        }
        Ok(())
    }

    pub fn mark_checkpoint(&self, checkpoint_id: &str) -> anyhow::Result<()> {
        let mut journal: Value = serde_json::from_str(&fs::read_to_string(&self.journal_path)?)?;
        if let Some(obj) = journal.as_object_mut() {
            obj.insert(
                "last_checkpoint".to_string(),
                Value::String(checkpoint_id.to_string()),
            );
        }
        fs::write(&self.journal_path, serde_json::to_string_pretty(&journal)?)?;
        Ok(())
    }

    pub fn add_pending_operation(&self, op: &Value) -> anyhow::Result<()> {
        let mut journal: Value = serde_json::from_str(&fs::read_to_string(&self.journal_path)?)?;
        if let Some(obj) = journal.as_object_mut() {
            if let Some(ops) = obj
                .get_mut("pending_operations")
                .and_then(|v| v.as_array_mut())
            {
                ops.push(op.clone());
            }
        }
        fs::write(&self.journal_path, serde_json::to_string_pretty(&journal)?)?;
        Ok(())
    }

    pub fn get_pending_operations(&self) -> anyhow::Result<Vec<Value>> {
        let journal: Value = serde_json::from_str(&fs::read_to_string(&self.journal_path)?)?;
        Ok(journal["pending_operations"]
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    pub fn clear_pending_operations(&self) -> anyhow::Result<()> {
        let mut journal: Value = serde_json::from_str(&fs::read_to_string(&self.journal_path)?)?;
        if let Some(obj) = journal.as_object_mut() {
            obj.insert("pending_operations".to_string(), Value::Array(vec![]));
        }
        fs::write(&self.journal_path, serde_json::to_string_pretty(&journal)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_persistence_init() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let pm = PersistenceManager::new(dir.path());
        pm.init()?;
        assert!(dir.path().join("sandboxes").exists());
        Ok(())
    }

    #[test]
    fn test_persistence_save_load() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let pm = PersistenceManager::new(dir.path());
        pm.init()?;

        let sandbox = PersistedSandbox {
            id: 42,
            created_at: 0,
            last_updated: 0,
            ram_bytes: Some(1024),
            cpu_millis: Some(500),
            root_path: "/tmp".to_string(),
            metadata: HashMap::new(),
            tools_registered: vec!["echo".to_string()],
            state_summary: "test".to_string(),
        };

        pm.save_sandbox(&sandbox)?;
        let loaded = pm.load_sandbox(42)?;
        assert_eq!(loaded.id, 42);
        Ok(())
    }
}
