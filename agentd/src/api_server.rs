use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use serde_json::json;

/// Shared state for the API server — tracks all orchestration tasks.
pub struct ApiState {
    pub tasks: RwLock<HashMap<String, TaskInfo>>,
}

#[derive(Clone, Debug)]
pub struct TaskInfo {
    pub id: String,
    pub prompt: String,
    pub status: String,
    pub log: Vec<String>,
    pub diff: Option<String>,
    pub summary: Option<String>,
    pub interactive_prompt: Option<String>,
    pub interactive_response: Option<String>,
    pub created_at: u64,
}

impl TaskInfo {
    fn new(id: String, prompt: String) -> Self {
        Self {
            id,
            prompt,
            status: "starting".into(),
            log: Vec::new(),
            diff: None,
            summary: None,
            interactive_prompt: None,
            interactive_response: None,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

/// Start the HTTP API server on the given port.
/// Returns a handle to the shared state that can be updated by the orchestrator.
pub fn start_api_server(port: u16, socket_path: String) -> Arc<ApiState> {
    let state = Arc::new(ApiState {
        tasks: RwLock::new(HashMap::new()),
    });
    let state_clone = state.clone();

    thread::spawn(move || {
        let listener = match TcpListener::bind(format!("0.0.0.0:{}", port)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("API server failed to bind port {}: {}", port, e);
                return;
            }
        };
        eprintln!("API server listening on http://0.0.0.0:{}", port);

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    let state = state_clone.clone();
                    let socket_path = socket_path.clone();
                    thread::spawn(move || {
                        handle_request(&mut stream, &state, &socket_path);
                    });
                }
                Err(e) => {
                    eprintln!("API accept error: {}", e);
                }
            }
        }
    });

    state
}

fn handle_request(stream: &mut std::net::TcpStream, state: &Arc<ApiState>, socket_path: &str) {
    // Read the entire raw request as bytes — avoids BufReader consuming the body
    stream.set_read_timeout(Some(std::time::Duration::from_secs(30))).ok();
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                // Check if we've received the full request (headers + body)
                if let Some(header_end) = find_header_end(&raw) {
                    let headers_str = String::from_utf8_lossy(&raw[..header_end]);
                    let content_length = parse_content_length(&headers_str);
                    let body_start = header_end + 4; // Skip \r\n\r\n
                    let body_received = raw.len() - body_start;
                    if body_received >= content_length {
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }

    let raw_str = String::from_utf8_lossy(&raw);

    // Parse request line
    let first_line = raw_str.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        send_response(stream, 400, "Bad Request");
        return;
    }
    let method = parts[0];
    let path = parts[1].to_string();

    // Parse body
    let body = if let Some(header_end) = find_header_end(&raw) {
        let body_start = header_end + 4;
        if body_start < raw.len() {
            String::from_utf8_lossy(&raw[body_start..]).to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Route
    match (method, path.as_str()) {
        ("GET", "/api/health") => {
            send_json(stream, &json!({"status": "ok", "service": "mowisai-api"}));
        }

        ("GET", "/api/tasks") => {
            let tasks = state.tasks.read().unwrap();
            let task_list: Vec<serde_json::Value> = tasks.values().map(|t| {
                json!({
                    "id": t.id,
                    "prompt": t.prompt,
                    "status": t.status,
                    "log_lines": t.log.len(),
                    "has_diff": t.diff.is_some(),
                    "interactive_prompt": t.interactive_prompt,
                    "created_at": t.created_at,
                })
            }).collect();
            send_json(stream, &json!({"tasks": task_list}));
        }

        ("POST", "/api/orchestrate") => {
            let parsed: serde_json::Value = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(e) => {
                    send_json(stream, &json!({"error": format!("invalid JSON: {}", e)}));
                    return;
                }
            };

            let prompt = match parsed.get("prompt").and_then(|p| p.as_str()) {
                Some(p) => p.to_string(),
                None => {
                    send_json(stream, &json!({"error": "missing 'prompt' field"}));
                    return;
                }
            };

            let mode = parsed.get("mode").and_then(|m| m.as_str()).unwrap_or("fast").to_string();
            let output_dir = parsed.get("output_dir").and_then(|o| o.as_str()).map(|s| s.to_string());

            // Generate task ID
            let task_id = format!("{:016x}", fastrand::u64(..));
            let task = TaskInfo::new(task_id.clone(), prompt.clone());
            state.tasks.write().unwrap().insert(task_id.clone(), task);

            // Spawn orchestration in background
            let state_bg = state.clone();
            let task_id_bg = task_id.clone();
            let socket_path_bg = socket_path.to_string();
            let prompt_bg = prompt.clone();
            thread::spawn(move || {
                run_orchestration(state_bg, task_id_bg, prompt_bg, mode, socket_path_bg, output_dir);
            });

            send_json(stream, &json!({
                "task_id": task_id,
                "status": "started",
                "message": format!("Orchestration started for: {}", prompt),
            }));
        }

        ("GET", path) if path.starts_with("/api/status/") => {
            let task_id = path.trim_start_matches("/api/status/");
            let tasks = state.tasks.read().unwrap();
            match tasks.get(task_id) {
                Some(t) => {
                    send_json(stream, &json!({
                        "task_id": t.id,
                        "prompt": t.prompt,
                        "status": t.status,
                        "log": t.log,
                        "has_diff": t.diff.is_some(),
                        "summary": t.summary,
                        "interactive_prompt": t.interactive_prompt,
                    }));
                }
                None => send_json(stream, &json!({"error": "task not found"})),
            }
        }

        ("GET", path) if path.starts_with("/api/diff/") => {
            let task_id = path.trim_start_matches("/api/diff/");
            let tasks = state.tasks.read().unwrap();
            match tasks.get(task_id) {
                Some(t) if t.diff.is_some() => {
                    send_json(stream, &json!({
                        "task_id": t.id,
                        "diff": t.diff.as_ref().unwrap(),
                    }));
                }
                Some(_) => send_json(stream, &json!({"error": "no diff available yet"})),
                None => send_json(stream, &json!({"error": "task not found"})),
            }
        }

        ("GET", path) if path.starts_with("/api/stream/") => {
            let task_id = path.trim_start_matches("/api/stream/");
            // SSE stream — keep connection open, send events
            handle_sse_stream(stream, state, task_id);
        }

        ("POST", path) if path.starts_with("/api/input/") => {
            let task_id = path.trim_start_matches("/api/input/");
            let parsed: serde_json::Value = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(e) => {
                    send_json(stream, &json!({"error": format!("invalid JSON: {}", e)}));
                    return;
                }
            };

            let response = match parsed.get("response").and_then(|r| r.as_str()) {
                Some(r) => r.to_string(),
                None => {
                    send_json(stream, &json!({"error": "missing 'response' field"}));
                    return;
                }
            };

            // Send input to the running command via socket
            let socket_result = send_input_via_socket(socket_path, &response);

            let mut tasks = state.tasks.write().unwrap();
            match tasks.get_mut(task_id) {
                Some(t) => {
                    t.interactive_response = Some(response.clone());
                    t.interactive_prompt = None;
                    send_json(stream, &json!({
                        "task_id": task_id,
                        "response_sent": response,
                        "socket_result": socket_result,
                    }));
                }
                None => {
                    // Even if task not found, try sending via socket directly
                    send_json(stream, &json!({
                        "response_sent": response,
                        "socket_result": socket_result,
                        "note": "task not tracked, but input sent to socket",
                    }));
                }
            }
        }

        ("GET", "/api/interactive") => {
            // Check if a command is waiting for interactive input
            let socket_result = check_interactive_status(socket_path);
            send_json(stream, &socket_result);
        }

        _ => {
            send_json(stream, &json!({
                "error": "not found",
                "endpoints": [
                    "GET  /api/health",
                    "GET  /api/tasks",
                    "POST /api/orchestrate {prompt, mode?, output_dir?}",
                    "GET  /api/status/:task_id",
                    "GET  /api/diff/:task_id",
                    "GET  /api/stream/:task_id (SSE)",
                    "POST /api/input/:task_id {response}",
                    "GET  /api/interactive",
                ],
            }));
        }
    }
}

fn handle_sse_stream(stream: &mut std::net::TcpStream, state: &Arc<ApiState>, task_id: &str) {
    // Send SSE headers
    let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nAccess-Control-Allow-Origin: *\r\n\r\n";
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.flush();

    let mut last_log_line = 0;
    let mut last_status = String::new();

    loop {
        {
            let tasks = state.tasks.read().unwrap();
            if let Some(task) = tasks.get(task_id) {
                // Send new log lines
                while last_log_line < task.log.len() {
                    let event = json!({
                        "type": "log",
                        "line": task.log[last_log_line],
                    });
                    let msg = format!("data: {}\n\n", event);
                    if stream.write_all(msg.as_bytes()).is_err() { return; }
                    if stream.flush().is_err() { return; }
                    last_log_line += 1;
                }

                // Send status changes
                if task.status != last_status {
                    let event = json!({
                        "type": "status",
                        "status": task.status,
                    });
                    let msg = format!("data: {}\n\n", event);
                    if stream.write_all(msg.as_bytes()).is_err() { return; }
                    if stream.flush().is_err() { return; }
                    last_status = task.status.clone();
                }

                // Send interactive prompt if present
                if let Some(ref prompt) = task.interactive_prompt {
                    let event = json!({
                        "type": "interactive",
                        "prompt": prompt,
                    });
                    let msg = format!("data: {}\n\n", event);
                    if stream.write_all(msg.as_bytes()).is_err() { return; }
                    if stream.flush().is_err() { return; }
                }

                // Send diff when available
                if task.status == "complete" || task.status == "failed" {
                    if let Some(ref diff) = task.diff {
                        let event = json!({
                            "type": "diff",
                            "diff": diff,
                            "summary": task.summary,
                        });
                        let msg = format!("data: {}\n\n", event);
                        let _ = stream.write_all(msg.as_bytes());
                        let _ = stream.flush();
                    }
                    // Terminal state — close stream
                    let event = json!({"type": "done"});
                    let msg = format!("data: {}\n\n", event);
                    let _ = stream.write_all(msg.as_bytes());
                    let _ = stream.flush();
                    return;
                }
            } else {
                let event = json!({"type": "error", "message": "task not found"});
                let msg = format!("data: {}\n\n", event);
                let _ = stream.write_all(msg.as_bytes());
                return;
            }
        }

        thread::sleep(std::time::Duration::from_millis(500));
    }
}

fn run_orchestration(
    state: Arc<ApiState>,
    task_id: String,
    prompt: String,
    mode: String,
    socket_path: String,
    output_dir: Option<String>,
) {
    // Update status
    {
        let mut tasks = state.tasks.write().unwrap();
        if let Some(t) = tasks.get_mut(&task_id) {
            t.status = "running".into();
            t.log.push("Starting orchestration...".into());
        }
    }

    // Build orchestrate command
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap_or_else(|_| "agentd".into()));
    cmd.arg("orchestrate")
        .arg("--prompt").arg(&prompt)
        .arg("--project").arg("company-internal-tools-490516")
        .arg("--socket").arg(&socket_path)
        .arg("--mode").arg(&mode);

    if let Some(ref dir) = output_dir {
        cmd.arg("--output").arg(dir);
    } else {
        let output_path = format!("/tmp/mowis-api-{}.patch", task_id);
        cmd.arg("--output").arg(&output_path);
    }

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    match cmd.spawn() {
        Ok(mut child) => {
            // Stream stdout
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            let state_out = state.clone();
            let task_id_out = task_id.clone();
            let state_err = state.clone();
            let task_id_err = task_id.clone();

            let stdout_thread = stdout.map(|out| {
                thread::spawn(move || {
                    let reader = BufReader::new(out);
                    for line in reader.lines() {
                        if let Ok(l) = line {
                            let mut tasks = state_out.tasks.write().unwrap();
                            if let Some(t) = tasks.get_mut(&task_id_out) {
                                t.log.push(l);
                            }
                        }
                    }
                })
            });

            let stderr_thread = stderr.map(|err| {
                thread::spawn(move || {
                    let reader = BufReader::new(err);
                    for line in reader.lines() {
                        if let Ok(l) = line {
                            let mut tasks = state_err.tasks.write().unwrap();
                            if let Some(t) = tasks.get_mut(&task_id_err) {
                                t.log.push(l);
                            }
                        }
                    }
                })
            });

            let status = child.wait();
            if let Some(t) = stdout_thread { let _ = t.join(); }
            if let Some(t) = stderr_thread { let _ = t.join(); }

            let mut tasks = state.tasks.write().unwrap();
            if let Some(t) = tasks.get_mut(&task_id) {
                match status {
                    Ok(s) if s.success() => {
                        t.status = "complete".into();
                        t.log.push("Orchestration complete.".into());
                        // Try to read the diff
                        let diff_path = format!("/tmp/mowis-api-{}.patch", task_id);
                        if let Ok(diff) = std::fs::read_to_string(&diff_path) {
                            t.diff = Some(diff);
                        }
                    }
                    Ok(s) => {
                        t.status = "failed".into();
                        t.log.push(format!("Orchestration failed with exit code: {:?}", s.code()));
                    }
                    Err(e) => {
                        t.status = "failed".into();
                        t.log.push(format!("Orchestration process error: {}", e));
                    }
                }
            }
        }
        Err(e) => {
            let mut tasks = state.tasks.write().unwrap();
            if let Some(t) = tasks.get_mut(&task_id) {
                t.status = "failed".into();
                t.log.push(format!("Failed to start orchestration: {}", e));
            }
        }
    }
}

fn send_response(stream: &mut std::net::TcpStream, status: u16, body: &str) {
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
        status, status_text, body.len(), body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn send_json(stream: &mut std::net::TcpStream, value: &serde_json::Value) {
    let body = serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string());
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
        body.len(), body
    );
    let _ = stream.write_all(response.as_bytes());
}

/// Send input to the running command via the agentd socket.
fn send_input_via_socket(socket_path: &str, data: &str) -> serde_json::Value {
    let request = json!({
        "request_type": "send_input",
        "input": { "data": data }
    });
    match socket_request(socket_path, &request) {
        Ok(resp) => resp,
        Err(e) => json!({"error": e.to_string()}),
    }
}

/// Check if a command is waiting for interactive input.
fn check_interactive_status(socket_path: &str) -> serde_json::Value {
    let request = json!({
        "request_type": "interactive_status"
    });
    match socket_request(socket_path, &request) {
        Ok(resp) => resp,
        Err(e) => json!({"error": e.to_string()}),
    }
}

/// Simple socket request — connect, send, read response, close.
fn socket_request(socket_path: &str, request: &serde_json::Value) -> Result<serde_json::Value, String> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path)
        .map_err(|e| format!("connect: {}", e))?;

    stream.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .map_err(|e| format!("set timeout: {}", e))?;

    let payload = format!("{}\n", serde_json::to_string(request).unwrap());
    stream.write_all(payload.as_bytes())
        .map_err(|e| format!("write: {}", e))?;

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response)
        .map_err(|e| format!("read: {}", e))?;

    serde_json::from_str(&response)
        .map_err(|e| format!("parse: {}", e))
}

/// Find the \r\n\r\n that separates headers from body.
fn find_header_end(data: &[u8]) -> Option<usize> {
    for i in 0..data.len().saturating_sub(3) {
        if &data[i..i + 4] == b"\r\n\r\n" {
            return Some(i);
        }
    }
    None
}

/// Parse Content-Length from raw header string.
fn parse_content_length(headers: &str) -> usize {
    for line in headers.lines() {
        if line.to_lowercase().starts_with("content-length:") {
            if let Some(val) = line.split(':').nth(1) {
                return val.trim().parse().unwrap_or(0);
            }
        }
    }
    0
}
