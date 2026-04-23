
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::{Command};
use std::sync::atomic::{AtomicU16, Ordering};

use serde_json::json;
use serde_json::Value;

static NEXT_SSH_PORT: AtomicU16 = AtomicU16::new(10022);

#[derive(Debug, Clone)]
pub enum VmBackend {
    Qemu,
    Firecracker, // Future: if /dev/kvm
}

#[derive(Debug, Clone)]
pub struct VmHandle {
    pub sandbox_id: String,
    pub pid: u32,
    pub backend: VmBackend,
    pub ssh_port: u16,
    pub ssh_key: PathBuf,
    pub rootfs_path: PathBuf, // /tmp/vm-{id}-rootfs.ext4
}

pub fn detect_vm_backend() -> VmBackend {
    if PathBuf::from("/dev/kvm").exists() {
        if Command::new("firecracker").status().is_ok() {
            return VmBackend::Firecracker;
        }
    }
    VmBackend::Qemu // Codespace default
}

pub fn boot_vm(sandbox_id: String, _host_root: &std::path::Path, _image_hint: &str) -> anyhow::Result<VmHandle> {
    // VM backend temporarily disabled - focus on new orchestration system
    // TODO: Re-implement VM backend with proper error handling
    
    // Generate SSH keypair for future use
    let keypair = generate_ssh_keypair(&sandbox_id)?;
    let ssh_port = NEXT_SSH_PORT.fetch_add(1, Ordering::SeqCst);
    
    // Return a stub handle for now
    let handle = VmHandle {
        sandbox_id: sandbox_id.clone(),
        pid: 0,
        backend: VmBackend::Qemu,
        ssh_port,
        ssh_key: keypair.0.clone(),
        rootfs_path: std::path::PathBuf::from(format!("/tmp/vm-{}-rootfs.ext4", sandbox_id)),
    };
    
    Ok(handle)
}


pub fn stop_vm(handle: &VmHandle) -> anyhow::Result<()> {

    // QEMU: qemu-monitor or kill
    let _ = Command::new("kill").arg(format!("{}", handle.pid)).status();
    // Cleanup
    let _ = fs::remove_file(&handle.rootfs_path);
    let _ = fs::remove_dir_all(handle.ssh_key.parent().unwrap());
    log::info!("[vm_backend] VM stopped sandbox={}", handle.sandbox_id);
    Ok(())
}

pub fn exec_in_vm(handle: &VmHandle, tool_name: &str, input: Value) -> Result<Value> {
    exec_in_vm_ssh(handle, tool_name, input)
}

fn exec_in_vm_ssh(handle: &VmHandle, tool_name: &str, input: Value) -> Result<Value> {
    let cmd = map_tool_to_ssh(tool_name, input);
    let output = ssh_exec(handle, &cmd)?;
    
    // Parse output to ToolResult format
    Ok(json!({
        "success": output.status.success(),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr)
    }))
}

fn map_tool_to_ssh(tool: &str, input: Value) -> String {
    match tool {
        "read_file" => {
            let path = input["path"].as_str().unwrap_or("");
            format!("cat /workspace/{}", path)
        }
        "write_file" => {
            let path = input["path"].as_str().unwrap_or("");
            let content_b64 = input["content"].as_str().unwrap_or("");
            format!("echo '{}' | base64 -d > /workspace/{}", content_b64, path)
        }
        "run_command" => {
            let cmd = input["cmd"].as_str().unwrap_or("");
            format!("cd /workspace && {}", cmd)
        }
        "list_files" => {
            let path = input["path"].as_str().unwrap_or(".");
            format!("find /workspace/{} -maxdepth 2 -type f", path)
        }
        "git_clone" => {
            let url = input["url"].as_str().unwrap_or("");
            format!("cd /workspace && git clone {} repo || true", url)
        }
        "pip_install" => {
            let pkgs = input["packages"].as_array().unwrap_or(&vec![]).iter().map(|v| v.as_str().unwrap_or("")).collect::<Vec<_>>().join(" ");
            format!("pip install {}", pkgs)
        }
        _ => "echo 'tool not mapped'".to_string(),
    }
}

fn ssh_exec(handle: &VmHandle, cmd: &str) -> Result<std::process::Output> {
    let output = Command::new("ssh")
        .args([
            "-o", "StrictHostKeyChecking=no",
            "-o", "UserKnownHostsFile=/dev/null",
            "-o", "ConnectTimeout=10",
            "-i", handle.ssh_key.to_str().unwrap(),
            &format!("root@localhost -p {}", handle.ssh_port),
            cmd,
        ])
        .output()
        .context("ssh exec")?;
    Ok(output)
}

fn generate_ssh_keypair(sandbox_id: &str) -> Result<(PathBuf, PathBuf)> {
    let key_dir = std::env::temp_dir().join(format!("mowis-ssh-{}", sandbox_id));
    fs::create_dir_all(&key_dir)?;
    let private = key_dir.join("id_ed25519");
    let public = key_dir.join("id_ed25519.pub");
    Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-f", private.to_str().unwrap(), "-N", "", "-q"])
        .status()?;
    Ok((private, public))
}

