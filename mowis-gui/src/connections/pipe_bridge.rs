/// Named pipe bridge for WSL2
///
/// Bridges WSL2 Unix socket to Windows named pipe.
/// This allows Windows applications to communicate with agentd running in WSL2.

#[cfg(target_os = "windows")]
pub async fn bridge_wsl_to_pipe(
    wsl_distro: &str,
    pipe_name: &str,
) -> anyhow::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    log::info!("Starting WSL2 → Named Pipe bridge");
    log::info!("  WSL socket: \\\\wsl$\\{}\\tmp\\agentd.sock", wsl_distro);
    log::info!("  Named pipe: {}", pipe_name);

    // Create named pipe server
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(pipe_name)?;

    log::info!("Named pipe server created, waiting for client connection");

    loop {
        // Wait for client to connect
        server.connect().await?;
        log::info!("Client connected to named pipe");

        // Connect to WSL2 Unix socket
        let _wsl_socket_path = format!("\\\\wsl$\\{}\\tmp\\agentd.sock", wsl_distro);
        
        // On Windows, we need to use a different approach to connect to WSL sockets
        // For now, we'll use TCP forwarding as a workaround
        // TODO: Implement proper WSL socket bridging
        
        log::warn!("WSL socket bridging not fully implemented yet");
        log::warn!("Consider using TCP+token connection as fallback");

        // For now, just close the connection
        break;
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub async fn bridge_wsl_to_pipe(
    _wsl_distro: &str,
    _pipe_name: &str,
) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("Named pipe bridge only available on Windows"))
}
