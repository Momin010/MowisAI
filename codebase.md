# MowisAI Codebase Reference

## Workspace Structure

```
MowisAI/
├── Cargo.toml                  # Workspace root
├── mowis-protocol/             # Wire protocol (host ↔ guest)
│   ├── Cargo.toml
│   └── src/lib.rs              # Payload enum, Envelope, PROTOCOL_VERSION=2
│
├── mowis-orchestration/        # Core orchestration engine
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs              # Module declarations
│       ├── conductor.rs        # User-facing planner (long-running actor)
│       ├── critic.rs           # Blind plan reviewer
│       ├── captain.rs          # Execution orchestrator (long-running actor)
│       ├── crew.rs             # Per-task LLM loop
│       ├── plan.rs             # Multi-file plan format (DAG)
│       ├── events.rs           # EventBus (broadcast channel)
│       ├── providers.rs        # LLM client (7 providers, SSE streaming)
│       ├── tools.rs            # ToolGateway, LocalTransport, whitelists
│       ├── summaries.rs        # Deterministic per-tool summary templates
│       ├── digest.rs           # SummaryDigestBuffer
│       ├── config.rs           # OrchConfig (~/.mowisai/mowis.toml)
│       ├── crypto.rs           # AES-256-GCM encryption
│       ├── host_tools.rs       # Local tool execution
│       ├── merger.rs           # Overlay merge coordinator
│       └── prompts/            # System prompts (.md)
│
├── mowis-host/                 # Host-side CLI + VM lifecycle
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs              # Re-exports
│       ├── main.rs             # CLI (setup, chat, boot, ping, exec, -p)
│       ├── transport/mod.rs    # vsock client
│       ├── vmm/                # QEMU backend
│       ├── initrd.rs           # initramfs builder
│       └── tui/                # Terminal UI
│
├── mowis-executor/             # Guest-side executor (runs in VM)
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs             # Entry point (PID 1 or normal)
│       ├── server.rs           # vsock server, dispatch
│       ├── sandbox.rs          # overlayfs, chroot, namespaces
│       ├── tools.rs            # Tool registry
│       └── init.rs             # PID 1 mode
│
├── agentd/                     # Legacy agent daemon
├── agentd-protocol/            # Legacy shared types
├── runtime/                    # Legacy control plane
├── mowis-desktop/              # Desktop GUI (Tauri)
└── mowis-cli/                  # Standalone CLI (not in workspace)
```

## Key Types

### mowis-protocol
- `Payload` — enum of all wire messages (30+ variants)
- `Envelope` — `{ id: u64, payload: Payload }`
- `PROTOCOL_VERSION` — 2

### mowis-orchestration
- `ConductorCommand` — `UserMessage`, `CriticVerdict`, `EndConversation`
- `ConductorReply` — `Chat`, `PlanDrafted`, `PlanRevised`, `Error`
- `Event` — 25+ variants (PlanDrafted, CrewToolSummary, etc.)
- `EventBus` — broadcast channel wrapper
- `Plan` — plan.toml + tasks.toml + overview.md + ...
- `TaskNode` — `{ id, title, description, deps, model_tier, tool_budget }`
- `Tier` — `Conductor | Critic | Captain | Crew`
- `ToolCall` — `{ id, name, args }`
- `ToolOutcome` — `Ok(Value) | Err(String) | Denied`
- `OrchConfig` — providers, tiers, sandbox config
- `LlmConfig` — provider, model, api_key

### mowis-host
- `Connection` — vsock client with `call()` and `call_streaming()`
- `Vmm` trait — `boot()`, `shutdown()`
- `TuiApp` — terminal UI state machine

### mowis-executor
- `Sandbox` — overlayfs sandbox with `create()`, `create_agent_overlay()`
- `merge_overlay()` — copy upper dir to parent

## Data Flow

```
User message
  → Conductor.handle_user_message()
    → LLM call (streaming)
    → Returns ConductorReply::PlanDrafted or ::Chat
  → Critic reviews plan (LLM call)
  → User approves
  → Captain starts
    → Spawns Crews in parallel (event-driven scheduling)
    → Each Crew calls LLM in tool-calling loop
    → Tool calls → ToolGateway → LocalTransport → host_tools
    → CrewToolSummary emitted per tool call
    → CrewDone/CrewFailed on completion
  → Captain merges overlays
  → PlanCompleted event
```

## Configuration

`~/.mowisai/mowis.toml`:
```toml
[providers.anthropic]
api_key_enc = "encrypted..."

[tier.conductor]
provider = "anthropic"
model = "claude-opus-4-7"

[tier.critic]
provider = "anthropic"
model = "claude-opus-4-7"

[tier.captain]
provider = "anthropic"
model = "claude-sonnet-4-6"

[tier.crew]
provider = "anthropic"
model = "claude-haiku-4-5-20251001"
```

## Build Commands

```bash
cargo build --release                          # All crates
cargo build --release -p mowis-host            # Just mowisd binary
cargo test --workspace                         # All tests
cargo test -p mowis-protocol -p mowis-orchestration  # Core tests
npx playwright test                            # UI tests
```
