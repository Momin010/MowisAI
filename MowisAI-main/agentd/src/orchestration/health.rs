//! Health monitoring and circuit breakers for sandbox agents

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// State of a circuit breaker
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation — work is dispatched
    Closed,
    /// Too many failures — stop sending work
    Open,
    /// Probe state — allow one request to test recovery
    HalfOpen,
}

/// Circuit breaker for a sandbox
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    pub state: CircuitState,
    pub consecutive_failures: usize,
    pub last_failure_time: u64,
    pub total_failures: usize,
}

impl CircuitBreaker {
    fn new() -> Self {
        Self {
            state: CircuitState::Closed,
            consecutive_failures: 0,
            last_failure_time: 0,
            total_failures: 0,
        }
    }
}

/// Summary of the health monitoring system
#[derive(Debug, Clone)]
pub struct HealthStatus {
    pub total_agents_tracked: usize,
    pub dead_agents: Vec<String>,
    pub sandbox_states: HashMap<String, CircuitState>,
    pub open_circuits: usize,
}

/// Health monitor — tracks agent heartbeats and sandbox circuit breakers
pub struct HealthMonitor {
    /// agent_id -> last heartbeat timestamp (seconds)
    heartbeats: Arc<RwLock<HashMap<String, u64>>>,
    /// sandbox_name -> circuit breaker state
    circuit_breakers: Arc<RwLock<HashMap<String, CircuitBreaker>>>,
    /// Seconds without heartbeat before declaring agent dead
    heartbeat_timeout_secs: u64,
    /// Consecutive failures before opening circuit
    failure_threshold: usize,
    /// Seconds after opening before transitioning to HalfOpen
    recovery_timeout_secs: u64,
}

impl HealthMonitor {
    pub fn new(heartbeat_timeout_secs: u64, failure_threshold: usize) -> Self {
        Self {
            heartbeats: Arc::new(RwLock::new(HashMap::new())),
            circuit_breakers: Arc::new(RwLock::new(HashMap::new())),
            heartbeat_timeout_secs,
            failure_threshold,
            recovery_timeout_secs: 60,
        }
    }

    /// Record a heartbeat from an agent
    pub async fn heartbeat(&self, agent_id: &str) {
        let now = now_secs();
        let mut beats = self.heartbeats.write().await;
        beats.insert(agent_id.to_string(), now);
    }

    /// Remove an agent from heartbeat tracking (e.g. when it finishes)
    pub async fn remove_agent(&self, agent_id: &str) {
        let mut beats = self.heartbeats.write().await;
        beats.remove(agent_id);
    }

    /// Record a task failure for a sandbox
    pub async fn record_failure(&self, sandbox_name: &str) {
        let now = now_secs();
        let mut breakers = self.circuit_breakers.write().await;
        let cb = breakers
            .entry(sandbox_name.to_string())
            .or_insert_with(CircuitBreaker::new);

        cb.consecutive_failures += 1;
        cb.total_failures += 1;
        cb.last_failure_time = now;

        if cb.consecutive_failures >= self.failure_threshold {
            if cb.state != CircuitState::Open {
                eprintln!(
                    "[HealthMonitor] Circuit OPEN for sandbox {} after {} consecutive failures",
                    sandbox_name, cb.consecutive_failures
                );
                cb.state = CircuitState::Open;
            }
        }
    }

    /// Record a task success for a sandbox
    pub async fn record_success(&self, sandbox_name: &str) {
        let mut breakers = self.circuit_breakers.write().await;
        let cb = breakers
            .entry(sandbox_name.to_string())
            .or_insert_with(CircuitBreaker::new);

        cb.consecutive_failures = 0;

        if cb.state == CircuitState::HalfOpen || cb.state == CircuitState::Open {
            println!(
                "[HealthMonitor] Circuit CLOSED for sandbox {} after success",
                sandbox_name
            );
            cb.state = CircuitState::Closed;
        }
    }

    /// Check whether a sandbox is healthy (circuit not open)
    pub async fn is_sandbox_healthy(&self, sandbox_name: &str) -> bool {
        let now = now_secs();
        let mut breakers = self.circuit_breakers.write().await;
        let cb = breakers
            .entry(sandbox_name.to_string())
            .or_insert_with(CircuitBreaker::new);

        match cb.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open => {
                // Transition to HalfOpen after recovery_timeout_secs
                if now.saturating_sub(cb.last_failure_time) >= self.recovery_timeout_secs {
                    println!(
                        "[HealthMonitor] Circuit HALF-OPEN for sandbox {} (testing recovery)",
                        sandbox_name
                    );
                    cb.state = CircuitState::HalfOpen;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Get list of agents that haven't sent a heartbeat within the timeout
    pub async fn get_dead_agents(&self) -> Vec<String> {
        let now = now_secs();
        let beats = self.heartbeats.read().await;
        beats
            .iter()
            .filter(|(_, &last)| now.saturating_sub(last) > self.heartbeat_timeout_secs)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get a snapshot of the overall health status
    pub async fn get_status(&self) -> HealthStatus {
        let dead_agents = self.get_dead_agents().await;
        let beats = self.heartbeats.read().await;
        let total_agents_tracked = beats.len();
        drop(beats);

        let breakers = self.circuit_breakers.read().await;
        let open_circuits = breakers
            .values()
            .filter(|cb| cb.state == CircuitState::Open)
            .count();
        let sandbox_states: HashMap<String, CircuitState> = breakers
            .iter()
            .map(|(name, cb)| (name.clone(), cb.state.clone()))
            .collect();

        HealthStatus {
            total_agents_tracked,
            dead_agents,
            sandbox_states,
            open_circuits,
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_heartbeat_and_dead_agents() {
        let monitor = HealthMonitor::new(30, 3);
        monitor.heartbeat("agent-1").await;
        monitor.heartbeat("agent-2").await;

        let dead = monitor.get_dead_agents().await;
        assert!(dead.is_empty(), "fresh heartbeats should not be dead");
    }

    #[tokio::test]
    async fn test_circuit_breaker_opens_after_threshold() {
        let monitor = HealthMonitor::new(30, 3);
        assert!(monitor.is_sandbox_healthy("sb-1").await);

        monitor.record_failure("sb-1").await;
        monitor.record_failure("sb-1").await;
        assert!(monitor.is_sandbox_healthy("sb-1").await, "circuit still closed before threshold");

        monitor.record_failure("sb-1").await;
        assert!(!monitor.is_sandbox_healthy("sb-1").await, "circuit should be open");
    }

    #[tokio::test]
    async fn test_circuit_breaker_closes_after_success() {
        let monitor = HealthMonitor::new(30, 2);

        monitor.record_failure("sb-2").await;
        monitor.record_failure("sb-2").await;
        assert!(!monitor.is_sandbox_healthy("sb-2").await);

        // Force transition to HalfOpen by manually patching state
        {
            let mut breakers = monitor.circuit_breakers.write().await;
            if let Some(cb) = breakers.get_mut("sb-2") {
                cb.state = CircuitState::HalfOpen;
            }
        }

        monitor.record_success("sb-2").await;
        assert!(monitor.is_sandbox_healthy("sb-2").await, "circuit should be closed after success");
    }

    #[tokio::test]
    async fn test_health_status() {
        let monitor = HealthMonitor::new(30, 3);
        monitor.heartbeat("agent-A").await;
        monitor.record_failure("sb-3").await;
        monitor.record_failure("sb-3").await;
        monitor.record_failure("sb-3").await;

        let status = monitor.get_status().await;
        assert_eq!(status.open_circuits, 1);
        assert_eq!(status.total_agents_tracked, 1);
    }
}
