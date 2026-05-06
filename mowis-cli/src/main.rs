// mowis-cli — Standalone CLI version of the MowisAI desktop application
//
// Every boot step, every connection byte, every command — logged to your terminal.
// Runs on Windows (WSL2/QEMU), macOS (QEMU/HVF), Linux (direct socket).
//
// Usage:
//   cargo run                          # auto-detect platform and connect
//   cargo run -- --help                # show help
//   cargo run -- --skip-boot           # skip launcher, connect to existing agentd
//   cargo run -- list                  # send a single command and exit
//
// Environment variables:
//   MOWIS_QEMU        — path to QEMU binary
//   MOWIS_ISO         — path to Alpine ISO
//   MOWIS_DISK        — path to qcow2 disk image
//   MOWIS_SOCKET      — agentd Unix socket path (Linux)
//   MOWIS_AGENT_PORT  — agentd TCP port (default 8080 for dev mode, 9722 for QEMU)
//   MOWIS_DEBUG=1     — enable trace-level logging
//   RUST_LOG          — standard Rust log filter

mod auth;
mod connection;
#[cfg(windows)]
mod developer_mode;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
mod qemu;
mod types;
#[cfg(windows)]
mod windows;
mod agent_client;

use crate::connection::{open_connection, ConnectionStream};
use crate::types::*;
use anyhow::{Context, Result};
use colored::*;
use std::io::{self, Write};
use tokio::time::{sleep, Duration};

// ── Banner ───────────────────────────────────────────────────────────────────

fn banner() {
    eprintln!();
    eprintln!("{}", "  ┌─────────────────────────────────────────────────────────────┐".dimmed());
    eprintln!("{}", "  │                                                             │".dimmed());
    eprintln!("{}", "  │   ███╗   ███╗ ██████╗ ██╗    ██╗██╗███████╗               │".dimmed());
    eprintln!("{}", "  │   ████╗ ████║██╔═══██╗██║    ██║██║██╔════╝               │".dimmed());
    eprintln!("{}", "  │   ██╔████╔██║██║   ██║██║ █╗ ██║██║███████╗               │".dimmed());
    eprintln!("{}", "  │   ██║╚██╔╝██║██║   ██║██║███╗██║██║╚════██║               │".dimmed());
    eprintln!("{}", "  │   ██║ ╚═╝ ██║╚██████╔╝╚███╔███╔╝██║███████║               │".dimmed());
    eprintln!("{}", "  │   ╚═╝     ╚═╝ ╚═════╝  ╚══╝╚══╝ ╚═╝╚══════╝  CLI v0.1.0 │".dimmed());
    eprintln!("{}", "  │                                                             │".dimmed());
    eprintln!("{}", "  └─────────────────────────────────────────────────────────────┘".dimmed());
    eprintln!();
}

fn help() {
    banner();
    eprintln!("USAGE:");
    eprintln!("  mowis-cli [OPTIONS] [COMMAND]");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("  --help, -h        Show this help");
    eprintln!("  --skip-boot       Skip launcher, connect to existing agentd");
    eprintln!("  --socket <path>   Unix socket path (default: /tmp/agentd.sock)");
    eprintln!("  --tcp <addr>      TCP address (default: auto from launcher)");
    eprintln!();
    eprintln!("COMMANDS:");
    eprintln!("  (none)            Interactive REPL — type JSON commands");
    eprintln!("  list              List all sandboxes");
    eprintln!("  create_sandbox    Create a new sandbox");
    eprintln!("  <any JSON>        Send raw JSON command to agentd");
    eprintln!();
    eprintln!("AGENT COMMANDS (talk to mowis-agent):");
    eprintln!("  agent             Check if mowis-agent is running");
    eprintln!("  chat              Interactive chat with mowis-agent");
    eprintln!("  ask <prompt>      Send a one-shot prompt to mowis-agent");
    eprintln!("  sessions          List mowis-agent sessions");
    eprintln!();
    eprintln!("ENVIRONMENT:");
    eprintln!("  MOWIS_QEMU        Path to QEMU binary");
    eprintln!("  MOWIS_ISO         Path to Alpine Linux ISO");
    eprintln!("  MOWIS_DISK        Path to qcow2 disk image");
    eprintln!("  MOWIS_SOCKET      agentd Unix socket (Linux)");
    eprintln!("  MOWIS_AGENT_PORT  TCP port for agentd bridge");
    eprintln!("  MOWIS_DEBUG=1     Enable trace-level logging");
    eprintln!("  RUST_LOG          Standard log filter (e.g. debug,trace)");
    eprintln!();
    eprintln!("EXAMPLES:");
    eprintln!("  mowis-cli                         # auto-detect and boot");
    eprintln!("  mowis-cli --skip-boot             # connect to running agentd");
    eprintln!("  mowis-cli list                    # list sandboxes and exit");
    eprintln!("  mowis-cli '{{\"request_type\":\"list\"}}'  # raw JSON");
    eprintln!();
}

// ── Logging setup ────────────────────────────────────────────────────────────

fn setup_logging() {
    let mut builder = env_logger::Builder::new();

    // Default to info level
    builder.filter_level(log::LevelFilter::Info);

    // Override with RUST_LOG if set
    if std::env::var("RUST_LOG").is_ok() {
        builder.parse_filters(&std::env::var("RUST_LOG").unwrap());
    }

    // MOWIS_DEBUG=1 enables trace
    if std::env::var("MOWIS_DEBUG").is_ok() {
        builder.filter_level(log::LevelFilter::Debug);
    }

    builder.format_timestamp_millis();
    builder.format(move |buf, record| {
        let level = match record.level() {
            log::Level::Error => "ERR".red().bold(),
            log::Level::Warn => "WRN".yellow().bold(),
            log::Level::Info => "INF".green(),
            log::Level::Debug => "DBG".cyan(),
            log::Level::Trace => "TRC".dimmed(),
        };
        let ts = chrono::Local::now().format("%H:%M:%S%.3f");
        writeln!(buf, "{} {} {}", ts.to_string().dimmed(), level, record.args())
    });
    builder.init();
}

// ── Platform detection ───────────────────────────────────────────────────────

fn create_launcher() -> Box<dyn VmLauncher> {
    let os = std::env::consts::OS;
    log::info!("━━━ Platform: {} / {} ━━━", os, std::env::consts::ARCH);

    match os {
        #[cfg(target_os = "linux")]
        "linux" => {
            log::info!("Using LinuxDirect launcher (native socket)");
            Box::new(linux::LinuxDirectLauncher::new())
        }
        #[cfg(target_os = "macos")]
        "macos" => {
            log::info!("Using macOS QEMU/HVF launcher");
            Box::new(macos::MacOSLauncher::new())
        }
        #[cfg(windows)]
        "windows" => {
            log::info!("Using Windows launcher (WSL2 → Developer Mode → QEMU/WHPX)");
            Box::new(windows::WindowsLauncher::new())
        }
        other => {
            log::error!("Unsupported platform: {}", other);
            std::process::exit(1);
        }
    }
}

// ── Boot sequence ────────────────────────────────────────────────────────────

async fn boot_and_connect(skip_boot: bool, tcp_override: Option<&str>) -> Result<ConnectionStream> {
    if skip_boot {
        // Direct connection to existing agentd
        let addr = tcp_override.unwrap_or("127.0.0.1:9722");
        log::info!("Skipping boot, connecting directly to {}", addr);
        let info = ConnectionInfo {
            kind: ConnectionKind::TcpWithToken,
            socket_path: None,
            tcp_addr: Some(addr.to_owned()),
            pipe_name: None,
            auth_token: None,
        };
        return open_connection(&info).await.context("connect to agentd");
    }

    // Full boot sequence
    let launcher = create_launcher();
    eprintln!();
    eprintln!("{} {}", "Launcher:".bold(), launcher.name().yellow());
    eprintln!("{}", "─".repeat(60).dimmed());
    eprintln!();

    // Progress channel
    let (tx, mut rx) = tokio::sync::mpsc::channel::<BootProgress>(256);

    // Spawn progress printer (already handled by emit() in types.rs, but we
    // also keep this for any internal uses)
    tokio::spawn(async move {
        while let Some(_prog) = rx.recv().await {
            // Progress events are already printed by emit() in types.rs
        }
    });

    let start = std::time::Instant::now();
    let info = launcher.start(Some(tx)).await
        .context("launcher failed to start")?;

    let elapsed = start.elapsed();
    eprintln!();
    eprintln!("{} {:.1}s", "Boot completed in".green().bold(), elapsed.as_secs_f64());
    eprintln!();

    // If we have a TCP override, use it
    let mut info = info;
    if let Some(addr) = tcp_override {
        info.tcp_addr = Some(addr.to_owned());
    }

    log::info!("Opening connection: {:?}", info);
    eprintln!("{} {}…", "Connecting to".bold(),
        info.tcp_addr.as_deref().or(info.socket_path.as_ref().map(|p| p.to_str().unwrap_or(""))).unwrap_or("?"));

    let stream = open_connection(&info).await.context("open connection")?;
    eprintln!("{}", "Connected!".green().bold());
    eprintln!();

    Ok(stream)
}

// ── Interactive REPL ─────────────────────────────────────────────────────────

async fn repl(mut stream: ConnectionStream) -> Result<()> {
    eprintln!("{}", "━".repeat(60).dimmed());
    eprintln!("{}", "Interactive mode — type JSON commands or shortcuts:".bold());
    eprintln!("  {}  — list sandboxes", "list".cyan());
    eprintln!("  {}  — create sandbox", "create".cyan());
    eprintln!("  {}   — quit", "quit".cyan());
    eprintln!("  {}  — any valid JSON request", "<json>".cyan());
    eprintln!("{}", "━".repeat(60).dimmed());
    eprintln!();

    let stdin = io::stdin();
    let mut input = String::new();

    loop {
        print!("{} ", "mowis>".bold().blue());
        io::stdout().flush().ok();

        input.clear();
        if stdin.read_line(&mut input).is_err() || input.is_empty() {
            break;
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Shortcuts
        let json = match trimmed {
            "quit" | "exit" | "q" => {
                eprintln!("{}", "Goodbye.".dimmed());
                break;
            }
            "list" | "ls" => {
                serde_json::json!({ "request_type": "list" })
            }
            "create" | "new" => {
                serde_json::json!({ "request_type": "create_sandbox", "image": "alpine" })
            }
            "help" | "?" => {
                eprintln!("  Shortcuts: list, create, quit");
                eprintln!("  Or send raw JSON: {{\"request_type\":\"list\"}}");
                continue;
            }
            _ => {
                match serde_json::from_str::<serde_json::Value>(trimmed) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{} Invalid JSON: {}", "✗".red(), e);
                        eprintln!("  Tip: use shortcuts (list, create) or valid JSON");
                        continue;
                    }
                }
            }
        };

        // Send
        log::debug!("Sending: {}", serde_json::to_string(&json).unwrap_or_default());
        if let Err(e) = stream.send_json(&json).await {
            eprintln!("{} Send failed: {}", "✗".red(), e);
            break;
        }

        // Receive
        match stream.recv_json().await {
            Ok(Some(resp)) => {
                let status = resp.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                let pretty = serde_json::to_string_pretty(&resp).unwrap_or_default();

                match status {
                    "ok" => {
                        eprintln!("{} Response:", "✓".green());
                        // Syntax highlight the JSON
                        for line in pretty.lines() {
                            if line.contains("\"status\"") {
                                eprintln!("  {}", line.green());
                            } else if line.contains("\"error\"") {
                                eprintln!("  {}", line.red());
                            } else {
                                eprintln!("  {}", line);
                            }
                        }
                    }
                    "error" => {
                        let err = resp.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
                        eprintln!("{} Error: {}", "✗".red(), err.red());
                        if log::log_enabled!(log::Level::Debug) {
                            eprintln!("  Full response: {}", pretty);
                        }
                    }
                    _ => {
                        eprintln!("  {}", pretty);
                    }
                }
            }
            Ok(None) => {
                eprintln!("{}", "Connection closed by server.".yellow());
                break;
            }
            Err(e) => {
                eprintln!("{} Receive error: {}", "✗".red(), e);
                break;
            }
        }
        eprintln!();
    }

    Ok(())
}

// ── Single-command mode ──────────────────────────────────────────────────────

async fn single_command(mut stream: ConnectionStream, cmd: &str) -> Result<()> {
    let json: serde_json::Value = serde_json::from_str(cmd)
        .context("parse command as JSON")?;

    log::debug!("Sending command: {}", cmd);
    stream.send_json(&json).await.context("send command")?;

    match stream.recv_json().await {
        Ok(Some(resp)) => {
            println!("{}", serde_json::to_string_pretty(&resp).unwrap_or_default());
        }
        Ok(None) => {
            eprintln!("Connection closed");
        }
        Err(e) => {
            eprintln!("Error: {:#}", e);
        }
    }
    Ok(())
}

// ── Agent interactive chat ─────────────────────────────────────────────────

async fn agent_chat(port: u16) -> Result<()> {
    let client = agent_client::AgentClient::new(port);

    // Check health
    let health = client.health().await.context("mowis-agent not reachable")?;
    eprintln!("{} mowis-agent v{}", "✓".green(), health.version);
    eprintln!("{}", "━".repeat(60).dimmed());
    eprintln!("{}", "Interactive chat — type your prompt, or 'quit' to exit.".bold());
    eprintln!("{}", "━".repeat(60).dimmed());
    eprintln!();

    // Create session
    let session = client.create_session("CLI Chat").await.context("create session")?;
    eprintln!("{} Session {}", "Created".green(), session.id[..8].cyan());
    eprintln!();

    loop {
        print!("{} ", "you>".bold().blue());
        io::stdout().flush().ok();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() || input.is_empty() {
            break;
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "quit" || trimmed == "exit" || trimmed == "q" {
            eprintln!("{}", "Goodbye.".dimmed());
            break;
        }

        eprintln!("{}", "Thinking…".dimmed());

        match client.send_message(&session.id, trimmed).await {
            Ok(resp) => {
                // Extract assistant text from response
                if let Some(messages) = resp.get("messages").and_then(|v| v.as_array()) {
                    for msg in messages {
                        if msg.get("role").and_then(|v| v.as_str()) == Some("assistant") {
                            if let Some(parts) = msg.get("parts").and_then(|v| v.as_array()) {
                                for part in parts {
                                    if part.get("type").and_then(|v| v.as_str()) == Some("text") {
                                        if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                            eprintln!();
                                            eprintln!("{}", text);
                                            eprintln!();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("{} Error: {}", "✗".red(), e);
            }
        }
    }

    Ok(())
}

// ── Agent one-shot ask ─────────────────────────────────────────────────────

async fn agent_ask(port: u16, prompt: &str) -> Result<()> {
    let client = agent_client::AgentClient::new(port);

    let health = client.health().await.context("mowis-agent not reachable")?;
    eprintln!("{} mowis-agent v{}", "✓".green(), health.version);

    let session = client.create_session("CLI Ask").await.context("create session")?;
    eprintln!("{} Sending prompt…", "→".blue());

    match client.send_message(&session.id, prompt).await {
        Ok(resp) => {
            if let Some(messages) = resp.get("messages").and_then(|v| v.as_array()) {
                for msg in messages {
                    if msg.get("role").and_then(|v| v.as_str()) == Some("assistant") {
                        if let Some(parts) = msg.get("parts").and_then(|v| v.as_array()) {
                            for part in parts {
                                if part.get("type").and_then(|v| v.as_str()) == Some("text") {
                                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                        println!("{}", text);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("{} Error: {}", "✗".red(), e);
            std::process::exit(1);
        }
    }

    Ok(())
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    setup_logging();
    banner();

    // Parse args
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut skip_boot = false;
    let mut tcp_override: Option<String> = None;
    let mut command: Option<String> = None;
    let mut ask_prompt: Option<String> = None;
    let mut agent_port: u16 = 4096;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                help();
                return Ok(());
            }
            "--skip-boot" => {
                skip_boot = true;
            }
            "--socket" => {
                i += 1;
                if let Some(path) = args.get(i) {
                    std::env::set_var("MOWIS_SOCKET", path);
                }
            }
            "--tcp" => {
                i += 1;
                if let Some(addr) = args.get(i) {
                    tcp_override = Some(addr.clone());
                }
            }
            "--agent-port" => {
                i += 1;
                if let Some(port) = args.get(i) {
                    agent_port = port.parse().unwrap_or(4096);
                }
            }
            "ask" => {
                i += 1;
                ask_prompt = Some(args[i..].join(" "));
                break;
            }
            other => {
                command = Some(other.to_string());
            }
        }
        i += 1;
    }

    // Handle agent commands that don't need agentd connection
    match command.as_deref() {
        Some("agent") => {
            let client = agent_client::AgentClient::new(agent_port);
            match client.health().await {
                Ok(h) => {
                    eprintln!("{} mowis-agent v{} on port {}", "✓".green(), h.version, agent_port);
                    eprintln!("  CWD: {}", h.cwd);
                }
                Err(e) => {
                    eprintln!("{} mowis-agent not reachable on port {}: {}", "✗".red(), agent_port, e);
                    eprintln!("  Start it with: mowis-agent serve --port {}", agent_port);
                }
            }
            return Ok(());
        }
        Some("chat") => {
            return agent_chat(agent_port).await;
        }
        Some("sessions") => {
            let client = agent_client::AgentClient::new(agent_port);
            let sessions = client.list_sessions().await.context("list sessions")?;
            if sessions.is_empty() {
                eprintln!("No sessions.");
            } else {
                for s in &sessions {
                    eprintln!("  {} {} ({} messages)", s.id[..8].cyan(), s.title, s.message_count);
                }
            }
            return Ok(());
        }
        _ => {}
    }

    // Handle one-shot ask
    if let Some(ref prompt) = ask_prompt {
        return agent_ask(agent_port, prompt).await;
    }

    eprintln!("{}", "Starting MowisAI CLI…".bold());
    eprintln!("  Platform: {} / {}", std::env::consts::OS, std::env::consts::ARCH);
    eprintln!("  Skip boot: {}", skip_boot);
    if let Some(ref tcp) = tcp_override {
        eprintln!("  TCP override: {}", tcp);
    }
    eprintln!();

    // Boot and connect
    let stream = boot_and_connect(skip_boot, tcp_override.as_deref()).await?;

    // Run
    if let Some(cmd) = command {
        single_command(stream, &cmd).await
    } else {
        repl(stream).await
    }
}
