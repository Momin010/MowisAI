// Global install:
//   cargo build --release
//   sudo cp target/release/mowisai /usr/local/bin/
//   mowisai

use anyhow::Result;
use libagent::config::MowisConfig;
use libagent::setup::SetupWizard;
use std::path::Path;
use std::thread;
use std::time::Duration;

fn main() -> Result<()> {
    let log_path = MowisConfig::config_dir().join("mowisai.log");
    let _ = libagent::logging::init(&log_path);

    let config = if SetupWizard::needs_setup() {
        SetupWizard::run()?
    } else {
        MowisConfig::load()?.unwrap_or_default()
    };

    let socket_path = config.socket_path.clone();
    let socket_ready = start_socket_server(&socket_path);

    if !socket_ready {
        eprintln!("  \u{26a0} Socket server failed to start at {}", socket_path);
        eprintln!("  Chat mode will work, but orchestration requires the socket server.");
        eprintln!("  Try running with sudo for overlayfs support.");
    }

    libagent::tui::run_interactive(config)?;

    Ok(())
}

fn start_socket_server(socket_path: &str) -> bool {
    let _ = std::fs::remove_file(socket_path);

    let path = socket_path.to_string();
    thread::Builder::new()
        .name("socket-server".into())
        .spawn(move || {
            if let Err(e) = libagent::socket_server::run_server(&path) {
                eprintln!("Socket server error: {}", e);
            }
        })
        .ok();

    for _ in 0..30 {
        thread::sleep(Duration::from_millis(100));
        if Path::new(socket_path).exists() {
            return true;
        }
    }
    false
}
