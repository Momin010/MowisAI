use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};

use serde_json::json;
use serde_json::Value;

static NEXT_SSH_PORT: AtomicU16 = AtomicU16::new(10022);

#[derive(Debug, Clone)]
pub enum VmBackend {
    Qemu,
    Firecracker,
    Stub, // No VM available
}

#[derive(Debug, Clone)]
pub struct VmHandle {
    pub sandbox_id: String,
    pub pid: u32,
    pub backend: VmBackend,
    pub ssh_port: u16,
    pub ssh_key: PathBuf,
    pub rootfs_path: PathBuf,
    pub vm_root: PathBuf,
}

/// Detect the best available VM backend
pub fn detect_vm_backend() -> VmBackend {
    // Check for KVM support
    if PathBuf::from("/dev/kvm").exists() {
        // Check for Firecracker
        if Command::new("firecracker")
            .arg("--version")
            .output()
            .is_ok()
        {
            return VmBackend::Firecracker;
        }
        // Check for QEMU
        if Command::new("qemu-system-x86_64")
            .arg("--version")
            .output()
            .is_ok()
        {
            return VmBackend::Qemu;
        }
    }
    // Check for QEMU without KVM (slower but works)
    if Command::new("qemu-system-x86_64")
        .arg("--version")
        .output()
        .is_ok()
    {
        return VmBackend::Qemu;
    }
    VmBackend::Stub
}

/// Boot a VM for the given sandbox.
///
/// This creates a QEMU VM with:
/// - The sandbox's rootfs as the disk image
/// - SSH access on a unique port
/// - Network isolation via user-mode networking
/// - Resource limits from the sandbox spec
pub fn boot_vm(sandbox_id: String, host_root: &Path, image_hint: &str) -> Result<VmHandle> {
    let backend = detect_vm_backend();

    match backend {
        VmBackend::Qemu => boot_qemu_vm(&sandbox_id, host_root, image_hint),
        VmBackend::Firecracker => boot_firecracker_vm(&sandbox_id, host_root, image_hint),
        VmBackend::Stub => {
            // No VM backend available — return a stub handle that tracks the sandbox
            // but doesn't actually boot a VM. The sandbox will use chroot instead.
            log::info!(
                "[vm_backend] No VM backend available for sandbox {}, using stub",
                sandbox_id
            );
            let vm_root = host_root.to_path_buf();
            let ssh_key = generate_ssh_keypair(&sandbox_id)?.0;

            Ok(VmHandle {
                sandbox_id,
                pid: 0, // pid=0 means stub mode
                backend: VmBackend::Stub,
                ssh_port: 0,
                ssh_key,
                rootfs_path: vm_root.join("rootfs.ext4"),
                vm_root,
            })
        }
    }
}

/// Boot a QEMU VM with the sandbox's rootfs
fn boot_qemu_vm(sandbox_id: &str, host_root: &Path, image_hint: &str) -> Result<VmHandle> {
    let ssh_port = NEXT_SSH_PORT.fetch_add(1, Ordering::SeqCst);
    let vm_root = PathBuf::from(format!("/tmp/vm-{}", sandbox_id));
    fs::create_dir_all(&vm_root)?;

    // Generate SSH keypair
    let (ssh_key, ssh_pub) = generate_ssh_keypair(sandbox_id)?;

    // Locate kernel and rootfs
    let kernel_path = find_kernel()?;
    let rootfs_path = find_or_create_rootfs(sandbox_id, host_root, image_hint)?;

    // Prepare authorized_keys in the rootfs
    inject_ssh_key(&rootfs_path, &ssh_pub)?;

    // Build QEMU command
    let mut cmd = Command::new("qemu-system-x86_64");
    cmd
        // Machine config
        .arg("-machine")
        .arg("type=pc,accel=kvm")
        .arg("-cpu")
        .arg("host")
        .arg("-m")
        .arg("512M")
        .arg("-smp")
        .arg("2")
        // Boot
        .arg("-kernel")
        .arg(&kernel_path)
        .arg("-append")
        .arg(format!(
            "root=/dev/sda rw console=ttyS0 init=/sbin/init ip=dhcp"
        ))
        // Disk
        .arg("-drive")
        .arg(format!(
            "file={},format=raw,if=virtio",
            rootfs_path.display()
        ))
        // Network - user-mode with SSH port forward
        .arg("-netdev")
        .arg(format!("user,id=net0,hostfwd=tcp::{}-:22", ssh_port))
        .arg("-device")
        .arg("virtio-net-pci,netdev=net0")
        // Display - headless
        .arg("-nographic")
        .arg("-serial")
        .arg("mon:stdio")
        // PID file
        .arg("-pidfile")
        .arg(vm_root.join("qemu.pid"))
        // Daemonize
        .arg("-daemonize");

    let output = cmd.output().context("Failed to start QEMU")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("QEMU failed to start: {}", stderr));
    }

    // Read PID from pidfile
    let pid = fs::read_to_string(vm_root.join("qemu.pid"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);

    // Wait for SSH to become available
    wait_for_ssh(ssh_port, 30)?;

    log::info!(
        "[vm_backend] QEMU VM booted for sandbox {} (pid={}, ssh_port={})",
        sandbox_id,
        pid,
        ssh_port
    );

    Ok(VmHandle {
        sandbox_id: sandbox_id.to_string(),
        pid,
        backend: VmBackend::Qemu,
        ssh_port,
        ssh_key,
        rootfs_path,
        vm_root,
    })
}

/// Boot a Firecracker VM (microVM)
fn boot_firecracker_vm(sandbox_id: &str, host_root: &Path, image_hint: &str) -> Result<VmHandle> {
    let ssh_port = NEXT_SSH_PORT.fetch_add(1, Ordering::SeqCst);
    let vm_root = PathBuf::from(format!("/tmp/vm-{}", sandbox_id));
    fs::create_dir_all(&vm_root)?;

    let (ssh_key, ssh_pub) = generate_ssh_keypair(sandbox_id)?;
    let kernel_path = find_kernel()?;
    let rootfs_path = find_or_create_rootfs(sandbox_id, host_root, image_hint)?;

    inject_ssh_key(&rootfs_path, &ssh_pub)?;

    // Create Firecracker API socket
    let api_socket = vm_root.join("api.sock");

    // Create Firecracker config
    let config = json!({
        "boot-source": {
            "kernel_image_path": kernel_path.to_string_lossy(),
            "boot_args": "console=ttyS0 reboot=k panic=1 pci=off ip=dhcp"
        },
        "drives": [{
            "drive_id": "rootfs",
            "path_on_host": rootfs_path.to_string_lossy(),
            "is_root_device": true,
            "is_read_only": false
        }],
        "network-interfaces": [{
            "iface_id": "eth0",
            "guest_mac": format!("02:FC:00:00:00:{:02}", ssh_port % 256),
            "host_dev_name": format!("tap-{}", sandbox_id)
        }],
        "machine-config": {
            "vcpu_count": 2,
            "mem_size_mib": 512
        }
    });

    fs::write(
        vm_root.join("config.json"),
        serde_json::to_string_pretty(&config)?,
    )?;

    // Launch Firecracker
    let mut cmd = Command::new("firecracker");
    cmd.arg("--api-sock")
        .arg(&api_socket)
        .arg("--config-file")
        .arg(vm_root.join("config.json"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let child = cmd.spawn().context("Failed to start Firecracker")?;
    let pid = child.id();

    wait_for_ssh(ssh_port, 30)?;

    log::info!(
        "[vm_backend] Firecracker VM booted for sandbox {} (pid={}, ssh_port={})",
        sandbox_id,
        pid,
        ssh_port
    );

    Ok(VmHandle {
        sandbox_id: sandbox_id.to_string(),
        pid,
        backend: VmBackend::Firecracker,
        ssh_port,
        ssh_key,
        rootfs_path,
        vm_root,
    })
}

/// Stop a running VM
pub fn stop_vm(handle: &VmHandle) -> Result<()> {
    // Never kill PID 0 (stub mode)
    if handle.pid == 0 {
        log::info!("[vm_backend] VM handle is a stub (pid=0), skipping kill");
        return Ok(());
    }

    match handle.backend {
        VmBackend::Qemu => {
            // Try graceful shutdown via QEMU monitor (SIGTERM)
            let _ = Command::new("kill").arg(format!("{}", handle.pid)).status();

            // Wait up to 10 seconds for graceful shutdown
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_secs(10) {
                if !is_process_running(handle.pid) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }

            // Force kill if still running
            if is_process_running(handle.pid) {
                log::warn!(
                    "[vm_backend] QEMU pid {} didn't stop gracefully, sending SIGKILL",
                    handle.pid
                );
                let _ = Command::new("kill")
                    .arg("-9")
                    .arg(format!("{}", handle.pid))
                    .status();
            }
        }
        VmBackend::Firecracker => {
            // Firecracker exits when the API socket is removed
            let _ = Command::new("kill").arg(format!("{}", handle.pid)).status();
        }
        VmBackend::Stub => {
            // Nothing to stop
        }
    }

    // Cleanup VM root directory
    let _ = fs::remove_dir_all(&handle.vm_root);

    log::info!("[vm_backend] VM stopped for sandbox {}", handle.sandbox_id);
    Ok(())
}

/// Execute a command in the VM via SSH
pub fn exec_in_vm(handle: &VmHandle, tool_name: &str, input: Value) -> Result<Value> {
    if handle.pid == 0 && handle.ssh_port == 0 {
        return Err(anyhow::anyhow!(
            "VM is in stub mode — cannot execute commands. Use chroot sandbox instead."
        ));
    }

    exec_in_vm_ssh(handle, tool_name, input)
}

/// Execute a command in the VM via SSH
fn exec_in_vm_ssh(handle: &VmHandle, tool_name: &str, input: Value) -> Result<Value> {
    let cmd = map_tool_to_ssh(tool_name, input);
    let output = ssh_exec(handle, &cmd)?;

    Ok(json!({
        "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
        "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
        "exit_code": output.status.code().unwrap_or(-1),
        "success": output.status.success()
    }))
}

/// Execute a raw SSH command in the VM
fn ssh_exec(handle: &VmHandle, cmd: &str) -> Result<std::process::Output> {
    let output = Command::new("ssh")
        .args([
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=10",
            "-o",
            "ServerAliveCountMax=3",
            "-i",
            handle.ssh_key.to_str().unwrap_or("/dev/null"),
            "-p",
            &handle.ssh_port.to_string(),
            "root@localhost",
            cmd,
        ])
        .output()
        .context("SSH execution failed")?;

    Ok(output)
}

/// Map a tool name and input to an SSH command
fn map_tool_to_ssh(tool_name: &str, input: Value) -> String {
    // SECURITY: Use printf with %s format specifier to safely pass strings
    // This prevents shell injection through paths or content
    match tool_name {
        "read_file" => {
            let path = input["path"].as_str().unwrap_or("/workspace/.");
            // Use printf %s to safely pass path, then cat
            format!("cat -- $(printf '%s' '{}') | base64", shell_escape(path))
        }
        "write_file" => {
            let path = input["path"].as_str().unwrap_or("");
            let content = input["content"].as_str().unwrap_or("");
            // Write content via base64 to avoid shell metachar issues
            let encoded = base64_encode(content);
            format!(
                "printf '%s' '{}' | base64 -d > $(printf '%s' '{}')",
                shell_escape(&encoded),
                shell_escape(path)
            )
        }
        "run_command" => {
            let cmd = input["cmd"].as_str().unwrap_or("echo 'no command'");
            let cwd = input["cwd"].as_str().unwrap_or("/workspace");
            // For run_command, the user WANTS to execute a shell command
            // But we still need to safely pass the cwd
            format!("cd $(printf '%s' '{}') && {}", shell_escape(cwd), cmd)
        }
        "list_files" => {
            let path = input["path"].as_str().unwrap_or("/workspace");
            format!("ls -la -- $(printf '%s' '{}')", shell_escape(path))
        }
        "delete_file" => {
            let path = input["path"].as_str().unwrap_or("");
            format!("rm -f -- $(printf '%s' '{}')", shell_escape(path))
        }
        "create_directory" => {
            let path = input["path"].as_str().unwrap_or("");
            format!("mkdir -p -- $(printf '%s' '{}')", shell_escape(path))
        }
        "run_script" => {
            if let Some(script) = input["script"].as_str() {
                let encoded = base64_encode(script);
                let lang = input["language"].as_str().unwrap_or("sh");
                let interpreter = match lang {
                    "python" | "python3" => "python3",
                    "node" | "js" => "node",
                    _ => "sh",
                };
                // Pass script via base64 to avoid any shell injection
                format!(
                    "printf '%s' '{}' | base64 -d | {}",
                    shell_escape(&encoded),
                    interpreter
                )
            } else {
                "echo 'no script provided'".to_string()
            }
        }
        _ => {
            format!("echo 'Unsupported tool: {}'", shell_escape(tool_name))
        }
    }
}

/// Shell-escape a string for safe use in SSH commands
fn shell_escape(s: &str) -> String {
    // Replace single quotes with '\'' (end quote, escaped quote, start quote)
    // This is the standard POSIX shell escaping technique
    s.replace('\'', "'\\''")
}

/// Simple base64 encoding (for SSH command safety)
fn base64_encode(input: &str) -> String {
    use std::io::Write;
    let mut encoder = base64::write::EncoderWriter::new(Vec::new(), base64::STANDARD);
    let _ = encoder.write_all(input.as_bytes());
    let _ = encoder.finish();
    String::from_utf8_lossy(&encoder.into_inner().unwrap_or_default()).to_string()
}

/// Generate an SSH keypair for VM access
fn generate_ssh_keypair(sandbox_id: &str) -> Result<(PathBuf, String)> {
    let key_dir = PathBuf::from(format!("/tmp/vm-keys-{}", sandbox_id));
    fs::create_dir_all(&key_dir)?;

    let key_path = key_dir.join("id_ed25519");

    // Only generate if key doesn't exist
    if !key_path.exists() {
        let output = Command::new("ssh-keygen")
            .args([
                "-t",
                "ed25519",
                "-f",
                key_path.to_str().unwrap(),
                "-N",
                "", // No passphrase
                "-C",
                &format!("mowis-vm-{}", sandbox_id),
            ])
            .output()
            .context("Failed to generate SSH keypair")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "ssh-keygen failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Set restrictive permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600));
        }
    }

    let pub_path = key_dir.join("id_ed25519.pub");
    let pub_key = fs::read_to_string(&pub_path).unwrap_or_default();

    Ok((key_path, pub_key.trim().to_string()))
}

/// Find the kernel image for VM boot
fn find_kernel() -> Result<PathBuf> {
    let candidates = [
        "/root/.mowis/vm-assets/vmlinux",
        "/tmp/vmlinux",
        "/boot/vmlinuz",
    ];

    for candidate in &candidates {
        if Path::new(candidate).exists() {
            return Ok(PathBuf::from(candidate));
        }
    }

    Err(anyhow::anyhow!(
        "No kernel image found. Run 'agentd socket' with root to build rootfs, or place vmlinux at /root/.mowis/vm-assets/vmlinux"
    ))
}

/// Find or create a rootfs image for the VM
fn find_or_create_rootfs(sandbox_id: &str, host_root: &Path, image_hint: &str) -> Result<PathBuf> {
    let rootfs_path = PathBuf::from(format!("/tmp/vm-{}-rootfs.ext4", sandbox_id));

    if rootfs_path.exists() {
        return Ok(rootfs_path);
    }

    // Check for pre-built rootfs
    let candidates = [
        "/root/.mowis/vm-assets/mowis-rootfs.ext4",
        "/tmp/mowis-rootfs.ext4",
    ];

    for candidate in &candidates {
        if Path::new(candidate).exists() {
            // Copy the base rootfs for this sandbox
            fs::copy(candidate, &rootfs_path).context("Failed to copy base rootfs")?;
            return Ok(rootfs_path);
        }
    }

    // Create a minimal rootfs from the host root
    log::info!(
        "[vm_backend] Creating minimal rootfs for sandbox {}",
        sandbox_id
    );

    let output = Command::new("dd")
        .args([
            "if=/dev/zero",
            &format!("of={}", rootfs_path.display()),
            "bs=1M",
            "count=512",
        ])
        .output()
        .context("Failed to create rootfs image")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "dd failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let output = Command::new("mkfs.ext4")
        .arg("-F")
        .arg(&rootfs_path)
        .output()
        .context("Failed to format rootfs")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "mkfs.ext4 failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Mount and populate
    let mount_dir = PathBuf::from(format!("/tmp/vm-mount-{}", sandbox_id));
    fs::create_dir_all(&mount_dir)?;

    let output = Command::new("mount")
        .args([
            "-o",
            "loop",
            rootfs_path.to_str().unwrap(),
            mount_dir.to_str().unwrap(),
        ])
        .output()?;

    if output.status.success() {
        // Copy host root contents
        let _ = Command::new("cp")
            .args([
                "-a",
                host_root.to_str().unwrap(),
                mount_dir.to_str().unwrap(),
            ])
            .output();

        // Unmount
        let _ = Command::new("umount")
            .arg(mount_dir.to_str().unwrap())
            .output();
    }

    let _ = fs::remove_dir(&mount_dir);

    Ok(rootfs_path)
}

/// Inject an SSH public key into the rootfs
fn inject_ssh_key(rootfs_path: &Path, ssh_pub: &str) -> Result<()> {
    let mount_dir = PathBuf::from(format!("/tmp/vm-inject-{}", rootfs_path.display().len()));
    fs::create_dir_all(&mount_dir)?;

    let output = Command::new("mount")
        .args([
            "-o",
            "loop",
            rootfs_path.to_str().unwrap(),
            mount_dir.to_str().unwrap(),
        ])
        .output()?;

    if !output.status.success() {
        let _ = fs::remove_dir(&mount_dir);
        return Ok(()); // Non-fatal: VM may still work without SSH key
    }

    let ssh_dir = mount_dir.join("root/.ssh");
    let _ = fs::create_dir_all(&ssh_dir);
    let _ = fs::write(ssh_dir.join("authorized_keys"), format!("{}\n", ssh_pub));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&ssh_dir, fs::Permissions::from_mode(0o700));
        let _ = fs::set_permissions(
            ssh_dir.join("authorized_keys"),
            fs::Permissions::from_mode(0o600),
        );
    }

    let _ = Command::new("umount")
        .arg(mount_dir.to_str().unwrap())
        .output();
    let _ = fs::remove_dir(&mount_dir);

    Ok(())
}

/// Wait for SSH to become available on the given port
fn wait_for_ssh(port: u16, timeout_secs: u64) -> Result<()> {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);

    while start.elapsed() < timeout {
        // Try to connect to the SSH port
        match std::net::TcpStream::connect_timeout(
            &format!("127.0.0.1:{}", port).parse().unwrap(),
            std::time::Duration::from_secs(1),
        ) {
            Ok(_) => {
                log::debug!("[vm_backend] SSH available on port {}", port);
                return Ok(());
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }

    Err(anyhow::anyhow!(
        "SSH not available on port {} after {}s",
        port,
        timeout_secs
    ))
}

/// Check if a process is still running
fn is_process_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        use nix::sys::signal::kill;
        use nix::unistd::Pid;
        kill(Pid::from_raw(pid as i32), None).is_ok()
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}
