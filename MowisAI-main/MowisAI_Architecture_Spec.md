**MowisAI**

Complete Architecture Specification

New Orchestration System — 1000-Agent Scale

_Confidential — Internal Technical Document_

April 2026

# **Table of Contents**

# **1\. Project Overview**

MowisAI is an AI agent execution engine that runs thousands of isolated AI agents in parallel on-premise using OS-level sandboxing. It is built as a single Rust binary (agentd) and targets European regulated enterprise customers who require sovereign, on-premise infrastructure for AI workloads.

## **1.1 Core Value Proposition**

- OS-level filesystem isolation via overlayfs + chroot for every agent
- Local tool execution — tools run on-device, not in the cloud
- Sovereign infrastructure for GDPR and DORA compliance
- Single deployable Rust binary — no Kubernetes, no cloud dependencies
- 1000+ parallel agents with full isolation between each

## **1.2 Competitive Position**

|     |     |     |     |
| --- | --- | --- | --- |
|     | **MowisAI** | **E2B / Daytona** | **CrewAI / LangGraph** |
| On-premise | Yes | No (cloud only) | Yes |
| OS-level isolation | Yes | Partial | No  |
| Execution layer | Yes (agentd) | Yes | No  |
| 1000+ agents | Yes (new arch) | Limited | No  |
| GDPR/DORA ready | Yes | No  | Partial |

## **1.3 Current Technical Status**

- agentd Rust engine: production-ready
- 67 tests passing — this number never regresses
- Unix socket server with overlayfs/chroot sandboxing
- 75 tools across 14 categories
- Vertex AI Gemini 2.5 Pro integration via GCP
- Previous orchestration (5-layer pipeline): deprecated in favour of new architecture described in this document

# **2\. New Orchestration Architecture**

The previous 5-layer sequential pipeline was deprecated due to fundamental performance problems: it required multiple sequential LLM calls before any worker started, used a single merge container as a serialization bottleneck, and had no checkpoint or recovery mechanism. The new architecture replaces all planning and coordination logic while keeping the agentd execution engine completely intact.

## **2.1 Design Principles**

- Fast start: workers begin executing within seconds of receiving a prompt, not minutes
- Event-driven dispatch: tasks fire the instant their dependencies complete — no batching
- Sandbox specialization: agents are domain specialists, not generalists
- Three-layer isolation: base repo, sandbox scope, agent writes — all independent CoW layers
- Checkpoint-first reliability: every tool call is checkpointed; failures never lose work
- Parallel merge: merge workers scale horizontally, not serially
- Verification loop: test failures re-enter the scheduler as new tasks

## **2.2 The Seven Layers**

The new architecture has seven functional layers, executed in sequence at the macro level but with extensive parallelism within each layer.

|     |     |     |
| --- | --- | --- |
| **Layer** | **Name** | **Responsibility** |
| 1   | Fast Planner | Shell scan + 1 LLM call → task graph + sandbox topology |
| 2   | Overlayfs Topology | 3-level CoW filesystem: base → sandbox → agent |
| 3   | Scheduler | Event-driven dispatch, fires tasks instantly on dep completion |
| 4   | Agent Execution | Gemini tool loop + checkpoint after every tool call |
| 5   | Parallel Merge | Tree-pattern merge within sandbox, LLM conflict repair |
| 6   | Verification Loop | Test task graph per sandbox, failures re-enter scheduler |
| 7   | Cross-Sandbox Merge | Final integration merge, integration conflict repair |

# **3\. Layer 1 — Fast Planner**

The fast planner replaces the previous Context Gatherer (up to 128 LLM tool-calling rounds) and Architect (1 LLM call) and Sandbox Owner (1 LLM call per sandbox) with a single operation that completes in 1-3 seconds before any worker starts.

## **3.1 How It Works**

- Step 1: Run a shell command (find or tree) to produce the directory tree as a string — no LLM involved, ~10ms
- Step 2: Send one Gemini call with: the user prompt + the directory tree string
- Step 3: Gemini outputs two things in one response: the task graph AND the sandbox topology
- No file contents are read at planning stage — workers read the files they need themselves

## **3.2 Task Graph Format**

The task graph is a JSON array of task objects:

{ "id": "t1", "description": "implement auth module", "deps": \[\], "hint": "src/auth/" }

{ "id": "t2", "description": "implement API routes", "deps": \["t1"\], "hint": "src/routes/" }

{ "id": "t3", "description": "write auth tests", "deps": \["t1"\], "hint": "tests/auth/" }

{ "id": "t4", "description": "integration test", "deps": \["t2", "t3"\], "hint": "" }

Fields:

- id: unique task identifier
- description: what this task does — passed directly to the agent as its objective
- deps: list of task IDs that must complete before this task can start
- hint: filesystem path hint — tells the scheduler which sandbox this task belongs to

## **3.3 Sandbox Topology Decision**

The planner also decides sandbox topology in the same LLM call. The decision rules are:

- Small/single-domain project → 1 sandbox, up to 50 agents
- Large/multi-domain project → N sandboxes, each with up to 100 agents
- Each sandbox gets a domain name (frontend, backend, infra, testing, etc.)
- Each sandbox gets a tool subset appropriate to its domain
- Each sandbox gets a scoped filesystem view (configurable)

Example topology output:

{ "name": "frontend", "scope": "src/frontend/", "tools": \["read_file","write_file","run_command","npm_install"\], "max_agents": 100 }

{ "name": "backend", "scope": "src/backend/", "tools": \["read_file","write_file","run_command","git_commit","http_get"\], "max_agents": 100 }

{ "name": "infra", "scope": "deployment/", "tools": \["read_file","write_file","run_command","docker_build","kubectl_apply"\], "max_agents": 50 }

## **3.4 Performance Comparison**

|     |     |     |
| --- | --- | --- |
|     | **Old Pipeline** | **New Fast Planner** |
| LLM calls before first worker | 3+ (up to 128 rounds) | 1   |
| File reads at planning stage | Up to 128 files | 0   |
| Time before first worker starts | 2-5 minutes | 1-3 seconds |
| Context gathering | LLM tool loop | Shell command |

# **4\. Layer 2 — Overlayfs Topology**

The overlayfs topology is a three-level copy-on-write filesystem stack. This is the mechanism that allows 1000 agents to each have a fully isolated view of the filesystem without 1000 copies of the repository.

## **4.1 The Three Levels**

### **Level 0 — Base Layer (read-only, shared)**

The full repository is mounted as a read-only base layer. Every sandbox and every agent shares this single read-only layer. No duplication. Writes to this layer are impossible by design — the OS enforces it.

### **Level 1 — Sandbox Layer (copy-on-write per sandbox)**

Each sandbox has its own copy-on-write layer sitting on top of the base layer. This layer can be configured to scope the visible filesystem — a frontend sandbox can be configured to see only src/frontend/ plus any shared utilities. All agents within a sandbox share this sandbox layer as their read base.

### **Level 2 — Agent Layer (copy-on-write per agent)**

Every individual agent has its own copy-on-write layer on top of the sandbox layer. This is the layer the agent writes to. All writes go here and are completely invisible to every other agent. When the agent completes its task, its layer is diffed against the sandbox layer to produce a clean git diff, then the layer is discarded.

## **4.2 Checkpointing**

A checkpoint is a snapshot of the agent's CoW layer at a point in time. After every tool call completes successfully, the agent runtime saves a checkpoint. Implementation options:

- cp -al: hard-link copy of the upper directory — fast, low disk usage, works on any Linux filesystem
- btrfs subvolume snapshot: instant, zero-copy, preferred if the volume is btrfs

The checkpoint log is a list of (checkpoint_id, tool_call, timestamp) tuples stored in memory and persisted to a small JSON file in the agent's working directory. If the agent crashes, the checkpoint log survives and is handed to the replacement agent.

## **4.3 Filesystem Scoping**

Sandbox-level filesystem scoping is optional but recommended for large projects. It reduces the amount of irrelevant context an agent sees and prevents cross-domain writes at the filesystem level rather than relying on LLM instructions alone.

- Scoped: frontend agents see only src/frontend/ — cannot accidentally write to src/backend/
- Unscoped: agents see the full repo through the sandbox layer — useful for cross-cutting tasks like documentation or testing

# **5\. Layer 3 — Scheduler**

The scheduler is the central dispatch engine. It is the piece that makes 1000 agents possible. Its core invariant: a task is dispatched the instant all its dependencies complete. No batching, no group boundaries, no artificial delays.

## **5.1 Internal Data Structures**

- task_graph: HashMap&lt;TaskId, Task&gt; — the full task graph from the planner
- dep_counter: HashMap&lt;TaskId, AtomicUsize&gt; — count of unmet dependencies per task
- ready_queue: tokio::sync::mpsc::Sender&lt;TaskId&gt; — tasks ready for dispatch
- running: HashMap&lt;TaskId, AgentHandle&gt; — currently executing tasks
- completed: HashSet&lt;TaskId&gt; — completed task IDs
- sandbox_queues: HashMap&lt;SandboxName, Vec<IdleAgent&gt;> — idle agents per sandbox

## **5.2 Dispatch Loop**

The scheduler runs as a tokio task. Its loop:

- Initialize: for each task with deps=\[\], push to ready_queue
- On ready_queue receive: find an idle agent in the correct sandbox, dispatch task
- On task completion signal: decrement dep_counter for all tasks that listed this task as a dep
- For each task whose dep_counter just hit 0: push to ready_queue
- Repeat

The critical property: the completion handler and the ready_queue push happen in the same atomic operation. There is no polling, no sleep loop, no batch check. The moment a task completes, its dependents are evaluated immediately.

## **5.3 Sandbox Awareness**

Every task carries a sandbox assignment from the planner (via the hint field). The scheduler uses this to route tasks to the correct agent pool. A frontend task will never be dispatched to a backend agent. This is enforced at the scheduler level, not via LLM instructions.

## **5.4 Scaling to 1000 Agents**

The scheduler itself is a single lightweight tokio task — it does no LLM work and no filesystem work. At 1000 agents, the scheduler's work is: receive 1000 completion signals, do 1000 HashMap lookups, send 1000 channel messages. This is microseconds of work, not seconds. The bottleneck at 1000 agents is always the agents themselves — network latency to Gemini, tool execution time — never the scheduler.

# **6\. Layer 4 — Agent Execution with Checkpoints**

Each agent is a Gemini tool-calling loop running inside an isolated container via the agentd socket. The key addition to the previous worker design is the checkpoint system.

## **6.1 Agent Execution Loop**

- Receive task: task description, available tools (filtered to sandbox subset), filesystem scope
- Tool-calling loop: send state to Gemini → receive text (done) or function calls → execute tools via agentd socket → send results back → repeat
- After EVERY tool call that succeeds: save checkpoint
- On task completion: run git diff against sandbox layer → emit AgentResult { agent_id, success, summary, diff, files_changed }

## **6.2 Checkpoint Format**

CheckpointEntry {

id: u64, // monotonic counter

tool_call: String, // which tool was called

tool_args: serde_json::Value,

tool_result: String,

timestamp: u64,

layer_snapshot_path: String, // path to the CoW layer snapshot

}

## **6.3 Error Handling — Three Tiers**

### **Tier 1 — Tool failure (retryable)**

The tool call returned an error (non-zero exit, IO error, timeout). Action: rollback agent CoW layer to last checkpoint snapshot, retry the tool call with the same arguments. Max retries: 3 per tool call.

### **Tier 2 — Agent crash (recoverable)**

The agent process died, the container became unresponsive, or the Gemini connection failed permanently. Action: kill the container, spawn a fresh agent container in the same sandbox with the same CoW layer restored from the last checkpoint, hand it the full checkpoint log and task description, continue from last checkpoint.

### **Tier 3 — Repeated failure (escalate)**

The same task has failed 3 times across Tier 1 and Tier 2 recovery attempts. Action: mark the task as failed, emit the full checkpoint log and all error details to the human-readable error output, continue the run without this task (dependent tasks are skipped with BLOCKED status), include a clear summary in the final output.

## **6.4 Agent Specialization**

Each sandbox type has a different agent configuration:

|     |     |     |     |
| --- | --- | --- | --- |
| **Sandbox** | **System prompt focus** | **Tool subset** | **FS scope** |
| frontend | React/TS/CSS patterns, component architecture | fs, shell, npm_install, web | src/frontend/ |
| backend | API design, data models, auth patterns | fs, shell, git, http, storage | src/backend/ |
| infra | Docker, k8s, CI/CD, deployment configs | fs, shell, docker, kubernetes | deployment/ |
| testing | Test coverage, edge cases, assertions | fs, shell, git, dev_tools | tests/ |

# **7\. Layer 5 — Parallel Merge**

When agents within a sandbox complete their tasks, their diffs need to be merged into a single sandbox result. The old architecture used a single merge container — a serial bottleneck. The new architecture uses a tree-pattern parallel merge.

## **7.1 Tree-Pattern Merge**

Given N completed agent diffs, the merge proceeds as a binary tree:

- Round 1: pair up diffs, spawn N/2 merge workers in parallel, each merges one pair
- Round 2: pair up the round-1 results, spawn N/4 merge workers in parallel
- Continue until one merged result remains
- Log2(N) rounds total — for 100 agents, 7 rounds, not 100 serial merges

## **7.2 Conflict Resolution**

Each merge worker attempts git apply. On conflict:

- Extract the conflict markers from the failed apply
- Send to Gemini: both diff versions + conflict region + task descriptions for both agents
- Gemini produces a repaired patch
- Retry git apply with repaired patch
- Max 3 repair attempts per conflict before the merge worker emits a CONFLICT_UNRESOLVED marker in the output

## **7.3 Merge Worker Lifecycle**

Merge workers are not pre-spawned. They are created on demand when two branches need combining and destroyed after their merge completes. This means merge worker count scales with the amount of parallel work happening, not with the total agent count.

# **8\. Layer 6 — Verification Loop**

After a sandbox completes its work and merges its results, the verification loop runs a test pass against the merged output before that sandbox's result is considered final.

## **8.1 Verification Planner**

A lightweight version of the fast planner, but scoped to verification. Given the sandbox's merged diff and the original task description:

- Generates a test task graph: unit tests, integration tests, linting, type checking
- Each test task is a small independent task with its own deps
- The test task graph is injected into the scheduler as new tasks, scoped to the same sandbox

## **8.2 Test Execution**

Test tasks run on the same agents that did the original work (warm containers, correct context). Test agents have access to dev_tools (run_tests, lint_code, format_code) in addition to standard tools.

## **8.3 Failure Re-injection**

When a test task fails:

- The test failure is parsed into a fix description
- A new fix task is created and injected into the scheduler with the failed test as a dependency
- The fix task runs on a regular agent (not a test agent)
- After the fix completes, the test is re-run
- Loop continues until all tests pass or max_verification_rounds is exceeded

## **8.4 Max Rounds**

Default: 3 verification rounds. If tests still fail after 3 rounds of fix-and-retest, the sandbox result is marked PARTIALLY_VERIFIED and the failing tests are included in the final output as known issues. The run does not abort — other sandboxes continue.

# **9\. Layer 7 — Cross-Sandbox Merge and Final Output**

Once all sandboxes have completed their work and passed (or exhausted) their verification loops, the cross-sandbox merge combines all sandbox results into the final output.

## **9.1 Cross-Sandbox Merge**

One cross-sandbox merge worker receives all verified sandbox diffs. It merges them sequentially in the order defined by the planner's execution_order, applying each sandbox's diff to a clean copy of the base repo. Integration conflicts — for example, a frontend component calling a backend endpoint that was renamed — are detected here.

## **9.2 Integration Conflict Repair**

Integration conflicts are more complex than intra-sandbox conflicts because they involve changes made by agents in different domains who had no visibility into each other's work. The conflict repair prompt for cross-sandbox conflicts includes:

- Both conflicting diffs
- The original task descriptions for both sandboxes
- The sandbox system prompts (which encode domain knowledge)
- The project context from the planner

## **9.3 Final Output Structure**

FinalOutput {

merged_diff: String, // the complete patch to apply to the repo

sandbox_results: Vec&lt;SandboxResult&gt;,

verification_status: HashMap&lt;SandboxName, VerificationStatus&gt;,

failed_tasks: Vec&lt;FailedTask&gt;, // tasks that hit tier 3 escalation

known_issues: Vec&lt;String&gt;, // tests that failed verification

summary: String, // Gemini-generated human summary

total_agents_used: usize,

total_duration_secs: u64,

}

# **10\. agentd Core — Do Not Modify**

The agentd binary is the execution engine. The new orchestration system is built on top of it. Nothing described in layers 1-7 modifies agentd internals. All orchestration communicates with agentd exclusively through the Unix socket API.

## **10.1 Socket API**

|     |     |
| --- | --- |
| **Call** | **Description** |
| create_sandbox | Create isolated environment with image + packages |
| create_container | Spawn container within sandbox |
| invoke_tool | Execute a tool in container context |
| pause_container | Pause a running container |
| resume_container | Resume a paused container |
| destroy_sandbox | Clean up sandbox and all its containers |

## **10.2 Tool Categories**

|     |     |     |
| --- | --- | --- |
| **Category** | **Count** | **Examples** |
| Filesystem | 11  | read_file, write_file, list_files, copy_file, move_file |
| Shell | 5   | run_command, run_script, kill_process |
| HTTP | 7   | http_get, http_post, download_file, websocket_send |
| Git | 9   | git_clone, git_commit, git_diff, git_status, git_push |
| Data | 5   | json_parse, json_query, csv_read |
| Docker | 7   | docker_build, docker_run, docker_ps |
| Kubernetes | 6   | kubectl_apply, kubectl_get, kubectl_logs |
| Dev Tools | 5   | format_code, lint_code, run_tests |
| Channels | 6   | channel_send, channel_receive, broadcast |
| Memory/Secrets | 8   | memory_read, secret_write |
| Package Managers | 3   | npm_install, pip_install, cargo_add |
| Web | 3   | web_search, web_fetch, web_screenshot |

## **10.3 Sandboxing Implementation**

- overlayfs: copy-on-write layers on top of read-only Alpine/Debian base images
- chroot: restricts process filesystem visibility to sandbox root
- PID namespaces: via unshare --fork --pid --mount-proc
- cgroups: RAM and CPU resource limits per container

# **11\. Hard Invariants**

These rules are absolute. They apply to every change, every session, every agent. They are not guidelines.

- Orchestrator-mediated coordination ONLY. No direct agent-to-agent communication under any circumstances.
- Sandbox and container IDs are always returned as String in JSON responses. Never u64, never integer.
- Never delete or modify tests to make them pass. Fix the actual implementation.
- Never stub or fake tool implementations.
- All tools execute within container context via chroot. No host-side tool execution.
- 67 tests must always pass. This number never regresses. New features add new tests.
- The agentd binary and socket API are not modified by orchestration work. Orchestration communicates through the socket only.

# **12\. Infrastructure and Configuration**

## **12.1 Requirements**

- OS: Linux (Unix domain sockets, overlayfs, chroot, cgroups all required)
- Root access: required for overlayfs mounts and chroot
- gcloud: authenticated, for Vertex AI Gemini access
- GCP project: company-internal-tools-490516
- Gemini model: gemini-2.5-pro

## **12.2 Resource Estimation**

|     |     |     |     |
| --- | --- | --- | --- |
| **Agent count** | **RAM estimate** | **CPU estimate** | **Notes** |
| 50 agents | ~2.5GB | ~5 cores | Single sandbox |
| 100 agents | ~5GB | ~10 cores | 2-3 sandboxes |
| 500 agents | ~25GB | ~50 cores | 5-10 sandboxes |
| 1000 agents | ~50GB | ~100 cores | 10+ sandboxes |

## **12.3 Key Configuration Parameters**

OrchestratorConfig {

max_total_agents: usize, // default: 1000

max_agents_per_sandbox: usize, // default: 100

max_sandboxes: usize, // default: 20

task_timeout_secs: u64, // default: 3600

checkpoint_interval: CheckpointTrigger, // AfterEveryToolCall

max_tier1_retries: usize, // default: 3

max_tier2_retries: usize, // default: 2

max_tier3_before_escalate: usize,// default: 3

max_verification_rounds: usize, // default: 3

merge_max_retries: usize, // default: 3

}

# **13\. Why the Old 5-Layer Pipeline Was Deprecated**

This section documents the reasoning behind the architectural change, for future reference.

|     |     |
| --- | --- |
| **Problem** | **Details** |
| Sequential startup | Context Gatherer (up to 128 LLM rounds) + Architect (1 call) + Sandbox Owner (1 call per sandbox) = 2-5 minutes before first worker starts |
| Serial merge bottleneck | One merge container processing diffs sequentially — parallelism of 100 workers collapses to serial at merge time |
| No validation layer | Architect's single LLM call decided everything; bad output poisoned the entire run with no recovery |
| No checkpoints | Agent crash = all work lost for that task, restart from zero |
| Tool filtering advisory only | Workers saw only their assigned tools in prompt, but socket server did not enforce it |
| No observability | Failed run on 20 agents had no structured way to identify which layer, sandbox, worker, or tool call failed |
| In-memory session state | Interactive session state lost on agentd crash |
| No verification loop | No built-in mechanism to test results and re-inject failures |

# **14\. Implementation Notes for Copilot**

This section contains specific guidance for implementing the new architecture in the existing Rust codebase.

## **14.1 Where to Build**

- New orchestration goes in agentd/src/orchestration/ — replace existing files
- Keep agentd/src/tools/, agentd/src/sandbox.rs, agentd/src/socket_server.rs completely unchanged
- agentd-protocol/src/lib.rs: add new types for the task graph, sandbox topology, and scheduler messages
- runtime/: may need updates for the new 3-level CoW layer management

## **14.2 New Files to Create**

- orchestration/planner.rs — fast planner: shell scan + single Gemini call
- orchestration/scheduler.rs — event-driven task dispatcher
- orchestration/sandbox_topology.rs — CoW layer management, agent spawning
- orchestration/checkpoint.rs — checkpoint save/restore logic
- orchestration/merge_worker.rs — parallel tree-pattern merge
- orchestration/verification.rs — verification planner and test loop
- orchestration/types.rs — replace with new types
- orchestration/mod.rs — replace with new module exports

## **14.3 Files to Delete**

- orchestration/context_gatherer.rs — replaced by planner.rs
- orchestration/architect.rs — replaced by planner.rs
- orchestration/sandbox_owner.rs — replaced by sandbox_topology.rs
- orchestration/sandbox_manager.rs — replaced by merge_worker.rs
- orchestration/worker.rs — replaced by new agent execution in sandbox_topology.rs
- orchestration/planner.rs (old) — replaced
- orchestration/coordinator.rs — no longer needed

## **14.4 Critical Implementation Detail: Checkpoint Snapshots**

The checkpoint system is the most technically complex new piece. The CoW layer snapshot after each tool call must be:

- Fast: cp -al (hard-link copy) is recommended — near-instant for typical agent layer sizes
- Atomic: use rename(2) to make the snapshot visible atomically
- Bounded: keep only the last N checkpoints to avoid disk exhaustion (default: keep last 10)
- Recoverable: the checkpoint log (JSON file) must survive agent container destruction

## **14.5 Scheduler Implementation**

The scheduler is the most important new piece for scale. Key implementation notes:

- Use tokio::sync::mpsc for the ready queue — bounded channel, size = max_total_agents
- Use DashMap (concurrent HashMap) for dep_counter — multiple completion signals may arrive simultaneously
- The dispatch function must be O(1): find_idle_agent_in_sandbox should be a simple pop from a per-sandbox VecDeque
- Never block the scheduler task on IO — all agentd socket calls go through spawned tokio tasks

# **Appendix A — Codebase Reference**

|     |     |
| --- | --- |
| **Component** | **File** |
| Socket server | agentd/src/socket_server.rs |
| Sandbox creation | agentd/src/sandbox.rs |
| Tool registry | agentd/src/tool_registry.rs |
| Protocol types | agentd-protocol/src/lib.rs |
| Runtime control plane | runtime/src/runtime.rs |
| agentd client | runtime/src/agentd_client.rs |
| Main entry point | agentd/src/main.rs |
| Integration tests | agentd/tests/comprehensive_integration_tests.rs |

# **Appendix B — Glossary**

|     |     |
| --- | --- |
| **Term** | **Definition** |
| agentd | The core Rust daemon and socket server — the execution engine, never modified by orchestration work |
| Base layer | Read-only overlayfs layer containing the full repository, shared by all sandboxes |
| Sandbox layer | Copy-on-write overlayfs layer per sandbox, scoped filesystem view |
| Agent layer | Copy-on-write overlayfs layer per agent, fully isolated writes |
| Checkpoint | Snapshot of an agent's CoW layer after a successful tool call |
| Task graph | DAG of tasks with dependency edges, produced by the fast planner |
| Sandbox topology | The set of sandboxes, their types, tool subsets, and agent counts for a run |
| Scheduler | Event-driven dispatcher that fires tasks the instant their deps complete |
| Merge worker | Ephemeral process that merges two agent diffs using git apply + LLM repair |
| Verification loop | Post-completion test pass that re-injects failures as new tasks |
| Tier 1/2/3 error | Tool failure (retryable) / agent crash (recoverable) / repeated failure (escalate) |
| CoW | Copy-on-write — filesystem layer that records only writes, reads fall through to layers below |