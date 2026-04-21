#![recursion_limit = "512"]
//! `libagent` provides the core runtime primitives for the MowisAI agent sandbox engine.
//!
//! This library implements the low-level runtime, the high-level API objects, and
//! exposes a minimal C-compatible FFI.

pub mod config;
pub mod setup;
pub mod logging;
pub mod tui;
pub mod agent;
pub mod agent_loop;
pub mod hub_agent_client;
pub mod claude_integration;
pub mod audit;
pub mod buckets;
pub mod channels;
pub mod dependency_graph;
pub mod hub_agent;
pub mod image_manager;
pub mod guest_backend;
pub mod vm_backend;
pub mod memory;
pub mod orchestration;
pub mod persistence;
pub mod protocol;
pub mod sandbox;
pub mod security;
pub mod socket_server;
pub mod tool_registry;
pub mod tools;
pub mod vertex_agent;
pub mod worker_agent;
pub mod intent;
pub mod version;

use std::sync::atomic::{AtomicBool, Ordering};

/// Global shutdown flag for signal handling
pub static SHUTDOWN_FLAG: AtomicBool = AtomicBool::new(false);

/// Check if shutdown was requested via signal
pub fn is_shutdown_requested() -> bool {
    SHUTDOWN_FLAG.load(Ordering::Acquire)
}

/// Set shutdown flag
pub fn set_shutdown() {
    SHUTDOWN_FLAG.store(true, Ordering::Release);
}

// Socket server management utilities
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;
use anyhow::Result;
use std::fs;

/// Check if socket server is responsive
pub fn socket_is_responsive(socket_path: &str) -> bool {
    use std::os::unix::net::UnixStream;

    if !Path::new(socket_path).exists() {
        return false;
    }

    match UnixStream::connect(socket_path) {
        Ok(stream) => {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
            true
        }
        Err(_) => false,
    }
}

/// Get the PID of the socket server process (by checking for mowisai socket process)
pub fn get_socket_server_pid() -> Result<u32> {
    let output = Command::new("pgrep")
        .args(["-f", "mowisai.*socket"])
        .output()?;

    if output.status.success() {
        let pid_str = String::from_utf8_lossy(&output.stdout);
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            return Ok(pid);
        }
    }

    Err(anyhow::anyhow!("Could not determine socket server PID"))
}

/// Save socket server PID to file
pub fn save_socket_pid(pid: u32) -> Result<()> {
    let config_dir = crate::config::MowisConfig::config_dir();
    let pid_file = config_dir.join(".socket-server.pid");
    fs::write(&pid_file, pid.to_string())?;
    Ok(())
}

/// Read saved socket server PID from file
pub fn read_socket_pid() -> Result<u32> {
    let config_dir = crate::config::MowisConfig::config_dir();
    let pid_file = config_dir.join(".socket-server.pid");
    let pid_str = fs::read_to_string(&pid_file)?;
    Ok(pid_str.trim().parse::<u32>()?)
}

/// Start socket server daemon with sudo
pub fn start_socket_server_daemon(socket_path: &str) -> Result<u32> {
    // Try to clean up stale socket
    let _ = fs::remove_file(socket_path);

    log::info!("Attempting to start socket server at {} with sudo...", socket_path);

    // Try to start socket server as background process with sudo
    // Get the directory of the current executable to find the correct binary path
    let current_exe = std::env::current_exe().unwrap_or_default();
    let bin_dir = current_exe.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| std::path::PathBuf::from("."));
    let release_binary = bin_dir.join("mowisai");
    let debug_binary = std::path::PathBuf::from("target/debug/mowisai");
    
    // Prefer release binary if it exists, fall back to debug
    let binary_path = if release_binary.exists() {
        release_binary.to_string_lossy().to_string()
    } else if debug_binary.exists() {
        debug_binary.to_string_lossy().to_string()
    } else {
        // Try to find mowisai in PATH
        "mowisai".to_string()
    };
    
    log::info!("Starting socket server using binary: {}", binary_path);
    
    let result = Command::new("sudo")
        .args([
            "-n", // non-interactive (use cached credentials)
            &binary_path,
            "socket",
            "--path",
            socket_path,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    match result {
        Ok(_child) => {
            // Give it a moment to start
            thread::sleep(Duration::from_millis(500));

            // Verify it started
            if socket_is_responsive(socket_path) {
                log::info!("✓ Socket server started successfully");

                // Get PID and save it
                match get_socket_server_pid() {
                    Ok(pid) => {
                        save_socket_pid(pid).ok();
                        log::info!("Socket server PID: {}", pid);
                        return Ok(pid);
                    }
                    Err(e) => {
                        log::warn!("Started socket server but couldn't get PID: {}", e);
                        return Err(e);
                    }
                }
            } else {
                log::warn!("Socket server started but not responding yet, retrying...");
                thread::sleep(Duration::from_millis(1000));

                if socket_is_responsive(socket_path) {
                    log::info!("✓ Socket server up after retry");
                    if let Ok(pid) = get_socket_server_pid() {
                        save_socket_pid(pid).ok();
                        return Ok(pid);
                    }
                }
            }
        }
        Err(e) => {
            log::warn!("Failed to start socket server with sudo -n: {}", e);
            log::warn!("Trying interactive sudo prompt...");

            // Fall back to interactive sudo (will prompt user)
            let result = Command::new("sudo")
                .args([
                    &binary_path,
                    "socket",
                    "--path",
                    socket_path,
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();

            match result {
                Ok(_child) => {
                    thread::sleep(Duration::from_millis(500));

                    if socket_is_responsive(socket_path) {
                        log::info!("✓ Socket server started successfully");
                        if let Ok(pid) = get_socket_server_pid() {
                            save_socket_pid(pid).ok();
                            return Ok(pid);
                        } else {
                            return Err(anyhow::anyhow!("Started but couldn't get PID"));
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Failed to start socket server: {}", e);
                    return Err(anyhow::anyhow!("Failed to start socket server: {}", e));
                }
            }
        }
    }

    // If we get here, socket server didn't start
    Err(anyhow::anyhow!("Socket server failed to start"))
}

/// Re-export infrastructure `Runtime` (crate `runtime`) for orchestration callers.
pub mod runtime {
    pub use ::runtime::Runtime;
    pub use ::runtime::{RuntimeError, RuntimeResult};
}

// re-export common types at the top level for convenience
pub use agent::{Agent, AgentConfig, AgentResult};
pub use agent_loop::{AgentCoordinator, AgentLoop};
pub use audit::{AuditEvent, AuditLogger, SecurityAuditor};
pub use image_manager::ImageManager;
pub use vm_backend::{boot_vm, exec_in_vm, stop_vm, VmHandle};
pub use memory::{AgentMemory, LongTermMemory, ShortTermMemory};
pub use persistence::{Checkpointer, PersistenceManager, RecoveryJournal, WriteAheadLog};
pub use protocol::*;
pub use sandbox::ResourceLimits;
pub use sandbox::Sandbox;
pub use security::{SeccompFilter, SecurityContext, SecurityPolicy};
pub use tools::{Tool, ToolContext, ToolDefinition};

// C FFI helpers
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// create a new sandbox and return a pointer (caller owns). ram and cpu are optional
/// limits (0 means none).
#[unsafe(no_mangle)]
pub extern "C" fn agent_sandbox_new(ram: u64, cpu: u64) -> *mut Sandbox {
    let limits = ResourceLimits {
        ram_bytes: if ram == 0 { None } else { Some(ram) },
        cpu_millis: if cpu == 0 { None } else { Some(cpu) },
    };
    match Sandbox::new(limits) {
        Ok(sb) => Box::into_raw(Box::new(sb)),
        Err(_) => std::ptr::null_mut(),
    }
}

/// run a command in sandbox; returns owned C string which must be freed by caller.
#[unsafe(no_mangle)]
pub extern "C" fn agent_sandbox_run(sb: *mut Sandbox, cmd: *const c_char) -> *mut c_char {
    if sb.is_null() || cmd.is_null() {
        return std::ptr::null_mut();
    }
    let sb = unsafe { &*sb };
    let cstr = unsafe { CStr::from_ptr(cmd) };
    if let Ok(s) = cstr.to_str() {
        if let Ok(output) = sb.run_command(s) {
            if let Ok(cout) = CString::new(output) {
                return cout.into_raw();
            }
        }
    }
    std::ptr::null_mut()
}

/// free string returned by agent_sandbox_run
#[unsafe(no_mangle)]
pub extern "C" fn agent_string_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    unsafe {
        // `from_raw` returns a CString which drops when it goes out of scope.
        let _ = CString::from_raw(s);
    }
}

/// free sandbox
#[unsafe(no_mangle)]
pub extern "C" fn agent_sandbox_free(sb: *mut Sandbox) {
    if sb.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(sb);
    }
}

// Extended C FFI for memory and agent loop
#[unsafe(no_mangle)]
pub extern "C" fn agent_memory_new(agent_id: u64, session_id: u64) -> *mut AgentMemory {
    Box::into_raw(Box::new(AgentMemory::new(agent_id, session_id)))
}

#[unsafe(no_mangle)]
pub extern "C" fn agent_memory_free(mem: *mut AgentMemory) {
    if mem.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(mem);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn agent_loop_new(
    agent_id: u64,
    session_id: u64,
    max_iter: usize,
) -> *mut AgentLoop {
    Box::into_raw(Box::new(AgentLoop::new(agent_id, session_id, max_iter)))
}

#[unsafe(no_mangle)]
pub extern "C" fn agent_loop_free(loop_ptr: *mut AgentLoop) {
    if loop_ptr.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(loop_ptr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_builds() {
        // simple sanity check
        assert_eq!(2 + 2, 4);
    }
}
