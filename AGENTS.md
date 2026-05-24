# MowisAI — Developer Reference

## Architecture

MowisAI is a four-tier AI agent orchestration engine:

| Tier | Role | Model Tier | Lifetime |
|---|---|---|---|
| **Conductor** | User-facing planner | Flagship | Whole conversation |
| **Critic** | Blind plan reviewer | Flagship | One-shot per plan version |
| **Captain** | Execution orchestrator | Mid | Whole conversation |
| **Crew** | Per-task agent | Fast | Until task completes |

## mowis-orchestration Crate

The core engine. Contains:

- `conductor.rs` — Long-running actor. Accepts `ConductorCommand` via mpsc channel. Drafts plans, handles mid-run messages, classifies user intent.
- `critic.rs` — Subscribes to `PlanDrafted` events. Reviews plans with LLM, emits `CriticVerdict`.
- `captain.rs` — Event-driven scheduler. Spawns crews as soon as dependencies are met. Uses mpsc channel for completions.
- `crew.rs` — Per-task LLM loop. Calls tools via `ToolGateway`, emits `CrewToolSummary` per tool call.
- `plan.rs` — Multi-file plan format. DAG validation, history snapshots, atomic writes.
- `events.rs` — `EventBus` with `tokio::sync::broadcast`. All tiers subscribe.
- `providers.rs` — LLM client for 7 providers. Streaming support via SSE.
- `tools.rs` — `ToolGateway`, `LocalTransport`, tier whitelists.
- `summaries.rs` — Deterministic per-tool summary templates.
- `digest.rs` — `SummaryDigestBuffer` for Conductor's summary digest.
- `config.rs` — `OrchConfig` loaded from `~/.mowisai/mowis.toml`.
- `crypto.rs` — AES-256-GCM encryption for API keys.
- `host_tools.rs` — Local tool execution (filesystem, shell, git, http, search).

## mowis-protocol Crate

Wire protocol between host and guest. Payloads include:

- `CreateSandbox`, `DestroySandbox`, `CreateAgentOverlay`, `MergeAgentOverlay`
- `InvokeToolAsAgent` — tool call scoped to agent's overlay
- `UploadCodebase` — transfer project files to VM
- `HealthCheck`, `SendInput`, `InteractiveStatus`

## mowis-host Crate

Host-side CLI (`mowisd`) and VM lifecycle:

- `main.rs` — CLI with `setup`, `chat`, `boot`, `ping`, `exec`, `-p` flag
- `transport/mod.rs` — vsock client
- `vmm/` — QEMU backend
- `initrd.rs` — initramfs builder
- `tui/` — Terminal UI (splash, setup wizard, chat)

## mowis-executor Crate

Guest-side executor (runs inside VM):

- `server.rs` — vsock server, dispatches tool calls
- `sandbox.rs` — overlayfs, chroot, namespaces, cgroups
- `tools.rs` — Tool registry (filesystem, shell, git, http, search)
- `init.rs` — PID 1 mode (mount filesystems, load vsock modules)

## Event Bus

All tiers communicate via `EventBus` (broadcast channel, capacity 1024):

| Event | Source | Consumers |
|---|---|---|
| `PlanDrafted` | Conductor | Critic, UI |
| `CriticVerdict` | Critic | Conductor |
| `UserApproved` | User | Captain |
| `CrewToolSummary` | Crew | UI, Digest |
| `CrewDone` / `CrewFailed` | Crew | Captain |
| `PlanCompleted` / `PlanFailed` | Captain | UI |
| `ConversationEnded` | Conductor | Captain |

## Plan Format

Stored at `.mowis/plans/<plan_id>/`:

- `plan.toml` — metadata
- `overview.md` — Conductor's prose
- `tasks.toml` — task graph (DAG)
- `sandbox.toml` — VM config
- `models.toml` — model assignments
- `tools.toml` — tool whitelist deltas
- `status.toml` — Captain's execution log
- `critic/vN.md` + `critic/vN.toml` — critic reviews
- `history/vN/` — plan version snapshots

## Build

```bash
cargo build --release
cargo test --workspace
```

## Hard Rules

- No `unwrap()` in production code
- No direct crew-to-crew communication
- Plans are immutable once approved
- Every tool call emits exactly one `CrewToolSummary`
- Conductor never reads raw summary stream — only digest
