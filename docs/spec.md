# MowisAI Agent Runtime Specification

This document represents the **foundational design** for the next‑generation MowisAI engine: a native, kernel‑adjacent runtime that provides secure, isolated execution environments for autonomous AI agents. It captures the formal definitions, architectural principles, laws, and system model that every implementation must abide by.

## 1. Mission & Scope

- **Mission:** build a native, Rust/C engine providing a two‑layer platform for AI agents.
  - **Low-level runtime**: resource management, sandbox isolation, syscall filtering, container/VM abstraction.
  - **High-level API**: agent creation, sandbox hierarchy, tool registry, memory/bucket system, inter-agent channels.

- **Scope**: this project *is not* an application or agent framework. It is the **execution substrate** upon which agents and agent frameworks run. The runtime must remain free of UI or language-specific logic.

- **Deliverables**:
  1. Accurate, enforceable specification (this document).
  2. Rust library with C-compatible FFI.
  3. Daemon binary (`agentd`) exposing runtime APIs.
  4. Example agents and integration tests.

## 2. Core Concepts

### 2.1 Agent
An **Agent** is a logical entity consisting of two inseparable components:

1. **Brain (Cognition Layer)**
   - Typically an LLM or reasoning engine.
   - Operates purely in terms of *thought*: plans, requests, decisions.
   - Has access only to the abstracted view of its environment: goals, identity, resource limits, permitted tools, and memory stores.
   - Cannot execute actions directly; it can only emit *tool requests*.

2. **Hands (Tool Layer)**
   - The set of capabilities through which the brain affects the world.
   - Examples: `read_file`, `write_file`, `run_command`, `web_search`, `spawn_agent`, `communicate_channel`.
   - Each tool is mediated by the sandbox runtime and subject to permissions and resource accounting.
   - Tools are first‑class objects with defined schemas, costs, and audit logs.

Agents are instantiated by the controller and assigned an isolated **Sandbox**.

### 2.2 Sandbox
A **Sandbox** is an isolated execution environment with its own resource allocations, filesystem view, tools, policies, and identity. It provides the following guarantees:

- **Isolation:** code within a sandbox cannot access the host OS or sibling sandboxes unless explicitly permitted.
- **Resource limits:** RAM, CPU, GPU, storage quotas are enforced by the runtime.
- **Tool policy:** only approved tools are available; calls are audited.
- **Hierarchy:** sandboxes may have children (subagents) with subset resources/permissions.
- **Auditability:** every action is logged for replay.

Conceptually, a sandbox is a miniature virtual machine or container tailored to agent semantics.

### 2.3 Subagent & Hierarchy
When an agent spawns a **subagent**, the runtime creates a new sandbox derived from the parent. The child receives:

- A subset of the parent's resources (RAM/CPU/storage, etc.).
- A subset of the parent's tools and permissions (possibly further restricted).
- Its own independent identity and memory buckets.

The relationship forms a **sandbox tree**. Information flows downward; parents cannot read or influence children except through defined channels.

### 2.4 Buckets
A **Bucket** is a persistent, structured storage unit for an agent. Buckets can contain:

- Long-term memory entries.
- Knowledge artifacts (documents, embeddings).
- Tool definitions or learned strategies.
- Partial results or outputs from subagents.

Buckets may be:

- Private to a single agent.
- Shared among a group of agents.
- Only visible to descendants in the sandbox tree.
- Mutable or immutable; versioned by the runtime.

Buckets provide the only way agents persist state outside their ephemeral short-term memory.

### 2.5 Memory
Memory is categorized into two types:

- **Short-Term Memory (STM):** volatile state held for the duration of a session or task. Stored in RAM within the sandbox and cleared when the agent stops.
- **Long-Term Memory (LTM):** durable knowledge stored in buckets, retrievable across sessions. Agents can query LTM semantically or by key.

### 2.6 Tools & Capabilities
Tools are the only mechanism through which an agent can affect its sandbox or the external world. Key characteristics:

- Defined by a schema (inputs/outputs).
- Assigned a permission level (e.g. `read`, `write`, `execute`).
- Subject to resource costs and quotas.
- Logged on every invocation for auditability and replay.

A capability-based security model governs tools: sandboxes are granted tokens representing allowed capabilities. No tool may be invoked without a valid token.

### 2.7 Channels
**Agent Channels** ("doors") enable controlled communication between sandboxes. Channels are created by the controller and have properties such as:

- Directionality: one-way, two-way.
- Access control: which agents may send/receive.
- Persistence: transient or long-lived.
- Bandwidth or message size limits.

Channels are the only allowed IPC; direct memory access is forbidden.

## 3. Laws of the System

To guide implementation, development, and review, the runtime must enforce the following laws. Violations must be detected and cause test failures.

1. **Tool-Only Action Law**: Agents may only change state or perform I/O through authorized tools. The brain has no other execution path.
2. **Sandbox Isolation Law**: Code running in one sandbox cannot read, write, or execute anything outside its own environment except via approved channels.
3. **Inheritance Law**: Child sandboxes inherit no more resources or permissions than their parent. Granting is strictly downward and explicit.
4. **Bucket Exclusivity Law**: Buckets are the sole mechanism for persistent state; agents may not directly access the host filesystem.
5. **Channel Law**: All inter-agent communication must travel through a defined channel object; no implicit sharing.
6. **Resource Enforcement Law**: The runtime enforces all declared resource limits; overuse results in throttling or sandbox termination.
7. **Audit & Replay Law**: Every tool invocation, resource allocation, and message must be logged with enough detail to deterministically replay the execution given identical inputs.
8. **Permission Transparency Law**: Agents must be able to query their own granted capabilities; the controller must be able to inspect any sandbox's permissions.
9. **Determinism Law**: Given the same seed state (model responses, bucket contents, etc.) and same tool implementations, replaying an agent session must yield the same actions in the same order.
10. **Failure Containment Law**: Sandbox crashes or misbehavior must not propagate to the host or peer sandboxes; failures are confined and reported.

## 4. API Sketches

### 4.1 Rust Library (`libagent`)

```rust
pub struct AgentConfig {
    pub model: ModelHandle,
    pub tools: Vec<ToolDefinition>,
    pub resources: ResourceLimits,
    pub initial_buckets: Vec<BucketSpec>,
}

pub struct Agent {
    // methods to interact with the agent
}

impl Agent {
    pub fn spawn(config: AgentConfig) -> Result<Agent>;
    pub fn run(&mut self, prompt: &str) -> AgentResult;
    pub fn spawn_subagent(&self, config: AgentConfig) -> Result<Agent>;
    // etc.
}

// C FFI counterparts (extern "C" fn) mirror these.
```

### 4.2 Daemon CLI (`agentd`)

CLI commands:

```text
agentd start-agent --config path/to/config.json
agentd list-sandboxes
agentd inspect --id 1234
agentd replay --id 1234 > session.log
agentd create-channel --from 1234 --to 5678
```

### 4.3 Tool Trait

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn invoke(&self, ctx: &ToolContext, input: Value) -> Result<Value>;
}
```

Tools are registered with the sandbox at creation; invocation goes through the runtime which checks permissions, logs, and executes inside the sandbox context.

## 5. Interaction Diagrams

^{
  (pending; will add sequence diagrams showing agent lifecycle, spawning, tool calls, channel messages, etc.)
}

## 6. Security Model

- The runtime runs with the minimal necessary host privileges, ideally unprivileged.
- Sandboxes leverage kernel features (namespaces, seccomp, cgroups) for isolation.
- No `unsafe` Rust code escapes the core sandbox manager without thorough review.
- FFI boundaries are audited; C interface is minimal and audited.
- Policies for aggressive resource reclamation must exist to prevent DoS.

## 7. Test Strategy

- **Unit tests** for individual modules following TDD.
- **Integration tests** that spin up actual sandboxes (using Docker or unshare) and run sample agents.
- **Law tests**: each law is encoded as a test; for example, attempt an action outside a sandbox should be rejected.
- **Fuzzing** on tool input and serialization boundaries.

## 8. Future Extensions (non‑exhaustive)

- GPU isolation via MIG or passthrough.
- Support for microVMs (Firecracker) in addition to containers.
- Remote sandbox execution (agent cloud).
- Versioned bucket syncing across hosts.
- Policy language for dynamic permissions.
- WebAssembly‑based tool sandboxing for extra security.

---

*This document will grow as the project evolves. Every new feature must map back to the laws above.*
