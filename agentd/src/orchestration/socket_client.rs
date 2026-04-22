//! Bounded socket client — limits concurrent Unix socket connections to prevent
//! EMFILE (too many open files) under high agent concurrency.
//!
//! Architecture: bounded thread pool of worker threads. Each worker owns one
//! socket connection at a time. All callers submit requests to a shared queue
//! and block until a worker processes their request.
//!
//! The server closes connections after each request (see socket_server.rs
//! process_job), so connection reuse is not possible. The pool controls
//! concurrency, not connection lifetime.

use anyhow::{anyhow, Context, Result};
use once_cell::sync::OnceCell;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

/// Number of concurrent socket connections allowed.
/// Must not exceed server's SLOW_WORKERS (128) to avoid server-side queueing.
const POOL_WORKERS: usize = 96;

/// Bounded queue depth. Requests beyond this block the caller until a worker
/// is free. Prevents unbounded memory growth under extreme load.
const QUEUE_DEPTH: usize = 512;

/// Per-connection read timeout. Must be longer than the slowest server operation.
const READ_TIMEOUT_SECS: u64 = 60;

/// Per-connection write timeout.
const WRITE_TIMEOUT_SECS: u64 = 10;

/// Max retries on retryable errors (WouldBlock, ConnectionReset, BrokenPipe).
const MAX_RETRIES: usize = 3;

/// A single request submitted to the pool.
struct PoolRequest {
    socket_path: String,
    payload: String, // pre-serialized JSON line (with trailing \n)
    /// Channel to send the result back to the caller.
    reply: mpsc::SyncSender<Result<serde_json::Value>>,
}

/// The global socket client pool. Initialized once on first use.
static POOL: OnceCell<SocketClientPool> = OnceCell::new();

struct SocketClientPool {
    sender: mpsc::SyncSender<PoolRequest>,
}

impl SocketClientPool {
    fn new() -> Self {
        // Bounded channel — callers block if queue is full (backpressure)
        let (tx, rx) = mpsc::sync_channel::<PoolRequest>(QUEUE_DEPTH);
        let rx = Arc::new(Mutex::new(rx));

        for worker_id in 0..POOL_WORKERS {
            let rx = Arc::clone(&rx);
            thread::Builder::new()
                .name(format!("socket-client-{}", worker_id))
                .spawn(move || {
                    loop {
                        // Block until a request arrives
                        let req = {
                            let locked = match rx.lock() {
                                Ok(g) => g,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            match locked.recv() {
                                Ok(r) => r,
                                Err(_) => break, // sender dropped, pool is shutting down
                            }
                        };
                        // Execute the request and send result back to caller
                        let result = execute_request(&req.socket_path, &req.payload);
                        // Ignore send error — caller may have timed out
                        let _ = req.reply.send(result);
                    }
                })
                .expect("failed to spawn socket client worker");
        }

        SocketClientPool { sender: tx }
    }

    fn submit(&self, socket_path: &str, payload: String) -> Result<serde_json::Value> {
        // Synchronous reply channel (capacity 1 — exactly one response per request)
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);

        let req = PoolRequest {
            socket_path: socket_path.to_string(),
            payload,
            reply: reply_tx,
        };

        // This blocks if the queue is full (backpressure)
        self.sender
            .send(req)
            .map_err(|_| anyhow!("socket client pool is shut down"))?;

        // Block until worker sends the result
        reply_rx
            .recv()
            .map_err(|_| anyhow!("socket client worker dropped reply"))?
    }
}

/// Execute one request: connect → write → read → close.
/// Retries on transient errors (WouldBlock, ConnectionReset, BrokenPipe).
/// Never reuses connections — server closes after each response.
fn execute_request(socket_path: &str, payload: &str) -> Result<serde_json::Value> {
    let mut last_err: Option<anyhow::Error> = None;

    for attempt in 0..=MAX_RETRIES {
        // Always open a fresh connection — server closes after each request
        let mut stream = match UnixStream::connect(socket_path) {
            Ok(s) => s,
            Err(e) => {
                last_err = Some(anyhow!("connect: {}", e));
                if attempt < MAX_RETRIES {
                    thread::sleep(std::time::Duration::from_millis(
                        50 * (attempt as u64 + 1),
                    ));
                    continue;
                }
                break;
            }
        };

        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(READ_TIMEOUT_SECS)))
            .context("set read timeout")?;
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(WRITE_TIMEOUT_SECS)))
            .context("set write timeout")?;

        // Write phase
        if let Err(e) = write_all(&mut stream, payload.as_bytes()) {
            let retryable = is_retryable_write(&e);
            last_err = Some(anyhow!("write: {}", e));
            if retryable && attempt < MAX_RETRIES {
                thread::sleep(std::time::Duration::from_millis(
                    100 * (attempt as u64 + 1),
                ));
                continue;
            }
            break;
        }

        if let Err(e) = stream.flush() {
            last_err = Some(anyhow!("flush: {}", e));
            break;
        }

        // Read phase
        let mut reader = BufReader::new(&mut stream);
        let mut response_line = String::new();

        match reader.read_line(&mut response_line) {
            Ok(0) => {
                // Server closed connection before responding — retry
                last_err = Some(anyhow!("read: server closed connection"));
                if attempt < MAX_RETRIES {
                    thread::sleep(std::time::Duration::from_millis(
                        100 * (attempt as u64 + 1),
                    ));
                    continue;
                }
                break;
            }
            Ok(_) => {}
            Err(e) => {
                let retryable = matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::Interrupted
                );
                last_err = Some(anyhow!("read: {}", e));
                if retryable && attempt < MAX_RETRIES {
                    thread::sleep(std::time::Duration::from_millis(
                        200 * (attempt as u64 + 1),
                    ));
                    continue;
                }
                break;
            }
        }

        // Explicit shutdown (best-effort, server may have already closed)
        let _ = stream.shutdown(std::net::Shutdown::Both);

        let trimmed = response_line.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("empty response from server"));
        }

        return serde_json::from_str(trimmed).context("parse server response JSON");
    }

    Err(last_err.unwrap_or_else(|| {
        anyhow!("socket request failed after {} retries", MAX_RETRIES)
    }))
}

fn write_all(stream: &mut UnixStream, mut buf: &[u8]) -> std::io::Result<()> {
    while !buf.is_empty() {
        match stream.write(buf) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "socket closed by server",
                ))
            }
            Ok(n) => buf = &buf[n..],
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn is_retryable_write(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::Interrupted
    )
}

/// Public API: submit a socket request through the bounded pool.
///
/// This is the replacement for `socket_roundtrip`. Drop-in compatible:
/// same signature, same return type.
pub fn socket_request(socket_path: &str, req: &serde_json::Value) -> Result<serde_json::Value> {
    let pool = POOL.get_or_init(SocketClientPool::new);

    let req_type = req
        .get("request_type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let mut payload = serde_json::to_string(req).context("serialize request")?;
    payload.push('\n');

    pool.submit(socket_path, payload)
        .with_context(|| format!("socket_request failed for request_type={}", req_type))
}
