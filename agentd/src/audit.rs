use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

/// Event types for audit logging
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum EventType {
    SandboxCreated,
    SandboxDestroyed,
    ToolRegistered,
    ToolInvoked,
    ToolFailed,
    MemoryStored,
    MemoryRetrieved,
    TaskStarted,
    TaskCompleted,
    TaskFailed,
    ChannelCreated,
    MessageSent,
    MessageReceived,
    AgentSpawned,
    AgentTerminated,
    SecurityViolation,
    ResourceLimitExceeded,
    CheckpointCreated,
    StateRestored,
    Custom(String),
}

/// A single audit event
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp: u64,
    pub event_type: EventType,
    pub actor_id: u64,
    pub target_id: Option<u64>,
    pub description: String,
    pub details: Value,
    pub result: String,
}

impl AuditEvent {
    pub fn new(event_type: EventType, actor_id: u64, description: impl Into<String>) -> Self {
        AuditEvent {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            event_type,
            actor_id,
            target_id: None,
            description: description.into(),
            details: Value::Null,
            result: "pending".to_string(),
        }
    }

    pub fn with_target(mut self, target_id: u64) -> Self {
        self.target_id = Some(target_id);
        self
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    pub fn with_result(mut self, result: impl Into<String>) -> Self {
        self.result = result.into();
        self
    }
}

/// Audit logger for tracking all agent operations
pub struct AuditLogger {
    log_path: std::path::PathBuf,
    buffer: Mutex<Vec<AuditEvent>>,
    buffer_size: usize,
}

impl AuditLogger {
    pub fn new(log_path: &Path, buffer_size: usize) -> anyhow::Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        Ok(AuditLogger {
            log_path: log_path.to_path_buf(),
            buffer: Mutex::new(Vec::new()),
            buffer_size,
        })
    }

    pub fn log(&self, event: AuditEvent) -> anyhow::Result<()> {
        let should_flush = {
            let mut buffer = self.buffer.lock().unwrap_or_else(|e| e.into_inner());
            buffer.push(event);
            buffer.len() >= self.buffer_size
        };
        if should_flush {
            self.flush()?;
        }
        Ok(())
    }

    pub fn flush(&self) -> anyhow::Result<()> {
        let mut buffer = self.buffer.lock().unwrap_or_else(|e| e.into_inner());
        if buffer.is_empty() {
            return Ok(());
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;

        for event in buffer.drain(..) {
            let line = serde_json::to_string(&event)? + "\n";
            file.write_all(line.as_bytes())?;
        }
        file.flush()?;
        Ok(())
    }

    /// Read events from the log file
    pub fn read_events(&self, count: usize) -> anyhow::Result<Vec<AuditEvent>> {
        self.flush()?;
        let content = std::fs::read_to_string(&self.log_path).unwrap_or_default();
        let mut events = Vec::new();
        for line in content.lines().rev() {
            if events.len() >= count {
                break;
            }
            if let Ok(event) = serde_json::from_str::<AuditEvent>(line) {
                events.push(event);
            }
        }
        events.reverse();
        Ok(events)
    }
}

/// Query builder for audit events
pub struct AuditQuery {
    event_type: Option<EventType>,
    actor_id: Option<u64>,
    limit: usize,
}

impl AuditQuery {
    pub fn new() -> Self {
        AuditQuery {
            event_type: None,
            actor_id: None,
            limit: 100,
        }
    }

    pub fn with_type(mut self, event_type: EventType) -> Self {
        self.event_type = Some(event_type);
        self
    }

    pub fn with_actor(mut self, actor_id: u64) -> Self {
        self.actor_id = Some(actor_id);
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

/// Security auditor for analyzing audit logs
pub struct SecurityAuditor {
    logger: AuditLogger,
}

impl SecurityAuditor {
    pub fn new(log_path: &Path) -> anyhow::Result<Self> {
        let logger = AuditLogger::new(log_path, 100)?;
        Ok(SecurityAuditor { logger })
    }

    pub fn log(&self, event: AuditEvent) -> anyhow::Result<()> {
        self.logger.log(event)
    }

    pub fn flush(&self) -> anyhow::Result<()> {
        self.logger.flush()
    }

    pub fn read_events(&self, count: usize) -> anyhow::Result<Vec<AuditEvent>> {
        self.logger.read_events(count)
    }

    /// Record an audit event (alias for `log`)
    pub fn record_event(&self, event: AuditEvent) -> anyhow::Result<()> {
        self.logger.log(event)
    }

    /// Get aggregate statistics from the audit log
    pub fn get_stats(&self) -> serde_json::Value {
        let events = self.read_events(10000).unwrap_or_default();
        let stats = AuditStats::from_events(&events);
        serde_json::json!({
            "total_events": stats.total_events,
            "security_violations": stats.security_violations,
            "failed_operations": stats.failed_operations,
            "unique_actors": stats.unique_actors,
        })
    }

    /// Detect anomalies in the audit log
    pub fn detect_anomalies(&self) -> serde_json::Value {
        let events = self.read_events(10000).unwrap_or_default();
        let violations: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == EventType::SecurityViolation)
            .map(|e| serde_json::json!({
                "timestamp": e.timestamp,
                "actor_id": e.actor_id,
                "description": e.description,
            }))
            .collect();
        serde_json::json!({ "security_violations": violations })
    }
}

/// Compliance checker for verifying policy adherence
pub struct ComplianceChecker {
    policies: std::collections::HashMap<String, String>,
}

impl ComplianceChecker {
    pub fn new() -> Self {
        ComplianceChecker {
            policies: std::collections::HashMap::new(),
        }
    }

    pub fn add_policy(&mut self, name: String, rule: String) {
        self.policies.insert(name, rule);
    }

    pub fn check(&self, event: &AuditEvent) -> bool {
        // Security violations always fail compliance
        if event.event_type == EventType::SecurityViolation {
            return false;
        }
        // Check if any policy is violated
        for (_name, _rule) in &self.policies {
            // Policy-specific checks would go here
        }
        true
    }
}

/// Replay engine for re-executing audit events
pub struct ReplayEngine;

impl ReplayEngine {
    pub fn new() -> Self {
        ReplayEngine
    }

    pub fn replay(&self, events: &[AuditEvent]) -> anyhow::Result<()> {
        for event in events {
            log::info!("Replay: {:?} - {}", event.event_type, event.description);
        }
        Ok(())
    }
}

/// Audit statistics
#[derive(Debug, Clone)]
pub struct AuditStats {
    pub total_events: usize,
    pub security_violations: usize,
    pub failed_operations: usize,
    pub unique_actors: usize,
}

impl AuditStats {
    pub fn from_events(events: &[AuditEvent]) -> Self {
        let mut actors = std::collections::HashSet::new();
        let mut violations = 0;
        let mut failures = 0;

        for event in events {
            actors.insert(event.actor_id);
            if event.event_type == EventType::SecurityViolation {
                violations += 1;
            }
            if event.result == "failed" {
                failures += 1;
            }
        }

        AuditStats {
            total_events: events.len(),
            security_violations: violations,
            failed_operations: failures,
            unique_actors: actors.len(),
        }
    }
}
