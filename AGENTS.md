# MowisAI — Developer Reference

## What Is MowisAI?

MowisAI is an **OS-level AI agent execution engine** targeting European regulated enterprise
(GDPR, DORA compliance). The core differentiator: competitors like E2B and Daytona are
cloud-only SaaS; CrewAI and LangGraph have no execution layer. MowisAI runs thousands of
isolated AI agents in parallel on-premises using overlayfs/chroot/cgroups sandboxing — no
cloud dependency, no data leaving the building.

The system consists of two products sharing this repository:

| Crate | Binary | Role |
|---|---|---|
| `agentd` | `agentd` | Core daemon + CLI. Orchestration engine, socket server, TUI. |
| `runtime` | `runtime` | Control plane. State management, delegates to agentd via socket. |
| `agentd-protocol` | (library) | Shared types — no circular deps. |
| `mowis-desktop` | (Tauri app) | Desktop GUI wrapping agentd. Separate product. |

---

## agentd — Core Daemon

`agentd` is a single Rust binary. It does everything:

- Hosts a Unix socket server (`agentd socket`) that agents communicate with
- Runs the 7-layer orchestration pipeline (`agentd orchestrate`)
- Provides a terminal UI for interactive use (`agentd` with no subcommand)
- Exposes 160+ tools to agents across 14 categories + 10 integration platforms
- Loads and injects `.skill` files into every agent's LLM system prompt

### CLI Commands

```
agentd                          # Launch interactive TUI
agentd orchestrate              # Run full orchestration pipeline
agentd simulate                 # Simulation mode (no LLM calls, zero cost)
agentd socket                   # Start Unix socket server
agentd skills list              # List installed skills
agentd skills show <name>       # Print skill content
agentd skills create            # Launch LLM-powered skill creator (terminal)
agentd skills load <path>       # Install a .skill file
agentd skills remove <name>     # Uninstall a skill
agentd skills dir               # Print skills directory path
agentd --version                # Print version + build number
```

### Running Locally

Requires two terminals. Socket server requires root for overlayfs.

```bash
# Terminal 1 — start socket server
sudo ./target/release/agentd socket --path /tmp/agentd.sock

# Terminal 2 — run orchestration
./target/release/agentd orchestrate \
    --prompt "Implement JWT authentication for the REST API" \
    --project company-internal-tools-490516 \
    --socket /tmp/agentd.sock \
    --max-agents 1000

# Simulation mode (no LLM calls, no cost)
./target/release/agentd simulate \
    --socket /tmp/agentd.sock \
    --project-root /path/to/project \
    --max-agents 100 \
    --tasks 200 \
    --verify
```

---

## 7-Layer Orchestration System

Each layer has a single responsibility. Files are in `agentd/src/orchestration/`.

### Layer 1 — Fast Planner (`planner.rs`)
Shell scan (`find`/`tree`) takes ~10ms. A single Gemini call produces the full task graph
and sandbox topology JSON. No file reading at planning time — workers read files themselves.

### Layer 2 — Overlayfs Topology (`sandbox_topology.rs`)
Three-level copy-on-write filesystem:
- **Level 0:** Base layer — full repo, read-only, shared by ALL sandboxes (zero duplication)
- **Level 1:** Sandbox layer — CoW per sandbox, scoped filesystem view
- **Level 2:** Agent layer — CoW per agent, fully isolated writes

On task completion the agent layer produces a clean git diff and is discarded.
Checkpoints are snapshots of the agent CoW layer after each tool call.

### Layer 3 — Scheduler (`scheduler.rs`)
Event-driven task dispatcher. Maintains dependency counters (`DashMap<TaskId, AtomicUsize>`).
When a task completes, counters on all dependents decrement. Any task hitting zero is
immediately dispatched to an idle agent in the correct sandbox. No batching.

### Layer 4 — Agent Execution with Checkpoints (`agent_execution.rs`, `checkpoint.rs`)
Each agent is a Gemini tool-calling loop running inside an isolated container via the agentd
socket. Checkpoint saved after **every** tool call. Three-tier error handling:
- **Tier 1** (tool failure): Rollback to checkpoint, retry (max 3)
- **Tier 2** (agent crash): Spawn fresh agent, restore from checkpoint, continue
- **Tier 3** (repeated failure): Escalate to human with full log

### Layer 5 — Parallel Merge (`merge_worker.rs`)
Tree-pattern merge within each sandbox: N diffs → N/2 merge workers in parallel, repeat
until one result remains. Total rounds: log₂(N). LLM-assisted conflict repair on failure.

### Layer 6 — Verification Loop (`verification.rs`)
After sandbox work completes, a verification planner generates a test task graph (planned
once, not regenerated each round). Test agents run against the merged result. Failures inject
fix tasks back into the scheduler. Maximum 3 rounds, then sandbox is marked `PARTIALLY_VERIFIED`.

### Layer 7 — Cross-Sandbox Merge (`new_orchestrator.rs`)
All verified sandbox diffs are merged by one worker. Integration conflicts (frontend calling
a changed backend endpoint) are caught and LLM-repaired here. Final output: clean, tested,
merged codebase.

---

## Skills System

Skills are domain-specific knowledge files (`.skill`) stored in `~/.mowisai/skills/`.
On every orchestration run, all installed skills are loaded and injected into every agent's
LLM system instruction — giving agents persistent institutional knowledge without re-prompting.

### Skill File Format

Skills are TOML files with a `[meta]` section and `[content]` section:

```toml
[meta]
name = "our-api-conventions"
display_name = "Our API Conventions"
version = "1.0.0"
description = "REST API patterns and naming rules used across all services"
created = "2026-05-18"
author = "engineering-team"
tags = ["api", "conventions", "rest"]
always_load = true

[content]
knowledge = """
All REST endpoints follow snake_case naming.
Auth uses Bearer JWT tokens — never API keys in query params.
Error responses always include { "error": string, "code": string }.
Pagination uses cursor-based pagination with `next_cursor` field.
"""
```

### Creating Skills — LLM-Powered Creator

Skills are **not** written by hand. The LLM writes them for you via natural conversation.

**In the TUI** (interactive terminal):
```
/skill create
```
The LLM opens a conversation, asks questions about what you want to encode, then generates
and saves the `.skill` file automatically. When it's ready, it emits a `<skill>...</skill>`
TOML block in its response — the system detects this, parses it, saves it to disk, and
confirms with a status message. You never write TOML manually.

**From the terminal** (non-TUI):
```bash
agentd skills create
```
Launches the same LLM conversation in your terminal via stdin/stdout.

### TUI Commands

| Command | Action |
|---|---|
| `/skill create` or `/skill new` | Start LLM skill creator |
| `/skill cancel` | Exit skill creator mode |
| `/skill list` | List installed skills |
| `/skill remove <name>` | Remove a skill |

### Skills Directory
```bash
agentd skills dir       # e.g. /home/user/.mowisai/skills/
```

---

## Integration Plugins (80 Tools)

80 native tools across 10 external platforms, available to every agent via the socket API.
All credentials are read from environment variables — never hardcoded.

### GitHub (16 tools) — `GITHUB_TOKEN`
`github_list_repos`, `github_get_repo`, `github_list_issues`, `github_get_issue`,
`github_create_issue`, `github_update_issue`, `github_add_issue_comment`,
`github_list_pull_requests`, `github_get_pull_request`, `github_create_pull_request`,
`github_merge_pull_request`, `github_search_code`, `github_search_issues`,
`github_get_file_contents`, `github_list_workflow_runs`, `github_get_commit`

### Linear (8 tools) — `LINEAR_API_KEY`
`linear_list_teams`, `linear_list_issues`, `linear_get_issue`, `linear_create_issue`,
`linear_update_issue`, `linear_add_comment`, `linear_list_projects`, `linear_list_workflow_states`

### Slack (8 tools) — `SLACK_BOT_TOKEN`
`slack_list_channels`, `slack_post_message`, `slack_get_channel_history`,
`slack_get_thread_replies`, `slack_add_reaction`, `slack_list_users`,
`slack_set_channel_topic`, `slack_upload_file`

### Jira (8 tools) — `JIRA_BASE_URL` + `JIRA_API_TOKEN` + `JIRA_EMAIL`
`jira_list_projects`, `jira_search_issues`, `jira_get_issue`, `jira_create_issue`,
`jira_update_issue`, `jira_add_comment`, `jira_get_transitions`, `jira_transition_issue`

### Notion (6 tools) — `NOTION_TOKEN`
`notion_search`, `notion_get_page`, `notion_create_page`, `notion_update_page`,
`notion_list_databases`, `notion_query_database`

### Sentry (7 tools) — `SENTRY_AUTH_TOKEN` + `SENTRY_ORG` (+ optional `SENTRY_BASE_URL`)
`sentry_list_projects`, `sentry_list_issues`, `sentry_get_issue`, `sentry_update_issue`,
`sentry_list_events`, `sentry_get_event`, `sentry_list_releases`

### Stripe (8 tools) — `STRIPE_SECRET_KEY`
`stripe_list_customers`, `stripe_get_customer`, `stripe_create_customer`,
`stripe_list_charges`, `stripe_get_charge`, `stripe_list_subscriptions`,
`stripe_list_invoices`, `stripe_list_products`

### Vercel (9 tools) — `VERCEL_TOKEN` (+ optional `team_id` parameter)
`vercel_list_projects`, `vercel_get_project`, `vercel_list_deployments`,
`vercel_get_deployment`, `vercel_get_deployment_events`, `vercel_list_domains`,
`vercel_get_domain`, `vercel_cancel_deployment`, `vercel_list_env_vars`

### PagerDuty (7 tools) — `PAGERDUTY_TOKEN`
`pagerduty_list_services`, `pagerduty_list_incidents`, `pagerduty_get_incident`,
`pagerduty_create_incident`, `pagerduty_update_incident`, `pagerduty_list_escalation_policies`,
`pagerduty_list_oncalls`

### Datadog (7 tools) — `DATADOG_API_KEY` + `DATADOG_APP_KEY` (+ optional `DATADOG_SITE` for EU)
`datadog_query_metrics`, `datadog_list_monitors`, `datadog_get_monitor`,
`datadog_create_monitor`, `datadog_mute_monitor`, `datadog_list_dashboards`,
`datadog_search_logs`

---

## mowis-desktop — Desktop Application

`mowis-desktop/` is a **Tauri-based desktop application** — a separate product that wraps
`agentd` with a native GUI. It lives as its own workspace crate and has its own build process.

- **Rust backend:** `mowis-desktop/src-tauri/`
- **Frontend:** Web-based UI served by Tauri
- **Relationship to agentd:** mowis-desktop spawns and communicates with agentd; it does
  not share a process. Think of agentd as the engine and mowis-desktop as the dashboard.

The desktop app is a **separate product** from the agentd CLI/daemon. Development of the two
is independent. Do not add mowis-desktop dependencies into agentd, and vice versa.

---

## Build and Test

```bash
# Build everything
cargo build --release

# Run all tests (must stay at 67+, never regress)
cargo test

# Run a specific test
cargo test --package agentd test_socket_pool_bounded

# Performance gate: 2000 tasks, 1000 agents, <420s
bash scripts/perf-gate.sh

# Override budget
BUDGET_MS=300000 bash scripts/perf-gate.sh
```

---

## AI Backend

- **Model:** Vertex AI Gemini 2.5 Pro
- **GCP Project:** `company-internal-tools-490516`
- **Auth:** `gcloud auth application-default login`

---

## Hard Invariants

These are never negotiable:

- **No direct agent-to-agent communication** — orchestrator-mediated coordination only
- **IDs are always `String` in JSON** — never `u64`
- **Never delete or modify tests** — fix the implementation, not the test
- **Never stub tool implementations** — all tools execute in container via chroot
- **67+ tests must always pass** — never regress the test suite
- **No `unwrap()` in production code paths** — use `?` or proper error handling
- **agentd socket API is immutable** — orchestration talks to agentd via socket only
- **Always read stdout/stderr concurrently** — sequential pipe reading causes deadlock
- **Bump `BUILD_NUMBER` before every push to main** — see `agentd/src/version.rs`

---

## Version Tracking

`agentd/src/version.rs` is the single source of truth for which binary is running:

```rust
pub const BUILD_NUMBER: &str = "YYYYMMDD.N";
```

Format: date + Nth push of the day (e.g. `20260518.3`). Exposed via:
- CLI: `agentd --version`
- Socket API: `get_config` response includes `version` and `build_number` fields
- Printed on socket server startup

**Bump this before every push to main. Failure = impossible to verify deployments.**

---

## Project Layout

```
MowisAI/
├── agentd/
│   └── src/
│       ├── main.rs               # CLI entry point
│       ├── lib.rs                # Crate root, module declarations
│       ├── version.rs            # BUILD_NUMBER — bump before every push
│       ├── socket_server.rs      # Unix socket server + worker thread pools
│       ├── sandbox.rs            # Sandbox primitives, cgroup/chroot
│       ├── vertex_agent.rs       # Gemini/Vertex AI client, tool-calling loop
│       ├── persistence.rs        # WAL, checkpointing, recovery journal
│       ├── orchestration/        # 7-layer orchestration system
│       ├── tools/                # 160+ tools across 14 categories + 10 platforms
│       │   ├── mod.rs            # Registry + factory functions
│       │   ├── discover_tools.rs # Tool catalog for agent discovery
│       │   ├── github_api.rs     # GitHub (16 tools)
│       │   ├── linear_api.rs     # Linear (8 tools)
│       │   ├── slack_api.rs      # Slack (8 tools)
│       │   ├── jira_api.rs       # Jira (8 tools)
│       │   ├── notion_api.rs     # Notion (6 tools)
│       │   ├── sentry_api.rs     # Sentry (7 tools)
│       │   ├── stripe_api.rs     # Stripe (8 tools)
│       │   ├── vercel_api.rs     # Vercel (9 tools)
│       │   ├── pagerduty_api.rs  # PagerDuty (7 tools)
│       │   └── datadog_api.rs    # Datadog (7 tools)
│       ├── skills/               # Skills system
│       │   ├── mod.rs            # SkillManager, Skill types, build_skills_context()
│       │   └── creator.rs        # LLM-powered skill creator
│       └── tui/                  # Interactive terminal UI
│           ├── mod.rs            # TUI entry point, event loop
│           ├── app.rs            # App state, key handlers, skill creator TUI flow
│           ├── ui.rs             # Rendering
│           ├── event.rs          # TuiEvent enum, event thread
│           └── widgets/          # Custom ratatui widgets
├── agentd-protocol/
│   └── src/lib.rs                # Shared types: TaskGraph, SandboxTopology, Checkpoint
├── runtime/
│   └── src/
│       ├── main.rs               # Control plane entry point
│       └── agentd_client.rs      # Typed client for agentd socket API
├── mowis-desktop/                # Tauri desktop application (separate product)
│   └── src-tauri/                # Rust backend for desktop app
└── scripts/
    └── perf-gate.sh              # Performance gate (2000 tasks, 1000 agents, 420s)
```

---

## Cross-Crate Dependencies

```
agentd        → agentd-protocol
agentd        → runtime
runtime       → agentd-protocol
mowis-desktop → (agentd via process/socket, not Cargo dependency)
```

`agentd-protocol` is the shared types crate. It has no dependencies on the other crates —
this prevents circular dependencies.
