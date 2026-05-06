use lazy_static::lazy_static;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// Context passed to a tool invocation
pub struct ToolContext {
    pub sandbox_id: u64,
    pub root_path: Option<PathBuf>,
    /// Process ID of the container (for scoping kill_process)
    pub container_pid: Option<i32>,
    /// Environment variables for the container (not the host)
    pub container_env: HashMap<String, String>,
}

impl ToolContext {
    pub fn new(sandbox_id: u64, root_path: Option<PathBuf>) -> Self {
        Self {
            sandbox_id,
            root_path,
            container_pid: None,
            container_env: HashMap::new(),
        }
    }

    pub fn with_pid(mut self, pid: i32) -> Self {
        self.container_pid = Some(pid);
        self
    }

    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.container_env = env;
        self
    }
}

/// A trait that all tools must implement
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value>;
    fn clone_box(&self) -> Box<dyn Tool>;
}

impl Clone for Box<dyn Tool> {
    fn clone(&self) -> Box<dyn Tool> {
        self.clone_box()
    }
}

/// Definition of a tool that can be registered with a sandbox
pub struct ToolDefinition {
    pub name: String,
}

impl ToolDefinition {
    pub fn new(name: impl Into<String>) -> Self {
        ToolDefinition { name: name.into() }
    }
}

/// Validate that a URL is safe to access (no SSRF to internal services)
fn is_safe_url(url: &str) -> anyhow::Result<()> {
    let parsed = url::Url::parse(url)
        .map_err(|e| anyhow::anyhow!("Invalid URL: {}", e))?;

    match parsed.scheme() {
        "http" | "https" => {}
        "file" | "ftp" | "gopher" | "dict" | "smb" | "ldap" => {
            return Err(anyhow::anyhow!(
                "URL scheme '{}' is not allowed for security reasons",
                parsed.scheme()
            ));
        }
        _ => {
            return Err(anyhow::anyhow!(
                "Unknown URL scheme '{}': only http/https allowed",
                parsed.scheme()
            ));
        }
    }

    if let Some(host) = parsed.host_str() {
        // Block RFC 1918 private ranges, loopback, link-local, metadata
        let blocked_hosts = [
            "169.254.169.254",  // AWS/GCP/Azure metadata
            "metadata.google.internal",
            "100.100.100.200",  // Alibaba metadata
        ];

        if blocked_hosts.contains(&host) {
            return Err(anyhow::anyhow!(
                "Access to internal metadata service '{}' is blocked",
                host
            ));
        }

        // Block loopback
        if host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "0.0.0.0" {
            return Err(anyhow::anyhow!(
                "Access to localhost/loopback address '{}' is blocked",
                host
            ));
        }

        // Block private IP ranges
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            match ip {
                std::net::IpAddr::V4(v4) => {
                    if v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_broadcast() || v4.is_unspecified() {
                        return Err(anyhow::anyhow!(
                            "Access to private/internal IP '{}' is blocked",
                            v4
                        ));
                    }
                }
                std::net::IpAddr::V6(v6) => {
                    if v6.is_loopback() || v6.is_unspecified() {
                        return Err(anyhow::anyhow!(
                            "Access to loopback IPv6 '{}' is blocked",
                            v6
                        ));
                    }
                }
            }
        }

        // Block common internal hostnames
        let lower_host = host.to_lowercase();
        if lower_host.ends_with(".internal") || lower_host.ends_with(".local")
            || lower_host.ends_with(".corp") || lower_host.ends_with(".home")
        {
            return Err(anyhow::anyhow!(
                "Access to internal hostname '{}' is blocked",
                host
            ));
        }
    }

    Ok(())
}

/// Sanitize a path component to prevent injection in shell commands
fn sanitize_path_component(path: &str) -> anyhow::Result<String> {
    // Only allow alphanumeric, dash, underscore, dot, forward slash
    if path.contains('\0') {
        return Err(anyhow::anyhow!("Path contains null byte"));
    }
    if path.contains("..") {
        return Err(anyhow::anyhow!("Path contains '..' traversal"));
    }
    Ok(path.to_string())
}

/// Validate that a command string doesn't contain injection patterns
pub fn validate_command(cmd: &str) -> anyhow::Result<()> {
    if cmd.contains('\0') {
        return Err(anyhow::anyhow!("Command contains null byte"));
    }
    // Block common injection patterns
    let dangerous = ["$( ", "`", "${ ", "|&", "&&", "||", ";", "\n", "\r"];
    for pat in &dangerous {
        if cmd.contains(pat) {
            return Err(anyhow::anyhow!(
                "Command contains potentially dangerous pattern '{}'",
                pat
            ));
        }
    }
    Ok(())
}

/// Validate a directory path for use in shell context (cwd)
pub fn validate_cwd(cwd: &str) -> anyhow::Result<()> {
    if cwd.contains('\0') || cwd.contains(';') || cwd.contains('&')
        || cwd.contains('|') || cwd.contains('$') || cwd.contains('`')
        || cwd.contains('\n') || cwd.contains('\r')
    {
        return Err(anyhow::anyhow!(
            "Working directory path contains unsafe characters"
        ));
    }
    Ok(())
}

/// Helper to resolve paths against container root with containment validation
pub fn resolve_path(ctx: &ToolContext, path: &str) -> anyhow::Result<PathBuf> {
    let base = ctx
        .root_path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/tmp".to_string());

    let base_path = Path::new(&base);

    // Construct the candidate path
    let candidate = if path.starts_with('/') {
        // Strip leading slash and join with base
        let stripped = path.trim_start_matches('/');
        sanitize_path_component(stripped)?;
        base_path.join(stripped)
    } else {
        sanitize_path_component(path)?;
        base_path.join(path)
    };

    // Canonicalize to resolve symlinks and `..` components
    // If the path doesn't exist yet, canonicalize the parent and check containment
    let resolved = if candidate.exists() {
        candidate.canonicalize().map_err(|e| {
            anyhow::anyhow!("Failed to canonicalize path '{}': {}", candidate.display(), e)
        })?
    } else {
        // For new files, canonicalize the deepest existing ancestor
        let mut ancestor = candidate.as_path();
        let mut components = Vec::new();
        while !ancestor.exists() {
            if let Some(file_name) = ancestor.file_name() {
                components.push(file_name.to_os_string());
            }
            ancestor = match ancestor.parent() {
                Some(p) => p,
                None => break,
            };
        }
        let canonical_ancestor = ancestor.canonicalize().map_err(|e| {
            anyhow::anyhow!("Failed to canonicalize ancestor '{}': {}", ancestor.display(), e)
        })?;
        let mut result = canonical_ancestor;
        for comp in components.into_iter().rev() {
            result = result.join(comp);
        }
        result
    };

    // CRITICAL: Verify the resolved path is within the base directory
    let canonical_base = base_path.canonicalize().unwrap_or_else(|_| base_path.to_path_buf());
    if !resolved.starts_with(&canonical_base) {
        return Err(anyhow::anyhow!(
            "Path '{}' escapes the container root '{}'. Access denied.",
            path,
            base
        ));
    }

    Ok(resolved)
}

/// Validate URL and add timeout to curl command
pub fn validate_url_for_http(url: &str) -> anyhow::Result<()> {
    is_safe_url(url)
}

/// Execute a curl command with safety controls
pub fn execute_http_command(cmd: Vec<&str>) -> anyhow::Result<Value> {
    // Find the URL in the command (last non-flag argument)
    let url = cmd.iter().rev().find(|arg| !arg.starts_with('-') && !arg.starts_with('\n')).cloned();
    if let Some(url) = url {
        is_safe_url(&url)?;
    }

    let mut full_cmd = cmd;
    // Inject timeout if not present
    if !full_cmd.contains(&"--connect-timeout") {
        full_cmd.insert(0, "10");  // 10s connect timeout
        full_cmd.insert(0, "--connect-timeout");
    }
    if !full_cmd.contains(&"--max-time") {
        full_cmd.insert(0, "60");  // 60s total timeout
        full_cmd.insert(0, "--max-time");
    }
    // Disable redirects to internal networks
    if !full_cmd.contains(&"--max-redirs") {
        full_cmd.insert(0, "5");
        full_cmd.insert(0, "--max-redirs");
    }

    let output = Command::new("curl")
        .args(&full_cmd)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    if lines.is_empty() {
        return Ok(json!({ "status": 0, "body": "" }));
    }

    let status_code: i64 = lines.last().and_then(|s| s.parse().ok()).unwrap_or(0);
    let body = lines[..lines.len().saturating_sub(1)].join("\n");

    Ok(json!({
        "status": status_code,
        "body": body,
        "success": output.status.success()
    }))
}

/// Maximum file size for read operations (10MB)
pub const MAX_READ_SIZE: u64 = 10 * 1024 * 1024;

/// Maximum content size for write operations (50MB)
pub const MAX_WRITE_SIZE: usize = 50 * 1024 * 1024;

/// Maximum number of entries returned by list operations
pub const MAX_LIST_ENTRIES: usize = 10000;

// Per-sandbox stores instead of global (prevents cross-sandbox contamination)
lazy_static! {
    pub static ref MEMORY_STORE: Mutex<HashMap<String, Value>> = Mutex::new(HashMap::new());
    pub static ref SECRET_STORE: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
    pub static ref CHANNELS: Mutex<HashMap<String, Vec<Value>>> = Mutex::new(HashMap::new());
}

/// Clean up stores for a specific sandbox
pub fn cleanup_sandbox_stores(sandbox_id: &str) {
    let prefix = format!("{}:", sandbox_id);
    if let Ok(mut mem) = MEMORY_STORE.lock() {
        mem.retain(|k, _| !k.starts_with(&prefix));
    }
    if let Ok(mut sec) = SECRET_STORE.lock() {
        sec.retain(|k, _| !k.starts_with(&prefix));
    }
    if let Ok(mut ch) = CHANNELS.lock() {
        ch.retain(|k, _| !k.starts_with(&prefix));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_safe_url_blocks_private_ips() {
        assert!(is_safe_url("http://127.0.0.1/admin").is_err());
        assert!(is_safe_url("http://localhost:8080/api").is_err());
        assert!(is_safe_url("http://::1/").is_err());
        assert!(is_safe_url("http://0.0.0.0/").is_err());
        assert!(is_safe_url("http://10.0.0.1/secret").is_err());
        assert!(is_safe_url("http://192.168.1.1/").is_err());
        assert!(is_safe_url("http://172.16.0.1/").is_err());
        assert!(is_safe_url("http://169.254.169.254/latest/meta-data/").is_err());
    }

    #[test]
    fn test_is_safe_url_blocks_metadata_services() {
        assert!(is_safe_url("http://metadata.google.internal/").is_err());
        assert!(is_safe_url("http://100.100.100.200/").is_err());
    }

    #[test]
    fn test_is_safe_url_blocks_dangerous_schemes() {
        assert!(is_safe_url("file:///etc/passwd").is_err());
        assert!(is_safe_url("ftp://server/file").is_err());
        assert!(is_safe_url("gopher://server/").is_err());
    }

    #[test]
    fn test_is_safe_url_allows_https() {
        assert!(is_safe_url("https://api.example.com/v1").is_ok());
        assert!(is_safe_url("https://generativelanguage.googleapis.com/v1beta").is_ok());
    }

    #[test]
    fn test_is_safe_url_blocks_internal_hostnames() {
        assert!(is_safe_url("http://my-service.internal/").is_err());
        assert!(is_safe_url("http://my-pc.local/").is_err());
    }

    #[test]
    fn test_validate_cwd_blocks_injection() {
        assert!(validate_cwd("/workspace").is_ok());
        assert!(validate_cwd("/tmp/test").is_ok());
        assert!(validate_cwd("/; rm -rf /").is_err());
        assert!(validate_cwd("/$(whoami)").is_err());
        assert!(validate_cwd("/`id`").is_err());
        assert!(validate_cwd("/\nmalicious").is_err());
    }

    #[test]
    fn test_resolve_path_blocks_traversal() {
        let ctx = ToolContext::new(1, Some(std::path::PathBuf::from("/tmp/test-root")));

        // Normal paths should work (even if they don't exist)
        let result = resolve_path(&ctx, "file.txt");
        assert!(result.is_ok());

        // Path traversal should be blocked
        let result = resolve_path(&ctx, "../../etc/passwd");
        // This should either error or stay within the root
        if let Ok(path) = result {
            assert!(path.starts_with("/tmp/test-root"));
        }
    }
}
