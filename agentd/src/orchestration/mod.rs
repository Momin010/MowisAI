//! Multi-sandbox orchestration: New 7-layer orchestration system

// NEW 7-LAYER ARCHITECTURE
pub mod types;
pub mod sandbox_topology;
pub mod scheduler;
pub mod planner;
pub mod checkpoint;
pub mod merge_worker;
pub mod merge_reviewer;
pub mod verification;
pub mod agent_execution;
pub mod new_orchestrator;
pub mod mock_agent;
pub mod simulate;
pub mod health;

// Re-export main types
pub use new_orchestrator::{NewOrchestrator, OrchestratorConfig, FinalOutput, OrchestratorEvent};
pub use agent_execution::{set_verbose, is_verbose};

// KEEP: Still needed files
pub mod session_store;
pub mod sandbox_profiles;

/// Long-running generateContent calls (large outputs / tool loops).
pub(crate) const HTTP_TIMEOUT_SECS: u64 = 900;

/// Safety cap for tool-calling loops only (each round is one API call). Raise if needed.
pub(crate) const MAX_TOOL_ROUNDS: usize = 256;

/// Context-gatherer tool rounds (Layer 1).
pub(crate) const MAX_CONTEXT_GATHER_ROUNDS: usize = 128;

/// `maxOutputTokens` for Vertex `generateContent`. The API still applies per-model server-side limits.
pub(crate) const VERTEX_MAX_OUTPUT_TOKENS: u32 = 65_536;

/// Gemini 2.5 “thinking” budget (tokens). Omit by setting to 0 if your endpoint rejects the field.
pub(crate) const VERTEX_THINKING_BUDGET_TOKENS: u32 = 24_576;

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

// ─────────────────────────────────────────────────────────────────────────────
// Unix-domain socket connection pool
// ─────────────────────────────────────────────────────────────────────────────
//
// Each unique socket path gets one pool. Connections are lazily created up to
// `max_size` (default 32). When the pool is exhausted, callers block on a
// condvar up to `POOL_WAIT` rather than opening uncapped raw connections.
// `PooledConn` is an RAII guard: on drop it probes liveness via MSG_PEEK and
// either returns the stream to the idle queue or discards it.
//
// NOTE: The agentd socket server closes each connection after one request, so
// `is_alive()` will always return false with the production server.  The pool
// therefore acts as a bounded concurrency semaphore in that mode.  With any
// persistent-connection test server, full reuse works automatically.

#[cfg(unix)]
pub(crate) mod pool {
    use dashmap::DashMap;
    use parking_lot::{Condvar, Mutex};
    use std::collections::VecDeque;
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    /// Default maximum connections per socket path.
    pub const DEFAULT_POOL_SIZE: usize = 32;

    static POOL_SIZE: AtomicUsize = AtomicUsize::new(DEFAULT_POOL_SIZE);
    /// Default wait before giving up when every slot is in use (5 minutes).
    static POOL_WAIT_MS: AtomicU64 = AtomicU64::new(300_000);

    /// Override the pool size used when a *new* pool is created for a path.
    /// Already-existing pools keep their original size.
    pub fn set_pool_size(n: usize) {
        assert!(n > 0, "pool size must be > 0");
        POOL_SIZE.store(n, Ordering::Relaxed);
    }

    /// Current configured pool size.
    pub fn pool_size() -> usize {
        POOL_SIZE.load(Ordering::Relaxed)
    }

    /// Override how long `acquire` waits when the pool is exhausted.
    /// Useful in tests to avoid multi-minute waits.
    pub fn set_wait_timeout(d: Duration) {
        POOL_WAIT_MS.store(d.as_millis() as u64, Ordering::Relaxed);
    }

    fn wait_timeout() -> Duration {
        Duration::from_millis(POOL_WAIT_MS.load(Ordering::Relaxed))
    }

    // ── Inner state ──────────────────────────────────────────────────────────

    struct Inner {
        idle: VecDeque<UnixStream>,
        in_use: usize,
    }

    impl Inner {
        /// Total connections that exist (idle + checked-out).
        fn total(&self) -> usize {
            self.idle.len() + self.in_use
        }
    }

    // ── SocketPool ───────────────────────────────────────────────────────────

    pub struct SocketPool {
        path: String,
        max_size: usize,
        inner: Mutex<Inner>,
        returned: Condvar,
    }

    impl SocketPool {
        pub fn new(path: String, max_size: usize) -> Arc<Self> {
            Arc::new(SocketPool {
                path,
                max_size,
                inner: Mutex::new(Inner {
                    idle: VecDeque::new(),
                    in_use: 0,
                }),
                returned: Condvar::new(),
            })
        }

        /// Acquire a connection, waiting up to the configured timeout.
        pub fn acquire(self: &Arc<Self>) -> anyhow::Result<PooledConn> {
            self.acquire_timeout(wait_timeout())
        }

        /// Acquire a connection, waiting up to `timeout`.
        /// Returns `Err` if `timeout` elapses while all slots are in use.
        pub fn acquire_timeout(
            self: &Arc<Self>,
            timeout: Duration,
        ) -> anyhow::Result<PooledConn> {
            let deadline = Instant::now() + timeout;

            loop {
                let mut guard = self.inner.lock();

                // ① Take an idle (already-open) connection.
                if let Some(stream) = guard.idle.pop_front() {
                    guard.in_use += 1;
                    return Ok(PooledConn {
                        pool: self.clone(),
                        stream: Some(stream),
                        dead: false,
                    });
                }

                // ② Create a new connection if below the cap.
                if guard.total() < self.max_size {
                    guard.in_use += 1;
                    drop(guard); // release lock before blocking connect()
                    match UnixStream::connect(&self.path) {
                        Ok(stream) => {
                            return Ok(PooledConn {
                                pool: self.clone(),
                                stream: Some(stream),
                                dead: false,
                            });
                        }
                        Err(e) => {
                            // Roll back the in_use increment we just took.
                            let mut g = self.inner.lock();
                            g.in_use -= 1;
                            drop(g);
                            self.returned.notify_one();
                            return Err(anyhow::anyhow!("pool connect to {}: {}", self.path, e));
                        }
                    }
                }

                // ③ Pool exhausted — wait for a slot to be returned.
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(anyhow::anyhow!(
                        "connection pool exhausted: all {} slots busy for path {}",
                        self.max_size,
                        self.path
                    ));
                }
                if self.returned.wait_for(&mut guard, remaining).timed_out() {
                    return Err(anyhow::anyhow!(
                        "connection pool exhausted: timed out after {:?} (pool_size={})",
                        timeout,
                        self.max_size
                    ));
                }
                // Spurious wakeup or a slot was genuinely returned — loop again.
            }
        }

        /// Return a healthy stream to the idle queue.
        fn put_back(&self, stream: UnixStream) {
            {
                let mut g = self.inner.lock();
                g.in_use -= 1;
                g.idle.push_back(stream);
            }
            self.returned.notify_one();
        }

        /// Discard a dead connection, freeing its slot.
        fn discard(&self) {
            {
                let mut g = self.inner.lock();
                g.in_use -= 1;
            }
            self.returned.notify_one();
        }

        /// Snapshot of pool counters (for tests / monitoring).
        pub fn stats(&self) -> PoolStats {
            let g = self.inner.lock();
            PoolStats {
                idle: g.idle.len(),
                in_use: g.in_use,
                max_size: self.max_size,
            }
        }
    }

    /// Counters returned by `SocketPool::stats`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PoolStats {
        pub idle: usize,
        pub in_use: usize,
        pub max_size: usize,
    }

    // ── RAII connection guard ─────────────────────────────────────────────────

    /// A checked-out connection.  Dropping it returns the stream to the pool
    /// (if still alive) or frees the slot (if dead).
    pub struct PooledConn {
        pool: Arc<SocketPool>,
        stream: Option<UnixStream>,
        /// True once the caller has determined the stream is unusable.
        dead: bool,
    }

    impl PooledConn {
        /// Mutable access to the underlying stream.
        pub fn stream_mut(&mut self) -> &mut UnixStream {
            self.stream.as_mut().expect("stream already consumed")
        }

        /// Mark this connection as dead so it is discarded on drop instead of
        /// being returned to the idle queue.
        pub fn kill(&mut self) {
            self.dead = true;
        }
    }

    impl Drop for PooledConn {
        fn drop(&mut self) {
            if let Some(stream) = self.stream.take() {
                if !self.dead && is_alive(&stream) {
                    self.pool.put_back(stream);
                } else {
                    drop(stream);
                    self.pool.discard();
                }
            }
        }
    }

    // ── Liveness probe ───────────────────────────────────────────────────────

    /// Non-blocking peek: returns `true` if the socket is still open on the
    /// remote end (WouldBlock), `false` on EOF or any error.
    fn is_alive(stream: &UnixStream) -> bool {
        if stream.set_nonblocking(true).is_err() {
            return false;
        }
        let mut buf = [0u8; 1];
        let alive = match stream.peek(&mut buf) {
            Ok(0) => false, // remote sent FIN
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => true,
            _ => false,
        };
        let _ = stream.set_nonblocking(false);
        alive
    }

    // ── Global per-path registry ─────────────────────────────────────────────

    lazy_static::lazy_static! {
        static ref POOLS: DashMap<String, Arc<SocketPool>> = DashMap::new();
    }

    /// Get the pool for `path`, creating it (with the current `pool_size()`)
    /// on first call.
    pub fn get_pool(path: &str) -> Arc<SocketPool> {
        POOLS
            .entry(path.to_string())
            .or_insert_with(|| SocketPool::new(path.to_string(), pool_size()))
            .clone()
    }

    /// Remove (and drain) the pool for `path`.  Primarily for tests.
    pub fn remove_pool(path: &str) {
        POOLS.remove(path);
    }

    /// Stats for the pool at `path`, or `None` if it hasn't been created yet.
    pub fn pool_stats(path: &str) -> Option<PoolStats> {
        POOLS.get(path).map(|p| p.stats())
    }
}

/// Enable/disable verbose orchestration logging (socket/HTTP payloads, round timings, etc).
/// Normal mode prints only high-signal CLI events (tool calls / file ops).
pub fn set_debug(enabled: bool) {
    DEBUG_ENABLED.store(enabled, Ordering::Relaxed);
}

pub(crate) fn debug_enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

/// Standard generation block for text / tools (no JSON mode).
pub(crate) fn vertex_generation_config(temperature: f64) -> Value {
    if VERTEX_THINKING_BUDGET_TOKENS == 0 {
        return json!({
            "temperature": temperature,
            "maxOutputTokens": VERTEX_MAX_OUTPUT_TOKENS
        });
    }
    json!({
        "temperature": temperature,
        "maxOutputTokens": VERTEX_MAX_OUTPUT_TOKENS,
        "thinkingConfig": {
            "thinkingBudget": VERTEX_THINKING_BUDGET_TOKENS
        }
    })
}

/// Like [`vertex_generation_config`] but requests JSON-only responses (architect / planner).
pub(crate) fn vertex_generation_config_json(temperature: f64) -> Value {
    if VERTEX_THINKING_BUDGET_TOKENS == 0 {
        return json!({
            "temperature": temperature,
            "maxOutputTokens": VERTEX_MAX_OUTPUT_TOKENS,
            "responseMimeType": "application/json"
        });
    }
    json!({
        "temperature": temperature,
        "maxOutputTokens": VERTEX_MAX_OUTPUT_TOKENS,
        "responseMimeType": "application/json",
        "thinkingConfig": {
            "thinkingBudget": VERTEX_THINKING_BUDGET_TOKENS
        }
    })
}

pub(crate) fn trace(msg: &str) {
    if !debug_enabled() {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    log::info!("[orchestration:{}] {}", ts, msg);
}

// ── Vertex / gcloud (shared by planner, agent_runner, orchestrator) ────────

#[cfg(unix)]
pub(crate) fn gcloud_access_token() -> anyhow::Result<String> {
    use anyhow::{anyhow, Context};
    use std::process::Command;
    trace("gcloud auth print-access-token: starting");
    let out = Command::new("gcloud")
        .args(["auth", "print-access-token"])
        .output()
        .context("spawn gcloud — is it installed and on PATH? Install with: gcloud auth application-default login")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = if stderr.contains("Application Default Credentials") {
            format!(
                "gcloud authentication not configured. Error: {}\n\
                 Fix: Run 'gcloud auth application-default login' or set GOOGLE_APPLICATION_CREDENTIALS",
                stderr
            )
        } else if stderr.contains("permission denied") || stderr.contains("not found") {
            format!(
                "gcloud command not found or not executable. Error: {}",
                stderr
            )
        } else {
            format!(
                "gcloud auth failed: {}\n\
                 Ensure you have: 1) gcloud installed, 2) logged in with 'gcloud auth login', \
                 3) application default credentials with 'gcloud auth application-default login'",
                stderr
            )
        };
        return Err(anyhow!(msg));
    }
    let s = String::from_utf8(out.stdout).context("token utf-8")?;
    let t = s.trim().to_string();
    if t.is_empty() {
        return Err(anyhow!(
            "empty access token from gcloud. This usually means authentication failed. \
             Try: gcloud auth application-default login"
        ));
    }
    trace(&format!(
        "gcloud auth print-access-token: OAuth access token length={} chars (not Gemini output)",
        t.len()
    ));
    Ok(t)
}

#[cfg(not(unix))]
pub(crate) fn gcloud_access_token() -> anyhow::Result<String> {
    Err(anyhow::anyhow!(
        "orchestration requires Unix (agentd uses Unix domain sockets)"
    ))
}

pub(crate) fn vertex_generate_url(project_id: &str) -> String {
    format!(
        "https://us-central1-aiplatform.googleapis.com/v1/projects/{}/locations/us-central1/publishers/google/models/gemini-2.5-pro:generateContent",
        project_id
    )
}

/// Same five tools as `vertex_agent.rs` / agentd socket.
pub(crate) fn gemini_tool_declarations() -> serde_json::Value {
    use serde_json::json;
    json!([
        {
            "name": "read_file",
            "description": "Read a file from the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to read" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "write_file",
            "description": "Write text content to a file path in the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to write" },
                    "content": { "type": "string", "description": "Text content to write" }
                },
                "required": ["path", "content"]
            }
        },
        {
            "name": "append_file",
            "description": "Append text content to a file path in the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to append to" },
                    "content": { "type": "string", "description": "Text content to append" }
                },
                "required": ["path", "content"]
            }
        },
        {
            "name": "delete_file",
            "description": "Delete a file from the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to delete" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "copy_file",
            "description": "Copy a file from one path to another inside the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Source file path" },
                    "to": { "type": "string", "description": "Destination file path" }
                },
                "required": ["from", "to"]
            }
        },
        {
            "name": "move_file",
            "description": "Move (rename) a file from one path to another inside the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Source file path" },
                    "to": { "type": "string", "description": "Destination file path" }
                },
                "required": ["from", "to"]
            }
        },
        {
            "name": "list_files",
            "description": "List files and subdirectories in a directory.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path to list" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "create_directory",
            "description": "Create a directory (and parents) in the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path to create" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "delete_directory",
            "description": "Delete a directory and its contents from the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path to delete" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "get_file_info",
            "description": "Get information about a file in the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to inspect" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "file_exists",
            "description": "Check whether a file exists in the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to check" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "run_command",
            "description": "Run a shell command inside the sandbox (chroot).",
            "parameters": {
                "type": "object",
                "properties": {
                    "cmd": { "type": "string", "description": "Shell command to run" },
                    "cwd": { "type": "string", "description": "Optional working directory" }
                },
                "required": ["cmd"]
            }
        },
        {
            "name": "run_script",
            "description": "Run a script inside the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Script path" },
                    "interpreter": { "type": "string", "description": "Optional interpreter (e.g. python3, bash)" },
                    "script": { "type": "string", "description": "Optional inline script content" },
                    "language": { "type": "string", "description": "Optional script language" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "kill_process",
            "description": "Kill a process by PID inside the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pid": { "type": "integer", "description": "Process ID to kill" }
                },
                "required": ["pid"]
            }
        },
        {
            "name": "get_env",
            "description": "Get an environment variable inside the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "var": { "type": "string", "description": "Environment variable name" }
                },
                "required": ["var"]
            }
        },
        {
            "name": "set_env",
            "description": "Set an environment variable inside the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "var": { "type": "string", "description": "Environment variable name" },
                    "value": { "type": "string", "description": "Environment variable value" }
                },
                "required": ["var", "value"]
            }
        },
        {
            "name": "http_get",
            "description": "Perform an HTTP GET request.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to GET" }
                },
                "required": ["url"]
            }
        },
        {
            "name": "http_post",
            "description": "Perform an HTTP POST request.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to POST" },
                    "body": { "type": "string", "description": "Request body" }
                },
                "required": ["url", "body"]
            }
        },
        {
            "name": "http_put",
            "description": "Perform an HTTP PUT request.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to PUT" },
                    "body": { "type": "string", "description": "Request body" }
                },
                "required": ["url", "body"]
            }
        },
        {
            "name": "http_delete",
            "description": "Perform an HTTP DELETE request.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to DELETE" }
                },
                "required": ["url"]
            }
        },
        {
            "name": "http_patch",
            "description": "Perform an HTTP PATCH request.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to PATCH" },
                    "body": { "type": "string", "description": "Request body" }
                },
                "required": ["url", "body"]
            }
        },
        {
            "name": "download_file",
            "description": "Download a file from a URL into the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "File URL to download" },
                    "path": { "type": "string", "description": "Destination path in the sandbox" }
                },
                "required": ["url", "path"]
            }
        },
        {
            "name": "websocket_send",
            "description": "Send a message to a WebSocket URL.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "WebSocket URL" },
                    "message": { "type": "string", "description": "Message to send" }
                },
                "required": ["url", "message"]
            }
        },
        {
            "name": "json_parse",
            "description": "Parse JSON text into a JSON object/value.",
            "parameters": {
                "type": "object",
                "properties": {
                    "data": { "type": "string", "description": "JSON input string" }
                },
                "required": ["data"]
            }
        },
        {
            "name": "json_stringify",
            "description": "Stringify JSON value into text.",
            "parameters": {
                "type": "object",
                "properties": {
                    "data": { "type": "string", "description": "JSON input value (as string or JSON)" }
                },
                "required": ["data"]
            }
        },
        {
            "name": "json_query",
            "description": "Query a JSON value using a path expression.",
            "parameters": {
                "type": "object",
                "properties": {
                    "data": { "type": "string", "description": "JSON data to query" },
                    "path": { "type": "string", "description": "Query path" }
                },
                "required": ["data", "path"]
            }
        },
        {
            "name": "csv_read",
            "description": "Read a CSV file from the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "CSV file path" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "csv_write",
            "description": "Write CSV rows to a file in the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "CSV output path" },
                    "rows": { "type": "array", "description": "CSV rows" }
                },
                "required": ["path", "rows"]
            }
        },
        {
            "name": "git_clone",
            "description": "Clone a git repository into the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "repo": { "type": "string", "description": "Repository URL" },
                    "path": { "type": "string", "description": "Destination path" }
                },
                "required": ["repo", "path"]
            }
        },
        {
            "name": "git_status",
            "description": "Get git status for a repository path.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "git_add",
            "description": "Stage files in a git repository.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" },
                    "files": { "type": "array", "description": "Files to stage" }
                },
                "required": ["path", "files"]
            }
        },
        {
            "name": "git_commit",
            "description": "Create a git commit in a repository.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" },
                    "message": { "type": "string", "description": "Commit message" }
                },
                "required": ["path", "message"]
            }
        },
        {
            "name": "git_push",
            "description": "Push commits to a remote repository.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" },
                    "remote": { "type": "string", "description": "Remote name (e.g. origin)" },
                    "branch": { "type": "string", "description": "Branch name" }
                },
                "required": ["path", "remote", "branch"]
            }
        },
        {
            "name": "git_pull",
            "description": "Pull updates from a remote repository.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" },
                    "remote": { "type": "string", "description": "Remote name (e.g. origin)" },
                    "branch": { "type": "string", "description": "Branch name" }
                },
                "required": ["path", "remote", "branch"]
            }
        },
        {
            "name": "git_branch",
            "description": "Create or list branches in a git repository.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" },
                    "name": { "type": "string", "description": "Optional branch name" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "git_checkout",
            "description": "Checkout a branch in a git repository.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" },
                    "branch": { "type": "string", "description": "Branch name" }
                },
                "required": ["path", "branch"]
            }
        },
        {
            "name": "git_diff",
            "description": "Get git diff for a repository path.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "docker_build",
            "description": "Build a Docker image.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Docker build context path" },
                    "tag": { "type": "string", "description": "Image tag" }
                },
                "required": ["path", "tag"]
            }
        },
        {
            "name": "docker_run",
            "description": "Run a Docker image.",
            "parameters": {
                "type": "object",
                "properties": {
                    "image": { "type": "string", "description": "Docker image name" },
                    "cmd": { "type": "string", "description": "Optional command override" },
                    "name": { "type": "string", "description": "Optional container name" }
                },
                "required": ["image"]
            }
        },
        {
            "name": "docker_stop",
            "description": "Stop a Docker container.",
            "parameters": {
                "type": "object",
                "properties": {
                    "container": { "type": "string", "description": "Container id/name" }
                },
                "required": ["container"]
            }
        },
        {
            "name": "docker_ps",
            "description": "List Docker containers.",
            "parameters": {
                "type": "object",
                "properties": {
                    "all": { "type": "boolean", "description": "Optional: include stopped containers" }
                },
                "required": []
            }
        },
        {
            "name": "docker_logs",
            "description": "Get logs for a Docker container.",
            "parameters": {
                "type": "object",
                "properties": {
                    "container": { "type": "string", "description": "Container id/name" }
                },
                "required": ["container"]
            }
        },
        {
            "name": "docker_exec",
            "description": "Execute a command inside a Docker container.",
            "parameters": {
                "type": "object",
                "properties": {
                    "container": { "type": "string", "description": "Container id/name" },
                    "cmd": { "type": "string", "description": "Command to execute" }
                },
                "required": ["container", "cmd"]
            }
        },
        {
            "name": "docker_pull",
            "description": "Pull a Docker image.",
            "parameters": {
                "type": "object",
                "properties": {
                    "image": { "type": "string", "description": "Image name" }
                },
                "required": ["image"]
            }
        },
        {
            "name": "kubectl_apply",
            "description": "Apply a Kubernetes manifest.",
            "parameters": {
                "type": "object",
                "properties": {
                    "manifest": { "type": "string", "description": "Kubernetes manifest YAML" }
                },
                "required": ["manifest"]
            }
        },
        {
            "name": "kubectl_get",
            "description": "Get Kubernetes resources.",
            "parameters": {
                "type": "object",
                "properties": {
                    "resource": { "type": "string", "description": "Resource type (e.g. pods)" }
                },
                "required": ["resource"]
            }
        },
        {
            "name": "kubectl_delete",
            "description": "Delete a Kubernetes resource.",
            "parameters": {
                "type": "object",
                "properties": {
                    "resource": { "type": "string", "description": "Resource type" },
                    "name": { "type": "string", "description": "Resource name" }
                },
                "required": ["resource", "name"]
            }
        },
        {
            "name": "kubectl_logs",
            "description": "Fetch logs from a Kubernetes pod.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pod": { "type": "string", "description": "Pod name" }
                },
                "required": ["pod"]
            }
        },
        {
            "name": "kubectl_exec",
            "description": "Execute a command in a Kubernetes pod.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pod": { "type": "string", "description": "Pod name" },
                    "cmd": { "type": "string", "description": "Command to execute" }
                },
                "required": ["pod", "cmd"]
            }
        },
        {
            "name": "kubectl_describe",
            "description": "Describe a Kubernetes resource.",
            "parameters": {
                "type": "object",
                "properties": {
                    "resource": { "type": "string", "description": "Resource type" },
                    "name": { "type": "string", "description": "Resource name" }
                },
                "required": ["resource", "name"]
            }
        },
        {
            "name": "memory_set",
            "description": "Store a key/value in persistent memory.",
            "parameters": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Memory key" },
                    "value": { "type": "string", "description": "Memory value" }
                },
                "required": ["key", "value"]
            }
        },
        {
            "name": "memory_get",
            "description": "Retrieve a value from persistent memory by key.",
            "parameters": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Memory key" }
                },
                "required": ["key"]
            }
        },
        {
            "name": "memory_delete",
            "description": "Delete a key from persistent memory.",
            "parameters": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Memory key" }
                },
                "required": ["key"]
            }
        },
        {
            "name": "memory_list",
            "description": "List all keys in persistent memory.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        },
        {
            "name": "memory_save",
            "description": "Save memory contents to a file.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Save path" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "memory_load",
            "description": "Load memory contents from a file.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Load path" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "secret_set",
            "description": "Store a secret value.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Secret name" },
                    "value": { "type": "string", "description": "Secret value" }
                },
                "required": ["name", "value"]
            }
        },
        {
            "name": "secret_get",
            "description": "Retrieve a secret value by name.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Secret name" }
                },
                "required": ["name"]
            }
        },
        {
            "name": "npm_install",
            "description": "Install an npm package (optionally from a working directory).",
            "parameters": {
                "type": "object",
                "properties": {
                    "package": { "type": "string", "description": "Optional package name" },
                    "cwd": { "type": "string", "description": "Optional working directory" }
                },
                "required": []
            }
        },
        {
            "name": "pip_install",
            "description": "Install a Python package via pip (optionally with version).",
            "parameters": {
                "type": "object",
                "properties": {
                    "package": { "type": "string", "description": "Python package name" },
                    "version": { "type": "string", "description": "Optional version" }
                },
                "required": ["package"]
            }
        },
        {
            "name": "cargo_add",
            "description": "Add a dependency to a Rust project via cargo-edit.",
            "parameters": {
                "type": "object",
                "properties": {
                    "package": { "type": "string", "description": "Crate/package to add" },
                    "cwd": { "type": "string", "description": "Optional working directory" }
                },
                "required": ["package"]
            }
        },
        {
            "name": "web_search",
            "description": "Search the web for a query.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "web_fetch",
            "description": "Fetch a URL from the web.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to fetch" }
                },
                "required": ["url"]
            }
        },
        {
            "name": "web_screenshot",
            "description": "Take a screenshot of a URL.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to screenshot" },
                    "output": { "type": "string", "description": "Output path/filename" }
                },
                "required": ["url", "output"]
            }
        },
        {
            "name": "create_channel",
            "description": "Create a message bus channel.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Channel name" }
                },
                "required": ["name"]
            }
        },
        {
            "name": "send_message",
            "description": "Send a message to a channel on the message bus.",
            "parameters": {
                "type": "object",
                "properties": {
                    "channel": { "type": "string", "description": "Channel name" },
                    "message": { "type": "string", "description": "Message content" }
                },
                "required": ["channel", "message"]
            }
        },
        {
            "name": "read_messages",
            "description": "Read messages from a channel on the message bus.",
            "parameters": {
                "type": "object",
                "properties": {
                    "channel": { "type": "string", "description": "Channel name" }
                },
                "required": ["channel"]
            }
        },
        {
            "name": "broadcast",
            "description": "Broadcast a message to all subscribers in the message bus.",
            "parameters": {
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "Message content" }
                },
                "required": ["message"]
            }
        },
        {
            "name": "wait_for",
            "description": "Wait for messages on a channel for an optional timeout.",
            "parameters": {
                "type": "object",
                "properties": {
                    "channel": { "type": "string", "description": "Channel name" },
                    "timeout": { "type": "integer", "description": "Optional timeout in milliseconds/seconds" }
                },
                "required": ["channel"]
            }
        },
        {
            "name": "spawn_agent",
            "description": "Spawn a new agent task via the orchestrator/message bus.",
            "parameters": {
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "Agent task description/instruction" },
                    "tools": { "type": "array", "description": "Optional list of tool names" }
                },
                "required": ["task"]
            }
        },
        {
            "name": "lint",
            "description": "Run a linter over a project path.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project path" },
                    "language": { "type": "string", "description": "Optional language hint" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "test",
            "description": "Run tests for a project.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project path" },
                    "command": { "type": "string", "description": "Optional custom test command" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "build",
            "description": "Build a project (optionally with a custom command).",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project path" },
                    "command": { "type": "string", "description": "Optional custom build command" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "type_check",
            "description": "Run a type checker over a project.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project path" },
                    "language": { "type": "string", "description": "Optional language hint" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "format",
            "description": "Format code in a project path.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project path" },
                    "language": { "type": "string", "description": "Optional language hint" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "echo",
            "description": "Echo the provided message.",
            "parameters": {
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "Message to echo" }
                },
                "required": ["message"]
            }
        },
        {
            "name": "grep",
            "description": "Search files for a regex pattern recursively.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern to search for" },
                    "path": { "type": "string", "description": "Directory path to search" },
                    "include": { "type": "string", "description": "Optional file glob filter (e.g. *.rs)" },
                    "max_results": { "type": "integer", "description": "Optional max results (default 100)" }
                },
                "required": ["pattern", "path"]
            }
        },
        {
            "name": "find_files",
            "description": "Find files matching a name pattern in a directory tree.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Glob/name pattern to match files" },
                    "path": { "type": "string", "description": "Directory to search" },
                    "max_depth": { "type": "integer", "description": "Optional max directory depth (default 10)" }
                },
                "required": ["pattern", "path"]
            }
        },
        {
            "name": "search_code",
            "description": "Case-insensitive substring search across source files.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search string (case-insensitive)" },
                    "path": { "type": "string", "description": "Directory to search" },
                    "include": { "type": "string", "description": "Optional file glob filter (e.g. *.rs)" },
                    "context_lines": { "type": "integer", "description": "Optional context lines around match (default 2)" }
                },
                "required": ["query", "path"]
            }
        },
        {
            "name": "read_multiple_files",
            "description": "Read multiple files in one call and return their contents.",
            "parameters": {
                "type": "object",
                "properties": {
                    "paths": { "type": "array", "description": "Array of file paths to read" }
                },
                "required": ["paths"]
            }
        }
    ])
}

// ── Socket protocol (matches vertex_agent / socket_server) ───────────────────

#[cfg(not(unix))]
pub(crate) fn socket_roundtrip(
    _socket_path: &str,
    _req: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    Err(anyhow::anyhow!(
        "orchestration requires Unix (agentd uses Unix domain sockets)"
    ))
}

#[cfg(unix)]
pub(crate) fn socket_roundtrip(
    socket_path: &str,
    req: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    use anyhow::{anyhow, Context};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let req_type = req
        .get("request_type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let start = std::time::Instant::now();
    trace(&format!(
        "socket request -> {} (path={})",
        req_type, socket_path
    ));

    let mut line = serde_json::to_string(req).context("serialize request")?;
    line.push('\n');

    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 0..=3 {
        // Acquire from pool (blocks if all slots are in use, up to the configured timeout).
        let mut conn = pool::get_pool(socket_path).acquire()?;

        {
            let s = conn.stream_mut();
            s.set_read_timeout(Some(std::time::Duration::from_secs(60)))
                .context("set read timeout")?;
            s.set_write_timeout(Some(std::time::Duration::from_secs(10)))
                .context("set write timeout")?;
        }

        // Write phase — pass stream by explicit argument so the borrow ends before read phase.
        let write_result =
            (|s: &mut UnixStream| -> anyhow::Result<()> {
                let mut bytes_written = 0;
                let bytes = line.as_bytes();
                while bytes_written < bytes.len() {
                    match s.write(&bytes[bytes_written..]) {
                        Ok(0) => return Err(anyhow!("write socket: socket closed by server")),
                        Ok(n) => bytes_written += n,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(e) => return Err(anyhow!("write socket: {}", e)),
                    }
                }
                s.flush().map_err(|e| anyhow!("flush socket: {}", e))?;
                Ok(())
            })(conn.stream_mut());

        // Classify write errors by ErrorKind directly — no string matching
        let write_retryable = match &write_result {
            Err(e) => {
                let msg = e.to_string();
                msg.contains("connection reset")
                    || msg.contains("socket closed")
                    || msg.contains("broken pipe")
            }
            Ok(_) => false,
        };
        if let Err(err) = write_result {
            conn.kill();
            if attempt < 3 && write_retryable {
                std::thread::sleep(std::time::Duration::from_millis(200));
                last_error = Some(err);
                continue;
            }
            return Err(err);
        }

        // Read phase — classify ErrorKind directly, not via string matching.
        // BufReader borrow is scoped so conn is free again after this block.
        enum ReadOutcome {
            Ok(String),
            RetryableErr(anyhow::Error),
            FatalErr(anyhow::Error),
        }

        let outcome = (|s: &mut UnixStream| -> ReadOutcome {
            let mut reader = BufReader::new(s);
            let mut response_line = String::new();
            match reader.read_line(&mut response_line) {
                Ok(0) => ReadOutcome::RetryableErr(anyhow!("read socket: socket closed by server")),
                Ok(_) => ReadOutcome::Ok(response_line),
                Err(e) => match e.kind() {
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                        // EAGAIN / timeout — server busy, retry
                        ReadOutcome::RetryableErr(anyhow!("read socket: {}", e))
                    }
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe => {
                        ReadOutcome::RetryableErr(anyhow!("read socket: {}", e))
                    }
                    std::io::ErrorKind::Interrupted => {
                        // Spurious signal — retry immediately without counting attempt
                        ReadOutcome::RetryableErr(anyhow!("read socket: {}", e))
                    }
                    _ => ReadOutcome::FatalErr(anyhow!("read socket: {}", e)),
                },
            }
        })(conn.stream_mut());

        let response_line = match outcome {
            ReadOutcome::Ok(l) => l,
            ReadOutcome::RetryableErr(err) => {
                conn.kill();
                if attempt < 3 {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    last_error = Some(err);
                    continue;
                }
                return Err(err);
            }
            ReadOutcome::FatalErr(err) => {
                conn.kill();
                return Err(err);
            }
        };

        let trimmed = response_line.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("empty socket response for request: {}", req_type));
        }

        let parsed: serde_json::Value = serde_json::from_str(trimmed)
            .context(format!("parse socket JSON (request: {})", req_type))?;
        let status = parsed
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        if status == "error" {
            if let Some(error_msg) = parsed.get("error").and_then(|e| e.as_str()) {
                log::warn!("⚠️  Socket error for {}: {}", req_type, error_msg);
            }
        }

        trace(&format!(
            "socket response <- {} status={} elapsed_ms={}",
            req_type,
            status,
            start.elapsed().as_millis()
        ));

        return Ok(parsed);
        // conn drops here; PooledConn::drop calls is_alive() and either
        // returns the stream to the idle queue or frees the slot.
    }

    Err(last_error.unwrap_or_else(|| anyhow!("socket request failed after retries")))
}

#[cfg(not(unix))]
pub(crate) fn parse_ok_field(
    _resp: &serde_json::Value,
    _key: &str,
) -> anyhow::Result<String> {
    Err(anyhow::anyhow!(
        "orchestration requires Unix (agentd uses Unix domain sockets)"
    ))
}

#[cfg(unix)]
pub(crate) fn parse_ok_field(resp: &serde_json::Value, key: &str) -> anyhow::Result<String> {
    use anyhow::anyhow;
    if resp.get("status").and_then(|s| s.as_str()) != Some("ok") {
        let err = resp
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("unknown socket error");
        return Err(anyhow!("socket: {}", err));
    }
    let result = resp
        .get("result")
        .ok_or_else(|| anyhow!("socket response missing result"))?;
    if let Some(s) = result.get(key).and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    if let Some(n) = result.get(key).and_then(|v| v.as_u64()) {
        return Ok(n.to_string());
    }
    Err(anyhow!("result missing string/number field '{}'", key))
}

#[cfg(not(unix))]
pub(crate) fn invoke_tool_via_socket(
    _socket_path: &str,
    _sandbox_id: &str,
    _container_id: &str,
    _tool_name: &str,
    _input: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    Err(anyhow::anyhow!(
        "orchestration requires Unix (agentd uses Unix domain sockets)"
    ))
}

#[cfg(unix)]
pub(crate) fn invoke_tool_via_socket(
    socket_path: &str,
    sandbox_id: &str,
    container_id: &str,
    tool_name: &str,
    input: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    use serde_json::json;
    // Allow only tools that are registered in the agentd tool registry.
    // This keeps the allow-list in sync with the real set of executable tools.
    let allowed = crate::tool_registry::list_all_tools();
    if !allowed.contains(&tool_name) {
        return Ok(json!({
            "error": format!("unknown tool '{}'", tool_name),
            "success": false
        }));
    }
    let req = json!({
        "request_type": "invoke_tool",
        "sandbox": sandbox_id,
        "container": container_id,
        "name": tool_name,
        "input": input
    });
    trace(&format!(
        "invoke_tool request: tool={} sandbox={} container={}",
        tool_name, sandbox_id, container_id
    ));
    let resp = socket_roundtrip(socket_path, &req)?;
    if resp.get("status").and_then(|s| s.as_str()) == Some("ok") {
        Ok(resp.get("result").cloned().unwrap_or(json!({})))
    } else {
        let err = resp
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("tool error");
        Ok(json!({ "error": err, "success": false }))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Connection-pool tests
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(all(unix, test))]
mod pool_tests {
    use super::pool;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    // ── helpers ───────────────────────────────────────────────────────────────

    /// One-shot echo server: accepts a connection, reads one '\n'-terminated
    /// line, echoes it back, then closes the connection (mimics agentd).
    fn one_shot_echo_server(path: &str) {
        let listener = UnixListener::bind(path).expect("bind");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let mut line = String::new();
                {
                    let mut reader = BufReader::new(&stream);
                    if reader.read_line(&mut line).is_err() {
                        continue;
                    }
                }
                let _ = stream.write_all(line.as_bytes());
                // Close after each request — one-shot protocol.
            }
        });
    }

    /// Persistent echo server: accepts connections and keeps them open,
    /// replying to every '\n'-terminated line sent.  Used to verify that
    /// healthy connections are returned to the idle queue.
    fn persistent_echo_server(path: &str) {
        let listener = UnixListener::bind(path).expect("bind");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let stream = match stream {
                    Ok(s) => s,
                    Err(_) => break,
                };
                std::thread::spawn(move || {
                    let read_stream = stream.try_clone().expect("clone");
                    let mut write_stream = stream;
                    let reader = BufReader::new(read_stream);
                    for line in reader.lines() {
                        let line = match line {
                            Ok(l) => l,
                            Err(_) => break,
                        };
                        let reply = format!("{}\n", line);
                        if write_stream.write_all(reply.as_bytes()).is_err() {
                            break;
                        }
                    }
                });
            }
        });
    }

    /// Unique socket path per test to avoid cross-test interference.
    fn sock_path(tag: &str) -> String {
        format!("/tmp/pool_test_{}.sock", tag)
    }

    fn cleanup(path: &str) {
        let _ = std::fs::remove_file(path);
        pool::remove_pool(path);
    }

    // ── tests ─────────────────────────────────────────────────────────────────

    /// 100 threads hammer a pool of size 32 against a one-shot server.
    /// All 100 requests must succeed.
    #[test]
    fn test_pool_concurrent_100() {
        let path = sock_path("concurrent100");
        cleanup(&path);
        one_shot_echo_server(&path);
        std::thread::sleep(Duration::from_millis(50)); // let server start

        // Create pool directly to avoid racing on the global POOL_SIZE setting.
        let p = pool::SocketPool::new(path.clone(), 32);
        let barrier = Arc::new(Barrier::new(100));
        let errors: Arc<std::sync::Mutex<Vec<String>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));

        let handles: Vec<_> = (0..100)
            .map(|i| {
                let p = p.clone();
                let barrier = barrier.clone();
                let errors = errors.clone();
                std::thread::spawn(move || {
                    barrier.wait(); // all threads start together
                    let msg = format!("hello {}\n", i);
                    let result: anyhow::Result<String> = (|| {
                        let mut conn = p.acquire()?;
                        {
                            let s = conn.stream_mut();
                            s.set_read_timeout(Some(Duration::from_secs(10))).ok();
                            s.set_write_timeout(Some(Duration::from_secs(10))).ok();
                            s.write_all(msg.as_bytes())?;
                        } // borrow on conn ends here
                        let mut reader = BufReader::new(conn.stream_mut());
                        let mut line = String::new();
                        reader.read_line(&mut line)?;
                        Ok(line)
                    })();
                    if let Err(e) = result {
                        errors.lock().unwrap().push(format!("thread {}: {}", i, e));
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread panicked");
        }

        let errs = errors.lock().unwrap();
        assert!(errs.is_empty(), "errors from concurrent threads: {:?}", *errs);
        cleanup(&path);
    }

    /// With pool_size=1 and one thread holding the connection, a second
    /// acquire must block until the first is released.
    #[test]
    fn test_pool_exhaustion_waits() {
        let path = sock_path("exhaustion");
        cleanup(&path);
        one_shot_echo_server(&path);
        std::thread::sleep(Duration::from_millis(50));

        let p = pool::SocketPool::new(path.clone(), 1);

        // Hold the sole slot.
        let conn1 = p.acquire_timeout(Duration::from_secs(5)).expect("first acquire");

        // Second acquire with short timeout must time out.
        let result = p.acquire_timeout(Duration::from_millis(200));
        assert!(result.is_err(), "expected timeout error, got Ok");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("exhausted") || msg.contains("timed out"),
            "unexpected error: {}",
            msg
        );

        // Release first connection; second acquire should now succeed.
        drop(conn1);
        let conn2 = p.acquire_timeout(Duration::from_secs(5));
        assert!(conn2.is_ok(), "expected success after release: {:?}", conn2);

        cleanup(&path);
    }

    /// With a *persistent* server (keeps connection alive), the stream should
    /// be returned to the idle queue after use, so stats show idle=1.
    #[test]
    fn test_pool_connection_reuse() {
        let path = sock_path("reuse");
        cleanup(&path);
        persistent_echo_server(&path);
        std::thread::sleep(Duration::from_millis(50));

        let p = pool::SocketPool::new(path.clone(), 4);

        // First round-trip.
        {
            let mut conn = p.acquire_timeout(Duration::from_secs(5)).expect("acquire 1");
            {
                let s = conn.stream_mut();
                s.set_read_timeout(Some(Duration::from_secs(5))).ok();
                s.set_write_timeout(Some(Duration::from_secs(5))).ok();
                s.write_all(b"ping\n").expect("write");
            }
            let mut reader = BufReader::new(conn.stream_mut());
            let mut line = String::new();
            reader.read_line(&mut line).expect("read");
            assert_eq!(line.trim(), "ping");
            // conn drops here — persistent server keeps it alive → put_back()
        }

        // The connection must be back in the idle queue.
        let stats = p.stats();
        assert_eq!(stats.idle, 1, "expected 1 idle connection, got {:?}", stats);
        assert_eq!(stats.in_use, 0);

        // Second round-trip reuses the same slot (total stays at 1).
        {
            let mut conn = p.acquire_timeout(Duration::from_secs(5)).expect("acquire 2");
            let stats_during = p.stats();
            assert_eq!(stats_during.idle, 0, "idle should be 0 while in use");
            assert_eq!(stats_during.in_use, 1);

            {
                let s = conn.stream_mut();
                s.write_all(b"world\n").expect("write");
            }
            let mut reader = BufReader::new(conn.stream_mut());
            let mut line = String::new();
            reader.read_line(&mut line).expect("read");
            assert_eq!(line.trim(), "world");
        }

        let stats_after = p.stats();
        assert_eq!(stats_after.idle, 1, "expected 1 idle after second use");
        assert_eq!(stats_after.in_use, 0);

        cleanup(&path);
    }
}
