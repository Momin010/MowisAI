 #![recursion_limit = "512"]
//! `libagent` provides the core runtime primitives for the MowisAI agent sandbox engine.
//!
//! This library implements the low-level runtime, the high-level API objects, and
//! exposes a minimal C-compatible FFI.

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
pub mod orchestrator;
pub mod persistence;
pub mod protocol;
pub mod sandbox;
pub mod security;
pub mod socket_server;
pub mod tool_registry;
pub mod tools;
pub mod vertex_agent;
pub mod worker_agent;

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
#[no_mangle]
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
#[no_mangle]
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
#[no_mangle]
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
#[no_mangle]
pub extern "C" fn agent_sandbox_free(sb: *mut Sandbox) {
    if sb.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(sb);
    }
}

// Extended C FFI for memory and agent loop
#[no_mangle]
pub extern "C" fn agent_memory_new(agent_id: u64, session_id: u64) -> *mut AgentMemory {
    Box::into_raw(Box::new(AgentMemory::new(agent_id, session_id)))
}

#[no_mangle]
pub extern "C" fn agent_memory_free(mem: *mut AgentMemory) {
    if mem.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(mem);
    }
}

#[no_mangle]
pub extern "C" fn agent_loop_new(
    agent_id: u64,
    session_id: u64,
    max_iter: usize,
) -> *mut AgentLoop {
    Box::into_raw(Box::new(AgentLoop::new(agent_id, session_id, max_iter)))
}

#[no_mangle]
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
