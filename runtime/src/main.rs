use std::env;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Usage: runtime <command>");
        println!("Commands:");
        println!("  status                Show runtime status");
        println!("  provision <spec.json> Provision sandboxes from JSON spec");
        println!("  list                  List active sandboxes");
        println!("  help                  Show this help");
        return Ok(());
    }

    match args[1].as_str() {
        "status" => {
            let socket_path =
                env::var("AGENTD_SOCKET").unwrap_or_else(|_| "/tmp/agentd.sock".to_string());

            // Check if agentd socket is responsive
            #[cfg(unix)]
            {
                use std::os::unix::net::UnixStream;
                match UnixStream::connect(&socket_path) {
                    Ok(_) => println!("Runtime status: OK (agentd connected at {})", socket_path),
                    Err(e) => println!("Runtime status: DEGRADED (agentd not reachable: {})", e),
                }
            }
            #[cfg(not(unix))]
            {
                let _ = socket_path;
                println!("Runtime status: OK (Unix sockets not available on this platform)");
            }
        }
        "list" => {
            let socket_path =
                env::var("AGENTD_SOCKET").unwrap_or_else(|_| "/tmp/agentd.sock".to_string());
            let client = runtime::AgentdClient::new(socket_path);
            match client.list_sandboxes() {
                Ok(sandboxes) => {
                    if sandboxes.is_empty() {
                        println!("No active sandboxes.");
                    } else {
                        println!("Active sandboxes:");
                        for sb in &sandboxes {
                            println!("  - {}", sb);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error listing sandboxes: {:?}", e);
                }
            }
        }
        "help" => {
            println!("Runtime control plane for agentd");
            println!("Manages sandbox/container lifecycle via the agentd socket API.");
            println!();
            println!("Environment variables:");
            println!("  AGENTD_SOCKET  Path to agentd Unix socket (default: /tmp/agentd.sock)");
        }
        _ => {
            println!("Unknown command: {}", args[1]);
            println!("Use 'runtime help' for available commands.");
        }
    }

    Ok(())
}
