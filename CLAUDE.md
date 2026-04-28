# MowisAI — agentd

## What this is
OS-level AI agent execution engine. Single Rust binary. Runs thousands of isolated agents in parallel using overlayfs/chroot sandboxing. Targeting European regulated enterprise (GDPR, DORA). Competitive gap: E2B/Daytona are cloud-only, CrewAI/LangGraph have no execution layer.

## ⚠️ CRITICAL: NEW ARCHITECTURE TO BUILD

**This document describes the NEW 7-layer orchestration system that REPLACES the old 5-layer pipeline.**

The old 5-layer system (Context Gatherer → Architect → Sandbox Owner → Sandbox Manager → Workers) is **DEPRECATED** and should be **DELETED**.

The new system is described below in "Architecture: new orchestration system".

## Workspace structure
```
MowisAI/
├── agentd/           # Main daemon, library, orchestration
├── agentd-protocol/  # Shared protocol types (separate crate, no circular deps)
└── runtime/          # Control plane — state management, delegates to agentd
```

## 🎯 CLAUDE CODE MISSION: BUILD THE NEW ORCHESTRATION SYSTEM

**Environment Context:**
- You are working on **Windows** locally
- **No Cargo available locally** — you cannot run `cargo build` or `cargo test`
- Your job is to **WRITE THE CODE** — the user will test and debug later on their Linux environment
- Focus on **correct Rust code** that follows the architecture below
- The user will handle compilation, testing, and debugging

**What You Need to Build:**

### Phase 1: Foundation (Start Here)
1. **Delete old orchestration files** (see "Files to Delete" below)
2. **Create new types** in `agentd-protocol/src/lib.rs` for task graph, sandbox topology, scheduler messages
3. **Implement Layer 2** (Overlayfs Topology) — 3-level CoW filesystem management
4. **Implement Layer 3** (Scheduler) — Event-driven task dispatcher with tokio

### Phase 2: Agent Execution
5. **Implement Layer 4** (Agent Execution with Checkpoints) — Tool-calling loop with checkpoint system
6. **Implement Layer 1** (Fast Planner) — Shell scan + single Gemini call

### Phase 3: Merge & Verify
7. **Implement Layer 5** (Parallel Merge) — Tree-pattern merge workers
8. **Implement Layer 6** (Verification Loop) — Test task injection

### Phase 4: Integration
9. **Implement Layer 7** (Cross-Sandbox Merge) — Final integration
10. **Wire everything together** in `orchestration/mod.rs`

---

## Architecture: new orchestration system (replacing 5-layer pipeline)

### Layer 1 — Fast planner
- Shell scan (`find`/`tree`) — no LLM, ~10ms
- ONE Gemini call: prompt + dir tree string → task graph JSON + sandbox topology
- Task graph: `{ id, description, deps[], hint }` per task
- Topology: decides how many sandboxes, which type, how many agents per sandbox
- Small project → 1 sandbox, up to 50 agents
- Large project → N sandboxes (frontend/backend/infra/etc), up to 100 agents each
- NO file reading at planning stage — workers read files themselves

**Output:**
```json
{
  "tasks": [
    { "id": "t1", "description": "implement auth", "deps": [], "hint": "backend" },
    { "id": "t2", "description": "implement API", "deps": ["t1"], "hint": "backend" },
    { "id": "t3", "description": "build UI", "deps": [], "hint": "frontend" }
  ],
  "topology": [
    { "name": "backend", "scope": "src/backend/", "tools": ["read_file", "write_file", "run_command"], "max_agents": 50 },
    { "name": "frontend", "scope": "src/frontend/", "tools": ["read_file", "write_file", "npm_install"], "max_agents": 50 }
  ]
}
```

### Layer 2 — Overlayfs topology (3 levels)
- Level 0: base layer — full repo, read-only, shared by ALL sandboxes (zero duplication)
- Level 1: sandbox layer — copy-on-write per sandbox, scoped filesystem view (frontend sandbox sees src/frontend only, etc — configurable)
- Level 2: agent layer — copy-on-write per agent on top of sandbox layer, fully isolated writes
- On task complete: agent layer produces clean git diff, layer is discarded
- Checkpoint = snapshot of agent's CoW layer after each tool call (`cp -al` upper dir or btrfs snapshot)

**Key Implementation:**
- Use `mount -t overlay` with proper lowerdir/upperdir/workdir
- Scoped sandboxes use bind mounts or overlayfs options to limit visibility
- Agent layers are ephemeral — created on task start, diffed on completion, discarded

### Layer 3 — Scheduler
- Maintains `HashMap<TaskId, AtomicUsize>` dep counters per task
- On task complete: decrements counter of all dependent tasks
- Any task whose counter hits 0 → immediately dispatched to idle agent in correct sandbox
- NO batching, NO group boundaries — pure event-driven dispatch
- tokio::sync::mpsc ready queue
- Sandbox-aware: frontend task → frontend agent only

**Key Implementation:**
```rust
// Pseudocode structure
struct Scheduler {
    task_graph: HashMap<TaskId, Task>,
    dep_counter: DashMap<TaskId, AtomicUsize>,  // DashMap for concurrent access
    ready_queue: mpsc::Sender<TaskId>,
    running: HashMap<TaskId, AgentHandle>,
    completed: HashSet<TaskId>,
    sandbox_queues: HashMap<SandboxName, VecDeque<IdleAgent>>,
}
```

### Layer 4 — Agent execution with checkpoints
- Agent = Gemini tool-calling loop inside isolated container via agentd socket
- Checkpoint saved after EVERY tool call (write_file, run_command, git_commit, etc.)
- On tool failure → rollback to last checkpoint, retry from there
- On agent crash → kill agent, spawn fresh agent, hand it checkpoint log, continue from last checkpoint
- On 3x repeated failure on same task → escalate to human with full checkpoint log
- Agents are sandbox-specialists: frontend agents have frontend tools + system prompt, backend agents have backend tools + system prompt
- Agents emit git diff on completion

**Checkpoint Format:**
```rust
struct Checkpoint {
    id: u64,  // monotonic counter
    tool_call: String,
    tool_args: serde_json::Value,
    tool_result: String,
    timestamp: u64,
    layer_snapshot_path: String,  // path to CoW layer snapshot
}
```

**Three-Tier Error Handling:**
- Tier 1 (Tool failure): Rollback to checkpoint, retry same tool (max 3 retries)
- Tier 2 (Agent crash): Spawn fresh agent, restore from checkpoint, continue
- Tier 3 (Repeated failure): Escalate to human with full checkpoint log

### Layer 5 — Parallel merge (per sandbox)
- Merge workers spin up dynamically when two branches need combining
- Tree-pattern merge: pairs of diffs merged in parallel, not serial queue
- LLM-assisted conflict repair on git apply failure
- Each sandbox produces one clean merged diff independently

**Merge Algorithm:**
```
Round 1: N diffs → N/2 merge workers (parallel)
Round 2: N/2 results → N/4 merge workers (parallel)
...
Until 1 result remains
Total rounds: log₂(N)
```

### Layer 6 — Verification loop (per sandbox)
- After sandbox work completes: verification planner generates test task graph
- Test agents run in same sandbox against merged result
- Test failures → new fix tasks injected back into scheduler
- Loop continues until all tests pass or max retries exceeded

**Verification Flow:**
1. Generate test task graph from merged diff
2. Inject test tasks into scheduler
3. Run tests in same sandbox (warm containers)
4. On failure: create fix tasks, inject into scheduler
5. After fix: re-run tests
6. Max 3 verification rounds, then mark PARTIALLY_VERIFIED

### Layer 7 — Cross-sandbox merge + final output
- All verified sandbox diffs merged by one cross-sandbox merge worker
- Integration conflicts (frontend calling changed backend endpoint) caught here
- LLM repair for integration conflicts
- Final output: clean tested merged codebase

---

## 📁 Files to Create (New Architecture)

Create these new files in `agentd/src/orchestration/`:

| File | Purpose |
|------|---------|
| `planner.rs` | Layer 1: Fast planner (shell scan + Gemini call) |
| `scheduler.rs` | Layer 3: Event-driven task dispatcher |
| `sandbox_topology.rs` | Layer 2: CoW layer management, agent spawning |
| `checkpoint.rs` | Layer 4: Checkpoint save/restore logic |
| `merge_worker.rs` | Layer 5: Parallel tree-pattern merge |
| `verification.rs` | Layer 6: Verification planner and test loop |
| `types.rs` | New types for task graph, topology, scheduler |
| `mod.rs` | New module exports |

## 🗑️ Files to Delete (Old Architecture)

**DELETE these files** — they are replaced by the new system:

- `agentd/src/orchestration/context_gatherer.rs` — replaced by planner.rs
- `agentd/src/orchestration/architect.rs` — replaced by planner.rs
- `agentd/src/orchestration/sandbox_owner.rs` — replaced by sandbox_topology.rs
- `agentd/src/orchestration/sandbox_manager.rs` — replaced by merge_worker.rs
- `agentd/src/orchestration/worker.rs` — replaced by sandbox_topology.rs (agent execution)
- `agentd/src/orchestration/planner.rs` (old) — replaced
- `agentd/src/orchestration/coordinator.rs` — no longer needed
- `agentd/src/orchestration/executor.rs` — replaced by scheduler.rs
- `agentd/src/orchestration/agent_runner.rs` — replaced

**KEEP these files** — they are still needed:

- `agentd/src/orchestration/session_store.rs` — for interactive sessions
- `agentd/src/orchestration/sandbox_profiles.rs` — for package/tool presets

---

## 🔌 agentd core (DO NOT CHANGE)

**CRITICAL:** The agentd binary and socket API are NOT modified by orchestration work. Orchestration communicates through the socket ONLY.

- Unix socket server at `/tmp/agentd.sock`
- overlayfs + chroot + cgroups sandboxing
- 75 tools across 14 categories
- Socket API: create_sandbox, create_container, invoke_tool, pause_container, resume_container, destroy_sandbox

## Hard invariants — never violate
- Orchestrator-mediated coordination ONLY. No direct agent-to-agent communication.
- Sandbox/container IDs always returned as String in JSON, never u64.
- Never delete or modify tests to make them pass. Fix the actual implementation.
- Never stub or fake tool implementations.
- All tools execute within container context via chroot.
- 67 tests must always pass — never regress.

## AI backend
- Vertex AI Gemini 2.5 Pro
- GCP project: `company-internal-tools-490516`
- Auth via gcloud — must be present in environment

## Running locally (User will do this, not Claude Code)

**Note:** Claude Code writes the code. The user will build and test on their Linux environment.

```bash
# User runs this on their Linux machine:
cargo build

# Terminal 1 (requires root for overlayfs)
sudo ./target/debug/agentd socket --path /tmp/agentd.sock

# Terminal 2
cargo run -- orchestrate --prompt "..." --project company-internal-tools-490516 --socket /tmp/agentd.sock --max-agents 1000
```

## Code conventions
- Rust stable, async via tokio, errors via anyhow
- No unwrap() in production paths
- JSON serialization via serde
- IDs always String, never u64 in JSON responses
- Full updated code when making changes — never partial without request
- When tests fail: read actual source before suggesting fix. Never guess.

## 🚀 Implementation Order for Claude Code

### Step 1: Setup
1. Read existing code to understand current structure
2. Delete old orchestration files listed above
3. Update `agentd-protocol/src/lib.rs` with new types

### Step 2: Core Types
Define in `agentd-protocol/src/lib.rs`:
- `TaskGraph`, `Task`, `TaskId`
- `SandboxTopology`, `SandboxConfig`
- `SchedulerMessage`, `TaskCompletion`
- `Checkpoint`, `CheckpointLog`
- `AgentResult`, `SandboxResult`

### Step 3: Layer 2 + Layer 3 (Foundation)
- Implement `sandbox_topology.rs` — CoW layer management
- Implement `scheduler.rs` — event-driven dispatch
- These two layers are the foundation — get them right first

### Step 4: Layer 4 (Agent Execution)
- Implement checkpoint system in `checkpoint.rs`
- Integrate with agent execution loop

### Step 5: Layer 1 (Fast Planner)
- Implement `planner.rs` — shell scan + Gemini call

### Step 6: Layers 5-7 (Merge & Verify)
- Implement `merge_worker.rs` — tree-pattern merge
- Implement `verification.rs` — test loop
- Implement cross-sandbox merge

### Step 7: Integration
- Wire everything in `orchestration/mod.rs`
- Update `main.rs` CLI to use new orchestration
- Ensure all 67 tests still pass (user will verify)

---

## 📋 Summary Checklist for Claude Code

- [ ] Delete old orchestration files
- [ ] Create new types in agentd-protocol
- [ ] Implement Layer 2 (Overlayfs Topology)
- [ ] Implement Layer 3 (Scheduler)
- [ ] Implement Layer 4 (Agent Execution with Checkpoints)
- [ ] Implement Layer 1 (Fast Planner)
- [ ] Implement Layer 5 (Parallel Merge)
- [ ] Implement Layer 6 (Verification Loop)
- [ ] Implement Layer 7 (Cross-Sandbox Merge)
- [ ] Wire everything together
- [ ] Ensure no unwrap() in production paths
- [ ] Use String for all IDs in JSON
- [ ] Follow Rust stable + tokio + anyhow conventions
