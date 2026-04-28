# Design Document: Cross-Platform Support

## Overview

This design implements Docker-Desktop-style cross-platform support for MowisAI, enabling the application to run seamlessly on Linux, macOS, and Windows. The core insight is that `agentd` remains Linux-only (leveraging overlayfs, cgroups v2, namespaces), while `mowis-gui` becomes truly cross-platform by launching `agentd` inside a lightweight Linux VM on non-Linux hosts.

**Architecture Philosophy:**
- **Native where possible**: Linux runs `agentd` directly with zero overhead
- **Transparent virtualization**: macOS and Windows users see no VM — it's an implementation detail
- **Security-first IPC**: Use the strongest available channel on each platform (Unix socket → vsock → named pipe → authenticated TCP)
- **Zero manual setup**: No Homebrew, no WSL2 configuration, no prerequisites — just download and run

**Platform Strategy:**
- **Linux**: Direct execution, Unix socket, native performance
- **macOS 10.15+**: Virtualization.framework microVM, virtio-vsock bridge, ~5s startup
- **Windows 10 2004+**: WSL2 distribution, named pipe bridge, ~5s startup
- **Fallback**: Bundled static QEMU for older OS versions, TCP+token auth

**Key Design Decisions:**
1. **VM launcher abstraction**: Platform-specific launchers implement a common `VmLauncher` trait, selected at runtime
2. **Socket bridge abstraction**: Connection logic is unified behind a `DaemonConnection` trait that hides platform differences
3. **Static musl binary**: `agentd` compiles to a fully static binary for embedding in the Alpine image
4. **Auth token protocol**: Jupyter-style token authentication for TCP fallback paths
5. **Snapshot-based fast boot**: First boot takes 15-20s, subsequent boots reuse VM snapshots for <5s startup


## Architecture

### High-Level Component Diagram

```mermaid
graph TB
    subgraph "Host OS (Linux/macOS/Windows)"
        GUI[mowis-gui<br/>Native Binary]
        Backend[Backend<br/>Lifecycle Manager]
        Launcher{VM Launcher<br/>Platform Selector}
        
        LinuxLauncher[Linux Direct<br/>Process Spawn]
        MacLauncher[macOS Launcher<br/>Virtualization.framework]
        WSLLauncher[WSL2 Launcher<br/>wsl.exe]
        QEMULauncher[QEMU Launcher<br/>Bundled Binary]
        
        Bridge[Socket Bridge<br/>Connection Abstraction]
        UnixConn[Unix Socket]
        VsockConn[virtio-vsock]
        PipeConn[Named Pipe]
        TCPConn[TCP + Auth Token]
    end
    
    subgraph "Linux Environment"
        VM[Linux VM<br/>Alpine Image]
        AgentD[agentd<br/>Static Binary]
        Socket[/tmp/agentd.sock]
    end
    
    GUI --> Backend
    Backend --> Launcher
    Backend --> Bridge
    
    Launcher --> LinuxLauncher
    Launcher --> MacLauncher
    Launcher --> WSLLauncher
    Launcher --> QEMULauncher
    
    LinuxLauncher --> AgentD
    MacLauncher --> VM
    WSLLauncher --> VM
    QEMULauncher --> VM
    
    VM --> AgentD
    AgentD --> Socket
    
    Bridge --> UnixConn
    Bridge --> VsockConn
    Bridge --> PipeConn
    Bridge --> TCPConn
    
    UnixConn -.-> Socket
    VsockConn -.-> Socket
    PipeConn -.-> Socket
    TCPConn -.-> Socket
```

### Platform Detection and Launcher Selection

The `Backend` component performs runtime platform detection and selects the appropriate launcher:

```rust
// Pseudocode
fn select_launcher() -> Box<dyn VmLauncher> {
    match std::env::consts::OS {
        "linux" => Box::new(LinuxDirectLauncher),
        "macos" => {
            if virtualization_framework_available() {
                Box::new(MacOSLauncher)
            } else {
                Box::new(QEMULauncher)
            }
        }
        "windows" => {
            if wsl2_available() {
                Box::new(WSL2Launcher)
            } else {
                Box::new(QEMULauncher)
            }
        }
        _ => panic!("Unsupported platform")
    }
}
```

**Detection Logic:**
- **macOS**: Check for `Virtualization.framework` availability via FFI probe (macOS 10.15+)
- **Windows**: Execute `wsl --status` and check exit code (Windows 10 2004+)
- **Fallback**: If primary launcher unavailable, use QEMU launcher

### VM Launcher Trait

All platform-specific launchers implement a common trait:

```rust
pub trait VmLauncher: Send + Sync {
    /// Start the VM and agentd daemon
    /// Returns connection info (socket path or TCP address + token)
    async fn start(&self) -> Result<ConnectionInfo>;
    
    /// Stop the VM and clean up resources
    async fn stop(&self) -> Result<()>;
    
    /// Check if the VM is running and healthy
    async fn health_check(&self) -> Result<bool>;
    
    /// Get the connection info for an already-running VM
    async fn connection_info(&self) -> Result<ConnectionInfo>;
}

pub enum ConnectionInfo {
    UnixSocket { path: PathBuf },
    Vsock { cid: u32, port: u32 },
    NamedPipe { name: String },
    TcpWithToken { addr: SocketAddr, token: String },
}
```

### Socket Bridge Architecture

The `SocketBridge` component abstracts connection establishment and message framing:

```rust
pub trait DaemonConnection: Send + Sync {
    /// Establish connection to agentd
    async fn connect(&mut self) -> Result<()>;
    
    /// Send a JSON-RPC request
    async fn send_request(&mut self, req: SocketRequest) -> Result<()>;
    
    /// Receive a JSON-RPC response (blocking until available)
    async fn recv_response(&mut self) -> Result<SocketResponse>;
    
    /// Close the connection
    async fn close(&mut self) -> Result<()>;
}
```

**Implementation Strategy:**
- **Unix Socket**: `tokio::net::UnixStream` with newline-delimited JSON
- **Vsock**: `tokio::net::UnixStream` to host-side vsock proxy socket
- **Named Pipe**: `tokio::net::windows::named_pipe` with newline-delimited JSON
- **TCP**: `tokio::net::TcpStream` with auth token in first message, then newline-delimited JSON

**Message Framing:**
All transports use newline-delimited JSON (`\n` separator). Each message is a complete JSON object followed by `\n`. This ensures compatibility with the existing `agentd` socket protocol.


## Components and Interfaces

### 1. Backend Refactoring (`mowis-gui/src/backend.rs`)

**Current State:**
- Hard-coded Unix socket path `/tmp/agentd.sock`
- Direct `tokio::net::UnixStream` usage
- Linux-only `tokio::process::Command` spawn

**New Design:**

```rust
pub struct Backend {
    launcher: Box<dyn VmLauncher>,
    connection: Option<Box<dyn DaemonConnection>>,
    event_tx: mpsc::Sender<BackendEvent>,
    command_rx: mpsc::Receiver<FrontendCommand>,
}

impl Backend {
    pub fn spawn(project_dir: String) -> Self {
        let launcher = select_launcher();
        // ... spawn background thread with tokio runtime
    }
}

async fn run(
    launcher: Box<dyn VmLauncher>,
    event_tx: mpsc::Sender<BackendEvent>,
    command_rx: mpsc::Receiver<FrontendCommand>,
) {
    // 1. Start VM/daemon
    let conn_info = launcher.start().await?;
    
    // 2. Establish connection
    let mut connection = create_connection(conn_info).await?;
    connection.connect().await?;
    
    // 3. Notify GUI
    event_tx.send(BackendEvent::DaemonStarted).await?;
    
    // 4. Command loop
    run_command_handler(command_rx, connection, event_tx).await;
}
```

**Key Changes:**
- Replace direct socket code with `DaemonConnection` trait
- Add launcher selection and lifecycle management
- Add connection retry logic (5 attempts, 1s delay)
- Add health check polling (every 10s)

### 2. Linux Direct Launcher (`mowis-gui/src/launchers/linux.rs`)

**Responsibilities:**
- Spawn `agentd` as a child process
- Create Unix socket at `$XDG_RUNTIME_DIR/agentd.sock` (fallback: `/tmp/agentd-$UID.sock`)
- Set socket permissions to `0600`
- Monitor process health

**Implementation:**

```rust
pub struct LinuxDirectLauncher {
    socket_path: PathBuf,
    child: Option<tokio::process::Child>,
}

impl VmLauncher for LinuxDirectLauncher {
    async fn start(&self) -> Result<ConnectionInfo> {
        // 1. Determine socket path
        let socket_path = runtime_dir()
            .map(|d| d.join("agentd.sock"))
            .unwrap_or_else(|| PathBuf::from(format!("/tmp/agentd-{}.sock", nix::unistd::getuid())));
        
        // 2. Remove stale socket
        let _ = tokio::fs::remove_file(&socket_path).await;
        
        // 3. Spawn agentd
        let child = tokio::process::Command::new("agentd")
            .args(["socket", "--path", socket_path.to_str().unwrap()])
            .spawn()?;
        
        // 4. Wait for socket to appear
        wait_for_socket(&socket_path, Duration::from_secs(5)).await?;
        
        // 5. Set permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&socket_path, perms)?;
        }
        
        Ok(ConnectionInfo::UnixSocket { path: socket_path })
    }
}
```

### 3. macOS Launcher (`mowis-gui/src/launchers/macos.rs`)

**Responsibilities:**
- Invoke `Virtualization.framework` via FFI or Swift shim
- Boot Alpine image with 512 MB RAM, 1 vCPU
- Configure virtio-vsock for socket bridging
- Create/restore VM snapshots for fast boot

**FFI Strategy:**

Option A: Direct Rust FFI to Objective-C runtime
```rust
#[link(name = "Virtualization", kind = "framework")]
extern "C" {
    fn VZVirtualMachineConfiguration_new() -> *mut c_void;
    // ... other FFI declarations
}
```

Option B: Swift shim compiled into app bundle
```swift
// vm_launcher.swift
@_cdecl("mowis_start_vm")
func startVM(imagePath: UnsafePointer<CChar>, socketPath: UnsafeMutablePointer<CChar>) -> Int32 {
    let config = VZVirtualMachineConfiguration()
    // ... configure VM
    return 0
}
```

**Recommended**: Option B (Swift shim) for maintainability and type safety.

**Implementation:**

```rust
pub struct MacOSLauncher {
    vm_handle: Option<VmHandle>,
    snapshot_path: PathBuf,
}

impl VmLauncher for MacOSLauncher {
    async fn start(&self) -> Result<ConnectionInfo> {
        // 1. Check for existing snapshot
        if self.snapshot_path.exists() {
            return self.restore_from_snapshot().await;
        }
        
        // 2. Full boot from Alpine image
        let image_path = bundle_resource_path("alpine.img")?;
        let vsock_path = runtime_dir()?.join("agentd-vsock.sock");
        
        // 3. Call Swift shim via FFI
        unsafe {
            let result = mowis_start_vm(
                image_path.to_str().unwrap().as_ptr() as *const i8,
                vsock_path.to_str().unwrap().as_ptr() as *mut i8,
            );
            if result != 0 {
                return Err(anyhow!("VM start failed: {}", result));
            }
        }
        
        // 4. Wait for agentd socket inside VM to be bridged
        wait_for_socket(&vsock_path, Duration::from_secs(20)).await?;
        
        // 5. Create snapshot for next boot
        self.create_snapshot().await?;
        
        Ok(ConnectionInfo::UnixSocket { path: vsock_path })
    }
}
```

**Vsock Bridge:**
The Swift shim configures a virtio-vsock device that exposes the guest's `/tmp/agentd.sock` as a Unix socket on the host at `$XDG_RUNTIME_DIR/agentd-vsock.sock`. This is done using `VZVirtioSocketDeviceConfiguration` with a socket listener.

### 4. WSL2 Launcher (`mowis-gui/src/launchers/wsl2.rs`)

**Responsibilities:**
- Import Alpine image as WSL2 distribution
- Start `agentd` inside WSL2
- Bridge Unix socket to Windows named pipe
- Handle distribution corruption recovery

**Implementation:**

```rust
pub struct WSL2Launcher {
    distro_name: String,
    pipe_name: String,
}

impl VmLauncher for WSL2Launcher {
    async fn start(&self) -> Result<ConnectionInfo> {
        // 1. Check if distribution exists
        if !self.distro_exists().await? {
            self.import_distro().await?;
        }
        
        // 2. Start agentd inside WSL2
        let output = tokio::process::Command::new("wsl")
            .args(["-d", &self.distro_name, "--", "/usr/local/bin/agentd", "socket", "--path", "/tmp/agentd.sock"])
            .spawn()?;
        
        // 3. Start named pipe bridge
        self.start_pipe_bridge().await?;
        
        // 4. Wait for pipe to be connectable
        wait_for_pipe(&self.pipe_name, Duration::from_secs(10)).await?;
        
        Ok(ConnectionInfo::NamedPipe { name: self.pipe_name.clone() })
    }
}

impl WSL2Launcher {
    async fn import_distro(&self) -> Result<()> {
        let image_path = bundle_resource_path("alpine.tar.gz")?;
        let install_dir = app_data_dir()?.join("wsl");
        tokio::fs::create_dir_all(&install_dir).await?;
        
        tokio::process::Command::new("wsl")
            .args(["--import", &self.distro_name, install_dir.to_str().unwrap(), image_path.to_str().unwrap()])
            .status()
            .await?;
        
        Ok(())
    }
    
    async fn start_pipe_bridge(&self) -> Result<()> {
        // Spawn a background task that forwards between WSL2 Unix socket and Windows named pipe
        tokio::spawn(async move {
            bridge_wsl_to_pipe("/tmp/agentd.sock", &self.pipe_name).await
        });
        Ok(())
    }
}
```

**Named Pipe Bridge:**
A background tokio task connects to the WSL2 Unix socket via `\\wsl$\MowisAI\tmp\agentd.sock` (WSL2 exposes Unix sockets as Windows paths) and forwards all traffic to a Windows named pipe `\\.\pipe\MowisAI\agentd` secured with a DACL.

### 5. QEMU Launcher (`mowis-gui/src/launchers/qemu.rs`)

**Responsibilities:**
- Boot Alpine image using bundled QEMU binary
- Forward guest socket to host TCP port
- Generate and validate auth tokens
- Monitor QEMU process health

**Implementation:**

```rust
pub struct QEMULauncher {
    qemu_binary: PathBuf,
    tcp_port: u16,
    auth_token: String,
}

impl VmLauncher for QEMULauncher {
    async fn start(&self) -> Result<ConnectionInfo> {
        // 1. Generate auth token
        let token = generate_auth_token();
        let token_file = app_data_dir()?.join("auth-token");
        write_token_file(&token_file, &token).await?;
        
        // 2. Choose random TCP port
        let port = choose_ephemeral_port();
        
        // 3. Start QEMU with port forwarding
        let image_path = bundle_resource_path("alpine.img")?;
        let child = tokio::process::Command::new(&self.qemu_binary)
            .args([
                "-m", "512",
                "-smp", "1",
                "-drive", &format!("file={},format=qcow2", image_path.display()),
                "-netdev", &format!("user,id=net0,hostfwd=tcp:127.0.0.1:{}-:8080", port),
                "-device", "virtio-net-pci,netdev=net0",
                "-nographic",
                "-enable-kvm",  // Linux only
            ])
            .spawn()?;
        
        // 4. Wait for agentd to start and listen on TCP
        wait_for_tcp(port, Duration::from_secs(20)).await?;
        
        Ok(ConnectionInfo::TcpWithToken {
            addr: SocketAddr::from(([127, 0, 0, 1], port)),
            token,
        })
    }
}

fn generate_auth_token() -> String {
    use rand::Rng;
    let mut rng = rand::rngs::OsRng;
    let bytes: [u8; 32] = rng.gen();
    base64::encode(&bytes)
}
```

**QEMU Binary Selection:**
- macOS: `qemu-system-x86_64` (Intel) or `qemu-system-aarch64` (Apple Silicon)
- Windows: `qemu-system-x86_64.exe`
- Acceleration: `-enable-kvm` (Linux), `-accel hvf` (macOS), `-accel whpx` (Windows)


### 6. Socket Bridge Implementations

#### Unix Socket Connection (`mowis-gui/src/connections/unix.rs`)

```rust
pub struct UnixSocketConnection {
    stream: Option<tokio::net::UnixStream>,
    reader: Option<tokio::io::BufReader<tokio::net::UnixStream>>,
    socket_path: PathBuf,
}

impl DaemonConnection for UnixSocketConnection {
    async fn connect(&mut self) -> Result<()> {
        let stream = tokio::net::UnixStream::connect(&self.socket_path).await?;
        let (read_half, write_half) = stream.into_split();
        self.reader = Some(tokio::io::BufReader::new(read_half));
        self.stream = Some(write_half.reunite(self.reader.take().unwrap().into_inner())?);
        Ok(())
    }
    
    async fn send_request(&mut self, req: SocketRequest) -> Result<()> {
        let mut json = serde_json::to_string(&req)?;
        json.push('\n');
        self.stream.as_mut().unwrap().write_all(json.as_bytes()).await?;
        Ok(())
    }
    
    async fn recv_response(&mut self) -> Result<SocketResponse> {
        let mut line = String::new();
        self.reader.as_mut().unwrap().read_line(&mut line).await?;
        Ok(serde_json::from_str(&line)?)
    }
}
```

#### TCP with Auth Token Connection (`mowis-gui/src/connections/tcp.rs`)

```rust
pub struct TcpTokenConnection {
    stream: Option<tokio::net::TcpStream>,
    reader: Option<tokio::io::BufReader<tokio::net::TcpStream>>,
    addr: SocketAddr,
    token: String,
    authenticated: bool,
}

impl DaemonConnection for TcpTokenConnection {
    async fn connect(&mut self) -> Result<()> {
        let stream = tokio::net::TcpStream::connect(self.addr).await?;
        let (read_half, write_half) = stream.into_split();
        self.reader = Some(tokio::io::BufReader::new(read_half));
        self.stream = Some(write_half.reunite(self.reader.take().unwrap().into_inner())?);
        
        // Send auth token as first message
        let auth_msg = json!({
            "type": "auth",
            "token": self.token
        });
        let mut json = serde_json::to_string(&auth_msg)?;
        json.push('\n');
        self.stream.as_mut().unwrap().write_all(json.as_bytes()).await?;
        
        // Wait for auth response
        let mut line = String::new();
        self.reader.as_mut().unwrap().read_line(&mut line).await?;
        let response: Value = serde_json::from_str(&line)?;
        
        if response["status"] != "authenticated" {
            return Err(anyhow!("Authentication failed"));
        }
        
        self.authenticated = true;
        Ok(())
    }
    
    async fn send_request(&mut self, req: SocketRequest) -> Result<()> {
        if !self.authenticated {
            return Err(anyhow!("Not authenticated"));
        }
        // Same as Unix socket
        let mut json = serde_json::to_string(&req)?;
        json.push('\n');
        self.stream.as_mut().unwrap().write_all(json.as_bytes()).await?;
        Ok(())
    }
}
```

#### Windows Named Pipe Connection (`mowis-gui/src/connections/pipe.rs`)

```rust
#[cfg(windows)]
pub struct NamedPipeConnection {
    pipe: Option<tokio::net::windows::named_pipe::NamedPipeClient>,
    reader: Option<tokio::io::BufReader<tokio::net::windows::named_pipe::NamedPipeClient>>,
    pipe_name: String,
}

#[cfg(windows)]
impl DaemonConnection for NamedPipeConnection {
    async fn connect(&mut self) -> Result<()> {
        let pipe = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(&self.pipe_name)?;
        
        // Named pipes are bidirectional, but we split for consistent API
        self.pipe = Some(pipe);
        Ok(())
    }
    
    async fn send_request(&mut self, req: SocketRequest) -> Result<()> {
        let mut json = serde_json::to_string(&req)?;
        json.push('\n');
        self.pipe.as_mut().unwrap().write_all(json.as_bytes()).await?;
        Ok(())
    }
    
    async fn recv_response(&mut self) -> Result<SocketResponse> {
        let mut line = String::new();
        let mut reader = tokio::io::BufReader::new(self.pipe.as_mut().unwrap());
        reader.read_line(&mut line).await?;
        Ok(serde_json::from_str(&line)?)
    }
}
```

### 7. agentd Auth Token Handler (`agentd/src/socket_server.rs`)

**New Code:**

```rust
// Add to socket_server.rs

struct ConnectionState {
    authenticated: bool,
    stream: UnixStream,
}

fn handle_client(mut stream: UnixStream) -> Result<()> {
    let mut state = ConnectionState {
        authenticated: !auth_required(),
        stream,
    };
    
    let reader = BufReader::new(&state.stream);
    for line in reader.lines() {
        let line = line?;
        let request: Value = serde_json::from_str(&line)?;
        
        // First message must be auth if token is required
        if !state.authenticated {
            if request["type"] != "auth" {
                let error = json!({"status": "error", "message": "Authentication required"});
                writeln!(state.stream, "{}", error)?;
                return Ok(());
            }
            
            let token = request["token"].as_str().ok_or(anyhow!("Missing token"))?;
            if !validate_token(token)? {
                let error = json!({"status": "error", "message": "Invalid token"});
                writeln!(state.stream, "{}", error)?;
                return Ok(());
            }
            
            state.authenticated = true;
            let success = json!({"status": "authenticated"});
            writeln!(state.stream, "{}", success)?;
            continue;
        }
        
        // Normal request handling
        let response = handle_request(request)?;
        writeln!(state.stream, "{}", serde_json::to_string(&response)?)?;
    }
    
    Ok(())
}

fn auth_required() -> bool {
    // Auth required when using TCP (QEMU fallback)
    std::env::var("AGENTD_AUTH_REQUIRED").is_ok()
}

fn validate_token(token: &str) -> Result<bool> {
    let token_file = dirs::home_dir()
        .ok_or(anyhow!("No home directory"))?
        .join(".mowisai")
        .join("auth-token");
    
    let expected = std::fs::read_to_string(token_file)?;
    Ok(token == expected.trim())
}
```

**Token Generation (in Alpine init script):**

```bash
#!/bin/sh
# /etc/init.d/agentd

if [ -n "$AGENTD_AUTH_REQUIRED" ]; then
    # Generate 256-bit token
    TOKEN=$(head -c 32 /dev/urandom | base64)
    mkdir -p /root/.mowisai
    echo "$TOKEN" > /root/.mowisai/auth-token
    chmod 600 /root/.mowisai/auth-token
    
    # Write to shared volume so host can read it
    echo "$TOKEN" > /mnt/host/auth-token
fi

exec /usr/local/bin/agentd socket --path /tmp/agentd.sock
```


## Data Models

### ConnectionInfo Enum

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionInfo {
    /// Unix domain socket (Linux, macOS direct)
    UnixSocket {
        path: PathBuf,
    },
    
    /// virtio-vsock (macOS Virtualization.framework)
    /// Exposed as Unix socket on host side
    Vsock {
        path: PathBuf,  // Host-side socket path
    },
    
    /// Windows named pipe (WSL2)
    NamedPipe {
        name: String,  // e.g. "\\.\pipe\MowisAI\agentd"
    },
    
    /// TCP with auth token (QEMU fallback)
    TcpWithToken {
        addr: SocketAddr,
        token: String,
    },
}
```

### VmHandle Struct

```rust
#[derive(Debug, Clone)]
pub struct VmHandle {
    /// Unique identifier for this VM instance
    pub id: String,
    
    /// Platform-specific process ID or handle
    pub pid: Option<u32>,
    
    /// Connection information
    pub connection: ConnectionInfo,
    
    /// VM state snapshot path (for fast restart)
    pub snapshot_path: Option<PathBuf>,
    
    /// Timestamp of last health check
    pub last_health_check: Instant,
}
```

### LauncherConfig Struct

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherConfig {
    /// Path to Alpine image
    pub image_path: PathBuf,
    
    /// Path to agentd binary (for Linux direct launch)
    pub agentd_binary: Option<PathBuf>,
    
    /// VM memory in MB (default: 512)
    pub memory_mb: u64,
    
    /// VM CPU count (default: 1)
    pub cpu_count: u32,
    
    /// Enable snapshot-based fast boot
    pub enable_snapshots: bool,
    
    /// Snapshot directory
    pub snapshot_dir: PathBuf,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            image_path: bundle_resource_path("alpine.img").unwrap(),
            agentd_binary: which::which("agentd").ok(),
            memory_mb: 512,
            cpu_count: 1,
            enable_snapshots: true,
            snapshot_dir: app_data_dir().unwrap().join("snapshots"),
        }
    }
}
```

### Auth Token File Format

**Location:** `~/.mowisai/auth-token` (Unix) or `%USERPROFILE%\.mowisai\auth-token` (Windows)

**Format:** Single line containing base64-encoded 256-bit random value

**Permissions:** `0600` (Unix) or ACL granting access only to current user (Windows)

**Example:**
```
dGhpc2lzYTI1NmJpdHJhbmRvbXRva2VuZXhhbXBsZQ==
```

**Generation:**
```rust
use rand::Rng;

fn generate_auth_token() -> String {
    let mut rng = rand::rngs::OsRng;
    let bytes: [u8; 32] = rng.gen();
    base64::encode(&bytes)
}
```

**Validation:**
```rust
fn validate_token(provided: &str) -> Result<bool> {
    let token_file = dirs::home_dir()
        .ok_or(anyhow!("No home directory"))?
        .join(".mowisai")
        .join("auth-token");
    
    let expected = std::fs::read_to_string(token_file)
        .context("Failed to read auth token")?;
    
    Ok(provided == expected.trim())
}
```

### Platform Detection

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    MacOS,
    Windows,
}

impl Platform {
    pub fn current() -> Self {
        match std::env::consts::OS {
            "linux" => Platform::Linux,
            "macos" => Platform::MacOS,
            "windows" => Platform::Windows,
            _ => panic!("Unsupported platform: {}", std::env::consts::OS),
        }
    }
    
    pub fn supports_virtualization_framework(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            // Check macOS version >= 10.15
            // This requires FFI to NSProcessInfo or parsing `sw_vers`
            check_macos_version() >= (10, 15)
        }
        #[cfg(not(target_os = "macos"))]
        false
    }
    
    pub fn supports_wsl2(&self) -> bool {
        #[cfg(target_os = "windows")]
        {
            // Execute `wsl --status` and check exit code
            std::process::Command::new("wsl")
                .arg("--status")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
        #[cfg(not(target_os = "windows"))]
        false
    }
}
```


## Error Handling

### Error Categories

```rust
#[derive(Debug, thiserror::Error)]
pub enum LauncherError {
    #[error("Platform not supported: {0}")]
    UnsupportedPlatform(String),
    
    #[error("VM failed to start: {0}")]
    VmStartFailed(String),
    
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    
    #[error("Auth token validation failed")]
    AuthFailed,
    
    #[error("Resource not found: {0}")]
    ResourceNotFound(String),
    
    #[error("Snapshot corrupted: {0}")]
    SnapshotCorrupted(String),
    
    #[error("Health check failed: {0}")]
    HealthCheckFailed(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
```

### Error Recovery Strategies

| Error | Recovery Strategy | User Impact |
|-------|------------------|-------------|
| `VmStartFailed` | Retry up to 3 times with 2s delay. If all fail, show error dialog with "Retry" button | Blocks first-run setup |
| `ConnectionFailed` | Retry up to 5 times with 1s delay. If all fail, attempt VM restart | Blocks all operations |
| `AuthFailed` | Regenerate token and retry connection once. If fails, show error | Blocks TCP connections |
| `SnapshotCorrupted` | Delete snapshot and perform full boot. Create new snapshot | Adds 10-15s to startup |
| `HealthCheckFailed` | Attempt VM restart. If fails 3 times, show error dialog | May interrupt active operations |
| `ResourceNotFound` | Show error dialog identifying missing file. Suggest reinstall | Blocks startup |

### Graceful Degradation

**Scenario: Virtualization.framework unavailable on macOS**
- Fallback: Use QEMU launcher
- User notification: "Using compatibility mode (slower startup)"
- Performance impact: +5-10s startup time

**Scenario: WSL2 unavailable on Windows**
- Fallback: Use QEMU launcher
- User notification: "Using compatibility mode. For better performance, enable WSL2 in Windows Features"
- Performance impact: +5-10s startup time

**Scenario: Network unavailable during first boot**
- Error: Cannot download container images
- Recovery: Cache Alpine image in bundle, defer container pulls until network available
- User notification: "Some features require internet connection"

### Logging Strategy

**Log Levels:**
- `ERROR`: VM start failures, connection failures, auth failures
- `WARN`: Fallback to QEMU, snapshot corruption, health check failures
- `INFO`: VM started, connection established, snapshot created
- `DEBUG`: Platform detection, launcher selection, socket operations
- `TRACE`: Raw socket messages, FFI calls

**Log Locations:**
- Linux: `~/.local/share/mowisai/logs/launcher.log`
- macOS: `~/Library/Logs/MowisAI/launcher.log`
- Windows: `%APPDATA%\MowisAI\logs\launcher.log`

**Log Rotation:**
- Max size: 10 MB per file
- Keep last 5 files
- Compress old logs with gzip


## Testing Strategy

### Why Property-Based Testing Does NOT Apply

This feature is primarily **infrastructure as code** and **platform integration work**:

1. **VM lifecycle management** — Starting/stopping VMs, process spawning, FFI calls to platform APIs
2. **IPC configuration** — Socket creation, named pipe setup, TCP port forwarding
3. **File system operations** — Image extraction, snapshot management, permission setting
4. **External dependencies** — Virtualization.framework, WSL2, QEMU binaries

**None of these have universal properties that hold across all inputs.** They are one-shot operations with specific success/failure conditions, not pure functions with round-trip properties or invariants.

**Appropriate testing strategies:**
- **Unit tests** for specific components (token generation, platform detection, connection parsing)
- **Integration tests** for end-to-end launcher workflows (mock VM, real socket)
- **Manual testing** on each target platform (macOS, Windows, Linux)
- **Snapshot tests** for configuration generation (QEMU args, WSL2 import commands)

### Unit Tests

**Test Coverage:**

1. **Platform Detection** (`tests/platform_detection.rs`)
   - Test `Platform::current()` returns correct value on each OS
   - Test `supports_virtualization_framework()` on macOS 10.15+ vs 10.14
   - Test `supports_wsl2()` with mocked `wsl --status` output

2. **Auth Token Generation** (`tests/auth_token.rs`)
   - Test token is 32 bytes (256 bits) when decoded from base64
   - Test token file has correct permissions (0600 on Unix)
   - Test token validation succeeds with correct token
   - Test token validation fails with incorrect token
   - Test token validation fails when file missing

3. **Connection Info Parsing** (`tests/connection_info.rs`)
   - Test `ConnectionInfo::UnixSocket` serialization round-trip
   - Test `ConnectionInfo::TcpWithToken` serialization round-trip
   - Test `ConnectionInfo::NamedPipe` serialization round-trip (Windows only)

4. **Socket Message Framing** (`tests/message_framing.rs`)
   - Test newline-delimited JSON parsing with multiple messages
   - Test handling of incomplete messages (no trailing newline)
   - Test handling of malformed JSON
   - Test large message handling (>1MB payload)

**Example Unit Test:**

```rust
#[test]
fn test_auth_token_generation() {
    let token = generate_auth_token();
    
    // Decode from base64
    let bytes = base64::decode(&token).expect("Invalid base64");
    
    // Verify length
    assert_eq!(bytes.len(), 32, "Token must be 256 bits (32 bytes)");
    
    // Verify randomness (no all-zeros)
    assert!(bytes.iter().any(|&b| b != 0), "Token must not be all zeros");
}

#[test]
fn test_token_validation() {
    let token = generate_auth_token();
    let token_file = temp_dir().join("test-auth-token");
    
    // Write token
    std::fs::write(&token_file, &token).unwrap();
    std::fs::set_permissions(&token_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    
    // Validate correct token
    assert!(validate_token_from_file(&token, &token_file).unwrap());
    
    // Validate incorrect token
    assert!(!validate_token_from_file("wrong-token", &token_file).unwrap());
}
```

### Integration Tests

**Test Coverage:**

1. **Linux Direct Launcher** (`tests/integration/linux_launcher.rs`)
   - Test spawning agentd process
   - Test Unix socket creation and permissions
   - Test connection establishment
   - Test graceful shutdown

2. **QEMU Launcher** (`tests/integration/qemu_launcher.rs`)
   - Test QEMU process spawning (with mock Alpine image)
   - Test TCP port forwarding
   - Test auth token flow
   - Test VM health check

3. **Socket Bridge** (`tests/integration/socket_bridge.rs`)
   - Test Unix socket connection
   - Test TCP connection with auth
   - Test message send/receive
   - Test connection retry logic
   - Test reconnection after disconnect

4. **End-to-End Workflow** (`tests/integration/e2e.rs`)
   - Test full startup sequence: launcher selection → VM start → connection → first request
   - Test graceful shutdown: stop command → VM cleanup → resource release
   - Test error recovery: VM crash → reconnection → resume operations

**Example Integration Test:**

```rust
#[tokio::test]
async fn test_linux_launcher_full_workflow() {
    let launcher = LinuxDirectLauncher::new(LauncherConfig::default());
    
    // Start VM
    let conn_info = launcher.start().await.expect("Failed to start");
    
    // Verify socket exists
    if let ConnectionInfo::UnixSocket { path } = &conn_info {
        assert!(path.exists(), "Socket file should exist");
        
        // Verify permissions
        let metadata = std::fs::metadata(path).unwrap();
        let permissions = metadata.permissions();
        assert_eq!(permissions.mode() & 0o777, 0o600, "Socket should have 0600 permissions");
    } else {
        panic!("Expected UnixSocket connection info");
    }
    
    // Establish connection
    let mut connection = UnixSocketConnection::new(conn_info);
    connection.connect().await.expect("Failed to connect");
    
    // Send test request
    let request = SocketRequest {
        request_type: "ping".to_string(),
        ..Default::default()
    };
    connection.send_request(request).await.expect("Failed to send");
    
    // Receive response
    let response = connection.recv_response().await.expect("Failed to receive");
    assert_eq!(response.status, "ok");
    
    // Cleanup
    launcher.stop().await.expect("Failed to stop");
}
```

### Manual Testing Plan

**Platform: macOS (Intel and Apple Silicon)**

| Test Case | Steps | Expected Result |
|-----------|-------|-----------------|
| First run | 1. Fresh install<br/>2. Launch app | VM boots in 15-20s, GUI shows progress, then landing screen |
| Subsequent run | 1. Quit app<br/>2. Relaunch | VM boots in <5s using snapshot |
| Virtualization.framework | 1. Check macOS version >= 10.15<br/>2. Launch app | Uses Virtualization.framework, no QEMU |
| QEMU fallback | 1. Simulate macOS 10.14<br/>2. Launch app | Falls back to QEMU, shows "compatibility mode" message |
| Socket communication | 1. Send orchestration request<br/>2. Monitor logs | Messages flow through vsock, no errors |
| Graceful shutdown | 1. Quit app<br/>2. Check Activity Monitor | VM process terminated, no orphans |

**Platform: Windows 10/11**

| Test Case | Steps | Expected Result |
|-----------|-------|-----------------|
| First run with WSL2 | 1. Fresh install on Windows with WSL2<br/>2. Launch app | Imports WSL2 distro, starts agentd, GUI shows progress |
| Subsequent run | 1. Quit app<br/>2. Relaunch | Reuses existing WSL2 distro, <5s startup |
| Named pipe | 1. Send orchestration request<br/>2. Check pipe with `pipelist` | Named pipe exists, secured with ACL |
| QEMU fallback | 1. Disable WSL2<br/>2. Launch app | Falls back to QEMU, uses TCP+token |
| Auth token | 1. Check `%USERPROFILE%\.mowisai\auth-token`<br/>2. Verify permissions | File exists, only current user can read |
| Graceful shutdown | 1. Quit app<br/>2. Check Task Manager | WSL2 distro stopped, no orphan processes |

**Platform: Linux**

| Test Case | Steps | Expected Result |
|-----------|-------|-----------------|
| Direct execution | 1. Launch app<br/>2. Check process list | agentd runs as child process, no VM |
| Unix socket | 1. Check `$XDG_RUNTIME_DIR/agentd.sock`<br/>2. Verify permissions | Socket exists, 0600 permissions |
| Socket communication | 1. Send orchestration request<br/>2. Monitor logs | Messages flow through Unix socket, no errors |
| Graceful shutdown | 1. Quit app<br/>2. Check process list | agentd terminated, socket removed |

### CI/CD Testing

**GitHub Actions Workflow:**

```yaml
name: Cross-Platform Build and Test

on: [push, pull_request]

jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    
    steps:
      - uses: actions/checkout@v3
      
      - name: Install Rust
        uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: stable
      
      - name: Install musl target (Linux)
        if: matrix.os == 'ubuntu-latest'
        run: rustup target add x86_64-unknown-linux-musl
      
      - name: Install musl target (macOS)
        if: matrix.os == 'macos-latest'
        run: |
          rustup target add x86_64-unknown-linux-musl
          rustup target add aarch64-unknown-linux-musl
      
      - name: Build workspace
        run: cargo build --workspace --release
      
      - name: Run unit tests
        run: cargo test --workspace --lib
      
      - name: Run integration tests (Linux only)
        if: matrix.os == 'ubuntu-latest'
        run: cargo test --workspace --test '*'
      
      - name: Run clippy
        run: cargo clippy --workspace -- -D warnings
      
      - name: Build static agentd (Linux)
        if: matrix.os == 'ubuntu-latest'
        run: cargo build --package agentd --release --target x86_64-unknown-linux-musl
      
      - name: Build static agentd (macOS)
        if: matrix.os == 'macos-latest'
        run: |
          cargo build --package agentd --release --target x86_64-unknown-linux-musl
          cargo build --package agentd --release --target aarch64-unknown-linux-musl
      
      - name: Verify binary is static (Linux)
        if: matrix.os == 'ubuntu-latest'
        run: |
          ldd target/x86_64-unknown-linux-musl/release/agentd && exit 1 || echo "Binary is static"
      
      - name: Upload artifacts
        uses: actions/upload-artifact@v3
        with:
          name: mowisai-${{ matrix.os }}
          path: |
            target/release/mowisai*
            target/*/release/agentd
```

### Performance Testing

**Metrics to Track:**

1. **VM Boot Time**
   - First boot (no snapshot): Target <20s
   - Subsequent boot (with snapshot): Target <5s
   - Measure: Time from `launcher.start()` to `connection.connect()` success

2. **Connection Latency**
   - Unix socket: Target <1ms
   - Vsock: Target <5ms
   - Named pipe: Target <10ms
   - TCP loopback: Target <5ms
   - Measure: Round-trip time for ping request

3. **Message Throughput**
   - Target: >1000 messages/second
   - Measure: Time to send and receive 10,000 small JSON messages

4. **Memory Overhead**
   - VM memory: 512 MB (configurable)
   - GUI memory: <100 MB
   - Total: <650 MB
   - Measure: RSS after startup and after 1 hour of operation

**Performance Test Script:**

```rust
#[tokio::test]
async fn bench_connection_latency() {
    let launcher = select_launcher();
    let conn_info = launcher.start().await.unwrap();
    let mut connection = create_connection(conn_info).await.unwrap();
    connection.connect().await.unwrap();
    
    let start = Instant::now();
    for _ in 0..1000 {
        let request = SocketRequest {
            request_type: "ping".to_string(),
            ..Default::default()
        };
        connection.send_request(request).await.unwrap();
        let _ = connection.recv_response().await.unwrap();
    }
    let elapsed = start.elapsed();
    
    let avg_latency = elapsed / 1000;
    println!("Average latency: {:?}", avg_latency);
    assert!(avg_latency < Duration::from_millis(10), "Latency too high");
}
```


## Alpine Image Build Process

### Image Requirements

**Contents:**
- Alpine Linux 3.19 base system
- Static `agentd` binary (musl-linked)
- `skopeo` for container image operations
- Init script to start `agentd` on boot
- Network configuration (DHCP)
- SSH server (for QEMU debugging, optional)

**Size Constraints:**
- Uncompressed: ~200 MB
- Compressed (qcow2 or tar.gz): <100 MB
- Target: 50-80 MB compressed

### Build Script

**Location:** `scripts/build-alpine-image.sh`

```bash
#!/bin/bash
set -euo pipefail

ARCH="${1:-x86_64}"  # x86_64 or aarch64
OUTPUT_DIR="build/alpine-images"
IMAGE_NAME="alpine-mowisai-${ARCH}.img"

echo "Building Alpine image for ${ARCH}..."

# 1. Create temporary directory
WORK_DIR=$(mktemp -d)
trap "rm -rf ${WORK_DIR}" EXIT

# 2. Download Alpine mini root filesystem
ALPINE_VERSION="3.19"
ALPINE_MIRROR="https://dl-cdn.alpinelinux.org/alpine"
ROOTFS_URL="${ALPINE_MIRROR}/v${ALPINE_VERSION}/releases/${ARCH}/alpine-minirootfs-${ALPINE_VERSION}.0-${ARCH}.tar.gz"

echo "Downloading Alpine rootfs..."
curl -L "${ROOTFS_URL}" | tar -xz -C "${WORK_DIR}"

# 3. Copy static agentd binary
echo "Copying agentd binary..."
AGENTD_BINARY="target/${ARCH}-unknown-linux-musl/release/agentd"
if [ ! -f "${AGENTD_BINARY}" ]; then
    echo "Error: agentd binary not found at ${AGENTD_BINARY}"
    echo "Run: cargo build --release --target ${ARCH}-unknown-linux-musl --package agentd"
    exit 1
fi
cp "${AGENTD_BINARY}" "${WORK_DIR}/usr/local/bin/agentd"
chmod +x "${WORK_DIR}/usr/local/bin/agentd"

# 4. Install skopeo (from Alpine package)
echo "Installing skopeo..."
cat > "${WORK_DIR}/etc/apk/repositories" <<EOF
${ALPINE_MIRROR}/v${ALPINE_VERSION}/main
${ALPINE_MIRROR}/v${ALPINE_VERSION}/community
EOF

# Use chroot to install packages
sudo chroot "${WORK_DIR}" /bin/sh -c "apk add --no-cache skopeo ca-certificates"

# 5. Create init script
echo "Creating init script..."
cat > "${WORK_DIR}/etc/init.d/agentd" <<'EOF'
#!/sbin/openrc-run

name="agentd"
command="/usr/local/bin/agentd"
command_args="socket --path /tmp/agentd.sock"
command_background=true
pidfile="/run/agentd.pid"

depend() {
    need net
    after firewall
}

start_pre() {
    # Generate auth token if required
    if [ -n "${AGENTD_AUTH_REQUIRED}" ]; then
        TOKEN=$(head -c 32 /dev/urandom | base64)
        mkdir -p /root/.mowisai
        echo "${TOKEN}" > /root/.mowisai/auth-token
        chmod 600 /root/.mowisai/auth-token
        
        # Write to shared volume if mounted
        if [ -d /mnt/host ]; then
            echo "${TOKEN}" > /mnt/host/auth-token
        fi
    fi
}
EOF

chmod +x "${WORK_DIR}/etc/init.d/agentd"

# 6. Enable agentd service
sudo chroot "${WORK_DIR}" /bin/sh -c "rc-update add agentd default"

# 7. Configure networking
cat > "${WORK_DIR}/etc/network/interfaces" <<EOF
auto lo
iface lo inet loopback

auto eth0
iface eth0 inet dhcp
EOF

# 8. Create disk image
echo "Creating disk image..."
mkdir -p "${OUTPUT_DIR}"
IMAGE_PATH="${OUTPUT_DIR}/${IMAGE_NAME}"

# Create 1GB sparse file
dd if=/dev/zero of="${IMAGE_PATH}" bs=1M count=0 seek=1024

# Format as ext4
mkfs.ext4 -F "${IMAGE_PATH}"

# Mount and copy files
MOUNT_DIR=$(mktemp -d)
trap "sudo umount ${MOUNT_DIR} 2>/dev/null || true; rm -rf ${MOUNT_DIR}" EXIT
sudo mount -o loop "${IMAGE_PATH}" "${MOUNT_DIR}"
sudo cp -a "${WORK_DIR}"/* "${MOUNT_DIR}/"
sudo umount "${MOUNT_DIR}"

# 9. Convert to qcow2 for better compression
echo "Converting to qcow2..."
qemu-img convert -f raw -O qcow2 -c "${IMAGE_PATH}" "${IMAGE_PATH}.qcow2"
mv "${IMAGE_PATH}.qcow2" "${IMAGE_PATH}"

# 10. Verify image
echo "Verifying image..."
qemu-img info "${IMAGE_PATH}"

echo "Alpine image built successfully: ${IMAGE_PATH}"
echo "Size: $(du -h ${IMAGE_PATH} | cut -f1)"
```

### WSL2 Distribution Build

**Location:** `scripts/build-wsl2-distro.sh`

```bash
#!/bin/bash
set -euo pipefail

OUTPUT_DIR="build/wsl2-distro"
DISTRO_NAME="alpine-mowisai-wsl2.tar.gz"

echo "Building WSL2 distribution..."

# 1. Use same Alpine rootfs as VM image
WORK_DIR=$(mktemp -d)
trap "rm -rf ${WORK_DIR}" EXIT

# ... (same steps 2-7 as VM image build)

# 8. Create tar.gz for WSL2 import
echo "Creating WSL2 distribution archive..."
mkdir -p "${OUTPUT_DIR}"
sudo tar -czf "${OUTPUT_DIR}/${DISTRO_NAME}" -C "${WORK_DIR}" .

echo "WSL2 distribution built successfully: ${OUTPUT_DIR}/${DISTRO_NAME}"
echo "Size: $(du -h ${OUTPUT_DIR}/${DISTRO_NAME} | cut -f1)"
```

### Image Integrity Verification

**Checksum Generation:**

```bash
# Generate SHA-256 checksums
cd build/alpine-images
sha256sum alpine-mowisai-x86_64.img > checksums.txt
sha256sum alpine-mowisai-aarch64.img >> checksums.txt

cd ../wsl2-distro
sha256sum alpine-mowisai-wsl2.tar.gz >> ../alpine-images/checksums.txt
```

**Checksum Verification (in app):**

```rust
fn verify_image_integrity(image_path: &Path, expected_checksum: &str) -> Result<()> {
    use sha2::{Sha256, Digest};
    
    let mut file = std::fs::File::open(image_path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize();
    let hash_hex = format!("{:x}", hash);
    
    if hash_hex != expected_checksum {
        return Err(anyhow!(
            "Image integrity check failed: expected {}, got {}",
            expected_checksum,
            hash_hex
        ));
    }
    
    Ok(())
}
```

### Embedded Checksums

**Location:** `mowis-gui/src/resources.rs`

```rust
pub const ALPINE_X86_64_CHECKSUM: &str = 
    "a1b2c3d4e5f6...";  // Generated during build

pub const ALPINE_AARCH64_CHECKSUM: &str = 
    "f6e5d4c3b2a1...";  // Generated during build

pub const WSL2_DISTRO_CHECKSUM: &str = 
    "1a2b3c4d5e6f...";  // Generated during build

pub fn verify_bundled_image() -> Result<()> {
    let platform = Platform::current();
    let arch = std::env::consts::ARCH;
    
    let (image_path, expected_checksum) = match (platform, arch) {
        (Platform::MacOS, "x86_64") => (
            bundle_resource_path("alpine-x86_64.img")?,
            ALPINE_X86_64_CHECKSUM,
        ),
        (Platform::MacOS, "aarch64") => (
            bundle_resource_path("alpine-aarch64.img")?,
            ALPINE_AARCH64_CHECKSUM,
        ),
        (Platform::Windows, _) => (
            bundle_resource_path("alpine-wsl2.tar.gz")?,
            WSL2_DISTRO_CHECKSUM,
        ),
        _ => return Ok(()), // Linux doesn't use bundled image
    };
    
    verify_image_integrity(&image_path, expected_checksum)
}
```


## Build System Changes

### Cargo.toml Modifications

**agentd/Cargo.toml:**

```toml
[package]
name = "agentd"
version = "0.2.0+1"
edition = "2024"

[dependencies]
# ... existing dependencies ...

# Platform-specific dependencies
[target.'cfg(target_os = "linux")'.dependencies]
nix = "0.26"
signal-hook = "0.3"

# Remove nix and signal-hook from main dependencies
```

**mowis-gui/Cargo.toml:**

```toml
[package]
name = "mowis-gui"
version = "0.1.0"
edition = "2024"

[dependencies]
# ... existing dependencies ...
rand = "0.9"
base64 = "0.21"
sha2 = "0.10"

# Platform-specific dependencies
[target.'cfg(unix)'.dependencies]
# Unix-specific socket handling (already using tokio)

[target.'cfg(windows)'.dependencies]
# Windows-specific named pipe handling (already using tokio)

[target.'cfg(target_os = "macos")'.dependencies]
# macOS-specific Virtualization.framework FFI
# Will be added when implementing macOS launcher
```

### Conditional Compilation Guards

**agentd/src/lib.rs:**

```rust
// Platform-specific modules
#[cfg(target_os = "linux")]
pub mod sandbox;

#[cfg(target_os = "linux")]
pub mod vm_backend;

// Stub implementations for non-Linux platforms
#[cfg(not(target_os = "linux"))]
pub mod sandbox {
    // Empty module - not used on non-Linux
}

#[cfg(not(target_os = "linux"))]
pub mod vm_backend {
    // Empty module - not used on non-Linux
}
```

**agentd/src/socket_server.rs:**

```rust
use std::io::{BufRead, BufReader, Write};

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(target_os = "linux")]
use nix::mount;
#[cfg(target_os = "linux")]
use signal_hook;

// ... rest of file with #[cfg(target_os = "linux")] guards around Linux-specific code
```

### Static Linking Configuration

**Build script for musl targets:**

```bash
#!/bin/bash
# scripts/build-static-agentd.sh

set -euo pipefail

TARGET="${1:-x86_64-unknown-linux-musl}"

echo "Building static agentd for ${TARGET}..."

# Install musl target if not present
rustup target add "${TARGET}"

# Set flags for static linking
export RUSTFLAGS="-C target-feature=+crt-static"

# Build
cargo build \
    --package agentd \
    --release \
    --target "${TARGET}"

# Verify binary is static
BINARY="target/${TARGET}/release/agentd"
echo "Verifying ${BINARY} is statically linked..."

if ldd "${BINARY}" 2>&1 | grep -q "not a dynamic executable"; then
    echo "✓ Binary is statically linked"
else
    echo "✗ Binary is NOT statically linked:"
    ldd "${BINARY}"
    exit 1
fi

echo "Static agentd built successfully: ${BINARY}"
```

### Cross-Compilation Setup

**For macOS → Linux (Apple Silicon → aarch64-musl):**

```bash
# Install cross-compilation toolchain
brew install filosottile/musl-cross/musl-cross

# Add to ~/.cargo/config.toml
[target.aarch64-unknown-linux-musl]
linker = "aarch64-linux-musl-gcc"

[target.x86_64-unknown-linux-musl]
linker = "x86_64-linux-musl-gcc"
```

**For Windows → Linux (via WSL2):**

```powershell
# Build inside WSL2 Ubuntu
wsl --exec bash -c "cd /mnt/c/path/to/project && ./scripts/build-static-agentd.sh"
```

### Bundle Packaging

**macOS App Bundle Structure:**

```
MowisAI.app/
├── Contents/
│   ├── Info.plist
│   ├── MacOS/
│   │   ├── mowisai              # GUI binary
│   │   └── vm_launcher          # Swift shim for Virtualization.framework
│   ├── Resources/
│   │   ├── alpine-x86_64.img    # Intel image
│   │   ├── alpine-aarch64.img   # Apple Silicon image
│   │   ├── agentd-x86_64        # Static binary (Intel)
│   │   ├── agentd-aarch64       # Static binary (Apple Silicon)
│   │   ├── qemu-system-x86_64   # Fallback (Intel)
│   │   ├── qemu-system-aarch64  # Fallback (Apple Silicon)
│   │   └── checksums.txt
│   └── Frameworks/
│       └── (none - fully static)
```

**Windows Installer Structure:**

```
MowisAI-Setup.exe (NSIS installer)
├── mowisai.exe                  # GUI binary
├── agentd.linux                 # Static binary
├── alpine-wsl2.tar.gz           # WSL2 distribution
├── qemu-system-x86_64.exe       # Fallback
├── checksums.txt
└── uninstall.exe
```

**Linux Tarball Structure:**

```
mowisai-linux-x86_64.tar.gz
├── mowisai                      # GUI binary (includes embedded agentd)
├── agentd                       # Standalone agentd binary
└── README.txt
```

### Release Workflow

**GitHub Actions: `.github/workflows/release.yml`**

```yaml
name: Release Build

on:
  push:
    tags:
      - 'v*'

jobs:
  build-alpine-images:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Install dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y qemu-utils
      
      - name: Install Rust
        uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: stable
      
      - name: Install musl targets
        run: |
          rustup target add x86_64-unknown-linux-musl
          rustup target add aarch64-unknown-linux-musl
      
      - name: Build static agentd (x86_64)
        run: ./scripts/build-static-agentd.sh x86_64-unknown-linux-musl
      
      - name: Build static agentd (aarch64)
        run: ./scripts/build-static-agentd.sh aarch64-unknown-linux-musl
      
      - name: Build Alpine images
        run: |
          ./scripts/build-alpine-image.sh x86_64
          ./scripts/build-alpine-image.sh aarch64
      
      - name: Build WSL2 distribution
        run: ./scripts/build-wsl2-distro.sh
      
      - name: Generate checksums
        run: |
          cd build/alpine-images
          sha256sum *.img *.tar.gz > checksums.txt
      
      - name: Upload artifacts
        uses: actions/upload-artifact@v3
        with:
          name: alpine-images
          path: build/alpine-images/*

  build-macos:
    needs: build-alpine-images
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Download Alpine images
        uses: actions/download-artifact@v3
        with:
          name: alpine-images
          path: resources/
      
      - name: Install Rust
        uses: actions-rust-lang/setup-rust-toolchain@v1
      
      - name: Build GUI
        run: cargo build --package mowis-gui --release
      
      - name: Build Swift shim
        run: |
          swiftc -o target/release/vm_launcher \
            mowis-gui/src/launchers/macos/vm_launcher.swift \
            -framework Virtualization
      
      - name: Create app bundle
        run: ./scripts/package-macos.sh
      
      - name: Sign app bundle
        env:
          MACOS_CERTIFICATE: ${{ secrets.MACOS_CERTIFICATE }}
          MACOS_CERTIFICATE_PWD: ${{ secrets.MACOS_CERTIFICATE_PWD }}
        run: ./scripts/sign-macos.sh
      
      - name: Create DMG
        run: ./scripts/create-dmg.sh
      
      - name: Upload artifact
        uses: actions/upload-artifact@v3
        with:
          name: MowisAI-macOS.dmg
          path: build/MowisAI-macOS.dmg

  build-windows:
    needs: build-alpine-images
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Download Alpine images
        uses: actions/download-artifact@v3
        with:
          name: alpine-images
          path: resources/
      
      - name: Install Rust
        uses: actions-rust-lang/setup-rust-toolchain@v1
      
      - name: Build GUI
        run: cargo build --package mowis-gui --release
      
      - name: Download QEMU
        run: |
          Invoke-WebRequest -Uri "https://qemu.weilnetz.de/w64/qemu-w64-setup-latest.exe" -OutFile qemu-setup.exe
          # Extract qemu-system-x86_64.exe from installer
      
      - name: Create installer
        run: |
          makensis /DVERSION=${{ github.ref_name }} scripts/windows-installer.nsi
      
      - name: Upload artifact
        uses: actions/upload-artifact@v3
        with:
          name: MowisAI-Windows.exe
          path: build/MowisAI-Windows.exe

  build-linux:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Install Rust
        uses: actions-rust-lang/setup-rust-toolchain@v1
      
      - name: Build GUI and agentd
        run: cargo build --workspace --release
      
      - name: Create tarball
        run: ./scripts/package-linux.sh
      
      - name: Upload artifact
        uses: actions/upload-artifact@v3
        with:
          name: mowisai-linux-x86_64.tar.gz
          path: build/mowisai-linux-x86_64.tar.gz

  create-release:
    needs: [build-macos, build-windows, build-linux]
    runs-on: ubuntu-latest
    steps:
      - name: Download all artifacts
        uses: actions/download-artifact@v3
      
      - name: Create GitHub Release
        uses: softprops/action-gh-release@v1
        with:
          files: |
            MowisAI-macOS.dmg/MowisAI-macOS.dmg
            MowisAI-Windows.exe/MowisAI-Windows.exe
            mowisai-linux-x86_64.tar.gz/mowisai-linux-x86_64.tar.gz
          draft: true
```


## Implementation Phases

### Phase 1: Foundation (Week 1-2)

**Goal:** Establish cross-platform compilation and basic abstractions

**Tasks:**
1. Add conditional compilation guards to `agentd`
   - Guard Linux-specific imports (`nix`, `signal-hook`)
   - Guard Linux-specific modules (`sandbox`, `vm_backend`)
   - Ensure `agentd` compiles on macOS and Windows (with stubs)

2. Add conditional compilation guards to `mowis-gui`
   - Guard Unix-specific socket code
   - Add platform detection module
   - Ensure `mowis-gui` compiles on all three platforms

3. Define core traits and types
   - `VmLauncher` trait
   - `DaemonConnection` trait
   - `ConnectionInfo` enum
   - `LauncherConfig` struct

4. Implement platform detection
   - `Platform::current()`
   - `supports_virtualization_framework()`
   - `supports_wsl2()`

5. Set up CI for cross-platform builds
   - GitHub Actions matrix for Linux/macOS/Windows
   - Verify compilation on all platforms
   - Run existing tests on Linux

**Deliverables:**
- All crates compile on Linux, macOS, Windows
- CI passes on all platforms
- Core traits defined and documented

### Phase 2: Linux Direct Launcher (Week 2-3)

**Goal:** Refactor existing Linux code into new launcher architecture

**Tasks:**
1. Implement `LinuxDirectLauncher`
   - Extract existing `ensure_daemon()` logic from `backend.rs`
   - Implement `VmLauncher` trait
   - Add socket path resolution (`$XDG_RUNTIME_DIR`)
   - Add socket permission setting (0600)

2. Implement `UnixSocketConnection`
   - Extract existing socket code from `backend.rs`
   - Implement `DaemonConnection` trait
   - Add connection retry logic

3. Refactor `Backend` to use new abstractions
   - Replace direct socket code with `DaemonConnection`
   - Add launcher selection (currently only Linux)
   - Add health check polling

4. Add unit tests
   - Platform detection tests
   - Socket path resolution tests
   - Connection retry tests

5. Add integration tests
   - Full Linux launcher workflow
   - Socket communication tests
   - Graceful shutdown tests

**Deliverables:**
- Linux functionality unchanged (regression-free)
- New launcher architecture in place
- Tests passing on Linux

### Phase 3: Static agentd Build (Week 3-4)

**Goal:** Build fully static agentd binaries for embedding in Alpine images

**Tasks:**
1. Set up musl cross-compilation
   - Add musl targets to CI
   - Configure linker for static linking
   - Add build script `build-static-agentd.sh`

2. Build static binaries
   - x86_64-unknown-linux-musl
   - aarch64-unknown-linux-musl (for Apple Silicon)

3. Verify static linking
   - Add `ldd` check to build script
   - Verify no dynamic dependencies

4. Test static binaries
   - Run on minimal Alpine container
   - Verify all features work (socket server, sandbox, tools)

**Deliverables:**
- Static agentd binaries for x86_64 and aarch64
- Build script integrated into CI
- Verification tests passing

### Phase 4: Alpine Image Build (Week 4-5)

**Goal:** Create minimal Alpine Linux images with agentd pre-installed

**Tasks:**
1. Create Alpine image build script
   - Download Alpine mini rootfs
   - Install skopeo and dependencies
   - Copy static agentd binary
   - Create init script
   - Configure networking

2. Build VM images (qcow2)
   - x86_64 image for Intel Macs and QEMU
   - aarch64 image for Apple Silicon

3. Build WSL2 distribution (tar.gz)
   - Same rootfs as VM images
   - Optimized for WSL2 import

4. Add integrity verification
   - Generate SHA-256 checksums
   - Embed checksums in `mowis-gui`
   - Add verification function

5. Test images
   - Boot in QEMU
   - Verify agentd starts automatically
   - Verify network connectivity
   - Verify socket creation

**Deliverables:**
- Alpine images for x86_64 and aarch64
- WSL2 distribution tarball
- Build scripts integrated into CI
- Integrity verification working

### Phase 5: QEMU Launcher (Week 5-6)

**Goal:** Implement fallback launcher using bundled QEMU

**Tasks:**
1. Bundle QEMU binaries
   - Download static QEMU builds
   - Add to app bundle resources
   - Verify size (<25 MB per binary)

2. Implement `QEMULauncher`
   - Implement `VmLauncher` trait
   - Add QEMU process spawning
   - Add TCP port forwarding configuration
   - Add VM health monitoring

3. Implement auth token system
   - Add token generation function
   - Add token file writing (with permissions)
   - Add token validation in `agentd`

4. Implement `TcpTokenConnection`
   - Implement `DaemonConnection` trait
   - Add auth handshake
   - Add message framing

5. Add tests
   - Token generation tests
   - Token validation tests
   - QEMU launcher integration tests
   - TCP connection tests

**Deliverables:**
- QEMU launcher working on all platforms
- Auth token system implemented and tested
- TCP connection working with authentication

### Phase 6: macOS Launcher (Week 6-8)

**Goal:** Implement native macOS launcher using Virtualization.framework

**Tasks:**
1. Create Swift shim for Virtualization.framework
   - Define C-compatible API
   - Implement VM configuration
   - Implement virtio-vsock setup
   - Implement snapshot management

2. Implement `MacOSLauncher`
   - Add FFI bindings to Swift shim
   - Implement `VmLauncher` trait
   - Add snapshot-based fast boot
   - Add vsock socket bridging

3. Add macOS-specific build steps
   - Compile Swift shim
   - Bundle in app bundle
   - Add code signing

4. Test on macOS
   - Test on Intel Mac (x86_64 image)
   - Test on Apple Silicon (aarch64 image)
   - Test first boot (no snapshot)
   - Test subsequent boots (with snapshot)
   - Verify <5s startup with snapshot

**Deliverables:**
- macOS launcher working on Intel and Apple Silicon
- Virtualization.framework integration complete
- Fast boot with snapshots working
- macOS app bundle packaging working

### Phase 7: Windows WSL2 Launcher (Week 8-10)

**Goal:** Implement Windows launcher using WSL2

**Tasks:**
1. Implement `WSL2Launcher`
   - Add WSL2 detection
   - Add distribution import
   - Add agentd startup in WSL2
   - Implement `VmLauncher` trait

2. Implement named pipe bridge
   - Create Windows named pipe server
   - Connect to WSL2 Unix socket
   - Forward traffic bidirectionally
   - Add ACL security

3. Implement `NamedPipeConnection`
   - Implement `DaemonConnection` trait
   - Add Windows-specific error handling

4. Add Windows-specific build steps
   - Create NSIS installer script
   - Bundle resources
   - Add uninstaller

5. Test on Windows
   - Test on Windows 10 2004+ with WSL2
   - Test distribution import
   - Test named pipe connection
   - Test fallback to QEMU when WSL2 unavailable

**Deliverables:**
- WSL2 launcher working on Windows 10 2004+
- Named pipe bridge working
- Windows installer working
- Fallback to QEMU working

### Phase 8: First-Run UX (Week 10-11)

**Goal:** Polish the first-run experience with progress feedback

**Tasks:**
1. Add progress events
   - `DaemonStarting` event
   - `DaemonProgress { message, percent }` event
   - `DaemonStarted` event
   - `DaemonFailed { error }` event

2. Update GUI to show progress
   - Add progress indicator to landing view
   - Show progress messages during first boot
   - Add "Retry" button on failure
   - Ensure render loop never blocks

3. Add error recovery UI
   - Show detailed error messages
   - Suggest fixes (e.g., "Enable WSL2 in Windows Features")
   - Add "View Logs" button
   - Add "Report Issue" button

4. Add graceful degradation messages
   - "Using compatibility mode" for QEMU fallback
   - "First-time setup may take 15-20 seconds"
   - "Subsequent launches will be faster"

**Deliverables:**
- First-run progress feedback working
- Error messages clear and actionable
- Graceful degradation messages shown
- Non-blocking render loop verified

### Phase 9: Testing and Polish (Week 11-12)

**Goal:** Comprehensive testing and bug fixes

**Tasks:**
1. Manual testing on all platforms
   - Execute full manual test plan
   - Document any issues found
   - Fix critical bugs

2. Performance testing
   - Measure boot times
   - Measure connection latency
   - Measure memory usage
   - Optimize if needed

3. Security audit
   - Verify socket permissions
   - Verify auth token security
   - Verify ACL configuration
   - Test privilege escalation scenarios

4. Documentation
   - Update README with platform support
   - Add troubleshooting guide
   - Document build process
   - Document release process

**Deliverables:**
- All manual tests passing
- Performance targets met
- Security audit complete
- Documentation updated

### Phase 10: Release (Week 12)

**Goal:** Ship cross-platform support to users

**Tasks:**
1. Create release builds
   - Run release workflow
   - Verify all artifacts
   - Test installers on clean machines

2. Create release notes
   - List new features
   - List known issues
   - Provide upgrade instructions

3. Publish release
   - Create GitHub release
   - Upload artifacts
   - Announce on social media

**Deliverables:**
- Release published
- Installers available for download
- Release notes published

## Migration Path

### For Existing Linux Users

**No changes required.** The Linux direct launcher preserves existing behavior:
- `agentd` runs as a child process (no VM)
- Unix socket at `$XDG_RUNTIME_DIR/agentd.sock`
- Native performance (no virtualization overhead)

### For New macOS Users

**First launch:**
1. Download `MowisAI-macOS.dmg`
2. Drag `MowisAI.app` to Applications
3. Double-click to launch
4. Wait 15-20s for first-time setup (VM boot + snapshot creation)
5. Use normally

**Subsequent launches:**
- VM boots from snapshot in <5s
- No user-visible setup

### For New Windows Users

**First launch:**
1. Download `MowisAI-Windows.exe`
2. Run installer (no admin required)
3. Launch MowisAI from Start Menu
4. If WSL2 available: Wait 10-15s for distribution import
5. If WSL2 unavailable: Falls back to QEMU, shows "compatibility mode" message
6. Use normally

**Subsequent launches:**
- WSL2 distribution reused, <5s startup
- QEMU fallback: ~10s startup

## Security Considerations

### Threat Model

**Threats:**
1. **Unauthorized IPC access**: Malicious process on same machine connects to agentd socket
2. **Token theft**: Attacker reads auth token file
3. **VM escape**: Attacker breaks out of VM to host
4. **Supply chain attack**: Bundled Alpine image or QEMU binary is compromised

**Mitigations:**

| Threat | Mitigation | Effectiveness |
|--------|-----------|---------------|
| Unauthorized IPC | Unix socket with 0600 permissions, named pipe with ACL, TCP with auth token | High |
| Token theft | File permissions 0600 (Unix) or ACL (Windows), token rotated on each boot | Medium-High |
| VM escape | Use platform-native virtualization (Virtualization.framework, WSL2), keep QEMU updated | High |
| Supply chain | SHA-256 integrity checks, reproducible builds, signed releases | Medium-High |

### Security Best Practices

1. **Principle of least privilege**: VM runs with minimal permissions, no root access to host
2. **Defense in depth**: Multiple layers (file permissions + ACLs + auth tokens)
3. **Fail secure**: On auth failure, close connection immediately without processing payload
4. **Audit logging**: Log all connection attempts, auth failures, VM lifecycle events
5. **Regular updates**: Keep QEMU and Alpine packages updated for security patches


## Open Questions and Future Work

### Open Questions

1. **macOS Virtualization.framework FFI**
   - Should we use direct Rust FFI or a Swift shim?
   - **Recommendation**: Swift shim for type safety and maintainability
   - **Decision needed**: Architecture review

2. **QEMU Binary Size**
   - Can we reduce QEMU binary size below 25 MB?
   - **Options**: Custom QEMU build with minimal features, use TinyVM
   - **Decision needed**: Performance vs. size tradeoff

3. **Snapshot Storage Location**
   - Where should VM snapshots be stored?
   - **Options**: User's home directory, app data directory, temp directory
   - **Recommendation**: App data directory with size limits
   - **Decision needed**: UX review

4. **WSL2 Distribution Naming**
   - Should we use a unique name per version or a single name?
   - **Options**: `MowisAI` (single) vs. `MowisAI-v0.2.0` (versioned)
   - **Recommendation**: Single name with in-place upgrades
   - **Decision needed**: Upgrade strategy

5. **Auth Token Rotation**
   - Should tokens rotate on every boot or persist?
   - **Current design**: Rotate on every boot
   - **Alternative**: Persist and rotate weekly
   - **Decision needed**: Security vs. convenience tradeoff

### Future Enhancements

#### 1. VM Resource Configuration

**Goal:** Allow users to configure VM memory and CPU allocation

**Design:**
- Add settings UI in `mowis-gui`
- Store config in `~/.mowisai/config.toml`
- Apply on next VM boot

**Benefits:**
- Power users can allocate more resources for faster builds
- Resource-constrained machines can reduce allocation

#### 2. VM Snapshot Management

**Goal:** Allow users to manage VM snapshots (view, delete, reset)

**Design:**
- Add "Advanced" settings panel
- Show snapshot size and creation date
- Add "Reset VM" button to delete snapshot and force full boot

**Benefits:**
- Recover from corrupted snapshots
- Free disk space

#### 3. Multi-VM Support

**Goal:** Run multiple isolated VMs for different projects

**Design:**
- Add project-specific VM instances
- Each project gets its own VM with isolated filesystem
- Share base Alpine image, separate snapshots

**Benefits:**
- True project isolation
- Parallel development on multiple projects

#### 4. Remote agentd Support

**Goal:** Connect to agentd running on a remote machine

**Design:**
- Add "Remote" connection type to launcher selection
- Support SSH tunneling for socket forwarding
- Add remote host configuration UI

**Benefits:**
- Develop on powerful remote machines
- Share agentd instance across team

#### 5. Container-Based Backend (Alternative to VM)

**Goal:** Use Docker/Podman instead of VM on platforms where available

**Design:**
- Add `DockerLauncher` implementing `VmLauncher`
- Run Alpine container with agentd
- Mount host filesystem into container

**Benefits:**
- Faster startup than VM
- Lower memory overhead
- Familiar to developers

**Challenges:**
- Requires Docker/Podman installed
- Nested containerization complexity (agentd creates containers)

#### 6. Telemetry and Crash Reporting

**Goal:** Collect anonymous usage data and crash reports

**Design:**
- Add opt-in telemetry during first run
- Collect: platform, launcher type, boot time, connection latency
- Send crash dumps to Sentry or similar service

**Benefits:**
- Identify platform-specific issues
- Prioritize performance improvements
- Improve error messages based on real failures

**Privacy:**
- Fully opt-in
- No PII collected
- Open-source telemetry client

### Known Limitations

1. **Windows without WSL2**
   - Falls back to QEMU (slower startup)
   - Requires ~100 MB additional disk space for QEMU binary
   - **Workaround**: Encourage users to enable WSL2

2. **macOS < 10.15**
   - Falls back to QEMU (slower startup)
   - No virtio-vsock (uses TCP + auth token)
   - **Workaround**: Recommend upgrading to macOS 10.15+

3. **ARM64 Linux**
   - Not currently supported (no ARM64 Alpine image)
   - **Future work**: Add aarch64-unknown-linux-musl build target

4. **Nested Virtualization**
   - Running MowisAI inside a VM may not work (depends on nested virt support)
   - **Workaround**: Run on bare metal or use Docker backend

5. **Firewall/Antivirus Interference**
   - Some security software may block VM networking or socket creation
   - **Workaround**: Add MowisAI to firewall exceptions

### Performance Targets

| Metric | Target | Measured | Status |
|--------|--------|----------|--------|
| First boot time (macOS) | <20s | TBD | Not measured |
| Subsequent boot time (macOS) | <5s | TBD | Not measured |
| First boot time (Windows WSL2) | <15s | TBD | Not measured |
| Subsequent boot time (Windows WSL2) | <5s | TBD | Not measured |
| Connection latency (Unix socket) | <1ms | TBD | Not measured |
| Connection latency (vsock) | <5ms | TBD | Not measured |
| Connection latency (named pipe) | <10ms | TBD | Not measured |
| Connection latency (TCP) | <5ms | TBD | Not measured |
| Message throughput | >1000 msg/s | TBD | Not measured |
| VM memory overhead | <512 MB | TBD | Not measured |
| GUI memory overhead | <100 MB | TBD | Not measured |
| Bundle size (macOS) | <200 MB | TBD | Not measured |
| Bundle size (Windows) | <150 MB | TBD | Not measured |
| Bundle size (Linux) | <50 MB | TBD | Not measured |

### Success Criteria

**Must Have (MVP):**
- ✅ Compiles on Linux, macOS, Windows
- ✅ Linux direct launcher works (no regression)
- ✅ macOS Virtualization.framework launcher works
- ✅ Windows WSL2 launcher works
- ✅ QEMU fallback works on all platforms
- ✅ Auth token security implemented
- ✅ First-run UX with progress feedback
- ✅ CI builds and tests on all platforms
- ✅ Installers for macOS and Windows

**Should Have (Post-MVP):**
- ⏳ VM snapshot management UI
- ⏳ Resource configuration UI
- ⏳ Telemetry and crash reporting
- ⏳ ARM64 Linux support

**Nice to Have (Future):**
- ⏳ Multi-VM support
- ⏳ Remote agentd support
- ⏳ Docker backend alternative
- ⏳ Automatic updates

## Conclusion

This design provides a comprehensive path to cross-platform support for MowisAI, following the Docker Desktop model of transparent virtualization. By leveraging platform-native virtualization technologies (Virtualization.framework on macOS, WSL2 on Windows) and providing a QEMU fallback, we ensure broad compatibility while maintaining security and performance.

**Key Design Principles:**
1. **Native where possible**: Linux runs agentd directly, no VM overhead
2. **Transparent virtualization**: Users don't see the VM, it's an implementation detail
3. **Security-first IPC**: Use strongest available channel on each platform
4. **Zero manual setup**: No prerequisites, no configuration, just download and run
5. **Graceful degradation**: Fallback to QEMU when native virtualization unavailable

**Implementation Strategy:**
- Phased rollout over 12 weeks
- Foundation first (cross-compilation, abstractions)
- Platform-specific launchers in parallel
- Polish and testing before release

**Success Metrics:**
- <5s startup time on subsequent launches
- <1ms connection latency on Unix socket
- <200 MB bundle size on macOS
- Zero manual setup steps for users

This design is ready for implementation. Next steps: review with team, prioritize phases, and begin Phase 1 (Foundation).
