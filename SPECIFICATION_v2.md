# MowisAI Agent Sandbox Engine - Complete Specification v2.0

## Executive Summary

MowisAI is a production-grade agent execution engine written in Rust with C FFI bindings. It provides:
- **Complete isolation**: Linux namespaces + cgroups + chroot
- **Tool-based execution**: Extensible tool trait system for LLM agents
- **Memory management**: Separate short-term (volatile) and long-term (persistent) memory
- **Agent loops**: Planning → Execution → Reflection cycles
- **Security**: Mandatory access control, syscall filtering, capability management
- **Auditability**: Complete event tracking, replay capability, anomaly detection
- **Persistence**: State checkpointing, WAL, recovery journals

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│           MowisAI Agent Sandbox Engine                  │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │  Agent Loop Layer                               │   │
│  │  ├─ Planning (tool selection)                   │   │
│  │  ├─ Execution (tool invocation)                 │   │
│  │  └─ Reflection (memory updates)                 │   │
│  └─────────────────────────────────────────────────┘   │
│                        ↓                                 │
│  ┌─────────────────────────────────────────────────┐   │
│  │  Tool Registry & Execution                      │   │
│  │  ├─ File I/O tools                              │   │
│  │  ├─ Command execution tools                     │   │
│  │  ├─ Network tools                               │   │
│  │  ├─ Memory management tools                     │   │
│  │  └─ Subagent orchestration tools                │   │
│  └─────────────────────────────────────────────────┘   │
│                        ↓                                 │
│  ┌─────────────────────────────────────────────────┐   │
│  │  Memory System                                  │   │
│  │  ├─ STM: Task context + execution results       │   │
│  │  ├─ LTM: Learned patterns + knowledge base      │   │
│  │  └─ Semantic search + similarity matching       │   │
│  └─────────────────────────────────────────────────┘   │
│                        ↓                                 │
│  ┌─────────────────────────────────────────────────┐   │
│  │  Sandbox Isolation Layer                        │   │
│  │  ├─ Linux namespaces (pid, mount, ipc, uts)    │   │
│  │  ├─ cgroups v2 (memory, cpu limits)            │   │
│  │  ├─ chroot environment                          │   │
│  │  ├─ Seccomp syscall filtering                   │   │
│  │  └─ Capability management                       │   │
│  └─────────────────────────────────────────────────┘   │
│                        ↓                                 │
│  ┌─────────────────────────────────────────────────┐   │
│  │  Persistence & Auditing Layer                   │   │
│  │  ├─ State persistence to disk                   │   │
│  │  ├─ Write-ahead logging                         │   │
│  │  ├─ Event audit trail                           │   │
│  │  ├─ Crash recovery journal                      │   │
│  │  └─ Replay engine                               │   │
│  └─────────────────────────────────────────────────┘   │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

## 10 Immutable Laws

1. **Isolation is Mandatory** - Every agent execution happens in a fully isolated sandbox; no agent can see/modify host filesystem or processes
2. **Memory is Dual** - All agents have separate STM (volatile) and LTM (persistent) memory; never mixed or conflated
3. **Tools are Fungible** - Tools are simple, composable functions; agents interact exclusively via tools, not direct APIs
4. **Execution is Auditable** - Every action is logged with timestamp, actor, action, and result; audit trail is immutable
5. **State is Recoverable** - Checkpoints and WAL ensure any failure can be replayed; no corrupted partial state persists
6. **Security is Layered** - Isolation + capabilities + seccomp + MAC means defeat of one layer doesn't compromise others
7. **Resources are Bounded** - Memory, CPU, file handles, processes all have hard limits enforced by cgroups; no runaway
8. **Policy is External** - Security policy, capabilities, syscall rules are defined outside code; can be updated without recompili
ng
9. **Transparency is Intrinsic** - All decision logs, patterns learned, options considered are queryable; no hidden internal state
10. **Failure is Fast** - Any security violation, resource overrun, or anomaly halts execution immediately; no silent fai

lures

## Core Components

### 1. Sandbox (src/sandbox.rs)

Represents a fully isolated execution environment.

```rust
pub struct Sandbox {
    id: u64,                              // unique identifier
    limits: ResourceLimits,               // memory/CPU bounds
    root: TempDir,                        // isolated filesystem root
    bucket_dir: PathBuf,                  // persistent storage directory
    tools: HashMap<String, Box<dyn Tool>>, // registered tools
}
```

**Key Methods:**
- `new(limits: ResourceLimits) -> Result<Sandbox>` - Create isolated sandbox
- `spawn_child(parent_limits: ResourceLimits) -> Result<Sandbox>` - Create child (inherits limits)
- `run_command(cmd: &str) -> Result<String>` - Execute command in sandbox isolation
- `register_tool(tool: Box<dyn Tool>)` - Register a tool for this sandbox
- `invoke_tool(name: &str, input: Value) -> Result<Value>` - Execute tool

**Isolation Mechanisms:**
- tmpfs mount on sandboxroot for fast ephemeral storage
- Mount namespace (CLONE_NEWNS) for isolated /proc, /sys
- PID namespace (CLONE_NEWPID) for isolated process tree
- chroot to sandbox root directory
- cgroups2 for memory/cpu limits with kernel enforcement

### 2. Agent Loop (src/agent_loop.rs)

Main execution engine implementing ReAct (Reasoning + Acting) pattern.

```rust
pub struct AgentLoop {
    agent_id: u64,
    memory: AgentMemory,
    max_iterations: usize,
    current_iteration: usize,
}
```

**Execution Flow:**
1. **Planning** - Parse prompt + context, select next tool via keyword matching + learned patterns
2. **Execution** - Invoke tool, capture result, store in execution history
3. **Reflection** - Analyze result, update LTM patterns, log decision

**Tool Selection Strategies:**
- Keyword matching: parse context for "read", "write", "run", etc.
- Pattern matching: use previously successful tool combinations
- Exploration: occasionally try new tools for diversity
- Fallback: echo tool if all else fails

### 3. Memory System (src/memory.rs)

**Short-Term Memory (STM):**
- Temporary storage for current session
- Task stack (goal, state, tools used)
- Recent execution results (capped at 50)
- Context key-value store
- Cleared at session end

**Long-Term Memory (LTM):**
- Persistent across sessions
- Knowledge base (key-value with embeddings)
- Pattern index (detected successful sequences)
- Semantic cache (query embeddings + results)
- Decision log (reasoning trail)

**Semantic Matching:**
- Cosine similarity for embedding-based retrieval
- Keyword search over knowledge descriptions
- Tool suggestion based on success patterns

### 4. Tools System (src/tools.rs)

**Tool Trait:**
```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn invoke(&self, ctx: &ToolContext, input: Value) -> Result<Value>;
    fn clone_box(&self) -> Box<dyn Tool>;  // for Vec<Box<dyn Tool>>
}
```

**Built-in Tools (14+):**
- **File I/O**: `read_file`, `write_file`, `delete_file`, `list_files`, `copy_file`, `create_directory`, `get_file_info`
- **Command**: `run_command` (with timeout/limits)
- **Orchestration**: `spawn_subagent`
- **Data**: `json_parse`, `json_stringify`
- **Network**: `http_get`, `http_post`
- **Debug**: `echo`

### 5. Persistence (src/persistence.rs)

**Components:**
- **PersistenceManager** - Save/load sandbox definitions
- **Checkpointer** - Periodic state snapshots
- **WriteAheadLog (WAL)** - Durability via operation logging
- **RecoveryJournal** - Crash recovery metadata

**Workflow:**
1. Before each operation, write to WAL
2. Periodically checkpoint full state
3. On crash, replay WAL from last checkpoint
4. All persistent state goes to `/var/lib/agentd/`

### 6. Audit System (src/audit.rs)

**Event Types (20+):**
- SandboxCreated, SandboxDestroyed
- ToolRegistered, ToolInvoked, ToolFailed
- MemoryStored, MemoryRetrieved
- TaskStarted, TaskCompleted, TaskFailed
- ChannelCreated, MessageSent
- AgentSpawned, AgentTerminated
- SecurityViolation, ResourceLimitExceeded
- CheckpointCreated, StateRestored
- Custom events

**Features:**
- BufferedAuditLogger (batch writes for performance)
- AuditQuery DSL for filtering (actor, type, time range)
- SecurityAuditor for aggregation + anomaly detection
- ComplianceChecker for policy enforcement
- ReplayEngine for execution reconstruction

### 7. Security (src/security.rs)

**SecurityPolicy:**
- Allowed/denied syscalls lists
- File access rules (read/write/execute per path)
- Network rules (inbound/outbound, port ranges)
- Resource limits (memory, CPU, file handles, processes)

**Predefined Policies:**
- `default_restrictive()` - Minimal syscalls, tight limits (256MB RAM, 50% CPU)
- `default_permissive()` - Most syscalls allowed, loose limits (1GB RAM, 100% CPU)

**Capabilities:**
- LinuxCaps minimal set: CAP_CHOWN, CAP_DAC_OVERRIDE
- Full set includes CAP_SYS_ADMIN, CAP_NET_BIND_SERVICE, etc.
- None set for maximum restriction

**Seccomp:**
- BPF-style filter rules (simplified JSON representation)
- Kill-on-violation for denied syscalls
- Configurable default action (allow/deny)

### 8. Channels (src/channels.rs)

Inter-agent communication primitive.

```rust
pub struct Message {
    from: u64,
    to: u64,
    payload: String,
}

pub struct Channel {
    id: u64,
    from: u64,
    to: u64,
}
```

- `create_channel(from, to) -> u64` - Create channel, return ID
- `send_message(channel_id, msg) -> Result<()>` - Send msg (validates sender)
- `read_messages(channel_id) -> Result<Vec<Message>>` - Read all msgs on channel
- Messages stored in global MESSAGE_STORE (lazy_static)

### 9. Buckets (src/buckets.rs)

Persistent file-based key-value store.

```rust
pub struct BucketStore {
    dir: PathBuf,
}
```

- `put(key, value) -> Result<()>` - Write to `dir/key.json`
- `get(key) -> Result<Value>` - Read from disk
- `delete(key) -> Result<()>` - Remove file
- Used for agent long-term memory, sandbox metadata

## Testing Strategy

### Unit Tests (per-module)
- **sandbox_tests.rs** - ID counter, child limits, cgroup enforcement
- **tool_tests.rs** - Tool registry, invocation, cloning
- **channel_tests.rs** - Channel creation, send/receive
- **bucket_tests.rs** - Persistence, round-trip serialization
- **memory_tests.rs** - STM/LTM operations, semantic search
- **audit_tests.rs** - Event creation, query, anomaly detection
- **security_tests.rs** - Policy checks, capability sets

### Integration Tests
- Multi-agent workflow (spawn subagent, send messages)
- Tool chain execution (read → process → write)
- Memory persistence (save STM, reload LTM)
- Sandbox hierarchy (parent → child spawn)
- Checkpoint + crash recovery

### Property-based Tests
- Sandbox IDs always unique and monotonic (counter high bits)
- IDs include randomized low bits and are initially seeded from wall-clock time to avoid predictable sequences
- Security violations never escape audit log
- All executed operations appear in audit trail
- Memory limits enforced within 5% tolerance

### Privilege Tests
- Run when root: full cgroup/namespace tests
- Run when non-root: skip and report gracefully

## Deployment Model

### Single-Machine Deployment
```
/var/lib/agentd/
├── sandboxes/              # Persisted sandbox state
│   ├── sandbox_1.json
│   └── sandbox_2.json
├── memory/                 # Agent memory snapshots
│   ├── agent_1.json
│   └── agent_2.json
├── checkpoints/            # Full state checkpoints
│   ├── checkpoint_1234.json
│   └── checkpoint_1235.json
├── wal.log                 # Write-ahead log
├── recovery.json           # Recovery journal
└── audit.log               # Immutable event trail
```

### Distributed Deployment (Future)
- Multiple agentd instances on different machines
- Distributed audit log via message broker (Kafka)
- Centralized memory store (Redis/PostgreSQL)
- Shared checkpoint storage (S3/NFS)

## Security Model

### Threat Model

**Attacker Goals:**
1. Escape sandbox, execute arbitrary code on host
2. Read/write files outside sandbox
3. Interfere with other agents
4. Consume unbounded resources (DoS)
5. Hide malicious actions from audit trail

### Defense Layers

**Layer 1: Isolation**
- Namespace separation (pid, mount, ipc, uts)
- chroot to ephemeral tmpfs root
- Prevents access to host filesystem

**Layer 2: Resource Control**
- cgroups v2 with memory.max and cpu.max
- Prevents unbounded resource consumption
- Kernel-enforced, no userspace bypass

**Layer 3: Syscall Filtering**
- Seccomp BPF filters
- Whitelist/blacklist per sandbox
- Kills process on violation

**Layer 4: Capability Reduction**
- Drop CAP_SYS_ADMIN, CAP_NET_ADMIN, etc.
- Minimal privilege principle
- Enforced by kernel at exec time

**Layer 5: MAC (Mandatory Access Control)**
- SELinux or AppArmor (future)
- Policy-based file access (not user-based)
- Prevents privilege escalation

**Layer 6: Auditability**
- Every action logged with timestamp, actor, result
- Tamper-evident (write-once log)
- Enables post-incident forensics

## Future Enhancements

1. **Network Policy**: eBPF-based egress filtering per sandbox
2. **GPU Isolation**: MIG (Multi-Instance GPU) support
3. **Distributed Agents**: etcd-based coordination
4. **ML Tooling**: Native PyTorch/TensorFlow sandboxes
5. **Observability**: Prometheus metrics, Jaeger tracing
6. **Cost Tracking**: Per-agent resource accounting
7. **Replay Debugger**: GUI for stepping through executions
8. **Policy Language**: DSL for custom security policies
9. **Plugin System**: Custom tool registration via WASM
10. **Multi-Tenant**: Namespace isolation + RBAC

## Performance Characteristics

| Operation | Latency | Notes |
|-----------|---------|-------|
| Create sandbox | 50ms | tmpfs mount + unshare |
| Spawn subagent | 80ms | Fork + new sandbox setup |
| Invoke tool | 5ms | In-process function call |
| Plan next action | 10ms | Keyword match + pattern lookup |
| Save checkpoint | 100ms | Full memory serialization |
| Query LTM | 2ms | HashMap lookup (O(1)) |
| Semantic search | 50ms | Embeddings + cosine similarity |
| Audit lookup | 1ms | Well-indexed log |

## API Reference

### Rust Library API

```rust
// Create and register sandbox
let limits = ResourceLimits { ram_bytes: Some(512_000_000), cpu_millis: Some(1000) };
let sandbox = Sandbox::new(limits)?;

// Spawn child sandbox
let child = sandbox.spawn_child(limits)?;

// Register tool
sandbox.register_tool(Box::new(EchoTool));

// Invoke tool
let input = json!({"msg": "hello"});
let output: Value = sandbox.invoke_tool("echo", input)?;

// Create memory
let mut memory = AgentMemory::new(agent_id, session_id);
memory.short_term.set_context("goal".into(), Value::String("find answer".into()));

// Run agent loop
let mut loop_engine = AgentLoop::new(agent_id, session_id, 100);
let result = loop_engine.run("What is 2+2?", &tools)?;

// Persist state
MemoryPersistence::save_ltm(&memory, Path::new("/var/lib/agentd/agent_1.json"))?;

// Audit query
let events = auditor.logger.read_events(100)?;
for event in events {
    println!("{:?}", event);
}
```

### C FFI API

```c
// Create sandbox
Sandbox* sb = agent_sandbox_new(512_000_000, 1000);

// Run command
char* output = agent_sandbox_run(sb, "ls -la /tmp");

// Create memory
AgentMemory* mem = agent_memory_new(1, 1);

// Create agent loop
AgentLoop* loop_engine = agent_loop_new(1, 1, 100);

// Free resources
agent_string_free(output);
agent_sandbox_free(sb);
agent_memory_free(mem);
agent_loop_free(loop_engine);
```

### CLI Interface

```bash
# Create sandbox with 512MB RAM, 1000ms CPU
agentd create-sandbox --ram 512000000 --cpu 1000

# Run agent in sandbox
agentd run --sandbox 42 "What is 2+2?"

# Register tool
agentd register-tool --sandbox 42 --name echo

# Invoke tool
agentd invoke-tool --sandbox 42 --name echo --input '{"msg":"hello"}'

# List sandboxes
agentd list

# Get agent status
agentd status --agent 1
```

## References

- Linux Namespaces: https://man7.org/linux/man-pages/man7/namespaces.7.html
- cgroups v2: https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html
- seccomp: https://github.com/seccomp/libseccomp
- ReAct Pattern: https://arxiv.org/abs/2210.03629
- Cosine Similarity: https://en.wikipedia.org/wiki/Cosine_similarity
