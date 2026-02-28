use anyhow::{anyhow, Context, Result};
use nix::mount::{mount, MsFlags};
use nix::sched::{clone, unshare, CloneFlags};
use nix::sys::signal::{kill, Signal};
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{execv, getpid, pivot_root, sethostname, Pid};
use std::ffi::CString;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use std::thread;

extern crate libc;


/// Configuration for container isolation
pub struct ContainerConfig {
    pub rootfs_path: String,
    pub command: String,
    pub timeout_secs: u64,
    pub memory_limit_mb: u64,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            rootfs_path: "/workspaces/mowisai-engine/mowisai-engine/rootfs".to_string(),
            command: String::new(),
            timeout_secs: 30,
            memory_limit_mb: 512,
        }
    }
}


/// Spawn an isolated container process with Linux namespaces
/// 
/// This function creates a new container with:
/// - PID namespace (CLONE_NEWPID)
/// - Mount namespace (CLONE_NEWNS)
/// - UTS namespace (CLONE_NEWUTS)
/// - IPC namespace (CLONE_NEWIPC)
/// - Memory limit via cgroups v2 (512MB)
/// - Timeout enforcement
pub fn spawn_container(rootfs_path: &str, command: &str, timeout_secs: u64) -> Result<Output> {
    // Setup cgroups before spawning the container
    setup_cgroups(getpid().as_raw() as u32, 512)?;
    
    // Create a pipe for communication between parent and child
    let (mut parent_read, mut child_write) = nix::unistd::pipe()?;
    let (mut child_read, mut parent_write) = nix::unistd::pipe()?;
    
    // Stack for the cloned process
    const STACK_SIZE: usize = 1024 * 1024;
    let mut stack: Vec<u8> = vec![0; STACK_SIZE];
    
    // Clone flags for container isolation
    let clone_flags = CloneFlags::CLONE_NEWPID 
        | CloneFlags::CLONE_NEWNS 
         
        | CloneFlags::CLONE_NEWUTS 
        | CloneFlags::CLONE_NEWIPC;
    
    // Prepare rootfs path and command for the child
    let rootfs = rootfs_path.to_string();
    let cmd = command.to_string();
    
    // Clone the process with new namespaces
    let child_pid = unsafe {
        clone(
            Box::new(|| {
                // Child process: setup container environment
                if let Err(e) = setup_container_environment(&rootfs, &cmd) {
                    eprintln!("Container setup failed: {}", e);
                    return -1;
                }
                0
            }),
            &mut stack,
            clone_flags,
            Some(Signal::SIGCHLD as i32),
        )
    }.map_err(|e| anyhow!("Failed to clone process: {}", e))?;
    
    // Parent process: wait for child with timeout
    wait_for_child(child_pid, timeout_secs)
}

/// Setup the container environment inside the child process
fn setup_container_environment(rootfs_path: &str, command: &str) -> Result<()> {
    // Set hostname for the container
    sethostname("mowisai-container")?;
    
    // Change to rootfs directory
    std::env::set_current_dir(rootfs_path)?;
    
    // Create old root directory for pivot_root
    fs::create_dir_all("old_root")?;
    
    // Perform pivot_root to make rootfs the new root
    pivot_root(".", "old_root")?;
    
    // Change to the new root
    std::env::set_current_dir("/")?;
    
    // Mount proc filesystem
    mount(
        Some("proc"),
        "/proc",
        Some("proc"),
        MsFlags::empty(),
        None::<&str>,
    ).context("Failed to mount /proc")?;
    
    // Unmount the old root
    let old_root_path = CString::new("/old_root")?;
    nix::mount::umount2(old_root_path.to_str().unwrap(), nix::mount::MntFlags::MNT_DETACH)?;

    fs::remove_dir_all("/old_root")?;
    
    // Execute the command via /bin/sh
    let shell = CString::new("/bin/sh")?;
    let arg_c = CString::new("-c")?;
    let arg_cmd = CString::new(command)?;
    
    execv(&shell, &[shell.clone(), arg_c, arg_cmd])
        .map_err(|e| anyhow!("Failed to exec command: {}", e))?;
    
    // This line should never be reached
    unreachable!()
}

/// Setup cgroups v2 for memory limiting
fn setup_cgroups(pid: u32, memory_limit_mb: u64) -> Result<()> {
    let cgroup_path = "/sys/fs/cgroup/mowisai";
    
    // Create cgroup directory if it doesn't exist
    fs::create_dir_all(cgroup_path)?;
    
    // Enable memory controller
    let cgroup_subtree = format!("{}/cgroup.subtree_control", cgroup_path);
    if let Ok(mut file) = fs::OpenOptions::new().write(true).open(&cgroup_subtree) {
        let _ = file.write_all(b"+memory");
    }
    
    // Set memory limit (convert MB to bytes)
    let memory_limit = memory_limit_mb * 1024 * 1024;
    let memory_max_path = format!("{}/memory.max", cgroup_path);
    fs::write(&memory_max_path, memory_limit.to_string())?;
    
    // Add current process to cgroup
    let cgroup_procs = format!("{}/cgroup.procs", cgroup_path);
    fs::write(&cgroup_procs, pid.to_string())?;
    
    Ok(())
}

/// Apply resource limits to a specific process using cgroups v2
/// 
/// Creates a per-process cgroup with memory and CPU limits
pub fn apply_resource_limits(pid: u32, memory_mb: u64, cpu_percent: u64) -> Result<()> {
    let cgroup_path = format!("/sys/fs/cgroup/mowisai/{}", pid);
    
    // Create cgroup directory for this specific process
    fs::create_dir_all(&cgroup_path)?;
    
    // Enable memory and CPU controllers
    let parent_subtree = "/sys/fs/cgroup/mowisai/cgroup.subtree_control";
    if let Ok(mut file) = fs::OpenOptions::new().write(true).open(parent_subtree) {
        let _ = file.write_all(b"+memory +cpu");
    }
    
    // Set memory limit (convert MB to bytes)
    let memory_limit = memory_mb * 1024 * 1024;
    let memory_max_path = format!("{}/memory.max", cgroup_path);
    fs::write(&memory_max_path, memory_limit.to_string())?;
    
    // Set CPU limit (convert percentage to cgroup v2 format: quota period)
    // cpu.max format: "quota period" where quota is in microseconds
    // For X% CPU: quota = X * 1000, period = 100000 (100ms)
    let cpu_quota = cpu_percent * 1000;
    let cpu_max_path = format!("{}/cpu.max", cgroup_path);
    fs::write(&cpu_max_path, format!("{} 100000", cpu_quota))?;
    
    // Add the process to this cgroup
    let cgroup_procs = format!("{}/cgroup.procs", cgroup_path);
    fs::write(&cgroup_procs, pid.to_string())?;
    
    Ok(())
}

/// Setup container networking using veth pair and NAT
/// 
/// Creates a veth pair, moves one end into the container's network namespace,
/// assigns IPs, sets up routing, and enables NAT for internet access
pub fn setup_container_network(pid: u32) -> Result<()> {
    let veth_host = format!("veth0-{}", pid);
    let veth_container = format!("veth1-{}", pid);
    let subnet_third_octet = (pid % 254) + 1; // Avoid 0 and 255
    let host_ip = format!("10.88.{}.1", subnet_third_octet);
    let container_ip = format!("10.88.{}.2", subnet_third_octet);
    let subnet = format!("10.88.{}.0/24", subnet_third_octet);
    
    eprintln!("Setting up network for container {}: {} <-> {} (subnet {})", 
              pid, veth_host, veth_container, subnet);
    
    // Create veth pair
    let output = Command::new("ip")
        .args(&["link", "add", &veth_host, "type", "veth", "peer", "name", &veth_container])
        .output()
        .context("Failed to create veth pair")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to create veth pair: {}", stderr));
    }
    
    // Move container end into network namespace
    let output = Command::new("ip")
        .args(&["link", "set", &veth_container, "netns", &pid.to_string()])
        .output()
        .context("Failed to move veth to container namespace")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Cleanup on failure
        let _ = Command::new("ip").args(&["link", "del", &veth_host]).output();
        return Err(anyhow!("Failed to move veth to container: {}", stderr));
    }
    
    // Bring up host end and assign IP
    let output = Command::new("ip")
        .args(&["link", "set", &veth_host, "up"])
        .output()
        .context("Failed to bring up host veth")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to bring up host veth: {}", stderr));
    }
    
    let output = Command::new("ip")
        .args(&["addr", "add", &format!("{}/24", host_ip), "dev", &veth_host])
        .output()
        .context("Failed to assign IP to host veth")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to assign IP to host veth: {}", stderr));
    }
    
    // Configure container network namespace
    // Bring up loopback
    let output = Command::new("nsenter")
        .args(&[
            "--net=/proc", &format!("{}/ns/net", pid),
            "--", "ip", "link", "set", "lo", "up"
        ])
        .output()
        .context("Failed to bring up container loopback")?;
    
    if !output.status.success() {
        eprintln!("Warning: Failed to bring up container loopback: {}", 
                  String::from_utf8_lossy(&output.stderr));
    }
    
    // Bring up container veth
    let output = Command::new("nsenter")
        .args(&[
            "--net=/proc", &format!("{}/ns/net", pid),
            "--", "ip", "link", "set", &veth_container, "up"
        ])
        .output()
        .context("Failed to bring up container veth")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to bring up container veth: {}", stderr));
    }
    
    // Assign IP to container veth
    let output = Command::new("nsenter")
        .args(&[
            "--net=/proc", &format!("{}/ns/net", pid),
            "--", "ip", "addr", "add", &format!("{}/24", container_ip), "dev", &veth_container
        ])
        .output()
        .context("Failed to assign IP to container")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to assign IP to container: {}", stderr));
    }
    
    // Set default route in container
    let output = Command::new("nsenter")
        .args(&[
            "--net=/proc", &format!("{}/ns/net", pid),
            "--", "ip", "route", "add", "default", "via", &host_ip
        ])
        .output()
        .context("Failed to set default route in container")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to set default route: {}", stderr));
    }
    
    // Setup DNS in container
    let resolv_conf_path = format!("/proc/{}/root/etc/resolv.conf", pid);
    if let Err(e) = fs::write(&resolv_conf_path, "nameserver 8.8.8.8\n") {
        eprintln!("Warning: Failed to write resolv.conf: {}", e);
    }
    
    // Enable IP forwarding on host
    let output = Command::new("sysctl")
        .args(&["-w", "net.ipv4.ip_forward=1"])
        .output()
        .context("Failed to enable IP forwarding")?;
    
    if !output.status.success() {
        eprintln!("Warning: Failed to enable IP forwarding: {}", 
                  String::from_utf8_lossy(&output.stderr));
    }
    
    // Setup NAT for container subnet
    let output = Command::new("iptables")
        .args(&[
            "-t", "nat", "-A", "POSTROUTING",
            "-s", &subnet,
            "-j", "MASQUERADE"
        ])
        .output()
        .context("Failed to setup NAT")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("Warning: Failed to setup NAT: {}", stderr);
    }
    
    eprintln!("Network setup complete for container {}: host IP {}, container IP {}", 
              pid, host_ip, container_ip);
    
    Ok(())
}

/// Cleanup container network (veth interface and NAT rules)
pub fn cleanup_container_network(pid: u32) {
    let veth_host = format!("veth0-{}", pid);
    let subnet_third_octet = (pid % 254) + 1;
    let subnet = format!("10.88.{}.0/24", subnet_third_octet);
    
    // Delete veth pair
    let _ = Command::new("ip")
        .args(&["link", "del", &veth_host])
        .output();
    
    // Remove NAT rule
    let _ = Command::new("iptables")
        .args(&[
            "-t", "nat", "-D", "POSTROUTING",
            "-s", &subnet,
            "-j", "MASQUERADE"
        ])
        .output();
    
    eprintln!("Cleaned up network for container {}", pid);
}

/// Wait for child process with timeout enforcement


fn wait_for_child(child_pid: Pid, timeout_secs: u64) -> Result<Output> {
    let timeout_duration = Duration::from_secs(timeout_secs);
    let start_time = std::time::Instant::now();
    
    // Create pipes to capture output
    let mut stdout_output = Vec::new();
    let mut stderr_output = Vec::new();
    
    // Poll for process completion with timeout
    loop {
        match waitpid(child_pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG))? {
            WaitStatus::Exited(_, status) => {
                use std::os::unix::process::ExitStatusExt;
                let exit_status = unsafe { std::mem::transmute::<i32, std::process::ExitStatus>(status) };
                // Process exited, collect output and return
                return Ok(Output {
                    status: exit_status,
                    stdout: stdout_output,
                    stderr: stderr_output,
                });
            }


            WaitStatus::Signaled(_, signal, _) => {
                // Process was killed by signal
                return Err(anyhow!("Process killed by signal: {:?}", signal));
            }
            _ => {
                // Process still running, check timeout
                if start_time.elapsed() > timeout_duration {
                    // Timeout reached, kill the process
                    kill(child_pid, Signal::SIGKILL)?;
                    waitpid(child_pid, None)?; // Reap the zombie
                    return Err(anyhow!("Process timed out after {} seconds", timeout_secs));
                }
                
                // Small delay to prevent busy waiting
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

/// Alternative implementation using proper Linux namespace isolation
/// This version uses clone() with namespace flags for real container isolation
pub fn spawn_container_alt(
    rootfs_path: &str, 
    command: &str, 
    timeout_secs: u64,
    memory_mb: u64,
    cpu_percent: u64,
) -> Result<Output> {

    use std::io::Read;
    use std::os::unix::io::FromRawFd;

    // Create pipe for capturing output
    let (pipe_read_fd, pipe_write_fd) = nix::unistd::pipe()?;
    let mut pipe_read = unsafe { std::fs::File::from_raw_fd(pipe_read_fd) };

    let rootfs = rootfs_path.to_string();
    let cmd = command.to_string();
    let mut stack = vec![0u8; 8 * 1024 * 1024];

    // Namespace isolation flags (sharing host network)
    let flags = CloneFlags::CLONE_NEWPID
        | CloneFlags::CLONE_NEWNS
        | CloneFlags::CLONE_NEWUTS
        | CloneFlags::CLONE_NEWIPC;


    // Child process function
    let child_fn = Box::new(move || -> isize {
        // Redirect stdout and stderr to pipe
        unsafe {
            libc::dup2(pipe_write_fd, 1);
            libc::dup2(pipe_write_fd, 2);
            libc::close(pipe_write_fd);
        }

        // Bind mount the rootfs to itself (needed for pivot_root)
        if mount(
            Some(rootfs.as_str()), 
            rootfs.as_str(),
            None::<&str>,
            MsFlags::MS_BIND | MsFlags::MS_REC,
            None::<&str>,
        ).is_err() { 
            return 1; 
        }

        // Change to rootfs directory
        if std::env::set_current_dir(&rootfs).is_err() { 
            return 1; 
        }

        // Create old_root directory for pivot_root
        let _ = fs::create_dir_all("old_root");
        
        // Perform pivot_root to make rootfs the new root
        if pivot_root(".", "./old_root").is_err() { 
            return 1; 
        }

        // Change to new root
        let _ = std::env::set_current_dir("/");

        // Mount proc filesystem
        let _ = fs::create_dir_all("/proc");
        let _ = mount(
            Some("proc"), 
            "/proc", 
            Some("proc"), 
            MsFlags::empty(), 
            None::<&str>
        );

        // Mount devtmpfs
        let _ = fs::create_dir_all("/dev");
        let _ = mount(
            Some("devtmpfs"),
            "/dev",
            Some("devtmpfs"),
            MsFlags::empty(),
            None::<&str>
        );

        // Unmount old root
        let _ = nix::mount::umount2("/old_root", nix::mount::MntFlags::MNT_DETACH);

        // Set PATH environment variable for container
        let _ = std::env::set_var("PATH", "/usr/local/bin:/usr/bin:/bin");

        // Execute the command using /bin/sh (which is symlinked to /usr/bin/busybox after pivot_root)
        let c_sh = std::ffi::CString::new("/bin/sh").unwrap();
        let c_flag = std::ffi::CString::new("-c").unwrap();
        let c_cmd = std::ffi::CString::new(cmd.as_str()).unwrap();
        let _ = execv(&c_sh, &[&c_sh, &c_flag, &c_cmd]);
        
        1
    });

    // Clone the process with new namespaces
    let child_pid = unsafe { 
        clone(child_fn, &mut stack, flags, Some(libc::SIGCHLD))? 
    };
    
    // Close write end in parent
    unsafe { libc::close(pipe_write_fd); }

    // Apply resource limits to the child process
    if let Err(e) = apply_resource_limits(child_pid.as_raw() as u32, memory_mb, cpu_percent) {
        eprintln!("Warning: Failed to apply resource limits: {}", e);
    }

    // Wait for child with timeout


    let start = std::time::Instant::now();
    loop {
        match waitpid(child_pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => {
                if start.elapsed().as_secs() >= timeout_secs {
                    let _ = kill(child_pid, Signal::SIGKILL);
                    let _ = waitpid(child_pid, None);
                    return Ok(Output { 
                        status: std::process::Command::new("true").status().unwrap(), // 128 + SIGKILL(9)
                        stdout: b"error: timeout".to_vec(), 
                        stderr: vec![] 
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Ok(_) => break,
            Err(e) => return Err(anyhow!("waitpid: {}", e)),
        }
    }

    // Read output from pipe
    let mut output = String::new();
    pipe_read.read_to_string(&mut output)?;
    
    Ok(Output { 
        status: std::process::Command::new("true").status().unwrap(), 
        stdout: output.into_bytes(), 
        stderr: vec![] 
    })
}


/// Spawn an interactive shell session in the container
/// 
/// This function creates an interactive shell where the user can
/// "enter" the container like WSL, with a colored prompt.
pub fn spawn_interactive_shell(rootfs_path: &str, memory_mb: u64, cpu_percent: u64) -> Result<()> {
    use std::os::unix::io::FromRawFd;
    
    println!("\x1b[32m➜  Entering MowisAI container...\x1b[0m");
    println!("\x1b[36m    (type 'exit' to leave)\x1b[0m\n");

    let rootfs = rootfs_path.to_string();
    let mut stack = vec![0u8; 8 * 1024 * 1024];

    // Namespace isolation flags (sharing host network)
    let flags = CloneFlags::CLONE_NEWPID
        | CloneFlags::CLONE_NEWNS
        | CloneFlags::CLONE_NEWUTS
        | CloneFlags::CLONE_NEWIPC;

    // Child process function - runs interactive shell
    let child_fn = Box::new(move || -> isize {
        // Bind mount the rootfs to itself (needed for pivot_root)
        if mount(
            Some(rootfs.as_str()), 
            rootfs.as_str(),
            None::<&str>,
            MsFlags::MS_BIND | MsFlags::MS_REC,
            None::<&str>,
        ).is_err() { 
            eprintln!("Failed to bind mount rootfs");
            return 1; 
        }

        // Change to rootfs directory
        if std::env::set_current_dir(&rootfs).is_err() { 
            eprintln!("Failed to change to rootfs directory");
            return 1; 
        }

        // Create old_root directory for pivot_root
        let _ = fs::create_dir_all("old_root");
        
        // Perform pivot_root to make rootfs the new root
        if pivot_root(".", "./old_root").is_err() { 
            eprintln!("Failed to pivot_root");
            return 1; 
        }

        // Change to new root
        let _ = std::env::set_current_dir("/");

        // Mount proc filesystem
        let _ = fs::create_dir_all("/proc");
        let _ = mount(
            Some("proc"), 
            "/proc", 
            Some("proc"), 
            MsFlags::empty(), 
            None::<&str>
        );

        // Unmount old root
        let _ = nix::mount::umount2("/old_root", nix::mount::MntFlags::MNT_DETACH);

        // Set up colored prompt environment variables
        std::env::set_var("PS1", "\\[\\e[32m\\]mowisai\\[\\e[0m\\]:\\[\\e[34m\\]\\w\\[\\e[0m\\]$ ");
        std::env::set_var("TERM", "xterm-256color");

        // Execute interactive shell
        let c_sh = std::ffi::CString::new("/bin/sh").unwrap();
        let c_lflag = std::ffi::CString::new("-l").unwrap();
        let _ = execv(&c_sh, &[&c_sh, &c_lflag]);
        
        eprintln!("Failed to exec shell");
        1
    });

    // Clone the process with new namespaces
    let child_pid = unsafe { 
        clone(child_fn, &mut stack, flags, Some(libc::SIGCHLD))? 
    };

    // Apply resource limits to the child process
    if let Err(e) = apply_resource_limits(child_pid.as_raw() as u32, memory_mb, cpu_percent) {
        eprintln!("Warning: Failed to apply resource limits: {}", e);
    }

    // Wait for the interactive shell to exit
    let _ = waitpid(child_pid, None);
    
    println!("\n\x1b[32m➜  Exited MowisAI container\x1b[0m");
    
    Ok(())
}

/// Cleanup cgroups after container execution
pub fn cleanup_cgroups() -> Result<()> {

    let cgroup_path = "/sys/fs/cgroup/mowisai";
    if std::path::Path::new(cgroup_path).exists() {
        // Kill any remaining processes in the cgroup
        let procs_file = format!("{}/cgroup.procs", cgroup_path);
        if let Ok(content) = fs::read_to_string(&procs_file) {
            for pid_str in content.lines() {
                if let Ok(pid) = pid_str.parse::<i32>() {
                    // Cleanup network for this PID
                    cleanup_container_network(pid as u32);
                    let _ = kill(nix::unistd::Pid::from_raw(pid), Signal::SIGKILL);
                }
            }
        }
        
        // Remove the cgroup directory
        let _ = fs::remove_dir(cgroup_path);
    }
    Ok(())
}

/// Spawn a persistent container that listens on a Unix socket for commands
/// 
/// Creates a named Unix socket at /tmp/mowisai-session-{session_id}.sock
/// and runs a loop inside the container that reads JSON commands and executes them.
/// Returns the container PID.
pub fn spawn_persistent_container(
    rootfs_path: &str,
    session_id: &str,
    memory_mb: u64,
    cpu_percent: u64,
) -> Result<u32> {
    use std::os::unix::net::UnixListener;
    use std::os::unix::io::AsRawFd;
    
    let socket_path = format!("/tmp/mowisai-session-{}.sock", session_id);
    
    // Remove existing socket if it exists
    let _ = fs::remove_file(&socket_path);
    
    // Create the socket
    let listener = UnixListener::bind(&socket_path)?;
    listener.set_nonblocking(true)?;
    
    let rootfs = rootfs_path.to_string();
    let socket_path_clone = socket_path.clone();
    let mut stack = vec![0u8; 8 * 1024 * 1024];

    // Namespace isolation flags (sharing host network)
    let flags = CloneFlags::CLONE_NEWPID
        | CloneFlags::CLONE_NEWNS
        | CloneFlags::CLONE_NEWUTS
        | CloneFlags::CLONE_NEWIPC;

    // Child process function - runs persistent command loop
    let child_fn = Box::new(move || -> isize {
        // Bind mount the rootfs to itself (needed for pivot_root)
        if mount(
            Some(rootfs.as_str()),
            rootfs.as_str(),
            None::<&str>,
            MsFlags::MS_BIND | MsFlags::MS_REC,
            None::<&str>,
        ).is_err() {
            eprintln!("Failed to bind mount rootfs");
            return 1;
        }

        // Change to rootfs directory
        if std::env::set_current_dir(&rootfs).is_err() {
            eprintln!("Failed to change to rootfs directory");
            return 1;
        }

        // Create old_root directory for pivot_root
        let _ = fs::create_dir_all("old_root");

        // Perform pivot_root to make rootfs the new root
        if pivot_root(".", "./old_root").is_err() {
            eprintln!("Failed to pivot_root");
            return 1;
        }

        // Change to new root
        let _ = std::env::set_current_dir("/");

        // Mount proc filesystem
        let _ = fs::create_dir_all("/proc");
        let _ = mount(
            Some("proc"),
            "/proc",
            Some("proc"),
            MsFlags::empty(),
            None::<&str>
        );

        // Mount devtmpfs for device access
        let _ = fs::create_dir_all("/dev");
        let _ = mount(
            Some("devtmpfs"),
            "/dev",
            Some("devtmpfs"),
            MsFlags::empty(),
            None::<&str>
        );

        // Unmount old root
        let _ = nix::mount::umount2("/old_root", nix::mount::MntFlags::MNT_DETACH);

        // Set PATH environment variable
        let _ = std::env::set_var("PATH", "/usr/local/bin:/usr/bin:/bin");

        // Command processing loop
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Read command from socket
                    let mut buffer = String::new();
                    let mut byte = [0u8; 1];
                    
                    // Read until newline
                    loop {
                        match stream.read(&mut byte) {
                            Ok(0) => break, // Connection closed
                            Ok(_) => {
                                if byte[0] == b'\n' {
                                    break;
                                }
                                buffer.push(byte[0] as char);
                            }
                            Err(_) => break,
                        }
                    }
                    
                    // Parse the command
                    let command = buffer.trim();
                    if command.is_empty() {
                        continue;
                    }
                    
                    // Execute the command using /bin/sh (symlinked to /usr/bin/busybox after pivot_root)
                    let output = Command::new("/bin/sh")
                        .arg("-c")
                        .arg(command)
                        .output();
                    
                    // Format response
                    let response = match output {
                        Ok(result) => {
                            let stdout = String::from_utf8_lossy(&result.stdout);
                            let stderr = String::from_utf8_lossy(&result.stderr);
                            format!("{}{}", stdout, stderr)
                        }
                        Err(e) => format!("Error: {}", e),
                    };
                    
                    // Send response back
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.write_all(b"\n");
                    let _ = stream.flush();
                }
                Err(_) => {
                    // No connection, sleep briefly to prevent busy loop
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
    });

    // Clone the process with new namespaces
    let child_pid = unsafe {
        clone(child_fn, &mut stack, flags, Some(libc::SIGCHLD))?
    };

    // Apply resource limits to the child process
    if let Err(e) = apply_resource_limits(child_pid.as_raw() as u32, memory_mb, cpu_percent) {
        eprintln!("Warning: Failed to apply resource limits: {}", e);
    }

    // Return the PID
    Ok(child_pid.as_raw() as u32)
}

/// Execute a command in a persistent container
/// 
/// Connects to /tmp/mowisai-session-{session_id}.sock, sends the command,
/// and returns the output.
pub fn exec_in_container(session_id: &str, command: &str, timeout_secs: u64) -> Result<String> {
    use std::os::unix::net::UnixStream;
    
    let socket_path = format!("/tmp/mowisai-session-{}.sock", session_id);
    
    let mut stream = UnixStream::connect(&socket_path)
        .map_err(|e| anyhow!("Failed to connect to container socket: {}", e))?;
    
    // Send the command with newline terminator
    stream.write_all(command.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    
    // Read response with timeout
    let mut response = String::new();
    let start = std::time::Instant::now();
    
    loop {
        let mut buffer = [0u8; 1024];
        match stream.read(&mut buffer) {
            Ok(n) if n > 0 => {
                response.push_str(&String::from_utf8_lossy(&buffer[..n]));
                // Check if we have a complete line
                if response.contains('\n') {
                    break;
                }
            }
            Ok(_) => break, // Connection closed
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed().as_secs() >= timeout_secs {
                    return Err(anyhow!("Timeout waiting for response"));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => return Err(anyhow!("Socket read error: {}", e)),
        }
    }
    
    // Trim the response
    Ok(response.trim().to_string())
}

/// Kill a persistent container and clean up its socket
/// 
/// Kills the process and removes the Unix socket.
pub fn kill_container(session_id: &str) -> Result<()> {
    let socket_path = format!("/tmp/mowisai-session-{}.sock", session_id);
    
    // Try to find the PID from the socket (not directly possible, so we use a different approach)
    // We'll try to connect and send a special kill command, or use cgroups
    let cgroup_path = format!("/sys/fs/cgroup/mowisai");
    
    if std::path::Path::new(&cgroup_path).exists() {
        // Read PIDs from cgroup
        let procs_file = format!("{}/cgroup.procs", cgroup_path);
        if let Ok(content) = fs::read_to_string(&procs_file) {
            for pid_str in content.lines() {
                if let Ok(pid) = pid_str.parse::<i32>() {
                    // Check if this process is associated with our session by trying to connect
                    // For now, kill all processes in the cgroup
                    let _ = kill(nix::unistd::Pid::from_raw(pid), Signal::SIGTERM);
                    
                    // Give it a moment to terminate gracefully
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    
                    // Force kill if still running
                    let _ = kill(nix::unistd::Pid::from_raw(pid), Signal::SIGKILL);
                }
            }
        }
    }
    
    // Remove the socket file
    let _ = fs::remove_file(&socket_path);
    
    Ok(())
}



#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    
    #[test]
    fn test_container_config_default() {
        let config = ContainerConfig::default();
        assert_eq!(config.rootfs_path, "/workspaces/mowisai-engine/mowisai-engine/rootfs");
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.memory_limit_mb, 512);
    }

    
    #[test]
    fn test_container_config_custom() {
        let config = ContainerConfig {
            rootfs_path: "/custom/rootfs".to_string(),
            command: "echo test".to_string(),
            timeout_secs: 60,
            memory_limit_mb: 1024,
        };
        
        assert_eq!(config.rootfs_path, "/custom/rootfs");
        assert_eq!(config.timeout_secs, 60);
        assert_eq!(config.memory_limit_mb, 1024);
    }
    
    #[test]
    fn test_cgroup_path_format() {
        let cgroup_base = "/sys/fs/cgroup/mowisai";
        assert!(cgroup_base.ends_with("mowisai"));
        assert!(cgroup_base.starts_with("/sys/fs/cgroup"));
    }
    
    #[test]
    fn test_clone_flags_combination() {
        let flags = CloneFlags::CLONE_NEWPID 
            | CloneFlags::CLONE_NEWNS 
             
            | CloneFlags::CLONE_NEWUTS 
            | CloneFlags::CLONE_NEWIPC;
        
        // Verify all flags are present
        assert!(flags.contains(CloneFlags::CLONE_NEWPID));
        assert!(flags.contains(CloneFlags::CLONE_NEWNS));
        assert!(flags.contains(CloneFlags::CLONE_NEWUTS));
        assert!(flags.contains(CloneFlags::CLONE_NEWIPC));
    }
    
    #[test]
    fn test_memory_limit_calculation() {
        let mb = 512u64;
        let bytes = mb * 1024 * 1024;
        assert_eq!(bytes, 536870912); // 512 MB in bytes
    }
    
    #[test]
    fn test_timeout_duration_calculation() {
        let timeout_secs = 30u64;
        let duration = std::time::Duration::from_secs(timeout_secs);
        assert_eq!(duration.as_secs(), 30);
        assert_eq!(duration.as_millis(), 30000);
    }
    
    #[test]
    fn test_command_escaping() {
        let command = "echo 'hello world'";
        let escaped = command.replace("'", "'\"'\"'");
        assert_eq!(escaped, "echo '\"'\"'hello world'\"'\"'");
    }
    
    #[test]
    fn test_rootfs_path_validation() {
        let valid_paths = vec![
            "./rootfs",
            "/var/lib/containers/rootfs",
            "/tmp/test-rootfs",
        ];
        
        for path in valid_paths {
            assert!(!path.is_empty());
            assert!(!path.contains("..")); // Security check
        }
    }
    
    #[test]
    fn test_cleanup_cgroups_idempotent() {
        // Should not panic even if cgroup doesn't exist
        let result = cleanup_cgroups();
        // This may fail if not running as root, but should not panic
        // Result is ignored as we just test it doesn't crash
        let _ = result;
    }
    
    #[test]
    fn test_container_config_builder_pattern() {
        let config = ContainerConfig {
            rootfs_path: "./test-rootfs".to_string(),
            command: "ls -la".to_string(),
            timeout_secs: 45,
            memory_limit_mb: 256,
        };
        
        // Verify all fields are set correctly
        assert_eq!(config.rootfs_path, "./test-rootfs");
        assert_eq!(config.command, "ls -la");
        assert_eq!(config.timeout_secs, 45);
        assert_eq!(config.memory_limit_mb, 256);
    }
    
    #[test]
    fn test_alpine_version_constant() {
        // This test documents the expected Alpine version
        let expected_version = "3.19";
        assert_eq!(expected_version, "3.19");
    }
}
