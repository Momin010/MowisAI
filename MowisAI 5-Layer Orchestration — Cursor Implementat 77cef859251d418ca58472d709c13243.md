# MowisAI 5-Layer Orchestration — Cursor Implementation Plan

## Context for Cursor

You are working on **MowisAI**, an AI agent execution engine written in Rust. The codebase has a working Unix socket server (`agentd`) that creates real sandboxes using overlayfs/chroot, provisions containers inside them, and executes tools (75 tools across 14 categories). There is also a working Vertex AI agent loop (`vertex_agent.rs`) that connects Gemini 2.5 to agentd — it creates a sandbox, creates a container, then loops: sends prompt to Gemini, Gemini returns tool calls, tool calls are executed via the agentd socket, results go back to Gemini.

**The codebase compiles clean. 43 tests pass. Do NOT break existing tests or modify existing working files unless explicitly told to.**

---

## What We Are Building

A 5-layer orchestration system where a complex user task (like "build me a REST API with auth, database, and tests") gets broken down and executed by multiple AI agents running in parallel, each in their own isolated container inside a sandbox.

---

## The Five Layers

### Layer 1 — Context Gatherer

**Purpose:** Understand the project before any work starts.

**Behavior:**

- Reads existing project files via agentd tools (`list_files`, `read_file`) to understand the codebase
- If no codebase exists, asks the user clarifying questions
- Produces a `ProjectContext` struct (JSON-serializable)

**Input:** User prompt (String) + agentd socket path

**Output:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContext {
    pub project_name: String,
    pub description: String,
    pub tech_stack: Vec<String>,
    pub existing_structure: String,  // directory tree summary
    pub key_files: Vec<String>,
    pub constraints: Vec<String>,
    pub task_summary: String,
}
```

**Implementation:** Single function `gather_context(prompt: &str, project_id: &str, socket_path: &str) -> Result<ProjectContext>` that runs a Gemini tool-loop (reuse the pattern from `vertex_agent.rs`) with a system prompt focused on exploration, NOT coding.

---

### Layer 2 — Architect

**Purpose:** Take the ProjectContext and create the full implementation plan.

**Behavior:**

- Reads the ProjectContext
- Decides how many sandboxes are needed, what each sandbox contains
- Decides how many agents per sandbox, what tools each gets
- Produces an `ImplementationBlueprint`

**Input:** `ProjectContext` + project_id

**Output:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub name: String,           // e.g. "frontend", "backend"
    pub os: String,             // e.g. "alpine"
    pub packages: Vec<String>,  // e.g. ["nodejs", "npm"]
    pub tools: Vec<String>,     // which agentd tools agents get
    pub agent_count: usize,
    pub deliverable: String,    // what this sandbox produces
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplementationBlueprint {
    pub sandboxes: Vec<SandboxConfig>,
    pub execution_order: Vec<String>,  // sandbox names in order, or parallel groups
    pub merge_strategy: String,
}
```

**Implementation:** Single function `create_blueprint(context: &ProjectContext, project_id: &str) -> Result<ImplementationBlueprint>` that calls Gemini once with the ProjectContext as input and asks it to output the blueprint as JSON. Parse the JSON into the struct.

---

### Layer 3 — Sandbox Owner (one per sandbox)

**Purpose:** For a given sandbox, break its deliverable into specific agent tasks.

**Behavior:**

- Receives the ProjectContext + its SandboxConfig
- Creates the actual sandbox via agentd socket (with `create_sandbox` request)
- Decides exactly which files each agent works on
- Produces a `SandboxExecutionPlan`

**Input:** `ProjectContext` + `SandboxConfig` + project_id + socket_path

**Output:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTask {
    pub agent_id: String,       // e.g. "fe-agent-01"
    pub task: String,           // what this agent does
    pub files: Vec<String>,     // files this agent owns
    pub tools: Vec<String>,     // tool subset for this agent
    pub context: String,        // additional context for the agent
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxExecutionPlan {
    pub sandbox_id: String,     // the agentd sandbox ID
    pub agents: Vec<AgentTask>,
    pub dependency_order: Vec<Vec<String>>,  // groups of agent_ids that can run in parallel
}
```

**Implementation:** Single function `create_sandbox_plan(context: &ProjectContext, config: &SandboxConfig, project_id: &str, socket_path: &str) -> Result<SandboxExecutionPlan>`

**Multiple Sandbox Owners run in parallel** — one thread per sandbox.

---

### Layer 4 — Sandbox Manager (one per sandbox)

**Purpose:** Lives inside the sandbox. Executes the plan by running agents and merging their work.

**Behavior:**

- Creates a container for each agent inside the sandbox
- Starts agents running in parallel (respecting dependency_order)
- Collects agent results (diffs/patches) as they complete
- Merges patches sequentially in a dedicated merge container
- If merge conflicts occur, calls Gemini to resolve them
- Reports completion status

**Input:** `SandboxExecutionPlan` + project_id + socket_path

**Output:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub agent_id: String,
    pub success: bool,
    pub summary: String,
    pub diff: String,           // git diff output
    pub files_changed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    pub sandbox_id: String,
    pub success: bool,
    pub agent_results: Vec<AgentResult>,
    pub merged_diff: String,    // final merged diff
}
```

**Implementation:** Function `run_sandbox(plan: &SandboxExecutionPlan, project_id: &str, socket_path: &str) -> Result<SandboxResult>`

This is the most complex layer. Key details:

- Use `std::thread::spawn` for parallelism (one thread per agent)
- Use `std::sync::mpsc::channel` to collect results
- Each agent thread calls `run_worker` (Layer 5) and sends the result back
- After all agents in a dependency group finish, start the next group
- Merge by applying `git diff` patches sequentially

---

### Layer 5 — Worker (the actual agent)

**Purpose:** Execute a single task inside a container.

**Behavior:**

- Gets a dynamically generated system prompt from the Sandbox Owner's task description
- Runs a Gemini tool-loop (same pattern as `vertex_agent.rs`)
- Creates a git branch, does its work, produces a diff
- Returns structured result

**Input:** `AgentTask` + sandbox_id + container_id + project_id + socket_path

**Output:** `AgentResult`

**Implementation:** Function `run_worker(task: &AgentTask, sandbox_id: &str, container_id: &str, project_id: &str, socket_path: &str) -> Result<AgentResult>`

This should reuse the core Gemini loop logic from `vertex_agent.rs`. Do NOT duplicate that code — extract the reusable parts into a shared function.

---

## File Structure

Create these NEW files. Do NOT modify existing files except `lib.rs` (to add module declarations) and `Cargo.toml` (if new dependencies are needed).

```
agentd/src/orchestration/
├── mod.rs              // DO NOT REWRITE — only add new module declarations
├── types.rs            // NEW — all shared types above
├── context_gatherer.rs // NEW — Layer 1
├── architect.rs        // NEW — Layer 2  
├── sandbox_owner.rs    // NEW — Layer 3
├── sandbox_manager.rs  // NEW — Layer 4
├── worker.rs           // NEW — Layer 5
├── orchestrator.rs     // EXISTING — do not touch
├── executor.rs         // EXISTING — do not touch
├── planner.rs          // EXISTING — do not touch
├── agent_runner.rs     // EXISTING — do not touch
└── dependency_graph.rs // EXISTING — do not touch
```

**Critical rule: DO NOT remove, rename, or modify any existing modules in `mod.rs`.** Only ADD new `pub mod` declarations for the new files.

---

## How the Vertex AI / Gemini Integration Works

Reuse the pattern from `vertex_agent.rs`. Here is the pattern:

1. Get access token: `gcloud auth print-access-token` via `std::process::Command`
2. Build request body with `contents` (conversation history) and `tools` (function declarations)
3. POST to `https://us-central1-aiplatform.googleapis.com/v1/projects/{project}/locations/us-central1/publishers/google/models/gemini-2.5-pro:generateContent`
4. Parse response — if it contains `functionCall` parts, execute them via agentd socket and loop
5. If response is text-only (no function calls), the agent is done

For **workers** (Layer 5), use `gemini-2.5-flash` instead of `gemini-2.5-pro` — cheaper and faster.

For **tool execution via agentd socket**, send JSON over Unix socket:

```json
{"request_type": "invoke_tool", "sandbox": "SANDBOX_ID", "container": "CONTAINER_ID", "name": "TOOL_NAME", "input": {}}
```

Read response as newline-terminated JSON.

For **creating sandboxes:**

```json
{"request_type": "create_sandbox", "os": "alpine"}
```

For **creating containers:**

```json
{"request_type": "create_container", "sandbox": "SANDBOX_ID"}
```

---

## Implementation Order

Do these in order. After each file, run `cargo build` to verify no errors.

### Step 1: `types.rs`

Create all the shared types: `ProjectContext`, `SandboxConfig`, `ImplementationBlueprint`, `AgentTask`, `SandboxExecutionPlan`, `AgentResult`, `SandboxResult`. All must derive `Debug, Clone, Serialize, Deserialize`.

### Step 2: `worker.rs` (Layer 5)

Implement `run_worker`. This is the simplest layer — just a Gemini tool-loop with a dynamic system prompt. Reuse the Gemini HTTP call pattern from `vertex_agent.rs`. DO NOT duplicate the HTTP/auth code — extract it into a shared helper if possible.

### Step 3: `sandbox_manager.rs` (Layer 4)

Implement `run_sandbox`. Spawns worker threads, collects results via channels, handles dependency ordering. Start simple — just run all agents in parallel with no dependency ordering first, then add ordering.

### Step 4: `sandbox_owner.rs` (Layer 3)

Implement `create_sandbox_plan`. Calls Gemini once to break a sandbox's deliverable into agent tasks. Creates the sandbox via socket.

### Step 5: `architect.rs` (Layer 2)

Implement `create_blueprint`. Calls Gemini once to produce the blueprint from the context.

### Step 6: `context_gatherer.rs` (Layer 1)

Implement `gather_context`. Runs a Gemini tool-loop to explore the project.

### Step 7: Wire it together

Add a new binary command to `main.rs`:

```
agentd orchestrate --prompt "Build me X" --project GCP_PROJECT --socket /tmp/agentd.sock
```

This runs all 5 layers in sequence.

---

## Rules for Cursor

1. **Do NOT delete or modify existing files** unless explicitly told to. Only ADD new files and ADD module declarations.
2. **Do NOT modify existing tests.** All 43 existing tests must continue to pass.
3. **Run `cargo build` after every file.** Fix errors before moving to the next file.
4. **Use `anyhow::Result` for error handling** — it is already a dependency.
5. **Use `reqwest::blocking` for HTTP calls** — it is already a dependency.
6. **Use `serde_json` for JSON** — it is already a dependency.
7. **One file at a time.** Do not create all files at once.
8. **Keep functions small.** No function should be longer than 100 lines.
9. **No `json!()` macro calls longer than 50 lines.** If you need large JSON, build it programmatically with `serde_json::Value` or load from a const string.
10. **Workers use `gemini-2.5-flash`, all other layers use `gemini-2.5-pro`.**