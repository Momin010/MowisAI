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
                log::warn!(
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
            log::info!(
                "[HealthMonitor] Circuit CLOSED for sandbox {} after success",
                sandbox_name
            );
            cb.state = CircuitState::Closed;
        }
    }

    /// Check whether a sandbox is healthy (circuit not open)
    pub async fn is_sandbox_healthy(&self, sandbox_name: &str) -> bool {
        let now = now_secs();
        // Use read lock first, only upgrade to write if we need to transition state
        let breakers = self.circuit_breakers.read().await;
        if let Some(cb) = breakers.get(sandbox_name) {
            match cb.state {
                CircuitState::Closed => true,
                CircuitState::HalfOpen => true,
                CircuitState::Open => {
                    if now.saturating_sub(cb.last_failure_time) >= self.recovery_timeout_secs {
                        // Need to transition to HalfOpen — drop read lock, take write lock
                        drop(breakers);
                        let mut breakers = self.circuit_breakers.write().await;
                        if let Some(cb) = breakers.get_mut(sandbox_name) {
                            if cb.state == CircuitState::Open
                                && now.saturating_sub(cb.last_failure_time) >= self.recovery_timeout_secs
                            {
                                log::info!(
                                    "[HealthMonitor] Circuit HALF-OPEN for sandbox {} (testing recovery)",
                                    sandbox_name
                                );
                                cb.state = CircuitState::HalfOpen;
                            }
                        }
                        true
                    } else {
                        false
                    }
                }
            }
        } else {
            true // No circuit breaker = healthy
        }
    }

    /// Get list of agents that haven't sent a heartbeat within the timeout
    pub async fn get_dead_agents(&self) -> Vec<String> {
        let now = now_secs();
        let beats = self.heartbeats.read().await;
        beats
            .iter()
            .filter(|&(_, &last)| now.saturating_sub(last) > self.heartbeat_timeout_secs)
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

/// System resource snapshot
#[derive(Debug, Clone)]
pub struct SystemResources {
    /// Total system memory in bytes
    pub total_memory_bytes: u64,
    /// Available system memory in bytes
    pub available_memory_bytes: u64,
    /// Memory usage percentage (0-100)
    pub memory_usage_percent: f64,
    /// Number of CPU cores
    pub cpu_cores: u32,
    /// System load average (1 minute)
    pub load_avg_1m: f64,
    /// Available disk space in bytes for the overlay root
    pub disk_available_bytes: u64,
    /// Disk usage percentage (0-100)
    pub disk_usage_percent: f64,
    /// Number of open file descriptors (process)
    pub open_fds: u32,
    /// Maximum file descriptors
    pub max_fds: u32,
    /// Number of running processes
    pub process_count: u32,
}

impl SystemResources {
    /// Collect current system resource usage
    pub fn collect() -> Self {
        let (total_mem, avail_mem) = Self::read_memory();
        let disk = Self::read_disk("/");
        let load = Self::read_load_avg();
        let fds = Self::read_fd_count();

        SystemResources {
            total_memory_bytes: total_mem,
            available_memory_bytes: avail_mem,
            memory_usage_percent: if total_mem > 0 {
                ((total_mem - avail_mem) as f64 / total_mem as f64) * 100.0
            } else {
                0.0
            },
            cpu_cores: Self::read_cpu_cores(),
            load_avg_1m: load,
            disk_available_bytes: disk.0,
            disk_usage_percent: disk.1,
            open_fds: fds.0,
            max_fds: fds.1,
            process_count: Self::read_process_count(),
        }
    }

    /// Check if the system has enough resources to spawn more agents
    pub fn can_spawn_agent(&self) -> bool {
        // Need at least 100MB free memory
        if self.available_memory_bytes < 100 * 1024 * 1024 {
            return false;
        }
        // Need at least 80% of FDs available
        if self.open_fds as f64 / self.max_fds.max(1) as f64 > 0.8 {
            return false;
        }
        // Need at least 1GB free disk
        if self.disk_available_bytes < 1024 * 1024 * 1024 {
            return false;
        }
        // Load should not exceed 4x CPU cores
        if self.load_avg_1m > (self.cpu_cores as f64 * 4.0) {
            return false;
        }
        true
    }

    /// Get maximum recommended concurrent agents based on resources
    pub fn max_recommended_agents(&self) -> u32 {
        // Each agent uses roughly 50MB RAM + 1 FD + some disk
        let mem_limit = (self.available_memory_bytes / (50 * 1024 * 1024)) as u32;
        let fd_limit = (self.max_fds - self.open_fds).saturating_sub(100); // Reserve 100 FDs
        let disk_limit = (self.disk_available_bytes / (100 * 1024 * 1024)) as u32; // 100MB per agent

        mem_limit.min(fd_limit).min(disk_limit).max(1)
    }

    #[cfg(target_os = "linux")]
    fn read_memory() -> (u64, u64) {
        if let Ok(info) = std::fs::read_to_string("/proc/meminfo") {
            let mut total = 0u64;
            let mut available = 0u64;
            for line in info.lines() {
                if line.starts_with("MemTotal:") {
                    total = Self::parse_meminfo_kb(line) * 1024;
                } else if line.starts_with("MemAvailable:") {
                    available = Self::parse_meminfo_kb(line) * 1024;
                }
            }
            (total, available)
        } else {
            (0, 0)
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn read_memory() -> (u64, u64) {
        (0, 0)
    }

    fn parse_meminfo_kb(line: &str) -> u64 {
        line.split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    }

    #[cfg(target_os = "linux")]
    fn read_cpu_cores() -> u32 {
        if let Ok(info) = std::fs::read_to_string("/proc/cpuinfo") {
            info.lines().filter(|l| l.starts_with("processor")).count() as u32
        } else {
            1
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn read_cpu_cores() -> u32 {
        1
    }

    fn read_load_avg() -> f64 {
        if let Ok(load) = std::fs::read_to_string("/proc/loadavg") {
            load.split_whitespace()
                .next()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0)
        } else {
            0.0
        }
    }

    fn read_disk(path: &str) -> (u64, f64) {
        #[cfg(unix)]
        {
            use std::ffi::CString;
            let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
            if let Ok(c_path) = CString::new(path) {
                if unsafe { libc::statfs(c_path.as_ptr(), &mut stat) } == 0 {
                    let total = stat.f_blocks as u64 * stat.f_frsize as u64;
                    let available = stat.f_bavail as u64 * stat.f_frsize as u64;
                    let usage = if total > 0 {
                        ((total - available) as f64 / total as f64) * 100.0
                    } else {
                        0.0
                    };
                    return (available, usage);
                }
            }
            (0, 0.0)
        }
        #[cfg(not(unix))]
        {
            (0, 0.0)
        }
    }

    fn read_fd_count() -> (u32, u32) {
        #[cfg(target_os = "linux")]
        {
            let open = std::fs::read_dir("/proc/self/fd")
                .map(|d| d.count() as u32)
                .unwrap_or(0);
            let max = std::fs::read_to_string("/proc/sys/fs/file-max")
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(1024);
            (open, max)
        }
        #[cfg(not(target_os = "linux"))]
        {
            (0, 1024)
        }
    }

    #[cfg(target_os = "linux")]
    fn read_process_count() -> u32 {
        std::fs::read_dir("/proc")
            .map(|d| d.filter(|e| e.as_ref().map(|e| e.file_name().to_string_lossy().chars().all(|c| c.is_numeric())).unwrap_or(false)).count() as u32)
            .unwrap_or(0)
    }

    #[cfg(not(target_os = "linux"))]
    fn read_process_count() -> u32 {
        0
    }
}

impl std::fmt::Display for SystemResources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Memory: {:.1}% ({}/{} MB), CPUs: {}, Load: {:.2}, Disk: {:.1}% ({} MB free), FDs: {}/{}, Procs: {}",
            self.memory_usage_percent,
            (self.total_memory_bytes - self.available_memory_bytes) / (1024 * 1024),
            self.total_memory_bytes / (1024 * 1024),
            self.cpu_cores,
            self.load_avg_1m,
            self.disk_usage_percent,
            self.disk_available_bytes / (1024 * 1024),
            self.open_fds,
            self.max_fds,
            self.process_count,
        )
    }
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
