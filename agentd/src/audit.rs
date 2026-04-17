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
                .unwrap()
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
    log_file: Mutex<File>,
    buffer: Mutex<Vec<AuditEvent>>,
    buffer_size: usize,
}

impl AuditLogger {
    pub fn new(log_path: &Path, buffer_size: usize) -> anyhow::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;

        Ok(AuditLogger {
            log_file: Mutex::new(file),
            buffer: Mutex::new(Vec::new()),
            buffer_size,
        })
    }

    pub fn log(&self, event: AuditEvent) -> anyhow::Result<()> {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.push(event);

        if buffer.len() >= self.buffer_size {
            self.flush()?;
        }
        Ok(())
    }

    pub fn flush(&self) -> anyhow::Result<()> {
        let mut buffer = self.buffer.lock().unwrap();
        let mut file = self.log_file.lock().unwrap();

        for event in buffer.drain(..) {
            let line = serde_json::to_string(&event)? + "\n";
            file.write_all(line.as_bytes())?;
        }
        file.flush()?;
        Ok(())
    }

    pub fn read_events(&self, _count: usize) -> anyhow::Result<Vec<AuditEvent>> {
        self.flush()?;
        // Re-read from file (simple implementation)
        // In production, would use more efficient seeking
        Ok(Vec::new())
    }
}

/// Query builder for audit events
#[derive(Debug)]
pub struct AuditQuery {
    pub event_type: Option<EventType>,
    pub actor_id: Option<u64>,
    pub target_id: Option<u64>,
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    pub limit: usize,
}

impl AuditQuery {
    pub fn new() -> Self {
        AuditQuery {
            event_type: None,
            actor_id: None,
            target_id: None,
            start_time: None,
            end_time: None,
            limit: 100,
        }
    }

    pub fn with_event_type(mut self, event_type: EventType) -> Self {
        self.event_type = Some(event_type);
        self
    }

    pub fn with_actor(mut self, actor_id: u64) -> Self {
        self.actor_id = Some(actor_id);
        self
    }

    pub fn with_target(mut self, target_id: u64) -> Self {
        self.target_id = Some(target_id);
        self
    }

    pub fn with_time_range(mut self, start: u64, end: u64) -> Self {
        self.start_time = Some(start);
        self.end_time = Some(end);
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

/// Statistics for audit trail
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditStats {
    pub total_events: u64,
    pub events_by_type: std::collections::HashMap<String, u64>,
    pub actors: Vec<u64>,
    pub time_span: (u64, u64),
}

/// Security audit system
pub struct SecurityAuditor {
    pub logger: AuditLogger,
    pub stats: Mutex<AuditStats>,
}

impl SecurityAuditor {
    pub fn new(log_path: &Path) -> anyhow::Result<Self> {
        let logger = AuditLogger::new(log_path, 100)?;
        Ok(SecurityAuditor {
            logger,
            stats: Mutex::new(AuditStats {
                total_events: 0,
                events_by_type: std::collections::HashMap::new(),
                actors: Vec::new(),
                time_span: (u64::MAX, 0),
            }),
        })
    }

    pub fn record_event(&self, event: AuditEvent) -> anyhow::Result<()> {
        // Update stats
        let mut stats = self.stats.lock().unwrap();
        stats.total_events += 1;
        *stats
            .events_by_type
            .entry(format!("{:?}", event.event_type))
            .or_insert(0) += 1;

        if !stats.actors.contains(&event.actor_id) {
            stats.actors.push(event.actor_id);
        }

        if event.timestamp < stats.time_span.0 {
            stats.time_span.0 = event.timestamp;
        }
        if event.timestamp > stats.time_span.1 {
            stats.time_span.1 = event.timestamp;
        }

        drop(stats);

        // Log event
        self.logger.log(event)
    }

    pub fn detect_anomalies(&self) -> Value {
        let stats = self.stats.lock().unwrap();
        let mut anomalies = vec![];

        // Simple anomaly detection rules
        for (etype, count) in &stats.events_by_type {
            if *count > 1000 && etype.contains("Invoked") {
                anomalies.push(format!("High frequency of {}: {}", etype, count));
            }
        }

        json!({
            "anomalies": anomalies,
            "total_events": stats.total_events,
            "unique_actors": stats.actors.len(),
        })
    }

    pub fn get_stats(&self) -> Value {
        let stats = self.stats.lock().unwrap();
        serde_json::to_value(stats.clone()).unwrap_or(Value::Null)
    }
}

/// Compliance checker for policy enforcement
pub struct ComplianceChecker {
    policies: std::collections::HashMap<String, String>,
}

impl ComplianceChecker {
    pub fn new() -> Self {
        ComplianceChecker {
            policies: std::collections::HashMap::new(),
        }
    }

    pub fn add_policy(&mut self, name: impl Into<String>, rule: impl Into<String>) {
        self.policies.insert(name.into(), rule.into());
    }

    pub fn check(&self, event: &AuditEvent) -> bool {
        // Simple policy checking (in production, would be more sophisticated)
        match &event.event_type {
            EventType::SecurityViolation => false, // always fail
            _ => true,
        }
    }

    pub fn get_policies(&self) -> Value {
        serde_json::to_value(&self.policies).unwrap_or(Value::Null)
    }
}

/// Replay engine for reproducing executions
pub struct ReplayEngine {
    events: Vec<AuditEvent>,
}

impl ReplayEngine {
    pub fn new(events: Vec<AuditEvent>) -> Self {
        ReplayEngine { events }
    }

    pub fn filter_by_actor(&self, actor_id: u64) -> Vec<&AuditEvent> {
        self.events
            .iter()
            .filter(|e| e.actor_id == actor_id)
            .collect()
    }

    pub fn filter_by_type(&self, event_type: &EventType) -> Vec<&AuditEvent> {
        self.events
            .iter()
            .filter(|e| &e.event_type == event_type)
            .collect()
    }

    pub fn replay_tool_invocations(&self) -> Vec<&AuditEvent> {
        self.events
            .iter()
            .filter(|e| matches!(e.event_type, EventType::ToolInvoked))
            .collect()
    }

    pub fn timeline(&self) -> Vec<&AuditEvent> {
        // Return events in chronological order
        let mut sorted = self.events.iter().collect::<Vec<_>>();
        sorted.sort_by_key(|e| e.timestamp);
        sorted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_audit_event_creation() {
        let event = AuditEvent::new(EventType::SandboxCreated, 1, "Created sandbox")
            .with_target(100)
            .with_result("success");
        assert_eq!(event.actor_id, 1);
        assert_eq!(event.target_id, Some(100));
        assert_eq!(event.result, "success");
    }

    #[test]
    fn test_compliance_checker() {
        let checker = ComplianceChecker::new();
        let event = AuditEvent::new(EventType::ToolInvoked, 1, "test");
        assert!(checker.check(&event));
    }

    #[test]
    fn test_replay_engine() {
        let events = vec![
            AuditEvent::new(EventType::SandboxCreated, 1, "test"),
            AuditEvent::new(EventType::ToolInvoked, 1, "test"),
        ];
        let engine = ReplayEngine::new(events);
        assert_eq!(engine.replay_tool_invocations().len(), 1);
    }

    #[test]
    fn test_audit_stats() {
        // Test that we can get stats without requiring file system initialization
        let stats = AuditStats {
            total_events: 5,
            events_by_type: {
                let mut map = std::collections::HashMap::new();
                map.insert("ToolInvoked".to_string(), 3);
                map.insert("SandboxCreated".to_string(), 2);
                map
            },
            actors: vec![1, 2, 3],
            time_span: (1000, 2000),
        };

        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["total_events"], 5);
        assert_eq!(json["actors"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_compliance_checker_policies() {
        let mut checker = ComplianceChecker::new();
        checker.add_policy("test_policy", "rule1");
        let policies = checker.get_policies();
        assert!(policies["test_policy"].is_string());
    }
}
