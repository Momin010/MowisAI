# 🔥 BRUTAL AUDIT: MowisAI Codebase — The Horrifying Truth

> **Status**: Not production-ready. Not beta-ready. Not even alpha-ready.  
> **1000-agent parallel execution goal**: Currently hard-capped at 50.  
> **Conclusion**: This is a sophisticated prototype with massive gaps between theory and working product.

---

## 1. 🔴 CRITICAL — PRODUCTION BLOCKERS

### 1.1 CLI Is Completely Broken — Fire-and-Forget Garbage
**File**: `agentd/src/main.rs` (lines 152-448)

The CLI has NO interactivity. It's a disgrace:

```rust
// Current usage - pathetic:
agentd orchestrate-new --prompt "build api" --project x --socket /tmp/agentd.sock
// Then wait... and wait... with ZERO visibility
```

**What's Missing**:
- No REPL session management
- No real-time agent status view
- No ability to "click into" an agent to see what it's doing
- No parallel execution visualization (shows sequential output despite parallel execution)
- No pause/resume of the orchestration itself
- No way to send follow-up commands to running agents
- No session persistence between CLI invocations

**The OrchestrateInteractive Command is DEPRECATED** (lines 390-402):
```rust
Commands::OrchestrateInteractive { ... } => {
    eprintln!("❌ ERROR: The old OrchestrateInteractive command is deprecated.");
    eprintln!("   The new 7-layer orchestration system doesn't support interactive mode yet.");
    std::process::exit(1);
}
```

**What the User Wants**:
```
$ mowisai
> Build a web app with auth
┌─────────────────────────────────────────┐
│ MowisAI Top-Level Manager             │
│ Planning... ✓                           │
│ Dispatching to 4 sandboxes...         │
│                                         │
│ ⏵ backend-sandbox   [3 agents active]   │
│ ⏵ frontend-sandbox  [2 agents active]   │
│ ⏵ testing-sandbox   [idle]              │
│ ⏵ infra-sandbox     [1 agent active]    │
│                                         │
│ Press [1-4] to inspect, [q] to quit     │
└─────────────────────────────────────────┘

> 1  [enters backend sandbox view]
┌─────────────────────────────────────────┐
│ Backend Sandbox — 3 Agents              │
│                                         │
│ Jake    ⏵ Working on auth middleware    │
│         read_file: src/auth.rs          │
│         write_file: src/auth.rs (847b)  │
│                                         │
│ Mike    ⏵ Database schema               │
│ Sarah   ⏵ API routes                  │
└─────────────────────────────────────────┘
```

**Verdict**: The CLI is a toy. Competitors (Claude Code, OpenAI Codex CLI) have rich TUI interfaces. This is embarrassing.

---

### 1.2 Verification Layer is Completely Stubbed — Fake Feature
**File**: `agentd/src/orchestration/verification.rs` (lines 262-314)

The entire Layer 6 verification loop is **FAKE**:

```rust
// Line 282-297 — This is the ENTIRE verification "implementation":
// Verification rounds
for round in 0..self.max_rounds {
    rounds_completed = round + 1;

    // In production, test tasks would be injected into scheduler and executed
    // For now, we'll simulate test execution
    // This is a placeholder - actual implementation would:
    // 1. Inject test tasks into scheduler
    // 2. Wait for completion
    // 3. Collect results
    // 4. Generate fix tasks for failures
    // 5. Inject fix tasks
    // 6. Re-run tests

    // Simulate: all tests pass on first round
    for task in &plan.test_tasks.tasks {
        passed_tests.push(task.id.clone());
    }

    break; // Exit after first successful round
}
```

**Translation**: "We wrote the architecture doc but didn't implement it. Just pretend everything passes."

**Impact**: Agents can produce broken code, and the system will happily mark it "verified" and merge it.

---

### 1.3 Container Sleep/Wake Exists But Isn't Used
**File**: `runtime/src/runtime.rs` (lines 224-298)

The Runtime has `pause_container()` and `resume_container()` methods. They call agentd to freeze containers with SIGSTOP.

**But in `new_orchestrator.rs` (lines 113-255), containers are DESTROYED immediately after task completion**:

```rust
// Line 231-233:
// Cleanup agent layer
let _ = topology_clone.destroy_agent_layer(&agent.agent_id).await;
```

There's no container pool, no sleep/wake cycle, no keeping containers warm for the next command. The "OrchestrateInteractive" that was supposed to do this is deprecated.

**Result**: Every new task = new container creation overhead = slow = expensive.

---

### 1.4 Hard-Capped at 50 Agents — "1000 Agents" is a Lie
**File**: `agentd/src/orchestration/new_orchestrator.rs` (line 113)

```rust
let max_concurrent_agents = self.config.max_agents.min(50); // Cap at 50 concurrent agents
```

The user wants 1000 parallel agents. The code silently caps at 50. No explanation, no scaling logic, no resource-based dynamic adjustment.

**Where's the bottleneck?**
- Socket server? (`socket_server.rs` lines 19-20: `FAST_WORKERS=16, SLOW_WORKERS=32`)
- Gemini API rate limits? Not handled
- File descriptor limits? Not handled
- Memory pressure? Not handled

---

### 1.5 No Search/Grep Tool — Agents Are Blind
**File**: `agentd/src/tools/filesystem.rs` (lines 1-266)

The filesystem tools are:
- `read_file` — Read a specific file
- `write_file` — Write a file
- `append_file` — Append to file
- `delete_file` — Delete file
- `copy_file` — Copy file
- `move_file` — Move file
- `list_files` — List directory
- `create_directory` — Create directory
- `delete_directory` — Delete directory
- `get_file_info` — Get metadata
- `file_exists` — Check existence

**What's Missing**: 
- `grep` — Search for patterns across files
- `find` — Find files by name/pattern
- `search_code` — Semantic code search
- `read_multiple_files` — Batch read

**Impact**: When an agent hits an error, it can't search the codebase to find where to fix. It has to guess file paths. This is crippling for real debugging.

**Competitor comparison**: Claude Code has `@` search, glob patterns, regex search. This is basic functionality.

---

### 1.6 Parallel Execution Looks Sequential — UI Deception
**File**: `agentd/src/orchestration/new_orchestrator.rs` (lines 121-255)

The worker loop spawns 50 tokio tasks (real parallelism), but the output is printed sequentially:

```rust
// Line 234:
println!("    ✓ [Worker {}] Completed: {}", worker_id, task_description);
```

Because workers are in a loop printing to stdout, the output appears sequential even though execution is parallel. Users can't see the actual parallel activity.

**What the user sees**:
```
✓ [Worker 0] Completed: Task 1
✓ [Worker 1] Completed: Task 2
✓ [Worker 2] Completed: Task 3
// Looks sequential!
```

**What they should see**:
```
┌─────────────────────────────────┐
│ Parallel Execution Monitor      │
│                                 │
│ Worker 0 ⏵ Task 1 [12s]        │
│ Worker 1 ⏵ Task 2 [10s]        │
│ Worker 2 ⏵ Task 3 [11s]        │
│ ...                             │
└─────────────────────────────────┘
```

---

## 2. 🟠 MAJOR — FUNCTIONAL GAPS

### 2.1 Session Persistence Exists But Isn't Integrated
**File**: `agentd/src/orchestration/session_store.rs`

Session snapshots ARE implemented:
```rust
pub struct InteractiveSessionSnapshot {
    pub schema_version: u32,
    pub project_id: String,
    pub socket_path: String,
    pub max_agents: usize,
    pub context: ProjectContext,
    pub transcript: Vec<String>,
    pub sandbox_by_team: HashMap<String, String>,
    pub warm_by_sandbox: HashMap<String, SandboxWarmState>,
    pub assistant_turns: Vec<String>,
}
```

**But there's NO WAY to use it**:
- No `--resume` flag in the working command
- No automatic session saving on Ctrl+C
- No session listing/management
- The `OrchestrateInteractive` that had `--session-file` and `--resume` is deprecated

---

### 2.2 Checkpoint System Is Half-Implemented
**File**: `agentd/src/orchestration/checkpoint.rs`

Checkpoints save after every tool call. But:

```rust
// Line 103-107 in agent_execution.rs:
// Tier 2: Agent crash recovery
// NOTE: Checkpoint restoration requires agentd API support
// For now, we just log the attempt and continue
if let Some(_last_checkpoint) = checkpoint_log.latest() {
    eprintln!("Note: Checkpoint restoration not yet implemented (requires agentd support)");
}
```

So checkpoints are **written** but never **restored**. If an agent crashes, it starts from scratch. The tiered retry system (lines 76-120 in `agent_execution.rs`) doesn't actually use checkpoints.

---

### 2.3 Sandbox Topology Has Fake Scopes
**File**: `agentd/src/orchestration/sandbox_topology.rs` (lines 254-260)

The "scope" feature (limiting what files a sandbox can see) is broken:

```rust
let workspace_root = PathBuf::from(format!("/tmp/container-{}/root/workspace", container_id));
let project_base = if scope.trim().is_empty() || scope == "/" {
    self.project_root.clone()
} else {
    self.project_root.join(scope.trim_matches('/'))
};
```

**The problem**: The container workspace path is hardcoded to `/tmp/container-{id}/root/workspace`. The "scope" is only used for diff calculation against the project base. **The container can still SEE all files** — there's no actual filesystem isolation by scope.

**What it claims**: "frontend sandbox can only see src/frontend/"  
**What it does**: "All sandboxes see everything, diff just shows scoped changes"

---

### 2.4 Merge Reviewer Is Dumb — No Semantic Analysis
**File**: `agentd/src/orchestration/merge_worker.rs`

The merge system uses `git apply` + LLM repair on conflicts. It does NOT:

- Parse the diffs to understand what changed
- Check for semantic conflicts (e.g., auth middleware vs public routes)
- Verify import resolution after merge
- Run any validation on the merged result
- Review the merge quality

It just applies patches until git stops complaining. This is dangerous for production code.

---

### 2.5 No Health Monitoring / Circuit Breakers
**File**: `runtime/src/runtime.rs` (lines 300+)

The Runtime has `get_health_status()` but it's never called. There's no:
- Agent heartbeat monitoring
- Automatic restart of stuck agents
- Circuit breaker for failing sandboxes
- Resource pressure detection
- Graceful degradation

---

### 2.6 Error Handling is Primitive
Throughout the codebase:

```rust
// Example from new_orchestrator.rs:
let result = match executor_clone
    .execute_task(&agent, &task_description, &sandbox.tools, &system_prompt)
    .await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[Worker {}] Task execution failed: {}", worker_id, e);
            continue;  // Just skip it! Don't report to user!
        }
    };
```

Errors are logged to stderr and swallowed. The user has no visibility into:
- Which agents failed and why
- Retry attempts
- System-level failures
- API errors (Gemini rate limits, etc.)

---

## 3. 🟡 MINOR — POLISH & OPTIMIZATION

### 3.1 No Progress Indicators
The system prints lines like "Layer 4: Executing tasks with agents..." but no:
- Progress bars
- ETA calculations
- Real-time stats (tasks/minute, etc.)
- Agent efficiency metrics

### 3.2 No Log Levels / Verbosity Control
While there's a `--verbose` flag, the logging is all-or-nothing. No:
- Structured JSON logging
- Log levels (DEBUG, INFO, WARN, ERROR)
- Per-component log control
- Log rotation

### 3.3 Hardcoded Timeouts
**File**: `agentd/src/orchestration/mod.rs` (lines 25-37)

```rust
pub(crate) const HTTP_TIMEOUT_SECS: u64 = 900;  // 15 minutes
pub(crate) const MAX_TOOL_ROUNDS: usize = 256;
pub(crate) const MAX_CONTEXT_GATHER_ROUNDS: usize = 128;
```

These should be:
- Configurable per-task
- Adaptive based on task complexity
- Overrideable by user

### 3.4 No Caching of Tool Results
If 50 agents all read the same file, it hits the filesystem 50 times. No:
- File content caching
- Directory listing caching
- Git status caching

---

## 4. 🔵 ARCHITECTURAL — DESIGN ISSUES

### 4.1 Two Runtime Implementations — Confusing Duplication
- `runtime/src/runtime.rs` — The "real" Runtime crate
- `agentd/src/runtime.rs` — Identical code, different module path

Which one is used? The agentd one appears to be the one wired up. This is confusing and error-prone.

### 4.2 Tool Registry vs. Tool Declarations Out of Sync
**File**: `agentd/src/orchestration/mod.rs` (lines 144-1028)

The `gemini_tool_declarations()` function lists ~40 tools manually.  
**File**: `agentd/src/tool_registry.rs`

The `ToolRegistry` lists 75 tools.

These lists don't match. Some tools exist but aren't exposed to Gemini. Some are exposed but don't exist.

### 4.3 Socket Protocol is Ad-Hoc
**File**: `agentd/src/socket_server.rs`

The socket protocol handles 20+ request types with no versioning, no schema validation, no backwards compatibility strategy.

### 4.4 Global Orchestrator is Unused
**File**: `agentd/src/orchestrator.rs`

The "old" GlobalOrchestrator (350+ lines) is completely bypassed by the "new" `NewOrchestrator`. It's dead code that should be removed.

### 4.5 Hub Agent and Worker Agent are Abstractions Without Implementation
**Files**: `agentd/src/hub_agent.rs`, `agentd/src/worker_agent.rs`

These are conceptual implementations showing what the architecture "should" look like, but:
- HubAgent socket server isn't started
- Worker agents don't actually receive assignments through this path
- The real implementation is in `new_orchestrator.rs` worker loop

They're architecture documentation disguised as code.

---

## 5. ⬛ MISSING — COMPETITOR FEATURES

### 5.1 No Context Window Management
Claude Code and others manage context windows carefully:
- Automatic summarization when context gets long
- File summary caching
- Smart truncation

MowisAI sends the full conversation every time. For long tasks, this will hit token limits and fail.

### 5.2 No `@` Symbol References
Competitors allow:
- `@file` to include file content
- `@directory` to include directory listings
- `@function` to include function definitions

MowisAI has no such syntax. The planner does a naive directory scan and hopes for the best.

### 5.3 No Edit-Only Mode
Can't restrict agents to only editing files (no create/delete). Sometimes you want "fix this bug" without "reorganize the whole codebase".

### 5.4 No Preview/Accept/Reject Workflow
When agents complete work, it's auto-merged. No:
- Preview of changes
- Interactive accept/reject
- Partial merge (accept some files, reject others)
- Comment/feedback loop before merging

### 5.5 No Integration with IDEs
No:
- VS Code extension
- JetBrains plugin
- Language server protocol
- File watching for hot reload

### 5.6 No Test Execution Feedback
Agents can "write tests" but can't:
- Run tests and see failures
- Iterate on failing tests
- Get coverage reports
- Benchmark performance

### 5.7 No Cost Tracking / Budgeting
Running 1000 agents × 256 tool rounds × $0.0001/token = potentially expensive. No:
- Cost estimation
- Budget caps
- Token usage tracking
- Cost-per-task reporting

---

## 6. 🔧 TECHNICAL DEBT

### 6.1 Massive File Sizes
- `socket_server.rs`: 1300+ lines
- `mod.rs` (orchestration): 1200+ lines
- `sandbox_topology.rs`: 870+ lines

These violate single responsibility principle and need refactoring.

### 6.2 String-Based ID Handling
Throughout the codebase, IDs are passed as strings and parsed:
```rust
let sandbox_id = sandboxes
    .get(&sandbox_name)
    .ok_or_else(|| anyhow!("Sandbox not found: {}", sandbox_name))?
    .clone();  // This is a String!
```

Then later:
```rust
let sandbox_name = if recorded_sandbox.parse::<u64>().is_ok() {
    // It's numeric, resolve it...
}
```

Type-safe ID wrappers would prevent bugs.

### 6.3 No Comprehensive Integration Tests
The tests are mostly unit tests. There's no:
- End-to-end orchestration test with real LLM calls
- Chaos testing (simulate agent failures)
- Load testing (does it actually work with 50 agents?)
- Long-running session test

### 6.4 TODO Comments Everywhere
```bash
$ grep -r "TODO" --include="*.rs" agentd/src/ | wc -l
```
Dozens of TODOs indicating unfinished work.

---

## 7. 📊 SUMMARY TABLE

| Category | Issue | Severity | Effort |
|----------|-------|----------|--------|
| CLI | No REPL/interactivity | 🔴 Critical | High |
| CLI | Sequential output for parallel work | 🔴 Critical | Medium |
| Layer 6 | Verification is completely stubbed | 🔴 Critical | High |
| Containers | Sleep/wake exists but unused | 🔴 Critical | Medium |
| Scaling | Hard-capped at 50 agents | 🔴 Critical | High |
| Tools | No grep/search | 🔴 Critical | Medium |
| Session | Persistence not integrated | 🟠 Major | Medium |
| Checkpoints | Restore not implemented | 🟠 Major | Medium |
| Sandbox | Scope isolation doesn't work | 🟠 Major | High |
| Merge | No semantic conflict detection | 🟠 Major | High |
| Monitoring | No health checks | 🟠 Major | Medium |
| Error Handling | Errors swallowed | 🟠 Major | Medium |
| UX | No progress indicators | 🟡 Minor | Low |
| UX | No log levels | 🟡 Minor | Low |
| Config | Hardcoded timeouts | 🟡 Minor | Low |
| Performance | No caching | 🟡 Minor | Medium |
| Architecture | Duplicate Runtime | 🔵 Design | Medium |
| Architecture | Dead code (GlobalOrchestrator) | 🔵 Design | Low |
| Architecture | Hub/Worker abstractions unused | 🔵 Design | High |
| Features | No context management | ⬛ Missing | High |
| Features | No @ references | ⬛ Missing | Medium |
| Features | No preview/accept workflow | ⬛ Missing | High |
| Features | No IDE integration | ⬛ Missing | High |
| Features | No test execution | ⬛ Missing | High |
| Features | No cost tracking | ⬛ Missing | Medium |

---

## 8. 🎯 PRIORITY ROADMAP

### Phase 1: Absolute Minimum (Weeks 1-2)
1. Fix CLI — Interactive TUI with real-time agent visibility
2. Remove 50-agent cap — Test actual limits
3. Add grep/search tool — Agents need to find code
4. Fix verification — Make it actually run tests

### Phase 2: Usable (Weeks 3-4)
5. Implement container sleep/wake — Keep agents warm
6. Integrate session persistence — Resume sessions
7. Add progress indicators — Users need feedback
8. Fix error handling — Surface all errors to user

### Phase 3: Competitive (Weeks 5-8)
9. Fix sandbox scope isolation — Actually restrict file visibility
10. Implement semantic merge review — Not just git apply
11. Add @ references — Include files in prompts
12. Add preview/accept workflow — User control over merges

### Phase 4: Production (Months 2-3)
13. Context window management — Handle long sessions
14. Test execution integration — Actually run and verify tests
15. IDE plugins — VS Code, JetBrains
16. Cost tracking and budgeting — Control expenses

---

## 9. 💀 THE BRUTAL TRUTH

**What works:**
- Single-sandbox task execution with Gemini tool loop
- Basic overlayfs container isolation
- File read/write/list tools
- Checkpoint saving (not restoring)
- Diff generation and application

**What's broken/missing:**
- Everything for multi-sandbox coordination
- CLI user experience
- Real verification
- Scale beyond 50 agents
- Session management
- Error handling
- Monitoring
- Search/navigation tools

**Bottom line**: This is a sophisticated prototype that demonstrates the CONCEPT of multi-agent orchestration, but it's not a product. It would fail in production in spectacular ways (silent failures, incorrect verification, lost sessions, angry users).

**To make this real**: 2-3 months of solid engineering work, focus on CLI UX first (it's the entry point), then verification (can't ship without knowing code works), then scale (the 1000-agent vision requires fundamental rearchitecture of the worker pool and socket handling).
