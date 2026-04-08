# MowisAI New 7-Layer Orchestration System

## Overview

This document describes the **new 7-layer orchestration architecture** that replaces the old 5-layer sequential pipeline.

The new system is designed to scale to 1000+ parallel agents with:
- **Fast startup** (seconds, not minutes)
- **Event-driven task dispatch** (tasks fire instantly when dependencies complete)
- **Checkpoint-first reliability** (every tool call is checkpointed)
- **Parallel merge** (tree-pattern, not serial bottleneck)
- **Verification loop** (test failures auto-inject fix tasks)

---

## Architecture Layers

| Layer | Name | Responsibility |
|-------|------|----------------|
| 1 | **Fast Planner** | Shell scan + 1 LLM call → task graph + sandbox topology |
| 2 | **Overlayfs Topology** | 3-level CoW filesystem: base → sandbox → agent |
| 3 | **Scheduler** | Event-driven dispatch, fires tasks on dep completion |
| 4 | **Agent Execution** | Gemini tool loop + checkpoint after every tool call |
| 5 | **Parallel Merge** | Tree-pattern merge within sandbox, LLM conflict repair |
| 6 | **Verification Loop** | Test task graph per sandbox, failures re-enter scheduler |
| 7 | **Cross-Sandbox Merge** | Final integration merge, integration conflict repair |

---

## Implementation Status

### ✅ Completed (Phase 1-3)

- **Layer 1: Fast Planner** (`planner.rs`)
  - Shell-based directory scanning
  - Single Gemini call for task graph + topology
  - JSON parsing and validation

- **Layer 2: Overlayfs Topology** (`sandbox_topology.rs`)
  - 3-level CoW layer management (base/sandbox/agent)
  - Agent layer creation and destruction
  - Git diff capture
  - Checkpoint snapshot integration

- **Layer 3: Scheduler** (`scheduler.rs`)
  - Event-driven task dispatch
  - Dependency counter management (DashMap + AtomicUsize)
  - Sandbox-aware task routing
  - Ready queue (tokio::mpsc)
  - Idle agent pool per sandbox

- **Layer 4: Agent Execution** (`agent_execution.rs`)
  - Gemini tool-calling loop
  - Checkpoint system integration
  - 3-tier error handling:
    - Tier 1: Tool failure (retry)
    - Tier 2: Agent crash (recover from checkpoint)
    - Tier 3: Repeated failure (escalate)

- **Layer 4: Checkpoint System** (`checkpoint.rs`)
  - Checkpoint log (JSON file + in-memory)
  - Snapshot creation (cp -al on Linux, recursive copy on Windows)
  - Snapshot restoration
  - Automatic pruning (keep last 10)

- **Layer 5: Parallel Merge** (`merge_worker.rs`)
  - Tree-pattern merge (log2(N) rounds)
  - LLM-based conflict repair
  - Concurrent merge workers (tokio spawn)

- **Layer 6: Verification** (`verification.rs`)
  - Test task generation from sandbox results
  - Fix task generation from test failures
  - Verification loop controller (max rounds)

- **Layer 7: Integration** (`new_orchestrator.rs`)
  - Main orchestration coordinator
  - Wires all layers together
  - Final output generation
  - CLI integration

---

## File Structure

```
agentd/src/orchestration/
├── types.rs                 # Type definitions (new + deprecated legacy)
├── planner.rs              # Layer 1: Fast Planner
├── sandbox_topology.rs     # Layer 2: Overlayfs Topology
├── scheduler.rs            # Layer 3: Event-driven Scheduler
├── checkpoint.rs           # Layer 4: Checkpoint system
├── agent_execution.rs      # Layer 4: Agent execution loop
├── merge_worker.rs         # Layer 5: Parallel merge
├── verification.rs         # Layer 6: Verification loop
├── new_orchestrator.rs     # Layer 7: Main coordinator
├── mod.rs                  # Module exports
│
├── session_store.rs        # KEPT: Session persistence
├── sandbox_profiles.rs     # KEPT: Tool/package presets
│
└── [DEPRECATED STUBS]
    ├── architect.rs
    ├── context_gatherer.rs
    ├── coordinator.rs
    ├── sandbox_manager.rs
    ├── sandbox_owner.rs
    └── orchestrator.rs      # Old 5-layer system (to be removed)
```

---

## Usage

### Prerequisites

1. **Linux system** (overlayfs, chroot, mount required)
2. **Root access** (for overlayfs mounts)
3. **gcloud authenticated** (for Vertex AI Gemini)
4. **agentd socket server running**

### Start agentd Socket Server

Terminal 1:
```bash
sudo ./target/debug/agentd socket --path /tmp/agentd.sock
```

### Run New Orchestration

Terminal 2:
```bash
cargo run -- orchestrate-new \
  --prompt "implement user authentication with JWT tokens" \
  --project company-internal-tools-490516 \
  --socket /tmp/agentd.sock \
  --project-root /path/to/your/project \
  --overlay-root /tmp/mowis-overlay \
  --checkpoint-root /tmp/mowis-checkpoints \
  --merge-work-dir /tmp/mowis-merge \
  --max-agents 1000 \
  --max-verification-rounds 3
```

### Example Output

```
🚀 MowisAI — New 7-Layer Orchestration System
═══════════════════════════════════════════════

Layer 1: Planning tasks...
  → Generated 12 tasks across 3 sandboxes

Layer 2: Creating sandbox topology...
  → Created sandbox: backend
  → Created sandbox: frontend
  → Created sandbox: testing

Layer 3: Initializing scheduler...
  → Scheduler ready with 12 tasks

Layer 4: Executing tasks with agents...
  → Executing tasks in sandbox: backend
    ✓ Completed: implement JWT auth module
    ✓ Completed: add login endpoint
  → Executing tasks in sandbox: frontend
    ✓ Completed: build login form
  → Completed: 12/12 tasks

Layer 5: Merging agent results per sandbox...
  → Merging 4 diffs for sandbox: backend
    ✓ Merged with 2 conflicts resolved
  → Merging 3 diffs for sandbox: frontend
    ✓ Merged with 0 conflicts resolved

Layer 6: Verifying sandbox results...
  → Verifying sandbox: backend
    ✓ Verification: Passed (1 rounds)
  → Verifying sandbox: frontend
    ✓ Verification: Passed (1 rounds)

Layer 7: Final cross-sandbox merge...
  → Merging 2 sandbox results

✓ Orchestration complete!
  Total duration: 45s
  Agents used: 12
  Tasks completed: 12/12

═══════════════════════════════════════════════
📊 FINAL RESULTS
═══════════════════════════════════════════════

Summary: Completed 12/12 tasks using 12 agents in 45s. 0 failed.

Scheduler Stats:
  Total tasks: 12
  Completed: 12
  Failed: 0
  Running: 0
  Pending: 0

Sandbox Results:
  backend - Passed
  frontend - Passed
  testing - Passed

✅ Orchestration complete!
```

---

## Configuration

### OrchestratorConfig

```rust
pub struct OrchestratorConfig {
    pub project_id: String,           // GCP project ID
    pub socket_path: String,           // agentd socket path
    pub project_root: PathBuf,         // Project root directory
    pub overlay_root: PathBuf,         // Overlayfs mount root
    pub checkpoint_root: PathBuf,      // Checkpoint storage
    pub merge_work_dir: PathBuf,       // Merge workspace
    pub max_agents: usize,             // Max agents (default: 1000)
    pub max_verification_rounds: usize, // Max verification rounds (default: 3)
}
```

### Default Paths

- **overlay_root**: `/tmp/mowis-overlay`
- **checkpoint_root**: `/tmp/mowis-checkpoints`
- **merge_work_dir**: `/tmp/mowis-merge`

---

## Performance Comparison

| Metric | Old 5-Layer Pipeline | New 7-Layer System |
|--------|---------------------|-------------------|
| LLM calls before first worker | 3+ (up to 128 rounds) | 1 |
| Time to first worker | 2-5 minutes | 1-3 seconds |
| Merge parallelism | 1 (serial bottleneck) | log2(N) rounds |
| Checkpoint support | None | Every tool call |
| Verification | None | Built-in with re-injection |
| Scale target | ~50 agents | 1000+ agents |

---

## Testing

The new orchestration system includes comprehensive tests:

```bash
# Run all orchestration tests
cargo test --package agentd orchestration

# Run specific layer tests
cargo test --package agentd planner
cargo test --package agentd scheduler
cargo test --package agentd checkpoint
cargo test --package agentd merge_worker
```

---

## Troubleshooting

### Overlayfs Mount Failures

**Error**: `Failed to mount overlayfs`

**Solution**:
- Ensure running with root privileges (`sudo`)
- Check that overlayfs kernel module is loaded: `lsmod | grep overlay`
- Verify mount directories exist and have correct permissions

### Checkpoint Creation Failures

**Error**: `Failed to create checkpoint snapshot`

**Solution**:
- Ensure sufficient disk space in checkpoint_root
- Check directory permissions
- On Linux, verify `cp` command is available

### Gemini API Errors

**Error**: `Gemini API error: 401 Unauthorized`

**Solution**:
- Run `gcloud auth print-access-token` to verify authentication
- Ensure correct GCP project ID
- Check GCP project has Vertex AI API enabled

### Agent Execution Timeouts

**Error**: `Max tool rounds exceeded`

**Solution**:
- Increase `MAX_TOOL_ROUNDS` in `orchestration/mod.rs`
- Check task descriptions are clear and actionable
- Verify agent has access to required tools

---

## Architecture Decisions

### Why 3-Level CoW Layers?

- **Level 0 (Base)**: Shared read-only repo — zero duplication across all agents
- **Level 1 (Sandbox)**: Sandbox-scoped writes — isolation between domains
- **Level 2 (Agent)**: Agent-isolated writes — complete isolation, clean diffs

This enables 1000 agents to share a single base repo with minimal disk overhead.

### Why Event-Driven Scheduler?

Sequential batch processing creates artificial delays. Event-driven dispatch fires tasks **instantly** when dependencies complete, maximizing parallelism.

### Why Tree-Pattern Merge?

Serial merge is a bottleneck. Tree-pattern reduces N serial merges to log2(N) parallel rounds:
- 100 agents: 7 rounds instead of 100 serial merges
- 1000 agents: 10 rounds instead of 1000 serial merges

### Why Checkpoint Every Tool Call?

Tool calls are the atomic unit of work. Checkpointing after each call ensures:
- No work is lost on agent crash
- Rollback is granular and precise
- Recovery continues from exact failure point

---

## Future Enhancements

### Planned

1. **Dynamic agent scaling**: Spawn agents on-demand based on ready queue depth
2. **Resource-aware scheduling**: Consider RAM/CPU constraints when dispatching
3. **Incremental verification**: Only re-run affected tests after fixes
4. **Cross-sandbox communication**: Allow sandboxes to query each other via orchestrator

### Under Consideration

1. **Distributed execution**: Spread agents across multiple machines
2. **Persistent checkpoint storage**: S3/object storage for checkpoint logs
3. **Real-time progress streaming**: WebSocket API for live orchestration status
4. **LLM model selection**: Per-sandbox model choice (fast vs. smart)

---

## Migration from Old System

To migrate from the old 5-layer system:

1. Use the new CLI command: `orchestrate-new` instead of `orchestrate`
2. Update any automation scripts to use new parameter names
3. The old system remains available for backward compatibility
4. Once validated, the old system files will be removed

---

## Contact

For questions or issues with the new orchestration system:
- GitHub Issues: [mowisai/agentd/issues](https://github.com/mowisai/agentd/issues)
- Email: engineering@mowis.ai

---

**Last Updated**: 2026-04-02
**Version**: 1.0.0
**Status**: Production Ready
