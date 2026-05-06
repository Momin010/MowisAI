# MowisAI Codebase — Complete Analysis

## What This Application Is

MowisAI is an OS-level AI agent execution engine. It runs thousands of isolated AI agents in parallel using overlayfs/chroot sandboxing on Linux. It targets European regulated enterprises (GDPR, DORA). The competitive gap it fills: E2B/Daytona are cloud-only, CrewAI/LangGraph have no execution layer. MowisAI provides both the orchestration brain and the sandboxed execution body.

The system has three primary interfaces:
1. **mowis-desktop** — A Tauri 2.0 desktop app with a chat UI. Talks to **mowis-agent** via HTTP for single-agent coding tasks, and to **agentd** for multi-agent orchestration.
2. **mowis-cli** — A standalone CLI that connects to both agentd (via Unix socket/TCP) and mowis-agent (via HTTP).
3. **mowis-agent** — A Go binary (forked from OpenCode) that provides the unified coding agent: agent loop, 13+ tools, session management, LLM providers, permissions. Runs as an HTTP server on localhost:4096.

The core daemon **agentd** is a single Rust binary that provides:
- A Unix socket API for sandbox/container management
- 75 tools across 14 categories (filesystem, shell, git, docker, k8s, HTTP, etc.)
- A 7-layer orchestration pipeline (planner → topology → scheduler → agent execution → merge → verify → cross-sandbox merge)
- Multiple AI provider backends (Vertex AI, Gemini, Anthropic, OpenAI, Grok, Groq, Mimo)

---

## Workspace Structure

```
MowisAI/                          # Workspace root
├── Cargo.toml                    # Workspace manifest (4 members)
├── agentd/                       # Main daemon — binary + library (Rust)
├── agentd-protocol/              # Shared protocol types (no circular deps)
├── runtime/                      # Control plane — state management
├── mowis-agent/                  # Unified coding agent (Go, forked from OpenCode)
│   ├── cmd/                      # CLI commands (root, serve, setup, schema)
│   ├── internal/                 # Backend: agent, tools, providers, sessions, server
│   ├── main.go                   # Entry point
│   ├── go.mod                    # Module: github.com/mowisai/mowis-agent
│   └── sqlc.yaml                 # SQL code generation config
├── mowis-cli/                    # Standalone CLI (Rust, not in workspace)
├── mowis-desktop/                # Tauri desktop app
│   ├── src-tauri/                # Rust backend (workspace member)
│   └── src/                      # Frontend (JS/HTML/CSS)
├── .cargo/config.toml            # Cargo build config
├── .github/workflows/            # CI: build-windows.yml (3 jobs)
├── AGENTS.md                     # Architecture specification
├── README.md                     # Project documentation
├── CLAUDE.md                     # Claude-specific instructions
├── system.md                     # System instructions
├── deny.toml                     # cargo-deny config
├── scripts/perf-gate.sh          # Performance gate script
├── codemagic.yaml                # CI configuration
└── SKILL_FRONTNED (1).md         # Frontend skill docs (typo in filename intentional)
```

---

## Workspace Cargo.toml

**File:** `Cargo.toml` (12 lines)

- **Members:** `agentd`, `agentd-protocol`, `runtime`, `mowis-desktop/src-tauri`
- **Resolver:** 2
- **Workspace dependencies:** serde (1.0+derive), serde_json (1.0), anyhow (1.0), thiserror (1.0), log (0.4), env_logger (0.10)

Note: `mowis-cli` is NOT a workspace member — it's a standalone crate.

---

## Crate 1: `agentd` (Main Daemon)

**File:** `agentd/Cargo.toml` (67 lines)
**Version:** 0.2.0+1, Edition 2024
**Binary:** `agentd` (from `src/main.rs`)
**Library:** `libagent` (from `src/lib.rs`, rlib + cdylib)
**Dependencies:** 35+ crates including tokio (full), dashmap, ratatui, crossterm, aes-gcm, reqwest, nix, regex, walkdir

### `src/main.rs` (273 lines)

CLI entry point using clap.

**Structs:**
- `Args` (line 12): `commands: Option<Commands>`
- `OrchCommand` (line 20): `prompt`, `project`, `socket` (default `/tmp/agentd.sock`), `project_root` (default `.`), `mode` (default `auto`), `max_agents` (default 50), `verbose`, `save`

**Enum `Commands` (line 55):**
- `Socket { path }` — start socket server
- `Simulate(SimulateCommand)` — run mock orchestration
- `Orchestrate(OrchCommand)` — run real Gemini orchestration

**Binary entry point (line 69):**
- No subcommand: verifies `skopeo` installed, runs SetupWizard, creates config, auto-starts socket server via `ensure_socket_server()`, launches TUI via `libagent::tui::run_interactive()`
- `Socket` subcommand: calls `libagent::socket_server::run(&path)`
- `Simulate` subcommand: creates tokio runtime, runs `cmd.run()`
- `Orchestrate` subcommand: creates `OrchestratorConfig`, instantiates `NewOrchestrator`, runs `orchestrator.run(&cmd.prompt)`

**Helper functions:**
- `setup_signal_handlers()` (line 223): SIGINT/SIGTERM via signal_hook
- `ensure_socket_server()` (line 248): checks socket responsiveness, auto-starts daemon with sudo if needed

### `src/lib.rs` (352 lines)

Exports 39 public modules. Key re-exports at crate root:
- `Agent`, `AgentConfig`, `AgentResult`, `AgentCoordinator`, `AgentLoop`
- `AuditEvent`, `AuditLogger`, `SecurityAuditor`
- `ImageManager`, `VmHandle`, `boot_vm`, `exec_in_vm`, `stop_vm`
- `AgentMemory`, `LongTermMemory`, `ShortTermMemory`
- `Checkpointer`, `PersistenceManager`, `RecoveryJournal`, `WriteAheadLog`
- `ResourceLimits`, `Sandbox`, `SeccompFilter`, `SecurityContext`, `SecurityPolicy`
- `Tool`, `ToolContext`, `ToolDefinition`

**Global state:**
- `SHUTDOWN_FLAG: AtomicBool` (line 45) — global shutdown for signal handling

**Socket server management (lines 57-226):**
- `socket_is_responsive()` (line 66): UnixStream connect with 100ms timeout
- `get_socket_server_pid()` (line 83): `pgrep -f mowisai.*socket`
- `save_socket_pid()` / `read_socket_pid()` (line 99): persist to `~/.mowisai/.socket-server.pid`
- `start_socket_server_daemon()` (line 115): tries `sudo -n` first, falls back to interactive sudo

**C FFI (lines 248-340):**
- `agent_sandbox_new(ram, cpu)` → `*mut Sandbox`
- `agent_sandbox_run(sb, cmd)` → `*mut c_char`
- `agent_string_free(s)`, `agent_sandbox_free(sb)`
- `agent_memory_new(agent_id, session_id)` → `*mut AgentMemory`
- `agent_loop_new(agent_id, session_id, max_iter)` → `*mut AgentLoop`

### `src/agent.rs` (53 lines)

- `AgentConfig` (line 6): `{ model: String, tools: Vec<String>, resources: ResourceLimits }`
- `AgentResult` (line 13): `{ success: bool, output: Option<String> }`
- `Agent` (line 19): `{ sandbox: Sandbox, _config: AgentConfig }`
- `Agent::spawn(config)` (line 25): creates Sandbox from config.resources
- `Agent::run(prompt)` (line 30): placeholder that echoes prompt
- `Agent::spawn_subagent(config)` (line 38): creates child sandbox

### `src/agent_loop.rs` (394 lines)

Main agent execution loop with tool selection.

- `AgentLoop` (line 9): `{ agent_id: u64, memory: AgentMemory, max_iterations: usize, current_iteration: usize }`
- `AgentState` enum (line 17): `Idle`, `Planning`, `Executing`, `Reflecting`, `Done`
- `ToolSelectionStrategy` enum (line 26): `GreedyBest`, `Exploration`, `PatternMatching`, `RandomSelection`
- `LoopIteration` (line 34): `{ iteration, tool_used, input, output, success, reasoning }`
- `AgentCoordinator` (line 292): `{ agents: DashMap<u64, Mutex<AgentLoop>>, next_agent_id: AtomicU64 }`

**Key logic:**
- `AgentLoop::run()` (line 54): main loop — initializes task, iterates up to max_iterations: `plan()` → `execute_tool()` → `reflect()`
- `select_tool()` (line 168): keyword-based heuristic matching (read/file → read_file, write → write_file, etc.)
- `AgentCoordinator::spawn_agent()` (line 305): allocates unique ID, creates AgentLoop

### `src/sandbox.rs` (1036 lines)

Core sandboxing with overlayfs, chroot, cgroups.

- `ResourceLimits` (line 5): `{ ram_bytes: Option<u64>, cpu_millis: Option<u64> }`
- `Container` (line 48): `{ id: u64, upper: PathBuf, work: PathBuf, root: PathBuf }`
- `ToolInvocationPrep` (line 58): `{ tool, sandbox_id, container_root, policy, tool_name }`
- `Sandbox` (line 78): `{ id, limits, root: TempDir, tools, policy, image_path, sandbox_upper, limits_enforced, containers, project_root, scope }`

**Key methods:**
- `Sandbox::new()` / `new_with_image()` (line 136): creates sandbox with overlayfs (image as lowerdir, tmpfs upper), applies cgroup limits
- `restore()` (line 240): restores from persisted metadata
- `spawn_child()` (line 290): enforces inheritance rules (child limits clamped to parent)
- `seed_git_repo()` (line 338): clones git repo into sandbox /workspace
- `apply_limits()` (line 372): writes to cgroup v2 (`memory.max`, `cpu.max`)
- `prepare_tool_invocation()` (line 548): comprehensive policy checks, clones tool, prepares for lock-free execution
- `create_container()` (line 622): creates overlayfs container on top of sandbox image layer, mounts workspace overlay with project root
- `checkpoint_container()` / `restore_container()` (line 832): snapshot/restore via `cp -a`
- `destroy_container()` (line 892): unmounts and removes
- `run_command()` (line 919): chroot + namespace isolation (CLONE_NEWNS|NEWPID|NEWUSER|NEWNET|NEWIPC|NEWUTS), applies RLIMIT_AS/NOFILE/NPROC

### `src/socket_server.rs` (1036+ lines)

Unix socket API server with dual-lane worker pools.

**Wire types:**
- `SocketRequest` (line 59): `request_type`, plus optional fields for every operation
- `SocketResponse` (line 97): `{ status, result, error }`

**Constants:** `FAST_WORKERS = 64`, `SLOW_WORKERS = 128`

**Global state (lines 23-55):**
- `SANDBOXES: DashMap<u64, Sandbox>` — all active sandboxes
- `AUDITOR: SecurityAuditor` — audit logger
- `PERSISTENCE: PersistenceManager` — state persistence
- `MEMORY_STORE: DashMap<u64, AgentMemory>` — agent memory
- `COORDINATOR: AgentCoordinator` — multi-agent coordinator
- `VM_HANDLES: DashMap<u64, VmHandle>` — VM handles

**Supported request types (line 421+):**
`create_sandbox`, `create_container`, `invoke_tool`, `list`, `list_containers`, `destroy_container`, `destroy_sandbox`, `register_tool`, `set_policy`, `get_policy`, `get_audit_stats`, `get_anomalies`, `create_channel`, `send_message`, `read_messages`, `bucket_put`, `bucket_get`, `memory_set`, `memory_get`, `memory_save`, `memory_load`, `agent_spawn`, `agent_run`, `agent_status`, `create_checkpoint`, `restore_checkpoint`

**Request lane classification (line 414):**
- Fast lane: `list`, `list_containers`, `get_policy`, `get_audit_stats`, `get_anomalies`, `agent_status`, `bucket_get`, `memory_get`, `memory_load`, `read_messages`
- Slow lane: everything else

**Connection architecture:**
- `run()`: creates UnixListener, spawns 64 fast + 128 slow worker threads
- Each connection: reads one JSON line, classifies to lane, dispatches
- `ParsedConnection`, `WorkerJob` structs (line 1053)
- `configure_stream()` (line 1062): 30s read timeout, 10s write timeout

### `src/config.rs` (239 lines)

- `AiProvider` enum (line 7): `VertexAi`, `Grok`, `Groq`, `Anthropic`, `OpenAi`, `Gemini`, `Mimo`
- `MowisConfig` (line 37): provider, GCP project, encrypted API keys (AES-256-GCM), model names, socket path, max agents, overlay/checkpoint/merge paths
- `config_dir()` → `~/.mowisai/`
- `load()` (line 137): reads TOML
- `save()` (line 147): writes TOML with 0o700/0o600 permissions
- API key getters (lines 172-219): decrypt via `crypto::decrypt()`

### `src/setup.rs` (1058 lines)

Interactive setup wizard with arrow-key navigation.

- `SetupWizard` (line 187): unit struct
- `needs_setup()` (line 190): checks if config exists and is valid
- `run()` (line 197): full interactive setup — clear screen, print banner, pick provider, provider-specific setup, save
- `pick_provider()` (line 268): arrow-key navigation menu using crossterm raw mode
- `setup_vertex()` (line 382): checks gcloud CLI, checks auth, detects/probes GCP project
- `setup_grok()` (line 478): masked API key input, validates xAI key format
- Model constants: GROK_MODELS (6), GROQ_MODELS (4), ANTHROPIC_MODELS (3), OPENAI_MODELS (4), GEMINI_MODELS (3), MIMO_MODELS (7)

### `src/security.rs` (444 lines)

- `SecurityPolicy` (line 6): `{ name, allowed_syscalls, denied_syscalls, resource_limits, file_access_rules, network_rules, allow_shell_execution }`
- `ResourceSecurityLimits` (line 17): `{ max_memory_mb, max_cpu_percent, max_open_files, max_processes }`
- `FileAccessRule` (line 25): `{ path, allow_read, allow_write, allow_execute }`
- `NetworkRule` (line 33): `{ allow_outbound, allow_inbound, allowed_ports, blocked_ports }`
- `SeccompFilter` (line 193): wraps SecurityPolicy, `to_bpf_rules()` generates JSON seccomp BPF rules
- `SecurityContext` (line 297): `{ policy, capabilities, mac_policy, user_network_ipc_pid_uts_namespace }`
- `ThreatAnalyzer` (line 338): `analyze_syscall()`, `analyze_resource_usage()`, `generate_report()`
- `default_restrictive()` (line 42): whitelist of 18 safe syscalls, denies 16 dangerous, 256MB RAM, no network
- `default_permissive()` (line 114): allow all, 1GB RAM, network allowed

### `src/audit.rs` (387 lines)

- `EventType` enum (line 10): 18 variants — SandboxCreated, SandboxDestroyed, ToolRegistered, ToolInvoked, ToolFailed, MemoryStored, MemoryRetrieved, TaskStarted, TaskCompleted, TaskFailed, ChannelCreated, MessageSent, MessageReceived, AgentSpawned, AgentTerminated, SecurityViolation, ResourceLimitExceeded, CheckpointCreated, StateRestored, Custom(String)
- `AuditEvent` (line 34): `{ timestamp, event_type, actor_id, target_id, description, details, result }`
- `AuditLogger` (line 78): `{ log_file, buffer, buffer_size }`
- `SecurityAuditor` (line 190): `{ logger, stats }` — `record_event()`, `detect_anomalies()`, `get_stats()`
- `ComplianceChecker` (line 260): policy compliance checking
- `ReplayEngine` (line 289): event replay for debugging — `filter_by_actor()`, `filter_by_type()`, `replay_tool_invocations()`, `timeline()`

### `src/crypto.rs` (105 lines)

AES-256-GCM encryption with SHA-256 key derivation from machine-specific ID.

- `machine_id()` (line 18): reads or generates random 32-byte machine ID at `~/.mowisai/machine-id` (0o400)
- `machine_key()` (line 52): `SHA256(machine_id + "|mowisai-provider-key-v1")`
- `encrypt(plaintext)` (line 62): random 12-byte nonce, AES-256-GCM, returns `"<nonce_b64>:<ciphertext_b64>"`
- `decrypt(encoded)` (line 80): splits on `:`, base64-decodes, decrypts

### `src/memory.rs` (362 lines)

Short-term + long-term agent memory with semantic matching.

- `ShortTermMemory` (line 10): `{ session_id, context, task_stack: Vec<TaskFrame>, recent_results }` (max 50 results)
- `TaskFrame` (line 17): `{ task_id, goal, state: TaskState, tools_used, subtasks }`
- `TaskState` enum (line 26): `Pending`, `Running`, `Completed`, `Failed`, `Blocked`
- `LongTermMemory` (line 92): `{ agent_id, knowledge_base, pattern_index, semantic_cache, decision_log }`
- `KnowledgeEntry` (line 101): `{ key, value, embedding: Vec<f32>, confidence, source, created_at, accessed_count }`
- `AgentMemory` (line 196): `{ short_term, long_term }`
- `SemanticMatcher` (line 237): `cosine_similarity()`, `find_similar_knowledge()`, `suggest_tools()`
- `MemoryPersistence` (line 282): static save/load methods for STM, LTM, full memory

### `src/persistence.rs` (350 lines)

- `PersistedSandbox` (line 8): `{ id, created_at, last_updated, ram_bytes, cpu_millis, root_path, metadata, tools_registered, state_summary }`
- `PersistenceManager` (line 22): `{ base_path }` — `init()`, `save_sandbox()`, `load_sandbox()`, `list_sandboxes()`, `delete_sandbox()`, `save_agent_memory()`, `load_agent_memory()`
- `Checkpointer` (line 137): wraps PersistenceManager — `save_checkpoint()`, `load_checkpoint()`, `list_checkpoints()`
- `WriteAheadLog` (line 197): `{ log_path }` — `append()`, `read_all()`, `clear()`
- `RecoveryJournal` (line 244): `{ journal_path }` — `mark_checkpoint()`, `add_pending_operation()`, `get_pending_operations()`

### `src/channels.rs` (72 lines)

- `Message` (line 8): `{ from: u64, to: u64, payload: String }`
- `Channel` (line 16): `{ id: u64, from: u64, to: u64 }`
- `create_channel(from, to)` → channel_id
- `send_message(channel_id, msg)` — validates sender matches channel.from
- `read_messages(channel_id)` — returns cloned messages (non-destructive)

### `src/buckets.rs` (42 lines)

- `BucketStore` (line 7): `{ base: PathBuf }` — `put(key, value)` via `fs::write`, `get(key)` via file read

### `src/dependency_graph.rs` (285 lines)

- `GraphError` enum (line 11): `CyclicDependency`, `MissingDependency`, `InvalidTask`
- `DependencyGraphBuilder` (line 21): `{ tasks: HashMap<String, TaskNode> }`
- `add_task()` (line 26), `build()` (line 46): checks cycles, validates deps, computes execution stages via Kahn's algorithm
- `check_cycles()` (line 63): DFS cycle detection with recursion stack
- `compute_execution_stages()` (line 119): Kahn's algorithm → `Vec<Vec<String>>` (stages of parallel tasks)
- `ComplexityAnalyzer` (line 181): `estimate_sandbox_count()` (1 per 100 complexity), `estimate_ram_per_sandbox()` (512MB + 10MB per unit), `estimate_cpu_per_sandbox()`

### `src/image_manager.rs` (301 lines)

- `ImageManager` (line 8): `{ cache_dir }` — defaults to `AGENTD_IMAGE_CACHE` env or `/var/lib/agentd/images`
- `resolve(image_ref)` (line 35): dispatches to `resolve_local()`, `resolve_http()`, or `resolve_registry()`
- `resolve_local()` (line 51): handles directory paths and tarballs
- `resolve_http()` (line 80): downloads tarball via curl, caches by MD5 hash
- `resolve_registry()` (line 102): normalizes "alpine" to `docker.io/library/alpine:latest`, uses `skopeo copy` to pull

### `src/version.rs` (38 lines)

- `VERSION` from `CARGO_PKG_VERSION`
- `get_version()` → `"MowisAI v{VERSION} ({ARCH})"`
- `full_version()` → `"MowisAI v{VERSION} ({ARCH}-{build_type})"`

### `src/logging.rs` (152 lines)

- `MAX_LOG_SIZE = 10 MB`, `MAX_LOG_BACKUPS = 5`
- `StructuredLogger` (line 20): `{ file, path, bytes_written }`
- `log(level, message)` (line 94): writes JSON `{"ts": N, "level": "...", "msg": "..."}`, forwards to TUI, mirrors to stderr
- TUI log integration via `TUI_LOG_SENDER` and `TUI_ACTIVE` globals

### `src/intent.rs` (348 lines)

Score-based intent classifier for Chat vs Build.

- `UserIntent` enum (line 18): `Chat`, `Build`
- `classify_intent(message)` (line 28):
  - Hard Chat overrides (line 33): "what is", "how does", "explain", etc.
  - Strong Build signals (line 49): 2 points each — "create a", "build a", "implement", etc.
  - Weak Build signals (line 80): 1 point each — "website", "dashboard", "api", etc.
  - Decision: Build if build_score >= chat_score && build_score > 0

### `src/tool_registry.rs` (411 lines)

Registers **75 tools** across 14 categories:

| Category | Count | Tools |
|----------|-------|-------|
| Filesystem | 11 | read_file, write_file, append_file, delete_file, copy_file, move_file, list_files, create_directory, delete_directory, get_file_info, file_exists |
| Shell | 5 | run_command, run_script, kill_process, get_env, set_env |
| HTTP | 6 | http_get/post/put/delete/patch, download_file |
| WebSocket | 1 | websocket_send |
| Data | 5 | json_parse/stringify/query, csv_read/write |
| Git | 9 | git_clone/status/add/commit/push/pull/branch/checkout/diff |
| Docker | 7 | docker_build/run/stop/ps/logs/exec/pull |
| Kubernetes | 6 | kubectl_apply/get/delete/logs/exec/describe |
| Memory | 6 | memory_set/get/delete/list/save/load |
| Secrets | 2 | secret_set/get |
| Package managers | 3 | npm_install, pip_install, cargo_add |
| Web | 3 | web_search, web_fetch, web_screenshot |
| Agent coordination | 6 | create_channel, send_message, read_messages, broadcast, wait_for, spawn_agent |
| Dev tools | 5 | lint, test, build, type_check, format |
| Search | 4 | grep, find_files, search_code, read_multiple_files |
| Utility | 1 | echo |

- `ToolRegistry` (line 15): `{ tools: Mutex<HashMap<&'static str, ToolFactory>> }`
- `TOOL_REGISTRY` — lazy_static singleton

### `src/vm_backend.rs` (144 lines)

- `VmBackend` enum (line 13): `Qemu`, `Firecracker`
- `VmHandle` (line 19): `{ sandbox_id, pid, backend, ssh_port, ssh_key, rootfs_path }`
- `detect_vm_backend()` (line 29): checks `/dev/kvm` and firecracker binary
- `boot_vm()` (line 38): currently returns stub (VM backend temporarily disabled)
- `map_tool_to_ssh()` (line 75): maps tool names to shell commands
- `ssh_exec()` (line 118): runs command via SSH with ed25519 key

---

### Agent Backends

All in `agentd/src/`. Each implements an AI provider's tool-calling loop.

#### `gemini_agent.rs` (106 lines)
- `run(prompt, api_key, model, socket_path)` — creates sandbox/container via socket, runs Gemini tool-calling loop (up to 64 rounds) via `generativelanguage.googleapis.com`
- `stream_chat(api_key, model, contents, tx)` — SSE streaming via `streamGenerateContent`

#### `anthropic_agent.rs` (162 lines)
- `run(prompt, api_key, model, socket_path)` — Anthropic Messages API at `api.anthropic.com/v1/messages`, supports thinking blocks for claude-3-7 models
- `anthropic_tool_declarations()` — maps all 75 tools to Anthropic's `input_schema` format

#### `openai_agent.rs` (132 lines)
- `run(prompt, api_key, model, socket_path)` — OpenAI Chat Completions at `api.openai.com/v1/chat/completions`, supports o-series reasoning models
- `openai_tool_declarations()` — maps tools to OpenAI function-calling format

#### `groq_agent.rs` (85 lines)
- `stream_chat(api_key, model, messages, tx)` — Groq API at `api.groq.com/openai/v1/chat/completions`, 180s timeout, SSE streaming

#### `grok_agent.rs` (415 lines)
- `run(prompt, api_key, model, socket_path)` — xAI API at `api.x.ai/v1/chat/completions`, OpenAI-compatible format
- `grok_tool_declarations()` — 5 tools (read_file, write_file, run_command, list_directory, delete_file)
- Full socket helpers: `socket_roundtrip()`, `parse_ok_field()`, `build_socket_request()`

#### `vertex_agent.rs` (1172 lines)
- `run(prompt, project_id, socket_path)` — Vertex AI Gemini 2.5 Pro at `us-central1-aiplatform.googleapis.com`, uses `gcloud auth print-access-token`
- `gemini_tool_declarations()` — comprehensive declarations for ALL 75+ tools with full JSON schemas

#### `hub_agent.rs` (578 lines)
- `LocalHubAgent` — runs inside each sandbox, manages worker pool
- `HubAgentConfig` (line ~30): `{ team_id, sandbox_id, max_workers, socket_path, peer_sockets }`
- `WorkerEntry`: `{ name, container_id, assignment, completion, status: WorkerStatus }`
- `WorkerStatus` enum: `Idle`, `Assigned`, `Running`, `Completed`, `Failed`
- Methods: `init_worker_pool()`, `receive_team_task()`, `break_down_task()`, `assign_to_worker()`, `record_worker_completion()`, `collect_outputs()`, `run_integration_tests()`, `create_completion_report()`, `handle_peer_rpc()`, `register_api_contract()`, `start_socket_server()`

#### `hub_agent_client.rs` (219 lines)
- `HubAgentClient` — `{ socket_path, request_timeout }`
- Methods: `assign_task()`, `wait_for_completion()`, `get_status()`

#### `guest_backend.rs` (74 lines)
- `boot_guest_os_scaffold(root, image_hint)` — tries `/sbin/init`, falls back to keepalive shell loop, spawns via chroot
- `stop_guest_os(pid)` — sends SIGTERM

#### `worker_agent.rs` (463 lines)
- `WorkerAgent` — `{ config, client: AgentdClient, state: WorkerExecutionState, current_assignment, execution_history, planning_steps, work_output }`
- `WorkerExecutionState` enum: `Idle`, `Assigned`, `Thinking`, `ExecutingTool`, `Testing`, `Completed`, `Failed`
- Methods: `receive_assignment()`, `execute_task()` (plan → execute → test), `create_completion()`, `signal_idle()`

---

### Tools Subsystem (`agentd/src/tools/`)

#### `mod.rs` (295 lines)
Module declarations for 15 submodules. 80+ factory functions (`create_*_tool()`).

#### `common.rs` (79 lines)
- `ToolContext` (line 6): `{ sandbox_id: u64, root_path: Option<PathBuf> }`
- `Tool` trait: `name() -> &'static str`, `invoke(&self, ctx, input) -> Result<Value>`, `clone_box() -> Box<dyn Tool>`
- `ToolDefinition` (line 18): `{ name: String }`
- `resolve_path(ctx, path)` — resolves relative paths against container root
- Global stores: `MEMORY_STORE`, `SECRET_STORE`, `CHANNELS`

#### `filesystem.rs` (200+ lines)
11 tools: `ReadFileTool`, `WriteFileTool`, `AppendFileTool`, `DeleteFileTool`, `CopyFileTool`, `MoveFileTool`, `ListFilesTool`, `CreateDirectoryTool`, `DeleteDirectoryTool`, `GetFileInfoTool`, `FileExistsTool`. Each implements `Tool` trait with `resolve_path()` for container-scoped operations.

#### `shell.rs` (200+ lines)
5 tools: `RunCommandTool` (chroot + PID namespace isolation, 30s timeout), `RunScriptTool` (inline or file-based, supports python/node/sh), `KillProcessTool` (SIGTERM via nix), `GetEnvTool`, `SetEnvTool`.

#### `web.rs` (101 lines)
3 tools: `WebSearchTool` (DuckDuckGo API), `WebFetchTool` (curl -L), `WebScreenshotTool` (chromium headless)

#### `git.rs` (200+ lines)
9 tools: All set GIT_AUTHOR/COMMITTER to `agentd <agentd@mowisai.com>`. Support both chroot and direct execution modes.

#### `docker.rs` (7 tools), `kubernetes.rs` (6 tools), `http.rs` (7 tools), `search.rs` (4 tools), `storage.rs` (8 tools), `data.rs` (5 tools), `channels.rs` (5 tools), `package_managers.rs` (3 tools), `dev_tools.rs` (5 tools), `utils.rs` (EchoTool + SpawnAgentTool)

---

### TUI Subsystem (`agentd/src/tui/`)

#### `mod.rs` (116 lines)
- `run_interactive(config, socket_pid)` — enables raw mode, enters alternate screen, creates Terminal with CrosstermBackend, runs event loop
- Event loop drains all pending `TuiEvent`s per frame

#### `app.rs` (200+ lines)
- `MainView` enum: `Chat`, `Orchestration`, `Development`
- `MessageRole` enum: `User`, `Assistant`, `System`
- `ChatMessage` (TUI): `{ role, content }`
- `AgentInfo`: `{ agent_id, description, status, current_tool }`
- `App` struct: extensive state including config, socket_pid, event_tx, view_mode, messages, input_text, scroll_offset, is_loading, spinner_frame, orchestrating, agents, dev_log, pending_diff, save_selector, mode_override, should_quit, cwd

#### `event.rs` (60 lines)
- `OrchActivityEvent` enum: `AgentStarted`, `ToolCall`, `AgentCompleted`, `AgentFailed`, `LayerProgress`, `StatsUpdate`
- `TuiEvent` enum: `Key`, `Tick`, `GeminiChunk`, `GeminiDone`, `GeminiError`, `OrchEvent`, `OrchComplete`, `OrchDone`, `LogEntry`

#### `ui.rs` (100+ lines)
- `draw()` — main layout: title bar (1 row), content area (min 5 rows), input (3 rows), status bar (1 row)
- Color palette: GREEN, BRIGHT_GREEN, DARK_GREEN, DIM, WHITE, AMBER, RED, BLUE

#### `widgets.rs` (75 lines)
- `Spinner` widget — 8-frame braille animation with label
- `MessagePreview` widget — single-line chat message with role-colored prefix

---

### Orchestration Subsystem (`agentd/src/orchestration/`)

The 7-layer orchestration pipeline.

#### `mod.rs` (1435 lines)

Root module. Constants, gcloud auth, Vertex AI URL generation, Gemini tool declarations (75 tools), socket protocol client.

**Constants:**
- `HTTP_TIMEOUT_SECS = 900` (line 35)
- `MAX_TOOL_ROUNDS = 256` (line 38)
- `VERTEX_MAX_OUTPUT_TOKENS = 65536` (line 41)
- `VERTEX_THINKING_BUDGET_TOKENS = 24576` (line 44)

**Key functions:**
- `gcloud_access_token()` (line 146): gets OAuth access token via gcloud CLI, handles SUDO_USER HOME passthrough
- `vertex_generate_url(project_id)` (line 228): builds Vertex AI generateContent URL
- `gemini_tool_declarations()` (line 236): returns JSON array of all 75 tool declarations (936 lines of JSON)
- `socket_roundtrip(socket_path, req)` (line 1187): direct socket request with 3-attempt retry logic
- `pooled_socket_request(socket_path, req)` (line 1343): pooled socket request via socket_client module
- `parse_ok_field(resp, key)` (line 1361): extract string/number field from ok response
- `invoke_tool_via_socket(...)` (line 1396): invoke tool on sandbox/container via socket

#### `types.rs` (134 lines)

**Deprecated types (old 5-layer):** `ProjectContext`, `ImplementationBlueprint`, `SandboxConfig`, `SandboxExecutionPlan`, `SandboxResult`, `SandboxWarmState`

**New 7-layer types:**
- `DepCounter` (line 64): `{ count: AtomicUsize }` — dependency counter for scheduler
- `SandboxState` (line 78): `{ name, base_layer_path, sandbox_layer_path, scope, tools, max_agents, active_agents, idle_agents }`
- `AgentPool` (line 91): `{ agents: RwLock<Vec<AgentHandle>>, max_size: usize }` — `take_idle()`, `return_idle()`
- `MergeNode` enum (line 121): `Leaf { diff }` | `Branch { left, right }` — merge tree node
- `VerificationTask` (line 128): `{ test_id, description, command, expected_result }`

#### `sandbox_topology.rs` (1415 lines)

Layer 2 — Overlayfs Topology. Delegates to agentd socket API.

- `TopologyManager` (line 40): `{ socket_path, sandboxes, sandbox_scopes, containers, agent_sandboxes, project_root, staged_workspaces, sleeping_containers }`
- `SleepingContainer` (line 22): `{ agent_id, container_id, sandbox_id, paused_at }` — reusable sleeping container
- `StagedWorkspace` (line 31): `{ agent_id, sandbox_name, container_id, staging_path }`

**Key methods:**
- `create_sandbox_layer(config)` (line 90): create sandbox via socket
- `create_agent_layer(sandbox_name, task_id)` (line 131): create container, init git repo with base commit
- `capture_agent_diff(agent_id)` (line 277): two-strategy diff — host-side `git diff --no-index`, then socket `git diff --cached HEAD`
- `sleep_agent_layer(agent_id, sandbox_name)` (line 845): put container to sleep in pool
- `wake_or_create_agent_layer(...)` (line 899): wake sleeping container or create new
- `apply_diff_to_sandbox(sandbox_name, diff)` (line 1030): two-strategy apply — host-side `git apply`, then socket
- `destroy_agent_layer(agent_id)` (line 968): destroy container via socket
- `stage_agent_workspace(...)` (line 767): stage workspace before container destruction
- `export_staged_workspaces(output_dir)` (line 812): export all staged workspaces

#### `scheduler.rs` (352 lines)

Layer 3 — Event-driven task dispatcher.

- `Scheduler` (line 17): `{ task_graph, dep_counter: DashMap, dependents, sandbox_hints, ready_queues, running, completed, failed, idle_agents, results }`
- `SchedulerStats` (line 269): `{ total_tasks, completed, failed, running, pending }`

**Key methods:**
- `new(task_graph, sandbox_hints)` (line 51): build task map, dep counters, reverse deps, initial ready queues
- `get_ready_task(sandbox_name)` (line 115): pop next ready task from sandbox queue (O(1))
- `mark_task_started(task_id, agent)` (line 127): record task as running
- `handle_task_completion(result)` (line 134): decrement dependent counters, enqueue newly-ready tasks
- `handle_task_failure(task_id, error)` (line 191): mark task as failed
- `register_idle_agent(agent)` (line 208): register idle agent for sandbox

#### `checkpoint.rs` (273 lines)

Layer 4 checkpoint save/restore.

- `CheckpointLog` (line 16): `{ agent_id, task_id, checkpoints: Vec<Checkpoint>, log_path }`
- `CheckpointManager` (line 95): `{ checkpoint_root, socket_path }`
- `create_snapshot(agent_id, checkpoint_id, sandbox_id, container_id)` (line 115)
- `restore_snapshot(sandbox_id, container_id, snapshot_path)` (line 157)

#### `merge_worker.rs` (454 lines)

Layer 5 — Parallel tree-pattern merge.

- `MergeResult` (line 11): `{ success, merged_diff, conflicts_resolved, unresolved_conflicts }`
- `ParallelMergeCoordinator` (line 19): `{ project_id, work_dir, base_repo_path, max_conflict_retries }`
- `merge_diffs(diffs)` (line 39): tree-pattern merge — pairs diffs in rounds (log2(N) rounds)
- `merge_two_diffs(diff1, diff2, ...)` (line 119): merge with git apply, LLM repair on failure
- `repair_conflict(diff1, diff2, conflict_text, project_id, max_retries)` (line 341): LLM conflict repair via Gemini

#### `merge_reviewer.rs` (934 lines)

LLM-powered intelligent merge reviewer.

- `FileChange` (line 17): `{ file_path, change_type: ChangeType, hunks: Vec<DiffHunk>, raw_diff }`
- `ChangeType` enum (line 24): `Added`, `Modified`, `Deleted`, `Renamed { from }`
- `AgentContribution` (line 43): `{ agent_id, task_id, task_description, file_changes, raw_diff }`
- `MergeConflict` (line 54): `{ conflict_type, file_path, agents_involved, description, severity }`
- `ConflictSeverity` enum (line 69): `Low`, `Medium`, `High`, `Critical`
- `MergeDecision` (line 80): `{ file_path, action: MergeAction, final_content, reasoning }`
- `MergeAction` enum (line 88): `Accept { from_agent }`, `Merge`, `Reject { reason }`, `Delete`
- `ConflictDetector` (line 270): static conflict detection — build file map, check delete/modify, overlapping hunks
- `MergeReviewerAgent` (line 384): `{ llm_config, max_retries }` — auto-accept non-conflicting, LLM resolve conflicts

#### `verification.rs` (912 lines)

Layer 6 — VeriMAP-style verification.

- `VerificationFunction` (line 25): `{ id, description, command, expected_schema, assertion, deps }`
- `VerificationPlan` (line 47): `{ sandbox_name, vfs: Vec<VerificationFunction> }`
- `VerificationPlanner` (line 74): `{ llm_config, max_rounds, max_test_execution_time }`
- `VerificationLoop` (line 244): `{ planner, max_rounds }`
- `verify_sandbox(sandbox_name, merged_diff, original_tasks, topology, agent_executor)` (line 269): main verification loop — VFs generated ONCE, executed every round

#### `agent_execution.rs` (475 lines)

Layer 4 — Provider-agnostic tool-calling loop.

- `AgentExecutor` (line 24): `{ llm_config, socket_path, checkpoint_manager, max_tool_rounds, max_tier1_retries, max_tier2_retries }`
- `execute_task(agent, task_description, tools, system_prompt)` (line 50): main entry — tier 2 retry loop
- `execute_with_checkpoints(agent, ..., checkpoint_log)` (line 155): tool-calling loop with checkpoints
- `execute_tool_with_retry(...)` (line 346): tier 1 retry (max 3 retries per tool)
- `capture_git_diff(agent)` (line 400): capture diff via `git add -A` + `git diff --cached HEAD`

#### `new_orchestrator.rs` (1389 lines)

Main orchestrator entry point with 3 execution modes.

- `OrchestratorEvent` enum (line 7): `TaskStarted`, `ToolCall`, `ToolResult`, `TaskCompleted`, `TaskFailed`, `StatsUpdate`, `LayerProgress`, `Done`
- `FinalOutput` (line 34): `{ merged_diff, sandbox_results, verification_status, failed_tasks, known_issues, summary, total_agents_used, total_duration_secs, scheduler_stats, execution_errors }`
- `OrchestratorConfig` (line 54): `{ llm_config, socket_path, project_root, overlay_root, checkpoint_root, merge_work_dir, max_agents, max_verification_rounds, staging_dir, event_tx, mode_override }`
- `NewOrchestrator` (line 82): `{ config }`
- `run(prompt)` (line 99): routing gate — scans directory tree, classifies complexity (Simple/Standard/Full), dispatches to `run_simple()`, `run_standard()`, or full 7-layer pipeline

**Full 7-layer pipeline flow:**
1. Layer 1: `plan_task()` (planner)
2. Layer 2: `TopologyManager::create_sandbox_layer()` (overlayfs)
3. Layer 3: `Scheduler::new()` (event-driven dispatch)
4. Layer 4: `AgentExecutor` (parallel agent execution)
5. Layer 5: Parallel merge (merge workers)
6. Layer 6: `VerificationLoop` (test loop)
7. Layer 7: Cross-sandbox merge

#### `planner.rs` (444 lines)

Layer 1 — Fast planner: shell scan + single LLM call.

- `PlannerOutput` (line 16): `{ task_graph, sandbox_topology, sandbox_hints }`
- `plan_task(prompt, project_root, llm_config)` (line 23): full-mode planner
- `plan_task_standard(prompt, project_root, llm_config, dir_tree)` (line 62): constrained standard-mode (1 sandbox, <=3 tasks)
- `scan_directory_tree(root)` (line 161): shell scan (`tree -L 3` or `find`, with Windows fallback)

#### `mock_agent.rs` (299 lines)

Deterministic mock agent for $0 testing.

- `MockAgentExecutor` (line 15): `{ failure_rate, tool_delay_ms, verbose }`
- `execute_task(agent, task_index, total_tasks, topology)` (line 36): write mock JS file + run ls, capture diff

#### `simulate.rs` (994 lines)

Full 7-layer orchestration simulation at $0 cost.

- `SimulateCommand` (line 262): CLI command struct with fields for socket, tasks, sandboxes, max_agents, failure_rate, tool_delay, project_root, output_dir, verbose, verify, verify_failure_rate, no_save
- `run()` (line 316): full 7-layer simulation pipeline

#### `health.rs` (258 lines)

Health monitoring and circuit breakers.

- `CircuitState` enum (line 10): `Closed`, `Open`, `HalfOpen`
- `CircuitBreaker` (line 21): `{ state, consecutive_failures, last_failure_time, total_failures }`
- `HealthMonitor` (line 49): `{ heartbeats, circuit_breakers, heartbeat_timeout_secs, failure_threshold, recovery_timeout_secs }`
- `heartbeat(agent_id)` (line 74), `record_failure(sandbox_name)` (line 87), `is_sandbox_healthy(sandbox_name)` (line 128), `get_dead_agents()` (line 155)

#### `complexity_classifier.rs` (451 lines)

Pre-orchestration complexity classifier. Pure heuristics, ~1ms, zero LLM cost.

- `ComplexityMode` enum (line 26): `Simple`, `Standard`, `Full`
- `ComplexityScore` (line 64): `{ domain_count, file_count, broad_scope, cross_service, score, mode }`
- `classify_complexity(prompt, dir_tree)` (line 173): score 0/1/2 → Simple/Standard/Full
- Keyword constants: `GREENFIELD_KEYWORDS` (20), `SINGLE_ARTIFACT_KEYWORDS` (17), `DOMAIN_DIRS` (28), `BROAD_SCOPE_KEYWORDS` (12), `CROSS_SERVICE_KEYWORDS` (10), `SIMPLE_ACTION_KEYWORDS` (16)

#### `provider_client.rs` (1074 lines)

Provider-agnostic LLM client supporting 7 providers.

- `LlmConfig` (line 18): `{ provider, model, vertex_project_id, api_key }`
- `ToolCall` (line 119): `{ id, name, args }`
- `AgentConversation` (line 137): `{ messages: Vec<ConvMessage> }`
- `AgentRoundResult` (line 165): `{ text, tool_calls }`

**Public functions:**
- `generate_text(llm_config, system_prompt, user_message, json_mode, temperature)` (line 175)
- `call_agent_round(llm_config, system_prompt, conversation, allowed_tools, temperature)` (line 434)

**Provider-specific implementations:**
- `call_agent_round_gemini()` (line 475)
- `call_agent_round_openai_compat()` (line 576)
- `call_agent_round_anthropic()` (line 677)

#### `session_store.rs` (79 lines)

- `InteractiveSessionSnapshot` (line 15): `{ schema_version, project_id, socket_path, max_agents, context, transcript, sandbox_by_team, warm_by_sandbox, assistant_turns }`
- `write_snapshot(path, snap)` (line 53): atomic write (tmp + rename)
- `read_snapshot(path)` (line 67): read and validate schema version

#### `sandbox_profiles.rs` (68 lines)

Pre-defined Alpine package sets per team type.

- `get_packages_for_team(team_type)` (line 4): frontend, backend, devops, testing, data, security, general
- `merge_packages(team_type, extra)` (line 58): merge profile packages with extras, deduplicated

#### `socket_client.rs` (257 lines)

Bounded socket client pool to prevent EMFILE. Unix-only.

- `POOL_WORKERS = SLOW_WORKERS * 3 / 4`, `QUEUE_DEPTH = 512`, `READ_TIMEOUT_SECS = 60`, `MAX_RETRIES = 3`
- `SocketClientPool` (line 51): `{ sender: SyncSender<PoolRequest> }` — spawns POOL_WORKERS threads
- `submit(socket_path, payload)` (line 90): submit request, block for reply
- `socket_request(socket_path, req)` (line 243): public API — init pool once, submit request

---

## Crate 2: `agentd-protocol`

**File:** `agentd-protocol/Cargo.toml` (13 lines)
**Version:** 0.1.0, Edition 2021
**Dependencies:** serde (workspace), serde_json (workspace)
**Purpose:** Shared protocol types between agentd and runtime. Pure types — no logic, no functions.

### `src/lib.rs` (366 lines)

**Legacy types (old 5-layer):**
- `TeamTask` (line 9): `{ task_id, team_id, description, dependencies, estimated_complexity, timeout_secs, context }`
- `ProvisioningSpec` (line 21): `{ request_id, num_sandboxes, sandbox_specs, max_concurrent_agents_per_sandbox }`
- `SandboxSpec` (line 30): `{ sandbox_id, os_image, ram_bytes, cpu_millis, init_packages, initial_containers }`
- `ProvisioningReady` (line 41): `{ request_id, sandboxes, timestamp }`
- `SandboxHandle` (line 49): `{ sandbox_id, socket_path, containers }`
- `ContainerHandle` (line 57): `{ container_id, sandbox_id, status }`
- `ContainerStatus` enum (line 65): `Creating`, `Ready`, `Active`, `Paused`, `Terminated`
- `SandboxHealthStatus` (line 75): `{ sandbox_id, hub_agent_alive, container_states, ram_usage_bytes, cpu_usage_millis, timestamp }`
- `TaskCompletion` (line 86): `{ task_id, team_id, success, output, errors, timestamp }`
- `WorkerAssignment` (line 97): `{ assignment_id, worker_name, task_description, system_prompt, tools_available, timeout_secs, context }`
- `WorkerCompletion` (line 109): `{ assignment_id, worker_name, success, output, errors, timestamp }`
- `WorkerIdleSignal` (line 120): `{ worker_name, container_id, sandbox_id, timestamp }`
- `ResourceRequest` (line 129): `{ request_id, sandbox_id, resource_type, amount }`
- `ResourceType` enum (line 138): `RAM`, `CPU`
- `ApiContract` (line 145): `{ contract_id, providing_team_id, consuming_team_ids, endpoint_spec, schema, created_at }`
- `InterTeamRpc` (line 156): `{ call_id, method, params, timeout_secs }`
- `InterTeamRpcResponse` (line 165): `{ call_id, success, result, error }`
- `ContainerControlRequest` (line 174): `{ sandbox_id, container_ids, action }`
- `ContainerAction` enum (line 181): `Pause`, `Resume`, `Terminate`
- `DependencyGraph` (line 189): `{ tasks, execution_order }`
- `TaskNode` (line 195): `{ task_id, depends_on, team_type }`
- `ExecutionSession` (line 327): `{ session_id, user_task, dependency_graph, provisioning_spec, sandbox_handles, completed_tasks, failed_tasks, status, created_at }`
- `ExecutionStatus` enum (line 340): `Planning`, `Provisioning`, `Running`, `Completed`, `Failed`, `Cancelled`
- `OrchestratorPlan` (line 351): `{ plan_id, dependency_graph, provisioning_spec, estimated_total_time_secs, estimated_resource_usage }`
- `ResourceEstimate` (line 360): `{ total_ram_bytes, total_cpu_millis, total_containers }`

**New 7-layer types:**
- `TaskId` type alias (line 207): `String`
- `SandboxName` type alias (line 210): `String`
- `Task` (line 213): `{ id, description, deps, hint }`
- `TaskGraph` (line 222): `{ tasks: Vec<Task> }`
- `SandboxConfig` (line 228): `{ name, scope, tools, max_agents, image }`
- `SandboxTopology` (line 241): `{ sandboxes: Vec<SandboxConfig> }`
- `Checkpoint` (line 247): `{ id, tool_call, tool_args, tool_result, timestamp, layer_snapshot_path }`
- `AgentResult` (line 258): `{ task_id, success, git_diff, error, checkpoint_log, timestamp }`
- `SandboxResult` (line 269): `{ sandbox_name, success, merged_diff, verification_status, timestamp }`
- `VerificationStatus` enum (line 279): `NotStarted`, `Running`, `Passed`, `PartiallyVerified`, `Failed`
- `SchedulerMessage` enum (line 289): `TaskReady`, `TaskStarted`, `TaskCompleted`, `TaskFailed`, `AgentIdle`, `Shutdown`
- `OverlayfsLayer` (line 300): `{ level, mount_path, upper_dir, work_dir, lower_dirs }`
- `LayerLevel` enum (line 309): `Base`, `Sandbox`, `Agent`
- `AgentHandle` (line 317): `{ agent_id, sandbox_name, container_id, task_id, layer }`

---

## Crate 3: `runtime`

**File:** `runtime/Cargo.toml` (22 lines)
**Version:** 0.1.0, Edition 2021
**Binary:** `runtime` (from `src/main.rs`)
**Dependencies:** agentd-protocol (path), serde, serde_json, anyhow, thiserror, log, env_logger

### `src/main.rs` (32 lines)
Placeholder CLI. Imports `Runtime` but never instantiates it. Only prints status/help messages.

### `src/lib.rs` (12 lines)
Module declarations: `agentd_client`, `runtime`. Re-exports: `AgentdClient`, `AgentdClientError`, `AgentdClientResult`, `Runtime`, `RuntimeError`, `RuntimeResult`.

### `src/runtime.rs` (432 lines)

Core infrastructure manager for sandbox/container lifecycle.

- `RuntimeError` enum (line 25): `SandboxCreationFailed`, `ContainerCreationFailed`, `ResourceUnavailable`, `SandboxNotFound`, `ContainerNotFound`, `InvalidState`
- `ManagedContainer` (line 38): `{ id, status, created_at, paused_at, agentd_pid, agentd_rootfs }`
- `ManagedSandbox` (line 49): `{ id, spec, hub_agent_pid, containers, created_at, total_ram_used, total_cpu_used, agentd_sandbox_path, agentd_sandbox_pid }`
- `Runtime` (line 63): `{ sandboxes: Arc<Mutex<HashMap<String, ManagedSandbox>>>, client: Arc<AgentdClient> }`

**Methods:**
- `new(agentd_socket_path)` (line 70): creates Runtime instance
- `provision_sandboxes(spec)` (line 80): provisions sandboxes according to spec — iterates specs, calls `client.create_sandbox()`, creates containers, returns `ProvisioningReady`
- `request_additional_containers(sandbox_id, count, spec)` (line 173): dynamic scaling
- `pause_container(sandbox_id, container_id)` (line 226): SIGSTOP via agentd
- `resume_container(sandbox_id, container_id)` (line 264): SIGCONT via agentd
- `register_hub_agent(sandbox_id, pid)` (line 301): mark hub agent as running
- `get_health_status(sandbox_id)` (line 313): build health snapshot
- `list_sandboxes()` (line 336): return all active sandbox IDs
- `destroy_sandbox(sandbox_id)` (line 343): destroy sandbox and all containers

### `src/agentd_client.rs` (308 lines)

Unix socket client for communicating with agentd daemon.

- `AgentdClientError` enum (line 12): 9 variants — `ConnectionFailed`, `SendFailed`, `ReceiveFailed`, `Timeout`, `InvalidResponse`, `SandboxCreationFailed`, `ContainerCreationFailed`, `ToolInvocationFailed`, `SerializationError`
- `AgentdRequest` (line 28): `{ method, params, id }`
- `AgentdResponse` (line 36): `{ result, error, id }`
- `CreateSandboxParams` (line 44): `{ sandbox_id, os_image, ram_bytes, cpu_millis, packages }`
- `CreateSandboxResponse` (line 54): `{ sandbox_id, path, pid }`
- `CreateContainerParams` (line 62): `{ sandbox_id, container_id }`
- `CreateContainerResponse` (line 69): `{ container_id, sandbox_id, pid, rootfs_path }`
- `InvokeToolParams` (line 78): `{ sandbox_id, container_id, tool_name, input }`
- `InvokeToolResponse` (line 87): `{ output, exit_code, stderr }`
- `ContainerControlAction` enum (line 95): `Pause`, `Resume`, `Terminate`
- `AgentdClient` (line 111): `{ socket_path, request_timeout: Duration }`

**Wire protocol:** Newline-delimited JSON (NDJSON). Each request is one JSON object + `\n`, each response is one JSON object + `\n`. New `UnixStream` per request (no persistent connections).

---

## Crate 4: `mowis-cli`

**File:** `mowis-cli/Cargo.toml` (24 lines)
**Version:** 0.1.0, Edition 2021
**NOT a workspace member** — standalone crate
**Dependencies:** anyhow, serde, serde_json, tokio (full), log, env_logger, rand, hex, dirs, async-trait, which, chrono, colored, rustyline, reqwest, futures-util

### `src/main.rs` (~480 lines)

CLI entry point with platform auto-detection and agent commands.

**Module declarations (lines 21-33):**
- Always compiled: `auth`, `connection`, `qemu`, `types`, `agent_client`
- Linux only: `linux`
- macOS only: `macos`
- Windows only: `windows`, `developer_mode`

**Functions:**
- `banner()` (line 43): ASCII-art MOWIS logo
- `help()` (line 58): structured help with options, commands, agent commands, env vars, examples
- `setup_logging()` (line 94): colored log prefixes (ERR/WRN/INF/DBG/TRC), timestamps via chrono
- `create_launcher()` (line 127): platform dispatch — Linux → `LinuxDirectLauncher`, macOS → `MacOSLauncher`, Windows → `WindowsLauncher`
- `boot_and_connect(skip_boot, tcp_override)` (line 156): full boot sequence with progress channel, or direct TCP connection
- `repl(stream)` (line 217): interactive REPL with shortcuts (list/ls, create/new, quit/exit/q, help/?)
- `single_command(stream, cmd)` (line 329): send one JSON command, print response
- `agent_chat(port)` (~line 380): interactive chat with mowis-agent via HTTP — creates session, loops prompts, prints responses
- `agent_ask(port, prompt)` (~line 430): one-shot prompt to mowis-agent, prints response
- `main()` (line 352): manual arg parsing (--help, --skip-boot, --socket, --tcp, --agent-port), dispatches to agent commands (agent/chat/ask/sessions) or agentd commands (repl/single_command)

**Agent subcommands:**
- `agent` — health check mowis-agent
- `chat` — interactive chat session
- `ask <prompt>` — one-shot prompt
- `sessions` — list mowis-agent sessions

### `src/agent_client.rs` (~120 lines)

HTTP client for mowis-agent.

- `HealthResponse` (line 7): `{ healthy, version, cwd }`
- `Session` (line 13): `{ id, title, message_count, created_at }`
- `AgentMessage` (line 20): `{ id, session_id, role, parts, created_at }`
- `AgentClient` (line 28): `{ base_url, http: reqwest::Client }`
- `AgentClient::new(port)` (line 30): constructs client for `http://127.0.0.1:{port}`
- `health()` (line 40): GET /health
- `create_session(title)` (line 48): POST /session
- `send_message(session_id, text)` (line 58): POST /session/{id}/message (blocking)
- `send_message_async(session_id, text)` (line 70): POST /session/{id}/message/async
- `list_messages(session_id)` (line 82): GET /session/{id}/message
- `list_sessions()` (line 92): GET /session
- `abort(session_id)` (line 102): POST /session/{id}/abort

### `src/types.rs` (92 lines)

- `ConnectionInfo` (line 11): `{ kind, socket_path, tcp_addr, pipe_name, auth_token }`
- `ConnectionKind` enum (line 20): `UnixSocket`, `NamedPipe`, `TcpWithToken`
- `BootProgress` (line 29): `{ stage, message, pct, kind, detail }`
- `VmLauncher` trait (line 42): `start()`, `stop()`, `health_check()`, `name()`, `read_logs()`
- `emit()` (line 55): print progress with prefix symbols (▶/✓/✗/⚠/•)

### `src/connection.rs` (136 lines)

- `ConnectionStream` enum (line 15): `Unix(BufReader<UnixStream>)`, `Tcp(BufReader<TcpStream>)`, `Pipe(BufReader<NamedPipeClient>)`
- `send_json(value)` (line 24): serialize + newline + write_all
- `recv_line()` (line 38): read_line + trim
- `recv_json()` (line 55): loop recv_line, skip empty, parse JSON
- `open_connection(info)` (line 70): dispatch by ConnectionKind
- `is_tcp_reachable(addr)` (line 130): 2-second TCP probe

### `src/auth.rs` (69 lines)

- `token_path()` (line 11): `~/.mowisai/token`
- `load_or_create()` (line 18): read or generate 64-char hex token
- `generate()` (line 43): 32 random bytes → hex
- `persist(token)` (line 49): write to file
- `validate(received, expected)` (line 59): constant-time comparison (XOR + OR accumulation)

### `src/linux.rs` (77 lines)

- `LinuxDirectLauncher` (line 9): `{ socket_path }`
- `start()` (line 25): checks socket existence, tests responsiveness via `UnixStream::connect()` with 500ms timeout. Does NOT start agentd — assumes it's already running.
- `name()` → `"LinuxDirect"`

### `src/windows.rs` (287 lines)

3-tier fallback: WSL2 → Developer Mode → QEMU/WHPX.

- `WindowsLauncher` (line 75): `{ qemu_fallback: QemuLauncher }`
- `start()` (line 125): loads auth token, tries WSL2 first, then Developer Mode, then QEMU fallback
- `start_wsl2(token, pw)` (line 180): starts agentd in WSL2, writes auth token, starts socat bridge (TCP:9722 → Unix socket), polls for readiness
- `start_qemu_fallback(token, pw)` (line 258): uses QemuLauncher with WHPX accelerator

**Constants:** `WSL_DISTRO = "MowisAI"`, `AGENT_TCP_ADDR = "127.0.0.1:9722"`, `AGENT_SOCKET = "/tmp/agentd.sock"`, `WSL_BRIDGE_TIMEOUT = 60`

### `src/macos.rs` (81 lines)

- `MacOSLauncher` (line 11): `{ qemu: QemuLauncher }`
- `start()` (line 32): checks if QEMU already running, if not boots with HVF accelerator, saves snapshot for fast restart
- `name()` → `"macOS-QEMU-HVF"`

### `src/qemu.rs` (183 lines)

- `QemuConfig` (line 13): `{ qemu_bin, image_path, agent_tcp, monitor_tcp, accel, ram_mb, vcpus }`
- `QemuConfig::macos_hvf(image_path)` (line 25): HVF accelerator, 1024MB RAM, 2 vCPUs
- `QemuConfig::windows_whpx(image_path)` (line 37): WHPX accelerator, 1024MB RAM, 2 vCPUs
- `QemuLauncher` (line 50): `{ config, snapshot_exists: AtomicBool }`
- `build_args(token, load_snapshot)` (line 63): QEMU args including `-fw_cfg opt/mowis/token,string={token}` for auth injection
- `spawn_process(token, load_snapshot)` (line 89): spawns QEMU with stdout/stderr loggers
- `wait_for_agent()` (line 126): polls TCP for 90 seconds
- `hmp_command(cmd)` (line 153): QEMU Human Monitor Protocol commands
- `save_snapshot()` (line 165): `savevm mowis-snap`

### `src/developer_mode.rs` (1074 lines)

Full automated Alpine Linux VM bootstrap on Windows for non-admin users.

- `DeveloperConfig` (line 86): `{ qemu_path, iso_path, disk_path, ram_mb, agent_port, monitor_port, serial_port, mount_point, disk_device, agentd_path }`
- `SerialConsole` (line 217): `{ reader, writer, boot_log }` — connects to QEMU serial console TCP port
- `DeveloperLauncher` (line 400): `{ config, child: Mutex<Option<Child>>, running: AtomicBool }`
- `bootstrap(child, token, pw)` (line 527): 12-step automated VM bootstrap:
  1. Verify QEMU didn't crash
  2. Wait for serial console
  3. Drain boot output
  4. Login as root
  5. Network activation (DHCP)
  6. Mount persistent storage
  7. Verify agentd binary
  8. Install socat via apk
  9. Start agentd
  10. Start socat TCP bridge
  11. Wait for host-side port
  12. Write auth token into VM

---

## Component 6: `mowis-agent` (Unified Coding Agent — Go)

**Location:** `mowis-agent/`
**Language:** Go 1.24
**Module:** `github.com/mowisai/mowis-agent`
**Origin:** Forked from OpenCode (github.com/opencode-ai/opencode), TUI stripped, HTTP server added
**Binary:** `mowis-agent` (built with `-tags headless` for bundled distribution)

### Architecture

mowis-agent is the unified coding agent that powers all single-agent coding tasks. It runs as an HTTP server on localhost:4096 and is called by the desktop app, CLI, and potentially agentd.

```
mowis-agent (Go binary)
├── Agent Loop (tool-calling loop with LLM)
├── 13+ Tools (bash, edit, write, read, glob, grep, patch, fetch, etc.)
├── LLM Providers (Anthropic, OpenAI, Gemini, Vertex AI, Azure, Bedrock, Groq, xAI, Copilot, OpenRouter)
├── Session/Message Store (SQLite)
├── Permission System (blocking request-grant)
├── LSP Integration (code intelligence)
├── Pub/Sub Event Bus (real-time events)
└── HTTP Server (REST API + SSE)
```

### HTTP API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health check + version |
| `GET` | `/session` | List all sessions |
| `POST` | `/session` | Create session `{ "title": "..." }` |
| `GET` | `/session/:id` | Get session details |
| `DELETE` | `/session/:id` | Delete session |
| `GET` | `/session/:id/message` | List messages |
| `POST` | `/session/:id/message` | Send message (blocking — waits for agent) |
| `POST` | `/session/:id/message/async` | Send message (non-blocking) |
| `POST` | `/session/:id/abort` | Abort running agent |
| `POST` | `/session/:id/permission/:pid` | Approve/deny permission |
| `GET` | `/event` | SSE event stream |
| `GET` | `/config` | Get config |
| `GET` | `/provider` | List providers |
| `GET` | `/agent` | List agents (Build/Plan) |

### Key Go Packages

| Package | Purpose |
|---------|---------|
| `internal/app/` | App bootstrap — creates services, wires dependencies |
| `internal/llm/agent/` | Agent loop — stream LLM → process tool calls → loop |
| `internal/llm/tools/` | 13 tools: bash, edit, write, view, glob, grep, ls, patch, fetch, diagnostics, sourcegraph, file, shell |
| `internal/llm/provider/` | LLM provider abstraction (8 providers) |
| `internal/llm/models/` | Model definitions (Anthropic, OpenAI, Gemini, etc.) |
| `internal/llm/prompt/` | System prompts (coder, summarizer, task, title) |
| `internal/session/` | Session CRUD (SQLite-backed) |
| `internal/message/` | Message types + content parts |
| `internal/permission/` | Blocking permission requests |
| `internal/pubsub/` | Event bus (generic broker with typed subscribers) |
| `internal/db/` | SQLite database (migrations, queries via sqlc) |
| `internal/lsp/` | LSP client integration |
| `internal/diff/` | Diff engine + patch application |
| `internal/history/` | File version history |
| `internal/config/` | Configuration loading |
| `internal/server/` | HTTP API server (new — added for MowisAI) |
| `cmd/` | CLI commands (root, serve, setup, schema) |
| `internal/tui/theme/` | Theme definitions (kept for app.go dependency) |

### Build Tags

- `headless` — builds without TUI dependencies (used for bundled binary)
- `!headless` — builds with TUI (default, for standalone OpenCode usage)

### cmd/ Structure

- `root_tui.go` (`!headless`): Original OpenCode interactive TUI mode
- `root_headless.go` (`headless`): Headless mode — default to serve, no TUI
- `serve.go`: `mowis-agent serve --port 4096 --hostname 127.0.0.1`
- `setup.go`: Shared setup code (config loading, DB connection, app creation)

### CI/CD

Built in GitHub Actions as Job 2 (`build-mowis-agent`):
- Runs on `ubuntu-latest`
- Cross-compiles for Windows: `GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go build -tags headless -o mowis-agent.exe .`
- Uploads as artifact, downloaded by Job 3 and bundled into Tauri NSIS installer

---

## Crate 5: `mowis-desktop` (Tauri App)

**File:** `mowis-desktop/src-tauri/Cargo.toml` (35 lines)
**Version:** 0.1.0, Edition 2021
**Dependencies:** tauri (2.0), tauri-plugin-dialog, serde, serde_json, tokio (full), uuid, anyhow, log, which, sha2, rand, hex, dirs, async-trait, tokio-util, reqwest (json, rustls-tls, stream), futures-util, lazy_static, gcp_auth, base64, rsa

### `src-tauri/src/main.rs` (~80 lines)

Tauri entry point. Registers **37 Tauri commands** (30 existing + 7 new agent commands). Creates `BackendBridge`, wraps `AppState`, starts bridge loop.

### `src-tauri/src/agent_client.rs` (~300 lines)

HTTP client for mowis-agent — shared types and request methods.

**Types:**
- `HealthResponse` (line 12): `{ healthy, version, cwd }`
- `Session` (line 18): `{ id, parent_session_id, title, message_count, prompt_tokens, completion_tokens, cost, created_at, updated_at }`
- `AgentMessage` (line 30): `{ id, session_id, role, parts: Vec<ContentPart>, model, created_at, updated_at }`
- `ContentPart` enum (line 40): `Text { text }`, `Reasoning { text }`, `ToolCall { call_id, name, input }`, `ToolResult { call_id, name, content, is_error }`, `Finish { reason }`
- `PermissionRequest` (line 62): `{ id, session_id, tool_name, description, action, params, path }`
- `SseEvent` (line 72): `{ event_type, payload: Value }`

**AgentClient methods:**
- `new(port)` / `with_base_url(url)` — constructors
- `health()` — GET /health
- `create_session(title)` — POST /session
- `list_sessions()` — GET /session
- `get_session(id)` — GET /session/{id}
- `delete_session(id)` — DELETE /session/{id}
- `list_messages(session_id)` — GET /session/{id}/message
- `send_message(session_id, text)` — POST /session/{id}/message (blocking)
- `send_message_async(session_id, text)` — POST /session/{id}/message/async
- `abort(session_id)` — POST /session/{id}/abort
- `approve_permission(session_id, perm_id)` — POST /session/{id}/permission/{pid} (approve)
- `deny_permission(session_id, perm_id)` — POST /session/{id}/permission/{pid} (deny)
- `subscribe_events()` — GET /event (SSE stream → mpsc::Receiver)

### `src-tauri/src/agent_manager.rs` (~160 lines)

Manages mowis-agent subprocess lifecycle.

- `DEFAULT_AGENT_PORT` (line 9): 4096
- `AgentManager` (line 13): `{ process: Option<Child>, client: AgentClient, port }`
- `new(port)` (line 20): creates manager with client
- `client()` (line 26): returns reference to AgentClient
- `start(resource_dir)` (line 30): finds mowis-agent binary in resources, spawns subprocess, waits for health check
- `stop()` (line 56): graceful shutdown with 5s timeout
- `is_healthy()` (line 72): health check
- `find_agent_binary(resource_dir)` (line 77): looks in Tauri resources, then executable directory
- `wait_for_health()` (line 95): exponential backoff (100ms → 1s), max 30 attempts

### `src-tauri/src/types.rs` (~305 lines)

- `BridgeCommand` enum (line 249): `StartOrchestration`, `StartZeroMode` (deprecated), `ContinueZeroMode` (deprecated), `StopOrchestration`, `CheckSocket`
- Note: `StartZeroMode` and `ContinueZeroMode` now use `serde_json::Value` for workspace (was `zero_mode::ZeroWorkspaceInfo`)

### `src-tauri/src/state.rs` (~356 lines)

- `AppState` (line 16): `{ bridge, config, current_session_id, messages, tasks, sessions, session_history, usage_history, daemon_connected, tokens_total, tool_calls_total, storage_path, cmd_tx, active_sandbox, agent_manager }`
- Note: `zero_workspace` field replaced by `agent_manager: Mutex<Option<AgentManager>>`

### `src-tauri/src/bridge_loop.rs` (~570 lines)

Central event loop connecting backend to frontend.

- `start_bridge(app, state)` (line 10): creates 5 concurrent subsystems:
  1. Platform harness startup
  2. **mowis-agent subprocess startup** (new) — spawns agent, waits for health, stores in state
  3. Connection state watcher
  4. Command handler (dedicated OS thread with own tokio runtime)
  5. Event consumer

- Note: `StartZeroMode` and `ContinueZeroMode` command handlers now log deprecation warnings

### `src-tauri/src/commands.rs` (~760 lines)

37 Tauri commands exposed to frontend (30 existing + 7 new agent commands):

| Command | Line | Purpose |
|---------|------|---------|
| `validate_git_repository` | 160 | Check if path is valid Git repo |
| `clone_github_repo` | 165 | Clone GitHub URL into destination |
| `get_messages` | 211 | Return all ChatMessages |
| `get_tasks` | 216 | Return all Tasks |
| `get_session_history` | 221 | Return Vec<SessionSummary> |
| `get_usage_history` | 228 | Return Vec<UsageRecord> |
| `get_config` | 235 | Return Config |
| `save_config` | 240 | Persist new Config |
| `get_daemon_status` | 246 | Return bool: daemon connected? |
| `check_daemon` | 251 | Send CheckSocket command |
| `start_session` | 261 | Start orchestration (zero mode deprecated) |
| `stop_session` | 404 | Stop active session, destroy sandbox |
| `send_message` | 430 | Send follow-up (deprecated zero mode path) |
| `get_current_session` | 464 | Return Option<SessionDetail> |
| `load_session` | 474 | Load persisted session by ID |
| `clear_current_session` | 499 | Clear current session |
| `get_zero_workspace` | 510 | Deprecated — returns null |
| `get_zero_workspace_base` | 516 | Deprecated — returns empty string |
| `get_sandbox_status` | 522 | Return active sandbox info |
| `discard_sandbox` | 528 | Remove sandbox from disk |
| `get_sandbox_size` | 538 | Return sandbox upper_dir size |
| `window_control` | 544 | Close/minimize/toggle_maximize |
| `get_connection_state` | 567 | Return connection info JSON |
| `get_engine_logs` | 577 | Retrieve diagnostic logs |
| `get_system_info` | 582 | Return {os, arch, version} |
| `get_stats` | 591 | Return comprehensive statistics |
| `get_developer_config` | 635 | Load DeveloperConfig |
| `save_developer_config` | 640 | Save DeveloperConfig |
| `validate_developer_config` | 645 | Validate, return warnings |
| `start_developer_bootstrap` | 650 | Save config, return boot instructions |
| `clear_developer_config` | 678 | Delete developer config file |
| **`agent_health`** | 700 | **Check mowis-agent health** |
| **`agent_create_session`** | 710 | **Create session via mowis-agent** |
| **`agent_list_sessions`** | 720 | **List mowis-agent sessions** |
| **`agent_send_message`** | 730 | **Send message to mowis-agent (blocking or async)** |
| **`agent_abort`** | 750 | **Abort mowis-agent session** |
| **`agent_approve_permission`** | 760 | **Approve tool permission** |
| **`agent_deny_permission`** | 770 | **Deny tool permission** |

### ~~Zero Mode~~ (DELETED)

The `src-tauri/src/zero_mode/` directory has been deleted. All zero-mode functionality is replaced by mowis-agent.

Previous zero-mode files:
- `mod.rs` (950 lines) — tool-calling loop, intent classification, quality gates
- `llm.rs` (712 lines) — 7 LLM providers
- `tools.rs` (745 lines) — 13 filesystem/shell tools
- `workspace.rs` (139 lines) — workspace folder management
- `intent.rs` (107 lines) — intent classifier

All of this is now handled by mowis-agent's Go backend (better tools, more providers, session persistence, permission system, LSP integration).

---

## Frontend (`mowis-desktop/src/`)

- `main.js` — Main JS entry point
- `bridge.js` — Tauri bridge communication (invoke commands)
- `styles.css` — Application styles
- `utils.js` — Utility functions
- `mock.js` — Mock data for development

---

## Test Suite

15 integration test files in `agentd/tests/`. 67+ tests total (hard invariant: must always pass).

| File | Lines | Tests | What It Covers |
|------|-------|-------|----------------|
| `comprehensive_integration_tests.rs` | 1158 | 13 | All 75 tools across 14 categories |
| `sandbox_tests.rs` | 52 | 3 | Sandbox IDs, child limits, cgroup limits |
| `sandbox_operations_tests.rs` | 394 | 17 | Creation, destruction, isolation, concurrency |
| `tool_tests.rs` | 38 | 1 | Tool registry basic |
| `socket_protocol_tests.rs` | 281 | 15 | Socket API operations |
| `shell_tools_tests.rs` | 589 | 19 | Shell tool suite |
| `save_workflow_mock_test.rs` | 236 | 2 | Staging+export workflow |
| `new_orchestration_tests.rs` | 713 | 13 | Full orchestration pipeline |
| `law_tests.rs` | 45 | 2 | Sandbox isolation laws |
| `json_csv_tools_tests.rs` | 551 | 20 | JSON/CSV tool suite |
| `http_tools_tests.rs` | 536 | 25 | HTTP tool suite |
| `filesystem_tools_tests.rs` | 835 | 23 | Filesystem tool suite |
| `engine_tests.rs` | 125 | 4 | Container invocation, security policy, agent loop |
| `channel_tests.rs` | 27 | 1 | Channel send/receive |
| `bucket_tests.rs` | 17 | 1 | Bucket persistence |

### Inline Unit Tests

Orchestration modules contain ~60+ inline tests:
- `sandbox_topology.rs`: 3 tests (creation, tool mapping, export)
- `scheduler.rs`: 2 tests (basic flow, stats)
- `checkpoint.rs`: 2 tests (log persistence, manager paths)
- `merge_worker.rs`: 2 tests (empty diffs, single diff)
- `merge_reviewer.rs`: 7 tests (diff parsing, conflict detection, severity)
- `verification.rs`: 22 tests (JSON extraction, status determination, timeouts)
- `agent_execution.rs`: 1 test (timestamp)
- `planner.rs`: 2 tests (response parsing, Windows dir scan)
- `health.rs`: 4 tests (heartbeat, circuit breaker)
- `complexity_classifier.rs`: 9 tests (mode classification)
- `provider_client.rs`: 6 tests (conversation formats, tool declarations)

### Benchmarks

`agentd/benches/scheduler_bench.rs` (35 lines): 2 placeholder benchmarks using Criterion (mock dispatch, mock execution).

### Example

`agentd/examples/complete_usage.rs` (42 lines): demonstrates sandbox creation with resource limits and AgentLoop execution.

---

## Non-Rust Files

| File | Purpose |
|------|---------|
| `scripts/perf-gate.sh` | Performance gate: 2000 tasks, 420s budget |
| `agentd/scripts/build-rootfs.sh` | Build rootfs script |
| `.github/workflows/build-windows.yml` | CI: 3-job Windows build (agentd, mowis-agent, Tauri installer) |
| `.cargo/config.toml` | Cargo build configuration |
| `codemagic.yaml` | CI configuration |
| `deny.toml` | cargo-deny config |
| `AGENTS.md` | Architecture specification for new 7-layer system |
| `CLAUDE.md` | Claude-specific instructions |
| `system.md` | System instructions |
| `SKILL_FRONTNED (1).md`, `SKILL_FRONTNED (2).md` | Frontend skill docs |
| `mowis-agent/go.mod` | Go module definition for mowis-agent |
| `mowis-agent/go.sum` | Go dependency checksums |
| `mowis-agent/sqlc.yaml` | SQL code generation config |
| `mowis-agent/opencode-schema.json` | Config schema (from OpenCode) |

---

## Key Architectural Patterns

1. **Socket-only orchestration:** Orchestration communicates with agentd through the Unix socket API only. No shared memory, no direct function calls across the daemon boundary.

2. **Dual-lane worker pools:** Socket server uses 64 fast-lane + 128 slow-lane workers to prevent slow operations from blocking fast reads.

3. **3-level overlayfs:** Level 0 (base, read-only, shared), Level 1 (sandbox CoW), Level 2 (agent CoW). Zero duplication of base repo.

4. **Tree-pattern merge:** N diffs merged in log2(N) rounds by pairing diffs in parallel, not serial queue.

5. **3-tier error handling:** Tier 1 (tool failure → retry), Tier 2 (agent crash → respawn from checkpoint), Tier 3 (repeated failure → escalate to human).

6. **Platform abstraction:** mowis-cli and mowis-desktop both use the `VmLauncher` trait with platform-specific implementations (Linux direct socket, macOS QEMU+HVF, Windows WSL2/QEMU+WHPX/Developer Mode).

7. **Unified coding agent (mowis-agent):** Single-agent coding tasks go through mowis-agent (Go binary, HTTP API). Multi-agent orchestration goes through agentd (Rust binary, Unix socket). Desktop and CLI both talk to mowis-agent via HTTP.

8. **Subprocess lifecycle:** mowis-desktop spawns mowis-agent as a subprocess on startup, waits for health check, stores the manager in AppState. The agent runs on localhost:4096 and is bundled inside the Tauri installer.

9. **C FFI:** agentd library exposes C-compatible functions for sandbox/memory/loop management, enabling integration from non-Rust languages.

10. **AES-256-GCM encryption:** API keys encrypted at rest with machine-derived keys (SHA-256 of machine-id + salt).

11. **Constant-time auth:** Token validation uses XOR + OR accumulation to prevent timing attacks.
