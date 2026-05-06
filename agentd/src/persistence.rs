use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
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

    pub fn init(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.base_path)?;
        fs::create_dir_all(self.base_path.join("sandboxes"))?;
        fs::create_dir_all(self.base_path.join("agents"))?;
        fs::create_dir_all(self.base_path.join("memory"))?;
        Ok(())
    }

    /// Atomic write: write to temp file then rename
    pub fn atomic_write(path: &Path, content: &[u8]) -> anyhow::Result<()> {
        let tmp_path = path.with_extension("tmp");
        {
            let mut f = fs::File::create(&tmp_path)?;
            f.write_all(content)?;
            f.sync_all()?;
        }
        fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Save sandbox state to disk (atomically)
    pub fn save_sandbox(&self, sandbox: &PersistedSandbox) -> anyhow::Result<()> {
        let path = self
            .base_path
            .join("sandboxes")
            .join(format!("sandbox_{}.json", sandbox.id));
        let json = serde_json::to_string_pretty(sandbox)?;
        Self::atomic_write(&path, json.as_bytes())?;
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

    /// Save agent state to disk (atomically)
    pub fn save_agent_memory(&self, agent_id: u64, memory_json: &Value) -> anyhow::Result<()> {
        let path = self
            .base_path
            .join("memory")
            .join(format!("agent_{}.json", agent_id));
        let json = serde_json::to_string_pretty(memory_json)?;
        Self::atomic_write(&path, json.as_bytes())?;
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

    pub fn sandbox_exists(&self, sandbox_id: u64) -> bool {
        let path = self
            .base_path
            .join("sandboxes")
            .join(format!("sandbox_{}.json", sandbox_id));
        path.exists()
    }

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
}

impl Checkpointer {
    pub fn new(base_path: &Path) -> Self {
        Checkpointer {
            persistence: PersistenceManager::new(base_path),
        }
    }

    pub fn init(&self) -> anyhow::Result<()> {
        self.persistence.init()?;
        fs::create_dir_all(self.persistence.base_path.join("checkpoints"))?;
        Ok(())
    }

    /// Save checkpoint with sanitized ID (prevent path traversal)
    pub fn save_checkpoint(&self, checkpoint_id: &str, data: &Value) -> anyhow::Result<()> {
        // Sanitize checkpoint_id: only allow alphanumeric, dash, underscore
        let sanitized: String = checkpoint_id
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect();

        if sanitized.is_empty() {
            return Err(anyhow::anyhow!(
                "Invalid checkpoint ID: '{}' (sanitized to empty)",
                checkpoint_id
            ));
        }

        let path = self
            .persistence
            .base_path
            .join("checkpoints")
            .join(format!("checkpoint_{}.json", sanitized));
        let json = serde_json::to_string_pretty(data)?;
        PersistenceManager::atomic_write(&path, json.as_bytes())?;
        Ok(())
    }

    pub fn load_checkpoint(&self, checkpoint_id: &str) -> anyhow::Result<Value> {
        let sanitized: String = checkpoint_id
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect();

        let path = self
            .persistence
            .base_path
            .join("checkpoints")
            .join(format!("checkpoint_{}.json", sanitized));
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
/// Uses O_APPEND for atomic appends (no read-modify-write)
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

    /// Atomic append using O_APPEND flag
    pub fn append(&self, entry: &Value) -> anyhow::Result<()> {
        let mut line = serde_json::to_string(entry)?;
        line.push('\n');

        use std::fs::OpenOptions;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;
        file.write_all(line.as_bytes())?;
        file.sync_all()?;
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
            PersistenceManager::atomic_write(
                &self.journal_path,
                serde_json::to_string_pretty(&empty)?.as_bytes(),
            )?;
        }
        Ok(())
    }

    pub fn mark_checkpoint(&self, checkpoint_id: &str) -> anyhow::Result<()> {
        let content = fs::read_to_string(&self.journal_path)?;
        let mut journal: Value = serde_json::from_str(&content)?;
        if let Some(obj) = journal.as_object_mut() {
            obj.insert(
                "last_checkpoint".to_string(),
                Value::String(checkpoint_id.to_string()),
            );
        }
        PersistenceManager::atomic_write(
            &self.journal_path,
            serde_json::to_string_pretty(&journal)?.as_bytes(),
        )?;
        Ok(())
    }

    pub fn add_pending_operation(&self, op: &Value) -> anyhow::Result<()> {
        let content = fs::read_to_string(&self.journal_path)?;
        let mut journal: Value = serde_json::from_str(&content)?;
        if let Some(obj) = journal.as_object_mut() {
            if let Some(ops) = obj
                .get_mut("pending_operations")
                .and_then(|v| v.as_array_mut())
            {
                ops.push(op.clone());
            }
        }
        PersistenceManager::atomic_write(
            &self.journal_path,
            serde_json::to_string_pretty(&journal)?.as_bytes(),
        )?;
        Ok(())
    }

    pub fn get_pending_operations(&self) -> anyhow::Result<Vec<Value>> {
        let content = fs::read_to_string(&self.journal_path)?;
        let journal: Value = serde_json::from_str(&content)?;
        Ok(journal["pending_operations"]
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    pub fn clear_pending_operations(&self) -> anyhow::Result<()> {
        let content = fs::read_to_string(&self.journal_path)?;
        let mut journal: Value = serde_json::from_str(&content)?;
        if let Some(obj) = journal.as_object_mut() {
            obj.insert("pending_operations".to_string(), Value::Array(vec![]));
        }
        PersistenceManager::atomic_write(
            &self.journal_path,
            serde_json::to_string_pretty(&journal)?.as_bytes(),
        )?;
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

    #[test]
    fn test_checkpoint_id_sanitization() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let cp = Checkpointer::new(dir.path());
        cp.init()?;

        // Normal ID works
        cp.save_checkpoint("test-123", &json!({"ok": true}))?;

        // Path traversal attempt is sanitized
        cp.save_checkpoint("../../etc/evil", &json!({"ok": true}))?;

        // List should show sanitized names
        let ids = cp.list_checkpoints()?;
        assert!(ids.contains(&"test-123".to_string()));
        assert!(ids.contains(&"etcevil".to_string())); // Slashes removed

        Ok(())
    }

    #[test]
    fn test_wal_atomic_append() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let wal = WriteAheadLog::new(dir.path());
        wal.init()?;

        wal.append(&json!({"op": "create", "id": 1}))?;
        wal.append(&json!({"op": "update", "id": 2}))?;

        let entries = wal.read_all()?;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["op"], "create");
        assert_eq!(entries[1]["op"], "update");

        Ok(())
    }
}
