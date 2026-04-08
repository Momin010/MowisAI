use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Syscall security policy
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SecurityPolicy {
    pub name: String,
    pub allowed_syscalls: Vec<String>,
    pub denied_syscalls: Vec<String>,
    pub resource_limits: ResourceSecurityLimits,
    pub file_access_rules: Vec<FileAccessRule>,
    pub network_rules: Vec<NetworkRule>,
    /// Whether shell commands (run_command, run_script) are allowed
    pub allow_shell_execution: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceSecurityLimits {
    pub max_memory_mb: Option<u64>,
    pub max_cpu_percent: Option<u64>,
    pub max_open_files: Option<u32>,
    pub max_processes: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileAccessRule {
    pub path: String,
    pub allow_read: bool,
    pub allow_write: bool,
    pub allow_execute: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkRule {
    pub allow_outbound: bool,
    pub allow_inbound: bool,
    pub allowed_ports: Vec<u16>,
    pub blocked_ports: Vec<u16>,
}

impl SecurityPolicy {
    pub fn default_restrictive() -> Self {
        SecurityPolicy {
            name: "restrictive".to_string(),
            allowed_syscalls: vec![
                "read",
                "write",
                "open",
                "close",
                "exit",
                "exit_group",
                "brk",
                "mmap",
                "munmap",
                "mprotect",
                "rt_sigaction",
                "rt_sigprocmask",
                "arch_prctl",
                "access",
                "getpid",
                "lseek",
                "stat",
                "fstat",
                "lstat",
                "poll",
                "fcntl",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            denied_syscalls: vec![
                "clone", "fork", "vfork", "execve", "ptrace", "socket", "connect", "bind",
                "listen", "accept", "mount", "umount2", "unshare", "setns", "reboot",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            resource_limits: ResourceSecurityLimits {
                max_memory_mb: Some(256),
                max_cpu_percent: Some(50),
                max_open_files: Some(100),
                max_processes: Some(10),
            },
            file_access_rules: vec![
                FileAccessRule {
                    path: "/tmp".to_string(),
                    allow_read: true,
                    allow_write: true,
                    allow_execute: false,
                },
                FileAccessRule {
                    path: "/etc".to_string(),
                    allow_read: true,
                    allow_write: false,
                    allow_execute: false,
                },
                FileAccessRule {
                    path: "/home".to_string(),
                    allow_read: false,
                    allow_write: false,
                    allow_execute: false,
                },
            ],
            network_rules: vec![NetworkRule {
                allow_outbound: false,
                allow_inbound: false,
                allowed_ports: vec![],
                blocked_ports: vec![],
            }],
            allow_shell_execution: false,
        }
    }

    pub fn default_permissive() -> Self {
        SecurityPolicy {
            name: "permissive".to_string(),
            allowed_syscalls: vec![], // empty = allow all
            denied_syscalls: vec![],
            resource_limits: ResourceSecurityLimits {
                max_memory_mb: Some(1024),
                max_cpu_percent: Some(100),
                max_open_files: Some(1000),
                max_processes: Some(100),
            },
            file_access_rules: vec![FileAccessRule {
                path: "/".to_string(),
                allow_read: true,
                allow_write: true,
                allow_execute: true,
            }],
            network_rules: vec![NetworkRule {
                allow_outbound: true,
                allow_inbound: true,
                allowed_ports: vec![],
                blocked_ports: vec![],
            }],
            allow_shell_execution: true,
        }
    }

    pub fn check_syscall(&self, syscall: &str) -> bool {
        // If denied list empty, allow if not in denied
        // If allowed list non-empty, only allow those
        if !self.denied_syscalls.is_empty() && self.denied_syscalls.contains(&syscall.to_string()) {
            return false;
        }
        if !self.allowed_syscalls.is_empty()
            && !self.allowed_syscalls.contains(&syscall.to_string())
        {
            return false;
        }
        true
    }

    pub fn check_file_access(&self, path: &str, access_type: FileAccessType) -> bool {
        for rule in &self.file_access_rules {
            if path.starts_with(&rule.path) {
                return match access_type {
                    FileAccessType::Read => rule.allow_read,
                    FileAccessType::Write => rule.allow_write,
                    FileAccessType::Execute => rule.allow_execute,
                };
            }
        }
        false
    }

    pub fn check_network_access(&self, outbound: bool) -> bool {
        for rule in &self.network_rules {
            if outbound && rule.allow_outbound {
                return true;
            }
            if !outbound && rule.allow_inbound {
                return true;
            }
        }
        false
    }

    pub fn check_shell_execution(&self) -> bool {
        self.allow_shell_execution
    }
}

#[derive(Clone, Debug)]
pub enum FileAccessType {
    Read,
    Write,
    Execute,
}

/// Seccomp filter builder
pub struct SeccompFilter {
    policy: SecurityPolicy,
}

impl SeccompFilter {
    pub fn new(policy: SecurityPolicy) -> Self {
        SeccompFilter { policy }
    }

    /// Generate seccomp BPF rules (simplified JSON representation)
    pub fn to_bpf_rules(&self) -> Value {
        let mut rules = vec![];

        // Denied syscalls
        for syscall in &self.policy.denied_syscalls {
            rules.push(json!({
                "type": "deny",
                "syscall": syscall,
                "action": "kill"
            }));
        }

        // Allowed syscalls
        if !self.policy.allowed_syscalls.is_empty() {
            rules.push(json!({
                "type": "allow_list",
                "syscalls": self.policy.allowed_syscalls,
                "action": "allow"
            }));
        }

        json!({
            "default_action": if self.policy.allowed_syscalls.is_empty() { "allow" } else { "kill" },
            "rules": rules
        })
    }

    pub fn to_json(&self) -> Value {
        json!({
            "name": self.policy.name,
            "syscall_rules": self.to_bpf_rules(),
            "file_rules": self.policy.file_access_rules,
            "network_rules": self.policy.network_rules,
            "resource_limits": self.policy.resource_limits,
        })
    }
}

/// Capability manager for Linux capabilities
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilitySet {
    pub capabilities: Vec<String>,
}

impl CapabilitySet {
    pub fn minimal() -> Self {
        CapabilitySet {
            capabilities: vec!["CAP_CHOWN".to_string(), "CAP_DAC_OVERRIDE".to_string()],
        }
    }

    pub fn none() -> Self {
        CapabilitySet {
            capabilities: vec![],
        }
    }

    pub fn full() -> Self {
        CapabilitySet {
            capabilities: vec![
                "CAP_CHOWN",
                "CAP_DAC_OVERRIDE",
                "CAP_DAC_READ_SEARCH",
                "CAP_FOWNER",
                "CAP_FSETID",
                "CAP_KILL",
                "CAP_SETGID",
                "CAP_SETUID",
                "CAP_SETFCAP",
                "CAP_SETPCAP",
                "CAP_NET_BIND_SERVICE",
                "CAP_SYS_CHROOT",
                "CAP_SYS_ADMIN",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        }
    }

    pub fn has_capability(&self, cap: &str) -> bool {
        self.capabilities.contains(&cap.to_string())
    }
}

/// MAC (Mandatory Access Control) policy
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MACPolicy {
    SELinux,
    AppArmor,
    None,
}

/// Complete security context for a sandbox
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SecurityContext {
    pub policy: SecurityPolicy,
    pub capabilities: CapabilitySet,
    pub mac_policy: MACPolicy,
    pub user_namespace: bool,
    pub network_namespace: bool,
    pub ipc_namespace: bool,
    pub pid_namespace: bool,
    pub uts_namespace: bool,
}

impl SecurityContext {
    pub fn default_sandbox() -> Self {
        SecurityContext {
            policy: SecurityPolicy::default_restrictive(),
            capabilities: CapabilitySet::minimal(),
            mac_policy: MACPolicy::None,
            user_namespace: true,
            network_namespace: true,
            ipc_namespace: true,
            pid_namespace: true,
            uts_namespace: true,
        }
    }

    pub fn permissive_sandbox() -> Self {
        SecurityContext {
            policy: SecurityPolicy::default_permissive(),
            capabilities: CapabilitySet::full(),
            mac_policy: MACPolicy::None,
            user_namespace: false,
            network_namespace: false,
            ipc_namespace: false,
            pid_namespace: false,
            uts_namespace: false,
        }
    }
}

/// Threat analyzer for detecting potential security issues
pub struct ThreatAnalyzer {
    blocked_syscalls: Vec<String>,
    suspicious_patterns: Vec<String>,
}

impl ThreatAnalyzer {
    pub fn new() -> Self {
        ThreatAnalyzer {
            blocked_syscalls: vec![
                "ptrace",
                "process_vm_readv",
                "process_vm_writev",
                "syslog",
                "mount",
                "umount2",
                "unshare",
                "setns",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            suspicious_patterns: vec!["privilege_escalation", "information_leak", "DoS_attempt"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    pub fn analyze_syscall(&self, syscall: &str) -> Option<String> {
        if self.blocked_syscalls.contains(&syscall.to_string()) {
            Some(format!("Blocked syscall detected: {}", syscall))
        } else {
            None
        }
    }

    pub fn analyze_resource_usage(&self, memory_mb: u64, limit_mb: Option<u64>) -> Option<String> {
        if let Some(limit) = limit_mb {
            if memory_mb > limit {
                return Some(format!(
                    "Resource limit exceeded: {} > {}",
                    memory_mb, limit
                ));
            }
        }
        None
    }

    pub fn generate_report(&self, policy: &SecurityPolicy) -> Value {
        json!({
            "policy_name": policy.name,
            "blocked_syscalls": policy.denied_syscalls.len(),
            "file_rules": policy.file_access_rules.len(),
            "network_restricted": !policy.network_rules[0].allow_outbound,
            "capabilities_limited": policy.resource_limits.max_memory_mb.is_some(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_policy_check() {
        let policy = SecurityPolicy::default_restrictive();
        assert!(policy.check_syscall("read"));
        assert!(!policy.check_syscall("clone"));
    }

    #[test]
    fn test_capability_set() {
        let caps = CapabilitySet::minimal();
        assert!(caps.has_capability("CAP_CHOWN"));
        assert!(!caps.has_capability("CAP_SYS_ADMIN"));
    }

    #[test]
    fn test_seccomp_filter_generation() {
        let policy = SecurityPolicy::default_restrictive();
        let filter = SeccompFilter::new(policy);
        let rules = filter.to_bpf_rules();
        assert!(!rules["rules"].is_null());
    }

    #[test]
    fn test_set_policy_restrictive() {
        let policy = SecurityPolicy::default_restrictive();
        assert_eq!(policy.name, "restrictive");
        assert!(policy.check_syscall("read"));
        assert!(!policy.check_syscall("clone"));
    }

    #[test]
    fn test_set_policy_permissive() {
        let policy = SecurityPolicy::default_permissive();
        assert_eq!(policy.name, "permissive");
        // Permissive policies should allow more syscalls
        assert!(policy.check_syscall("read"));
    }

    #[test]
    fn test_update_security_policy() {
        let mut policy = SecurityPolicy::default_restrictive();
        assert_eq!(policy.allow_shell_execution, false);
        
        // Verify policy can be modified
        policy.allow_shell_execution = true;
        assert!(policy.allow_shell_execution);
    }
}
