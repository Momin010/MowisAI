//! Integration test: simulates the desktop app connecting to agentd.
//!
//! This test exercises the exact protocol path the desktop uses:
//!   1. Connect to the agentd Unix socket
//!   2. Send `set_config` → read JSON response → connection closes
//!   3. Open NEW connection, send `orchestrate` → read streaming events
//!
//! The test verifies that:
//!   - set_config succeeds (returns {"status":"ok",...})
//!   - orchestrate does NOT immediately close/reset the connection
//!   - orchestrate returns at least one JSON event before closing
//!
//! Run with: cargo test --package agentd --test desktop_session_test

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

/// Helper: connect to socket, send one JSON line, read response lines.
fn send_request(socket_path: &str, request: &serde_json::Value) -> Vec<String> {
    let mut stream = UnixStream::connect(socket_path)
        .unwrap_or_else(|e| panic!("Failed to connect to {}: {}", socket_path, e));

    stream.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(10))).unwrap();

    let payload = serde_json::to_string(request).unwrap();
    stream.write_all(payload.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    stream.flush().unwrap();

    // Read all response lines until EOF or timeout
    let reader = BufReader::new(&stream);
    let mut lines = Vec::new();
    for line in reader.lines() {
        match line {
            Ok(l) if l.trim().is_empty() => continue,
            Ok(l) => lines.push(l),
            Err(e) => {
                // Timeout or connection closed — that's fine, we have what we have
                if lines.is_empty() {
                    panic!("Connection error before receiving any data: {}", e);
                }
                break;
            }
        }
    }
    lines
}

/// Test: set_config returns a valid JSON response with status "ok"
#[test]
fn test_set_config_protocol() {
    let socket_path = std::env::var("AGENTD_TEST_SOCKET")
        .unwrap_or_else(|_| "/tmp/agentd.sock".into());

    if !std::path::Path::new(&socket_path).exists() {
        eprintln!(
            "SKIP: agentd socket not found at {}. \
             Start agentd first: agentd socket --path {}",
            socket_path, socket_path
        );
        return;
    }

    let request = serde_json::json!({
        "request_type": "set_config",
        "provider": "gemini",
        "model": "gemini-2.5-pro",
        "api_key": "test-key-for-protocol-verification",
    });

    let lines = send_request(&socket_path, &request);
    assert!(!lines.is_empty(), "set_config should return at least one line");

    let resp: serde_json::Value = serde_json::from_str(&lines[0])
        .unwrap_or_else(|e| panic!("Invalid JSON response: {} — raw: {}", e, lines[0]));

    assert_eq!(
        resp["status"].as_str(),
        Some("ok"),
        "set_config should succeed. Got: {}",
        resp
    );
    eprintln!("✓ set_config response: {}", resp);
}

/// Test: orchestrate returns JSON events (not a connection reset)
#[test]
fn test_orchestrate_protocol() {
    let socket_path = std::env::var("AGENTD_TEST_SOCKET")
        .unwrap_or_else(|_| "/tmp/agentd.sock".into());

    if !std::path::Path::new(&socket_path).exists() {
        eprintln!(
            "SKIP: agentd socket not found at {}. \
             Start agentd first: agentd socket --path {}",
            socket_path, socket_path
        );
        return;
    }

    // First ensure config exists (orchestrate requires it)
    let config_req = serde_json::json!({
        "request_type": "set_config",
        "provider": "gemini",
        "model": "gemini-2.5-pro",
        "api_key": "test-key-for-protocol-verification",
    });
    let config_lines = send_request(&socket_path, &config_req);
    assert!(!config_lines.is_empty(), "set_config prerequisite failed");

    // Now send orchestrate on a NEW connection (matching desktop's fresh_stream behavior)
    let orchestrate_req = serde_json::json!({
        "type": "orchestrate",
        "prompt": "Add a hello world endpoint to the API",
        "project": "/tmp",
        "max_agents": 1,
        "mode": "simple",
    });

    let lines = send_request(&socket_path, &orchestrate_req);

    // We should get at least ONE JSON line back — either an event or an error.
    // Getting zero lines means the connection was reset (the bug we're fixing).
    assert!(
        !lines.is_empty(),
        "orchestrate should return at least one JSON event. \
         Got 0 lines — connection was likely reset by agentd. \
         Check /tmp/agentd.log for errors."
    );

    // Parse each line and verify they're valid JSON with a "type" field
    for (i, line) in lines.iter().enumerate() {
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("Line {} is not valid JSON: {} — raw: {}", i, e, line));

        let event_type = v.get("type").or(v.get("status"));
        assert!(
            event_type.is_some(),
            "Line {} should have a 'type' or 'status' field: {}",
            i, v
        );
        eprintln!("  event[{}]: type={}", i, event_type.unwrap());
    }

    // Check that we got either a proper error message or orchestration events
    let first: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
    let first_type = first.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match first_type {
        "error" => {
            // This is acceptable — it means agentd received the request and
            // responded with a structured error (e.g., "LLM config error").
            // The desktop can display this to the user.
            eprintln!(
                "✓ orchestrate returned structured error: {}",
                first["message"].as_str().unwrap_or("(no message)")
            );
        }
        "agent_message" | "task_added" | "stats" | "complete" => {
            eprintln!("✓ orchestrate returned {} event(s)", lines.len());
        }
        other => {
            eprintln!("✓ orchestrate returned event type '{}' ({} total lines)", other, lines.len());
        }
    }
}

/// Test: full desktop session flow (set_config → orchestrate → read events)
/// This is the exact sequence the desktop app performs.
#[test]
fn test_full_desktop_session_flow() {
    let socket_path = std::env::var("AGENTD_TEST_SOCKET")
        .unwrap_or_else(|_| "/tmp/agentd.sock".into());

    if !std::path::Path::new(&socket_path).exists() {
        eprintln!(
            "SKIP: agentd socket not found at {}.",
            socket_path
        );
        return;
    }

    eprintln!("=== Desktop Session Flow Test ===");
    eprintln!("Socket: {}", socket_path);

    // Step 1: set_config (fresh connection)
    eprintln!("\n--- Step 1: set_config ---");
    let config_req = serde_json::json!({
        "request_type": "set_config",
        "provider": "gemini",
        "model": "gemini-2.5-pro",
        "api_key": "test-key-12345",
    });
    let resp_lines = send_request(&socket_path, &config_req);
    assert!(!resp_lines.is_empty(), "set_config: no response (connection reset?)");
    let resp: serde_json::Value = serde_json::from_str(&resp_lines[0]).unwrap();
    assert_eq!(resp["status"].as_str(), Some("ok"), "set_config failed: {}", resp);
    eprintln!("  ✓ Config saved: {}", resp);

    // Step 2: orchestrate (fresh connection — different from step 1)
    eprintln!("\n--- Step 2: orchestrate ---");
    let orch_req = serde_json::json!({
        "type": "orchestrate",
        "prompt": "Create a REST API with health check endpoint",
        "project": "/tmp",
        "max_agents": 1,
        "mode": "simple",
    });
    let event_lines = send_request(&socket_path, &orch_req);
    assert!(
        !event_lines.is_empty(),
        "orchestrate: no response (connection reset!). \
         This is the exact bug that causes 'Connection lost: OS error 10054' on the desktop."
    );

    eprintln!("  Received {} event(s):", event_lines.len());
    for (i, line) in event_lines.iter().enumerate() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap_or_default();
        let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("?");
        let preview = if line.len() > 100 { &line[..100] } else { line };
        eprintln!("  [{}] type={:<15} {}", i, t, preview);
    }

    eprintln!("\n=== PASS: Desktop session flow completed without connection reset ===");
}
