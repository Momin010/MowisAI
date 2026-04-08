# MowisAI Orchestration System Implementation

## Overview

The MowisAI Orchestration System is a distributed agent framework for executing complex tasks with a team of specialized agents. The system is built on a clean separation of concerns:

- **Global Orchestrator**: Task planning and team coordination
- **Runtime**: Infrastructure management (sandboxes and containers)
- **Local Hub Agents**: Team-level task management
- **Worker Agents**: Individual task execution
- **agentd**: Unchanged tool execution engine

## Architecture Components

### 1. Global Orchestrator (`orchestrator.rs`)

**Responsibility**: Analyze user tasks, plan execution, provision sandboxes, and coordinate teams.

**Key Functions**:
- `execute_task(user_task: String) -> Result<session_id>`
  - Analyzes task complexity
  - Builds dependency graph
  - Provisions sandboxes
  - Coordinates team execution
  - Collects final results

**Decision Logic**:
```
User Task (plain text)
  ↓
Decompose into team tasks
  ↓
Analyze dependencies
  ↓
Build execution DAG (directed acyclic graph)
  ↓
Estimate resource needs
  ↓
Allocate sandboxes
  ↓
Execute stages sequentially or in parallel
```

**Complexity Estimation**:
- Analyzes keywords (backend, frontend, testing, etc.)
- Estimates sandbox count (1 per team type, min 1, max 10)
- Allocates RAM based on complexity (~512 MB base + 10 MB per complexity unit)
- Allocates CPU based on complexity (~1000 millis base + 10 per complexity unit)

### 2. Runtime (`runtime.rs`)

**Responsibility**: Pure infrastructure management—no business logic.

**Key Functions**:
- `provision_sandboxes()`: Create isolation boundaries
- `request_additional_containers()`: Dynamic scaling
- `pause_container()` / `resume_container()`: Idle management
- `get_health_status()`: Monitoring
- `destroy_sandbox()`: Cleanup

**Sandbox Lifecycle**:
```
CREATING → READY → ACTIVE → PAUSED → ACTIVE → DESTROYED
```

**Resource Enforcement**:
- Per-sandbox RAM limits
- Per-sandbox CPU limits
- Container state tracking
- Hub Agent health monitoring

### 3. Local Hub Agent (`hub_agent.rs`)

**Responsibility**: Own everything inside a sandbox—worker management, task breakdown, peer coordination.

**Key Functions**:
- `receive_team_task()`: Accept work from Global Orchestrator
- `break_down_task()`: Split task into worker assignments
- `assign_to_worker()`: Distribute work to idle workers
- `run_integration_tests()`: Validate combined output
- `handle_peer_rpc()`: Communicate with other Hub Agents via sockets
- `register_api_contract()`: Publish API specs for cross-team use

**Worker Pool Management**:
- Tracks worker states: Idle, Assigned, Running, Completed, Failed
- Generates unique worker names (Jake, Mike, Sarah, etc.)
- Task breakdown strategy: split description by sentences/lines

**Cross-Team Communication**:
- Direct socket-based RPC (no central message bus)
- Each Hub Agent listens on `/sandbox/{sandbox_id}/agent.sock`
- Can query peer contracts: `get_api_contract(contract_id)`

### 4. Worker Agent (`worker_agent.rs`)

**Responsibility**: Execute assigned work with high quality and independence.

**Key Functions**:
- `receive_assignment()`: Accept task from Hub Agent
- `execute_task()`: Run the work (plan → execute → test)
- `signal_idle()`: Return to idle state after completion
- `create_completion()`: Generate result report

**Execution Pipeline**:
```
1. Thinking Phase
   - Plan task using LLM (Claude)
   - Generate step-by-step plan
   
2. Execution Phase
   - Execute planned steps
   - Invoke tools via agentd socket
   - Record all tool calls
   
3. Testing Phase
   - Validate output quality
   - Run tests if applicable
   - Self-assess results
```

**Tool Availability**:
- shell (command execution)
- filesystem (read/write files)
- git (version control)
- http (API calls)
- (extensible via agentd)

### 5. Protocol Definitions (`protocol.rs`)

**Message Types**:

**From Global Orchestrator → Runtime**:
- `ProvisioningSpec`: Request to create sandboxes
- `ContainerControlRequest`: Pause/resume/terminate

**From Global Orchestrator → Local Hub Agent**:
- `TeamTask`: Work assignment with dependencies

**From Local Hub Agent → Worker Agent**:
- `WorkerAssignment`: Individual task details

**From Worker Agent → Local Hub Agent**:
- `WorkerCompletion`: Results and status
- `WorkerIdleSignal`: Ready for next task

**From Worker Agent → Runtime**:
- `ResourceRequest`: Request more RAM/CPU

**Between Local Hub Agents**:
- `InterTeamRpc`: Request/response for API contracts
- `ApiContract`: Shared specifications for integration

### 6. Dependency Graph (`dependency_graph.rs`)

**Analysis**:
- Detects cyclic dependencies (returns error)
- Validates all dependencies exist
- Computes execution stages using topological sort (Kahn's algorithm)
- Stages are independent tasks that can run in parallel

**Example**:
```
Task A (no dependencies)
  ↓                ↓
Task B        Task C
  ↓                ↓
Task D (depends on B)    Task E (depends on C)
  
Execution Order:
Stage 1: [A]           (1 task, parallel: 1)
Stage 2: [B, C]        (2 tasks, parallel: 2)
Stage 3: [D, E]        (2 tasks, parallel: 2)
```

## Communication Patterns

### Synchronous (RPC-like)

**Global Orchestrator ↔ Runtime**:
```
Orchestrator: "Provision 3 sandboxes with [specs]"
Runtime: "Ready. Sandboxes: [handles]"
```

**Global Orchestrator ↔ Local Hub Agent**:
```
Orchestrator: "Team, execute task [spec]"
Hub Agent: executes...
Hub Agent: "Done. Output: [results]"
```

**Local Hub Agent ↔ Local Hub Agent**:
```
Team A: "Team B, give me your API contract"
Team B: "Here's the spec: [contract]"
```

### Asynchronous (Signaling)

**Worker Agent → Local Hub Agent**:
```
Worker: "I'm done with assignment, here's output: [results]"
```

**Worker Agent → Runtime**:
```
Worker: "I'm idle, pause my container"
```

### Socket-based Communication

Each Local Hub Agent exposes a Unix socket for peer communication:

```
/sandbox/sandbox-1/agent.sock   (Team A Hub Agent)
/sandbox/sandbox-2/agent.sock   (Team B Hub Agent)
/sandbox/sandbox-3/agent.sock   (Team C Hub Agent)

Team A can call methods on Team B via socket:
POST /sandbox/sandbox-2/agent.sock
{
  "method": "get_api_contract",
  "params": {"contract_id": "deployment-spec"}
}
```

## Data Flow Example

**Scenario**: Build a web application with API and frontend

```
1. User asks:
   "Build a web application with Express API and React frontend"

2. Global Orchestrator analyzes:
   - Detects: backend (API) + frontend (React)
   - Creates tasks: backend_task, frontend_task
   - frontend_task depends on backend_task (API must be ready first)
   - Builds DAG with 2 stages

3. Orchestrator provisions:
   - 2 sandboxes (one per team)
   - Each with 10 containers
   - Alpine OS + npm, git, curl, etc.

4. Orchestrator sends work:
   - To Hub Agent A: "Build Express API"
   - To Hub Agent B: "Build React frontend"

5. Hub Agent A (Backend Team):
   - Breaks into task: "Build API, create routes, add auth"
   - Assigns to 3 workers: Jake, Mike, Sarah
   - Jake: Authentication system
   - Mike: Core routes and business logic
   - Sarah: Database integration
   - All run in parallel in their own containers

6. Workers execute:
   - Each calls Claude to reason about their part
   - Execute tools: mkdir, npm init, git init, write code
   - Test their code
   - Report completion

7. Hub Agent A (integration):
   - Collects all outputs
   - Runs integration tests
   - Reports: "API ready on port 3000, routes: [list]"

8. Hub Agent A publishes API contract:
   - Registers: Listening on localhost:3000, routes: /api/...

9. Hub Agent B (Frontend Team):
   - Queries Hub Agent A: "What's your API?"
   - Gets contract
   - Breaks into task: "Build React app, integrate API, test"
   - Workers execute frontend tasks
   - Mock API during early development
   - Final: Switch to real API

10. Global Orchestrator collects:
    - Both teams done
    - Aggregates output: full application artifact
    - Returns to user

11. Timeline:
    Stage 1 (0-60s): Backend team executes in parallel
    Stage 2 (60-120s): Frontend team executes in parallel (can now use real API)
    Total: ~120s for full application
```

## Key Design Decisions

### 1. Sockets for Inter-team Communication
- **Why**: Same pattern as agentd, minimal infrastructure, easy to debug
- **How**: Each Hub Agent listens on a well-known socket path
- **Alternative rejected**: Message bus (adds complexity, single point of failure)

### 2. Dynamic Provisioning
- **Why**: Tasks may need more resources mid-execution
- **How**: Workers can request additional containers from Runtime
- **Alternative rejected**: Fixed provisioning (wasted resources or insufficient)

### 3. Idle Container Management
- **Why**: Save resources when worker not actively executing
- **How**: Worker signals idle, Hub Agent resumes when needed
- **Alternative rejected**: Always-on (wastes RAM/CPU)

### 4. Hub Agent Owns Task Breakdown
- **Why**: Team context matters for task decomposition
- **How**: Hub Agent understands its team-type and task details
- **Alternative rejected**: Global Orchestrator breaks down (loses team-level knowledge)

### 5. Worker Names Are Just IDs
- **Why**: No special meaning, just human-readable identifiers
- **How**: Generated from a list (Jake, Mike, Sarah, etc.)
- **Alternative rejected**: Meaningful names (adds coupling)

## Testing & Validation

### Unit Tests
- Runtime: provisioning, pause/resume, health checks
- Hub Agent: task breakdown, worker pool management
- Worker Agent: task execution, idle signaling
- Dependency Graph: cycle detection, topological sort

### Integration Tests (in examples/orchestration_system.rs)
1. Single backend task execution
2. Full-stack web application with teams
3. Multi-team coordination with contracts

### Load Test Scenarios (future)
- 100 workers in 10 sandboxes
- Tasks with 50+ dependencies
- Cross-sandbox communication at scale
- Hot resource constraints

## Future Enhancements

1. **LLM-based Task Analysis**
   - Use Claude to intelligently decompose tasks
   - Learn task patterns from history
   - Predict resource needs more accurately

2. **Distributed Task Queues**
   - Global queue for pending tasks
   - Worker pool elastically scales
   - Tasks distributed fairly

3. **Persistence & Recovery**
   - Save execution state to disk
   - Resume interrupted tasks
   - Replay for debugging

4. **Observability**
   - Emit metrics: task completion time, resource usage
   - Structured logging: JSON logs for analysis
   - Tracing: end-to-end execution traces

5. **Advanced Scheduling**
   - Priority queues (urgent vs. background tasks)
   - Resource affinity (GPU tasks on GPU nodes)
   - Cost optimization (run cheap tasks when expensive ones blocked)

6. **Cross-Org Sandboxes**
   - Sandboxes on multiple machines
   - Hub Agents communicate across network
   - Global load balancing

## File Structure

```
agentd/src/
├─ protocol.rs              (38 types, message definitions)
├─ runtime.rs               (Infrastructure manager)
├─ orchestrator.rs          (Task planning & coordination)
├─ hub_agent.rs             (Team-level management)
├─ worker_agent.rs          (Task execution engine)
├─ dependency_graph.rs      (DAG analysis & topological sort)
├─ lib.rs                   (Module exports)
└─ main.rs                  (CLI entry point - unchanged)

agentd/examples/
└─ orchestration_system.rs  (End-to-end example & architecture docs)
```

## Module Statistics

| Module | Lines | Exports | Key Types |
|--------|-------|---------|-----------|
| protocol.rs | 350+ | 20+ | 30+ struct types |
| runtime.rs | 350+ | 5 | Runtime, RuntimeError, ManagedSandbox |
| orchestrator.rs | 350+ | 2 | GlobalOrchestrator, OrchestratorConfig |
| hub_agent.rs | 400+ | 2 | LocalHubAgent, HubAgentConfig |
| worker_agent.rs | 350+ | 2 | WorkerAgent, WorkerConfig |
| dependency_graph.rs | 300+ | 2 | DependencyGraphBuilder, ComplexityAnalyzer |

## Getting Started

1. **Build**:
   ```bash
   cd agentd
   cargo build --release
   ```

2. **Run example**:
   ```bash
   cargo run --example orchestration_system
   ```

3. **Run tests**:
   ```bash
   cargo test
   ```

4. **Integration with agentd socket**:
   ```bash
   # Start agentd
   ./target/release/agentd socket --path /tmp/agentd.sock
   
   # Workers will invoke tools via this socket
   ```

## API Reference

### GlobalOrchestrator
```rust
pub fn execute_task(&self, user_task: String) -> Result<String>
pub fn get_session_status(&self, session_id: &str) -> Option<ExecutionSession>
pub fn get_session_results(&self, session_id: &str) -> Result<serde_json::Value>
```

### LocalHubAgent
```rust
pub fn receive_team_task(&self, task: TeamTask) -> Result<()>
pub fn break_down_task(&self) -> Result<Vec<WorkerAssignment>>
pub fn assign_to_worker(&self, assignment: WorkerAssignment) -> Result<()>
pub fn record_worker_completion(&self, completion: WorkerCompletion) -> Result<()>
pub fn create_completion_report(&self) -> Result<TaskCompletion>
pub fn handle_peer_rpc(&self, rpc: InterTeamRpc) -> Result<InterTeamRpcResponse>
```

### WorkerAgent
```rust
pub fn receive_assignment(&self, assignment: WorkerAssignment) -> Result<()>
pub fn execute_task(&self) -> Result<()>
pub fn create_completion(&self) -> Result<WorkerCompletion>
pub fn signal_idle(&self) -> Result<WorkerIdleSignal>
```

### Runtime
```rust
pub fn provision_sandboxes(&self, spec: &ProvisioningSpec) -> Result<ProvisioningReady>
pub fn request_additional_containers(&self, sandbox_id: &str, count: usize, spec: &SandboxSpec) -> Result<Vec<ContainerHandle>>
pub fn pause_container(&self, sandbox_id: &str, container_id: &str) -> Result<()>
pub fn resume_container(&self, sandbox_id: &str, container_id: &str) -> Result<()>
pub fn get_health_status(&self, sandbox_id: &str) -> Result<SandboxHealthStatus>
```

---

**Status**: Implementation complete ✓
**All components functional and tested**: ✓
**Ready for integration with agentd**: ✓
