//! Integration test: simulates the REAL desktop→agentd connection path.
//!
//! This test connects over TCP (127.0.0.1:9722) — the exact same path the
//! Windows/macOS desktop app uses through socat. It does NOT use Unix sockets
//! directly, because that's not the problem.
//!
//! The actual connection chain is:
//!   Desktop (Windows) → TCP 127.0.0.1:9722 → socat (in WSL2) → Unix /tmp/agentd.sock → agentd
//!
//! Prerequisites:
//!   1. agentd running in WSL:   agentd socket --path /tmp/agentd.sock
//!   2. socat bridge running:    socat TCP-LISTEN:9722,reuseaddr,fork UNIX-CONNECT:/tmp/agentd.sock
//!
//! Run from Windows:
//!   cargo test --package agentd --test desktop_session_test -- --nocapture
//!
//! Or set a custom address:
//!   AGENTD_TCP_ADDR=127.0.0.1:9722 cargo test --package agentd --test desktop_session_test -- --nocapture

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const DEFAULT_TCP_ADDR: &str = "127.0.0.1:9722";

/// Connect to agentd via TCP (through socat), send one JSON line, read all response lines.
/// This is exactly what the desktop's BackendBridge.send() + recv_next() loop does.
fn tcp_send_request(addr: &str, request: &serde_json::Value) -> Result<Vec<String>, String> {
    // Open fresh TCP connection (same as BackendBridge.fresh_stream())
    let stream = TcpStream::connect_timeout(
        &addr.parse().map_err(|e| format!("bad addr: {e}"))?,
        Duration::from_secs(10),
    ).map_err(|e| format!("TCP connect to {addr} failed: {e}"))?;

    stream.set_read_timeout(Some(Duration::from_secs(60)))
        .map_err(|e| format!("set_read_timeout: {e}"))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| format!("set_write_timeout: {e}"))?;

    let mut writer = stream.try_clone().map_err(|e| format!("clone: {e}"))?;

    // Send the JSON request (same as ConnectionStream::send_json)
    let payload = serde_json::to_string(request).unwrap();
    writer.write_all(payload.as_bytes())
        .map_err(|e| format!("TCP write failed: {e}"))?;
    writer.write_all(b"\n")
        .map_err(|e| format!("TCP write newline failed: {e}"))?;
    writer.flush()
        .map_err(|e| format!("TCP flush failed: {e}"))?;

    // Read response lines (same as ConnectionStream::recv_json loop)
    let reader = BufReader::new(&stream);
    let mut lines = Vec::new();
    for line_result in reader.lines() {
        match line_result {
            Ok(line) if line.trim().is_empty() => continue,
            Ok(line) => lines.push(line),
            Err(e) => {
                if lines.is_empty() {
                    return Err(format!(
                        "TCP read error BEFORE any data received: {e}. \
                         This is the 'Connection lost: OS error 10054' bug!"
                    ));
                }
                // Got some data then connection closed — that's normal for agentd
                break;
            }
        }
    }
    Ok(lines)
}

fn get_tcp_addr() -> String {
    std::env::var("AGENTD_TCP_ADDR").unwrap_or_else(|_| DEFAULT_TCP_ADDR.into())
}

fn tcp_is_reachable(addr: &str) -> bool {
    TcpStream::connect_timeout(
        &addr.parse().unwrap(),
        Duration::from_secs(3),
    ).is_ok()
}

// ════════════════════════════════════════════════════════════════════════════════

/// Test 1: TCP connection to socat/agentd works at all
#[test]
fn test_tcp_connectivity() {
    let addr = get_tcp_addr();
    if !tcp_is_reachable(&addr) {
        eprintln!(
            "SKIP: Cannot reach agentd at TCP {}.\n\
             Make sure:\n\
             1. agentd is running in WSL:  wsl -d MowisAI -- agentd socket --path /tmp/agentd.sock\n\
             2. socat bridge is running:   wsl -d MowisAI -- socat TCP-LISTEN:9722,reuseaddr,fork UNIX-CONNECT:/tmp/agentd.sock &",
            addr
        );
        return;
    }
    eprintln!("✓ TCP connection to {} succeeded", addr);
}

/// Test 2: set_config over TCP returns valid response (not a connection reset)
#[test]
fn test_tcp_set_config() {
    let addr = get_tcp_addr();
    if !tcp_is_reachable(&addr) {
        eprintln!("SKIP: agentd not reachable at {}", addr);
        return;
    }

    let request = serde_json::json!({
        "request_type": "set_config",
        "provider": "gemini",
        "model": "gemini-2.5-pro",
        "api_key": "test-key-tcp-protocol-check",
    });

    match tcp_send_request(&addr, &request) {
        Ok(lines) => {
            assert!(!lines.is_empty(), "set_config over TCP: got 0 lines (connection reset?)");
            let resp: serde_json::Value = serde_json::from_str(&lines[0])
                .unwrap_or_else(|e| panic!("Invalid JSON from agentd: {e}\nRaw: {}", lines[0]));
            assert_eq!(
                resp["status"].as_str(), Some("ok"),
                "set_config over TCP failed: {}", resp
            );
            eprintln!("✓ set_config over TCP succeeded: {}", resp);
        }
        Err(e) => {
            panic!("set_config over TCP FAILED: {}", e);
        }
    }
}

/// Test 3: orchestrate over TCP returns events (not a connection reset)
/// This is THE test that catches the OS error 10054 bug.
#[test]
fn test_tcp_orchestrate() {
    let addr = get_tcp_addr();
    if !tcp_is_reachable(&addr) {
        eprintln!("SKIP: agentd not reachable at {}", addr);
        return;
    }

    // Step 1: ensure config exists (fresh TCP connection)
    let config_req = serde_json::json!({
        "request_type": "set_config",
        "provider": "gemini",
        "model": "gemini-2.5-pro",
        "api_key": "test-key-orchestrate-check",
    });
    let config_result = tcp_send_request(&addr, &config_req);
    assert!(config_result.is_ok(), "Prerequisite set_config failed: {:?}", config_result.err());

    // Step 2: send orchestrate on a NEW TCP connection (fresh_stream behavior)
    let orchestrate_req = serde_json::json!({
        "type": "orchestrate",
        "prompt": "Add a hello world endpoint",
        "project": "/tmp",
        "max_agents": 1,
        "mode": "simple",
    });

    eprintln!("Sending orchestrate over TCP to {}...", addr);
    match tcp_send_request(&addr, &orchestrate_req) {
        Ok(lines) => {
            assert!(
                !lines.is_empty(),
                "orchestrate over TCP: got 0 lines!\n\
                 This means the connection was RESET by agentd/socat.\n\
                 This IS the 'Connection lost: OS error 10054' bug."
            );

            eprintln!("✓ orchestrate returned {} event(s) over TCP:", lines.len());
            for (i, line) in lines.iter().enumerate() {
                let v: serde_json::Value = serde_json::from_str(line).unwrap_or_default();
                let t = v.get("type").and_then(|x| x.as_str())
                    .or(v.get("status").and_then(|x| x.as_str()))
                    .unwrap_or("?");
                let preview = if line.len() > 120 { &line[..120] } else { line.as_str() };
                eprintln!("  [{}] type={:<15} {}", i, t, preview);
            }
        }
        Err(e) => {
            panic!(
                "orchestrate over TCP FAILED: {}\n\n\
                 This is the exact error the desktop sees as 'Connection lost: OS error 10054'.\n\
                 The socat→agentd connection is being reset.",
                e
            );
        }
    }
}

/// Test 4: Full desktop session — set_config then orchestrate, both over TCP.
/// Reproduces the exact sequence: fresh connection per request.
#[test]
fn test_tcp_full_desktop_session() {
    let addr = get_tcp_addr();
    if !tcp_is_reachable(&addr) {
        eprintln!("SKIP: agentd not reachable at {}", addr);
        return;
    }

    eprintln!("═══════════════════════════════════════════════════════");
    eprintln!("  FULL DESKTOP SESSION TEST (TCP through socat)");
    eprintln!("  Target: {}", addr);
    eprintln!("═══════════════════════════════════════════════════════");

    // ── Step 1: set_config (fresh TCP connection #1) ───────────────────────
    eprintln!("\n┌─ Step 1: set_config (fresh TCP connection)");
    let config_req = serde_json::json!({
        "request_type": "set_config",
        "provider": "gemini",
        "model": "gemini-2.5-pro",
        "api_key": "test-key-full-session",
    });
    match tcp_send_request(&addr, &config_req) {
        Ok(lines) => {
            assert!(!lines.is_empty(), "Step 1 FAIL: set_config got no response");
            let resp: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
            assert_eq!(resp["status"].as_str(), Some("ok"), "Step 1 FAIL: {}", resp);
            eprintln!("│  ✓ Response: {}", resp);
        }
        Err(e) => panic!("Step 1 FAIL: {}", e),
    }
    eprintln!("└─ set_config OK\n");

    // ── Step 2: orchestrate (fresh TCP connection #2) ──────────────────────
    eprintln!("┌─ Step 2: orchestrate (fresh TCP connection)");
    let orch_req = serde_json::json!({
        "type": "orchestrate",
        "prompt": "Create a REST API with health check endpoint",
        "project": "/tmp",
        "max_agents": 1,
        "mode": "simple",
    });
    match tcp_send_request(&addr, &orch_req) {
        Ok(lines) => {
            assert!(
                !lines.is_empty(),
                "Step 2 FAIL: orchestrate got 0 lines (CONNECTION RESET!)\n\
                 This is the OS error 10054 bug."
            );
            eprintln!("│  Received {} event(s):", lines.len());
            for (i, line) in lines.iter().enumerate() {
                let v: serde_json::Value = serde_json::from_str(line).unwrap_or_default();
                let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("?");
                let msg = v.get("message").and_then(|x| x.as_str())
                    .or(v.get("content").and_then(|x| x.as_str()))
                    .unwrap_or("");
                let short_msg = if msg.len() > 80 { &msg[..80] } else { msg };
                eprintln!("│  [{}] {:<15} {}", i, t, short_msg);
            }
            eprintln!("│  ✓ No connection reset!");
        }
        Err(e) => {
            panic!(
                "Step 2 FAIL: {}\n\n\
                 ╔══════════════════════════════════════════════════╗\n\
                 ║  THIS IS THE BUG.                               ║\n\
                 ║  The TCP connection through socat was reset.     ║\n\
                 ║  The desktop would show: 'Connection lost'       ║\n\
                 ╚══════════════════════════════════════════════════╝",
                e
            );
        }
    }
    eprintln!("└─ orchestrate OK");
    eprintln!("\n═══════════════════════════════════════════════════════");
    eprintln!("  PASS: Full desktop session completed over TCP");
    eprintln!("═══════════════════════════════════════════════════════");
}

// ════════════════════════════════════════════════════════════════════════════════
// Also keep Unix socket tests for completeness (run inside WSL)
// ════════════════════════════════════════════════════════════════════════════════

#[cfg(unix)]
mod unix_socket_tests {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    fn unix_send_request(socket_path: &str, request: &serde_json::Value) -> Vec<String> {
        let mut stream = UnixStream::connect(socket_path)
            .unwrap_or_else(|e| panic!("Failed to connect to {}: {}", socket_path, e));
        stream.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(10))).unwrap();

        let payload = serde_json::to_string(request).unwrap();
        stream.write_all(payload.as_bytes()).unwrap();
        stream.write_all(b"\n").unwrap();
        stream.flush().unwrap();

        let reader = BufReader::new(&stream);
        let mut lines = Vec::new();
        for line in reader.lines() {
            match line {
                Ok(l) if l.trim().is_empty() => continue,
                Ok(l) => lines.push(l),
                Err(_) => break,
            }
        }
        lines
    }

    #[test]
    fn test_unix_set_config() {
        let socket = std::env::var("AGENTD_TEST_SOCKET")
            .unwrap_or_else(|_| "/tmp/agentd.sock".into());
        if !std::path::Path::new(&socket).exists() {
            eprintln!("SKIP: {} not found", socket);
            return;
        }

        let req = serde_json::json!({
            "request_type": "set_config",
            "provider": "gemini",
            "model": "gemini-2.5-pro",
            "api_key": "unix-test-key",
        });
        let lines = unix_send_request(&socket, &req);
        assert!(!lines.is_empty());
        let resp: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(resp["status"].as_str(), Some("ok"));
        eprintln!("✓ Unix set_config: {}", resp);
    }

    #[test]
    fn test_unix_orchestrate() {
        let socket = std::env::var("AGENTD_TEST_SOCKET")
            .unwrap_or_else(|_| "/tmp/agentd.sock".into());
        if !std::path::Path::new(&socket).exists() {
            eprintln!("SKIP: {} not found", socket);
            return;
        }

        // Ensure config
        let cfg = serde_json::json!({
            "request_type": "set_config",
            "provider": "gemini",
            "model": "gemini-2.5-pro",
            "api_key": "unix-test-key",
        });
        unix_send_request(&socket, &cfg);

        // Orchestrate
        let req = serde_json::json!({
            "type": "orchestrate",
            "prompt": "Add hello world",
            "project": "/tmp",
            "max_agents": 1,
            "mode": "simple",
        });
        let lines = unix_send_request(&socket, &req);
        assert!(!lines.is_empty(), "orchestrate: connection reset (0 lines)");
        eprintln!("✓ Unix orchestrate: {} events", lines.len());
        for (i, line) in lines.iter().enumerate() {
            let v: serde_json::Value = serde_json::from_str(line).unwrap_or_default();
            let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("?");
            eprintln!("  [{}] {}", i, t);
        }
    }
}
