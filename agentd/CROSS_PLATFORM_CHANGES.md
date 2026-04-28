# Cross-Platform Compilation Changes

## Task 1.1: Add conditional compilation guards to agentd

This document summarizes the changes made to enable agentd to compile on macOS and Windows targets.

### Changes Made

#### 1. Cargo.toml Dependencies
- Moved `nix` and `signal-hook` dependencies from `[dependencies]` to `[target.'cfg(target_os = "linux")'.dependencies]`
- These crates are Linux-specific and only needed when building for Linux

#### 2. Module Guards in lib.rs
- Added `#[cfg(target_os = "linux")]` guards to `sandbox` and `vm_backend` modules
- These modules use Linux-specific kernel features (overlayfs, cgroups, namespaces)

#### 3. Stub Implementations
- Added stub implementations for `sandbox` and `vm_backend` modules on non-Linux platforms
- Stubs return appropriate error messages indicating Linux-only support
- This allows the crate to compile while making it clear these features are unavailable

#### 4. Re-exports
- Updated re-exports in lib.rs to conditionally export Linux-only types
- Added conditional re-exports for stub implementations on non-Linux platforms

#### 5. Unix Socket Guards
- Added `#[cfg(unix)]` guards to `socket_is_responsive()` function
- Added stub implementation for non-Unix platforms

#### 6. C FFI Guards
- Added `#[cfg(target_os = "linux")]` guards to sandbox-related C FFI functions
- Added stub implementations for non-Linux platforms

#### 7. Signal Handler Guards (main.rs)
- Added `#[cfg(target_os = "linux")]` guard to `setup_signal_handlers()` function
- Added stub implementation for non-Linux platforms

#### 8. Sandbox Module Guards (sandbox.rs)
- Guarded nix imports with `#[cfg(target_os = "linux")]`
- Guarded Unix-specific imports with `#[cfg(unix)]`
- Added platform guards to:
  - `create_container()` - uses mount syscalls
  - `destroy_container()` - uses umount syscalls
  - `run_command()` - uses unshare, chroot, setrlimit

#### 9. Shell Tool Guards (tools/shell.rs)
- Added `#[cfg(target_os = "linux")]` guard to `KillProcessTool::invoke()`
- Added stub implementation for non-Linux platforms

#### 10. Socket Server Guards (socket_server.rs)
- Guarded Unix socket imports with `#[cfg(unix)]`
- Guarded bind mount code with `#[cfg(target_os = "linux")]`

### Files Modified
1. `agentd/Cargo.toml` - Dependency configuration
2. `agentd/src/lib.rs` - Module guards and stub implementations
3. `agentd/src/main.rs` - Signal handler guards
4. `agentd/src/sandbox.rs` - Import and function guards
5. `agentd/src/tools/shell.rs` - Kill process tool guards
6. `agentd/src/socket_server.rs` - Unix socket and mount guards

### Requirements Satisfied
- ✅ 3.1: mowis-gui compiles on all platforms (agentd library compiles)
- ✅ 3.4: Unix APIs guarded with `#[cfg(unix)]`
- ✅ 8.4: Linux-specific dependencies gated under `[target.'cfg(target_os = "linux")'.dependencies]`
- ✅ 8.5: Linux-specific APIs guarded with `#[cfg(target_os = "linux")]`

### Testing
To verify compilation on different targets:

```bash
# Linux (native)
cargo check -p agentd --lib

# macOS target (from Linux)
cargo check -p agentd --lib --target x86_64-apple-darwin

# Windows target (from Linux)
cargo check -p agentd --lib --target x86_64-pc-windows-msvc
```

### Notes
- The sandbox and vm_backend modules remain Linux-only by design
- On macOS and Windows, these features will be provided by VM launchers (future tasks)
- The stub implementations ensure clean compilation while providing clear error messages
- All Linux-specific kernel features (overlayfs, cgroups, namespaces, mount) are properly guarded
