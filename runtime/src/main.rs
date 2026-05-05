use std::env;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    let socket_path = env::var("AGENTD_SOCKET").unwrap_or_else(|_| "/tmp/agentd.sock".to_string());

    match args[1].as_str() {
        "status" => {
            #[cfg(unix)]
            {
                use std::os::unix::net::UnixStream;
                match UnixStream::connect(&socket_path) {
                    Ok(stream) => {
                        // Send a list_sandboxes request to verify the connection works
                        use std::io::{BufRead, BufReader, Write};
                        stream
                            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                            .ok();
                        let mut stream = stream;
                        let req = serde_json::json!({"request_type": "list_sandboxes"});
                        let _ = stream.write_all(serde_json::to_string(&req)?.as_bytes());
                        let _ = stream.write_all(b"\n");
                        let _ = stream.flush();

                        let mut reader = BufReader::new(&stream);
                        let mut response = String::new();
                        match reader.read_line(&mut response) {
                            Ok(_) => {
                                if let Ok(resp) =
                                    serde_json::from_str::<serde_json::Value>(&response)
                                {
                                    let count = resp
                                        .get("result")
                                        .and_then(|r| r.get("sandboxes"))
                                        .and_then(|s| s.as_array())
                                        .map(|a| a.len())
                                        .unwrap_or(0);
                                    println!("Runtime status: OK");
                                    println!("  Agentd socket: {}", socket_path);
                                    println!("  Active sandboxes: {}", count);
                                } else {
                                    println!(
                                        "Runtime status: OK (agentd connected at {})",
                                        socket_path
                                    );
                                }
                            }
                            Err(e) => {
                                println!("Runtime status: DEGRADED (read error: {})", e);
                            }
                        }
                    }
                    Err(e) => {
                        println!("Runtime status: UNAVAILABLE");
                        println!("  Agentd socket: {}", socket_path);
                        println!("  Error: {}", e);
                        println!(
                            "  Hint: Start agentd with: sudo agentd socket --path {}",
                            socket_path
                        );
                    }
                }
            }
            #[cfg(not(unix))]
            {
                println!("Runtime status: OK (Unix sockets not available on this platform)");
            }
        }

        "list" => {
            let client = runtime::AgentdClient::new(socket_path.clone());
            match client.list_sandboxes() {
                Ok(sandboxes) => {
                    if sandboxes.is_empty() {
                        println!("No active sandboxes.");
                    } else {
                        println!("Active sandboxes ({}):", sandboxes.len());
                        for sb in &sandboxes {
                            println!("  - {}", sb);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error listing sandboxes: {:?}", e);
                    eprintln!(
                        "Is agentd running? Start with: sudo agentd socket --path {}",
                        socket_path
                    );
                }
            }
        }

        "provision" => {
            if args.len() < 3 {
                eprintln!("Usage: runtime provision <spec.json>");
                eprintln!("  spec.json should contain a ProvisioningSpec JSON object");
                return Ok(());
            }

            let spec_path = &args[2];
            let spec_content = std::fs::read_to_string(spec_path)
                .map_err(|e| anyhow::anyhow!("Failed to read spec file '{}': {}", spec_path, e))?;
            let spec: agentd_protocol::ProvisioningSpec = serde_json::from_str(&spec_content)
                .map_err(|e| anyhow::anyhow!("Failed to parse spec JSON: {}", e))?;

            println!("Provisioning {} sandboxes...", spec.sandbox_specs.len());
            let runtime = runtime::Runtime::new(socket_path);
            match runtime.provision_sandboxes(&spec) {
                Ok(ready) => {
                    println!("Provisioning complete!");
                    println!("  Sandboxes: {}", ready.sandboxes.len());
                    for sb in &ready.sandboxes {
                        println!("  - {} ({} containers)", sb.sandbox_id, sb.containers.len());
                        for ct in &sb.containers {
                            println!("    - {} [{}]", ct.container_id, format!("{:?}", ct.status));
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Provisioning failed: {:?}", e);
                    std::process::exit(1);
                }
            }
        }

        "destroy" => {
            if args.len() < 3 {
                eprintln!("Usage: runtime destroy <sandbox_id>");
                return Ok(());
            }

            let sandbox_id = &args[2];
            let client = runtime::AgentdClient::new(socket_path);
            match client.destroy_sandbox(sandbox_id) {
                Ok(_) => println!("Sandbox '{}' destroyed.", sandbox_id),
                Err(e) => {
                    eprintln!("Failed to destroy sandbox '{}': {:?}", sandbox_id, e);
                    std::process::exit(1);
                }
            }
        }

        "health" => {
            let client = runtime::AgentdClient::new(socket_path.clone());
            match client.list_sandboxes() {
                Ok(sandboxes) => {
                    println!("System Health Report");
                    println!("===================");
                    println!("Agentd socket: {}", socket_path);
                    println!("Active sandboxes: {}", sandboxes.len());

                    // Print system resources if on Linux
                    #[cfg(target_os = "linux")]
                    {
                        if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
                            let total_kb: u64 = meminfo
                                .lines()
                                .find(|l| l.starts_with("MemTotal:"))
                                .and_then(|l| l.split_whitespace().nth(1))
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0);
                            let avail_kb: u64 = meminfo
                                .lines()
                                .find(|l| l.starts_with("MemAvailable:"))
                                .and_then(|l| l.split_whitespace().nth(1))
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0);
                            println!(
                                "Memory: {} MB / {} MB ({:.1}% used)",
                                (total_kb - avail_kb) / 1024,
                                total_kb / 1024,
                                ((total_kb - avail_kb) as f64 / total_kb as f64) * 100.0
                            );
                        }
                        if let Ok(loadavg) = std::fs::read_to_string("/proc/loadavg") {
                            println!("Load avg: {}", loadavg.trim());
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Health check failed: {:?}", e);
                    std::process::exit(1);
                }
            }
        }

        "help" | "--help" | "-h" => {
            print_usage();
        }

        _ => {
            eprintln!("Unknown command: {}", args[1]);
            eprintln!("Run 'runtime help' for available commands.");
            std::process::exit(1);
        }
    }

    Ok(())
}

fn print_usage() {
    println!("Runtime — Control plane for agentd");
    println!();
    println!("USAGE:");
    println!("  runtime <command> [args...]");
    println!();
    println!("COMMANDS:");
    println!("  status                Show runtime and agentd status");
    println!("  list                  List active sandboxes");
    println!("  provision <spec.json> Provision sandboxes from JSON spec");
    println!("  destroy <sandbox_id>  Destroy a sandbox");
    println!("  health                Show system health report");
    println!("  help                  Show this help");
    println!();
    println!("ENVIRONMENT:");
    println!("  AGENTD_SOCKET  Path to agentd Unix socket (default: /tmp/agentd.sock)");
    println!();
    println!("EXAMPLES:");
    println!("  runtime status");
    println!("  runtime list");
    println!("  runtime provision my-spec.json");
    println!("  runtime destroy sandbox-123");
}
