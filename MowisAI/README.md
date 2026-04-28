# agentd — OS-level AI agent execution engine

agentd is a single Rust binary that runs thousands of isolated AI agents in parallel.
It uses overlayfs, chroot, and cgroups for sandboxing, with no cloud dependency.
Designed for European regulated enterprise environments (GDPR, DORA compliance).
Competitive gap: E2B and Daytona are cloud-only; CrewAI and LangGraph have no execution layer.

---

## Architecture

The orchestration system is a 7-layer pipeline. Each layer has a single responsibility.

**Layer 1 — Fast Planner**
Shell scan (find/tree) takes ~10ms, then a single Gemini call produces the full task graph
and sandbox topology JSON. No file reading at planning time — workers read files themselves.

**Layer 2 — Overlayfs Topology (3 levels)**
- Level 0: base layer — full repo, read-only, shared by all sandboxes (zero duplication)
- Level 1: sandbox layer — copy-on-write per sandbox, scoped filesystem view
- Level 2: agent layer — copy-on-write per agent on top of sandbox layer, fully isolated writes

On task completion the agent layer produces a clean git diff, then is discarded.
Checkpoints are snapshots of the agent CoW layer after each tool call.

**Layer 3 — Scheduler**
Event-driven task dispatcher. Maintains a DashMap<TaskId, AtomicUsize> of dependency counters.
When a task completes, counters on all dependents are decremented. Any task whose counter hits
zero is immediately dispatched to an idle agent in the correct sandbox. No batching.

**Layer 4 — Agent Execution with Checkpoints**
Each agent is a Gemini tool-calling loop running inside an isolated container via the agentd
socket. A checkpoint is saved after every tool call. On tool failure the agent rolls back to the
last checkpoint and retries (max 3). On agent crash a fresh agent is spawned with the checkpoint
log and continues from the last checkpoint. After 3 repeated failures the task escalates to human.

**Layer 5 — Parallel Merge (per sandbox)**
Tree-pattern merge: N diffs to N/2 merge workers in parallel, repeating until one result remains.
Total rounds: log2(N). LLM-assisted conflict repair on git apply failure.

**Layer 6 — Verification Loop (per sandbox)**
After sandbox work completes, a verification planner generates a test task graph (planned once,
not re-generated each round). Test agents run in the same sandbox against the merged result.
Failures produce fix tasks injected back into the scheduler. Maximum 3 verification rounds,
then the sandbox is marked PARTIALLY_VERIFIED.

**Layer 7 — Cross-Sandbox Merge + Final Output**
All verified sandbox diffs are merged by one cross-sandbox merge worker. Integration conflicts
(frontend calling a changed backend endpoint) are caught and LLM-repaired here.
Final output: clean tested merged codebase.

---

## Prerequisites

### Linux (Full Functionality)
- Linux (overlayfs and cgroups required — **primary supported platform**)
- Root access (overlayfs mount requires root)
- Rust stable toolchain (rustup install stable)
- gcloud CLI authenticated to GCP project company-internal-tools-490516
- Vertex AI Gemini 2.5 Pro enabled on the project

### macOS/Windows (Limited Functionality)
- Basic agent execution without sandboxing
- No overlayfs support (agents run in process isolation)
- Reduced security guarantees
- Some tools may not work as expected

**Note**: While agentd builds and runs on macOS and Windows, the full sandboxing and security features require Linux with overlayfs and cgroups support. For production use, Linux is strongly recommended.

Install gcloud and authenticate:

    gcloud auth application-default login
    gcloud config set project company-internal-tools-490516

---

## Installation

### Download Pre-built Binaries

Download the latest release for your platform from the [Releases page](https://github.com/mowisai/agentd/releases).

#### Linux
```bash
# Download for your architecture
curl -L -o agentd.tar.gz https://github.com/mowisai/agentd/releases/latest/download/agentd-v0.2.0-linux-x86_64.tar.gz

# Extract and install
tar -xzf agentd.tar.gz
chmod +x agentd runtime
sudo mv agentd runtime /usr/local/bin/

# Verify installation
agentd --version
```

#### macOS
```bash
# Intel Macs
curl -L -o agentd.tar.gz https://github.com/mowisai/agentd/releases/latest/download/agentd-v0.2.0-macos-x86_64.tar.gz

# Apple Silicon Macs
curl -L -o agentd.tar.gz https://github.com/mowisai/agentd/releases/latest/download/agentd-v0.2.0-macos-arm64.tar.gz

# Extract and install
tar -xzf agentd.tar.gz
chmod +x agentd runtime
sudo mv agentd runtime /usr/local/bin/

# Verify installation
agentd --version
```

#### Windows
```powershell
# Download and extract manually from releases page, or use PowerShell:
Invoke-WebRequest -Uri "https://github.com/mowisai/agentd/releases/latest/download/agentd-v0.2.0-windows-x86_64.zip" -OutFile "agentd.zip"
Expand-Archive -Path "agentd.zip" -DestinationPath "agentd"

# Add to PATH or run directly
cd agentd
.\agentd.exe --version
```

### Build from Source

    cargo build --release

The workspace produces two binaries: agentd (the daemon + CLI) and runtime (control plane).

---

## Run

Two terminals are required. The socket server must run as root for overlayfs.

**Terminal 1 — socket server (requires root)**

    sudo ./target/release/agentd socket --path /tmp/agentd.sock

**Terminal 2 — orchestrate a task**

    ./target/release/agentd orchestrate \
        --prompt "Implement JWT authentication for the REST API" \
        --project company-internal-tools-490516 \
        --socket /tmp/agentd.sock \
        --max-agents 1000

---

## Simulate (no LLM, zero cost)

The simulate command runs the entire 7-layer stack with mock agents. Use it for development,
debugging, and CI without incurring Gemini API costs.

    # Start socket server first (Terminal 1)
    sudo ./target/release/agentd socket --path /tmp/agentd.sock

    # Run simulation (Terminal 2)
    ./target/release/agentd simulate \
        --socket /tmp/agentd.sock \
        --project-root /path/to/your/project \
        --max-agents 100 \
        --tasks 200 \
        --verify

The simulate command emits machine-parseable lines to stdout on completion:

    SIMULATE_TOTAL_MS=12345
    SIMULATE_COMPLETED=200
    SIMULATE_FAILED=0

---

## Performance Gate

scripts/perf-gate.sh runs a 2000-task simulation with 1000 agents and enforces a 420-second
wall-clock budget. It also checks for EPIPE and EMFILE errors that indicate resource exhaustion.

    # Default budget: 420000ms (7 minutes)
    bash scripts/perf-gate.sh

    # Override budget
    BUDGET_MS=300000 bash scripts/perf-gate.sh

The script builds the release binary, raises the file descriptor limit to 65536, starts the
socket server under sudo, runs the simulation with --tasks 2000 --max-agents 1000 --verify,
checks for EPIPE/EMFILE in output, enforces the millisecond budget, and prints PASS or FAIL.

The client pool size (POOL_WORKERS) is derived from the server SLOW_WORKERS constant at
compile time (SLOW_WORKERS * 3 / 4), so the two never drift out of sync.

---

## Testing

    cargo test

67 tests must always pass. The invariant: never delete or modify a test to make it pass —
fix the implementation instead. Never stub or fake tool implementations.

Run a specific test:

    cargo test --package agentd test_socket_pool_bounded

---

## Code Layout

    MowisAI/
    ├── Cargo.toml                       Workspace root
    ├── agentd/                          Main daemon, library, orchestration
    │   └── src/
    │       ├── main.rs                  CLI entry point (orchestrate, simulate, socket)
    │       ├── lib.rs                   Library root, re-exports
    │       ├── socket_server.rs         Unix socket server, worker thread pools
    │       ├── sandbox.rs               Sandbox primitives, cgroup/chroot management
    │       ├── security.rs              Seccomp, threat analysis, security policies
    │       ├── vertex_agent.rs          Gemini/Vertex AI client, tool-calling loop
    │       ├── persistence.rs           WAL, checkpointing, recovery journal
    │       ├── hub_agent.rs             Hub agent for inter-team coordination
    │       ├── tools/                   75 tools across 14 categories
    │       └── orchestration/
    │           ├── mod.rs               Module exports, shared constants
    │           ├── types.rs             Core types: task graph, sandbox state, merge nodes
    │           ├── planner.rs           Layer 1: fast planner (shell scan + Gemini)
    │           ├── sandbox_topology.rs  Layer 2: CoW layer management
    │           ├── scheduler.rs         Layer 3: event-driven task dispatcher
    │           ├── agent_execution.rs   Layer 4: agent loop with checkpoints
    │           ├── checkpoint.rs        Checkpoint save/restore logic
    │           ├── merge_worker.rs      Layer 5: parallel tree-pattern merge
    │           ├── verification.rs      Layer 6: verification planner and test loop
    │           ├── new_orchestrator.rs  Layer 7: cross-sandbox merge, final output
    │           ├── simulate.rs          Simulation command (mock agents, zero cost)
    │           ├── mock_agent.rs        Deterministic mock agent for testing
    │           └── socket_client.rs     Bounded socket client pool
    ├── agentd-protocol/                 Shared protocol types (no circular deps)
    │   └── src/lib.rs                   TaskGraph, SandboxTopology, Checkpoint, AgentResult
    ├── runtime/                         Control plane — state management
    │   └── src/
    │       ├── runtime.rs               Sandbox/container lifecycle via agentd socket
    │       └── agentd_client.rs         Typed client for the agentd socket API
    └── scripts/
        └── perf-gate.sh                 Performance gate: 2000 tasks, 420s budget

---

## Hard Invariants

- Orchestrator-mediated coordination only. No direct agent-to-agent communication.
- Sandbox and container IDs are always String in JSON, never u64.
- Never delete or modify tests to make them pass. Fix the implementation.
- Never stub or fake tool implementations.
- All tools execute within container context via chroot.
- 67 tests must always pass.
- No unwrap() in production code paths.

---

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

Copyright (c) 2026 MowisAI
