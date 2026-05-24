# MowisAI

An AI agent orchestration engine that runs thousands of isolated agents in parallel on your machine.

## What It Does

MowisAI takes a user prompt, drafts a plan with multiple tasks, and executes them in parallel using isolated AI agents. Each agent works in its own sandbox with overlay filesystem isolation.

```
$ mowisd -p "build a cryptocurrency dashboard"

→ Conductor drafts plan (5 tasks, all parallel)
→ Critic reviews and approves
→ Captain spawns 5 agents simultaneously
→ Each agent writes code, runs commands, tests
→ Results merge back to your filesystem
→ Done in ~90 seconds
```

## Architecture

```
User → Conductor (LLM) → Plan → Critic (LLM) → Captain → Crews (parallel)
                                                              ↓
                                                     Sandbox (overlayfs)
                                                     Tools (filesystem, shell, git, http)
```

**Four tiers:**
- **Conductor** — User-facing planner. Drafts plans, handles mid-run messages.
- **Critic** — Blind reviewer. Reviews plans without seeing conversation history.
- **Captain** — Execution orchestrator. Spawns crews, manages sandboxes, merges results.
- **Crew** — Per-task fast agent. Executes tools, writes code, runs commands.

## Quick Start

```bash
# Build
cargo build --release -p mowis-host

# Setup (configure AI provider)
mowisd setup

# Run autonomously
mowisd -p "build a REST API with Express.js"

# Run with verbose logging
mowisd -p "build a weather app" --log

# Run interactively (TUI)
mowisd
```

## Supported Providers

- Anthropic (Claude)
- OpenAI (GPT)
- Google Gemini
- Vertex AI (GCP)
- Grok (xAI)
- Groq
- Mimo (Xiaomi)

## Project Structure

```
MowisAI/
├── mowis-orchestration/    # Core engine: Conductor, Critic, Captain, Crew
├── mowis-protocol/         # Wire protocol (host ↔ guest VM)
├── mowis-host/             # Host-side CLI + VM lifecycle
├── mowis-executor/         # Guest-side: sandbox primitives, tool registry
├── agentd/                 # Legacy agent daemon (being migrated)
├── agentd-protocol/        # Legacy shared types
├── runtime/                # Legacy control plane
└── mowis-desktop/          # Desktop GUI (Tauri)
```

## How It Works

1. **Plan** — Conductor creates a task graph (DAG) from your prompt
2. **Review** — Critic reviews the plan for correctness, safety, and efficiency
3. **Execute** — Captain spawns crews in parallel based on task dependencies
4. **Isolate** — Each crew runs in its own overlayfs sandbox
5. **Merge** — Results are merged back to the base filesystem
6. **Output** — Final code lands in your project directory

## Testing

```bash
# Rust tests
cargo test --workspace

# Playwright UI tests
npx playwright test
```

## License

MIT
