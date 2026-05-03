# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is MowisAI agentd?

OS-level AI agent execution engine. Single Rust binary that runs thousands of isolated AI agents in parallel using overlayfs/chroot sandboxing. Targets European regulated enterprise (GDPR, DORA compliance). Competitive differentiation: E2B/Daytona are cloud-only; CrewAI/LangGraph lack an execution layer.

## Project Structure

```
MowisAI/
├── agentd/           # Main daemon, CLI, orchestration engine
├── agentd-protocol/  # Shared protocol types (separate crate, no circular deps)
├── runtime/          # Control plane — state management, delegates to agentd
└── mowis-desktop/    # Tauri-based desktop application (separate product)
```

The workspace produces two binaries: `agentd` (daemon + CLI) and `runtime` (control plane).

## Build and Test Commands

**Build:**
```bash
cargo build --release
```

**Run tests (must always pass — 67 tests):**
```bash
cargo test
```

**Run a specific test:**
```bash
cargo test --package agentd test_socket_pool_bounded
```

**Performance gate (2000 tasks, 1000 agents, 420s budget):**
```bash
bash scripts/perf-gate.sh

# Override budget
BUDGET_MS=300000 bash scripts/perf-gate.sh
```

## Running agentd Locally

Requires two terminals. Socket server requires root for overlayfs.

**Terminal 1 — Start socket server:**
```bash
sudo ./target/release/agentd socket --path /tmp/agentd.sock
```

**Terminal 2 — Run orchestration:**
```bash
./target/release/agentd orchestrate \
    --prompt "Implement JWT authentication for the REST API" \
    --project company-internal-tools-490516 \
    --socket /tmp/agentd.sock \
    --max-agents 1000
```

**Simulation mode (no LLM calls, zero cost):**
```bash
# Terminal 1: same socket server as above

# Terminal 2:
./target/release/agentd simulate \
    --socket /tmp/agentd.sock \
    --project-root /path/to/project \
    --max-agents 100 \
    --tasks 200 \
    --verify
```

## Architecture: 7-Layer Orchestration System

1. **Fast Planner** (`planner.rs`) — Shell scan + single Gemini call → task graph JSON + sandbox topology
2. **Overlayfs Topology** (`sandbox_topology.rs`) — 3-level CoW filesystem (base/sandbox/agent layers)
3. **Scheduler** (`scheduler.rs`) — Event-driven task dispatch with dependency tracking
4. **Agent Execution** (`agent_execution.rs`, `checkpoint.rs`) — Gemini tool-calling loop with checkpoints after every tool call
5. **Parallel Merge** (`merge_worker.rs`) — Tree-pattern merge (log₂(N) rounds), LLM-assisted conflict repair
6. **Verification Loop** (`verification.rs`) — Test task graph, inject fix tasks on failure, max 3 rounds
7. **Cross-Sandbox Merge** (`new_orchestrator.rs`) — Final integration, handles cross-boundary conflicts

### Key Concepts

**3-Level CoW Filesystem:**
- Level 0: base layer (full repo, read-only, shared by ALL sandboxes)
- Level 1: sandbox layer (CoW per sandbox, scoped filesystem view)
- Level 2: agent layer (CoW per agent, fully isolated writes)

**Checkpoint System:**
Saved after EVERY tool call. Three-tier error handling:
- Tier 1 (Tool failure): Rollback to checkpoint, retry (max 3)
- Tier 2 (Agent crash): Spawn fresh agent, restore from checkpoint
- Tier 3 (Repeated failure): Escalate to human with full log

**Socket API:**
Orchestration communicates with agentd core via Unix socket at `/tmp/agentd.sock`. Available operations: `create_sandbox`, `create_container`, `invoke_tool`, `pause_container`, `resume_container`, `destroy_sandbox`.

## Key Files

- `agentd/src/main.rs` — CLI entry point (orchestrate, simulate, socket commands)
- `agentd/src/socket_server.rs` — Unix socket server, worker thread pools (SLOW_WORKERS constant)
- `agentd/src/sandbox.rs` — Sandbox primitives, cgroup/chroot management
- `agentd/src/vertex_agent.rs` — Gemini/Vertex AI client, tool-calling loop
- `agentd/src/persistence.rs` — WAL, checkpointing, recovery journal
- `agentd/src/orchestration/` — All 7 orchestration layers
- `agentd/src/tools/` — 75 tools across 14 categories (filesystem, git, docker, k8s, etc)
- `agentd-protocol/src/lib.rs` — Shared types (TaskGraph, SandboxTopology, Checkpoint, AgentResult)
- `runtime/src/agentd_client.rs` — Typed client for agentd socket API
- `scripts/perf-gate.sh` — Performance gate script

## Hard Invariants (Never Violate)

- **No direct agent-to-agent communication** — orchestrator-mediated coordination only
- **IDs are always String in JSON**, never u64
- **Never delete or modify tests to make them pass** — fix the implementation
- **Never stub or fake tool implementations** — all tools execute in container via chroot
- **67 tests must always pass** — never regress
- **No `unwrap()` in production code paths** — use `?` or proper error handling
- **agentd core (socket API) is immutable** — orchestration communicates via socket only
- **Always read stdout/stderr concurrently** — sequential pipe reading causes deadlock when buffers fill

## AI Backend

- **Model:** Vertex AI Gemini 2.5 Pro
- **GCP Project:** `company-internal-tools-490516`
- **Auth:** gcloud CLI must be authenticated (`gcloud auth application-default login`)

## Code Conventions

- **Language:** Rust stable
- **Async:** tokio
- **Error handling:** anyhow for applications, thiserror for libraries
- **Serialization:** serde + serde_json
- **When making changes:** Provide full updated code, not partial diffs (unless explicitly requested)
- **When tests fail:** Read actual source before suggesting fixes — never guess

## Performance Constraints

- **Performance gate:** 2000 tasks with 1000 agents must complete in <420 seconds
- **Client pool sizing:** Derived from `SLOW_WORKERS` constant at compile time (`SLOW_WORKERS * 3 / 4`)
- **File descriptor limit:** Raised to 65536 during perf gate
- **No EPIPE/EMFILE errors** — indicates resource exhaustion

## Platform Support

- **Linux (primary):** Full functionality with overlayfs/cgroups sandboxing (requires root)
- **macOS/Windows (limited):** Basic agent execution without sandboxing, reduced security

## Desktop Application (mowis-desktop)

Separate Tauri-based desktop app. Lives in `mowis-desktop/` workspace member. Uses `src-tauri/` for Rust backend.

## Cross-Crate Dependencies

```
agentd → agentd-protocol
agentd → runtime
runtime → agentd-protocol
```

agentd-protocol is the shared types crate with no circular dependencies.

## Known Issues and Fixes

### QEMU Serial Bridge Timeout on Windows

**Symptom:** "Timed out waiting for agentd serial bridge on 127.0.0.1:9722" after QEMU process spawns.

**Root Causes:**
1. **WHPX not enabled** — Windows Hypervisor Platform must be enabled in Windows Features
2. **Corrupted Alpine image** — The qcow2 disk image may be incomplete or corrupted
3. **agentd not starting** — The agentd service inside Alpine may be failing
4. **Serial port misconfiguration** — virtio-serial device not properly configured

**Troubleshooting:**
1. Enable debug logging: Set environment variable `MOWIS_DEBUG=1` before running
2. Check WHPX: `Get-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform` (PowerShell)
3. Enable if needed: `Enable-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform` (requires reboot)
4. Verify Alpine image exists and is valid qcow2 format
5. Check Windows Event Viewer → Application logs for Hyper-V errors
6. Try Developer Mode for automatic Alpine bootstrap

**Debug Logging:**
QEMU output is normally redirected to null for performance. To see QEMU boot logs, either:
- Build in debug mode (`cargo build` instead of `cargo build --release`)
- Set environment variable `MOWIS_DEBUG=1` before launching

### Pipe Deadlock During Image Setup (FIXED)
**Symptom:** Orchestration hangs at "Initializing..." during first sandbox creation, never completes.

**Root Cause:** `chroot_run_streaming()` in `socket_server.rs` was reading stdout and stderr **sequentially**. When the child process writes to stderr before finishing stdout, stderr's buffer fills up (typically 64KB), the child blocks waiting for someone to read it, but the parent is still blocked waiting for more stdout. Classic pipe deadlock.

**Fix:** Read both streams **concurrently** using separate threads. Never read process output streams sequentially — always use threads or async I/O to prevent buffer deadlock.

**Code Pattern to Avoid:**
```rust
// BAD - Sequential reading causes deadlock
if let Some(stdout) = child.stdout.take() {
    for line in BufReader::new(stdout).lines() { /* ... */ }
}
if let Some(stderr) = child.stderr.take() {
    for line in BufReader::new(stderr).lines() { /* ... */ }
}
```

**Correct Pattern:**
```rust
// GOOD - Concurrent reading prevents deadlock
let stdout_thread = child.stdout.take().map(|stdout| {
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().flatten() { /* ... */ }
    })
});
let stderr_thread = child.stderr.take().map(|stderr| {
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().flatten() { /* ... */ }
    })
});
if let Some(t) = stdout_thread { let _ = t.join(); }
if let Some(t) = stderr_thread { let _ = t.join(); }
```
