# MowisAI

> **NOTE:** the project is currently being rewritten from the ground up.  The previous
> `mowisai-engine` and `mowisai-bridge` folders embodied the first prototype.  A
> fresh Rust/C engine is now underway under `agentd/` with a formal specification in
> `docs/spec.md`.  The content below describes the legacy system and may be outdated.

**AI-native multi-agent sandbox platform. Lightweight, embeddable, no Docker daemon required.**

MowisAI lets you spin up isolated sandboxes containing multiple AI agents that can write code, execute it, share files, and communicate with each other — all running in real Linux containers managed by a single Rust binary.

---

## What It Is

Most AI agent platforms run agents as bare processes with no real isolation. MowisAI gives every agent its own container — isolated filesystem, memory limits, CPU limits — while still allowing agents inside the same sandbox to talk to each other and share a network.

```
Sandbox: "Frontend Team"
├── Orchestration Hub
├── Container: Planner Agent   ─┐
├── Container: Coder Agent      ├── shared network, isolated filesystem
├── Container: UI Agent         │   agents message each other
└── Container: Reviewer Agent  ─┘
```

---

## Architecture

```
Electron UI
    │
    ▼
MCP Bridge (Node.js)          ← standard Model Context Protocol
    │
    ▼
MowisAI Engine (Rust)         ← single binary, no daemon
    │
    ▼
Alpine Linux Containers       ← real Linux namespace isolation
    ├── PID namespace
    ├── Mount namespace
    ├── IPC namespace
    └── UTS namespace
```

---

## Features

- **Real isolation** — Linux namespaces (PID, mount, IPC, UTS), not just process sandboxing
- **Resource limits** — RAM and CPU limits per container via cgroups v2
- **Persistent sessions** — containers stay alive between commands, agents remember state
- **Multi-agent sandboxes** — group agents into sandboxes with shared networking
- **Agent messaging** — agents send and receive messages from each other via a built-in message bus
- **MCP compatible** — standard Model Context Protocol bridge, works with any MCP-compatible LLM
- **No Docker** — single Rust binary, no daemon, no Docker socket
- **Pre-installed runtimes** — Node.js 20 and Python 3.11 baked into the base image
- **Internet access** — containers can install packages and make network requests

---

## 📊 Professional Document Generation

Agents can now generate professional-grade documents with **automatic visualizations**:

### Built-in Capabilities

- **📈 Chart Generation** — Bar, pie, line, doughnut, and comparison charts (auto-embedded in Excel/PowerPoint/PDF)
- **🖼️ Image Handling** — Download, cache, optimize, and embed images in documents
- **🌐 Web Integration** — Search the web, fetch real-time crypto/stock data, GitHub trends, and news
- **📄 Multi-Format Export** — Excel (.xlsx), PowerPoint (.pptx), Word (.docx), PDF (.pdf), CSV, JSON

### Example: Financial Report with Charts

```javascript
// Agent: Financial Analyst
"Create a quarterly revenue report with charts"

// Orchestrator automatically:
// 1. Detects chartable data [{ label: "Q1", value: 450000 }, ...]
// 2. Generates professional bar and pie charts
// 3. Embeds charts in Excel/PowerPoint
// 4. Output: Professional report with visualizations
```

### Supported Data Sources

| Source | Capabilities |
|---|---|
| **Web Search** | DuckDuckGo search, no API key needed |
| **Stock Market** | Real-time prices, market data |
| **Cryptocurrency** | Bitcoin, Ethereum, and 1000+ coins |
| **Weather** | Real-time weather and forecasts |
| **GitHub** | Trending repositories, trending languages |
| **News** | Tech news and article aggregation |

### Available Agent Types

✅ **Financial Analyst** — Generate charts in Excel/PowerPoint, access market data
✅ **Data Scientist** — Professional chart generation with automatic embedding
✅ **Researcher** — Web search, fetch data, generate reports with visualizations
✅ **Designer** — Build PowerPoint presentations with professional layouts
✅ **Writer** — Create documents with embedded charts and images
✅ **Coder** — Full development capabilities with documentation generation

📚 [Full Guide →](AGENT_PROFESSIONAL_FEATURES.md)

---

## Stack

| Component | Technology |
|---|---|
| Sandbox Engine | Rust |
| Container Isolation | Linux namespaces + cgroups v2 |
| Base Image | Alpine Linux 3.19 |
| MCP Bridge | Node.js + `@modelcontextprotocol/sdk` |
| LLM Integration | Groq (Llama 3.3 70B) / OpenRouter |
| Frontend (planned) | Electron |

---

## Project Structure

```
mowisai-engine/
├── mowisai-engine/          # Rust sandbox engine
│   ├── src/
│   │   ├── main.rs          # Socket server + sandbox/session manager
│   │   ├── container.rs     # Namespace isolation + persistent sessions
│   │   ├── executor.rs      # Task execution
│   │   └── protocol.rs      # JSON request/response types
│   ├── setup_rootfs.sh      # Alpine Linux base image setup
│   └── Cargo.toml
│
└── mowisai-bridge/          # MCP bridge + AI agent
    ├── index.js             # MCP server (shell, file tools)
    └── agent.js             # LLM agent loop (Groq)
```

---

## Getting Started

> **Requires:** Linux (Ubuntu 20.04+), Rust, Node.js, sudo

### 1. Build the engine

```bash
cd mowisai-engine
cargo build --release
```

### 2. Set up the base container image

```bash
bash setup_rootfs.sh
sudo chroot rootfs /bin/sh -c "apk add --no-cache nodejs npm python3"
```

### 3. Start the engine

```bash
sudo ./target/release/mowisai-engine
```

> **Note:** the Unix socket is created world-writable (`0666`) so your
> non-root bridge process or CLI tools can connect without needing sudo.

### 4. Run an AI agent

```bash
cd ../mowisai-bridge
npm install
export GROQ_API_KEY="your-key"
sudo -E node agent.js "create a Python script that calculates fibonacci numbers and run it"
```

---

## API Protocol

The engine communicates over a Unix socket (`/tmp/mowisai.sock`) using newline-delimited JSON.

### Request Types

| `request_type` | Description |
|---|---|
| `exec` | Run a command in a one-shot container |
| `create_sandbox` | Create a named sandbox for a team of agents |
| `join_sandbox` | Add an agent to a sandbox (spawns persistent container) |
| `run_in_session` | Run a command in an existing agent session |
| `message_send` | Send a message from one agent to another |
| `message_read` | Read an agent's message inbox |
| `kill_session` | Stop an agent's container |

### Example: Create a sandbox and add agents

```json
{"task_id":"1","request_type":"create_sandbox","sandbox_name":"my-team"}
{"task_id":"2","request_type":"join_sandbox","sandbox_name":"my-team","agent_name":"planner"}
{"task_id":"3","request_type":"join_sandbox","sandbox_name":"my-team","agent_name":"coder"}
```

### Example: Agent messaging

```json
{"task_id":"4","request_type":"message_send","sandbox_name":"my-team","from_agent":"planner","to_agent":"coder","content":"Build a login form"}
{"task_id":"5","request_type":"message_read","sandbox_name":"my-team","agent_name":"coder"}
```

---

## MCP Tools

The bridge exposes these tools to any connected LLM:

| Tool | Description |
|---|---|
| `shell_exec` | Execute a shell command in the sandbox |
| `file_read` | Read a file from the sandbox |
| `file_write` | Write a file to the sandbox |
| `file_list` | List files in the sandbox |

---

## Roadmap

- [x] Linux namespace isolation
- [x] cgroups v2 resource limits
- [x] Persistent agent sessions
- [x] Multi-agent sandboxes
- [x] Agent-to-agent messaging
- [x] MCP bridge
- [x] LLM integration (Groq)
- [ ] Orchestration hub (task splitting + agent assignment)
- [ ] Streaming output
- [ ] Electron UI
- [ ] Cross-platform packaging (Windows via Hyper-V, Mac via vfkit)
- [ ] GPU passthrough

---

## About

Built by [Momin](https://github.com/Momin010) — started February 2026.

MowisAI is a lightweight alternative to heavy container runtimes for AI agent workloads. Every agent gets real OS-level isolation without the overhead of a full container daemon.# MowisAI
